//! Copying a library into a user's project.
//!
//! Two rules shape everything here.
//!
//! The library's files are replaced on every install, so an update actually updates, and a library the
//! user broke by accident is put right.
//!
//! What the user wrote between USER CODE BEGIN and USER CODE END survives that, carried into the new
//! file in the section of the same name, the way STM32CubeMX keeps the user's code when it generates
//! again. See usercode.rs.
//!
//! Neither reaches past what is the library's. A file of the user's, or of another library's, where one
//! of its files would go stops the install before anything is written, and a hand edit that no section
//! can carry, an extra's among them, is saved beside the file before it is replaced or removed.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use regex::Regex;
use serde_json::{Map, Value, json};

use crate::files::{self, backup, unlink};
use crate::manifest::{DEFINE, FileEntry, Manifest};
use crate::paths;
use crate::platform::Platform;
use crate::record::{OLD_RECORD_NAME, RECORD_NAME, installed_libraries, read_record, write_record};
use crate::target::{Outcome, Target};
use crate::usercode;

/// What a library repository carries that a firmware project must not.
///
/// test/ matters most. STM32CubeIDE compiles every .c file under the project tree, so a test harness
/// left in place brings a second main() into the build and the link fails with an error that tells the
/// user nothing useful.
///
/// This is a fixed list rather than "everything that is not a library file", so that anything the user
/// put in the folder themselves survives.
///
/// LICENSE.md and NOTICE are deliberately absent. The Apache licence requires both to travel with the
/// code, and the NOTICE file is what carries the author's attribution into the user's product. So are
/// the library's own file, libnima.yml or installer.yml, which no IDE compiles and which the folder is
/// installed from again, and .git: a repository is never installed where it stands, see flow.rs.
const DEFAULT_CLEANUP: [&str; 11] = [
    ".github",
    ".clang-format",
    ".gitignore",
    "build",
    "test",
    "inc",
    "src",
    "template",
    "CMakeLists.txt",
    "CONTRIBUTING.md",
    "install.py",
];

/// The shape of the record. 2 adds where a library came from, the project's own once files, the blocks
/// in project files, and the run steps that ran. 3 also says which platform the project is. 4 also keeps
/// what each extra held, so a hand edit to one is saved before it goes.
const RECORD_VERSION: i64 = 4;

/// The install cannot proceed, with a reason worth showing the user.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallError(pub String);

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InstallError {}

fn io_error(error: std::io::Error, doing: &str, path: &Path) -> InstallError {
    InstallError(format!(
        "Could not {doing} {}: {}",
        path.display(),
        files::reason(&error)
    ))
}

/// A file carried to where a new version wants it, and the USER CODE sections that went with it.
#[derive(Debug, Clone)]
pub struct Carried {
    pub from: PathBuf,
    pub to: PathBuf,
    pub kept: Vec<String>,
}

/// A copy put aside before something in it was replaced or removed, and why.
#[derive(Debug, Clone)]
pub struct Saved {
    pub file: PathBuf,
    pub copy: PathBuf,
    pub reason: String,
}

/// What an install actually did, so the caller can report it honestly.
#[derive(Debug, Clone, Default)]
pub struct InstallResult {
    pub destination: PathBuf,
    pub installed: Vec<PathBuf>,
    pub removed: Vec<String>,
    /// An update only: the library's own files from the last install that the new version no longer
    /// ships.
    pub dropped: Vec<PathBuf>,
    /// An update only: each library file the new version puts in another folder.
    pub carried: Vec<Carried>,
    /// An update only: defines the last install of this library put in the project and this version no
    /// longer asks for, so the IDE files can let go of them. Only ever ones this library added.
    pub dropped_defines: Vec<String>,
    /// The once files, in the project's folder: created because they were missing, or kept because they
    /// were there. Either way the project's own from then on.
    pub yours_created: Vec<PathBuf>,
    pub yours_kept: Vec<PathBuf>,
    /// The extras with a root, in the project's folder: the library's own there, written on every install.
    pub placed: Vec<PathBuf>,
    /// Each file whose USER CODE sections were carried into the new version.
    pub preserved: Vec<(PathBuf, Vec<String>)>,
    pub backups: Vec<Saved>,
    /// What each file written holds outside its USER CODE sections, kept in the record so the next install
    /// can tell a hand edit there.
    pub fingerprints: Vec<(PathBuf, String)>,
    /// What each extra written holds, the whole of it, since an extra has no sections of the user's: in the
    /// library's folder and in the project's. Kept in the record for the same reason.
    pub extras: Vec<(PathBuf, String)>,
}

impl InstallResult {
    /// True when the user's work was found and kept.
    pub fn was_update(&self) -> bool {
        !self.preserved.is_empty()
    }
}

/// One of the other libraries of a project, as far as an install needs to know it: what it holds, which an install never
/// writes over.
#[derive(Debug, Clone)]
pub struct Held {
    pub name: String,
    pub folder: Option<PathBuf>,
    /// Its own files: those in its folder, and those it puts in the project's.
    pub files: Vec<PathBuf>,
    /// The project's own files it made once, which are the user's from then on.
    pub yours: Vec<PathBuf>,
}

/// The other libraries of a project: those its record has, and those a run is about to install beside it.
#[derive(Debug, Clone, Default)]
pub struct Others {
    pub held: Vec<Held>,
}

impl Others {
    /// Every library the project's record has, but `name`.
    pub fn recorded(project_root: &Path, name: &str) -> Others {
        let root = paths::resolve(project_root);
        let held = installed_libraries(&root)
            .iter()
            .filter(|(other, entry)| *other != name && entry.is_object())
            .map(|(other, entry)| {
                let installed = Installed::read(&root, other, entry);

                Held {
                    name: other.clone(),
                    // A record that names the project's own folder as a library's, as one from 1.0.0 can, holds no folder
                    // of a library that anything could be inside.
                    folder: installed
                        .destination
                        .filter(|folder| !paths::same(folder, &root)),
                    files: installed
                        .files
                        .into_iter()
                        .chain(installed.placed)
                        .collect(),
                    yours: installed.project_files,
                }
            })
            .collect();

        Others { held }
    }

    /// The library whose folder a path is, or is in.
    pub fn folder_of(&self, path: &Path) -> Option<&Held> {
        self.held.iter().find(|held| {
            held.folder
                .as_deref()
                .is_some_and(|folder| paths::same(path, folder) || paths::within(path, folder))
        })
    }

    /// The library whose folder is inside a path.
    pub fn inside(&self, path: &Path) -> Option<&Held> {
        self.held.iter().find(|held| {
            held.folder
                .as_deref()
                .is_some_and(|folder| paths::within(folder, path))
        })
    }

