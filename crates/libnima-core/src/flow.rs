//! A whole run, the same from the command line and from the window.
//!
//! What to install can be any of these:
//!
//! ```text
//! libnima owner/example                      GitHub, by owner and name
//! libnima example                            GitHub, owner nimaltd
//! libnima https://github.com/owner/example   GitHub, by address
//! libnima D:/Downloads/example-main.zip      the zip GitHub hands out
//! libnima D:/Downloads/example-main          a folder anywhere on disk
//! libnima example                            a folder already in the project
//! ```
//!
//! A path that exists always wins over a name, so a folder called "example" in the project is used as it is and never
//! confused with the repository of that name.
//!
//! Several can be given in one run, such as a library and the zip of a library it needs. A library lists what it needs
//! under requires.libraries. What is missing or too old goes in first, from the command line or else from GitHub, and
//! what is already in the project at a version that will do is left alone.
//!
//! Where the library comes from decides what happens to it:
//!
//! - From GitHub, from a zip, or from a folder outside the project, the files the manifest lists are copied into a folder
//!   of the project, asked for or given with --dir. The source is never changed.
//! - A folder already inside the project becomes the library where it stands. The repository scaffolding is removed from
//!   it, because STM32CubeIDE compiles every .c under the project and a test harness brings a second main() with it. Its
//!   libnima.yml stays. A git repository is never installed that way: its .git, and its history with it, are not
//!   libNIMA's to remove, so the run stops and says to install from a copy outside the project.
//!
//! Either way the work happens in the same order: check the project against what the library needs, copy the files, then
//! register them with the project. The checks and the registering are the target's, see target.rs. With several libraries
//! in a run, every check of every one is made before the first is written, so a run that is refused leaves the project as
//! it was.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::check;
use crate::detect;
use crate::download::{self, Client, DownloadError};
use crate::installer::{self, Held, InstallError, InstallResult, Others, Prepared, Removal};
use crate::manifest::{self, Dependency, MANIFEST_NAME, Manifest, version_tuple};
use crate::paths;
use crate::platform::{Platform, PlatformArgs, arduino, target_of};
use crate::record::{self, installed_libraries};
use crate::report::{self, Question, QuestionKind, Span, Style, Ui, item, item_spans, span};
use crate::steps::{self, StepChoices};
use crate::target::{Dropped, Finding, Level, Outcome, Status, Target};

/// A drive letter, as in D:/Downloads or C:\Users.
static DRIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]:").expect("a valid pattern"));

/// What a run was asked to do.
#[derive(Debug, Clone, Default)]
pub struct Request {
    /// What to install: a name like owner/example, a GitHub URL, a downloaded .zip, or a folder.
    pub libraries: Vec<String>,
    pub reference: Option<String>,
    /// The folder of the project to install one library into.
    pub folder: Option<String>,
    /// The folder for each library by its name, for a window that knows them all before the run.
    pub folders: Vec<(String, String)>,
    pub with_options: Vec<String>,
    pub without_options: Vec<String>,
    pub project: Option<PathBuf>,
    pub run_on: Vec<String>,
    pub run_off: Vec<String>,
    pub yes: bool,
    pub key: Option<String>,
    pub uninstall: Vec<String>,
    pub platform: Option<Platform>,
    pub args: PlatformArgs,
    /// Only say what the check finds, and write nothing.
    pub check_only: bool,
}

/// An error on the way that has been shown already, with the exit code to stop with.
pub struct Stop(pub i32);

/// A library on disk, ready to install, and how it gets into the project.
///
/// `in_place` is a folder already inside the project, which becomes the library where it stands. `staging` is a temporary
/// folder this run made, the download or the unpacked zip, which is removed once the run is over. `project_root` is set
/// only when the project is not the one the run was given.
#[derive(Debug, Clone)]
pub struct Source {
    pub root: PathBuf,
    pub in_place: bool,
    pub staging: Option<PathBuf>,
    pub project_root: Option<PathBuf>,
    pub library: Option<Manifest>,
    /// Where it came from, for the record.
    pub origin: Map<String, Value>,
}

impl Source {
    fn new(root: PathBuf) -> Source {
        Source {
            root,
            in_place: false,
            staging: None,
            project_root: None,
            library: None,
            origin: Map::new(),
        }
    }

    pub(crate) fn library(&self) -> &Manifest {
        self.library.as_ref().expect("read before it is asked for")
    }
}

/// One install, in the order the plan puts them.
struct Step {
    source: usize,
    needed_by: Option<String>,
    dependency: Option<Dependency>,
    installed: Option<Value>,
}

/// A library that cannot be had. Raised before anything is installed.
struct PlanError(String);

impl From<DownloadError> for PlanError {
    fn from(error: DownloadError) -> PlanError {
        PlanError(error.message)
    }
}

/// A path relative to the project when possible, so output stays readable.
fn show(path: &Path, root: &Path) -> String {
    paths::shown(path, root)
}

/// Whether the argument can only be a path, never a library name.
///
/// A path that does not exist is then reported as missing, rather than sent to GitHub, where a mistyped
/// D:/Downloads/example.zip would come back as a baffling "repository not found" for an owner called "D:".
pub fn looks_like_path(text: &str) -> bool {
    // A ref after an @ is a branch, and a branch can hold a slash: feature/x.
    let name = text.split('@').next().unwrap_or(text);

    text.to_lowercase().ends_with(".zip")
        || name.contains('\\')
        || text.starts_with(['.', '/', '~'])
        || DRIVE.is_match(text)
        || (!text.starts_with("http://")
            && !text.starts_with("https://")
            && name.matches('/').count() > 1)
}

/// A path as a shell reads a leading ~.
fn expand_user(given: &str) -> PathBuf {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" });

    match (
        given
            .strip_prefix("~/")
            .or_else(|| given.strip_prefix("~\\")),
        home,
    ) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest),
        _ => PathBuf::from(given),
    }
}

/// What is on disk at a path.
#[derive(PartialEq)]
enum Kind {
    Zip,
    File,
    Folder,
    Nothing,
}

fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        Kind::Folder
    } else if path.is_file() {
        if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        {
            Kind::Zip
        } else {
            Kind::File
        }
    } else {
        Kind::Nothing
    }
}

/// An address, on the web or for SSH: never a path, and never a name with a ref after its @.
fn is_url(given: &str) -> bool {
    download::is_address(given)
}

const NOT_A_ZIP: &str = "is not a .zip. Give the zip GitHub offers under Code, Download ZIP, or the folder it unpacks to.";

/// The parsed manifest of a library named in a run, read before anything else for the platform it names: from a zip
/// without unpacking it, from a folder, or from GitHub, where the fetch that follows asks nothing twice.
fn peek(
    given: &str,
    reference: Option<&str>,
    client: &mut Client,
) -> Result<crate::yaml::Node, PlanError> {
    let raw = if is_url(given) {
        Some(client.peek(given, reference)?)
    } else {
        let path = expand_user(given);

        match kind_of(&path) {
            // One that cannot be opened is not one without a library in it: it is said so, as it is to be had again.
            Kind::Zip => match download::zip_problem(&path) {
                Some(why) => {
                    return Err(PlanError(format!(
                        "{given} cannot be read as a zip ({why}). It may be an incomplete download: download it again."
                    )));
                }
                None => download::peek_zip(&path),
            },
            Kind::Folder => {
                let root = download::find_root(&paths::resolve(&path)).unwrap_or(path.clone());

                manifest::manifest_path(&root).and_then(|found| {
                    let name = found.file_name()?.to_string_lossy().into_owned();
                    Some((std::fs::read(&found).ok()?, name))
                })
            }
            Kind::File => return Err(PlanError(format!("{given} {NOT_A_ZIP}"))),
            Kind::Nothing if looks_like_path(given) => {
                return Err(PlanError(format!("{given} does not exist.")));
            }
            Kind::Nothing => Some(client.peek(given, reference)?),
        }
    };

    let Some((raw, name)) = raw else {
        return Err(PlanError(format!(
            "{given} holds no {MANIFEST_NAME}, so there is no library in it to install."
        )));
    };

    let text = String::from_utf8(raw).map_err(|error| PlanError(error.to_string()))?;
    let (text, _) = manifest::new_form(&name, &text)
        .map_err(|error| PlanError(format!("{given}: {}", error.message)))?;

    manifest::parse(&text).map_err(|error| PlanError(format!("{given}: {}", error.message)))
}

/// Which of a library's platforms this project is, asked. None when nobody answers.
fn ask_platform(name: &str, choices: &[Platform], ui: &mut dyn Ui) -> Option<Platform> {
    ui.say(report::blank());
    ui.say(report::heading(format!(
        "{name} works on more than one platform. Which is this project?"
    )));

    for (number, choice) in choices.iter().enumerate() {
        ui.say(report::plain(format!(
            "  {}  {}",
            number + 1,
            choice.name()
        )));
    }

    let question = Question {
        prompt: report::line("Platform: ", Style::Bold),
        kind: QuestionKind::Text,
    };
    let answer = ui.ask(&question).unwrap_or_default().trim().to_lowercase();

    if let Ok(number) = answer.parse::<usize>()
        && number >= 1
        && number <= choices.len()
    {
        return Some(choices[number - 1]);
    }

    choices
        .iter()
        .copied()
        .find(|choice| choice.name() == answer)
}

