mod config;

use std::collections::HashMap;
use std::rc::Rc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Axis, Entity, Global, IntoElement, ParentElement as _,
    SharedString, Styled as _, Subscription, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _,
    button::Button,
    combobox::{Combobox, ComboboxEvent, ComboboxState},
    h_flex,
    input::{Input, InputEvent, InputState},
    searchable_list::{SearchableListItem, SearchableVec},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    tab::{Tab, TabBar},
    v_flex,
};
use plugin_api::{ColumnMapContributions, ColumnMapSpec, CommandContext, PluginHooks};
use plugin_host::{SettingKind, SettingSpec};
use serde_json::Value as Json;
use settings::{Setting, columns, project::CurrentProject};

pub fn build_pages(cx: &App) -> Vec<SettingPage> {
    let mut pages = vec![
        SettingPage::new("Table")
            .group(
                SettingGroup::new().title("Appearance").item(
                    Setting::Switch {
                        key: table::TABLE_STRIPES_KEY,
                        label: "Row Stripes",
                        description: "Alternate row background color in the data table.",
                    }
                    .into(),
                ),
            )
            .group(saving_group(cx))
            .group(previews_group()),
        columns_page(cx),
        authorities_page(cx),
        SettingPage::new("Agent")
            .description(
                "An AI agent you run yourself can read the project open in qrate over a local \
                 connection. It cannot change a cell — it can only stage findings you accept.",
            )
            .group(
                SettingGroup::new().title("Local bridge").item(
                    Setting::Switch {
                        key: crate::agent_bridge::AGENT_BRIDGE_KEY,
                        label: "Allow agents to read this app",
                        description: "Listens on your own machine only, behind a token that \
                                      changes every launch. Switch off to close the port now.",
                    }
                    .into(),
                ),
            ),
        SettingPage::new("Spelling").group(spelling_group(cx)),
        plugins_page(cx),
    ];
    pages.extend(plugin_pages(cx));
    pages
}

/// Every plugin on disk, running or not: whether it runs at all, what it is allowed to reach, and
/// why it did not load when it did not.
///
/// Keyed by the name on disk rather than the descriptor's, because a plugin that fails to load has
/// no descriptor and switching a broken one off has to work.
fn plugins_page(cx: &App) -> SettingPage {
    let listing = plugin_host::listing(cx);
    if listing.is_empty() {
        return SettingPage::new("Plugins").group(
            SettingGroup::new()
                .title("Installed")
                .description("No plugins found. Help ▸ Open Plugins Folder is where they go."),
        );
    }

    listing.into_iter().fold(
        SettingPage::new("Plugins").description(
            "Scripts found in the plugins folder. A plugin is code from somebody else, so it \
             reaches the network only if you say so.",
        ),
        |page, plugin| {
            let id = plugin.id.clone();
            let mut group = SettingGroup::new().title(plugin.id.clone()).item({
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
                    (None, Some(description)) => item.description(description.clone()),
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
                        permission_owner = plugin.id
                    ))),
                );
            }
            page.group(group)
        },
    )
}

