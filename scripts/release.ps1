$ErrorActionPreference = "Stop"
$key = Join-Path $env:USERPROFILE ".tauri\nidong.key"
if (-not (Test-Path -LiteralPath $key)) {
  Write-Error "Missing signing key: $key"
  exit 1
}
Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PATH -ErrorAction SilentlyContinue
$env:TAURI_SIGNING_PRIVATE_KEY = $key
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ""
$env:CI = "true"
Set-Location (Resolve-Path (Join-Path $PSScriptRoot ".."))
cmd /c "npx tauri build"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
$conf = [System.IO.File]::ReadAllText((Join-Path $root "src-tauri\tauri.conf.json"))
if ($conf -notmatch '"version"\s*:\s*"(\d+\.\d+\.\d+)"') {
  Write-Error "tauri.conf.json 里没有版本号"
  exit 1
}
$ver = $Matches[1]
$nsis = Join-Path $root "src-tauri\target\release\bundle\nsis"
$setup = Get-ChildItem -LiteralPath $nsis -Filter "*_${ver}_x64-setup.exe" -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $setup) {
  Write-Error "tauri build 结束了，但没有 ${ver} 的安装包。"
  exit 1
}
Write-Output ("Installer: " + $setup.FullName)
exit 0
