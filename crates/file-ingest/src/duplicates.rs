//! What an import does about material the project already holds.
//!
//! Digitization arrives in batches, so the same folder is dropped again with new files in it. This
//! module decides, without touching a project, which planned components are already components.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{EntryKind, ImportPlan, normalized_path};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DuplicatePolicy {
    /// Leave what is already catalogued alone. Re-importing a batch folder is then a no-op.
    #[default]
    Skip,
    /// Keep the existing row and its typed metadata, re-linking it to the file just imported.
    Update,
    AddAsNew,
}

impl DuplicatePolicy {
    pub fn key(self) -> &'static str {
        match self {
            Self::Skip => "skip",
            Self::Update => "update",
            Self::AddAsNew => "add_as_new",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "update" => Self::Update,
            "add_as_new" => Self::AddAsNew,
            _ => Self::Skip,
        }
    }
}

/// One component the project already holds. `key` is the caller's own identifier — a row id in the
/// open table, a row index in the wizard — and comes back untouched in a [`Resolution`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingComponent {
    pub key: u64,
    /// Absolute path of the file this component is linked to, when it has one.
    pub absolute_source: Option<PathBuf>,
    /// `settings::filenames::lookup_keys` of whatever names its file.
    pub filename_keys: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Resolution {
    Create,
    Skip {
        existing: u64,
    },
    Update {
        existing: u64,
    },
    /// Several existing components could be this file. Never merged automatically.
    Ambiguous {
        candidates: Vec<u64>,
    },
}

