//! What a window shows, as data: a project, a library looked at against it, and what an install would change.
//!
//! The command says all this in lines of text as it goes. A window wants it in rows it can draw and buttons it can offer, so
//! this reads the same things with the same code and hands them over whole. Nothing here writes to the project: a preview
//! runs a whole install, with the code an install runs, on a copy of the project in a temporary folder.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::check::{Mark, mark_of};
use crate::detect::{self, Candidate};
use crate::download::{self, Client};
use crate::flow::{self, Request, Session, choose_platform};
use crate::manifest::{self, Manifest};
use crate::paths;
use crate::platform::arduino::sketchbook;
use crate::platform::{Platform, PlatformArgs};
use crate::record::{OLD_RECORD_NAME, RECORD_NAME, installed_libraries, recorded_platform};
use crate::report::{Line, Question, Ui};
use crate::steps;
use crate::target::{Finding, Level};
use crate::updates;

/// A file as big as this is left out of a preview's copy, and shown by its name only.
const LARGEST_COPIED: u64 = 2 * 1024 * 1024;

/// A file's text is sent to a preview up to this size.
const LARGEST_SHOWN: usize = 256 * 1024;

/// Folders no install reads, and which can be most of a project: what a build leaves, and a CubeMX project's copy of the HAL.
const LEFT_OUT: [&str; 12] = [
    ".git",
    ".svn",
    ".vs",
    ".pio",
    "build",
    "Debug",
    "Release",
    "managed_components",
    "node_modules",
    "target",
    "Drivers",
    "Middlewares",
];

/// A project, as a window lists it.
#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub folder: String,
    /// The platform to go with: what its record says, or what it looks like most.
    pub platform: Option<Platform>,
    /// Whether that came from the record, which an install wrote, rather than from the look of the folder.
    pub recorded: bool,
    /// What the folder looks like, and which file told.
    pub candidates: Vec<Candidate>,
    /// The IDEs an STM32 project is registered with.
    pub ides: Vec<String>,
    /// Zephyr's Kconfig files, for choosing one.
    pub conf_files: Vec<String>,
    /// Zephyr's overlays, for choosing the one that includes the libraries' devicetree.
    pub overlay_files: Vec<String>,
    /// Whether it is an Arduino sketch, which can hold libraries of its own.
    pub sketch: bool,
    pub installed: Vec<InstalledLibrary>,
}

/// A library a project has.
#[derive(Debug, Clone, Serialize)]
pub struct InstalledLibrary {
    pub name: String,
    pub version: String,
    pub folder: String,
    /// What an update fetches it from: owner/name, or a folder on disk. None for what came from a zip, which only a new zip
    /// can update.
    pub source: Option<String>,
    /// What it came from, for showing.
    pub origin: String,
    /// The options it has on now.
    pub options: Vec<String>,
    /// The libraries it needs.
    pub needs: Vec<String>,
}

