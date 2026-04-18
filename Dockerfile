FROM debian:trixie as build-mingw

WORKDIR /build

RUN apt update && apt -y install clang llvm git make ninja-build cmake wget

RUN git clone https://git.code.sf.net/p/mingw-w64/mingw-w64
RUN cd mingw-w64 && git checkout v14.0.0
RUN wget https://github.com/llvm/llvm-project/archive/refs/tags/llvmorg-22.1.3.tar.gz
RUN tar xzf llvmorg-22.1.3.tar.gz

# MinGW headers
RUN mkdir /build/build-headers-x86_64
WORKDIR /build/build-headers-x86_64
RUN /build/mingw-w64/mingw-w64-headers/configure --prefix=/build/x86_64-w64-mingw32  --enable-idl --with-default-win32-winnt=0x601 --with-default-msvcrt=ucrt
RUN make
RUN make install

RUN mkdir /build/build-headers-aarch64
WORKDIR /build/build-headers-aarch64
RUN /build/mingw-w64/mingw-w64-headers/configure --prefix=/build/aarch64-w64-mingw32  --enable-idl --with-default-win32-winnt=0x601 --with-default-msvcrt=ucrt
RUN make
RUN make install

# MinGW libs
RUN mkdir /build/build-crt-x86_64
WORKDIR /build/build-crt-x86_64
RUN AS=llvm-as-19 AR=llvm-ar-19 RANLIB=llvm-ranlib-19 DLLTOOL=llvm-dlltool-19 CC="clang-19 --target=x86_64-w64-mingw32 --sysroot=/build/x86_64-w64-mingw32" CXX="clang++-19 --target=x86_64-w64-mingw32 --sysroot=/build/x86_64-w64-mingw32" /build/mingw-w64/mingw-w64-crt/configure --host=x86_64-w64-mingw32 --prefix=/build/x86_64-w64-mingw32 --disable-lib32 --enable-lib64 --with-default-msvcrt=ucrt --enable-cfguard
RUN make -j$(nproc)
RUN make install

RUN mkdir /build/build-crt-aarch64
WORKDIR /build/build-crt-aarch64
RUN AS=llvm-as-19 AR=llvm-ar-19 RANLIB=llvm-ranlib-19 DLLTOOL=llvm-dlltool-19 CC="clang-19 --target=aarch64-w64-mingw32 --sysroot=/build/aarch64-w64-mingw32" CXX="clang++-19 --target=aarch64-w64-mingw32 --sysroot=/build/aarch64-w64-mingw32" /build/mingw-w64/mingw-w64-crt/configure --host=aarch64-w64-mingw32 --prefix=/build/aarch64-w64-mingw32 --disable-lib32 --disable-lib64  --enable-libarm64 --with-default-msvcrt=ucrt --enable-cfguard
RUN make -j$(nproc)
RUN make install

# libunwind
COPY <<EOF /build/toolchain-x86_64.cmake
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSROOT /build/x86_64-w64-mingw32)

set(CMAKE_ASM_COMPILER clang-19)
set(CMAKE_ASM_COMPILER_TARGET x86_64-w64-mingw32)
set(CMAKE_C_COMPILER clang-19)
set(CMAKE_C_COMPILER_TARGET x86_64-w64-mingw32)
set(CMAKE_CXX_COMPILER clang++-19)
set(CMAKE_CXX_COMPILER_TARGET x86_64-w64-mingw32)

set(CMAKE_AR llvm-ar-19)
set(CMAKE_RANLIB llvm-ranlib-19)
EOF
RUN mkdir /build/build-unwind-x86_64
WORKDIR /build/build-unwind-x86_64
RUN cmake -G Ninja -DCMAKE_TOOLCHAIN_FILE=/build/toolchain-x86_64.cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/build/x86_64-w64-mingw32 -DCMAKE_C_COMPILER_WORKS=TRUE -DCMAKE_CXX_COMPILER_WORKS=TRUE -DCXX_SUPPORTS_FNO_EXCEPTIONS_FLAG=ON  -DLLVM_ENABLE_RUNTIMES="libunwind" -DLIBUNWIND_USE_COMPILER_RT=TRUE -DLIBUNWIND_ENABLE_SHARED=OFF -DLIBUNWIND_ENABLE_STATIC=ON -DCMAKE_C_FLAGS_INIT="-mguard=cf -D__USE_MINGW_ANSI_STDIO=1" -DCMAKE_CXX_FLAGS_INIT="-mguard=cf -D__USE_MINGW_ANSI_STDIO=1" /build/llvm-project-llvmorg-22.1.3/runtimes
RUN cmake --build .
RUN cmake --install .