    /// The library a path is a file of.
    fn owner_of(&self, path: &Path) -> Option<&Held> {
        self.held
            .iter()
            .find(|held| held.files.iter().any(|file| paths::same(file, path)))
    }

    /// The library that made a path as a file of the project's own, which is the user's.
    fn maker_of(&self, path: &Path) -> Option<&Held> {
        self.held
            .iter()
            .find(|held| held.yours.iter().any(|file| paths::same(file, path)))
    }
}

/// An install checked, with nothing written yet: where its files go, each one known not to be someone else's.
#[derive(Debug, Clone)]
pub struct Prepared {
    pub destination: PathBuf,
    previous: Option<Value>,
    /// The library's own files in its folder.
    pub files: Vec<PathBuf>,
    /// The once files: the template, and where it lands.
    once: Vec<(PathBuf, PathBuf)>,
    /// The extras with a root: the file, and where it lands.
    placed: Vec<(PathBuf, PathBuf)>,
}

impl Prepared {
    /// What the library will hold once it is in, for the next library of a run to be checked against.
    pub fn held(&self, name: &str) -> Held {
        Held {
            name: name.to_string(),
            folder: Some(self.destination.clone()),
            files: self
                .files
                .iter()
                .chain(self.placed.iter().map(|(_, found)| found))
                .cloned()
                .collect(),
            yours: self.once.iter().map(|(_, found)| found.clone()).collect(),
        }
    }
}

/// Whether two files hold the same bytes. The same file counts, of course.
fn same_content(path: &Path, other: &Path) -> bool {
    if paths::same(&paths::resolve(path), &paths::resolve(other)) {
        return true;
    }

    match (fs::read(path), fs::read(other)) {
        (Ok(one), Ok(two)) => one == two,
        _ => false,
    }
}

fn relative_text(path: &Path, root: &Path) -> String {
    paths::shown(path, root)
}

/// The record of this library's last install, when it went into this folder.
///
/// Installed somewhere else, it is a separate copy, and nothing of it is moved or removed from here.
fn previous_of(project_root: &Path, name: &str, destination: &Path) -> Option<Value> {
    let entry = installed_libraries(project_root).remove(name)?;
    let folder = entry.get("folder")?.as_str()?;

    paths::same(
        &paths::resolve(&project_root.join(folder)),
        &paths::resolve(destination),
    )
    .then_some(entry)
}

