//! What libNIMA knows about PlatformIO: what a PlatformIO project is.
//!
//! A library goes into the project's lib folder, which PlatformIO's Library Dependency Finder searches
//! for every framework it builds with, and where one library finds another: measured, a library in lib/
//! including the header of another in lib/ builds, so what a library needs goes there too. Each gets a
//! library.json (library_json.rs), and the defines it needs go into platformio.ini (ini.rs), since a
//! define in library.json reaches the library's own files and nothing else. So do the build flags a
//! manifest gives under pio.project.build_flags.

pub mod ini;
pub mod library_json;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::files;
use crate::installer::Installed;
use crate::manifest::{Manifest, ManifestError};
use crate::paths;
use crate::platform::Platform;
use crate::report::Ui;
use crate::target::{Dropped, Finding, Outcome, Status, Target};
use crate::text::given_twice;
use crate::titles::{self, Listed};
use crate::yaml::{Mapping, Node, Value};

pub(crate) const LIB: &str = "lib";

/// The keys of the pio section, and of its two parts: what each writes, and into which file.
const KEYS: [&str; 2] = ["project", "library"];
const PROJECT_KEYS: [&str; 1] = ["build_flags"];
const LIBRARY_KEYS: [&str; 3] = ["frameworks", "platforms", "dependencies"];

const FLAGS_KEY: &str = "pio.project.build_flags";

/// A framework or a platform as platformio.ini names it: arduino, espidf, stm32cube, espressif32,
/// ststm32, native.
static NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9_-]*$").expect("a valid pattern"));

/// A library of the PlatformIO Registry: owner/name, with @version when one is needed.
static REGISTRY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:[A-Za-z0-9_.-]+/)?[A-Za-z0-9 _.+-]+(?:@\S+)?$").expect("a valid pattern")
});

/// A line of build_flags: words of letters, digits and _ . + - / = , :, the first an option, such as
/// -O2, -Wl,-u,_printf_float or -I include. It goes into platformio.ini as it stands, and PlatformIO
/// hands it to the compiler and the linker of every file of the build, so nothing platformio.ini, SCons
/// or a shell would read as more than a flag gets through: no ; or #, which start a comment, no quote, no
/// $, which SCons expands, and no ! at the start, which has PlatformIO run a command.
static FLAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^-[A-Za-z0-9_.+=,:/-]+(?: +[A-Za-z0-9_.+=,:/-]+)*$").expect("a valid pattern")
});

/// The pio section of libnima.yml, with what it leaves out filled in: what goes into the project's
/// platformio.ini under project, and what goes into the library's library.json under library.
#[derive(Debug, Clone, Default)]
pub struct PioPart {
    /// The flags it gives every file of the build: written after its defines in its block of build_flags,
    /// each group under its title and a flag with its note, as comments of platformio.ini.
    pub build_flags: Vec<Listed<String>>,
    pub frameworks: Vec<String>,
    pub platforms: Vec<String>,
    /// As library.json writes them: (owner/name, version).
    pub dependencies: Vec<(String, String)>,
}

/// What goes into library.json, refused with a title or a note: JSON has no comments to write them as.
fn plain(value: &Node, key: &str) -> Result<(), ManifestError> {
    let has_note = value
        .as_list()
        .is_some_and(|list| list.iter().any(|entry| entry.as_map().is_some()));

    if value.as_map().is_some() || has_note {
        return Err(ManifestError::at(
            value.line,
            format!(
                "{key} has a title or a note, which library.json, being JSON, has no room for. Give a plain list."
            ),
        ));
    }

    Ok(())
}

/// frameworks or platforms: a list of names, or nothing for any.
fn names(data: Option<&Mapping>, key: &str) -> Result<Vec<String>, ManifestError> {
    let Some(value) = data.and_then(|data| data.given(key)) else {
        return Ok(Vec::new());
    };

    let where_ = format!("pio.library.{key}");
    let bad = || {
        let example = if key == "frameworks" {
            "[arduino, espidf]"
        } else {
            "[espressif32, ststm32]"
        };

        ManifestError::at(
            value.line,
            format!(
                "{where_} is a list of names as platformio.ini writes them, such as {example}, or \"*\" for any."
            ),
        )
    };

    let found: Vec<String> = match &value.value {
        Value::Str(text) if text == "*" => return Ok(Vec::new()),
        Value::Str(text) => text
            .split(',')
            .map(|part| part.trim().to_string())
            .collect(),
        Value::List(list) => {
            plain(value, &where_)?;

            if list.len() == 1 && list[0].text() == "*" {
                return Ok(Vec::new());
            }

            list.iter().map(Node::text).collect()
        }
        Value::Map(_) => {
            plain(value, &where_)?;
            return Err(bad());
        }
        _ => return Err(bad()),
    };

    if !found.iter().all(|name| NAME.is_match(name)) {
        return Err(bad());
    }

    Ok(found)
}

