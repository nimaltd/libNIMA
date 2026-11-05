//! What libNIMA knows about Zephyr: what a Zephyr application is.
//!
//! The core does everything a library needs whatever it is installed into. This says the
//! rest for Zephyr: the part of libnima.yml only Zephyr reads, the checks against the
//! application's prj.conf, and the integration, which makes the library a Zephyr module of
//! the application (module.rs) and adds it to the application's CMakeLists.txt and prj.conf
//! (app.rs).

pub mod app;
pub mod module;

use std::path::{Path, PathBuf};

use crate::blocks::Group;
use crate::installer::{InstallError, Installed};
use crate::manifest::{Manifest, ManifestError};
use crate::paths;
use crate::platform::Platform;
use crate::platform::kconfig::{self, Assignment, Setting, read_settings, symbol_of};
use crate::report::{self, Question, QuestionKind, Ui};
use crate::target::{Dropped, Finding, Outcome, Status, Target};
use crate::text::given_twice;
use crate::titles::{self, Listed};
use crate::yaml::{Mapping, Node, Value};

/// The keys of the zephyr section, and of its two parts: what each writes, and into which file.
const KEYS: [&str; 4] = ["symbol", "settings", "project", "library"];
const PROJECT_KEYS: [&str; 3] = ["conf", "sysbuild", "cmake"];
const LIBRARY_KEYS: [&str; 3] = ["kconfig", "conf", "cmake"];

/// Where a group of CMake lines goes: the module's own CMakeLists.txt, inside its
/// if(CONFIG_...); the application's, above find_package(Zephyr), for what Zephyr reads as it
/// starts, such as EXTRA_CONF_FILE; or the application's, at its end, for what needs the
/// application's own targets.
pub const MODULE: &str = "module";
pub const BEFORE_ZEPHYR: &str = "before-zephyr";
pub const AFTER_ZEPHYR: &str = "after-zephyr";
const PLACES: [&str; 3] = [MODULE, BEFORE_ZEPHYR, AFTER_ZEPHYR];

const EXAMPLE: &str = "I2C";

pub const PRJ_KEY: &str = "zephyr.project.conf";
pub const SYSBUILD_KEY: &str = "zephyr.project.sysbuild";
pub const DEFAULTS_KEY: &str = "zephyr.library.kconfig";
pub const CONF_KEY: &str = "zephyr.library.conf";

/// The zephyr section of libnima.yml, with what it leaves out filled in. It lists what a
/// library writes under where it goes: project, the application's own files, and library, its
/// own folder, each list in groups under their titles when it likes.
#[derive(Debug, Clone)]
pub struct ZephyrPart {
    /// The library's switch, CONFIG_<symbol>=y.
    pub symbol: String,
    /// The switch can carry a note, as a setting's does, written above it in the block.
    pub symbol_note: Option<String>,
    pub settings: Vec<Setting>,
    /// What it sets in the application's Kconfig file, beside its switch.
    pub prj: Vec<Listed<Assignment>>,
    /// What it sets in the application's sysbuild.conf.
    pub sysbuild: Vec<Listed<Assignment>>,
    /// Settings of other symbols while it is on: configdefault in its Kconfig.
    pub defaults: Vec<Listed<Assignment>>,
    /// Its Kconfig fragment, read after the application's Kconfig files.
    pub conf: Vec<Listed<Assignment>>,
    pub cmake: Vec<Group>,
}