fn strings_of<'a>(entry: &'a Value, key: &str) -> Vec<&'a str> {
    entry
        .get(key)
        .and_then(Value::as_array)
        .map(|list| list.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// Where each once file lands: (template, target) pairs.
///
/// An error, before anything is written, for a file with nowhere to go, or one that would be a file
/// libNIMA edits or reads as the project's own, or one inside the library's folder, which every install
/// replaces, or inside another library's folder or on a file of another library's, which that library's
/// install would replace and its uninstall remove.
fn project_once(
    library: &Manifest,
    project_root: Option<&Path>,
    destination: &Path,
    target: &dyn Target,
    others: &Others,
) -> Result<Vec<(PathBuf, PathBuf)>, InstallError> {
    if library.once.is_empty() {
        return Ok(Vec::new());
    }

    let root = check_root(library, &library.once, project_root, target)?;
    let managed = managed_of(library, project_root, target);
    let mut placed = Vec::new();

    for entry in &library.once {
        let to = &entry.destination;
        let found = paths::resolve(&root.join(to));

        // The manifest's check already refused .. and the like, so this only catches a link in the project
        // that leads out of it.
        if !paths::within(&found, &root) {
            return Err(InstallError(format!(
                "{} would put {to} outside the project's folder. Nothing was installed.",
                library.name
            )));
        }

        if paths::same(&found, destination) || paths::within(&found, destination) {
            return Err(InstallError(format!(
                "{} would put {to} inside its own folder, which every install replaces. A file of the library's own \
                 belongs under headers or sources, with USER CODE sections for what is the user's. Nothing was installed.",
                library.name
            )));
        }

        refuse_others(library, to, &found, &root, others)?;
        refuse_managed(library, to, &found, &root, &managed)?;
        placed.push((library.root.join(&entry.source), found));
    }

    Ok(placed)
}

/// A file for the project's folder that would land in another library's folder, or on a file of another library's, is
/// refused: that library's next install would write over it, and its uninstall would remove it.
fn refuse_others(
    library: &Manifest,
    to: &str,
    found: &Path,
    root: &Path,
    others: &Others,
) -> Result<(), InstallError> {
    if let Some(other) = others.folder_of(found) {
        let folder = other
            .folder
            .as_deref()
            .map(|folder| relative_text(folder, root))
            .unwrap_or_default();

        return Err(InstallError(format!(
            "{} would put {to} inside {folder}, the folder of {}, which every install of it replaces. Nothing was \
             installed.",
            library.name, other.name
        )));
    }

    if let Some(other) = others.owner_of(found) {
        return Err(InstallError(format!(
            "{} would put {to} in the project, where {} has a file of its own. Nothing was installed.",
            library.name, other.name
        )));
    }

    Ok(())
}

/// The folder a library's files for the project land in, or an error saying there is none.
fn check_root(
    library: &Manifest,
    entries: &[FileEntry],
    project_root: Option<&Path>,
    target: &dyn Target,
) -> Result<PathBuf, InstallError> {
    let root = project_root.and_then(|project_root| target.once_root(project_root));

    let Some(root) = root else {
        let listed: Vec<&str> = entries
            .iter()
            .map(|entry| entry.destination.as_str())
            .collect();

        return Err(InstallError(format!(
            "{} has files for the project's own folder ({}). {} Nothing was installed.",
            library.name,
            listed.join(", "),
            target.no_once_root()
        )));
    };

    Ok(paths::resolve(&root))
}

fn managed_of(library: &Manifest, project_root: Option<&Path>, target: &dyn Target) -> Vec<String> {
    project_root
        .map(|project_root| target.managed_files(project_root, library))
        .unwrap_or_default()
        .iter()
        .map(|pattern| pattern.replace('\\', "/"))
        .collect()
}

fn refuse_managed(
    library: &Manifest,
    to: &str,
    found: &Path,
    root: &Path,
    managed: &[String],
) -> Result<(), InstallError> {
    let relative = relative_text(found, root);

    if managed
        .iter()
        .any(|pattern| paths::name_matches(&relative, pattern))
    {
        return Err(InstallError(format!(
            "{} would put {to} in the project, and {relative} is a file libNIMA edits or reads as the project's own. \
             Nothing was installed.",
            library.name
        )));
    }

    Ok(())
}

/// Where each extra with a root lands, in the project's folder: (source, target) pairs.
///
/// Checked as once files are, before anything is written. A file already there that the last install did
/// not put there is the user's, and is never written over: the install stops and says so. So is one a
/// library made as a file of the project's own, which is the user's whether or not it is there yet.
fn project_placed(
    library: &Manifest,
    project_root: Option<&Path>,
    destination: &Path,
    previous: Option<&Value>,
    target: &dyn Target,
    others: &Others,
) -> Result<Vec<(PathBuf, PathBuf)>, InstallError> {
    if library.placed.is_empty() {
        return Ok(Vec::new());
    }

    let root = check_root(library, &library.placed, project_root, target)?;
    let managed = managed_of(library, project_root, target);
    let ours: Vec<PathBuf> = match (previous, project_root) {
        (Some(previous), Some(project_root)) => strings_of(previous, "placed")
            .iter()
            .map(|relative| paths::resolve(&project_root.join(relative)))
            .collect(),
        _ => Vec::new(),
    };

    let mut placed = Vec::new();

    for entry in &library.placed {
        let to = &entry.destination;
        let source = library.root.join(&entry.source);
        let found = paths::resolve(&root.join(to));

        if !paths::within(&found, &root) {
            return Err(InstallError(format!(
                "{} would put {to} outside the project's folder. Nothing was installed.",
                library.name
            )));
        }

        if paths::same(&found, destination) || paths::within(&found, destination) {
            return Err(InstallError(format!(
                "{} would put {to} inside its own folder. An extra for the library's own folder needs no root. Nothing \
                 was installed.",
                library.name
            )));
        }

        refuse_others(library, to, &found, &root, others)?;
        refuse_managed(library, to, &found, &root, &managed)?;

        let made = others.maker_of(&found);

        if made.is_some()
            || (found.is_file()
                && !ours.iter().any(|path| paths::same(path, &found))
                && !paths::same(&paths::resolve(&source), &found)
                && !same_content(&found, &source))
        {
            let shown = relative_text(&found, &root);

            return Err(InstallError(match made {
                Some(other) if !found.is_file() => format!(
                    "{} would write {shown} in the project, where {} makes a file of the project's own. Nothing was \
                     installed.",
                    library.name, other.name
                ),
                _ => format!(
                    "{} would write {shown} in the project, where a file of yours already is. Move it or rename it, then \
                     install again. Nothing was installed.",
                    library.name
                ),
            }));
        }

        placed.push((source, found));
    }

    Ok(placed)
}

/// Everything an install checks before it writes a thing, so a library that cannot go in leaves no trace: the folder, where
/// each file lands, that the markers of its code pair, and that no file it would write is someone else's.
///
/// A run of several libraries checks each this way before the first is written, with the libraries of the run checked
/// before it among `others`.
pub fn prepare(
    library: &Manifest,
    destination: &Path,
    project_root: Option<&Path>,
    target: &dyn Target,
    others: &Others,
) -> Result<Prepared, InstallError> {
    let destination = paths::resolve(destination);

    if destination.exists() && !destination.is_dir() {
        return Err(InstallError(format!(
            "{} is a file, so it cannot hold the library.",
            destination.display()
        )));
    }

    let previous = project_root
        .and_then(|project_root| previous_of(project_root, &library.name, &destination));

    // Where the project's own once files go, each checked before anything is written, so a manifest that
    // names a file it must not leaves no trace.
    let once = project_once(library, project_root, &destination, target, others)?;
    // The library's own files in the project's folder, checked the same way.
    let placed = project_placed(
        library,
        project_root,
        &destination,
        previous.as_ref(),
        target,
        others,
    )?;

    // Every code file is checked before any is written. Markers that do not pair are the library's mistake,
    // and finding one half way would leave the project with half of each version.
    for entry in &library.code_files() {
        let path = library.root.join(&entry.source);
        let data = fs::read(&path).map_err(|error| io_error(error, "read", &path))?;

        if let Err(error) = usercode::parse(&data) {
            return Err(InstallError(format!(
                "{} in {} has USER CODE markers that do not pair, {error}. Nothing was installed.",
                entry.source, library.name
            )));
        }
    }

    let files = own_files(
        library,
        &destination,
        project_root,
        previous.as_ref(),
        others,
    )?;

    Ok(Prepared {
        destination,
        previous,
        files,
        once,
        placed,
    })
}

/// The library's own files in its folder, each checked against what is there already.
///
/// A file the last install wrote there is the library's, and is replaced. One that holds what the library's does loses
/// nothing when it is, and becomes the library's. Anything else there is someone else's, and is never written over, as a
/// file of the user's in the project's folder is not: another library's, or the user's own, such as a copy of the library
/// they put there by hand.
fn own_files(
    library: &Manifest,
    destination: &Path,
    project_root: Option<&Path>,
    previous: Option<&Value>,
    others: &Others,
) -> Result<Vec<PathBuf>, InstallError> {
    let root = project_root.map_or_else(|| destination.to_path_buf(), paths::resolve);
    let ours: Vec<PathBuf> = previous
        .map(|previous| {
            strings_of(previous, "files")
                .into_iter()
                .chain(strings_of(previous, "once"))
                .map(|relative| paths::resolve(&root.join(relative)))
                .collect()
        })
        .unwrap_or_default();
    let code = library.code_files();
    let every = code
        .iter()
        .map(|entry| (*entry, true))
        .chain(library.extras.iter().map(|entry| (entry, false)));
    let mut files = Vec::new();

    for (entry, is_code) in every {
        let source = paths::resolve(&library.root.join(&entry.source));
        let found = paths::resolve(&destination.join(&entry.destination));

        files.push(found.clone());

        if !found.is_file()
            || paths::same(&source, &found)
            || ours.iter().any(|path| paths::same(path, &found))
        {
            continue;
        }

        let shown = relative_text(&found, &root);

        if let Some(other) = others.owner_of(&found) {
            return Err(InstallError(format!(
                "{} would write {shown}, a file of {}'s. Nothing was installed.",
                library.name, other.name
            )));
        }

        if others.maker_of(&found).is_some() || !loses_nothing(&found, &source, is_code) {
            // An update stays in its folder, so only a first install can go somewhere else.
            let elsewhere = if previous.is_none() {
                format!(", or install {} into another folder", library.name)
            } else {
                String::new()
            };

            return Err(InstallError(format!(
                "{} would write {shown}, where a file of yours already is. Move it or rename it{elsewhere}, then install \
                 again. Nothing was installed.",
                library.name
            )));
        }
    }

    Ok(files)
}

/// Whether writing the library's file over the one there would lose nothing. For code, the same outside the USER CODE
/// sections, with each section that holds something carried into the new one. For an extra, which is copied as it is, the
/// same.
fn loses_nothing(found: &Path, source: &Path, is_code: bool) -> bool {
    let (Ok(old), Ok(new)) = (fs::read(found), fs::read(source)) else {
        return false;
    };

    if !is_code {
        return usercode::whole_fingerprint(&old) == usercode::whole_fingerprint(&new);
    }

    usercode::fingerprint(&old) == usercode::fingerprint(&new)
        && usercode::merge(&new, &old)
            .is_ok_and(|merged| merged.lost.is_empty() && merged.broken.is_none())
}

/// Copy an extra into place, as it is, and keep what it holds for the record.
///
/// One changed by hand since the last install wrote it is put aside first, since an extra has no sections to carry the
/// change in. Installing in place, the file already is the library's own, and is left as it is.
fn write_extra(
    source: &Path,
    found: &Path,
    previous: Option<&Value>,
    project_root: Option<&Path>,
    result: &mut InstallResult,
) -> Result<(), InstallError> {
    if let Some(parent) = found.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(error, "make", parent))?;
    }

    let data = fs::read(source).map_err(|error| io_error(error, "read", source))?;

    if !paths::same(&paths::resolve(source), &paths::resolve(found)) {
        let edited = match (previous, project_root) {
            (Some(previous), Some(project_root)) => edited_extra(found, previous, project_root),
            _ => None,
        };

        // Nothing is lost when the hand edit made it what the new version holds anyway.
        if edited == Some(true)
            && fs::read(found).is_ok_and(|old| {
                usercode::whole_fingerprint(&old) != usercode::whole_fingerprint(&data)
            })
        {
            let copy = backup(found).map_err(|error| io_error(error, "copy", found))?;

            result.backups.push(Saved {
                file: found.to_path_buf(),
                copy,
                reason: "it was changed by hand".to_string(),
            });
        }

        fs::copy(source, found).map_err(|error| io_error(error, "write", found))?;
    }

    result
        .extras
        .push((found.to_path_buf(), usercode::whole_fingerprint(&data)));

    Ok(())
}

