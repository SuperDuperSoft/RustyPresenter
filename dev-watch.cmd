@echo off
rem Collaborator helper: rebuilds whenever a file named build.request appears. Close this window to stop.
cd /d "%~dp0"
title RustyPresenter dev builder (close when done)
echo Waiting for build requests... (close this window to stop)
:loop
if exist build.request (
  del build.request
  echo [%TIME%] Building...
  call "%~dp0dev-build.cmd"
  echo [%TIME%] Done. See build.log
)
if exist stop.request (
  del stop.request
  echo [%TIME%] Stopping the app...
  taskkill /im rustypresenter.exe /f >nul 2>nul
)
if exist run.request (
  del run.request
  echo [%TIME%] Starting the app...
  start "RustyPresenter" cmd /c "%~dp0RustyPresenter.cmd"
)
timeout /t 2 /nobreak >nul
goto loop
