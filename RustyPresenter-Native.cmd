@echo off
rem Starts the native preview (room and stage drawn natively, operator still in the browser).
rem The first start builds the app (a few minutes); later starts are quick. Quit with Ctrl+C here.
setlocal EnableDelayedExpansion
cd /d "%~dp0"
if exist "%~dp0.tools\cargo\bin\cargo.exe" (
  set "CARGO_HOME=%~dp0.tools\cargo"
  set "RUSTUP_HOME=%~dp0.tools\rustup"
  set "PATH=%~dp0.tools\cargo\bin;%PATH%"
  set "PATH=%~dp0.tools\mingw64\bin;!PATH!"
  set "CARGO_ENCODED_RUSTFLAGS=-Cdlltool=%~dp0.tools\mingw64\bin\dlltool.exe"
)
set "CARGO_TARGET_DIR=%LOCALAPPDATA%\RustyPresenter\build"
title RustyPresenter (native preview)
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust is not installed. See README.md.
  pause
  exit /b 1
)
echo Starting the RustyPresenter native preview... (the first start takes a few minutes to build)
cargo run --release --quiet --bin rustypresenter-native -- %*
if errorlevel 1 pause
