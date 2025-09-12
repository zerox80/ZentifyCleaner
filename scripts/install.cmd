@echo off
setlocal

REM Wrapper to run the PowerShell installer with proper flags
set SCRIPT_DIR=%~dp0

REM Call PowerShell installer, pass through all user arguments
powershell -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%install.ps1" %*
set EXITCODE=%ERRORLEVEL%
if %EXITCODE% NEQ 0 (
  echo Installer failed with exit code %EXITCODE%.
  exit /b %EXITCODE%
)

echo Installation completed successfully.
exit /b 0
