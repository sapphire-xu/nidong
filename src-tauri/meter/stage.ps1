$ErrorActionPreference = "Stop"
$manifest = Join-Path $PSScriptRoot "Cargo.toml"
$tauri = Split-Path $PSScriptRoot -Parent
$env:CARGO_TARGET_DIR = Join-Path $tauri "target"

# 提权的 nidong-meter 占着 target\release\nidong-meter.exe 时，cargo 换不掉它。
$running = Get-Process -Name "nidong-meter" -ErrorAction SilentlyContinue
if ($running) {
  $stopDir = Join-Path $env:APPDATA "edu.neu.nidong\meter"
  New-Item -ItemType Directory -Force -Path $stopDir | Out-Null
  Set-Content -LiteralPath (Join-Path $stopDir "stop") -Value "stop" -Encoding ascii
  cmd /c "schtasks /End /TN NidongMeter >nul 2>&1"
  cmd /c "taskkill /F /IM nidong-meter.exe /T >nul 2>&1"
  for ($i = 0; $i -lt 10; $i++) {
    if (-not (Get-Process -Name "nidong-meter" -ErrorAction SilentlyContinue)) { break }
    Start-Sleep -Seconds 1
  }
  if (Get-Process -Name "nidong-meter" -ErrorAction SilentlyContinue) {
    Write-Error "nidong-meter.exe 仍在运行，占着编译产物。"
    exit 1
  }
  Remove-Item -LiteralPath (Join-Path $stopDir "stop") -Force -ErrorAction SilentlyContinue
}

cargo build --release --manifest-path $manifest
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$destDir = Join-Path $tauri "binaries"
New-Item -ItemType Directory -Force -Path $destDir | Out-Null
Copy-Item (Join-Path $env:CARGO_TARGET_DIR "release\nidong-meter.exe") (Join-Path $destDir "nidong-meter-x86_64-pc-windows-msvc.exe") -Force
