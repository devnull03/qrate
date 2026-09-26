mod config;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use diagnostics::CheckList;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Axis, Entity, Global, IntoElement, ParentElement as _,
    PromptLevel, SharedString, Styled as _, Subscription, Task, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, IndexPath, Sizable as _,
    button::Button,
    combobox::{Combobox, ComboboxEvent, ComboboxState},
    h_flex,
    input::{Input, InputEvent, InputState},
    searchable_list::SearchableListItem,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    switch::Switch,
    tab::{Tab, TabBar},
    v_flex,
};
use plugin_api::{ColumnMapContributions, ColumnMapSpec, CommandContext, PluginHooks};
use plugin_host::{SettingKind, SettingSpec};
use serde_json::Value as Json;
use settings::{Setting, columns, project::CurrentProject};

/// Where the Columns page sits in [`build_pages`], for the Data menu's "Column Settings…". An index
/// rather than a title because `SettingPage` does not hand its title back.
pub const COLUMNS_PAGE: usize = 2;
pub const PLUGINS_PAGE: usize = 7;

struct AuthorityRevalidation(#[allow(dead_code)] Task<()>);

impl Global for AuthorityRevalidation {}

fn revalidate_authority_setting(cx: &mut App) {
    let task = cx.spawn(async move |cx| {
        cx.background_executor()
            .timer(Duration::from_millis(500))
            .await;
        cx.update(table::revalidate_now);
    });
    cx.set_global(AuthorityRevalidation(task));
}

pub fn build_pages(cx: &App) -> Vec<SettingPage> {
    let mut pages = vec![
        SettingPage::new("Application")
            .description("Settings for this user on this computer, across all projects.")
            .group(divided_group(cx).title("Identity").item(
                SettingItem::new(
                    "Author name",
                    SettingField::input(
                        |cx: &App| {
                            settings::AppSettings::get(cx)
                                .values
                                .get(settings::NOTE_AUTHOR_KEY)
                                .map(|value| value.text())
                                .unwrap_or_default()
                        },
                        |name: SharedString, cx: &mut App| {
                            settings::AppSettings::set_text(settings::NOTE_AUTHOR_KEY, name, cx)
                        },
                    ),
                )
                .description(
                    "Shown in the project window and recorded on new notes and history entries. \
                     Stored for this user on this computer; leave blank to omit attribution.",
                )
                .layout(Axis::Vertical),
            ))
            .group(
                divided_group(cx)
                    .title("Appearance")
                    .item(SettingItem::new(
                        "Theme",
                        SettingField::scrollable_dropdown(
                            crate::theming::theme_choices(cx)
                                .into_iter()
                                .map(|name| (name.clone(), name))
                                .collect(),
                            |cx: &App| cx.theme().theme_name().clone(),
                            |name: SharedString, cx: &mut App| {
                                crate::theming::switch_theme(&name, cx);
                            },
                        ),
                    ))
                    .item(
                        SettingItem::new(
                            "Interface size",
                            SettingField::dropdown(
                                options(crate::theming::UI_SCALES),
                                |cx: &App| {
                                    settings::AppSettings::get(cx)
                                        .values
                                        .get(crate::theming::UI_SCALE_KEY)
                                        .map(|value| value.text())
                                        .unwrap_or_default()
                                },
                                |scale: SharedString, cx: &mut App| {
                                    settings::AppSettings::set_text(
                                        crate::theming::UI_SCALE_KEY,
                                        scale,
                                        cx,
                                    );
                                    crate::theming::apply_ui_scale(cx);
                                },
                            ),
                        )
                        .description("Scales the text and controls of every qrate window."),
                    ),
            )
            .group(
                divided_group(cx).title("New projects").item(
                    Setting::DirPicker {
                        key: project_wizard::NEW_PROJECT_FOLDER_KEY,
                        label: "Save new projects in",
                        description: "Where the New Project wizard suggests saving. qrate \
                                      remembers the last folder you created a project in; \
                                      leave empty for Documents\\qrate.",
                        prompt: "Choose folder",
                    }
                    .into_item(cx),
                ),
            )
            .group(
                divided_group(cx)
                    .title("Updates and downloads")
                    .item(
                        SettingItem::new(
                            "Automatic updates",
                            SettingField::switch(
                                |cx: &App| crate::update_check::automatic_updates(cx),
                                |on: bool, cx: &mut App| {
                                    settings::AppSettings::set_bool(
                                        updater::AUTO_UPDATE_KEY,
                                        on,
                                        cx,
                                    );
                                },
                            ),
                        )
                        .description(
                            "Check for and download signed qrate updates in the background. \
                             Installing always waits for you to choose Restart to update.",
                        ),
                    )
                    .item(
                        SettingItem::new(
                            "Download source",
                            SettingField::input(
                                |cx: &App| {
                                    settings::AppSettings::get(cx)
                                        .values
                                        .get(updater::DOWNLOAD_SOURCE_KEY)
                                        .map(|value| value.text())
                                        .unwrap_or_default()
                                },
                                |source: SharedString, cx: &mut App| {
                                    settings::AppSettings::set_text(
                                        updater::DOWNLOAD_SOURCE_KEY,
                                        source,
                                        cx,
                                    );
                                },
                            ),
                        )
                        .description(
                            "A mirror or a folder to download updates and the plugin catalog \
                             from instead of GitHub and the qrate website, such as \
                             https://mirror.example.org/qrate or file:///D:/qrate-mirror. \
                             Everything is still checked against qrate's signatures, and \
                             anything the mirror does not have comes from the usual place. \
                             Leave blank to use the defaults.",
                        )
                        .layout(Axis::Vertical),
                    ),
            ),
        SettingPage::new("Table")
            .group(
                divided_group(cx)
                    .title("Appearance")
                    .item(
                        Setting::Switch {
                            key: table::TABLE_STRIPES_KEY,
                            label: "Row Stripes",
                            description: "Alternate row background color in the data table.",
                        }
                        .into_item(cx),
                    )
                    .item(
                        Setting::Dropdown {
                            key: table::ROW_DENSITY_KEY,
                            label: "Row density",
                            description: "Compact rows use smaller text and fit more of the \
                                          collection on screen.",
                            options: table::ROW_DENSITIES,
                        }
                        .into_item(cx),
                    )
                    .item(
                        Setting::Dropdown {
                            key: table::ROW_LINES_KEY,
                            label: "Row height",
                            description: "How many lines of text every row holds. Columns set \
                                          to Wrap fill the extra lines. Drag the bottom edge of \
                                          a row number to change it from the grid.",
                            options: table::ROW_LINES,
                        }
                        .into_item(cx),
                    ),
            )
            .group(
                divided_group(cx).title("Editing").item(
                    Setting::Dropdown {
                        key: table::UNDO_STEPS_KEY,
                        label: "Undo steps",
                        description: "How many edits Undo can step back through. Each step keeps \
                                      what it replaced, so a deeper history holds more memory.",
                        options: table::UNDO_STEPS,
                    }
                    .into_item(cx),
                ),
            )
            .group(
                divided_group(cx).title("Checks").item(
                    Setting::DropdownWithAction {
                        key: checks::dates::DATE_FORMAT_KEY,
                        label: "Date format",
                        description: "What a Date column accepts. EDTF allows uncertain and \
                                      approximate dates such as 1987? and 1987~. ISO 8601 only \
                                      allows YYYY, YYYY-MM and YYYY-MM-DD. Lenient also allows \
                                      circa 1920, ca. 1920, 1920s and [1920?].",
                        options: checks::dates::DATE_FORMATS,
                        on_change: revalidate_dates,
                    }
                    .into_item(cx),
                ),
            )
            .group(
                divided_group(cx)
                    .title("CSV export")
                    .item(
                        SettingItem::new(
                            "Byte order mark",
                            SettingField::switch(
                                |cx: &App| {
                                    settings::effective_text(crate::export::CSV_BOM_KEY, cx)
                                        != "false"
                                },
                                |on: bool, cx: &mut App| {
                                    settings::set_user_text(
                                        crate::export::CSV_BOM_KEY,
                                        if on { "true" } else { "false" }.into(),
                                        cx,
                                    );
                                },
                            ),
                        )
                        .description(
                            "Start the file with a UTF-8 marker. Excel on Windows needs it to \
                             show accented letters correctly; some older import scripts do not \
                             expect it.",
                        ),
                    )
                    .item(
                        Setting::Dropdown {
                            key: crate::export::CSV_DELIMITER_KEY,
                            label: "Separator",
                            description: "Excel in regions that write decimals with a comma \
                                          expects semicolons.",
                            options: crate::export::CSV_DELIMITERS,
                        }
                        .into_item(cx),
                    ),
            )
            .group(saving_group(cx))
            .group(history_group(cx))
            .group(previews_group(cx)),
        columns_page(cx),
        project_page(cx),
        SettingPage::new("Agent")
            .description(
                "An AI agent you run yourself can read the project open in qrate over a local \
                 connection. It cannot change a cell — it can only stage findings you accept.",
            )
            .group(
                divided_group(cx).title("Local bridge").item(
                    Setting::Switch {
                        key: crate::agent_bridge::AGENT_BRIDGE_KEY,
                        label: "Allow agents to read this app",
                        description: "Listens on your own machine only, behind a token that \
                                      changes every launch. Switch off to close the port now.",
                    }
                    .into_item(cx),
                ),
            )
            .group(
                divided_group(cx)
                    .title("Terminal")
                    .item(
                        Setting::Text {
                            key: workspace::AGENT_FONT_KEY,
                            label: "Font",
                            description: "A monospace family installed on this machine, such as \
                                          Cascadia Mono or Consolas. Leave empty to let qrate \
                                          pick one. A proportional font will not line up.",
                        }
                        .into_item(cx),
                    )
                    .item(
                        Setting::Dropdown {
                            key: workspace::AGENT_FONT_SIZE_KEY,
                            label: "Font size",
                            description: "Larger type gives the agent fewer columns to draw in.",
                            options: workspace::AGENT_FONT_SIZES,
                        }
                        .into_item(cx),
                    ),
            ),
        SettingPage::new("Spelling").group(spelling_group(cx)),
        google_page(cx),
        plugins_page(cx),
    ];
    pages.extend(plugin_pages(cx));
    pages
}

fn options(pairs: &[(&'static str, &'static str)]) -> Vec<(SharedString, SharedString)> {
    pairs
        .iter()
        .map(|(value, label)| ((*value).into(), (*label).into()))
        .collect()
}

fn revalidate_dates(cx: &mut App) {
    checks::dates::sync_mode(cx);
    table::revalidate_now(cx);
}

/// `AppSettings` key for the thumbnail cache's ceiling, in MB. A machine's disk is not project
/// data, so this has no project scope.
const PREVIEW_CACHE_KEY: &str = "preview_cache_size";

const PREVIEW_CACHE_SIZES: &[(&str, &str)] = &[
    ("512", "512 MB"),
    ("1024", "1 GB"),
    ("", "2 GB (default)"),
    ("5120", "5 GB"),
];

pub fn preview_cache_bytes(cx: &App) -> u64 {
    let megabytes = settings::AppSettings::get(cx)
        .values
        .get(PREVIEW_CACHE_KEY)
        .and_then(|value| value.text().parse::<u64>().ok())
        .filter(|megabytes| *megabytes > 0)
        .unwrap_or(2048);
    megabytes * 1024 * 1024
}

fn divided_group(cx: &App) -> SettingGroup {
    SettingGroup::new()
        .border_b_1()
        .border_color(cx.theme().border)
}

/// How much of the change log a project keeps. Set per project as well as app-wide, because the
/// answer belongs to the collection being catalogued rather than to the machine.
fn history_group(cx: &App) -> SettingGroup {
    divided_group(cx).title("History").item(
        Setting::Dropdown {
            key: settings::history::HISTORY_LIMIT_KEY,
            label: "Changes to keep",
            description: "The change log is an audit trail and keeps everything unless you say \n                          otherwise. A limit drops the oldest changes when the project is opened; \n                          named versions are never dropped.",
            options: settings::history::HISTORY_LIMITS,
        }
        .into_item(cx),
    )
}

/// Every plugin on disk, running or not: whether it runs at all, what it is allowed to reach, and
/// why it did not load when it did not.
///
/// Keyed by the name on disk rather than the descriptor's, because a plugin that fails to load has
/// no descriptor and switching a broken one off has to work.
fn plugins_page(cx: &App) -> SettingPage {
    let listing = plugin_host::listing(cx);
    let install = divided_group(cx)
        .title("Find and install")
        .description(
            "Browse the official catalog on qrate.dvnl.work, or review a public GitHub release.",
        )
        .item(
            SettingItem::render(|_opts: &_, window: &mut Window, cx: &mut App| {
                window
                    .use_keyed_state("plugin-installer", cx, |window, cx| {
                        crate::plugin_marketplace::MarketplaceWindow::new(
                            false, None, true, window, cx,
                        )
                    })
                    .into_any_element()
            })
            .keywords(["plugins", "catalog", "GitHub", "install"]),
        );
    let mut page = SettingPage::new("Plugins")
        .description(
            "Installed plugins run in a sandbox and reach the network only when you grant access.",
        )
        .group(install);
    if listing.is_empty() {
        return page.group(
            divided_group(cx)
                .title("Installed")
                .description("No plugins found. Plugins ▸ Plugins Folder is where they go."),
        );
    }

    for plugin in listing {
        let id = plugin.id.clone();
        let receipt = plugin_host::plugins_dir().and_then(|plugins| {
            let data = plugins.parent()?;
            plugin_package::read_receipt(&plugin_package::receipts_dir(data), &id)
                .ok()
                .flatten()
        });
        let mut group = divided_group(cx).title(plugin.name.clone()).item({
            let switched = id.clone();
            let item = SettingItem::new(
                "Enabled",
                SettingField::switch(
                    {
                        let id = id.clone();
                        move |cx: &App| settings::plugins::state(&id, cx).enabled
                    },
                    move |on: bool, cx: &mut App| {
                        settings::plugins::set_enabled(&switched, on, cx);
                        // Loading and unloading is what the switch *means*, and `reload` is
                        // already the one path that does either.
                        plugin_host::reload(cx);
                    },
                ),
            );
            // The load error goes on the switch rather than in a row of its own: it is the
            // reason someone is looking at this plugin, and a description is the only place on
            // a settings row that text of that length fits.
            match (&plugin.load_error, &plugin.description) {
                (Some(err), _) => item.description(SharedString::from(format!("✗ {err}"))),
                (None, Some(description)) => {
                    let description = receipt.as_ref().map_or_else(
                        || description.to_string(),
                        |receipt| {
                            format!(
                                "{description} Installed {} from {} (SHA-256 {}).",
                                receipt.version,
                                match receipt.source {
                                    plugin_package::InstallSource::OfficialCatalog =>
                                        "the official catalog",
                                    plugin_package::InstallSource::DirectGithub => "GitHub",
                                },
                                receipt.sha256
                            )
                        },
                    );
                    item.description(SharedString::from(description))
                }
                (None, None) => item,
            }
        });

        for permission in plugin.permissions {
            let (id, key) = (id.clone(), permission.clone());
            group = group.item(
                SettingItem::new(
                    "Network access",
                    SettingField::switch(
                        {
                            let (id, key) = (id.clone(), key.clone());
                            move |cx: &App| {
                                settings::plugins::state(&id, cx)
                                    .granted
                                    .iter()
                                    .any(|held| held.as_str() == key.as_ref())
                            }
                        },
                        move |on: bool, cx: &mut App| {
                            settings::plugins::set_granted(&id, &key, on, cx);
                            // The grant is read when a VM is built, so it takes effect on the
                            // next load and not before.
                            plugin_host::reload(cx);
                        },
                    ),
                )
                .description(SharedString::from(format!(
                    "Let {permission_owner} reach the internet. It asked for this; nothing \
                         stops it contacting any address once you agree.",
                    permission_owner = plugin.name
                ))),
            );
        }

        if let Some(receipt) = receipt {
            let remove_id = receipt.id.clone();
            group = group.item(
                SettingItem::new(
                    "Managed installation",
                    SettingField::element(move |_opts: &_, _window: &mut Window, _cx: &mut App| {
                        let id = remove_id.clone();
                        Button::new(format!("remove-managed-{id}"))
                            .small()
                            .label("Remove package…")
                            .on_click(move |_, window, cx| {
                                let answer = window.prompt(
                                    PromptLevel::Warning,
                                    &format!("Remove {id}?"),
                                    Some(
                                        "qrate will delete this managed package. Plugin settings are retained.",
                                    ),
                                    &["Remove", "Cancel"],
                                    cx,
                                );
                                let id = id.clone();
                                cx.spawn(async move |cx| {
                                    if answer.await.unwrap_or(1) != 0 {
                                        return;
                                    }
                                    cx.update(|cx| {
                                        let result = plugin_host::plugins_dir()
                                            .and_then(|plugins| {
                                                let data = plugins.parent()?;
                                                Some(plugin_package::remove_managed(
                                                    &id,
                                                    &plugins,
                                                    &plugin_package::receipts_dir(data),
                                                ))
                                            })
                                            .unwrap_or_else(|| {
                                                Err(anyhow::anyhow!(
                                                    "qrate application data is unavailable"
                                                ))
                                            });
                                        match result {
                                            Ok(()) => {
                                                log::info!("managed plugin removed: {id}");
                                                settings::plugins::set_enabled(&id, false, cx);
                                                plugin_host::reload(cx);
                                            }
                                            Err(error) => {
                                                log::error!(
                                                    "could not remove managed plugin {id}: {error:#}"
                                                );
                                            }
                                        }
                                    });
                                })
                                .detach();
                            })
                            .into_any_element()
                    }),
                )
                .description(format!(
                    "Version {}. Source: {}. Integrity: recorded SHA-256 and package size.",
                    receipt.version,
                    match receipt.source {
                        plugin_package::InstallSource::OfficialCatalog => "Official catalog",
                        plugin_package::InstallSource::DirectGithub => "Direct GitHub install",
                    }
                )),
            );
        }

        page = page.group(group);
    }
    page
}

/// One page per plugin that declares settings or a column mapping, titled with the plugin's name —
/// the same identity the Problems panel shows. Separate from [`plugins_page`] on purpose: that page
/// is about whether somebody else's code runs and what it may reach, which is a different question
/// from how it is configured once you have said yes.
fn plugin_pages(cx: &App) -> Vec<SettingPage> {
    let maps = ColumnMapContributions::all(cx);
    let mut pages: Vec<(SharedString, Option<SharedString>, Vec<SettingSpec>)> =
        plugin_host::setting_specs(cx);
    // A plugin whose only contribution is a mapping still needs somewhere to put it.
    for (plugin, _) in &maps {
        if !pages.iter().any(|(name, _, _)| name == plugin) {
            pages.push((plugin.clone(), None, Vec::new()));
        }
    }

    pages
        .into_iter()
        .map(|(name, description, specs)| {
            let mut page = SettingPage::new(name.clone());
            if let Some(description) = description {
                page = page.description(description);
            }
            if !specs.is_empty() {
                page = page.group(
                    divided_group(cx).title("Options").items(
                        specs
                            .into_iter()
                            .map(|spec| plugin_item(name.clone(), spec)),
                    ),
                );
            }
            match maps.iter().find(|(plugin, _)| plugin == &name) {
                Some((plugin, spec)) => page.group(mapping_group(plugin.clone(), spec.clone(), cx)),
                None => page,
            }
        })
        .collect()
}

/// A plugin's mapping tool: one picker per column over a list the plugin fetched, and the button
/// that refetches it.
///
/// No plugin code runs to draw this. The options are whatever the plugin last stored, which is what
/// lets the same list appear in a right-click menu that has to be built while the user waits.
fn mapping_group(plugin: SharedString, spec: ColumnMapSpec, cx: &App) -> SettingGroup {
    let refresh = {
        let (plugin, command) = (plugin.clone(), spec.refresh.clone());
        SettingItem::new(
            spec.label.clone(),
            SettingField::element(move |_opts: &_, _window: &mut _, _cx: &mut _| {
                let (plugin, command) = (plugin.clone(), command.clone());
                Button::new("refresh-map")
                    .small()
                    .label("Refresh from server")
                    .on_click(move |_, _, cx| {
                        let Some(hooks) = cx.try_global::<PluginHooks>().copied() else {
                            return;
                        };
                        (hooks.invoke)(&plugin, &command, &CommandContext::default(), cx);
                    })
                    .into_any_element()
            }),
        )
    };
    let refresh = match &spec.description {
        Some(description) => refresh.description(description.clone()),
        None => refresh,
    };

    let mut group = divided_group(cx).title("Mapping").item(refresh);

    let Some(project) = cx.try_global::<CurrentProject>() else {
        return group.description("Open a project to map its columns.");
    };
    let options: Vec<OptionItem> = ColumnMapContributions::options(&plugin, &spec, cx)
        .into_iter()
        .map(|(value, label)| OptionItem { value, label })
        .collect();
    if options.is_empty() {
        group = group.description(
            "Nothing to map to yet — refresh, and check the session log if the list stays empty.",
        );
    }

    // Read once for the whole page. `columns::get` parses the project's entire column-settings blob
    // on every call, and asking it per column inside each picker meant re-parsing it once per
    // column per frame — which is felt as the Settings window itself being slow.
    let stored = columns::load(cx);
    for column in column_items(project) {
        let mapped = Mapped {
            picked: ColumnMapContributions::picked(&plugin, &spec, stored.get(column.key.as_ref())),
            warns: warns(stored.get(column.key.as_ref()), &plugin),
        };
        let (plugin, spec, options) = (plugin.clone(), spec.clone(), options.clone());
        group = group.item(SettingItem::new(
            column.name.clone(),
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                map_picker(
                    plugin.clone(),
                    spec.clone(),
                    column.clone(),
                    options.clone(),
                    mapped.clone(),
                    window,
                    cx,
                )
            }),
        ));
    }
    group
}

