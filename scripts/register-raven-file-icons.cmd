@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0register-raven-file-icons.ps1"
if errorlevel 1 (
  echo.
  echo Raven Code file icon setup failed. Copy the error above when asking for help.
  if /i not "%~1"=="/quiet" pause
  exit /b 1
)
if /i not "%~1"=="/quiet" (
  echo.
  echo Setup completed. If icons do not refresh immediately, restart Windows Explorer.
  echo.
  pause
)
exit /b 0
