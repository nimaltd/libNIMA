//! Keil MDK, both uVision 5 and 6.
//!
//! The two share the .uvprojx format, so one integration covers both. Keil does not
//! discover files on its own, so each source file has to be registered as well as the
//! include path.
//!
//! Edited as text, not through an XML parser, so the rest of the file comes out of this
//! byte for byte identical.
//!
//! The group is named after the library and nothing else, because that name is what the
//! user reads in the project tree all day. Knowing whether the library is already there is
//! therefore worked out from the file paths in the project rather than from a marker in the
//! name, which also means a group the user renamed is still recognised.

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

pub const NAME: &str = "Keil MDK";

/// <IncludePath>..\Core\Inc;..\Drivers\...</IncludePath>
static INCLUDE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)(<IncludePath>)(.*?)(</IncludePath>)").expect("a valid pattern")
});

/// <FilePath>..\Core\Src\main.c</FilePath>, read to see which slash is used and to tell
/// whether the library's files are in the project already.
static FILE_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<FilePath>(.*?)</FilePath>").expect("a valid pattern"));

/// The <Groups> container holding a target's file groups, and its body. There is one per
/// target, and a project with a second target needs the library in both.
static GROUPS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)(<Groups>)(.*?)(</Groups>)").expect("a valid pattern"));

/// One <File> element as whole lines, with whatever per-file options uVision has stored
/// inside it. Elements do not nest, so the first </File> is its own.
static FILE_ELEMENT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?ms)^[ \t]*<File>.*?</File>[ \t]*\r?\n").expect("a valid pattern")
});

/// The C compiler settings of one target. The defines go in its <Define>, beside
/// USE_HAL_DRIVER, and never in the one under <Aads>, which is the assembler's.
static CADS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<Cads>.*?</Cads>").expect("a valid pattern"));
static DEFINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)(<Define>)(.*?)(</Define>)").expect("a valid pattern"));
static DEFINE_SPLIT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[,\s]+").expect("a valid pattern"));

/// The .uvprojx file, when this looks like a Keil project.
///
/// Backups are skipped for the same reason as in the IAR integration: a copy the IDE left
/// behind is a valid project file, and editing it changes nothing the user can see.
pub fn detect(project_root: &Path) -> Option<PathBuf> {
    paths::files_below(project_root)
        .into_iter()
        .find(|path| paths::suffix(path) == ".uvprojx" && !is_backup(path))
        .map(|path| project_root.join(path))
}

/// Which slash the project already writes its paths with.
///
/// uVision writes backslashes into a path added through its own dialogs, and CubeMX
/// generates forward slashes. Keil reads either, so the right one to use is simply
/// whichever the file already uses.
fn separator(text: &str) -> &'static str {
    let mut found = String::new();

    for matched in INCLUDE_PATH.captures_iter(text) {
        found.push_str(&matched[2]);
    }

    for matched in FILE_PATH.captures_iter(text) {
        found.push_str(&matched[1]);
    }

    if found.matches('\\').count() > found.matches('/').count() {
        "\\"
    } else {
        "/"
    }
}

/// The <FilePath> values this would write, in the project's own slash.
fn file_paths(folder: &str, sources: &[String], sep: &str) -> Vec<String> {
    let base = folder.replace('/', sep);

    // The source's own folder as well: a library keeps src/demo.c there.
    sources
        .iter()
        .map(|name| format!("{base}{sep}{}", name.replace('/', sep)))
        .collect()
}

/// One <File> element, in the shape uVision writes them.
fn file_entry(file_name: &str, file_path: &str, pad: &str, step: &str) -> Vec<String> {
    vec![
        format!("{pad}<File>"),
        format!("{pad}{step}<FileName>{file_name}</FileName>"),
        format!("{pad}{step}<FileType>1</FileType>"),
        format!("{pad}{step}<FilePath>{file_path}</FilePath>"),
        format!("{pad}</File>"),
    ]
}

/// A <Group> holding every source file of one library, as (name, path) pairs.
fn group(
    library_name: &str,
    found: &[(String, String)],
    pad: &str,
    step: &str,
    newline: &str,
) -> String {
    let mut lines = vec![
        format!("{pad}<Group>"),
        format!("{pad}{step}<GroupName>{library_name}</GroupName>"),
        format!("{pad}{step}<Files>"),
    ];

    for (file_name, file_path) in found {
        lines.extend(file_entry(
            file_name,
            file_path,
            &format!("{pad}{step}{step}"),
            step,
        ));
    }

    lines.push(format!("{pad}{step}</Files>"));
    lines.push(format!("{pad}</Group>"));

    lines.join(newline)
}

