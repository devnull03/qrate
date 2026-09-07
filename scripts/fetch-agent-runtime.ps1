param(
    [Parameter(Mandatory = $true)] [string] $Destination,
    [ValidateSet("windows-x64")] [string] $Platform = "windows-x64"
)

$ErrorActionPreference = "Stop"
$piVersion = "0.84.2"
$extensionVersion = "0.2.1"
$piSha256 = "741fc1ae1afecb573ac2888e011188ff446b3940f4aabe1583f60bf55be8a3d0"
$extensionSha256 = "feb4ce5dcb59f5d936122541b776a85cd9d2541e121b7c47de7c4efb517ed37d"
$runtime = Join-Path ([System.IO.Path]::GetFullPath($Destination)) "agent"
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("qrate-agent-" + [guid]::NewGuid())

function Download-ReleaseAsset([string] $Uri, [string] $OutFile) {
    for ($attempt = 1; $attempt -le 3; $attempt++) {
        try {
            Invoke-WebRequest $Uri -OutFile $OutFile
            return
        }
        catch {
            if ($attempt -eq 3) {
                throw
            }
            $delay = 2 * $attempt
            Write-Warning "Download failed (attempt $attempt of 3): $($_.Exception.Message). Retrying in $delay seconds."
            Start-Sleep -Seconds $delay
        }
    }
}

New-Item -ItemType Directory -Force -Path $runtime, $temp | Out-Null
try {
    $piArchive = Join-Path $temp "pi.zip"
    $extensionArchive = Join-Path $temp "extension.tar.gz"
    Download-ReleaseAsset "https://github.com/earendil-works/pi/releases/download/v$piVersion/pi-$Platform.zip" $piArchive
    Download-ReleaseAsset "https://github.com/devnull03/qrate-pi-extension/releases/download/v$extensionVersion/qrate-pi-extension-$extensionVersion.tar.gz" $extensionArchive

    if ((Get-FileHash $piArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $piSha256) {
        throw "Pi archive checksum did not match v$piVersion"
    }
    if ((Get-FileHash $extensionArchive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $extensionSha256) {
        throw "qrate Pi extension checksum did not match v$extensionVersion"
    }

    Expand-Archive $piArchive -DestinationPath (Join-Path $temp "pi")
    tar -xzf $extensionArchive -C $temp
    # Pi's executable loads its built-in themes and native helpers beside itself. Copy the complete
    # release payload; a lone pi.exe starts under ConPTY but exits as soon as the TUI loads.
    Copy-Item (Join-Path $temp "pi/*") $runtime -Recurse -Force
    $extension = Join-Path $runtime "qrate-pi-extension"
    New-Item -ItemType Directory -Force -Path $extension | Out-Null
    Copy-Item (Join-Path $temp "qrate-pi-extension-$extensionVersion/SYSTEM.md") $extension -Force
    Copy-Item (Join-Path $temp "qrate-pi-extension-$extensionVersion/extensions") $extension -Recurse -Force
    Copy-Item (Join-Path $temp "qrate-pi-extension-$extensionVersion/src") $extension -Recurse -Force
    Write-Host "Fetched Pi $piVersion and qrate-pi-extension $extensionVersion into $runtime"
}
finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
