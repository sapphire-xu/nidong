@echo off
REM ASCII-only to avoid cmd.exe GBK/UTF-8 mojibake.
cd /d "%~dp0"
powershell -NoProfile -STA -ExecutionPolicy Bypass -File "%~dp0set-now.ps1" -Gui
if errorlevel 1 pause