fn plugin_item(id: SharedString, spec: SettingSpec) -> SettingItem {
    let (label, description, kind) = (spec.label.clone(), spec.description.clone(), spec.kind);
    let secret = (id.clone(), spec.clone());
    let read = {
        let (id, spec) = (id.clone(), spec.clone());
        move |cx: &App| plugin_host::setting_value(&id, &spec, cx)
    };
    let write = move |value: Json, cx: &mut App| plugin_host::set_setting(&id, &spec, value, cx);

    let item = match kind {
        SettingKind::Switch => SettingItem::new(
            label,
            SettingField::switch(
                move |cx: &App| read(cx).as_bool().unwrap_or(false),
                move |on: bool, cx: &mut App| write(on.into(), cx),
            ),
        ),
        SettingKind::Text => SettingItem::new(
            label,
            SettingField::input(
                move |cx: &App| SharedString::from(read(cx).as_str().unwrap_or("").to_string()),
                move |text: SharedString, cx: &mut App| write(text.to_string().into(), cx),
            ),
        )
        // Same reason as `Setting::Text` in the settings crate: horizontal inputs are fixed-width.
        .layout(Axis::Vertical),
        // `SettingField::input` builds its own unmasked `InputState`, so a password needs one of
        // its own — which is why this is an `element` and the others are not.
        SettingKind::Password => SettingItem::new(
            label,
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                secret_input(secret.0.clone(), secret.1.clone(), window, cx)
            }),
        )
        .layout(Axis::Vertical),
    };
    match description {
        Some(description) => item.description(description),
        None => item,
    }
}

