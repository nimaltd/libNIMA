//! The record of what was installed into a project: `.libnima.json`.
//!
//! It keeps, for each library, where it came from down to the commit, where it went, every file it
//! wrote and what those held, so an update and an uninstall have something to work from. It is a file
//! in the user's project that anyone can edit, so whatever is read from it is checked as it is read.

use std::path::Path;

use serde_json::{Map, Value};

use crate::files;
use crate::platform::Platform;

pub const RECORD_NAME: &str = ".libnima.json";

/// The record lib-installer kept, which a first run moves over.
pub const OLD_RECORD_NAME: &str = ".lib-installer.json";

/// JSON as a person reads it in a diff: two spaces, a line ending at the end, and everything that is
/// not ASCII escaped, so the file reads the same whatever an editor takes it for.
pub fn json_text(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, 0, &mut out, false);
    out.push('\n');

    out
}

/// The same, with every mapping in the order of its keys, so a record changes only where it changed.
pub fn json_sorted_text(value: &Value) -> String {
    let mut out = String::new();
    write_json(value, 0, &mut out, true);
    out.push('\n');

    out
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');

    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            char if (' '..='~').contains(&char) || char == '\u{7f}' => out.push(char),
            char => {
                let mut units = [0; 2];

                for unit in char.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }

    out.push('"');
}

fn write_json(value: &Value, depth: usize, out: &mut String, sort: bool) {
    let pad = "  ".repeat(depth + 1);
    let close = "  ".repeat(depth);

    match value {
        Value::Array(items) if !items.is_empty() => {
            out.push_str("[\n");

            for (index, item) in items.iter().enumerate() {
                out.push_str(&pad);
                write_json(item, depth + 1, out, sort);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }

            out.push_str(&close);
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            let mut keys: Vec<&String> = map.keys().collect();

            if sort {
                keys.sort();
            }

            out.push_str("{\n");

            for (index, key) in keys.iter().enumerate() {
                out.push_str(&pad);
                write_string(key, out);
                out.push_str(": ");
                write_json(&map[*key], depth + 1, out, sort);
                out.push_str(if index + 1 < keys.len() { ",\n" } else { "\n" });
            }

            out.push_str(&close);
            out.push('}');
        }
        Value::Array(_) => out.push_str("[]"),
        Value::Object(_) => out.push_str("{}"),
        Value::String(text) => write_string(text, out),
        other => out.push_str(&other.to_string()),
    }
}

/// The whole of a record, when there is one that reads as a mapping.
pub fn read_record(path: &Path) -> Option<Map<String, Value>> {
    let text = files::read(path).ok()?;

    match serde_json::from_str(&text) {
        Ok(Value::Object(map)) => Some(map),
        _ => None,
    }
}

/// Read back what has been installed into a project. Empty when nothing has. A project not moved
/// over yet is read from lib-installer's record, or from that of one of the five installers before it.
pub fn installed_libraries(project_root: &Path) -> Map<String, Value> {
    let older = Platform::ALL.map(|platform| format!(".{}.json", platform.old_installer()));
    let path = [RECORD_NAME, OLD_RECORD_NAME]
        .into_iter()
        .chain(older.iter().map(String::as_str))
        .map(|name| project_root.join(name))
        .find(|path| path.is_file());

    match path
        .and_then(|path| read_record(&path))
        .and_then(|mut record| record.remove("libraries"))
    {
        Some(Value::Object(libraries)) => libraries,
        _ => Map::new(),
    }
}

/// The platform a folder's record names, or None. Lib-installer's counts, and so does the record of
/// one of the five installers before it, which says by its name.
pub fn recorded_platform(folder: &Path) -> Option<Platform> {
    for name in [RECORD_NAME, OLD_RECORD_NAME] {
        let named = read_record(&folder.join(name)).and_then(|record| {
            record
                .get("platform")
                .and_then(Value::as_str)
                .and_then(Platform::from_name)
        });

        if named.is_some() {
            return named;
        }
    }

    Platform::ALL.into_iter().find(|platform| {
        folder
            .join(format!(".{}.json", platform.old_installer()))
            .is_file()
    })
}

/// Write a record back. Never fails: a read-only project root is unusual, but it must not fail a good
/// install.
pub fn write_record(path: &Path, record: Map<String, Value>) {
    let _ = files::write(path, &json_sorted_text(&Value::Object(record)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_is_written_the_way_a_person_reads_it() {
        let value = json!({"name": "demo", "list": ["a", "b"], "empty": [], "none": {}, "n": 1, "yes": true, "text": "a\"b\n"});

        assert_eq!(
            json_text(&value),
            "{\n  \"name\": \"demo\",\n  \"list\": [\n    \"a\",\n    \"b\"\n  ],\n  \"empty\": [],\n  \"none\": {},\n  \"n\": 1,\n  \"yes\": true,\n  \"text\": \"a\\\"b\\n\"\n}\n"
        );
    }

    #[test]
    fn what_is_not_ascii_is_escaped() {
        assert_eq!(json_text(&json!("é😀")), "\"\\u00e9\\ud83d\\ude00\"\n");
    }

    #[test]
    fn a_sorted_record_changes_only_where_it_changed() {
        assert_eq!(
            json_sorted_text(&json!({"b": 1, "a": 2})),
            "{\n  \"a\": 2,\n  \"b\": 1\n}\n"
        );
    }

    #[test]
    fn nothing_recorded_reads_as_nothing() {
        let root = tempfile::tempdir().unwrap();

        assert!(installed_libraries(root.path()).is_empty());
        assert_eq!(recorded_platform(root.path()), None);

        std::fs::write(root.path().join(RECORD_NAME), "not json").unwrap();

        assert!(installed_libraries(root.path()).is_empty());
    }

    #[test]
    fn what_is_installed_is_read_from_any_record_the_newest_first() {
        let root = tempfile::tempdir().unwrap();
        let has = |name: &str| format!("{{\"libraries\": {{\"{name}\": {{}}}}}}");
        let names = |root: &Path| {
            installed_libraries(root)
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        };

        std::fs::write(root.path().join(".stm32-installer.json"), has("older")).unwrap();
        assert_eq!(names(root.path()), ["older"]);

        std::fs::write(root.path().join(OLD_RECORD_NAME), has("old")).unwrap();
        assert_eq!(names(root.path()), ["old"]);

        std::fs::write(root.path().join(RECORD_NAME), has("new")).unwrap();
        assert_eq!(names(root.path()), ["new"]);
    }

    #[test]
    fn the_platform_is_read_from_any_record() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(
            root.path().join(OLD_RECORD_NAME),
            "{\"platform\": \"zephyr\"}",
        )
        .unwrap();

        assert_eq!(recorded_platform(root.path()), Some(Platform::Zephyr));

        let older = tempfile::tempdir().unwrap();
        std::fs::write(older.path().join(".esp32-installer.json"), "{}").unwrap();

        assert_eq!(recorded_platform(older.path()), Some(Platform::Esp32));
    }
}
