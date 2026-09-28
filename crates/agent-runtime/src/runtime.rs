use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use components::{ComponentId, Found};
use gpui::{App, Global};

const PI_VERSION: &str = "0.84.2";

/// The qrate-owned Pi installation. The terminal adds the currently open project at launch.
#[derive(Clone, Debug)]
pub struct AgentRuntime {
    pub program: PathBuf,
    pub leading_args: Vec<String>,
    pub extension: PathBuf,
    pub profile: PathBuf,
    pub endpoint: PathBuf,
}

impl Global for AgentRuntime {}

/// Finds Pi now, and again whenever a component is installed or removed, so installing the
/// assistant needs no restart.
pub fn init(cx: &mut App) {
    components::found_by(ComponentId::Agent, found, cx);
    load(cx);
    let mut seen = components::generation();
    cx.observe_global::<components::Components>(move |cx| {
        if components::generation() != seen {
            seen = components::generation();
            load(cx);
        }
    })
    .detach();
}

fn load(cx: &mut App) {
    match prepare() {
        Ok(runtime) => {
            if cx
                .try_global::<AgentRuntime>()
                .is_some_and(|current| current.program == runtime.program)
            {
                return;
            }
            log::info!(
                "embedded Pi {PI_VERSION} ready at {}",
                runtime.program.display()
            );
            crate::terminal::warm_global_credential(runtime.program.clone());
            cx.set_global(runtime);
        }
        Err(err) => {
            log::warn!("embedded Pi is unavailable: {err}");
            if cx.has_global::<AgentRuntime>() {
                cx.remove_global::<AgentRuntime>();
            }
        }
    }
}

fn prepare() -> Result<AgentRuntime, String> {
    let root = root().ok_or_else(|| {
        "the agent runtime is not installed; install it as an optional component, reinstall the \
         full qrate, or run scripts/fetch-agent-runtime.ps1 in a checkout"
            .to_owned()
    })?;
    let program = root.join(if cfg!(windows) { "pi.exe" } else { "pi" });
    let package = root.join("qrate-pi-extension");
    let extension = package.join("extensions/qrate.ts");
    let extension_bridge = package.join("src/bridge.ts");
    let extension_permissions = package.join("src/permissions.ts");
    let source_system = package.join("SYSTEM.md");
    let dark_theme = root.join("theme/dark.json");
    let light_theme = root.join("theme/light.json");
    for required in [
        &program,
        &extension,
        &extension_bridge,
        &extension_permissions,
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
        profile,
        endpoint,
    })
}

/// Where Pi may be, in order: a full install's `agent` folder, beside the executable or in a
/// macOS bundle's `Contents/Resources`, then the copy qrate installed on demand.
fn candidates(exe_dir: Option<&Path>, installed: Option<PathBuf>) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(exe_dir) = exe_dir {
        candidates.push(exe_dir.join("agent"));
        if let Some(contents) = exe_dir.parent() {
            candidates.push(contents.join("Resources/agent"));
        }
    }
    candidates.extend(installed);
    candidates
}

fn root() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok();
    let installed = components::store().and_then(|store| store.locate(ComponentId::Agent));
    candidates(executable.as_deref().and_then(Path::parent), installed)
        .into_iter()
        .find(|path| runtime_exists(path))
}

/// Whether Pi came with this install. Every place but the installed
/// component is part of a full install.
fn found() -> Option<Found> {
    static FOUND: Mutex<Option<(u64, Option<Found>)>> = Mutex::new(None);
    components::remember(&FOUND, components::generation(), || {
        let installed = components::store().and_then(|store| store.locate(ComponentId::Agent));
        root()
            .filter(|root| Some(root) != installed.as_ref())
            .map(|_| Found::Bundled)
    })
}

fn runtime_exists(root: &Path) -> bool {
    root.join(if cfg!(windows) { "pi.exe" } else { "pi" })
        .is_file()
        && root.join("qrate-pi-extension").is_dir()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::candidates;

    #[test]
    fn a_full_install_wins_over_the_installed_component() {
        let exe = Path::new("/Applications/qrate.app/Contents/MacOS");
        let installed = PathBuf::from("/data/components/agent/0.84.2-ext.0.2.1");
        assert_eq!(
            candidates(Some(exe), Some(installed.clone())),
            [
                exe.join("agent"),
                Path::new("/Applications/qrate.app/Contents/Resources/agent").to_path_buf(),
                installed.clone(),
            ]
        );
        assert_eq!(candidates(None, Some(installed.clone())), [installed]);
    }
}
