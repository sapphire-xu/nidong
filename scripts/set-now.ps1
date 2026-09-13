# nidong test clock. Save as UTF-8 with BOM for Windows PowerShell 5.1.
param(
    [Parameter(Position = 0)]
    [string]$When,
    [switch]$Gui
)

$ErrorActionPreference = "Stop"
$appDir = Join-Path $env:APPDATA "edu.neu.nidong"
$appFile = Join-Path $appDir "dev-now.txt"
$repoTmp = Join-Path $PSScriptRoot "..\tmp\now.txt"

function Get-CurrentStamp {
    if (Test-Path -LiteralPath $appFile) {
        return ((Get-Content -LiteralPath $appFile -Raw -Encoding UTF8).Trim())
    }
    return ""
}

function Save-Stamp([string]$stamp) {
    New-Item -ItemType Directory -Force -Path $appDir | Out-Null
    $tmpDir = Split-Path -Parent $repoTmp
    New-Item -ItemType Directory -Force -Path $tmpDir | Out-Null
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText($appFile, ($stamp.Trim() + [Environment]::NewLine), $utf8)
    [System.IO.File]::WriteAllText($repoTmp, ($stamp.Trim() + [Environment]::NewLine), $utf8)
}

function Clear-Stamp {
    Remove-Item -LiteralPath $appFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $repoTmp -ErrorAction SilentlyContinue
}

if ($Gui) {
    Add-Type -AssemblyName Microsoft.VisualBasic
    $cur = Get-CurrentStamp
    $hint = $cur
    if (-not $hint) { $hint = Get-Date -Format "yyyy-MM-dd HH:mm" }
    $nl = [char]10
    $msg = "yyyy-MM-dd HH:mm" + $nl + "OK = apply. Cancel = no change. Use clear-now.bat to restore system time."
    $input = [Microsoft.VisualBasic.Interaction]::InputBox($msg, "nidong clock", $hint)
    if ($input -eq "") { exit 0 }
    Save-Stamp $input
    exit 0
}

if (-not $When) {
    $cur = Get-CurrentStamp
    if ($cur) { Write-Output ("override: " + $cur) } else { Write-Output "no override (system time)" }
    exit 0
}

if ($When -eq "clear" -or $When -eq "--clear" -or $When -eq "-c") {
    Clear-Stamp
    Write-Output "cleared"
    exit 0
}

Save-Stamp $When
Write-Output ("override: " + $When.Trim())
