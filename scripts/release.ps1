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
npx tauri build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Output "Installer: src-tauri\target\release\bundle\nsis\ . Paste the sibling .sig into Gitee updates/latest.json."
