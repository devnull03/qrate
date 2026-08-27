//! The one rule for deciding whether a spreadsheet cell names a file on disk.
//!
//! It lives in this leaf crate because two crates have to agree on it: `project_wizard`'s folder
//! check, which decides whether a files folder is accepted at all, and `table::photos`, which
//! resolves each row to a picture and reports the ones it cannot. When those two drifted apart,
//! the wizard accepted a folder and the app then reported every row in it as broken.

use std::path::Path;

/// Every name a file answers to: its filename, its stem, and each separator-truncated prefix of
/// that stem.
///
/// The prefixes are what let a row match a *partial* item — one whose media arrives as
/// `2020_04_001_001.jpg`, `2020_04_001_002.jpg` under the id `2020_04_001`, with any number of
/// its parts missing. Without them a whole shoot of multi-part items reads as "no file here".
/// All keys are lowercased; look them up with a lowercased, trimmed cell value.
pub fn keys(name: &str) -> Vec<String> {
    let lower = name.to_lowercase();
    let stem = Path::new(&lower)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&lower)
        .to_string();
    let mut keys = vec![lower, stem.clone()];
    keys.extend(
        stem.match_indices(['_', '-', '.', ' '])
            .map(|(at, _)| stem[..at].to_string())
            .filter(|p| !p.is_empty()),
    );
    keys
}

/// The names a spreadsheet *cell* may be looked up by, in order: the value itself, its basename
/// (an export writes `2020_04/img.jpg` as readily as `img.jpg`), and that basename's stem (an
/// access derivative rarely keeps the master's extension — the sheet says `.tif`, the folder
/// holds `.jpg`).
///
/// Deliberately not [`keys`]: a file offers up its id prefixes so the parts of one item can be
/// found together, but a cell reading `2020` must not claim the whole shoot.
// ponytail: one filename per cell. Split on a separator here if multi-value file columns
// (`a.jpg|b.jpg`) turn up in real exports.
pub fn lookup_keys(cell: &str) -> Vec<String> {
    let lower = cell.trim().to_lowercase();
    if lower.is_empty() {
        return Vec::new();
    }
    let base = lower
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&lower)
        .to_string();
    let stem = Path::new(&base)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(&base)
        .to_string();

    let mut out = vec![lower.clone()];
    if base != lower {
        out.push(base.clone());
    }
    if stem != base {
        out.push(stem);
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::filenames::{keys, lookup_keys};

    #[test]
    fn a_multi_part_name_answers_to_its_item_id() {
        let k = keys("2020_04_001_002.JPG");
        assert!(k.contains(&"2020_04_001_002.jpg".to_string()));
        assert!(k.contains(&"2020_04_001_002".to_string()));
        assert!(k.contains(&"2020_04_001".to_string()));
        assert!(k.contains(&"2020_04".to_string()));
        assert!(k.contains(&"2020".to_string()));
    }

    #[test]
    fn a_plain_name_answers_to_itself_and_its_stem() {
        assert_eq!(keys("photo1.png"), ["photo1.png", "photo1"]);
    }

    #[test]
    fn a_cell_is_looked_up_by_its_path_tail_and_extensionless_name() {
        assert_eq!(
            lookup_keys(" 2020_04\\IMG_1234.TIF "),
            ["2020_04\\img_1234.tif", "img_1234.tif", "img_1234"]
        );
        assert_eq!(lookup_keys("img_1234"), ["img_1234"]);
        assert!(lookup_keys("   ").is_empty());
    }

    /// A cell offers no prefixes of its own, or every row in a batch would claim its neighbours'
    /// files the moment one id was a prefix of another.
    #[test]
    fn a_cell_does_not_answer_to_its_own_prefixes() {
        assert!(!lookup_keys("2020_04_001.jpg").contains(&"2020_04".to_string()));
    }
}
