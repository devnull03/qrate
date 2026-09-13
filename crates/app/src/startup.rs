use std::{ffi::OsString, path::PathBuf};

#[derive(Debug, Eq, PartialEq)]
pub enum LaunchTarget {
    Project(PathBuf),
    Link(String),
}

impl LaunchTarget {
    pub fn as_link(&self) -> Option<&str> {
        match self {
            Self::Link(link) => Some(link),
            Self::Project(_) => None,
        }
    }
}

pub fn launch_argument(
    args: impl IntoIterator<Item = OsString>,
) -> Result<Option<LaunchTarget>, String> {
    let mut args = args.into_iter();
    let first = args.next();
    let project = if first.as_deref() == Some(std::ffi::OsStr::new("--")) {
        args.next()
    } else {
        first
    };
    if args.next().is_some() {
        return Err("expected at most one .qrate project file".into());
    }
    let Some(argument) = project else {
        return Ok(None);
    };
    if let Some(link) = argument
        .to_str()
        .filter(|value| value.to_ascii_lowercase().starts_with("qrate://"))
    {
        return Ok(Some(LaunchTarget::Link(link.to_owned())));
    }
    let path = PathBuf::from(argument);
    if path.extension() != Some(std::ffi::OsStr::new("qrate")) {
        return Err(format!(
            "expected a .qrate project file: {}",
            path.display()
        ));
    }
    std::path::absolute(&path)
        .map(LaunchTarget::Project)
        .map(Some)
        .map_err(|error| format!("cannot resolve project {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::{LaunchTarget, launch_argument};

    #[test]
    fn accepts_launcher_and_project() {
        assert_eq!(launch_argument([]).unwrap(), None);
        assert!(matches!(
            launch_argument(["--".into(), "my project.qrate".into()])
            .unwrap()
            .unwrap(),
            LaunchTarget::Project(path) if path.is_absolute()
        ));
        assert!(launch_argument(["a.qrate".into(), "b.qrate".into()]).is_err());
        assert!(matches!(
            launch_argument(["qrate://plugin/install/example".into()]).unwrap(),
            Some(LaunchTarget::Link(link)) if link == "qrate://plugin/install/example"
        ));
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_unicode_path() {
        use std::os::unix::ffi::OsStringExt;
        let file = std::ffi::OsString::from_vec(b"project \xff.qrate".to_vec());
        let LaunchTarget::Project(path) = launch_argument([file.clone()]).unwrap().unwrap() else {
            panic!("expected a project path");
        };
        assert_eq!(path.file_name().unwrap(), file);
    }
}
