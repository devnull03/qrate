use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, InteractiveElement as _, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, div, px,
};
use gpui_component::{
    ActiveTheme as _, IconName, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    popover::Popover,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage},
    v_flex,
};
use settings::{Setting, columns, project::CurrentProject};

pub fn build_pages(cx: &App) -> Vec<SettingPage> {
    vec![
        SettingPage::new("Table").group(
            SettingGroup::new().title("Appearance").item(
                Setting::Switch {
                    key: table::TABLE_STRIPES_KEY,
                    label: "Row Stripes",
                    description: "Alternate row background color in the data table.",
                }
                .into(),
            ),
        ),
        columns_page(cx),
    ]
}

/// Per-column settings, project-scoped. The page starts empty: a column is added by name, which
/// is what surfaces its settings. Built from `&App` rather than a fixed list because the columns
/// depend on whichever project is open.
///
/// These use `SettingField`/`SettingItem` directly rather than the `Setting::Switch` enum — that
/// one's `key` is `&'static str` and routes through the user/project scope helpers, while a column
/// key is computed at runtime and is always project-scoped.
fn columns_page(cx: &App) -> SettingPage {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return SettingPage::new("Columns").group(
            SettingGroup::new()
                .title("Add column")
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
    let tracked = columns::tracked(cx);
    let unadded: Vec<(String, SharedString)> = headers
        .iter()
        .filter(|(key, _)| !tracked.contains(key))
        .cloned()
        .collect();

    // Heading + one subtitle row carrying the Add button on its right.
    let mut page = SettingPage::new("Columns").group(SettingGroup::new().title("Add column").item(
        SettingItem::new(
            "Add a column to change its settings",
            SettingField::element(move |_opts: &_, _window: &mut _, _cx: &mut _| {
                add_column_button(unadded.clone())
            }),
        ),
    ));

    for key in tracked {
        let name = headers
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, n)| n.clone())
            .unwrap_or_else(|| SharedString::from(key.clone()));
        let (get_key, set_key, remove_key) = (key.clone(), key.clone(), key.clone());
        page = page.group(
            SettingGroup::new()
                // Column name is the group title so it lists under "Columns" in the sidebar (titled groups only).
                .title(name)
                .item(SettingItem::render(move |_opts, _window, _cx| {
                    let remove_key = remove_key.clone();
                    h_flex().w_full().justify_end().child(
                        Button::new(SharedString::from(format!("remove-{remove_key}")))
                            .icon(IconName::Delete)
                            .label("Remove")
                            .ghost()
                            .xsmall()
                            .on_click(move |_, _, cx: &mut App| {
                                columns::remove(&remove_key, cx);
                            }),
                    )
                }))
                .item(
                    SettingItem::new(
                        "Enable filter",
                        SettingField::switch(
                            move |cx: &App| columns::get(&get_key, cx).filter_enabled,
                            move |on: bool, cx: &mut App| {
                                columns::update(&set_key, |s| s.filter_enabled = on, cx);
                            },
                        ),
                    )
                    .description("Show a filter dropdown in this column's header."),
                ),
        );
    }
    page
}

/// The Add button and its column list. A popover rather than a `Dropdown` so the list can scroll
/// and so the trigger reads as an action rather than a value being picked.
fn add_column_button(unadded: Vec<(String, SharedString)>) -> impl gpui::IntoElement {
    Popover::new("add-column")
        .trigger(
            Button::new("add-column-btn")
                .icon(IconName::Plus)
                .label("Add")
                .outline()
                .xsmall(),
        )
        .content(move |_state, _window, cx| {
            let (hover, muted) = (cx.theme().secondary_hover, cx.theme().muted_foreground);
            let radius = cx.theme().radius;
            let popover = cx.entity();
            let rows: Vec<_> = unadded.clone();
            v_flex()
                .id("add-column-list")
                .w(px(220.))
                .max_h(px(280.))
                // `overflow_y_scroll` only — `overflow_hidden` would set *both* axes and silently
                // undo it.
                .overflow_y_scroll()
                .when(rows.is_empty(), |list| {
                    list.child(
                        div()
                            .px_2()
                            .py_1()
                            .text_color(muted)
                            .child("Every column has been added"),
                    )
                })
                .children(rows.into_iter().map(move |(key, label)| {
                    let popover = popover.clone();
                    div()
                        .id(SharedString::from(format!("add-{key}")))
                        .px_2()
                        .py_1()
                        .rounded(radius)
                        .cursor_pointer()
                        .hover(|r| r.bg(hover))
                        .child(label)
                        .on_click(move |_, window, cx: &mut App| {
                            columns::add(&key, cx);
                            popover.update(cx, |state, cx| state.dismiss(window, cx));
                        })
                }))
        })
}
