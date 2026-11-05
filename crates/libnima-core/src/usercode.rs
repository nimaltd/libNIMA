//! Sections of a library file that belong to the user, marked the way STM32CubeMX
//! marks its own generated files:
//!
//! ```c
//! /* USER CODE BEGIN SEQ_CONFIGURATION */
//! #define SEQ_MAX_TASKS  32
//! /* USER CODE END SEQ_CONFIGURATION */
//! ```
//!
//! Every install copies the library's files afresh, so an update updates and a library
//! the user broke by accident is put right. What sits between a BEGIN and its END is
//! carried from the copy already in the project into the new file, in the section of
//! the same name.
//!
//! Files are handled as bytes. Only the markers have to be ASCII, and the user's text
//! goes back exactly as it was, apart from its line endings, which follow the new file
//! so that one file never mixes the two.

use std::fmt;
use std::sync::LazyLock;

use regex::bytes::Regex;

use crate::text::{byte_lines, find_bytes, is_blank, short_hash, trim_byte_ending};

/// A marker is a comment alone on its line, the way CubeMX writes it, so a sentence or
/// a README that only mentions one is not taken for one. The name is one word:
/// Includes, PV, 0, SEQ_CONFIGURATION.
static MARKER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u)\A[ \t]*(?:/\*|//|#)[ \t]*USER CODE (BEGIN|END)[ \t]+([^\s*]+)[ \t]*(?:\*/)?[ \t]*\z")
        .expect("a valid pattern")
});

/// The markers in a file do not pair up, with the line that shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkerError(pub String);

impl fmt::Display for MarkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A file's lines with their endings, and each section's name with where its body is:
/// `lines[first..last]`, the lines between its BEGIN and its END.
pub struct Parsed<'a> {
    pub lines: Vec<&'a [u8]>,
    pub sections: Vec<(String, usize, usize)>,
}

impl Parsed<'_> {
    fn section(&self, name: &str) -> Option<(usize, usize)> {
        self.sections
            .iter()
            .find(|(found, _, _)| found == name)
            .map(|(_, first, last)| (*first, *last))
    }

    /// Whether any section has something in it.
    pub fn holds_something(&self) -> bool {
        self.sections
            .iter()
            .any(|(_, first, last)| !blank_lines(&self.lines[*first..*last]))
    }
}

fn blank_lines(lines: &[&[u8]]) -> bool {
    lines.iter().all(|line| is_blank(line))
}

fn ascii_name(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| {
            if byte.is_ascii() {
                *byte as char
            } else {
                char::REPLACEMENT_CHARACTER
            }
        })
        .collect()
}

/// Split a file into its lines and its sections.
///
/// An error is a BEGIN with no END, an END with no BEGIN, a section that begins inside
/// another, or one name used twice.
pub fn parse(data: &[u8]) -> Result<Parsed<'_>, MarkerError> {
    let lines = byte_lines(data);
    let mut sections: Vec<(String, usize, usize)> = Vec::new();
    let mut open: Option<(String, usize)> = None;

    // Most files have no marker at all, and most lines of those that do are not one.
    if !find_bytes(data, b"USER CODE ") {
        return Ok(Parsed { lines, sections });
    }

    for (index, line) in lines.iter().enumerate() {
        let number = index + 1;
        let Some(marker) = MARKER.captures(trim_byte_ending(line)) else {
            continue;
        };

        let name = ascii_name(&marker[2]);

        if &marker[1] == b"BEGIN" {
            if let Some((inside, _)) = &open {
                return Err(MarkerError(format!(
                    "line {number}: {name} begins inside {inside}"
                )));
            }

            if sections.iter().any(|(found, _, _)| *found == name) {
                return Err(MarkerError(format!("line {number}: {name} is used twice")));
            }

            open = Some((name, number));
        } else {
            let Some((inside, first)) = open.take() else {
                return Err(MarkerError(format!(
                    "line {number}: END {name} has no BEGIN"
                )));
            };

            if name != inside {
                return Err(MarkerError(format!(
                    "line {number}: END {name} closes {inside}"
                )));
            }

            sections.push((name, first, number - 1));
        }
    }

    if let Some((inside, _)) = open {
        return Err(MarkerError(format!("{inside} has no END")));
    }

    Ok(Parsed { lines, sections })
}

/// The line ending a file uses: CRLF when it has any, otherwise LF.
pub fn line_ending(data: &[u8]) -> &'static [u8] {
    if find_bytes(data, b"\r\n") {
        b"\r\n"
    } else {
        b"\n"
    }
}

