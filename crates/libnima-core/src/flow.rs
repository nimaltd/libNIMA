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
//!   it, because STM32CubeIDE compiles every .c under the project and a test harness brings a second main() with it.
//!
//! Either way the work happens in the same order: check the project against what the library needs, copy the files, then
//! register them with the project. The checks and the registering are the target's, see target.rs.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::check;
use crate::detect;
use crate::download::{self, Client, DownloadError};
use crate::installer::{self, InstallError, InstallResult, Removal};
use crate::manifest::{self, Dependency, MANIFEST_NAME, Manifest};
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

fn is_url(given: &str) -> bool {
    given.starts_with("http://") || given.starts_with("https://")
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
            Kind::Zip => download::peek_zip(&path),
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
                    let shown: Vec<String> = moved.iter().map(|path| show(path, &root)).collect();

                    ui.say(report::note(format!(
                        "Moved over from {from} to libnima: {}. Each was saved first as a .bak.",
                        shown.join(", ")
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
                // Installing in place removes libnima.yml, so the most likely reason it is missing there is that this
                // folder has already been installed once.
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
    /// A name given to --with or --without that none of them has is refused before anything is asked or installed, since it
    /// is almost always a typo, and going ahead without it would install something other than what was asked for. None, or
    /// the exit code to stop with.
    fn settle_options(&mut self, requested: &mut [Source], request: &Request) -> Option<i32> {
        let offered = sorted(requested.iter().flat_map(|source| {
            source
                .library()
                .options
                .iter()
                .map(|option| option.name.clone())
        }));
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

        let unknown = sorted(
            request
                .with_options
                .iter()
                .chain(&request.without_options)
                .filter(|name| !offered.contains(name))
                .cloned(),
        );

        if !unknown.is_empty() {
            let has = if offered.is_empty() {
                "What is installed here has no options.".to_string()
            } else {
                format!("What is installed here has: {}.", offered.join(", "))
            };

            self.ui
                .error(&format!("No option called {}. {has}", unknown.join(", ")));
            return Some(2);
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

        self.settle_run(requested, request)
    }

    /// Check --run and --no-run against the steps of the libraries named, the way --with and --without are checked. None,
    /// or the exit code to stop with.
    fn settle_run(&mut self, requested: &[Source], request: &Request) -> Option<i32> {
        self.choices = StepChoices {
            turn_on: request.run_on.clone(),
            turn_off: request.run_off.clone(),
            assume_yes: request.yes,
        };

        let offered = sorted(
            requested
                .iter()
                .flat_map(|source| source.library().run.iter().map(|step| step.label.clone())),
        );
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
    fn fetch_dependency(
        &mut self,
        dependency: &Dependency,
        needed_by: &str,
        needer_root: &Path,
    ) -> Result<Source, PlanError> {
        self.ui.say(report::note(format!(
            "{needed_by} needs {dependency}. Fetching {} ...",
            dependency.fetch()
        )));

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
                PlanError(format!(
                    "{needed_by} needs {dependency}, which is not in this project, and it could not be fetched: {}
                     Without the internet, download {name} as well, and give both zips on one line:
                         libnima {needed_by}-main.zip {name}-main.zip",
                    error.message,
                    name = dependency.name,
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

        for dependency in dependencies {
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
                    let mut source = self.fetch_dependency(&dependency, &name, &needer_root)?;

                    // Its options before what it needs, since they can bring more.
                    self.choose_options(&mut source, elsewhere, &[], &[], false);
                    sources.push(source);
                    sources.len() - 1
                }
            };

            if !paths::same(elsewhere, &self.project_root) && !sources[other].in_place {
                sources[other].project_root = Some(elsewhere.to_path_buf());
            }

            let version = sources[other].library().version.clone();

            if !dependency.satisfied_by(Some(&version)) {
                return Err(PlanError(format!(
                    "{name} needs {dependency}, and the newest {} to be had is {version}.",
                    dependency.name
                )));
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
                installed: have,
            });
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
    fn install_step(&mut self, step: &Step, source: &Source, folder: Option<&str>) -> i32 {
        let library = source.library();
        let root = source
            .project_root
            .clone()
            .unwrap_or_else(|| self.project_root.clone());
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
        self.ui.say(report::note(library.summary()));

        let findings = self.target.check(library, &root, self.ui);
        say_requirements(&findings, self.ui);

        let result = if source.in_place {
            installer::install_in_place(library, &root, self.target.as_ref())
        } else {
            self.ui.say(report::blank());

            // A library being updated because another needs a newer one goes where it already is, without a question: it is
            // not new to the user. Anything else is asked for, or given with --dir.
            let recorded = step
                .installed
                .as_ref()
                .filter(|_| step.needed_by.is_some())
                .and_then(|entry| entry.get("folder"))
                .and_then(Value::as_str)
                .map(str::to_string);

            let chosen = match (recorded, folder) {
                (Some(recorded), _) => recorded,
                (None, Some(folder)) => folder.to_string(),
                (None, None) => {
                    let default = self.default_folder(library, &root);
                    self.ask_folder(&default, &library.name)
                }
            };

            installer::install_to(
                library,
                &root.join(chosen),
                Some(&root),
                self.target.as_ref(),
                true,
            )
        };

        match result {
            Ok(result) => self.finish(library, &result, &root),
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
            say_outcomes(&outcomes, self.ui);
        }

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

        self.say_next(result, project_root, library);

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

    /// The last word, which is the one people actually read.
    fn say_next(&mut self, result: &InstallResult, root: &Path, library: &Manifest) {
        let header = self
            .target
            .include_line(library, &result.destination, root)
            .or_else(|| arduino::properties::include_name(library));

        self.ui.say(report::blank());

        self.ui.say(report::good(match header {
            Some(header) => format!("Done. #include \"{header}\" and you are away."),
            None => "Done.".to_string(),
        }));

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

    /// Get every library onto disk, work out the order, then install them in it. Returns an exit code.
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

            if source.in_place && request.folder.is_some() {
                let root = source
                    .project_root
                    .clone()
                    .unwrap_or_else(|| self.project_root.clone());
                let place = show(&source.root, &root);

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

        let mut worst = 0;

        for step in &order {
            let source = sources[step.source].clone();
            let named = request
                .folders
                .iter()
                .find(|(name, _)| *name == source.library().name)
                .map(|(_, folder)| folder.as_str());
            let folder = if step.needed_by.is_none() {
                request.folder.as_deref().or(named)
            } else {
                named
            };
            let code = self.install_step(step, &source, folder);

            // 1 is a run step that did not finish: the library is in, and so can the rest be. Anything else stops the run.
            if code != 0 && code != 1 {
                return code;
            }

            worst = worst.max(code);
        }

        worst
    }

    /// Take the libraries named after --uninstall out of the project. Returns an exit code.
    pub fn uninstall(&mut self, names: &[String]) -> i32 {
        for name in uninstall_order(names, &self.project_root) {
            self.ui.new_part();

            match installer::uninstall(&self.project_root, &name, self.target.as_mut()) {
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
        ui.say(item("written", "", show(path, root), Style::Good));
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
        ui.say(item("kept", "yours", show(path, root), Style::Accent));
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

/// Each project file changed, or left for the user, under its heading.
pub fn say_outcomes(outcomes: &[Outcome], ui: &mut dyn Ui) {
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
            let name = backup.file_name().unwrap_or_default().to_string_lossy();
            ui.say(report::detail(format!("backup: {name}")));
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
        say_outcomes(&removal.outcomes, ui);
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
        ui.say(item(
            "kept",
            "not the library's",
            show(path, root),
            Style::Accent,
        ));
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
        ui.say(report::note(if others.is_empty() {
            format!("{folder} stays, for what was saved above. Delete it once you have taken what you need.")
        } else {
            format!("{folder} stays, for the files above that were not the library's.")
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

    let Some(platform) = choose_platform(request, &folder, &mut client, ui) else {
        return 2;
    };

    if !check_platform_args(&request.args, platform, ui) {
        return 2;
    }

    if !request.uninstall.is_empty() && !request.libraries.is_empty() {
        ui.error("--uninstall takes libraries out. Install on a run of its own.");
        return 2;
    }

    let mut args = request.args.clone();
    args.uninstalling = request.uninstall.clone();

    let moving = !request.check_only;
    let mut session = match Session::open(client, platform, &args, &folder, moving, ui) {
        Ok(session) => session,
        Err(Stop(code)) => return code,
    };

    if !request.uninstall.is_empty() {
        return session.uninstall(&request.uninstall);
    }

    if request.check_only {
        return check::run(&mut session, request);
    }

    session.install(request)
}
