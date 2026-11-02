//! What a library's file means, as the window shows it beside the file, and how to put right what is wrong with it.
//!
//! The reader says whether a file is good. This says what it comes to: what would be installed, what is missing, what is in
//! the folder and not listed, each with the line it stands on. It works on the text being edited, so it reads what it can and
//! never stops at the first problem.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::check::{Problem, lint_text};
use crate::manifest::{self, COMPILED};
use crate::paths;
use crate::platform::Platform;
use crate::scaffold;
use crate::schema::{self, Shape};
use crate::yaml::{self, Entry, Node, Value};
use crate::ymledit::{self, Step, to_json};

/// One entry of a list of files.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listed {
    /// As the file gives it.
    pub given: String,
    pub line: usize,
    /// The files it comes to.
    pub files: Vec<String>,
    /// It names nothing that is in the folder.
    pub missing: bool,
}

/// A list of files of the library.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileList {
    pub key: String,
    pub label: String,
    pub entries: Vec<Listed>,
}

/// A platform the file names, and why it would not install on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlatformState {
    pub name: &'static str,
    pub label: &'static str,
    pub problem: Option<String>,
    pub line: Option<usize>,
}

/// A file of the folder's code that no list names.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Unlisted {
    pub path: String,
    /// The list it would go on.
    pub belongs: &'static str,
}

/// What a library's file comes to.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct Outline {
    /// The file as data, when it reads.
    pub data: Option<Json>,
    /// Why it does not read.
    pub error: Option<Problem>,
    /// The line of each key and item, by its steps joined with dots.
    pub lines: BTreeMap<String, usize>,
    /// Every file of the folder worth offering.
    pub files: Vec<String>,
    pub name: String,
    pub version: String,
    /// Where the version comes from.
    pub version_from: String,
    /// What to #include.
    pub include: String,
    pub platforms: Vec<PlatformState>,
    pub lists: Vec<FileList>,
    pub unlisted: Vec<Unlisted>,
    pub options: Vec<String>,
    pub steps: Vec<String>,
    pub needs: Vec<String>,
    pub peripherals: Vec<String>,
}

fn note_lines(node: &Node, path: &str, lines: &mut BTreeMap<String, usize>) {
    let joined = |step: &str| {
        if path.is_empty() {
            step.to_string()
        } else {
            format!("{path}.{step}")
        }
    };

    match &node.value {
        Value::Map(map) => {
            for entry in &map.entries {
                let here = joined(&entry.key);

                lines.insert(here.clone(), entry.line);
                note_lines(&entry.value, &here, lines);
            }
        }
        Value::List(items) => {
            for (index, item) in items.iter().enumerate() {
                let here = joined(&index.to_string());

                lines.insert(here.clone(), item.line);
                note_lines(item, &here, lines);
            }
        }
        _ => {}
    }
}

/// What an entry of a list of files comes to.
fn listed(root: &Path, item: &Node) -> Listed {
    let given = match item.as_map() {
        Some(map) => map
            .given("from")
            .or_else(|| map.given("root"))
            .or_else(|| map.given("to"))
            .map(Node::text)
            .unwrap_or_default(),
        None if item.is_null() => String::new(),
        None => item.text(),
    };
    let files = if given.is_empty() {
        Vec::new()
    } else {
        manifest::expand(root, &given)
    };

    Listed {
        missing: files.is_empty(),
        files,
        given,
        line: item.line,
    }
}

fn file_list(root: &Path, data: &yaml::Mapping, key: &str, label: String) -> Option<FileList> {
    let items = data.given(key)?.as_list()?;

    Some(FileList {
        key: key.to_string(),
        label,
        entries: items.iter().map(|item| listed(root, item)).collect(),
    })
}

fn is_code(path: &str) -> Option<&'static str> {
    let suffix = paths::suffix(path);

    if matches!(suffix.as_str(), ".h" | ".hh" | ".hpp" | ".hxx" | ".inc") {
        Some("headers")
    } else if COMPILED.contains(&suffix.as_str()) {
        Some("sources")
    } else {
        None
    }
}

