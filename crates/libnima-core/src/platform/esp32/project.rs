//! The project's own files: CMakeLists.txt, sdkconfig.defaults, and main's CMakeLists.txt.
//!
//! Each is touched only when the library needs it, and each edit is one the user can see and undo:
//!
//! ```text
//! CMakeLists.txt       a block that adds the library's folder to EXTRA_COMPONENT_DIRS, above
//!                      include(project.cmake), only for a library outside the components folder,
//!                      which ESP-IDF finds by itself
//! sdkconfig.defaults   a block with the Kconfig symbols the library needs, only when it needs any
//! main/CMakeLists.txt  the library's name in main's REQUIRES, when main lists its own, or when the
//!                      project builds only what main needs. Otherwise main uses every component of
//!                      the build by itself, this one too
//! ```
//!
//! All of them are the user's, so the rules of every edit here hold: a copy is taken before anything
//! changes, a file this cannot read safely is left alone with the lines to add by hand, a second
//! install adds nothing twice, and the file's own line endings are kept.
//!
//! What the user set in sdkconfig.defaults outside the block is theirs. A symbol they set is not set
//! again in the block, whatever value they gave it, since the line read last would win and quietly
//! undo their choice. And sdkconfig itself, which ESP-IDF writes and reads before sdkconfig.defaults,
//! is never edited: a value it holds already is named, for the user to change in menuconfig.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::component::{component_name, groups};
use super::{AFTER_PROJECT, BEFORE_PROJECT};
use crate::blocks::{self, Group, INSTALLER, Marker};
use crate::files::{self, backup, text_or_empty};
use crate::manifest::{LIBRARY, MANIFEST_NAME, Manifest};
use crate::paths;
use crate::platform::kconfig::{ASSIGNED, NOT_SET, cmake_quote};
use crate::record::installed_libraries;
use crate::target::{Outcome, Status};
use crate::text::{line_ending, lines_of};
use crate::titles::{annotated, grouped};

pub const CMAKELISTS: &str = "CMakeLists.txt";
pub const DEFAULTS: &str = "sdkconfig.defaults";
pub const SDKCONFIG: &str = "sdkconfig";
pub const MAIN: &str = "main/CMakeLists.txt";
pub const COMPONENTS: &str = "components";

/// The line an ESP-IDF project's CMakeLists.txt is built around. CMake commands ignore case, and only
/// a line that starts with it counts, so a commented out one does not.
pub static PROJECT_CMAKE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^[ \t]*include[ \t]*\([^)\n]*tools/cmake/project\.cmake")
        .expect("a valid pattern")
});

/// A project that names its own defaults files, in place of sdkconfig.defaults.
static NAMES_DEFAULTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^[ \t]*set[ \t]*\([ \t]*SDKCONFIG_DEFAULTS\b").expect("a valid pattern")
});

/// A project that builds only the components main needs: MINIMAL_BUILD, which ESP-IDF's own examples
/// set, or a COMPONENTS list. A component nothing names is then left out of the build, however it got
/// into the project.
static MINIMAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?mi)^[ \t]*(?:idf_build_set_property[ \t]*\([ \t]*MINIMAL_BUILD[ \t]+(?:ON|TRUE|YES|1)\b|set[ \t]*\([ \t]*COMPONENTS\b)",
    )
    .expect("a valid pattern")
});

/// A block libNIMA wrote, any library's: its first marker line.
static BLOCK_OPEN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[ \t]*# >>> libnima: (\S+) >>>").expect("a valid pattern"));

/// The arguments of idf_component_register() that take a list. A word that is one of these starts a
/// new list.
const KEYWORDS: [&str; 14] = [
    "SRCS",
    "SRC_DIRS",
    "EXCLUDE_SRCS",
    "INCLUDE_DIRS",
    "PRIV_INCLUDE_DIRS",
    "LDFRAGMENTS",
    "REQUIRES",
    "PRIV_REQUIRES",
    "REQUIRED_IDF_TARGETS",
    "EMBED_FILES",
    "EMBED_TXTFILES",
    "KCONFIG",
    "KCONFIG_PROJBUILD",
    "WHOLE_ARCHIVE",
];

static REGISTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bidf_component_register[ \t]*\(").expect("a valid pattern"));

/// The two lines around a block of this library, or of one of its groups.
fn marker(name: &str, label: Option<&str>) -> Marker {
    Marker::new(name, label)
}

/// The project's CMakeLists.txt, when it is an ESP-IDF project.
pub fn find_project(project_root: &Path) -> Option<PathBuf> {
    let path = project_root.join(CMAKELISTS);

    PROJECT_CMAKE
        .is_match(&text_or_empty(&path))
        .then_some(path)
}

/// A file's text with every block libNIMA wrote taken out, so what is left is the project's own.
fn without_any_block(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;

    while let Some(found) = BLOCK_OPEN.captures_at(text, at) {
        let open = found.get(0).expect("the whole match");
        let close = format!("# <<< libnima: {} <<<", &found[1]);

        let Some(end) = text[open.end()..].find(&close) else {
            break;
        };

        out.push_str(&text[at..open.start()]);

        let mut after = open.end() + end + close.len();

        if let Some(newline) = text[after..].find('\n') {
            after += newline + 1;
        } else {
            after = text.len();
        }

        at = after;
    }

    out.push_str(&text[at..]);

    out
}

/// Whether the project's CMakeLists.txt sets SDKCONFIG_DEFAULTS itself, in a line of its own. A
/// library's block that sets it, as one adding a defaults file of its own does, keeps
/// sdkconfig.defaults in the list, and is not the project's to change, so it is passed by.
pub fn names_its_defaults(project_root: &Path) -> bool {
    NAMES_DEFAULTS.is_match(&without_any_block(&text_or_empty(
        &project_root.join(CMAKELISTS),
    )))
}

/// Whether the project builds only what main needs, so main has to name every component it uses.
fn builds_minimal(project_root: &Path) -> bool {
    MINIMAL.is_match(&text_or_empty(&project_root.join(CMAKELISTS)))
}

/// Whether the library's folder sits right in the components folder, where ESP-IDF looks.
fn in_components(destination: &Path, project_root: &Path) -> bool {
    paths::resolve(destination)
        .parent()
        .is_some_and(|parent| paths::same(parent, &paths::resolve(&project_root.join(COMPONENTS))))
}

fn block_span(text: &str, name: &str) -> Result<Option<(usize, usize)>, blocks::HalfWritten> {
    blocks::span(text, &marker(name, None))
}

/// Every symbol a Kconfig file sets: (symbol, value), n for one it switches off.
fn assigned(text: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();

    for symbol in NOT_SET.captures_iter(text) {
        if !found.iter().any(|(name, _)| name == &symbol[1]) {
            found.push((symbol[1].to_string(), "n".to_string()));
        }
    }

    for symbol in ASSIGNED.captures_iter(text) {
        if !found.iter().any(|(name, _)| name == &symbol[1]) {
            found.push((symbol[1].to_string(), symbol[2].to_string()));
        }
    }

    found
}

/// Every symbol sdkconfig.defaults sets outside this library's block.
pub fn user_defaults(project_root: &Path, name: &str) -> Vec<(String, String)> {
    assigned(&blocks::outside(
        &text_or_empty(&project_root.join(DEFAULTS)),
        &marker(name, None),
    ))
}

/// The symbols this library's own block in sdkconfig.defaults sets, and their values.
pub fn in_block(project_root: &Path, name: &str) -> Vec<(String, String)> {
    let text = text_or_empty(&project_root.join(DEFAULTS));

    match block_span(&text, name) {
        Ok(Some((start, end))) => ASSIGNED
            .captures_iter(&text[start..end])
            .map(|found| (found[1].to_string(), found[2].to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// What sdkconfig, the configuration ESP-IDF builds with, sets.
pub fn configured(project_root: &Path) -> Vec<(String, String)> {
    assigned(&text_or_empty(&project_root.join(SDKCONFIG)))
}

/// Back the file up and write it.
fn write_block(path: &Path, label: &str, text: &str, updated: &str, what: &str) -> Outcome {
    if updated == text {
        return Outcome::new(
            label,
            Status::Already,
            format!("{label} already has {what}."),
        );
    }

    let saved = if path.exists() {
        backup(path).ok()
    } else {
        None
    };

    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| files::write(path, updated));

    if let Err(error) = written {
        return Outcome::new(
            label,
            Status::Manual,
            format!("Could not write {label}: {}", files::reason(&error)),
        )
        .with_backup(saved);
    }

    // An update, when the file had a block of the library's before.
    if text.contains(&format!(">>> {INSTALLER}: {what} >>>"))
        || text.contains(&format!(">>> {INSTALLER}: {what}/"))
    {
        return Outcome::new(
            label,
            Status::Changed,
            format!("Updated {what} in {label}."),
        )
        .with_backup(saved);
    }

    Outcome::new(label, Status::Changed, format!("Added {what} to {label}.")).with_backup(saved)
}

fn half_written(label: &str, name: &str, block: &str) -> Outcome {
    Outcome::new(
        label,
        Status::Manual,
        format!("{label} has a half written libnima block for {name}. Fix it by hand first."),
    )
    .with_steps(lines_of(block).iter().map(ToString::to_string).collect())
}

/// Take this library's blocks out of one file, with the blank line each brought: its own, and in
/// CMakeLists.txt every group of its CMake lines.
fn cut_block(path: &Path, label: &str, name: &str) -> Outcome {
    let Ok(text) = files::read(path) else {
        return Outcome::new(
            label,
            Status::Already,
            format!("{label} has nothing of {name}."),
        );
    };

    let newline = line_ending(&text);
    let mut updated = text.clone();
    let mut markers = vec![marker(name, None)];
    markers.extend(
        blocks::labels(&text, name)
            .iter()
            .map(|group| marker(name, Some(group))),
    );

    for found in &markers {
        match blocks::span(&updated, found) {
            Err(_) => {
                return Outcome::new(
                    label,
                    Status::Manual,
                    format!(
                        "{label} has a half written libnima block for {}. Take it out by hand.",
                        found.name
                    ),
                );
            }
            Ok(Some(span)) => updated = blocks::cut(&updated, span, newline),
            Ok(None) => {}
        }
    }

    if updated == text {
        return Outcome::new(
            label,
            Status::Already,
            format!("{label} has nothing of {name}."),
        );
    }

    let saved = backup(path).ok();

    if let Err(error) = files::write(path, &updated) {
        return Outcome::new(
            label,
            Status::Manual,
            format!("Could not write {label}: {}", files::reason(&error)),
        )
        .with_backup(saved);
    }

    Outcome::new(
        label,
        Status::Changed,
        format!("Removed {name} from {label}."),
    )
    .with_backup(saved)
}

/// A line for each block the user changed inside since it was written.
fn edited(
    project_root: &Path,
    name: &str,
    label: &str,
    markers: &[Marker],
    text: &str,
) -> Vec<String> {
    let installed = installed_libraries(project_root);
    let recorded = installed.get(name).and_then(|entry| entry.get("blocks"));

    markers
        .iter()
        .filter(|found| blocks::edited(recorded, label, found, text))
        .map(|found| {
            format!(
                "Your change inside the {} block is replaced: the block is written again on every install. The file as \
                 it was is in the backup.",
                found.name
            )
        })
        .collect()
}

/// The library's folder as CMake reads it, from the project's folder.
fn folder_path(destination: &Path, project_root: &Path) -> String {
    match paths::relative(destination, project_root) {
        None => cmake_quote(&paths::posix(&paths::resolve(destination))),
        Some(folder) => format!(
            "\"${{CMAKE_CURRENT_SOURCE_DIR}}/{}",
            &cmake_quote(&folder)[1..]
        ),
    }
}

/// The lines this tool owns in the project's CMakeLists.txt.
fn cmake_block(
    library: &Manifest,
    destination: &Path,
    project_root: &Path,
    newline: &str,
) -> String {
    let found = marker(&library.name, None);

    [
        found.open.clone(),
        format!(
            "# Added by libnima, since {} is not in the components folder,",
            library.name
        ),
        "# where ESP-IDF looks by itself. Delete this block to drop it from the build.".to_string(),
        format!(
            "list(APPEND EXTRA_COMPONENT_DIRS {})",
            folder_path(destination, project_root)
        ),
        found.close.clone(),
    ]
    .join(newline)
}

/// Add a library outside the components folder to EXTRA_COMPONENT_DIRS, or take out the block of one
/// that has moved into it, and write its groups of CMake lines whose place is the project's. None when
/// there is nothing to say.
fn integrate_cmake(project_root: &Path, library: &Manifest, destination: &Path) -> Option<Outcome> {
    let name = &library.name;
    let wanted = !in_components(destination, project_root);
    let all_groups = groups(library);
    let ours: Vec<&Group> = all_groups
        .iter()
        .filter(|group| group.place == BEFORE_PROJECT || group.place == AFTER_PROJECT)
        .collect();

    let Some(path) = find_project(project_root) else {
        if !wanted && ours.is_empty() {
            return None;
        }

        let mut steps = vec![
            "Add this to your project's CMakeLists.txt, above include(.../project.cmake):"
                .to_string(),
        ];
        steps.extend(
            lines_of(&cmake_block(library, destination, project_root, "\n"))
                .iter()
                .map(ToString::to_string),
        );

        return Some(
            Outcome::new(
                CMAKELISTS,
                Status::Manual,
                "No ESP-IDF project CMakeLists.txt here, so the library was not added to the build.",
            )
            .with_steps(steps),
        );
    };

    let text = files::read(&path).ok()?;
    let newline = line_ending(&text);
    let span = block_span(&text, name);

    if !wanted && ours.is_empty() {
        // In the components folder now, where ESP-IDF finds it, so a block from when it was elsewhere
        // would only add it a second time. Groups an earlier version had go with it.
        if matches!(span, Ok(None)) && blocks::labels(&text, name).is_empty() {
            return None;
        }

        return Some(cut_block(&path, CMAKELISTS, name));
    }

    let Ok(span) = span else {
        return Some(half_written(
            CMAKELISTS,
            name,
            &cmake_block(library, destination, project_root, newline),
        ));
    };

    let updated = if wanted {
        with_main_block(
            &text,
            span,
            &cmake_block(library, destination, project_root, newline),
            newline,
        )
    } else {
        match span {
            Some(span) => blocks::cut(&text, span, newline),
            None => text.clone(),
        }
    };

    let folder = library_folder(destination, project_root);

    let Ok(updated) = with_groups(&updated, library, &ours, newline, &folder) else {
        let lines: Vec<String> = ours
            .iter()
            .flat_map(|group| group_lines(library, group, &folder))
            .collect();

        return Some(half_written(CMAKELISTS, name, &lines.join("\n")));
    };

    let mut markers: Vec<Marker> = if wanted {
        vec![marker(name, None)]
    } else {
        Vec::new()
    };
    markers.extend(ours.iter().map(|group| marker(name, Some(&group.label))));

    let mut outcome = write_block(&path, CMAKELISTS, &text, &updated, name)
        .with_blocks(CMAKELISTS, blocks::fingerprints(&updated, &markers));
    outcome
        .steps
        .extend(edited(project_root, name, CMAKELISTS, &markers, &text));

    Some(outcome)
}

/// text with the library's EXTRA_COMPONENT_DIRS block above include(project.cmake).
fn with_main_block(text: &str, span: Option<(usize, usize)>, block: &str, newline: &str) -> String {
    let mut rest = text.to_string();

    if let Some((start, end)) = span {
        let found = PROJECT_CMAKE.find(text).map_or(0, |found| found.start());

        // Where it is, when that is above include(project.cmake).
        if start < found {
            return format!("{}{block}{}", &text[..start], &text[end..]);
        }

        // Below it the list does nothing, since ESP-IDF has looked for components by then. It comes
        // out, and goes above.
        let mut end = end;

        if text[end..].starts_with(newline) {
            end += newline.len();
        }

        rest = format!("{}{}", &text[..start], &text[end..]);
    }

    // Right above include(project.cmake), with a blank line between, and nothing added above it, so an
    // uninstall gives the file back as it was.
    let first = PROJECT_CMAKE.find(&rest).map_or(0, |found| found.start());

    format!(
        "{}{block}{newline}{newline}{}",
        &rest[..first],
        &rest[first..]
    )
}

/// The library's folder as a line of the project's CMakeLists.txt names it, for {library} in a
/// manifest's groups: from the project's folder, or in full when it is somewhere else. Unquoted, since
/// the line has its own quotes around it.
fn library_folder(destination: &Path, project_root: &Path) -> String {
    match paths::relative(destination, project_root).as_deref() {
        None => paths::posix(&paths::resolve(destination)),
        Some(".") | Some("") => "${CMAKE_CURRENT_SOURCE_DIR}".to_string(),
        Some(folder) => format!("${{CMAKE_CURRENT_SOURCE_DIR}}/{folder}"),
    }
}

/// A group's block in the project's CMakeLists.txt, as lines, {library} standing for folder.
fn group_lines(library: &Manifest, group: &Group, folder: &str) -> Vec<String> {
    let found = marker(&library.name, Some(&group.label));
    let mut lines = vec![
        found.open,
        format!(
            "# From {}'s {MANIFEST_NAME}, and written again on every install.",
            library.name
        ),
    ];

    lines.extend(group.lines.iter().map(|line| line.replace(LIBRARY, folder)));
    lines.push(found.close);

    lines
}

/// text with the library's groups for the project each in its place, and any group it no longer has
/// taken out.
fn with_groups(
    text: &str,
    library: &Manifest,
    ours: &[&Group],
    newline: &str,
    folder: &str,
) -> Result<String, blocks::HalfWritten> {
    let mut text = text.to_string();

    for label in blocks::labels(&text, &library.name) {
        if !ours.iter().any(|group| group.label == label)
            && let Some(span) = blocks::span(&text, &marker(&library.name, Some(&label)))?
        {
            text = blocks::cut(&text, span, newline);
        }
    }

    for group in ours {
        let lines = group_lines(library, group, folder);
        let content = &lines[1..lines.len() - 1];
        let found = marker(&library.name, Some(&group.label));

        text = if group.place == BEFORE_PROJECT {
            blocks::place(&text, &found, content, newline, Some(&above_project))?
        } else {
            blocks::place(&text, &found, content, newline, None)?
        };
    }

    Ok(text)
}

/// Where include(project.cmake)'s line starts, for a group to go above it.
fn above_project(text: &str) -> Option<usize> {
    PROJECT_CMAKE
        .find(text)
        .map(|found| blocks::line_start(text, found.start()))
}

/// Every line the library asks for in sdkconfig.defaults: (symbol, value, line).
pub fn wanted(library: &Manifest) -> Vec<(String, String, String)> {
    library
        .part
        .esp32()
        .conf
        .iter()
        .map(|entry| {
            (
                entry.item.symbol.clone(),
                entry.item.value.clone(),
                entry.item.line(),
            )
        })
        .collect()
}

/// The lines this tool owns in sdkconfig.defaults.
///
/// A symbol the user sets already is written too, commented out with what they set, so the block shows
/// what the library asked for and why it is not set here.
///
/// Symbols listed in groups are written under their titles, each between two lines of stars, after a
/// blank line below the block's own comment, and a symbol's note above it.
fn defaults_block(library: &Manifest, project_root: &Path, newline: &str) -> String {
    let taken = user_defaults(project_root, &library.name);
    let entries = &library.part.esp32().conf;
    let found = marker(&library.name, None);
    let mut lines = vec![
        found.open.clone(),
        "# Added by libnima, and written again on every install, so set".to_string(),
        format!(
            "# {}'s symbols below this block rather than in it.",
            library.name
        ),
    ];

    if grouped(entries) {
        lines.push(String::new());
    }

    lines.extend(annotated(
        entries,
        |entry| match taken.iter().find(|(name, _)| *name == entry.symbol) {
            Some((symbol, have)) => format!(
                "# {}  (left out: {DEFAULTS} sets CONFIG_{symbol}={have})",
                entry.line()
            ),
            None => entry.line(),
        },
        "#",
        true,
    ));
    lines.push(found.close);

    lines.join(newline)
}

/// Set the symbols the library needs in sdkconfig.defaults, created when it is not there. A library
/// that needs none has no block, and one an earlier version left is taken out.
fn integrate_defaults(project_root: &Path, library: &Manifest) -> Option<Outcome> {
    let path = project_root.join(DEFAULTS);
    let name = &library.name;

    if wanted(library).is_empty() {
        return match block_span(&text_or_empty(&path), name) {
            Ok(None) => None,
            _ => Some(cut_block(&path, DEFAULTS, name)),
        };
    }

    let text = if path.exists() {
        match files::read(&path) {
            Ok(text) => text,
            Err(error) => {
                return Some(
                    Outcome::new(
                        DEFAULTS,
                        Status::Manual,
                        format!("Could not read {DEFAULTS}: {}", files::reason(&error)),
                    )
                    .with_steps(
                        lines_of(&defaults_block(library, project_root, "\n"))
                            .iter()
                            .map(ToString::to_string)
                            .collect(),
                    ),
                );
            }
        }
    } else {
        String::new()
    };

    let newline = line_ending(&text);
    let block = defaults_block(library, project_root, newline);

    let Ok(span) = block_span(&text, name) else {
        return Some(half_written(DEFAULTS, name, &block));
    };

    let updated = match span {
        Some((start, end)) => format!("{}{block}{}", &text[..start], &text[end..]),
        // At the end, after a blank line when the file has anything in it.
        None => {
            let head = text.trim_end_matches(['\r', '\n']);

            if head.is_empty() {
                format!("{block}{newline}")
            } else {
                format!("{head}{newline}{newline}{block}{newline}")
            }
        }
    };

    let markers = [marker(name, None)];
    let mut outcome = write_block(&path, DEFAULTS, &text, &updated, name)
        .with_blocks(DEFAULTS, blocks::fingerprints(&updated, &markers));
    outcome
        .steps
        .extend(edited(project_root, name, DEFAULTS, &markers, &text));
    outcome.steps.extend(stale(library, project_root));

    Some(outcome)
}

/// Sentences for the symbols sdkconfig holds at another value than the library needs. ESP-IDF reads
/// sdkconfig.defaults only for a symbol sdkconfig does not have yet, so sdkconfig's value is the one
/// built with.
fn stale(library: &Manifest, project_root: &Path) -> Vec<String> {
    let have = configured(project_root);
    let taken = user_defaults(project_root, &library.name);

    wanted(library)
        .into_iter()
        .filter_map(|(symbol, value, _)| {
            let set = have.iter().find(|(name, _)| *name == symbol).map(|(_, set)| set)?;

            (*set != value && !taken.iter().any(|(name, _)| *name == symbol)).then(|| {
                format!(
                    "sdkconfig has CONFIG_{symbol}={set}, which is what ESP-IDF builds with. Set it to {value} in idf.py menuconfig."
                )
            })
        })
        .collect()
}

/// One argument of a call, and where it stands.
type Word = (String, usize, usize);

/// The arguments of the call whose opening parenthesis ends at `start`, and where its closing
/// parenthesis is.
///
/// Read the way CMake reads them: a comment runs from # to the end of the line, a quoted argument is
/// one word whatever it holds, and a parenthesis inside an argument, such as one of a generator
/// expression, does not close the call. None for a call that never closes.
fn words_of(text: &str, start: usize) -> Option<(Vec<Word>, usize)> {
    let bytes = text.as_bytes();
    let mut words = Vec::new();
    let mut depth = 0;
    let mut index = start;

    while index < bytes.len() {
        let char = bytes[index];

        if char == b'#' {
            index = text[index..]
                .find('\n')
                .map_or(bytes.len(), |newline| index + newline);
            continue;
        }

        if matches!(char, b' ' | b'\t' | b'\r' | b'\n') {
            index += 1;
            continue;
        }

        if char == b')' && depth == 0 {
            return Some((words, index));
        }

        let first = index;

        if char == b'"' {
            index += 1;

            while index < bytes.len() && bytes[index] != b'"' {
                index += if bytes[index] == b'\\' { 2 } else { 1 };
            }

            index += 1;
        } else {
            while index < bytes.len()
                && !matches!(bytes[index], b' ' | b'\t' | b'\r' | b'\n' | b'#' | b'"')
                && !(bytes[index] == b')' && depth == 0)
            {
                if bytes[index] == b'(' {
                    depth += 1;
                } else if bytes[index] == b')' {
                    depth -= 1;
                }

                index += 1;
            }
        }

        let index_end = index.min(bytes.len());
        words.push((text[first..index_end].to_string(), first, index_end));
    }

    None
}

/// One list of main's: where its keyword stands, and its arguments.
struct MainList {
    keyword: String,
    first: usize,
    last: usize,
    items: Vec<Word>,
}

/// main's REQUIRES and PRIV_REQUIRES, with where the keyword itself is, and where its last argument
/// ends, the place for a list main does not have yet. None when main/CMakeLists.txt has no
/// idf_component_register() it can read.
fn lists_of(text: &str) -> Option<(Vec<MainList>, usize)> {
    let found = REGISTER.find(text)?;
    let (words, _close) = words_of(text, found.end())?;
    let mut lists: Vec<MainList> = Vec::new();
    let mut current: Option<String> = None;

    for (word, first, last) in &words {
        if KEYWORDS.contains(&word.as_str()) {
            current = (word == "REQUIRES" || word == "PRIV_REQUIRES").then(|| word.clone());

            if let Some(keyword) = &current
                && !lists.iter().any(|list| list.keyword == *keyword)
            {
                lists.push(MainList {
                    keyword: keyword.clone(),
                    first: *first,
                    last: *last,
                    items: Vec::new(),
                });
            }

            continue;
        }

        if let Some(keyword) = &current
            && let Some(list) = lists.iter_mut().find(|list| list.keyword == *keyword)
        {
            list.items.push((word.clone(), *first, *last));
        }
    }

    let end = words.last().map_or(found.end(), |(_, _, last)| *last);

    Some((lists, end))
}

/// Add the component to main's REQUIRES, when main lists its own, or when the project builds only what
/// main needs. None when main needs nothing: otherwise main uses every component of the build by itself.
fn integrate_main(project_root: &Path, name: &str) -> Option<Outcome> {
    let path = project_root.join(MAIN);
    let minimal = builds_minimal(project_root);

    if !path.is_file() {
        return None;
    }

    let text = match files::read(&path) {
        Ok(text) => text,
        Err(error) => {
            return Some(
                Outcome::new(
                    MAIN,
                    Status::Manual,
                    format!("Could not read {MAIN}: {}", files::reason(&error)),
                )
                .with_steps(vec![format!(
                    "Add {name} to PRIV_REQUIRES of idf_component_register() in {MAIN}."
                )]),
            );
        }
    };

    let Some((lists, last)) = lists_of(&text) else {
        return Some(
            Outcome::new(
                MAIN,
                Status::Manual,
                format!("{MAIN} has no idf_component_register() this could read."),
            )
            .with_steps(vec![format!(
                "If main lists REQUIRES or PRIV_REQUIRES, add {name} to them."
            )]),
        );
    };

    if lists.is_empty() && !minimal {
        return None;
    }

    if lists.iter().any(|list| {
        list.items
            .iter()
            .any(|(word, _, _)| word.trim_matches('"') == name)
    }) {
        return Some(Outcome::new(
            MAIN,
            Status::Already,
            format!("{MAIN} already requires {name}."),
        ));
    }

    let (updated, why) = if lists.is_empty() {
        // A list of main's own, after its last argument, on the same line.
        (
            format!("{} PRIV_REQUIRES {name}{}", &text[..last], &text[last..]),
            format!(
                "Added PRIV_REQUIRES {name} to {MAIN}, since this project builds only the components main names."
            ),
        )
    } else {
        // Main's own use, so PRIV_REQUIRES when it has one, as ESP-IDF suggests for a component nothing
        // else builds on.
        let list = lists
            .iter()
            .find(|list| list.keyword == "PRIV_REQUIRES")
            .or_else(|| lists.iter().find(|list| list.keyword == "REQUIRES"))
            .expect("not empty");

        (
            format!("{} {name}{}", &text[..list.last], &text[list.last..]),
            format!(
                "Added {name} to {} in {MAIN}, since main lists the components it uses.",
                list.keyword
            ),
        )
    };

    let mut outcome = write_block(&path, MAIN, &text, &updated, name);

    if outcome.status == Status::Changed {
        outcome.message = why;
    }

    Some(outcome)
}

/// The text with one word taken out: its whole line when it stood alone on one.
fn without_word(text: &str, first: usize, last: usize) -> String {
    let start = text[..first].rfind('\n').map_or(0, |index| index + 1);
    let end = text[last..]
        .find('\n')
        .map_or(text.len(), |index| last + index);
    let line_end = if end < text.len() { end + 1 } else { end };

    if text[start..first].trim().is_empty() && text[last..end].trim().is_empty() {
        return format!("{}{}", &text[..start], &text[line_end..]);
    }

    let mut first = first;

    while first > 0 && matches!(text.as_bytes()[first - 1], b' ' | b'\t') {
        first -= 1;
    }

    format!("{}{}", &text[..first], &text[last..])
}

/// Take the component out of main's REQUIRES and PRIV_REQUIRES. A keyword it leaves with nothing after
/// it goes too, so a list this added is taken out whole.
fn unrequire_main(project_root: &Path, name: &str) -> Option<Outcome> {
    let path = project_root.join(MAIN);
    let text = files::read(&path).ok()?;
    let lists = lists_of(&text).map_or_else(Vec::new, |(lists, _)| lists);
    let mut found: Vec<(usize, usize)> = Vec::new();

    for list in &lists {
        let mine: Vec<(usize, usize)> = list
            .items
            .iter()
            .filter(|(word, _, _)| word.trim_matches('"') == name)
            .map(|(_, first, last)| (*first, *last))
            .collect();

        if !mine.is_empty() && mine.len() == list.items.len() {
            found.push((list.first, list.last));
        }

        found.extend(mine);
    }

    if found.is_empty() {
        return None;
    }

    found.sort_unstable();

    let mut updated = text;

    for (first, last) in found.into_iter().rev() {
        updated = without_word(&updated, first, last);
    }

    let saved = backup(&path).ok();

    if files::write(&path, &updated).is_err() {
        return Some(
            Outcome::new(MAIN, Status::Manual, format!("Could not write {MAIN}."))
                .with_steps(vec![format!(
                    "Take {name} out of the components main requires."
                )])
                .with_backup(saved),
        );
    }

    Some(
        Outcome::new(
            MAIN,
            Status::Changed,
            format!("Removed {name} from the components main requires."),
        )
        .with_backup(saved),
    )
}

/// Every edit the library needs.
pub fn integrate(project_root: &Path, library: &Manifest, destination: &Path) -> Vec<Outcome> {
    let mut outcomes: Vec<Option<Outcome>> =
        vec![integrate_cmake(project_root, library, destination)];

    if find_project(project_root).is_some() {
        outcomes.push(integrate_defaults(project_root, library));
        outcomes.push(integrate_main(project_root, &component_name(destination)));
    }

    outcomes.into_iter().flatten().collect()
}

/// Take this library out of the project's files.
pub fn uninstall(project_root: &Path, name: &str, component: Option<&str>) -> Vec<Outcome> {
    let mut outcomes = Vec::new();

    for relative in [CMAKELISTS, DEFAULTS] {
        let path = project_root.join(relative);
        let text = text_or_empty(&path);

        // Its own block, or any group of its CMake lines.
        if path.is_file()
            && (!matches!(block_span(&text, name), Ok(None))
                || !blocks::labels(&text, name).is_empty())
        {
            outcomes.push(cut_block(&path, relative, name));
        }
    }

    if let Some(component) = component {
        outcomes.extend(unrequire_main(project_root, component));
    }

    outcomes
}

/// The lines of sdkconfig.defaults that still set one of symbols, once the block is out.
pub fn leftover(project_root: &Path, symbols: &[String]) -> Vec<String> {
    let text = text_or_empty(&project_root.join(DEFAULTS));

    lines_of(&text)
        .iter()
        .filter(|line| {
            ASSIGNED
                .captures(line)
                .is_some_and(|found| symbols.iter().any(|symbol| symbol == &found[1]))
                || NOT_SET
                    .captures(line)
                    .is_some_and(|found| symbols.iter().any(|symbol| symbol == &found[1]))
        })
        .map(|line| line.trim().to_string())
        .collect()
}
