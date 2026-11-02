//! Finding which platform a project is, from the project's own files.
//!
//! lib-installer took the platform from the library. libNIMA starts from the project, so it reads what each platform
//! leaves behind, and says which file told it. It can be wrong, a folder can look like two things, and a folder can look
//! like nothing yet, so this only ever offers: the window lets the person change it, and the command takes --platform.

use std::path::{Path, PathBuf};

use crate::files;
use crate::platform::arduino::sketchbook;
use crate::platform::stm32::{checks, ide};
use crate::platform::{Platform, esp32, zephyr};
use crate::record;

/// One platform a folder looks like, and what told.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Candidate {
    pub platform: Platform,
    pub evidence: String,
}

/// What a folder looks like, what it looks like most first.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize)]
pub struct Detection {
    pub candidates: Vec<Candidate>,
    /// The IDEs an STM32 project is registered with, when it is one.
    pub ides: Vec<String>,
}

impl Detection {
    /// The platform to go with, when one stands out.
    pub fn platform(&self) -> Option<Platform> {
        self.candidates.first().map(|candidate| candidate.platform)
    }

    /// Whether a platform is among what the folder looks like.
    pub fn looks_like(&self, platform: Platform) -> bool {
        self.candidates
            .iter()
            .any(|candidate| candidate.platform == platform)
    }

    /// What told, for a platform it looks like.
    pub fn evidence_of(&self, platform: Platform) -> Option<&str> {
        self.candidates
            .iter()
            .find(|candidate| candidate.platform == platform)
            .map(|candidate| candidate.evidence.as_str())
    }
}

