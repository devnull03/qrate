//! Archival description vocabularies used by folder import and hierarchy editing.

use serde::{Deserialize, Serialize};

pub const DESCRIPTION_PROFILE_KEY: &str = "description_profile";
pub const DESCRIPTION_LEVELS_KEY: &str = "description_levels";
pub const FOLDER_LEVEL_KEY: &str = "folder_level_key";
pub const FILE_LEVEL_KEY: &str = "file_level_key";
pub const HIERARCHY_EXPANDED_KEY: &str = "table_hierarchy_expanded";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DescriptionProfile {
    #[default]
    Rad,
    Dacs,
    Isadg,
    Ric,
    Custom,
}

impl DescriptionProfile {
    pub const ALL: [Self; 5] = [Self::Rad, Self::Dacs, Self::Isadg, Self::Ric, Self::Custom];

    pub fn key(self) -> &'static str {
        match self {
            Self::Rad => "rad",
            Self::Dacs => "dacs",
            Self::Isadg => "isadg",
            Self::Ric => "ric",
            Self::Custom => "custom",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Rad => "RAD",
            Self::Dacs => "DACS",
            Self::Isadg => "ISAD(G)",
            Self::Ric => "Records in Contexts",
            Self::Custom => "Custom",
        }
    }

    pub fn parse(value: &str) -> Self {
        let key = qrate_export::description::profile_key(value);
        Self::ALL
            .into_iter()
            .find(|profile| profile.key() == key)
            .unwrap_or_default()
    }

    pub fn defaults(self) -> DescriptionConfig {
        let names = qrate_export::description::profile_levels(self.key());
        let levels = names
            .iter()
            .enumerate()
            .map(|(index, (key, label))| DescriptionLevel {
                key: (*key).into(),
                label: (*label).into(),
                broader_default: index.checked_sub(1).map(|index| names[index].0.into()),
            })
            .collect();
        let (folder_level_key, file_level_key) = match self {
            Self::Ric => ("record_set", "record"),
            Self::Custom => ("group", "item"),
            _ => ("series", "item"),
        };
        DescriptionConfig {
            profile: self,
            levels,
            folder_level_key: folder_level_key.into(),
            file_level_key: file_level_key.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionLevel {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub broader_default: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionConfig {
    pub profile: DescriptionProfile,
    pub levels: Vec<DescriptionLevel>,
    pub folder_level_key: String,
    pub file_level_key: String,
}

impl DescriptionConfig {
    pub fn from_values(values: &std::collections::HashMap<String, crate::Val>) -> Self {
        let profile = values
            .get(DESCRIPTION_PROFILE_KEY)
            .map(|value| DescriptionProfile::parse(&value.text()))
            .unwrap_or_default();
        let mut config = profile.defaults();
        if let Some(levels) = values
            .get(DESCRIPTION_LEVELS_KEY)
            .and_then(|value| serde_json::from_str(&value.text()).ok())
        {
            config.levels = levels;
        }
        if let Some(value) = values.get(FOLDER_LEVEL_KEY) {
            config.folder_level_key = value.text().to_string();
        }
        if let Some(value) = values.get(FILE_LEVEL_KEY) {
            config.file_level_key = value.text().to_string();
        }
        config
    }

    /// This vocabulary moved to `profile`: its levels and import defaults, plus every level of the
    /// old one it lacks, so a row already filed under one keeps a key that resolves to a label.
    /// A level both share keeps the label it had. Rows themselves are not touched.
    pub fn with_profile(&self, profile: DescriptionProfile) -> Self {
        let mut config = profile.defaults();
        for level in &self.levels {
            match config.levels.iter_mut().find(|kept| kept.key == level.key) {
                Some(kept) => kept.label = level.label.clone(),
                None => config.levels.push(level.clone()),
            }
        }
        config
    }

    /// Rename the level `key` names, if this vocabulary has it and the new label is not blank.
    pub fn relabel(&mut self, key: &str, label: &str) {
        if let Some(level) = self.levels.iter_mut().find(|level| level.key == key)
            && !label.trim().is_empty()
        {
            level.label = label.trim().to_string();
        }
    }

    /// The project settings this is stored as, one `__settings` row each.
    pub fn values(&self) -> [(&'static str, String); 4] {
        [
            (DESCRIPTION_PROFILE_KEY, self.profile.key().to_string()),
            (
                DESCRIPTION_LEVELS_KEY,
                serde_json::to_string(&self.levels).unwrap_or_default(),
            ),
            (FOLDER_LEVEL_KEY, self.folder_level_key.clone()),
            (FILE_LEVEL_KEY, self.file_level_key.clone()),
        ]
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.levels.is_empty() {
            return Err("a description vocabulary needs at least one level");
        }
        let mut keys = std::collections::HashSet::new();
        for level in &self.levels {
            if level.key.trim().is_empty() || level.label.trim().is_empty() {
                return Err("description level keys and labels cannot be empty");
            }
            if !keys.insert(level.key.as_str()) {
                return Err("description level keys must be unique");
            }
        }
        if !keys.contains(self.folder_level_key.as_str())
            || !keys.contains(self.file_level_key.as_str())
        {
            return Err("folder and file defaults must name configured levels");
        }
        if self.levels.iter().any(|level| {
            level
                .broader_default
                .as_ref()
                .is_some_and(|key| !keys.contains(key.as_str()))
        }) {
            return Err("broader defaults must name configured levels");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_standard_profile_has_valid_editable_defaults() {
        for profile in DescriptionProfile::ALL {
            let config = profile.defaults();
            assert!(config.validate().is_ok(), "{}", profile.label());
            assert!(!config.levels.is_empty());
        }
    }

    #[test]
    fn project_values_override_vocabulary_and_import_defaults() {
        let mut values = std::collections::HashMap::new();
        values.insert(
            DESCRIPTION_PROFILE_KEY.into(),
            crate::Val::Text("custom".into()),
        );
        values.insert(
            DESCRIPTION_LEVELS_KEY.into(),
            crate::Val::Text(
                serde_json::json!([
                    {"key": "box", "label": "Box"},
                    {"key": "folder", "label": "Folder", "broader_default": "box"}
                ])
                .to_string()
                .into(),
            ),
        );
        values.insert(FOLDER_LEVEL_KEY.into(), crate::Val::Text("box".into()));
        values.insert(FILE_LEVEL_KEY.into(), crate::Val::Text("folder".into()));

        let config = DescriptionConfig::from_values(&values);
        assert_eq!(config.profile, DescriptionProfile::Custom);
        assert_eq!(config.levels[0].label, "Box");
        assert_eq!(config.folder_level_key, "box");
        assert_eq!(config.file_level_key, "folder");
        assert!(config.validate().is_ok());
    }

    /// Switching standards changes what imports default to, never what a row is already filed as:
    /// RAD's `fonds` and `sous_fonds` have no RiC counterpart and must still resolve afterwards.
    #[test]
    fn switching_profile_keeps_levels_rows_may_use() {
        let mut rad = DescriptionProfile::Rad.defaults();
        rad.relabel("series", "Accession");
        let ric = rad.with_profile(DescriptionProfile::Ric);

        assert_eq!(ric.profile, DescriptionProfile::Ric);
        assert_eq!(ric.folder_level_key, "record_set");
        assert_eq!(ric.file_level_key, "record");
        assert_eq!(ric.levels[0].key, "record_set");
        for key in ["fonds", "sous_fonds", "series", "item"] {
            assert!(
                ric.levels.iter().any(|level| level.key == key),
                "{key} kept"
            );
        }
        let series = ric
            .levels
            .iter()
            .find(|level| level.key == "series")
            .unwrap();
        assert_eq!(
            series.label, "Accession",
            "a custom label survives the switch"
        );
        assert!(ric.validate().is_ok());

        let mut values = std::collections::HashMap::new();
        for (key, value) in ric.values() {
            values.insert(key.to_string(), crate::Val::Text(value.into()));
        }
        assert_eq!(DescriptionConfig::from_values(&values), ric);
    }

    #[test]
    fn a_blank_label_does_not_replace_one() {
        let mut config = DescriptionProfile::Dacs.defaults();
        config.relabel("item", "  ");
        config.relabel("missing", "Box");
        assert_eq!(config, DescriptionProfile::Dacs.defaults());
    }
}
