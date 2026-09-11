$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$safe = Join-Path $env:LOCALAPPDATA "qrate-site-dev-$PID"

Push-Location $root
try {
    if (Get-NetTCPConnection -LocalPort 4321 -State Listen -ErrorAction SilentlyContinue) {
        throw "Port 4321 is already in use. Stop the previous site server before starting this one."
    }
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

    $version = bunx wrangler kv key get current --binding PLUGIN_CATALOG --preview false --remote --text
    if ($LASTEXITCODE -ne 0 -or $version -notmatch "^[a-f0-9]{64}$") {
        throw "Could not read the current signed catalog version from Cloudflare KV."
    }
    $version = $version.Trim()
    $catalogFile = Join-Path $safe "catalog.json"
    $seed = [ordered]@{
        "catalog:${version}:json"      = $catalogFile
        "catalog:${version}:signature" = "$catalogFile.sig"
        "catalog:${version}:status"    = Join-Path $safe "catalog-status.json"
        "schema:catalog"               = Join-Path $safe "catalog.schema.json"
        "schema:listing"               = Join-Path $safe "listing.schema.json"
        "schema:package"               = Join-Path $safe "package.schema.json"
    }
    # astro dev reads the preview namespace, so local seeding targets preview_id.
    foreach ($key in $seed.Keys) {
        $file = $seed[$key]
        cmd.exe /d /c "bunx wrangler kv key get `"$key`" --binding PLUGIN_CATALOG --preview false --remote > `"$file`""
        if ($LASTEXITCODE -ne 0) {
            throw "Could not read $key from Cloudflare KV."
        }
        bunx wrangler kv key put $key --path $file --binding PLUGIN_CATALOG --preview --local
        if ($LASTEXITCODE -ne 0) {
            throw "Could not seed local Cloudflare KV with $key."
        }
    }

    bunx wrangler kv key put current $version --binding PLUGIN_CATALOG --preview --local
    if ($LASTEXITCODE -ne 0) {
        throw "Could not seed local Cloudflare KV with the current catalog version."
    }

    $env:QRATE_PLUGIN_CATALOG_FILE = $catalogFile
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
