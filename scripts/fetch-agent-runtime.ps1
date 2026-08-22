param(
    [Parameter(Mandatory = $true)] [string] $Destination,
    [ValidateSet("windows-x64")] [string] $Platform = "windows-x64"
)

$ErrorActionPreference = "Stop"
$piVersion = "0.84.2"
$extensionVersion = "0.1.0"
$piSha256 = "741fc1ae1afecb573ac2888e011188ff446b3940f4aabe1583f60bf55be8a3d0"
$extensionSha256 = "16683b3ec9d93c3955b121a282d0c8ff8dfd8087a0fac4eb4297e24f9a516926"
$runtime = Join-Path ([System.IO.Path]::GetFullPath($Destination)) "agent"
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("qrate-agent-" + [guid]::NewGuid())

New-Item -ItemType Directory -Force -Path $runtime, $temp | Out-Null
try {
    $piArchive = Join-Path $temp "pi.zip"
    $extensionArchive = Join-Path $temp "extension.tar.gz"
    Invoke-WebRequest "https://github.com/earendil-works/pi/releases/download/v$piVersion/pi-$Platform.zip" -OutFile $piArchive
    Invoke-WebRequest "https://github.com/devnull03/qrate-pi-extension/releases/download/v$extensionVersion/qrate-pi-extension-$extensionVersion.tar.gz" -OutFile $extensionArchive

    if ((Get-FileHash $piArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $piSha256) {
        throw "Pi archive checksum did not match v$piVersion"
    }
    if ((Get-FileHash $extensionArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $extensionSha256) {
        throw "qrate Pi extension checksum did not match v$extensionVersion"
    }

    Expand-Archive $piArchive -DestinationPath (Join-Path $temp "pi")
    tar -xzf $extensionArchive -C $temp
    Copy-Item (Join-Path $temp "pi/pi.exe") (Join-Path $runtime "pi.exe") -Force
    $extension = Join-Path $runtime "qrate-pi-extension"
    New-Item -ItemType Directory -Force -Path $extension | Out-Null
    Copy-Item (Join-Path $temp "qrate-pi-extension-$extensionVersion/SYSTEM.md") $extension -Force
    Copy-Item (Join-Path $temp "qrate-pi-extension-$extensionVersion/extensions") $extension -Recurse -Force
    Copy-Item (Join-Path $temp "qrate-pi-extension-$extensionVersion/skills") $extension -Recurse -Force
    Write-Host "Fetched Pi $piVersion and qrate-pi-extension $extensionVersion into $runtime"
}
finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
