# Builds qrate-export from qrate's main into vendor/qrate-export, which
# astro.config.mjs then uses in place of the released package. Needs a qrate
# worktree, wasm-pack, the wasm32-unknown-unknown target and clang (LLVM).
param(
    [string]$Qrate = (Join-Path $PSScriptRoot "..\..\worktrees\qrate-export-main")
)

$ErrorActionPreference = "Stop"
$out = Join-Path (Split-Path $PSScriptRoot -Parent) "vendor\qrate-export"
$env:PATH = "C:\Program Files\LLVM\bin;$env:PATH"

git -C $Qrate fetch origin main
if ($LASTEXITCODE -ne 0) { throw "git fetch failed" }
git -C $Qrate checkout --detach origin/main
if ($LASTEXITCODE -ne 0) { throw "git checkout failed" }
wasm-pack build (Join-Path $Qrate "crates\qrate-export") --release --target web --out-dir $out -- --features wasm
if ($LASTEXITCODE -ne 0) { throw "wasm-pack failed" }
Write-Host "qrate-export $(git -C $Qrate rev-parse --short HEAD) -> $out"
