//! A reader for the slice of YAML that `libnima.yml` uses.
//!
//! It keeps the line each value stands on, so a problem in a file can name its
//! line, which is what the editor and `libnima --lint` show.
//!
//! What is supported, which is everything the format uses:
//!
//! ```text
//! key: value                  mappings
//! nested:                     nested mappings, by indentation
//!   key: value
//! list:                       block sequences
//!   - item
//!   - key: value              block sequences of mappings
//!     other: value
//! flow: [a, [b], {c: d}]      lists and mappings in brackets, on one line
//! # comments                  to end of line
//! 'it''s' and "a\tb"           strings, with what each kind of quote escapes
//! true false yes no null      the scalars YAML resolves the same way
//! help: |                     block text, kept line for line, with |- and |+
//!   Two lines,                to say what happens to the line endings at its
//!   kept as they are.         end; > folds the lines into one, the same way
//! ```
//!
//! What is not supported, because no manifest needs it: anchors and aliases, tags,
//! multiple documents, brackets or quotes that go on over several lines, complex
//! keys, and a block of text with an indentation number or with lines indented deeper
//! inside a folded one. Anything unsupported is an error naming the line, rather than
//! being guessed at, because a wrong guess would be far worse than a refusal.

use std::fmt;
use std::sync::LazyLock;

use regex::Regex;

/// A line whose value is a block of text on the lines below it: "help: |", "- |"
/// or "- help: >-". The style and the chomping, nothing after them.
static BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^-|^-?.*?:)[ \t]+(?P<style>[|>])(?P<chomp>[-+]?)$").expect("a valid pattern")
});

/// One value, and the line it stands on.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub value: Value,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Vec<Node>),
    Map(Mapping),
}

/// A mapping, in the order the file gives it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Mapping {
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub line: usize,
    pub value: Node,
}

