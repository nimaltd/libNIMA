//! The libnima command. It has no window, so it runs on a build server too.
//!
//! The real work lives in `libnima-core`. This file only connects it to the terminal.

mod console;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use clap::{CommandFactory, Parser};
use libnima_core::check;
use libnima_core::convert;
use libnima_core::flow::{self, Request};
use libnima_core::manifest::{INSTALLER_NAME, MANIFEST_NAME};
use libnima_core::paths;
use libnima_core::platform::stm32::ide;
use libnima_core::platform::{Platform, PlatformArgs};
use libnima_core::report::{self, Style, Ui};
use libnima_core::shell;
use libnima_core::updates;

use console::Terminal;

/// Install a library into your project.
#[derive(Parser)]
#[command(
    name = "libnima",
    version = libnima_core::VERSION,
    infer_long_args = true,
    after_help = "Run this from the root of your project.\n\n\
                  Exit codes:\n  \
                  0  done, or nothing found to report\n  \
                  1  --check found something to do first (CHECK) or a library that cannot go in (NO), --lint found an \
                  error, or a run step failed once its library was in\n  \
                  2  a mistake in the command line, or a run that was refused or failed: nothing of it was written"
)]
struct Args {
    /// What to install: a name like "owner/example", a GitHub URL, a downloaded .zip, or a folder. Give several to install
    /// them together, such as a library and the zip of one it needs.
    library: Vec<String>,

    /// Branch, tag or commit to fetch for the libraries named here, the same as owner/name@ref. Default the repository's
    /// default branch, whatever it is called. A library one of them needs comes from the ref its libnima.yml names, or its
    /// default branch.
    #[arg(long = "ref")]
    reference: Option<String>,

    /// Folder to install into.
    #[arg(long = "dir", value_name = "FOLDER")]
    folder: Option<String>,

    /// Take this option of the library, such as littlefs for spif, without being asked. Give it once for each option.
    #[arg(long = "with", value_name = "OPTION")]
    with_options: Vec<String>,

    /// Leave this option out, or take it out of a library already installed.
    #[arg(long = "without", value_name = "OPTION")]
    without_options: Vec<String>,

    /// Root of your project. Defaults to this folder.
    #[arg(long, value_name = "FOLDER")]
    project: Option<PathBuf>,

    /// Run this step of the library once it is in, without being asked, such as one that makes a key. Give it once for each
    /// step.
    #[arg(long = "run", value_name = "STEP")]
    run: Vec<String>,

    /// Leave this step of the library unrun, without being asked.
    #[arg(long = "no-run", value_name = "STEP")]
    no_run: Vec<String>,

    /// Take the answer each question offers, without asking, and run every step. For a script or a build server.
    #[arg(short = 'y', long)]
    yes: bool,

    /// Do not ask GitHub whether a newer libNIMA is out. CI and LIBNIMA_NO_UPDATE_CHECK turn it off as well.
    #[arg(long = "no-update-check")]
    no_update_check: bool,

    /// A GitHub token, for a library in a private repository. GITHUB_TOKEN works as well, and keeps it out of your shell's
    /// history. Sent to GitHub and nowhere else.
    #[arg(long = "key", visible_alias = "token", value_name = "TOKEN")]
    key: Option<String>,

    /// Take these libraries out of the project: their files, and their place in the build. What you wrote in them is saved
    /// beside them first.
    #[arg(long, num_args = 1.., value_name = "NAME")]
    uninstall: Vec<String>,

    /// The platform of this project, when what libNIMA finds in it is wrong, or a library names several.
    #[arg(long, value_parser = ["stm32", "zephyr", "esp32", "arduino", "pio"])]
    platform: Option<String>,

    /// Only check the libraries named against this project, and write nothing.
    #[arg(long)]
    check: bool,

    /// Check the libnima.yml in a folder, this one when none is given, with no project. For a library's own CI.
    #[arg(long, num_args = 0..=1, value_name = "FOLDER")]
    lint: Option<Option<PathBuf>>,

    /// Write the installer.yml in a folder, this one when none is given, out as libnima.yml.
    #[arg(long, num_args = 0..=1, value_name = "FOLDER")]
    convert: Option<Option<PathBuf>>,