/// A new file with the user's sections from the old one put back in.
pub struct Merge {
    pub data: Vec<u8>,
    /// Sections carried over with something in them.
    pub kept: Vec<String>,
    /// Sections of the old file with something in them that the new file has no
    /// section of the same name for.
    pub lost: Vec<String>,
    /// Why the old file's sections could not be read, or None.
    pub broken: Option<String>,
}

/// Put the user's sections from `old` into `new`.
///
/// An old file whose markers do not pair gives nothing to carry, and says why in
/// `broken`. One with no markers at all is not broken: it simply has nothing to carry.
/// An error comes from the new file, which is the library's mistake.
pub fn merge(new: &[u8], old: &[u8]) -> Result<Merge, MarkerError> {
    let fresh = parse(new)?;
    let (before, broken) = match parse(old) {
        Ok(parsed) => (parsed, None),
        Err(error) => (
            Parsed {
                lines: Vec::new(),
                sections: Vec::new(),
            },
            Some(error.0),
        ),
    };

    let ending = line_ending(new);
    let mut kept = Vec::new();
    // Where each carried section starts in the new file: its first line, the line it
    // ends before, and the body to put there.
    let mut bodies: Vec<(usize, usize, Vec<Vec<u8>>)> = Vec::new();

    for (name, first, last) in &fresh.sections {
        let Some((old_first, old_last)) = before.section(name) else {
            continue;
        };

        let body = &before.lines[old_first..old_last];

        bodies.push((
            *first,
            *last,
            body.iter()
                .map(|line| [trim_byte_ending(line), ending].concat())
                .collect(),
        ));

        if !blank_lines(body) {
            kept.push(name.clone());
        }
    }

    let mut out = Vec::with_capacity(new.len());
    let mut index = 0;

    while index < fresh.lines.len() {
        out.extend_from_slice(fresh.lines[index]);

        match bodies.iter().find(|(first, _, _)| *first == index + 1) {
            Some((_, last, body)) => {
                for line in body {
                    out.extend_from_slice(line);
                }

                index = *last;
            }
            None => index += 1,
        }
    }

    let lost = before
        .sections
        .iter()
        .filter(|(name, first, last)| {
            fresh.section(name).is_none() && !blank_lines(&before.lines[*first..*last])
        })
        .map(|(name, _, _)| name.clone())
        .collect();

    Ok(Merge {
        data: out,
        kept,
        lost,
        broken,
    })
}

/// What a file holds outside its sections, as a short hash.
///
/// libNIMA keeps this for every file it writes. A copy that no longer matches it was
/// changed by hand somewhere the next install will replace. Line endings do not count,
/// since git and editors change those on their own.
pub fn fingerprint(data: &[u8]) -> String {
    let parsed = parse(data).unwrap_or_else(|_| Parsed {
        lines: byte_lines(data),
        sections: Vec::new(),
    });

    let mut outside = Vec::with_capacity(data.len());
    let mut first = true;

    for (index, line) in parsed.lines.iter().enumerate() {
        if parsed
            .sections
            .iter()
            .any(|(_, start, end)| index >= *start && index < *end)
        {
            continue;
        }

        if !first {
            outside.push(b'\n');
        }

        first = false;
        outside.extend_from_slice(trim_byte_ending(line));
    }

    short_hash(&outside)
}

