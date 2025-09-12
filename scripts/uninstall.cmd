@echo off
setlocal

REM Wrapper to run the PowerShell uninstaller with proper flags
set SCRIPT_DIR=%~dp0

powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%uninstall.ps1" %*
set EXITCODE=%ERRORLEVEL%
if %EXITCODE% NEQ 0 (
  echo Uninstaller failed with exit code %EXITCODE%.
  exit /b %EXITCODE%
)

echo Uninstall completed successfully.
exit /b 0
