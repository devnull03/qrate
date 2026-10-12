use std::path::PathBuf;

use diagnostics::{DATASET_MAIN, Diagnostics, Location, NoteKind, Region};
use gpui::{Entity, TestAppContext};
use gpui_component::table::TableState;
use settings::history::{Change, Origin};

use crate::{QrateTableDelegate, Structural, TablePanel, TableStateHandle};

fn project(
    cx: &mut TestAppContext,
    name: &str,
) -> (PathBuf, Entity<TableState<QrateTableDelegate>>) {
    let folder = std::env::temp_dir().join(format!("qrate-audit-tests-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let file = folder.join(format!("{name}.qrate"));
    let _ = std::fs::remove_file(&file);
    settings::project::create_project_file(
        &file,
        &settings::project::ProjectSpec {
            name: "T",
            headers: &["Title".into()],
            rows: &[vec!["one".into()], vec!["two".into()]],
            ..Default::default()
        },
    )
    .unwrap();
    cx.update(|cx| {
        gpui_component::init(cx);
        let mut app = settings::AppSettings::default();
        app.values.insert(
            settings::AUTOSAVE_KEY.into(),
            settings::Val::Text("off".into()),
        );
        cx.set_global(app);
        cx.set_global(settings::SettingsPersistence::default());
        cx.set_global(settings::project::CurrentProject {
            file: file.clone(),
            data: settings::project::load_project_file(&file).unwrap(),
        });
        diagnostics::init(cx);
    });
    cx.add_window_view(TablePanel::new);
    let state = cx.update(|cx| cx.global::<TableStateHandle>().0.upgrade().unwrap());
    (file, state)
}

fn location(row: usize, row_id: i64) -> Location {
    Location {
        dataset: DATASET_MAIN.into(),
        row: Some(row),
        row_id: Some(row_id),
        column: Some("Title".into()),
    }
}

#[gpui::test]
fn row_delete_undo_restores_notes_and_annotations(cx: &mut TestAppContext) {
    let (file, state) = project(cx, "delete-undo");
    cx.update(|cx| {
        let region = Region {
            page: 0,
            x: 100,
            y: 200,
            w: 300,
            h: 400,
            of: None,
        };
        let note =
            Diagnostics::file_note(location(0, 1), None, None, "note".into(), Origin::Typed, cx)
                .unwrap();
        let annotation = Diagnostics::file_note(
            location(0, 1),
            Some(region),
            Some(NoteKind::Question),
            "annotation".into(),
            Origin::Drawn,
            cx,
        )
        .unwrap();
        crate::structural(Structural::DeleteRows(vec![0]), cx);
        assert!(Diagnostics::note(note, cx).is_none());
        assert!(Diagnostics::note(annotation, cx).is_none());

        crate::history_step(false, cx);
        assert_eq!(state.read(cx).delegate().row_of(1), Some(0));
        assert_eq!(
            Diagnostics::note(note, cx).unwrap().message.as_ref(),
            "note"
        );
        let restored = Diagnostics::note(annotation, cx).unwrap();
        assert_eq!(restored.location, location(0, 1));
        assert_eq!(restored.note.as_ref().unwrap().region, Some(region));
        assert_eq!(
            restored.note.as_ref().unwrap().kind,
            Some(NoteKind::Question)
        );

        Diagnostics::edit_note(note, "revised".into(), None, Origin::Typed, cx);
        crate::history_step(true, cx);
        assert!(Diagnostics::note(note, cx).is_none());
        crate::history_step(false, cx);
        assert_eq!(
            Diagnostics::note(note, cx).unwrap().message.as_ref(),
            "revised"
        );

        Diagnostics::edit_note(note, "".into(), None, Origin::Typed, cx);
        crate::history_step(true, cx);
        crate::history_step(false, cx);
        assert!(
            Diagnostics::note(note, cx).is_none(),
            "a separately deleted note stays deleted"
        );
    });
    let notes = settings::project::read_notes(&file).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].row_id, Some(1));
    assert_eq!(notes[0].message, "annotation");
}

#[gpui::test]
fn redo_insert_restores_notes_written_after_insertion(cx: &mut TestAppContext) {
    let (_, state) = project(cx, "insert-redo");
    cx.update(|cx| {
        crate::structural(Structural::InsertRow { at: 1 }, cx);
        let row_id = state.read(cx).delegate().row_id(1).unwrap();
        let note = Diagnostics::file_note(
            location(1, row_id),
            None,
            None,
            "new".into(),
            Origin::Typed,
            cx,
        )
        .unwrap();
        crate::history_step(false, cx);
        assert!(Diagnostics::note(note, cx).is_none());
        crate::history_step(true, cx);
        assert_eq!(
            Diagnostics::note(note, cx).unwrap().location,
            location(1, row_id)
        );
        assert_eq!(Diagnostics::note(note, cx).unwrap().message.as_ref(), "new");
    });
}

#[gpui::test]
fn restoring_note_follows_saved_and_pending_column_renames(cx: &mut TestAppContext) {
    let (file, _) = project(cx, "restore-renamed-note");
    cx.update(|cx| {
        let note = Diagnostics::file_note(
            location(0, 1),
            None,
            None,
            "original".into(),
            Origin::Typed,
            cx,
        )
        .unwrap();
        Diagnostics::edit_note(note, "edited".into(), None, Origin::Typed, cx);
        let edited = settings::history::entries_after(&file, 0)
            .unwrap()
            .pop()
            .unwrap();
        crate::structural(
            Structural::RenameColumn {
                col: 0,
                name: "Caption".into(),
            },
            cx,
        );
        crate::save_now(cx).unwrap();
        crate::structural(
            Structural::RenameColumn {
                col: 0,
                name: "Label".into(),
            },
            cx,
        );
        Diagnostics::edit_note(note, "".into(), None, Origin::Typed, cx);

        crate::restore_note(&edited.changes[0], edited.id, cx);
        let restored = Diagnostics::note(note, cx).unwrap();
        assert_eq!(restored.message.as_ref(), "original");
        assert_eq!(restored.location.column.as_deref(), Some("Label"));
        assert_eq!(restored.location.row_id, Some(1));
    });
}

#[gpui::test]
fn clearing_history_flushes_edits_and_keeps_queued_saves_from_recreating_it(
    cx: &mut TestAppContext,
) {
    let (file, state) = project(cx, "clear-history");
    cx.update(|cx| {
        crate::write_cell(0, 0, "before clear".into(), Origin::Typed, cx);
        let queued = state.read(cx).delegate().save_snapshot();
        assert!(!queued.history.is_empty());
        crate::clear_history(cx).unwrap();
        assert!(state.read(cx).delegate().unsaved_history().is_empty());
        assert!(settings::history::entries_after(&file, 0).unwrap().is_empty());
        assert_eq!(settings::project::load_project_file(&file).unwrap().rows[0][0], "before clear");

        crate::write_snapshot(&file, &queued).unwrap();
        assert!(settings::history::entries_after(&file, 0).unwrap().is_empty());
        crate::write_cell(0, 0, "after clear".into(), Origin::Typed, cx);
        crate::save_now(cx).unwrap();
        let entries = settings::history::entries_after(&file, 0).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(&entries[0].changes[0], Change::Cell { before, after, .. } if before == "before clear" && after == "after clear"));
    });
}
