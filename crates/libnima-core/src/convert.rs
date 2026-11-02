//! Turning an `installer.yml` of lib-installer 4.0.0 into a `libnima.yml`.
//!
//! A library released for lib-installer has only the old file, and it goes on working: libNIMA reads it through this,
//! in memory, and `libnima --convert` writes the result out for the library's author. Only a file that asks for
//! lib-installer 3 or 4 is read: those keep every file at its path, as libnima.yml does, and where the sections of a
//! file for 3 differ, the reader refuses them with where each key went. The meanings of the older ones are gone, and
//! reading one as if it asked for 4 could put its files somewhere else without a word.
//!
//! The text is edited, not written anew, so its comments and its order stay: a key moves with the comment above it and
//! the comments that go on from its line, and every other line comes out as it went in.
//!
//! ```text
//! requires.lib_installer: 4.0.0                     requires.libnima: 1.0.0
//! requires.hal, cmsis, rtos, hal_modules, c_standard  stm32.requires, in a section after requires
//! install.include_dirs, install.defines             include_dirs and defines, at the top level
//! files.headers, files.sources                      headers and sources, where files was
//! run[].script: x.py                                run[].command: [python, x.py]
//! ```

use crate::manifest::{ManifestError, is_installer_key, version_tuple};
use crate::yaml::{self, Entry, Mapping, Node, comment_start};

/// The keys of requires that are STM32's alone.
const STM32_KEYS: [&str; 5] = ["hal", "cmsis", "rtos", "hal_modules", "c_standard"];

/// What libNIMA calls the file's first version.
const FIRST_VERSION: &str = "1.0.0";

/// The lines of a file, each a whole line, and the ones already moved or taken out.
struct Lines {
    lines: Vec<String>,
    gone: Vec<bool>,
}

impl Lines {
    fn new(text: &str) -> Lines {
        let text = text.replace("\r\n", "\n");
        let body = text.strip_suffix('\n').unwrap_or(&text);
        let lines: Vec<String> = body.split('\n').map(str::to_string).collect();

        Lines {
            gone: vec![false; lines.len()],
            lines,
        }
    }

    fn indent(&self, index: usize) -> usize {
        let line = &self.lines[index];

        line.len() - line.trim_start_matches(' ').len()
    }

    /// Whether a line holds nothing but a comment, or nothing at all.
    fn is_quiet(&self, index: usize) -> bool {
        let line = self.lines[index].trim_start();

        line.is_empty() || line.starts_with('#')
    }

    /// The lines an entry of a mapping stands on: from the comment lines right above it at its own depth, to the last
    /// line that belongs to it. Comments indented deeper than a key, the kind that go on from the comment on its line,
    /// are its own; those at or above its depth lead into what comes next. `end` is the line after the mapping's last.
    fn range_of(&self, map: &Mapping, position: usize, end: usize) -> (usize, usize) {
        let entry = &map.entries[position];
        let first = entry.line - 1;
        let depth = self.indent(first);
        let next = map
            .entries
            .get(position + 1)
            .map_or(end, |next| next.line - 1);
        let mut last = next;

        while last > first + 1
            && self.is_quiet(last - 1)
            && (self.lines[last - 1].trim().is_empty() || self.indent(last - 1) <= depth)
        {
            last -= 1;
        }

        let mut start = first;

        while start > 0
            && !self.gone[start - 1]
            && self.lines[start - 1].trim_start().starts_with('#')
            && self.indent(start - 1) == depth
        {
            start -= 1;
        }

        (start, last)
    }

    /// Take a range of lines out, each moved left or right by `shift` columns, and return them.
    fn take(&mut self, (start, end): (usize, usize), shift: isize) -> Vec<String> {
        let mut found = Vec::new();

        for index in start..end {
            self.gone[index] = true;

            let line = &self.lines[index];

            found.push(if line.trim().is_empty() {
                String::new()
            } else if shift >= 0 {
                format!("{}{line}", " ".repeat(shift.unsigned_abs()))
            } else {
                let cut = shift.unsigned_abs().min(self.indent(index));
                line[cut..].to_string()
            });
        }

        found
    }
}

/// One line with the value of its key replaced, the comment after it staying in its column when there is room.
fn replaced_value(line: &str, old_key: &str, new_key: &str, new_value: &str) -> String {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let head = format!("{}{new_key}: {new_value}", " ".repeat(indent));

    match comment_start(line) {
        Some(at) => {
            let comment = &line[at..];
            let column = at.max(indent + old_key.len());

            if head.len() < column {
                format!("{head}{}{comment}", " ".repeat(column - head.len()))
            } else {
                format!("{head} {comment}")
            }
        }
        None => head,
    }
}