/// What a library's file comes to, read against the library's folder.
pub fn outline(folder: &Path, path: &Path, text: &str) -> Outline {
    let files = scaffold::files_of(folder);
    let mut found = Outline {
        files,
        ..Outline::default()
    };

    // A file with nothing in it yet is no error, only a file to fill in.
    if matches!(yaml::loads(text), Ok(None)) {
        found.data = Some(json!({}));
        return found;
    }

    let root = match manifest::parse(text) {
        Ok(root) => root,
        Err(error) => {
            found.error = Some(Problem {
                line: error.line,
                message: error.message,
                warning: false,
            });
            return found;
        }
    };
    let data = root.as_map().expect("parse() gives a mapping");

    found.data = Some(to_json(&root));
    note_lines(&root, "", &mut found.lines);
    found.name = data.given("name").map(Node::text).unwrap_or_default();

    // Every list of files, an option's among them.
    for (key, label) in [
        ("headers", "Headers"),
        ("sources", "Sources"),
        ("extras", "Extras"),
        ("once", "The project's own"),
    ] {
        found
            .lists
            .extend(file_list(folder, data, key, label.to_string()));
    }

    if let Some(options) = data.given("options").and_then(Node::as_map) {
        for option in &options.entries {
            found.options.push(option.key.clone());

            if let Some(body) = option.value.as_map() {
                for (key, label) in [("headers", "headers"), ("sources", "sources")] {
                    if let Some(mut list) =
                        file_list(folder, body, key, format!("{} {label}", option.key))
                    {
                        list.key = format!("options.{}.{key}", option.key);
                        found.lists.push(list);
                    }
                }

                for library in body
                    .given("libraries")
                    .and_then(Node::as_list)
                    .unwrap_or_default()
                {
                    let name = need_text(library);

                    if !found.needs.contains(&name) {
                        found.needs.push(name);
                    }
                }
            }
        }
    }

    // What is in the folder's code and not listed.
    let named: Vec<&String> = found
        .lists
        .iter()
        .flat_map(|list| &list.entries)
        .flat_map(|entry| &entry.files)
        .collect();

    found.unlisted = found
        .files
        .iter()
        .filter(|file| !named.contains(file) && !is_about_the_code(file))
        .filter_map(|file| {
            is_code(file).map(|belongs| Unlisted {
                path: file.clone(),
                belongs,
            })
        })
        .collect();

    // The version, from where the reader takes it.
    let first_header = found
        .lists
        .first()
        .filter(|list| list.key == "headers")
        .and_then(|list| list.entries.first())
        .and_then(|entry| entry.files.first())
        .cloned();

    if let Some(first) = &first_header {
        found.include = paths::leaf(first).to_string();

        if let Some(version) = std::fs::read(folder.join(first))
            .ok()
            .and_then(|data| manifest::version_in(&String::from_utf8_lossy(&data)))
        {
            found.version = version;
            found.version_from = format!("the @version tag in {first}");
        }
    }

    if found.version.is_empty() {
        found.version = data.given("version").map(Node::text).unwrap_or_default();

        if !found.version.is_empty() {
            found.version_from = "version in this file".to_string();
        }
    }

    if let Some(steps) = data.given("run").and_then(Node::as_list) {
        found.steps = steps
            .iter()
            .filter_map(|step| step.as_map()?.given("label").map(Node::text))
            .collect();
    }

    if let Some(requires) = data.given("requires").and_then(Node::as_map) {
        for library in requires
            .given("libraries")
            .and_then(Node::as_list)
            .unwrap_or_default()
        {
            found.needs.insert(0, need_text(library));
        }

        for peripheral in requires
            .given("peripherals")
            .and_then(Node::as_list)
            .unwrap_or_default()
        {
            let kind = match peripheral.as_map() {
                Some(map) => map.given("type").map(Node::text).unwrap_or_default(),
                None => peripheral.text(),
            };

            found.peripherals.push(kind);
        }
    }

    // Each platform it names, read the way an install on it reads it.
    if let Ok(Some(platforms)) = manifest::platforms_of(data) {
        for platform in platforms {
            let problem = manifest::load_text(folder, path, text, Some(platform)).err();

            found.platforms.push(PlatformState {
                name: platform.name(),
                label: platform.label(),
                line: problem.as_ref().and_then(|problem| problem.line),
                problem: problem.map(|problem| strip_file(&problem.message, path)),
            });
        }
    }

    found
}

