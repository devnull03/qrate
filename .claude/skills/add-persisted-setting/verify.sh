#!/usr/bin/env bash
# Verification harness for persistence changes in the `settings` crate.
#
# Persistence in qrate has no GUI-testable surface worth booting for a settings
# change — the layer that actually matters is the round-trip through SQLite
# (create project file -> write __settings row -> reload -> read it back, and
# the AppSettings JSON blob). Those round-trips live in `crates/settings` unit
# tests. This script runs them and fails loudly if any break.
#
# Run from the repo root after adding or changing a persisted setting:
#   bash .claude/skills/add-persisted-setting/verify.sh
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

echo "== fmt =="
cargo fmt --all --check

echo "== clippy (settings) =="
cargo clippy -p settings --all-targets -- -D warnings -A dead_code

echo "== settings round-trip tests =="
cargo test -p settings

echo "ALL GREEN"