/// One page per plugin that declares settings or a column mapping, titled with the plugin's name —
/// the same identity the Problems panel shows. Nothing here knows what any knob means: a spec says
/// where the value lives and how to edit it, and the host does the storing.
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
                    SettingGroup::new().title("Options").items(
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

    let mut group = SettingGroup::new().title("Mapping").item(refresh);

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

/// Autosave as a toggle plus, when on, the method. Both edit the one `AUTOSAVE_KEY` the table reads
/// (`off`/`timed`/`immediate`): the switch is off iff the value is `off`, so an unset value reads as
/// on. The method row only exists while autosave is on — the Settings window observes settings
/// globals (see `SettingsWindow`), so flipping the switch rebuilds this page live.
/// The cache looks after itself — entries are keyed by the file's mtime and size, so a stale one
/// is never served, and it drops its oldest once it passes a couple of gigabytes. The button is
/// for reclaiming the space now rather than for correctness.
fn previews_group() -> SettingGroup {
    SettingGroup::new().title("Previews").item(
        SettingItem::new(
            "Cached thumbnails",
            SettingField::element(|_opts: &_, _window: &mut _, _cx: &mut _| {
                Button::new("clear-preview-cache")
                    .small()
                    .label("Clear cache")
                    .on_click(|_, _, _| match preview::cache::clear() {
                        Ok(count) => log::info!("cleared {count} cached preview thumbnails"),
                        Err(err) => {
                            log::error!("could not clear the preview thumbnail cache: {err}");
                        }
                    })
                    .into_any_element()
            }),
        )
        .description(
            "Downscaled copies of your files, so photos and scans open instantly the second time. \
             They are capped at 2 GB and rebuilt as you browse, so deleting them is safe — it \
             only makes the next look at each file slower.",
        ),
    )
}

fn saving_group(cx: &App) -> SettingGroup {
    let mut group = SettingGroup::new().title("Saving").item(
        SettingItem::new(
            "Autosave",
            SettingField::switch(
                |cx: &App| settings::scoped_text(settings::AUTOSAVE_KEY, cx) != "off",
                |on: bool, cx: &mut App| {
                    settings::set_scoped_text(
                        settings::AUTOSAVE_KEY,
                        if on { "timed" } else { "off" }.into(),
                        cx,
                    );
                },
            ),
        )
        .description("Save cell edits automatically. Ctrl+S always saves."),
    );

    if settings::scoped_text(settings::AUTOSAVE_KEY, cx) != "off" {
        group = group.item(
            SettingItem::new(
                "Method",
                SettingField::dropdown(
                    vec![
                        ("timed".into(), "After a short pause".into()),
                        ("immediate".into(), "On every edit".into()),
                    ],
                    |cx: &App| {
                        let v = settings::scoped_text(settings::AUTOSAVE_KEY, cx);
                        if v == "immediate" { v } else { "timed".into() }
                    },
                    |val: SharedString, cx: &mut App| {
                        settings::set_scoped_text(settings::AUTOSAVE_KEY, val, cx);
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
/// the other per-column knobs. Changing either takes effect on the next launch, since the
/// dictionary is parsed once at startup.
fn spelling_group(cx: &App) -> SettingGroup {
    let mut group = SettingGroup::new().title("Spelling").item(
        SettingItem::new(
            "Check spelling",
            SettingField::switch(
                |cx: &App| spellcheck::enabled(cx),
                |on: bool, cx: &mut App| {
                    settings::set_scoped_bool(spellcheck::SPELLCHECK_ENABLED_KEY, on, cx);
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
                        settings::set_scoped_bool(spellcheck::SPELLCHECK_NAMES_KEY, on, cx);
                    },
                ),
            )
            .description(
                "Skip capitalized words. No dictionary holds every person, place, or studio, so \
                 without this a catalogue of names is mostly underlines. Turn it off to catch \
                 typos that begin with a capital. Takes effect on restart.",
            ),
        );
        group = group.item(
            SettingItem::new(
                "Language",
                SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                    language_picker(window, cx)
                }),
            )
            .description(
                "Canadian and American English are built in. Any other language downloads on \
                 first use and is kept beside your logs. Takes effect on restart.",
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
    state: Entity<ComboboxState<SearchableVec<I>>>,
    /// What the list was last built from. `set_items` replaces the delegate wholesale, which
    /// throws away the active search filter — so it only runs when this actually changed.
    items: Vec<I>,
    _sub: Subscription,
}

/// Whether one column has the property a [`column_picker`] is over, and how to set it.
type ReadsColumn = Rc<dyn Fn(&ColumnItem, &App) -> bool>;
type WritesColumn = Rc<dyn Fn(&ColumnItem, bool, &mut App)>;

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
    state: &Entity<ComboboxState<SearchableVec<I>>>,
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
) -> Entity<ComboboxState<SearchableVec<I>>>
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
                ComboboxState::new(SearchableVec::new(items.clone()), vec![], window, cx)
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
            state.set_items(SearchableVec::new(items.clone()), window, cx);
        });
        if let Some(picker) = slot(cx.global_mut::<Pickers>()).get_mut(&id) {
            picker.items = items;
        }
    }
    state
}

/// The language list, in the shape a phone's language screen uses: everything available, each row
/// saying whether it is already here or a download away, and one tap doing whichever applies.
///
/// Picking a language that isn't here yet starts its download instead of switching to it. Nothing
/// undoes the highlight by hand — the selection re-syncs from the setting on the next render, and
/// the setting did not change.
fn language_picker(window: &mut Window, cx: &mut App) -> AnyElement {
    use spellcheck::catalogue::State;

    // The catalogue is fixed, but download state is not, so the rows change under the picker and
    // `picker` rebuilds its list — which is how a finished download stops showing its arrow.
    let rows = language_rows(cx);
    let state = picker(
        |pickers| &mut pickers.language,
        LANGUAGE.to_string(),
        rows.clone(),
        PickerKind {
            multiple: false,
            searchable: true,
        },
        |values: &[SharedString], cx: &mut App| {
            let Some(code) = values.first().cloned() else {
                return;
            };
            match spellcheck::catalogue::listing()
                .into_iter()
                .find(|((c, _, _), _)| *c == code.as_ref())
                .map(|(_, state)| state)
            {
                Some(State::Available) => spellcheck::start_download(code, cx),
                // Built in and installed both mean "here already"; only the reason differs, and
                // the reason is not the user's problem.
                Some(_) => settings::set_scoped_text(spellcheck::SPELLCHECK_LANGUAGE_KEY, code, cx),
                None => {}
            }
        },
        window,
        cx,
    );
    sync_selection(&state, &rows, &[spellcheck::language(cx)], window, cx);

    Combobox::new(&state)
        .small()
        .menu_width(px(320.))
        .menu_max_h(px(360.))
        .search_placeholder("Search languages…")
        .into_any_element()
}

/// Per-column filters, project-scoped. A master switch gates the feature; when on, a multi-select
/// picker chooses which columns show a filter dropdown (a picked column *is* a filter-enabled one —
/// selection and `filter_enabled` are the same thing now). Built from `&App` because the columns
/// depend on whichever project is open.
fn columns_page(cx: &App) -> SettingPage {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return SettingPage::new("Columns").group(
            SettingGroup::new()
                .title("Filters")
                .description("Open a project to configure its columns."),
        );
    };

    let headers = column_items(project);

    let mut group = SettingGroup::new().title("Filters").item(
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
        group = group.item(
            Setting::Text {
                key: settings::FILTER_SUBDELIMITER_KEY,
                label: "Sub-delimiter",
                description: "Split cells that hold several values, e.g. \";\" for \"Film; Video\", \
                              so the dropdown lists each value on its own. Leave empty to filter \
                              whole cells.",
            }
            .into(),
        );
        let picked = headers.clone();
        group = group.item(
            SettingItem::new(
                "Filtered columns",
                SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                    column_picker(
                        "filtered-columns",
                        picked.clone(),
                        Rc::new(|c: &ColumnItem, cx: &App| columns::get(&c.key, cx).filter_enabled),
                        Rc::new(|c: &ColumnItem, on: bool, cx: &mut App| {
                            columns::update(&c.key, |s| s.filter_enabled = on, cx);
                        }),
                        window,
                        cx,
                    )
                }),
            )
            .description("Columns that show a filter dropdown."),
        );
    }

    let picked = headers.clone();
    group = group.item(
        SettingItem::new(
            "Spell-checked columns",
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                column_picker(
                    "spellchecked-columns",
                    picked.clone(),
                    Rc::new(|c: &ColumnItem, cx: &App| columns::get(&c.key, cx).spellcheck),
                    Rc::new(|c: &ColumnItem, on: bool, cx: &mut App| {
                        columns::update(&c.key, |s| s.spellcheck = on, cx);
                    }),
                    window,
                    cx,
                )
            }),
        )
        .description("Columns whose text is spell-checked. New columns start checked."),
    );

    SettingPage::new("Columns")
        .group(group)
        .group(data_types_group(headers))
        .group(config_file_group())
}

