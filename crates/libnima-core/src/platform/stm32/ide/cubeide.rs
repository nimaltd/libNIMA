//! STM32CubeIDE, which is Eclipse underneath.
//!
//! Two things have to be told to the project: where the header is, and that the folder
//! holds sources worth compiling. The second one is easy to miss, because leaving it out
//! looks like it worked. The header resolves, the editor stops underlining things, and the
//! build fails at link time on every symbol in the library.
//!
//! The .cproject file is edited as text rather than through an XML parser. It opens with a
//! <?fileVersion?> processing instruction many parsers silently drop, and a .cproject
//! missing that line is a project that will not open.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::base::{
    clash_steps, include_folders, indent_of, inner_indent, insert_before, plan_defines, relative,
    told, under,
};
use crate::files::{self, backup};
use crate::installer::Installed;
use crate::manifest::Manifest;
use crate::target::{Dropped, Outcome, Status};
use crate::text::line_ending;

pub const NAME: &str = "STM32CubeIDE";

/// <option ... superClass="....compiler.option.includepaths" ...>. Its body runs to the
/// first </option> after it, since options never nest, even with self closing siblings in
/// between. A self closing <option .../> has no body to add a path to.
static INCLUDE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<option[^>]*superClass="[^"]*compiler\.option\.includepaths"[^>]*>"#)
        .expect("a valid pattern")
});

/// The "Define symbols (-D)" list of the C and the C++ compiler, the one holding
/// USE_HAL_DRIVER, once per build configuration. The assembler has a list of its own,
/// ...assembler.option.definedsymbols, which this does not match: a define that shapes a C
/// header means nothing to it.
static DEFINE_OPEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<option[^>]*superClass="[^"]*compiler\.option\.definedsymbols"[^>]*>"#)
        .expect("a valid pattern")
});

static OPTION_CLOSE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)</option>").expect("a valid pattern"));

/// One value in such a list.
static LIST_VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"<listOptionValue\b[^>]*\bvalue="([^"]*)"[^>]*/>"#).expect("a valid pattern")
});

/// One value in such a list as a whole line, the line ending before it included.
static LIST_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\r?\n[ \t]*<listOptionValue\b[^>]*\bvalue="([^"]*)"[^>]*/>"#)
        .expect("a valid pattern")
});

/// The list of folders one build configuration compiles, and its body.
static SOURCE_ENTRIES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(<sourceEntries>)(.*?)(</sourceEntries>)").expect("a valid pattern")
});

/// One <entry .../> inside it. The attributes are read separately rather than matched in a
/// fixed order, since nothing promises CDT writes them the same way twice.
static ENTRY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<entry\b[^>]*/>").expect("a valid pattern"));
static ENTRY_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\r?\n[ \t]*<entry\b[^>]*/>").expect("a valid pattern"));
static ATTRIBUTE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(\w+)="([^"]*)""#).expect("a valid pattern"));

/// An option element with a body: where it opens, and where its body starts and ends.
struct Block {
    start: usize,
    body_start: usize,
    body_end: usize,
}

/// Every option of this kind in text.
fn blocks(text: &str, open: &Regex) -> Vec<Block> {
    let mut found = Vec::new();
    let mut at = 0;

    while let Some(tag) = open.find_at(text, at) {
        at = tag.end();

        if tag.as_str().ends_with("/>") {
            continue;
        }

        let Some(close) = OPTION_CLOSE.find_at(text, tag.end()) else {
            break;
        };

        found.push(Block {
            start: tag.start(),
            body_start: tag.end(),
            body_end: close.start(),
        });
        at = close.end();
    }

    found
}

/// The .cproject file, when this looks like a CubeIDE project.
pub fn detect(project_root: &Path) -> Option<PathBuf> {
    let cproject = project_root.join(".cproject");

    (cproject.is_file() && project_root.join(".project").is_file()).then_some(cproject)
}

fn attribute(element: &str, name: &str) -> Option<String> {
    ATTRIBUTE
        .captures_iter(element)
        .find(|found| &found[1] == name)
        .map(|found| found[2].to_string())
}

/// The folder names a <sourceEntries> body lists as source.
fn source_paths(body: &str) -> Vec<String> {
    ENTRY
        .find_iter(body)
        .filter(|element| attribute(element.as_str(), "kind").as_deref() == Some("sourcePath"))
        .map(|element| attribute(element.as_str(), "name").unwrap_or_default())
        .collect()
}

