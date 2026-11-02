//! What libNIMA knows about ESP-IDF: what an ESP-IDF project is.
//!
//! The core does everything a library needs whatever it is installed into. This says the rest
//! for ESP-IDF: the part of libnima.yml only ESP-IDF reads, the checks against the project's
//! sdkconfig, and the integration, which makes the library a component of the project
//! (component.rs) and adds what it needs to the project's own files (project.rs).

pub mod component;
pub mod project;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::blocks::Group;
use crate::installer::Installed;
use crate::manifest::{Manifest, ManifestError};
use crate::platform::Platform;
use crate::platform::kconfig::{Assignment, Setting, read_settings, symbol_of};
use crate::record::installed_libraries;
use crate::report::Ui;
use crate::target::{Dropped, Finding, Outcome, Status, Target};
use crate::text::given_twice;
use crate::titles::{self, Listed};
use crate::yaml::{Mapping, Node};

/// The keys of the esp32 section, and of its two parts: what each writes, and into which file.
const KEYS: [&str; 3] = ["settings", "project", "library"];
const PROJECT_KEYS: [&str; 2] = ["conf", "cmake"];
const LIBRARY_KEYS: [&str; 2] = ["requires", "cmake"];

/// Where a group of CMake lines goes: the component's own CMakeLists.txt, after
/// idf_component_register(), where ${COMPONENT_LIB} is its library; the project's, above
/// include(project.cmake), for what ESP-IDF reads as the project starts, such as
/// EXTRA_COMPONENT_DIRS; or the project's, at its end, for what needs the project's own targets.
pub const COMPONENT_PLACE: &str = "component";
pub const BEFORE_PROJECT: &str = "before-project";
pub const AFTER_PROJECT: &str = "after-project";
const PLACES: [&str; 3] = [COMPONENT_PLACE, BEFORE_PROJECT, AFTER_PROJECT];

const EXAMPLE: &str = "FREERTOS_HZ";
const CONF_KEY: &str = "esp32.project.conf";

/// An ESP-IDF component's name, as it lands unquoted in REQUIRES: esp_timer, esp_driver_i2c, esp-tls.
static COMPONENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.+-]+$").expect("a valid pattern"));

/// The esp32 section of libnima.yml, with what it leaves out filled in. It lists what a library
/// writes under where it goes: project, the project's own files, and library, the component's own
/// folder, each list in groups under their titles when it likes.
#[derive(Debug, Clone)]
pub struct Esp32Part {
    pub settings: Vec<Setting>,
    /// The symbols it sets in the project's sdkconfig.defaults.
    pub conf: Vec<Listed<Assignment>>,
    /// The ESP-IDF components the library's code uses.
    pub requires: Vec<String>,
    pub cmake: Vec<Group>,
}

/// The ESP-IDF components the library's code uses, such as esp_timer, each once. They go into
/// REQUIRES of its component, which is how ESP-IDF puts their headers on the library's include path.
fn components(value: Option<&Node>, key: &str) -> Result<Vec<String>, ManifestError> {
    let Some(value) = value.filter(|value| value.truthy()) else {
        return Ok(Vec::new());
    };

    let Some(list) = value.as_list() else {
        return Err(ManifestError::at(
            value.line,
            format!("{key} is a list of ESP-IDF components, such as [esp_timer]."),
        ));
    };

    let mut found = Vec::new();

    for entry in list {
        if entry.as_map().is_some() {
            return Err(ManifestError::at(
                entry.line,
                format!(
                    "{key} has {}. It is a plain list of ESP-IDF components, such as [esp_timer], with no titles or notes.",
                    entry.shown()
                ),
            ));
        }

        let name = entry.text().trim().to_string();

        if !COMPONENT.is_match(&name) {
            return Err(ManifestError::at(
                entry.line,
                format!(
                    "{key} has {}. Write an ESP-IDF component by its name, such as esp_timer or esp_driver_i2c.",
                    entry.shown()
                ),
            ));
        }

        if !found.contains(&name) {
            found.push(name);
        }
    }

    Ok(found)
}