fn need_text(library: &Node) -> String {
    match library.as_map() {
        Some(map) => map.given("source").map(Node::text).unwrap_or_default(),
        None => library.text(),
    }
}

fn strip_file(message: &str, path: &Path) -> String {
    let in_file = format!("{}: ", path.display());

    message
        .strip_prefix(&in_file)
        .unwrap_or(message)
        .to_string()
}

fn is_about_the_code(path: &str) -> bool {
    path.split('/').rev().skip(1).any(|part| {
        matches!(
            part.to_lowercase().as_str(),
            "doc"
                | "docs"
                | "example"
                | "examples"
                | "extras"
                | "scripts"
                | "test"
                | "tests"
                | "tools"
        )
    })
}

/// A way to put a problem right: what it says on its button, and the whole of the file with it done.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fix {
    pub label: String,
    pub text: String,
}

/// A problem in a file, with the ways to put it right that are known.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Advice {
    pub line: Option<usize>,
    pub message: String,
    pub warning: bool,
    pub fixes: Vec<Fix>,
}

/// Keys that have moved, from an installer.yml or from before this format, and where they went.
const MOVED: [(&[&str], &[&str]); 12] = [
    (&["requires", "lib_installer"], &["requires", "libnima"]),
    (&["requires", "hal"], &["stm32", "requires", "hal"]),
    (&["requires", "cmsis"], &["stm32", "requires", "cmsis"]),
    (&["requires", "rtos"], &["stm32", "requires", "rtos"]),
    (
        &["requires", "hal_modules"],
        &["stm32", "requires", "hal_modules"],
    ),
    (
        &["requires", "c_standard"],
        &["stm32", "requires", "c_standard"],
    ),
    (&["files", "headers"], &["headers"]),
    (&["files", "sources"], &["sources"]),
    (&["install", "defines"], &["defines"]),
    (&["install", "include_dirs"], &["include_dirs"]),
    (&["esp32", "requires"], &["esp32", "library", "requires"]),
    (&["zephyr", "defaults"], &["zephyr", "library", "kconfig"]),
];

fn to_steps(path: &[&str]) -> Vec<Step> {
    path.iter().map(|key| Step::from(*key)).collect()
}

/// A word on a line swapped for another, the first time it stands there.
fn on_line(text: &str, line: usize, old: &str, new: &str) -> Option<String> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    let at = lines.get(line.checked_sub(1)?)?;
    let changed = at.replacen(old, new, 1);

    if changed == **at {
        return None;
    }

    lines[line - 1] = &changed;

    let made = lines.join("\n");

    yaml::loads(&made).ok()?;

    Some(made)
}

/// Every problem in a file, with what can be done about it. `fallback` is the platform for a file that names none.
pub fn advise(folder: &Path, path: &Path, text: &str, fallback: Option<Platform>) -> Vec<Advice> {
    let mut found: Vec<Advice> = lint_text(folder, path, text)
        .into_iter()
        .map(|problem| Advice {
            line: problem.line,
            message: problem.message,
            warning: problem.warning,
            fixes: Vec::new(),
        })
        .collect();

    let Ok(root) = manifest::parse(text) else {
        return found;
    };
    let data = root.as_map().expect("parse() gives a mapping");

    move_old_keys(text, &root, &mut found);
    key_fixes(text, &root, &mut found);
    missing_file_fixes(folder, text, &root, &mut found);
    missing_key_fixes(folder, text, data, fallback, &mut found);

    found
}

