//! IAR Embedded Workbench.
//!
//! Like Keil, IAR registers every source file explicitly, so both the files and the include
//! path have to be added. Paths use the $PROJ_DIR$ variable, which keeps the project movable.
//!
//! Edited as text rather than through an XML parser, so everything this does not touch
//! survives byte for byte.
//!
//! The group is named after the library and nothing else, because that name is what the user
//! reads in the project tree. Whether the library is already there is read off the file paths
//! instead of a marker in the name, so a group the user renamed is still recognised.
//!
//! Three things about the real files are easy to get wrong:
//!
//! - IAR leaves a "Backup of <name>.ewp" next to the project when it upgrades one, and that
//!   copy is a valid .ewp that sorts before the original. The workspace file says which
//!   project is the real one, so it is asked first.
//! - CubeMX mixes the two slashes inside one file, forward in the include paths and back in
//!   the file list, so which one to write is a question per section rather than per file.
//! - Groups nest, so a <group>...</group> pattern closes on a child's tag rather than its
//!   own. Nothing here tries to match a whole group.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::{Captures, Regex};

use super::base::{
    clash_steps, compiled_names, element_indent, forward_slashes, include_folders, indent_of,
    indent_step, inner_indent, insert_before, is_backup, plan_defines, relative, under,
};
use crate::files::{self, backup};
use crate::installer::Installed;
use crate::manifest::Manifest;
use crate::paths;
use crate::target::{Dropped, Outcome, Status};
use crate::text::line_ending;

pub const NAME: &str = "IAR Embedded Workbench";

/// The include path option, which repeats once per build configuration.
static INCLUDE_OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(<option>\s*<name>CCIncludePath2</name>)(.*?)(</option>)")
        .expect("a valid pattern")
});

/// The compiler's defines, beside USE_HAL_DRIVER, once per build configuration.
static DEFINE_OPTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(<option>\s*<name>CCDefines</name>)(.*?)(</option>)").expect("a valid pattern")
});

/// One value in an option. An empty <state></state> holds none.
static STATE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<state>([^<]*)</state>").expect("a valid pattern"));

/// One value in an option as a whole line, the line ending before it included.
static STATE_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\r?\n[ \t]*<state>([^<]*)</state>").expect("a valid pattern"));

/// A source file already in the project, read to see which slash is used and to tell whether
/// the library's files are in the project already.
static FILE_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<file>\s*<name>([^<]*)</name>").expect("a valid pattern"));

/// A group and the <name> line under it. That pair gives both the indentation of a group and
/// the size of one step, without matching a whole group, which nesting makes impossible.
static GROUP_INDENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^([ \t]*)<group>[ \t]*\r?\n([ \t]*)<name>").expect("a valid pattern")
});

/// <path>$WS_DIR$\Name.ewp</path>, the project a workspace points at.
static WORKSPACE_PROJECT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"<path>\s*\$WS_DIR\$[\\/]([^<]+?)\s*</path>").expect("a valid pattern")
});

/// End of the project, where a new group can be appended.
static PROJECT_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"</project>\s*\z").expect("a valid pattern"));

/// One <file> element as whole lines, with any <excluded> configurations inside it. Files do
/// not nest, so the first </file> is its own, even though groups do.
static FILE_ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^[ \t]*<file>.*?</file>[ \t]*\r?\n").expect("a valid pattern")
});

/// The .ewp file, when this looks like an IAR project.
///
/// The workspace comes first because it names the real project. Falling back to a plain
/// search and taking the first hit is what put a library into "Backup of STM32G431CBU6.ewp"
/// and reported success, while the project the user builds was never touched.
pub fn detect(project_root: &Path) -> Option<PathBuf> {
    let below = paths::files_below(project_root);

    for workspace in below.iter().filter(|path| paths::suffix(path) == ".eww") {
        let path = project_root.join(workspace);
        let text = files::read(&path).unwrap_or_default();
        let parent = path.parent().unwrap_or(project_root);

        for matched in WORKSPACE_PROJECT.captures_iter(&text) {
            let named = parent.join(matched[1].replace('\\', "/"));

            if named.is_file() {
                return Some(named);
            }
        }
    }

    below
        .into_iter()
        .find(|path| paths::suffix(path) == ".ewp" && !is_backup(path))
        .map(|path| project_root.join(path))
}