/// The text with what lib-installer calls itself changed, in the comments only, so a value is never touched.
fn renamed_comment(line: &str) -> String {
    let Some(at) = comment_start(line) else {
        return line.to_string();
    };

    let comment = line[at..]
        .replace(
            "https://github.com/nimaltd/lib-installer",
            "https://github.com/nimaltd/libNIMA",
        )
        .replace("lib_installer", "libnima")
        .replace("lib-installer", "libNIMA")
        .replace("installer.yml", "libnima.yml");

    format!("{}{comment}", &line[..at])
}

fn entry_position(map: &Mapping, key: &str) -> Option<usize> {
    map.entries.iter().position(|entry| entry.key == key)
}

/// The line after the last of a mapping that stands under the key at `line`: the line of what comes next at the depth of
/// that key or above, or the end of the file.
fn end_of(lines: &Lines, line: usize) -> usize {
    let depth = lines.indent(line);

    for index in line + 1..lines.lines.len() {
        if !lines.is_quiet(index) && lines.indent(index) <= depth {
            return index;
        }
    }

    lines.lines.len()
}

/// Bring the headers and the sources up out of a files key, where the files key was.
fn hoist(
    lines: &mut Lines,
    parent: &Mapping,
    parent_end: usize,
    keys: &[&str],
    key: &str,
) -> Option<(usize, Vec<String>)> {
    let position = entry_position(parent, key)?;
    let entry: &Entry = &parent.entries[position];
    let at = entry.line - 1;
    let (start, end) = lines.range_of(parent, position, parent_end);
    let depth = lines.indent(at);
    let mut out = Vec::new();

    if let Some(map) = entry.value.as_map() {
        let below = map
            .entries
            .first()
            .map(|first| lines.indent(first.line - 1))
            .unwrap_or(depth + 2);
        let shift = depth as isize - below as isize;

        for (index, child) in map.entries.iter().enumerate() {
            if keys.contains(&child.key.as_str()) {
                let range = lines.range_of(map, index, end);
                out.extend(lines.take(range, shift));
            }
        }
    }

    // What is left of it goes: the key's own line, and anything under it not brought up.
    let comments_above = at - start;
    let mut lead = Vec::new();

    for index in start..start + comments_above {
        if !lines.gone[index] {
            lead.push(lines.lines[index].clone());
        }
    }

    for flag in &mut lines.gone[start..end] {
        *flag = true;
    }

    let mut whole = lead;
    whole.extend(out);

    Some((start, whole))
}

/// Why this file cannot be converted, or what it asks for.
fn check_asks(data: &Mapping) -> Result<(), ManifestError> {
    let requires = data.given("requires").and_then(Node::as_map);
    let asked: Vec<(String, &Node)> = requires
        .map(|requires| {
            requires
                .entries
                .iter()
                .filter(|entry| is_installer_key(&entry.key))
                .map(|entry| (entry.key.clone(), &entry.value))
                .collect()
        })
        .unwrap_or_default();

    let best = asked
        .iter()
        .filter_map(|(_, value)| version_tuple(&value.text()))
        .max();

    match best {
        Some((3 | 4, _, _)) => Ok(()),
        Some((major, minor, patch)) if major > 4 => Err(ManifestError::new(format!(
            "It asks for lib-installer {major}.{minor}.{patch}, which libNIMA does not know."
        ))),
        Some((major, minor, patch)) => Err(ManifestError::new(format!(
            "It asks for lib-installer {major}.{minor}.{patch}, and libNIMA reads only what asks for 3.0.0 or newer: the \
             older meanings of its keys are gone. Ask for lib_installer: 4.0.0 in it after moving it on, or install the \
             library with lib-installer."
        ))),
        None => Err(ManifestError::new(
            "It does not say which lib-installer it was written for, and libNIMA reads only what asks for 3.0.0 or newer: \
             the older meanings of its keys are gone. Ask for lib_installer: 4.0.0 in it after moving it on, or install the \
             library with lib-installer.",
        )),
    }
}