fn text_of(entry: &Value, key: &str) -> String {
    entry
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn library_of(name: &str, entry: &Value) -> InstalledLibrary {
    let origin = entry.get("origin").cloned().unwrap_or(Value::Null);
    let kind = text_of(&origin, "kind");
    let path = text_of(&origin, "path");
    let github = text_of(&origin, "source");

    let (source, shown) = match kind.as_str() {
        "github" => {
            let reference = text_of(&origin, "ref");
            let shown = if reference.is_empty() {
                github.clone()
            } else {
                format!("{github}@{reference}")
            };

            (Some(github), shown)
        }
        "folder" => (Some(path.clone()), path),
        "zip" => (None, path),
        // A record from before the origin was kept says where a library came from by its repository.
        _ => {
            let repository = text_of(entry, "repository");

            match download::split_ref(&repository) {
                Ok((owner, name, _)) if repository.contains("github.com") => {
                    let source = format!("{owner}/{name}");

                    (Some(source.clone()), source)
                }
                _ => (None, String::new()),
            }
        }
    };

    let options = entry
        .get("options")
        .and_then(Value::as_object)
        .map(|options| {
            options
                .iter()
                .filter(|(_, on)| on.as_bool() == Some(true))
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default();

    let needs = entry
        .get("needs")
        .and_then(Value::as_array)
        .map(|needs| {
            needs
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    InstalledLibrary {
        name: name.to_string(),
        version: text_of(entry, "version"),
        folder: text_of(entry, "folder"),
        source,
        origin: shown,
        options,
        needs,
    }
}

/// Read a folder as a project: what it looks like, and what it has. It writes nothing, and a folder that holds no project yet
/// is only a folder with nothing to say.
pub fn project(folder: &Path, sketchbook_given: Option<&str>) -> Project {
    let folder = paths::resolve(folder);
    let detection = detect::detect(&folder);
    let recorded = recorded_platform(&folder);
    let platform = recorded.or(detection.platform());
    let sketch = sketchbook::is_sketch(&folder);

    let mut libraries: Map<String, Value> = installed_libraries(&folder);

    // What an Arduino project has is in the sketchbook's libraries folder as well.
    if platform == Some(Platform::Arduino) {
        let (book, _) = sketchbook::sketchbook(sketchbook_given);

        for (name, entry) in installed_libraries(&book.join("libraries")) {
            libraries.entry(name).or_insert(entry);
        }
    }

    let installed = libraries
        .iter()
        .map(|(name, entry)| library_of(name, entry))
        .collect();

    Project {
        conf_files: if platform == Some(Platform::Zephyr) {
            crate::platform::zephyr::app::conf_files(&folder)
        } else {
            Vec::new()
        },
        overlay_files: if platform == Some(Platform::Zephyr) {
            crate::platform::zephyr::overlay::offered(&folder)
        } else {
            Vec::new()
        },
        folder: paths::posix(&folder),
        platform,
        recorded: recorded.is_some(),
        candidates: detection.candidates,
        ides: detection.ides,
        sketch,
        installed,
    }
}

/// What a run says, kept rather than shown, with every question going unanswered.
#[derive(Default)]
struct Kept {
    said: Vec<Line>,
    errors: Vec<String>,
}

impl Ui for Kept {
    fn say(&mut self, line: Line) {
        self.said.push(line);
    }

    fn error(&mut self, message: &str) {
        self.errors.push(message.to_string());
    }

    fn ask(&mut self, _question: &Question) -> Option<String> {
        None
    }

    fn shows_output(&self) -> bool {
        false
    }
}

/// Something the check found.
#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub level: Level,
    pub message: String,
    pub hint: String,
}

/// A part of a library, for the choosing.
#[derive(Debug, Clone, Serialize)]
pub struct OptionOffered {
    pub name: String,
    pub description: String,
    /// It is in the project now.
    pub on: bool,
    /// Libraries it brings.
    pub needs: Vec<String>,
}

/// A step a library runs once it is in, in full, since what it runs is what is agreed to.
#[derive(Debug, Clone, Serialize)]
pub struct StepOffered {
    pub label: String,
    pub description: String,
    pub command: String,
}

/// Where a library it needs stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NeedState {
    /// In the project at a version that will do.
    Here,
    /// In the project, too old, and updated first.
    Update,
    /// Not in the project, and installed first.
    Get,
}

#[derive(Debug, Clone, Serialize)]
pub struct Need {
    pub name: String,
    pub wanted: String,
    pub state: NeedState,
    pub have: String,
}

/// A library's file, for looking at.
#[derive(Debug, Clone, Serialize)]
pub struct ManifestText {
    pub name: String,
    pub text: String,
}

/// A library looked at against a project, with nothing written.
#[derive(Debug, Clone, Serialize)]
pub struct Looked {
    pub given: String,
    pub mark: Mark,
    /// One line for beside it: the first thing to do, or the reason it cannot go in.
    pub why: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub repository: String,
    pub license: String,
    pub platform: Option<Platform>,
    pub findings: Vec<Found>,
    pub options: Vec<OptionOffered>,
    pub steps: Vec<StepOffered>,
    pub needs: Vec<Need>,
    /// The folder it goes into, unless it is told another.
    pub folder: String,
    /// A folder of the project already, which becomes the library where it stands.
    pub in_place: bool,
    /// The version in the project now, when it is there.
    pub installed: Option<String>,
    pub manifest: Option<ManifestText>,
    /// Whether what stopped it may be put right with a token.
    pub needs_token: bool,
}

impl Looked {
    fn refused(given: &str, why: String) -> Looked {
        Looked {
            given: given.to_string(),
            mark: Mark::No,
            needs_token: why.contains("token"),
            why,
            name: String::new(),
            version: String::new(),
            description: String::new(),
            repository: String::new(),
            license: String::new(),
            platform: None,
            findings: Vec::new(),
            options: Vec::new(),
            steps: Vec::new(),
            needs: Vec::new(),
            folder: String::new(),
            in_place: false,
            installed: None,
            manifest: None,
        }
    }
}

fn why_of(kept: &Kept) -> String {
    kept.errors.join("\n")
}

/// What a library it needs would mean for this project.
fn needs_of(library: &Manifest, have: &Map<String, Value>) -> Vec<Need> {
    library
        .dependencies()
        .iter()
        .map(|dependency| {
            let version = have
                .get(&dependency.name)
                .map(|entry| text_of(entry, "version"));
            let state = match &version {
                None => NeedState::Get,
                Some(version) if dependency.satisfied_by(Some(version)) => NeedState::Here,
                Some(_) => NeedState::Update,
            };

            Need {
                name: dependency.name.clone(),
                wanted: dependency.to_string(),
                state,
                have: version.unwrap_or_default(),
            }
        })
        .collect()
}

fn manifest_of(root: &Path) -> Option<ManifestText> {
    let path = manifest::manifest_path(root)?;

    Some(ManifestText {
        name: path.file_name()?.to_string_lossy().into_owned(),
        text: fs::read_to_string(path).ok()?,
    })
}

/// Look at one library against a project: fetch what it lists, read its file, and check it, the same as an install would and
/// writing nothing into the project.
pub fn look_at(
    client: Client,
    folder: &Path,
    platform: Option<Platform>,
    args: &PlatformArgs,
    given: &str,
) -> Looked {
    let mut kept = Kept::default();

    look_with(
        client,
        &paths::resolve(folder),
        platform,
        args,
        given,
        &mut kept,
    )
    .unwrap_or_else(|| Looked::refused(given, why_of(&kept)))
}

/// The look itself: None when the library cannot go in, with the reason kept.
fn look_with(
    mut client: Client,
    folder: &Path,
    platform: Option<Platform>,
    args: &PlatformArgs,
    given: &str,
    kept: &mut Kept,
) -> Option<Looked> {
    let request = Request {
        libraries: vec![given.to_string()],
        platform,
        args: args.clone(),
        ..Request::default()
    };
    let chosen = choose_platform(&request, folder, &mut client, kept)?;

    if !flow::check_platform_args(args, chosen, kept) {
        return None;
    }

    let mut session = Session::open(client, chosen, args, folder, false, kept).ok()?;
    let mut source = session.stage(given, None)?;

    if session.load(&mut source).is_some() {
        return None;
    }

    let library = source.library().clone();
    let root = source
        .project_root
        .clone()
        .unwrap_or_else(|| session.project_root.clone());
    let findings = session.check_library(&library, &root);
    let place = session.default_folder(&library, &root);
    // What the install would refuse before writing a thing, in the folder it offers, is a NO with the reason beside it.
    let refused = session
        .checked(
            &source,
            &session.folder_to_check(&source, Some(place.clone())),
            &[],
        )
        .err();
    let mark = if refused.is_some() {
        Mark::No
    } else {
        mark_of(&findings)
    };
    let have = installed_libraries(&root);
    let entry = have.get(&library.name);
    let on: Vec<String> = entry
        .map(|entry| library_of(&library.name, entry).options)
        .unwrap_or_default();
    let destination = root.join(&place);

    // What it needs may be kept somewhere else, as Arduino's libraries folder is.
    let mut needing = installed_libraries(&session.target.dependency_root(&root));

    needing.extend(have.clone());

    Some(Looked {
        given: given.to_string(),
        mark,
        why: refused.unwrap_or_else(|| {
            findings
                .iter()
                .find(|finding| finding.level == Level::Warn)
                .map(|finding| finding.message.clone())
                .unwrap_or_default()
        }),
        name: library.name.clone(),
        version: library.version.clone(),
        description: library.description.clone(),
        repository: library.repository.clone(),
        license: library.license.clone(),
        platform: Some(chosen),
        options: library
            .options
            .iter()
            .map(|option| OptionOffered {
                name: option.name.clone(),
                description: option.description.clone(),
                on: on.contains(&option.name),
                needs: option.libraries.iter().map(ToString::to_string).collect(),
            })
            .collect(),
        steps: library
            .run
            .iter()
            .map(|step| StepOffered {
                label: step.label.clone(),
                description: step.description.clone(),
                command: steps::shown(step, &root, &destination),
            })
            .collect(),
        needs: needs_of(&library, &needing),
        folder: place,
        in_place: source.in_place,
        installed: entry.map(|entry| text_of(entry, "version")),
        manifest: manifest_of(&source.root),
        findings: findings.iter().map(Found::from).collect(),
        needs_token: false,
    })
}

/// The newest version of each library that came from GitHub, for those with a newer one than the project has.
pub fn newer_versions(
    client: &mut Client,
    libraries: &[InstalledLibrary],
) -> Vec<(String, String)> {
    let mut found = Vec::new();

    for library in libraries {
        let Some(source) = &library.source else {
            continue;
        };

        // A folder on disk has no newer version to ask about: it is what it is.
        if !library.origin.starts_with(source.as_str()) || Path::new(source).is_dir() {
            continue;
        }

        if let Some(newest) = client.newest_version(source)
            && updates::is_newer(&newest, &library.version)
        {
            found.push((library.name.clone(), newest));
        }
    }

    found
}

/// One file an install would change.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    /// From the project's folder, or the sketchbook's libraries folder under libraries/.
    pub path: String,
    pub kind: ChangeKind,
    /// The text before and after, when it is text and small enough to show.
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Changed,
    Removed,
}

/// What an install would do.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    /// The exit code the run ended with.
    pub code: i32,
    pub changes: Vec<Change>,
    pub said: Vec<Line>,
    pub errors: Vec<String>,
}

/// Every file under a folder and what it holds, by a hash of it, with what a preview leaves out left out.
fn fingerprints(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut found = BTreeMap::new();

    for path in paths::files_below(root) {
        if let Ok(data) = fs::read(root.join(&path)) {
            found.insert(path, Sha256::digest(&data).to_vec());
        }
    }

    found
}

/// Copy a folder, without what no install reads and what is too big to matter. Returns whether it could be.
fn copy_project(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;

    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        let kind = entry.file_type()?;

        if kind.is_dir() {
            if !LEFT_OUT.contains(&name.to_string_lossy().as_ref()) {
                copy_project(&entry.path(), &to.join(&name))?;
            } else if name == ".git" {
                // What a repository holds is left out, but not that the folder is one: a repository in the project is
                // not installed where it stands, and the preview says so as the install would.
                fs::create_dir_all(to.join(&name))?;
            }
        } else if kind.is_file() && entry.metadata()?.len() <= LARGEST_COPIED {
            fs::copy(entry.path(), to.join(&name))?;
        }
    }

    Ok(())
}

