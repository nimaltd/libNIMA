//! What the engine has to say and to ask, for whoever is showing it.
//!
//! The engine never prints and never reads a keyboard. It hands each line to a [`Ui`],
//! which the command prints in the terminal and the window shows in its log. Each line
//! is a row of spans with a style, so the two can each draw it in their own way.

/// How a span of text is meant to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Style {
    Plain,
    Bold,
    /// A library's name, above what is said about it.
    Title,
    Dim,
    Good,
    Warn,
    Bad,
    Note,
    /// What is the user's: kept, created or moved.
    Accent,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

/// One line of what a run says.
pub type Line = Vec<Span>;

pub fn span(text: impl Into<String>, style: Style) -> Span {
    Span {
        text: text.into(),
        style,
    }
}

pub fn line(text: impl Into<String>, style: Style) -> Line {
    vec![span(text, style)]
}

pub fn plain(text: impl Into<String>) -> Line {
    line(text, Style::Plain)
}

pub fn note(text: impl Into<String>) -> Line {
    line(text, Style::Note)
}

pub fn warn(text: impl Into<String>) -> Line {
    line(text, Style::Warn)
}

pub fn good(text: impl Into<String>) -> Line {
    line(text, Style::Good)
}

pub fn bad(text: impl Into<String>) -> Line {
    line(text, Style::Bad)
}

pub fn heading(text: impl Into<String>) -> Line {
    line(text, Style::Bold)
}

pub fn blank() -> Line {
    Vec::new()
}

/// One line of the file report.
///
/// The mark is a short word, so the column stays aligned and the output still reads
/// correctly when colour is off or the font has no symbols.
pub fn item(mark: &str, label: &str, path: impl Into<String>, style: Style) -> Line {
    item_spans(mark, label, vec![span(path, Style::Plain)], style)
}

/// A line of the file report whose text has styles of its own.
pub fn item_spans(mark: &str, label: &str, text: Vec<Span>, style: Style) -> Line {
    let mut found = vec![span("  ", Style::Plain), span(format!("{mark:<8}"), style)];
    found.extend(text);

    if !label.is_empty() {
        found.push(span("  ", Style::Plain));
        found.push(span(label, Style::Note));
    }

    found
}

/// A line under an item, about it.
pub fn detail(text: impl Into<String>) -> Line {
    vec![span("          ", Style::Plain), span(text, Style::Note)]
}

/// A library's name and version, and what it is, before anything is written.
pub fn banner(name: &str, version: &str, description: &str) -> Vec<Line> {
    let mut head = vec![span(name, Style::Title)];

    // A library whose header carries no @version simply shows none.
    if !version.is_empty() {
        head.push(span(" ", Style::Plain));
        head.push(span(version, Style::Dim));
    }

    let mut lines = vec![head];

    if !description.is_empty() {
        lines.push(note(description));
    }

    lines
}

/// A question the engine needs answered to go on.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Question {
    /// What is asked, with the answers it offers, as the command prints it.
    pub prompt: Line,
    /// What kind of answer it takes, so a window can offer it in its own way.
    pub kind: QuestionKind,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub enum QuestionKind {
    /// Any text, or nothing for what is in brackets.
    Text,
    /// Yes or no, or nothing for the one offered.
    YesNo { default: bool },
    /// One of `options`, or several when `many`, as text: a number of the list, several as 1,3, a name, a part of one,
    /// a wildcard, or * for all. A window offers the list itself, with a search and a box to tick for each, and answers
    /// with the names ticked.
    Choose { options: Vec<String>, many: bool },
}

/// Whoever is showing a run: the terminal, or the window.
pub trait Ui {
    /// Show one line of what the run is doing.
    fn say(&mut self, line: Line);

    /// Show why the run could not go on.
    fn error(&mut self, message: &str);

    /// Put a question and return what was answered, or None when nobody is there to
    /// answer, so a question is asked only when it can be.
    fn ask(&mut self, question: &Question) -> Option<String>;

    /// Whether a step's own output reaches the person by itself, as it does in a terminal. When not, it is said line by line.
    fn shows_output(&self) -> bool {
        true
    }

    /// What follows is a part of its own, apart from what came before: each library of a run is one, so whoever shows the run
    /// can keep them apart.
    fn new_part(&mut self) {}

    /// Show several lines.
    fn say_all(&mut self, lines: Vec<Line>) {
        for found in lines {
            self.say(found);
        }
    }
}

/// A run nobody watches: what it says goes nowhere, and every question goes unanswered.
pub struct Silent;

impl Ui for Silent {
    fn say(&mut self, _line: Line) {}

    fn error(&mut self, _message: &str) {}

    fn ask(&mut self, _question: &Question) -> Option<String> {
        None
    }
}
