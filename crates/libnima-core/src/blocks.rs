//! Marked blocks in a project's own files, the same on every platform.
//!
//! libNIMA adds a library's lines to a file the project owns, such as a CMakeLists.txt
//! or platformio.ini, between two marker lines that name it:
//!
//! ```text
//! # >>> libnima: gcore >>>
//! ...
//! # <<< libnima: gcore <<<
//! ```
//!
//! and finds them again by those markers on the next install, to write them anew or
//! take them out. A label after the library's name keeps several blocks of one library
//! apart, "gcore/conf" and "gcore/report", and the sign is the file's own comment sign:
//! # in CMake and Kconfig, ; in platformio.ini.
//!
//! Three rules hold in every file:
//!
//! - A file with only one marker of a block is never changed: whatever is written could
//!   land in the wrong place. The lines to add by hand are given instead.
//! - The file is backed up before any change, and its line endings are kept.
//! - Lines outside the blocks are never touched.
//!
//! What each block held when it was written is kept in the record, as a fingerprint, so
//! the next install can tell the user edited inside one.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use crate::text::short_hash;

/// How the markers name this tool.
pub const INSTALLER: &str = "libnima";

/// A marker with only one of its two lines in the file, or standing the wrong way round.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HalfWritten;

/// The two lines around one block, and the name the record keeps it by.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub name: String,
    pub open: String,
    pub close: String,
}

impl Marker {
    pub fn new(library: &str, label: Option<&str>) -> Marker {
        Marker::signed(library, label, "#")
    }

    pub fn signed(library: &str, label: Option<&str>, sign: &str) -> Marker {
        let name = match label {
            Some(label) => format!("{library}/{label}"),
            None => library.to_string(),
        };

        Marker {
            open: format!("{sign} >>> {INSTALLER}: {name} >>>"),
            close: format!("{sign} <<< {INSTALLER}: {name} <<<"),
            name,
        }
    }
}

/// Where a block is in text: from its first marker's first character to its last
/// marker's last. None when there is none.
pub fn span(text: &str, marker: &Marker) -> Result<Option<(usize, usize)>, HalfWritten> {
    Ok(spans(text, marker)?.first().copied())
}

fn find_from(text: &str, wanted: &str, at: usize) -> Option<usize> {
    text[at..].find(wanted).map(|index| index + at)
}

/// Every block of this marker in text, for a file that holds one in several places, as
/// platformio.ini does in each environment.
pub fn spans(text: &str, marker: &Marker) -> Result<Vec<(usize, usize)>, HalfWritten> {
    let mut found = Vec::new();
    let mut at = 0;

    loop {
        let start = find_from(text, &marker.open, at);
        let stray = find_from(text, &marker.close, at);

        let Some(start) = start else {
            // A closing marker with no opening one before it.
            return if stray.is_some() {
                Err(HalfWritten)
            } else {
                Ok(found)
            };
        };

        if stray.is_some_and(|stray| stray < start) {
            return Err(HalfWritten);
        }

        let after = start + marker.open.len();
        let Some(end) = find_from(text, &marker.close, after) else {
            return Err(HalfWritten);
        };

        if find_from(text, &marker.open, after).is_some_and(|nested| nested < end) {
            return Err(HalfWritten);
        }

        found.push((start, end + marker.close.len()));
        at = end + marker.close.len();
    }
}

/// Where the line holding position `at` begins.
pub fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |index| index + 1)
}

/// The lines between a block's two markers, as text.
pub fn body(text: &str, (start, end): (usize, usize)) -> &str {
    let last = line_start(text, end);

    match find_from(text, "\n", start) {
        Some(first) if first < last => &text[first + 1..last],
        _ => "",
    }
}

/// What a block holds, as a short hash. Line endings and the indentation the block was
/// written with do not count, since an editor changes those freely.
pub fn fingerprint(text: &str) -> String {
    let joined = text.replace("\r\n", "\n");
    let lines: Vec<&str> = joined
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();

    short_hash(lines.join("\n").as_bytes())
}

/// The text with every block of this marker taken out, for reading what is the user's.
pub fn outside(text: &str, marker: &Marker) -> String {
    let Ok(found) = spans(text, marker) else {
        return text.to_string();
    };

    let mut out = String::with_capacity(text.len());
    let mut at = 0;

    for (start, end) in found {
        out.push_str(&text[at..start]);
        at = end;
    }

    out.push_str(&text[at..]);

    out
}