/// A file's text for showing, when it is text and small enough.
fn shown_text(path: &Path) -> Option<String> {
    let data = fs::read(path).ok()?;

    if data.len() > LARGEST_SHOWN {
        return None;
    }

    String::from_utf8(data).ok()
}

/// What is different under a folder from when it was fingerprinted, as paths with `prefix` in front. `original` holds what it
/// was before.
fn changes_in(
    root: &Path,
    before: &BTreeMap<String, Vec<u8>>,
    original: &Path,
    prefix: &str,
) -> Vec<Change> {
    let after = fingerprints(root);
    let mut changes = Vec::new();
    let older = Platform::ALL.map(|platform| format!(".{}.json", platform.old_installer()));
    let hidden = |path: &str| {
        let name = paths::leaf(path);

        path.ends_with(".bak")
            || name == RECORD_NAME
            || name == OLD_RECORD_NAME
            || older.iter().any(|record| record == name)
    };

    for (path, sum) in &after {
        if hidden(path) || before.get(path) == Some(sum) {
            continue;
        }

        let was = before.contains_key(path);

        changes.push(Change {
            path: format!("{prefix}{path}"),
            kind: if was {
                ChangeKind::Changed
            } else {
                ChangeKind::Added
            },
            before: was.then(|| shown_text(&original.join(path))).flatten(),
            after: shown_text(&root.join(path)),
        });
    }

    for path in before.keys() {
        if !after.contains_key(path) && !hidden(path) {
            changes.push(Change {
                path: format!("{prefix}{path}"),
                kind: ChangeKind::Removed,
                before: shown_text(&original.join(path)),
                after: None,
            });
        }
    }

    changes.sort_by(|one, other| one.path.cmp(&other.path));

    changes
}

