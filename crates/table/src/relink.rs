//! The disk side of keeping rows linked: how many linked names a candidate folder holds, which
//! missing files sit beside one the archivist located, and copying a file into the files folder.
//! Everything here blocks on the file system, so callers run it off the UI thread.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::photos::PhotoIndex;

/// The subfolder of the files folder that files copied in from elsewhere land in.
pub const IMPORTED: &str = "imported";

/// What relinking to `chosen` would find, before anything is committed.
#[derive(Debug, PartialEq, Eq)]
pub struct Preview {
    pub chosen: PathBuf,
    pub found: usize,
    pub total: usize,
    /// When `chosen` holds none of the names: the nearby folder holding the most, and how many.
    pub instead: Option<(PathBuf, usize)>,
}

/// How many of `names` (see `file_links::folder_names`) `chosen` holds. With none there, the
/// chosen folder's parent and its other subfolders are tried, the narrowest best one winning.
pub fn preview(chosen: &Path, names: &[String]) -> Preview {
    let index = PhotoIndex::build(&chosen.to_string_lossy());
    let found = count(names, |name| index.resolve_cell(name).is_some());
    let instead = match (found, names.is_empty()) {
        (0, false) => nearby(chosen, names),
        _ => None,
    };
    Preview {
        chosen: chosen.to_path_buf(),
        found,
        total: names.len(),
        instead,
    }
}

fn count(names: &[String], found: impl Fn(&str) -> bool) -> usize {
    names.iter().filter(|name| found(name)).count()
}

/// The parent of `chosen` and each of the parent's subfolders, from one walk of the parent.
fn nearby(chosen: &Path, names: &[String]) -> Option<(PathBuf, usize)> {
    let parent = chosen.parent()?;
    let inventory = file_ingest::scan(parent, true).ok()?;
    let files: Vec<&Path> = inventory
        .files()
        .map(|entry| entry.relative_path.as_path())
        .collect();
    let subfolders = inventory
        .entries
        .iter()
        .filter(|entry| entry.kind == file_ingest::EntryKind::Directory && entry.parent.is_none())
        .map(|entry| entry.relative_path.clone())
        .filter(|relative| parent.join(relative) != chosen);
    std::iter::once(PathBuf::new())
        .chain(subfolders)
        .map(|folder| {
            let index = qrate_export::PhotoIndex::from_paths(
                files
                    .iter()
                    .filter_map(|file| file.strip_prefix(&folder).ok())
                    .map(Path::to_path_buf),
            );
            let found = count(names, |name| index.resolve_cell(name).is_some());
            (parent.join(folder), found)
        })
        // `max_by_key` keeps the last of equals, so a subfolder beats the parent holding the same.
        .max_by_key(|(_, found)| *found)
        .filter(|(_, found)| *found > 0)
}

/// Which of `missing` (as `(row, col, value)`) name a file sitting in `folder` itself, as
/// `(row, col, that file)`.
pub fn beside(folder: &Path, missing: &[(usize, usize, String)]) -> Vec<(usize, usize, PathBuf)> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };
    let names: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| PathBuf::from(entry.file_name()))
        .filter(|name| !name.to_str().is_some_and(file_ingest::is_ignored))
        .collect();
    let index = qrate_export::PhotoIndex::from_paths(names);
    missing
        .iter()
        .filter_map(|(row, col, value)| {
            Some((*row, *col, folder.join(index.resolve_cell(value.trim())?)))
        })
        .collect()
}

/// The text a Filename cell and `source_path` hold for `path`: relative to the files folder when
/// it is inside it, the absolute path otherwise.
pub fn link_text(folder: &Path, path: &Path) -> String {
    file_ingest::normalized_path(
        &qrate_export::relative_to(folder, path)
            .filter(|relative| !relative.as_os_str().is_empty())
            .unwrap_or_else(|| path.to_path_buf()),
    )
}

