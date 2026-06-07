<#
.SYNOPSIS
  Regenerate the Windows .ico from assets/icons/app-icon.png.

.DESCRIPTION
  Single source of truth is assets/icons/app-icon.png (1024x1024).
  To change the app icon: replace that PNG, then run this script and commit
  the updated assets/icons/app-icon.ico. The macOS .icns is generated in CI
  (scripts/bundle-mac.sh); on macOS/Linux use scripts/gen-icons.sh instead.

  Produces a multi-resolution .ico whose entries are PNG-compressed
  (supported by Windows Vista+ and by the winresource crate used in build.rs).
#>
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

$root = Split-Path -Parent $PSScriptRoot
$src  = Join-Path $root 'assets\icons\app-icon.png'
$dest = Join-Path $root 'assets\icons\app-icon.ico'

if (-not (Test-Path $src)) { throw "Source icon not found: $src" }

$sizes = 256, 128, 64, 48, 32, 16
$source = [System.Drawing.Image]::FromFile($src)

# Render each size to PNG bytes in memory.
$pngs = foreach ($s in $sizes) {
    $bmp = New-Object System.Drawing.Bitmap($s, $s)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.InterpolationMode = 'HighQualityBicubic'
    $g.DrawImage($source, 0, 0, $s, $s)
    $g.Dispose()
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    [pscustomobject]@{ Size = $s; Bytes = $ms.ToArray() }
}
$source.Dispose()

$fs = [System.IO.File]::Create($dest)
$bw = New-Object System.IO.BinaryWriter($fs)
# ICONDIR header
$bw.Write([uint16]0)               # reserved
$bw.Write([uint16]1)               # type = icon
$bw.Write([uint16]$pngs.Count)     # image count
# ICONDIRENTRY (16 bytes each); data follows all entries
$offset = 6 + (16 * $pngs.Count)
foreach ($p in $pngs) {
    $dim = if ($p.Size -ge 256) { 0 } else { $p.Size }  # 0 means 256
    $bw.Write([byte]$dim)          # width
    $bw.Write([byte]$dim)          # height
    $bw.Write([byte]0)             # palette count
    $bw.Write([byte]0)             # reserved
    $bw.Write([uint16]1)           # color planes
    $bw.Write([uint16]32)          # bits per pixel
    $bw.Write([uint32]$p.Bytes.Length)
    $bw.Write([uint32]$offset)
    $offset += $p.Bytes.Length
}
foreach ($p in $pngs) { $bw.Write($p.Bytes) }
$bw.Flush(); $bw.Close(); $fs.Close()

Write-Output ("Wrote {0} ({1} sizes, {2} bytes)" -f $dest, $pngs.Count, (Get-Item $dest).Length)
