//! The library as an ESP-IDF component: its CMakeLists.txt, and a Kconfig for its settings.
//!
//! ESP-IDF builds a folder of the project's components folder when it holds a CMakeLists.txt that
//! calls idf_component_register(). A library written for any project has none, so it is written
//! here, from libnima.yml, into the folder the library was installed in. It is written again on
//! every install and update, so a source file the new version adds is built without anyone editing
//! anything.
//!
//! A library that ships a CMakeLists.txt of its own knows its build better than this does, and is
//! left alone.
//!
//! The component is named after its folder, as ESP-IDF names every component: components/ee24 is
//! the component ee24, which other components list in their REQUIRES. A folder named like one of
//! ESP-IDF's own components takes its place in the build, which the target warns about before the
//! library goes in.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::COMPONENT_PLACE;
use crate::blocks::{Group, Marker};
use crate::files::{self, backup, unlink};
use crate::manifest::{LIBRARY, MANIFEST_NAME, Manifest};
use crate::platform::kconfig::{cmake_quote, generated, marks, one_line, symbol_of};
use crate::target::{Outcome, Status};
use crate::usercode;

pub const NAME: &str = "ESP-IDF component";
const CMAKELISTS: &str = "CMakeLists.txt";
const KCONFIG: &str = "Kconfig";

/// How far the lines inside idf_component_register() and Kconfig are indented, the way ESP-IDF's
/// own components write them.
const INDENT: &str = "    ";

/// A symbol the Kconfig defines: config NAME, or menuconfig NAME.
static DEFINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:menu)?config[ \t]+([A-Za-z0-9_]+)").expect("a valid pattern")
});

/// Every first line taken as this one's, so a component an installer before wrote is still written
/// again, not refused.
fn our_marks() -> Vec<String> {
    marks("esp32-installer")
}

/// The library's groups of CMake lines, and those of the options taken, in order.
pub fn groups(library: &Manifest) -> Vec<Group> {
    let mut found = library.part.esp32().cmake.clone();

    for option in library.chosen_options() {
        found.extend(option.cmake.iter().cloned());
    }

    found
}