/// Copy `source` (a file or a folder) into `<folder>/imported/`, never over anything already
/// there: a taken name gets " (2)", " (3)", … before its extension. Returns where it landed.
pub fn copy_in(folder: &Path, source: &Path) -> io::Result<PathBuf> {
    let into = folder.join(IMPORTED);
    fs::create_dir_all(&into)?;
    let name = source
        .file_name()
        .ok_or_else(|| io::Error::other(format!("{} has no file name", source.display())))?;
    let mut file = match source.is_dir() {
        true => None,
        false => Some(fs::File::open(source)?),
    };
    for n in 1.. {
        let target = into.join(numbered(Path::new(name), n, file.is_none()));
        let claimed = match &mut file {
            None => fs::create_dir(&target).and_then(|()| copy_tree(source, &target)),
            Some(file) => fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
                .and_then(|mut copy| io::copy(file, &mut copy))
                .map(drop),
        };
        match claimed {
            Ok(()) => return Ok(target),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    unreachable!("an unbounded counter always finds a free name")
}

/// `name` with " (n)" before its extension (after the whole name for a folder); `n` 1 is `name`.
fn numbered(name: &Path, n: usize, is_dir: bool) -> PathBuf {
    if n == 1 {
        return name.to_path_buf();
    }
    let whole = name.to_string_lossy();
    match (is_dir, name.file_stem(), name.extension()) {
        (false, Some(stem), Some(ext)) => {
            format!("{} ({n}).{}", stem.to_string_lossy(), ext.to_string_lossy()).into()
        }
        _ => format!("{whole} ({n})").into(),
    }
}

/// Copy a folder's contents into the freshly created `target`, skipping folder symlinks the way
/// the import walk does.
fn copy_tree(source: &Path, target: &Path) -> io::Result<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let (from, to) = (entry.path(), target.join(entry.file_name()));
        let kind = entry.file_type()?;
        if kind.is_dir() {
            fs::create_dir(&to)?;
            copy_tree(&from, &to)?;
        } else if from.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use crate::relink::{beside, copy_in, link_text, preview};

    fn tempdir(case: &str) -> PathBuf {
        let dir = std::env::temp_dir().join("qrate-relink-test").join(case);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "x").unwrap();
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_preview_counts_the_names_the_chosen_folder_holds() {
        let dir = tempdir("count");
        touch(&dir.join("1.jpg"));
        touch(&dir.join("nested").join("2.jpg"));
        let found = preview(&dir, &names(&["1.jpg", "2.jpg", "3.jpg"]));
        assert_eq!((found.found, found.total), (2, 3));
        assert_eq!(found.instead, None, "only an empty answer looks elsewhere");
    }

    /// The archivist picked one folder too deep, or the wrong sibling: the parent and its other
    /// subfolders are tried, and the narrowest folder holding the most wins.
    #[test]
    fn with_none_found_a_sibling_or_the_parent_is_offered() {
        let dir = tempdir("fallback");
        fs::create_dir_all(dir.join("empty")).unwrap();
        touch(&dir.join("scans").join("1.jpg"));
        touch(&dir.join("scans").join("2.jpg"));
        touch(&dir.join("other").join("3.jpg"));
        let wanted = names(&["1.jpg", "2.jpg", "3.jpg"]);

        let found = preview(&dir.join("empty"), &wanted);
        assert_eq!(found.found, 0);
        assert_eq!(
            found.instead,
            Some((dir.clone(), 3)),
            "the parent holds all three"
        );

        let found = preview(&dir.join("empty"), &names(&["1.jpg", "2.jpg"]));
        assert_eq!(
            found.instead,
            Some((dir.join("scans"), 2)),
            "a subfolder holding as many as the parent is the narrower answer"
        );

        let found = preview(&dir.join("empty"), &names(&["gone.jpg"]));
        assert_eq!(found.instead, None, "nothing nearby either");
    }

    #[test]
    fn missing_files_beside_a_located_one_are_found_by_name() {
        let dir = tempdir("beside");
        touch(&dir.join("a.jpg"));
        touch(&dir.join("b.jpg"));
        touch(&dir.join("deeper").join("c.jpg"));
        let missing = vec![
            (1, 0, "b.jpg".to_string()),
            (2, 0, "old/place/c.jpg".to_string()),
            (3, 0, "B".to_string()),
        ];
        assert_eq!(
            beside(&dir, &missing),
            vec![(1, 0, dir.join("b.jpg")), (3, 0, dir.join("b.jpg"))],
            "only the folder itself, not its subfolders"
        );
    }

    #[test]
    fn a_link_is_relative_inside_the_files_folder_and_absolute_outside_it() {
        let root = std::env::temp_dir().join("qrate-link-root");
        assert_eq!(link_text(&root, &root.join("a").join("1.jpg")), "a/1.jpg");
        let outside = std::env::temp_dir().join("elsewhere.jpg");
        assert_eq!(
            link_text(&root, &outside),
            file_ingest::normalized_path(&outside)
        );
    }

    #[test]
    fn a_copy_never_overwrites_and_numbers_the_next_one() {
        let dir = tempdir("copy");
        let (root, source) = (dir.join("files"), dir.join("elsewhere"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("photo.jpg"), "first").unwrap();

        let first = copy_in(&root, &source.join("photo.jpg")).unwrap();
        fs::write(source.join("photo.jpg"), "second").unwrap();
        let second = copy_in(&root, &source.join("photo.jpg")).unwrap();
        let third = copy_in(&root, &source.join("photo.jpg")).unwrap();

        let imported = root.join("imported");
        assert_eq!(first, imported.join("photo.jpg"));
        assert_eq!(second, imported.join("photo (2).jpg"));
        assert_eq!(third, imported.join("photo (3).jpg"));
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "first",
            "never overwritten"
        );
        assert_eq!(fs::read_to_string(&second).unwrap(), "second");
    }

    #[test]
    fn a_copied_folder_keeps_its_contents_and_is_numbered_whole() {
        let dir = tempdir("copy-folder");
        let root = dir.join("files");
        touch(&dir.join("Box.v2").join("inner").join("1.jpg"));
        fs::create_dir_all(&root).unwrap();

        let first = copy_in(&root, &dir.join("Box.v2")).unwrap();
        let second = copy_in(&root, &dir.join("Box.v2")).unwrap();
        assert!(first.join("inner").join("1.jpg").is_file());
        assert_eq!(second, root.join("imported").join("Box.v2 (2)"));
        assert!(second.join("inner").join("1.jpg").is_file());
    }
}
