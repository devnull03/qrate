//! Writing the open project's column setup back out as a `column_config.csv`.
//!
//! The same shape `project-wizard`'s `load_column_config` reads, so the file this writes is the
//! file the New Project wizard takes — that round trip is the entire point, and why the header
//! spellings here and the ones it looks for have to stay in step.

use std::path::{Path, PathBuf};

use gpui::App;
use plugin_api::ColumnMapContributions;
use settings::{columns, project::CurrentProject};

/// One value per header, in the order [`headers`] lists them.
type Row = Vec<String>;

/// Ask where to put it, then write it. Everything is read out of `cx` before the dialog opens, so
/// the picked path is all the spawned task still needs.
pub fn export(cx: &mut App) {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return;
    };
    let (directory, headers, rows) = (
        project
            .file
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf),
        headers(cx),
        rows(cx),
    );

    let receiver = cx.prompt_for_new_path(&directory, Some("column_config.csv"));
    cx.spawn(async move |_cx| {
        if let Ok(Ok(Some(path))) = receiver.await
            && let Err(err) = data_exchange::export::write_csv(&path, &headers, &rows)
        {
            log::error!("could not export the column config to {path:?}: {err}");
        }
    })
    .detach();
}

/// The fixed columns, then a mapping and a severity per active plugin. A plugin that is switched
/// off contributed nothing to load, so it contributes no column here either.
///
/// A plugin's columns carry its id — `islandora::Vocabularies` — so two plugins that both call
/// their mapping "Vocabularies" stay apart, and so a reader can see whose column it is.
fn headers(cx: &App) -> Vec<String> {
    let fixed = [
        "Column Name",
        "Data Type",
        "Description",
        "Authority",
        "Spellcheck",
        AUTHORITY_SEVERITY,
    ];
    fixed
        .into_iter()
        .map(str::to_string)
        .chain(
            ColumnMapContributions::all(cx)
                .into_iter()
                .flat_map(|(plugin, spec)| {
                    [
                        format!("{plugin}::{}", spec.label),
                        format!("{plugin}::Severity"),
                    ]
                }),
        )
        .collect()
}

/// Severity is per producer, so it is one column per producer: this one governs whichever list the
/// `Authority` column names, and each plugin gets a `<id>::Severity` of its own.
const AUTHORITY_SEVERITY: &str = "Authority Severity";

fn rows(cx: &App) -> Vec<Row> {
    let Some(project) = cx.try_global::<CurrentProject>() else {
        return Vec::new();
    };
    let stored = columns::load(cx);
    let maps = ColumnMapContributions::all(cx);

    project
        .data
        .headers
        .iter()
        .enumerate()
        .map(|(ix, name)| {
            let key = format!("c{ix}");
            let settings = stored.get(&key);
            let declared = project.data.columns.iter().find(|c| &c.name == name);
            let mut row = vec![
                name.clone(),
                declared.map_or_else(
                    || columns::ColumnType::Text.as_str().to_string(),
                    |c| c.data_type.clone(),
                ),
                declared.map(|c| c.notes.clone()).unwrap_or_default(),
                settings
                    .and_then(|s| s.authority.clone())
                    .unwrap_or_default(),
                if settings.is_none_or(|s| s.spellcheck) {
                    "yes"
                } else {
                    "no"
                }
                .to_string(),
                settings
                    .and_then(|s| s.authority.as_ref().and_then(|a| s.severity.get(a)))
                    .cloned()
                    .unwrap_or_default(),
            ];
            row.extend(maps.iter().flat_map(|(plugin, spec)| {
                [
                    ColumnMapContributions::picked(plugin, spec, settings)
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join("; "),
                    settings
                        .and_then(|s| s.severity.get(plugin.as_ref()))
                        .cloned()
                        .unwrap_or_default(),
                ]
            }));
            row
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use gpui::TestAppContext;
    use plugin_api::{ColumnMapContributions, ColumnMapSpec};

    fn spec(label: &str) -> ColumnMapSpec {
        ColumnMapSpec {
            key: label.to_lowercase().into(),
            label: label.into(),
            description: None,
            options: "options".into(),
            refresh: "refresh".into(),
            multiple: true,
        }
    }

    /// The header spellings are the contract with `project-wizard`'s importer, and the `id::`
    /// prefix is what keeps two plugins that both call their mapping "Vocabularies" apart.
    #[gpui::test]
    fn a_plugin_gets_a_prefixed_mapping_and_severity_column(cx: &mut TestAppContext) {
        cx.update(|cx| {
            cx.set_global(ColumnMapContributions(vec![
                ("islandora".into(), spec("Vocabularies")),
                ("other".into(), spec("Vocabularies")),
            ]));
            assert_eq!(
                super::headers(cx),
                [
                    "Column Name",
                    "Data Type",
                    "Description",
                    "Authority",
                    "Spellcheck",
                    "Authority Severity",
                    "islandora::Vocabularies",
                    "islandora::Severity",
                    "other::Vocabularies",
                    "other::Severity",
                ]
            );
        });
    }

    /// No plugins loaded means no plugin columns — the file a plain project exports is the file
    /// somebody hand-writes.
    #[gpui::test]
    fn without_plugins_only_the_fixed_columns_are_written(cx: &mut TestAppContext) {
        cx.update(|cx| assert_eq!(super::headers(cx).len(), 6));
    }
}
