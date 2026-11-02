//! A Makefile, the kind STM32CubeMX writes when the toolchain is set to Makefile.
//!
//! CubeMX lists every C file in C_SOURCES, every assembler file in ASM_SOURCES and every
//! include folder in C_INCLUDES, one per line, each line but the last ending in a
//! backslash. The library's entries go at the end of those lists, in the same form as the
//! ones already there, and nothing else in the file is touched.
//!
//! Edited as text, like every other project file here, so the rest of it comes out byte for
//! byte identical.
//!
//! CubeMX can write this file again when it generates code, and anything added to it by
//! hand, or by this, is gone after that. The report says to run libNIMA again if it
//! happens, which puts the entries back.

use std::io;
use std::path::{Path, PathBuf};

use regex::Regex;

use super::base::{clash_steps, compiled_names, include_folders, plan_defines, relative, under};
use crate::files::{self, backup};
use crate::installer::Installed;
use crate::manifest::Manifest;
use crate::paths;
use crate::target::{Dropped, Outcome, Status};
use crate::text::{line_ending, lines_with_endings};

pub const NAME: &str = "Makefile";

/// Which list each kind of file belongs in. CubeMX has no C++ rules, so a C++ file gets a
/// manual step rather than a line the build would not know what to do with.
fn list_of(path: &str) -> Option<&'static str> {
    match paths::suffix(path).as_str() {
        ".c" => Some("C_SOURCES"),
        ".s" | ".asm" => Some("ASM_SOURCES"),
        _ => None,
    }
}

const LISTS: [&str; 2] = ["ASM_SOURCES", "C_SOURCES"];

const RERUN: &str = "CubeMX may write this Makefile again when it generates code. Run libNIMA again if the library is gone from it.";

/// The first line of a list, NAME = followed by whatever is on that line.
fn list_start(name: &str) -> Regex {
    Regex::new(&format!(r"^{name}[ \t]*=[^\r\n]*$")).expect("a valid pattern")
}

/// The Makefile at the project's root, when it is one CubeMX wrote.
///
/// Recognised by its C_SOURCES list rather than by name alone, since plenty of projects
/// have a Makefile of some other shape.
pub fn detect(project_root: &Path) -> Option<PathBuf> {
    let path = project_root.join("Makefile");
    let text = files::read(&path).ok()?;
    let start = list_start("C_SOURCES");

    text.split('\n')
        .any(|line| start.is_match(line.strip_suffix('\r').unwrap_or(line)))
        .then_some(path)
}

/// The first and last line of a list, as indexes into lines, or None. A list runs on for as
/// long as its lines end in a backslash.
fn span(lines: &[String], name: &str) -> Option<(usize, usize)> {
    let start_of = list_start(name);

    for (start, line) in lines.iter().enumerate() {
        if start_of.is_match(line.trim_end_matches(['\r', '\n'])) {
            let mut end = start;

            while lines[end].trim_end().ends_with('\\') && end + 1 < lines.len() {
                end += 1;
            }

            return Some((start, end));
        }
    }

    None
}

/// The entry a list line holds, without its backslash, or "" for none.
fn entry(line: &str) -> &str {
    line.trim_end().trim_end_matches('\\').trim()
}

/// An entry as it compares: forward slashes, no leading ./
fn key(entry: &str) -> String {
    let entry = entry.replace('\\', "/");

    entry
        .strip_prefix("./")
        .map_or_else(|| entry.clone(), str::to_string)
}

/// (line index, entry) for each entry in a list, the first line's own included.
fn entries(lines: &[String], (start, end): (usize, usize)) -> Vec<(usize, String)> {
    let mut found = Vec::new();

    for (index, line) in lines.iter().enumerate().take(end + 1).skip(start) {
        let text = if index == start {
            line.split_once('=').map_or("", |(_, rest)| rest)
        } else {
            line.as_str()
        };

        let value = entry(text);

        if !value.is_empty() {
            found.push((index, value.to_string()));
        }
    }

    found
}

/// The line ending a line has.
fn ending_of(line: &str) -> &str {
    &line[without_ending_len(line)..]
}

fn without_ending_len(line: &str) -> usize {
    line.trim_end_matches(['\r', '\n']).len()
}