/// The component's name, which is its folder's: ee24 for components/ee24.
pub fn component_name(destination: &Path) -> String {
    destination
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// The name of the USER CODE section in the component's CMakeLists.txt: EXAMPLE_CMAKE.
fn section(library: &Manifest) -> String {
    format!("{}_CMAKE", symbol_of(&library.name))
}

fn label(library: &Manifest) -> String {
    format!("{} {}", library.name, library.version)
        .trim()
        .to_string()
}

/// One keyword of idf_component_register() and its values, a line each.
fn argument(keyword: &str, values: &[String]) -> Vec<String> {
    let mut lines = vec![format!("{INDENT}{keyword}")];

    lines.extend(
        values
            .iter()
            .map(|value| format!("{INDENT}{INDENT}{value}")),
    );

    lines
}

/// The component's CMakeLists.txt: the library's files, and the components it uses.
///
/// `requires` is every component the library needs, ESP-IDF's own and the libraries it depends on,
/// by the names their folders give them.
fn cmakelists(library: &Manifest, requires: &[String]) -> String {
    let sources = library.build_sources();
    let mut lines = vec![
        format!("{} for {}.", crate::MARK, label(library)),
        "#".to_string(),
        "# ESP-IDF builds this folder as a component, since it sits in the project's".to_string(),
        "# components folder or in EXTRA_COMPONENT_DIRS. Written again on every install"
            .to_string(),
        "# and update, so a change made here does not last, except between the USER".to_string(),
        format!("# CODE lines at the end. The files come from the library's {MANIFEST_NAME}."),
        String::new(),
        "idf_component_register(".to_string(),
    ];

    if !sources.is_empty() {
        let quoted: Vec<String> = sources.iter().map(|name| cmake_quote(name)).collect();
        lines.extend(argument("SRCS", &quoted));
    }

    let folders: Vec<String> = library
        .include_dirs()
        .iter()
        .map(|folder| cmake_quote(folder))
        .collect();
    lines.extend(argument("INCLUDE_DIRS", &folders));

    if !requires.is_empty() {
        lines.extend(argument("REQUIRES", requires));
    }

    lines.push(")".to_string());

    // For every component that uses the library, not only its own files: a define can change what the
    // library's header declares, and every file including it has to agree. A component of headers only
    // is an interface library, and takes nothing but INTERFACE.
    if !library.defines.is_empty() {
        let scope = if sources.is_empty() {
            "INTERFACE"
        } else {
            "PUBLIC"
        };

        lines.push(String::new());
        lines.push(format!(
            "target_compile_definitions(${{COMPONENT_LIB}} {scope}"
        ));
        lines.extend(
            library
                .defines
                .iter()
                .map(|define| format!("{INDENT}{define}")),
        );
        lines.push(")".to_string());
    }

    // The groups whose place is the component, each between markers that name it, so a reader finds
    // each again.
    for group in groups(library) {
        if group.place != COMPONENT_PLACE {
            continue;
        }

        let found = Marker::new(&library.name, Some(&group.label));

        lines.push(String::new());
        lines.push(found.open);
        lines.push(format!(
            "# From {}'s {MANIFEST_NAME}, and written again on every install.",
            library.name
        ));

        // {library} in a manifest's lines is the component's own folder.
        lines.extend(
            group
                .lines
                .iter()
                .map(|line| line.replace(LIBRARY, "${COMPONENT_DIR}")),
        );
        lines.push(found.close);
    }

    // The user's own lines, which every install carries over. Empty to start with, so anything in it on
    // an uninstall is the user's.
    let scope = if sources.is_empty() {
        "INTERFACE"
    } else {
        "PRIVATE"
    };

    lines.extend([
        String::new(),
        format!("# Lines of your own for {}, such as", library.name),
        format!("# target_compile_options(${{COMPONENT_LIB}} {scope} -O2), go between these"),
        "# two, and every install keeps them.".to_string(),
        format!("# USER CODE BEGIN {}", section(library)),
        format!("# USER CODE END {}", section(library)),
    ]);

    format!("{}\n", lines.join("\n"))
}

/// The component's Kconfig: a menu of the library's settings in menuconfig.
///
/// None when the library has no settings. ESP-IDF reads a Kconfig file at the top of any component on
/// its own, so nothing else has to name it.
fn kconfig(library: &Manifest) -> Option<String> {
    let settings = &library.part.esp32().settings;

    if settings.is_empty() {
        return None;
    }

    let title = if library.description.is_empty() {
        library.name.clone()
    } else {
        format!("{}: {}", library.name, library.description)
    };

    let mut lines = vec![
        format!("{} for {}. Written again on", crate::MARK, label(library)),
        "# every install and update, so a change made here does not last.".to_string(),
        String::new(),
        format!("menu \"{}\"", one_line(&title)),
    ];

    for setting in settings {
        lines.push(String::new());
        lines.push(format!("{INDENT}config {}", setting.name));
        lines.push(format!(
            "{INDENT}{INDENT}{} \"{}\"",
            setting.kind, setting.prompt
        ));
        lines.push(format!("{INDENT}{INDENT}default {}", setting.default));

        if let Some((low, high)) = setting.range {
            lines.push(format!(
                "{INDENT}{INDENT}range {} {}",
                setting.format(low),
                setting.format(high)
            ));
        }

        if !setting.help.is_empty() {
            lines.push(format!("{INDENT}{INDENT}help"));
            lines.extend(setting.help.iter().map(|line| {
                if line.trim().is_empty() {
                    String::new()
                } else {
                    format!("{INDENT}{INDENT}{INDENT}{line}")
                }
            }));
        }
    }

    lines.push(String::new());
    lines.push("endmenu".to_string());

    Some(format!("{}\n", lines.join("\n")))
}

/// Whether the library brought a CMakeLists.txt of its own.
fn ships_its_own(destination: &Path) -> bool {
    let path = destination.join(CMAKELISTS);

    path.is_file() && !generated(&path, &our_marks())
}

/// Write the component's files, each only when it changed.
pub fn write(library: &Manifest, destination: &Path, requires: &[String]) -> Outcome {
    let name = component_name(destination);

    if ships_its_own(destination) {
        return Outcome::new(
            NAME,
            Status::Already,
            format!(
                "{} has its own CMakeLists.txt, used as it is.",
                library.name
            ),
        );
    }

    let wanted = [
        (CMAKELISTS, Some(cmakelists(library, requires))),
        (KCONFIG, kconfig(library)),
    ];
    let mut written = Vec::new();
    let mut removed = Vec::new();
    let mut steps = Vec::new();
    let mut saved = None;

    let mut each = || -> std::io::Result<()> {
        for (file_name, text) in &wanted {
            let path = destination.join(file_name);

            // A Kconfig an earlier version wrote, for settings this one no longer has. One of the
            // library's own is never touched.
            let Some(text) = text else {
                if path.is_file() && generated(&path, &our_marks()) {
                    fs::remove_file(&path)?;
                    removed.push(*file_name);
                }

                continue;
            };

            let mut data = text.clone().into_bytes();
            let old = if path.is_file() {
                Some(fs::read(&path)?)
            } else {
                None
            };

            // What the user wrote between the USER CODE lines of the last copy goes into the new one.
            // Markers that no longer pair leave nothing to carry, so the copy is put aside first, whole.
            if let Some(old) = old.as_ref().filter(|_| *file_name == CMAKELISTS)
                && let Ok(merged) = usercode::merge(&data, old)
            {
                if merged.broken.is_some() || !merged.lost.is_empty() {
                    let copy = backup(&path)?;
                    let shown = copy
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    steps.push(format!(
                        "Your lines in {file_name} could not be carried over: they are in {shown}."
                    ));
                    saved = Some(copy);
                } else {
                    data = merged.data;
                }

                if !merged.kept.is_empty() {
                    steps.push(format!(
                        "Kept your USER CODE {} in {file_name}.",
                        merged.kept.join(", ")
                    ));
                }
            }

            // Left alone when nothing changed, so the build does not see a newer file and configure
            // again for nothing.
            if old.as_ref() == Some(&data) {
                continue;
            }

            fs::write(&path, &data)?;
            written.push(*file_name);
        }

        Ok(())
    };

    if let Err(error) = each() {
        return Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "Could not write {}: {}",
                destination.display(),
                files::reason(&error)
            ),
        );
    }

    if written.is_empty() && removed.is_empty() {
        return Outcome::new(
            NAME,
            Status::Already,
            format!("The component {name} is up to date."),
        )
        .with_steps(steps);
    }

    let mut done = Vec::new();

    if !written.is_empty() {
        done.push(format!("wrote {}", written.join(", ")));
    }

    if !removed.is_empty() {
        done.push(format!(
            "removed its {}, as {} has no settings now",
            removed.join(", "),
            library.name
        ));
    }

    Outcome::new(
        NAME,
        Status::Changed,
        format!("The component {name}: {}.", done.join("; ")),
    )
    .with_steps(steps)
    .with_backup(saved)
}

