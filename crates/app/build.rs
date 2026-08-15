//! Stamps the build's commit into the binary so a pasted bug report names an exact commit.
//!
//! Nothing else carries it: the release workflow's SHA256SUMS are artifact checksums, and the tag
//! only reaches the binary as `CARGO_PKG_VERSION`, which every build between two tags shares.

use std::{env, fs, path::Path, process::Command};

/// Lists whatever `assets/themes/*.json` exists, so dropping a theme file in is the whole job —
/// see `crates/app/src/theming.rs`.
fn generate_theme_list() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("assets/themes is missing")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".json"))
        .collect();
    files.sort();

    let entries: String = files
        .iter()
        .map(|name| format!("    ({name:?}, include_str!({:?})),\n", dir.join(name)))
        .collect();

    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("themes.rs"),
        format!("const EMBEDDED_THEMES: &[(&str, &str)] = &[\n{entries}];\n"),
    )
    .unwrap();
}

fn main() {
    generate_theme_list();

    println!("cargo:rerun-if-changed=../../assets/icons/app-icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon("../../assets/icons/app-icon.ico");
        resource
            .compile()
            .expect("failed to embed the Windows app icon");
    }

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
