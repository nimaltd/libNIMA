//! Shared pieces for the IDE integrations.
//!
//! Every integration edits a file the user cannot afford to lose. A corrupted .cproject or
//! .uvprojx means a project that will not open, and the user will not connect that to
//! having run an installer. So three rules hold everywhere:
//!
//! 1. Back the file up before touching it.
//! 2. Bail out rather than guess. An integration that does not recognise the file reports
//!    what to do by hand and changes nothing.
//! 3. Be idempotent. Installing twice must not add anything twice.
//!
//! A fourth rule covers how the edit reads afterwards. These files are written by tools,
//! but people do open them, and they land in version control where every line of them is
//! reviewed. So an added line copies the indentation and the line ending of the lines
//! around it, rather than carrying a shape of its own.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::manifest::{COMPILED, Manifest, define_name};
use crate::paths;
use crate::text::lines_of;

/// Every folder of a library that belongs on the include path.
pub fn include_folders(library: &Manifest, folder: &str) -> Vec<String> {
    library
        .include_dirs()
        .iter()
        .map(|dir| {
            if dir == "." {
                folder.to_string()
            } else {
                format!("{folder}/{dir}")
            }
        })
        .collect()
}

/// A forward slash path from root to path, going up where it has to.
///
/// Keil and IAR keep their project file in a subfolder such as MDK or EWARM, so the
/// library sits above it and the answer has to start with "..". Falls back to an absolute
/// path only when the two are on different drives.
pub fn relative(path: &Path, root: &Path) -> String {
    paths::relative_or_full(path, root)
}

/// The path with every backslash made forward, for comparing two paths.
///
/// Keil and IAR read both slashes, and the one written here is whichever the project
/// already uses most. Files the user adds through the IDE can change that, and then the
/// library is in the project under the other slash. It is still the same file, and an
/// exact comparison would add it a second time.
pub fn forward_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// Whether this file is a copy an IDE or libNIMA left lying around.
///
/// IAR writes "Backup of <name>.ewp" beside the real project when it upgrades one, and
/// that copy is a perfectly valid project file whose name sorts before the original.
/// Picking it is the worst kind of failure: the tool reports success, names a file that
/// looks right, and the project the user actually builds is never touched.
pub fn is_backup(path: &str) -> bool {
    let name = paths::leaf(path);

    name.starts_with("Backup of ") || name.ends_with(".bak")
}

/// The whitespace that starts the line holding this offset.
pub fn indent_of(text: &str, at: usize) -> String {
    let start = text[..at].rfind('\n').map_or(0, |index| index + 1);
    let line = &text[start..at];

    line[..line.len() - line.trim_start_matches([' ', '\t']).len()].to_string()
}

/// How deep the children of an element sit.
///
/// Copied from a child that is already there, so it comes out in whatever the tool that
/// wrote the file uses, tabs in a .cproject and spaces elsewhere. Only an element with no
/// children left to copy from has to be guessed at.
///
/// Takes the element's body rather than the whole element, so the element's own line
/// cannot be mistaken for the first child.
pub fn inner_indent(body: &str, outer: &str) -> String {
    static CHILD: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\n([ \t]+)\S").expect("a valid pattern"));

    if let Some(found) = CHILD.captures(body) {
        return found[1].to_string();
    }

    format!("{outer}{}", if outer.ends_with('\t') { "\t" } else { "  " })
}

/// One nesting level, read off as the difference between two of them.
pub fn indent_step(outer: &str, inner: &str) -> String {
    match inner.get(outer.len()..) {
        Some(step) if !step.is_empty() => step.to_string(),
        _ => (if outer.ends_with('\t') { "\t" } else { "  " }).to_string(),
    }
}

/// The indentation an element starts at, and one step into it. Read off an element that
/// is already in the file, so one added beside it lines up with it.
pub fn element_indent(element: &str) -> (String, String) {
    let lines = lines_of(element);
    let leading =
        |line: &str| line[..line.len() - line.trim_start_matches([' ', '\t']).len()].to_string();
    let pad = leading(lines[0]);
    let inner = lines
        .get(1)
        .map_or_else(|| pad.clone(), |line| leading(line));

    let step = indent_step(&pad, &inner);

    (pad, step)
}

/// The files among paths that a compiler takes, as posix names inside destination.
///
/// Used for what an update removed, so that an IDE naming every file can let go of them.
/// Anything outside the library's folder is not the library's.
pub fn compiled_names(found: &[PathBuf], destination: &Path) -> Vec<String> {
    let destination = paths::resolve(destination);

    found
        .iter()
        .filter_map(|path| paths::relative_to(&paths::resolve(path), &destination))
        .map(|rest| paths::posix(&rest))
        .filter(|name| COMPILED.contains(&paths::suffix(name).as_str()))
        .collect()
}

/// What to do to one list of defines in a project file.
pub struct DefinePlan {
    /// Entries of the list to take out, only ever ones the library put there.
    pub remove: Vec<String>,
    /// Wanted defines the list does not have.
    pub add: Vec<String>,
    /// (wanted, present) pairs where the list already sets the same name to something
    /// else. Those are left alone: libNIMA cannot tell a value the user chose on purpose
    /// from a stale one, and changing the user's mind for them is worse than saying so.
    pub clashes: Vec<(String, String)>,
}