/// The keys from lib-installer before 4.0.0, each refused with where it went, and any key the
/// section does not have.
fn refuse_moved(data: &Mapping) -> Result<(), ManifestError> {
    let line = |key: &str| data.line_of(key).unwrap_or(0);

    if data.has("defaults") {
        return Err(ManifestError::at(
            line("defaults"),
            "zephyr.defaults is zephyr.library.kconfig in libnima.yml. List the settings there, under library and \
             then kconfig.",
        ));
    }

    if let Some(kconfig) = data
        .get("kconfig")
        .filter(|kconfig| kconfig.as_str().is_some())
    {
        return Err(ManifestError::at(
            kconfig.line,
            format!(
                "zephyr.kconfig has {}, one symbol. In libnima.yml the library's switch is zephyr.symbol, as symbol: \
                 {}, and its settings of other symbols are zephyr.library.kconfig.",
                kconfig.shown(),
                kconfig.text()
            ),
        ));
    }

    for (key, where_) in [
        (
            "kconfig",
            "zephyr.library.kconfig. List the settings there, under library and then kconfig.",
        ),
        (
            "conf",
            "zephyr.library.conf. List the settings there, under library and then conf.",
        ),
        (
            "sysbuild",
            "zephyr.project.sysbuild. List the settings there, under project and then sysbuild.",
        ),
        (
            "cmake",
            "zephyr.project.cmake and zephyr.library.cmake: the lines of the application's CMakeLists.txt under \
             project, by their place, before-zephyr or after-zephyr, and those of the library's own under library.",
        ),
    ] {
        if data.has(key) {
            return Err(ManifestError::at(
                line(key),
                format!("zephyr.{key} is {where_}"),
            ));
        }
    }

    let unknown: Vec<&str> = data.keys().filter(|key| !KEYS.contains(key)).collect();

    if let Some(first) = unknown.first() {
        return Err(ManifestError::at(
            line(first),
            format!(
                "zephyr has {}. It holds {}.",
                unknown.join(", "),
                KEYS.join(", ")
            ),
        ));
    }

    Ok(())
}

fn settings_list(
    value: Option<&Node>,
    key: &str,
    prefix: &'static str,
) -> Result<Vec<Listed<Assignment>>, ManifestError> {
    titles::listed(value, key, true, |text| {
        Assignment::read(text, key, EXAMPLE, prefix).map(Some)
    })
}

/// What every list refuses: a symbol given twice in one, the library's switch where it cannot
/// be, a symbol in two of them, which would never count in one, and the fragment's place taken
/// by a file of the library's own.
fn check_lists(part: &ZephyrPart, library: &Manifest) -> Result<(), ManifestError> {
    let names = |list: &[Listed<Assignment>]| -> Vec<String> {
        list.iter().map(|entry| entry.item.symbol.clone()).collect()
    };
    let are = |both: &[String]| if both.len() == 1 { "is" } else { "are" };
    let in_both = |one: &[String], other: &[String]| -> Vec<String> {
        let mut both: Vec<String> = one
            .iter()
            .filter(|name| other.contains(name))
            .cloned()
            .collect();
        both.sort();
        both.dedup();
        both
    };

    let prj = names(&part.prj);
    let twice = given_twice(prj.iter().map(String::as_str));

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "{PRJ_KEY} has {} more than once.",
            twice.join(", ")
        )));
    }

    if prj.contains(&part.symbol) {
        return Err(ManifestError::new(format!(
            "{PRJ_KEY} has {symbol}, the library's own switch, which libNIMA writes last in the block by itself, y, \
             under the library's name. Give it a note under zephyr.symbol, as {symbol}: its note.",
            symbol = part.symbol
        )));
    }

    let sysbuild = names(&part.sysbuild);
    let twice = given_twice(sysbuild.iter().map(String::as_str));

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "{SYSBUILD_KEY} has {} more than once.",
            twice.join(", ")
        )));
    }

    // The library's settings of other symbols: defaults, which hold while it is on, and which
    // any line of the application's own wins over.
    let defaults = names(&part.defaults);
    let twice = given_twice(defaults.iter().map(String::as_str));

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "{DEFAULTS_KEY} has {} more than once.",
            twice.join(", ")
        )));
    }

    if defaults.contains(&part.symbol) {
        return Err(ManifestError::new(format!(
            "{DEFAULTS_KEY} has {}, the library's own switch. prj.conf switches it, and the defaults hold while it is on.",
            part.symbol
        )));
    }

    // A symbol set in prj.conf is an assignment, and a default for it would never be used.
    let both = in_both(&prj, &defaults);

    if !both.is_empty() {
        return Err(ManifestError::new(format!(
            "{} {} in both {PRJ_KEY} and {DEFAULTS_KEY}. Give each in one of them: {PRJ_KEY} sets it in prj.conf, \
             {DEFAULTS_KEY} gives it a default while the library is on.",
            both.join(", "),
            are(&both)
        )));
    }

    // The library's Kconfig fragment, read after the application's Kconfig files: assignments,
    // for what a default cannot set, a member of a choice, or a symbol Kconfig reads before the
    // library's Kconfig.
    let assigned = names(&part.conf);
    let twice = given_twice(assigned.iter().map(String::as_str));

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "{CONF_KEY} has {} more than once.",
            twice.join(", ")
        )));
    }

    if assigned.contains(&part.symbol) {
        return Err(ManifestError::new(format!(
            "{CONF_KEY} has {}, the library's own switch. The application's Kconfig file switches it, and {CONF_KEY}, \
             read after it, would decide in its place.",
            part.symbol
        )));
    }

    // What the fragment sets is set after prj.conf, and over any default, so the same symbol in
    // prj.conf's block or among the defaults would never count.
    let both = in_both(&prj, &assigned);

    if !both.is_empty() {
        return Err(ManifestError::new(format!(
            "{} {} in both {PRJ_KEY} and {CONF_KEY}. Give each in one of them: {PRJ_KEY} sets it in prj.conf, {CONF_KEY} \
             in the library's own Kconfig fragment, read after prj.conf.",
            both.join(", "),
            are(&both)
        )));
    }

    let both = in_both(&defaults, &assigned);

    if !both.is_empty() {
        return Err(ManifestError::new(format!(
            "{} {} in both {DEFAULTS_KEY} and {CONF_KEY}. Give each in one of them: {DEFAULTS_KEY} gives it a default \
             while the library is on, {CONF_KEY} sets it in every build.",
            both.join(", "),
            are(&both)
        )));
    }

    // The fragment's place in the library's folder, which a file of the library's own would
    // share, each install writing over the other.
    let place = format!("zephyr/{}", module::fragment_name(&library.name));

    if !part.conf.is_empty()
        && library
            .extras
            .iter()
            .chain(library.all_code_files())
            .any(|entry| entry.destination == place)
    {
        return Err(ManifestError::new(format!(
            "libnima.yml puts a file of {}'s own at {place}, where libNIMA writes the lines of {CONF_KEY}. Give the file \
             another place, or its lines under {CONF_KEY}.",
            library.name
        )));
    }

    Ok(())
}

