//! The application's devicetree overlay: a block of #include lines for the files a library's zephyr.project.overlay
//! names, such as a .dtsi with the library's nodes, which a once file put in the application for the user to edit.
//!
//! ```yaml
//! zephyr:
//!   project:
//!     overlay:
//!       - boards/gled.dtsi             # into the overlay found, or those picked when there are several
//!       - from: boards/gled.dtsi       # into this one, or every one a wildcard matches
//!         to: boards/gtec_ble_nrf54l15_cpuapp.overlay
//! ```
//!
//! ```text
//! // >>> libnima: gled >>>
//! // From gled's libnima.yml, and written again on every install.
//! #include "gled.dtsi"
//! // <<< libnima: gled <<<
//! ```
//!
//! The block is marked with //, since a line of a devicetree file that starts with # is the preprocessor's. Each line
//! names its file from the overlay's own folder, where the preprocessor looks first. A file the overlay includes already,
//! outside the block, or one that is not in the application, is written commented out, with why, as a symbol the user
//! sets is in the block of prj.conf: the build never sees a file twice, or one that is not there.
//!
//! Which overlay: the one a to names, or every one its wildcard matches. For a path alone, the one the application has,
//! app.overlay when it has none, or those the person picks when it has several, asked once for every library of the run.
//! --overlay says it in place of all of that. On an update a path alone stays where it is. An overlay named in full that
//! the application does not have is never made, but for app.overlay and the one --overlay names: a board's overlay made
//! by an install would stop Zephyr reading app.overlay for that board.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::app::{append_block, edited, half_written, write_block};
use crate::blocks::{self, Marker};
use crate::files::{self, backup, text_or_empty};
use crate::manifest::{
    LIBRARY, MANIFEST_NAME, Manifest, ManifestError, in_library, is_pattern, unsafe_path, wildcard,
};
use crate::paths;
use crate::target::{Finding, Outcome, Status};
use crate::text::line_ending;
use crate::titles::{self, Listed, annotated, grouped};
use crate::yaml::{Node, Value, quoted};

/// What makes a line of a devicetree file a comment, and so what the block's markers start with.
const SIGN: &str = "//";

/// What an overlay's name ends with.
pub const SUFFIX: &str = ".overlay";

/// The overlay Zephyr reads for a board that has none of its own: made for an application that has no overlay.
pub const APP_OVERLAY: &str = "app.overlay";

/// How a run names what goes into no overlay, as it names a file it changes.
const NOWHERE: &str = "Devicetree overlay";

/// An #include line of a devicetree file, and the file it names.
static INCLUDE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*#[ \t]*include[ \t]*["<]([^">\r\n]+)[">]"#).expect("a valid pattern")
});

/// An #include anywhere in a line, a commented one too, as the block leaves one out.
static MENTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"#[ \t]*include[ \t]*["<]([^">\r\n]+)[">]"#).expect("a valid pattern")
});

/// One file the overlay includes, from zephyr.project.overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct Include {
    /// The file: from the application's folder, or from the library's when in_library.
    pub file: String,
    pub in_library: bool,
    /// The overlay it goes into, a path or a wildcard from the application's folder. None to look for one.
    pub to: Option<String>,
}

impl Include {
    /// The file, as the manifest writes it.
    pub fn written(&self) -> String {
        if self.in_library {
            format!("{LIBRARY}/{}", self.file)
        } else {
            self.file.clone()
        }
    }
}

/// Where one of a library's includes goes, once that is settled.
#[derive(Debug, Clone)]
pub struct Placed {
    pub entry: Listed<Include>,
    /// The overlays it goes into.
    pub into: Vec<String>,
    /// Why it goes into none, when it does not.
    pub why: Option<String>,
}

/// The two lines around a library's block in an overlay.
fn marker(name: &str) -> Marker {
    Marker::signed(name, None, SIGN)
}

// ---------------------------------------------------------------------------
// The manifest
// ---------------------------------------------------------------------------