impl Mapping {
    /// The value of a key, a null one included.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| &entry.value)
    }

    /// The value of a key, when it is there and is not null.
    pub fn given(&self, key: &str) -> Option<&Node> {
        self.get(key).filter(|node| !node.is_null())
    }

    pub fn has(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// The line a key stands on.
    pub fn line_of(&self, key: &str) -> Option<usize> {
        self.entries
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.line)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|entry| entry.key.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Node {
    pub fn new(value: Value, line: usize) -> Node {
        Node { value, line }
    }

    pub fn string(text: &str, line: usize) -> Node {
        Node::new(Value::Str(text.to_string()), line)
    }

    pub fn is_null(&self) -> bool {
        matches!(self.value, Value::Null)
    }

    pub fn as_map(&self) -> Option<&Mapping> {
        match &self.value {
            Value::Map(map) => Some(map),
            _ => None,
        }
    }

    pub fn as_list(&self) -> Option<&[Node]> {
        match &self.value {
            Value::List(list) => Some(list),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match &self.value {
            Value::Str(text) => Some(text),
            _ => None,
        }
    }

    /// Whether YAML would call this true: nothing, zero, and empty text or lists are not.
    pub fn truthy(&self) -> bool {
        match &self.value {
            Value::Null => false,
            Value::Bool(value) => *value,
            Value::Int(value) => *value != 0,
            Value::Float(value) => *value != 0.0,
            Value::Str(text) => !text.is_empty(),
            Value::List(list) => !list.is_empty(),
            Value::Map(map) => !map.is_empty(),
        }
    }

    /// The value as text, the way a scalar is read wherever text is wanted.
    pub fn text(&self) -> String {
        match &self.value {
            Value::Null => "None".to_string(),
            Value::Bool(true) => "True".to_string(),
            Value::Bool(false) => "False".to_string(),
            Value::Int(value) => value.to_string(),
            Value::Float(value) => format!("{value:?}"),
            Value::Str(text) => text.clone(),
            Value::List(_) | Value::Map(_) => self.shown(),
        }
    }

    /// The value as a message quotes it: text in quotes, a list or a mapping written out.
    pub fn shown(&self) -> String {
        match &self.value {
            Value::Str(text) => quoted(text),
            Value::List(list) => {
                let items: Vec<String> = list.iter().map(Node::shown).collect();
                format!("[{}]", items.join(", "))
            }
            Value::Map(map) => {
                let items: Vec<String> = map
                    .entries
                    .iter()
                    .map(|entry| format!("{}: {}", quoted(&entry.key), entry.value.shown()))
                    .collect();
                format!("{{{}}}", items.join(", "))
            }
            _ => self.text(),
        }
    }
}

/// Text as a message quotes it.
pub fn quoted(text: &str) -> String {
    if text.contains('\'') && !text.contains('"') {
        format!("\"{text}\"")
    } else {
        format!("'{}'", text.replace('\'', "\\'"))
    }
}

/// The document uses something this reader does not understand.
#[derive(Debug, Clone, PartialEq)]
pub struct YamlError {
    pub line: Option<usize>,
    pub message: String,
}

impl YamlError {
    fn at(line: usize, message: impl Into<String>) -> YamlError {
        YamlError {
            line: Some(line),
            message: message.into(),
        }
    }
}

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for YamlError {}

/// One meaningful line, with its indentation measured.
#[derive(Clone)]
struct Line {
    number: usize,
    text: String,
    indent: usize,
    /// The text of a block scalar this line introduces.
    block: Option<String>,
}

impl Line {
    fn new(number: usize, raw: &str) -> Line {
        Line {
            number,
            text: raw.trim().to_string(),
            indent: leading_spaces(raw),
            block: None,
        }
    }
}

fn leading_spaces(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

/// Whether a quote after `out` starts a quoted scalar. Only one at the start of a
/// value does, as in YAML: after "key: ", "- ", "[", "{" or ",". One inside a word,
/// the apostrophe of "device's", is a plain character, and a comment after it is
/// still a comment.
fn opens_quote(out: &str) -> bool {
    let text = out.trim_end_matches([' ', '\t']);

    match text.chars().last() {
        None | Some('[') | Some('{') | Some(',') => true,
        Some(':') | Some('-') | Some('?') => out.ends_with([' ', '\t']),
        Some(_) => false,
    }
}

/// Whether `char`, inside a quote opened with `open`, takes the character after it along: a backslash in double quotes,
/// and the first of two apostrophes in single ones.
fn escapes_next(open: char, char: char, next: Option<char>) -> bool {
    (open == '"' && char == '\\') || (open == '\'' && char == '\'' && next == Some('\''))
}

/// Where a line's trailing comment starts, one inside quotes left alone, or None.
pub(crate) fn comment_start(raw: &str) -> Option<usize> {
    let mut quote = None;
    let mut chars = raw.char_indices().peekable();

    while let Some((index, char)) = chars.next() {
        let out = &raw[..index];

        match quote {
            Some(open) => {
                if escapes_next(open, char, chars.peek().map(|(_, next)| *next)) {
                    chars.next();
                } else if char == open {
                    quote = None;
                }
            }
            None if (char == '\'' || char == '"') && opens_quote(out) => quote = Some(char),
            // A "#" only starts a comment when it follows whitespace or begins the
            // line, which is what keeps a value like "a#b" intact.
            None if char == '#' && (out.is_empty() || out.ends_with([' ', '\t'])) => {
                return Some(index);
            }
            None => {}
        }
    }

    None
}

fn strip_comment(raw: &str) -> &str {
    match comment_start(raw) {
        Some(index) => raw[..index].trim_end(),
        None => raw.trim_end(),
    }
}

/// The text of a block scalar whose lines start at `raws[start]`, and the index of
/// the first line after it.
///
/// Its lines are every one indented deeper than the line that introduced it, and
/// the blank ones among them, taken as they are: a "#" in them is text, not a comment.
fn block_text(
    raws: &[&str],
    start: usize,
    indent: usize,
    style: &str,
    chomp: &str,
    number: usize,
) -> Result<(String, usize), YamlError> {
    let mut end = start;

    while end < raws.len() && (raws[end].trim().is_empty() || leading_spaces(raws[end]) > indent) {
        end += 1;
    }

    let body = &raws[start..end];
    let Some(first) = body.iter().find(|line| !line.trim().is_empty()) else {
        // A block that is all blank lines, or none, is empty.
        return Ok((String::new(), end));
    };

    let depth = leading_spaces(first);

    for (offset, line) in body.iter().enumerate() {
        if !line.trim().is_empty() && leading_spaces(line) < depth {
            return Err(YamlError::at(
                number + 1 + offset,
                "indented less than the first line of its block",
            ));
        }
    }

    let mut lines: Vec<&str> = body
        .iter()
        .map(|line| {
            if line.trim().is_empty() {
                ""
            } else {
                &line[depth..]
            }
        })
        .collect();

    // The blank lines at the end belong to the chomping, not to the text.
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }

    let text = if style == "|" {
        lines.join("\n")
    } else {
        if lines.iter().any(|line| line.starts_with([' ', '\t'])) {
            return Err(YamlError::at(
                number,
                "a folded block with lines indented deeper is not supported",
            ));
        }

        // Folded: a line break between two lines of text becomes a space, and each
        // blank line between them stays a line break.
        let mut text = String::new();

        for (index, line) in lines.iter().enumerate() {
            if index == 0 {
                text.push_str(line);
            } else if line.is_empty() {
                text.push('\n');
            } else if lines[index - 1].is_empty() {
                text.push_str(line);
            } else {
                text.push(' ');
                text.push_str(line);
            }
        }

        text
    };

    match chomp {
        "-" => Ok((text, end)),
        "+" => {
            let blank = body
                .iter()
                .rev()
                .take_while(|line| line.trim().is_empty())
                .count();
            Ok((format!("{text}\n{}", "\n".repeat(blank)), end))
        }
        _ => Ok((format!("{text}\n"), end)),
    }
}

/// Meaningful lines only, comments and blanks dropped, and each block of text read whole.
fn clean(text: &str) -> Result<Vec<Line>, YamlError> {
    let text = text.replace("\r\n", "\n");
    let raws: Vec<&str> = text.split('\n').collect();
    let mut lines = Vec::new();
    let mut index = 0;

    while index < raws.len() {
        let number = index + 1;
        let raw = raws[index];
        index += 1;

        let stripped = strip_comment(raw);

        if stripped.trim().is_empty() {
            continue;
        }

        if raw[..raw.len() - raw.trim_start().len()].contains('\t') {
            return Err(YamlError::at(
                number,
                "indented with a tab, which YAML forbids",
            ));
        }

        // A file may say where its document starts.
        if lines.is_empty() && stripped == "---" {
            continue;
        }

        let mut line = Line::new(number, stripped);
        let found = BLOCK
            .captures(&line.text)
            .map(|found| (found["style"].to_string(), found["chomp"].to_string()));

        if let Some((style, chomp)) = found {
            // Its text is indented deeper than the key it belongs to, which in
            // "- help: |" stands after the dash, not at it.
            let mut parent = line.indent;
            let rest = &line.text[1..];

            if line.text.starts_with("- ") && split_key(rest.trim()).is_some() {
                parent = line.indent + 1 + rest.len() - rest.trim_start().len();
            }

            // The line keeps a placeholder of its own form, and the block it
            // introduces is read here, before comments and blanks are dropped.
            let (block, next) = block_text(&raws, index, parent, &style, &chomp, number)?;
            line.block = Some(block);
            index = next;
        }

        lines.push(line);
    }

    Ok(lines)
}

/// The text of the quoted scalar that `text` starts with, and how far it reaches.
fn quoted_scalar(text: &str, line: usize) -> Result<(String, usize), YamlError> {
    let mut chars = text.char_indices();
    let (_, open) = chars.next().expect("text that starts with a quote");
    let mut out = String::new();

    while let Some((at, char)) = chars.next() {
        if char == open {
            if open == '\'' && text[at + 1..].starts_with('\'') {
                chars.next();
                out.push('\'');
                continue;
            }

            return Ok((out, at + 1));
        }

        if open != '"' || char != '\\' {
            out.push(char);
            continue;
        }

        let Some((_, escape)) = chars.next() else {
            break;
        };

        let wide = match escape {
            'x' => 2,
            'u' => 4,
            'U' => 8,
            _ => 0,
        };

        if wide > 0 {
            let digits: String = chars.by_ref().take(wide).map(|(_, digit)| digit).collect();
            let found = u32::from_str_radix(&digits, 16)
                .ok()
                .filter(|_| digits.len() == wide)
                .and_then(char::from_u32);

            let Some(found) = found else {
                return Err(YamlError::at(line, bad_escape(escape, text)));
            };

            out.push(found);
            continue;
        }

        out.push(match escape {
            '0' => '\0',
            'a' => '\x07',
            'b' => '\x08',
            't' | '\t' => '\t',
            'n' => '\n',
            'v' => '\x0b',
            'f' => '\x0c',
            'r' => '\r',
            'e' => '\x1b',
            ' ' => ' ',
            '"' => '"',
            '/' => '/',
            '\\' => '\\',
            'N' => '\u{85}',
            '_' => '\u{a0}',
            'L' => '\u{2028}',
            'P' => '\u{2029}',
            _ => return Err(YamlError::at(line, bad_escape(escape, text))),
        });
    }

    Err(YamlError::at(
        line,
        format!("{text} has a quote that does not close on its line"),
    ))
}

fn bad_escape(escape: char, text: &str) -> String {
    format!(
        "{text} has \\{escape}, and in double quotes a backslash starts an escape. Use single quotes, or write it \\\\."
    )
}

/// Turn one scalar into a value, the way YAML would.
fn scalar(text: &str, line: usize) -> Result<Value, YamlError> {
    let text = text.trim();

    if text.starts_with(['\'', '"']) {
        let (found, end) = quoted_scalar(text, line)?;

        if !text[end..].trim().is_empty() {
            return Err(YamlError::at(
                line,
                format!("{text} has more text after its closing quote"),
            ));
        }

        return Ok(Value::Str(found));
    }

    if text.starts_with(['[', '{']) {
        return in_brackets(text, line);
    }

    if let Some(first) = text
        .chars()
        .next()
        .filter(|first| RESERVED.contains(*first))
    {
        return Err(YamlError::at(
            line,
            format!("{text} starts with {first}, which YAML keeps for itself. Put it in quotes."),
        ));
    }

    if is_item(text) {
        return Err(YamlError::at(
            line,
            format!(
                "{text} is a list item, which goes on a line of its own. Put it in quotes if it is text."
            ),
        ));
    }

    // A second "key: value" on one line, which YAML takes for a mistake, and so does this.
    if text.contains(": ") || text.ends_with(':') {
        return Err(YamlError::at(
            line,
            format!("{text} has a colon that reads as a key. Put it in quotes."),
        ));
    }

    Ok(plain_scalar(text))
}

/// What a plain scalar cannot start with in YAML.
const RESERVED: &str = "&*!|>%@`,]}";

/// A scalar with no quotes, as what it reads as: the forms YAML 1.1 names, and nothing else, so that 1e3 and yEs stay words.
fn plain_scalar(text: &str) -> Value {
    match text {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" | "yes" | "Yes" | "YES" | "on" | "On" | "ON" => {
            return Value::Bool(true);
        }
        "false" | "False" | "FALSE" | "no" | "No" | "NO" | "off" | "Off" | "OFF" => {
            return Value::Bool(false);
        }
        _ => {}
    }

    if let Some(number) = integer(text) {
        return number;
    }

    if FLOAT.is_match(text)
        && let Ok(value) = text.replace('_', "").parse::<f64>()
    {
        return Value::Float(value);
    }

    Value::Str(text.to_string())
}

static FLOAT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?:[-+]?[0-9][0-9_]*\.[0-9_]*(?:[eE][-+][0-9]+)?|\.[0-9][0-9_]*(?:[eE][-+][0-9]+)?)$",
    )
    .expect("a valid pattern")
});

