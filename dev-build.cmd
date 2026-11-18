@echo off
rem Builds and tests RustyPresenter, writing everything to build.log (for collaborators).
setlocal EnableDelayedExpansion
cd /d "%~dp0"
if exist "%~dp0.tools\cargo\bin\cargo.exe" (
  set "CARGO_HOME=%~dp0.tools\cargo"
  set "RUSTUP_HOME=%~dp0.tools\rustup"
  set "PATH=%~dp0.tools\cargo\bin;%PATH%"
  rem MinGW-w64 in .tools\mingw64 links the app and compiles its icon resource (windres). Its dlltool replaces
  rem the toolchain's self-contained one, which has no assembler beside it, so raw-dylib imports would fail.
  set "PATH=%~dp0.tools\mingw64\bin;!PATH!"
  set "CARGO_ENCODED_RUSTFLAGS=-Cdlltool=%~dp0.tools\mingw64\bin\dlltool.exe"
)
set "CARGO_TARGET_DIR=%LOCALAPPDATA%\RustyPresenter\build"
set "CARGO_TERM_COLOR=never"
echo BUILD STARTED %DATE% %TIME% > build.log
where dlltool >> build.log 2>&1
if exist "%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" ("%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe" -products * -property displayName >> build.log 2>&1) else (echo NO VSWHERE >> build.log)
where ffmpeg >> build.log 2>&1
cargo build --locked --release >> build.log 2>&1
set "BUILD_EXIT=%ERRORLEVEL%"
echo BUILD EXIT %BUILD_EXIT% >> build.log
cargo test --locked --release >> build.log 2>&1
set "TEST_EXIT=%ERRORLEVEL%"
echo TEST EXIT %TEST_EXIT% >> build.log
echo BUILD FINISHED %DATE% %TIME% >> build.log
if not "%BUILD_EXIT%"=="0" exit /b %BUILD_EXIT%
exit /b %TEST_EXIT%