/// Which slash a set of paths is written with.
///
/// IAR takes either, and CubeMX uses both in one file: forward slashes in the include paths,
/// backslashes in the file list. So this has to be asked of the section being edited, never
/// of the whole file.
fn separator(sample: &str) -> &'static str {
    if sample.matches("$PROJ_DIR$\\").count() > sample.matches("$PROJ_DIR$/").count() {
        "\\"
    } else {
        "/"
    }
}

/// The <name> values this would write into the file list.
fn file_names(text: &str, folder: &str, sources: &[String]) -> Vec<String> {
    let sample: String = FILE_PATH
        .captures_iter(text)
        .map(|found| found[1].to_string())
        .collect();
    let sep = separator(&sample);
    let base = folder.replace('/', sep);

    // The source's own folder as well: a library keeps src/demo.c there.
    sources
        .iter()
        .map(|name| format!("$PROJ_DIR${sep}{base}{sep}{}", name.replace('/', sep)))
        .collect()
}

/// A <group> holding every source file of one library.
fn group(library_name: &str, names: &[String], pad: &str, step: &str, newline: &str) -> String {
    let mut lines = vec![
        format!("{pad}<group>"),
        format!("{pad}{step}<name>{library_name}</name>"),
    ];

    for name in names {
        lines.push(format!("{pad}{step}<file>"));
        lines.push(format!("{pad}{step}{step}<name>{name}</name>"));
        lines.push(format!("{pad}{step}</file>"));
    }

    lines.push(format!("{pad}</group>"));

    lines.join(newline)
}

/// The path a <file> element names, forward slashed, or None.
fn entry_name(element: &str) -> Option<String> {
    FILE_PATH
        .captures(element)
        .map(|found| forward_slashes(&found[1]))
}

/// Bring the entries of files an update moved or removed in step with it.
///
/// An entry whose file moved keeps its place, its group and any <excluded> settings, and only
/// its path changes. An entry whose file has nothing to replace it is removed. Returns the new
/// text and the names the project still does not list at all.
fn follow(text: &str, dropped_names: &[String], names: &[String]) -> (String, Vec<String>) {
    let wanted: Vec<String> = names.iter().map(|name| forward_slashes(name)).collect();
    let listed: Vec<String> = FILE_PATH
        .captures_iter(text)
        .map(|found| forward_slashes(&found[1]))
        .collect();
    let mut missing: Vec<String> = names
        .iter()
        .filter(|name| !listed.contains(&forward_slashes(name)))
        .cloned()
        .collect();
    let mut text = text.to_string();

    for old in dropped_names {
        let key = forward_slashes(old);

        if wanted.contains(&key) || !listed.contains(&key) {
            continue;
        }

        let leaf = paths::leaf(&key);
        let moved = missing
            .iter()
            .position(|name| paths::leaf(&forward_slashes(name)) == leaf);

        if let Some(position) = moved {
            let moved = missing.remove(position);

            text = FILE_PATH
                .replace_all(&text, |found: &Captures| {
                    if forward_slashes(&found[1]) != key {
                        return found[0].to_string();
                    }

                    let whole = found.get(0).expect("the whole match");
                    let name = found.get(1).expect("a name");

                    format!(
                        "{}{moved}{}",
                        &whole.as_str()[..name.start() - whole.start()],
                        &whole.as_str()[name.end() - whole.start()..]
                    )
                })
                .into_owned();
        } else {
            text = FILE_ELEMENT
                .replace_all(&text, |found: &Captures| {
                    if entry_name(&found[0]).as_deref() == Some(key.as_str()) {
                        String::new()
                    } else {
                        found[0].to_string()
                    }
                })
                .into_owned();
        }
    }

    (text, missing)
}