/// A fix for the problem it belongs to: the one on that line, or one that says this, or one of its own.
fn attach(
    found: &mut Vec<Advice>,
    line: usize,
    says: &str,
    message: String,
    warning: bool,
    fix: Option<Fix>,
) {
    let at = found
        .iter()
        .position(|advice| advice.line == Some(line))
        .or_else(|| {
            found
                .iter()
                .position(|advice| advice.line.is_none() && advice.message.contains(says))
        });

    match at {
        Some(at) => {
            found[at].line = Some(line);
            found[at].fixes.extend(fix);
        }
        None => found.push(Advice {
            line: Some(line),
            message,
            warning,
            fixes: fix.into_iter().collect(),
        }),
    }
}

fn move_old_keys(text: &str, root: &Node, found: &mut Vec<Advice>) {
    let old = to_json(root);
    let mut made = text.to_string();
    let mut first = None;

    for (from, to) in MOVED {
        let mut here = &old;
        let mut there = true;

        for key in from {
            match here.get(*key) {
                Some(next) => here = next,
                None => there = false,
            }

            if !there {
                break;
            }
        }

        if !there {
            continue;
        }

        let moved = ymledit::set(&made, &to_steps(to), Some(here))
            .and_then(|with| ymledit::set(&with, &to_steps(from), None));

        if let Ok(moved) = moved {
            made = moved;
            first.get_or_insert(from);
        }
    }

    if let Some(from) = first
        && made != text
    {
        let fix = Fix {
            label: "Move the old keys to where they went".to_string(),
            text: made,
        };
        let line = line_of(root, from).unwrap_or(1);

        attach(
            found,
            line,
            from[from.len() - 1],
            "It has keys from before this format.".to_string(),
            false,
            Some(fix),
        );
    }
}

fn line_of(root: &Node, path: &[&str]) -> Option<usize> {
    let mut node = root;
    let mut line = None;

    for key in path {
        let map = node.as_map()?;

        line = map.line_of(key);
        node = map.get(key)?;
    }

    line
}

/// The path above a table's path.
fn holder_of(path: &str) -> &str {
    path.rfind('.').map_or("", |at| &path[..at])
}

fn key_fixes(text: &str, root: &Node, found: &mut Vec<Advice>) {
    let mut pending: Vec<(usize, String, String, bool, Option<Fix>)> = Vec::new();

    schema::walk_keys(root, "", &mut |path: &str, entry: &Entry| {
        let holder = holder_of(path);

        // A name the author gave, or what is below a list of lines, is the author's to choose.
        if path.ends_with(".*") {
            check_choice(text, path, entry, &mut pending);
            return;
        }

        match schema::find(path) {
            Some(_) => check_choice(text, path, entry, &mut pending),
            None => {
                let holder_key = schema::find(holder);

                if holder_key.is_some_and(|key| matches!(key.shape, Shape::Lines | Shape::Text)) {
                    return;
                }

                let names: Vec<&str> = schema::children(holder)
                    .iter()
                    .map(|key| key.path.rsplit('.').next().unwrap_or(key.path))
                    .collect();

                let fix = schema::nearest(&entry.key, names.iter().copied())
                    .filter(|name| !names_taken(root, holder, name))
                    .and_then(|name| {
                        on_line(text, entry.line, &entry.key, name).map(|made| Fix {
                            label: format!("Change it to {name}"),
                            text: made,
                        })
                    });

                // At the top a key it does not know only leaves it out, and below it a key is refused.
                pending.push((
                    entry.line,
                    entry.key.clone(),
                    format!("{} is no key libNIMA reads here.", entry.key),
                    holder.is_empty(),
                    fix,
                ));
            }
        }
    });

    for (line, says, message, warning, fix) in pending {
        attach(found, line, &says, message, warning, fix);
    }
}

