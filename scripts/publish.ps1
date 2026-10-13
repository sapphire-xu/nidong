# 改版本号，打包，把安装包和签名写进 updates/，再推到 Gitee。
# 用法：scripts\publish.bat -Version 2.2.0 -Notes "写给用户的说明"
# -NoPush 只改本地文件，不提交、不推送。
param(
  [Parameter(Mandatory = $true)][string]$Version,
  [Parameter(Mandatory = $true)][string]$Notes,
  [string]$Remote = "gitee",
  [switch]$NoPush
)

$ErrorActionPreference = "Stop"
$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
  Write-Error "版本号用 x.y.z，例如 2.2.0"
  exit 1
}
$Notes = $Notes.Trim()
if (-not $Notes) {
  Write-Error "Notes 是写给用户的更新说明，不能空着"
  exit 1
}

function Read-CurrentVersion {
  $conf = [System.IO.File]::ReadAllText((Join-Path $root "src-tauri\tauri.conf.json"))
  if ($conf -notmatch '"version"\s*:\s*"(\d+\.\d+\.\d+)"') {
    throw "tauri.conf.json 里没有版本号"
  }
  return $Matches[1]
}

function Replace-Once([string]$path, [string]$pattern, [string]$replacement) {
  $full = Join-Path $root $path
  $text = [System.IO.File]::ReadAllText($full)
  $re = New-Object System.Text.RegularExpressions.Regex $pattern
  $new = $re.Replace($text, $replacement, 1)
  if ($new -eq $text) {
    throw "没有改到版本号: $path"
  }
  $utf8 = New-Object System.Text.UTF8Encoding $false
  [System.IO.File]::WriteAllText($full, $new, $utf8)
}

function Escape-Json([string]$s) {
  $sb = New-Object System.Text.StringBuilder
  foreach ($ch in $s.ToCharArray()) {
    switch ([int]$ch) {
      34 { [void]$sb.Append('\"'); continue }
      92 { [void]$sb.Append('\\'); continue }
      10 { [void]$sb.Append('\n'); continue }
      13 { continue }
      9 { [void]$sb.Append('\t'); continue }
      default {
        if ([int]$ch -lt 32) {
          [void]$sb.AppendFormat('\u{0:x4}', [int]$ch)
        } else {
          [void]$sb.Append($ch)
        }
      }
    }
  }
  return $sb.ToString()
}

function Read-ManifestVersion {
  $path = Join-Path $root "updates\latest.json"
  if (-not (Test-Path -LiteralPath $path)) { return "0.0.0" }
  $text = [System.IO.File]::ReadAllText($path)
  if ($text -notmatch '"version"\s*:\s*"(\d+\.\d+\.\d+)"') {
    throw "updates\latest.json 里没有版本号"
  }
  return $Matches[1]
}

$current = Read-CurrentVersion
$published = Read-ManifestVersion
if ([Version]$Version -le [Version]$published) {
  Write-Error "版本 $Version 不高于已发布的 $published。已安装的泥洞只认更高的清单版本。"
  exit 1
}
if ([Version]$Version -lt [Version]$current) {
  Write-Error "源码版本已经是 $current，高于要发的 $Version。要发就用 $current 或更高。"
  exit 1
}

$tracked = @(
  "package.json",
  "package-lock.json",
  "src-tauri\tauri.conf.json",
  "src-tauri\Cargo.toml",
  "src-tauri\Cargo.lock"
)
$saved = @{}
foreach ($rel in $tracked) {
  $saved[$rel] = [System.IO.File]::ReadAllBytes((Join-Path $root $rel))
}

function Restore-Versions {
  foreach ($rel in $saved.Keys) {
    [System.IO.File]::WriteAllBytes((Join-Path $root $rel), $saved[$rel])
  }
}