    /// Write the JSON Schema of libnima.yml, which an editor can use for help on the file.
    #[arg(long)]
    schema: bool,

    /// Open the window, on this folder or the one --project gives.
    #[arg(long)]
    gui: bool,

    /// Put this program's folder on your PATH, so libnima runs from the root of any project. The installers do it already: this
    /// is for the command alone, from its zip.
    #[arg(long = "add-to-path")]
    add_to_path: bool,

    /// Take this program's folder off your PATH.
    #[arg(long = "remove-from-path", conflicts_with = "add_to_path")]
    remove_from_path: bool,

    /// Only register with this IDE. Default is every one found.
    #[arg(long, help_heading = "STM32 only", value_parser = ide::NAMES)]
    ide: Option<String>,

    /// The Kconfig file of the application that switches the library on, such as prj.conf or boards/nrf52840dk_nrf52840.conf.
    /// Asked when the application has more than one, prj.conf otherwise. A library already in one stays there.
    #[arg(long, help_heading = "Zephyr only", value_name = "FILE")]
    conf: Option<String>,

    /// The overlay of the application that includes a library's devicetree files, such as boards/nrf52840dk_nrf52840.overlay,
    /// or every one a wildcard matches, such as 'boards/*.overlay', in place of what the library's libnima.yml says. Asked
    /// when the application has more than one and the library names none.
    #[arg(long, help_heading = "Zephyr only", value_name = "FILE")]
    overlay: Option<String>,

    /// Where the libraries go: libraries, the sketchbook's libraries folder, for every sketch, or sketch, the src folder of the
    /// sketch this is run from, for it only. Asked when run from a sketch, libraries otherwise.
    #[arg(long, help_heading = "Arduino only", value_parser = ["libraries", "sketch"])]
    into: Option<String>,

    /// The sketchbook folder, the one holding libraries/. Found the way Arduino finds it when not given.
    #[arg(long, help_heading = "Arduino only", value_name = "FOLDER")]
    sketchbook: Option<String>,
}

/// Where the window's program is: beside this one, in the macOS app, or on the PATH.
fn find_window() -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "libnima-app.exe"
    } else {
        "libnima-app"
    };
    let here = std::env::current_exe().ok()?;
    let folder = here.parent()?;
    let beside = folder.join(name);

    if beside.is_file() {
        return Some(beside);
    }

    let on_mac = Path::new("/Applications/libNIMA.app/Contents/MacOS").join(name);

    if on_mac.is_file() {
        return Some(on_mac);
    }

    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|folder| folder.join(name))
        .find(|path| path.is_file())
}

fn open_window(project: &Path, ui: &mut Terminal) -> u8 {
    let Some(window) = find_window() else {
        ui.error(&format!(
            "The window is not on this computer: this is the command alone. Get the installer from {}",
            libnima_core::RELEASES_PAGE
        ));
        return 2;
    };

    match Command::new(&window).arg(project).spawn() {
        Ok(_) => 0,
        Err(error) => {
            ui.error(&format!("Could not start {}: {error}", window.display()));
            2
        }
    }
}

/// Show what is wrong with a library's file. Returns an exit code: 1 when it cannot be read as a library.
fn run_lint(folder: &Path, ui: &mut Terminal) -> u8 {
    let (path, problems) = match check::lint(folder) {
        Ok(found) => found,
        Err(message) => {
            ui.error(&message);
            return 2;
        }
    };

    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // The verdict says which file it is about, as a folder can be given before --lint, or none at all.
    let whole = paths::posix(&path);
    let failed = problems.iter().any(|problem| !problem.warning);

    for problem in &problems {
        let place = problem
            .line
            .map_or_else(|| name.clone(), |line| format!("{name}:{line}"));
        let (mark, style) = if problem.warning {
            ("warning", Style::Warn)
        } else {
            ("error", Style::Bad)
        };

        ui.say(vec![
            report::span(format!("{mark}: "), style),
            report::span(place, Style::Bold),
            report::span(format!("  {}", problem.message), Style::Plain),
        ]);
    }

    if failed {
        return 1;
    }

    ui.say(report::good(if problems.is_empty() {
        format!("{whole} is good.")
    } else {
        format!("{whole} can be read, with what is said above.")
    }));

    0
}

