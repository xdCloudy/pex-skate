@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-PEXSkateData.ps1"
if errorlevel 1 (
  echo PEX Skate data setup failed. See the error dialog for details.
  pause
)
