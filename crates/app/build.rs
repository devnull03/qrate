//! Stamps the build's commit into the binary so a pasted bug report names an exact commit.
//!
//! Nothing else carries it: the release workflow's SHA256SUMS are artifact checksums, and the tag
//! only reaches the binary as `CARGO_PKG_VERSION`, which every build between two tags shares.

use std::process::Command;

fn main() {
    // Authoritative on a runner, and does not need `git` on PATH.
    let sha = std::env::var("GITHUB_SHA")
        .map(|sha| sha[..sha.len().min(7)].to_string())
        .ok()
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short", "HEAD"])
                .output()
                .ok()
                .filter(|out| out.status.success())
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|sha| sha.trim().to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=QRATE_GIT_SHA={sha}");
    // HEAD alone misses a commit made on the branch you are already on, which is every commit.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
}