/// Where a component belongs once duplicates are resolved: under another planned component, or
/// under one the project already holds, so a second batch lands inside the first batch's series.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parent {
    Planned(usize),
    Existing(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedPlan {
    pub plan: ImportPlan,
    /// Parallel to `plan.components`.
    pub resolutions: Vec<Resolution>,
    /// Parallel to `plan.components`.
    pub parents: Vec<Option<Parent>>,
}

impl ResolvedPlan {
    pub fn created(&self) -> usize {
        self.count(|resolution| matches!(resolution, Resolution::Create))
    }

    pub fn skipped(&self) -> usize {
        self.count(|resolution| matches!(resolution, Resolution::Skip { .. }))
    }

    pub fn updated(&self) -> usize {
        self.count(|resolution| matches!(resolution, Resolution::Update { .. }))
    }

    pub fn ambiguous(&self) -> usize {
        self.count(|resolution| matches!(resolution, Resolution::Ambiguous { .. }))
    }

    /// Components the policy did not leave as new material.
    pub fn duplicates(&self) -> usize {
        self.skipped() + self.updated()
    }

    fn count(&self, matching: impl Fn(&Resolution) -> bool) -> usize {
        self.resolutions
            .iter()
            .filter(|resolution| matching(resolution))
            .count()
    }
}

/// Matches `plan` against what the project already holds.
///
/// `file_keys` is the host's filename-key rule (`settings::filenames::keys`), passed in so this
/// crate stays free of project types and both callers match the same way.
pub fn resolve(
    plan: ImportPlan,
    existing: &[ExistingComponent],
    file_keys: impl Fn(&str) -> Vec<String>,
    policy: DuplicatePolicy,
) -> ResolvedPlan {
    let plan = collapse_repeats(plan);
    let by_source: HashMap<String, u64> = existing
        .iter()
        .filter_map(|component| {
            let source = component.absolute_source.as_deref()?;
            Some((comparable(source), component.key))
        })
        .collect();
    let mut by_key: HashMap<&str, Vec<u64>> = HashMap::new();
    for component in existing {
        for key in &component.filename_keys {
            by_key.entry(key.as_str()).or_default().push(component.key);
        }
    }

    let resolutions: Vec<Resolution> = plan
        .components
        .iter()
        .map(|component| {
            if let Some(existing) = by_source.get(&comparable(&component.absolute_path)) {
                return matched(policy, *existing);
            }
            if component.kind == EntryKind::Directory {
                return Resolution::Create;
            }
            let mut candidates: Vec<u64> = file_keys(&normalized_path(&component.absolute_path))
                .iter()
                .filter_map(|key| by_key.get(key.as_str()))
                .flatten()
                .copied()
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
            match candidates.len() {
                0 => Resolution::Create,
                1 => matched(policy, candidates[0]),
                _ => Resolution::Ambiguous { candidates },
            }
        })
        .collect();

    let parents = plan
        .components
        .iter()
        .map(|component| {
            let parent = component.parent?;
            Some(match &resolutions[parent] {
                Resolution::Skip { existing } | Resolution::Update { existing } => {
                    Parent::Existing(*existing)
                }
                _ => Parent::Planned(parent),
            })
        })
        .collect();

    ResolvedPlan {
        plan,
        resolutions,
        parents,
    }
}

fn matched(policy: DuplicatePolicy, existing: u64) -> Resolution {
    match policy {
        DuplicatePolicy::Skip => Resolution::Skip { existing },
        DuplicatePolicy::Update => Resolution::Update { existing },
        DuplicatePolicy::AddAsNew => Resolution::Create,
    }
}

/// One path reached twice in a single import — a folder dropped alongside a file inside it — is
/// one component, not a question for the archivist.
fn collapse_repeats(plan: ImportPlan) -> ImportPlan {
    let mut kept_at: HashMap<String, usize> = HashMap::new();
    let mut moved_to: Vec<usize> = Vec::with_capacity(plan.components.len());
    let mut components = Vec::with_capacity(plan.components.len());
    for component in plan.components {
        match kept_at.get(&comparable(&component.absolute_path)) {
            Some(kept) => moved_to.push(*kept),
            None => {
                kept_at.insert(comparable(&component.absolute_path), components.len());
                moved_to.push(components.len());
                components.push(component);
            }
        }
    }
    for component in &mut components {
        component.parent = component.parent.map(|parent| moved_to[parent]);
    }
    ImportPlan {
        components,
        warnings: plan.warnings,
    }
}

/// Windows treats paths case-insensitively and qrate stores them with `/`, so compare them that
/// way rather than as raw `Path`s.
fn comparable(path: &Path) -> String {
    normalized_path(path).to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::duplicates::{DuplicatePolicy, ExistingComponent, Parent, Resolution, resolve};
    use crate::{EntryKind, ImportPlan, PlannedComponent, Warning};

    /// The host's rule, reproduced here so the tests do not depend on `settings`.
    fn file_keys(path: &str) -> Vec<String> {
        let lower = path.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower).to_string();
        let stem = name.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(&name);
        vec![name.clone(), stem.to_string()]
    }

    fn component(path: &str, kind: EntryKind, parent: Option<usize>) -> PlannedComponent {
        PlannedComponent {
            title: path.rsplit('/').next().unwrap_or(path).into(),
            absolute_path: PathBuf::from(path),
            source_path: PathBuf::from(path),
            kind,
            parent,
            level_key: "item".into(),
        }
    }

    fn plan(components: Vec<PlannedComponent>) -> ImportPlan {
        ImportPlan {
            components,
            warnings: Vec::new(),
        }
    }

    fn existing(key: u64, source: Option<&str>, keys: &[&str]) -> ExistingComponent {
        ExistingComponent {
            key,
            absolute_source: source.map(PathBuf::from),
            filename_keys: keys.iter().map(|key| (*key).to_string()).collect(),
        }
    }

    #[test]
    fn the_same_stored_file_is_a_duplicate_under_every_policy() {
        let plan = || plan(vec![component("C:/a/one.jpg", EntryKind::File, None)]);
        let existing = [existing(7, Some("c:/A/one.jpg"), &[])];

        for (policy, expected) in [
            (DuplicatePolicy::Skip, Resolution::Skip { existing: 7 }),
            (DuplicatePolicy::Update, Resolution::Update { existing: 7 }),
            (DuplicatePolicy::AddAsNew, Resolution::Create),
        ] {
            let resolved = resolve(plan(), &existing, file_keys, policy);
            assert_eq!(resolved.resolutions, [expected], "{}", policy.key());
        }
    }

    #[test]
    fn one_filename_match_claims_the_row_and_several_stay_ambiguous() {
        let resolved = resolve(
            plan(vec![
                component("C:/drop/one.jpg", EntryKind::File, None),
                component("C:/drop/two.jpg", EntryKind::File, None),
            ]),
            &[
                existing(1, None, &["one.jpg", "one"]),
                existing(2, None, &["two.jpg", "two"]),
                existing(3, None, &["two.jpg", "two"]),
            ],
            file_keys,
            DuplicatePolicy::Skip,
        );
        assert_eq!(
            resolved.resolutions,
            [
                Resolution::Skip { existing: 1 },
                Resolution::Ambiguous {
                    candidates: vec![2, 3]
                }
            ]
        );
        assert_eq!(resolved.ambiguous(), 1);
    }

    #[test]
    fn a_folder_matches_only_its_own_path() {
        let resolved = resolve(
            plan(vec![component(
                "C:/drop/Photographs",
                EntryKind::Directory,
                None,
            )]),
            &[existing(1, None, &["photographs"])],
            file_keys,
            DuplicatePolicy::Skip,
        );
        assert_eq!(resolved.resolutions, [Resolution::Create]);
    }

    #[test]
    fn new_files_land_inside_the_folder_the_project_already_has() {
        let resolved = resolve(
            plan(vec![
                component("C:/drop/Series", EntryKind::Directory, None),
                component("C:/drop/Series/old.jpg", EntryKind::File, Some(0)),
                component("C:/drop/Series/new.jpg", EntryKind::File, Some(0)),
            ]),
            &[
                existing(10, Some("C:/drop/Series"), &[]),
                existing(11, Some("C:/drop/Series/old.jpg"), &[]),
            ],
            file_keys,
            DuplicatePolicy::Skip,
        );
        assert_eq!(
            resolved.resolutions,
            [
                Resolution::Skip { existing: 10 },
                Resolution::Skip { existing: 11 },
                Resolution::Create,
            ]
        );
        // The genuinely new file belongs to the series already in the project, not to a second
        // copy of it.
        assert_eq!(
            resolved.parents,
            [None, Some(Parent::Existing(10)), Some(Parent::Existing(10))]
        );
        assert_eq!((resolved.created(), resolved.duplicates()), (1, 2));
    }

    #[test]
    fn a_path_reached_twice_in_one_import_collapses_without_asking() {
        let mut duplicated = plan(vec![
            component("C:/drop/Series", EntryKind::Directory, None),
            component("C:/drop/Series/one.jpg", EntryKind::File, Some(0)),
            component("C:/drop/Series/one.jpg", EntryKind::File, Some(0)),
        ]);
        duplicated
            .warnings
            .push(Warning::UnreadableDirectory("C:/x".into()));

        let resolved = resolve(duplicated, &[], file_keys, DuplicatePolicy::Skip);
        assert_eq!(resolved.plan.components.len(), 2);
        assert_eq!(resolved.plan.warnings.len(), 1);
        assert_eq!(resolved.created(), 2);
    }
}
