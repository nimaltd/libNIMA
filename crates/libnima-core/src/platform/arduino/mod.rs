//! What libNIMA knows about Arduino: where Arduino finds a library.
//!
//! Arduino has two places a library can go, and the user chooses:
//!
//! ```text
//! the libraries folder   of the sketchbook, where Library Manager installs. Every sketch sees the
//!                        library, and a library that includes another finds it. The library gets a
//!                        library.properties, which Arduino knows it by.
//! a sketch's src folder  for that sketch only, with its own settings. Arduino compiles everything in
//!                        it, but puts none of it on the include path: the sketch includes the library
//!                        by its path, "src/example/example.h", and one library there cannot find
//!                        another.
//! ```
//!
//! So what a library needs always goes into the libraries folder, where any library finds it, a
//! library installed into a sketch included.

pub mod properties;
pub mod sketchbook;

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::installer::{InstallError, Installed};
use crate::manifest::{Manifest, ManifestError};
use crate::paths;
use crate::platform::Platform;
use crate::record::installed_libraries;
use crate::report::{self, Question, QuestionKind, Style, Ui};
use crate::target::{Dropped, Finding, Outcome, Status, Target};
use crate::titles;
use crate::yaml::{Mapping, Node};

const LIBRARIES: &str = "libraries";
const SKETCH: &str = "sketch";

/// The keys of the arduino section.
const KEYS: [&str; 5] = [
    "architectures",
    "category",
    "author",
    "maintainer",
    "depends",
];

/// An architecture as boards name it in library.properties: avr, esp32, mbed_nano, or * for any.
static ARCHITECTURE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\*|[A-Za-z0-9_.-]+)$").expect("a valid pattern"));

/// A library of Library Manager's, with a version when one is needed: "Adafruit BusIO" or
/// "ArduinoJson (>=6.0.0)".
static DEPENDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9][A-Za-z0-9 _.-]*(?: \([<>=!]*[0-9][0-9.]*\))?$")
        .expect("a valid pattern")
});

/// The arduino section of libnima.yml, with what it leaves out filled in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ArduinoPart {
    pub architectures: Vec<String>,
    pub category: Option<String>,
    pub author: Option<String>,
    pub maintainer: Option<String>,
    pub depends: Vec<String>,
}

/// One line of text from the arduino section, or None.
fn one_line_of(data: &Mapping, key: &str) -> Result<Option<String>, ManifestError> {
    let Some(value) = data.given(key) else {
        return Ok(None);
    };

    let text = value.text().trim().to_string();

    if text.contains('\n') || text.contains('\r') {
        return Err(ManifestError::at(
            value.line,
            format!("arduino.{key} is one line of text."),
        ));
    }

    Ok((!text.is_empty()).then_some(text))
}

pub fn read_part(data: &Mapping) -> Result<ArduinoPart, ManifestError> {
    let section = data.given("arduino");

    if let Some(section) = section
        && section.as_map().is_none()
    {
        return Err(ManifestError::at(
            section.line,
            "arduino should hold architectures, category, author, maintainer and depends.",
        ));
    }

    // A key the section does not have, such as a misspelt architectures.
    let map = titles::part(section, "arduino", &KEYS)?;
    let empty = Mapping::default();
    let map = map.unwrap_or(&empty);

    let architectures: Vec<String> = match map.given("architectures").filter(|value| value.truthy())
    {
        None => vec!["*".to_string()],
        Some(value) => {
            let names: Option<Vec<String>> = match (value.as_str(), value.as_list()) {
                (Some(one), _) => Some(vec![one.to_string()]),
                (_, Some(list)) => Some(list.iter().map(Node::text).collect()),
                _ => None,
            };

            match names.filter(|names| names.iter().all(|name| ARCHITECTURE.is_match(name))) {
                Some(names) => names,
                None => {
                    return Err(ManifestError::at(
                        value.line,
                        "arduino.architectures is a list of the architectures it runs on, such as [avr, esp32], or [\"*\"] for any.",
                    ));
                }
            }
        }
    };

    let category = one_line_of(map, "category")?;

    if let Some(category) = &category
        && !properties::CATEGORIES.contains(&category.as_str())
    {
        return Err(ManifestError::at(
            map.line_of("category").unwrap_or(0),
            format!(
                "arduino.category has {}. Use one of Arduino's: {}.",
                crate::yaml::quoted(category),
                properties::CATEGORIES.join(", ")
            ),
        ));
    }

    let depends: Vec<String> = match map.given("depends").filter(|value| value.truthy()) {
        None => Vec::new(),
        Some(value) => {
            let names: Option<Vec<String>> = value.as_list().map(|list| {
                list.iter()
                    .map(|name| name.text().trim().to_string())
                    .collect()
            });

            match names.filter(|names| names.iter().all(|name| DEPENDS.is_match(name))) {
                Some(names) => names,
                None => {
                    return Err(ManifestError::at(
                        value.line,
                        "arduino.depends is a list of Library Manager libraries by their name, such as [\"Adafruit \
                         BusIO\"], with a version when one is needed: \"ArduinoJson (>=6.0.0)\".",
                    ));
                }
            }
        }
    };

    Ok(ArduinoPart {
        architectures,
        category,
        author: one_line_of(map, "author")?,
        maintainer: one_line_of(map, "maintainer")?,
        depends,
    })
}