/// What the last "Clear cache" press did, shown beside the button.
struct CacheCleared(SharedString);

impl Global for CacheCleared {}

/// Autosave as a toggle plus, when on, the method. Both edit the one `AUTOSAVE_KEY` the table reads
/// (`off`/`timed`/`immediate`): the switch is off iff the value is `off`, so an unset value reads as
/// on. The method row only exists while autosave is on — the Settings window observes settings
/// globals (see `SettingsWindow`), so flipping the switch rebuilds this page live.
/// The cache looks after itself — entries are keyed by the file's mtime and size, so a stale one
/// is never served, and it drops its oldest once it passes a couple of gigabytes. The button is
/// for reclaiming the space now rather than for correctness.
fn previews_group(cx: &App) -> SettingGroup {
    divided_group(cx).title("Previews").item(
        SettingItem::new(
            "Cached thumbnails",
            SettingField::element(|_opts: &_, _window: &mut _, cx: &mut App| {
                let outcome = cx.try_global::<CacheCleared>().map(|it| it.0.clone());
                h_flex()
                    .gap_2()
                    .items_center()
                    .children(outcome.map(|outcome| {
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(outcome)
                    }))
                    .child(
                        Button::new("clear-preview-cache")
                            .small()
                            .label("Clear cache")
                            .on_click(|_, _, cx| {
                                cx.set_global(CacheCleared("Clearing…".into()));
                                let clear = cx.background_spawn(async { preview::cache::clear() });
                                cx.spawn(async move |cx| {
                                    let outcome = match clear.await {
                                        Ok(cleared) => {
                                            log::info!(
                                                "cleared {} cached preview thumbnails",
                                                cleared.thumbnails
                                            );
                                            format!(
                                                "Cleared {} thumbnail{}, {}",
                                                cleared.thumbnails,
                                                if cleared.thumbnails == 1 { "" } else { "s" },
                                                preview::file_size(cleared.bytes)
                                            )
                                        }
                                        Err(err) => {
                                            log::error!(
                                                "could not clear the preview thumbnail cache: {err}"
                                            );
                                            format!("Could not clear the cache: {err}")
                                        }
                                    };
                                    cx.update(|cx| {
                                        cx.set_global(CacheCleared(outcome.into()));
                                        cx.refresh_windows();
                                    });
                                })
                                .detach();
                                cx.refresh_windows();
                            })
                            .into_any_element(),
                    )
                    .into_any_element()
            }),
        )
        .description(
            "Downscaled copies of your files, so photos and scans open instantly the second time. \
             They are rebuilt as you browse, so deleting them is safe — it only makes the next \
             look at each file slower.",
        ),
    )
    .item(
        SettingItem::new(
            "Cache size",
            SettingField::dropdown(
                options(PREVIEW_CACHE_SIZES),
                |cx: &App| {
                    settings::AppSettings::get(cx)
                        .values
                        .get(PREVIEW_CACHE_KEY)
                        .map(|value| value.text())
                        .unwrap_or_default()
                },
                |size: SharedString, cx: &mut App| {
                    settings::AppSettings::set_text(PREVIEW_CACHE_KEY, size, cx);
                    let bytes = preview_cache_bytes(cx);
                    preview::cache::set_cap(bytes);
                    cx.background_spawn(async move {
                        if let Some(dir) = preview::cache::dir() {
                            preview::cache::prune(&dir, bytes);
                        }
                    })
                    .detach();
                },
            ),
        )
        .description("The oldest thumbnails are dropped once the cache is larger than this."),
    )
    .item(
        SettingItem::new(
            "Visual search model",
            SettingField::element(|_opts: &_, _window: &mut _, cx: &mut App| {
                Button::new("remove-visual-model")
                    .small()
                    .label("Remove model…")
                    .disabled(!matches!(
                        components::state(components::ComponentId::Clip, cx),
                        components::State::Installed { .. } | components::State::UpdateRequired
                    ))
                    .on_click(|_, window, cx| {
                        let answer = window.prompt(
                            PromptLevel::Warning,
                            "Remove the visual search model?",
                            Some(
                                "This frees about 600 MB. Visual search stops working until you \
                                 download the model again from the search bar.",
                            ),
                            &["Remove", "Cancel"],
                            cx,
                        );
                        let handle = window.window_handle();
                        cx.spawn(async move |cx| {
                            if answer.await.unwrap_or(1) != 0 {
                                return;
                            }
                            let removed = cx.update(table::remove_visual_model);
                            if let Err(err) = removed {
                                log::warn!("did not remove the visual search model: {err:#}");
                                let _ = cx.update_window(handle, |_, window, cx| {
                                    window.prompt(
                                        PromptLevel::Info,
                                        "The model was not removed",
                                        Some(&err.to_string()),
                                        &["OK"],
                                        cx,
                                    )
                                });
                            }
                            cx.update(|cx| cx.refresh_windows());
                        })
                        .detach();
                    })
                    .into_any_element()
            }),
        )
        .description(
            "The CLIP weights visual search installs into qrate's data folder. Removing them \
             is refused while the model is downloading or indexing.",
        ),
    )
}

