//! The terminal: colored lines, and questions.
//!
//! Windows 10 and later can render ANSI escapes once virtual terminal processing is switched on, which is done here. Where
//! that fails, and wherever the output is not a terminal, every line degrades to plain text rather than printing escape
//! codes into a log file.
//!
//! Honours the NO_COLOR convention, see https://no-color.org

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, IsTerminal, Write};

use libnima_core::report::{self, Line, Question, Style, Ui};

const RESET: &str = "\x1b[0m";

/// How wide the line between two libraries is.
const RULE_WIDTH: usize = 60;

/// Turn on virtual terminal processing. Returns true when colour is usable.
#[cfg(windows)]
fn enable_ansi() -> bool {
    use windows_sys::Win32::System::Console::{
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, GetConsoleMode, GetStdHandle, STD_OUTPUT_HANDLE,
        SetConsoleMode,
    };

    // SAFETY: both calls take the process's own output handle, and the mode is written where it is read from.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        let mut mode = 0;

        GetConsoleMode(handle, &mut mode) != 0
            && SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
    }
}

#[cfg(not(windows))]
fn enable_ansi() -> bool {
    true
}

/// Whether to emit escape codes at all.
fn supported() -> bool {
    env::var_os("NO_COLOR").is_none()
        && env::var("TERM").map_or(true, |term| term != "dumb")
        && io::stdout().is_terminal()
        && enable_ansi()
}

fn codes(style: Style) -> &'static str {
    match style {
        Style::Plain => "",
        Style::Bold => "\x1b[1m",
        Style::Title => "\x1b[1m\x1b[36m",
        Style::Dim => "\x1b[2m",
        Style::Good => "\x1b[32m",
        Style::Warn => "\x1b[33m",
        Style::Bad => "\x1b[31m",
        Style::Note => "\x1b[90m",
        Style::Accent => "\x1b[36m",
    }
}

/// The terminal a run shows in.
pub struct Terminal {
    color: bool,
    /// Given --yes: every question takes the answer it offers.
    pub yes: bool,
    /// How many of a run's parts have started.
    parts: usize,
}

impl Terminal {
    pub fn new(yes: bool) -> Terminal {
        Terminal {
            color: supported(),
            yes,
            parts: 0,
        }
    }

    fn text_of(&self, line: &Line) -> String {
        let mut out = String::new();

        for span in line {
            let code = if self.color { codes(span.style) } else { "" };

            if code.is_empty() {
                out.push_str(&span.text);
            } else {
                out.push_str(code);
                out.push_str(&span.text);
                out.push_str(RESET);
            }
        }

        out
    }
}

/// Read one line from the console itself: /dev/tty on Linux and macOS, CONIN$ on Windows.
fn read_console() -> Option<String> {
    let device = if cfg!(windows) { "CONIN$" } else { "/dev/tty" };
    let mut line = String::new();

    BufReader::new(File::open(device).ok()?)
        .read_line(&mut line)
        .ok()?;

    Some(line.trim().to_string())
}

impl Ui for Terminal {
    fn say(&mut self, line: Line) {
        println!("{}", self.text_of(&line));
    }

    /// A line between two parts. The first has nothing to be kept apart from.
    fn new_part(&mut self) {
        if self.parts > 0 {
            println!();
            println!("{}", self.text_of(&report::note("-".repeat(RULE_WIDTH))));
        }

        self.parts += 1;
    }

    fn error(&mut self, message: &str) {
        let text = format!("Error: {message}");

        if self.color && io::stderr().is_terminal() {
            eprintln!("{}{text}{RESET}", codes(Style::Bad));
        } else {
            eprintln!("{text}");
        }
    }

    /// Ask a question and return what was typed, or None when nobody can answer.
    ///
    /// With the input piped in, stdin cannot also carry the answer, so the console is read directly instead. That is only
    /// tried when the output is going to a screen. Otherwise nobody is there to read the question, and waiting for an answer
    /// would hang a script or a build server for ever.
    fn ask(&mut self, question: &Question) -> Option<String> {
        let prompt = self.text_of(&question.prompt);

        if self.yes {
            let mark = if self.color {
                format!("{}(--yes){RESET}", codes(Style::Note))
            } else {
                "(--yes)".to_string()
            };

            println!("{prompt}{mark}");

            return Some(String::new());
        }

        if io::stdin().is_terminal() {
            print!("{prompt}");
            let _ = io::stdout().flush();

            let mut line = String::new();

            return match io::stdin().read_line(&mut line) {
                Ok(0) | Err(_) => None,
                Ok(_) => Some(line.trim().to_string()),
            };
        }

        if !io::stdout().is_terminal() {
            return None;
        }

        print!("{prompt}");
        let _ = io::stdout().flush();

        read_console()
    }
}