/// Arduino: the sketchbook's libraries folder, or one sketch's src folder.
pub struct Arduino {
    into: Option<String>,
    sketchbook_given: Option<String>,
    uninstalling: Vec<String>,
    /// Settled by project_root() at the start of each run.
    libraries: PathBuf,
}

impl Arduino {
    pub fn new(
        into: Option<String>,
        sketchbook_given: Option<String>,
        uninstalling: Vec<String>,
    ) -> Arduino {
        Arduino {
            into,
            sketchbook_given,
            uninstalling,
            libraries: PathBuf::new(),
        }
    }

    fn in_libraries(&self, project_root: &Path) -> bool {
        paths::same(
            &paths::resolve(project_root),
            &paths::resolve(&self.libraries),
        )
    }

    /// Ask which of the two places the libraries go into.
    fn ask_where(&self, sketch: &Path, default: &str, ui: &mut dyn Ui) -> String {
        let first = if default == LIBRARIES { "1" } else { "2" };
        let name = sketch.file_name().unwrap_or_default().to_string_lossy();

        ui.say(report::blank());
        ui.say(report::heading(format!(
            "This is the sketch {name}. Where do the libraries go?"
        )));
        ui.say(report::plain(format!(
            "  1  the libraries folder, for every sketch: {}",
            paths::posix(&self.libraries)
        )));
        ui.say(report::plain(
            "  2  this sketch's src folder, for this sketch only",
        ));

        let question = Question {
            prompt: report::line(format!("Install into [{first}]: "), Style::Bold),
            kind: QuestionKind::Text,
        };

        let Some(answer) = ui.ask(&question).filter(|answer| !answer.is_empty()) else {
            return default.to_string();
        };

        let answer = answer.trim().to_lowercase();

        if matches!(answer.as_str(), "2" | "s" | SKETCH | "src") {
            SKETCH.to_string()
        } else {
            LIBRARIES.to_string()
        }
    }

    /// Where the libraries to take out are recorded: sketch, libraries, or None.
    fn recorded_where(&self, given: &Path) -> Result<Option<String>, InstallError> {
        let in_sketch = if sketchbook::is_sketch(given) {
            installed_libraries(given)
        } else {
            Default::default()
        };
        let in_libraries = installed_libraries(&self.libraries);
        let names = &self.uninstalling;

        if names.iter().all(|name| in_sketch.contains_key(name)) {
            return Ok(Some(SKETCH.to_string()));
        }

        if names.iter().all(|name| in_libraries.contains_key(name)) {
            return Ok(Some(LIBRARIES.to_string()));
        }

        if names.iter().any(|name| in_sketch.contains_key(name)) {
            return Err(InstallError(
                "Some of these are in this sketch and some in the libraries folder. Take them out in two runs, with \
                 --into sketch and --into libraries."
                    .to_string(),
            ));
        }

        Ok(None)
    }
}

impl Target for Arduino {
    fn platform(&self) -> Platform {
        Platform::Arduino
    }