/// Google Sheets has three deliberately separate states: the app-wide feature switch, the
/// machine's OAuth grant, and the open project's chosen sync destination.
fn google_page(cx: &App) -> SettingPage {
    let mut group = divided_group(cx).title("Google Sheets").item(
        SettingItem::new(
            "Enable Google Sheets",
            SettingField::render(|_opts: &_, _window: &mut _, cx: &mut App| {
                let on = settings::google_enabled(cx);
                Switch::new("google-sync")
                    .checked(on)
                    .on_click(|on: &bool, _, cx| {
                        log::info!(
                            "Google Sheets integration switched {}",
                            if *on { "on" } else { "off" }
                        );
                        settings::AppSettings::set_bool(settings::GOOGLE_SYNC_KEY, *on, cx);
                        if !*on {
                            crate::google::clear_refresh_token(cx);
                        }
                        crate::app_menus::install(cx);
                    })
            }),
        )
        .description(
            "Show Google Sheets export and sync tools. Turning this off also forgets the Google \
             sign-in stored on this machine; public-sheet import remains available.",
        ),
    );

    if settings::google_enabled(cx) {
        let authenticated = crate::google::authenticated(cx);
        group = group.item(
            SettingItem::new(
                "Google account",
                SettingField::element(move |_opts: &_, _window: &mut _, _cx: &mut _| {
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(div().text_sm().child(if authenticated {
                            "Signed in"
                        } else {
                            "Not signed in"
                        }))
                        .child(
                            Button::new("google-sign-in")
                                .small()
                                .label(if authenticated {
                                    "Reauthenticate…"
                                } else {
                                    "Sign in…"
                                })
                                .on_click(|_, _, cx| crate::export::authenticate(cx)),
                        )
                        // Nothing is stored until there is a sign-in, so before one this button
                        // offers to undo something that never happened.
                        .when(authenticated, |row| {
                            row.child(
                                Button::new("google-sign-out")
                                    .small()
                                    .label("Forget sign-in")
                                    .on_click(|_, _, cx| {
                                        log::info!(
                                            "Google sign-in removal requested from Settings"
                                        );
                                        crate::google::clear_refresh_token(cx);
                                    }),
                            )
                        })
                        .into_any_element()
                }),
            )
            .description(
                "Authenticate this computer with Google. qrate only requests access to sheets it \
                 creates or that you explicitly choose.",
            )
            // Three controls do not fit the fixed-width field cell a horizontal row gets; stacked,
            // they have the pane's full width. Same reason as `Setting::Text`.
            .layout(Axis::Vertical),
        );

        let destination = cx
            .try_global::<CurrentProject>()
            .and_then(|project| {
                project
                    .data
                    .values
                    .get(settings::project::GOOGLE_SHEET_ID_KEY)
            })
            .map(|value| value.text().to_string())
            .filter(|id| !id.is_empty());
        let project_open = cx.has_global::<CurrentProject>();
        let destination_description = match destination.as_ref() {
            Some(_) => "Sync to Google Sheet writes to the selected spreadsheet.",
            None if project_open => "No sync destination is selected for this project.",
            None => "Open a project to choose its sync destination.",
        };
        group = group.item(
            SettingItem::new(
                "Sync destination",
                SettingField::element(move |_opts: &_, _window: &mut _, _cx: &mut App| {
                    let current = destination.clone();
                    h_flex()
                        .gap_2()
                        .flex_wrap()
                        .child(
                            Button::new("google-sync-destination")
                                .small()
                                .label(if current.is_some() {
                                    "Change spreadsheet…"
                                } else {
                                    "Choose spreadsheet…"
                                })
                                .disabled(!project_open)
                                .on_click(|_, _, cx| crate::export::choose_sync_destination(cx)),
                        )
                        .when_some(current, |row, id| {
                            let open_id = id.clone();
                            row.child(
                                Button::new("open-google-sync-destination")
                                    .small()
                                    .label("Open")
                                    .on_click(move |_, _, cx| {
                                        cx.open_url(&data_exchange::google::sheet_url(&open_id));
                                    }),
                            )
                            .child(
                                Button::new("clear-google-sync-destination")
                                    .small()
                                    .label("Clear")
                                    .on_click(|_, _, cx| crate::export::clear_sync_destination(cx)),
                            )
                        })
                        .into_any_element()
                }),
            )
            .description(destination_description)
            .layout(Axis::Vertical),
        );

        group = group.item(
            SettingItem::new(
                "Credential endpoint",
                SettingField::input(
                    |cx: &App| {
                        settings::AppSettings::get(cx)
                            .values
                            .get(settings::GOOGLE_CONFIG_ENDPOINT_KEY)
                            .map(|v| v.text())
                            .unwrap_or_default()
                    },
                    |val: SharedString, cx: &mut App| {
                        settings::AppSettings::set_text(
                            settings::GOOGLE_CONFIG_ENDPOINT_KEY,
                            val,
                            cx,
                        );
                    },
                ),
            )
            .description(
                "Where qrate asks which Google project to sign in against. Leave it empty for \
                 ours. An institution that runs its own Google project points this at its own \
                 endpoint — see docs/dev/site-oauth-handoff.md for what to serve.",
            )
            .layout(Axis::Vertical),
        );
    }

    SettingPage::new("Google").group(group)
}

fn saving_group(cx: &App) -> SettingGroup {
    let mut group = divided_group(cx).title("Saving").item(
        SettingItem::new(
            "Autosave",
            SettingField::switch(
                |cx: &App| settings::effective_text(settings::AUTOSAVE_KEY, cx) != "off",
                |on: bool, cx: &mut App| {
                    settings::set_user_text(
                        settings::AUTOSAVE_KEY,
                        if on { "timed" } else { "off" }.into(),
                        cx,
                    );
                },
            ),
        )
        .description("Save cell edits automatically. Ctrl+S always saves."),
    );

    if settings::effective_text(settings::AUTOSAVE_KEY, cx) != "off" {
        group = group.item(
            SettingItem::new(
                "Method",
                SettingField::dropdown(
                    vec![
                        ("timed".into(), "After a short pause".into()),
                        ("immediate".into(), "On every edit".into()),
                    ],
                    |cx: &App| {
                        let v = settings::effective_text(settings::AUTOSAVE_KEY, cx);
                        if v == "immediate" { v } else { "timed".into() }
                    },
                    |val: SharedString, cx: &mut App| {
                        settings::set_user_text(settings::AUTOSAVE_KEY, val, cx);
                    },
                ),
            )
            .description("When edits reach the file: after you pause typing, or on every edit."),
        );
    }
    group
}

/// The spell-check master switch and, when on, the dictionary language. Both read through their
/// own closures rather than `Setting::Switch`, because an unset value has to mean *on* — see
/// `SPELLCHECK_ENABLED_KEY`. Which columns are checked lives on the Columns page instead, next to
/// the other per-column knobs. Changing any of them reloads the checker.
fn spelling_group(cx: &App) -> SettingGroup {
    let mut group = divided_group(cx).title("Spelling").item(
        SettingItem::new(
            "Check spelling",
            SettingField::switch(
                |cx: &App| spellcheck::enabled(cx),
                |on: bool, cx: &mut App| {
                    settings::set_user_bool(spellcheck::SPELLCHECK_ENABLED_KEY, on, cx);
                    crate::register_spell_checker(cx);
                },
            ),
        )
        .description("Underline misspelled words in table cells. Right-click a cell to fix one."),
    );

    if spellcheck::enabled(cx) {
        group = group.item(
            SettingItem::new(
                "Ignore names",
                SettingField::switch(
                    |cx: &App| spellcheck::ignore_capitalized(cx),
                    |on: bool, cx: &mut App| {
                        settings::set_user_bool(spellcheck::SPELLCHECK_NAMES_KEY, on, cx);
                        crate::register_spell_checker(cx);
                    },
                ),
            )
            .description(
                "Skip capitalized words. No dictionary holds every person, place, or studio, so \
                 without this a catalogue of names is mostly underlines. Turn it off to catch \
                 typos that begin with a capital.",
            ),
        );
        group = group.item(
            SettingItem::new(
                "Language",
                SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                    // Rows carry download state, so `picker` rebuilds the list as downloads finish.
                    let rows = language_rows(cx);
                    let state = picker(
                        |pickers| &mut pickers.language,
                        LANGUAGE.to_string(),
                        rows.clone(),
                        PickerKind {
                            multiple: true,
                            searchable: true,
                        },
                        |values: &[SharedString], cx: &mut App| {
                            spellcheck::retain_wanted_downloads(values, cx);
                            let listing = spellcheck::catalogue::listing();
                            let state_of = |code: &str| {
                                listing
                                    .iter()
                                    .find(|((c, _, _), _)| *c == code)
                                    .map(|(_, state)| *state)
                            };
                            let mut codes: Vec<String> = spellcheck::languages(cx)
                                .into_iter()
                                .filter(|code| values.iter().any(|v| v.as_ref() == code))
                                .collect();
                            for value in values {
                                match state_of(value) {
                                    Some(spellcheck::catalogue::State::Available) => {
                                        spellcheck::start_download(
                                            value.clone(),
                                            crate::register_spell_checker,
                                            cx,
                                        )
                                    }
                                    Some(_) if !codes.iter().any(|c| c == value.as_ref()) => {
                                        codes.push(value.to_string())
                                    }
                                    _ => {}
                                }
                            }
                            if codes != spellcheck::languages(cx) {
                                spellcheck::set_languages(&codes, cx);
                                crate::register_spell_checker(cx);
                            }
                        },
                        window,
                        cx,
                    );
                    let chosen: Vec<SharedString> = spellcheck::languages(cx)
                        .into_iter()
                        .map(SharedString::from)
                        .collect();
                    sync_selection(&state, &rows, &chosen, window, cx);
                    let label = picker_label(
                        &chosen
                            .iter()
                            .filter_map(|code| rows.iter().find(|row| &row.code == code))
                            .map(|row| row.name.clone())
                            .collect::<Vec<_>>(),
                    );

                    Combobox::new(&state)
                        .small()
                        .menu_width(px(320.))
                        .menu_max_h(px(360.))
                        .search_placeholder("Search languages…")
                        .render_trigger(move |_ctx, _, _| div().child(label.clone()))
                        .into_any_element()
                }),
            )
            .description(
                "Each value is checked in whichever ticked language fits it. Canadian and \
                 American English are built in; ticking another language downloads it. The first \
                 one ticked sets the preferred regional spelling.",
            ),
        );
    }
    group
}

