#[cfg(debug_assertions)]
const ORIGIN: &str = "http://localhost:4321";
#[cfg(not(debug_assertions))]
const ORIGIN: &str = "https://qrate.dvnl.work";

pub fn url(path: &str) -> String {
    debug_assert!(path.starts_with('/'));
    format!("{ORIGIN}{path}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn development_builds_use_the_local_site() {
        assert_eq!(super::url("/plugins"), "http://localhost:4321/plugins");
    }
}