    fn no_project(&self) -> &'static str {
        "Nothing was registered."
    }

    fn no_project_hint(&self) -> &'static str {
        "Arduino finds a library in the libraries folder by itself."
    }

    /// The libraries folder, or the sketch run from, as the user chose.
    ///
    /// Run from a sketch, the user is asked, and in a sketch libraries have been put into before, its
    /// src folder is the answer offered. Anywhere else it is the libraries folder, without a question.
    fn project_root(&mut self, given: &Path, ui: &mut dyn Ui) -> Result<PathBuf, InstallError> {
        let (found, how) = sketchbook::sketchbook(self.sketchbook_given.as_deref());
        self.libraries = found.join("libraries");

        let is_sketch = sketchbook::is_sketch(given);
        let mut place = self.into.clone();

        if place.is_none() && !self.uninstalling.is_empty() {
            place = self.recorded_where(given)?;
        }

        if place.as_deref() == Some(SKETCH) && !is_sketch {
            let name = given.file_name().unwrap_or_default().to_string_lossy();

            return Err(InstallError(format!(
                "{} is not a sketch: it has no {name}.ino. Run this from the sketch's folder, or give it with --project.",
                paths::posix(given)
            )));
        }

        if place.is_none() && is_sketch && self.uninstalling.is_empty() {
            let default = if installed_libraries(given).is_empty() {
                LIBRARIES
            } else {
                SKETCH
            };
            place = Some(self.ask_where(given, default, ui));
        }

        if place.as_deref() == Some(SKETCH) {
            return Ok(given.to_path_buf());
        }

        ui.say(report::note(format!(
            "Libraries folder: {}  (from {how})",
            paths::posix(&self.libraries)
        )));

        Ok(self.libraries.clone())
    }

    /// What a library needs goes into the libraries folder, where any library finds it.
    fn dependency_root(&self, _project_root: &Path) -> PathBuf {
        self.libraries.clone()
    }

    /// The sketch, for a library installed into one. The libraries folder is no sketch's own, so there a
    /// once file, or a run step, has nowhere to go.
    fn once_root(&self, project_root: &Path) -> Option<PathBuf> {
        (!self.in_libraries(project_root)).then(|| project_root.to_path_buf())
    }

    fn no_once_root(&self) -> &'static str {
        "The libraries folder is shared by every sketch, so it has no place for them. Install it into a sketch, with \
         --into sketch, to get them."
    }

    /// Its name in the libraries folder, src/<name> in a sketch.
    fn folder_for(&self, library: &Manifest, project_root: &Path) -> String {
        if self.in_libraries(project_root) {
            library.name.clone()
        } else {
            format!("src/{}", library.name)
        }
    }

    /// The sketch's own .ino files, which a once file must never be.
    fn managed_files(&self, _project_root: &Path, _library: &Manifest) -> Vec<String> {
        vec![crate::record::RECORD_NAME.to_string(), "*.ino".to_string()]
    }

    /// In a sketch, the header by its path from the sketch, which is how a sketch finds it.
    fn include_line(
        &self,
        library: &Manifest,
        destination: &Path,
        project_root: &Path,
    ) -> Option<String> {
        if self.in_libraries(project_root) {
            return None;
        }

        let first = library.headers.first()?;
        let header = paths::resolve(&destination.join(&first.destination));

        paths::relative_to(&header, &paths::resolve(project_root)).map(|rest| paths::posix(&rest))
    }

    /// What Arduino cannot do for this library, said before it goes in. Never stops an install.
    fn check(&mut self, library: &Manifest, project_root: &Path, _ui: &mut dyn Ui) -> Vec<Finding> {
        let mut findings = Vec::new();
        let in_libraries = self.in_libraries(project_root);

        if !library.defines.is_empty() {
            findings.push(Finding::warn(
                format!(
                    "Arduino has no defines for a whole build, so these are not set: {}",
                    library.defines.join(", ")
                ),
                format!(
                    "Set them in {}'s configuration header, if it has one.",
                    library.name
                ),
            ));
        }

        if !in_libraries {
            for dependency in library.dependencies() {
                findings.push(Finding::info(
                    format!("{}, which it needs, goes into the libraries folder", dependency.name),
                    "In a sketch's src folder one library cannot find another. In the libraries folder every library and \
                     sketch can.",
                ));
            }
        }

        for name in &library.part.arduino().depends {
            let plain = name.split(" (").next().unwrap_or(name);

            if self.libraries.join(plain).is_dir()
                || self.libraries.join(plain.replace(' ', "_")).is_dir()
            {
                findings.push(Finding::ok(format!(
                    "{plain}, from Library Manager, is installed"
                )));
            } else {
                findings.push(Finding::warn(
                    format!("Needs {plain}, from Library Manager"),
                    "Install it with the Library Manager, or arduino-cli lib install.",
                ));
            }
        }

        for peripheral in &library.requires.peripherals {
            findings.push(Finding::info(
                format!("Needs {}", peripheral.describe()),
                "Wire it up and set it up in your sketch.",
            ));
        }

        findings
    }

    /// In the libraries folder, a library.properties. In a sketch, nothing: Arduino builds src/ itself.
    fn integrate(
        &mut self,
        project_root: &Path,
        library: &Manifest,
        destination: &Path,
        _dropped: &Dropped,
        _ui: &mut dyn Ui,
    ) -> Vec<Outcome> {
        if self.in_libraries(project_root) {
            return vec![properties::write(library, destination)];
        }

        vec![Outcome::new(
            "Sketch",
            Status::Already,
            "Arduino builds every file in the sketch's src folder, so there is nothing to register.",
        )]
    }

    /// The library.properties written for it goes, before its files do.
    fn uninstall(&mut self, _project_root: &Path, library: &Installed) -> Vec<Outcome> {
        if let Some(destination) = &library.destination {
            properties::remove(destination);
        }

        Vec::new()
    }

    /// The sketch run from and the sketchbook's libraries folder, since a record can be in whichever a
    /// library went into.
    fn legacy_roots(&self, given: &Path) -> Vec<PathBuf> {
        let (found, _) = sketchbook::sketchbook(self.sketchbook_given.as_deref());
        let libraries = found.join("libraries");

        if libraries == given {
            vec![given.to_path_buf()]
        } else {
            vec![given.to_path_buf(), libraries]
        }
    }
}
