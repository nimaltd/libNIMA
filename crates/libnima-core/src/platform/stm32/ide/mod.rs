//! Registering an installed library with whatever IDE the project uses.
//!
//! A project can be more than one thing at once. A CubeMX project that also emits CMake
//! is both a CubeIDE project and a CMake project, and a user may build it either way, so
//! every integration that recognises the project runs.
//!
//! Copying the files always succeeds. Registering them with an IDE may not, and a failure
//! there is reported rather than raised: the library is on disk either way, and the user
//! can finish by hand from the steps printed.

pub mod base;
pub mod cmake;
pub mod cubeide;
pub mod iar;
pub mod keil;
pub mod makefile;

use std::io;
use std::path::{Path, PathBuf};

use crate::files;
use crate::installer::Installed;
use crate::manifest::Manifest;
use crate::target::{Dropped, Outcome, Status};

/// What --ide can choose from.
pub const NAMES: [&str; 5] = ["cmake", "cubeide", "keil", "iar", "makefile"];

/// One integration: its name for --ide, its name in messages, and the project file found.
pub struct Found {
    pub backend: &'static str,
    pub label: &'static str,
    pub path: PathBuf,
}

/// Every IDE that recognises this project.
pub fn detect(project_root: &Path) -> Vec<Found> {
    let mut found = Vec::new();
    let mut add = |backend: &'static str, label: &'static str, path: Option<PathBuf>| {
        if let Some(path) = path {
            found.push(Found {
                backend,
                label,
                path,
            });
        }
    };

    add("cmake", cmake::NAME, cmake::detect(project_root));
    add("cubeide", cubeide::NAME, cubeide::detect(project_root));
    add("keil", keil::NAME, keil::detect(project_root));
    add("iar", iar::NAME, iar::detect(project_root));
    add("makefile", makefile::NAME, makefile::detect(project_root));

    found
}

fn manual_steps(backend: &str, folder: &str, library: &str) -> Vec<String> {
    match backend {
        "cmake" => cmake::manual_steps(folder, library),
        "cubeide" => cubeide::manual_steps(folder),
        "keil" => keil::manual_steps(folder),
        "iar" => iar::manual_steps(folder),
        _ => makefile::manual_steps(folder),
    }
}

/// Register a library with every IDE found in the project, or with one by its name.
pub fn integrate(
    project_root: &Path,
    library: &Manifest,
    destination: &Path,
    only: Option<&str>,
    dropped: &Dropped,
) -> Vec<Outcome> {
    let mut outcomes = Vec::new();

    for found in detect(project_root) {
        if only.is_some_and(|only| only != found.backend) {
            continue;
        }

        let done: io::Result<Outcome> = match found.backend {
            "cmake" => cmake::integrate(&found.path, library, destination, project_root),
            "cubeide" => {
                cubeide::integrate(&found.path, library, destination, project_root, dropped)
            }
            "keil" => keil::integrate(&found.path, library, destination, dropped),
            "iar" => iar::integrate(&found.path, library, destination, dropped),
            _ => makefile::integrate(&found.path, library, destination, dropped),
        };

        // An integration must never take the install down with it. The files are
        // already copied, and the user can finish the job by hand.
        outcomes.push(done.unwrap_or_else(|error| {
            let folder = destination
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();

            Outcome::new(
                found.label,
                Status::Manual,
                format!(
                    "Could not update the project automatically: {}",
                    files::reason(&error)
                ),
            )
            .with_steps(manual_steps(found.backend, &folder, &library.name))
        }));
    }

    outcomes
}

/// Take a library out of every IDE project found, before its files go.
pub fn uninstall(project_root: &Path, library: &Installed) -> Vec<Outcome> {
    let mut outcomes = Vec::new();

    for found in detect(project_root) {
        let done = match found.backend {
            "cmake" => cmake::remove(&found.path, library),
            "cubeide" => cubeide::remove(&found.path, library, project_root),
            "keil" => keil::remove(&found.path, library),
            "iar" => iar::remove(&found.path, library),
            _ => makefile::remove(&found.path, library),
        };

        // As with an install, a project file this cannot edit is left for the user, and
        // the files still go.
        outcomes.push(done.unwrap_or_else(|error| {
            Outcome::new(
                found.label,
                Status::Manual,
                format!(
                    "Could not update the project automatically: {}",
                    files::reason(&error)
                ),
            )
            .with_steps(vec![format!(
                "Take {} out by hand: its files, its include path and its defines.",
                library.name
            )])
        }));
    }

    outcomes
}
