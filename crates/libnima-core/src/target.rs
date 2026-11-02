//! What one platform adds to the core.
//!
//! The core fetches a library, reads its libnima.yml, copies its files and keeps what the
//! user wrote. What a project looks like belongs to each platform: STM32CubeIDE, Keil and
//! IAR for one, Zephyr's CMake and Kconfig for another. So each platform fills in a
//! [`Target`], and the core asks it wherever the answer depends on the kind of project.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::installer::{InstallError, Installed};
use crate::manifest::Manifest;
use crate::platform::Platform;
use crate::report::Ui;

/// How seriously to take a [`Finding`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Warn,
    Info,
}

/// One observation about the project, with how seriously to take it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Finding {
    pub level: Level,
    pub message: String,
    pub hint: String,
}

impl Finding {
    pub fn ok(message: impl Into<String>) -> Finding {
        Finding {
            level: Level::Ok,
            message: message.into(),
            hint: String::new(),
        }
    }

    pub fn warn(message: impl Into<String>, hint: impl Into<String>) -> Finding {
        Finding {
            level: Level::Warn,
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn info(message: impl Into<String>, hint: impl Into<String>) -> Finding {
        Finding {
            level: Level::Info,
            message: message.into(),
            hint: hint.into(),
        }
    }
}

/// What an integration did to the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Changed,
    Already,
    Manual,
}

/// What an integration did, and what the user still has to do themselves.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub ide: String,
    pub status: Status,
    pub message: String,
    pub steps: Vec<String>,
    pub backup: Option<PathBuf>,
    /// The blocks this left in project files, {file: {marker name: fingerprint}}, for
    /// the record: see blocks.rs.
    pub blocks: Map<String, Value>,
}

impl Outcome {
    pub fn new(ide: impl Into<String>, status: Status, message: impl Into<String>) -> Outcome {
        Outcome {
            ide: ide.into(),
            status,
            message: message.into(),
            steps: Vec::new(),
            backup: None,
            blocks: Map::new(),
        }
    }

    pub fn with_steps(mut self, steps: Vec<String>) -> Outcome {
        self.steps = steps;
        self
    }

    pub fn with_backup(mut self, backup: Option<PathBuf>) -> Outcome {
        self.backup = backup;
        self
    }

    pub fn with_blocks(mut self, file: impl Into<String>, found: Map<String, Value>) -> Outcome {
        self.blocks.insert(file.into(), Value::Object(found));
        self
    }
}

/// What a run gives an integration besides the library: what it dropped from an update.
#[derive(Debug, Clone, Default)]
pub struct Dropped {
    /// Files an update just removed, because the new version no longer ships them there.
    pub files: Vec<PathBuf>,
    /// Defines the last install of this library put in the project and this version no
    /// longer asks for.
    pub defines: Vec<String>,
}

/// The answers that depend on the kind of project. Each platform gives its own.
pub trait Target {
    fn platform(&self) -> Platform;

    /// Said when no project file was recognised, and what to do instead.
    fn no_project(&self) -> &'static str;
    fn no_project_hint(&self) -> &'static str;

    /// The folder this run works in. `given` is the folder the run was pointed at. A
    /// target whose libraries live somewhere of their own, such as Arduino's libraries
    /// folder, answers with that.
    fn project_root(&mut self, given: &Path, _ui: &mut dyn Ui) -> Result<PathBuf, InstallError> {
        Ok(given.to_path_buf())
    }

    /// The folder a library another one needs is installed into, and looked for in. The
    /// project's own, unless the target keeps them elsewhere.
    fn dependency_root(&self, project_root: &Path) -> PathBuf {
        project_root.to_path_buf()
    }

    /// The folder offered for a library installed for the first time into `project_root`.
    fn folder_for(&self, library: &Manifest, project_root: &Path) -> String;

    /// The folder a manifest's once files land in: the project's own, or None for a
    /// target with no project of its own, such as Arduino's sketchbook, where such a file
    /// has nowhere to go and is refused.
    fn once_root(&self, project_root: &Path) -> Option<PathBuf> {
        Some(project_root.to_path_buf())
    }

    /// Said when once_root() has no folder, with the way to get one.
    fn no_once_root(&self) -> &'static str {
        "This kind of project has no folder of its own to put them in."
    }

    /// Files in the project a once file must never be, because libNIMA edits them or
    /// reads them as the project's own: paths from once_root(), or patterns such as
    /// *.ioc. The record is always one of them.
    fn managed_files(&self, project_root: &Path, library: &Manifest) -> Vec<String>;

    /// What goes between the quotes of the #include the last line names, or None for the
    /// first header, relative to its include folder.
    fn include_line(
        &self,
        _library: &Manifest,
        _destination: &Path,
        _project_root: &Path,
    ) -> Option<String> {
        None
    }

    /// Findings about the project, before the library goes in.
    fn check(&mut self, library: &Manifest, project_root: &Path, ui: &mut dyn Ui) -> Vec<Finding>;

    /// Register an installed library with the project.
    fn integrate(
        &mut self,
        project_root: &Path,
        library: &Manifest,
        destination: &Path,
        dropped: &Dropped,
        ui: &mut dyn Ui,
    ) -> Vec<Outcome>;

    /// Take a library out of the project's build, before its files go.
    fn uninstall(&mut self, project_root: &Path, library: &Installed) -> Vec<Outcome>;

    /// The folders where a record of lib-installer can be, which the first run moves over
    /// before anything reads them: the project's.
    fn legacy_roots(&self, given: &Path) -> Vec<PathBuf> {
        vec![given.to_path_buf()]
    }

    /// The project's files that can hold blocks between lib-installer's markers, which
    /// the first run renames to libNIMA's.
    fn legacy_files(&self, _project_root: &Path) -> Vec<PathBuf> {
        Vec::new()
    }
}