pub fn read_part(library: &mut Manifest, data: &Mapping) -> Result<Esp32Part, ManifestError> {
    let section = data.given("esp32");

    if let Some(section) = section
        && section.as_map().is_none()
    {
        return Err(ManifestError::at(
            section.line,
            "esp32 should hold settings, project and library.",
        ));
    }

    let empty = Mapping::default();
    let map = section.and_then(Node::as_map).unwrap_or(&empty);

    // The keys from lib-installer before 4.0.0, each refused with where it went.
    for (key, where_) in [
        (
            "requires",
            "esp32.library.requires: the ESP-IDF components the library's code uses. List them there, under library and then requires.",
        ),
        (
            "cmake",
            "esp32.project.cmake and esp32.library.cmake: the lines of the project's CMakeLists.txt under project, by \
             their place, before-project or after-project, and those of the component's own under library.",
        ),
    ] {
        if map.has(key) {
            return Err(ManifestError::at(
                map.line_of(key).unwrap_or(0),
                format!("esp32.{key} is {where_}"),
            ));
        }
    }

    // A key the section does not have, such as a misspelt settings, which would take what it says
    // with it unseen.
    let unknown: Vec<&str> = map.keys().filter(|key| !KEYS.contains(key)).collect();

    if let Some(first) = unknown.first() {
        return Err(ManifestError::at(
            map.line_of(first).unwrap_or(0),
            format!(
                "esp32 has {}. It holds {}.",
                unknown.join(", "),
                KEYS.join(", ")
            ),
        ));
    }

    let settings = read_settings(map.get("settings"), "esp32", EXAMPLE)?;
    let project = titles::part(map.get("project"), "esp32.project", &PROJECT_KEYS)?;
    let own = titles::part(map.get("library"), "esp32.library", &LIBRARY_KEYS)?;
    let from_project = |key: &str| project.and_then(|project| project.get(key));
    let from_own = |key: &str| own.and_then(|own| own.get(key));

    // The symbols it sets in the project's sdkconfig.defaults. One given twice is refused: the line
    // read last would quietly undo the first.
    let conf = titles::listed(from_project("conf"), CONF_KEY, true, |text| {
        Assignment::read(text, CONF_KEY, EXAMPLE, "CONFIG_").map(Some)
    })?;
    let twice = given_twice(conf.iter().map(|entry| entry.item.symbol.as_str()));

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "{CONF_KEY} has {} more than once.",
            twice.join(", ")
        )));
    }

    let part = Esp32Part {
        settings,
        conf,
        requires: components(from_own("requires"), "esp32.library.requires")?,
        // Lines of CMake: the project's, under before-project, above include(project.cmake), and
        // after-project, at its end; and the component's own, after idf_component_register().
        cmake: titles::cmake_groups(
            from_project("cmake"),
            from_own("cmake"),
            "esp32",
            &[BEFORE_PROJECT, AFTER_PROJECT],
            COMPONENT_PLACE,
            None,
        )?,
    };

    // Each option's own groups of CMake lines, written while it is taken.
    let options = data.given("options").and_then(Node::as_map);

    for option in &mut library.options {
        let given = options
            .and_then(|options| options.get(&option.name))
            .and_then(Node::as_map)
            .and_then(|body| body.given("esp32"));

        if let Some(given) = given
            && given.as_map().is_none()
        {
            return Err(ManifestError::at(
                given.line,
                format!(
                    "options.{}.esp32 should hold project and library.",
                    option.name
                ),
            ));
        }

        option.cmake =
            titles::option_cmake(&option.name, given, "esp32", &PLACES, COMPONENT_PLACE)?;
    }

    // A label names one group, among the library's and all its options'.
    let mut labels: Vec<&str> = part
        .cmake
        .iter()
        .map(|group| group.label.as_str())
        .collect();
    labels.extend(
        library
            .options
            .iter()
            .flat_map(|option| option.cmake.iter().map(|group| group.label.as_str())),
    );
    let twice = given_twice(labels);

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "esp32.cmake has two groups labelled {}, its options' groups counted. Give each its own label.",
            twice.join(", ")
        )));
    }

    Ok(part)
}