pub fn read_part(library: &mut Manifest, data: &Mapping) -> Result<ZephyrPart, ManifestError> {
    let section = data.given("zephyr");

    if let Some(section) = section
        && section.as_map().is_none()
    {
        return Err(ManifestError::at(
            section.line,
            "zephyr should hold symbol, settings, project and library.",
        ));
    }

    let empty = Mapping::default();
    let map = section.and_then(Node::as_map).unwrap_or(&empty);

    refuse_moved(map)?;

    let mut symbol_note = None;
    let given = map.given("symbol").filter(|given| given.truthy());

    // The switch can carry a note, a mapping of one, GCORE: its note, written above it in the block.
    let symbol = match given {
        None => symbol_of(&library.name),
        Some(given) => match &given.value {
            Value::Map(entries) => {
                if entries.entries.len() != 1 {
                    return Err(ManifestError::at(
                        given.line,
                        format!(
                            "zephyr.symbol has {}. Give the library's switch alone, or with its note after a colon, on \
                             a line of its own below symbol, such as GCORE: builds gcore.",
                            given.shown()
                        ),
                    ));
                }

                let only = &entries.entries[0];
                symbol_note = Some(titles::comment(
                    &only.value,
                    &format!("The note of {} under zephyr.symbol", only.key),
                )?);

                kconfig::symbol(
                    &Node::string(&only.key, only.line),
                    "zephyr.symbol",
                    EXAMPLE,
                )?
            }
            _ => kconfig::symbol(given, "zephyr.symbol", EXAMPLE)?,
        },
    };

    let settings = read_settings(map.get("settings"), "zephyr", EXAMPLE)?;

    if let Some(setting) = settings.iter().find(|setting| setting.name == symbol) {
        return Err(ManifestError::new(format!(
            "zephyr.settings {} is the library's own symbol. Give the setting a name of its own.",
            setting.name
        )));
    }

    let project = titles::part(map.get("project"), "zephyr.project", &PROJECT_KEYS)?;
    let own = titles::part(map.get("library"), "zephyr.library", &LIBRARY_KEYS)?;
    let from_project = |key: &str| project.and_then(|project| project.get(key));
    let from_own = |key: &str| own.and_then(|own| own.get(key));

    let part = ZephyrPart {
        prj: settings_list(from_project("conf"), PRJ_KEY, "CONFIG_")?,
        sysbuild: settings_list(from_project("sysbuild"), SYSBUILD_KEY, "SB_CONFIG_")?,
        defaults: settings_list(from_own("kconfig"), DEFAULTS_KEY, "CONFIG_")?,
        conf: settings_list(from_own("conf"), CONF_KEY, "CONFIG_")?,
        cmake: titles::cmake_groups(
            from_project("cmake"),
            from_own("cmake"),
            "zephyr",
            &[BEFORE_ZEPHYR, AFTER_ZEPHYR],
            MODULE,
            None,
        )?,
        symbol,
        symbol_note,
        settings,
    };

    check_lists(&part, library)?;

    // Each option's own groups of CMake lines, written while it is taken.
    let options = data.given("options").and_then(Node::as_map);

    for option in &mut library.options {
        let given = options
            .and_then(|options| options.get(&option.name))
            .and_then(Node::as_map)
            .and_then(|body| body.given("zephyr"));

        if let Some(given) = given
            && given.as_map().is_none()
        {
            return Err(ManifestError::at(
                given.line,
                format!(
                    "options.{}.zephyr should hold project and library.",
                    option.name
                ),
            ));
        }

        option.cmake = titles::option_cmake(&option.name, given, "zephyr", &PLACES, MODULE)?;
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
            "zephyr.cmake has two groups labelled {}, its options' groups counted. Give each its own label.",
            twice.join(", ")
        )));
    }

    Ok(part)
}

