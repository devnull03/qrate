#!/usr/bin/env bash
# The checks CI runs, in CI's order, in their own target dirs.
#
# The dir matters: clippy's `-D warnings` and rust-analyzer's plain clippy are different
# fingerprints, so sharing `target/` would make every run rebuild what the editor just built.
set -euo pipefail
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/ci}"
cargo fmt --all --check
# Clippy runs without zccache's path remapping. Cargo runs a linted crate as
# `<wrapper> <clippy-driver> <rustc> <args>`, and clippy-driver copes with that by dropping the
# rustc path — but not once the remap has rewritten it, so the crate sees that path as a second
# input file and stops with "multiple input filenames provided". `[env]` in `.cargo/config.toml`
# does not force, so an empty value here wins while zccache stays the wrapper and keeps caching.
# The cost is that this step's artifacts are not shared between worktrees; `cargo test` below
# keeps the remap, and so does every ordinary build.
ZCCACHE_PATH_REMAP= cargo clippy --workspace --all-targets -- -D warnings -A dead_code
cargo test --workspace
# The browser build needs nightly (see docs/dev/web-support-plan.md); CI's `wasm` job runs it.
if rustup toolchain list | grep -q '^nightly'; then
  CARGO_TARGET_DIR="${CARGO_TARGET_DIR}-wasm" cargo +nightly wasm-check -p app -p web
else
  echo "skipping the wasm32 check: install it with rustup toolchain install nightly -c rust-src -t wasm32-unknown-unknown" >&2
fi
