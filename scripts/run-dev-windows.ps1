$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$safe = Join-Path $env:LOCALAPPDATA "qrate-site-dev-$PID"

Push-Location $root
try {
    $changes = git status --porcelain
    if ($LASTEXITCODE -ne 0 -or $changes) {
        throw "Commit or stash site changes before starting the Windows dev server."
    }

    git worktree add --detach $safe HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "Could not create the safe-path site worktree."
    }
    foreach ($name in @(".env", ".env.production", ".dev.vars")) {
        $source = Join-Path $root $name
        if (Test-Path $source -PathType Leaf) {
            Copy-Item $source (Join-Path $safe $name)
        }
    }

    Set-Location $safe
    bun install --frozen-lockfile
    if ($LASTEXITCODE -ne 0) {
        throw "Site dependency installation failed."
    }
    bun run dev
    if ($LASTEXITCODE -ne 0) {
        throw "Site development server exited with an error."
    }
}
finally {
    Set-Location $root
    if (Test-Path $safe) {
        git worktree remove --force $safe
    }
    Pop-Location
}