/// Whether an extra the last install wrote was changed by hand since, or None when the record does not say what it held:
/// a file of the library's code, or one from a record older than this.
fn edited_extra(path: &Path, entry: &Value, project_root: &Path) -> Option<bool> {
    let known = entry
        .get("extras")?
        .get(relative_text(path, project_root))?
        .as_str()?;
    let data = fs::read(path).ok()?;

    Some(usercode::whole_fingerprint(&data) != known)
}

/// Copy a library's files into one folder. Each keeps the path it has in the library.
///
/// When the project already has this library in the same folder, this is an update, and the project is
/// brought in step with the new version. Files the last install wrote and the new version no longer ships
/// are removed. Without that, a library that moved its code into src/ left the old copies behind, CubeIDE
/// compiled both, and a fresh default config sat next to the header, silently winning over the user's own.
///
/// `project_root` is where the record of what was installed lives. Without it there is no record to read
/// or write, so no update handling. `record` writes the record at the end; install_in_place() turns this
/// off, since it records only after its cleanup has run. `others` is what the project's other libraries
/// hold, which nothing here writes over: see prepare().
pub fn install_to(
    library: &Manifest,
    destination: &Path,
    project_root: Option<&Path>,
    target: &dyn Target,
    others: &Others,
    write_it: bool,
) -> Result<InstallResult, InstallError> {
    let Prepared {
        destination,
        previous,
        once: yours,
        placed,
        ..
    } = prepare(library, destination, project_root, target, others)?;

    let mut result = InstallResult {
        destination: destination.clone(),
        ..InstallResult::default()
    };
    let code = library.code_files();

    fs::create_dir_all(&destination).map_err(|error| io_error(error, "make", &destination))?;

    // Code belongs to the library. Always replaced, so an update takes effect, with the user's sections
    // carried over.
    let targets: Vec<PathBuf> = code
        .iter()
        .map(|entry| paths::resolve(&destination.join(&entry.destination)))
        .collect();

    for entry in &code {
        let found = destination.join(&entry.destination);
        let earlier = moved_code(previous.as_ref(), project_root, &found, &targets);

        write_one(
            &library.root.join(&entry.source),
            &found,
            previous.as_ref(),
            project_root,
            &mut result,
            earlier,
        )?;
    }

    // The project's own: made when missing, and never touched again, not by an update and not by an uninstall.
    for (template, found) in yours {
        if found.exists() {
            result.yours_kept.push(found);
            continue;
        }

        if let Some(parent) = found.parent() {
            fs::create_dir_all(parent).map_err(|error| io_error(error, "make", parent))?;
        }

        fs::copy(&template, &found).map_err(|error| io_error(error, "write", &found))?;
        result.yours_created.push(found);
    }

    // The licence and the NOTICE ride along, because the licence says they must, and anything else the manifest
    // lists comes with them. Copied as they are: they hold no USER CODE of the user's, and a README that shows
    // the markers in an example must not be read as having sections. One changed by hand since is put aside first.
    for entry in &library.extras {
        let source = library.root.join(&entry.source);
        let found = destination.join(&entry.destination);

        write_extra(
            &source,
            &found,
            previous.as_ref(),
            project_root,
            &mut result,
        )?;
        result.installed.push(found);
    }

    // Extras with a root: the library's own files, such as its tools, in the project's folder. Written again on
    // every install, as every extra is.
    for (source, found) in placed {
        write_extra(
            &source,
            &found,
            previous.as_ref(),
            project_root,
            &mut result,
        )?;
        result.placed.push(found);
    }

    if let (Some(previous), Some(project_root)) = (&previous, project_root) {
        drop_stale(previous, project_root, &destination, &mut result);
        drop_stale_placed(previous, project_root, &mut result);
        result.dropped_defines = dropped_defines(previous, library);
    }

    if let Some(project_root) = project_root
        && write_it
    {
        record_install(project_root, library, &result, target.platform());
    }

    Ok(result)
}

/// Turn a downloaded repository into a usable library folder, where it sits.
///
/// Used when someone drops the whole repository into their project. The repository scaffolding is removed,
/// leaving a folder that holds only what the firmware needs, and the library's own file it is installed from.
pub fn install_in_place(
    library: &Manifest,
    project_root: &Path,
    target: &dyn Target,
    others: &Others,
) -> Result<InstallResult, InstallError> {
    let mut result = install_to(
        library,
        &library.root.clone(),
        Some(project_root),
        target,
        others,
        false,
    )?;

    remove_scaffolding(library, &mut result);
    remove_unchosen(library, &mut result);
    record_install(project_root, library, &result, target.platform());

    Ok(result)
}