/// Combobox states outlive the page builders that render them: `SettingsWindow` re-invokes
/// [`build_pages`] on every render, so an entity created in there would be rebuilt each frame and
/// forget both its selection and whether it was open. Keyed by picker id, with each one's
/// `Change` subscription parked alongside so it stays alive.
#[derive(Default)]
struct Pickers {
    columns: HashMap<String, Picker<ColumnItem>>,
    /// One per `(plugin, mapping, column)`, so the ids are built at runtime rather than named here.
    maps: HashMap<String, Picker<OptionItem>>,
    descriptions: HashMap<String, Picker<OptionItem>>,
    description_rows: HashSet<(PathBuf, String)>,
    /// One per declared `password` setting.
    secrets: HashMap<String, Secret>,
    /// The one language picker, in a map only so it is the same shape as the others — see
    /// [`picker`], which is generic over which of these it reaches into.
    language: HashMap<String, Picker<LanguageRow>>,
}

/// The single language picker's key in [`Pickers::language`].
const LANGUAGE: &str = "language";

struct Secret {
    state: Entity<InputState>,
    _sub: Subscription,
}

impl Global for Pickers {}

struct Picker<I: SearchableListItem + PartialEq + 'static>
where
    I::Value: PartialEq + Clone,
{
    state: Entity<ComboboxState<CheckList<I>>>,
    /// What the list was last built from. `set_items` replaces the delegate wholesale, which
    /// throws away the active search filter — so it only runs when this actually changed.
    items: Vec<I>,
    _sub: Subscription,
}

/// Which of the columns have the property a [`column_picker`] is over, read once per build.
type ReadsColumns = Rc<dyn Fn(&[ColumnItem], &App) -> HashSet<SharedString>>;
/// Set it on or off for every column the picker changed, as one write.
type WritesColumns = Rc<dyn Fn(&[(&ColumnItem, bool)], &mut App)>;

/// A [`column_picker`] over one flag of the column settings: read from one parse of the settings,
/// written back as one update however many columns changed.
fn settings_flag(
    get: fn(&columns::ColumnSettings) -> bool,
    put: fn(&mut columns::ColumnSettings, bool),
) -> (ReadsColumns, WritesColumns) {
    let reads = Rc::new(
        move |items: &[ColumnItem], cx: &App| -> HashSet<SharedString> {
            let stored = columns::shared(cx);
            let default = columns::ColumnSettings::default();
            items
                .iter()
                .filter(|c| get(stored.get(c.key.as_ref()).unwrap_or(&default)))
                .map(|c| c.key.clone())
                .collect()
        },
    );
    let writes = Rc::new(move |changes: &[(&ColumnItem, bool)], cx: &mut App| {
        let mut map = columns::load(cx);
        for (column, want) in changes {
            put(map.entry(column.key.to_string()).or_default(), *want);
        }
        let Ok(json) = serde_json::to_string(&map) else {
            return;
        };
        CurrentProject::set_text(columns::COLUMN_SETTINGS_KEY, json.into(), cx);
        settings::dirty::mark(settings::dirty::COLUMN_SETTINGS, cx);
    });
    (reads, writes)
}

/// One data column, as the pickers on the Columns page list it. The value is the stable `c{ix}`
/// key, not the display name, so two columns sharing a header stay distinct.
#[derive(Clone, PartialEq)]
struct ColumnItem {
    key: SharedString,
    name: SharedString,
}

impl SearchableListItem for ColumnItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.key
    }
}

/// A masked field for a declared `password` setting, written back as it is typed.
///
/// Its `InputState` is kept in [`Pickers`] for the same reason the comboboxes are: `build_pages`
/// runs on every render, and an entity created in there would be rebuilt each frame — here that
/// would drop the caret mid-word.
fn secret_input(
    plugin: SharedString,
    spec: SettingSpec,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !cx.has_global::<Pickers>() {
        cx.set_global(Pickers::default());
    }
    let id = format!("{plugin}/{}", spec.key);
    let state = match cx.global::<Pickers>().secrets.get(&id) {
        Some(secret) => secret.state.clone(),
        None => {
            let stored = plugin_host::setting_value(&plugin, &spec, cx)
                .as_str()
                .unwrap_or_default()
                .to_string();
            let state = cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder("Not set")
                    .default_value(stored)
            });
            let _sub = cx.subscribe(&state, {
                let (plugin, spec) = (plugin.clone(), spec.clone());
                move |state: Entity<InputState>, event: &InputEvent, cx: &mut App| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let typed = state.read(cx).value().to_string();
                    plugin_host::set_setting(&plugin, &spec, typed.into(), cx);
                }
            });
            cx.global_mut::<Pickers>().secrets.insert(
                id.clone(),
                Secret {
                    state: state.clone(),
                    _sub,
                },
            );
            state
        }
    };

    Input::new(&state).small().into_any_element()
}

/// One entry in a plugin's mapping list — a vocabulary, in the only plugin that has one. The value
/// is what gets stored; the label is only ever shown.
#[derive(Clone, PartialEq)]
struct OptionItem {
    value: SharedString,
    label: SharedString,
}

impl SearchableListItem for OptionItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

/// What one column is mapped to, and how loud that plugin's findings on it are. Both are read out
/// of the same already-loaded settings entry, so they travel together.
#[derive(Clone)]
struct Mapped {
    picked: Vec<SharedString>,
    warns: bool,
}

/// One column's mapping, over whatever the plugin last stored.
///
/// Same shape as [`column_picker`] and for the same reason: the stored settings stay the source of
/// truth, synced *from* here and written *back* in the `Change` subscription, so there is never a
/// second answer to reconcile. Keyed per plugin and column, since each column gets its own widget.
fn map_picker(
    plugin: SharedString,
    spec: ColumnMapSpec,
    column: ColumnItem,
    options: Vec<OptionItem>,
    mapped: Mapped,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Mapped { picked, warns } = mapped;
    let state = picker(
        |pickers| &mut pickers.maps,
        format!("{plugin}/{}/{}", spec.key, column.key),
        options.clone(),
        PickerKind {
            multiple: spec.multiple,
            searchable: true,
        },
        {
            let (plugin, spec, key) = (plugin.clone(), spec.clone(), column.key.clone());
            move |values: &[SharedString], cx: &mut App| {
                ColumnMapContributions::select(&plugin, &spec, &key, values.to_vec(), cx);
                // A validator reads this mapping, so its published findings are stale the moment
                // it changes — and only a run replaces them.
                table::revalidate_now(cx);
            }
        },
        window,
        cx,
    );
    sync_selection(&state, &options, &picked, window, cx);

    let label: SharedString = match picked.len() {
        0 => "Not mapped".into(),
        _ => options
            .iter()
            .filter(|option| picked.contains(&option.value))
            .map(|option| option.label.to_string())
            .collect::<Vec<_>>()
            .join(", ")
            .into(),
    };

    h_flex()
        .gap_2()
        // Nothing mapped means the plugin has nothing to check this column against.
        .when(!picked.is_empty(), |row| {
            row.child(severity_switch(column.key.clone(), plugin.clone(), warns))
        })
        .child(
            Combobox::new(&state)
                .small()
                .menu_width(px(240.))
                .menu_max_h(px(320.))
                .search_placeholder("Search…")
                .empty(|_, _| div().p_2().child("Nothing to map to yet"))
                .render_trigger(move |_ctx, _, _| div().child(label.clone())),
        )
        .into_any_element()
}

/// One language as the picker draws it.
#[derive(Clone, PartialEq)]
struct LanguageRow {
    code: SharedString,
    name: SharedString,
    licence: SharedString,
    state: spellcheck::catalogue::State,
    downloading: bool,
}

impl SearchableListItem for LanguageRow {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.name.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.code
    }

    /// Name over licence, because several of these word lists are GPL and that is a thing to see
    /// before downloading rather than after.
    fn render(&self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        h_flex()
            .w_full()
            .gap_x_2()
            .justify_between()
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().text_sm().child(self.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.licence.clone()),
                    ),
            )
            .when(self.downloading, |row| {
                row.child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(muted)
                        .child("Downloading…"),
                )
            })
            .when(
                !self.downloading && self.state == spellcheck::catalogue::State::Available,
                |row| row.child(Icon::new(IconName::ArrowDown).xsmall()),
            )
    }
}

fn language_rows(cx: &App) -> Vec<LanguageRow> {
    spellcheck::catalogue::listing()
        .into_iter()
        .map(|((code, name, licence), state)| LanguageRow {
            code: code.into(),
            name: name.into(),
            licence: licence.into(),
            state,
            downloading: spellcheck::is_downloading(code, cx),
        })
        .collect()
}