/// The text with a block taken out, with the line it stood on and the one blank line
/// that came with it: below it, or above it when it ends the file.
pub fn cut(text: &str, (start, end): (usize, usize), newline: &str) -> String {
    let mut start = line_start(text, start);
    let mut end = end;

    if text[end..].starts_with(newline) {
        end += newline.len();
    }

    if text[end..].starts_with(newline) {
        end += newline.len();
    } else if end == text.len() && text[..start].ends_with(&format!("{newline}{newline}")) {
        start -= newline.len();
    }

    format!("{}{}", &text[..start], &text[end..])
}

/// Whether the user changed what is inside a block since it was written, by the
/// fingerprint the record keeps. False when the record does not know it.
pub fn edited(recorded: Option<&Value>, file_name: &str, marker: &Marker, text: &str) -> bool {
    let Ok(Some(found)) = span(text, marker) else {
        return false;
    };

    let known = recorded
        .and_then(Value::as_object)
        .and_then(|files| files.get(file_name))
        .and_then(Value::as_object)
        .and_then(|blocks| blocks.get(&marker.name));

    match known {
        Some(known) => known.as_str() != Some(fingerprint(body(text, found)).as_str()),
        None => false,
    }
}

/// {marker name: fingerprint} for each of these blocks found in text.
pub fn fingerprints(text: &str, markers: &[Marker]) -> Map<String, Value> {
    let mut found = Map::new();

    for marker in markers {
        if let Ok(Some(place)) = span(text, marker) {
            found.insert(
                marker.name.clone(),
                Value::String(fingerprint(body(text, place))),
            );
        }
    }

    found
}

/// The labels of this library's labelled blocks in text, in the order they stand.
pub fn labels(text: &str, library: &str) -> Vec<String> {
    static LABEL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_-]* >>>").expect("a valid pattern"));

    let prefix = format!("# >>> {INSTALLER}: {library}/");
    let mut found: Vec<String> = Vec::new();
    let mut at = 0;

    while let Some(start) = find_from(text, &prefix, at) {
        let rest = &text[start + prefix.len()..];
        at = start + prefix.len();

        if let Some(matched) = LABEL.find(rest) {
            let label = matched.as_str().trim_end_matches(" >>>").to_string();

            if !found.contains(&label) {
                found.push(label);
            }
        }
    }

    found
}

/// Where in a text a block goes: the start of the line it goes above, when there is one.
pub type Anchor<'a> = &'a dyn Fn(&str) -> Option<usize>;

/// text with a block of content in its place: taken out from wherever it is, and put
/// back above the line `anchor(text)` gives the start of, with a blank line after it,
/// or, with no anchor or none found, at the end of the file after a blank line. Done
/// that way, a block already in its place comes back exactly as it was.
pub fn place(
    text: &str,
    marker: &Marker,
    content: &[String],
    newline: &str,
    anchor: Option<Anchor<'_>>,
) -> Result<String, HalfWritten> {
    let found = span(text, marker)?;
    let rest = match found {
        Some(found) => cut(text, found, newline),
        None => text.to_string(),
    };

    let mut rows = vec![marker.open.clone()];
    rows.extend_from_slice(content);
    rows.push(marker.close.clone());
    let block = rows.join(newline);

    if let Some(at) = anchor.and_then(|anchor| anchor(&rest)) {
        return Ok(format!(
            "{}{block}{newline}{newline}{}",
            &rest[..at],
            &rest[at..]
        ));
    }

    let head = rest.trim_end_matches(['\r', '\n']);

    Ok(if head.is_empty() {
        format!("{block}{newline}")
    } else {
        format!("{head}{newline}{newline}{block}{newline}")
    })
}

