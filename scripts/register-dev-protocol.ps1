param(
    [string]$Executable,
    [switch]$Unregister
)

$ErrorActionPreference = "Stop"
$scheme = "HKCU:\Software\Classes\qrate"

if ($Unregister) {
    if (Test-Path $scheme) {
        Remove-Item $scheme -Recurse -Force
    }
    Write-Host "Removed the per-user qrate:// development handler."
    exit 0
}

if (-not $Executable) {
    $root = Split-Path $PSScriptRoot -Parent
    $Executable = Join-Path $root "target\debug\app.exe"
}
if (-not (Test-Path $Executable -PathType Leaf)) {
    throw "qrate executable not found at '$Executable'. Run 'cargo build -p app' first."
}
$Executable = (Resolve-Path $Executable).Path

New-Item $scheme -Force | Out-Null
Set-Item $scheme -Value "URL:qrate Protocol"
New-ItemProperty $scheme -Name "URL Protocol" -PropertyType String -Value "" -Force | Out-Null
New-Item "$scheme\DefaultIcon" -Force | Out-Null
Set-Item "$scheme\DefaultIcon" -Value "`"$Executable`",0"
New-Item "$scheme\shell\open\command" -Force | Out-Null
Set-Item "$scheme\shell\open\command" -Value "`"$Executable`" `"%1`""

Write-Host "Registered qrate:// to $Executable"
Write-Host "Run this script with -Unregister to remove the development handler."