/// Add entries at the end of a list, in the shape of the ones already there.
///
/// The line that used to be last gains a backslash, and so does every new line but the new
/// last one.
fn append(lines: &mut Vec<String>, (start, end): (usize, usize), added: &[String], newline: &str) {
    let existing = entries(lines, (start, end));
    let mut pad = String::new();

    if let Some((index, _)) = existing.last()
        && *index != start
    {
        let last = &lines[*index];
        pad = last[..last.len() - last.trim_start_matches([' ', '\t']).len()].to_string();
    }

    let ending = match ending_of(&lines[end]) {
        "" => newline.to_string(),
        found => found.to_string(),
    };

    let body = lines[end].trim_end().to_string();
    lines[end] = format!(
        "{body}{}{ending}",
        if body.is_empty() { "\\" } else { " \\" }
    );

    let mut new_lines: Vec<String> = added
        .iter()
        .map(|item| format!("{pad}{item} \\{ending}"))
        .collect();

    if let (Some(last), Some(item)) = (new_lines.last_mut(), added.last()) {
        *last = format!("{pad}{item}{ending}");
    }

    lines.splice(end + 1..end + 1, new_lines);
}

/// Remove one entry line from a list.
///
/// When it was the last, the line now last loses its backslash, or the list would run on
/// into whatever follows it.
fn drop_line(lines: &mut Vec<String>, (start, end): (usize, usize), index: usize) {
    lines.remove(index);

    if index == end && end > start {
        let before = lines[end - 1].clone();
        let ending = ending_of(&before).to_string();
        let body = before[..without_ending_len(&before)].trim_end();

        if let Some(rest) = body.strip_suffix('\\') {
            lines[end - 1] = format!("{}{ending}", rest.trim_end());
        }
    }
}

/// Bring a list in step with an update, before anything is added to it.
///
/// A dropped file whose name the new version still has, somewhere else, has its entry
/// changed where it stands. One with nothing to replace it loses its line. Returns the
/// entries of wanted that are still not in the list.
fn follow(
    lines: &mut Vec<String>,
    name: &str,
    dropped: &[String],
    wanted: &[String],
) -> Vec<String> {
    let Some(mut found) = span(lines, name) else {
        return wanted.to_vec();
    };

    let mut listed: Vec<(String, usize)> = entries(lines, found)
        .into_iter()
        .map(|(index, value)| (key(&value), index))
        .collect();
    let in_list = |listed: &Vec<(String, usize)>, wanted: &str| {
        listed
            .iter()
            .rev()
            .find(|(key, _)| key == wanted)
            .map(|(_, index)| *index)
    };

    let mut missing: Vec<String> = wanted
        .iter()
        .filter(|item| in_list(&listed, &key(item)).is_none())
        .cloned()
        .collect();
    let keys: Vec<String> = wanted.iter().map(|item| key(item)).collect();

    for old in dropped {
        let old_key = key(old);

        if keys.contains(&old_key) {
            continue;
        }

        let Some(index) = in_list(&listed, &old_key) else {
            continue;
        };

        let leaf = paths::leaf(&old_key).to_string();
        let moved = missing
            .iter()
            .position(|item| paths::leaf(&key(item)) == leaf);

        if let Some(position) = moved {
            let replacement = missing.remove(position);
            let current = entry(&lines[index]).to_string();
            lines[index] = lines[index].replacen(&current, &replacement, 1);
        } else {
            drop_line(lines, found, index);
            found = span(lines, name).unwrap_or((0, 0));
            listed = entries(lines, found)
                .into_iter()
                .map(|(index, value)| (key(&value), index))
                .collect();
        }
    }

    missing
}