pub fn warnings(manifest: &Manifest) -> Vec<String> {
    let part = manifest.part.zephyr();
    let mut found = Vec::new();

    for setting in &part.settings {
        if !setting.name.starts_with(&format!("{}_", part.symbol)) {
            found.push(format!(
                "zephyr.settings {} does not start with {}_, so it may be taken by another library",
                setting.name, part.symbol
            ));
        }

        if !setting.prompt_given {
            found.push(format!(
                "zephyr.settings {} has no prompt, so menuconfig shows only its name",
                setting.name
            ));
        }
    }

    found
}

/// A Zephyr application: CMakeLists.txt with find_package(Zephyr), and prj.conf.
pub struct Zephyr {
    /// The Kconfig file given for this run, by --conf.
    conf_given: Option<String>,
    /// The one chosen by the answer to the question, which is asked once however many
    /// libraries go in.
    conf_chosen: Option<String>,
    /// The file each library of this run goes into, settled by check() and used by
    /// integrate(), so the two always agree.
    conf_of: Vec<(String, String)>,
}

impl Zephyr {
    pub fn new(conf_given: Option<String>) -> Zephyr {
        Zephyr {
            conf_given,
            conf_chosen: None,
            conf_of: Vec::new(),
        }
    }

    /// The Kconfig file this library's symbols go into, as a path from the application's
    /// folder.
    ///
    /// The one that already has its block, on an update. Otherwise the one given with --conf,
    /// or chosen earlier in this run. Otherwise, with more than one to choose from and someone
    /// to ask, the answer, and prj.conf when there is nobody or nothing to choose.
    fn conf_for(&mut self, library: &Manifest, project_root: &Path, ui: &mut dyn Ui) -> String {
        if let Some((_, conf)) = self.conf_of.iter().find(|(name, _)| *name == library.name) {
            return conf.clone();
        }

        let mut chosen = app::holding(project_root, &library.name)
            .or_else(|| self.conf_given.clone())
            .or_else(|| self.conf_chosen.clone());

        if chosen.is_none() {
            let offered = app::conf_files(project_root);
            let mut answer = app::PRJ_CONF.to_string();

            if offered.len() > 1 {
                ui.say(report::blank());
                ui.say(report::note(format!(
                    "This application has more than one Kconfig file: {}",
                    offered.join(", ")
                )));

                let question = Question {
                    prompt: report::line(
                        format!("Kconfig file for the libraries [{}]: ", app::PRJ_CONF),
                        report::Style::Bold,
                    ),
                    kind: QuestionKind::Text,
                };

                if let Some(given) = ui.ask(&question).filter(|given| !given.is_empty()) {
                    answer = given;
                }
            }

            self.conf_chosen = Some(answer.clone());
            chosen = Some(answer);
        }

        let chosen = chosen.unwrap_or_default().replace('\\', "/");
        self.conf_of.push((library.name.clone(), chosen.clone()));

        chosen
    }
}

impl Target for Zephyr {
    fn platform(&self) -> Platform {
        Platform::Zephyr
    }

