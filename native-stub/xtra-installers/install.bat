@echo off

if NOT exist "%APPDATA%\awawausb-native-stub\" mkdir "%APPDATA%\awawausb-native-stub\"

if ["%PROCESSOR_ARCHITECTURE%"]==["AMD64"] (
    set binary=awawausb-native-stub-win-x86_64.exe
) else if ["%PROCESSOR_ARCHITECTURE%"]==["ARM64"] (
    set binary=awawausb-native-stub-win-aarch64.exe
) else (
    echo CPU architecture %PROCESSOR_ARCHITECTURE% not supported, sorry
    exit /b 1
)

REM Copy the binary
copy /b /y "%~dp0%binary%" "%APPDATA%\awawausb-native-stub\awawausb-native-stub.exe"
call :CHECK_FAIL

REM Copy the manifest
copy /b /y "%~dp0manifest-win.json" "%APPDATA%\awawausb-native-stub\awawausb-native-stub.json"
call :CHECK_FAIL

REM Make registry key
reg add "HKCU\Software\Mozilla\NativeMessagingHosts\awawausb_native_stub" /ve /t REG_SZ /d "%APPDATA%\awawausb-native-stub\awawausb-native-stub.json" /f /reg:64 
call :CHECK_FAIL

goto :eof

:CHECK_FAIL
if NOT ["%errorlevel%"]==["0"] (
    pause
    exit /b %errorlevel%
)

:EOF