/// Put one of the library's files in place, keeping the user's sections.
///
/// The copy already there is put aside first when replacing it would lose something of the user's: a section
/// the new version has no place for, sections whose markers no longer pair, or a hand edit outside them.
///
/// `earlier` is where the last install put this file, when the new version puts it in another folder. Its
/// sections are carried into the new place, the same as from a copy already there.
fn write_one(
    source: &Path,
    found: &Path,
    previous: Option<&Value>,
    project_root: Option<&Path>,
    result: &mut InstallResult,
    earlier: Option<PathBuf>,
) -> Result<(), InstallError> {
    // A file can land in a subfolder that does not exist yet.
    if let Some(parent) = found.parent() {
        fs::create_dir_all(parent).map_err(|error| io_error(error, "make", parent))?;
    }

    let mut data = fs::read(source).map_err(|error| io_error(error, "read", source))?;

    // Installing in place: the file already is the library's own.
    if !paths::same(&paths::resolve(source), &paths::resolve(found)) {
        let current = if found.is_file() {
            Some(found.to_path_buf())
        } else {
            earlier
        };

        if let Some(current) = current {
            let old = fs::read(&current).map_err(|error| io_error(error, "read", &current))?;
            let merged =
                usercode::merge(&data, &old).map_err(|error| InstallError(error.to_string()))?;
            let reasons = losses(&merged, &old, &data, &current, previous, project_root);

            if !reasons.is_empty() {
                let copy = backup(&current).map_err(|error| io_error(error, "copy", &current))?;

                result.backups.push(Saved {
                    file: current.clone(),
                    copy,
                    reason: reasons.join("; "),
                });
            }

            if !merged.kept.is_empty() {
                result
                    .preserved
                    .push((found.to_path_buf(), merged.kept.clone()));
            }

            let moved = current != found;

            if moved {
                result.carried.push(Carried {
                    from: current.clone(),
                    to: found.to_path_buf(),
                    kept: merged.kept.clone(),
                });
            }

            // Left alone when nothing changed, so the build does not see a newer file and compile it again for
            // nothing.
            if moved || old != merged.data {
                fs::write(found, &merged.data).map_err(|error| io_error(error, "write", found))?;
            }

            data = merged.data;
        } else {
            fs::write(found, &data).map_err(|error| io_error(error, "write", found))?;
        }
    }

    result.installed.push(found.to_path_buf());
    result
        .fingerprints
        .push((found.to_path_buf(), usercode::fingerprint(&data)));

    Ok(())
}

