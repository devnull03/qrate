use std::fs;
use std::path::{Path, PathBuf};

use gpui::{App, Global};

const PI_VERSION: &str = "0.84.2";

/// The qrate-owned Pi installation. The terminal adds the currently open project at launch.
#[derive(Clone, Debug)]
pub struct AgentRuntime {
    pub program: PathBuf,
    pub leading_args: Vec<String>,
    pub extension: PathBuf,
    pub skill: PathBuf,
    pub profile: PathBuf,
    pub endpoint: PathBuf,
}

impl Global for AgentRuntime {}

pub fn init(cx: &mut App) {
    match prepare() {
        Ok(runtime) => {
            log::info!(
                "embedded Pi {PI_VERSION} ready at {}",
                runtime.program.display()
            );
            cx.set_global(runtime);
        }
        Err(err) => log::warn!("embedded Pi is unavailable: {err}"),
    }
}

fn prepare() -> Result<AgentRuntime, String> {
    let root = bundled_root().ok_or_else(|| {
        "the agent runtime is missing; reinstall qrate or run scripts/fetch-agent-runtime.ps1"
            .to_owned()
    })?;
    let program = root.join(if cfg!(windows) { "pi.exe" } else { "pi" });
    let package = root.join("qrate-pi-extension");
    let extension = package.join("extensions/qrate.ts");
    let extension_bridge = package.join("src/bridge.ts");
    let extension_permissions = package.join("src/permissions.ts");
    let skill = package.join("skills/qrate-live-review/SKILL.md");
    let source_system = package.join("SYSTEM.md");
    let dark_theme = root.join("theme/dark.json");
    let light_theme = root.join("theme/light.json");
    for required in [
        &program,
        &extension,
        &extension_bridge,
        &extension_permissions,
        &skill,
        &source_system,
        &dark_theme,
        &light_theme,
    ] {
        if !required.is_file() {
            return Err(format!("{} is missing", required.display()));
        }
    }

    let profile = settings::data_dir()
        .ok_or_else(|| "qrate has no writable application-data directory".to_owned())?
        .join("pi-agent");
    fs::create_dir_all(&profile)
        .map_err(|err| format!("could not create {}: {err}", profile.display()))?;
    // qrate owns the assistant policy, while Pi continues to own auth.json and session data.
    fs::copy(&source_system, profile.join("SYSTEM.md"))
        .map_err(|err| format!("could not seed Pi's system prompt: {err}"))?;
    let settings = profile.join("settings.json");
    if !settings.exists() {
        fs::write(
            &settings,
            "{\n  \"defaultProvider\": \"openrouter\",\n  \"defaultModel\": \"openrouter/free\"\n}\n",
        )
        .map_err(|err| format!("could not seed Pi's settings: {err}"))?;
    }

    let endpoint = settings::data_dir()
        .expect("data directory was available above")
        .join("agent-bridge.json");
    Ok(AgentRuntime {
        program,
        leading_args: Vec::new(),
        extension,
        skill,
        profile,
        endpoint,
    })
}

fn bundled_root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?;
    let executable_dir = executable.parent()?;
    let mut candidates = vec![executable_dir.join("agent")];
    // A macOS .app keeps auxiliary executables and data in Contents/Resources.
    if let Some(contents) = executable_dir.parent() {
        candidates.push(contents.join("Resources/agent"));
    }
    candidates.into_iter().find(|path| runtime_exists(path))
}

fn runtime_exists(root: &Path) -> bool {
    root.join(if cfg!(windows) { "pi.exe" } else { "pi" })
        .is_file()
        && root.join("qrate-pi-extension").is_dir()
}