/// Bring C_DEFS in step with the library, where the -DUSE_HAL_DRIVER line is. Returns the
/// defines left alone because the list already sets the same name to something else.
fn edit_defines(
    lines: &mut Vec<String>,
    wanted: &[String],
    dropped: &[String],
    newline: &str,
) -> Vec<(String, String)> {
    let Some(mut found) = span(lines, "C_DEFS") else {
        return Vec::new();
    };

    let present: Vec<(String, usize)> = entries(lines, found)
        .into_iter()
        .filter_map(|(index, value)| {
            value
                .strip_prefix("-D")
                .map(|define| (define.trim().to_string(), index))
        })
        .collect();
    let names: Vec<String> = present.iter().map(|(define, _)| define.clone()).collect();
    let plan = plan_defines(&names, wanted, dropped);

    // From the bottom up, so each line taken out leaves the indexes above it where they
    // were. Never the NAME = line itself, which would end the list.
    let mut going: Vec<usize> = plan
        .remove
        .iter()
        .filter_map(|define| {
            present
                .iter()
                .find(|(name, _)| name == define)
                .map(|(_, index)| *index)
        })
        .collect();
    going.sort_unstable();
    going.dedup();

    for index in going.into_iter().rev() {
        if index != found.0 {
            drop_line(lines, found, index);
            found = span(lines, "C_DEFS").unwrap_or(found);
        }
    }

    if !plan.add.is_empty() {
        let added: Vec<String> = plan
            .add
            .iter()
            .map(|define| format!("-D{define}"))
            .collect();
        append(lines, found, &added, newline);
    }

    plan.clashes
}

/// Add the library's sources, include folder and defines to a CubeMX Makefile.
///
/// `dropped` lists the library files an update has just removed, so their entries can follow
/// the file to its new place or go, and its dropped defines are taken out of C_DEFS.
pub fn integrate(
    makefile: &Path,
    library: &Manifest,
    destination: &Path,
    dropped: &Dropped,
) -> io::Result<Outcome> {
    // CubeMX writes every path relative to the project root, forward slashed.
    let parent = makefile.parent().unwrap_or(Path::new("."));
    let folder = relative(destination, parent);

    let text = match std::fs::read(makefile) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        Err(error) => {
            return Ok(Outcome::new(
                NAME,
                Status::Manual,
                format!("Could not read the Makefile: {}", files::reason(&error)),
            )
            .with_steps(manual_steps(&folder)));
        }
    };

    let newline = line_ending(&text);
    let mut lines: Vec<String> = lines_with_endings(&text)
        .into_iter()
        .map(str::to_string)
        .collect();

    if span(&lines, "C_SOURCES").is_none() || span(&lines, "C_INCLUDES").is_none() {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            "Could not find C_SOURCES and C_INCLUDES in the Makefile.",
        )
        .with_steps(manual_steps(&folder)));
    }

    let gone: Vec<String> = compiled_names(&dropped.files, destination)
        .iter()
        .map(|name| format!("{folder}/{name}"))
        .collect();

    let mut listed: Vec<String> = Vec::new();

    for name in LISTS {
        if let Some(found) = span(&lines, name) {
            listed.extend(
                entries(&lines, found)
                    .into_iter()
                    .map(|(_, value)| key(&value)),
            );
        }
    }

    let build_sources = library.build_sources();
    let was_there = gone
        .iter()
        .cloned()
        .chain(build_sources.iter().map(|name| format!("{folder}/{name}")))
        .any(|item| listed.contains(&key(&item)));

    let mut by_list: Vec<(&str, Vec<String>)> = Vec::new();
    let mut other = Vec::new();

    for name in &build_sources {
        match list_of(name) {
            None => other.push(format!("{folder}/{name}")),
            Some(list) => match by_list.iter_mut().find(|(found, _)| *found == list) {
                Some((_, items)) => items.push(format!("{folder}/{name}")),
                None => by_list.push((list, vec![format!("{folder}/{name}")])),
            },
        }
    }

    for list in LISTS {
        let old: Vec<String> = gone
            .iter()
            .filter(|item| list_of(item) == Some(list))
            .cloned()
            .collect();
        let wanted = by_list
            .iter()
            .find(|(found, _)| *found == list)
            .map_or(Vec::new(), |(_, items)| items.clone());
        let missing = follow(&mut lines, list, &old, &wanted);

        if !missing.is_empty() {
            match span(&lines, list) {
                Some(found) => append(&mut lines, found, &missing, newline),
                None => other.extend(missing),
            }
        }
    }

    let includes: Vec<String> = include_folders(library, &folder)
        .iter()
        .map(|dir| format!("-I{dir}"))
        .collect();
    let missing = follow(&mut lines, "C_INCLUDES", &[], &includes);

    if !missing.is_empty()
        && let Some(found) = span(&lines, "C_INCLUDES")
    {
        append(&mut lines, found, &missing, newline);
    }

    let mut steps: Vec<String> = other
        .iter()
        .map(|item| {
            format!("Add {item} to the build by hand. CubeMX's Makefile has no rule for it.")
        })
        .collect();

    // Without C_DEFS the library would build, but with its defaults instead of the settings
    // it reads through the define, and nothing would say so.
    let has_defs = span(&lines, "C_DEFS").is_some();
    let unplaced = !library.defines.is_empty() && !has_defs;

    if has_defs {
        let clashes = edit_defines(&mut lines, &library.defines, &dropped.defines, newline);
        steps.extend(clash_steps(&clashes, "the Makefile"));
    } else if unplaced {
        let flags: Vec<String> = library
            .defines
            .iter()
            .map(|define| format!("-D{define}"))
            .collect();
        steps.push(format!("Add to C_DEFS by hand: {}", flags.join(" ")));
    }

    let updated = lines.concat();

    if updated == text && unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "The Makefile has {}, but no C_DEFS to add to.",
                library.name
            ),
        )
        .with_steps(steps));
    }

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("The Makefile already has {}.", library.name),
        )
        .with_steps(steps));
    }

    let saved = backup(makefile)?;

    if let Err(error) = files::write(makefile, &updated) {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("Could not write the Makefile: {}", files::reason(&error)),
        )
        .with_steps(manual_steps(&folder))
        .with_backup(Some(saved)));
    }

    steps.push(RERUN.to_string());

    if unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "Added {} to the Makefile, but found no C_DEFS to add to.",
                library.name
            ),
        )
        .with_steps(steps)
        .with_backup(Some(saved)));
    }

    let named = if library.defines.is_empty() {
        "C_SOURCES and C_INCLUDES"
    } else {
        "C_SOURCES, C_INCLUDES and C_DEFS"
    };
    let message = if was_there {
        format!("Updated {} in the Makefile.", library.name)
    } else {
        format!("Added {} to {named} in the Makefile.", library.name)
    };

    Ok(Outcome::new(NAME, Status::Changed, message)
        .with_steps(steps)
        .with_backup(Some(saved)))
}

