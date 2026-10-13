@echo off
setlocal
cd /d "%~dp0.."

call :stop_meter
if errorlevel 1 exit /b 1

echo [nidong] building traffic meter...
cargo build --manifest-path src-tauri/meter/Cargo.toml --target-dir src-tauri/target
if errorlevel 1 exit /b 1
if not exist "src-tauri\binaries" mkdir "src-tauri\binaries"
copy /Y "src-tauri\target\debug\nidong-meter.exe" "src-tauri\binaries\nidong-meter-x86_64-pc-windows-msvc.exe" >nul
if errorlevel 1 (
  echo [nidong] could not copy nidong-meter.exe
  exit /b 1
)
exit /b 0

:stop_meter
tasklist /FI "IMAGENAME eq nidong-meter.exe" 2>nul | find /I "nidong-meter.exe" >nul
if errorlevel 1 exit /b 0

echo [nidong] stopping traffic meter so the build can replace it...
set "METER_DIR=%APPDATA%\edu.neu.nidong\meter"
if not exist "%METER_DIR%" mkdir "%METER_DIR%"
> "%METER_DIR%\stop" echo stop
schtasks /End /TN "NidongMeter" >nul 2>&1
taskkill /F /IM nidong-meter.exe /T >nul 2>&1

set /a TRIES=0
:wait_meter
tasklist /FI "IMAGENAME eq nidong-meter.exe" 2>nul | find /I "nidong-meter.exe" >nul
if errorlevel 1 goto :meter_gone
set /a TRIES+=1
if %TRIES% GEQ 10 goto :meter_stuck
ping -n 2 127.0.0.1 >nul
goto :wait_meter

:meter_stuck
echo [nidong] nidong-meter.exe is still running and locking the build.
echo [nidong] End the NidongMeter task, or close that process, then start again.
exit /b 1

:meter_gone
del /q "%METER_DIR%\stop" >nul 2>&1
exit /b 0