/// The path a <File> element names, forward slashed, or None.
fn entry_path(element: &str) -> Option<String> {
    FILE_PATH
        .captures(element)
        .map(|found| forward_slashes(&found[1]))
}

/// Bring the entries of files an update moved or removed in step with it.
///
/// An entry whose file moved keeps its place, its group and any per-file options uVision
/// stored in it. Only its path changes. An entry whose file has nothing to replace it is
/// removed. Returns the new text and the (name, path) pairs the project still does not name
/// at all.
fn follow(
    text: &str,
    dropped_paths: &[String],
    found: &[(String, String)],
) -> (String, Vec<(String, String)>) {
    let wanted: Vec<String> = found
        .iter()
        .map(|(_, path)| forward_slashes(path))
        .collect();
    let listed: Vec<String> = FILE_PATH
        .captures_iter(text)
        .map(|matched| forward_slashes(&matched[1]))
        .collect();
    let mut missing: Vec<(String, String)> = found
        .iter()
        .filter(|(_, path)| !listed.contains(&forward_slashes(path)))
        .cloned()
        .collect();
    let mut text = text.to_string();

    for old in dropped_paths {
        let key = forward_slashes(old);

        if wanted.contains(&key) || !listed.contains(&key) {
            continue;
        }

        let name = paths::leaf(&key);
        let moved = missing.iter().position(|(file_name, _)| file_name == name);

        if let Some(position) = moved {
            let (_, path) = missing.remove(position);

            text = FILE_PATH
                .replace_all(&text, |matched: &Captures| {
                    if forward_slashes(&matched[1]) == key {
                        format!("<FilePath>{path}</FilePath>")
                    } else {
                        matched[0].to_string()
                    }
                })
                .into_owned();
        } else {
            text = FILE_ELEMENT
                .replace_all(&text, |matched: &Captures| {
                    if entry_path(&matched[0]).as_deref() == Some(key.as_str()) {
                        String::new()
                    } else {
                        matched[0].to_string()
                    }
                })
                .into_owned();
        }
    }

    (text, missing)
}

/// Add files the library gained next to its entries, in every target.
///
/// Right after its last entry, which puts them in the library's own group whatever the user
/// has renamed it to, lined up with the entries around them.
fn add_beside(
    text: &str,
    missing: &[(String, String)],
    library: &[String],
    newline: &str,
) -> String {
    let mut updated = text.to_string();

    for container in GROUPS.captures_iter(text).collect::<Vec<_>>().iter().rev() {
        let body = container.get(2).expect("a body");
        let mut last = None;

        for entry in FILE_ELEMENT.find_iter(&text[..body.end()]) {
            if entry.start() >= body.start()
                && entry_path(entry.as_str()).is_some_and(|path| library.contains(&path))
            {
                last = Some(entry);
            }
        }

        let Some(last) = last else {
            continue;
        };

        let (pad, step) = element_indent(last.as_str());
        let block: String = missing
            .iter()
            .flat_map(|(name, path)| file_entry(name, path, &pad, &step))
            .map(|line| format!("{line}{newline}"))
            .collect();

        updated.insert_str(last.end(), &block);
    }

    updated
}

/// Bring the C define list of every target in step with the library.
///
/// uVision separates them with commas, as CubeMX writes them, or with spaces, and reads
/// either. What is added uses whichever the list already has.
///
/// Returns the new text, the defines left alone because the list already sets the same name
/// to something else, and whether any target had a list at all.
fn edit_defines(
    text: &str,
    wanted: &[String],
    dropped: &[String],
) -> (String, Vec<(String, String)>, bool) {
    let mut clashes = Vec::new();
    let mut found_any = false;
    let mut out = String::with_capacity(text.len());
    let mut last = 0;

    for block in CADS.find_iter(text) {
        let Some(matched) = DEFINE.captures(block.as_str()) else {
            continue;
        };

        found_any = true;

        let body = matched.get(2).expect("a body");
        let written = body.as_str();
        let present: Vec<String> = DEFINE_SPLIT
            .split(written)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
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

        let sep = if written.contains(", ") {
            ", "
        } else if written.contains(',') || !written.trim().contains(' ') {
            ","
        } else {
            " "
        };

        let mut kept: Vec<String> = present
            .iter()
            .filter(|part| !plan.remove.contains(part))
            .cloned()
            .collect();
        kept.extend(plan.add);

        out.push_str(&text[last..block.start() + body.start()]);
        out.push_str(&kept.join(sep));
        last = block.start() + body.end();
    }

    out.push_str(&text[last..]);

    (out, clashes, found_any)
}