/// Whether a CMakeLists.txt write() made has anything between its USER CODE lines.
fn holds_yours(path: &Path) -> bool {
    let Ok(data) = fs::read(path) else {
        return false;
    };

    usercode::parse(&data).map_or(true, |parsed| parsed.holds_something())
}

/// Delete the files write() made. A library's own CMakeLists.txt is not touched.
///
/// Returns the copy put aside of a CMakeLists.txt that had the user's lines in it, or None.
pub fn remove(destination: &Path) -> Option<PathBuf> {
    let mut saved = None;

    for file_name in [CMAKELISTS, KCONFIG] {
        let path = destination.join(file_name);

        if !path.is_file() || !generated(&path, &our_marks()) {
            continue;
        }

        if file_name == CMAKELISTS && holds_yours(&path) {
            saved = backup(&path).ok();
        }

        let _ = unlink(&path);
    }

    saved
}

/// Every symbol the Kconfig write() made defines, read before it is removed.
pub fn symbols(destination: &Path) -> Vec<String> {
    let Ok(text) = files::read(&destination.join(KCONFIG)) else {
        return Vec::new();
    };

    if !our_marks()
        .iter()
        .any(|mark| text.starts_with(mark.as_str()))
    {
        return Vec::new();
    }

    DEFINED
        .captures_iter(&text)
        .map(|found| found[1].to_string())
        .collect()
}
