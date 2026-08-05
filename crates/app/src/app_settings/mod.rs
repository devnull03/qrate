use std::collections::HashMap;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Axis, Entity, Global, IntoElement, ParentElement as _,
    SharedString, Styled as _, Subscription, Window, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, IndexPath, Sizable as _,
    combobox::{Combobox, ComboboxEvent, ComboboxState},
    h_flex,
    searchable_list::{SearchableListItem, SearchableVec},
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    v_flex,
};
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
            .group(saving_group(cx)),
        columns_page(cx),
        SettingPage::new("Spelling").group(spelling_group(cx)),
    ];
    pages.extend(plugin_pages(cx));
    pages
}

/// One page per plugin that declares settings, titled with the plugin's name — the same identity
/// the Problems panel shows. Nothing here knows what any knob means: a spec says where the value
/// lives and how to edit it, and the host does the storing.
fn plugin_pages(cx: &App) -> Vec<SettingPage> {
    plugin_host::setting_specs(cx)
        .into_iter()
        .map(|(name, description, specs)| {
            let page = SettingPage::new(name.clone()).group(
                SettingGroup::new().title("Options").items(
                    specs
                        .into_iter()
                        .map(|spec| plugin_item(name.clone(), spec)),
                ),
            );
            match description {
                Some(description) => page.description(description),
                None => page,
            }
        })
        .collect()
}

fn plugin_item(id: SharedString, spec: SettingSpec) -> SettingItem {
    let (label, description, kind) = (spec.label.clone(), spec.description.clone(), spec.kind);
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
    columns: HashMap<&'static str, Picker<ColumnItem>>,
    language: Option<Picker<LanguageRow>>,
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

/// One data column, as the two pickers on the Columns page list it. The value is the stable
/// `c{ix}` key, not the display name, so two columns sharing a header stay distinct.
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

/// The language list, in the shape a phone's language screen uses: everything available, each row
/// saying whether it is already here or a download away, and one tap doing whichever applies.
///
/// Picking a language that isn't here yet starts its download instead of switching to it. Nothing
/// undoes the highlight by hand — the selection re-syncs from the setting on the next render, and
/// the setting did not change.
fn language_picker(window: &mut Window, cx: &mut App) -> AnyElement {
    use spellcheck::catalogue::State;

    let rows = language_rows(cx);
    if !cx.has_global::<Pickers>() {
        cx.set_global(Pickers::default());
    }
    let state = match cx.global::<Pickers>().language.as_ref() {
        Some(picker) => picker.state.clone(),
        None => {
            let state = cx.new(|cx| {
                ComboboxState::new(SearchableVec::new(rows.clone()), vec![], window, cx)
                    .searchable(true)
            });
            let _sub = cx.subscribe(&state, |_state, event, cx| {
                let ComboboxEvent::Change(values) = event else {
                    return;
                };
                let Some(code) = values.first().cloned() else {
                    return;
                };
                match spellcheck::catalogue::listing()
                    .into_iter()
                    .find(|((c, _, _), _)| *c == code.as_ref())
                    .map(|(_, state)| state)
                {
                    Some(State::Available) => spellcheck::start_download(code, cx),
                    // Built in and installed both mean "here already"; only the reason differs,
                    // and the reason is not the user's problem.
                    Some(_) => {
                        settings::set_scoped_text(spellcheck::SPELLCHECK_LANGUAGE_KEY, code, cx)
                    }
                    None => {}
                }
            });
            cx.global_mut::<Pickers>().language = Some(Picker {
                state: state.clone(),
                items: rows.clone(),
                _sub,
            });
            state
        }
    };

    // The catalogue is fixed, but download state is not — rebuild so a finished download stops
    // showing its arrow.
    if cx
        .global::<Pickers>()
        .language
        .as_ref()
        .is_some_and(|p| p.items != rows)
    {
        state.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(rows.clone()), window, cx);
        });
        if let Some(picker) = cx.global_mut::<Pickers>().language.as_mut() {
            picker.items = rows.clone();
        }
    }
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
                key: table::FILTER_SUBDELIMITER_KEY,
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
                        |s| s.filter_enabled,
                        |s, on| s.filter_enabled = on,
                        window,
                        cx,
                    )
                }),
            )
            .description("Columns that show a filter dropdown."),
        );
    }

    group = group.item(
        SettingItem::new(
            "Spell-checked columns",
            SettingField::element(move |_opts: &_, window: &mut _, cx: &mut _| {
                column_picker(
                    "spellchecked-columns",
                    headers.clone(),
                    |s| s.spellcheck,
                    |s, on| s.spellcheck = on,
                    window,
                    cx,
                )
            }),
        )
        .description("Columns whose text is spell-checked. New columns start checked."),
    );
    SettingPage::new("Columns").group(group)
}

/// The open project's data columns. `c{ix}` is the column's index into the on-disk header order —
/// the same identity the table mints in `set_data`, so it survives reordering and reopening.
fn column_items(project: &CurrentProject) -> Vec<ColumnItem> {
    project
        .data
        .headers
        .iter()
        .enumerate()
        .map(|(ix, name)| ColumnItem {
            key: SharedString::from(format!("c{ix}")),
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

/// A multi-select over the project's columns. `on`/`set` are the only difference between the two
/// pickers on this page — one field's getter and setter, passed as plain `fn`s so both stay one
/// component rather than two copies.
///
/// The settings map stays the source of truth: selection is synced *from* it here and written
/// *back* in the `Change` subscription, so nothing has to reconcile two answers.
fn column_picker(
    id: &'static str,
    headers: Vec<ColumnItem>,
    on: fn(&columns::ColumnSettings) -> bool,
    set: fn(&mut columns::ColumnSettings, bool),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    if !cx.has_global::<Pickers>() {
        cx.set_global(Pickers::default());
    }
    let state = match cx.global::<Pickers>().columns.get(id) {
        Some(picker) => picker.state.clone(),
        None => {
            let state = cx.new(|cx| {
                ComboboxState::new(SearchableVec::new(headers.clone()), vec![], window, cx)
                    .multiple(true)
                    .searchable(true)
            });
            let _sub = cx.subscribe(&state, move |_state, event, cx| {
                let ComboboxEvent::Change(values) = event else {
                    return;
                };
                // Re-read the columns rather than capturing them: this closure outlives the project
                // whose headers built it.
                let Some(keys) = cx.try_global::<CurrentProject>().map(|p| {
                    column_items(p)
                        .into_iter()
                        .map(|c| c.key)
                        .collect::<Vec<_>>()
                }) else {
                    return;
                };
                for key in keys {
                    let want = values.contains(&key);
                    if on(&columns::get(&key, cx)) != want {
                        columns::update(&key, |s| set(s, want), cx);
                    }
                }
                // A validator reads these settings, so its published findings are stale the moment
                // one changes — and only a run replaces them.
                table::revalidate_now(cx);
            });
            cx.global_mut::<Pickers>().columns.insert(
                id,
                Picker {
                    state: state.clone(),
                    items: headers.clone(),
                    _sub,
                },
            );
            state
        }
    };

    // A project switch is the only thing that changes the column list.
    if cx.global::<Pickers>().columns[id].items != headers {
        state.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(headers.clone()), window, cx);
        });
        if let Some(picker) = cx.global_mut::<Pickers>().columns.get_mut(id) {
            picker.items = headers.clone();
        }
    }

    let picked: Vec<SharedString> = headers
        .iter()
        .filter(|c| on(&columns::get(&c.key, cx)))
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