/// The entries of zephyr.project.overlay, or of an option's, under `key`: each the file alone, or a from and a to, in
/// groups under their titles when it likes, and the file alone with its note.
pub fn read(value: Option<&Node>, key: &str) -> Result<Vec<Listed<Include>>, ManifestError> {
    let found = titles::listed_or_whole(
        value,
        key,
        true,
        |map| map.has("from") || map.has("to"),
        |node| include(node, key).map(Some),
    )?;

    for (index, entry) in found.iter().enumerate() {
        if found[..index]
            .iter()
            .any(|before| before.item == entry.item)
        {
            return Err(ManifestError::new(format!(
                "{key} has {} more than once{}.",
                entry.item.written(),
                entry
                    .item
                    .to
                    .as_ref()
                    .map(|to| format!(", into {to}"))
                    .unwrap_or_default()
            )));
        }
    }

    Ok(found)
}

/// One entry: the file to include, from the application's folder or {library}/... in the library's, and the overlay a
/// to names.
fn include(node: &Node, key: &str) -> Result<Include, ManifestError> {
    let shape = || {
        ManifestError::at(
            node.line,
            format!(
                "{key} has {}. An entry is the file to include, or a from and a to: the file, and the overlay it goes \
                 into, such as from: boards/example.dtsi and to: boards/example.overlay.",
                node.shown()
            ),
        )
    };

    let (given, to) = match &node.value {
        Value::Map(map) => {
            if map.keys().any(|name| name != "from" && name != "to") {
                return Err(shape());
            }

            let from = map.given("from").and_then(Node::as_str).ok_or_else(shape)?;
            let to = map.given("to").and_then(Node::as_str).ok_or_else(shape)?;

            (from.trim().to_string(), Some(to.trim().to_string()))
        }
        _ => (node.as_str().ok_or_else(shape)?.trim().to_string(), None),
    };

    let (library_file, rest) = in_library(&given);
    let place = if library_file {
        "the library's folder"
    } else {
        "the application's folder"
    };

    if let Some(why) = unsafe_path(&rest) {
        return Err(ManifestError::at(
            node.line,
            format!("{key} has {}, outside {place}: {why}.", quoted(&given)),
        ));
    }

    if is_pattern(&rest) {
        return Err(ManifestError::at(
            node.line,
            format!(
                "{key} has {}, a wildcard. An #include names one file: give each in full.",
                quoted(&given)
            ),
        ));
    }

    let to = match to {
        None => None,
        Some(to) => Some(overlay_path(&to).map_err(|why| {
            ManifestError::at(
                node.line,
                format!("{key} puts {} into {}: {why}", quoted(&given), quoted(&to)),
            )
        })?),
    };

    Ok(Include {
        file: paths::clean_posix(&rest),
        in_library: library_file,
        to,
    })
}

/// An overlay, or a wildcard of overlays, as a to or --overlay gives it, from the application's folder: cleaned, or why
/// it is none.
pub fn overlay_path(text: &str) -> Result<String, String> {
    let text = text.trim().replace('\\', "/");

    if in_library(&text).0 {
        return Err(format!(
            "the overlay is the application's, so it has no {LIBRARY}. Give its path from the application's folder, such \
             as boards/example.overlay."
        ));
    }

    if let Some(why) = unsafe_path(&text) {
        return Err(format!("it is outside the application's folder: {why}."));
    }

    let cleaned = paths::clean_posix(&text);

    if paths::suffix(&cleaned) != SUFFIX {
        return Err(
            "that is no overlay. Give one, such as boards/example.overlay, or a wildcard of them, such as \
             boards/*.overlay."
                .to_string(),
        );
    }

    Ok(cleaned)
}

/// The library's includes, with those of the options taken, in the file's order.
pub fn entries(library: &Manifest) -> Vec<Listed<Include>> {
    let part = library.part.zephyr();
    let mut found = part.overlay.clone();

    for option in library.chosen_options() {
        if let Some((_, own)) = part
            .option_overlay
            .iter()
            .find(|(name, _)| *name == option.name)
        {
            found.extend(own.iter().cloned());
        }
    }

    found
}