/// Register the library's sources, include path and defines with a Keil project.
///
/// `dropped` lists the library files an update has just removed, so the entries that named
/// them can follow the file to its new place or go, and its dropped defines are taken out of
/// the define lists.
pub fn integrate(
    uvprojx: &Path,
    library: &Manifest,
    destination: &Path,
    dropped: &Dropped,
) -> io::Result<Outcome> {
    let name = uvprojx
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // Paths inside a .uvprojx are relative to the folder holding it.
    let parent = uvprojx.parent().unwrap_or(Path::new("."));
    let folder = relative(destination, parent);

    let text = match std::fs::read(uvprojx) {
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

    let containers = GROUPS.captures_iter(&text).count();

    if !INCLUDE_PATH.is_match(&text) || containers == 0 {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("Could not find the file groups or include paths in {name}."),
        )
        .with_steps(manual_steps(&folder)));
    }

    let newline = line_ending(&text);
    let sep = separator(&text);
    let sources = library.build_sources();
    let new_paths = file_paths(&folder, &sources, sep);

    // The bare name comes from the manifest entry, which always uses forward slashes.
    let found: Vec<(String, String)> = sources
        .iter()
        .map(|source| paths::leaf(source).to_string())
        .zip(new_paths.iter().cloned())
        .collect();

    // An update first. Entries for files that moved follow them, and entries for files that
    // are gone are removed, so what is missing after this is only what the project has
    // never had.
    let dropped_paths = file_paths(&folder, &compiled_names(&dropped.files, destination), sep);
    let listed: Vec<String> = FILE_PATH
        .captures_iter(&text)
        .map(|matched| forward_slashes(&matched[1]))
        .collect();
    let was_there = new_paths
        .iter()
        .chain(&dropped_paths)
        .any(|path| listed.contains(&forward_slashes(path)));
    let (mut updated, missing) = follow(&text, &dropped_paths, &found);

    // A header only library has nothing to compile, so it gets an include path and no group.
    // An empty group would be added again on every run, since there would be no file in the
    // project to recognise it by.
    if !found.is_empty() && missing.len() == found.len() {
        // The group is appended after the target's own groups, which is where uVision puts
        // one added through Manage Project Items. Walking backwards keeps each insertion
        // from moving the offsets of the next.
        let before = updated.clone();

        for container in GROUPS
            .captures_iter(&before)
            .collect::<Vec<_>>()
            .iter()
            .rev()
        {
            let whole = container.get(0).expect("the whole match");
            let outer = indent_of(&before, whole.start());
            let inner = inner_indent(&container[2], &outer);
            let step = indent_step(&outer, &inner);
            let at = insert_before(&updated, container.get(3).expect("the close").start());
            let block = group(&library.name, &found, &inner, &step, newline);

            updated.insert_str(at, &format!("{newline}{block}"));
        }
    } else if !missing.is_empty() {
        // The library is in the project and this version added files to it.
        let ours: Vec<String> = found
            .iter()
            .map(|(_, path)| forward_slashes(path))
            .collect();
        updated = add_beside(&updated, &missing, &ours, newline);
    }

    let directories: Vec<String> = include_folders(library, &folder.replace('/', sep))
        .iter()
        .map(|dir| dir.replace('/', sep))
        .collect();

    let updated = INCLUDE_PATH
        .replace_all(&updated, |matched: &Captures| {
            let mut parts: Vec<String> = matched[2]
                .split(';')
                .filter(|part| !part.trim().is_empty())
                .map(str::to_string)
                .collect();
            let present: Vec<String> = parts.iter().map(|part| forward_slashes(part)).collect();

            parts.extend(
                directories
                    .iter()
                    .filter(|dir| !present.contains(&forward_slashes(dir)))
                    .cloned(),
            );

            format!("{}{}{}", &matched[1], parts.join(";"), &matched[3])
        })
        .into_owned();

    let (updated, clashes, has_list) = edit_defines(&updated, &library.defines, &dropped.defines);
    let mut steps = clash_steps(&clashes, &name);

    // Without a define list the library would build, but with its defaults instead of the
    // settings it reads through the define, and nothing would say so. So it is said here.
    let unplaced = !library.defines.is_empty() && !has_list;

    if unplaced {
        steps.extend(define_steps(&library.defines));
    }

    if updated == text && unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("{name} has {}, but no define list to add to.", library.name),
        )
        .with_steps(steps));
    }

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("{name} already has {}.", library.name),
        )
        .with_steps(steps));
    }

    let saved = backup(uvprojx)?;

    if let Err(error) = files::write(uvprojx, &updated) {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("Could not write {name}: {}", files::reason(&error)),
        )
        .with_steps(manual_steps(&folder))
        .with_backup(Some(saved)));
    }

    let message = if was_there {
        format!("Updated {} in {name}", library.name)
    } else {
        format!("Added {} to {name} in {containers} target(s)", library.name)
    };

    if unplaced {
        return Ok(Outcome::new(
            NAME,
            Status::Manual,
            format!("{message}, but found no define list to add to."),
        )
        .with_steps(steps)
        .with_backup(Some(saved)));
    }

    steps.push("Close and reopen the project in uVision so it reloads the file list.".to_string());

    Ok(Outcome::new(NAME, Status::Changed, format!("{message}."))
        .with_steps(steps)
        .with_backup(Some(saved)))
}

