@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-raven-code.ps1"
exit /b %errorlevel%
