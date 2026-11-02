//! Titles and notes: the comments a manifest gives the lines it writes.
//!
//! A list a library writes into a file of the project's, or of its own, can be given in
//! groups, a mapping of each group's title to its list, and a setting can carry a note,
//! as a mapping of one, SYMBOL=value: its note. Every file written from them carries
//! both as comments, the same way on every platform: a group's title between two lines
//! of stars, a note as a comment of its own above its setting. The sign is the file's
//! own: # in Kconfig, CMake and sdkconfig.defaults, ; in platformio.ini.
//!
//! A platform's section also gives every line under where it goes: project, the
//! project's own files, and library, the library's own folder. The groups of CMake
//! lines read from there are here too, since Zephyr and ESP-IDF read them the same way,
//! each with places of its own.

use std::sync::LazyLock;

use regex::Regex;

use crate::blocks::Group;
use crate::manifest::ManifestError;
use crate::yaml::{Mapping, Node, Value, quoted};

/// How many stars the lines around a title have: 80 columns with their "# ".
const WIDTH: usize = 78;

/// A comment Kconfig reads as a setting, "# CONFIG_FOO is not set", which no title or
/// note of a manifest may be.
static NOT_SET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:SB_)?CONFIG_\S+ is not set").expect("a valid pattern"));

/// A line of stars, above and below a title, as a comment of the file's.
fn stars(sign: &str) -> String {
    format!("{sign} {}", "*".repeat(WIDTH))
}

/// A title or a note of the manifest's, as the comment libNIMA writes it into a file
/// with: one line, which every file it goes into reads as a comment and nothing else,
/// and which is never taken for a line that starts or ends a block.
pub fn comment(value: &Node, what: &str) -> Result<String, ManifestError> {
    let text = match &value.value {
        Value::Null | Value::List(_) | Value::Map(_) => String::new(),
        _ => value.text().trim().to_string(),
    };

    if text.is_empty() || text.contains('\n') || text.contains('\r') {
        return Err(ManifestError::at(
            value.line,
            format!("{what} is {}. Give it one line of text.", value.shown()),
        ));
    }

    if text.contains(">>>") || text.contains("<<<") || text.contains("USER CODE") {
        return Err(ManifestError::at(
            value.line,
            format!(
                "{what} is {}, which has what libNIMA marks its blocks with. Say it another way.",
                quoted(&text)
            ),
        ));
    }

    if NOT_SET.is_match(&text) {
        return Err(ManifestError::at(
            value.line,
            format!(
                "{what} is {}, which Kconfig would read as a setting. Say it another way.",
                quoted(&text)
            ),
        ));
    }

    Ok(text)
}

/// One entry of a list, with the title of its group and its note.
#[derive(Debug, Clone, PartialEq)]
pub struct Listed<T> {
    pub item: T,
    pub group: Option<String>,
    pub note: Option<String>,
}

/// One list of what `listed` reads: its title, its entries, and the line it stands on.
type OneList<'a> = (Option<&'a str>, Option<&'a [Node]>, usize);