/// Whether a key is given already, beside the one that was misspelt.
fn names_taken(root: &Node, holder: &str, name: &str) -> bool {
    holder.is_empty() && root.as_map().is_some_and(|map| map.has(name))
}

fn check_choice(
    text: &str,
    path: &str,
    entry: &Entry,
    pending: &mut Vec<(usize, String, String, bool, Option<Fix>)>,
) {
    let Some(key) = schema::find(path) else {
        return;
    };

    if key.shape != Shape::Choice || key.open || key.choices.is_empty() {
        return;
    }

    let value = entry.value.text();

    if key.choices.contains(&value.as_str())
        || key
            .choices
            .iter()
            .any(|choice| choice.eq_ignore_ascii_case(&value))
    {
        return;
    }

    let fix = schema::nearest(&value, key.choices.iter().copied()).and_then(|choice| {
        on_line(text, entry.value.line.max(entry.line), &value, choice).map(|made| Fix {
            label: format!("Change it to {choice}"),
            text: made,
        })
    });

    pending.push((
        entry.line,
        format!("'{value}' is not one of"),
        format!(
            "{} is {value}, and it is one of: {}.",
            entry.key,
            key.choices.join(", ")
        ),
        true,
        fix,
    ));
}

/// Files a list names that the folder does not have, each with the nearest it does.
fn missing_file_fixes(folder: &Path, text: &str, root: &Node, found: &mut Vec<Advice>) {
    let data = root.as_map().expect("parse() gives a mapping");
    let files = scaffold::files_of(folder);
    let mut lists: Vec<(&str, &[Node])> = Vec::new();

    for key in ["headers", "sources"] {
        if let Some(items) = data.given(key).and_then(Node::as_list) {
            lists.push((key, items));
        }
    }

    if let Some(options) = data.given("options").and_then(Node::as_map) {
        for option in &options.entries {
            if let Some(body) = option.value.as_map() {
                for key in ["headers", "sources"] {
                    if let Some(items) = body.given(key).and_then(Node::as_list) {
                        lists.push((key, items));
                    }
                }
            }
        }
    }

    for (key, items) in lists {
        let offered = schema::files_for(key, &files);

        for item in items {
            let entry = listed(folder, item);

            if !entry.missing || manifest::is_pattern(&entry.given) {
                continue;
            }

            let leaf = paths::leaf(&entry.given);
            let near = offered
                .iter()
                .find(|file| paths::leaf(file) == leaf)
                .map(String::as_str)
                .or_else(|| schema::nearest(&entry.given, offered.iter().map(String::as_str)));
            let fix = near.and_then(|near| {
                on_line(text, item.line, &entry.given, near).map(|made| Fix {
                    label: format!("Change it to {near}"),
                    text: made,
                })
            });

            attach(
                found,
                item.line,
                &entry.given,
                format!("{} is not in the library's folder.", entry.given),
                false,
                fix,
            );
        }
    }
}

