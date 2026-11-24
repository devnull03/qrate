param(
  [Parameter(Mandatory)] [string] $OutputPath
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
if (-not (Test-Path -LiteralPath $vswhere)) { throw 'Visual Studio vswhere.exe was not found.' }
$installation = & $vswhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
if (-not $installation) { throw 'The Visual Studio x64 C++ build tools are required for qrate-shell.dll.' }
$devcmd = Join-Path $installation 'Common7\Tools\VsDevCmd.bat'
# Run from a batch file: a quoted path inside a quoted cmd.exe command line does not survive
# PowerShell's native argument passing.
$probe = Join-Path ([IO.Path]::GetTempPath()) ('qrate-vsdevcmd-' + [guid]::NewGuid().ToString('N') + '.cmd')
Set-Content -LiteralPath $probe -Encoding ascii -Value @(
  "@call `"$devcmd`" -no_logo -arch=x64 -host_arch=x64 >nul || exit /b 1"
  '@set'
)
try {
  $environment = & $env:ComSpec /d /c $probe
  if ($LASTEXITCODE -ne 0) { throw 'Could not initialize the x64 Visual Studio environment.' }
} finally {
  Remove-Item -LiteralPath $probe -Force -ErrorAction SilentlyContinue
}
foreach ($line in $environment) {
  if ($line -match '^([^=]+)=(.*)$') {
    [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process')
  }
}

$output = [IO.Path]::GetFullPath($OutputPath)
New-Item -ItemType Directory -Path (Split-Path -Parent $output) -Force | Out-Null
$temporary = Join-Path ([IO.Path]::GetTempPath()) ('qrate-shell-build-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
  $source = Join-Path $PSScriptRoot 'store\qrate-shell.cpp'
  $exports = Join-Path $PSScriptRoot 'store\qrate-shell.def'
  & cl.exe /nologo /std:c++17 /EHsc /W4 /WX /O2 /MT /LD /DUNICODE /D_UNICODE /D_WIN32_WINNT=0x0A00 `
    $source "/Fo$temporary\qrate-shell.obj" "/Fe$output" `
    /link "/DEF:$exports" "/IMPLIB:$temporary\qrate-shell.lib" /INCREMENTAL:NO ole32.lib shell32.lib shlwapi.lib uuid.lib
  if ($LASTEXITCODE -ne 0) { throw "qrate-shell.dll compilation failed ($LASTEXITCODE)." }
  Write-Host "Built x64 Explorer command: $output"
} finally {
  Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
}