/// Whether a project file of an IDE that builds for any chip is one for an STM32.
fn names_stm32(path: &Path) -> bool {
    files::read_lossy(path).is_ok_and(|text| text.to_lowercase().contains("stm32"))
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// What in a folder says it is an STM32 project, the most telling first.
fn stm32_evidence(folder: &Path) -> Option<String> {
    if let Some(ioc) = checks::find_ioc(folder) {
        return Some(name_of(&ioc));
    }

    if let Some(hal_conf) = checks::find_hal_conf(folder) {
        return Some(name_of(&hal_conf));
    }

    let cproject = ide::cubeide::detect(folder);
    let uvprojx = ide::keil::detect(folder);
    let ewp = ide::iar::detect(folder);
    let found: [Option<PathBuf>; 3] = [cproject, uvprojx, ewp];

    found
        .into_iter()
        .flatten()
        .find(|path| names_stm32(path))
        .map(|path| name_of(&path))
}

/// What a folder looks like.
pub fn detect(folder: &Path) -> Detection {
    let mut found = Detection::default();
    let mut add = |platform: Platform, evidence: String| {
        if !found
            .candidates
            .iter()
            .any(|candidate| candidate.platform == platform)
        {
            found.candidates.push(Candidate { platform, evidence });
        }
    };

    // A project libraries went into has said what it is already.
    if let Some(platform) = record::recorded_platform(folder) {
        add(platform, "this project's record".to_string());
    }

    if folder.join("platformio.ini").is_file() {
        add(Platform::Pio, "platformio.ini".to_string());
    }

    if zephyr::app::find_cmakelists(folder).is_some() {
        add(
            Platform::Zephyr,
            "find_package(Zephyr) in CMakeLists.txt".to_string(),
        );
    }

    if esp32::project::find_project(folder).is_some() {
        add(
            Platform::Esp32,
            "project.cmake in CMakeLists.txt".to_string(),
        );
    }

    if let Some(evidence) = stm32_evidence(folder) {
        add(Platform::Stm32, evidence);
    }

    if sketchbook::is_sketch(folder) {
        let name = name_of(folder);
        let file = if folder.join(format!("{name}.ino")).is_file() {
            format!("{name}.ino")
        } else {
            format!("{name}.pde")
        };

        add(Platform::Arduino, file);
    }

    if found.looks_like(Platform::Stm32) {
        found.ides = ide::detect(folder)
            .iter()
            .map(|ide| ide.label.to_string())
            .collect();
    }

    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();

        for (name, text) in files {
            let path = root.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        root
    }

    fn platform_of(files: &[(&str, &str)]) -> Option<Platform> {
        detect(folder(files).path()).platform()
    }

    #[test]
    fn nothing_recognised_is_said_to_be_nothing() {
        assert_eq!(platform_of(&[("README.md", "hello")]), None);
    }

    #[test]
    fn each_platform_is_known_by_what_it_leaves() {
        assert_eq!(
            platform_of(&[("platformio.ini", "[env]\n")]),
            Some(Platform::Pio)
        );
        assert_eq!(
            platform_of(&[("CMakeLists.txt", "find_package(Zephyr REQUIRED)\n")]),
            Some(Platform::Zephyr)
        );
        assert_eq!(
            platform_of(&[(
                "CMakeLists.txt",
                "include($ENV{IDF_PATH}/tools/cmake/project.cmake)\n"
            )]),
            Some(Platform::Esp32)
        );
        assert_eq!(
            platform_of(&[("Board.ioc", "Mcu.IP0=SPI1\n")]),
            Some(Platform::Stm32)
        );
        assert_eq!(
            platform_of(&[("Core/Inc/stm32g4xx_hal_conf.h", "")]),
            Some(Platform::Stm32)
        );
    }

    #[test]
    fn the_evidence_names_the_file() {
        let root = folder(&[("MyBoard.ioc", "")]);
        let found = detect(root.path());

        assert_eq!(found.candidates[0].evidence, "MyBoard.ioc");
    }

    #[test]
    fn a_sketch_is_known_by_the_ino_named_after_it() {
        let root = tempfile::tempdir().unwrap();
        let sketch = root.path().join("Blink");
        fs::create_dir_all(&sketch).unwrap();
        fs::write(sketch.join("Blink.ino"), "").unwrap();

        let found = detect(&sketch);

        assert_eq!(found.platform(), Some(Platform::Arduino));
        assert_eq!(found.candidates[0].evidence, "Blink.ino");
    }

    #[test]
    fn an_ide_file_for_another_chip_says_nothing() {
        assert_eq!(
            platform_of(&[(".cproject", "<cproject/>"), (".project", "")]),
            None
        );
        assert_eq!(
            platform_of(&[("MDK/P.uvprojx", "<Device>LPC1768</Device>")]),
            None
        );
        assert_eq!(
            platform_of(&[
                (".cproject", "<x id=\"com.st.stm32cube.ide\"/>"),
                (".project", "")
            ]),
            Some(Platform::Stm32)
        );
        assert_eq!(
            platform_of(&[("MDK/P.uvprojx", "<Device>STM32G431</Device>")]),
            Some(Platform::Stm32)
        );
    }

    #[test]
    fn a_cmake_project_alone_says_nothing() {
        assert_eq!(platform_of(&[("CMakeLists.txt", "project(x)\n")]), None);
    }

    #[test]
    fn the_record_comes_before_everything() {
        let found = detect(
            folder(&[
                (".libnima.json", "{\"platform\": \"stm32\"}"),
                ("platformio.ini", "[env]\n"),
            ])
            .path(),
        );

        assert_eq!(found.platform(), Some(Platform::Stm32));
        assert_eq!(found.candidates[0].evidence, "this project's record");
        assert!(found.looks_like(Platform::Pio));
    }

    #[test]
    fn the_ides_of_an_stm32_project_are_named() {
        let found = detect(
            folder(&[
                ("Board.ioc", ""),
                (".project", ""),
                (".cproject", ""),
                ("CMakeLists.txt", "project(x)\n"),
            ])
            .path(),
        );

        assert_eq!(found.ides, ["CMake", "STM32CubeIDE"]);
    }
}
