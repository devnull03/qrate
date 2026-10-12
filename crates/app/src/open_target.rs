use std::{ffi::OsStr, path::PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum OpenTarget {
    Project(PathBuf),
    Folder(PathBuf),
    Plugin(String),
}

impl OpenTarget {
    pub(crate) fn parse(argument: &OsStr) -> Option<Self> {
        let path = match argument.to_str() {
            Some(value) if value.starts_with("qrate://") => {
                return match plugin_package::parse_install_link(value) {
                    Ok(_) => Some(Self::Plugin(value.to_owned())),
                    Err(error) => {
                        log::warn!("ignored invalid plugin install link: {error:#}");
                        None
                    }
                };
            }
            Some(value) if value.starts_with("file:") => {
                url::Url::parse(value).ok()?.to_file_path().ok()?
            }
            Some(value) if value.contains("://") => return None,
            _ => PathBuf::from(argument),
        };
        if path.is_dir() {
            Some(Self::Folder(path))
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("qrate"))
        {
            Some(Self::Project(path))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OpenTarget;

    #[test]
    fn os_paths_and_file_urls_reach_the_same_target() {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("Collection with spaces é");
        std::fs::create_dir(&folder).unwrap();
        let project = folder.join("Collection.QRATE");
        for (path, expected) in [
            (&folder, OpenTarget::Folder(folder.clone())),
            (&project, OpenTarget::Project(project.clone())),
        ] {
            assert_eq!(OpenTarget::parse(path.as_os_str()), Some(expected));
            let url = url::Url::from_file_path(path).unwrap();
            assert_eq!(
                OpenTarget::parse(url.as_str().as_ref()),
                OpenTarget::parse(path.as_os_str())
            );
        }
        assert_eq!(OpenTarget::parse("--onboarding".as_ref()), None);
        assert_eq!(
            OpenTarget::parse("https://example.com/collection.qrate".as_ref()),
            None
        );
    }
}
