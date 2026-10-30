@echo off
rem Starts RustyPresenter. The first start builds the app (a few minutes); later starts are quick.
setlocal EnableDelayedExpansion
cd /d "%~dp0"
if exist "%~dp0.tools\cargo\bin\cargo.exe" (
  set "CARGO_HOME=%~dp0.tools\cargo"
  set "RUSTUP_HOME=%~dp0.tools\rustup"
  set "PATH=%~dp0.tools\cargo\bin;%PATH%"
  rem MinGW-w64 in .tools\mingw64 links the app and compiles Tauri's icon resource (windres). Its dlltool replaces
  rem the toolchain's self-contained one, which has no assembler beside it, so raw-dylib imports would fail.
  set "PATH=%~dp0.tools\mingw64\bin;!PATH!"
  set "CARGO_ENCODED_RUSTFLAGS=-Cdlltool=%~dp0.tools\mingw64\bin\dlltool.exe"
)
set "CARGO_TARGET_DIR=%LOCALAPPDATA%\RustyPresenter\build"
title RustyPresenter
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust is not installed. See README.md.
  pause
  exit /b 1
)
echo Starting RustyPresenter... (the first start takes a few minutes to build)
cargo run --release --quiet
if errorlevel 1 pause