/// The entries of a list under `key`, each made by `make`: a plain list, or groups of
/// them, a mapping of each group's title to its list. An entry of a list of settings
/// can carry a note, as a mapping of one, SYMBOL=value: its note; a line of CMake
/// cannot, since a line holding ': ' is CMake's own and goes in quotes. Each entry is
/// given the title of its group and its note, None when it has none, and one `make`
/// gives None for, a blank line of CMake, is passed by.
pub fn listed<T>(
    value: Option<&Node>,
    key: &str,
    notes: bool,
    mut make: impl FnMut(&Node) -> Result<Option<T>, ManifestError>,
) -> Result<Vec<Listed<T>>, ManifestError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(Vec::new());
    };

    let mut parts: Vec<OneList> = Vec::new();

    match &value.value {
        Value::List(list) => parts.push((None, Some(list), value.line)),
        Value::Map(map) => {
            for entry in &map.entries {
                parts.push((Some(&entry.key), entry.value.as_list(), entry.line));
            }
        }
        _ => {
            return Err(ManifestError::at(
                value.line,
                format!("{key} is a list, or groups of lists, each under its title."),
            ));
        }
    }

    let mut found = Vec::new();

    for (title, entries, line) in parts {
        let title = match title {
            Some(title) => {
                let written = comment(&Node::string(title, line), &format!("A title under {key}"))?;

                if entries.is_none_or(<[Node]>::is_empty) {
                    return Err(ManifestError::at(
                        line,
                        format!(
                            "{key} has the title {} with no list under it. Give its lines there.",
                            quoted(&written)
                        ),
                    ));
                }

                Some(written)
            }
            None => None,
        };

        for entry in entries.unwrap_or_default() {
            let mut note = None;
            let mut given = entry.clone();

            if let Value::Map(map) = &entry.value {
                if !notes {
                    return Err(ManifestError::at(
                        entry.line,
                        format!(
                            "{key} has {}, which YAML reads as a mapping: a line holding ': ' goes in quotes.",
                            entry.shown()
                        ),
                    ));
                }

                if map.entries.len() != 1 {
                    return Err(ManifestError::at(
                        entry.line,
                        format!(
                            "{key} has {}. Give one setting, with its note after a colon, such as LOG=n: no messages.",
                            entry.shown()
                        ),
                    ));
                }

                let only = &map.entries[0];
                note = Some(comment(
                    &only.value,
                    &format!("The note of {} under {key}", only.key),
                )?);
                given = Node::string(&only.key, only.line);
            }

            if given.is_null() {
                return Err(ManifestError::at(
                    entry.line,
                    format!(
                        "{key} has an empty entry. A comment for the file goes in a title or a note."
                    ),
                ));
            }

            if let Some(item) = make(&given)? {
                found.push(Listed {
                    item,
                    group: title.clone(),
                    note,
                });
            }
        }
    }

    Ok(found)
}

/// A group's title between two lines of stars, as comment lines.
pub fn titled(title: &str, sign: &str) -> Vec<String> {
    vec![stars(sign), format!("{sign} {title}"), stars(sign)]
}

/// The lines of entries, each written by `line`: the title of an entry's group between
/// two lines of stars above the group's first entry, and an entry's note, when it has
/// one, as a comment of its own above it. A title that does not start them comes after
/// a blank line, unless `blank` is off, for a file that reads a blank line as the end
/// of the list, as platformio.ini does build_flags.
pub fn annotated<T>(
    entries: &[Listed<T>],
    mut line: impl FnMut(&T) -> String,
    sign: &str,
    blank: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    let mut title: Option<&str> = None;

    for entry in entries {
        if let Some(group) = entry.group.as_deref()
            && Some(group) != title
        {
            if !lines.is_empty() && blank {
                lines.push(String::new());
            }

            lines.extend(titled(group, sign));
            title = Some(group);
        }

        if let Some(note) = &entry.note {
            lines.push(format!("{sign} {note}"));
        }

        lines.push(line(&entry.item));
    }

    lines
}

/// Whether any of entries is in a group, so the file written from them sets its titles apart.
pub fn grouped<T>(entries: &[Listed<T>]) -> bool {
    entries.iter().any(|entry| entry.group.is_some())
}

/// A line of CMake, or None for a blank one, which is passed by. The space it starts
/// with is kept, for the body of an if().
fn line_of(text: &Node, key: &str) -> Result<Option<String>, ManifestError> {
    let Some(written) = text.as_str() else {
        return Err(ManifestError::at(
            text.line,
            format!("{key} is a list of lines, each in quotes when it holds a colon."),
        ));
    };

    if written.contains('\n') || written.contains('\r') {
        return Err(ManifestError::at(
            text.line,
            format!("{key} has a line that runs onto another. Give each line on its own."),
        ));
    }

    let written = written.trim_end();

    Ok((!written.trim().is_empty()).then(|| written.to_string()))
}

