use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub enum LaunchTarget {
    Project(PathBuf),
    Link(String),
}

impl LaunchTarget {
    /// The text handed to an already-running qrate, which only accepts UTF-8.
    pub fn handoff(&self) -> Option<&str> {
        match self {
            Self::Link(link) => Some(link),
            Self::Project(path) => path.to_str(),
        }
    }
}

/// The first `qrate://` link or `.qrate` file. Anything else is logged and ignored, because the OS
/// and shells add arguments of their own and a GUI has nowhere to report a refusal.
pub fn launch_argument(args: impl IntoIterator<Item = OsString>) -> Option<LaunchTarget> {
    args.into_iter()
        .filter(|argument| argument != "--")
        .find_map(|argument| {
            let target = parse(&argument);
            if target.is_none() {
                log::warn!("ignored startup argument {}", argument.to_string_lossy());
            }
            target
        })
}

pub fn parse(argument: &std::ffi::OsStr) -> Option<LaunchTarget> {
    if let Some(link) = argument
        .to_str()
        .filter(|value| value.to_ascii_lowercase().starts_with("qrate://"))
    {
        return Some(LaunchTarget::Link(link.to_owned()));
    }
    let path = PathBuf::from(argument);
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("qrate"))
    {
        return None;
    }
    std::path::absolute(&path).ok().map(LaunchTarget::Project)
}

#[cfg(test)]
mod tests {
    use super::{LaunchTarget, launch_argument};

    #[test]
    fn picks_the_first_target_and_ignores_the_rest() {
        assert_eq!(launch_argument([]), None);
        assert!(matches!(
            launch_argument(["--".into(), "my project.QRATE".into()]),
            Some(LaunchTarget::Project(path)) if path.is_absolute()
        ));
        assert!(matches!(
            launch_argument(["-psn_0_1".into(), "a.qrate".into(), "b.qrate".into()]),
            Some(LaunchTarget::Project(path)) if path.ends_with("a.qrate")
        ));
        assert!(matches!(
            launch_argument(["qrate://plugin/install/example".into()]),
            Some(LaunchTarget::Link(link)) if link == "qrate://plugin/install/example"
        ));
        assert_eq!(launch_argument(["notes.txt".into()]), None);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_unicode_path() {
        use std::os::unix::ffi::OsStringExt;
        let file = std::ffi::OsString::from_vec(b"project \xff.qrate".to_vec());
        let Some(LaunchTarget::Project(path)) = launch_argument([file.clone()]) else {
            panic!("expected a project path");
        };
        assert_eq!(path.file_name().unwrap(), file);
    }
}
