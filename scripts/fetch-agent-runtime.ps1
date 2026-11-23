param(
    [Parameter(Mandatory = $true)] [string] $Destination,
    [ValidateSet("windows-x64")] [string] $Platform = "windows-x64"
)

$ErrorActionPreference = "Stop"
$piVersion = "1.6.0"
$extensionVersion = "0.7.0"
$piSha256 = "f5defb6c403ee698f5b3f8886db473e55b78aac670a8ce03620a47f5f0aec57c"
$extensionSha256 = "8e4493e7aaa41770321800d2abb706b6febe9ea213c0e242a88d321fa448ddb2"
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