/// A part of a platform's section, such as zephyr.project: a mapping of some of `keys`,
/// empty when left out.
pub fn part<'a>(
    value: Option<&'a Node>,
    key: &str,
    keys: &[&str],
) -> Result<Option<&'a Mapping>, ManifestError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };

    let Some(map) = value.as_map() else {
        return Err(ManifestError::at(
            value.line,
            format!(
                "{key} holds {}, each a key of its own under it.",
                keys.join(", ")
            ),
        ));
    };

    let unknown: Vec<&str> = map.keys().filter(|name| !keys.contains(name)).collect();

    if let Some(first) = unknown.first() {
        return Err(ManifestError::at(
            map.line_of(first).unwrap_or(value.line),
            format!(
                "{key} has {}. It holds {}.",
                unknown.join(", "),
                keys.join(", ")
            ),
        ));
    }

    Ok(Some(map))
}

/// The groups of CMake lines of a section: those of {key}.project.cmake, the project's
/// own CMakeLists.txt, under `places`, and those of {key}.library.cmake, the library's
/// own, in the place `own`. Each place is one group, labelled after it, or for an
/// option after the option and it, so the blocks in the files keep their names. A title
/// of the manifest's is written among the lines, between two lines of stars.
pub fn cmake_groups(
    project: Option<&Node>,
    library: Option<&Node>,
    key: &str,
    places: &[&str],
    own: &str,
    prefix: Option<&str>,
) -> Result<Vec<Group>, ManifestError> {
    let given = part(project, &format!("{key}.project.cmake"), places)?;
    let mut found = Vec::new();
    let mut each: Vec<(&str, Option<&Node>)> = Vec::new();

    if let Some(given) = given {
        for entry in &given.entries {
            each.push((&entry.key, Some(&entry.value)));
        }
    }

    each.push((own, library));

    for (place, value) in each {
        let name = if place == own {
            format!("{key}.library.cmake")
        } else {
            format!("{key}.project.cmake.{place}")
        };

        let entries = listed(value, &name, false, |text| line_of(text, &name))?;
        let lines = annotated(&entries, String::clone, "#", true);

        if !lines.is_empty() {
            found.push(Group {
                label: match prefix {
                    Some(prefix) => format!("{prefix}-{place}"),
                    None => place.to_string(),
                },
                place: place.to_string(),
                lines,
            });
        }
    }

    Ok(found)
}