/// What to edit, when this could not do it safely.
pub fn manual_steps(folder: &str) -> Vec<String> {
    vec![
        format!("In the Makefile, add the .c files from {folder} to C_SOURCES,"),
        format!("and -I{folder} to C_INCLUDES."),
    ]
}

/// Take the library's sources, include folders and defines out of a CubeMX Makefile.
pub fn remove(makefile: &Path, library: &Installed) -> io::Result<Outcome> {
    let parent = makefile.parent().unwrap_or(Path::new("."));
    let destination = library.destination.as_deref().unwrap_or(parent);
    let folder = relative(destination, parent);
    let text = files::read(makefile)?;
    let newline = line_ending(&text);
    let mut lines: Vec<String> = lines_with_endings(&text)
        .into_iter()
        .map(str::to_string)
        .collect();

    for name in LISTS.iter().chain(&["C_INCLUDES"]) {
        let Some(mut found) = span(&lines, name) else {
            continue;
        };

        // From the bottom up, so each line taken out leaves the indexes above it where
        // they were. Never the NAME = line itself, which would end the list.
        for (index, value) in entries(&lines, found).into_iter().rev() {
            let value = if *name == "C_INCLUDES" {
                value.strip_prefix("-I").unwrap_or(&value).to_string()
            } else {
                value
            };

            if index != found.0 && under(&key(&value), &folder) {
                drop_line(&mut lines, found, index);
                found = span(&lines, name).unwrap_or(found);
            }
        }
    }

    edit_defines(&mut lines, &[], &library.defines, newline);

    let updated = lines.concat();

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("The Makefile has nothing of {}.", library.name),
        ));
    }

    let saved = backup(makefile)?;
    files::write(makefile, &updated)?;

    Ok(Outcome::new(
        NAME,
        Status::Changed,
        format!("Removed {} from the Makefile.", library.name),
    )
    .with_backup(Some(saved)))
}