// ---------------------------------------------------------------------------
// The application's overlays
// ---------------------------------------------------------------------------

/// The overlays in one folder of the application, as paths from its folder.
fn overlays_in(project_root: &Path, folder: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(project_root.join(folder)) else {
        return Vec::new();
    };

    let mut found: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| paths::suffix(name) == SUFFIX)
        .map(|name| {
            if folder.is_empty() {
                name
            } else {
                format!("{folder}/{name}")
            }
        })
        .collect();

    found.sort_by_key(|path| paths::sort_key(path));
    found
}

/// The overlays a file alone can go into: those beside CMakeLists.txt, app.overlay first, then those of boards/ and
/// socs/, which Zephyr reads by the name of the board and of the SoC. Never those of sysbuild/, which are the other
/// images'.
pub fn offered(project_root: &Path) -> Vec<String> {
    let mut found = overlays_in(project_root, "");

    if let Some(index) = found.iter().position(|name| name == APP_OVERLAY) {
        let first = found.remove(index);
        found.insert(0, first);
    }

    found.extend(overlays_in(project_root, "boards"));
    found.extend(overlays_in(project_root, "socs"));

    found
}

/// Every overlay a library's block can be in, and a wildcard can match: beside CMakeLists.txt, and one folder down, but
/// not in a build folder or a hidden one.
fn every(project_root: &Path) -> Vec<String> {
    let mut found = overlays_in(project_root, "");

    if let Ok(entries) = fs::read_dir(project_root) {
        let mut folders: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| !name.starts_with("build") && !name.starts_with('.'))
            .collect();

        folders.sort_by_key(|name| paths::sort_key(name));

        for folder in folders {
            found.extend(overlays_in(project_root, &folder));
        }
    }

    found
}

/// The overlays a to, or --overlay, names: the one named in full, when the application has it, or every one a wildcard
/// matches, those of sysbuild/ only for a wildcard that starts there.
pub fn named(project_root: &Path, given: &str) -> Vec<String> {
    if !is_pattern(given) {
        return if project_root.join(given).is_file() {
            vec![given.to_string()]
        } else {
            Vec::new()
        };
    }

    let Some(pattern) = wildcard(given) else {
        return Vec::new();
    };

    every(project_root)
        .into_iter()
        .filter(|path| !path.starts_with("sysbuild/") || given.starts_with("sysbuild/"))
        .filter(|path| pattern.is_match(path))
        .collect()
}

/// The overlays that have the library's block, a half written one too.
pub fn holding(project_root: &Path, name: &str) -> Vec<String> {
    every(project_root)
        .into_iter()
        .filter(|overlay| {
            !matches!(
                blocks::span(&text_or_empty(&project_root.join(overlay)), &marker(name)),
                Ok(None)
            )
        })
        .collect()
}

/// Whether the library's block in an overlay includes a file of one of these names, written or left out.
pub fn includes_any(project_root: &Path, overlay: &str, name: &str, leaves: &[String]) -> bool {
    let text = text_or_empty(&project_root.join(overlay));

    let Ok(Some(span)) = blocks::span(&text, &marker(name)) else {
        return false;
    };

    MENTION
        .captures_iter(blocks::body(&text, span))
        .any(|found| leaves.iter().any(|leaf| paths::leaf(&found[1]) == leaf))
}

// ---------------------------------------------------------------------------
// The block
// ---------------------------------------------------------------------------

/// Where an include's file is.
fn file_of(include: &Include, project_root: &Path, destination: &Path) -> PathBuf {
    if include.in_library {
        destination.join(&include.file)
    } else {
        project_root.join(&include.file)
    }
}

/// The file as an #include line of the overlay names it: from the overlay's own folder.
fn include_path(project_root: &Path, overlay: &str, file: &Path) -> String {
    let at = project_root.join(overlay);
    let folder = at.parent().unwrap_or(project_root);

    paths::relative(file, folder).unwrap_or_else(|| paths::posix(&paths::resolve(file)))
}

