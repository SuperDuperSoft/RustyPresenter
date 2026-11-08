@echo off
rem Double-click to make RustyPresenter-Setup-<version>.exe on your Desktop: one file that installs
rem RustyPresenter on any Windows 10/11 PC for the person using it (no administrator password), with Start menu
rem and desktop shortcuts and an uninstaller that keeps the songs and services unless you tick a box.
rem
rem It builds the app exactly like try-latest.cmd (same branch, same build folder, so each makes the other faster),
rem then downloads NSIS (the installer maker) and the video/PDF libraries once into .tools\installer, checks each
rem download against packaging\pins.env, and builds the setup file.
rem
rem   make-installer.cmd                 the branch in .tools\xfer\latest-branch.txt
rem   make-installer.cmd finish-3        another branch
rem   make-installer.cmd --dry-run       only print the steps; nothing is built, downloaded or changed
rem
rem Run from a checkout of the code (not the folder with .tools), it builds that checkout as it is instead.
rem What happened goes to .tools\xfer\make-installer (build.txt, installer.txt, hashes.txt) for Claude to read.
rem Needs Git for Windows (its Git Bash runs packaging\windows\make-installer.sh) and the tools in .tools.
setlocal EnableDelayedExpansion
set "DRY=0"
set "BRANCH="
:args
if "%~1"=="" goto args_done
if /i "%~1"=="--dry-run" (set "DRY=1") else set "BRANCH=%~1"
shift
goto args
:args_done

rem HERE: this script's folder, without the trailing backslash (a "C:\x\" argument ends in an escaped quote).
set "HERE=%~dp0"
set "HERE=%HERE:~0,-1%"
title RustyPresenter - make the installer
cd /d "%HERE%"

where git >nul 2>nul
if errorlevel 1 goto no_git

rem ROOT: the folder with .tools (this one, or the main checkout this worktree belongs to).
set "ROOT="
if exist "%HERE%\.tools\cargo\bin\cargo.exe" set "ROOT=%HERE%"
if defined ROOT goto have_root
set "COMMON="
for /f "usebackq delims=" %%g in (`git -C "%HERE%" rev-parse --path-format^=absolute --git-common-dir 2^>nul`) do set "COMMON=%%g"
if not defined COMMON goto no_tools
for %%p in ("%COMMON%\..") do set "ROOT=%%~fp"
if not exist "%ROOT%\.tools\cargo\bin\cargo.exe" goto no_tools
:have_root
set "TOOLS=%ROOT%\.tools"
set "OUT=%TOOLS%\xfer\make-installer"
set "WT=%USERPROFILE%\rp-wt\windows-check"

rem SRC: the code to build. From the .tools folder: the branch, checked out in try-latest's worktree.
rem From any other checkout: that checkout.
set "SRC=%HERE%"
set "MODE=checkout"
if /i "%HERE%"=="%ROOT%" set "MODE=branch"
if "%MODE%"=="checkout" goto have_src
set "SRC=%WT%"
if "%BRANCH%"=="" if exist "%TOOLS%\xfer\latest-branch.txt" set /p BRANCH=<"%TOOLS%\xfer\latest-branch.txt"
if "%BRANCH%"=="" set "BRANCH=finish-3"
:have_src

set "DESKTOP="
for /f "usebackq delims=" %%d in (`powershell -NoProfile -Command "[Environment]::GetFolderPath('Desktop')"`) do set "DESKTOP=%%d"
if "%DESKTOP%"=="" set "DESKTOP=%USERPROFILE%\Desktop"

rem Git Bash, never C:\Windows\System32\bash.exe (that is WSL).
set "BASH="
for /f "delims=" %%g in ('where git 2^>nul') do if not defined GITEXE set "GITEXE=%%g"
for %%g in ("%GITEXE%") do set "GITDIR=%%~dpg"
if exist "%GITDIR%..\bin\bash.exe" for %%b in ("%GITDIR%..\bin\bash.exe") do set "BASH=%%~fb"
if not defined BASH if exist "%GITDIR%..\..\bin\bash.exe" for %%b in ("%GITDIR%..\..\bin\bash.exe") do set "BASH=%%~fb"
if not defined BASH if exist "%ProgramFiles%\Git\bin\bash.exe" set "BASH=%ProgramFiles%\Git\bin\bash.exe"
if not defined BASH if exist "%LOCALAPPDATA%\Programs\Git\bin\bash.exe" set "BASH=%LOCALAPPDATA%\Programs\Git\bin\bash.exe"

set "CARGO_HOME=%TOOLS%\cargo"
set "RUSTUP_HOME=%TOOLS%\rustup"
set "PATH=%TOOLS%\mingw64\bin;%TOOLS%\cargo\bin;%PATH%"
set "CARGO_ENCODED_RUSTFLAGS=-Cdlltool=%TOOLS%\mingw64\bin\dlltool.exe"
set "CARGO_TARGET_DIR=%LOCALAPPDATA%\RustyPresenter\build-check"
set "CARGO_TERM_COLOR=never"
set "CARGO_BUILD_JOBS=2"