/// Put the library on the include path of every build configuration.
fn add_include_paths(text: &str, wanted: &[String], newline: &str) -> String {
    let mut updated = text.to_string();

    // Walk backwards, so each insertion does not move the offsets of the next.
    for block in blocks(text, &INCLUDE_OPEN).iter().rev() {
        // Asked of each configuration separately rather than of the whole file. A project
        // that has the path on Debug but not on Release is one a whole file check would
        // call finished, and the missing half would only turn up as a build that fails in
        // one configuration and not the other.
        let body = &text[block.body_start..block.body_end];
        let gaps: Vec<&String> = wanted
            .iter()
            .filter(|value| !body.contains(&format!("value=\"{value}\"")))
            .collect();

        if gaps.is_empty() {
            continue;
        }

        let outer = indent_of(text, block.start);
        let inner = inner_indent(body, &outer);
        let at = insert_before(&updated, block.body_end);

        // Appended after the paths that are already there, which is where CubeIDE itself
        // puts one added through the Properties dialog.
        let added: String = gaps
            .iter()
            .map(|value| {
                format!("{newline}{inner}<listOptionValue builtIn=\"false\" value=\"{value}\"/>")
            })
            .collect();

        updated.insert_str(at, &added);
    }

    updated
}

/// Bring the define list of every configuration in step with the library. Returns the new
/// text and the defines left alone because the list already sets the same name to
/// something else.
fn edit_defines(
    text: &str,
    wanted: &[String],
    dropped: &[String],
    newline: &str,
) -> (String, Vec<(String, String)>) {
    let mut updated = text.to_string();
    let mut clashes = Vec::new();

    for block in blocks(text, &DEFINE_OPEN).iter().rev() {
        let original = &text[block.body_start..block.body_end];
        let present: Vec<String> = LIST_VALUE
            .captures_iter(original)
            .map(|found| found[1].to_string())
            .collect();
        let plan = plan_defines(&present, wanted, dropped);

        for pair in plan.clashes {
            if !clashes.contains(&pair) {
                clashes.push(pair);
            }
        }

        if plan.remove.is_empty() && plan.add.is_empty() {
            continue;
        }

        let mut body = original.to_string();

        for define in &plan.remove {
            let line = LIST_LINE
                .captures_iter(&body)
                .find(|found| &found[1] == define.as_str())
                .map(|found| found.get(0).expect("the whole match").range());

            if let Some(range) = line {
                body.replace_range(range, "");
            }
        }

        // Appended after the values already there, which is where CubeIDE puts one added
        // through the Properties dialog.
        let inner = inner_indent(original, &indent_of(text, block.start));
        let cut = body.trim_end_matches([' ', '\t', '\r', '\n']).len();
        let added: String = plan
            .add
            .iter()
            .map(|define| {
                format!("{newline}{inner}<listOptionValue builtIn=\"false\" value=\"{define}\"/>")
            })
            .collect();

        body.insert_str(cut, &added);
        updated.replace_range(block.body_start..block.body_end, &body);
    }

    (updated, clashes)
}

/// Register the library folder as one the project compiles.
///
/// A <sourceEntries> block that names folders is the list of what gets built, and a folder
/// missing from it is compiled by nobody.
///
/// A block that is empty, or that carries a root entry with name="", already means the
/// whole project is source. Adding a named entry to one of those would turn "build
/// everything" into "build only this folder" and break the user's project rather than fix
/// it, so they are left exactly as they are.
fn add_source_folder(text: &str, folder: &str, newline: &str) -> String {
    let mut updated = text.to_string();
    let found: Vec<_> = SOURCE_ENTRIES.captures_iter(text).collect();

    for matched in found.iter().rev() {
        let body = matched.get(2).expect("a body").as_str();
        let names = source_paths(body);

        if names.is_empty()
            || names.iter().any(String::is_empty)
            || names.iter().any(|name| name == folder)
        {
            continue;
        }

        let outer = indent_of(text, matched.get(0).expect("the whole match").start());
        let inner = inner_indent(body, &outer);
        let at = insert_before(&updated, matched.get(3).expect("the close").start());

        updated.insert_str(
            at,
            &format!(
                "{newline}{inner}<entry flags=\"VALUE_WORKSPACE_PATH|RESOLVED\" kind=\"sourcePath\" name=\"{folder}\"/>"
            ),
        );
    }

    updated
}