pub fn warnings(manifest: &Manifest) -> Vec<String> {
    let prefix = symbol_of(&manifest.name);
    let mut found = Vec::new();

    for setting in &manifest.part.esp32().settings {
        if !setting.name.starts_with(&format!("{prefix}_")) {
            found.push(format!(
                "esp32.settings {} does not start with {prefix}_, so it may be taken by another component",
                setting.name
            ));
        }

        if !setting.prompt_given {
            found.push(format!(
                "esp32.settings {} has no prompt, so menuconfig shows only its name",
                setting.name
            ));
        }
    }

    found
}

/// The folder of ESP-IDF's own component of this name, when there is one.
///
/// Read from IDF_PATH, which ESP-IDF's own shell sets. A project component of the same name takes
/// the place of ESP-IDF's in the build.
fn idf_component(name: &str) -> Option<PathBuf> {
    let idf = std::env::var_os("IDF_PATH")?;
    let path = Path::new(&idf).join("components").join(name);

    path.is_dir().then_some(path)
}

/// The library's folder: the one it is in already, or the one it is offered.
fn folder_of(library: &Manifest, project_root: &Path) -> String {
    installed_libraries(project_root)
        .get(&library.name)
        .and_then(|entry| entry.get("folder"))
        .and_then(|folder| folder.as_str())
        .map_or_else(
            || format!("{}/{}", project::COMPONENTS, library.name),
            str::to_string,
        )
}

/// Every component the library's component needs: ESP-IDF's own from libnima.yml, then the libraries
/// it depends on, by the names their folders give them, as the record has them.
fn requires_of(library: &Manifest, project_root: &Path) -> Vec<String> {
    let recorded = installed_libraries(project_root);
    let mut names = library.part.esp32().requires.clone();

    for dependency in library.dependencies() {
        let folder = recorded
            .get(&dependency.name)
            .and_then(|entry| entry.get("folder"))
            .and_then(|folder| folder.as_str());
        let name = folder.map_or_else(
            || dependency.name.clone(),
            |folder| component::component_name(Path::new(folder)),
        );

        if !names.contains(&name) {
            names.push(name);
        }
    }

    names
}

/// An ESP-IDF project: CMakeLists.txt with include(project.cmake), main, and sdkconfig.
pub struct Esp32;

impl Target for Esp32 {
    fn platform(&self) -> Platform {
        Platform::Esp32
    }