/// Write a folder's installer.yml out as libnima.yml. The old file stays, for whoever still installs with lib-installer.
fn run_convert(folder: &Path, ui: &mut Terminal) -> u8 {
    let old = folder.join(INSTALLER_NAME);
    let new = folder.join(MANIFEST_NAME);

    if new.exists() {
        ui.error(&format!(
            "{} is there already, so nothing was written.",
            paths::posix(&new)
        ));
        return 2;
    }

    let text = match std::fs::read_to_string(&old) {
        Ok(text) => text,
        Err(error) => {
            ui.error(&format!("Could not read {}: {error}", paths::posix(&old)));
            return 2;
        }
    };

    let converted = match convert::convert(&text) {
        Ok(converted) => converted,
        Err(error) => {
            ui.error(&format!("{INSTALLER_NAME}: {}", error.message));
            return 2;
        }
    };

    if let Err(error) = std::fs::write(&new, converted) {
        ui.error(&format!("Could not write {}: {error}", paths::posix(&new)));
        return 2;
    }

    ui.say(report::good(format!(
        "Wrote {MANIFEST_NAME}. {INSTALLER_NAME} is left as it was."
    )));
    ui.say(report::note(
        "A script is now a command that runs python: look over any step under run. Comments that show an old key stay as they were.",
    ));

    run_lint(folder, ui)
}

/// Put this program's folder on the PATH, or take it off.
fn run_path(adding: bool, ui: &mut Terminal) -> u8 {
    let folder = std::env::current_exe()
        .ok()
        .and_then(|here| here.parent().map(Path::to_path_buf));

    let Some(folder) = folder else {
        ui.error("Could not tell which folder this program is in.");
        return 2;
    };

    let done = if adding {
        shell::add_to_path(&folder)
    } else {
        shell::remove_from_path(&folder)
    };

    match done {
        Ok(said) => {
            ui.say(report::good(said));
            0
        }
        Err(error) => {
            ui.error(&format!("Could not change your PATH: {error}"));
            2
        }
    }
}

/// What in the command line would go unused: two of the runs that are each a run of their own, or something beside one
/// of them it does not take. A folder given before --lint or --convert, with none after it, is the one it takes. None
/// when every argument is used.
fn unused(args: &mut Args) -> Option<String> {
    let modes: Vec<&str> = [
        (args.schema, "--schema"),
        (args.lint.is_some(), "--lint"),
        (args.convert.is_some(), "--convert"),
    ]
    .into_iter()
    .filter_map(|(given, name)| given.then_some(name))
    .collect();

    if modes.len() > 1 {
        return Some(format!(
            "{} are each a run of their own. Give one of them.",
            modes.join(" and ")
        ));
    }

    for slot in [&mut args.lint, &mut args.convert] {
        if let Some(folder) = slot
            && folder.is_none()
            && args.library.len() == 1
        {
            *folder = Some(PathBuf::from(args.library.remove(0)));
        }
    }

    match modes.first() {
        Some(mode) if !args.library.is_empty() => Some(format!(
            "{mode} takes {}, so {} would go unused.",
            if *mode == "--schema" {
                "nothing more"
            } else {
                "one folder"
            },
            args.library.join(" and ")
        )),
        _ => None,
    }
}

