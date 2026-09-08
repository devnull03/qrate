param(
    [string] $Destination = "target/debug"
)

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

New-Item -ItemType Directory -Force -Path $destinationPath, $temp | Out-Null
Write-Host "==> platform: win-$architecture, destination: $destinationPath"

try {
    $pdfiumUrl = "https://github.com/bblanchon/pdfium-binaries/releases/latest/download/pdfium-win-$architecture.tgz"
    $pdfiumArchive = Join-Path $temp "pdfium.tgz"
    Write-Host "==> pdfium: $pdfiumUrl"
    try {
        Invoke-WebRequest -UseBasicParsing $pdfiumUrl -OutFile $pdfiumArchive
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
        $ffmpegUrl = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
        $ffmpegArchive = Join-Path $temp "ffmpeg.zip"
        Write-Host "==> ffmpeg: $ffmpegUrl"
        try {
            Invoke-WebRequest -UseBasicParsing $ffmpegUrl -OutFile $ffmpegArchive
            $ffmpegDirectory = Join-Path $temp "ffmpeg"
            Expand-Archive $ffmpegArchive -DestinationPath $ffmpegDirectory -Force
            $ffmpeg = Get-ChildItem $ffmpegDirectory -Filter "ffmpeg.exe" -File -Recurse |
                Select-Object -First 1
            if ($null -eq $ffmpeg) {
                throw "No ffmpeg.exe was found in $ffmpegUrl; the archive layout may have changed"
            }
            Copy-Item $ffmpeg.FullName $destinationPath -Force
            Write-Host "    installed ffmpeg.exe"
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