/// What each column holds, as one picker per type. Inverted from the obvious row-per-column shape
/// because a project has far more columns than types, and because a column has exactly one type —
/// which the pickers enforce between them: setting a column here clears it from whichever type
/// held it, since each reads its selection back from the same declared value.
///
/// Unchecking falls back to `Text`, the type that assumes least. It is why `Text`'s own picker
/// cannot be emptied: a column always has a type, and that is the one it has when nothing is said.
fn data_types_group(headers: Vec<ColumnItem>) -> SettingGroup {
    let mut group = SettingGroup::new().title("Data types").description(
        "What a column holds. Checks key off this: dates are validated as EDTF, filenames are \
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
                    Rc::new(move |c: &ColumnItem, cx: &App| declared_type(&c.name, cx) == ty),
                    Rc::new(move |c: &ColumnItem, want: bool, cx: &mut App| {
                        let ty = if want { ty } else { columns::ColumnType::Text };
                        CurrentProject::set_column_type(&c.name, ty.as_str(), cx);
                    }),
                    window,
                    cx,
                )
            }),
        ));
    }
    group
}

/// A column's declared type, read from `__columns` by name. Unconfigured and unrecognised both read
/// as `Text` — the same tolerance [`columns::ColumnType::from_declared`] gives everywhere else.
fn declared_type(name: &str, cx: &App) -> columns::ColumnType {
    cx.try_global::<CurrentProject>()
        .and_then(|p| p.data.columns.iter().find(|c| c.name == name))
        .map_or(columns::ColumnType::Text, |c| {
            columns::ColumnType::from_declared(&c.data_type)
        })
}