/// What every file has to have, from what the folder says.
fn missing_key_fixes(
    folder: &Path,
    text: &str,
    data: &yaml::Mapping,
    fallback: Option<Platform>,
    found: &mut [Advice],
) {
    let with = |at: &[&str], value: Json| ymledit::set(text, &to_steps(at), Some(&value)).ok();
    let scanned = scaffold::scan(folder, fallback);

    for advice in found.iter_mut() {
        if advice.message.contains("required 'name'") && !data.has("name") {
            advice
                .fixes
                .extend(with(&["name"], json!(scanned.name)).map(|text| Fix {
                    label: format!("Name it {}", scanned.name),
                    text,
                }));
        }

        if advice.message.contains("names no platform") && !data.has("platform") {
            let names: Vec<&str> = scanned
                .platforms
                .iter()
                .map(|platform| platform.name())
                .collect();
            let value = match names.as_slice() {
                [] => None,
                [only] => Some(json!(only)),
                many => Some(json!(many)),
            };

            advice.fixes.extend(value.and_then(|value| {
                with(&["platform"], value).map(|text| Fix {
                    label: format!("It is for {}", names.join(", ")),
                    text,
                })
            }));
        }

        if advice.message.contains("headers and its sources")
            || advice.message.contains("no headers and no sources")
        {
            let mut made = text.to_string();

            for (key, list) in [("headers", &scanned.headers), ("sources", &scanned.sources)] {
                if !list.is_empty()
                    && let Ok(with_it) = ymledit::set(&made, &to_steps(&[key]), Some(&json!(list)))
                {
                    made = with_it;
                }
            }

            if made != text {
                advice.fixes.push(Fix {
                    label: "List the files in the folder".to_string(),
                    text: made,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();

        for (name, text) in files {
            let path = root.path().join(name);

            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        root
    }

    const FILES: &[(&str, &str)] = &[
        ("src/ee24.h", "/**\n * @version 4.0.0\n */\n"),
        ("src/ee24_config.h", ""),
        ("src/ee24_port.h", ""),
        ("src/ee24.c", ""),
        ("tests/t.c", ""),
        ("README.md", ""),
    ];

    const FILE: &str = "name: ee24\nplatform: [stm32, zephyr]\nrequires:\n  libraries:\n    - osal >= 1.0.0\n  peripherals:\n    - type: i2c\n\
                        headers:\n  - src/ee24.h\n  - src/ee24_config.h\nsources:\n  - src/ee24.c\n\
                        options:\n  lfs:\n    description: x\n    libraries:\n      - littlefs\n\
                        run:\n  - label: key\n    command: [make]\n";

    fn read(root: &Path, text: &str) -> Outline {
        outline(root, &root.join("libnima.yml"), text)
    }

    #[test]
    fn it_says_what_the_file_comes_to() {
        let root = folder(FILES);
        let found = read(root.path(), FILE);

        assert_eq!(found.error, None);
        assert_eq!(found.name, "ee24");
        assert_eq!(found.version, "4.0.0");
        assert_eq!(found.version_from, "the @version tag in src/ee24.h");
        assert_eq!(found.include, "ee24.h");
        assert_eq!(found.options, ["lfs"]);
        assert_eq!(found.steps, ["key"]);
        assert_eq!(found.needs, ["osal >= 1.0.0", "littlefs"]);
        assert_eq!(found.peripherals, ["i2c"]);
        assert_eq!(found.lists[0].key, "headers");
        assert_eq!(found.lists[0].entries[1].given, "src/ee24_config.h");
        assert_eq!(found.lists[0].entries[1].line, 10);
        assert!(!found.lists[0].entries[1].missing);
        assert_eq!(found.lines["run.0.label"], 19);
        assert_eq!(found.lines["requires.libraries.0"], 5);
        assert_eq!(found.data.as_ref().unwrap()["name"], "ee24");

        let names: Vec<&str> = found.platforms.iter().map(|one| one.name).collect();

        assert_eq!(names, ["stm32", "zephyr"]);
        assert!(
            found.platforms.iter().all(|one| one.problem.is_none()),
            "{:#?}",
            found.platforms
        );
    }

    #[test]
    fn what_is_in_the_folder_and_not_listed_is_said() {
        let root = folder(FILES);
        let found = read(root.path(), FILE);

        assert_eq!(
            found.unlisted,
            [Unlisted {
                path: "src/ee24_port.h".to_string(),
                belongs: "headers"
            }],
            "not the tests, and not what is no code"
        );
    }

    #[test]
    fn what_is_listed_and_not_in_the_folder_is_said() {
        let root = folder(FILES);
        let found = read(
            root.path(),
            "name: x\nplatform: stm32\nheaders:\n  - src/nope.h\n  - src/*.h\n",
        );
        let entries = &found.lists[0].entries;

        assert!(entries[0].missing);
        assert_eq!(entries[0].line, 4);
        assert!(!entries[1].missing);
        assert_eq!(entries[1].files.len(), 3);
        assert!(
            found.platforms[0]
                .problem
                .as_ref()
                .is_some_and(|problem| problem.contains("nope.h")),
            "{:#?}",
            found.platforms
        );
    }

    #[test]
    fn an_item_with_nothing_in_it_yet_names_no_file() {
        let root = folder(FILES);
        let found = read(
            root.path(),
            "name: x\nplatform: stm32\nheaders:\n  - src/ee24.h\nsources:\n  -\n",
        );
        let entry = &found.lists[1].entries[0];

        assert_eq!(entry.given, "");
        assert!(entry.missing);
    }

    #[test]
    fn a_version_can_come_from_the_file() {
        let root = folder(&[("a.h", "")]);
        let found = read(
            root.path(),
            "name: x\nplatform: stm32\nversion: 1.2.3\nheaders:\n  - a.h\n",
        );

        assert_eq!(found.version, "1.2.3");
        assert_eq!(found.version_from, "version in this file");
    }

    #[test]
    fn a_file_that_does_not_read_says_why_and_still_offers_the_folder() {
        let root = folder(FILES);
        let found = read(root.path(), "name: [\n");

        assert_eq!(found.data, None);
        assert!(found.error.is_some());
        assert!(found.files.contains(&"src/ee24.h".to_string()));
    }

    #[test]
    fn an_empty_file_is_only_a_file_with_nothing_in_it_yet() {
        let root = folder(FILES);
        let found = read(root.path(), "");

        assert_eq!(found.data, Some(json!({})));
        assert_eq!(found.error, None);
    }

    fn advised(root: &Path, text: &str) -> Vec<Advice> {
        advise(root, &root.join("libnima.yml"), text, Some(Platform::Stm32))
    }

    fn fix_of<'a>(found: &'a [Advice], says: &str) -> &'a Fix {
        found
            .iter()
            .find_map(|advice| advice.fixes.iter().find(|fix| fix.label.contains(says)))
            .unwrap_or_else(|| panic!("no fix saying {says}: {found:#?}"))
    }

    #[test]
    fn a_good_file_has_no_advice() {
        let root = folder(FILES);

        assert_eq!(advised(root.path(), FILE), []);
    }

    #[test]
    fn a_misspelt_key_at_the_top_is_offered_the_right_one_with_its_line() {
        let root = folder(FILES);
        let text = "name: x\nplatform: stm32\nheaders:\n  - src/ee24.h\nsource:\n  - src/ee24.c\n";
        let found = advised(root.path(), text);
        let fix = fix_of(&found, "Change it to sources");

        assert_eq!(found[0].line, Some(5));
        assert!(found[0].warning);
        assert!(
            fix.text.contains("\nsources:\n  - src/ee24.c\n"),
            "{}",
            fix.text
        );
    }

    #[test]
    fn a_misspelt_key_in_a_section_is_offered_the_right_one() {
        let root = folder(FILES);
        let text = "name: x\nplatform: stm32\nheaders:\n  - src/ee24.h\nstm32:\n  requires:\n    hal_module: [spi]\n";
        let found = advised(root.path(), text);
        let fix = fix_of(&found, "Change it to hal_modules");

        assert_eq!(found[0].line, Some(7));
        assert!(
            fix.text.contains("    hal_modules: [spi]\n"),
            "{}",
            fix.text
        );
    }

    #[test]
    fn a_word_that_is_not_among_the_choices_is_offered_the_nearest() {
        let root = folder(FILES);
        let text = "name: x\nplatform: stm32\nkind: drivr\nheaders:\n  - src/ee24.h\n";
        let found = advised(root.path(), text);
        let fix = fix_of(&found, "Change it to driver");

        assert_eq!(found[0].line, Some(3));
        assert!(fix.text.contains("kind: driver\n"), "{}", fix.text);
    }

    #[test]
    fn a_file_that_is_not_there_is_offered_the_nearest_that_is() {
        let root = folder(FILES);
        let text = "name: x\nplatform: stm32\nheaders:\n  - inc/ee24.h\n  - src/ee24_confg.h\n";
        let found = advised(root.path(), text);

        assert!(
            fix_of(&found, "src/ee24.h")
                .text
                .contains("  - src/ee24.h\n")
        );
        assert!(
            fix_of(&found, "src/ee24_config.h")
                .text
                .contains("  - src/ee24_config.h\n")
        );
        assert_eq!(
            found.iter().filter(|advice| advice.line == Some(5)).count(),
            1,
            "{found:#?}"
        );
    }

    #[test]
    fn what_every_file_has_to_have_is_offered_from_the_folder() {
        let root = folder(&[("src/x.h", ""), ("src/x.c", "")]);
        let found = advised(root.path(), "platform: stm32\nheaders:\n  - src/x.h\n");
        let fix = fix_of(&found, "Name it");

        assert!(
            fix.text.contains("\nname: ") || fix.text.starts_with("name: "),
            "{}",
            fix.text
        );

        let found = advised(root.path(), "name: x\nheaders:\n  - src/x.h\n");

        assert!(
            fix_of(&found, "It is for stm32")
                .text
                .contains("platform: stm32\n")
        );

        let found = advised(root.path(), "name: x\nplatform: stm32\n");
        let fix = fix_of(&found, "List the files");

        assert!(fix.text.contains("headers:\n  - src/x.h\n"), "{}", fix.text);
        assert!(fix.text.contains("sources:\n  - src/x.c\n"), "{}", fix.text);
    }

    #[test]
    fn old_keys_are_offered_their_new_places_together() {
        let root = folder(FILES);
        let text = "name: x\nplatform: stm32\nrequires:\n  lib_installer: 4.0.0\n  hal: true\nfiles:\n  headers:\n    - src/ee24.h\n";
        let found = advised(root.path(), text);
        let fix = fix_of(&found, "Move the old keys");

        assert!(!fix.text.contains("lib_installer"), "{}", fix.text);
        assert!(fix.text.contains("  libnima: 4.0.0\n"), "{}", fix.text);
        assert!(
            fix.text.contains("stm32:\n  requires:\n    hal: true\n"),
            "{}",
            fix.text
        );
        assert!(
            fix.text.contains("headers:\n  - src/ee24.h\n"),
            "{}",
            fix.text
        );
        assert!(!fix.text.contains("files:"), "{}", fix.text);
    }

    #[test]
    fn every_fix_it_offers_gets_rid_of_what_it_is_for() {
        let root = folder(FILES);

        for text in [
            "name: x\nplatform: stm32\nheaders:\n  - src/ee24.h\nsource:\n  - src/ee24.c\n",
            "name: x\nplatform: stm32\nkind: drivr\nheaders:\n  - src/ee24.h\n",
            "name: x\nplatform: stm32\nheaders:\n  - inc/ee24.h\n",
            "platform: stm32\nheaders:\n  - src/ee24.h\n",
            "name: x\nheaders:\n  - src/ee24.h\n",
            "name: x\nplatform: stm32\n",
            "name: x\nplatform: stm32\nrequires:\n  hal: true\nheaders:\n  - src/ee24.h\n",
        ] {
            let found = advised(root.path(), text);
            let fix = found.iter().flat_map(|advice| &advice.fixes).next();
            let fix = fix.unwrap_or_else(|| panic!("no fix for {text}: {found:#?}"));

            assert_eq!(
                advised(root.path(), &fix.text),
                [],
                "{text} became\n{}",
                fix.text
            );
        }
    }
}