/// An installer.yml of lib-installer 4.0.0 as the text of a libnima.yml.
pub fn convert(text: &str) -> Result<String, ManifestError> {
    let parsed = yaml::loads(text).map_err(|error| ManifestError {
        message: format!("It is not valid YAML: {error}"),
        line: error.line,
    })?;

    let Some(node) = parsed.filter(|node| node.as_map().is_some()) else {
        return Err(ManifestError::new(
            "It should hold a mapping of settings, not a bare value.",
        ));
    };

    let data = node.as_map().expect("checked above");

    check_asks(data)?;

    if data.has("stm32") {
        return Err(ManifestError::at(
            data.line_of("stm32").unwrap_or(0),
            "It has a key called stm32, and libnima.yml gives that key a meaning of its own. Move what it says, then convert it.",
        ));
    }

    let crlf = text.contains("\r\n");
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lines = Lines::new(stripped);
    let total = lines.lines.len();

    // What each key's lines turn into, by the line they start at, added back in order.
    let mut inserts: Vec<(usize, Vec<String>)> = Vec::new();

    // requires: the version it asks for, and the keys that are STM32's alone.
    if let Some(requires) = data.get("requires").and_then(Node::as_map) {
        let requires_at = data.line_of("requires").unwrap_or(1) - 1;
        let requires_end = end_of(&lines, requires_at);
        let mut first = true;
        let mut moved: Vec<String> = Vec::new();

        for (position, entry) in requires.entries.iter().enumerate() {
            let at = entry.line - 1;

            if is_installer_key(&entry.key) {
                if first {
                    lines.lines[at] =
                        replaced_value(&lines.lines[at], &entry.key, "libnima", FIRST_VERSION);
                    first = false;
                } else {
                    let range = lines.range_of(requires, position, requires_end);
                    lines.take(range, 0);
                }
            } else if STM32_KEYS.contains(&entry.key.as_str()) {
                let range = lines.range_of(requires, position, requires_end);
                let depth = lines.indent(at) as isize;
                moved.extend(lines.take(range, 4 - depth));
            }
        }

        if !moved.is_empty() {
            let mut block = vec![
                String::new(),
                "stm32:".to_string(),
                "  requires:".to_string(),
            ];
            block.extend(moved);

            // Right after what is left of requires.
            let top = data
                .entries
                .iter()
                .position(|entry| entry.key == "requires")
                .unwrap_or(0);
            let (_, end) = lines.range_of(data, top, total);
            inserts.push((end, block));
        }
    }

    // install: include_dirs and defines come up to the top level, and the rest of it goes.
    if let Some(found) = hoist(
        &mut lines,
        data,
        total,
        &["include_dirs", "defines"],
        "install",
    ) {
        inserts.push(found);
    }

    // files: headers and sources come up, in the file and in each option.
    if let Some(found) = hoist(&mut lines, data, total, &["headers", "sources"], "files") {
        inserts.push(found);
    }

    if let Some(options) = data.given("options").and_then(Node::as_map) {
        for entry in &options.entries {
            if let Some(body) = entry.value.as_map() {
                let at = entry.line - 1;
                let end = end_of(&lines, at);

                if let Some(found) = hoist(&mut lines, body, end, &["headers", "sources"], "files")
                {
                    inserts.push(found);
                }
            }
        }
    }

    // run: a script is Python, and libNIMA has none of its own to run one with, so it asks for the machine's.
    if let Some(steps) = data.given("run").and_then(Node::as_list) {
        for step in steps {
            if let Some(script) = step.as_map().and_then(|map| map.get("script")) {
                let at = script.line - 1;
                let path = script.text();
                let word = if path.contains('{') || path.contains(' ') {
                    format!("\"{path}\"")
                } else {
                    path
                };

                lines.lines[at] = replaced_script(&lines.lines[at], &word);
            }
        }
    }

    inserts.sort_by_key(|(at, _)| *at);

    let mut out: Vec<String> = Vec::new();
    let mut next = inserts.iter().peekable();

    for index in 0..=total {
        while let Some((_, block)) = next.next_if(|(at, _)| *at == index) {
            out.extend(block.iter().map(|line| renamed_comment(line)));
        }

        if index < total && !lines.gone[index] {
            out.push(renamed_comment(&lines.lines[index]));
        }
    }

    let newline = if crlf { "\r\n" } else { "\n" };
    let mut result = out.join(newline);

    if stripped.ends_with('\n') {
        result.push_str(newline);
    }

    Ok(result)
}