/// Add files the library gained right after its last entry.
///
/// That puts them in the library's own group, whatever it has been renamed to, and lines them
/// up with the entries already there.
fn add_beside(text: &str, missing: &[String], library: &[String]) -> String {
    let last = FILE_ELEMENT
        .find_iter(text)
        .filter(|entry| entry_name(entry.as_str()).is_some_and(|name| library.contains(&name)))
        .last();

    let Some(last) = last else {
        return text.to_string();
    };

    let (pad, step) = element_indent(last.as_str());
    let newline = line_ending(text);
    let block: String = missing
        .iter()
        .map(|name| {
            format!(
                "{pad}<file>{newline}{pad}{step}<name>{name}</name>{newline}{pad}</file>{newline}"
            )
        })
        .collect();

    format!("{}{block}{}", &text[..last.end()], &text[last.end()..])
}

/// The text with the library's files in it, or unchanged when nothing is missing.
///
/// On an update, entries for files that moved follow them and entries for files that are gone
/// go. Files the library gained join its existing group. A library the project has none of
/// gets a group of its own.
///
/// Used for the .ewp and again for the .ewt beside it, which carries the same file tree for
/// the analysis tools and no include paths at all.
fn with_group(
    text: &str,
    library_name: &str,
    folder: &str,
    sources: &[String],
    dropped: &[String],
) -> String {
    let names = file_names(text, folder, sources);
    let (text, missing) = follow(text, &file_names(text, folder, dropped), &names);

    // A header only library has nothing to compile, so it gets an include path and no group. An
    // empty group would be added again on every run, since there would be no file in the project
    // to recognise it by.
    if names.is_empty() || missing.is_empty() {
        return text;
    }

    if missing.len() < names.len() {
        let ours: Vec<String> = names.iter().map(|name| forward_slashes(name)).collect();
        return add_beside(&text, &missing, &ours);
    }

    let Some(closing) = PROJECT_CLOSE.find(&text) else {
        return text;
    };

    // The shallowest pair, so the indentation of a top level group rather than of one nested
    // inside another.
    let mut levels: Vec<(String, String)> = GROUP_INDENT
        .captures_iter(&text)
        .map(|found| (found[1].to_string(), found[2].to_string()))
        .collect();
    levels.sort_by_key(|(pad, _)| pad.len());

    let (pad, step) = match levels.first() {
        Some((pad, inner)) => (pad.clone(), indent_step(pad, inner)),
        None => ("  ".to_string(), "  ".to_string()),
    };

    let newline = line_ending(&text);
    let at = insert_before(&text, closing.start());
    let block = group(library_name, &names, &pad, &step, newline);

    format!("{}{newline}{block}{}", &text[..at], &text[at..])
}

/// Bring the define list of every configuration in step with the library. Returns the new text
/// and the defines left alone because the list already sets the same name to something else.
fn edit_defines(
    text: &str,
    wanted: &[String],
    dropped: &[String],
    newline: &str,
) -> (String, Vec<(String, String)>) {
    let mut clashes: Vec<(String, String)> = Vec::new();

    let updated = DEFINE_OPTION.replace_all(text, |found: &Captures| {
        let whole = found.get(0).expect("the whole match");
        let original = &found[2];
        let present: Vec<String> = STATE
            .captures_iter(original)
            .map(|state| state[1].to_string())
            .filter(|value| !value.trim().is_empty())
            .collect();
        let plan = plan_defines(&present, wanted, dropped);

        for pair in plan.clashes {
            if !clashes.contains(&pair) {
                clashes.push(pair);
            }
        }

        if plan.remove.is_empty() && plan.add.is_empty() {
            return found[0].to_string();
        }

        let mut body = original.to_string();

        for define in &plan.remove {
            let line = STATE_LINE
                .captures_iter(&body)
                .find(|state| &state[1] == define.as_str())
                .map(|state| state.get(0).expect("the whole match").range());

            if let Some(range) = line {
                body.replace_range(range, "");
            }
        }

        let outer = indent_of(text, whole.start());
        let inner = inner_indent(original, &outer);
        let added: String = plan
            .add
            .iter()
            .map(|define| format!("{newline}{inner}<state>{define}</state>"))
            .collect();

        format!(
            "{}{}{added}{newline}{outer}{}",
            &found[1],
            body.trim_end(),
            &found[3]
        )
    });

    (updated.into_owned(), clashes)
}