/// Where the last install put a library file that this version puts elsewhere.
///
/// Found by its name among the library's own files from the last install, littlefs/lfs_defines.h for
/// littlefs/src/lfs_defines.h. Only when exactly one matches, it is still there, and this version installs
/// nothing in its place. Anything else is not a move, and the file starts from what the library ships, as before.
fn moved_code(
    previous: Option<&Value>,
    project_root: Option<&Path>,
    found: &Path,
    targets: &[PathBuf],
) -> Option<PathBuf> {
    let (previous, project_root) = (previous?, project_root?);

    if found.exists() {
        return None;
    }

    let name = found.file_name()?;
    let matches: Vec<PathBuf> = strings_of(previous, "files")
        .iter()
        .map(|relative| project_root.join(relative))
        .filter(|old| {
            old.file_name() == Some(name)
                && !targets
                    .iter()
                    .any(|target| paths::same(target, &paths::resolve(old)))
                && old.is_file()
        })
        .collect();

    match matches.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// What replacing the copy already in the project would lose, in words.
fn losses(
    merged: &usercode::Merge,
    old: &[u8],
    new: &[u8],
    found: &Path,
    previous: Option<&Value>,
    project_root: Option<&Path>,
) -> Vec<String> {
    let mut reasons = Vec::new();

    if !merged.lost.is_empty() {
        reasons.push(format!(
            "the new version has no USER CODE {}",
            merged.lost.join(", ")
        ));
    }

    if let Some(broken) = &merged.broken {
        // Nothing in it could be told apart from the library's own text.
        reasons.push(format!("its USER CODE markers do not pair ({broken})"));
        return reasons;
    }

    let (Some(previous), Some(project_root)) = (previous, project_root) else {
        // No record of what was written here, so no way to tell a hand edit.
        return reasons;
    };

    let relative = relative_text(found, project_root);

    if let Some(known) = previous
        .get("fingerprints")
        .and_then(|known| known.get(&relative))
    {
        if known.as_str() != Some(usercode::fingerprint(old).as_str()) {
            reasons.push("it was changed outside its USER CODE sections".to_string());
        }
    } else if strings_of(previous, "once").contains(&relative.as_str())
        && usercode::fingerprint(old) != usercode::fingerprint(new)
    {
        // The user's own file until now, from a version that listed it under once. Anything outside its
        // sections may be theirs.
        reasons.push(
            "it was yours to edit until now, and differs outside its USER CODE sections"
                .to_string(),
        );
    }

    reasons
}

/// Whether a file about to be removed has something of the user's in it.
fn holds_user_work(path: &Path, previous: &Value, project_root: &Path) -> bool {
    // An extra has no sections of the user's, so all there is to tell is a hand edit.
    if let Some(edited) = edited_extra(path, previous, project_root) {
        return edited;
    }

    let Ok(data) = fs::read(path) else {
        return false;
    };

    let Ok(parsed) = usercode::parse(&data) else {
        return true;
    };

    if parsed.holds_something() {
        return true;
    }

    let relative = relative_text(path, project_root);

    previous
        .get("fingerprints")
        .and_then(|known| known.get(&relative))
        .is_some_and(|known| known.as_str() != Some(usercode::fingerprint(&data).as_str()))
}

/// Remove the library's own files that the last install wrote and this one did not.
///
/// Only files the record lists as the library's, and only inside its folder. The user's files are recorded
/// apart from those, and anything the user added was never recorded at all, so neither can end up here.
fn drop_stale(
    previous: &Value,
    project_root: &Path,
    destination: &Path,
    result: &mut InstallResult,
) {
    let current: Vec<PathBuf> = result
        .installed
        .iter()
        .map(|path| paths::resolve(path))
        .collect();
    let carried: Vec<PathBuf> = result
        .carried
        .iter()
        .map(|carried| paths::resolve(&carried.from))
        .collect();

    for relative in strings_of(previous, "files") {
        let old = project_root.join(relative);
        let resolved = paths::resolve(&old);

        if current.iter().any(|path| paths::same(path, &resolved))
            || !paths::within(&resolved, destination)
            || !old.is_file()
        {
            continue;
        }

        // Removed all the same, since a stale copy can be compiled beside the new one, but not before anything
        // of the user's in it is put aside. A file whose sections went to its new place holds nothing more of
        // the user's. Anything it held outside them was put aside as it went.
        if !carried.iter().any(|path| paths::same(path, &resolved))
            && holds_user_work(&old, previous, project_root)
        {
            match backup(&old) {
                Ok(copy) => result.backups.push(Saved {
                    file: old.clone(),
                    copy,
                    reason: "no longer part of the library, and it held your changes".to_string(),
                }),
                Err(_) => continue,
            }
        }

        // A file the IDE holds open stays. Better than failing the update, and it is not reported as removed.
        if unlink(&old).is_err() {
            continue;
        }

        if let Some(parent) = old.parent() {
            prune(parent, destination);
        }

        result.dropped.push(old);
    }
}

/// Remove the extras with a root that the last install put in the project's folder and this one did not, such
/// as a tool the library no longer ships. Only files the record lists as placed, and only inside the project.
/// One changed by hand since is put aside first.
fn drop_stale_placed(previous: &Value, project_root: &Path, result: &mut InstallResult) {
    let root = paths::resolve(project_root);
    let current: Vec<PathBuf> = result
        .placed
        .iter()
        .map(|path| paths::resolve(path))
        .collect();

    for relative in strings_of(previous, "placed") {
        let old = root.join(relative);
        let resolved = paths::resolve(&old);

        if current.iter().any(|path| paths::same(path, &resolved))
            || !paths::within(&resolved, &root)
            || !old.is_file()
        {
            continue;
        }

        if edited_extra(&old, previous, project_root) == Some(true) {
            match backup(&old) {
                Ok(copy) => result.backups.push(Saved {
                    file: old.clone(),
                    copy,
                    reason: "no longer part of the library, and it held your changes".to_string(),
                }),
                Err(_) => continue,
            }
        }

        if unlink(&old).is_err() {
            continue;
        }

        if let Some(parent) = old.parent() {
            prune(parent, &root);
        }

        result.dropped.push(old);
    }
}

/// The defines the last install of this library asked for and this one does not.
///
/// Read from the record, which is a file in the user's project anyone can edit, so an entry is checked as
/// strictly as one from a manifest before it is used to find text to take out of a project file.
fn dropped_defines(previous: &Value, library: &Manifest) -> Vec<String> {
    strings_of(previous, "defines")
        .into_iter()
        .filter(|define| {
            DEFINE.is_match(define) && !library.defines.iter().any(|have| have == define)
        })
        .map(str::to_string)
        .collect()
}

/// Remove folders left empty, up to but not including stop.
fn prune(folder: &Path, stop: &Path) {
    let mut folder = folder.to_path_buf();

    while paths::within(&folder, stop) {
        if fs::remove_dir(&folder).is_err() {
            break;
        }

        if !folder.pop() {
            break;
        }
    }
}

/// Delete the repository-only paths, leaving anything the user added.
///
/// What is kept is worked out from where the files actually landed, not from their names. Files stay in inc/
/// and src/, and those folders are on the cleanup list, so going by name alone would install four files and
/// then delete all four.
fn remove_scaffolding(library: &Manifest, result: &mut InstallResult) {
    let mut keep: Vec<String> = Vec::new();

    for path in &result.installed {
        if let Some(rest) =
            paths::relative_to(&paths::resolve(path), &paths::resolve(&library.root))
            && let Some(first) = rest.components().next()
        {
            // The top level name under the install folder, which is what the cleanup list is written in terms of.
            keep.push(first.as_os_str().to_string_lossy().into_owned());
        }
    }

    for name in DEFAULT_CLEANUP {
        let found = library.root.join(name);

        if !found.exists() || keep.iter().any(|kept| kept == name) {
            continue;
        }

        let done = if found.is_dir() {
            files::remove_tree(&found)
        } else {
            unlink(&found)
        };

        match done {
            // A locked file is worth mentioning, never worth failing the install.
            Err(error) => result.removed.push(format!(
                "{name} (could not remove: {})",
                files::reason(&error)
            )),
            Ok(()) => result.removed.push(name.to_string()),
        }
    }
}

/// Delete the files of the options not taken, from a library installed in place.
///
/// They came with the repository, and left there, STM32CubeIDE would compile them with everything else under the
/// project: spif's littlefs port without littlefs, and a build that fails on a header nobody asked for.
fn remove_unchosen(library: &Manifest, result: &mut InstallResult) {
    for option in &library.options {
        if library.chosen.contains(&option.name) {
            continue;
        }

        for entry in option.code_files() {
            let found = library.root.join(&entry.source);

            if !found.is_file() {
                continue;
            }

            match unlink(&found) {
                Err(error) => result.removed.push(format!(
                    "{} (could not remove: {})",
                    entry.source,
                    files::reason(&error)
                )),
                Ok(()) => {
                    result.removed.push(entry.source.clone());

                    if let Some(parent) = found.parent() {
                        prune(parent, &library.root);
                    }
                }
            }
        }
    }
}

/// Note what was installed, so update and uninstall have something to work from.
fn record_install(
    project_root: &Path,
    library: &Manifest,
    result: &InstallResult,
    platform: Platform,
) {
    let path = project_root.join(RECORD_NAME);

    // A damaged record should never block an install. It gets rewritten.
    let mut data = read_record(&path)
        .filter(|record| record.get("libraries").is_some_and(Value::is_object))
        .unwrap_or_else(|| {
            let mut fresh = Map::new();
            fresh.insert("libraries".to_string(), json!({}));
            fresh
        });

    data.insert("record".to_string(), json!(RECORD_VERSION));
    // The platform, so a later run, an uninstall above all, knows it without being told, and a library for another
    // platform is refused.
    data.insert("platform".to_string(), json!(platform.name()));

    let sorted = |found: &[PathBuf]| -> Value {
        let mut names: Vec<String> = found
            .iter()
            .map(|path| relative_text(path, project_root))
            .collect();
        names.sort();

        json!(names)
    };

    let libraries = data
        .get_mut("libraries")
        .and_then(Value::as_object_mut)
        .expect("checked above");
    let previous = libraries
        .get(&library.name)
        .filter(|entry| entry.is_object())
        .cloned();
    let kept = |key: &str| -> Value {
        previous
            .as_ref()
            .and_then(|previous| previous.get(key))
            .filter(|value| value.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}))
    };

    let mut fingerprints = Map::new();

    for (path, value) in &result.fingerprints {
        fingerprints.insert(relative_text(path, project_root), json!(value));
    }

    let mut extras = Map::new();

    for (path, value) in &result.extras {
        extras.insert(relative_text(path, project_root), json!(value));
    }

    let mut yours = result.yours_created.clone();
    yours.extend(result.yours_kept.iter().cloned());

    let mut needs: Vec<String> = library
        .dependencies()
        .iter()
        .map(|dependency| dependency.name.clone())
        .collect();
    needs.sort();
    needs.dedup();

    let mut options = Map::new();

    for option in &library.options {
        options.insert(
            option.name.clone(),
            json!(library.chosen.contains(&option.name)),
        );
    }

    libraries.insert(
        library.name.clone(),
        json!({
            "version": library.version,
            "repository": library.repository,
            // Where it came from: owner/name on GitHub, the ref asked for, none for the default branch, and the
            // commit that was, or a zip or a folder.
            "origin": library.origin,
            "folder": relative_text(&result.destination, project_root),
            "files": sorted(&result.installed),
            "once": [],
            // The once files, the project's own: never removed.
            "project_files": sorted(&yours),
            // The extras with a root: the library's own, in the project's folder, replaced on an update and removed
            // by an uninstall.
            "placed": sorted(&result.placed),
            // The blocks it has in project files, and the run steps that ran, kept from the last install until the
            // target and the steps say again.
            "blocks": kept("blocks"),
            "ran": kept("ran"),
            "fingerprints": fingerprints,
            // What each extra held as it was written, the whole of it, in the library's folder and in the project's,
            // so an install or an uninstall can tell a hand edit and save it first.
            "extras": extras,
            "defines": library.defines,
            // The libraries this one needs, so an uninstall can refuse to take one out from under another, and say
            // which are no longer needed.
            "needs": needs,
            // Every option offered, and whether it was taken. One the record does not name is new to this project,
            // and is asked about.
            "options": options,
            "installed_at": Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        }),
    );

    write_record(&path, data);
}

