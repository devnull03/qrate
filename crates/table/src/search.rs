//! The one predicate shared by free-text find and the per-column value list: a
//! case-insensitive substring test. Deliberately *not* a `Matcher`/`Filter` trait — the whole
//! shared surface between the two features is this single function (see the ASNT-29/ASNT-40
//! notes), and a trait for a `contains` call is exactly the speculative generality CLAUDE.md
//! forbids.

/// Case-insensitive substring test. `needle` must already be lowercased by the caller so a
/// scan lowercases the query once, not once per cell.
pub(crate) fn cell_matches(cell: &str, needle_lower: &str) -> bool {
    needle_lower.is_empty() || cell.to_lowercase().contains(needle_lower)
}

#[cfg(test)]
mod tests {
    use super::cell_matches;

    #[test]
    fn case_insensitive_contains() {
        // `needle` is pre-lowercased by the caller; the cell side is lowercased here.
        assert!(cell_matches("Hello World", "world"));
        assert!(cell_matches("HELLO WORLD", "lo wo"));
        assert!(!cell_matches("Hello", "xyz"));
    }

    #[test]
    fn empty_needle_matches_everything() {
        assert!(cell_matches("", ""));
        assert!(cell_matches("anything", ""));
    }
}