    fn no_project(&self) -> &'static str {
        "No ESP-IDF project was recognised here, so nothing was registered."
    }

    fn no_project_hint(&self) -> &'static str {
        "Run this from your project's folder, the one holding CMakeLists.txt and main/."
    }

    /// Libraries go into the project's components folder, where ESP-IDF finds them.
    fn folder_for(&self, library: &Manifest, _project_root: &Path) -> String {
        format!("{}/{}", project::COMPONENTS, library.name)
    }

    /// The project's files this edits or reads as the project's own, which a once file must never be.
    fn managed_files(&self, _project_root: &Path, _library: &Manifest) -> Vec<String> {
        [
            crate::record::RECORD_NAME,
            project::CMAKELISTS,
            project::MAIN,
            project::SDKCONFIG,
            project::DEFAULTS,
        ]
        .map(String::from)
        .to_vec()
    }

    /// What the project is missing for this library. Never stops an install.
    fn check(&mut self, library: &Manifest, project_root: &Path, _ui: &mut dyn Ui) -> Vec<Finding> {
        let mut findings = Vec::new();

        if project::find_project(project_root).is_none() {
            findings.push(Finding::warn(
                "No CMakeLists.txt with include(.../project.cmake) here",
                "Run this from your ESP-IDF project's folder, or give it with --project.",
            ));

            return findings;
        }

        let name = component::component_name(Path::new(&folder_of(library, project_root)));

        if idf_component(&name).is_some() {
            findings.push(Finding::warn(
                format!("ESP-IDF has a component called {name}, and this one would take its place"),
                format!(
                    "Install it into a folder of another name, such as --dir components/{name}_lib."
                ),
            ));
        }

        let part = library.part.esp32();

        if !part.conf.is_empty() && project::names_its_defaults(project_root) {
            findings.push(Finding::warn(
                "This project sets SDKCONFIG_DEFAULTS, so ESP-IDF may not read sdkconfig.defaults",
                format!(
                    "Keep {} in that list, or set the lines below in one of yours.",
                    project::DEFAULTS
                ),
            ));
        }

        let taken = project::user_defaults(project_root, &library.name);
        let ours = project::in_block(project_root, &library.name);
        let have = project::configured(project_root);
        let of = |list: &[(String, String)], symbol: &str| {
            list.iter()
                .find(|(name, _)| name == symbol)
                .map(|(_, value)| value.clone())
        };

        for (symbol, value, line) in project::wanted(library) {
            let in_defaults = of(&taken, &symbol);
            let in_sdkconfig = of(&have, &symbol);

            match (in_defaults, in_sdkconfig) {
                (Some(set), _) if set == value => findings.push(Finding::warn(
                    format!("{} already sets {line}, so the block has it commented out", project::DEFAULTS),
                    "Your line is the one used. Nothing to do.",
                )),
                (Some(set), _) => findings.push(Finding::warn(
                    format!(
                        "{} sets CONFIG_{symbol}={set}, and {} needs {value}. The block has it commented out",
                        project::DEFAULTS, library.name
                    ),
                    "Your line is the one used. Change it if the library is to work.",
                )),
                (None, Some(set)) if set != value => findings.push(Finding::warn(
                    format!("sdkconfig has CONFIG_{symbol}={set}, and {} needs {value}", library.name),
                    "sdkconfig is what ESP-IDF builds with. Set it in idf.py menuconfig.",
                )),
                _ if of(&ours, &symbol).as_deref() == Some(value.as_str()) => {
                    findings.push(Finding::ok(format!("{line} is in {}", project::DEFAULTS)));
                }
                _ => findings.push(Finding::info(format!("{line} goes into {}", project::DEFAULTS), "")),
            }
        }

        for peripheral in &library.requires.peripherals {
            findings.push(Finding::info(
                format!("Needs {}", peripheral.describe()),
                "Set it up in your code, or in idf.py menuconfig.",
            ));
        }

        findings
    }

    /// Make the library a component of the project, and give it what it needs.
    ///
    /// What an update dropped needs nothing here: the component's files are written again from the
    /// new manifest on every install, so a file or a define that has gone is simply no longer in them.
    fn integrate(
        &mut self,
        project_root: &Path,
        library: &Manifest,
        destination: &Path,
        _dropped: &Dropped,
        _ui: &mut dyn Ui,
    ) -> Vec<Outcome> {
        let mut outcomes = vec![component::write(
            library,
            destination,
            &requires_of(library, project_root),
        )];

        outcomes.extend(project::integrate(project_root, library, destination));

        outcomes
    }

    /// Take the library out of the project's files, and delete the component files this wrote into
    /// its folder.
    fn uninstall(&mut self, project_root: &Path, library: &Installed) -> Vec<Outcome> {
        let destination = library.destination.as_deref();
        let defined = destination.map(component::symbols).unwrap_or_default();
        let name = destination.map(component::component_name);
        let mut outcomes = project::uninstall(project_root, &library.name, name.as_deref());

        if let Some(destination) = destination
            && let Some(saved) = component::remove(destination)
        {
            let shown = saved
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            outcomes.push(
                Outcome::new(
                    component::NAME,
                    Status::Changed,
                    format!("Your lines in its CMakeLists.txt are saved, in {shown}."),
                )
                .with_backup(Some(saved)),
            );
        }

        // A setting the user set in sdkconfig.defaults names a symbol that is gone now. ESP-IDF only
        // warns about it, but the line is the user's, so it is named rather than taken out.
        let lines = project::leftover(project_root, &defined);

        if !lines.is_empty() {
            outcomes.push(
                Outcome::new(
                    project::DEFAULTS,
                    Status::Manual,
                    format!(
                        "{} still sets what came with {}, which ESP-IDF no longer knows. Take these lines out:",
                        project::DEFAULTS, library.name
                    ),
                )
                .with_steps(lines),
            );
        }

        outcomes
    }

    /// The project's files lib-installer wrote its blocks into.
    fn legacy_files(&self, project_root: &Path) -> Vec<PathBuf> {
        vec![
            project_root.join(project::CMAKELISTS),
            project_root.join(project::DEFAULTS),
            project_root.join(project::MAIN),
        ]
    }
}