/// What to click, when this could not do it safely.
pub fn manual_steps(folder: &str) -> Vec<String> {
    let windows_folder = folder.replace('/', "\\");

    vec![
        "In uVision: right click the target, Manage Project Items,".to_string(),
        format!("add a group and add the .c files from {windows_folder} to it."),
        "Then Options for Target, C/C++, Include Paths,".to_string(),
        format!("and add {windows_folder}."),
    ]
}

/// What to click to add the library's defines by hand.
fn define_steps(defines: &[String]) -> Vec<String> {
    vec![
        "In uVision: Options for Target, C/C++, Preprocessor Symbols, and add to Define,"
            .to_string(),
        "for every target:".to_string(),
        format!("    {}", defines.join(",")),
    ]
}

/// Take the library's files, its group, include path and defines out of a Keil project.
pub fn remove(uvprojx: &Path, library: &Installed) -> io::Result<Outcome> {
    let name = uvprojx
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let parent = uvprojx.parent().unwrap_or(Path::new("."));
    let folder = relative(library.destination.as_deref().unwrap_or(parent), parent);
    let text = files::read(uvprojx)?;

    // Every file of the library's folder, in every target, wherever the user has moved its
    // entry since.
    let updated = FILE_ELEMENT.replace_all(&text, |matched: &Captures| {
        if under(&entry_path(&matched[0]).unwrap_or_default(), &folder) {
            String::new()
        } else {
            matched[0].to_string()
        }
    });

    // The group it leaves empty, when it still has the library's name. One the user renamed
    // is theirs to remove.
    let empty_group = Regex::new(&format!(
        r"(?m)^[ \t]*<Group>\s*<GroupName>{}</GroupName>\s*<Files>\s*</Files>\s*</Group>[ \t]*\r?\n",
        regex::escape(&library.name)
    ))
    .expect("a valid pattern");
    let updated = empty_group.replace_all(&updated, "");

    let updated = INCLUDE_PATH.replace_all(&updated, |matched: &Captures| {
        let kept: Vec<&str> = matched[2]
            .split(';')
            .filter(|part| part.trim().is_empty() || !under(part, &folder))
            .collect();

        format!("{}{}{}", &matched[1], kept.join(";"), &matched[3])
    });

    let (updated, _, _) = edit_defines(&updated, &[], &library.defines);

    if updated == text {
        return Ok(Outcome::new(
            NAME,
            Status::Already,
            format!("{name} has nothing of {}.", library.name),
        ));
    }

    let saved = backup(uvprojx)?;
    files::write(uvprojx, &updated)?;

    Ok(Outcome::new(
        NAME,
        Status::Changed,
        format!("Removed {} from {name}.", library.name),
    )
    .with_steps(vec![
        "Close and reopen the project in uVision so it reloads the file list.".to_string(),
    ])
    .with_backup(Some(saved)))
}