COPY <<EOF /build/toolchain-aarch64.cmake
set(CMAKE_SYSTEM_NAME Windows)
set(CMAKE_SYSROOT /build/aarch64-w64-mingw32)

set(CMAKE_ASM_COMPILER clang-19)
set(CMAKE_ASM_COMPILER_TARGET aarch64-w64-mingw32)
set(CMAKE_C_COMPILER clang-19)
set(CMAKE_C_COMPILER_TARGET aarch64-w64-mingw32)
set(CMAKE_CXX_COMPILER clang++-19)
set(CMAKE_CXX_COMPILER_TARGET aarch64-w64-mingw32)

set(CMAKE_AR llvm-ar-19)
set(CMAKE_RANLIB llvm-ranlib-19)
EOF
RUN mkdir /build/build-unwind-aarch64
WORKDIR /build/build-unwind-aarch64
RUN cmake -G Ninja -DCMAKE_TOOLCHAIN_FILE=/build/toolchain-aarch64.cmake -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/build/aarch64-w64-mingw32 -DCMAKE_C_COMPILER_WORKS=TRUE -DCMAKE_CXX_COMPILER_WORKS=TRUE -DCXX_SUPPORTS_FNO_EXCEPTIONS_FLAG=ON  -DLLVM_ENABLE_RUNTIMES="libunwind" -DLIBUNWIND_USE_COMPILER_RT=TRUE -DLIBUNWIND_ENABLE_SHARED=OFF -DLIBUNWIND_ENABLE_STATIC=ON -DCMAKE_C_FLAGS_INIT="-mguard=cf -D__USE_MINGW_ANSI_STDIO=1" -DCMAKE_CXX_FLAGS_INIT="-mguard=cf -D__USE_MINGW_ANSI_STDIO=1" /build/llvm-project-llvmorg-22.1.3/runtimes
RUN cmake --build .
RUN cmake --install .

WORKDIR /build
RUN tar czf mingw-w64-v14.0.0.tar.gz aarch64-w64-mingw32 x86_64-w64-mingw32

FROM debian:trixie as build-awawausb

# NOTE: gcc is required for building native build scripts etc
RUN apt update && apt -y install gcc llvm curl zip
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | bash -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup target add x86_64-apple-darwin
RUN rustup target add x86_64-pc-windows-gnullvm
RUN rustup target add x86_64-unknown-linux-musl
RUN rustup target add aarch64-apple-darwin
RUN rustup target add aarch64-pc-windows-gnullvm
RUN rustup target add aarch64-unknown-linux-musl

WORKDIR /build/mingw-w64-minimal
COPY --from=build-mingw /build/mingw-w64-v14.0.0.tar.gz /build/mingw-w64-minimal
RUN tar xzf mingw-w64-v14.0.0.tar.gz
RUN rm -f mingw-w64-v14.0.0.tar.gz

COPY native-stub /build/awawausb/native-stub
COPY usb-ch9 /build/awawausb/usb-ch9

WORKDIR /build/awawausb/native-stub
RUN rm -rf target
RUN cargo build --release --target x86_64-apple-darwin
RUN cargo build --release --target x86_64-pc-windows-gnullvm
RUN cargo build --release --target x86_64-unknown-linux-musl
RUN cargo build --release --target aarch64-apple-darwin
RUN cargo build --release --target aarch64-pc-windows-gnullvm
RUN cargo build --release --target aarch64-unknown-linux-musl

RUN mkdir awawausb-native-stub
RUN llvm-lipo-19 target/aarch64-apple-darwin/release/awawausb-native-stub target/x86_64-apple-darwin/release/awawausb-native-stub -create -output awawausb-native-stub/awawausb-native-stub-mac
RUN cp target/x86_64-unknown-linux-musl/release/awawausb-native-stub awawausb-native-stub/awawausb-native-stub-linux-x86_64
RUN cp target/aarch64-unknown-linux-musl/release/awawausb-native-stub awawausb-native-stub/awawausb-native-stub-linux-aarch64
RUN cp target/x86_64-pc-windows-gnullvm/release/awawausb-native-stub.exe awawausb-native-stub/awawausb-native-stub-win-x86_64.exe
RUN cp target/aarch64-pc-windows-gnullvm/release/awawausb-native-stub.exe awawausb-native-stub/awawausb-native-stub-win-aarch64.exe
RUN zip -r awawausb-native-stub-dist.zip awawausb-native-stub/

FROM scratch
COPY --from=build-awawausb /build/awawausb/native-stub/awawausb-native-stub-dist.zip /