/// The libraries of the PlatformIO Registry it needs, as library.json writes them, * for any version.
fn dependencies(value: Option<&Node>, key: &str) -> Result<Vec<(String, String)>, ManifestError> {
    let Some(value) = value.filter(|value| value.truthy()) else {
        return Ok(Vec::new());
    };

    plain(value, key)?;

    let bad = || {
        ManifestError::at(
            value.line,
            format!(
                "{key} is a list of PlatformIO Registry libraries, such as [bblanchon/ArduinoJson@^7.0.0]: owner/name, \
                 and @version when one is needed."
            ),
        )
    };

    let list = value.as_list().ok_or_else(bad)?;
    let mut found: Vec<(String, String)> = Vec::new();

    for entry in list {
        let text = entry.text();
        let text = text.trim();

        if !REGISTRY.is_match(text) {
            return Err(bad());
        }

        let (name, version) = text.split_once('@').unwrap_or((text, ""));

        // One version each: a second would take the first's place unseen.
        if found.iter().any(|(have, _)| have == name) {
            return Err(ManifestError::at(
                entry.line,
                format!(
                    "{key} has {name} more than once. Give it once, with the version that will do."
                ),
            ));
        }

        found.push((
            name.to_string(),
            if version.is_empty() {
                "*".to_string()
            } else {
                version.to_string()
            },
        ));
    }

    Ok(found)
}

pub fn read_part(data: &Mapping) -> Result<PioPart, ManifestError> {
    let section = data.given("pio");

    if let Some(section) = section
        && section.as_map().is_none()
    {
        return Err(ManifestError::at(
            section.line,
            "pio should hold project and library.",
        ));
    }

    let empty = Mapping::default();
    let map = section.and_then(Node::as_map).unwrap_or(&empty);

    // The keys from lib-installer before 4.0.0, each refused with where it went.
    for key in LIBRARY_KEYS {
        if map.has(key) {
            return Err(ManifestError::at(
                map.line_of(key).unwrap_or(0),
                format!(
                    "pio.{key} is pio.library.{key}: what goes into the library's library.json is under library."
                ),
            ));
        }
    }

    // A key the section does not have, such as a misspelt library.
    let unknown: Vec<&str> = map.keys().filter(|key| !KEYS.contains(key)).collect();

    if let Some(first) = unknown.first() {
        return Err(ManifestError::at(
            map.line_of(first).unwrap_or(0),
            format!(
                "pio has {}. It holds {}.",
                unknown.join(", "),
                KEYS.join(", ")
            ),
        ));
    }

    let project = titles::part(map.get("project"), "pio.project", &PROJECT_KEYS)?;
    let own = titles::part(map.get("library"), "pio.library", &LIBRARY_KEYS)?;

    let build_flags = titles::listed(
        project.and_then(|project| project.get("build_flags")),
        FLAGS_KEY,
        true,
        |text| {
            let line = text.as_str().map(str::trim).unwrap_or_default();

            if line.is_empty() || !FLAG.is_match(line) {
                return Err(ManifestError::at(
                    text.line,
                    format!(
                        "{FLAGS_KEY} has {}. Give a flag as the compiler takes it, such as -O2, -Wl,-u,_printf_float or -I \
                     include, in letters, digits and _ . + - / = , : only.",
                        text.shown()
                    ),
                ));
            }

            Ok(Some(line.to_string()))
        },
    )?;

    let twice = given_twice(build_flags.iter().map(|flag| flag.item.as_str()));

    if !twice.is_empty() {
        return Err(ManifestError::new(format!(
            "{FLAGS_KEY} has {} more than once.",
            twice.join(", ")
        )));
    }

    Ok(PioPart {
        build_flags,
        frameworks: names(own, "frameworks")?,
        platforms: names(own, "platforms")?,
        dependencies: dependencies(
            own.and_then(|own| own.get("dependencies")),
            "pio.library.dependencies",
        )?,
    })
}

/// A platform from platformio.ini as a plain name: espressif32 from platformio/espressif32@^6.5.
fn plain_platform(text: &str) -> Option<String> {
    let name = text.split('@').next().unwrap_or("").trim();

    if name.contains("://") || name.ends_with(".git") {
        return None;
    }

    Some(paths::leaf(name).to_string())
}

/// A PlatformIO project: platformio.ini, src/, and lib/ for its libraries.
pub struct PlatformIO;

impl Target for PlatformIO {
    fn platform(&self) -> Platform {
        Platform::Pio
    }