    fn no_project(&self) -> &'static str {
        "No Zephyr application was recognised here, so nothing was registered."
    }

    fn no_project_hint(&self) -> &'static str {
        "Run this from your application's folder, the one holding CMakeLists.txt and prj.conf."
    }

    /// An application is a CMakeLists.txt that finds Zephyr. Without one, nothing would build the library.
    fn not_a_project(&self, project_root: &Path) -> Option<String> {
        app::find_cmakelists(project_root).is_none().then(|| {
            format!(
                "{} is not a Zephyr application: it has no CMakeLists.txt with find_package(Zephyr). Run this from your \
                 application's folder, the one holding CMakeLists.txt and prj.conf, or give it with --project. Nothing \
                 was installed.",
                paths::posix(project_root)
            )
        })
    }

    /// Refuses sysbuild.conf as the Kconfig file of --conf.
    fn project_root(&mut self, given: &Path, _ui: &mut dyn Ui) -> Result<PathBuf, InstallError> {
        if let Some(conf) = &self.conf_given
            && paths::leaf(&conf.replace('\\', "/")) == app::SYSBUILD_CONF
        {
            return Err(InstallError(format!(
                "{0} holds the settings of sysbuild, not the application's, so a library is not switched on there. Its \
                 sysbuild settings go in zephyr.project.sysbuild of its libnima.yml. Leave out --conf, or give prj.conf \
                 or a board's own file.",
                app::SYSBUILD_CONF
            )));
        }

        Ok(given.to_path_buf())
    }

    /// Libraries go together in modules/ of the application.
    fn folder_for(&self, library: &Manifest, _project_root: &Path) -> String {
        super::default_folder(Platform::Zephyr, &library.name)
    }

    /// The application's files this edits, which a once file must never be: CMakeLists.txt,
    /// the Kconfig file with the library's block, and sysbuild.conf when the library has
    /// sysbuild settings.
    fn managed_files(&self, project_root: &Path, library: &Manifest) -> Vec<String> {
        let mut found = vec![
            crate::record::RECORD_NAME.to_string(),
            app::CMAKELISTS.to_string(),
        ];

        found.push(
            self.conf_of
                .iter()
                .find(|(name, _)| *name == library.name)
                .map(|(_, conf)| conf.clone())
                .or_else(|| app::holding(project_root, &library.name))
                .unwrap_or_else(|| app::PRJ_CONF.to_string()),
        );

        if !library.part.zephyr().sysbuild.is_empty() {
            found.push(app::SYSBUILD_CONF.to_string());
        }

        found
    }

    /// What the application is missing for this library. Never stops an install.
    fn check(&mut self, library: &Manifest, project_root: &Path, ui: &mut dyn Ui) -> Vec<Finding> {
        let mut findings = Vec::new();

        if app::find_cmakelists(project_root).is_none() {
            findings.push(Finding::warn(
                "No CMakeLists.txt with find_package(Zephyr) here",
                "Run this from your application's folder, or give it with --project.",
            ));

            return findings;
        }

        let conf = self.conf_for(library, project_root, ui);
        let assigned = app::assigned(project_root, &library.name, &conf);
        let ours = app::in_block(project_root, &library.name, &conf);
        let part = library.part.zephyr();
        let of = |list: &[(String, String, String)], symbol: &str| {
            list.iter()
                .find(|(name, _, _)| name == symbol)
                .map(|(_, value, file)| (value.clone(), file.clone()))
        };

        for (symbol, value, line) in app::wanted(library) {
            let have = of(&assigned, &symbol);

            // The library's own switch is no requirement, and is only worth a line when the
            // user has set it already.
            if symbol == part.symbol && have.is_none() {
                continue;
            }

            match have {
                None if ours.iter().any(|(name, set)| *name == symbol && *set == value) => {
                    findings.push(Finding::ok(format!("{line} is in {conf}")));
                }
                None => findings.push(Finding::info(format!("{line} goes into {conf}"), "")),
                // The same, so nothing is wrong, but the block will not set it, and the user
                // should know which line does.
                Some((set, file)) if set == value => findings.push(Finding::warn(
                    format!("{file} already sets {line}, so the block has it commented out"),
                    "Your line is the one used. Nothing to do.",
                )),
                Some((set, file)) => findings.push(Finding::warn(
                    format!(
                        "{file} sets CONFIG_{symbol}={set}, and {} needs {value}. The block has it commented out",
                        library.name
                    ),
                    "Your line is the one used. Change it if the library is to work.",
                )),
            }
        }

        // The library's settings of other symbols, zephyr.library.kconfig, which hold while it
        // is on. A line of the application's that sets the same symbol wins, and says so.
        for default in &part.defaults {
            match of(&assigned, &default.item.symbol) {
                Some((set, file)) if set != default.item.value => findings.push(Finding::info(
                    format!(
                        "{file} sets CONFIG_{}={set}, in place of {}'s default {}",
                        default.item.symbol, library.name, default.item.value
                    ),
                    "Your line is the one used.",
                )),
                _ => findings.push(Finding::ok(format!(
                    "{} by default, while CONFIG_{} is on",
                    default.item.line(),
                    part.symbol
                ))),
            }
        }

        // The library's Kconfig fragment, zephyr.library.conf, read after the application's
        // Kconfig files. A line of the application's that sets the same symbol loses to it,
        // and is named, since nothing else would say so.
        let fragment = format!("zephyr/{}", module::fragment_name(&library.name));

        for setting in &part.conf {
            match of(&assigned, &setting.item.symbol) {
                Some((set, file)) if set != setting.item.value => findings.push(Finding::warn(
                    format!(
                        "{file} sets CONFIG_{}={set}, and {}'s {CONF_KEY} sets {}",
                        setting.item.symbol, library.name, setting.item.value
                    ),
                    format!(
                        "{fragment} is read after {file}, so its line is the one used. A file given with \
                         -DEXTRA_CONF_FILE is read after it."
                    ),
                )),
                _ => findings.push(Finding::ok(format!(
                    "{} in {fragment}, read after {conf}",
                    setting.item.line()
                ))),
            }
        }

        for peripheral in &library.requires.peripherals {
            findings.push(Finding::info(
                format!("Needs {}", peripheral.describe()),
                "Enable it in your board's devicetree, or in an overlay of the application.",
            ));
        }

        findings
    }

    /// Make the library a module of the application, and switch it on.
    ///
    /// What an update dropped needs nothing here: the module's files are written again from the
    /// new manifest on every install, so a file or a define that has gone is simply no longer
    /// in them.
    fn integrate(
        &mut self,
        project_root: &Path,
        library: &Manifest,
        destination: &Path,
        _dropped: &Dropped,
        ui: &mut dyn Ui,
    ) -> Vec<Outcome> {
        let conf = self.conf_for(library, project_root, ui);
        let mut outcomes = vec![module::write(library, destination)];

        outcomes.extend(app::integrate(project_root, library, destination, &conf));

        outcomes
    }

    /// Take the library out of the application's CMakeLists.txt and Kconfig file, and delete
    /// the module files and the Kconfig fragment this wrote into its folder.
    fn uninstall(&mut self, project_root: &Path, library: &Installed) -> Vec<Outcome> {
        let defined = library
            .destination
            .as_deref()
            .map(module::symbols)
            .unwrap_or_default();
        let mut outcomes = app::uninstall(project_root, &library.name);

        if let Some(destination) = &library.destination
            && let Some(saved) = module::remove(destination)
        {
            let name = saved
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            outcomes.push(
                Outcome::new(
                    module::NAME,
                    Status::Changed,
                    format!("Your lines in zephyr/CMakeLists.txt are saved, in zephyr/{name}."),
                )
                .with_backup(Some(saved)),
            );
        }

        // A setting the user set below the block names a symbol that is gone now, and Zephyr
        // stops on a symbol it does not know. The line is the user's, so it is named rather
        // than taken out.
        for (conf, lines) in app::leftover(project_root, &defined) {
            outcomes.push(
                Outcome::new(
                    conf.clone(),
                    Status::Manual,
                    format!(
                        "{conf} still sets what came with {}, and Zephyr stops on a symbol it does not know. Take these \
                         lines out:",
                        library.name
                    ),
                )
                .with_steps(lines),
            );
        }

        outcomes
    }

    /// The application's files lib-installer wrote its blocks into.
    fn legacy_files(&self, project_root: &Path) -> Vec<PathBuf> {
        let mut found = vec![
            project_root.join(app::CMAKELISTS),
            project_root.join(app::SYSBUILD_CONF),
        ];

        found.extend(
            app::kconfig_files(project_root)
                .iter()
                .map(|name| project_root.join(name)),
        );

        found
    }
}