/// Add what was learnt after the files went in to a library's record: the blocks the target wrote, the run steps
/// that ran. Never fails, as the record itself.
pub fn note(project_root: &Path, name: &str, key: &str, value: Value) {
    let path = project_root.join(RECORD_NAME);

    let Some(mut data) = read_record(&path) else {
        return;
    };

    let Some(entry) = data
        .get_mut("libraries")
        .and_then(Value::as_object_mut)
        .and_then(|libraries| libraries.get_mut(name))
        .and_then(Value::as_object_mut)
    else {
        return;
    };

    entry.insert(key.to_string(), value);
    write_record(&path, data);
}

/// A library as the record remembers it, which is all an uninstall has.
///
/// The record is a file in the user's project that anyone can edit, so what comes from it is checked as it is
/// read: a path that would lead out of the library's folder is dropped, never deleted, and a define that could
/// not have come from a manifest is ignored.
#[derive(Debug, Clone)]
pub struct Installed {
    pub name: String,
    pub version: String,
    pub destination: Option<PathBuf>,
    pub needs: Vec<String>,
    pub defines: Vec<String>,
    pub files: Vec<PathBuf>,
    pub once: Vec<PathBuf>,
    pub project_files: Vec<PathBuf>,
    pub placed: Vec<PathBuf>,
}

impl Installed {
    pub fn read(project_root: &Path, name: &str, entry: &Value) -> Installed {
        let root = paths::resolve(project_root);
        let destination = entry
            .get("folder")
            .and_then(Value::as_str)
            .map(|folder| paths::resolve(&root.join(folder)));

        let in_project = |key: &str| -> Vec<PathBuf> {
            strings_of(entry, key)
                .iter()
                .map(|relative| paths::resolve(&root.join(relative)))
                .filter(|path| paths::within(path, &root))
                .collect()
        };

        // The recorded paths that are files of this library's folder.
        let inside = |key: &str| -> Vec<PathBuf> {
            let Some(destination) = &destination else {
                return Vec::new();
            };

            in_project(key)
                .into_iter()
                .filter(|path| paths::within(path, destination))
                .collect()
        };

        Installed {
            name: name.to_string(),
            version: entry
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            needs: strings_of(entry, "needs")
                .into_iter()
                .map(str::to_string)
                .collect(),
            defines: strings_of(entry, "defines")
                .into_iter()
                .filter(|define| DEFINE.is_match(define))
                .map(str::to_string)
                .collect(),
            files: inside("files"),
            once: inside("once"),
            project_files: in_project("project_files"),
            placed: in_project("placed"),
            destination,
        }
    }
}

/// What an uninstall did, so the caller can report it honestly.
#[derive(Debug, Clone)]
pub struct Removal {
    pub library: Installed,
    pub removed: Vec<PathBuf>,
    /// Each file of the user's put aside first.
    pub backups: Vec<Saved>,
    /// Files in the library's folder the record does not name: the user's own.
    pub left: Vec<PathBuf>,
    /// The project's own files the library made once, left where they are.
    pub yours: Vec<PathBuf>,
    pub folder_removed: bool,
    /// Outcomes of taking it out of the project's build, from the target.
    pub outcomes: Vec<Outcome>,
    /// Libraries it needed that nothing left in the project needs any more.
    pub unneeded: Vec<String>,
}

