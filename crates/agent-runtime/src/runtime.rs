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
    /// The `qrate-cli` shipped beside this app, which is how Pi reads the live project.
    pub cli: PathBuf,
    /// Pi's own `light.json` and `dark.json`, which qrate's theme for Pi starts from.
    pub themes: PathBuf,
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
    let extension_cli = package.join("src/cli.ts");
    let extension_permissions = package.join("src/permissions.ts");
    let source_system = package.join("SYSTEM.md");
    let dark_theme = root.join("theme/dark.json");
    let light_theme = root.join("theme/light.json");
    for required in [
        &program,
        &extension,
        &extension_cli,
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
    seed_settings(&profile.join("settings.json"))
        .map_err(|err| format!("could not seed Pi's settings: {err}"))?;

    let cli = std::env::current_exe()
        .ok()
        .and_then(|exe| {
            Some(
                exe.parent()?
                    .join(format!("qrate-cli{}", std::env::consts::EXE_SUFFIX)),
            )
        })
        .ok_or_else(|| "qrate cannot locate its own executable".to_owned())?;
    if !cli.is_file() {
        return Err(format!(
            "{} is missing, so the assistant could not read the project; reinstall qrate, or in \
             a checkout run `cargo build -p qrate-cli`",
            cli.display()
        ));
    }
    Ok(AgentRuntime {
        program,
        leading_args: Vec::new(),
        extension,
        profile,
        cli,
        themes: root.join("theme"),
    })
}

/// The name of the theme qrate writes for Pi, in the profile's `themes` folder.
const QRATE_THEME: &str = "qrate";

/// Start Pi on OpenRouter's free router in qrate's theme, and give an existing profile qrate's
/// theme only when it has not chosen one, so a theme picked in Pi stays picked.
fn seed_settings(path: &Path) -> Result<(), String> {
    let mut settings = match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
        {
            Ok(settings) => settings,
            Err(err) => {
                log::warn!(
                    "left Pi's settings at {} as they are, since they are not a JSON object: {err}",
                    path.display()
                );
                return Ok(());
            }
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => serde_json::Map::from_iter([
            ("defaultProvider".to_owned(), "openrouter".into()),
            ("defaultModel".to_owned(), "openrouter/free".into()),
        ]),
        Err(err) => return Err(err.to_string()),
    };
    if settings.contains_key("theme") && path.exists() {
        return Ok(());
    }
    settings.insert("theme".to_owned(), QRATE_THEME.into());
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    fs::write(path, text + "\n").map_err(|err| err.to_string())
}

