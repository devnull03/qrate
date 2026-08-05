use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Axis, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _,
    button::Button,
    h_flex,
    popover::Popover,
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
            Setting::Text {
                key: spellcheck::SPELLCHECK_LANGUAGE_KEY,
                label: "Language",
                description: "A dictionary name, e.g. \"en_US\". English ships with qrate; for any \
                              other language put its Hunspell .aff and .dic files in the \
                              \"dictionaries\" folder beside your logs. Takes effect on restart.",
            }
            .into(),
        );
    }
    group
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

    // `c{ix}` is the column's index into the on-disk header order — the same identity the table
    // mints in `set_data`, so it survives reordering and reopening.
    let headers: Vec<(String, SharedString)> = project
        .data
        .headers
        .iter()
        .enumerate()
        .map(|(ix, name)| (format!("c{ix}"), SharedString::from(name.clone())))
        .collect();

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
                SettingField::element(move |_opts: &_, _window: &mut _, cx: &mut _| {
                    let on: fn(&columns::ColumnSettings) -> bool = |s| s.filter_enabled;
                    column_picker(
                        "filtered-columns",
                        picked.clone(),
                        picker_label(&picked, on, cx),
                        on,
                        |s| s.filter_enabled = !s.filter_enabled,
                    )
                }),
            )
            .description("Columns that show a filter dropdown."),
        );
    }

    group = group.item(
        SettingItem::new(
            "Spell-checked columns",
            SettingField::element(move |_opts: &_, _window: &mut _, cx: &mut _| {
                let on: fn(&columns::ColumnSettings) -> bool = |s| s.spellcheck;
                column_picker(
                    "spellchecked-columns",
                    headers.clone(),
                    picker_label(&headers, on, cx),
                    on,
                    |s| s.spellcheck = !s.spellcheck,
                )
            }),
        )
        .description("Columns whose text is spell-checked. New columns start checked."),
    );
    SettingPage::new("Columns").group(group)
}

/// Trigger label for the picker: "First +N others", or "No columns" when nothing is selected.
fn picker_label(
    headers: &[(String, SharedString)],
    on: fn(&columns::ColumnSettings) -> bool,
    cx: &App,
) -> SharedString {
    let selected: Vec<&SharedString> = headers
        .iter()
        .filter(|(key, _)| on(&columns::get(key, cx)))
        .map(|(_, name)| name)
        .collect();
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

/// The picker trigger ("First +N others") and its checklist. A popover, not a `Dropdown`, so the
/// list scrolls and clicking a row toggles that column without dismissing.
///
/// `on`/`toggle` are the only difference between the two pickers on this page — one field's
/// getter and setter, passed as plain `fn`s so both stay one component rather than two copies.
///
/// Takes the label already resolved rather than an `&App`: in edition 2024 a returned
/// `impl IntoElement` captures every input lifetime, and a borrowed context outlives the closure
/// gpui hands this to.
fn column_picker(
    id: &'static str,
    headers: Vec<(String, SharedString)>,
    label: SharedString,
    on: fn(&columns::ColumnSettings) -> bool,
    toggle: fn(&mut columns::ColumnSettings),
) -> impl gpui::IntoElement {
    Popover::new(id)
        .trigger(
            Button::new(SharedString::from(format!("{id}-btn")))
                .label(label)
                .outline()
                .xsmall(),
        )
        .content(move |_state, _window, cx| {
            // Match gpui-component's own menu items: text_sm rows, accent highlight on hover, a
            // right-aligned check marking selection (see `menu/popup_menu.rs`).
            let (accent, accent_fg, muted) = (
                cx.theme().accent,
                cx.theme().accent_foreground,
                cx.theme().muted_foreground,
            );
            let radius = cx.theme().radius;
            let rows = headers.clone();
            v_flex()
                .id(SharedString::from(format!("{id}-list")))
                .w(px(240.))
                .max_h(px(320.))
                .p_1()
                // `overflow_y_scroll` only — `overflow_hidden` would set *both* axes and undo it.
                .overflow_y_scroll()
                .when(rows.is_empty(), |list| {
                    list.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_sm()
                            .text_color(muted)
                            .child("No columns in this project"),
                    )
                })
                .children(rows.into_iter().map(move |(key, name)| {
                    let checked = on(&columns::get(&key, cx));
                    let toggle_key = key.clone();
                    h_flex()
                        .id(SharedString::from(format!("{id}-pick-{key}")))
                        .h(px(26.))
                        .px(px(8.))
                        .gap_x_1()
                        .rounded(radius)
                        .items_center()
                        .justify_between()
                        .text_sm()
                        .cursor_pointer()
                        .hover(|r| r.bg(accent).text_color(accent_fg))
                        .child(name)
                        .when(checked, |r| r.child(Icon::new(IconName::Check).xsmall()))
                        .on_click(move |_, _, cx: &mut App| {
                            columns::update(&toggle_key, toggle, cx);
                        })
                }))
        })
}
