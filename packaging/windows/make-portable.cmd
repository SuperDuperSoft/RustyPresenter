@echo off
rem Makes a portable copy of RustyPresenter: one zip to unzip and run on any Windows PC, with no install and no
rem Rust tools, that keeps its songs and services in the folder beside it (because of portable.txt).
rem Run try-latest.cmd (or windows-check.cmd) first, so the newest version is built. Builds from before
rem November 18, 2026 (version 1.4.0's portable-release branch) do not know portable.txt and keep the data in
rem %LOCALAPPDATA%\RustyPresenter.
rem
rem   make-portable.cmd              the zip on your Desktop
rem   make-portable.cmd --no-pause   the same, without waiting for a key at the end (for scripts)
rem   set RP_SRC=<checkout>          the checkout the build came from, when it was not try-latest's
rem
rem Result: RustyPresenter-<version>-windows-portable.zip on your Desktop. Copy it to the other PC (USB stick,
rem network share...). On the other PC: right-click the zip > Extract All, then double-click rustypresenter.exe.
rem The release builds (packaging\release-files.sh in .github\workflows\package.yml) make the same zip.
setlocal
set "NOPAUSE="
if /i "%~1"=="--no-pause" set "NOPAUSE=1"
rem HERE: this checkout's packaging\windows folder; its packaging\portable has the texts that go beside the app.
set "HERE=%~dp0"
set "TEXTS=%HERE%..\portable"
set "BUILD=%LOCALAPPDATA%\RustyPresenter\build-check\release"
set "WT=%USERPROFILE%\rp-wt\windows-check"
set "DEPS=%LOCALAPPDATA%\RustyPresenter\deps"
set "STAGE=%TEMP%\RustyPresenter-portable"
rem SRC: the code the build came from (try-latest's worktree), for the licences and the version; else this checkout.
set "SRC=%WT%"
if defined RP_SRC set "SRC=%RP_SRC%"
if not exist "%SRC%\Cargo.toml" for %%r in ("%HERE%..\..") do set "SRC=%%~fr"
set "VER="
for /f "usebackq delims=" %%v in (`powershell -NoProfile -Command "((Get-Content '%SRC%\Cargo.toml' | Where-Object { $_ -match '^version' } | Select-Object -First 1) -split [char]34)[1]"`) do set "VER=%%v"
if "%VER%"=="" set "VER=latest"
set "DESKTOP="
for /f "usebackq delims=" %%d in (`powershell -NoProfile -Command "[Environment]::GetFolderPath('Desktop')"`) do set "DESKTOP=%%d"
if "%DESKTOP%"=="" set "DESKTOP=%USERPROFILE%\Desktop"
set "ZIP=%DESKTOP%\RustyPresenter-%VER%-windows-portable.zip"
title RustyPresenter - make a portable copy

if not exist "%BUILD%\rustypresenter.exe" goto not_built
if not exist "%TEXTS%\portable.txt" goto no_texts
if exist "%STAGE%" rmdir /s /q "%STAGE%"
mkdir "%STAGE%\RustyPresenter"
set "DEST=%STAGE%\RustyPresenter"

echo Copying the app...
copy /y "%BUILD%\rustypresenter.exe" "%DEST%\" >nul
if exist "%DEPS%\mpv\libmpv-2.dll" copy /y "%DEPS%\mpv\libmpv-2.dll" "%DEST%\" >nul
if exist "%DEPS%\pdfium\bin\pdfium.dll" (copy /y "%DEPS%\pdfium\bin\pdfium.dll" "%DEST%\" >nul) else if exist "%DEPS%\pdfium\pdfium.dll" copy /y "%DEPS%\pdfium\pdfium.dll" "%DEST%\" >nul
for %%f in (LICENSE LICENSE-MIT LICENSE-APACHE README.md) do if exist "%SRC%\%%f" copy /y "%SRC%\%%f" "%DEST%\" >nul
mkdir "%DEST%\licenses"
if exist "%SRC%\THIRD-PARTY-LICENSES.txt" copy /y "%SRC%\THIRD-PARTY-LICENSES.txt" "%DEST%\licenses\" >nul
if exist "%SRC%\packaging\licenses" xcopy /e /i /q /y "%SRC%\packaging\licenses" "%DEST%\licenses" >nul
rem pdfium's own notices, named as packaging\stage.sh names them.
if exist "%DEPS%\pdfium\LICENSE" copy /y "%DEPS%\pdfium\LICENSE" "%DEST%\licenses\pdfium-LICENSE" >nul
if exist "%DEPS%\pdfium\licenses" for %%f in ("%DEPS%\pdfium\licenses\*") do copy /y "%%f" "%DEST%\licenses\pdfium-%%~nxf" >nul
for /f %%c in ('git -C "%SRC%" rev-parse --short HEAD 2^>nul') do > "%DEST%\VERSION.txt" echo RustyPresenter %VER%, built from %%c on %DATE%
rem The file that keeps the data beside the app, and how to start it.
copy /y "%TEXTS%\portable.txt" "%DEST%\portable.txt" >nul
copy /y "%TEXTS%\HOW-TO-START-windows.txt" "%DEST%\HOW-TO-START.txt" >nul

echo Making the zip...
if exist "%ZIP%" del /q "%ZIP%"
powershell -NoProfile -Command "Compress-Archive -Path '%DEST%' -DestinationPath '%ZIP%' -Force"
if errorlevel 1 goto zip_failed
rmdir /s /q "%STAGE%"
echo.
echo Done. The portable copy is here:
echo   %ZIP%
echo Copy it to the other PC (e.g. a USB stick), right-click it, Extract All, then double-click rustypresenter.exe.
echo Everything it saves goes in the folder "RustyPresenter Data" beside rustypresenter.exe.
if not exist "%DEPS%\mpv\libmpv-2.dll" echo NOTE: libmpv-2.dll was not found, so videos will not play on the other PC.
if not defined NOPAUSE pause
exit /b 0

:not_built
echo RustyPresenter has not been built yet. Run try-latest.cmd first, then run this again.
if not defined NOPAUSE pause
exit /b 1

:no_texts
echo %TEXTS%\portable.txt is missing. Run this from a checkout of the code that has packaging\portable.
if not defined NOPAUSE pause
exit /b 1

:zip_failed
echo Could not make the zip. The files are still in %DEST%
if not defined NOPAUSE pause
exit /b 1
