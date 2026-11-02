//! library.properties: how a library in the libraries folder introduces itself to Arduino.
//!
//! The Arduino IDE and arduino-cli read it for the library's name, version and the boards it runs on,
//! and show it in Library Manager's list of what is installed. A library written for any project has
//! none, so it is written here, from libnima.yml, into the library's folder, and written again on every
//! install and update.
//!
//! A library made for Arduino ships its own, which knows the library better than this does, and is
//! left alone. Its first line tells the two apart: this one starts with a comment, which Arduino skips.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::files;
use crate::manifest::Manifest;
use crate::platform::kconfig::{generated, marks};
use crate::target::{Outcome, Status};

const NAME: &str = "library.properties";

/// Arduino's own categories, the only ones Library Manager takes.
pub const CATEGORIES: [&str; 9] = [
    "Display",
    "Communication",
    "Signal Input/Output",
    "Sensors",
    "Device Control",
    "Timing",
    "Data Storage",
    "Data Processing",
    "Other",
];

static OWNER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)github\.com[/:]([^/\s]+)/").expect("a valid pattern"));

fn our_marks() -> Vec<String> {
    marks("arduino-installer")
}

/// The author: as libnima.yml gives it, or the owner of the repository it comes from.
fn author(library: &Manifest) -> String {
    if let Some(author) = &library.part.arduino().author {
        return author.clone();
    }

    OWNER
        .captures(&library.repository)
        .map_or_else(|| "Unknown".to_string(), |found| found[1].to_string())
}

/// Arduino's category for the library: as given, or the nearest to its own category.
fn category(library: &Manifest) -> String {
    if let Some(category) = &library.part.arduino().category {
        return category.clone();
    }

    match library.category.as_str() {
        "sensor" => "Sensors",
        "storage" => "Data Storage",
        "display" => "Display",
        "communication" | "wireless" => "Communication",
        "timing" => "Timing",
        "power" => "Device Control",
        "input" => "Signal Input/Output",
        "math" => "Data Processing",
        _ => "Other",
    }
    .to_string()
}

/// The header a sketch includes, the way the include folder makes it.
pub fn include_name(library: &Manifest) -> Option<String> {
    let landed = crate::paths::clean_posix(&library.headers.first()?.destination);
    let mut folders = library.include_dirs();
    folders.sort_by_key(|folder| std::cmp::Reverse(folder.len()));

    for folder in folders {
        if folder == "." || folder.is_empty() {
            continue;
        }

        if let Some(rest) = landed.strip_prefix(&format!("{folder}/")) {
            return Some(rest.to_string());
        }
    }

    Some(landed)
}

/// A value for one line of the file: no line breaks.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text of library.properties for this library.
fn properties(library: &Manifest) -> String {
    let name = author(library);
    let part = library.part.arduino();
    let label = format!("{} {}", library.name, library.version);
    let mut lines = vec![
        format!("{} for {}. Written again on", crate::MARK, label.trim()),
        "# every install and update, so a change made here does not last.".to_string(),
        format!("name={}", library.name),
    ];

    if !library.version.is_empty() {
        lines.push(format!("version={}", library.version));
    }

    let sentence = if library.description.is_empty() {
        &library.name
    } else {
        &library.description
    };

    lines.extend([
        format!("author={}", one_line(&name)),
        format!(
            "maintainer={}",
            one_line(part.maintainer.as_ref().unwrap_or(&name))
        ),
        format!("sentence={}", one_line(sentence)),
        "paragraph=".to_string(),
        format!("category={}", category(library)),
        format!("url={}", one_line(&library.repository)),
        format!("architectures={}", part.architectures.join(",")),
    ]);

    if !part.depends.is_empty() {
        lines.push(format!("depends={}", part.depends.join(",")));
    }

    if let Some(header) = include_name(library) {
        lines.push(format!("includes={header}"));
    }

    format!("{}\n", lines.join("\n"))
}

/// Whether the library brought a library.properties of its own.
fn ships_its_own(destination: &Path) -> bool {
    let path = destination.join(NAME);

    path.is_file() && !generated(&path, &our_marks())
}

/// Write library.properties, only when it changed.
pub fn write(library: &Manifest, destination: &Path) -> Outcome {
    let path = destination.join(NAME);
    let name = &library.name;

    if ships_its_own(destination) {
        return Outcome::new(
            NAME,
            Status::Already,
            format!("{name} has its own library.properties, used as it is."),
        );
    }

    let data = properties(library).into_bytes();

    if path.is_file() && fs::read(&path).is_ok_and(|old| old == data) {
        return Outcome::new(
            NAME,
            Status::Already,
            format!("{name}'s library.properties is up to date."),
        );
    }

    if let Err(error) = fs::write(&path, &data) {
        return Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "Could not write {}: {}",
                path.display(),
                files::reason(&error)
            ),
        );
    }

    Outcome::new(
        NAME,
        Status::Changed,
        format!("Wrote {name}'s library.properties, which Arduino knows it by."),
    )
}

/// Delete the library.properties write() made. One of the library's own is not touched.
pub fn remove(destination: &Path) {
    let path = destination.join(NAME);

    if path.is_file() && generated(&path, &our_marks()) {
        let _ = fs::remove_file(path);
    }
}