/// Lines a library adds to a file of the project's, with a label and a place, such as a
/// group of CMake lines for a Zephyr application's CMakeLists.txt.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub label: String,
    pub place: String,
    pub lines: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker() -> Marker {
        Marker::new("gcore", None)
    }

    #[test]
    fn a_marker_names_the_library_and_its_label() {
        let found = Marker::new("gcore", Some("conf"));

        assert_eq!(found.name, "gcore/conf");
        assert_eq!(found.open, "# >>> libnima: gcore/conf >>>");
        assert_eq!(found.close, "# <<< libnima: gcore/conf <<<");
        assert_eq!(Marker::signed("x", None, ";").open, "; >>> libnima: x >>>");
    }

    #[test]
    fn a_block_is_found_between_its_markers() {
        let text = "a\n# >>> libnima: gcore >>>\nb\n# <<< libnima: gcore <<<\nc\n";
        let (start, end) = span(text, &marker()).unwrap().unwrap();

        assert_eq!(
            &text[start..end],
            "# >>> libnima: gcore >>>\nb\n# <<< libnima: gcore <<<"
        );
        assert_eq!(body(text, (start, end)), "b\n");
    }

    #[test]
    fn no_block_is_none() {
        assert_eq!(span("nothing here\n", &marker()), Ok(None));
    }

    #[test]
    fn a_marker_without_its_partner_is_half_written() {
        for text in [
            "# >>> libnima: gcore >>>\n",
            "# <<< libnima: gcore <<<\n",
            "# <<< libnima: gcore <<<\n# >>> libnima: gcore >>>\n",
            "# >>> libnima: gcore >>>\n# >>> libnima: gcore >>>\n# <<< libnima: gcore <<<\n",
        ] {
            assert_eq!(span(text, &marker()), Err(HalfWritten), "{text:?}");
        }
    }

    #[test]
    fn a_block_in_several_places_is_found_in_each() {
        let text = "; >>> libnima: x >>>\n; <<< libnima: x <<<\n\n; >>> libnima: x >>>\n; <<< libnima: x <<<\n";

        assert_eq!(
            spans(text, &Marker::signed("x", None, ";")).unwrap().len(),
            2
        );
    }

    #[test]
    fn a_fingerprint_ignores_line_endings_and_indentation() {
        assert_eq!(fingerprint("a\n  b\n"), fingerprint("a\r\nb\r\n\r\n"));
        assert_ne!(fingerprint("a\n"), fingerprint("b\n"));
    }

    #[test]
    fn cut_takes_the_blank_line_below_it_too() {
        let text = "a\n\n# >>> libnima: gcore >>>\nb\n# <<< libnima: gcore <<<\n\nc\n";
        let found = span(text, &marker()).unwrap().unwrap();

        assert_eq!(cut(text, found, "\n"), "a\n\nc\n");
    }

    #[test]
    fn cut_takes_the_blank_line_above_it_when_it_ends_the_file() {
        let text = "a\n\n# >>> libnima: gcore >>>\n# <<< libnima: gcore <<<";
        let found = span(text, &marker()).unwrap().unwrap();

        assert_eq!(cut(text, found, "\n"), "a\n");
    }

    #[test]
    fn labels_are_listed_in_the_order_they_stand() {
        let text = "# >>> libnima: gcore/report >>>\n# <<< libnima: gcore/report <<<\n\
                    # >>> libnima: gcore/conf >>>\n# <<< libnima: gcore/conf <<<\n\
                    # >>> libnima: gcore/zephyr.conf >>>\n# <<< libnima: gcore/zephyr.conf <<<\n\
                    # >>> libnima: other/x >>>\n# <<< libnima: other/x <<<\n";

        assert_eq!(labels(text, "gcore"), ["report", "conf"]);
    }

    #[test]
    fn place_is_the_same_the_second_time() {
        let lines = vec!["b".to_string()];
        let first = place("a\n", &marker(), &lines, "\n", None).unwrap();

        assert_eq!(
            first,
            "a\n\n# >>> libnima: gcore >>>\nb\n# <<< libnima: gcore <<<\n"
        );
        assert_eq!(place(&first, &marker(), &lines, "\n", None).unwrap(), first);
    }

    #[test]
    fn place_goes_above_its_anchor_and_stays() {
        let anchor: &dyn Fn(&str) -> Option<usize> = &|text: &str| text.find("find_package");
        let lines = vec!["b".to_string()];
        let first = place(
            "top\nfind_package(Zephyr)\n",
            &marker(),
            &lines,
            "\n",
            Some(anchor),
        )
        .unwrap();

        assert_eq!(
            first,
            "top\n# >>> libnima: gcore >>>\nb\n# <<< libnima: gcore <<<\n\nfind_package(Zephyr)\n"
        );
        assert_eq!(
            place(&first, &marker(), &lines, "\n", Some(anchor)).unwrap(),
            first
        );
    }

    #[test]
    fn a_stray_end_before_a_whole_block_is_half_written() {
        let text =
            "# <<< libnima: gcore <<<\n# >>> libnima: gcore >>>\nx\n# <<< libnima: gcore <<<\n";

        assert_eq!(span(text, &marker()), Err(HalfWritten));
    }

    #[test]
    fn a_label_keeps_two_blocks_of_one_library_apart() {
        let conf = Marker::new("gcore", Some("conf"));
        let text = "# >>> libnima: gcore >>>\na()\n# <<< libnima: gcore <<<\n\
                    # >>> libnima: gcore/conf >>>\nc()\n# <<< libnima: gcore/conf <<<\n";

        assert_eq!(body(text, span(text, &conf).unwrap().unwrap()), "c()\n");
        assert_eq!(body(text, span(text, &marker()).unwrap().unwrap()), "a()\n");
    }

    #[test]
    fn taken_out_indented_it_takes_its_indentation() {
        let text = "x\n    # >>> libnima: gcore >>>\n    a()\n    # <<< libnima: gcore <<<\ny\n";
        let found = span(text, &marker()).unwrap().unwrap();

        assert_eq!(cut(text, found, "\n"), "x\ny\n");
    }

    #[test]
    fn crlf_is_kept_when_a_block_is_taken_out() {
        let text =
            "top\r\n# >>> libnima: gcore >>>\r\na()\r\n# <<< libnima: gcore <<<\r\n\r\nbottom\r\n";
        let found = span(text, &marker()).unwrap().unwrap();

        assert_eq!(cut(text, found, "\r\n"), "top\r\nbottom\r\n");
    }

    #[test]
    fn outside_is_the_users_part() {
        let text = "mine\n# >>> libnima: gcore >>>\na()\n# <<< libnima: gcore <<<\nalso mine\n";

        assert_eq!(outside(text, &marker()), "mine\n\nalso mine\n");
    }

    #[test]
    fn a_block_with_no_anchor_goes_at_the_end_after_a_blank_line() {
        let lines = vec!["a()".to_string()];
        let block = "# >>> libnima: gcore >>>\na()\n# <<< libnima: gcore <<<\n";

        assert_eq!(
            place("x()\n", &marker(), &lines, "\n", None).unwrap(),
            format!("x()\n\n{block}")
        );
        assert_eq!(place("", &marker(), &lines, "\n", None).unwrap(), block);
    }

    #[test]
    fn two_blocks_in_one_place_keep_their_order() {
        let anchor: &dyn Fn(&str) -> Option<usize> = &|text: &str| text.find("find_package");
        let first = Marker::new("gcore", Some("a"));
        let second = Marker::new("gcore", Some("b"));
        let mut text = "find_package(Zephyr)\n".to_string();

        for _ in 0..2 {
            text = place(&text, &first, &["a()".to_string()], "\n", Some(anchor)).unwrap();
            text = place(&text, &second, &["b()".to_string()], "\n", Some(anchor)).unwrap();
        }

        let at = |wanted: &str| text.find(wanted).unwrap();

        assert!(at(&first.open) < at(&second.open) && at(&second.open) < at("find_package"));
        assert_eq!(text.matches(&first.open).count(), 1);
    }

    #[test]
    fn a_block_found_away_from_its_anchor_is_moved() {
        let anchor: &dyn Fn(&str) -> Option<usize> = &|text: &str| text.find("find_package");
        let text =
            "find_package(Zephyr)\n\n# >>> libnima: gcore >>>\na()\n# <<< libnima: gcore <<<\n";
        let moved = place(text, &marker(), &["a()".to_string()], "\n", Some(anchor)).unwrap();

        assert!(
            moved.find(&marker().open) < moved.find("find_package"),
            "{moved}"
        );
    }

    #[test]
    fn a_half_written_block_is_not_placed() {
        let text = format!("{}\n", marker().open);

        assert_eq!(place(&text, &marker(), &[], "\n", None), Err(HalfWritten));
    }

    #[test]
    fn crlf_is_kept_when_placing() {
        let placed = place("x()\r\n", &marker(), &["a()".to_string()], "\r\n", None).unwrap();

        assert!(!placed.replace("\r\n", "").contains('\n'), "{placed:?}");
    }

    #[test]
    fn an_edit_inside_a_block_is_told_by_its_fingerprint() {
        let text = "# >>> libnima: gcore >>>\nmine\n# <<< libnima: gcore <<<\n";
        let same = serde_json::json!({"prj.conf": {"gcore": fingerprint("mine\n")}});
        let other = serde_json::json!({"prj.conf": {"gcore": fingerprint("theirs\n")}});

        assert!(!edited(Some(&same), "prj.conf", &marker(), text));
        assert!(edited(Some(&other), "prj.conf", &marker(), text));
        assert!(!edited(None, "prj.conf", &marker(), text));
        assert!(!edited(Some(&other), "other.conf", &marker(), text));
    }
}
