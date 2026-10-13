@echo off
cd /d "%~dp0"
title nidong

set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

set "PF86=%ProgramFiles(x86)%"
set "VSWHERE=%PF86%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" goto :run
for /f "delims=" %%i in ('"%VSWHERE%" -latest -products * -property installationPath') do set "VSINSTALL=%%i"
if not defined VSINSTALL goto :run
if exist "%VSINSTALL%\VC\Auxiliary\Build\vcvars64.bat" call "%VSINSTALL%\VC\Auxiliary\Build\vcvars64.bat"

:run
where node >nul 2>&1
if errorlevel 1 (
  echo [nidong] Node.js not found.
  pause
  exit /b 1
)
where cargo >nul 2>&1
if errorlevel 1 (
  echo [nidong] cargo not found. Install rustup first.
  pause
  exit /b 1
)

if not exist "node_modules\" (
  echo [nidong] npm install...
  call npm install
  if errorlevel 1 (
    echo [nidong] npm install failed.
    pause
    exit /b 1
  )
)

taskkill /IM nidong.exe /F >nul 2>&1

echo [nidong] starting. Close this window to quit.
echo.
call "%~dp0scripts\stage-meter.bat"
if errorlevel 1 (
  echo [nidong] meter sidecar failed.
  pause
  exit /b 1
)
call npm start
echo.
echo [nidong] exited.
pause