/// Write Pi's `qrate` theme in qrate's colours. It starts from Pi's own light or dark theme, so
/// every colour Pi needs is set and code highlighting stays Pi's; Pi reloads the file while running.
pub(crate) fn write_pi_theme(
    runtime: &AgentRuntime,
    palette: crate::TerminalPalette,
) -> Result<(), String> {
    use crate::terminal::{hex, is_light};
    let base = if is_light(palette.background) {
        "light"
    } else {
        "dark"
    };
    let base_path = runtime.themes.join(format!("{base}.json"));
    let mut theme: serde_json::Value = fs::read_to_string(&base_path)
        .map_err(|err| format!("{}: {err}", base_path.display()))
        .and_then(|text| serde_json::from_str(&text).map_err(|err| err.to_string()))?;
    let tint =
        |color: gpui::Hsla, amount: f32| hex(palette.background.blend(color.opacity(amount)));
    let fg = hex(palette.foreground);
    let muted = hex(palette.muted);
    let faint = tint(palette.foreground, 0.25);
    let colors = [
        ("accent", hex(palette.magenta)),
        ("border", hex(palette.blue)),
        ("borderAccent", hex(palette.magenta)),
        ("borderMuted", faint.clone()),
        ("success", hex(palette.green)),
        ("error", hex(palette.red)),
        ("warning", hex(palette.yellow)),
        ("muted", muted.clone()),
        ("dim", muted.clone()),
        ("text", fg.clone()),
        ("thinkingText", muted.clone()),
        ("selectedBg", tint(palette.blue, 0.18)),
        ("scrollbarThumb", faint.clone()),
        ("searchMatchBg", tint(palette.yellow, 0.3)),
        ("searchMatchText", fg.clone()),
        ("userMessageBg", tint(palette.foreground, 0.07)),
        ("userMessageText", fg.clone()),
        ("customMessageBg", tint(palette.magenta, 0.1)),
        ("customMessageText", fg.clone()),
        ("customMessageLabel", hex(palette.magenta)),
        ("toolPendingBg", tint(palette.blue, 0.08)),
        ("toolSuccessBg", tint(palette.green, 0.08)),
        ("toolErrorBg", tint(palette.red, 0.08)),
        ("toolTitle", fg.clone()),
        ("toolOutput", muted.clone()),
        ("mdHeading", hex(palette.yellow)),
        ("mdLink", hex(palette.blue)),
        ("mdLinkUrl", muted.clone()),
        ("mdCode", hex(palette.cyan)),
        ("mdQuote", muted.clone()),
        ("mdQuoteBorder", faint.clone()),
        ("mdHr", faint),
        ("mdListBullet", hex(palette.green)),
        ("toolDiffAdded", hex(palette.green)),
        ("toolDiffRemoved", hex(palette.red)),
        ("toolDiffContext", muted),
    ];
    let map = theme
        .get_mut("colors")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| format!("{} has no colors", base_path.display()))?;
    for (key, value) in colors {
        map.insert(key.to_owned(), value.into());
    }
    theme["name"] = QRATE_THEME.into();
    let dir = runtime.profile.join("themes");
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let target = dir.join(format!("{QRATE_THEME}.json"));
    let partial = dir.join(format!(".{QRATE_THEME}.json.partial"));
    let text = serde_json::to_string_pretty(&theme).map_err(|err| err.to_string())?;
    fs::write(&partial, text)
        .and_then(|()| fs::rename(&partial, &target))
        .map_err(|err| format!("{}: {err}", target.display()))
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

    use gpui::hsla;

    use super::{AgentRuntime, candidates, seed_settings, write_pi_theme};
    use crate::TerminalPalette;

    fn scratch(case: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("qrate-agent-runtime-test")
            .join(case);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A new profile starts in qrate's theme; one where Pi was told another theme keeps it.
    #[test]
    fn pi_is_seeded_with_qrates_theme_but_a_chosen_theme_stays() {
        let dir = scratch("settings");
        let path = dir.join("settings.json");
        seed_settings(&path).unwrap();
        let seeded: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(seeded["theme"], "qrate");
        assert_eq!(seeded["defaultProvider"], "openrouter");

        std::fs::write(&path, r#"{"theme":"dark","defaultModel":"x"}"#).unwrap();
        seed_settings(&path).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"theme":"dark","defaultModel":"x"}"#
        );

        std::fs::write(&path, r#"{"defaultModel":"x"}"#).unwrap();
        seed_settings(&path).unwrap();
        let added: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(added["theme"], "qrate");
        assert_eq!(added["defaultModel"], "x");
    }

    /// The theme takes qrate's colours for Pi's interface and keeps Pi's own code colours.
    #[test]
    fn pis_qrate_theme_uses_qrates_colours() {
        let dir = scratch("theme");
        let themes = dir.join("theme");
        std::fs::create_dir_all(&themes).unwrap();
        std::fs::write(
            themes.join("light.json"),
            r##"{"name":"light","vars":{"text":"#111111"},"colors":{"text":"text","syntaxComment":"#008000"}}"##,
        )
        .unwrap();
        let runtime = AgentRuntime {
            program: PathBuf::new(),
            leading_args: Vec::new(),
            extension: PathBuf::new(),
            profile: dir.join("profile"),
            cli: PathBuf::new(),
            themes,
        };
        let palette = TerminalPalette {
            foreground: hsla(0., 0., 0.2, 1.),
            background: hsla(0., 0., 1., 1.),
            muted: hsla(0., 0., 0.5, 1.),
            red: hsla(0., 1., 0.5, 1.),
            green: hsla(0.33, 1., 0.25, 1.),
            yellow: hsla(0.15, 1., 0.5, 1.),
            blue: hsla(0.6, 1., 0.5, 1.),
            magenta: hsla(0.8, 1., 0.5, 1.),
            cyan: hsla(0.5, 1., 0.5, 1.),
        };
        write_pi_theme(&runtime, palette).unwrap();
        let theme: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(runtime.profile.join("themes/qrate.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(theme["name"], "qrate");
        assert_eq!(theme["colors"]["text"], "#333333");
        assert_eq!(theme["colors"]["error"], "#ff0000");
        assert_eq!(theme["colors"]["syntaxComment"], "#008000");
        assert!(
            theme["colors"]["toolSuccessBg"]
                .as_str()
                .unwrap()
                .starts_with('#')
        );
    }

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
