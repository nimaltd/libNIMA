//! The application's own files: CMakeLists.txt, a Kconfig file such as prj.conf, and
//! sysbuild.conf.
//!
//! Short blocks are added, each between marker lines so it can be found and written again on
//! the next install:
//!
//! ```text
//! CMakeLists.txt  adds the library's folder to ZEPHYR_EXTRA_MODULES, before
//!                 find_package(Zephyr), which is when Zephyr reads it, and the library's
//!                 Kconfig fragment to EXTRA_CONF_FILE. And any group of the library's CMake
//!                 lines whose place is the application's: above find_package(Zephyr), or
//!                 at the end
//! prj.conf        sets the library's symbol, and the ones it needs. Any other Kconfig file of
//!                 the application can take it instead, a board's own in boards/ for one, as
//!                 the user chooses
//! sysbuild.conf   the library's settings of sysbuild, MCUboot's for one, when it has any.
//!                 Never the application's Kconfig file
//! ```
//!
//! The application's devicetree overlay, where a library's #include lines go, is in overlay.rs.
//!
//! These files are the user's, so the rules of every edit here hold: a copy is taken before
//! anything changes, a file this cannot read safely is left alone with the lines to add by
//! hand, a second install adds nothing twice, and the file's own line endings are kept.
//!
//! What the user set in a Kconfig file outside the block is theirs. A symbol they set is not
//! set again in the block, whatever value they gave it, since the line read last would win
//! and quietly undo their choice.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::module::{fragment_name, groups};
use super::{AFTER_ZEPHYR, BEFORE_ZEPHYR, CONF_KEY};
use crate::blocks::{self, Group, INSTALLER, Marker, Place};
use crate::files::{self, backup, text_or_empty};
use crate::manifest::{LIBRARY, MANIFEST_NAME, Manifest};
use crate::paths;
use crate::platform::kconfig::{ASSIGNED, NOT_SET, cmake_quote};
use crate::record::installed_libraries;
use crate::target::{Outcome, Status};
use crate::text::{line_ending, lines_of};
use crate::titles::{annotated, titled};

pub const CMAKELISTS: &str = "CMakeLists.txt";
pub const PRJ_CONF: &str = "prj.conf";
pub const SYSBUILD_CONF: &str = "sysbuild.conf";

/// The label of the block that adds the library's Kconfig fragment to EXTRA_CONF_FILE: what
/// it adds, Zephyr's Kconfig fragment of the library's. A group's label has no dot, so no group
/// takes it, and blocks::labels(), which finds the groups, passes it by.
const FRAGMENT_LABEL: &str = "zephyr.conf";

/// The line Zephyr's build starts at. CMake commands ignore case, and only a line that starts
/// with it counts, so a commented out one does not.
pub static FIND_ZEPHYR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?mi)^[ \t]*find_package[ \t]*\([ \t]*Zephyr\b").expect("a valid pattern")
});

/// The same two forms as in a Kconfig file in sysbuild.conf, where every symbol starts SB_CONFIG_.
static SB_ASSIGNED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*SB_CONFIG_([A-Za-z0-9_]+)[ \t]*=[ \t]*(.*?)[ \t\r]*$")
        .expect("a valid pattern")
});
static SB_NOT_SET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*#[ \t]*SB_CONFIG_([A-Za-z0-9_]+) is not set[ \t\r]*$")
        .expect("a valid pattern")
});

/// The two lines around a block of this library, or of one of its groups.
fn marker(name: &str, label: Option<&str>) -> Marker {
    Marker::new(name, label)
}

/// The application's CMakeLists.txt, when it is a Zephyr application.
pub fn find_cmakelists(project_root: &Path) -> Option<PathBuf> {
    let path = project_root.join(CMAKELISTS);

    FIND_ZEPHYR.is_match(&text_or_empty(&path)).then_some(path)
}

fn conf_names_in(folder: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(folder) else {
        return Vec::new();
    };

    let mut found: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".conf"))
        .collect();

    found.sort();
    found
}