/// Register the library's sources, include path and defines with an IAR project.
///
/// `dropped` lists the library files an update has just removed, so the entries that named them
/// can follow the file to its new place or go, and its dropped defines are taken out of the
/// define lists.
pub fn integrate(
    ewp: &Path,
    library: &Manifest,
    destination: &Path,
    dropped: &Dropped,
) -> io::Result<Outcome> {
    let name = ewp
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let gone = compiled_names(&dropped.files, destination);
    // Paths inside a .ewp are written relative to the folder holding it.
    let parent = ewp.parent().unwrap_or(Path::new("."));
    let folder = relative(destination, parent);

    let text = match std::fs::read(ewp) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        Err(error) => {
            return Ok(Outcome::new(
                NAME,
                Status::Manual,
                format!("Could not read {name}: {}", files::reason(&error)),
            )
            .with_steps(manual_steps(&folder)));
        }
    };

    if !INCLUDE_OPTION.is_match(&text) || !PROJECT_CLOSE.is_match(&text) {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("Could not find the include paths or the end of {name}."),
        )
        .with_steps(manual_steps(&folder)));
    }

    let newline = line_ending(&text);
    let sample: String = INCLUDE_OPTION
        .captures_iter(&text)
        .map(|found| found[2].to_string())
        .collect();
    let include_sep = separator(&sample);
    let folders = include_folders(library, &folder);

    let with_includes = INCLUDE_OPTION.replace_all(&text, |found: &Captures| {
        let whole = found.get(0).expect("the whole match");
        let outer = indent_of(&text, whole.start());
        let inner = inner_indent(&found[2], &outer);
        let body = &found[2];
        let present = forward_slashes(body);
        let added: String = folders
            .iter()
            .filter(|dir| !present.contains(&format!("<state>$PROJ_DIR$/{dir}</state>")))
            .map(|dir| {
                format!(
                    "{newline}{inner}<state>$PROJ_DIR${include_sep}{}</state>",
                    dir.replace('/', include_sep)
                )
            })
            .collect();

        if added.is_empty() {
            return found[0].to_string();
        }

        format!(
            "{}{}{added}{newline}{outer}{}",
            &found[1],
            body.trim_end(),
            &found[3]
        )
    });

    let sources = library.build_sources();
    let listed: Vec<String> = FILE_PATH
        .captures_iter(&text)
        .map(|found| forward_slashes(&found[1]))
        .collect();
    let everything: Vec<String> = sources.iter().chain(&gone).cloned().collect();
    let was_there = file_names(&text, &folder, &everything)
        .iter()
        .any(|item| listed.contains(&forward_slashes(item)));

    let updated = with_group(&with_includes, &library.name, &folder, &sources, &gone);
    let (updated, clashes) = edit_defines(&updated, &library.defines, &dropped.defines, newline);
    let mut notes = clash_steps(&clashes, &name);

    // Without a define list the library would build, but with its defaults instead of the
    // settings it reads through the define, and nothing would say so. So it is said here.
    let unplaced = !library.defines.is_empty() && !DEFINE_OPTION.is_match(&text);

    if unplaced {
        notes.extend(define_steps(&library.defines));
    }

    if updated == text && unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("{name} has {}, but no define list to add to.", library.name),
        )
        .with_steps(notes));
    }

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("{name} already has {}.", library.name),
        )
        .with_steps(notes));
    }

    let saved = backup(ewp)?;

    if let Err(error) = files::write(ewp, &updated) {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("Could not write {name}: {}", files::reason(&error)),
        )
        .with_steps(manual_steps(&folder))
        .with_backup(Some(saved)));
    }

    let mut steps = vec!["Reopen the workspace in IAR so it reloads the file list.".to_string()];

    if let Some(also) = sync_companion(ewp, library, &folder, &gone) {
        steps.insert(0, also);
    }

    let message = if was_there {
        format!("Updated {} in {name}", library.name)
    } else {
        format!("Added {} to {name}", library.name)
    };

    notes.extend(steps);

    if unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("{message}, but found no define list to add to."),
        )
        .with_steps(notes)
        .with_backup(Some(saved)));
    }

    Ok(Outcome::new(NAME, Status::Changed, format!("{message}."))
        .with_steps(notes)
        .with_backup(Some(saved)))
}