/// Point a combobox's selection at `want` — but only when it isn't already there. `set_selected_
/// indices` notifies unconditionally, so an unguarded per-render sync repaints forever.
///
/// Indices come from `items` (the list the state was built from) rather than from the widget,
/// which only knows the search-filtered view. The guard keeps this a no-op during interaction, so
/// the two agree in practice.
fn sync_selection<I: SearchableListItem<Value = SharedString> + 'static>(
    state: &Entity<ComboboxState<CheckList<I>>>,
    items: &[I],
    want: &[SharedString],
    window: &mut Window,
    cx: &mut App,
) {
    let current = state.read(cx).selected_values();
    if current.len() == want.len() && want.iter().all(|v| current.contains(v)) {
        return;
    }
    let indices: Vec<IndexPath> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| want.contains(item.value()))
        .map(|(ix, _)| IndexPath::new(ix))
        .collect();
    state.update(cx, |state, cx| {
        state.set_selected_indices(indices, window, cx);
    });
}

/// How a picker's widget behaves, as opposed to what it lists.
#[derive(Clone, Copy)]
struct PickerKind {
    multiple: bool,
    searchable: bool,
}

/// Get — or, the first time, build — the combobox behind one picker.
///
/// Every picker on this window is the same widget over a different list, and each one needs the
/// same three things: state that outlives `build_pages` (see [`Pickers`]), a list replaced only
/// when it actually changed (`set_items` throws away the active search filter), and a `Change`
/// subscription parked alongside so it stays alive. `slot` says which of [`Pickers`]' maps this
/// kind of picker lives in, since the item type is what keeps them apart.
///
/// What is *selected* is deliberately not here: each caller reads that from a different corner of
/// settings, and the stored value stays the source of truth — callers sync it with
/// [`sync_selection`] once they have the state back.
fn picker<I>(
    slot: fn(&mut Pickers) -> &mut HashMap<String, Picker<I>>,
    id: String,
    items: Vec<I>,
    kind: PickerKind,
    on_change: impl Fn(&[SharedString], &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> Entity<ComboboxState<CheckList<I>>>
where
    I: SearchableListItem<Value = SharedString> + PartialEq + Clone + 'static,
{
    if !cx.has_global::<Pickers>() {
        cx.set_global(Pickers::default());
    }
    let existing = slot(cx.global_mut::<Pickers>())
        .get(&id)
        .map(|picker| picker.state.clone());
    let state = match existing {
        Some(state) => state,
        None => {
            let state = cx.new(|cx| {
                ComboboxState::new(CheckList::new(items.clone()), vec![], window, cx)
                    .multiple(kind.multiple)
                    .searchable(kind.searchable)
            });
            let _sub = cx.subscribe(&state, move |_state, event, cx| {
                if let ComboboxEvent::Change(values) = event {
                    on_change(values, cx);
                }
            });
            slot(cx.global_mut::<Pickers>()).insert(
                id.clone(),
                Picker {
                    state: state.clone(),
                    items: items.clone(),
                    _sub,
                },
            );
            return state;
        }
    };

    // A refresh replaces the list; rebuilding on every render would throw away the search filter.
    if slot(cx.global_mut::<Pickers>())[&id].items != items {
        state.update(cx, |state, cx| {
            state.set_items(CheckList::new(items.clone()), window, cx);
        });
        if let Some(picker) = slot(cx.global_mut::<Pickers>()).get_mut(&id) {
            picker.items = items;
        }
    }
    state
}

/// Per-column filters, project-scoped. A master switch gates the feature; when on, a multi-select
/// picker chooses which columns show a filter dropdown (a picked column *is* a filter-enabled one —
/// selection and `filter_enabled` are the same thing now). Built from `&App` because the columns
/// depend on whichever project is open.
fn columns_page(cx: &App) -> SettingPage {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return SettingPage::new("Columns")
            .group(
                divided_group(cx)
                    .title("Filters")
                    .description("Open a project to configure its columns."),
            )
            .group(authority_accounts_group(cx));
    };

    let headers = column_items(project);

    let values = divided_group(cx).title("Multi-value cells").item(
        Setting::Text {
            key: settings::FILTER_SUBDELIMITER_KEY,
            label: "Value separator",
            description: "Separate multiple values in one cell, e.g. \"|\" for \"Film|Video\". \
                          Filters and validators treat each part as one logical value. Leave empty \
                          to treat the complete cell as one value.",
        }
        .into_item(cx),
    );

    let mut group = divided_group(cx).title("Filters").item(
        SettingItem::new(
            "Enable column filters",
            SettingField::switch(
                |cx: &App| columns::filters_master_enabled(cx),
                |on: bool, cx: &mut App| columns::set_filters_master_enabled(on, cx),
            ),
        )
        .description("Show a filter dropdown in the header of the columns you pick."),
    );

    if columns::filters_master_enabled(cx) {
        let picked = headers.clone();
        group = group.item(
            SettingItem::new(
                "Filtered columns",
                SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                    column_picker(
                        "filtered-columns",
                        picked.clone(),
                        settings_flag(|s| s.filter_enabled, |s, on| s.filter_enabled = on),
                        window,
                        cx,
                    )
                }),
            )
            .description("Columns that show a filter dropdown."),
        );
    }

    let picked = headers.clone();
    let spelling = divided_group(cx).title("Spelling").item(
        SettingItem::new(
            "Spell-checked columns",
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                column_picker(
                    "spellchecked-columns",
                    picked.clone(),
                    settings_flag(|s| s.spellcheck, |s, on| s.spellcheck = on),
                    window,
                    cx,
                )
            }),
        )
        .description("Columns whose text is spell-checked. New columns start checked."),
    );

    let variant_columns = headers.clone();
    let variants = divided_group(cx).title("Value variants").item(
        SettingItem::new(
            "Reviewed columns",
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                column_picker(
                    "variant-reviewed-columns",
                    variant_columns.clone(),
                    settings_flag(|s| s.variant_review, |s, on| s.variant_review = on),
                    window,
                    cx,
                )
            }),
        )
        .description(
            "Columns checked for inconsistent displayed forms. Similar values are suggestions for \
             review, not automatic merges.",
        ),
    );

    let ignored = divided_group(cx)
        .title("Ignored problems")
        .description("Findings you chose to ignore. Remove one to see it in Problems again.")
        .item(
            SettingItem::new(
                "Ignored",
                SettingField::element(move |_opts: &_, _window: &mut _, cx: &mut App| {
                    // Only ignores that still hide something are listed; Clear all removes the rest.
                    let hidden = diagnostics::Diagnostics::ignored(cx);
                    let mut entries: Vec<(
                        SharedString,
                        String,
                        String,
                        String,
                        Option<settings::project::RowId>,
                    )> = Vec::new();
                    for (column, settings) in columns::load(cx) {
                        let matching = |source: &String, key: &String| {
                            hidden
                                .iter()
                                .filter(|d| {
                                    d.location.column.as_deref() == Some(column.as_str())
                                        && d.source.key() == *source
                                        && d.ignore_key() == *key
                                })
                                .collect::<Vec<_>>()
                        };
                        for (source, key) in &settings.ignored_diagnostics {
                            let hits = matching(source, key);
                            let Some(first) = hits.first() else {
                                continue;
                            };
                            let summary = first
                                .group
                                .as_ref()
                                .map_or(&first.message, |group| &group.summary);
                            let label = format!(
                                "{column} · {} · {summary} ({})",
                                first.source.label(),
                                hits.len()
                            );
                            entries.push((
                                label.into(),
                                column.clone(),
                                source.clone(),
                                key.clone(),
                                None,
                            ));
                        }
                        for (source, key, id) in &settings.ignored_occurrences {
                            let Some(first) = matching(source, key)
                                .into_iter()
                                .find(|d| d.location.row_id == Some(*id))
                            else {
                                continue;
                            };
                            let label = format!(
                                "{column} · Row {} · {}",
                                first.location.row.map_or(0, |row| row + 1),
                                first.message
                            );
                            entries.push((
                                label.into(),
                                column.clone(),
                                source.clone(),
                                key.clone(),
                                Some(*id),
                            ));
                        }
                    }
                    let muted = cx.theme().muted_foreground;
                    v_flex()
                        .w_full()
                        .gap_1()
                        .when(entries.is_empty(), |list| {
                            list.child(div().text_sm().text_color(muted).child("Nothing ignored"))
                        })
                        .children(entries.into_iter().enumerate().map(
                            |(ix, (label, column, source, key, row_id))| {
                                h_flex()
                                    .w_full()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .text_ellipsis()
                                            .text_sm()
                                            .child(label),
                                    )
                                    .child(
                                        Button::new(("unignore", ix))
                                            .xsmall()
                                            .icon(IconName::Close)
                                            .tooltip("Stop ignoring")
                                            .on_click(move |_, _, cx| {
                                                let entry = (source.clone(), key.clone());
                                                columns::update(
                                                    &column,
                                                    |settings| match row_id {
                                                        Some(id) => {
                                                            settings
                                                                .ignored_occurrences
                                                                .remove(&(entry.0, entry.1, id));
                                                        }
                                                        None => {
                                                            settings
                                                                .ignored_diagnostics
                                                                .remove(&entry);
                                                        }
                                                    },
                                                    cx,
                                                );
                                                table::revalidate_now(cx);
                                            }),
                                    )
                            },
                        ))
                        .child(
                            Button::new("clear-ignored-diagnostics")
                                .small()
                                .label("Clear all")
                                .on_click(move |_, _, cx| {
                                    columns::clear_ignored(cx);
                                    table::revalidate_now(cx);
                                }),
                        )
                        .into_any_element()
                }),
            )
            .layout(Axis::Vertical),
        );

    SettingPage::new("Columns")
        .group(values)
        .group(group)
        .group(spelling)
        .group(variants)
        .group(ignored)
        .group(descriptions_group(project, &headers, cx))
        .group(data_types_group(headers, cx))
        .group(authority_accounts_group(cx))
        .group(authority_columns_group(project, cx))
        .group(config_file_group(cx))
}