/// An option's own groups of CMake lines, written while it is taken: under
/// project.cmake and library.cmake of its part for the platform, `section`.
pub fn option_cmake(
    option_name: &str,
    given: Option<&Node>,
    section: &str,
    places: &[&str],
    own: &str,
) -> Result<Vec<Group>, ManifestError> {
    let key = format!("options.{option_name}.{section}");

    if let Some(map) = given.and_then(Node::as_map)
        && map.has("cmake")
    {
        return Err(ManifestError::at(
            map.line_of("cmake").unwrap_or(0),
            format!(
                "{key}.cmake is {key}.project.cmake and {key}.library.cmake, by the CMakeLists.txt each line goes into."
            ),
        ));
    }

    let given = part(given, &key, &["project", "library"])?;
    let project = part(
        given.and_then(|map| map.get("project")),
        &format!("{key}.project"),
        &["cmake"],
    )?;
    let library = part(
        given.and_then(|map| map.get("library")),
        &format!("{key}.library"),
        &["cmake"],
    )?;

    let mine: Vec<&str> = places
        .iter()
        .copied()
        .filter(|place| *place != own)
        .collect();

    cmake_groups(
        project.and_then(|map| map.get("cmake")),
        library.and_then(|map| map.get("cmake")),
        &key,
        &mine,
        own,
        Some(option_name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::loads;

    fn node(text: &str) -> Node {
        loads(text).unwrap().unwrap()
    }

    fn words(value: &Node) -> Result<Vec<Listed<String>>, ManifestError> {
        listed(Some(value), "k", true, |text| Ok(Some(text.text())))
    }

    #[test]
    fn a_plain_list_has_no_titles_or_notes() {
        let found = words(&node("- A\n- B\n")).unwrap();

        assert_eq!(found[0].item, "A");
        assert_eq!(found[0].group, None);
        assert_eq!(found[1].note, None);
    }

    #[test]
    fn groups_give_their_title_and_a_mapping_of_one_its_note() {
        let found = words(&node("Printing:\n  - A: the note\n  - B\nOther:\n  - C\n")).unwrap();

        assert_eq!(found[0].group.as_deref(), Some("Printing"));
        assert_eq!(found[0].note.as_deref(), Some("the note"));
        assert_eq!(found[2].group.as_deref(), Some("Other"));
    }

    #[test]
    fn a_title_with_nothing_under_it_is_refused() {
        let error = words(&node("Printing:\n")).unwrap_err();

        assert!(
            error.message.contains("no list under it"),
            "{}",
            error.message
        );
        assert_eq!(error.line, Some(1));
    }

    #[test]
    fn a_comment_that_would_read_as_something_else_is_refused() {
        for text in [
            "- A: '>>> x'\n",
            "- A: '<<< x'\n",
            "- A: USER CODE here\n",
            "- A: CONFIG_B is not set\n",
        ] {
            assert!(words(&node(text)).is_err(), "{text}");
        }
    }

    #[test]
    fn an_empty_entry_is_refused() {
        assert!(words(&node("- ~\n")).is_err());
    }

    #[test]
    fn titles_and_notes_are_written_as_comments() {
        let found = words(&node("One:\n  - A: note\n  - B\nTwo:\n  - C\n")).unwrap();
        let stars = "*".repeat(78);

        assert_eq!(
            annotated(&found, String::clone, "#", true),
            [
                format!("# {stars}"),
                "# One".to_string(),
                format!("# {stars}"),
                "# note".to_string(),
                "A".to_string(),
                "B".to_string(),
                String::new(),
                format!("# {stars}"),
                "# Two".to_string(),
                format!("# {stars}"),
                "C".to_string(),
            ]
        );
        assert!(grouped(&found));
    }

    #[test]
    fn a_line_of_cmake_keeps_its_indent_and_takes_no_note() {
        let map =
            node("project:\n  before-zephyr:\n    - if(X)\n    - '  set(Y)'\n    - endif()\n");
        let groups = cmake_groups(
            map.as_map().unwrap().get("project"),
            None,
            "zephyr",
            &["before-zephyr", "after-zephyr"],
            "module",
            None,
        )
        .unwrap();

        assert_eq!(groups[0].label, "before-zephyr");
        assert_eq!(groups[0].lines, ["if(X)", "  set(Y)", "endif()"]);

        let error =
            cmake_groups(None, Some(&node("- a: b\n")), "zephyr", &[], "module", None).unwrap_err();
        assert!(
            error.message.contains("goes in quotes"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_part_with_a_key_it_does_not_have_is_refused() {
        let error = part(
            Some(&node("cmake: []\nbad: 1\n")),
            "zephyr.library",
            &["cmake"],
        )
        .unwrap_err();

        assert_eq!(error.message, "zephyr.library has bad. It holds cmake.");
        assert_eq!(error.line, Some(2));
    }

    #[test]
    fn an_options_groups_are_named_after_it() {
        let given = node(
            "project:\n  cmake:\n    after-zephyr:\n      - a()\nlibrary:\n  cmake:\n    - b()\n",
        );
        let groups = option_cmake(
            "fast",
            Some(&given),
            "zephyr",
            &["module", "before-zephyr", "after-zephyr"],
            "module",
        )
        .unwrap();

        assert_eq!(groups[0].label, "fast-after-zephyr");
        assert_eq!(groups[1].label, "fast-module");
    }
}
