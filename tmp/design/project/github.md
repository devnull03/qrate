repo: devnull03/qrate
branch: main

## Last sync
date: 2026-10-02T19:30:13Z

### Updated in this project
- Added image annotations mockups: viewer annotate mode, Notes and History panels, composer, gallery badge

## Sync history
- 2026-09-25T05:28:07Z: launcher, wizard and workspace chrome recreated; onboarding mockups added

## Screen map
| Screen | Repo files |
|---|---|
| Launcher (first-run, returning) | crates/project-wizard/src/launcher.rs, crates/app/src/app_menus.rs, assets/themes/qrate.json |
| Wizard shell + stepper + footer | crates/project-wizard/src/wizard.rs |
| Files step | crates/project-wizard/src/steps/files.rs |
| Columns step | crates/project-wizard/src/steps/columns.rs |
| Create review | crates/project-wizard/src/steps/review.rs |
| Workspace (docks, views, gallery) | crates/workspace/src/lib.rs, crates/workspace/src/views/mod.rs, crates/window-wrapper/src/title_bar.rs, crates/window-wrapper/src/status_bar.rs, crates/app/src/title_items/mod.rs |
| Image annotations (viewer pills, page strip, note editor) | crates/workspace/src/viewer/mod.rs, crates/table/src/note.rs |
| Details / Problems panels | crates/workspace/src/panels/details.rs, docs/assets/final-report/diagnostics.png |