fn run(mut args: Args) -> u8 {
    let mut ui = Terminal::new(args.yes);
    let here = args.project.clone().unwrap_or_else(|| PathBuf::from("."));

    if let Some(problem) = unused(&mut args) {
        ui.error(&problem);
        return 2;
    }

    if args.schema {
        println!(
            "{}",
            serde_json::to_string_pretty(&libnima_core::schema::json_schema()).unwrap_or_default()
        );
        return 0;
    }

    if args.add_to_path || args.remove_from_path {
        return run_path(args.add_to_path, &mut ui);
    }

    if args.gui {
        return open_window(&paths::resolve(&here), &mut ui);
    }

    if let Some(folder) = &args.lint {
        return run_lint(
            &paths::resolve(folder.as_deref().unwrap_or(Path::new("."))),
            &mut ui,
        );
    }

    if let Some(folder) = &args.convert {
        return run_convert(
            &paths::resolve(folder.as_deref().unwrap_or(Path::new("."))),
            &mut ui,
        );
    }

    if args.library.is_empty() && args.uninstall.is_empty() {
        let _ = Args::command().print_help();
        println!();
    }

    let request = Request {
        libraries: args.library,
        reference: args.reference,
        folder: args.folder,
        with_options: args.with_options,
        without_options: args.without_options,
        project: args.project,
        run_on: args.run,
        run_off: args.no_run,
        yes: args.yes,
        key: args.key,
        uninstall: args.uninstall,
        platform: args.platform.as_deref().and_then(Platform::from_name),
        args: PlatformArgs {
            ide: args.ide,
            conf: args.conf,
            overlay: args.overlay,
            into: args.into,
            sketchbook: args.sketchbook,
            uninstalling: Vec::new(),
        },
        check_only: args.check,
        ..Request::default()
    };

    let check = updates::Check::start(!args.no_update_check);
    let code = flow::run(&request, &mut ui);

    if let Some(newer) = check.message() {
        ui.say(report::blank());
        ui.say(report::warn(newer));
    }

    u8::try_from(code).unwrap_or(2)
}

fn main() -> ExitCode {
    ExitCode::from(run(Args::parse()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(words: &[&str]) -> Args {
        let mut all = vec!["libnima"];
        all.extend_from_slice(words);

        Args::try_parse_from(all).unwrap()
    }

    #[test]
    fn the_options_are_put_together_correctly() {
        Args::command().debug_assert();
    }

    #[test]
    fn several_libraries_and_repeated_options_are_read() {
        let args = parse(&["a", "b", "--with", "x", "--with", "y", "--run", "k", "-y"]);

        assert_eq!(args.library, ["a", "b"]);
        assert_eq!(args.with_options, ["x", "y"]);
        assert_eq!(args.run, ["k"]);
        assert!(args.yes);
    }

    #[test]
    fn uninstall_takes_every_name_after_it() {
        assert_eq!(parse(&["--uninstall", "a", "b"]).uninstall, ["a", "b"]);
    }

    #[test]
    fn a_token_goes_by_either_name() {
        assert_eq!(parse(&["x", "--key", "t"]).key.as_deref(), Some("t"));
        assert_eq!(parse(&["x", "--token", "t"]).key.as_deref(), Some("t"));
    }

    #[test]
    fn a_folder_after_lint_and_convert_is_optional() {
        assert_eq!(parse(&["--lint"]).lint, Some(None));
        assert_eq!(parse(&["--lint", "a"]).lint, Some(Some(PathBuf::from("a"))));
        assert_eq!(parse(&["--convert"]).convert, Some(None));
    }

    #[test]
    fn every_argument_is_used_or_refused() {
        // A folder before --lint is the one it takes.
        let mut before = parse(&["../bad", "--lint"]);
        assert!(unused(&mut before).is_none());
        assert_eq!(before.lint, Some(Some(PathBuf::from("../bad"))));

        for words in [
            &["--lint", "good", "bad"][..],
            &["--lint", "bad", "--schema"],
            &["--schema", "x"],
            &["--lint", "--convert"],
        ] {
            let mut args = parse(words);

            assert!(unused(&mut args).is_some(), "{words:?}");
        }

        assert!(unused(&mut parse(&["--schema"])).is_none());
        assert!(unused(&mut parse(&["a", "b"])).is_none());
    }

    #[test]
    fn the_help_says_what_the_exit_codes_mean() {
        let help = Args::command().render_long_help().to_string();

        assert!(help.contains("Exit codes:"), "{help}");
    }

    #[test]
    fn a_platform_that_does_not_exist_is_refused() {
        assert!(Args::try_parse_from(["libnima", "x", "--platform", "avr"]).is_err());
        assert!(Args::try_parse_from(["libnima", "x", "--ide", "vscode"]).is_err());
        assert!(Args::try_parse_from(["libnima", "--add-to-path", "--remove-from-path"]).is_err());
        assert!(Args::try_parse_from(["libnima", "x", "--into", "there"]).is_err());
    }
}
