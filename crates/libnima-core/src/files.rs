//! Taking a copy of a file before changing it, and reading and writing the project's
//! own files exactly as they are.
//!
//! Every file libNIMA edits is one the user cannot afford to lose: a project file that
//! no longer opens, or a source with their own code in it. So a copy is taken first,
//! beside the file, under a name that never replaces an older copy.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Local;

thread_local! {
    /// The copies the run going on has made, by the file each is of, while a run goes on.
    static RUN: RefCell<Option<HashMap<PathBuf, PathBuf>>> = const { RefCell::new(None) };
}

/// While the guard this returns lives, every copy made is of the file as it was before the run: one a file, however
/// often the run writes it, moving a project over from lib-installer included, and the one the run names.
pub fn run_copies() -> RunCopies {
    RUN.with(|run| *run.borrow_mut() = Some(HashMap::new()));

    RunCopies
}

/// Ends the run's copies when it goes.
pub struct RunCopies;

impl Drop for RunCopies {
    fn drop(&mut self) {
        RUN.with(|run| *run.borrow_mut() = None);
    }
}

/// Copy a file aside before editing it.
///
/// Named with a timestamp rather than a plain .bak, so a second install does not
/// overwrite the copy taken before the first one. In a run, a file already copied is
/// not copied again: the first copy holds it as it was, and is the one returned.
pub fn backup(path: &Path) -> io::Result<PathBuf> {
    let key = crate::paths::resolve(path);
    let earlier = RUN.with(|run| {
        run.borrow()
            .as_ref()
            .and_then(|copies| copies.get(&key).cloned())
    });

    if let Some(copy) = earlier.filter(|copy| copy.is_file()) {
        return Ok(copy);
    }

    let copy = new_copy(path)?;

    RUN.with(|run| {
        if let Some(copies) = run.borrow_mut().as_mut() {
            copies.insert(key, copy.clone());
        }
    });

    Ok(copy)
}

fn new_copy(path: &Path) -> io::Result<PathBuf> {
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut target = path.with_file_name(format!("{name}.{stamp}.bak"));
    let mut count = 1;

    // A library and the one it needs are installed in one run, often inside the same
    // second. The second copy must not replace the first, which is the only one
    // holding the file as it was before either of them.
    while target.exists() {
        count += 1;
        target = path.with_file_name(format!("{name}.{stamp}-{count}.bak"));
    }

    fs::copy(path, &target)?;

    Ok(target)
}

/// The file's text, with its line endings exactly as they are on disk. Text that is
/// not UTF-8 is an error, so a file this cannot read is left as it is.
pub fn read(path: &Path) -> io::Result<String> {
    String::from_utf8(fs::read(path)?)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// A file's text, empty when it is missing or unreadable.
pub fn text_or_empty(path: &Path) -> String {
    read(path).unwrap_or_default()
}

/// Text in a file with whatever it could not read turned into a stand-in, for a file
/// only looked through for a line or a word.
pub fn read_lossy(path: &Path) -> io::Result<String> {
    Ok(String::from_utf8_lossy(&fs::read(path)?).into_owned())
}

/// Write the text back with its line endings untouched.
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    fs::write(path, text.as_bytes())
}

/// Delete a file, a read-only one included.
///
/// Git marks everything under .git/objects read-only, and on Windows that stops a
/// delete outright rather than being advisory as it is on Unix.
pub fn unlink(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            let mut permissions = fs::metadata(path)?.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            fs::set_permissions(path, permissions)?;
            fs::remove_file(path)
        }
        other => other,
    }
}

/// Delete a folder and all in it, read-only files included.
pub fn remove_tree(path: &Path) -> io::Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;

        if entry.file_type()?.is_dir() {
            remove_tree(&entry.path())?;
        } else {
            unlink(&entry.path())?;
        }
    }

    fs::remove_dir(path)
}

/// The words of an error, without the OS's number after them.
pub fn reason(error: &io::Error) -> String {
    let text = error.to_string();

    match text.find(" (os error") {
        Some(index) => text[..index].to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_backup_never_replaces_an_older_one() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a.c");
        fs::write(&file, "one").unwrap();

        let first = backup(&file).unwrap();
        fs::write(&file, "two").unwrap();
        let second = backup(&file).unwrap();

        assert_ne!(first, second);
        assert_eq!(fs::read_to_string(first).unwrap(), "one");
        assert_eq!(fs::read_to_string(second).unwrap(), "two");
    }

    #[test]
    fn line_endings_come_back_as_they_were() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a.txt");
        write(&file, "a\r\nb\n").unwrap();

        assert_eq!(read(&file).unwrap(), "a\r\nb\n");
    }

    #[test]
    fn a_read_only_file_can_be_deleted() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("a.txt");
        fs::write(&file, "a").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();

        unlink(&file).unwrap();

        assert!(!file.exists());
    }
}
