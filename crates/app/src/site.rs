#[cfg(debug_assertions)]
const DEFAULT_ORIGIN: &str = "http://localhost:4321";
#[cfg(not(debug_assertions))]
const DEFAULT_ORIGIN: &str = "https://qrate.dvnl.work";

pub fn url(path: &str) -> String {
    debug_assert!(path.starts_with('/'));
    format!(
        "{}{path}",
        option_env!("QRATE_SITE_ORIGIN").unwrap_or(DEFAULT_ORIGIN)
    )
}

#[cfg(test)]
mod tests {
    #[cfg(debug_assertions)]
    #[test]
    fn development_builds_use_the_local_site() {
        assert_eq!(super::url("/plugins"), "http://localhost:4321/plugins");
    }

    #[cfg(not(debug_assertions))]
    #[test]
    fn release_builds_use_the_production_site() {
        assert_eq!(super::url("/plugins"), "https://qrate.dvnl.work/plugins");
    }
}