fn names_of(platforms: &[Platform]) -> String {
    platforms
        .iter()
        .map(|platform| platform.name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The platform of this run.
///
/// --platform when it is given. Otherwise the one the libraries named in the run give in their libnima.yml, and of those,
/// when they name several, the one this project's record or its own files say. A library for one platform is refused in a
/// project that looks like another, since the look of a folder can be wrong only in a way --platform can put right.
pub(crate) fn choose_platform(
    request: &Request,
    folder: &Path,
    client: &mut Client,
    ui: &mut dyn Ui,
) -> Option<Platform> {
    let names = names_of(&Platform::ALL);
    let recorded = record::recorded_platform(folder);

    if let (Some(recorded), Some(given)) = (recorded, request.platform)
        && recorded != given
    {
        ui.error(&format!(
            "This project's record says {}, and --platform gives {}.",
            recorded.name(),
            given.name()
        ));
        return None;
    }

    if !request.uninstall.is_empty() {
        let found = request.platform.or(recorded);

        if found.is_none() {
            ui.error(&format!(
                "Nothing here says which platform this project is. Say it with --platform, one of {names}."
            ));
        }

        return found;
    }

    let detection = detect::detect(folder);

    let mut said: Vec<(String, Vec<Platform>)> = Vec::new();

    for given in &request.libraries {
        let node = match peek(given, request.reference.as_deref(), client) {
            Ok(node) => node,
            Err(PlanError(message)) => {
                ui.error(&message);
                return None;
            }
        };

        let data = node.as_map().expect("parse() gives a mapping");

        // Before the platform: one this libNIMA does not know may be one a newer libNIMA has, and the way forward is to
        // update, not to name another platform.
        if let Err(error) = manifest::check_version(data) {
            ui.error(&format!("{given}: {}", error.message));
            return None;
        }

        match manifest::platforms_of(data) {
            Ok(Some(platforms)) => {
                let name = data
                    .given("name")
                    .map_or_else(|| given.clone(), crate::yaml::Node::text);
                said.push((name, platforms));
            }
            Ok(None) => {}
            Err(error) => {
                ui.error(&format!("{given}: {}", error.message));
                return None;
            }
        }
    }

    if let Some(given) = request.platform {
        for (name, platforms) in &said {
            if !platforms.contains(&given) {
                ui.error(&format!(
                    "{name} is for {}, and --platform gives {}.",
                    names_of(platforms),
                    given.name()
                ));
                return None;
            }
        }

        return Some(given);
    }

    if said.is_empty() {
        let what = request.libraries.join(" and ");
        let hint = match recorded {
            Some(recorded) => format!(
                "This project's record says {0}, so add --platform {0}.",
                recorded.name()
            ),
            None => format!("Say which it goes into with --platform, one of {names}."),
        };

        ui.error(&format!(
            "{what} names no platform in its {MANIFEST_NAME}. {hint}"
        ));
        return None;
    }

    let common: Vec<Platform> = said[0]
        .1
        .iter()
        .copied()
        .filter(|platform| {
            said.iter()
                .all(|(_, platforms)| platforms.contains(platform))
        })
        .collect();

    if common.is_empty() {
        let each: Vec<String> = said
            .iter()
            .map(|(name, platforms)| format!("{name} for {}", names_of(platforms)))
            .collect();

        ui.error(&format!(
            "These libraries are for different platforms: {}. Install them in runs of their own.",
            each.join("; ")
        ));
        return None;
    }

    let chosen = if let [only] = common.as_slice() {
        *only
    } else if let Some(recorded) = recorded.filter(|recorded| common.contains(recorded)) {
        recorded
    } else if let Some(found) = detection
        .candidates
        .iter()
        .find(|candidate| common.contains(&candidate.platform))
    {
        found.platform
    } else {
        match ask_platform(&said[0].0, &common, ui) {
            Some(chosen) => chosen,
            None => {
                ui.error(&format!(
                    "{} works on {}. Say which this project is with --platform.",
                    said[0].0,
                    names_of(&common)
                ));
                return None;
            }
        }
    };

    if let Some(recorded) = recorded.filter(|recorded| *recorded != chosen) {
        ui.error(&format!(
            "This project's record says {}, and {} is for {}.",
            recorded.name(),
            said[0].0,
            chosen.name()
        ));
        return None;
    }

    if let Some(seen) = detection.candidates.first()
        && !detection.looks_like(chosen)
    {
        ui.error(&format!(
            "{} is for {}, and this looks like a {} project, from {}. If that is wrong, say which it is with --platform {}.",
            said[0].0,
            chosen.label(),
            seen.platform.label(),
            seen.evidence,
            chosen.name()
        ));
        return None;
    }

    Some(chosen)
}

/// An option given for another platform's projects is refused rather than quietly ignored.
pub(crate) fn check_platform_args(args: &PlatformArgs, chosen: Platform, ui: &mut dyn Ui) -> bool {
    let mut given: Vec<(&str, Platform)> = Vec::new();

    if args.ide.is_some() {
        given.push(("--ide", Platform::Stm32));
    }

    if args.conf.is_some() {
        given.push(("--conf", Platform::Zephyr));
    }

    if args.overlay.is_some() {
        given.push(("--overlay", Platform::Zephyr));
    }

    if args.into.is_some() {
        given.push(("--into", Platform::Arduino));
    }

    if args.sketchbook.is_some() {
        given.push(("--sketchbook", Platform::Arduino));
    }

    for (option, platform) in given {
        if platform != chosen {
            ui.error(&format!(
                "{option} is for {} projects, and this one is for {}.",
                platform.label(),
                chosen.label()
            ));
            return false;
        }
    }

    true
}

/// A run under way: the platform's target, and where things are.
pub struct Session<'a> {
    pub client: Client,
    pub target: Box<dyn Target>,
    /// The folder the run was pointed at.
    pub given_root: PathBuf,
    /// The folder the run works in, as the target sees it.
    pub project_root: PathBuf,
    /// Every temporary folder made on the way, removed when the run ends.
    staged: Vec<PathBuf>,
    choices: StepChoices,
    /// The options --with and --without name, for a library that comes in as a requirement as much as one named.
    turn_on: Vec<String>,
    turn_off: Vec<String>,
    ui: &'a mut dyn Ui,
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        for path in &self.staged {
            download::cleanup(path);
        }
    }
}

impl<'a> Session<'a> {
    /// A session on this platform, in this folder. `moving` moves over what lib-installer left in the project first;
    /// a run that writes nothing leaves that for later.
    pub fn open(
        client: Client,
        platform: Platform,
        args: &PlatformArgs,
        given_root: &Path,
        moving: bool,
        ui: &'a mut dyn Ui,
    ) -> Result<Session<'a>, Stop> {
        let mut target = target_of(platform, args);

        // A project an installer before this one installed into moves over first, so all that follows reads libNIMA's
        // record and markers.
        if moving {
            for root in target.legacy_roots(given_root) {
                let (moved, from) = installer::migrate(&root, target.as_ref());

                if !moved.is_empty() {
                    // Each with the copy that holds it as it was, the one the run keeps of it.
                    let shown: Vec<String> = moved
                        .iter()
                        .map(|(path, copy)| {
                            format!(
                                "{}, saved first as {}",
                                show(path, &root),
                                show(copy, &root)
                            )
                        })
                        .collect();

                    ui.say(report::note(format!(
                        "Moved over from {from} to libnima: {}.",
                        shown.join("; ")
                    )));
                }
            }
        }

        let project_root = match target.project_root(given_root, ui) {
            Ok(root) => root,
            Err(error) => {
                ui.error(&error.0);
                return Err(Stop(2));
            }
        };

