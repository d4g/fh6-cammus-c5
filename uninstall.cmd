@echo off
rem Double-click to undo the installation. Runs uninstall.ps1 next to this file.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %*
echo.
pause