/// The application's Kconfig files, as paths from its folder: prj.conf first, then any other
/// .conf beside it, then the boards' own in boards/.
pub fn conf_files(project_root: &Path) -> Vec<String> {
    // sysbuild.conf is sysbuild's, not the application's, and so is a sysbuild_<suffix>.conf
    // of a build with a file suffix: a library is never switched on there, so they are never
    // offered.
    let mut found: Vec<String> = conf_names_in(project_root)
        .into_iter()
        .filter(|name| name != SYSBUILD_CONF && !name.starts_with("sysbuild_"))
        .collect();

    found.extend(
        conf_names_in(&project_root.join("boards"))
            .into_iter()
            .map(|name| format!("boards/{name}")),
    );

    if let Some(index) = found.iter().position(|name| name == PRJ_CONF) {
        let first = found.remove(index);
        found.insert(0, first);
    }

    found
}

/// The Kconfig files a library's block can be in: one folder deeper than the files offered, so
/// one the user gave with --conf, such as conf/debug.conf, is found again on an update. Not in a
/// build folder, and not in a hidden one. Not in sysbuild.conf either, nor in sysbuild/, which
/// holds the settings of the other images.
pub fn kconfig_files(project_root: &Path) -> Vec<String> {
    let mut found: Vec<String> = conf_names_in(project_root);

    if let Ok(entries) = fs::read_dir(project_root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();

            if entry.path().is_dir()
                && !name.starts_with("build")
                && !name.starts_with('.')
                && !name.starts_with("sysbuild")
            {
                found.extend(
                    conf_names_in(&entry.path())
                        .into_iter()
                        .map(|conf| format!("{name}/{conf}")),
                );
            }
        }
    }

    found.retain(|name| {
        !name.starts_with("build") && !name.starts_with('.') && !name.starts_with("sysbuild")
    });
    found.sort();

    found
}

/// Where this library's block is, or None when it has none.
fn block_span(text: &str, name: &str) -> Result<Option<(usize, usize)>, blocks::HalfWritten> {
    blocks::span(text, &marker(name, None))
}

/// The Kconfig file that already has this library's block, or None.
pub fn holding(project_root: &Path, name: &str) -> Option<String> {
    kconfig_files(project_root).into_iter().find(|relative| {
        !matches!(
            block_span(&text_or_empty(&project_root.join(relative)), name),
            Ok(None)
        )
    })
}