/// A run step's script line as a command, whichever way it is written: "script: x.py" or "- script: x.py".
fn replaced_script(line: &str, word: &str) -> String {
    let Some(at) = line.find("script:") else {
        return line.to_string();
    };

    let head = format!("{}command: [python, {word}]", &line[..at]);

    match comment_start(line) {
        Some(comment) => {
            if head.len() < comment {
                format!(
                    "{head}{}{}",
                    " ".repeat(comment - head.len()),
                    &line[comment..]
                )
            } else {
                format!("{head} {}", &line[comment..])
            }
        }
        None => head,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest;
    use std::fs;

    #[test]
    fn a_real_stm32_manifest_comes_out_in_the_new_form() {
        let text = "# Read by lib-installer. See https://github.com/nimaltd/lib-installer\n\
                    \n\
                    name: ee24\n\
                    platform: stm32\n\
                    \n\
                    # What the project has to offer.\n\
                    requires:\n\
                    \x20 lib_installer: 4.0.0 # the oldest lib-installer that reads this file\n\
                    \x20 libraries:\n\
                    \x20   - nimaltd/osal >= 1.1.0   # the mutex\n\
                    \x20 hal: true           # HAL I2C\n\
                    \x20 cmsis: false\n\
                    \x20 rtos: optional      # none, or FreeRTOS\n\
                    \x20 hal_modules: [i2c]  # must be enabled\n\
                    \x20 peripherals:\n\
                    \x20   - type: i2c\n\
                    \x20     count: 1\n\
                    \x20 c_standard: c11\n\
                    \n\
                    # The headers.\n\
                    headers:\n\
                    \x20 - src/ee24.h\n";

        assert_eq!(
            convert(text).unwrap(),
            "# Read by libNIMA. See https://github.com/nimaltd/libNIMA\n\
             \n\
             name: ee24\n\
             platform: stm32\n\
             \n\
             # What the project has to offer.\n\
             requires:\n\
             \x20 libnima: 1.0.0       # the oldest libNIMA that reads this file\n\
             \x20 libraries:\n\
             \x20   - nimaltd/osal >= 1.1.0   # the mutex\n\
             \x20 peripherals:\n\
             \x20   - type: i2c\n\
             \x20     count: 1\n\
             \n\
             stm32:\n\
             \x20 requires:\n\
             \x20   hal: true           # HAL I2C\n\
             \x20   cmsis: false\n\
             \x20   rtos: optional      # none, or FreeRTOS\n\
             \x20   hal_modules: [i2c]  # must be enabled\n\
             \x20   c_standard: c11\n\
             \n\
             # The headers.\n\
             headers:\n\
             \x20 - src/ee24.h\n"
        );
    }

    #[test]
    fn comments_that_go_on_from_a_line_move_with_it() {
        let text = "name: x\nplatform: stm32\nrequires:\n  lib_installer: 4.0.0\n  hal_modules: []      # must be enabled\n                        # a second line of it\n  peripherals: []\n                        # stays with peripherals\nheaders: [a.h]\n";

        assert_eq!(
            convert(text).unwrap(),
            "name: x\nplatform: stm32\nrequires:\n  libnima: 1.0.0\n  peripherals: []\n                        # stays with peripherals\n\nstm32:\n  requires:\n    hal_modules: []      # must be enabled\n                          # a second line of it\nheaders: [a.h]\n"
        );
    }

    #[test]
    fn install_and_files_come_up_to_the_top() {
        let text = "name: x\nrequires:\n  lib_installer: 4.0.0\n# How it goes in.\ninstall:\n  layout: mirror\n  # Where to look.\n  include_dirs: [src]\n  defines:\n    - A\n    - B=1\nfiles:\n  headers:\n    - a.h\n  sources:\n    - a.c\nextras: [NOTICE]\n";

        assert_eq!(
            convert(text).unwrap(),
            "name: x\nrequires:\n  libnima: 1.0.0\n# How it goes in.\n# Where to look.\ninclude_dirs: [src]\ndefines:\n  - A\n  - B=1\nheaders:\n  - a.h\nsources:\n  - a.c\nextras: [NOTICE]\n"
        );
    }

    #[test]
    fn an_options_files_come_up_too() {
        let text = "name: x\nrequires:\n  lib_installer: 4.0.0\nheaders: [a.h]\noptions:\n  fast:\n    description: Go\n    files:\n      sources:\n        - f.c\n  slow:\n    sources: [s.c]\n";

        assert_eq!(
            convert(text).unwrap(),
            "name: x\nrequires:\n  libnima: 1.0.0\nheaders: [a.h]\noptions:\n  fast:\n    description: Go\n    sources:\n      - f.c\n  slow:\n    sources: [s.c]\n"
        );
    }

    #[test]
    fn a_script_becomes_a_command_with_the_machines_python() {
        let text = "name: x\nrequires:\n  lib_installer: 4.0.0\nrun:\n  - label: a\n    script: tools/make.py     # Python\n    args: [x]\n  - label: b\n    script: '{library}/m.py'\n  - label: c\n    command: [imgtool]\n";
        let converted = convert(text).unwrap();

        assert!(
            converted.contains("    command: [python, tools/make.py] # Python\n"),
            "{converted}"
        );
        assert!(
            converted.contains("    command: [python, \"{library}/m.py\"]\n"),
            "{converted}"
        );
        assert!(
            converted.contains("    command: [imgtool]\n"),
            "{converted}"
        );

        // And it reads.
        let node = yaml::loads(&converted).unwrap().unwrap();
        let steps = node
            .as_map()
            .unwrap()
            .get("run")
            .unwrap()
            .as_list()
            .unwrap();
        let second = steps[1]
            .as_map()
            .unwrap()
            .get("command")
            .unwrap()
            .as_list()
            .unwrap();

        assert_eq!(second[1].text(), "{library}/m.py");
    }

    #[test]
    fn the_keys_of_the_five_installers_all_go() {
        let text = "name: x\nrequires:\n  stm32_installer: 2.1.0\n  lib_installer: 4.0.0\n  zephyr_installer: 2.0.0\nheaders: [a.h]\n";

        assert_eq!(
            convert(text).unwrap(),
            "name: x\nrequires:\n  libnima: 1.0.0\nheaders: [a.h]\n"
        );
    }

    #[test]
    fn line_endings_stay() {
        let text = "name: x\r\nrequires:\r\n  lib_installer: 4.0.0\r\nheaders: [a.h]\r\n";

        assert_eq!(
            convert(text).unwrap(),
            "name: x\r\nrequires:\r\n  libnima: 1.0.0\r\nheaders: [a.h]\r\n"
        );
    }

    #[test]
    fn what_is_in_a_value_is_never_renamed() {
        let text = "name: lib-installer\nrepository: https://github.com/x/lib-installer\nrequires:\n  lib_installer: 4.0.0\nheaders: [installer.yml]\n";
        let converted = convert(text).unwrap();

        assert!(
            converted.starts_with(
                "name: lib-installer\nrepository: https://github.com/x/lib-installer\n"
            ),
            "{converted}"
        );
        assert!(
            converted.ends_with("headers: [installer.yml]\n"),
            "{converted}"
        );
    }

    #[test]
    fn a_file_from_before_4_is_refused_with_the_reason() {
        for (text, wanted) in [
            (
                "name: x\nrequires:\n  lib_installer: 2.0.0\n",
                "asks for lib-installer 2.0.0",
            ),
            (
                "name: x\nrequires:\n  zephyr_installer: 2.1.0\n",
                "asks for lib-installer 2.1.0",
            ),
            ("name: x\n", "does not say which lib-installer"),
            (
                "name: x\nrequires:\n  lib_installer: 5.0.0\n",
                "does not know",
            ),
            (
                "name: x\nrequires:\n  lib_installer: 4.0\n",
                "does not say which",
            ),
        ] {
            let error = convert(text).unwrap_err();
            assert!(
                error.message.contains(wanted),
                "{text:?}: {}",
                error.message
            );
        }
    }

    #[test]
    fn a_key_the_new_form_takes_is_refused() {
        let error = convert("name: x\nrequires:\n  lib_installer: 4.0.0\nstm32: 1\n").unwrap_err();

        assert_eq!(error.line, Some(4));
    }

    /// What every library beside this repository says comes out readable, and says the same.
    #[test]
    fn every_real_library_converts_and_reads() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let Ok(folders) = fs::read_dir(root) else {
            return;
        };

        for folder in folders.flatten() {
            let Ok(text) = fs::read_to_string(folder.path().join("installer.yml")) else {
                continue;
            };

            let converted = convert(&text)
                .unwrap_or_else(|error| panic!("{:?}: {}", folder.path(), error.message));
            let node = manifest::parse(&converted)
                .unwrap_or_else(|error| panic!("{:?}: {}", folder.path(), error.message));
            let data = node.as_map().unwrap();
            let platforms = manifest::platforms_of(data).unwrap().unwrap_or_default();
            let requires = data.get("requires").and_then(Node::as_map).unwrap();

            assert_eq!(requires.get("libnima").unwrap().text(), "1.0.0");
            assert!(!data.has("install"), "{:?}", folder.path());
            assert!(!data.has("files"), "{:?}", folder.path());

            for platform in platforms {
                let scratch = tempfile::tempdir().unwrap();

                for entry in crate::download::tests_listed(data) {
                    if !manifest::is_pattern(&entry) {
                        let path = scratch.path().join(&entry);
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(path, "").unwrap();
                    }
                }

                manifest::load_text(
                    scratch.path(),
                    &scratch.path().join("libnima.yml"),
                    &converted,
                    Some(platform),
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "{:?} on {}: {}",
                        folder.path(),
                        platform.name(),
                        error.message
                    )
                });
            }
        }
    }
}