        Ok(Session {
            client,
            target,
            given_root: given_root.to_path_buf(),
            project_root,
            staged: Vec::new(),
            choices: StepChoices::default(),
            turn_on: Vec::new(),
            turn_off: Vec::new(),
            ui,
        })
    }

    pub fn ui(&mut self) -> &mut dyn Ui {
        self.ui
    }

    /// What the target finds about a library, before it goes in.
    pub fn check_library(&mut self, library: &Manifest, project_root: &Path) -> Vec<Finding> {
        self.target.check(library, project_root, self.ui)
    }

    /// A library folder on disk: converted where it stands, or copied in.
    fn stage_folder(&mut self, folder: &Path) -> Source {
        let folder = paths::resolve(folder);
        let root = download::find_root(&folder).unwrap_or_else(|| folder.clone());
        let mut source = Source::new(root.clone());

        // Run from inside the library folder itself. The folder is the library and its parent the project.
        if paths::same(&root, &self.project_root) {
            source.in_place = true;
            source.project_root = root.parent().map(Path::to_path_buf);
        } else if paths::within(&root, &self.project_root) {
            source.in_place = true;
        } else {
            self.ui.say(report::note(format!(
                "Reading {} ...",
                paths::posix(&folder)
            )));
        }

        source
    }

    /// Get one library named in the run onto disk. None once the reason it could not be had is shown.
    pub fn stage(&mut self, given: &str, reference: Option<&str>) -> Option<Source> {
        if !is_url(given) {
            let path = expand_user(given);

            match kind_of(&path) {
                Kind::Zip => {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.ui.say(report::note(format!("Reading {name} ...")));

                    return match download::unpack(&paths::resolve(&path)) {
                        Ok((staging, root)) => {
                            self.staged.push(staging.clone());

                            let mut source = Source::new(root);
                            source.staging = Some(staging);
                            source.origin.insert("kind".to_string(), json!("zip"));
                            source.origin.insert("path".to_string(), json!(name));

                            Some(source)
                        }
                        Err(error) => {
                            self.ui.error(&error.message);
                            None
                        }
                    };
                }
                Kind::Folder => return Some(self.stage_folder(&path)),
                Kind::File => {
                    self.ui.error(&format!("{given} {NOT_A_ZIP}"));
                    return None;
                }
                Kind::Nothing => {
                    if looks_like_path(given) {
                        self.ui.error(&format!("{given} does not exist."));
                        return None;
                    }
                }
            }
        }

        self.ui.say(report::note(format!("Fetching {given} ...")));

        match self.client.fetch(given, reference, None) {
            Ok(staged) => {
                self.staged.push(staged.clone());

                let mut source = Source::new(staged.clone());
                source.staging = Some(staged.clone());
                source.origin = self.client.origin(&staged);

                Some(source)
            }
            Err(error) => {
                self.ui.error(&error.message);
                None
            }
        }
    }

    /// Read a source's libnima.yml. None when it was read, otherwise the exit code to stop with, the reason already shown.
    pub fn load(&mut self, source: &mut Source) -> Option<i32> {
        match manifest::load(&source.root, Some(self.target.platform())) {
            Ok(mut library) => {
                library.origin = if source.origin.is_empty() {
                    let mut origin = Map::new();
                    origin.insert("kind".to_string(), json!("folder"));
                    origin.insert("path".to_string(), json!(paths::posix(&source.root)));
                    origin
                } else {
                    source.origin.clone()
                };

                source.library = Some(library);

                None
            }
            Err(error) => {
                // libNIMA 1.0.0 removed libnima.yml from a folder it installed in place, so the most likely reason it is
                // missing there is that this folder has already been installed once.
                if source.in_place {
                    let root = source
                        .project_root
                        .clone()
                        .unwrap_or_else(|| self.project_root.clone());
                    let known = installed_libraries(&root);
                    let place = show(&source.root, &root);
                    let already = known
                        .iter()
                        .find(|(_, data)| {
                            data.get("folder").and_then(Value::as_str) == Some(place.as_str())
                        })
                        .map(|(name, _)| name.clone());

                    if let Some(already) = already {
                        self.ui.say(report::warn(format!(
                            "{already} is already installed in {place}."
                        )));
                        self.ui.say(report::note(
                            "  To update it, run this again with a fresh download.",
                        ));

                        return Some(0);
                    }
                }

                self.ui.error(&error.message);

                Some(2)
            }
        }
    }

    /// Settle which of a library's options this install takes, and say so.
    ///
    /// An option named with --with or --without is that. One the project has an answer for, from the last install, keeps it
    /// without a question: an update is not the moment to ask again, and the line printed says how to change it. Anything
    /// else is asked when it may be, and is no otherwise, since nobody chose it.
    fn choose_options(
        &mut self,
        source: &mut Source,
        project_root: &Path,
        turn_on: &[String],
        turn_off: &[String],
        ask: bool,
    ) {
        let library = source
            .library
            .as_mut()
            .expect("read before options are chosen");

        if library.options.is_empty() {
            return;
        }

        let recorded = installed_libraries(project_root);
        let before = recorded
            .get(&library.name)
            .and_then(|entry| entry.get("options"))
            .and_then(Value::as_object);
        let mut chosen = Vec::new();

        self.ui.say(report::blank());
        self.ui.say(report::heading("Options"));

        for option in &library.options {
            let name = &option.name;
            let label = format!("  {name:<10} ");
            let brings: Vec<&str> = option
                .libraries
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect();
            let was = before.and_then(|before| before.get(name));

            let (taken, said) = if turn_on.contains(name) {
                (true, Some(format!("yes, as asked with --with {name}")))
            } else if turn_off.contains(name) {
                let mut said = format!("no, as asked with --without {name}");

                if was == Some(&Value::Bool(true)) && !brings.is_empty() {
                    said.push_str(&format!(
                        ". Its files go. {} stays in the project, for anything else using it",
                        brings.join(", ")
                    ));
                }

                (false, Some(said))
            } else if let Some(was) = was {
                let taken = *was == Value::Bool(true);

                (
                    taken,
                    Some(if taken {
                        format!("yes, as before. Change it with --without {name}")
                    } else {
                        format!("no, as before. Add it with --with {name}")
                    }),
                )
            } else if ask {
                let shown = if option.description.is_empty() {
                    name
                } else {
                    &option.description
                };
                let question = Question {
                    prompt: vec![
                        span(label.clone(), Style::Plain),
                        span(format!("{shown}. Add it? [y/N]: "), Style::Bold),
                    ],
                    kind: QuestionKind::YesNo { default: false },
                };

                match self.ui.ask(&question) {
                    Some(reply) => (matches!(reply.to_lowercase().as_str(), "y" | "yes"), None),
                    None => (
                        false,
                        Some(format!("no, nobody was asked. Add it with --with {name}")),
                    ),
                }
            } else {
                // A library that came in because another needs it. The flags are for the libraries named in the run, so
                // this says how.
                (
                    false,
                    Some(format!(
                        "no. Add it with: libnima {} --with {name}",
                        library.name
                    )),
                )
            };

            if taken {
                chosen.push(name.clone());
            }

            if let Some(said) = said {
                self.ui.say(report::plain(format!("{label}{said}")));
            }
        }

        library.choose(&chosen).expect("only options it has");
    }

    /// Choose the options of every library named in the run.
    ///
    /// A name given to --with or --without, --run or --no-run, that no library of the run has is refused before anything is
    /// installed, since it is almost always a typo, and going ahead without it would install something other than what was
    /// asked for. They apply to a library that comes in as a requirement as much as to one named, so when the libraries named
    /// need others, the names are checked once those are known too, see unknown_names(). None, or the exit code to stop with.
    fn settle_options(&mut self, requested: &mut [Source], request: &Request) -> Option<i32> {
        let both = sorted(
            request
                .with_options
                .iter()
                .filter(|name| request.without_options.contains(name))
                .cloned(),
        );

        if !both.is_empty() {
            self.ui.error(&format!(
                "{} is given to both --with and --without. Give it to one.",
                both.join(", ")
            ));
            return Some(2);
        }

        let both = sorted(
            request
                .run_on
                .iter()
                .filter(|name| request.run_off.contains(name))
                .cloned(),
        );

        if !both.is_empty() {
            self.ui.error(&format!(
                "{} is given to both --run and --no-run. Give it to one.",
                both.join(", ")
            ));
            return Some(2);
        }

        self.turn_on = request.with_options.clone();
        self.turn_off = request.without_options.clone();
        self.choices = StepChoices {
            turn_on: request.run_on.clone(),
            turn_off: request.run_off.clone(),
            assume_yes: request.yes,
        };

        // Nothing else comes in: the libraries named are all there is, and a name none of them has is refused before
        // anything is asked.
        let may_bring = requested.iter().any(|source| {
            let library = source.library();

            !library.requires.libraries.is_empty()
                || library
                    .options
                    .iter()
                    .any(|option| !option.libraries.is_empty())
        });

        if !may_bring && let Some(code) = self.unknown_names(requested, request) {
            return Some(code);
        }

        for source in requested.iter_mut() {
            let root = source
                .project_root
                .clone()
                .unwrap_or_else(|| self.project_root.clone());
            self.choose_options(
                source,
                &root,
                &request.with_options,
                &request.without_options,
                true,
            );
        }

        None
    }

    /// Check what --with, --without, --run and --no-run name against the options and steps of every library of the run,
    /// those that come in as requirements included. None, or the exit code to stop with.
    pub(crate) fn unknown_names(&mut self, sources: &[Source], request: &Request) -> Option<i32> {
        let offered = sorted(sources.iter().flat_map(|source| {
            source
                .library()
                .options
                .iter()
                .map(|option| option.name.clone())
        }));
        let unknown = sorted(
            request
                .with_options
                .iter()
                .chain(&request.without_options)
                .filter(|name| !offered.contains(name))
                .cloned(),
        );

        if !unknown.is_empty() {
            // Said of the libraries of the run, by name: none of them need be installed yet.
            let has: Vec<String> = sources
                .iter()
                .filter(|source| !source.library().options.is_empty())
                .map(|source| {
                    let library = source.library();
                    let names: Vec<&str> = library
                        .options
                        .iter()
                        .map(|option| option.name.as_str())
                        .collect();

                    format!("{} has {}", library.name, names.join(", "))
                })
                .collect();
            let has = if has.is_empty() {
                "No library of this run has options.".to_string()
            } else {
                format!("{}.", has.join("; "))
            };

            self.ui
                .error(&format!("No option called {}. {has}", unknown.join(", ")));
            return Some(2);
        }

        let offered = sorted(
            sources
                .iter()
                .flat_map(|source| source.library().run.iter().map(|step| step.label.clone())),
        );
        let unknown = sorted(
            request
                .run_on
                .iter()
                .chain(&request.run_off)
                .filter(|name| !offered.contains(name))
                .cloned(),
        );

        if !unknown.is_empty() {
            let has = if offered.is_empty() {
                "What is named here has no steps to run.".to_string()
            } else {
                format!("What is named here has: {}.", offered.join(", "))
            };

            self.ui
                .error(&format!("No run step called {}. {has}", unknown.join(", ")));
            return Some(2);
        }

        None
    }

    /// Download a library another one needs, from GitHub.
    ///
    /// At the ref the needing library names, or the default branch. A library with no libnima.yml of its own comes with the
    /// one the needing library carries for it. An error when it cannot be had, saying how to get it without the internet,
    /// since a missing connection is the usual reason.
    ///
    /// `have` is the version in the project when there is one, too old: what is said names it, as an update and not an
    /// install. The zips the advice names are named as GitHub's Download ZIP names them, after the repositories.
    fn fetch_dependency(
        &mut self,
        dependency: &Dependency,
        needed_by: &str,
        needer_root: &Path,
        needer_repository: &str,
        have: Option<&str>,
    ) -> Result<Source, PlanError> {
        self.ui.say(report::note(match have {
            Some(have) => format!(
                "{needed_by} needs {dependency}, newer than the {} {have} in this project. Fetching {} ...",
                dependency.name,
                dependency.fetch()
            ),
            None => format!(
                "{needed_by} needs {dependency}. Fetching {} ...",
                dependency.fetch()
            ),
        }));

        let carried = match &dependency.manifest {
            Some(carried) => Some(std::fs::read(needer_root.join(carried)).map_err(|error| {
                PlanError(format!(
                    "{needed_by} carries a {MANIFEST_NAME} for {} at {carried}, and it cannot be read: {error}",
                    dependency.name
                ))
            })?),
            None => None,
        };

        let root = self
            .client
            .fetch(&dependency.fetch(), None, carried.as_deref())
            .map_err(|error| {
                let what = match have {
                    Some(have) => format!(
                        "{needed_by} needs {dependency}, but {} {have} is the one in this project, and a newer one could \
                         not be fetched",
                        dependency.name
                    ),
                    None => format!(
                        "{needed_by} needs {dependency}, which is not in this project, and it could not be fetched"
                    ),
                };

                PlanError(format!(
                    "{what}: {}\nWithout the internet, download {name} as well, and give both zips on one line:\n    \
                     libnima {first} {second}",
                    error.message,
                    name = dependency.name,
                    first = zip_name(needer_repository, None, needed_by),
                    second = zip_name(
                        &dependency.source,
                        dependency.reference.as_deref(),
                        &dependency.name
                    ),
                ))
            })?;

        self.staged.push(root.clone());

        let mut source = Source::new(root.clone());
        source.staging = Some(root.clone());
        source.origin = self.client.origin(&root);

        let mut library = manifest::load(&root, Some(self.target.platform())).map_err(|error| {
            PlanError(format!(
                "{}, which {needed_by} needs: {}",
                dependency.name, error.message
            ))
        })?;

        library.origin = source.origin.clone();
        source.library = Some(library);

        Ok(source)
    }

    /// What the libraries of a check need, worked out as an install of them would, fetching what it would fetch and writing
    /// nothing: why the install would be refused, or None when it would not.
    pub(crate) fn requirements_problem(&mut self, sources: &mut Vec<Source>) -> Option<String> {
        let requested = sources.len();

        match self.plan(sources, requested) {
            Ok(_) => None,
            Err(PlanError(message)) => Some(message),
        }
    }

    /// Put the installs in order: every library after the libraries it needs.
    ///
    /// A library that is needed and already in the project at a version that will do is left alone, and nobody is asked
    /// anything about it. One that is missing or too old comes from a zip or folder given in the same run, or else from
    /// GitHub. All of that is worked out, and fetched, before anything is installed, so a library that cannot be had stops
    /// the run with the project untouched.
    fn plan(
        &mut self,
        sources: &mut Vec<Source>,
        requested: usize,
    ) -> Result<Vec<Step>, PlanError> {
        // What other libraries need may live somewhere else, such as Arduino's libraries folder for a library in a sketch.
        let elsewhere = self.target.dependency_root(&self.project_root);
        let recorded = installed_libraries(&elsewhere);
        let mut planned: Vec<(String, Option<String>)> = Vec::new();
        let mut order = Vec::new();

        // One library given twice: which one was meant cannot be told, so neither is taken.
        for (at, source) in sources[..requested].iter().enumerate() {
            let name = &source.library().name;
            let twins: Vec<&str> = sources[..requested]
                .iter()
                .filter(|other| other.library().name == *name)
                .map(|other| other.library().version.as_str())
                .collect();

            if twins.len() > 1
                && sources[..at]
                    .iter()
                    .all(|earlier| earlier.library().name != *name)
            {
                return Err(PlanError(format!(
                    "{name} is given {} times in this run, at {}. Give it once.",
                    twins.len(),
                    twins.join(" and ")
                )));
            }
        }

        for index in 0..requested {
            let name = sources[index].library().name.clone();

            // Given in the run, and already installed as what an earlier one needs.
            if planned.iter().any(|(found, _)| *found == name) {
                continue;
            }

            self.visit(
                sources,
                requested,
                index,
                vec![name.clone()],
                &elsewhere,
                &recorded,
                &mut planned,
                &mut order,
            )?;

            order.push(Step {
                source: index,
                needed_by: None,
                dependency: None,
                installed: None,
            });

            planned.push((name, Some(sources[index].library().version.clone())));
        }

        self.check_installed_needs(&order, sources, &recorded)?;

        Ok(order)
    }

    #[allow(clippy::too_many_arguments)]
    fn visit(
        &mut self,
        sources: &mut Vec<Source>,
        requested: usize,
        index: usize,
        chain: Vec<String>,
        elsewhere: &Path,
        recorded: &Map<String, Value>,
        planned: &mut Vec<(String, Option<String>)>,
        order: &mut Vec<Step>,
    ) -> Result<(), PlanError> {
        let name = sources[index].library().name.clone();
        let needer_root = sources[index].library().root.clone();
        let dependencies: Vec<Dependency> = sources[index]
            .library()
            .dependencies()
            .into_iter()
            .cloned()
            .collect();

        for mut dependency in dependencies {
            // By the name it goes by here: a library from the repository it names counts, whatever it is called.
            let wanted = name_for(&dependency, recorded, sources);

            if wanted != dependency.name {
                self.rename(sources, index, &mut dependency, &wanted);
            }

            if chain.contains(&dependency.name) {
                let mut circle = chain.clone();
                circle.push(dependency.name.clone());

                return Err(PlanError(format!(
                    "These libraries need each other in a circle: {}",
                    circle.join(" -> ")
                )));
            }

            // Already sorted out for an earlier library: in the project, or on its way in. It only has to be new enough for
            // this one as well.
            if let Some((_, version)) = planned.iter().find(|(found, _)| *found == dependency.name)
            {
                if !dependency.satisfied_by(version.as_deref()) {
                    return Err(PlanError(format!(
                        "{name} needs {dependency}, but {} {} is the one this run has.",
                        dependency.name,
                        version.as_deref().unwrap_or_default()
                    )));
                }

                continue;
            }

            let have = recorded
                .get(&dependency.name)
                .filter(|entry| entry.is_object())
                .cloned();
            let have_version = have
                .as_ref()
                .and_then(|entry| entry.get("version"))
                .and_then(Value::as_str)
                .map(str::to_string);
            let given = sources[..requested]
                .iter()
                .position(|source| source.library().name == dependency.name);

            // In the project and new enough: nothing to do, nothing to ask. A copy given in the run is installed anyway, as
            // asked.
            if have.is_some() && given.is_none() && dependency.satisfied_by(have_version.as_deref())
            {
                // Kept as it is, it still needs what its record says: one of those leading back here is a circle.
                let in_run: Vec<String> = sources
                    .iter()
                    .map(|source| source.library().name.clone())
                    .collect();

                if let Some(circle) = recorded_circle(&chain, &dependency.name, recorded, &in_run) {
                    return Err(PlanError(format!(
                        "These libraries need each other in a circle: {}",
                        circle.join(" -> ")
                    )));
                }

                self.ui.say(report::note(format!(
                    "{name} needs {dependency}. {} {} is already in this project, kept.",
                    dependency.name,
                    have_version.as_deref().unwrap_or_default()
                )));
                planned.push((dependency.name.clone(), have_version));
                continue;
            }

            let other = match given {
                Some(other) => other,
                None => {
                    let needer_repository = sources[index].library().repository.clone();
                    let mut source = self.fetch_dependency(
                        &dependency,
                        &name,
                        &needer_root,
                        &needer_repository,
                        have.as_ref().and(have_version.as_deref()),
                    )?;

                    // Its options before what it needs, since they can bring more. --with and --without reach it as they
                    // reach a library named.
                    let (turn_on, turn_off) = (self.turn_on.clone(), self.turn_off.clone());
                    self.choose_options(&mut source, elsewhere, &turn_on, &turn_off, false);
                    sources.push(source);
                    sources.len() - 1
                }
            };

            // What it needs, by the name the library it got has, so the record names what is installed.
            let actual = sources[other].library().name.clone();
            let mut installed = have;

            if actual != dependency.name {
                self.rename(sources, index, &mut dependency, &actual);

                // Fetched, it is a library already in the project under that name. One at a version that will do is left
                // alone, never put back to the one fetched.
                let found = recorded
                    .get(&actual)
                    .filter(|entry| entry.is_object())
                    .cloned();
                let found_version = found
                    .as_ref()
                    .and_then(|entry| entry.get("version"))
                    .and_then(Value::as_str)
                    .map(str::to_string);

                if given.is_none()
                    && found.is_some()
                    && dependency.satisfied_by(found_version.as_deref())
                {
                    self.ui.say(report::note(format!(
                        "{name} needs {dependency}. {actual} {} is already in this project, kept.",
                        found_version.as_deref().unwrap_or_default()
                    )));
                    planned.push((actual, found_version));
                    continue;
                }

                installed = installed.or(found);
            }

            if !paths::same(elsewhere, &self.project_root) && !sources[other].in_place {
                sources[other].project_root = Some(elsewhere.to_path_buf());
            }

            let version = sources[other].library().version.clone();

            if !dependency.satisfied_by(Some(&version)) {
                // A version that cannot be compared is not an old one: what is wrong with it is said.
                return Err(PlanError(match manifest::version_problem(&version) {
                    Some(why) if dependency.minimum.is_some() => format!(
                        "{name} needs {dependency}, and {}'s version cannot be compared with that. {why}.",
                        dependency.name
                    ),
                    _ => format!(
                        "{name} needs {dependency}, and the newest {} to be had is {version}.",
                        dependency.name
                    ),
                }));
            }

            // What it needs comes before it.
            let mut deeper = chain.clone();
            deeper.push(dependency.name.clone());
            self.visit(
                sources, requested, other, deeper, elsewhere, recorded, planned, order,
            )?;

            planned.push((dependency.name.clone(), Some(version)));
            order.push(Step {
                source: other,
                needed_by: Some(name.clone()),
                dependency: Some(dependency.clone()),
                installed,
            });
        }

        Ok(())
    }

    /// Call what a library of the run needs by the name it goes by in the project, in its manifest too, so the record's
    /// needs and what is said name the library as it is installed.
    fn rename(
        &mut self,
        sources: &mut [Source],
        index: usize,
        dependency: &mut Dependency,
        name: &str,
    ) {
        if let Some(library) = sources[index].library.as_mut() {
            library.rename_dependency(dependency, name);
        }

        dependency.name = name.to_string();
    }

    /// What the libraries already in the project need of those on their way in.
    ///
    /// An installed library whose record says it needs one of them at a version the run would go below stops the run before
    /// anything is written, naming it, since the build would break under it. A library going back to an older version that
    /// nothing objects to goes in, and that is said, so it is not missed.
    fn check_installed_needs(
        &mut self,
        order: &[Step],
        sources: &[Source],
        recorded: &Map<String, Value>,
    ) -> Result<(), PlanError> {
        let incoming: Vec<&str> = order
            .iter()
            .map(|step| sources[step.source].library().name.as_str())
            .collect();

        for step in order {
            let library = sources[step.source].library();
            let new = version_tuple(&library.version);

            for (other, entry) in recorded
                .iter()
                .filter(|(other, entry)| entry.is_object() && !incoming.contains(&other.as_str()))
            {
                let Some(minimum) = entry
                    .get("minimums")
                    .and_then(|minimums| minimums.get(&library.name))
                    .and_then(Value::as_str)
                else {
                    continue;
                };

                if let (Some(want), Some(have)) = (version_tuple(minimum), new)
                    && have < want
                {
                    return Err(PlanError(format!(
                        "{other} needs {name} >= {minimum}, and {name} {version} would not do. Update or uninstall {other} \
                         first.",
                        name = library.name,
                        version = library.version
                    )));
                }
            }

            let before = recorded
                .get(&library.name)
                .and_then(|entry| entry.get("version"))
                .and_then(Value::as_str);

            if let Some(before) = before
                && let (Some(old), Some(new)) = (version_tuple(before), new)
                && new < old
            {
                self.ui.say(report::note(format!(
                    "{name} {before} is in this project, and {name} {version} goes in over it, an older version.",
                    name = library.name,
                    version = library.version
                )));
            }
        }

        Ok(())
    }

    /// The folder offered for a library: where it already is, or the target's choice.
    ///
    /// Offering the name for a library installed somewhere else, such as Libs/x, used to put a second copy of it in ./x on an
    /// update.
    pub(crate) fn default_folder(&self, library: &Manifest, project_root: &Path) -> String {
        installed_libraries(project_root)
            .get(&library.name)
            .and_then(|entry| entry.get("folder"))
            .and_then(Value::as_str)
            .filter(|folder| !folder.is_empty())
            .map_or_else(
                || self.target.folder_for(library, project_root),
                str::to_string,
            )
    }

    /// The folder the target works in for a library: the run's, or the one it was given for it, such as Arduino's libraries
    /// folder for a library another one needs.
    fn root_of(&self, source: &Source) -> PathBuf {
        source
            .project_root
            .clone()
            .unwrap_or_else(|| self.project_root.clone())
    }

    /// The folder a library of the plan goes into, when that is known without asking: where it already is, for one updated
    /// because another needs a newer one, or the one the run gives it. None when it is asked for.
    fn folder_known(step: &Step, given: Option<&str>) -> Option<String> {
        // Not new to the user, so it goes where it is without a question.
        let recorded = step
            .installed
            .as_ref()
            .filter(|_| step.needed_by.is_some())
            .and_then(|entry| entry.get("folder"))
            .and_then(Value::as_str)
            .map(str::to_string);

        recorded.or_else(|| given.map(str::to_string))
    }

    /// Why a folder of the project cannot become the library where it stands, or None.
    ///
    /// A git repository is not installed in place: the scaffolding removed from it would take its .git, and the history in
    /// it, which no install can give back.
    fn in_place_refusal(&self, source: &Source) -> Option<String> {
        if !source.in_place || !source.root.join(".git").exists() {
            return None;
        }

        let place = show(&source.root, &self.root_of(source));

        Some(format!(
            "{place} is a git repository, and installing it where it stands would remove its repository files from it, \
             its .git and history among them.\nInstall from a copy outside the project instead, or from GitHub."
        ))
    }

    /// Why a library cannot go into this folder of the project, or None.
    ///
    /// The folder has to be inside the project and not the project's own, however it is written, so the record names it
    /// from the project and an uninstall can take all of it out. It cannot be another library's folder, inside one or hold
    /// one, where each would write over the other's files. And a library already installed stays in its folder, since an
    /// install into another would leave the first copy behind, out of the record and out of the build.
    fn folder_problem(
        &self,
        library: &Manifest,
        given: &str,
        destination: &Path,
        root: &Path,
        others: &Others,
    ) -> Option<String> {
        let root = paths::resolve(root);
        let destination = paths::resolve(destination);
        let name = &library.name;
        let shown = show(&destination, &root);
        let offered = self.target.folder_for(library, &root);

        if paths::same(&destination, &root) {
            return Some(format!(
                "The folder {given} is the project's own, and a library needs one of its own. Give {name} one such as \
                 {offered}."
            ));
        }

        if !paths::within(&destination, &root) {
            return Some(format!(
                "The folder {given} is outside the project. Give {name} one inside it, such as {offered}."
            ));
        }

        let recorded = installed_libraries(&root)
            .get(name)
            .and_then(|entry| entry.get("folder"))
            .and_then(Value::as_str)
            .filter(|folder| !folder.is_empty())
            .map(str::to_string);

        if let Some(recorded) = recorded
            && !paths::same(&paths::resolve(&root.join(&recorded)), &destination)
        {
            return Some(format!(
                "{name} is installed in {recorded}, and stays there: installing it into {shown} would leave a second \
                 copy behind. To move it, take it out first with libnima --uninstall {name}, then install it again."
            ));
        }

        let folder_of = |held: &Held| {
            held.folder
                .as_deref()
                .map(|folder| show(folder, &root))
                .unwrap_or_default()
        };
        // The folder the question offers, unless that is the one refused.
        let such_as = if paths::same(&paths::resolve(&root.join(&offered)), &destination) {
            String::new()
        } else {
            format!(", such as {offered}")
        };

        if let Some(held) = others.folder_of(&destination) {
            let folder = folder_of(held);

            return Some(if folder == shown {
                format!(
                    "{shown} is where {} is installed. Give {name} a folder of its own{such_as}.",
                    held.name
                )
            } else {
                format!(
                    "{shown} is inside {folder}, the folder of {}. Give {name} a folder of its own{such_as}.",
                    held.name
                )
            });
        }

        if let Some(held) = others.inside(&destination) {
            return Some(format!(
                "{shown} holds {}, the folder of {}. Give {name} a folder of its own{such_as}.",
                folder_of(held),
                held.name
            ));
        }

        None
    }

    /// Whether a library can go into the project, in this folder, checked before anything is written: a folder of the
    /// project it stands in is no repository, the project is one of the platform's, the folder is one it can have, and
    /// nothing it would write is another library's or the user's. What it will hold, or why it cannot go in.
    ///
    /// `planned` is what the libraries of the run checked before it will hold, in the project each goes into.
    pub(crate) fn checked(
        &self,
        source: &Source,
        folder: &str,
        planned: &[(PathBuf, Held)],
    ) -> Result<Prepared, String> {
        let library = source.library();
        let root = self.root_of(source);

        if let Some(why) = self.in_place_refusal(source) {
            return Err(why);
        }

        if let Some(why) = self.target.not_a_project(&root) {
            return Err(why);
        }

        let destination = if source.in_place {
            source.root.clone()
        } else {
            root.join(folder)
        };
        let others = others_for(&root, &library.name, planned);

        if let Some(why) = self.folder_problem(library, folder, &destination, &root, &others) {
            return Err(why);
        }

        installer::prepare(
            library,
            &destination,
            Some(&root),
            self.target.as_ref(),
            &others,
        )
        .map_err(|InstallError(message)| message)
    }

    /// The folder a library is checked in before it is installed: where it stands, the one known without asking, or the one
    /// the question offers.
    pub(crate) fn folder_to_check(&self, source: &Source, known: Option<String>) -> String {
        let root = self.root_of(source);

        if source.in_place {
            return show(&source.root, &root);
        }

        known.unwrap_or_else(|| self.default_folder(source.library(), &root))
    }

    /// Every check of every library of the run, made before the first of them is written, so a run that is refused writes
    /// nothing at all. Each is checked in the folder it goes into as far as that is known without asking: the one given,
    /// the one it is in, or the one the question offers. What each will hold, for those after it to be checked against, or
    /// the exit code to stop with, the reason shown.
    fn check_all(
        &mut self,
        order: &[Step],
        sources: &[Source],
        request: &Request,
    ) -> Result<Vec<(PathBuf, Held)>, i32> {
        let mut planned: Vec<(PathBuf, Held)> = Vec::new();

        for step in order {
            let source = &sources[step.source];
            let name = source.library().name.clone();
            let known = Session::folder_known(step, folder_given(request, step, &name));
            let folder = self.folder_to_check(source, known);

            match self.checked(source, &folder, &planned) {
                Ok(prepared) => planned.push((self.root_of(source), prepared.held(&name))),
                Err(why) => {
                    self.ui.error(&why);
                    return Err(2);
                }
            }
        }

        Ok(planned)
    }

    /// Ask where a library should go, or take the default when nobody can answer.
    fn ask_folder(&mut self, default: &str, name: &str) -> String {
        let question = Question {
            prompt: report::line(
                format!("Folder to install {name} into [{default}]: "),
                Style::Bold,
            ),
            kind: QuestionKind::Text,
        };

        self.ui
            .ask(&question)
            .filter(|answer| !answer.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    /// Install one library of the plan. Returns an exit code.
    fn install_step(
        &mut self,
        step: &Step,
        source: &Source,
        folder: Option<&str>,
        planned: &[(PathBuf, Held)],
    ) -> i32 {
        let library = source.library();
        let root = self.root_of(source);
        let others = others_for(&root, &library.name, planned);
        let have = step
            .installed
            .as_ref()
            .and_then(|entry| entry.get("version"))
            .and_then(Value::as_str);

        self.ui.new_part();

        if let (Some(needed_by), Some(dependency)) = (&step.needed_by, &step.dependency) {
            self.ui.say(report::blank());

            // What is in the project will do, so the plan only has this step because a copy was given in the run. It is not
            // an update anybody needs.
            self.ui.say(report::note(match (&step.installed, dependency.satisfied_by(have)) {
                (Some(_), true) => format!(
                    "{needed_by} needs {dependency}, and the {} in this project will do. {} {} was given on the command line, \
                     so it is installed over that one first.",
                    have.unwrap_or_default(),
                    library.name,
                    library.version
                ),
                (Some(_), false) => format!(
                    "{needed_by} needs {dependency}, newer than the {} in this project. Updating {} first.",
                    have.unwrap_or_default(),
                    library.name
                ),
                (None, _) => format!(
                    "{needed_by} needs {dependency}, which is not in this project. Installing {} first.",
                    library.name
                ),
            }));
        }

        self.ui.say(report::blank());
        self.ui.say_all(report::banner(
            &library.name,
            &library.version,
            &library.description,
        ));
        let summary = library.summary();

        if !summary.is_empty() {
            self.ui.say(report::note(summary));
        }

        let findings = self.target.check(library, &root, self.ui);
        say_requirements(&findings, self.ui);

        let result = if source.in_place {
            installer::install_in_place(library, &root, self.target.as_ref(), &others)
        } else {
            self.ui.say(report::blank());

            // A library being updated because another needs a newer one goes where it already is, without a question: it is
            // not new to the user. Anything else is asked for, or given with --dir.
            let chosen = match Session::folder_known(step, folder) {
                Some(known) => known,
                None => {
                    let default = self.default_folder(library, &root);
                    self.ask_folder(&default, &library.name)
                }
            };

            // Checked before the run in the folder the question offers. Another answer is checked here, before anything of
            // this library is written.
            if let Some(why) =
                self.folder_problem(library, &chosen, &root.join(&chosen), &root, &others)
            {
                self.ui.error(&why);
                return 2;
            }

            installer::install_to(
                library,
                &root.join(chosen),
                Some(&root),
                self.target.as_ref(),
                &others,
                true,
            )
        };

        match result {
            Ok(result) => {
                // Which libraries it came in for: the one that needs it, when this run brought it in for that one. One that
                // was there already came in for whatever it came in for then, and one installed on its own for none.
                let brought: Vec<Value> = match (&step.needed_by, &step.installed) {
                    (None, _) => Vec::new(),
                    (Some(needed_by), None) => vec![json!(needed_by)],
                    (Some(_), Some(entry)) => entry
                        .get("brought_by")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                };

                installer::note(&root, &library.name, "brought_by", Value::Array(brought));

                self.finish(library, &result, &root)
            }
            Err(InstallError(message)) => {
                self.ui.error(&message);
                2
            }
        }
    }

    /// The part shared by every install route. Returns 1 when a run step failed.
    fn finish(&mut self, library: &Manifest, result: &InstallResult, project_root: &Path) -> i32 {
        say_files(result, project_root, library, self.ui);

        let dropped = Dropped {
            files: result.dropped.clone(),
            defines: result.dropped_defines.clone(),
        };
        let outcomes = self.target.integrate(
            project_root,
            library,
            &result.destination,
            &dropped,
            self.ui,
        );

        if outcomes.is_empty() {
            self.ui.say(report::blank());
            self.ui.say(report::warn(self.target.no_project()));
            self.ui
                .say(report::note(format!("  {}", self.target.no_project_hint())));
        } else {
            say_outcomes(&outcomes, project_root, self.ui);
        }

        // Nothing at all added it to a build: what is left to the user is said above.
        let built = outcomes
            .iter()
            .any(|outcome| outcome.status != Status::Manual);

        // The blocks the target wrote, so the next install can tell an edit inside one.
        let mut written = Map::new();

        for outcome in &outcomes {
            for (name, found) in &outcome.blocks {
                match (written.get_mut(name), found.as_object()) {
                    (Some(Value::Object(have)), Some(found)) => {
                        have.extend(
                            found
                                .iter()
                                .map(|(key, value)| (key.clone(), value.clone())),
                        );
                    }
                    _ => {
                        written.insert(name.clone(), found.clone());
                    }
                }
            }
        }

        if !written.is_empty() {
            installer::note(
                project_root,
                &library.name,
                "blocks",
                Value::Object(written),
            );
        }

        let code = self.run_steps(library, result, project_root);

        self.say_next(result, project_root, library, built, code == 0);

        code
    }

    /// The library's run steps, once everything else is in, and what ran recorded so an update does not run it again. Returns
    /// 1 when one failed, else 0.
    fn run_steps(
        &mut self,
        library: &Manifest,
        result: &InstallResult,
        project_root: &Path,
    ) -> i32 {
        if library.run.is_empty() {
            return 0;
        }

        // Nowhere to run them, such as Arduino's libraries folder, which belongs to no sketch: left unrun, as a no would leave
        // them, with the way to run them.
        let Some(folder) = self.target.once_root(project_root) else {
            self.ui.say(report::blank());
            self.ui.say(report::heading("Run"));

            for step in &library.run {
                self.ui.say(vec![
                    span(format!("  {:<12} ", step.label), Style::Plain),
                    span(
                        "not run, there is no project folder here to run it in",
                        Style::Note,
                    ),
                ]);
            }

            self.ui.say(report::note(format!(
                "               {}",
                self.target.no_once_root()
            )));

            return 0;
        };

        let recorded = installed_libraries(project_root)
            .get(&library.name)
            .and_then(|entry| entry.get("ran"))
            .cloned();
        let outcomes = steps::run(
            library,
            &folder,
            &result.destination,
            &self.choices,
            recorded.as_ref(),
            self.target.platform(),
            self.ui,
        );

        let mut ran = recorded
            .and_then(|ran| ran.as_object().cloned())
            .unwrap_or_default();

        for outcome in &outcomes {
            if outcome.status == steps::Status::Ran {
                ran.insert(outcome.label.clone(), json!(outcome.fingerprint));
            }
        }

        installer::note(project_root, &library.name, "ran", Value::Object(ran));

        let failed: Vec<&str> = outcomes
            .iter()
            .filter(|outcome| outcome.status == steps::Status::Failed)
            .map(|outcome| outcome.label.as_str())
            .collect();

        if failed.is_empty() {
            return 0;
        }

        self.ui.say(report::blank());
        self.ui.say(report::warn(format!(
            "{} did not finish. The library is installed; run {} again with --run once the reason above is put right.",
            failed.join(", "),
            if failed.len() > 1 { "them" } else { "it" }
        )));

        1
    }

    /// The last word, which is the one people actually read. Done only when the library is in a build and every step it ran
    /// finished: otherwise what is left to do.
    fn say_next(
        &mut self,
        result: &InstallResult,
        root: &Path,
        library: &Manifest,
        built: bool,
        steps_done: bool,
    ) {
        let header = self
            .target
            .include_line(library, &result.destination, root)
            .or_else(|| arduino::properties::include_name(library));

        self.ui.say(report::blank());

        if built && steps_done {
            self.ui.say(report::good(match header {
                Some(header) => format!("Done. #include \"{header}\" and you are away."),
                None => "Done.".to_string(),
            }));
        } else if !steps_done {
            self.ui.say(report::warn(
                "Installed, and a step it runs did not finish, as said above: put that right before you build.",
            ));
        } else {
            self.ui.say(report::warn(match header {
                Some(header) => format!(
                    "Installed, and in no build yet: add it as said above, then #include \"{header}\"."
                ),
                None => "Installed, and in no build yet: add it as said above.".to_string(),
            }));
        }

        if result.was_update() {
            self.ui.say(report::note(
                "This was an update. The library replaced, what was yours kept.",
            ));
        }

        if !result.backups.is_empty() {
            self.ui.say(report::warn(
                "Something of yours could not be carried over. Each copy marked saved holds the file as it was: take what you \
                 need from it, then delete it.",
            ));
        }

        for warning in &library.warnings {
            self.ui.say(report::note(format!("  manifest: {warning}")));
        }
    }

    /// Get every library onto disk, work out the order, check each, then install them in it. Returns an exit code.
    pub fn install(&mut self, request: &Request) -> i32 {
        if request.folder.is_some() && request.libraries.len() > 1 {
            self.ui.error(
                "--dir names the folder for one library. Give one library with it, or leave it out and answer the question for \
                 each.",
            );
            return 2;
        }

        let mut sources: Vec<Source> = Vec::new();

        for given in &request.libraries {
            let Some(mut source) = self.stage(given, request.reference.as_deref()) else {
                return 2;
            };

            if let Some(why) = self.in_place_refusal(&source) {
                self.ui.error(&why);
                return 2;
            }

            if source.in_place && request.folder.is_some() {
                let place = show(&source.root, &self.root_of(&source));

                self.ui.error(&format!(
                    "{place} is already in the project, so it becomes the library where it is and --dir has nothing to do.\n\
                     Rename the folder instead, or install from a copy outside the project."
                ));
                return 2;
            }

            if let Some(code) = self.load(&mut source) {
                return code;
            }

            sources.push(source);
        }

        // A ref is a ref of a repository: given with nothing fetched from GitHub, it would change nothing without a word.
        if let Some(reference) = &request.reference
            && !sources
                .iter()
                .any(|source| source.origin.get("kind").and_then(Value::as_str) == Some("github"))
        {
            self.ui.error(&format!(
                "--ref {reference} is for a library fetched from GitHub, and none of these is, so it would change nothing. \
                 Leave it out, or name the library by its repository."
            ));
            return 2;
        }

        // Before the plan, since an option taken can bring libraries of its own.
        if let Some(code) = self.settle_options(&mut sources, request) {
            return code;
        }

        let requested = sources.len();

        let order = match self.plan(&mut sources, requested) {
            Ok(order) => order,
            Err(PlanError(message)) => {
                self.ui.error(&message);
                return 2;
            }
        };

        // Every library of the run is known now, those it needs among them: what the flags name has to be one of theirs.
        if let Some(code) = self.unknown_names(&sources, request) {
            return code;
        }

        let planned = match self.check_all(&order, &sources, request) {
            Ok(planned) => planned,
            Err(code) => return code,
        };

        let mut worst = 0;
        let mut went_in: Vec<String> = Vec::new();

        for step in &order {
            let source = sources[step.source].clone();
            let folder = folder_given(request, step, &source.library().name);
            let code = self.install_step(step, &source, folder, &planned);

            // 1 is a run step that did not finish: the library is in, and so can the rest be. Anything else stops the run.
            if code != 0 && code != 1 {
                // Past the checks before the run, only what they could not know stops it with others in already: a folder
                // answered to a question, or a file that could not be written.
                if !went_in.is_empty() {
                    self.ui.say(report::warn(format!(
                        "{} went in before this, and {} in the project.",
                        went_in.join(", "),
                        if went_in.len() > 1 { "stay" } else { "stays" }
                    )));
                }

                return code;
            }

            went_in.push(source.library().name.clone());
            worst = worst.max(code);
        }

        worst
    }

    /// Take the libraries named after --uninstall out of the project. Returns an exit code.
    pub fn uninstall(&mut self, names: &[String]) -> i32 {
        for name in uninstall_order(names, &self.project_root) {
            self.ui.new_part();

            match installer::uninstall(&self.project_root, &name, self.target.as_mut(), names) {
                Ok(removal) => say_removal(&removal, &self.project_root, self.ui),
                Err(InstallError(message)) => {
                    self.ui.error(&message);
                    return 2;
                }
            }
        }

        0
    }
}

/// The name a library a requirement asks for goes by here.
///
/// The requirement's own when a library of that name is installed or in the run. Otherwise that of a library installed, or
/// in the run, that comes from the repository the requirement names, whatever it is called: the repository its libnima.yml
/// names, or the one it was fetched from. One in the run comes before one installed, as it goes in anyway. The requirement's
/// own name when none does, which then has to be fetched.
fn name_for(dependency: &Dependency, recorded: &Map<String, Value>, sources: &[Source]) -> String {
    let named = recorded.get(&dependency.name).is_some_and(Value::is_object)
        || sources
            .iter()
            .any(|source| source.library().name == dependency.name);

    let Some(wanted) = manifest::repository_key(&dependency.source).filter(|_| !named) else {
        return dependency.name.clone();
    };

    let from = |repository: Option<&str>, origin: Option<&str>| {
        [repository, origin]
            .into_iter()
            .flatten()
            .filter_map(manifest::repository_key)
            .any(|key| key == wanted)
    };

    let in_run = sources.iter().find(|source| {
        from(
            Some(source.library().repository.as_str()),
            source.origin.get("source").and_then(Value::as_str),
        )
    });

    if let Some(source) = in_run {
        return source.library().name.clone();
    }

    recorded
        .iter()
        .filter(|(_, entry)| entry.is_object())
        .find(|(_, entry)| {
            from(
                entry.get("repository").and_then(Value::as_str),
                entry
                    .get("origin")
                    .and_then(|origin| origin.get("source"))
                    .and_then(Value::as_str),
            )
        })
        .map_or_else(|| dependency.name.clone(), |(name, _)| name.clone())
}

/// A circle a library kept as it is closes with the run's chain that led to it: its record's needs, and theirs, leading
/// back into the chain. None when there is none.
///
/// A library of the run needs what the run says it needs, not what its record says, so the walk does not go through one:
/// its own turn in the plan looks there.
fn recorded_circle(
    chain: &[String],
    kept: &str,
    recorded: &Map<String, Value>,
    in_run: &[String],
) -> Option<Vec<String>> {
    let needs_of = |name: &str| -> Vec<String> {
        recorded
            .get(name)
            .and_then(|entry| entry.get("needs"))
            .and_then(Value::as_array)
            .map(|needs| {
                needs
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut paths_left: Vec<Vec<String>> = vec![vec![kept.to_string()]];
    let mut seen: Vec<String> = vec![kept.to_string()];

    while let Some(path) = paths_left.pop() {
        let last = path.last().expect("never empty").clone();

        for need in needs_of(&last) {
            if let Some(at) = chain.iter().position(|name| *name == need) {
                let mut circle = chain[at..].to_vec();
                circle.extend(path.iter().cloned());
                circle.push(need);

                return Some(circle);
            }

            if in_run.contains(&need) || seen.contains(&need) {
                continue;
            }

            seen.push(need.clone());

            let mut longer = path.clone();
            longer.push(need);
            paths_left.push(longer);
        }
    }

    None
}

/// The name GitHub's Download ZIP gives a library's zip: its repository's name and the branch, main for the default one,
/// or the tag without its v, as GitHub names a release's. A library with no repository known goes by its own name.
fn zip_name(repository: &str, reference: Option<&str>, name: &str) -> String {
    let repo = download::split_ref(repository)
        .ok()
        .filter(|_| !repository.trim().is_empty())
        .map_or_else(|| name.to_string(), |(_, repo, _)| repo);
    let at = match reference {
        Some(reference) => match reference.strip_prefix('v') {
            Some(rest) if rest.starts_with(|first: char| first.is_ascii_digit()) => rest,
            _ => reference,
        },
        None => "main",
    };

    format!("{repo}-{at}.zip")
}

/// The folder the run gives a library of its plan, with --dir or by its name in the window's list, or None.
fn folder_given<'a>(request: &'a Request, step: &Step, name: &str) -> Option<&'a str> {
    let named = request
        .folders
        .iter()
        .find(|(found, _)| found == name)
        .map(|(_, folder)| folder.as_str());

    if step.needed_by.is_none() {
        request.folder.as_deref().or(named)
    } else {
        named
    }
}

/// The libraries a library of a run is checked against: those the record of its project has, and the others of the run that
/// go into the same project, with what each will hold.
fn others_for(root: &Path, name: &str, planned: &[(PathBuf, Held)]) -> Others {
    let mut others = Others::recorded(root, name);
    let root = paths::resolve(root);

    others.held.extend(
        planned
            .iter()
            .filter(|(at, held)| held.name != name && paths::same(&paths::resolve(at), &root))
            .map(|(_, held)| held.clone()),
    );

    others
}

fn sorted(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut found: Vec<String> = items.collect();
    found.sort();
    found.dedup();

    found
}

/// What the library needs, and whether the project looks ready for it.
pub fn say_requirements(findings: &[Finding], ui: &mut dyn Ui) {
    if findings.is_empty() {
        return;
    }

    ui.say(report::blank());
    ui.say(report::heading("Requirements"));

    for finding in findings {
        let (mark, style) = match finding.level {
            Level::Ok => ("ok", Style::Good),
            Level::Warn => ("check", Style::Warn),
            Level::Info => ("note", Style::Note),
        };

        ui.say(item(mark, "", finding.message.clone(), style));

        if !finding.hint.is_empty() && finding.level != Level::Ok {
            ui.say(report::detail(finding.hint.clone()));
        }
    }
}

/// Every file written, created, or deliberately left alone.
fn say_files(result: &InstallResult, root: &Path, library: &Manifest, ui: &mut dyn Ui) {
    // The files of an option not taken, by where they would land, so a file that went because the option was turned off says
    // that rather than that the library dropped it.
    let mut unchosen: Vec<(PathBuf, &str)> = Vec::new();

    for option in &library.options {
        if !library.chosen.contains(&option.name) {
            for entry in option.code_files() {
                unchosen.push((
                    paths::resolve(&result.destination.join(&entry.destination)),
                    &option.name,
                ));
            }
        }
    }

    ui.say(report::blank());
    ui.say(report::heading("Files"));

    for path in result.installed.iter().chain(&result.placed) {
        if result.unchanged.contains(path) {
            ui.say(item("same", "as it was", show(path, root), Style::Plain));
        } else {
            ui.say(item("written", "", show(path, root), Style::Good));
        }
    }

    for path in &result.yours_created {
        ui.say(item(
            "created",
            "yours from now on",
            show(path, root),
            Style::Accent,
        ));
    }

    for path in &result.yours_kept {
        // Its template changed since it was made: said, for the user to compare and take what they want.
        let note = match result
            .templates_changed
            .iter()
            .find(|(file, _)| file == path)
        {
            Some((_, template)) => format!(
                "yours; {}'s {template} changed since it was made",
                library.name
            ),
            None => "yours".to_string(),
        };

        ui.say(item("kept", &note, show(path, root), Style::Accent));
    }

    // A file that took the user's sections to its new place says so. One with none to take is only an old copy gone, and is
    // reported as removed.
    let carried: Vec<PathBuf> = result
        .carried
        .iter()
        .filter(|carried| !carried.kept.is_empty())
        .map(|carried| paths::resolve(&carried.from))
        .collect();

    for carry in &result.carried {
        if !carry.kept.is_empty() {
            ui.say(item(
                "moved",
                &format!("your USER CODE {} went with it", carry.kept.join(", ")),
                format!("{} -> {}", show(&carry.from, root), show(&carry.to, root)),
                Style::Accent,
            ));
        }
    }

    for path in &result.dropped {
        let resolved = paths::resolve(path);

        if carried
            .iter()
            .any(|carried| paths::same(carried, &resolved))
        {
            continue;
        }

        let why = match unchosen
            .iter()
            .find(|(found, _)| paths::same(found, &resolved))
        {
            Some((_, option)) => format!("option {option} is off"),
            None => "no longer part of the library".to_string(),
        };

        ui.say(item("removed", &why, show(path, root), Style::Warn));
    }

    let moved_with: Vec<PathBuf> = result
        .carried
        .iter()
        .filter(|carried| !carried.kept.is_empty())
        .map(|carried| paths::resolve(&carried.to))
        .collect();

    for (path, names) in &result.preserved {
        // Already said on its "moved" line.
        if moved_with
            .iter()
            .any(|found| paths::same(found, &paths::resolve(path)))
        {
            continue;
        }

        ui.say(item(
            "kept",
            &format!("your USER CODE {}", names.join(", ")),
            show(path, root),
            Style::Accent,
        ));
    }

    for saved in &result.backups {
        ui.say(item_spans(
            "saved",
            "",
            vec![
                span(show(&saved.copy, root), Style::Plain),
                span("  ", Style::Plain),
                span(saved.reason.clone(), Style::Warn),
            ],
            Style::Warn,
        ));
    }

    if !result.removed.is_empty() {
        let mut removed = result.removed.clone();
        removed.sort();

        ui.say(report::blank());
        ui.say(report::note(
            "  Removed, since they are part of the repository, not the firmware:",
        ));
        ui.say(report::note(format!("    {}", removed.join(", "))));
    }
}

/// Each project file changed, or left for the user, under its heading. A copy is named by its path from the project's
/// folder, so the module's own copy does not read as one of the application's file of the same name.
pub fn say_outcomes(outcomes: &[Outcome], root: &Path, ui: &mut dyn Ui) {
    ui.say(report::blank());
    ui.say(report::heading("Project"));

    for outcome in outcomes {
        let (mark, style) = match outcome.status {
            Status::Changed => ("updated", Style::Good),
            Status::Already => ("ok", Style::Good),
            Status::Manual => ("manual", Style::Warn),
        };
        let text: Vec<Span> = vec![
            span(outcome.ide.clone(), Style::Bold),
            span("  ", Style::Plain),
            span(outcome.message.clone(), Style::Plain),
        ];

        ui.say(item_spans(mark, "", text, style));

        for step in &outcome.steps {
            ui.say(report::detail(step.clone()));
        }

        if let Some(backup) = &outcome.backup {
            ui.say(report::detail(format!("backup: {}", show(backup, root))));
        }
    }
}

/// The libraries to take out, each before any of the others it needs.
///
/// ee24 needs osal, so "--uninstall osal ee24" takes ee24 out first, and osal is then needed by nothing. A circle, which the
/// record should never hold, keeps the order given.
fn uninstall_order(names: &[String], project_root: &Path) -> Vec<String> {
    let installed = installed_libraries(project_root);
    let mut left: Vec<String> = Vec::new();

    for name in names {
        if !left.contains(name) {
            left.push(name.clone());
        }
    }

    let needs = |other: &str, name: &str| {
        installed
            .get(other)
            .and_then(|entry| entry.get("needs"))
            .and_then(Value::as_array)
            .is_some_and(|needs| needs.iter().any(|need| need.as_str() == Some(name)))
    };

    let mut order = Vec::new();

    while !left.is_empty() {
        let ready = left
            .iter()
            .position(|name| !left.iter().any(|other| other != name && needs(other, name)))
            .unwrap_or(0);

        order.push(left.remove(ready));
    }

    order
}

/// What an uninstall did, and what it left.
fn say_removal(removal: &Removal, root: &Path, ui: &mut dyn Ui) {
    let library = &removal.library;
    let folder = library
        .destination
        .as_deref()
        .map(|destination| show(destination, root))
        .unwrap_or_default();

    ui.say(report::blank());
    ui.say_all(report::banner(
        &library.name,
        &library.version,
        &format!("out of {folder}"),
    ));

    if !removal.outcomes.is_empty() {
        say_outcomes(&removal.outcomes, root, ui);
    }

    ui.say(report::blank());
    ui.say(report::heading("Files"));

    for path in &removal.removed {
        ui.say(item("removed", "", show(path, root), Style::Warn));
    }

    for saved in &removal.backups {
        ui.say(item_spans(
            "saved",
            "",
            vec![
                span(show(&saved.copy, root), Style::Plain),
                span("  ", Style::Plain),
                span(saved.reason.clone(), Style::Warn),
            ],
            Style::Accent,
        ));
    }

    // A copy this uninstall made of a file of the project's, the module's USER CODE among them, is the user's, saved.
    let made_here = |path: &PathBuf| {
        removal
            .outcomes
            .iter()
            .filter_map(|outcome| outcome.backup.as_ref())
            .any(|copy| paths::same(copy, path))
    };
    let others: Vec<&PathBuf> = removal
        .left
        .iter()
        .filter(|path| {
            !removal
                .backups
                .iter()
                .any(|saved| paths::same(&saved.copy, path))
        })
        .collect();

    for path in &others {
        if made_here(path) {
            ui.say(item(
                "kept",
                "yours, what you wrote in it, saved",
                show(path, root),
                Style::Accent,
            ));
        } else {
            ui.say(item(
                "kept",
                "not the library's",
                show(path, root),
                Style::Accent,
            ));
        }
    }

    for path in &removal.yours {
        ui.say(item(
            "kept",
            "yours, not removed",
            show(path, root),
            Style::Accent,
        ));
    }

    ui.say(report::blank());
    ui.say(report::good(format!(
        "Done. {} is out of the project.",
        library.name
    )));

    if !removal.folder_removed && library.destination.as_deref().is_some_and(Path::is_dir) {
        let foreign = others.iter().any(|path| !made_here(path));

        ui.say(report::note(if foreign {
            format!("{folder} stays, for the files above that were not the library's.")
        } else {
            format!("{folder} stays, for what was saved above. Delete it once you have taken what you need.")
        }));
    }

    for name in &removal.unneeded {
        ui.say(report::note(format!(
            "{name} came in for {}, and nothing here needs it now. Take it out too with: libnima --uninstall {name}",
            library.name
        )));
    }
}

/// A whole run. Returns a process exit code.
pub fn run(request: &Request, ui: &mut dyn Ui) -> i32 {
    run_with(Client::new(request.key.as_deref()), request, ui)
}

/// A whole run, with what reaches GitHub given.
pub fn run_with(mut client: Client, request: &Request, ui: &mut dyn Ui) -> i32 {
    // One copy a file for the whole run, of the file as it was before it.
    let _copies = crate::files::run_copies();

    if request.libraries.is_empty() && request.uninstall.is_empty() {
        let given = request
            .platform
            .map(|platform| format!("--platform {} ", platform.name()))
            .unwrap_or_default();

        ui.error(&format!(
            "Say what to install, for example: libnima {given}owner/example"
        ));

        return 2;
    }

    let folder = match &request.project {
        Some(project) => paths::resolve(project),
        None => paths::resolve(Path::new(".")),
    };

    // A mistyped folder is said to be one, rather than made and given a library no build will ever find.
    if let Some(project) = &request.project
        && !folder.is_dir()
    {
        let shown = paths::posix(project);

        ui.error(&if folder.exists() {
            format!("{shown}, given with --project, is a file. Give the folder of your project.")
        } else {
            format!("{shown}, given with --project, does not exist.")
        });

        return 2;
    }

    // Nothing was ever installed here: said so, rather than asked which platform it is. A sketch's libraries are
    // recorded in the sketchbook, so a sketch is left to its target.
    let sketch = request.platform == Some(Platform::Arduino)
        || detect::detect(&folder)
            .candidates
            .iter()
            .any(|candidate| candidate.platform == Platform::Arduino);

    if !request.uninstall.is_empty()
        && request.libraries.is_empty()
        && !sketch
        && !folder.join(record::RECORD_NAME).is_file()
        && Platform::ALL
            .iter()
            .all(|platform| installer::legacy_of(&folder, *platform).is_none())
    {
        ui.error(&format!(
            "{} {} not installed in this project: it has no {}, the record of what libNIMA installed.",
            request.uninstall.join(", "),
            if request.uninstall.len() > 1 { "are" } else { "is" },
            record::RECORD_NAME
        ));
        return 2;
    }

    let Some(platform) = choose_platform(request, &folder, &mut client, ui) else {
        return 2;
    };

    // Which platform, and what told it, once at the top of every run on a project.
    let told = match request.platform {
        Some(_) => "given with --platform".to_string(),
        None => detect::detect(&folder)
            .candidates
            .iter()
            .find(|candidate| candidate.platform == platform)
            .map_or_else(
                || "as the libraries' libnima.yml say".to_string(),
                |candidate| candidate.evidence.clone(),
            ),
    };

    ui.say(report::note(format!("{}: {told}.", project_word(platform))));

    if !check_platform_args(&request.args, platform, ui) {
        return 2;
    }

    if !request.uninstall.is_empty() && !request.libraries.is_empty() {
        ui.error("--uninstall takes libraries out. Install on a run of its own.");
        return 2;
    }

    let mut args = request.args.clone();
    args.uninstalling = request.uninstall.clone();

    // A check of a project an installer before libNIMA left reads it as the install will, once it is moved over: on a copy
    // moved over, since a check writes nothing.
    let mut copy = None;
    let folder = match installer::legacy_of(&folder, platform) {
        Some((_, from)) if request.check_only => match moved_over_copy(&folder, platform, &args) {
            Some(copied) => {
                ui.say(report::note(format!(
                    "Installed by {from}: the install moves the project over to libnima first, and it is checked here as it \
                     will be then, on a copy. Nothing in it is changed."
                )));
                copy = Some(copied.clone());
                copied
            }
            None => folder,
        },
        _ => folder,
    };

    let moving = !request.check_only;
    let mut session = match Session::open(client, platform, &args, &folder, moving, ui) {
        Ok(session) => session,
        Err(Stop(code)) => return code,
    };

    if !request.uninstall.is_empty() {
        return session.uninstall(&request.uninstall);
    }

    if request.check_only {
        let code = check::run(&mut session, request);

        drop(session);

        if let Some(copy) = copy {
            download::cleanup(&copy);
        }

        return code;
    }

    session.install(request)
}

/// What a project of the platform is called.
fn project_word(platform: Platform) -> &'static str {
    match platform {
        Platform::Stm32 => "STM32 project",
        Platform::Zephyr => "Zephyr application",
        Platform::Esp32 => "ESP-IDF project",
        Platform::Arduino => "Arduino sketch",
        Platform::Pio => "PlatformIO project",
    }
}

/// A copy of a project an installer before libNIMA left, moved over as the install would move it, for a check to read.
/// What a build makes, and the folders hidden from a listing, such as .git, are left out, being no part of it.
fn moved_over_copy(folder: &Path, platform: Platform, args: &PlatformArgs) -> Option<PathBuf> {
    let copy = download::new_staging().ok()?;

    for relative in paths::files_below(folder) {
        let parts: Vec<&str> = relative.split('/').collect();
        let skipped =
            parts.len() > 1 && (parts[0].starts_with('.') || parts[0].starts_with("build"));

        if skipped {
            continue;
        }

        let target = copy.join(&relative);

        if target
            .parent()
            .is_some_and(|parent| std::fs::create_dir_all(parent).is_err())
            || std::fs::copy(folder.join(&relative), &target).is_err()
        {
            download::cleanup(&copy);
            return None;
        }
    }

    let target = target_of(platform, args);

    installer::migrate(&copy, target.as_ref());

    Some(copy)
}