static INTEGER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[-+]?(?:0|[1-9][0-9_]*|0b[01_]+|0[0-7_]+|0x[0-9a-fA-F_]+)$")
        .expect("a valid pattern")
});

/// An integer in any of the forms YAML reads: decimal, 0x, 0b, and 0 for octal. Digits too many for a number stay as they
/// are written.
fn integer(text: &str) -> Option<Value> {
    if !INTEGER.is_match(text) {
        return None;
    }

    let clean = text.replace('_', "");
    let (negative, digits) = match clean.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, clean.strip_prefix('+').unwrap_or(&clean)),
    };

    let (radix, digits) = if let Some(rest) = digits.strip_prefix("0x") {
        (16, rest)
    } else if let Some(rest) = digits.strip_prefix("0b") {
        (2, rest)
    } else if digits.len() > 1 && digits.starts_with('0') {
        (8, &digits[1..])
    } else {
        (10, digits)
    };

    match i64::from_str_radix(digits, radix) {
        Ok(found) => Some(Value::Int(if negative { -found } else { found })),
        Err(_) => Some(Value::Str(text.to_string())),
    }
}

/// A list or a mapping in brackets, which has to close on its line.
fn in_brackets(text: &str, line: usize) -> Result<Value, YamlError> {
    let mut flow = Flow { text, at: 0, line };
    let found = flow.value()?;

    flow.skip_space();

    if !flow.rest().is_empty() {
        return Err(flow.wrong(
            "has more text after its closing bracket. One piece of text that starts with a bracket goes in quotes, as '{library}/tools'",
        ));
    }

    Ok(found)
}