fn failed(error: &std::io::Error) -> Preview {
    Preview {
        code: 2,
        changes: Vec::new(),
        said: Vec::new(),
        errors: vec![format!(
            "Could not make a copy of the project to try it on: {error}"
        )],
    }
}

/// Run a whole install on a copy of the project, and say what it did there. The project itself is not touched, a step is not
/// run, and each question takes the answer it offers, unless the request has said already.
pub fn preview(client: Client, request: &Request) -> Preview {
    let project = paths::resolve(request.project.as_deref().unwrap_or(Path::new(".")));

    let holder = match tempfile::Builder::new()
        .prefix("libnima-preview-")
        .tempdir()
    {
        Ok(holder) => holder,
        Err(error) => return failed(&error),
    };

    let name = project
        .file_name()
        .map_or_else(|| "project".into(), |name| name.to_os_string());
    let copy = holder.path().join(name);

    if let Err(error) = copy_project(&project, &copy) {
        return failed(&error);
    }

    let before = fingerprints(&copy);

    // An Arduino library goes into the sketchbook's libraries folder, so that is what is tried on as well.
    let mut asked = request.clone();
    let book = request
        .platform
        .or_else(|| recorded_platform(&project))
        .or_else(|| detect::detect(&project).platform())
        .filter(|platform| *platform == Platform::Arduino)
        .map(|_| sketchbook::sketchbook(request.args.sketchbook.as_deref()).0);
    let libraries = book.as_ref().map(|book| book.join("libraries"));
    let shelf: PathBuf = holder.path().join("sketchbook").join("libraries");

    if let Some(libraries) = &libraries {
        if libraries.is_dir()
            && let Err(error) = copy_project(libraries, &shelf)
        {
            return failed(&error);
        }

        asked.args.sketchbook = shelf
            .parent()
            .map(|book| book.to_string_lossy().into_owned());
    }

    let shelf_before = if libraries.is_some() {
        fingerprints(&shelf)
    } else {
        BTreeMap::new()
    };

    // A folder of the project is in the copy too, and is what becomes the library there.
    asked.libraries = request
        .libraries
        .iter()
        .map(|given| {
            let path = Path::new(given);

            if path.is_dir() && paths::within(&paths::resolve(path), &project) {
                paths::relative_to(&paths::resolve(path), &project)
                    .map_or_else(|| given.clone(), |inside| paths::posix(&copy.join(inside)))
            } else {
                given.clone()
            }
        })
        .collect();
    asked.project = Some(copy.clone());
    asked.run_on = Vec::new();
    asked.yes = false;

    let mut kept = Kept::default();
    let code = flow::run_with(client, &asked, &mut kept);

    let mut changes = changes_in(&copy, &before, &project, "");

    if let Some(libraries) = &libraries {
        changes.extend(changes_in(&shelf, &shelf_before, libraries, "libraries/"));
    }

    Preview {
        code,
        changes,
        said: kept.said,
        errors: kept.errors,
    }
}

