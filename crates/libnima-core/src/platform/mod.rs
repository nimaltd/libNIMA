//! The platforms libNIMA installs into, and what each adds to a manifest.

pub mod arduino;
pub mod esp32;
pub mod kconfig;
pub mod pio;
pub mod stm32;
pub mod zephyr;

use crate::manifest::{Manifest, ManifestError};
use crate::target::Target;
use crate::yaml::Mapping;

/// A platform, as platform: in libnima.yml and --platform name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Stm32,
    Zephyr,
    Esp32,
    Arduino,
    Pio,
}

impl Platform {
    /// In the order the help lists them.
    pub const ALL: [Platform; 5] = [
        Platform::Stm32,
        Platform::Zephyr,
        Platform::Esp32,
        Platform::Arduino,
        Platform::Pio,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Platform::Stm32 => "stm32",
            Platform::Zephyr => "zephyr",
            Platform::Esp32 => "esp32",
            Platform::Arduino => "arduino",
            Platform::Pio => "pio",
        }
    }

    /// How messages name the platform.
    pub fn label(self) -> &'static str {
        match self {
            Platform::Stm32 => "STM32",
            Platform::Zephyr => "Zephyr",
            Platform::Esp32 => "ESP-IDF",
            Platform::Arduino => "Arduino",
            Platform::Pio => "PlatformIO",
        }
    }

    pub fn from_name(name: &str) -> Option<Platform> {
        Platform::ALL
            .into_iter()
            .find(|platform| platform.name() == name)
    }

    /// The installer it was before lib-installer 3.0.0, such as zephyr-installer.
    pub fn old_installer(self) -> String {
        format!("{}-installer", self.name())
    }
}

/// A platform's own section of a manifest, once it is read.
#[derive(Debug, Clone, Default)]
pub enum Part {
    #[default]
    None,
    Stm32(stm32::Stm32Part),
    Zephyr(Box<zephyr::ZephyrPart>),
    Esp32(Box<esp32::Esp32Part>),
    Arduino(arduino::ArduinoPart),
    Pio(pio::PioPart),
}

impl Part {
    pub fn stm32(&self) -> &stm32::Stm32Part {
        match self {
            Part::Stm32(part) => part,
            _ => panic!("the manifest was not read for STM32"),
        }
    }

    pub fn zephyr(&self) -> &zephyr::ZephyrPart {
        match self {
            Part::Zephyr(part) => part,
            _ => panic!("the manifest was not read for Zephyr"),
        }
    }

    pub fn esp32(&self) -> &esp32::Esp32Part {
        match self {
            Part::Esp32(part) => part,
            _ => panic!("the manifest was not read for ESP-IDF"),
        }
    }

    pub fn arduino(&self) -> &arduino::ArduinoPart {
        match self {
            Part::Arduino(part) => part,
            _ => panic!("the manifest was not read for Arduino"),
        }
    }

    pub fn pio(&self) -> &pio::PioPart {
        match self {
            Part::Pio(part) => part,
            _ => panic!("the manifest was not read for PlatformIO"),
        }
    }
}

/// Read a platform's own section onto a manifest, once the rest is read.
/// The files of a project of the platform that libNIMA edits or reads as the project's own, by name or pattern, which a
/// once file, or an extra with a root, may never be. What a target knows only in a project, such as a Zephyr application's
/// Kconfig file when it is not prj.conf, it adds itself.
pub fn managed_patterns(platform: Platform) -> Vec<&'static str> {
    let mut found = vec![crate::record::RECORD_NAME];

    found.extend_from_slice(match platform {
        Platform::Stm32 => &[
            "CMakeLists.txt",
            ".cproject",
            ".project",
            "*.uvprojx",
            "*.ewp",
            "*.ewt",
            "*.eww",
            "Makefile",
            "*.ioc",
            "*_hal_conf.h",
        ][..],
        Platform::Zephyr => &["CMakeLists.txt", "prj.conf", "sysbuild.conf"],
        Platform::Esp32 => &[
            esp32::project::CMAKELISTS,
            esp32::project::MAIN,
            esp32::project::SDKCONFIG,
            esp32::project::DEFAULTS,
        ],
        Platform::Arduino => &["*.ino"],
        Platform::Pio => &[pio::ini::NAME],
    });

    found
}

pub fn read_part(
    platform: Platform,
    manifest: &mut Manifest,
    data: &Mapping,
) -> Result<(), ManifestError> {
    manifest.part = match platform {
        Platform::Stm32 => Part::Stm32(stm32::read_part(data)?),
        Platform::Zephyr => Part::Zephyr(Box::new(zephyr::read_part(manifest, data)?)),
        Platform::Esp32 => Part::Esp32(Box::new(esp32::read_part(manifest, data)?)),
        Platform::Arduino => Part::Arduino(arduino::read_part(data)?),
        Platform::Pio => Part::Pio(pio::read_part(data)?),
    };

    Ok(())
}

/// Values in a manifest a platform does not know, as sentences.
pub fn warnings(platform: Platform, manifest: &Manifest) -> Vec<String> {
    match platform {
        Platform::Stm32 => stm32::warnings(manifest),
        Platform::Zephyr => zephyr::warnings(manifest),
        Platform::Esp32 => esp32::warnings(manifest),
        Platform::Arduino | Platform::Pio => Vec::new(),
    }
}

/// The folder a library goes into when nobody gives it another, as a path from the project's folder: what the folder
/// question offers a project that has not got it yet. Arduino's is the one in a sketch, the only place where a library has
/// files of the project's own beside it.
pub fn default_folder(platform: Platform, name: &str) -> String {
    match platform {
        Platform::Stm32 => name.to_string(),
        Platform::Zephyr => format!("modules/{name}"),
        Platform::Esp32 => format!("{}/{name}", esp32::project::COMPONENTS),
        Platform::Pio => format!("{}/{name}", pio::LIB),
        Platform::Arduino => format!("src/{name}"),
    }
}

/// What a run asks of each platform's options, from the command line or the window.
#[derive(Debug, Clone, Default)]
pub struct PlatformArgs {
    /// STM32: only register with this IDE.
    pub ide: Option<String>,
    /// Zephyr: the Kconfig file that switches the libraries on.
    pub conf: Option<String>,
    /// Zephyr: the overlay, or a wildcard of overlays, that includes the libraries' devicetree.
    pub overlay: Option<String>,
    /// Arduino: "libraries" or "sketch".
    pub into: Option<String>,
    /// Arduino: the sketchbook folder.
    pub sketchbook: Option<String>,
    /// Arduino: the libraries a run takes out, to find where they are recorded.
    pub uninstalling: Vec<String>,
}

/// The target of a run on this platform.
pub fn target_of(platform: Platform, args: &PlatformArgs) -> Box<dyn Target> {
    match platform {
        Platform::Stm32 => Box::new(stm32::Stm32::new(args.ide.clone())),
        Platform::Zephyr => Box::new(zephyr::Zephyr::new(args.conf.clone(), args.overlay.clone())),
        Platform::Esp32 => Box::new(esp32::Esp32),
        Platform::Arduino => Box::new(arduino::Arduino::new(
            args.into.clone(),
            args.sketchbook.clone(),
            args.uninstalling.clone(),
        )),
        Platform::Pio => Box::new(pio::PlatformIO),
    }
}
