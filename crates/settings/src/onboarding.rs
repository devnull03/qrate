//! Keys for the workspace's Getting started guide, here because two crates that don't know each
//! other write them: `project-wizard` switches the guide on for a project it has just created, and
//! `workspace` reads and advances it.
//!
//! A project that has no [`GUIDE_KEY`] predates the guide, and gets no first-run card — only the
//! Help ▸ Getting Started entry, which writes the key when it is used.

/// Project setting: `open`, `later` (collapsed to the status bar) or `dismissed`.
pub const GUIDE_KEY: &str = "onboarding_guide";
/// Project setting: which three tasks the guide offers — see [`GuideKind`].
pub const GUIDE_KIND_KEY: &str = "onboarding_kind";
/// Project setting: the tasks already done, comma-separated, so progress survives a restart.
pub const GUIDE_DONE_KEY: &str = "onboarding_done";
/// App setting: the one-time tips already shown, comma-separated. User-wide rather than per
/// project, since what the Problems panel does is learned once.
pub const TIPS_SEEN_KEY: &str = "onboarding_tips_seen";

/// The value of [`GUIDE_KEY`] a new project starts with.
pub const GUIDE_OPEN: &str = "open";

/// What the project was made from, which decides the tasks worth teaching first.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuideKind {
    /// Nothing imported: one empty row to type into.
    Blank,
    /// A spreadsheet with no files linked yet.
    Import,
    /// Rows with linked media.
    Media,
}

impl GuideKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blank => "blank",
            Self::Import => "import",
            Self::Media => "media",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Blank, Self::Import, Self::Media]
            .into_iter()
            .find(|kind| kind.as_str() == text)
    }
}