$verJson = '"version": "' + $Version + '"'
try {
  if ($current -eq $Version) {
    Write-Output "源码已经是 $Version，接着打包"
  } else {
  Replace-Once "package.json" '"version"\s*:\s*"\d+\.\d+\.\d+"' $verJson
  Replace-Once "src-tauri\tauri.conf.json" '"version"\s*:\s*"\d+\.\d+\.\d+"' $verJson
  Replace-Once "src-tauri\Cargo.toml" 'version = "\d+\.\d+\.\d+"' ('version = "' + $Version + '"')
  Replace-Once "src-tauri\Cargo.lock" 'name = "nidong"\r?\nversion = "\d+\.\d+\.\d+"' ("name = `"nidong`"`nversion = `"$Version`"")
  $lock = [System.IO.File]::ReadAllText((Join-Path $root "package-lock.json"))
  $needle = '"version": "' + $current + '"'
  $repl = '"version": "' + $Version + '"'
  $reLock = New-Object System.Text.RegularExpressions.Regex ([regex]::Escape($needle))
  $lock2 = $reLock.Replace($lock, $repl, 2)
  if ($lock2 -eq $lock) { throw "没有改到 package-lock.json 的版本号" }
  $utf8 = New-Object System.Text.UTF8Encoding $false
  [System.IO.File]::WriteAllText((Join-Path $root "package-lock.json"), $lock2, $utf8)
  }
} catch {
  Restore-Versions
  throw
}

Write-Output "打包 $Version"
cmd /c "powershell -NoProfile -ExecutionPolicy Bypass -File scripts\release.ps1"
$buildCode = $LASTEXITCODE
if ($buildCode -ne 0) {
  Write-Output "打包退出码 $buildCode。继续查找已经打好的 $Version 安装包。"
}

$nsis = Join-Path $root "src-tauri\target\release\bundle\nsis"
$exe = Get-ChildItem -LiteralPath $nsis -Filter "*_${Version}_x64-setup.exe" |
  Sort-Object LastWriteTime -Descending |
  Select-Object -First 1
if (-not $exe) {
  if ($buildCode -ne 0) {
    Restore-Versions
    Write-Error "打包失败，也没有 $Version 的安装包。版本号已改回 $current"
    exit $buildCode
  }
  Write-Error "没有找到 $Version 的安装包。nsis 目录里没有 *_${Version}_x64-setup.exe。版本号留在 $Version。"
  exit 1
}
$product = [string]$exe.VersionInfo.ProductVersion
if ($product -notmatch ('^' + [regex]::Escape($Version) + '(\.|$)')) {
  Restore-Versions
  Write-Error "安装包 $product 不是 $Version。版本号已改回 $current。没有写入更新清单。"
  exit 1
}
$sigPath = $exe.FullName + ".sig"
if (-not (Test-Path -LiteralPath $sigPath)) {
  Write-Error "没有找到签名 $($exe.Name).sig"
  exit 1
}

$destName = "nidong_${Version}_x64-setup.exe"
$dest = Join-Path $root "updates\$destName"
Copy-Item -LiteralPath $exe.FullName -Destination $dest -Force
$sig = [System.IO.File]::ReadAllText($sigPath).Trim()
$pub = [DateTime]::UtcNow.ToString("yyyy-MM-ddTHH:mm:ss") + "Z"
$url = "https://gitee.com/sapphire_fic/nidong/raw/master/updates/$destName"
$json = @"
{
  "version": "$Version",
  "notes": "$(Escape-Json $Notes)",
  "pub_date": "$pub",
  "platforms": {
    "windows-x86_64": {
      "signature": "$(Escape-Json $sig)",
      "url": "$url"
    }
  }
}
"@
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText((Join-Path $root "updates\latest.json"), $json.Trim() + "`n", $utf8)
Write-Output "已写入 updates\$destName 和 updates\latest.json"

if ($NoPush) {
  Write-Output "未推送。检查 updates\latest.json 后，自己提交并推到 Gitee 的 master 和 main。"
  exit 0
}

$branch = (git rev-parse --abbrev-ref HEAD).Trim()
if ($branch -ne "main") {
  Write-Error "当前分支是 $branch。请在 main 上发布。文件已经写好，尚未提交。"
  exit 1
}

git add -- package.json package-lock.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock updates/latest.json "updates/$destName"
if ($LASTEXITCODE -ne 0) { Write-Error "git add 失败"; exit $LASTEXITCODE }
git diff --cached --stat
$msgPath = Join-Path $env:TEMP "nidong-release-msg.txt"
[System.IO.File]::WriteAllText($msgPath, "发布 $Version`n", $utf8)
git commit -F $msgPath
if ($LASTEXITCODE -ne 0) { Write-Error "提交失败"; exit $LASTEXITCODE }
git push $Remote HEAD:master
if ($LASTEXITCODE -ne 0) { Write-Error "推到 $Remote master 失败。本地提交还在。"; exit $LASTEXITCODE }
git push $Remote HEAD:main
if ($LASTEXITCODE -ne 0) { Write-Error "master 已更新，推到 $Remote main 失败。"; exit $LASTEXITCODE }
Write-Output "已推到 $Remote 的 master 和 main。"
