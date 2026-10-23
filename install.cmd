@echo off
rem Double-click to install. Runs install.ps1 next to this file; options for install.ps1
rem (e.g. -GameDir "D:\Games\ForzaHorizon6") can be added after the command name.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
echo.
pause
