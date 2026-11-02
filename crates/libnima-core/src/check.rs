//! Whether a library can go into a project, before anything is written.
//!
//! The answer comes in three marks, the same in the window's list and from `libnima --check`:
//!
//! ```text
//! OK     nothing stands in its way
//! CHECK  it installs, and there is something to do, with the way to do it
//! NO     it is not installed, and the reason is beside it
//! ```
//!
//! CHECK never stops an install, because a project can be laid out in ways this cannot read, and being wrong must never stop
//! someone using the library. NO is for what is known for certain: a library for another platform, one that needs a newer
//! libNIMA, or a file that does not describe a usable library.

use std::path::Path;

use crate::flow::{Request, Session, Source, say_requirements};
use crate::manifest::{self, MANIFEST_NAME};
use crate::paths;
use crate::platform::Platform;
use crate::report::{self, Style, item};
use crate::target::{Finding, Level};

/// What the check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mark {
    Ok,
    Check,
    No,
}

impl Mark {
    pub fn word(self) -> &'static str {
        match self {
            Mark::Ok => "OK",
            Mark::Check => "CHECK",
            Mark::No => "NO",
        }
    }
}

/// The mark findings add up to: a warning is something to do, and the rest is not.
pub fn mark_of(findings: &[Finding]) -> Mark {
    if findings.iter().any(|finding| finding.level == Level::Warn) {
        Mark::Check
    } else {
        Mark::Ok
    }
}

/// A problem in a library's file, and the line it is on when one line is.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Problem {
    pub line: Option<usize>,
    pub message: String,
    /// True for what is only worth saying, which refuses nothing.
    pub warning: bool,
}

/// Everything wrong with a libnima.yml's text, read against the library's own folder, for each platform it names.
///
/// `path` is what the problem is said to be in, and its text may be one being edited and not saved yet. What the editor
/// and `libnima --lint` show.
pub fn lint_text(root: &Path, path: &Path, text: &str) -> Vec<Problem> {
    let in_file = format!("{}: ", path.display());
    let plain = |message: &str| {
        message
            .strip_prefix(&in_file)
            .unwrap_or(message)
            .to_string()
    };
    let mut problems: Vec<Problem> = Vec::new();

    let node = match manifest::parse(text) {
        Ok(node) => node,
        Err(error) => {
            return vec![Problem {
                line: error.line,
                message: plain(&error.message),
                warning: false,
            }];
        }
    };

    let data = node.as_map().expect("parse() gives a mapping");
    let platforms = match manifest::platforms_of(data) {
        Ok(Some(platforms)) => platforms,
        Ok(None) => vec![],
        Err(error) => {
            return vec![Problem {
                line: error.line,
                message: plain(&error.message),
                warning: false,
            }];
        }
    };

    let mut add = |problem: Problem| {
        if !problems.iter().any(|have| have.message == problem.message) {
            problems.push(problem);
        }
    };

    // What every platform shares, and when it names none, the reason why that is a problem.
    let every: Vec<Option<Platform>> = if platforms.is_empty() {
        vec![None]
    } else {
        platforms.iter().copied().map(Some).collect()
    };

    for platform in every {
        match manifest::load_text(root, path, text, platform) {
            Ok(library) => {
                for warning in library.warnings {
                    add(Problem {
                        line: None,
                        message: warning,
                        warning: true,
                    });
                }
            }
            Err(error) => add(Problem {
                line: error.line,
                message: plain(&error.message),
                warning: false,
            }),
        }
    }

    problems
}

/// Everything wrong with the library's file in a folder: libnima.yml, or an installer.yml read through the converter.
pub fn lint(folder: &Path) -> Result<(std::path::PathBuf, Vec<Problem>), String> {
    let Some(path) = manifest::manifest_path(folder) else {
        return Err(format!("No {MANIFEST_NAME} in {}.", paths::posix(folder)));
    };

    let raw = std::fs::read(&path)
        .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let text =
        String::from_utf8(raw).map_err(|_| format!("{} is not UTF-8 text.", path.display()))?;
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    let (text, _) = match manifest::new_form(&name, &text) {
        Ok(found) => found,
        Err(error) => {
            return Ok((
                path,
                vec![Problem {
                    line: error.line,
                    message: error.message,
                    warning: false,
                }],
            ));
        }
    };

    let problems = lint_text(folder, &path, &text);

    Ok((path, problems))
}