/// What to do to one list of defines: `present` as written there, `wanted` what the library
/// asks for, `dropped` what the last install of it added and this one no longer asks for.
pub fn plan_defines(present: &[String], wanted: &[String], dropped: &[String]) -> DefinePlan {
    let present: Vec<String> = present
        .iter()
        .map(|entry| entry.trim().to_string())
        .collect();
    let remove: Vec<String> = present
        .iter()
        .filter(|entry| dropped.contains(entry) && !wanted.contains(entry))
        .cloned()
        .collect();
    let left: Vec<&String> = present
        .iter()
        .filter(|entry| !remove.contains(entry))
        .collect();
    let mut add = Vec::new();
    let mut clashes: Vec<(String, String)> = Vec::new();

    for define in wanted {
        if left.contains(&define) {
            continue;
        }

        // The last of one name is the one a dictionary of them would keep.
        match left
            .iter()
            .rev()
            .find(|entry| define_name(entry) == define_name(define))
        {
            None => add.push(define.clone()),
            Some(have) => {
                let pair = (define.clone(), (*have).clone());

                if !clashes.contains(&pair) {
                    clashes.push(pair);
                }
            }
        }
    }

    DefinePlan {
        remove,
        add,
        clashes,
    }
}

/// Lines for the user about defines left as they were, one per name.
pub fn clash_steps(clashes: &[(String, String)], where_: &str) -> Vec<String> {
    let mut steps = Vec::new();

    for (define, have) in clashes {
        let line = format!(
            "{} is already set as {have} in {where_}, so it was left as it is. The library sets {define}.",
            define_name(define)
        );

        if !steps.contains(&line) {
            steps.push(line);
        }
    }

    steps
}

/// Where a new line goes so that it lands above the element starting at `at`.
///
/// A closing tag usually sits alone on its line, and the caller wants the new content
/// above it rather than wedged between its indentation and the tag. So the answer is the
/// start of the line ending that closes the previous line, which puts the new content at
/// the end of that line instead. The start of it, not the line feed, because landing
/// between the carriage return and the line feed of a CRLF file splits that ending in two
/// and leaves the line above looking changed in the diff. When the tag shares its line
/// with something else there is no such spot, and the offset is returned unchanged.
pub fn insert_before(text: &str, at: usize) -> usize {
    let head = text[..at].rfind('\n').map_or(0, |index| index + 1);

    if head == 0 || !text[head..at].trim().is_empty() {
        return at;
    }

    let end = head - 1;

    if end > 0 && text.as_bytes()[end - 1] == b'\r' {
        end - 1
    } else {
        end
    }
}

/// Whether a path, as a project file writes it, is folder or inside it.
///
/// Compared forward slashed and without a leading ./, so ..\Libs\ee24\ee24.c is under
/// ../Libs/ee24 whichever slash the project writes.
pub fn under(path: &str, folder: &str) -> bool {
    let clean = |text: &str| {
        let text = forward_slashes(text);
        let text = text.trim().trim_end_matches('/').to_string();

        text.strip_prefix("./").map_or(text.clone(), str::to_string)
    };

    let (path, folder) = (clean(path), clean(folder));

    path == folder || path.starts_with(&format!("{folder}/"))
}

/// A list of what was told a project file, as a sentence reads it.
pub fn told(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn a_backup_is_told_by_its_name() {
        assert!(is_backup("EWARM/Backup of Proj.ewp"));
        assert!(is_backup("a/Proj.ewp.20260101-000000.bak"));
        assert!(!is_backup("EWARM/Proj.ewp"));
    }

    #[test]
    fn an_added_line_copies_what_is_around_it() {
        assert_eq!(inner_indent("\n\t\t<a/>\n\t", "\t"), "\t\t");
        assert_eq!(inner_indent("", "\t"), "\t\t");
        assert_eq!(inner_indent("", "  "), "    ");
        assert_eq!(indent_of("<a>\n    <b>", 8), "    ");
        assert_eq!(indent_step("  ", "    "), "  ");
        assert_eq!(indent_step("\t", "\t"), "\t");
        assert_eq!(
            element_indent("    <File>\n      <FileName>x</FileName>\n    </File>\n"),
            ("    ".to_string(), "  ".to_string())
        );
    }

    #[test]
    fn a_new_line_goes_at_the_end_of_the_line_above() {
        let text = "<a>\n  <b/>\n</a>\n";

        assert_eq!(
            insert_before(text, text.find("</a>").unwrap()),
            text.find("\n</a>").unwrap()
        );

        let crlf = "<a>\r\n  <b/>\r\n</a>\r\n";

        assert_eq!(
            insert_before(crlf, crlf.find("</a>").unwrap()),
            crlf.find("\r\n</a>").unwrap()
        );

        let shared = "<a><b/></a>";

        assert_eq!(insert_before(shared, 7), 7);
    }

    #[test]
    fn a_plan_only_takes_out_what_the_library_put_there() {
        let plan = plan_defines(
            &strings(&["USE_HAL_DRIVER", "OLD", "KEEP"]),
            &strings(&["NEW", "KEEP"]),
            &strings(&["OLD"]),
        );

        assert_eq!(plan.remove, ["OLD"]);
        assert_eq!(plan.add, ["NEW"]);
        assert!(plan.clashes.is_empty());
    }

    #[test]
    fn a_name_set_to_something_else_is_left_and_said() {
        let plan = plan_defines(&strings(&["A=1"]), &strings(&["A=2"]), &[]);

        assert!(plan.add.is_empty());
        assert_eq!(plan.clashes, [("A=2".to_string(), "A=1".to_string())]);
        assert_eq!(
            clash_steps(&plan.clashes, "x"),
            ["A is already set as A=1 in x, so it was left as it is. The library sets A=2."]
        );
    }

    #[test]
    fn under_reads_either_slash() {
        assert!(under("..\\Libs\\ee24\\ee24.c", "../Libs/ee24"));
        assert!(under("./Libs/ee24", "Libs/ee24"));
        assert!(!under("Libs/ee24x/a.c", "Libs/ee24"));
    }
}
