//! Run steps: commands a library runs once its files are in place.
//!
//! A library can need something made in the project that no file can carry, such as a key it signs its images
//! with, made fresh for each product. Its manifest names a step for that under run, and the step runs once the
//! library is in.
//!
//! It runs on the user's own computer, with the user's rights, so it is never run without a word. The user sees
//! exactly what will run and answers, or says so in advance with --run or --yes. Nothing in a manifest can turn the
//! question off: a library that changed hands on GitHub must not be able to run something unseen on the next install.
//!
//! A step whose file is already there is skipped without a question, since the file is what it makes. One with no
//! such file that ran before, and runs the same thing, is not asked again either: an update is not the moment to make
//! a second key.

use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::files;
use crate::manifest::{LIBRARY, Manifest, RunStep};
use crate::paths;
use crate::platform::Platform;
use crate::report::{self, Question, QuestionKind, Span, Style, Ui, span};

/// What became of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ran,
    Failed,
    Skipped,
    NotRun,
}

/// What became of one step, so the record and the exit code can say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepOutcome {
    pub label: String,
    pub fingerprint: String,
    pub status: Status,
}

/// What a run says about its steps: those to run or leave without a question, and whether to take every answer.
#[derive(Debug, Clone, Default)]
pub struct StepChoices {
    pub turn_on: Vec<String>,
    pub turn_off: Vec<String>,
    pub assume_yes: bool,
}

/// The library's folder as a step names it: from the project's folder, where the step runs, or in full when it is
/// somewhere else.
fn library_folder(project_root: &Path, destination: &Path) -> String {
    let destination = paths::resolve(destination);

    match paths::relative_to(&destination, &paths::resolve(project_root)) {
        Some(rest) if rest.as_os_str().is_empty() => ".".to_string(),
        Some(rest) => paths::posix(&rest),
        None => paths::posix(&destination),
    }
}

/// The command and the arguments, {library} in them standing for the library's folder.
pub fn words(step: &RunStep, project_root: &Path, destination: &Path) -> Vec<String> {
    let folder = library_folder(project_root, destination);

    step.command
        .iter()
        .chain(&step.args)
        .map(|word| word.replace(LIBRARY, &folder))
        .collect()
}

/// A word as a POSIX shell would need it.
fn quoted(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|char| char.is_alphanumeric() || "-_./=:@%+,".contains(char))
    {
        return word.to_string();
    }

    format!("'{}'", word.replace('\'', "'\\''"))
}

/// A list of words as Windows' own command line reads them.
fn windows_line(words: &[String]) -> String {
    let mut out = Vec::new();

    for word in words {
        let needs_quotes = word.is_empty() || word.contains([' ', '\t']);
        let mut written = String::new();
        let mut slashes = 0;

        if needs_quotes {
            written.push('"');
        }

        for char in word.chars() {
            match char {
                '\\' => slashes += 1,
                '"' => {
                    written.push_str(&"\\".repeat(slashes * 2));
                    slashes = 0;
                    written.push_str("\\\"");
                }
                other => {
                    written.push_str(&"\\".repeat(slashes));
                    slashes = 0;
                    written.push(other);
                }
            }
        }

        written.push_str(&"\\".repeat(slashes));

        if needs_quotes {
            written.push_str(&"\\".repeat(slashes));
            written.push('"');
        }

        out.push(written);
    }

    out.join(" ")
}