/// Add the library to the include path, the source folders and the defines.
///
/// A file an update removed needs nothing here: CubeIDE compiles whatever is in a source
/// folder, so a file removed from disk has left the build as well. Defines it dropped are
/// taken out of the define lists.
pub fn integrate(
    cproject: &Path,
    library: &Manifest,
    destination: &Path,
    project_root: &Path,
    dropped: &Dropped,
) -> io::Result<Outcome> {
    let folder = relative(destination, project_root);

    // Include paths in .cproject are relative to the build folder, not the project root,
    // which is why CubeIDE's own entries read ../Core/Inc. Source folders are relative to
    // the project, so the two are not the same string.
    let wanted: Vec<String> = include_folders(library, &folder)
        .iter()
        .map(|dir| format!("../{dir}"))
        .collect();

    let text = match std::fs::read(cproject) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
        Err(error) => {
            return Ok(Outcome::new(
                NAME,
                Status::Manual,
                format!("Could not read .cproject: {}", files::reason(&error)),
            )
            .with_steps(manual_steps(&folder)));
        }
    };

    if blocks(&text, &INCLUDE_OPEN).is_empty() {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            "Could not find the include path settings in .cproject.",
        )
        .with_steps(manual_steps(&folder)));
    }

    let newline = line_ending(&text);
    let with_paths = add_include_paths(&text, &wanted, newline);
    let with_sources = add_source_folder(&with_paths, &folder, newline);
    let registered = with_sources != with_paths;
    let (updated, clashes) =
        edit_defines(&with_sources, &library.defines, &dropped.defines, newline);
    let defined = updated != with_sources;

    let mut steps = clash_steps(&clashes, ".cproject");

    // Without a define list the library would build, but with its defaults instead of the
    // settings it reads through the define, and nothing would say so. So it is said here.
    let unplaced = !library.defines.is_empty() && blocks(&text, &DEFINE_OPEN).is_empty();

    if unplaced {
        steps.extend(define_steps(&library.defines));
    }

    if updated == text && unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("{folder} is in .cproject, but found no define list to add to."),
        )
        .with_steps(steps));
    }

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("{folder} is already set up in .cproject."),
        )
        .with_steps(steps));
    }

    let saved = backup(cproject)?;

    if let Err(error) = files::write(cproject, &updated) {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("Could not write .cproject: {}", files::reason(&error)),
        )
        .with_steps(manual_steps(&folder))
        .with_backup(Some(saved)));
    }

    let mut parts = vec!["include path"];

    if registered {
        parts.push("source folders");
    }

    if defined {
        parts.push("defines");
    }

    let parts = told(&parts);

    if unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "Added {folder} to the {parts} in .cproject, but found no define list to add to."
            ),
        )
        .with_steps(steps)
        .with_backup(Some(saved)));
    }

    steps.push("Refresh the project in CubeIDE (F5) so it picks up the new files.".to_string());

    Ok(Outcome::new(
        NAME,
        Status::Changed,
        format!("Added {folder} to the {parts} in .cproject."),
    )
    .with_steps(steps)
    .with_backup(Some(saved)))
}

/// What to click, when this could not do it safely.
pub fn manual_steps(folder: &str) -> Vec<String> {
    vec![
        "In STM32CubeIDE: right click the project, Properties,".to_string(),
        "C/C++ Build, Settings, MCU GCC Compiler, Include paths,".to_string(),
        format!("then add \"{folder}\" as a workspace path."),
        format!("If {folder} is greyed out in the project tree, right click it and"),
        "choose Resource Configurations, Exclude from Build, and clear it.".to_string(),
    ]
}

/// What to click to add the library's defines by hand.
fn define_steps(defines: &[String]) -> Vec<String> {
    vec![
        "In STM32CubeIDE: right click the project, Properties, C/C++ Build, Settings,".to_string(),
        "MCU GCC Compiler, Preprocessor, and add to Define symbols, in every configuration:"
            .to_string(),
        format!("    {}", defines.join(" ")),
    ]
}

/// Take the library's include paths, source folder and defines out of .cproject.
pub fn remove(cproject: &Path, library: &Installed, project_root: &Path) -> io::Result<Outcome> {
    let destination = library.destination.as_deref().unwrap_or(project_root);
    let folder = relative(destination, project_root);
    let text = files::read(cproject)?;
    let newline = line_ending(&text);

    // Every include path at the library's folder or under it, in every configuration,
    // whatever its include_dirs were.
    let inside = format!("../{folder}");
    let mut updated = text.clone();

    for block in blocks(&text, &INCLUDE_OPEN).iter().rev() {
        let body = LIST_LINE.replace_all(
            &text[block.body_start..block.body_end],
            |found: &regex::Captures| {
                if under(&found[1], &inside) {
                    String::new()
                } else {
                    found[0].to_string()
                }
            },
        );

        updated.replace_range(block.body_start..block.body_end, &body);
    }

    let updated = SOURCE_ENTRIES.replace_all(&updated, |found: &regex::Captures| {
        let body = ENTRY_LINE.replace_all(&found[2], |entry: &regex::Captures| {
            let element = &entry[0];

            if attribute(element, "name").as_deref() == Some(folder.as_str())
                && attribute(element, "kind").as_deref() == Some("sourcePath")
            {
                String::new()
            } else {
                element.to_string()
            }
        });

        format!("{}{body}{}", &found[1], &found[3])
    });

    let (updated, _) = edit_defines(&updated, &[], &library.defines, newline);

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!(".cproject has nothing of {}.", library.name),
        ));
    }

    let saved = backup(cproject)?;
    files::write(cproject, &updated)?;

    Ok(Outcome::new(
        NAME,
        Status::Changed,
        format!("Removed {folder} from the include path, source folders and defines in .cproject."),
    )
    .with_steps(vec!["Refresh the project in CubeIDE (F5).".to_string()])
    .with_backup(Some(saved)))
}
