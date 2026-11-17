//! Renaming a batch of files so that either every one moves or none does.

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Why a batch did not happen, and whether the renames already made were put back.
#[derive(Debug)]
pub struct Failed {
    pub from: PathBuf,
    pub to: PathBuf,
    pub error: io::Error,
    /// `false` when undoing the renames already made failed too, so the folder is half-renamed.
    pub restored: bool,
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "could not rename {} to {}: {}",
            self.from.display(),
            self.to.display(),
            self.error
        )?;
        match self.restored {
            true => write!(f, "; no file was renamed"),
            false => write!(f, "; some files could not be put back"),
        }
    }
}

/// Where `relative` lands under `root`, or `None` when it would leave it: an absolute path, a
/// `..` that climbs out, the root itself, or an existing parent folder that links elsewhere.
pub fn inside(root: &Path, relative: &str) -> Option<PathBuf> {
    let mut full = root.to_path_buf();
    let mut depth = 0usize;
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => {
                full.push(part);
                depth += 1;
            }
            Component::CurDir => {}
            Component::ParentDir => {
                depth = depth.checked_sub(1)?;
                full.pop();
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let parent = full.parent().filter(|_| depth > 0)?;
    match (parent.canonicalize(), root.canonicalize()) {
        (Ok(parent), Ok(root)) if !parent.starts_with(&root) => None,
        _ => Some(full),
    }
}

/// Rename every `(from, to)` pair through `rename`, in an order that lets one pair take the name
/// another gives up — a chain or a swap goes through a temporary name beside the file. If any
/// rename fails, those already made are reversed.
pub fn apply(
    pairs: &[(PathBuf, PathBuf)],
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), Failed> {
    let targets: HashSet<&Path> = pairs.iter().map(|(_, to)| to.as_path()).collect();
    let parked = |from: &Path| {
        let name = from.file_name().unwrap_or_default().to_string_lossy();
        from.with_file_name(format!(".{name}.qrate-renaming"))
    };
    let mut steps = Vec::with_capacity(pairs.len());
    for (from, _) in pairs.iter().filter(|(from, _)| targets.contains(&**from)) {
        steps.push((from.clone(), parked(from)));
    }
    for (from, to) in pairs {
        let held = match targets.contains(&**from) {
            true => parked(from),
            false => from.clone(),
        };
        steps.push((held, to.clone()));
    }

    for (done, (from, to)) in steps.iter().enumerate() {
        let moved = match to.exists() {
            true => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "a file with that name already exists",
            )),
            false => rename(from, to),
        };
        if let Err(error) = moved {
            let restored = steps[..done]
                .iter()
                .rev()
                .all(|(from, to)| rename(to, from).is_ok());
            return Err(Failed {
                from: from.clone(),
                to: to.clone(),
                error,
                restored,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{apply, inside};
    use std::fs;
    use std::path::{Path, PathBuf};

    fn folder(case: &str, files: &[&str]) -> PathBuf {
        let dir = std::env::temp_dir().join("qrate-rename-test").join(case);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for file in files {
            fs::write(dir.join(file), file).unwrap();
        }
        dir
    }

    fn pairs(dir: &Path, names: &[(&str, &str)]) -> Vec<(PathBuf, PathBuf)> {
        names
            .iter()
            .map(|(from, to)| (dir.join(from), dir.join(to)))
            .collect()
    }

    fn read(dir: &Path, name: &str) -> String {
        fs::read_to_string(dir.join(name)).unwrap_or_default()
    }

    #[test]
    fn a_swap_and_a_chain_go_through_a_temporary_name() {
        let dir = folder("swap", &["a", "b", "c"]);
        apply(
            &pairs(&dir, &[("a", "b"), ("b", "a"), ("c", "d")]),
            &mut |from, to| fs::rename(from, to),
        )
        .unwrap();
        assert_eq!(read(&dir, "a"), "b");
        assert_eq!(read(&dir, "b"), "a");
        assert_eq!(read(&dir, "d"), "c");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 3, "no temporary left");

        apply(
            &pairs(&dir, &[("a", "b"), ("b", "d"), ("d", "e")]),
            &mut |from, to| fs::rename(from, to),
        )
        .unwrap();
        assert_eq!(read(&dir, "b"), "b");
        assert_eq!(read(&dir, "d"), "a");
        assert_eq!(read(&dir, "e"), "c");
    }

    #[test]
    fn a_failure_on_the_third_of_five_puts_the_first_two_back() {
        let names = ["1", "2", "3", "4", "5"];
        let dir = folder("rollback", &names);
        let renames = [("1", "a"), ("2", "b"), ("3", "c"), ("4", "d"), ("5", "e")];
        let mut calls = 0;
        let failed = apply(&pairs(&dir, &renames), &mut |from, to| {
            calls += 1;
            match calls {
                3 => Err(std::io::Error::other("disk said no")),
                _ => fs::rename(from, to),
            }
        })
        .unwrap_err();
        assert!(failed.restored);
        assert_eq!(failed.from, dir.join("3"));
        for name in names {
            assert_eq!(read(&dir, name), name);
        }
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 5);
    }

    #[test]
    fn a_name_already_taken_by_a_file_outside_the_batch_is_not_overwritten() {
        let dir = folder("taken", &["a", "b"]);
        let failed = apply(&pairs(&dir, &[("a", "b")]), &mut |from, to| {
            fs::rename(from, to)
        })
        .unwrap_err();
        assert_eq!(failed.error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(read(&dir, "b"), "b");
    }

    #[test]
    fn a_path_stays_inside_the_project() {
        let root = folder("inside", &[]);
        assert_eq!(
            inside(&root, "photos/a.jpg"),
            Some(root.join("photos/a.jpg"))
        );
        assert_eq!(inside(&root, "photos/../a.jpg"), Some(root.join("a.jpg")));
        assert_eq!(inside(&root, "../a.jpg"), None);
        assert_eq!(inside(&root, "photos/../../a.jpg"), None);
        assert_eq!(inside(&root, root.join("a.jpg").to_str().unwrap()), None);
        assert_eq!(inside(&root, ""), None);
        assert_eq!(inside(&root, "."), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_folder_does_not_lead_out_of_the_project() {
        let outside = folder("link-outside", &[]);
        let root = folder("link-root", &[]);
        std::os::unix::fs::symlink(&outside, root.join("elsewhere")).unwrap();
        assert_eq!(inside(&root, "elsewhere/a.jpg"), None);
    }
}
