#!/usr/bin/env bash
# The three checks CI runs, in CI's order, in their own target dir.
#
# The dir matters: clippy's `-D warnings` and rust-analyzer's plain clippy are different
# fingerprints, so sharing `target/` would make every run rebuild what the editor just built.
set -euo pipefail
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-target/ci}"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings -A dead_code
cargo test --workspace