/// The command as the user reads it, and could type it in the project's folder.
pub fn shown(step: &RunStep, project_root: &Path, destination: &Path) -> String {
    let found = words(step, project_root, destination);

    if cfg!(windows) {
        windows_line(&found)
    } else {
        found
            .iter()
            .map(|word| quoted(word))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// The file whose being there skips a step: from the project's folder, or the library's with {library}.
fn unless_path(
    step: &RunStep,
    project_root: &Path,
    destination: &Path,
) -> Option<std::path::PathBuf> {
    let unless = step.unless.as_ref()?;

    Some(if step.unless_in_library {
        destination.join(unless)
    } else {
        project_root.join(unless)
    })
}

/// The label of a step's lines.
fn label_of(step: &RunStep) -> Span {
    span(format!("  {:<12} ", step.label), Style::Plain)
}

const UNDER: &str = "               ";

/// Run a library's steps, each as its rules say, and say what became of each.
///
/// `recorded` is what ran before, label to fingerprint, from the record.
pub fn run(
    library: &Manifest,
    project_root: &Path,
    destination: &Path,
    choices: &StepChoices,
    recorded: Option<&Value>,
    platform: Platform,
    ui: &mut dyn Ui,
) -> Vec<StepOutcome> {
    if library.run.is_empty() {
        return Vec::new();
    }

    let mut outcomes = Vec::new();

    ui.say(report::blank());
    ui.say(report::heading("Run"));

    for step in &library.run {
        let finish = |status: Status| StepOutcome {
            label: step.label.clone(),
            fingerprint: step.fingerprint(),
            status,
        };
        let label = label_of(step);
        let command = shown(step, project_root, destination);

        if unless_path(step, project_root, destination).is_some_and(|path| path.exists()) {
            ui.say(vec![
                label,
                span(
                    format!(
                        "skipped, {} is already there",
                        step.unless.as_deref().unwrap_or_default()
                    ),
                    Style::Note,
                ),
            ]);
            outcomes.push(finish(Status::Skipped));
            continue;
        }

        if choices.turn_off.contains(&step.label) {
            ui.say(vec![
                label,
                span(
                    format!("skipped, as asked with --no-run {}", step.label),
                    Style::Note,
                ),
            ]);
            outcomes.push(finish(Status::Skipped));
            continue;
        }

        let ran_before = recorded
            .and_then(|recorded| recorded.get(&step.label))
            .and_then(Value::as_str)
            .is_some_and(|fingerprint| fingerprint == step.fingerprint());

        if step.unless.is_none() && ran_before && !choices.turn_on.contains(&step.label) {
            ui.say(vec![
                label,
                span(
                    format!("ran before. Run it again with --run {}", step.label),
                    Style::Note,
                ),
            ]);
            outcomes.push(finish(Status::Skipped));
            continue;
        }

        let description = if step.description.is_empty() {
            &step.label
        } else {
            &step.description
        };

        ui.say(vec![label, span(description.as_str(), Style::Plain)]);
        ui.say(vec![span(UNDER, Style::Plain), span(command, Style::Bold)]);

        let go = if choices.turn_on.contains(&step.label) || choices.assume_yes {
            true
        } else {
            let question = Question {
                prompt: report::line(format!("{UNDER}Run it? [Y/n]: "), Style::Bold),
                kind: QuestionKind::YesNo { default: true },
            };

            let Some(reply) = ui.ask(&question) else {
                ui.say(report::warn(format!(
                    "{UNDER}not run, nobody was there to ask. Run it with --run {}.",
                    step.label
                )));
                outcomes.push(finish(Status::NotRun));
                continue;
            };

            matches!(reply.to_lowercase().as_str(), "" | "y" | "yes")
        };

        if !go {
            ui.say(report::note(format!("{UNDER}not run, as answered")));
            outcomes.push(finish(Status::NotRun));
            continue;
        }

        let status = execute(library, step, project_root, destination, platform, ui);
        outcomes.push(finish(status));
    }

    outcomes
}

/// Run one step, and say how it went.
fn execute(
    library: &Manifest,
    step: &RunStep,
    folder: &Path,
    destination: &Path,
    platform: Platform,
    ui: &mut dyn Ui,
) -> Status {
    let found = words(step, folder, destination);
    let mut command = Command::new(&found[0]);

    command.args(&found[1..]).current_dir(folder);

    // Where things are, under the names a step can read. The older names keep a script written for lib-installer
    // working as it was.
    for (prefix, tool) in [("LIBNIMA", "libnima"), ("INSTALLER", "libnima")] {
        command
            .env(format!("{prefix}_PROJECT"), folder)
            .env(format!("{prefix}_LIBRARY"), destination)
            .env(format!("{prefix}_NAME"), &library.name)
            .env(format!("{prefix}_VERSION"), &library.version)
            .env(format!("{prefix}_PLATFORM"), platform.name())
            .env(prefix, tool);
    }

    let code = if ui.shows_output() {
        command.status().map(|status| status.code())
    } else {
        quiet(&mut command);

        command.stdin(Stdio::null()).output().map(|output| {
            for stream in [&output.stdout, &output.stderr] {
                for line in String::from_utf8_lossy(stream).lines() {
                    ui.say(report::plain(line.to_string()));
                }
            }

            output.status.code()
        })
    };

    match code {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ui.say(report::bad(format!(
                "{UNDER}failed: {} is not installed, or not on the PATH",
                found[0]
            )));
            Status::Failed
        }
        Err(error) => {
            ui.say(report::bad(format!(
                "{UNDER}failed: {}",
                files::reason(&error)
            )));
            Status::Failed
        }
        Ok(Some(0)) => {
            ui.say(report::good(format!("{UNDER}done")));
            Status::Ran
        }
        Ok(code) => {
            ui.say(report::bad(format!(
                "{UNDER}failed, it ended with code {}",
                code.map_or_else(|| "none".to_string(), |code| code.to_string())
            )));
            Status::Failed
        }
    }
}

/// A step the window runs opens no console of its own on Windows.
#[cfg(windows)]
fn quiet(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn quiet(_command: &mut Command) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(command: &[&str], args: &[&str]) -> RunStep {
        RunStep {
            label: "make-key".to_string(),
            description: "Make it".to_string(),
            command: command.iter().map(ToString::to_string).collect(),
            args: args.iter().map(ToString::to_string).collect(),
            unless: None,
            unless_in_library: false,
        }
    }

    #[test]
    fn library_stands_for_its_folder() {
        let root = tempfile::tempdir().unwrap();
        let found = words(
            &step(&["tool", "{library}/a"], &["{library}"]),
            root.path(),
            &root.path().join("Libs/x"),
        );

        assert_eq!(found, ["tool", "Libs/x/a", "Libs/x"]);
    }

    #[test]
    fn a_word_is_shown_the_way_it_could_be_typed() {
        assert_eq!(quoted("a/b.c"), "a/b.c");
        assert_eq!(quoted("a b"), "'a b'");
        assert_eq!(quoted("it's"), "'it'\\''s'");
        assert_eq!(
            windows_line(&[
                "a b".to_string(),
                "c\"d".to_string(),
                String::new(),
                "e\\".to_string()
            ]),
            "\"a b\" c\\\"d \"\" e\\"
        );
    }
}