/// Take a library out of a project: out of its build, its files, and the record.
///
/// What the user wrote in its files is not lost. A file of the user's own, a configuration file the library
/// created or one with something written in its USER CODE sections or changed by hand, is copied aside before it
/// goes, as an update does. A file the library never installed is left where it is, and with it the folder.
///
/// An error when the library is not installed, or another one needs it.
pub fn uninstall(
    project_root: &Path,
    name: &str,
    target: &mut dyn Target,
) -> Result<Removal, InstallError> {
    let root = paths::resolve(project_root);
    let installed = installed_libraries(&root);

    let Some(entry) = installed.get(name).filter(|entry| entry.is_object()) else {
        let mut known: Vec<&str> = installed.keys().map(String::as_str).collect();
        known.sort_unstable();
        let known = if known.is_empty() {
            "nothing".to_string()
        } else {
            known.join(", ")
        };

        return Err(InstallError(format!(
            "{name} is not installed in this project. {RECORD_NAME} has {known}."
        )));
    };

    let others = || {
        installed
            .iter()
            .filter(|(other, data)| *other != name && data.is_object())
    };

    let mut needed_by: Vec<&str> = others()
        .filter(|(_, data)| strings_of(data, "needs").contains(&name))
        .map(|(other, _)| other.as_str())
        .collect();
    needed_by.sort_unstable();

    if !needed_by.is_empty() {
        return Err(InstallError(format!(
            "{} {} {name}, so it stays. Uninstall {} first, or name them all on one line.",
            needed_by.join(", "),
            if needed_by.len() == 1 {
                "needs"
            } else {
                "need"
            },
            needed_by.join(" and ")
        )));
    }

    let mut library = Installed::read(&root, name, entry);

    // Defines another library still asks for stay in the project.
    let still_defined: Vec<&str> = others()
        .flat_map(|(_, data)| strings_of(data, "defines"))
        .collect();
    library
        .defines
        .retain(|define| !still_defined.contains(&define.as_str()));

    // Out of the build first: a project file still naming a file that is gone fails the build, where one naming a
    // file still there does not.
    let outcomes = target.uninstall(&root, &library);

    let mut removal = Removal {
        library: library.clone(),
        removed: Vec::new(),
        backups: Vec::new(),
        left: Vec::new(),
        yours: Vec::new(),
        folder_removed: false,
        outcomes,
        unneeded: Vec::new(),
    };

    // What another library's record has as its own, or as the user's, is not this one's to take, whatever this one's
    // record says.
    let held = Others::recorded(&root, name);
    let claimed = |path: &Path| held.owner_of(path).is_some() || held.maker_of(path).is_some();

    for path in library.files.iter().chain(&library.once) {
        if !path.is_file() || claimed(path) {
            continue;
        }

        let reason = if library.once.contains(path) {
            Some("it was yours to edit".to_string())
        } else {
            what_is_yours(path, entry, &root)
        };

        if let Some(reason) = reason {
            match backup(path) {
                Ok(copy) => removal.backups.push(Saved {
                    file: path.clone(),
                    copy,
                    reason,
                }),
                Err(_) => continue,
            }
        }

        if unlink(path).is_err() {
            continue;
        }

        removal.removed.push(path.clone());

        if let (Some(parent), Some(destination)) = (path.parent(), &library.destination) {
            prune(parent, destination);
        }
    }

    // The library's own files in the project's folder go with it, as those in its own folder do: they were written
    // again on every install. One changed by hand since is put aside first.
    for path in &library.placed {
        if !path.is_file() || claimed(path) {
            continue;
        }

        if edited_extra(path, entry, &root) == Some(true) {
            match backup(path) {
                Ok(copy) => removal.backups.push(Saved {
                    file: path.clone(),
                    copy,
                    reason: "it was changed by hand".to_string(),
                }),
                Err(_) => continue,
            }
        }

        if unlink(path).is_err() {
            continue;
        }

        removal.removed.push(path.clone());

        if let Some(parent) = path.parent() {
            prune(parent, &root);
        }
    }

    // Never removed: they were the project's from the moment they landed.
    removal.yours = library
        .project_files
        .iter()
        .filter(|path| path.is_file())
        .cloned()
        .collect();

    if let Some(destination) = library
        .destination
        .as_ref()
        .filter(|destination| destination.is_dir())
    {
        removal.left = paths::files_below(destination)
            .iter()
            .map(|path| destination.join(path))
            .collect();

        if removal.left.is_empty() && fs::remove_dir(destination).is_ok() {
            removal.folder_removed = true;
        }
    }

    forget(&root, name);

    let still: Vec<&str> = others()
        .flat_map(|(_, data)| strings_of(data, "needs"))
        .collect();

    removal.unneeded = library
        .needs
        .iter()
        .filter(|need| installed.contains_key(*need) && !still.contains(&need.as_str()))
        .cloned()
        .collect();

    Ok(removal)
}

/// Why a library file is saved before an uninstall removes it, or None.
///
/// The library's code is asked about its sections. An extra, a README for one, is copied as it is, so it is asked only
/// whether it was changed by hand: one that shows USER CODE lines as an example must not be taken for the user's work.
fn what_is_yours(path: &Path, entry: &Value, project_root: &Path) -> Option<String> {
    if let Some(edited) = edited_extra(path, entry, project_root) {
        return edited.then(|| "it was changed by hand".to_string());
    }

    let relative = relative_text(path, project_root);
    let known = entry.get("fingerprints")?.get(&relative)?;
    let data = fs::read(path).ok()?;

    let Ok(parsed) = usercode::parse(&data) else {
        return Some(
            "its USER CODE markers do not pair, so what is yours in it cannot be told apart"
                .to_string(),
        );
    };

    if known.as_str() != Some(usercode::fingerprint(&data).as_str()) {
        return Some("it was changed outside its USER CODE sections".to_string());
    }

    if parsed.holds_something() {
        return Some(
            "it has something between its USER CODE lines, such as your settings".to_string(),
        );
    }

    None
}

/// Take one library out of the record of what was installed.
fn forget(project_root: &Path, name: &str) {
    let path = project_root.join(RECORD_NAME);

    let Some(mut data) = read_record(&path) else {
        return;
    };

    if let Some(libraries) = data.get_mut("libraries").and_then(Value::as_object_mut) {
        libraries.remove(name);
        write_record(&path, data);
    }
}

/// Only the markers an installer before this one wrote: # >>> lib-installer: spif >>>, or ; <<< pio-installer: x
/// <<< in platformio.ini.
fn old_marker(installer: &str) -> Regex {
    Regex::new(&format!(
        r"(?m)^([ \t]*[#;][ \t]*)(>>>|<<<) {}: ",
        regex::escape(installer)
    ))
    .expect("a valid pattern")
}

/// Move a project lib-installer, or one of the five installers before it, installed into over to libNIMA, the first
/// time libNIMA runs in it: the old record, such as .lib-installer.json, becomes .libnima.json with the platform in
/// it, and the markers of its blocks in the project's files name libnima. What is inside each block is not touched,
/// so the record's fingerprints of them still hold. Each file is saved first, as a .bak, and keeps its line endings.
///
/// Returns the files changed, the new record last, and nothing when there was nothing to move, with the name of what
/// it moved over from.
pub fn migrate(project_root: &Path, target: &dyn Target) -> (Vec<PathBuf>, &'static str) {
    let platform = target.platform();
    let older = format!(".{}.json", platform.old_installer());
    let new = project_root.join(RECORD_NAME);

    if new.exists() {
        return (Vec::new(), "");
    }

    let (old, installer): (PathBuf, &'static str) = if project_root.join(OLD_RECORD_NAME).is_file()
    {
        (project_root.join(OLD_RECORD_NAME), "lib-installer")
    } else if project_root.join(&older).is_file() {
        (project_root.join(&older), leak_name(platform))
    } else {
        return (Vec::new(), "");
    };

    let marker = old_marker(installer);
    let mut changed = Vec::new();

    for path in target.legacy_files(project_root) {
        let Ok(text) = files::read(&path) else {
            continue;
        };

        let updated = marker.replace_all(&text, |found: &regex::Captures| {
            format!("{}{} {}: ", &found[1], &found[2], crate::blocks::INSTALLER)
        });

        if updated != text && backup(&path).is_ok() && files::write(&path, &updated).is_ok() {
            changed.push(path);
        }
    }

    let mut data = read_record(&old).unwrap_or_default();
    data.insert("platform".to_string(), json!(platform.name()));

    if backup(&old).is_err() {
        return (changed, installer);
    }

    write_record(&new, data);
    let _ = fs::remove_file(&old);
    changed.push(new);

    (changed, installer)
}

fn leak_name(platform: Platform) -> &'static str {
    match platform {
        Platform::Stm32 => "stm32-installer",
        Platform::Zephyr => "zephyr-installer",
        Platform::Esp32 => "esp32-installer",
        Platform::Arduino => "arduino-installer",
        Platform::Pio => "pio-installer",
    }
}