/// The folder's file, or what it could be converted to, for the editor: the path to save to, the text to show, and whether it
/// came from lib-installer's and has not been saved as libnima.yml yet.
pub fn file_for_editing(folder: &Path) -> Result<(PathBuf, String, bool), String> {
    let new = folder.join(manifest::MANIFEST_NAME);

    if new.is_file() {
        return fs::read_to_string(&new)
            .map(|text| (new.clone(), text, false))
            .map_err(|error| format!("Could not read {}: {error}", paths::posix(&new)));
    }

    let old = folder.join(manifest::INSTALLER_NAME);

    if old.is_file() {
        let text = fs::read_to_string(&old)
            .map_err(|error| format!("Could not read {}: {error}", paths::posix(&old)))?;
        let converted = crate::convert::convert(&text)
            .map_err(|error| format!("{}: {}", manifest::INSTALLER_NAME, error.message))?;

        return Ok((new, converted, true));
    }

    Err(format!(
        "No {} in {}. Make one with New.",
        manifest::MANIFEST_NAME,
        paths::posix(folder)
    ))
}

/// A finding as what a window keeps of it.
impl From<&Finding> for Found {
    fn from(finding: &Finding) -> Found {
        Found {
            level: finding.level,
            message: finding.message.clone(),
            hint: finding.hint.clone(),
        }
    }
}