/// What a file holds, the whole of it, as a short hash: for a file copied as it is, such as an extra, which has no sections
/// of the user's, and whose markers, when it shows some, are only an example. Line endings do not count, as in
/// fingerprint().
pub fn whole_fingerprint(data: &[u8]) -> String {
    let lines: Vec<&[u8]> = byte_lines(data).into_iter().map(trim_byte_ending).collect();

    short_hash(&lines.join(&b'\n'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIBRARY: &[u8] = b"#define A 1\n/* USER CODE BEGIN Settings */\n#define B 2\n/* USER CODE END Settings */\nint x;\n";

    fn names(parsed: &Parsed) -> Vec<String> {
        parsed
            .sections
            .iter()
            .map(|(name, _, _)| name.clone())
            .collect()
    }

    #[test]
    fn sections_are_found_by_their_markers() {
        let parsed = parse(LIBRARY).unwrap();

        assert_eq!(names(&parsed), ["Settings"]);
        assert_eq!(parsed.lines[2], b"#define B 2\n");
        assert_eq!(parsed.section("Settings"), Some((2, 3)));
    }

    #[test]
    fn every_comment_style_counts() {
        for line in [
            "/* USER CODE BEGIN X */",
            "// USER CODE BEGIN X",
            "# USER CODE BEGIN X",
            "   /*USER CODE BEGIN X*/   ",
        ] {
            let text = format!("{line}\n/* USER CODE END X */\n");
            assert_eq!(names(&parse(text.as_bytes()).unwrap()), ["X"], "{line}");
        }
    }

    #[test]
    fn a_sentence_that_mentions_a_marker_is_not_one() {
        let text = b"Put it between USER CODE BEGIN and END.\nsee /* USER CODE BEGIN X */ here\n";

        assert!(parse(text).unwrap().sections.is_empty());
    }

    #[test]
    fn markers_that_do_not_pair_are_refused() {
        for (text, wanted) in [
            (&b"/* USER CODE BEGIN A */\n"[..], "A has no END"),
            (b"/* USER CODE END A */\n", "line 1: END A has no BEGIN"),
            (
                b"/* USER CODE BEGIN A */\n/* USER CODE BEGIN B */\n",
                "line 2: B begins inside A",
            ),
            (
                b"/* USER CODE BEGIN A */\n/* USER CODE END B */\n",
                "line 2: END B closes A",
            ),
            (
                b"/* USER CODE BEGIN A */\n/* USER CODE END A */\n/* USER CODE BEGIN A */\n/* USER CODE END A */\n",
                "line 3: A is used twice",
            ),
        ] {
            assert_eq!(parse(text).err().unwrap().0, wanted);
        }
    }

    #[test]
    fn what_the_user_wrote_goes_into_the_new_file() {
        let new = b"new top\n/* USER CODE BEGIN Settings */\n#define B 2\n/* USER CODE END Settings */\nnew end\n";
        let old = b"old top\n/* USER CODE BEGIN Settings */\n#define B 99\n#define C 1\n/* USER CODE END Settings */\nold end\n";
        let merged = merge(new, old).unwrap();

        assert_eq!(
            merged.data,
            b"new top\n/* USER CODE BEGIN Settings */\n#define B 99\n#define C 1\n/* USER CODE END Settings */\nnew end\n"
        );
        assert_eq!(merged.kept, ["Settings"]);
        assert!(merged.lost.is_empty());
    }

    #[test]
    fn the_new_file_decides_the_line_endings() {
        let new = b"/* USER CODE BEGIN S */\r\n/* USER CODE END S */\r\n";
        let old = b"/* USER CODE BEGIN S */\nmine\n/* USER CODE END S */\n";

        assert_eq!(
            merge(new, old).unwrap().data,
            b"/* USER CODE BEGIN S */\r\nmine\r\n/* USER CODE END S */\r\n"
        );
    }

    #[test]
    fn an_empty_section_is_not_called_kept() {
        let new = b"/* USER CODE BEGIN S */\ndefault\n/* USER CODE END S */\n";
        let old = b"/* USER CODE BEGIN S */\n\n/* USER CODE END S */\n";
        let merged = merge(new, old).unwrap();

        assert!(merged.kept.is_empty());
        assert_eq!(merged.data, old);
    }

    #[test]
    fn a_section_the_new_file_has_no_place_for_is_lost_and_said() {
        let new = b"new\n";
        let old = b"/* USER CODE BEGIN Gone */\nmine\n/* USER CODE END Gone */\n";
        let merged = merge(new, old).unwrap();

        assert_eq!(merged.lost, ["Gone"]);
        assert_eq!(merged.data, new);
    }

    #[test]
    fn an_old_file_that_does_not_pair_carries_nothing_and_says_why() {
        let merged = merge(LIBRARY, b"/* USER CODE BEGIN Settings */\nmine\n").unwrap();

        assert_eq!(merged.broken.as_deref(), Some("Settings has no END"));
        assert_eq!(merged.data, LIBRARY);
    }

    #[test]
    fn a_new_file_that_does_not_pair_is_the_librarys_mistake() {
        assert!(merge(b"/* USER CODE BEGIN A */\n", LIBRARY).is_err());
    }

    #[test]
    fn a_fingerprint_looks_only_outside_the_sections() {
        let one = fingerprint(b"a\n/* USER CODE BEGIN S */\none\n/* USER CODE END S */\nb\n");
        let two = fingerprint(b"a\n/* USER CODE BEGIN S */\ntwo\n/* USER CODE END S */\nb\n");
        let edited = fingerprint(b"a!\n/* USER CODE BEGIN S */\none\n/* USER CODE END S */\nb\n");
        let crlf =
            fingerprint(b"a\r\n/* USER CODE BEGIN S */\r\none\r\n/* USER CODE END S */\r\nb\r\n");

        assert_eq!(one, two);
        assert_eq!(one, crlf);
        assert_ne!(one, edited);
    }
}