/// The symbols this library's own block in conf sets, and their values.
pub fn in_block(project_root: &Path, name: &str, conf: &str) -> Vec<(String, String)> {
    let text = text_or_empty(&project_root.join(conf));

    match block_span(&text, name) {
        Ok(Some((start, end))) => ASSIGNED
            .captures_iter(&text[start..end])
            .map(|found| (found[1].to_string(), found[2].to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every symbol set outside this library's block, (symbol, value, file), in the Kconfig file
/// conf and in prj.conf, which Zephyr reads with a board's own file.
pub fn assigned(project_root: &Path, name: &str, conf: &str) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = Vec::new();
    let first = conf.replace('\\', "/");
    let mut files = vec![first.as_str()];

    if first != PRJ_CONF {
        files.push(PRJ_CONF);
    }

    for relative in files {
        let mut text = text_or_empty(&project_root.join(relative));

        if let Ok(Some((start, end))) = block_span(&text, name) {
            text.replace_range(start..end, "");
        }

        for symbol in NOT_SET.captures_iter(&text) {
            if !found.iter().any(|(name, _, _)| name == &symbol[1]) {
                found.push((symbol[1].to_string(), "n".to_string(), relative.to_string()));
            }
        }

        for symbol in ASSIGNED.captures_iter(&text) {
            if !found.iter().any(|(name, _, _)| name == &symbol[1]) {
                found.push((
                    symbol[1].to_string(),
                    symbol[2].to_string(),
                    relative.to_string(),
                ));
            }
        }
    }

    found
}

/// The library's folder as CMake reads it, from the application's folder.
fn module_path(destination: &Path, project_root: &Path) -> String {
    match paths::relative(destination, project_root) {
        None => cmake_quote(&paths::posix(&paths::resolve(destination))),
        Some(folder) => format!(
            "\"${{CMAKE_CURRENT_SOURCE_DIR}}/{}",
            &cmake_quote(&folder)[1..]
        ),
    }
}

/// Back the file up and write it.
pub(super) fn write_block(
    path: &Path,
    label: &str,
    text: &str,
    updated: &str,
    what: &str,
) -> Outcome {
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

pub(super) fn half_written(label: &str, name: &str, block: &str) -> Outcome {
    Outcome::new(
        label,
        Status::Manual,
        format!("{label} has a half written libnima block for {name}. Fix it by hand first."),
    )
    .with_steps(lines_of(block).iter().map(ToString::to_string).collect())
}

/// The lines this tool owns in the application's CMakeLists.txt.
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
            "# Added by libnima. Delete this block, and {}'s block in",
            library.name
        ),
        "# the Kconfig file, to drop the library from the build.".to_string(),
        format!(
            "list(APPEND ZEPHYR_EXTRA_MODULES {})",
            module_path(destination, project_root)
        ),
        found.close.clone(),
    ]
    .join(newline)
}

/// Add the library's folder to ZEPHYR_EXTRA_MODULES and its Kconfig fragment to EXTRA_CONF_FILE,
/// and write its groups of CMake lines whose place is the application's.
fn integrate_cmake(project_root: &Path, library: &Manifest, destination: &Path) -> Outcome {
    let name = &library.name;
    let all_groups = groups(library);
    let ours: Vec<&Group> = all_groups
        .iter()
        .filter(|group| group.place == BEFORE_ZEPHYR || group.place == AFTER_ZEPHYR)
        .collect();

    let Some(path) = find_cmakelists(project_root) else {
        let mut steps = vec![
            "Add this to your application's CMakeLists.txt, above find_package(Zephyr):"
                .to_string(),
        ];
        steps.extend(
            lines_of(&cmake_block(library, destination, project_root, "\n"))
                .iter()
                .map(ToString::to_string),
        );
        steps.extend(fragment_lines(library, destination, project_root));

        return Outcome::new(
            CMAKELISTS,
            Status::Manual,
            "No CMakeLists.txt with find_package(Zephyr) here, so the library was not added to the build.",
        )
        .with_steps(steps);
    };

    let text = match files::read(&path) {
        Ok(text) => text,
        Err(error) => {
            return Outcome::new(
                CMAKELISTS,
                Status::Manual,
                format!("Could not read {CMAKELISTS}: {}", files::reason(&error)),
            );
        }
    };

    let newline = line_ending(&text);
    let block = cmake_block(library, destination, project_root, newline);

    let Ok(span) = block_span(&text, name) else {
        return half_written(CMAKELISTS, name, &block);
    };

    let mut updated = None;
    let mut rest = text.clone();

    if let Some((start, end)) = span {
        let found = FIND_ZEPHYR.find(&text).map_or(0, |found| found.start());

        // Where it is, when that is above find_package(Zephyr).
        if start < found {
            updated = Some(format!("{}{block}{}", &text[..start], &text[end..]));
        } else {
            // Below it the list does nothing, since Zephyr has looked for modules by then. It
            // comes out, with the line it stood on, and goes above.
            let mut end = end;

            if text[end..].starts_with(newline) {
                end += newline.len();
            }

            rest = format!("{}{}", &text[..start], &text[end..]);
        }
    }

    let updated = updated.unwrap_or_else(|| {
        // Right above find_package(Zephyr), with a blank line between, and nothing added above
        // it, so an uninstall can give the file back as it was.
        let first = FIND_ZEPHYR.find(&rest).map_or(0, |found| found.start());

        format!(
            "{}{block}{newline}{newline}{}",
            &rest[..first],
            &rest[first..]
        )
    });

    // The fragment's block before the groups, so a group new above find_package(Zephyr) lands
    // below it. The blocks already above it stay where they are.
    let fragment = fragment_lines(library, destination, project_root);

    let Ok(updated) = with_fragment(&updated, library, &fragment, newline) else {
        return half_written(CMAKELISTS, name, &fragment.join("\n"));
    };

    let folder = library_folder(destination, project_root);

    let Ok(updated) = with_groups(&updated, library, &ours, newline, &folder) else {
        let lines: Vec<String> = ours
            .iter()
            .flat_map(|group| group_lines(library, group, &folder))
            .collect();

        return half_written(CMAKELISTS, name, &lines.join("\n"));
    };

    let mut markers = vec![marker(name, None), marker(name, Some(FRAGMENT_LABEL))];
    markers.extend(ours.iter().map(|group| marker(name, Some(&group.label))));

    let mut outcome = write_block(&path, CMAKELISTS, &text, &updated, name)
        .with_blocks(CMAKELISTS, blocks::fingerprints(&updated, &markers));
    outcome
        .steps
        .extend(edited(project_root, name, CMAKELISTS, &markers, &text));

    outcome
}

/// The block that adds the library's Kconfig fragment, zephyr/<name>.conf in its folder, to
/// EXTRA_CONF_FILE, as lines, or nothing for a library with no zephyr.library.conf. The file is
/// named from the application's folder, as {library} names the library's folder in a group's
/// lines, and quoted.
///
/// Read with zephyr_get(... MERGE REVERSE), EXTRA_CONF_FILE puts what the application's
/// CMakeLists.txt adds first: the fragment is read after prj.conf, and before a file given with
/// -DEXTRA_CONF_FILE, such as a debug build's, which can still change a line of it.
fn fragment_lines(library: &Manifest, destination: &Path, project_root: &Path) -> Vec<String> {
    if library.part.zephyr().conf.is_empty() {
        return Vec::new();
    }

    let found = marker(&library.name, Some(FRAGMENT_LABEL));
    let path = module_path(
        &destination
            .join("zephyr")
            .join(fragment_name(&library.name)),
        project_root,
    );

    vec![
        found.open,
        format!(
            "# {}'s Kconfig settings, from {CONF_KEY} in its {MANIFEST_NAME},",
            library.name
        ),
        "# read after prj.conf. Written again on every install.".to_string(),
        format!("list(APPEND EXTRA_CONF_FILE {path})"),
        found.close,
    ]
}

/// text with the block of the library's Kconfig fragment above find_package(Zephyr), or without
/// it once the library has none.
fn with_fragment(
    text: &str,
    library: &Manifest,
    lines: &[String],
    newline: &str,
) -> Result<String, blocks::HalfWritten> {
    let found = marker(&library.name, Some(FRAGMENT_LABEL));

    if !lines.is_empty() {
        return blocks::place(
            text,
            &found,
            &lines[1..lines.len() - 1],
            newline,
            Place::Above(&zephyr_line),
        );
    }

    Ok(match blocks::span(text, &found)? {
        Some(span) => blocks::cut(text, span, newline),
        None => text.to_string(),
    })
}

/// The library's folder as a line of the application's CMakeLists.txt names it, for {library} in
/// a manifest's groups: from the application's folder, or in full when it is somewhere else.
/// Unquoted, since the line has its own quotes around it.
fn library_folder(destination: &Path, project_root: &Path) -> String {
    match paths::relative(destination, project_root).as_deref() {
        None => paths::posix(&paths::resolve(destination)),
        Some(".") | Some("") => "${CMAKE_CURRENT_SOURCE_DIR}".to_string(),
        Some(folder) => format!("${{CMAKE_CURRENT_SOURCE_DIR}}/{folder}"),
    }
}

/// A group's block in the application's CMakeLists.txt, as lines, {library} standing for folder.
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

/// text with the library's groups for the application each in its place, and any group it no
/// longer has taken out.
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

        let side = if group.place == BEFORE_ZEPHYR {
            Place::Above(&zephyr_line)
        } else {
            Place::Below(&zephyr_line)
        };

        text = blocks::place(&text, &found, content, newline, side)?;
    }

    Ok(text)
}