/// Whether the overlay, outside the library's block, includes the file this path names.
fn included_outside(outside: &str, path: &str) -> bool {
    let wanted = paths::clean_posix(path);

    INCLUDE
        .captures_iter(outside)
        .any(|found| paths::clean_posix(&found[1]) == wanted)
}

/// The library's block in one overlay, and a step for each line it leaves out.
fn block_of(
    library: &Manifest,
    places: (&Path, &Path),
    overlay: &str,
    entries: &[Listed<Include>],
    outside: &str,
    newline: &str,
) -> (String, Vec<String>) {
    let (project_root, destination) = places;
    let found = marker(&library.name);
    let mut steps = Vec::new();
    let mut lines = vec![
        found.open.clone(),
        format!(
            "{SIGN} From {}'s {MANIFEST_NAME}, and written again on every install.",
            library.name
        ),
    ];

    if grouped(entries) {
        lines.push(String::new());
    }

    lines.extend(annotated(
        entries,
        |include| {
            let file = file_of(include, project_root, destination);
            let path = include_path(project_root, overlay, &file);
            let line = format!("#include \"{path}\"");
            let shown = paths::shown(&file, project_root);

            if included_outside(outside, &path) {
                steps.push(format!(
                    "{line} is left commented: {overlay} includes {shown} already, and that line is the one used."
                ));
                format!("{SIGN} {line}  (left out: {overlay} includes it already)")
            } else if !file.is_file() {
                steps.push(format!(
                    "{line} is left commented: {shown} is not in the application. Put it there, then install again."
                ));
                format!("{SIGN} {line}  (left out: {shown} is not there)")
            } else {
                line
            }
        },
        SIGN,
        true,
    ));
    lines.push(found.close);

    (lines.join(newline), steps)
}

/// Write the library's block into one overlay: where it is, or at its end. A missing overlay is made only when `make`
/// lets it be.
fn write_into(
    places: (&Path, &Path),
    library: &Manifest,
    overlay: &str,
    entries: &[Listed<Include>],
    make: bool,
) -> Outcome {
    let (project_root, _) = places;
    let path = project_root.join(overlay);
    let name = &library.name;

    let text = if path.is_file() {
        match files::read(&path) {
            Ok(text) => text,
            Err(error) => {
                return Outcome::new(
                    overlay,
                    Status::Manual,
                    format!("Could not read {overlay}: {}", files::reason(&error)),
                );
            }
        }
    } else if make {
        String::new()
    } else {
        return Outcome::new(
            overlay,
            Status::Manual,
            format!(
                "{overlay} is not in the application, so {name}'s #include lines did not go in."
            ),
        );
    };

    let newline = line_ending(&text);
    let found = marker(name);
    let outside = blocks::outside(&text, &found);
    let (block, steps) = block_of(library, places, overlay, entries, &outside, newline);

    let Ok(span) = blocks::span(&text, &found) else {
        return half_written(overlay, name, &block);
    };

    let updated = match span {
        Some((start, end)) => format!("{}{block}{}", &text[..start], &text[end..]),
        None => append_block(&text, &block, newline),
    };

    let markers = [found];
    let mut outcome = write_block(&path, overlay, &text, &updated, name)
        .with_blocks(overlay, blocks::fingerprints(&updated, &markers));
    outcome
        .steps
        .extend(edited(project_root, name, overlay, &markers, &text));
    outcome.steps.extend(steps);

    outcome
}

/// Take the library's block out of one overlay, with the blank line it brought.
fn cut(project_root: &Path, overlay: &str, name: &str) -> Outcome {
    let path = project_root.join(overlay);
    let nothing = || {
        Outcome::new(
            overlay,
            Status::Already,
            format!("{overlay} has nothing of {name}."),
        )
    };

    let Ok(text) = files::read(&path) else {
        return nothing();
    };

    let updated = match blocks::span(&text, &marker(name)) {
        Err(_) => {
            return Outcome::new(
                overlay,
                Status::Manual,
                format!(
                    "{overlay} has a half written libnima block for {name}. Take it out by hand."
                ),
            );
        }
        Ok(None) => return nothing(),
        Ok(Some(span)) => blocks::cut(&text, span, line_ending(&text)),
    };

    let saved = backup(&path).ok();

    if let Err(error) = files::write(&path, &updated) {
        return Outcome::new(
            overlay,
            Status::Manual,
            format!("Could not write {overlay}: {}", files::reason(&error)),
        )
        .with_backup(saved);
    }

    Outcome::new(
        overlay,
        Status::Changed,
        format!("Removed {name} from {overlay}."),
    )
    .with_backup(saved)
}

