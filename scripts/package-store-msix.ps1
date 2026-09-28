param(
  [Parameter(Mandatory)] [string] $SourceExe,
  [Parameter(Mandatory)] [string] $OutputPath,
  [Parameter(Mandatory)] [string] $QrateVersion,
  [Parameter(Mandatory)] [string] $IdentityName,
  [Parameter(Mandatory)] [string] $IdentityPublisher,
  [Parameter(Mandatory)] [string] $PublisherDisplayName
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Fail([string] $Message) {
  throw "Store MSIX packaging: $Message"
}

if (-not (Test-Path -LiteralPath $SourceExe -PathType Leaf)) {
  Fail "application executable not found: $SourceExe"
}
foreach ($value in @($IdentityName, $IdentityPublisher, $PublisherDisplayName)) {
  if ([string]::IsNullOrWhiteSpace($value)) { Fail 'all Partner Center identity values are required' }
}

if ($QrateVersion -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$') {
  Fail "expected a stable three-part qrate version, got '$QrateVersion'"
}
$major, $minor, $patch = $QrateVersion.Split('.') | ForEach-Object { [int] $_ }
if ($major -ge 65535 -or $minor -gt 65535 -or $patch -gt 65535) {
  Fail "version '$QrateVersion' cannot be represented by the MSIX version fields"
}
# Store packages require a nonzero first field. Offset the qrate major by one, preserving
# semver ordering and leaving the Store-reserved fourth field at zero.
$storeVersion = "$($major + 1).$minor.$patch.0"

$root = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$sourceIcon = Join-Path $root 'assets\icons\app-icon.png'
$manifestTemplate = Join-Path $PSScriptRoot 'store\Package.appxmanifest'
if (-not (Test-Path -LiteralPath $sourceIcon -PathType Leaf)) { Fail "icon not found: $sourceIcon" }
if (-not (Test-Path -LiteralPath $manifestTemplate -PathType Leaf)) { Fail "manifest template not found: $manifestTemplate" }

$stage = Join-Path ([IO.Path]::GetTempPath()) ("qrate-store-msix-" + [guid]::NewGuid().ToString('N'))
$unpacked = "$stage-unpacked"
$makeAppx = Get-ChildItem -LiteralPath "${env:ProgramFiles(x86)}\Windows Kits\10\bin" -Filter makeappx.exe -File -Recurse |
  Where-Object { $_.Directory.Name -eq 'x64' -and $_.Directory.Parent.Name -match '^\d+(\.\d+){3}$' } |
  Sort-Object { [version] $_.Directory.Parent.Name } -Descending |
  Select-Object -First 1 -ExpandProperty FullName
if (-not $makeAppx) { Fail 'Windows SDK MakeAppx.exe was not found' }

try {
  New-Item -ItemType Directory -Path (Join-Path $stage 'Assets') -Force | Out-Null
  Copy-Item -LiteralPath $SourceExe -Destination (Join-Path $stage 'qrate.exe')
  $marker = [ordered]@{
    schema = 1
    kind = 'windows-store'
    packaged_version = $QrateVersion
    flavor = 'base'
  } | ConvertTo-Json
  [IO.File]::WriteAllText((Join-Path $stage 'qrate-install.json'), $marker + "`n", [Text.UTF8Encoding]::new($false))

  Add-Type -AssemblyName System.Drawing
  $sourceBitmap = [System.Drawing.Bitmap]::new($sourceIcon)
  try {
    foreach ($size in @(50, 150, 44)) {
      $name = switch ($size) {
        50 { 'StoreLogo.png' }
        150 { 'Square150x150Logo.png' }
        44 { 'Square44x44Logo.png' }
      }
      $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
      try {
        $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
        try {
          $graphics.Clear([System.Drawing.Color]::Transparent)
          $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
          $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
          $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
          $graphics.DrawImage($sourceBitmap, 0, 0, $size, $size)
        } finally {
          $graphics.Dispose()
        }
        $bitmap.Save((Join-Path $stage "Assets\$name"), [System.Drawing.Imaging.ImageFormat]::Png)
      } finally {
        $bitmap.Dispose()
      }
    }
  } finally {
    $sourceBitmap.Dispose()
  }

  $manifest = [xml](Get-Content -LiteralPath $manifestTemplate -Raw)
  $ns = [System.Xml.XmlNamespaceManager]::new($manifest.NameTable)
  $ns.AddNamespace('a', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
  $identity = $manifest.SelectSingleNode('/a:Package/a:Identity', $ns)
  $identity.SetAttribute('Name', $IdentityName)
  $identity.SetAttribute('Publisher', $IdentityPublisher)
  $identity.SetAttribute('Version', $storeVersion)
  $manifest.SelectSingleNode('/a:Package/a:Properties/a:PublisherDisplayName', $ns).InnerText = $PublisherDisplayName
  $manifest.Save((Join-Path $stage 'AppxManifest.xml'))

  $resolvedOutput = [IO.Path]::GetFullPath($OutputPath)
  New-Item -ItemType Directory -Path (Split-Path -Parent $resolvedOutput) -Force | Out-Null
  if (Test-Path -LiteralPath $resolvedOutput) { Remove-Item -LiteralPath $resolvedOutput -Force }
  & $makeAppx pack /d $stage /p $resolvedOutput /o
  if ($LASTEXITCODE -ne 0) { Fail "MakeAppx pack failed with exit code $LASTEXITCODE" }
  & $makeAppx validate /p $resolvedOutput
  if ($LASTEXITCODE -ne 0) { Fail "MakeAppx validation failed with exit code $LASTEXITCODE" }

  & $makeAppx unpack /p $resolvedOutput /d $unpacked /o
  if ($LASTEXITCODE -ne 0) { Fail "MakeAppx unpack failed with exit code $LASTEXITCODE" }
  $unpackedMarker = Get-Content -LiteralPath (Join-Path $unpacked 'qrate-install.json') -Raw | ConvertFrom-Json
  if ($unpackedMarker.kind -ne 'windows-store' -or $unpackedMarker.flavor -ne 'base' -or $unpackedMarker.packaged_version -ne $QrateVersion) {
    Fail 'the package install marker does not declare this Store base build'
  }
  foreach ($excluded in @('qrate-update-helper.exe', 'pdfium.dll', 'ffmpeg.exe', 'agent')) {
    if (Test-Path -LiteralPath (Join-Path $unpacked $excluded)) { Fail "base package unexpectedly contains $excluded" }
  }
  Write-Host "Built unsigned x64 MSIX: $resolvedOutput"
  Write-Host "qrate version $QrateVersion -> Store package version $storeVersion"
} finally {
  Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $unpacked -Recurse -Force -ErrorAction SilentlyContinue
}