/// `libnima --check`: say what the check finds about each library, and write nothing. Returns an exit code: 0 when each can
/// go in, 1 when one cannot.
pub fn run(session: &mut Session, request: &Request) -> i32 {
    let mut worst = 0;
    let mut marks: Vec<(String, Mark, String)> = Vec::new();

    for given in &request.libraries {
        session.ui().new_part();

        let Some(mut source): Option<Source> = session.stage(given, request.reference.as_deref())
        else {
            marks.push((given.clone(), Mark::No, "it could not be had".to_string()));
            worst = 1;
            continue;
        };

        if let Some(code) = session.load(&mut source) {
            if code != 0 {
                marks.push((given.clone(), Mark::No, "see above".to_string()));
                worst = 1;
            }

            continue;
        }

        let library = source.library.as_ref().expect("just read");
        let root = source
            .project_root
            .clone()
            .unwrap_or_else(|| session.project_root.clone());

        session.ui().say(report::blank());
        session.ui().say_all(report::banner(
            &library.name,
            &library.version,
            &library.description,
        ));

        let findings = session.check_library(library, &root);

        say_requirements(&findings, session.ui());

        let mark = mark_of(&findings);
        let name = library.name.clone();
        let first = findings
            .iter()
            .find(|finding| finding.level == Level::Warn)
            .map(|finding| finding.message.clone())
            .unwrap_or_default();

        marks.push((name, mark, first));
    }

    // What is said of them all is about none of them.
    if marks.len() > 1 {
        session.ui().new_part();
    }

    session.ui().say(report::blank());

    for (name, mark, why) in &marks {
        let style = match mark {
            Mark::Ok => Style::Good,
            Mark::Check => Style::Warn,
            Mark::No => Style::Bad,
        };

        session
            .ui()
            .say(item(mark.word(), why, name.clone(), style));
    }

    worst
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::testing::library;

    #[test]
    fn a_warning_makes_it_something_to_check() {
        assert_eq!(mark_of(&[]), Mark::Ok);
        assert_eq!(
            mark_of(&[Finding::ok("a"), Finding::info("b", "")]),
            Mark::Ok
        );
        assert_eq!(
            mark_of(&[Finding::ok("a"), Finding::warn("b", "")]),
            Mark::Check
        );
    }

    #[test]
    fn a_good_file_has_no_problems() {
        let root = tempfile::tempdir().unwrap();
        let folder = library(root.path(), "demo", "headers: [a.h]\n", &["a.h"]);
        let (_, problems) = lint(&folder).unwrap();

        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_problem_names_its_line() {
        let root = tempfile::tempdir().unwrap();
        let folder = library(
            root.path(),
            "demo",
            "requires:\n  peripherals:\n    - type: i2c\n      count: 0\nheaders: [a.h]\n",
            &["a.h"],
        );
        let (_, problems) = lint(&folder).unwrap();

        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].line, Some(6));
        assert!(
            problems[0].message.contains("has a count of 0"),
            "{}",
            problems[0].message
        );
    }

    #[test]
    fn a_warning_is_told_from_a_problem() {
        let root = tempfile::tempdir().unwrap();
        let folder = library(
            root.path(),
            "demo",
            "kind: gadget\nheaders: [a.h]\n",
            &["a.h"],
        );
        let (_, problems) = lint(&folder).unwrap();

        assert!(problems[0].warning);
        assert!(problems[0].message.contains("kind 'gadget'"));
    }

    #[test]
    fn each_platform_it_names_is_read() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("demo");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.h"), "").unwrap();
        std::fs::write(
            folder.join("libnima.yml"),
            "name: demo\nplatform: [stm32, zephyr]\nheaders: [a.h]\nzephyr:\n  bad: 1\n",
        )
        .unwrap();

        let (_, problems) = lint(&folder).unwrap();

        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].message.contains("zephyr has bad"),
            "{}",
            problems[0].message
        );
        assert_eq!(problems[0].line, Some(5));
    }

    #[test]
    fn text_being_edited_is_what_is_read() {
        let root = tempfile::tempdir().unwrap();
        let folder = library(root.path(), "demo", "headers: [a.h]\n", &["a.h"]);
        let path = folder.join("libnima.yml");

        assert!(
            lint_text(
                &folder,
                &path,
                "name: demo\nplatform: stm32\nheaders: [a.h]\n"
            )
            .is_empty()
        );

        let problems = lint_text(
            &folder,
            &path,
            "name: demo\nplatform: stm32\nheaders: [a.h\n",
        );
        assert!(!problems.is_empty());
    }
}