// ---------------------------------------------------------------------------
// What the target does with them
// ---------------------------------------------------------------------------

/// What a check says of each include: the overlay it goes into, or why it goes into none.
pub fn findings(project_root: &Path, name: &str, plan: &[Placed]) -> Vec<Finding> {
    let mut found = Vec::new();

    for placed in plan {
        let written = placed.entry.item.written();

        if placed.into.is_empty() {
            found.push(Finding::warn(
                format!(
                    "{written} goes into no overlay: {}",
                    placed.why.clone().unwrap_or_default()
                ),
                "Choose the overlay with --overlay, or add its #include line to it by hand.",
            ));
            continue;
        }

        let leaf = paths::leaf(&placed.entry.item.file).to_string();

        for overlay in &placed.into {
            if includes_any(project_root, overlay, name, std::slice::from_ref(&leaf)) {
                found.push(Finding::ok(format!("{overlay} includes {written}")));
            } else {
                found.push(Finding::info(
                    format!("An #include of {written} goes into {overlay}"),
                    "",
                ));
            }
        }
    }

    found
}

/// The library's blocks in the application's overlays, each where `plan` puts its lines: written into each overlay that
/// gets one of them, made when `made` lets it be, and taken out of each one that had the block and gets none now. What
/// goes into no overlay is left for the user to add, with the lines.
pub fn integrate(
    project_root: &Path,
    library: &Manifest,
    destination: &Path,
    plan: &[Placed],
    made: &[String],
) -> Vec<Outcome> {
    let name = &library.name;
    let mut outcomes = Vec::new();
    let mut targets: Vec<&String> = Vec::new();

    for overlay in plan.iter().flat_map(|placed| &placed.into) {
        if !targets.contains(&overlay) {
            targets.push(overlay);
        }
    }

    for overlay in holding(project_root, name) {
        if !targets.iter().any(|target| **target == overlay) {
            outcomes.push(cut(project_root, &overlay, name));
        }
    }

    for overlay in &targets {
        let entries: Vec<Listed<Include>> = plan
            .iter()
            .filter(|placed| placed.into.contains(overlay))
            .map(|placed| placed.entry.clone())
            .collect();

        outcomes.push(write_into(
            (project_root, destination),
            library,
            overlay,
            &entries,
            made.contains(overlay),
        ));
    }

    let nowhere: Vec<&Placed> = plan
        .iter()
        .filter(|placed| placed.into.is_empty())
        .collect();

    if !nowhere.is_empty() {
        let mut steps = vec![
            "Add these to the overlay each belongs in, from its folder, or install again with --overlay:".to_string(),
        ];

        steps.extend(nowhere.iter().map(|placed| {
            let file = file_of(&placed.entry.item, project_root, destination);

            format!(
                "#include \"{}\"  ({})",
                paths::shown(&file, project_root),
                placed.why.clone().unwrap_or_default()
            )
        }));

        outcomes.push(
            Outcome::new(
                NOWHERE,
                Status::Manual,
                format!("Some of {name}'s devicetree goes into no overlay, so its #include lines are yours to add."),
            )
            .with_steps(steps),
        );
    }

    outcomes
}

/// Take the library's block out of every overlay that has it. The overlays stay, one an install made too: the user may
/// have put more in it since.
pub fn uninstall(project_root: &Path, name: &str) -> Vec<Outcome> {
    holding(project_root, name)
        .iter()
        .map(|overlay| cut(project_root, overlay, name))
        .collect()
}
