//! Small helpers for reading text and bytes the way project files need it.

use sha2::{Digest, Sha256};

/// The first 16 characters of a SHA-256, which is how the record keeps a fingerprint.
pub fn short_hash(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    let mut text = String::with_capacity(16);

    for byte in digest.iter().take(8) {
        text.push_str(&format!("{byte:02x}"));
    }

    text
}

/// Whether a character ends a line of text. A form feed in a project file does too.
fn ends_line(char: char) -> bool {
    matches!(
        char,
        '\n' | '\r'
            | '\u{0b}'
            | '\u{0c}'
            | '\u{1c}'
            | '\u{1d}'
            | '\u{1e}'
            | '\u{85}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Text split into lines, each with its ending. CRLF is one ending.
pub fn lines_with_endings(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();

    while let Some((index, char)) = chars.next() {
        if !ends_line(char) {
            continue;
        }

        let mut end = index + char.len_utf8();

        if char == '\r'
            && let Some(&(next, '\n')) = chars.peek()
        {
            end = next + 1;
            chars.next();
        }

        lines.push(&text[start..end]);
        start = end;
    }

    if start < text.len() {
        lines.push(&text[start..]);
    }

    lines
}

/// Text split into lines, without their endings.
pub fn lines_of(text: &str) -> Vec<&str> {
    lines_with_endings(text)
        .into_iter()
        .map(|line| line.trim_end_matches(ends_line))
        .collect()
}

/// A line without the one ending it has, a CRLF counted as one.
pub fn without_ending(line: &str) -> &str {
    line.strip_suffix("\r\n")
        .or_else(|| line.strip_suffix('\n'))
        .or_else(|| line.strip_suffix('\r'))
        .unwrap_or(line)
}

/// Bytes split into lines, each with its ending: LF, CR or CRLF.
pub fn byte_lines(data: &[u8]) -> Vec<&[u8]> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;

    while index < data.len() {
        let end = match data[index] {
            b'\n' => index + 1,
            b'\r' if data.get(index + 1) == Some(&b'\n') => index + 2,
            b'\r' => index + 1,
            _ => {
                index += 1;
                continue;
            }
        };

        lines.push(&data[start..end]);
        start = end;
        index = end;
    }

    if start < data.len() {
        lines.push(&data[start..]);
    }

    lines
}

/// Bytes without the line ending characters they end in.
pub fn trim_byte_ending(line: &[u8]) -> &[u8] {
    let mut end = line.len();

    while end > 0 && (line[end - 1] == b'\r' || line[end - 1] == b'\n') {
        end -= 1;
    }

    &line[..end]
}

/// Whether bytes hold nothing but white space.
pub fn is_blank(data: &[u8]) -> bool {
    data.iter()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c))
}

/// Find a sub-slice of bytes.
pub fn find_bytes(data: &[u8], wanted: &[u8]) -> bool {
    !wanted.is_empty() && data.windows(wanted.len()).any(|window| window == wanted)
}

/// The line ending a file uses: CRLF when it has any, otherwise LF.
pub fn line_ending(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// A list of names as a sentence reads it: "a, b and c".
pub fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [head @ .., last] => format!("{} and {last}", head.join(", ")),
    }
}

/// The names given more than once, sorted, each named once.
pub fn given_twice<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let names: Vec<&str> = names.into_iter().collect();
    let mut twice: Vec<String> = names
        .iter()
        .filter(|name| names.iter().filter(|other| other == name).count() > 1)
        .map(|name| name.to_string())
        .collect();

    twice.sort();
    twice.dedup();

    twice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_their_endings() {
        assert_eq!(lines_with_endings("a\r\nb\nc"), ["a\r\n", "b\n", "c"]);
        assert_eq!(lines_of("a\r\nb\n\nc\n"), ["a", "b", "", "c"]);
        assert!(lines_of("").is_empty());
    }

    #[test]
    fn byte_lines_split_at_every_ending() {
        assert_eq!(
            byte_lines(b"a\r\nb\nc\rd"),
            [&b"a\r\n"[..], b"b\n", b"c\r", b"d"]
        );
    }

    #[test]
    fn a_short_hash_is_sixteen_characters() {
        assert_eq!(short_hash(b""), "e3b0c44298fc1c14");
    }
}