/// Writing everything on this page back out as the `column_config.csv` the wizard reads, so the
/// next collection starts configured instead of starting again.
fn config_file_group() -> SettingGroup {
    SettingGroup::new().title("Config file").item(
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

/// Which authority each column is checked against, and the one account any of them needs.
///
/// A page rather than a section of Columns: these leave the machine, which is worth its own
/// heading, and GeoNames' account name has nowhere sensible to live among per-column toggles.
fn authorities_page(cx: &App) -> SettingPage {
    let described = checks::authorities()
        .iter()
        .map(|(name, describes)| format!("**{name}** — {describes}"))
        .collect::<Vec<_>>()
        .join("\n\n");

    let page = SettingPage::new("Authorities").group(
        SettingGroup::new()
            .title("Accounts")
            .description(SharedString::from(described))
            .item(
                Setting::Text {
                    key: checks::GEONAMES_USERNAME_KEY,
                    label: "GeoNames account",
                    description: "GeoNames refuses anonymous requests, so its check stays off \
                                  until this is filled in. A free account at geonames.org is \
                                  enough — this is a username, not a password.",
                }
                .into(),
            ),
    );

    let Some(project) = cx.try_global::<CurrentProject>() else {
        return page.group(
            SettingGroup::new()
                .title("Columns")
                .description("Open a project to choose what its columns are checked against."),
        );
    };

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
    let mut group = SettingGroup::new().title("Columns").description(
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
    page.group(group)
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
    on: ReadsColumn,
    set: WritesColumn,
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
                // Re-read the columns rather than capturing them: this closure outlives the
                // project whose headers built it.
                let Some(current) = cx.try_global::<CurrentProject>().map(column_items) else {
                    return;
                };
                for column in current {
                    let want = values.contains(&column.key);
                    if on(&column, cx) != want {
                        set(&column, want, cx);
                    }
                }
                // A validator reads these settings, so its published findings are stale the
                // moment one changes — and only a run replaces them.
                table::revalidate_now(cx);
            }
        },
        window,
        cx,
    );

    let picked: Vec<SharedString> = headers
        .iter()
        .filter(|c| on(c, cx))
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
