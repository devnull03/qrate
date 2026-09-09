param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]]$AppArguments
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$app = Join-Path $root "target\debug\app.exe"
$protocolKey = "HKCU\Software\Classes\qrate"
$protocolBackup = Join-Path ([System.IO.Path]::GetTempPath()) "qrate-protocol-$([guid]::NewGuid()).reg"
$hadProtocolHandler = Test-Path "HKCU:\Software\Classes\qrate"
$keepProtocolBackup = $false

Push-Location $root
try {
    $publicKey = gh variable get QRATE_PLUGIN_CATALOG_PUBLIC_KEY --repo devnull03/qrate
    if ($LASTEXITCODE -ne 0 -or -not $publicKey) {
        throw "Could not read QRATE_PLUGIN_CATALOG_PUBLIC_KEY with GitHub CLI."
    }
    $env:QRATE_PLUGIN_CATALOG_PUBLIC_KEY = $publicKey.Trim()

    cargo build -p app
    if ($LASTEXITCODE -ne 0) {
        throw "qrate development build failed."
    }

    if ($hadProtocolHandler) {
        & reg.exe export $protocolKey $protocolBackup /y | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "Could not back up the existing qrate:// handler."
        }
    }
    & "$PSScriptRoot\register-dev-protocol.ps1" -Executable $app
    try {
        & $app @AppArguments
    }
    finally {
        & "$PSScriptRoot\register-dev-protocol.ps1" -Unregister
        if ($hadProtocolHandler) {
            & reg.exe import $protocolBackup | Out-Null
            if ($LASTEXITCODE -ne 0) {
                $keepProtocolBackup = $true
                Write-Warning "Could not restore the previous qrate:// handler from $protocolBackup"
            }
        }
    }
}
finally {
    if (-not $keepProtocolBackup -and (Test-Path $protocolBackup)) {
        Remove-Item $protocolBackup -Force
    }
    Pop-Location
}
