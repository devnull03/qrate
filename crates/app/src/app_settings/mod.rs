use gpui::{App, SharedString};
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage};
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
/// unlocks its settings. Built from `&App` rather than a fixed list because the columns depend on
/// whichever project is open.
///
/// These use `SettingField` directly rather than the `Setting::Switch` enum — that one's `key` is
/// `&'static str` and routes through the user/project scope helpers, while a column key is
/// computed at runtime and is always project-scoped.
fn columns_page(cx: &App) -> SettingPage {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return SettingPage::new("Columns").group(
            SettingGroup::new()
                .title("No project open")
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
    let label_for = |key: &str| {
        headers
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| SharedString::from(key.to_string()))
    };

    let unadded: Vec<(SharedString, SharedString)> = headers
        .iter()
        .filter(|(key, _)| !tracked.contains(key))
        .map(|(key, name)| (SharedString::from(key.clone()), name.clone()))
        .collect();

    let mut page = SettingPage::new("Columns").group(
        SettingGroup::new()
            .title("Add a column")
            .description("Pick a column to unlock its settings.")
            .item(SettingItem::new(
                "Column",
                SettingField::dropdown(
                    unadded,
                    // Nothing is "selected" — this is an action dressed as a picker, so it always
                    // reads back empty and re-offers every remaining column.
                    |_| SharedString::default(),
                    |key: SharedString, cx: &mut App| {
                        if !key.is_empty() {
                            columns::add(key.as_ref(), cx);
                        }
                    },
                ),
            )),
    );

    // One group per added column. The closures capture the owned key, which is why these can't be
    // the `'static`-keyed `Setting::Switch`.
    for key in tracked {
        let (get_key, set_key, remove_key) = (key.clone(), key.clone(), key.clone());
        page = page.group(
            SettingGroup::new()
                .title(label_for(&key))
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
                )
                .item(
                    SettingItem::new(
                        "Remove",
                        SettingField::switch(
                            |_| false,
                            move |on: bool, cx: &mut App| {
                                if on {
                                    columns::remove(&remove_key, cx);
                                }
                            },
                        ),
                    )
                    .description("Drop this column from the page, discarding its settings."),
                ),
        );
    }
    page
}