/// Keep the .ewt beside the project in step with it.
///
/// IAR writes one next to every .ewp and mirrors the file tree into it for the analysis tools.
/// The build reads the .ewp, so this is not what makes the library compile, but leaving the two
/// disagreeing is a difference the IDE will silently undo later and nobody will understand why.
///
/// Returns a line for the user when it could not be done, and None when there was nothing to do
/// or it worked.
fn sync_companion(
    ewp: &Path,
    library: &Manifest,
    folder: &str,
    dropped: &[String],
) -> Option<String> {
    let companion = ewp.with_extension("ewt");

    if !companion.is_file() {
        return None;
    }

    let name = companion
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let project = ewp
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let Ok(text) = files::read(&companion) else {
        return Some(format!(
            "Could not read {name}. Only {project} was updated."
        ));
    };

    let updated = with_group(
        &text,
        &library.name,
        folder,
        &library.build_sources(),
        dropped,
    );

    if updated == text {
        return None;
    }

    if backup(&companion).is_err() || files::write(&companion, &updated).is_err() {
        return Some(format!(
            "Could not write {name}. Only {project} was updated."
        ));
    }

    None
}

/// What to click, when this could not do it safely.
pub fn manual_steps(folder: &str) -> Vec<String> {
    vec![
        "In IAR: right click the project, Add, Add Group, then add the".to_string(),
        format!(".c files from {folder} to it."),
        "Then Project, Options, C/C++ Compiler, Preprocessor,".to_string(),
        format!("and add $PROJ_DIR$/{folder} to the include directories."),
    ]
}

/// What to click to add the library's defines by hand.
fn define_steps(defines: &[String]) -> Vec<String> {
    let mut steps = vec![
        "In IAR: Project, Options, C/C++ Compiler, Preprocessor, and add to".to_string(),
        "Defined symbols, one per line, in every configuration:".to_string(),
    ];

    steps.extend(defines.iter().map(|define| format!("    {define}")));

    steps
}

/// The text without the files under prefix, and without the group they leave empty.
fn without(text: &str, name: &str, prefix: &str) -> String {
    let updated = FILE_ELEMENT.replace_all(text, |found: &Captures| {
        if under(&entry_name(&found[0]).unwrap_or_default(), prefix) {
            String::new()
        } else {
            found[0].to_string()
        }
    });

    // Groups nest, so only the empty form is matched: the group the install added, with nothing
    // left in it. One the user renamed is theirs.
    let empty = Regex::new(&format!(
        r"(?m)^[ \t]*<group>\s*<name>{}</name>\s*</group>[ \t]*\r?\n",
        regex::escape(name)
    ))
    .expect("a valid pattern");

    empty.replace_all(&updated, "").into_owned()
}

/// Take the library's files, its group, include path and defines out of an IAR project.
pub fn remove(ewp: &Path, library: &Installed) -> io::Result<Outcome> {
    let name = ewp
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let parent = ewp.parent().unwrap_or(Path::new("."));
    let prefix = format!(
        "$PROJ_DIR$/{}",
        relative(library.destination.as_deref().unwrap_or(parent), parent)
    );
    let text = files::read(ewp)?;
    let newline = line_ending(&text);

    let stripped = without(&text, &library.name, &prefix);
    let updated = INCLUDE_OPTION.replace_all(&stripped, |found: &Captures| {
        let body = STATE_LINE.replace_all(&found[2], |state: &Captures| {
            if under(&state[1], &prefix) {
                String::new()
            } else {
                state[0].to_string()
            }
        });

        format!("{}{body}{}", &found[1], &found[3])
    });

    let (updated, _) = edit_defines(&updated, &[], &library.defines, newline);

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("{name} has nothing of {}.", library.name),
        ));
    }

    let saved = backup(ewp)?;
    files::write(ewp, &updated)?;

    // The .ewt beside it carries the same file tree for the analysis tools.
    let companion = ewp.with_extension("ewt");

    if companion.is_file() {
        let old = files::read(&companion)?;
        let new = without(&old, &library.name, &prefix);

        if new != old {
            backup(&companion)?;
            files::write(&companion, &new)?;
        }
    }

    Ok(Outcome::new(
        NAME,
        Status::Changed,
        format!("Removed {} from {name}.", library.name),
    )
    .with_steps(vec![
        "Reopen the workspace in IAR so it reloads the file list.".to_string(),
    ])
    .with_backup(Some(saved)))
}