    fn no_project(&self) -> &'static str {
        "No platformio.ini here, so nothing was registered."
    }

    fn no_project_hint(&self) -> &'static str {
        "Run this from your project's folder, the one holding platformio.ini."
    }

    /// A project is a folder with platformio.ini. Without one, nothing would build the library.
    fn not_a_project(&self, project_root: &Path) -> Option<String> {
        (!project_root.join(ini::NAME).is_file()).then(|| {
            format!(
                "{} is not a PlatformIO project: it has no {name}. Run this from your project's folder, the one holding \
                 {name}, or give it with --project. Nothing was installed.",
                paths::posix(project_root),
                name = ini::NAME
            )
        })
    }

    /// Libraries go into lib/, which PlatformIO searches for every framework.
    fn folder_for(&self, library: &Manifest, _project_root: &Path) -> String {
        super::default_folder(Platform::Pio, &library.name)
    }

    /// platformio.ini, which this edits, and which a once file must never be.
    fn managed_files(&self, _project_root: &Path, _library: &Manifest) -> Vec<String> {
        super::managed_patterns(Platform::Pio)
            .into_iter()
            .map(String::from)
            .collect()
    }

    /// What the project is missing for this library. Never stops an install.
    fn check(&mut self, library: &Manifest, project_root: &Path, _ui: &mut dyn Ui) -> Vec<Finding> {
        let Ok(text) = files::read(&project_root.join(ini::NAME)) else {
            return vec![Finding::warn(
                "No platformio.ini here",
                "Run this from your PlatformIO project's folder, or give it with --project.",
            )];
        };

        let mut findings = Vec::new();
        let wanted = library.part.pio();
        let name = &library.name;

        // PlatformIO leaves a library a build cannot use out of that build without a word, and its header
        // is then simply not found.
        for (env, frameworks, platform) in ini::environments(&text) {
            let missing: Vec<&String> = frameworks
                .iter()
                .filter(|framework| {
                    !wanted.frameworks.is_empty() && !wanted.frameworks.contains(framework)
                })
                .collect();
            let plain = if platform.is_empty() {
                None
            } else {
                plain_platform(&platform)
            };

            if !missing.is_empty() {
                let missing: Vec<&str> =
                    missing.iter().map(|framework| framework.as_str()).collect();

                findings.push(Finding::warn(
                    format!(
                        "[env:{env}] builds with {}, which {name} does not list, so PlatformIO leaves it out of that build",
                        missing.join(", ")
                    ),
                    format!("{name} lists: {}.", wanted.frameworks.join(", ")),
                ));
            } else if let Some(plain) = plain
                && !plain.is_empty()
                && !wanted.platforms.is_empty()
                && !wanted.platforms.contains(&plain)
            {
                findings.push(Finding::warn(
                    format!("[env:{env}] builds for {plain}, which {name} does not list, so PlatformIO leaves it out of that build"),
                    format!("{name} lists: {}.", wanted.platforms.join(", ")),
                ));
            }
        }

        if !library.defines.is_empty() {
            findings.push(Finding::info(
                format!(
                    "{} goes into build_flags in platformio.ini",
                    library.defines.join(", ")
                ),
                "",
            ));
        }

        if !wanted.build_flags.is_empty() {
            let flags: Vec<&str> = wanted
                .build_flags
                .iter()
                .map(|flag| flag.item.as_str())
                .collect();

            findings.push(Finding::info(
                format!(
                    "{} {} into build_flags in platformio.ini",
                    flags.join(", "),
                    if flags.len() > 1 { "go" } else { "goes" }
                ),
                "",
            ));
        }

        for (dependency, version) in &wanted.dependencies {
            let version = if version == "*" {
                String::new()
            } else {
                format!(" {version}")
            };

            findings.push(Finding::info(
                format!("Needs {dependency}{version}, from the PlatformIO Registry"),
                "PlatformIO installs it by itself at the next build.",
            ));
        }

        for peripheral in &library.requires.peripherals {
            findings.push(Finding::info(
                format!("Needs {}", peripheral.describe()),
                "Set it up in your code.",
            ));
        }

        findings
    }

    /// Write the library's library.json, and its defines and build flags into platformio.ini.
    ///
    /// What an update dropped needs nothing here: library.json names the files of the new version only,
    /// and the block of defines is written again from the new manifest, so one that has gone is no longer
    /// in it.
    fn integrate(
        &mut self,
        project_root: &Path,
        library: &Manifest,
        destination: &Path,
        _dropped: &Dropped,
        _ui: &mut dyn Ui,
    ) -> Vec<Outcome> {
        let mut outcomes = vec![library_json::write(library, destination)];

        outcomes.extend(ini::integrate(project_root, library));

        let in_lib = paths::resolve(destination)
            .parent()
            .is_some_and(|parent| paths::same(parent, &paths::resolve(&project_root.join(LIB))));

        if !in_lib {
            outcomes.push(
                Outcome::new(
                    "lib_extra_dirs",
                    Status::Manual,
                    format!("PlatformIO looks for libraries in {LIB}/, and {} is not in it.", library.name),
                )
                .with_steps(vec![format!(
                    "Add its folder's parent to lib_extra_dirs in the [env] section of {}, or install it into {LIB}/ with --dir {LIB}/{}.",
                    ini::NAME,
                    library.name
                )]),
            );
        }

        outcomes
    }

    /// Its library.json, and its defines and build flags, go before its files do.
    fn uninstall(&mut self, project_root: &Path, library: &Installed) -> Vec<Outcome> {
        library_json::remove(library);

        ini::uninstall(project_root, &library.name)
            .into_iter()
            .collect()
    }

    /// platformio.ini, where lib-installer wrote its blocks.
    fn legacy_files(&self, project_root: &Path) -> Vec<PathBuf> {
        vec![project_root.join(ini::NAME)]
    }
}