fn descriptions_group(project: &CurrentProject, headers: &[ColumnItem], cx: &App) -> SettingGroup {
    let file = project.file.clone();
    let session_rows = cx
        .try_global::<Pickers>()
        .map(|pickers| &pickers.description_rows);
    let mut group = divided_group(cx).title("Descriptions").description(
        "Explain what each column means. Descriptions appear in the grid and agent context.",
    );
    let mut missing = Vec::new();

    for header in headers {
        let notes = project
            .data
            .columns
            .iter()
            .find(|column| column.name == header.name)
            .map(|column| column.notes.as_str())
            .unwrap_or_default();
        let shown = !notes.trim().is_empty()
            || session_rows
                .is_some_and(|rows| rows.contains(&(file.clone(), header.name.to_string())));
        if !shown {
            missing.push(OptionItem {
                value: header.name.clone(),
                label: header.name.clone(),
            });
            continue;
        }

        let getter_name = header.name.clone();
        let setter_name = header.name.clone();
        group = group.item(
            SettingItem::new(
                header.name.clone(),
                SettingField::input(
                    move |cx: &App| {
                        cx.try_global::<CurrentProject>()
                            .and_then(|project| {
                                project
                                    .data
                                    .columns
                                    .iter()
                                    .find(|column| column.name == getter_name)
                            })
                            .map(|column| column.notes.clone().into())
                            .unwrap_or_default()
                    },
                    move |notes: SharedString, cx: &mut App| {
                        if let Some(file) = cx
                            .try_global::<CurrentProject>()
                            .map(|project| project.file.clone())
                        {
                            cx.default_global::<Pickers>()
                                .description_rows
                                .insert((file, setter_name.to_string()));
                            CurrentProject::set_column_notes(&setter_name, &notes, cx);
                        }
                    },
                ),
            )
            .layout(Axis::Vertical),
        );
    }

    if !missing.is_empty() {
        group = group.item(SettingItem::new(
            "Add a description",
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                let state = picker(
                    |pickers| &mut pickers.descriptions,
                    "add-description".into(),
                    missing.clone(),
                    PickerKind {
                        multiple: false,
                        searchable: false,
                    },
                    |values: &[SharedString], cx: &mut App| {
                        let Some(column) = values.first().filter(|value| !value.is_empty()) else {
                            return;
                        };
                        let Some(file) = cx
                            .try_global::<CurrentProject>()
                            .map(|project| project.file.clone())
                        else {
                            return;
                        };
                        cx.default_global::<Pickers>()
                            .description_rows
                            .insert((file, column.to_string()));
                    },
                    window,
                    cx,
                );
                Combobox::new(&state)
                    .small()
                    .placeholder("Choose a column…")
                    .into_any_element()
            }),
        ));
    }
    group
}

/// What each column holds, as one picker per type. Inverted from the obvious row-per-column shape
/// because a project has far more columns than types, and because a column has exactly one type —
/// which the pickers enforce between them: setting a column here clears it from whichever type
/// held it, since each reads its selection back from the same declared value.
///
/// Unchecking falls back to `Text`, the type that assumes least. It is why `Text`'s own picker
/// cannot be emptied: a column always has a type, and that is the one it has when nothing is said.
fn data_types_group(headers: Vec<ColumnItem>, cx: &App) -> SettingGroup {
    let mut group = divided_group(cx).title("Data types").description(
        "What a column holds. Checks key off this: dates are validated in the Table ▸ Checks \
         date format, filenames are \
         resolved against the files folder, and only text is spell-checked.",
    );
    for ty in columns::ColumnType::ALL {
        let headers = headers.clone();
        group = group.item(SettingItem::new(
            ty.as_str(),
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                column_picker(
                    ty.as_str(),
                    headers.clone(),
                    (
                        Rc::new(
                            move |items: &[ColumnItem], cx: &App| -> HashSet<SharedString> {
                                let declared: HashMap<&str, &str> = cx
                                    .try_global::<CurrentProject>()
                                    .map(|p| {
                                        p.data
                                            .columns
                                            .iter()
                                            .map(|c| (c.name.as_str(), c.data_type.as_str()))
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                items
                                    .iter()
                                    .filter(|c| {
                                        declared
                                            .get(c.name.as_ref())
                                            .map_or(columns::ColumnType::Text, |declared| {
                                                columns::ColumnType::from_declared(declared)
                                            })
                                            == ty
                                    })
                                    .map(|c| c.key.clone())
                                    .collect()
                            },
                        ),
                        Rc::new(move |changes: &[(&ColumnItem, bool)], cx: &mut App| {
                            for (column, want) in changes {
                                let ty = if *want { ty } else { columns::ColumnType::Text };
                                CurrentProject::set_column_type(&column.name, ty.as_str(), cx);
                            }
                        }),
                    ),
                    window,
                    cx,
                )
            }),
        ));
    }
    group
}

/// What the open project itself holds, as opposed to how the grid draws it. The files folder was
/// only ever settable in the wizard, so a project pointed at the wrong folder had no in-app fix.
fn project_page(cx: &App) -> SettingPage {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return SettingPage::new("Project").group(
            divided_group(cx)
                .title("Files")
                .description("Open a project to see where it looks for files."),
        );
    };

    SettingPage::new("Project")
        .description(SharedString::from(format!(
            "{} — {}",
            project.display_name(),
            project.file.display()
        )))
        .group(
            divided_group(cx)
                .title("Files")
                .item(settings::path_picker_item(
                    settings::project::FILES_FOLDER_KEY,
                    "Files folder",
                    "Where row images and linked files are looked up. qrate never moves them, so \
                 moving the folder means pointing this at its new home.",
                    "Choose files folder",
                    settings::Picks::Directories,
                    |cx: &App| {
                        cx.try_global::<CurrentProject>()
                            .and_then(|p| p.data.values.get(settings::project::FILES_FOLDER_KEY))
                            .map(|v| v.text())
                            .unwrap_or_default()
                    },
                    |val: SharedString, cx: &mut App| {
                        CurrentProject::set_text(settings::project::FILES_FOLDER_KEY, val, cx);
                        // Row images are resolved against this folder on every repaint, so the grid
                        // shows the new one as soon as it is told to look again.
                        table::revalidate_now(cx);
                    },
                )),
        )
        .group({
            let description = |cx: &App| {
                cx.try_global::<CurrentProject>()
                    .map(|project| {
                        settings::description::DescriptionConfig::from_values(&project.data.values)
                    })
                    .unwrap_or_else(|| settings::description::DescriptionProfile::Rad.defaults())
            };
            let store = |config: settings::description::DescriptionConfig, cx: &mut App| {
                for (key, value) in config.values() {
                    CurrentProject::set_text(key, value.into(), cx);
                }
            };
            let level = move |label: &'static str, folder: bool| {
                SettingItem::new(
                    label,
                    SettingField::input(
                        move |cx: &App| {
                            let config = description(cx);
                            let key = match folder {
                                true => &config.folder_level_key,
                                false => &config.file_level_key,
                            };
                            config
                                .levels
                                .iter()
                                .find(|level| &level.key == key)
                                .map(|level| SharedString::from(level.label.clone()))
                                .unwrap_or_default()
                        },
                        move |label: SharedString, cx: &mut App| {
                            let mut config = description(cx);
                            let key = match folder {
                                true => config.folder_level_key.clone(),
                                false => config.file_level_key.clone(),
                            };
                            config.relabel(&key, &label);
                            store(config, cx);
                        },
                    ),
                )
                .layout(Axis::Vertical)
            };
            divided_group(cx)
                .title("Description")
                .item(
                    SettingItem::new(
                        "Description standard",
                        SettingField::dropdown(
                            settings::description::DescriptionProfile::ALL
                                .into_iter()
                                .map(|profile| (profile.key().into(), profile.label().into()))
                                .collect(),
                            move |cx: &App| description(cx).profile.key().into(),
                            move |key: SharedString, cx: &mut App| {
                                let profile =
                                    settings::description::DescriptionProfile::parse(&key);
                                let config = description(cx);
                                if config.profile != profile {
                                    store(config.with_profile(profile), cx);
                                }
                            },
                        ),
                    )
                    .description(
                        "The levels of description rows can be filed under. Changing it sets what \
                         new rows and imported folders and files default to; rows already filed \
                         keep their level, and their levels stay available.",
                    ),
                )
                .item(
                    level("Imported folders are", true).description(
                        "What a folder brought in by an import is called at its level.",
                    ),
                )
                .item(level("Imported files are", false).description(
                    "What a file brought in by an import, or a new row, is called at its level.",
                ))
        })
        .group(
            divided_group(cx).title("Import").item(
                SettingItem::new(
                    "Files already in the project",
                    SettingField::dropdown(
                        options(settings::project::IMPORT_DUPLICATE_POLICIES),
                        |cx: &App| {
                            cx.try_global::<CurrentProject>()
                                .and_then(|p| {
                                    p.data
                                        .values
                                        .get(settings::project::IMPORT_DUPLICATE_POLICY_KEY)
                                })
                                .map(|v| v.text())
                                .unwrap_or_else(|| "skip".into())
                        },
                        |policy: SharedString, cx: &mut App| {
                            CurrentProject::set_text(
                                settings::project::IMPORT_DUPLICATE_POLICY_KEY,
                                policy,
                                cx,
                            );
                        },
                    ),
                )
                .description(
                    "What importing a file does when a row already links to it or names it.",
                ),
            )
            .item(
                SettingItem::new(
                    "Files from outside the files folder",
                    SettingField::dropdown(
                        options(settings::project::IMPORT_OUTSIDE_FILES),
                        |cx: &App| {
                            cx.try_global::<CurrentProject>()
                                .and_then(|p| {
                                    p.data
                                        .values
                                        .get(settings::project::IMPORT_OUTSIDE_FILES_KEY)
                                })
                                .map(|v| v.text())
                                .unwrap_or_else(|| "ask".into())
                        },
                        |choice: SharedString, cx: &mut App| {
                            CurrentProject::set_text(
                                settings::project::IMPORT_OUTSIDE_FILES_KEY,
                                choice,
                                cx,
                            );
                        },
                    ),
                )
                .description(
                    "What dropping a file that is not in the files folder does. A copy goes into \
                     the folder's imported subfolder; a link keeps the file where it is and is \
                     listed in Problems.",
                ),
            ),
        )
}