set "RP_EXE=%CARGO_TARGET_DIR%\release\rustypresenter.exe"
set "RP_TOOLS=%TOOLS%"
set "RP_OUT_DIR=%DESKTOP%"
set "RP_MINGW_BIN=%TOOLS%\mingw64\bin"
set "RP_DEPS=%LOCALAPPDATA%\RustyPresenter\deps"
set "RP_XFER=%OUT%"
set "SH=%SRC%\packaging\windows\make-installer.sh"
set "SH=%SH:\=/%"

echo RustyPresenter installer
echo   tools folder  %TOOLS%
if "%MODE%"=="branch" echo   code          branch %BRANCH%, checked out in %WT%
if "%MODE%"=="checkout" echo   code          %SRC% (as it is)
echo   build folder  %CARGO_TARGET_DIR%
echo   setup file    goes to %DESKTOP%
echo   Git Bash      %BASH%
echo   log           %OUT%\installer.txt

if "%DRY%"=="0" goto real_run
echo.
echo == Dry run: nothing is fetched, built, downloaded or changed.
if "%MODE%"=="branch" echo == Would run: git -C "%WT%" checkout --force --detach %BRANCH%
echo == Would run in %SRC%: cargo build --release  (output to %OUT%\build.txt)
if not defined BASH goto no_bash
if not exist "%SRC%\packaging\windows\make-installer.sh" (
  echo == Would run packaging\windows\make-installer.sh from %SRC% once the branch is checked out.
  goto dry_done
)
echo == Would run: "%BASH%" "%SH%" --dry-run  -- its own dry run follows:
"%BASH%" "%SH%" --dry-run
:dry_done
echo.
echo Dry run finished.
pause
exit /b 0

:real_run
if not defined BASH goto no_bash
if not exist "%OUT%" mkdir "%OUT%"
if "%MODE%"=="checkout" goto build
echo.
echo Getting %BRANCH%...
if exist "%WT%\.git" goto have_worktree
git -C "%ROOT%" worktree add --detach "%WT%" %BRANCH% > "%OUT%\git.txt" 2>&1
if errorlevel 1 goto git_failed
:have_worktree
git -C "%WT%" checkout --force --detach %BRANCH% >> "%OUT%\git.txt" 2>&1
if errorlevel 1 goto git_failed

:build
for /f %%c in ('git -C "%SRC%" rev-parse --short HEAD 2^>nul') do set "COMMIT=%%c"
cd /d "%SRC%"
echo.
echo Building RustyPresenter (%COMMIT%). This window stays quiet until it is done (5-10 minutes)...
cargo build --release > "%OUT%\build.txt" 2>&1
if errorlevel 1 goto build_failed
if not exist "%SRC%\packaging\windows\make-installer.sh" goto old_branch

echo.
echo Making the installer...
if exist "%OUT%\setup-path.txt" del /q "%OUT%\setup-path.txt"
set "RP_LOG=%OUT%\installer.txt"
"%BASH%" "%SH%"
set "SH_EXIT=!ERRORLEVEL!"
if "%SH_EXIT%"=="3" goto unpinned
if not "%SH_EXIT%"=="0" goto installer_failed
if not exist "%OUT%\setup-path.txt" goto installer_failed
set /p SETUP=<"%OUT%\setup-path.txt"
echo.
echo Done. The installer is on your Desktop:
echo   %SETUP%
echo Double-click it on any Windows PC to install RustyPresenter (no administrator password needed).
echo If Windows says "Windows protected your PC", click "More info", then "Run anyway" (it is not signed yet).
pause
exit /b 0

:unpinned
echo.
echo The installer was NOT made yet: some downloads are new and their fingerprints are not in the code yet.
echo Send Claude this file, then run make-installer.cmd again once Claude says so:
echo   %OUT%\hashes.txt
pause
exit /b 3

:installer_failed
echo.
echo Making the installer failed. Claude can read the details in:
echo   %OUT%\installer.txt
pause
exit /b 1

:build_failed
echo.
echo The build failed. Claude can read the details in:
echo   %OUT%\build.txt
pause
exit /b 1

:old_branch
echo.
echo The code in %SRC% has no packaging\windows\make-installer.sh: that branch is older than this script.
pause
exit /b 1

:no_git
echo Git is not on this PC's PATH, so the code cannot be fetched. Install Git for Windows from git-scm.com.
pause
exit /b 1

:no_bash
echo Git Bash was not found (it comes with Git for Windows, in C:\Program Files\Git\bin\bash.exe).
pause
exit /b 1

:no_tools
echo The Rust tools are not in .tools\cargo next to this script or its main checkout. See README.md.
pause
exit /b 1

:git_failed
echo Git could not get %BRANCH%. See %OUT%\git.txt
type "%OUT%\git.txt"
pause
exit /b 1
