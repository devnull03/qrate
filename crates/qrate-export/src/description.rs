//! Archival description profiles and the level vocabulary each one starts with.

/// The canonical key of a stored profile name. Anything unrecognised is RAD, the default.
pub fn profile_key(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "dacs" => "dacs",
        "isadg" | "isad(g)" => "isadg",
        "ric" => "ric",
        "custom" => "custom",
        _ => "rad",
    }
}

/// A profile's preset levels as `(key, label)`, broadest first.
pub fn profile_levels(profile: &str) -> &'static [(&'static str, &'static str)] {
    match profile_key(profile) {
        "dacs" => &[
            ("collection", "Collection"),
            ("record_group", "Record group"),
            ("series", "Series"),
            ("subseries", "Subseries"),
            ("file", "File"),
            ("item", "Item"),
        ],
        "isadg" => &[
            ("fonds", "Fonds"),
            ("subfonds", "Sub-fonds"),
            ("series", "Series"),
            ("subseries", "Sub-series"),
            ("file", "File"),
            ("item", "Item"),
        ],
        "ric" => &[("record_set", "Record set"), ("record", "Record")],
        "custom" => &[("group", "Group"), ("item", "Item")],
        _ => &[
            ("fonds", "Fonds"),
            ("collection", "Collection"),
            ("sous_fonds", "Sous-fonds"),
            ("series", "Series"),
            ("subseries", "Subseries"),
            ("file", "File"),
            ("item", "Item"),
        ],
    }
}
