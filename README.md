# WebUSB extension for Firefox

This extension adds WebUSB functionality to Firefox by making use of [native messaging](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_messaging).

In order to use this, you need to _both_ install the extension in your browser and install a small program (separate from the browser) on your computer. This extra program is called the "native stub".

## Feature support

This extension is supposed to be compatible with Chrome's implementation. Please report any differences you encounter that result in software not working.

However, unlike Chrome, this API is only exposed on the main page and is not available in [Web Workers](https://developer.mozilla.org/en-US/docs/Web/API/Web_Workers_API/Using_web_workers).

Android cannot be supported, because it does not have native messaging capabilities.

## Installation instructions

You can install this extension by downloading binaries from the GitHub "Releases" section (in the right-hand column), or you can build from source.

### Installing the extension

To install a signed version of the extension, download the .xpi file and open it in Firefox.

To load a _testing_ version of the extension in Firefox Developer Edition, open `about:debugging`, select "This Firefox" in the left-hand list, then "Load Temporary Add-on…", and then browse to the `manifest.json` inside the `extension/` directory.

### Installing the native stub

If you are using prebuilt binaries, unzip _all_ of the files and then run either `./install.sh` (on Linux or macOS) or `install.bat` (on Windows). These installers will try to automatically copy the appropriate files into a sensible location and then configure a [native manifest](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_manifests) so that the browser can find it.

Prebuilt binaries are available for the following platforms:

- macOS x86_64 and ARM64
- Linux x86_64 and aarch64
- Windows AMD64 and ARM64

If you are not using prebuilt binaries, see [below](#compiling-from-source).

### "Unusual" configurations

The default installer is known to have problems with uncommon configurations such as:

- sharing a \*nix home directory across different computers with different CPU architectures
- Windows roaming user profiles across computers with different CPU architectures

The root cause of this is because the "native manifest" mechanism was not designed well to take these situations into account (for example, through its use of absolute paths). If you are in one of these situations, you will unfortunately need to invent an ad-hoc workaround.

## System requirements

This native stub tries to avoid doing anything too "exciting", but, due to development and testing resource constraints, it focuses on "reasonably modern" desktop platforms.

### macOS

macOS 10.15 or later is required, matching Firefox's system requirements. However, older systems are not very well-tested. Expect macOS 12 to be a much more reasonable baseline.

### Windows

Windows 10 or later is required, due to the requirements of the Rust platform support. This also matches Firefox system requirements. Backporting to Windows 8/8.1 might theoretically be possible, but any older is not expected to work (due to limitations in WinUSB).

### Linux

Linux kernel version 4.8 or later is required. (More specifically, a kernel containing commit `5cce438` and `USBDEVFS_CAP_REAP_AFTER_DISCONNECT` is _strongly_ recommended, and a kernel supporting `USBDEVFS_DISCONNECT_CLAIM` is required.)

Your system must have `/dev` and `/sys` mounted.

In order to detect USB devices being connected, a userspace with [udev](https://man7.org/linux/man-pages/man7/udev.7.html) or a compatible daemon is required. Specifically, a daemon which broadcasts `0xfeedcafe`-format messages on `NETLINK_KOBJECT_UEVENT` group `2` is required.

## Compiling from source

In general, this native stub is written entirely in Rust and can be built using `cargo build` in the `native-stub` directory. Cross-compiling _is_ supported and is configured by default for the supported platforms.

If this does not "just work", the following notes might help:

### macOS

This _really_ should just work. The repository contains a vendored copy of all the `.tbd` files needed to link the final binary. If that is somehow causing problems, disable the appropriate entries in `.cargo/config.toml` (which will then require you to have a macOS SDK installed).

### Linux

Linux prebuilt binaries are set up to use [musl libc](https://www.musl-libc.org/) with Rust's default of static linking. The goal of this is to produce binaries which work across any distro. If this is not desired, you may need to change the appropriate `RUSTFLAGS`.

Glibc builds _should_ work but are not tested.

### Windows

Windows prebuilt binaries are set up to build using [mingw-w64](https://www.mingw-w64.org/) targeting the UCRT. This corresponds to the `*-windows-gnullvm` targets in Rust.

Windows is primarily tested using cross-builds from platforms _other than_ Windows. Building on Windows is supposed to work, but may require adding the `rust-mingw` component. See the [rustc documentation](https://doc.rust-lang.org/nightly/rustc/platform-support/windows-gnullvm.html#building-rust-programs) for more details.

If you are building this from a system other than Windows, you will need to obtain mingw-w64 `.lib` files from somewhere (such as by following the steps which happen to be in the [Dockerfile](Dockerfile)). There is a _hardcoded_ path in `.cargo/config.toml` which will need to be examined/changed in order to find these libraries.

Building with the MSVC toolchain is _not_ supported (there is no fundamental reason why it cannot work, it just isn't tested).

### Setting up a native manifest

In order for the browser to find your compiled binaries, you will need to install a "manifest" file in a specific location on your computer. The manifest is a short JSON (JavaScript Object Notation) file which goes into [a specific location](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/Native_manifests#manifest_location) depending on your operating system. To repeat the relevant lines of the documentation:

#### macOS

`/Library/Application Support/Mozilla/NativeMessagingHosts/awawausb_native_stub.json` (global)

`~/Library/Application Support/Mozilla/NativeMessagingHosts/awawausb_native_stub.json` (user-local)

#### Linux

`/usr/lib/mozilla/native-messaging-hosts/awawausb_native_stub.json` (global)

`/usr/lib64/mozilla/native-messaging-hosts/awawausb_native_stub.json` (global)

`~/.mozilla/native-messaging-hosts/awawausb_native_stub.json` (user-local)

#### Windows

The manifest file can be placed anywhere, but a registry key must be set to point to it. The registry keys are:

`HKLM\SOFTWARE\Mozilla\NativeMessagingHosts\awawausb_native_stub` (global)

`HKCU\SOFTWARE\Mozilla\NativeMessagingHosts\awawausb_native_stub` (user-local)

The following screenshot shows a correctly-configured registry entry:

![reg](Documentation/win-reg.png)

#### Contents of the native manifest

The JSON file should contain the following contents:

```json
{
  "name": "awawausb_native_stub",
  "description": "Allows WebUSB extension to access USB devices",
  "path": "/path/to/awawausb-native-stub",
  "type": "stdio",
  "allowed_extensions": ["awawausb@arcanenibble.com"]
}
```

However, on Windows, a full path is not required (i.e. only `"awawausb-native-stub.exe"` is sufficient).

## Developer documentation

See [Documentation/architecture.md](Documentation/architecture.md) to get started.
