param(
    [string] $Destination = "target/debug"
)

# Pinned to the same releases and hashes as fetch-binaries.sh; change both scripts together.
$pdfiumRelease = "chromium/7881"
$pdfiumSha256 = @{
    "x64"   = "73cc0de638ac2095e7445bf56a38200a5b7c7ca0e9f4ba144598f2457377ac08"
    "arm64" = "d3035d4d2cacac6ecd1a2ece197a3d702a1b2a58466276b9f870b8cb278a9d84"
}
$ffmpegRelease = "autobuild-2026-08-31-13-27"
$ffmpegBuild = "n8.1.2-50-g1a748fe2cd"
$ffmpegBranch = "8.1"
$ffmpegTarget = @{ "x64" = "win64"; "arm64" = "winarm64" }
$ffmpegSha256 = @{
    "win64"    = "f6274bbd9c247f9e90c1bbed066b03ed4a3907cece2fb91be6dd352393936365"
    "winarm64" = "0ad4d6e7342d6d77bbae4ac230964ebd51bdd6192414392e5f521d295b39b111"
}

$ErrorActionPreference = "Stop"
$destinationPath = [System.IO.Path]::GetFullPath($Destination)
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("qrate-binaries-" + [guid]::NewGuid())
$strict = -not [string]::IsNullOrEmpty($env:QRATE_STRICT_BINARIES)
$skipFfmpeg = -not [string]::IsNullOrEmpty($env:QRATE_SKIP_FFMPEG)
$architecture = switch ($env:PROCESSOR_ARCHITECTURE) {
    "AMD64" { "x64" }
    "ARM64" { "arm64" }
    default { throw "Unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE" }
}

# A download whose hash differs from its pin is treated as a failed download and never installed.
function Get-Pinned([string] $Url, [string] $OutFile, [string] $Sha256) {
    Invoke-WebRequest -UseBasicParsing $Url -OutFile $OutFile
    $actual = (Get-FileHash -Algorithm SHA256 $OutFile).Hash.ToLowerInvariant()
    if ($actual -ne $Sha256) {
        throw "$Url has SHA-256 $actual, but the pin says $Sha256"
    }
}

New-Item -ItemType Directory -Force -Path $destinationPath, $temp | Out-Null
Write-Host "==> platform: win-$architecture, destination: $destinationPath"

try {
    $pdfiumUrl = "https://github.com/bblanchon/pdfium-binaries/releases/download/$pdfiumRelease/pdfium-win-$architecture.tgz"
    $pdfiumArchive = Join-Path $temp "pdfium.tgz"
    Write-Host "==> pdfium: $pdfiumUrl"
    try {
        Get-Pinned $pdfiumUrl $pdfiumArchive $pdfiumSha256[$architecture]
        tar -xzf $pdfiumArchive -C $temp
        if ($LASTEXITCODE -ne 0) {
            throw "tar exited with code $LASTEXITCODE"
        }
        $pdfium = Get-ChildItem $temp -Filter "pdfium.dll" -File -Recurse |
            Select-Object -First 1
        if ($null -eq $pdfium) {
            throw "No PDFium library was found in $pdfiumUrl; the archive layout may have changed"
        }
        Copy-Item $pdfium.FullName $destinationPath -Force
        Write-Host "    installed pdfium.dll"
    }
    catch {
        if ($strict) {
            throw
        }
        Write-Warning "PDFium download failed; PDF previews will fall back to an icon: $_"
    }

    if ($skipFfmpeg) {
        Write-Host "==> ffmpeg: skipped (QRATE_SKIP_FFMPEG set)"
    }
    else {
        $target = $ffmpegTarget[$architecture]
        $ffmpegAsset = "ffmpeg-$ffmpegBuild-$target-lgpl-$ffmpegBranch.zip"
        $ffmpegUrl = "https://github.com/BtbN/FFmpeg-Builds/releases/download/$ffmpegRelease/$ffmpegAsset"
        $ffmpegArchive = Join-Path $temp $ffmpegAsset
        Write-Host "==> ffmpeg: $ffmpegUrl"
        try {
            Get-Pinned $ffmpegUrl $ffmpegArchive $ffmpegSha256[$target]
            $ffmpegDirectory = Join-Path $temp "ffmpeg"
            Expand-Archive $ffmpegArchive -DestinationPath $ffmpegDirectory -Force
            $ffmpeg = Get-ChildItem $ffmpegDirectory -Filter "ffmpeg.exe" -File -Recurse |
                Select-Object -First 1
            if ($null -eq $ffmpeg) {
                throw "No ffmpeg.exe was found in $ffmpegUrl; the archive layout may have changed"
            }
            Copy-Item $ffmpeg.FullName $destinationPath -Force
            Write-Host "    installed ffmpeg.exe $ffmpegBuild"
        }
        catch {
            if ($strict) {
                throw
            }
            Write-Warning "ffmpeg download failed; video previews will fall back to an icon: $_"
        }
    }

    Write-Host "==> done. Installed preview binaries:"
    Get-ChildItem $destinationPath -File |
        Where-Object Name -Match "^(pdfium\.dll|ffmpeg\.exe)$" |
        ForEach-Object { Write-Host "    $($_.Name)" }
}
finally {
    Remove-Item -LiteralPath $temp -Recurse -Force -ErrorAction SilentlyContinue
}
