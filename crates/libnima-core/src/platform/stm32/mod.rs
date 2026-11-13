//! What libNIMA knows about STM32: what an STM32 project is.
//!
//! The core does everything a library needs whatever it is installed into. This says the
//! rest for STM32: the requirements a libnima.yml can state, checked against the CubeMX
//! project by checks.rs, and the IDE project files that ide/ registers the library with.

pub mod checks;
pub mod ide;

use std::path::{Path, PathBuf};

use crate::installer::Installed;
use crate::manifest::{Manifest, ManifestError};
use crate::platform::Platform;
use crate::report::Ui;
use crate::target::{Dropped, Finding, Outcome, Target};
use crate::titles;
use crate::yaml::{Mapping, Node};

/// How a library relates to an operating system.
const RTOS_CHOICES: [&str; 6] = [
    "none",
    "optional",
    "any",
    "freertos",
    "cmsis-os2",
    "threadx",
];

/// The STM32 section of a libnima.yml: the HAL, the RTOS, and what CubeMX sets up.
#[derive(Debug, Clone, PartialEq)]
pub struct Stm32Part {
    pub hal: bool,
    pub cmsis: bool,
    pub rtos: String,
    pub hal_modules: Vec<String>,
    pub c_standard: String,
    /// True when the library will not work without an operating system.
    pub needs_rtos: bool,
}

pub fn read_part(data: &Mapping) -> Result<Stm32Part, ManifestError> {
    let section = titles::part(data.get("stm32"), "stm32", &["requires"])?;
    let requires = titles::part(
        section.and_then(|section| section.get("requires")),
        "stm32.requires",
        &["hal", "cmsis", "rtos", "hal_modules", "c_standard"],
    )?;

    let get = |key: &str| requires.and_then(|requires| requires.get(key));
    let rtos = get("rtos").map_or_else(|| "none".to_string(), |rtos| rtos.text().to_lowercase());

    let hal_modules = match get("hal_modules").filter(|modules| modules.truthy()) {
        None => Vec::new(),
        Some(modules) => {
            let Some(list) = modules.as_list() else {
                return Err(ManifestError::at(
                    modules.line,
                    "stm32.requires.hal_modules is a list of HAL modules, such as [spi].",
                ));
            };

            list.iter()
                .map(|module| module.text().to_lowercase())
                .collect()
        }
    };

    Ok(Stm32Part {
        hal: get("hal").is_none_or(Node::truthy),
        cmsis: get("cmsis").is_some_and(Node::truthy),
        needs_rtos: rtos != "none" && rtos != "optional",
        rtos,
        hal_modules,
        c_standard: get("c_standard").map_or_else(
            || "c11".to_string(),
            |standard| standard.text().to_lowercase(),
        ),
    })
}

pub fn warnings(manifest: &Manifest) -> Vec<String> {
    let rtos = &manifest.part.stm32().rtos;

    if RTOS_CHOICES.contains(&rtos.as_str()) {
        return Vec::new();
    }

    vec![format!(
        "stm32.requires.rtos '{rtos}' is not one of: {}",
        RTOS_CHOICES.join(", ")
    )]
}

/// An STM32 project, made by CubeMX and built with any IDE it supports.
pub struct Stm32 {
    /// Only register with this IDE, of ide::NAMES.
    only: Option<String>,
}

impl Stm32 {
    pub fn new(only: Option<String>) -> Stm32 {
        Stm32 { only }
    }
}

impl Target for Stm32 {
    fn platform(&self) -> Platform {
        Platform::Stm32
    }

    fn no_project(&self) -> &'static str {
        "No IDE project was recognised here, so nothing was registered."
    }

    fn no_project_hint(&self) -> &'static str {
        "Add the folder to your include paths and the .c files to your build."
    }

    fn folder_for(&self, library: &Manifest, _project_root: &Path) -> String {
        super::default_folder(Platform::Stm32, &library.name)
    }

    /// The project's files this edits or reads as its own, which a once file must never
    /// be: every IDE's project file, the Makefile, CubeMX's .ioc, and the HAL
    /// configuration header.
    fn managed_files(&self, _project_root: &Path, _library: &Manifest) -> Vec<String> {
        super::managed_patterns(Platform::Stm32)
            .into_iter()
            .map(String::from)
            .collect()
    }

    fn check(&mut self, library: &Manifest, project_root: &Path, _ui: &mut dyn Ui) -> Vec<Finding> {
        checks::check(library, project_root)
    }

    fn integrate(
        &mut self,
        project_root: &Path,
        library: &Manifest,
        destination: &Path,
        dropped: &Dropped,
        _ui: &mut dyn Ui,
    ) -> Vec<Outcome> {
        ide::integrate(
            project_root,
            library,
            destination,
            self.only.as_deref(),
            dropped,
        )
    }

    fn uninstall(&mut self, project_root: &Path, library: &Installed) -> Vec<Outcome> {
        ide::uninstall(project_root, library)
    }

    /// The CMakeLists.txt at the project's top, where lib-installer wrote its blocks.
    fn legacy_files(&self, project_root: &Path) -> Vec<PathBuf> {
        vec![project_root.join("CMakeLists.txt")]
    }
}
