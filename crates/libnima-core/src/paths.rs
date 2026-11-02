//! Paths, compared the way the OS compares them.

use std::path::{Component, Path, PathBuf};

/// A path made absolute, with its links followed as far as it exists. The part that
/// does not exist yet is kept as written, without any "." or "..".
pub fn resolve(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };

    let mut clean = PathBuf::new();

    for part in absolute.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                clean.pop();
            }
            other => clean.push(other.as_os_str()),
        }
    }

    let mut head = clean.as_path();
    let mut tail = Vec::new();

    loop {
        if let Ok(found) = dunce::canonicalize(head) {
            let mut resolved = found;

            for part in tail.iter().rev() {
                resolved.push(part);
            }

            return resolved;
        }

        match (head.parent(), head.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                head = parent;
            }
            _ => return clean,
        }
    }
}

/// A path with forward slashes, for a project file or a message.
pub fn posix(path: &Path) -> String {
    let text = path.to_string_lossy();

    if cfg!(windows) {
        text.replace('\\', "/")
    } else {
        text.into_owned()
    }
}

fn same_part(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    if cfg!(windows) {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Whether two paths name one place as they are written, ignoring case where the OS does.
pub fn same(a: &Path, b: &Path) -> bool {
    let (a, b): (Vec<_>, Vec<_>) = (a.components().collect(), b.components().collect());

    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(a, b)| same_part(a.as_os_str(), b.as_os_str()))
}

/// The rest of `path` below `root`, or None when it is not there. "" for `root` itself.
pub fn relative_to(path: &Path, root: &Path) -> Option<PathBuf> {
    let mut parts = path.components();

    for wanted in root.components() {
        if !parts
            .next()
            .is_some_and(|part| same_part(part.as_os_str(), wanted.as_os_str()))
        {
            return None;
        }
    }

    Some(parts.as_path().to_path_buf())
}

/// Whether `path` is inside `root`, not `root` itself.
pub fn within(path: &Path, root: &Path) -> bool {
    relative_to(path, root).is_some_and(|rest| !rest.as_os_str().is_empty())
}

/// `path` as a posix path from `root` when it is below it, as a whole posix path when not.
pub fn shown(path: &Path, root: &Path) -> String {
    match relative_to(path, root) {
        Some(rest) if !rest.as_os_str().is_empty() => posix(&rest),
        Some(_) => ".".to_string(),
        None => posix(path),
    }
}

/// A forward slash path from `root` to `path`, going up where it has to. None when the
/// two are on different drives.
pub fn relative(path: &Path, root: &Path) -> Option<String> {
    let path = resolve(path);
    let root = resolve(root);
    let to: Vec<_> = path.components().collect();
    let from: Vec<_> = root.components().collect();
    let mut common = 0;

    while common < to.len()
        && common < from.len()
        && same_part(to[common].as_os_str(), from[common].as_os_str())
    {
        common += 1;
    }

    // Nothing in common, not even the root: another drive.
    if common == 0 {
        return None;
    }

    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );

    Some(if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    })
}

/// A forward slash path from `root` to `path`, or the whole of `path` on another drive.
pub fn relative_or_full(path: &Path, root: &Path) -> String {
    relative(path, root).unwrap_or_else(|| posix(&resolve(path)))
}

/// A path as a manifest writes it, cleaned: forward slashes, no "." and no doubled "/".
pub fn clean_posix(text: &str) -> String {
    let parts: Vec<&str> = text
        .split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();

    if parts.is_empty() {
        ".".to_string()
    } else {
        parts.join("/")
    }
}

/// The last part of a posix path.
pub fn leaf(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The extension of a posix path's last part, with its dot, in lower case, or "".
pub fn suffix(path: &str) -> String {
    let name = leaf(path);

    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => name[index..].to_lowercase(),
        _ => String::new(),
    }
}

/// The order paths are listed in: by their parts, and without case where the OS has none.
pub fn sort_key(path: &str) -> Vec<String> {
    path.split('/')
        .map(|part| {
            if cfg!(windows) {
                part.to_lowercase()
            } else {
                part.to_string()
            }
        })
        .collect()
}

/// A shell's match of one name against a pattern with * and ?, without case.
pub fn name_matches(name: &str, pattern: &str) -> bool {
    fn matches(name: &[char], pattern: &[char]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| matches(&name[skip..], rest)),
            Some(('?', rest)) => !name.is_empty() && matches(&name[1..], rest),
            Some((char, rest)) => name.first() == Some(char) && matches(&name[1..], rest),
        }
    }

    let name: Vec<char> = name.to_lowercase().chars().collect();
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();

    matches(&name, &pattern)
}

/// Every file below a folder, as sorted posix paths from it. Links to folders are not
/// followed, so a walk stays inside the folder and always ends.
pub fn files_below(folder: &Path) -> Vec<String> {
    fn walk(folder: &Path, prefix: &str, found: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };

            let Ok(kind) = entry.file_type() else {
                continue;
            };

            if kind.is_dir() {
                walk(&entry.path(), &relative, found);
            } else if entry.path().is_file() {
                found.push(relative);
            }
        }
    }

    let mut found = Vec::new();
    walk(folder, "", &mut found);
    found.sort_by_key(|path| sort_key(path));

    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_goes_up_where_it_has_to() {
        let root = tempfile::tempdir().unwrap();
        let mdk = root.path().join("MDK");
        let library = root.path().join("Libs/ee24");
        std::fs::create_dir_all(&mdk).unwrap();
        std::fs::create_dir_all(&library).unwrap();

        assert_eq!(relative(&library, &mdk).unwrap(), "../Libs/ee24");
        assert_eq!(relative(&library, root.path()).unwrap(), "Libs/ee24");
        assert_eq!(relative(root.path(), root.path()).unwrap(), ".");
    }

    #[test]
    fn a_path_that_does_not_exist_resolves_as_written() {
        let root = tempfile::tempdir().unwrap();
        let found = resolve(&root.path().join("a/./b/../c.txt"));

        assert!(posix(&found).ends_with("a/c.txt"), "{found:?}");
        assert!(within(&found, &resolve(root.path())));
    }

    #[test]
    fn a_folder_is_not_within_itself() {
        let root = Path::new("/x/y");

        assert!(!within(root, root));
        assert!(within(Path::new("/x/y/z"), root));
        assert!(!within(Path::new("/x/yz"), root));
    }

    #[test]
    fn names_match_the_way_a_shell_matches_them() {
        assert!(name_matches("Proj.ioc", "*.ioc"));
        assert!(name_matches("stm32g4xx_hal_conf.h", "*_hal_conf.h"));
        assert!(name_matches("makefile", "Makefile"));
        assert!(!name_matches("Proj.ioc2", "*.ioc"));
    }

    #[test]
    fn the_suffix_of_a_name() {
        assert_eq!(suffix("src/a.C"), ".c");
        assert_eq!(suffix("src/.hidden"), "");
        assert_eq!(suffix("src/a"), "");
    }
}