/// Writing everything on this page back out as the `column_config.csv` the wizard reads, so the
/// next collection starts configured instead of starting again.
fn config_file_group(cx: &App) -> SettingGroup {
    divided_group(cx).title("Config file").item(
        SettingItem::new(
            "Export",
            SettingField::element(|_opts: &_, _window: &mut _, _cx: &mut _| {
                Button::new("export-column-config")
                    .small()
                    .label("Export…")
                    .on_click(|_, _, cx| config::export(cx))
                    .into_any_element()
            }),
        )
        .description(
            "Save this project's columns — types, descriptions, authorities, and whatever active \
             plugins map — as a CSV. Load it in the New Project wizard to set another collection \
             up the same way.",
        ),
    )
}

/// The one account any authority check needs. A group on Columns rather than a page of its own:
/// what a column is checked against is a per-column knob like its type or its filter, and splitting
/// the two only made "where do I configure this column" a two-page question.
fn authority_accounts_group(cx: &App) -> SettingGroup {
    let described = checks::authorities()
        .iter()
        .map(|(name, describes)| format!("{name} — {describes}"))
        .collect::<Vec<_>>()
        .join("\n\n");

    divided_group(cx)
        .title("Authority accounts")
        .description(SharedString::from(described))
        .item(
            Setting::TextWithAction {
                key: checks::GEONAMES_USERNAME_KEY,
                label: "GeoNames account",
                description: "GeoNames refuses anonymous requests, so its check stays off \
                              until this is filled in. A free account at geonames.org is \
                              enough — this is a username, not a password.",
                on_change: revalidate_authority_setting,
            }
            .into_item(cx),
        )
}

/// Which authority each column is checked against.
fn authority_columns_group(project: &CurrentProject, cx: &App) -> SettingGroup {
    let options: Vec<OptionItem> = std::iter::once(OptionItem {
        value: SharedString::default(),
        label: "Not checked".into(),
    })
    .chain(
        checks::authorities()
            .into_iter()
            .map(|(name, _)| OptionItem {
                value: name.into(),
                label: name.into(),
            }),
    )
    .collect();

    // Read once for the whole page — see the note in `mapping_group`.
    let stored = columns::load(cx);
    let mut group = divided_group(cx).title("Checked against").description(
        "A column is checked against one list. Values are checked over the network, so a wrong \
         one is flagged once the answer arrives rather than as you type.",
    );
    for column in column_items(project) {
        let picked: SharedString = stored
            .get(column.key.as_ref())
            .and_then(|s| s.authority.clone())
            .map(SharedString::from)
            .unwrap_or_default();
        let warns = warns(stored.get(column.key.as_ref()), &picked);
        let options = options.clone();
        group = group.item(SettingItem::new(
            column.name.clone(),
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                authority_picker(
                    column.clone(),
                    options.clone(),
                    picked.clone(),
                    warns,
                    window,
                    cx,
                )
            }),
        ));
    }
    group
}

/// Whether `producer`'s findings on this column are set to warn. Absent reads as error, which is
/// what every built-in check reports when nothing overrides it.
fn warns(settings: Option<&columns::ColumnSettings>, producer: &str) -> bool {
    settings.is_some_and(|s| s.severity.get(producer).is_some_and(|key| key == "warning"))
}

/// How loud one producer's findings are on one column. Two icons rather than words — the same two
/// the Problems panel draws, so the switch shows its own effect.
fn severity_switch(column: SharedString, producer: SharedString, warns: bool) -> impl IntoElement {
    TabBar::new(SharedString::from(format!("severity/{producer}/{column}")))
        .segmented()
        .selected_index(usize::from(warns))
        .on_click(move |ix: &usize, _window, cx: &mut App| {
            let chosen = if *ix == 1 { "warning" } else { "error" };
            columns::update(
                &column,
                |s| {
                    s.severity.insert(producer.to_string(), chosen.into());
                },
                cx,
            );
            // The severity is applied where findings are addressed, so only a re-run republishes
            // them at the new one.
            table::revalidate_now(cx);
        })
        .child(Tab::new().icon(IconName::CircleX))
        .child(Tab::new().icon(IconName::TriangleAlert))
}

/// One column's authority choice. Single-select, because a value comes from one list or none —
/// two lists would mean a value has to be on both, which is not what anybody means by it.
fn authority_picker(
    column: ColumnItem,
    options: Vec<OptionItem>,
    picked: SharedString,
    warns: bool,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = picker(
        |pickers| &mut pickers.maps,
        format!("authority/{}", column.key),
        options.clone(),
        PickerKind {
            multiple: false,
            searchable: false,
        },
        {
            let key = column.key.clone();
            move |values: &[SharedString], cx: &mut App| {
                let chosen = values
                    .first()
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string);
                columns::update(&key, |s| s.authority = chosen.clone(), cx);
                // The check reads this setting, so what it published is stale the moment it
                // changes — and only a run replaces it.
                table::revalidate_now(cx);
            }
        },
        window,
        cx,
    );
    sync_selection(&state, &options, std::slice::from_ref(&picked), window, cx);

    let label = options
        .iter()
        .find(|o| o.value == picked)
        .map(|o| o.label.clone())
        .unwrap_or_else(|| "Not checked".into());
    h_flex()
        .gap_2()
        // Nothing is checked, so there is no severity to set — the switch would govern a producer
        // that isn't running.
        .when(!picked.is_empty(), |row| {
            row.child(severity_switch(column.key.clone(), picked.clone(), warns))
        })
        .child(Combobox::new(&state).small().placeholder(label))
        .into_any_element()
}

/// The open project's data columns. A column's key is its header name — the same identity the
/// table mints in `set_data`, so it survives reordering, reopening, and columns coming and going.
fn column_items(project: &CurrentProject) -> Vec<ColumnItem> {
    project
        .data
        .headers
        .iter()
        .map(|name| ColumnItem {
            key: SharedString::from(name.clone()),
            name: SharedString::from(name.clone()),
        })
        .collect()
}

/// Trigger label for the picker: "First +N others", or "No columns" when nothing is selected.
fn picker_label(selected: &[SharedString]) -> SharedString {
    match selected.len() {
        0 => "No columns".into(),
        1 => selected[0].clone(),
        n => format!(
            "{} +{} other{}",
            selected[0],
            n - 1,
            if n - 1 == 1 { "" } else { "s" }
        )
        .into(),
    }
}

/// A multi-select over the project's columns. `on`/`set` are the only difference between every
/// picker built on it — one property's getter and setter, so filters, spell checking, and each data
/// type stay one component rather than three copies. `Rc` because `on` is needed both inside the
/// `Change` subscription and outside it to draw the current selection.
///
/// The stored value stays the source of truth: selection is synced *from* it here and written
/// *back* in the subscription, so nothing has to reconcile two answers.
fn column_picker(
    id: &'static str,
    headers: Vec<ColumnItem>,
    (on, set): (ReadsColumns, WritesColumns),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let state = picker(
        |pickers| &mut pickers.columns,
        id.to_string(),
        headers.clone(),
        PickerKind {
            multiple: true,
            searchable: true,
        },
        {
            let on = on.clone();
            move |values: &[SharedString], cx: &mut App| {
                let started = std::time::Instant::now();
                // Re-read the columns rather than capturing them: this closure outlives the
                // project whose headers built it.
                let Some(current) = cx.try_global::<CurrentProject>().map(column_items) else {
                    return;
                };
                let chosen = on(&current, cx);
                let changes: Vec<(&ColumnItem, bool)> = current
                    .iter()
                    .filter_map(|column| {
                        let want = values.contains(&column.key);
                        (chosen.contains(&column.key) != want).then_some((column, want))
                    })
                    .collect();
                if changes.is_empty() {
                    return;
                }
                set(&changes, cx);
                // A validator reads these settings, so its published findings are stale the
                // moment one changes — and only a run replaces them.
                table::revalidate_now(cx);
                log::debug!("column picker {id:?} applied in {:?}", started.elapsed());
            }
        },
        window,
        cx,
    );

    let chosen = on(&headers, cx);
    let picked: Vec<SharedString> = headers
        .iter()
        .filter(|c| chosen.contains(&c.key))
        .map(|c| c.key.clone())
        .collect();
    sync_selection(&state, &headers, &picked, window, cx);

    let label = picker_label(
        &headers
            .iter()
            .filter(|c| picked.contains(&c.key))
            .map(|c| c.name.clone())
            .collect::<Vec<_>>(),
    );

    Combobox::new(&state)
        .small()
        .menu_width(px(240.))
        .menu_max_h(px(320.))
        .search_placeholder("Search columns…")
        .empty(|_, _| div().p_2().child("No columns in this project"))
        .render_trigger(move |_ctx, _, _| div().child(label.clone()))
        .into_any_element()
}