/// Reading through one line of brackets.
struct Flow<'a> {
    text: &'a str,
    at: usize,
    line: usize,
}

impl Flow<'_> {
    fn rest(&self) -> &str {
        &self.text[self.at..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn skip_space(&mut self) {
        self.at = self.text.len() - self.rest().trim_start().len();
    }

    fn wrong(&self, what: &str) -> YamlError {
        YamlError::at(self.line, format!("{} {what}", self.text))
    }

    fn unclosed(&self) -> YamlError {
        self.wrong("has a bracket that does not close on its line")
    }

    fn value(&mut self) -> Result<Value, YamlError> {
        self.skip_space();

        match self.peek() {
            Some('[') => self.list(),
            Some('{') => self.map(),
            Some('\'' | '"') => {
                let (found, end) = quoted_scalar(self.rest(), self.line)?;

                self.at += end;

                Ok(Value::Str(found))
            }
            _ => {
                let word = self.word();

                match word
                    .chars()
                    .next()
                    .filter(|first| RESERVED.contains(*first))
                {
                    Some(first) => Err(YamlError::at(
                        self.line,
                        format!(
                            "{word} starts with {first}, which YAML keeps for itself. Put it in quotes."
                        ),
                    )),
                    None => Ok(plain_scalar(&word)),
                }
            }
        }
    }

    /// Text with no quotes, up to what ends it in brackets: a comma, a bracket, or a colon before a space.
    fn word(&mut self) -> String {
        let rest = self.rest();
        let mut end = rest.len();

        for (at, char) in rest.char_indices() {
            let after = rest[at + char.len_utf8()..].chars().next();
            let colon = char == ':'
                && after.is_none_or(|after| matches!(after, ' ' | ',' | '[' | ']' | '{' | '}'));

            if colon || matches!(char, ',' | '[' | ']' | '{' | '}') {
                end = at;
                break;
            }
        }

        let word = rest[..end].trim().to_string();

        self.at += end;

        word
    }

    /// The name of an entry, which is always text.
    fn key(&mut self) -> Result<String, YamlError> {
        self.skip_space();

        match self.peek() {
            Some('[' | '{') => Err(self.wrong("has a list or a mapping for a key")),
            Some('\'' | '"') => {
                let (found, end) = quoted_scalar(self.rest(), self.line)?;

                self.at += end;

                Ok(found)
            }
            _ => Ok(self.word()),
        }
    }

    /// What follows a key: a colon and a value, or nothing, which is no value.
    fn after_key(&mut self) -> Result<Value, YamlError> {
        self.skip_space();

        if self.peek() != Some(':') {
            return Ok(Value::Null);
        }

        self.at += 1;
        self.skip_space();

        if matches!(self.peek(), Some(',' | ']' | '}')) {
            return Ok(Value::Null);
        }

        self.value()
    }

    /// After an item: a comma, or the bracket that closes the list or the mapping, which is left for the caller.
    fn separator(&mut self) -> Result<(), YamlError> {
        self.skip_space();

        match self.peek() {
            None => Err(self.unclosed()),
            Some(',') => {
                self.at += 1;

                Ok(())
            }
            Some(']' | '}') => Ok(()),
            Some(_) => Err(self.wrong("has an item that runs on without a comma")),
        }
    }

    fn list(&mut self) -> Result<Value, YamlError> {
        self.at += 1;

        let mut items = Vec::new();

        loop {
            self.skip_space();

            match self.peek() {
                None => return Err(self.unclosed()),
                Some(']') => {
                    self.at += 1;

                    return Ok(Value::List(items));
                }
                Some(',') => return Err(self.wrong("has a comma with nothing before it")),
                Some('}') => return Err(self.wrong("has a brace that opens nothing")),
                _ => {}
            }

            let start = self.at;
            let mut found = self.value()?;

            self.skip_space();

            // One key and its value is a mapping of one, as "[a: b]" is.
            if self.peek() == Some(':') && !matches!(found, Value::List(_) | Value::Map(_)) {
                self.at = start;

                let key = self.key()?;
                let value = self.after_key()?;

                found = Value::Map(Mapping {
                    entries: vec![Entry {
                        key,
                        line: self.line,
                        value: Node::new(value, self.line),
                    }],
                });
            }

            items.push(Node::new(found, self.line));
            self.separator()?;
        }
    }

    fn map(&mut self) -> Result<Value, YamlError> {
        self.at += 1;

        let mut result = Mapping::default();

        loop {
            self.skip_space();

            match self.peek() {
                None => return Err(self.unclosed()),
                Some('}') => {
                    self.at += 1;

                    return Ok(Value::Map(result));
                }
                Some(',') => return Err(self.wrong("has a comma with nothing before it")),
                Some(']') => return Err(self.wrong("has a bracket that opens nothing")),
                _ => {}
            }

            let key = self.key()?;

            if result.has(&key) {
                return Err(YamlError::at(self.line, twice_message(&key)));
            }

            let value = self.after_key()?;

            result.entries.push(Entry {
                key,
                line: self.line,
                value: Node::new(value, self.line),
            });
            self.separator()?;
        }
    }
}

/// Split "key: value" on the first colon that is not inside quotes. Only a quote that
/// starts the key opens one, as in YAML: the apostrophe of "gble's channel" is a
/// plain character of the key.
pub(crate) fn split_key(text: &str) -> Option<(String, String)> {
    // A list or a mapping in brackets, whatever colons it holds.
    if text.starts_with(['[', '{']) {
        return None;
    }

    let mut quote = None;
    let mut chars = text.char_indices().peekable();

    while let Some((index, char)) = chars.next() {
        match quote {
            Some(open) => {
                if escapes_next(open, char, chars.peek().map(|(_, next)| *next)) {
                    chars.next();
                } else if char == open {
                    quote = None;
                }
            }
            None if (char == '\'' || char == '"') && index == 0 => quote = Some(char),
            None if char == ':' => {
                let after = &text[index + 1..];

                // "a:b" is a plain scalar in YAML, not a mapping.
                if !after.is_empty() && !after.starts_with(' ') {
                    continue;
                }

                return Some((text[..index].trim().to_string(), after.trim().to_string()));
            }
            None => {}
        }
    }

    None
}

/// Whether a line is an item of a list: a dash and what it holds, or a dash alone.
fn is_item(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

/// Parse one block at the given indentation. Returns the value and the next index.
fn parse_block(lines: &[Line], index: usize, indent: usize) -> Result<(Value, usize), YamlError> {
    if index >= lines.len() {
        return Ok((Value::Null, index));
    }

    if is_item(&lines[index].text) {
        return parse_sequence(lines, index, indent);
    }

    parse_mapping(lines, index, indent)
}

fn twice_message(key: &str) -> String {
    format!(
        "{} is given twice in one place, and YAML would keep only the last without a word. \
         Give each its own name, or put the two together.",
        quoted(key)
    )
}

fn parse_mapping(
    lines: &[Line],
    mut index: usize,
    indent: usize,
) -> Result<(Value, usize), YamlError> {
    let mut result = Mapping::default();

    while index < lines.len() && lines[index].indent == indent {
        let line = &lines[index];

        if is_item(&line.text) {
            break;
        }

        let Some((mut key, value)) = split_key(&line.text) else {
            return Err(YamlError::at(
                line.number,
                format!("expected 'key: value', found {}", quoted(&line.text)),
            ));
        };

        // A quoted key is its text: "a: b": names a: b.
        if key.starts_with(['\'', '"']) {
            let (found, end) = quoted_scalar(&key, line.number)?;

            if end == key.len() {
                key = found;
            }
        }

        if result.has(&key) {
            return Err(YamlError::at(line.number, twice_message(&key)));
        }

        let number = line.number;

        if !value.is_empty() {
            let found = match &line.block {
                Some(block) => Value::Str(block.clone()),
                None => scalar(&value, number)?,
            };
            result.entries.push(Entry {
                key,
                line: number,
                value: Node::new(found, number),
            });
            index += 1;
            continue;
        }

        // The value is a block on the following lines, or nothing at all. A list
        // may stand at the same depth as its key, as many editors write it.
        let next = lines.get(index + 1);
        let below = next.is_some_and(|next| next.indent > indent);
        let beside = next.is_some_and(|next| next.indent == indent && is_item(&next.text));

        if below || beside {
            let depth = lines[index + 1].indent;
            let (found, after) = parse_block(lines, index + 1, depth)?;
            result.entries.push(Entry {
                key,
                line: number,
                value: Node::new(found, number),
            });
            index = after;
        } else {
            result.entries.push(Entry {
                key,
                line: number,
                value: Node::new(Value::Null, number),
            });
            index += 1;
        }
    }

    Ok((Value::Map(result), index))
}

fn parse_sequence(
    lines: &[Line],
    mut index: usize,
    indent: usize,
) -> Result<(Value, usize), YamlError> {
    let mut result = Vec::new();

    while index < lines.len() && lines[index].indent == indent {
        let line = &lines[index];

        if !is_item(&line.text) {
            break;
        }

        let content = line.text[1..].trim();

        // A dash alone: what it holds is on the lines below it, or it holds nothing.
        if content.is_empty() {
            let number = line.number;

            if lines
                .get(index + 1)
                .is_some_and(|next| next.indent > indent)
            {
                let depth = lines[index + 1].indent;
                let (found, after) = parse_block(lines, index + 1, depth)?;

                result.push(Node::new(found, number));
                index = after;
            } else {
                result.push(Node::new(Value::Null, number));
                index += 1;
            }

            continue;
        }

        if split_key(content).is_none() && !is_item(content) {
            let found = match &line.block {
                Some(block) => Value::Str(block.clone()),
                None => scalar(content, line.number)?,
            };
            result.push(Node::new(found, line.number));
            index += 1;
            continue;
        }

        // A mapping or another list inside a sequence item. Rewriting the "- " as spaces
        // turns the item and its continuation lines into one ordinary block, which the
        // parser already knows how to read.
        let item_indent = indent + 2;
        let mut first = Line::new(line.number, content);
        first.indent = item_indent;
        first.block = line.block.clone();
        let number = line.number;
        let mut block = vec![first];

        index += 1;

        while index < lines.len() && lines[index].indent >= item_indent {
            block.push(lines[index].clone());
            index += 1;
        }

        let (found, consumed) = parse_block(&block, 0, item_indent)?;

        if consumed != block.len() {
            return Err(YamlError::at(
                block[consumed].number,
                "could not read this list item",
            ));
        }

        result.push(Node::new(found, number));
    }

    Ok((Value::List(result), index))
}

/// Read a YAML document into values. None for one that holds nothing.
pub fn loads(text: &str) -> Result<Option<Node>, YamlError> {
    let lines = clean(text.strip_prefix('\u{feff}').unwrap_or(text))?;

    let Some(first) = lines.first() else {
        return Ok(None);
    };

    if first.indent != 0 {
        return Err(YamlError::at(first.number, "the document starts indented"));
    }

    let number = first.number;
    let (value, index) = parse_block(&lines, 0, 0)?;

    if index != lines.len() {
        return Err(YamlError::at(
            lines[index].number,
            format!("unexpected indentation, {}", quoted(&lines[index].text)),
        ));
    }

    Ok(Some(Node::new(value, number)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value as Json, json};

    fn plain(node: &Node) -> Json {
        match &node.value {
            Value::Null => Json::Null,
            Value::Bool(value) => json!(value),
            Value::Int(value) => json!(value),
            Value::Float(value) => json!(value),
            Value::Str(text) => json!(text),
            Value::List(list) => Json::Array(list.iter().map(plain).collect()),
            Value::Map(map) => Json::Object(
                map.entries
                    .iter()
                    .map(|entry| (entry.key.clone(), plain(&entry.value)))
                    .collect(),
            ),
        }
    }

    fn read(text: &str) -> Json {
        plain(&loads(text).unwrap().expect("a document"))
    }

    #[test]
    fn scalars_read_as_yaml_reads_them() {
        let text = "a_string: hello\na_quoted: 'has: a colon'\na_double: \"trailing space \"\n\
                    a_number: 42\na_float: 1.5\na_version: 2.0.0\na_true: true\na_yes: yes\n\
                    a_false: false\na_no: no\na_null: null\na_tilde: ~\nan_empty:\n\
                    a_hash_in_value: red#blue\na_url: https://github.com/nimaltd/fsm\n";

        assert_eq!(
            read(text),
            json!({
                "a_string": "hello", "a_quoted": "has: a colon", "a_double": "trailing space ",
                "a_number": 42, "a_float": 1.5, "a_version": "2.0.0", "a_true": true, "a_yes": true,
                "a_false": false, "a_no": false, "a_null": null, "a_tilde": null, "an_empty": null,
                "a_hash_in_value": "red#blue", "a_url": "https://github.com/nimaltd/fsm",
            })
        );
    }

    #[test]
    fn a_whole_manifest_reads() {
        let text = "name: spif\nversion: 3.0.0\nkind: driver\nrequires:\n  hal_modules: [spi, gpio, dma]\n  \
                    rtos: optional\n  peripherals:\n    - type: spi\n      count: 1\n      note: Master mode, 8 bit\n    \
                    - type: gpio\n      count: 2\nonce:\n  - from: src/spif_config.h\n  - from: src/spif_port.c\n    \
                    to: my_port.c\nextras: [LICENSE.md, NOTICE]\n";

        assert_eq!(
            read(text),
            json!({
                "name": "spif", "version": "3.0.0", "kind": "driver",
                "requires": {
                    "hal_modules": ["spi", "gpio", "dma"], "rtos": "optional",
                    "peripherals": [
                        {"type": "spi", "count": 1, "note": "Master mode, 8 bit"},
                        {"type": "gpio", "count": 2},
                    ],
                },
                "once": [{"from": "src/spif_config.h"}, {"from": "src/spif_port.c", "to": "my_port.c"}],
                "extras": ["LICENSE.md", "NOTICE"],
            })
        );
    }

    #[test]
    fn every_value_knows_its_line() {
        let found = loads(
            "# a comment\n\nname: x\nrequires:\n  peripherals:\n    - type: i2c\n      count: 0\n",
        )
        .unwrap()
        .unwrap();
        let map = found.as_map().unwrap();

        assert_eq!(map.get("name").unwrap().line, 3);
        assert_eq!(map.line_of("requires"), Some(4));

        let requires = map.get("requires").unwrap().as_map().unwrap();
        let item = requires.get("peripherals").unwrap().as_list().unwrap()[0]
            .as_map()
            .unwrap();

        assert_eq!(item.get("type").unwrap().line, 6);
        assert_eq!(item.get("count").unwrap().line, 7);
    }

    #[test]
    fn a_comment_inside_quotes_is_kept_and_a_trailing_one_dropped() {
        assert_eq!(read("note: 'a # b'"), json!({"note": "a # b"}));
        assert_eq!(read("name: fsm   # the library"), json!({"name": "fsm"}));
    }

    #[test]
    fn a_quote_inside_a_word_is_a_plain_character() {
        assert_eq!(
            read("description: Make this device's own ID   # shown with the question\n"),
            json!({"description": "Make this device's own ID"})
        );
        assert_eq!(read("note: a 'b   # c'\n"), json!({"note": "a 'b"}));
        assert_eq!(
            read("items:\n  - 'quoted # kept'\n  - it's   # dropped\n"),
            json!({"items": ["quoted # kept", "it's"]})
        );
        assert_eq!(
            read("list: ['a # b', \"c\", d's]\n"),
            json!({"list": ["a # b", "c", "d's"]})
        );
        assert_eq!(read("key: x-'y'   # z\n"), json!({"key": "x-'y'"}));
    }

    #[test]
    fn titles_and_notes_read_as_yaml_reads_them() {
        assert_eq!(
            read("conf:\n  gble's channel:\n    - BT_NUS\n"),
            json!({"conf": {"gble's channel": ["BT_NUS"]}})
        );
        assert_eq!(
            read("conf:\n  'quoted: title':\n    - BT\n  \"double: quoted\":\n    - BT_NUS\n"),
            json!({"conf": {"quoted: title": ["BT"], "double: quoted": ["BT_NUS"]}})
        );
        assert_eq!(
            read(
                "conf:\n  - LOG=n: no messages, and nowhere to send them\n  - BOOT_BANNER=n: Zephyr's banner\n  - PRINTK=n\n"
            ),
            json!({"conf": [
                {"LOG=n": "no messages, and nowhere to send them"},
                {"BOOT_BANNER=n": "Zephyr's banner"},
                "PRINTK=n",
            ]})
        );
        assert_eq!(
            read(
                "conf:\n  The product's gapp.c, gble's weak ones:\n    - BT_DEVICE_NAME=\"gcore\": its name\n"
            ),
            json!({"conf": {"The product's gapp.c, gble's weak ones": [{"BT_DEVICE_NAME=\"gcore\"": "its name"}]}})
        );
    }

    #[test]
    fn a_key_given_twice_is_refused() {
        for text in [
            "a: 1\na: 2\n",
            "conf:\n  Bluetooth:\n    - BT\n  Bluetooth:\n    - BT_NUS\n",
            "list:\n  - name: x\n    name: y\n",
        ] {
            let error = loads(text).unwrap_err();
            assert!(error.message.contains("given twice"), "{error}");
        }

        assert_eq!(loads("a: 1\nb: 2\na: 3\n").unwrap_err().line, Some(3));
    }

    #[test]
    fn an_empty_document_is_none() {
        assert_eq!(loads("# only a comment\n\n").unwrap(), None);
    }

    #[test]
    fn tabs_are_refused_rather_than_guessed() {
        let error = loads("files:\n\theaders: []\n").unwrap_err();
        assert!(error.message.contains("tab"), "{error}");
        assert_eq!(error.line, Some(2));
    }

    /// What PyYAML read for each document, or that it refused it. Two readers that disagree would mean a manifest reading
    /// one way in lib-installer and another here. tests/data/pyyaml.json was written by PyYAML 6, and keeps Python out of it.
    #[test]
    fn it_reads_what_pyyaml_reads_and_refuses_what_it_refuses() {
        let cases: Json = serde_json::from_str(include_str!("../tests/data/pyyaml.json")).unwrap();

        for case in cases.as_array().unwrap() {
            let text = case["text"].as_str().unwrap();

            if case.get("refused").is_some() {
                assert!(
                    loads(text).is_err(),
                    "PyYAML refuses {text:?}, and this read {:?}",
                    loads(text)
                );
            } else {
                match loads(text) {
                    Ok(found) => assert_eq!(
                        found.as_ref().map(plain).unwrap_or(Json::Null),
                        case["reads"],
                        "{text:?}"
                    ),
                    Err(error) => panic!("PyYAML reads {text:?}, and this refused it: {error}"),
                }
            }
        }
    }

    #[test]
    fn brackets_read_as_yaml_reads_them() {
        assert_eq!(
            read("a: [[1, 2], [3]]\nb: {x: 1, y: [2, three], z: {w: null}}\nc: []\nd: {}\n"),
            json!({"a": [[1, 2], [3]], "b": {"x": 1, "y": [2, "three"], "z": {"w": null}}, "c": [], "d": {}})
        );
        assert_eq!(
            read(
                "l:\n  - {type: i2c, count: 2, note: 400 kHz}\n  - [a: b, c]\n  - {from: 'x y', to: \"z\"}\n"
            ),
            json!({"l": [{"type": "i2c", "count": 2, "note": "400 kHz"}, [{"a": "b"}, "c"], {"from": "x y", "to": "z"}]})
        );
        assert_eq!(
            read("a: {b, c: }\nd: [1, 2,]\n"),
            json!({"a": {"b": null, "c": null}, "d": [1, 2]})
        );
        assert_eq!(
            read("a: ['{library}/m.py', \"x, y\", 'it''s']\n"),
            json!({"a": ["{library}/m.py", "x, y", "it's"]})
        );
    }

    #[test]
    fn brackets_that_cannot_be_read_are_refused_not_guessed() {
        for (text, says) in [
            ("a: [1, 2\n", "does not close on its line"),
            ("a: {b: 1\n", "does not close on its line"),
            ("a: [1,,2]\n", "a comma with nothing before it"),
            ("a: [1 [2]]\n", "runs on without a comma"),
            ("a: {b: 1, b: 2}\n", "is given twice"),
            ("a: {library}/tools/x.py\n", "goes in quotes"),
            ("a: [{library}/x]\n", "without a comma"),
            ("a: {[b]: 1}\n", "a list or a mapping for a key"),
            ("a: [*.c]\n", "keeps for itself"),
        ] {
            let error = loads(text).unwrap_err();

            assert!(error.message.contains(says), "{text}: {error}");
            assert_eq!(error.line, Some(1), "{text}");
        }
    }

    #[test]
    fn each_kind_of_quote_escapes_what_it_escapes() {
        assert_eq!(
            read(
                "a: \"x\\\"y\\nz\\t\\\\ \\x41\\u0042\\U00000043\"\nb: 'it''s \\n'\n\"c\\\"d\": 1\n'e''f': 2\n"
            ),
            json!({"a": "x\"y\nz\t\\ ABC", "b": "it's \\n", "c\"d": 1, "e'f": 2})
        );
    }

    #[test]
    fn a_comment_after_a_quote_with_an_escape_in_it_is_still_a_comment() {
        assert_eq!(
            read("a: \"x\\\" # y\" # z\nb: 'it''s # y' # z\n"),
            json!({"a": "x\" # y", "b": "it's # y"})
        );
    }

    #[test]
    fn a_quote_that_cannot_be_read_is_refused() {
        for (text, says) in [
            ("a: \"b\n", "does not close on its line"),
            ("a: 'b\n", "does not close on its line"),
            ("a: \"b\" c\n", "more text after its closing quote"),
            ("a: \"C:\\Users\\x\"\n", "a backslash starts an escape"),
            ("a: \"\\q\"\n", "a backslash starts an escape"),
        ] {
            let error = loads(text).unwrap_err();

            assert!(error.message.contains(says), "{text}: {error}");
        }
    }

    #[test]
    fn what_yaml_keeps_for_itself_is_refused() {
        for text in [
            "a: &x 1\n",
            "a: *x\n",
            "a: !!str 1\n",
            "- *.c\n",
            "a: >5 MHz\n",
            "a: |2\n  b\n",
            "a: %x\n",
            "a: @x\n",
            "a: `x\n",
        ] {
            let error = loads(text).unwrap_err();

            assert!(
                error.message.contains("keeps for itself"),
                "{text}: {error}"
            );
        }
    }

    #[test]
    fn a_list_may_stand_at_the_depth_of_its_key() {
        assert_eq!(
            read("requires:\n  libraries:\n  - osal\n  - spif\nname: x\n"),
            json!({"requires": {"libraries": ["osal", "spif"]}, "name": "x"})
        );
    }

    #[test]
    fn a_file_may_say_where_its_document_starts() {
        assert_eq!(read("---\nname: x\n"), json!({"name": "x"}));
    }

    #[test]
    fn crlf_and_a_byte_order_mark_change_nothing() {
        assert_eq!(
            read("\u{feff}name: x\r\nhelp: |\r\n  a\r\n  b\r\n"),
            json!({"name": "x", "help": "a\nb\n"})
        );
    }

    #[test]
    fn blocks_of_text_read_as_yaml_reads_them() {
        let cases = [
            ("help: |\n  One line.\n", json!({"help": "One line.\n"})),
            (
                "help: |\n  Two lines,\n  kept as they are.\nnext: 1\n",
                json!({"help": "Two lines,\nkept as they are.\n", "next": 1}),
            ),
            (
                "help: |-\n  No line ending at the end.\nnext: 1\n",
                json!({"help": "No line ending at the end.", "next": 1}),
            ),
            (
                "help: |+\n  Every blank line at the end kept.\n\n\nnext: 1\n",
                json!({"help": "Every blank line at the end kept.\n\n\n", "next": 1}),
            ),
            (
                "help: |\n  Blank lines at the end dropped.\n\n\nnext: 1\n",
                json!({"help": "Blank lines at the end dropped.\n", "next": 1}),
            ),
            (
                "help: |\n  A # is text here, not a comment.\n  # and so is this line.\n",
                json!({"help": "A # is text here, not a comment.\n# and so is this line.\n"}),
            ),
            (
                "help: |\n  First paragraph.\n\n  Second paragraph.\n",
                json!({"help": "First paragraph.\n\nSecond paragraph.\n"}),
            ),
            (
                "help: |\n  Text\n    indented deeper\n  and back.\n",
                json!({"help": "Text\n  indented deeper\nand back.\n"}),
            ),
            (
                "help: >\n  Folded into\n  one line.\n",
                json!({"help": "Folded into one line.\n"}),
            ),
            (
                "help: >-\n  Folded, and\n  no ending.\nnext: 1\n",
                json!({"help": "Folded, and no ending.", "next": 1}),
            ),
            (
                "help: >\n  Two\n  paragraphs.\n\n  Folded\n  each.\n",
                json!({"help": "Two paragraphs.\nFolded each.\n"}),
            ),
            ("help: |\nnext: 1\n", json!({"help": "", "next": 1})),
            (
                "settings:\n  - name: A\n    help: |\n      For A.\n      Second line.\n    type: int\n  - name: B\n    help: >-\n      For B.\n",
                json!({"settings": [
                    {"name": "A", "help": "For A.\nSecond line.\n", "type": "int"},
                    {"name": "B", "help": "For B."},
                ]}),
            ),
            (
                "settings:\n  - help: |\n      First key of the item.\n    name: A\n",
                json!({"settings": [{"help": "First key of the item.\n", "name": "A"}]}),
            ),
            (
                "list:\n  - |\n    A block as an item.\n  - plain\n",
                json!({"list": ["A block as an item.\n", "plain"]}),
            ),
            (
                "outer:\n  inner:\n    help: |\n      Deep.\n  after: 2\n",
                json!({"outer": {"inner": {"help": "Deep.\n"}, "after": 2}}),
            ),
            (
                "help: |  # a comment after the indicator\n  Text.\n",
                json!({"help": "Text.\n"}),
            ),
        ];

        for (text, expected) in cases {
            assert_eq!(read(text), expected, "{text:?}");
        }
    }

    #[test]
    fn a_block_indented_less_than_its_first_line_is_refused() {
        assert!(loads("outer:\n  help: |\n      Deep first.\n    then shallower.\n").is_err());
    }

    #[test]
    fn a_folded_block_with_deeper_lines_is_refused_not_guessed() {
        assert!(loads("help: >\n  Text\n    deeper\n").is_err());
    }

    #[test]
    fn every_real_library_file_reads() {
        // The libraries beside this repository, when they are checked out.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let Ok(folders) = std::fs::read_dir(root) else {
            return;
        };

        for folder in folders.flatten() {
            if let Ok(text) = std::fs::read_to_string(folder.path().join("installer.yml")) {
                let read =
                    loads(&text).unwrap_or_else(|error| panic!("{:?}: {error}", folder.path()));
                assert!(read.is_some());
            }
        }
    }
}
