//! Theme switching: a curated subset of gpui-component's built-in themes, embedded in the
//! binary (qrate ships as a bundled installer, so a loose `themes/` folder next to the exe
//! isn't reliable), extracted once to the OS data dir, and loaded through
//! `ThemeRegistry::watch_dir` — the only public way to register non-default themes.
//! Selection persists via the existing `AppSettings` text-value store.

use std::{fs, path::PathBuf};

use gpui::*;
use gpui_component::{Theme, ThemeRegistry};
use schemars::JsonSchema;
use serde::Deserialize;
use settings::AppSettings;

const THEME_NAME_KEY: &str = "theme_name";

const EMBEDDED_THEMES: &[(&str, &str)] = &[
    ("ayu.json", include_str!("../../../assets/themes/ayu.json")),
    (
        "catppuccin.json",
        include_str!("../../../assets/themes/catppuccin.json"),
    ),
    (
        "everforest.json",
        include_str!("../../../assets/themes/everforest.json"),
    ),
    (
        "gruvbox.json",
        include_str!("../../../assets/themes/gruvbox.json"),
    ),
    (
        "solarized.json",
        include_str!("../../../assets/themes/solarized.json"),
    ),
    (
        "tokyonight.json",
        include_str!("../../../assets/themes/tokyonight.json"),
    ),
];

/// Curated theme names available from the "Theme" menu, in menu order. Must match the `name`
/// field of a `ThemeConfig` inside one of the `EMBEDDED_THEMES` files (or gpui-component's own
/// built-in "Default Light"/"Default Dark").
pub const THEME_CHOICES: &[&str] = &[
    "Default Light",
    "Default Dark",
    "Ayu Light",
    "Ayu Dark",
    "Catppuccin Latte",
    "Catppuccin Frappe",
    "Catppuccin Macchiato",
    "Catppuccin Mocha",
    "Everforest Light",
    "Everforest Dark",
    "Gruvbox Light",
    "Gruvbox Dark",
    "Solarized Light",
    "Solarized Dark",
    "Tokyo Night",
    "Tokyo Storm",
    "Tokyo Moon",
];

#[derive(Clone, PartialEq, Deserialize, JsonSchema, Action)]
#[action(namespace = this_app)]
#[serde(deny_unknown_fields)]
pub struct SwitchTheme {
    pub name: String,
}

fn themes_dir() -> Option<PathBuf> {
    Some(dirs::data_local_dir()?.join("qrate").join("themes"))
}

fn extract_embedded_themes(dir: &PathBuf) -> anyhow::Result<()> {
    fs::create_dir_all(dir)?;
    for (file_name, contents) in EMBEDDED_THEMES {
        fs::write(dir.join(file_name), contents)?;
    }
    Ok(())
}

/// Applies the theme saved in `AppSettings`, if any. Called once the theme registry has
/// finished loading, and again live whenever the user picks a new theme from the menu.
fn apply_saved_theme(cx: &mut App) {
    let Some(name) = AppSettings::get(cx)
        .values
        .get(THEME_NAME_KEY)
        .map(|v| v.text())
    else {
        return;
    };
    apply_theme_by_name(&name, cx);
}

fn apply_theme_by_name(name: &str, cx: &mut App) {
    if let Some(config) = ThemeRegistry::global(cx).themes().get(name).cloned() {
        Theme::global_mut(cx).apply_config(&config);
        cx.refresh_windows();
    }
}

/// Extracts the embedded themes to disk and registers the theme switcher. Call once at
/// startup, after `gpui_component::init(cx)` and after `AppSettings` has been loaded.
pub fn init(cx: &mut App) {
    if let Some(dir) = themes_dir() {
        match extract_embedded_themes(&dir) {
            Ok(()) => {
                if let Err(err) = ThemeRegistry::watch_dir(dir, cx, apply_saved_theme) {
                    eprintln!("Failed to watch themes directory: {err}");
                }
            }
            Err(err) => eprintln!("Failed to extract embedded themes: {err}"),
        }
    }

    cx.on_action(|action: &SwitchTheme, cx| {
        apply_theme_by_name(&action.name, cx);
        AppSettings::set_text(THEME_NAME_KEY, action.name.clone().into(), cx);
    });
}