/// Where find_package(Zephyr)'s line starts, for a block to go above it or below it.
fn zephyr_line(text: &str) -> Option<usize> {
    FIND_ZEPHYR
        .find(text)
        .map(|found| blocks::line_start(text, found.start()))
}

/// A line for each block the user changed inside since it was written.
pub(super) fn edited(
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

/// Every line the library asks for: (symbol, value, line), its own switch last.
pub fn wanted(library: &Manifest) -> Vec<(String, String, String)> {
    let part = library.part.zephyr();
    let mut found: Vec<(String, String, String)> = part
        .prj
        .iter()
        .map(|entry| {
            (
                entry.item.symbol.clone(),
                entry.item.value.clone(),
                entry.item.line(),
            )
        })
        .collect();

    found.push((
        part.symbol.clone(),
        "y".to_string(),
        format!("CONFIG_{}=y", part.symbol),
    ));

    found
}

/// The lines this tool owns in the Kconfig file.
///
/// A symbol the user sets already is written too, commented out with who sets it, so the block
/// shows what the library asked for and why it is not set here. Set again here, the line read
/// last would win and quietly undo the user's choice.
///
/// Settings listed in groups are written under their titles, each between two lines of stars, a
/// setting's note above it. The switch is last, a group of its own under the library's name,
/// after a blank line, with the note zephyr.symbol gives it.
fn conf_block(library: &Manifest, project_root: &Path, conf: &str, newline: &str) -> String {
    let taken = assigned(project_root, &library.name, conf);
    let part = library.part.zephyr();

    let written =
        |symbol: &str, line: String| match taken.iter().find(|(name, _, _)| name == symbol) {
            Some((_, have, file)) => {
                format!("# {line}  (left out: {file} sets CONFIG_{symbol}={have})")
            }
            None => line,
        };

    let body = annotated(
        &part.prj,
        |entry| written(&entry.symbol, entry.line()),
        "#",
        true,
    );
    let switch = written(&part.symbol, format!("CONFIG_{}=y", part.symbol));
    let found = marker(&library.name, None);

    let mut lines = vec![
        found.open.clone(),
        "# Added by libnima, and written again on every install, so set".to_string(),
        format!(
            "# {}'s settings below this block rather than in it.",
            library.name
        ),
    ];

    if !part.settings.is_empty() {
        lines.push("# Its settings, as they are unless you set them:".to_string());
        lines.extend(
            part.settings
                .iter()
                .map(|setting| format!("#   {}", setting.line())),
        );
    }

    lines.push(String::new());
    let had_body = !body.is_empty();
    lines.extend(body);

    if had_body {
        lines.push(String::new());
    }

    lines.extend(titled(&library.name, "#"));

    if let Some(note) = &part.symbol_note {
        lines.push(format!("# {note}"));
    }

    lines.push(switch);
    lines.push(found.close);

    lines.join(newline)
}

/// Switch the library on in the Kconfig file conf, a path from the application's folder, created
/// when it is not there.
fn integrate_conf(project_root: &Path, library: &Manifest, conf: &str) -> Outcome {
    let path = project_root.join(conf);
    let label = conf.replace('\\', "/");
    let name = &library.name;

    let text = if path.exists() {
        match files::read(&path) {
            Ok(text) => text,
            Err(error) => {
                return Outcome::new(
                    label.clone(),
                    Status::Manual,
                    format!("Could not read {label}: {}", files::reason(&error)),
                )
                .with_steps(
                    lines_of(&conf_block(library, project_root, conf, "\n"))
                        .iter()
                        .map(ToString::to_string)
                        .collect(),
                );
            }
        }
    } else {
        String::new()
    };

    let newline = line_ending(&text);
    let block = conf_block(library, project_root, conf, newline);

    let Ok(span) = block_span(&text, name) else {
        return half_written(&label, name, &block);
    };

    let updated = match span {
        Some((start, end)) => format!("{}{block}{}", &text[..start], &text[end..]),
        // At the end, after a blank line when the file has anything in it.
        None => append_block(&text, &block, newline),
    };

    let markers = [marker(name, None)];
    let mut outcome = write_block(&path, &label, &text, &updated, name)
        .with_blocks(label.clone(), blocks::fingerprints(&updated, &markers));
    outcome
        .steps
        .extend(edited(project_root, name, &label, &markers, &text));

    // Said again where the file's changes are, so it is not missed.
    let taken = assigned(project_root, name, conf);

    for (symbol, _, line) in wanted(library) {
        if let Some((_, have, file)) = taken.iter().find(|(found, _, _)| *found == symbol) {
            outcome.steps.push(format!(
                "{line} is left commented: {file} sets CONFIG_{symbol}={have}, and that is the one used."
            ));
        }
    }

    outcome
}

pub(super) fn append_block(text: &str, block: &str, newline: &str) -> String {
    blocks::appended(text, block, newline)
}

/// Every symbol sysbuild.conf sets outside this library's block: (symbol, value).
fn sysbuild_assigned(text: &str, name: &str) -> Vec<(String, String)> {
    let text = blocks::outside(text, &marker(name, None));
    let mut found: Vec<(String, String)> = Vec::new();

    for symbol in SB_NOT_SET.captures_iter(&text) {
        if !found.iter().any(|(name, _)| name == &symbol[1]) {
            found.push((symbol[1].to_string(), "n".to_string()));
        }
    }

    for symbol in SB_ASSIGNED.captures_iter(&text) {
        if !found.iter().any(|(name, _)| name == &symbol[1]) {
            found.push((symbol[1].to_string(), symbol[2].to_string()));
        }
    }

    found
}

/// The lines this tool owns in sysbuild.conf. A symbol the user sets outside it is written
/// commented out, with what the user set, as in conf_block, and settings listed in groups under
/// their titles, as there.
fn sysbuild_block(library: &Manifest, text: &str, newline: &str) -> String {
    let taken = sysbuild_assigned(text, &library.name);
    let found = marker(&library.name, None);
    let part = library.part.zephyr();
    let mut lines = vec![
        found.open.clone(),
        "# Added by libnima, and written again on every install, so set".to_string(),
        format!(
            "# {}'s sysbuild settings below this block rather than in it.",
            library.name
        ),
    ];

    if part.sysbuild.iter().any(|entry| entry.group.is_some()) {
        lines.push(String::new());
    }

    lines.extend(annotated(
        &part.sysbuild,
        |setting| match taken.iter().find(|(name, _)| *name == setting.symbol) {
            Some((symbol, have)) => {
                format!(
                    "# {}  (left out: {SYSBUILD_CONF} sets SB_CONFIG_{symbol}={have})",
                    setting.line()
                )
            }
            None => setting.line(),
        },
        "#",
        true,
    ));
    lines.push(found.close);

    lines.join(newline)
}

/// Write the library's sysbuild settings into sysbuild.conf, made when it is missing, or take out
/// the block of a version that had some. None for a library that has none and never had.
fn integrate_sysbuild(project_root: &Path, library: &Manifest) -> Option<Outcome> {
    let path = project_root.join(SYSBUILD_CONF);
    let name = &library.name;
    let part = library.part.zephyr();

    let text = if path.exists() {
        match files::read(&path) {
            Ok(text) => text,
            Err(error) => {
                return Some(
                    Outcome::new(
                        SYSBUILD_CONF,
                        Status::Manual,
                        format!("Could not read {SYSBUILD_CONF}: {}", files::reason(&error)),
                    )
                    .with_steps(
                        lines_of(&sysbuild_block(library, "", "\n"))
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

    let span = block_span(&text, name);

    if part.sysbuild.is_empty() {
        return match span {
            Ok(None) => None,
            _ => Some(cut_block(project_root, &path, SYSBUILD_CONF, name)),
        };
    }

    let newline = line_ending(&text);
    let block = sysbuild_block(library, &text, newline);

    let Ok(span) = span else {
        return Some(half_written(SYSBUILD_CONF, name, &block));
    };

    let updated = match span {
        Some((start, end)) => format!("{}{block}{}", &text[..start], &text[end..]),
        None => append_block(&text, &block, newline),
    };

    let markers = [marker(name, None)];
    let mut outcome = write_block(&path, SYSBUILD_CONF, &text, &updated, name)
        .with_blocks(SYSBUILD_CONF, blocks::fingerprints(&updated, &markers));
    outcome
        .steps
        .extend(edited(project_root, name, SYSBUILD_CONF, &markers, &text));

    let taken = sysbuild_assigned(&text, name);

    for setting in &part.sysbuild {
        if let Some((symbol, have)) = taken
            .iter()
            .find(|(found, _)| *found == setting.item.symbol)
        {
            outcome.steps.push(format!(
                "{} is left commented: {SYSBUILD_CONF} sets SB_CONFIG_{symbol}={have}, and that is the one used.",
                setting.item.line()
            ));
        }
    }

    Some(outcome)
}

/// Both edits, the Kconfig one only in a Zephyr application.
pub fn integrate(
    project_root: &Path,
    library: &Manifest,
    destination: &Path,
    conf: &str,
) -> Vec<Outcome> {
    let mut outcomes = vec![integrate_cmake(project_root, library, destination)];

    if find_cmakelists(project_root).is_some() {
        outcomes.push(integrate_conf(project_root, library, conf));
        outcomes.extend(integrate_sysbuild(project_root, library));
    }

    outcomes
}

/// Take this library's blocks out of one file, with the blank line each brought: its own, and in
/// CMakeLists.txt the one of its Kconfig fragment and every group of its CMake lines. A block the
/// user changed inside since it was written goes too, and that is said, with the copy that keeps
/// it, as an install says it.
fn cut_block(project_root: &Path, path: &Path, label: &str, name: &str) -> Outcome {
    let Ok(text) = files::read(path) else {
        return Outcome::new(
            label,
            Status::Already,
            format!("{label} has nothing of {name}."),
        );
    };

    let newline = line_ending(&text);
    let mut updated = text.clone();
    let mut markers = vec![marker(name, None), marker(name, Some(FRAGMENT_LABEL))];
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

    let installed = installed_libraries(project_root);
    let recorded = installed.get(name).and_then(|entry| entry.get("blocks"));
    let changed: Vec<&Marker> = markers
        .iter()
        .filter(|found| blocks::edited(recorded, label, found, &text))
        .collect();

    let saved = backup(path).ok();

    if let Err(error) = files::write(path, &updated) {
        return Outcome::new(
            label,
            Status::Manual,
            format!("Could not write {label}: {}", files::reason(&error)),
        )
        .with_backup(saved);
    }

    let kept = saved
        .as_ref()
        .and_then(|copy| copy.file_name())
        .map(|copy| format!(" {} keeps the file as it was.", copy.to_string_lossy()))
        .unwrap_or_default();
    let mut outcome = Outcome::new(
        label,
        Status::Changed,
        format!("Removed {name} from {label}."),
    )
    .with_backup(saved);

    outcome.steps.extend(changed.iter().map(|found| {
        format!(
            "Your change inside the {} block went with it, as the block was {name}'s.{kept}",
            found.name
        )
    }));

    outcome
}

/// Take this library's blocks out of CMakeLists.txt, its Kconfig file and sysbuild.conf.
/// sysbuild.conf itself stays, even with nothing left in it: the user may have made it.
pub fn uninstall(project_root: &Path, name: &str) -> Vec<Outcome> {
    let mut outcomes = Vec::new();
    let cmakelists = project_root.join(CMAKELISTS);

    if cmakelists.is_file() {
        outcomes.push(cut_block(project_root, &cmakelists, CMAKELISTS, name));
    }

    if let Some(conf) = holding(project_root, name) {
        outcomes.push(cut_block(
            project_root,
            &project_root.join(&conf),
            &conf,
            name,
        ));
    }

    let sysbuild = project_root.join(SYSBUILD_CONF);

    if sysbuild.is_file() && !matches!(block_span(&text_or_empty(&sysbuild), name), Ok(None)) {
        outcomes.push(cut_block(project_root, &sysbuild, SYSBUILD_CONF, name));
    }

    outcomes
}

/// The blocks of this library's that the user changed inside since they were written, as (file, block), in every file
/// of the application that holds one: what a check says, as the install will write them again.
pub(super) fn changed_blocks(project_root: &Path, name: &str, conf: &str) -> Vec<(String, String)> {
    let installed = installed_libraries(project_root);
    let recorded = installed.get(name).and_then(|entry| entry.get("blocks"));
    let mut found = Vec::new();

    for label in [CMAKELISTS, conf, SYSBUILD_CONF] {
        let text = text_or_empty(&project_root.join(label));

        if text.is_empty() || found.iter().any(|(file, _)| file == label) {
            continue;
        }

        let mut markers = vec![marker(name, None)];

        if label == CMAKELISTS {
            markers.push(marker(name, Some(FRAGMENT_LABEL)));
            markers.extend(
                blocks::labels(&text, name)
                    .iter()
                    .map(|group| marker(name, Some(group))),
            );
        }

        for found_marker in markers {
            if blocks::edited(recorded, label, &found_marker, &text) {
                found.push((label.to_string(), found_marker.name.clone()));
            }
        }
    }

    found
}

/// Lines of the application's Kconfig files that still set one of symbols: (file, lines). Read
/// once the library's own block is out.
pub fn leftover(project_root: &Path, symbols: &[String]) -> Vec<(String, Vec<String>)> {
    let mut found = Vec::new();

    for relative in conf_files(project_root) {
        let text = text_or_empty(&project_root.join(&relative));
        let lines: Vec<String> = lines_of(&text)
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
            .collect();

        if !lines.is_empty() {
            found.push((relative, lines));
        }
    }

    found
}
