//! The command itself, as someone types it at the root of a project, with nobody at the keyboard: its input is empty and its
//! output a pipe, so nothing here can wait for an answer nobody can give.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const CODE: &str = "headers:\n  - inc/demo.h\nsources:\n  - src/demo.c\n";

fn command(folder: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_libnima"));

    command
        .current_dir(folder)
        .args(args)
        .env("LIBNIMA_NO_UPDATE_CHECK", "1")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null());

    command
}

fn libnima(folder: &Path, args: &[&str]) -> Output {
    command(folder, args).output().unwrap()
}

fn out(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn code(output: &Output) -> i32 {
    output.status.code().unwrap_or(-1)
}

fn put(root: &Path, files: &[(&str, &str)]) {
    for (name, text) in files {
        let path = root.join(name);

        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

/// A library in a folder of its own.
fn library(root: &Path, name: &str, platform: &str, body: &str) -> PathBuf {
    let folder = root.join(name);

    put(
        &folder,
        &[
            ("inc/demo.h", "/**\n * @version 1.2.0\n */\n"),
            ("src/demo.c", "/* source */\n"),
            (
                "libnima.yml",
                &format!("name: {name}\nplatform: {platform}\n{CODE}{body}"),
            ),
        ],
    );

    folder
}

fn project(root: &Path) -> PathBuf {
    let folder = root.join("Proj");

    put(&folder, &[("Core/Src/main.c", "/* main */\n")]);

    folder
}

fn text(path: &Path) -> &str {
    path.to_str().unwrap()
}

fn listing(folder: &Path) -> Vec<String> {
    libnima_core::paths::files_below(folder)
}

// ----------------------------------------------------------------------------
// What it says of itself
// ----------------------------------------------------------------------------

#[test]
fn the_version_is_shown_and_nothing_else_happens() {
    let root = tempfile::tempdir().unwrap();

    for flag in ["--version", "-V"] {
        let done = libnima(root.path(), &[flag]);

        assert_eq!(code(&done), 0);
        assert_eq!(
            out(&done),
            format!("libnima {}\n", env!("CARGO_PKG_VERSION"))
        );
    }
}

#[test]
fn the_help_lists_each_platforms_options_under_its_name() {
    let root = tempfile::tempdir().unwrap();
    let done = libnima(root.path(), &["--help"]);
    let shown = out(&done);

    assert_eq!(code(&done), 0);

    for option in [
        "--uninstall",
        "--key",
        "--with",
        "--without",
        "--run",
        "--no-run",
        "--ref",
        "--dir",
        "--check",
        "--lint",
        "--convert",
        "--gui",
    ] {
        assert!(shown.contains(option), "{option} is not in:\n{shown}");
    }

    assert!(
        shown.contains("[possible values: stm32, zephyr, esp32, arduino, pio]"),
        "{shown}"
    );

    for (heading, option) in [
        ("STM32 only", "--ide"),
        ("Zephyr only", "--conf"),
        ("Zephyr only", "--overlay"),
        ("Arduino only", "--into"),
    ] {
        let at = shown
            .find(heading)
            .unwrap_or_else(|| panic!("{heading} is not in:\n{shown}"));

        assert!(
            shown[at..].contains(option),
            "{option} is not under {heading}:\n{shown}"
        );
    }
}

#[test]
fn nothing_to_install_says_how_rather_than_crashing() {
    // The first thing many people type.
    let root = tempfile::tempdir().unwrap();
    let done = libnima(root.path(), &[]);

    assert_eq!(code(&done), 2);
    assert!(
        err(&done).contains("Say what to install, for example: libnima owner/example"),
        "{}",
        err(&done)
    );
    assert!(out(&done).contains("Usage: libnima"), "{}", out(&done));
    assert!(!err(&done).contains("panicked"), "{}", err(&done));
}

#[test]
fn a_message_naming_the_command_names_platform_when_it_was_given() {
    let root = tempfile::tempdir().unwrap();
    let done = libnima(root.path(), &["--platform", "zephyr"]);

    assert_eq!(code(&done), 2);
    assert!(
        err(&done).contains("libnima --platform zephyr owner/example"),
        "{}",
        err(&done)
    );
}

#[test]
fn an_option_may_be_shortened_as_far_as_it_stays_one() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let source = library(root.path(), "demo", "stm32", "");
    let done = libnima(root.path(), &[text(&source), "--proj", text(&folder)]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(folder.join("demo/inc/demo.h").is_file());
}

// ----------------------------------------------------------------------------
// From the root of a project
// ----------------------------------------------------------------------------

#[test]
fn run_from_the_root_of_a_project_it_installs_there() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let source = library(root.path(), "demo", "stm32", "");
    let done = libnima(&folder, &[text(&source)]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(folder.join("demo/inc/demo.h").is_file());
    assert!(folder.join(".libnima.json").is_file());
    assert!(out(&done).contains("#include \"demo.h\""), "{}", out(&done));
}

#[test]
fn an_existing_folder_wins_over_a_library_name() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());

    library(&folder, "demo", "stm32", "");

    let done = libnima(&folder, &["demo"]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(folder.join("demo/inc/demo.h").is_file());
    // The library where it stands, which keeps the file it is installed from.
    assert!(folder.join("demo/libnima.yml").is_file());
    assert_eq!(
        libnima_core::record::installed_libraries(&folder)["demo"]["folder"],
        "demo"
    );
}

#[test]
fn a_project_that_does_not_exist_is_not_made() {
    // A typo in --project, which would otherwise get a folder of its own and a library no build will ever find.
    let root = tempfile::tempdir().unwrap();
    let source = library(root.path(), "demo", "stm32", "");
    let typo = root.path().join("Porj/deeper");

    for check in [false, true] {
        let mut args = vec![text(&source), "--project", text(&typo), "--yes"];

        if check {
            args.push("--check");
        }

        let done = libnima(root.path(), &args);

        assert_eq!(code(&done), 2, "{}", out(&done));
        assert!(
            err(&done).contains("given with --project, does not exist."),
            "{}",
            err(&done)
        );
        assert!(!root.path().join("Porj").exists());
    }

    // A file is no project either.
    let file = root.path().join("notes.txt");

    fs::write(&file, "mine\n").unwrap();

    let done = libnima(root.path(), &[text(&source), "--project", text(&file)]);

    assert_eq!(code(&done), 2, "{}", out(&done));
    assert!(err(&done).contains("is a file"), "{}", err(&done));
    assert_eq!(fs::read_to_string(&file).unwrap(), "mine\n");
}

#[test]
fn yes_shows_each_question_with_the_answer_it_takes() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let source = library(root.path(), "demo", "stm32", "");
    let done = libnima(&folder, &[text(&source), "--yes"]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(
        out(&done).contains("Folder to install demo into [demo]: (--yes)"),
        "{}",
        out(&done)
    );
}

#[test]
fn nothing_is_asked_when_nobody_can_answer() {
    // A script or a build server would otherwise wait for an answer for ever, and this test with it.
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let source = library(root.path(), "demo", "stm32", "");
    let done = libnima(&folder, &[text(&source)]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(
        !out(&done).contains("Folder to install demo into"),
        "{}",
        out(&done)
    );
}

#[test]
fn a_mistyped_path_says_so() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let done = libnima(&folder, &["./no-such-library-9f3"]);

    assert_eq!(code(&done), 2);
    assert!(
        err(&done).contains("./no-such-library-9f3 does not exist."),
        "{}",
        err(&done)
    );
    assert!(!err(&done).contains("panicked"), "{}", err(&done));
}

#[test]
fn a_pipe_gets_plain_text_with_no_escape_codes() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let source = library(root.path(), "demo", "stm32", "");
    let done = libnima(&folder, &[text(&source)]);

    assert!(!out(&done).contains('\u{1b}'), "{:?}", out(&done));
    assert!(!err(&done).contains('\u{1b}'), "{:?}", err(&done));
}

#[test]
fn a_step_that_fails_ends_the_run_with_one() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let failing = if cfg!(windows) {
        "[cmd, /c, \"exit 3\"]"
    } else {
        "[sh, -c, \"exit 3\"]"
    };
    let source = library(
        root.path(),
        "demo",
        "stm32",
        &format!("run:\n  - label: bad\n    command: {failing}\n"),
    );
    let done = libnima(&folder, &[text(&source), "--run", "bad"]);

    assert_eq!(code(&done), 1);
    assert!(folder.join("demo/inc/demo.h").is_file());
}

#[test]
fn a_library_comes_back_out() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let source = library(root.path(), "demo", "stm32", "");

    assert_eq!(code(&libnima(&folder, &[text(&source)])), 0);

    let done = libnima(&folder, &["--uninstall", "demo"]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(!folder.join("demo").exists());
}

#[test]
fn a_platform_that_is_none_is_refused_before_anything() {
    let root = tempfile::tempdir().unwrap();
    let done = libnima(root.path(), &["x", "--platform", "avr"]);

    assert_eq!(code(&done), 2);
    assert!(err(&done).contains("avr"), "{}", err(&done));
}

// ----------------------------------------------------------------------------
// --check
// ----------------------------------------------------------------------------

#[test]
fn a_check_writes_nothing_and_marks_each_library() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let good = library(root.path(), "demo", "stm32", "");
    let before = listing(&folder);
    let done = libnima(&folder, &[text(&good), "--check"]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert_eq!(listing(&folder), before);
    assert!(out(&done).contains("OK"), "{}", out(&done));
}

#[test]
fn a_check_of_what_cannot_go_in_ends_with_one() {
    let root = tempfile::tempdir().unwrap();
    let folder = project(root.path());
    let bad = library(root.path(), "demo", "stm32", "");

    fs::remove_file(bad.join("src/demo.c")).unwrap();

    let done = libnima(&folder, &[text(&bad), "--check"]);

    assert_eq!(code(&done), 1);
    assert!(out(&done).contains("NO"), "{}", out(&done));
}

#[test]
fn a_name_esp_idf_has_already_is_warned_about() {
    let root = tempfile::tempdir().unwrap();
    let idf = root.path().join("idf");
    let folder = root.path().join("app");
    let source = library(root.path(), "demo", "esp32", "");

    fs::create_dir_all(idf.join("components/demo")).unwrap();
    put(
        &folder,
        &[(
            "CMakeLists.txt",
            "include($ENV{IDF_PATH}/tools/cmake/project.cmake)\n",
        )],
    );

    let done = command(&folder, &[text(&source), "--check"])
        .env("IDF_PATH", &idf)
        .output()
        .unwrap();
    let said = out(&done);

    assert!(
        said.contains("ESP-IDF has a component called demo"),
        "{said}"
    );
    assert!(said.contains("components/demo_lib"), "{said}");
    assert!(said.contains("CHECK"), "{said}");
}

#[test]
fn the_sketchbook_is_found_where_arduino_cli_looks_first() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Arduino");
    let source = library(root.path(), "demo", "arduino", "");
    let here = root.path().join("here");

    fs::create_dir_all(&here).unwrap();

    let done = command(&here, &[text(&source)])
        .env("ARDUINO_DIRECTORIES_USER", &book)
        .output()
        .unwrap();

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(
        out(&done).contains("(from ARDUINO_DIRECTORIES_USER)"),
        "{}",
        out(&done)
    );
    assert!(book.join("libraries/demo/library.properties").is_file());
}

// ----------------------------------------------------------------------------
// --lint and --convert
// ----------------------------------------------------------------------------

#[test]
fn a_good_file_is_said_to_be_good() {
    let root = tempfile::tempdir().unwrap();
    let source = library(root.path(), "demo", "stm32", "");
    let done = libnima(&source, &["--lint"]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(
        out(&done).contains("libnima.yml is good."),
        "{}",
        out(&done)
    );
}

#[test]
fn a_warning_names_its_line_and_stops_nothing() {
    let root = tempfile::tempdir().unwrap();
    let source = library(root.path(), "demo", "stm32", "kind: gadget\n");
    let done = libnima(root.path(), &["--lint", text(&source)]);

    assert_eq!(code(&done), 0, "{}", err(&done));
    assert!(
        out(&done).contains("warning: libnima.yml"),
        "{}",
        out(&done)
    );
    assert!(out(&done).contains("gadget"), "{}", out(&done));
}

#[test]
fn a_problem_names_its_line_and_ends_with_one() {
    let root = tempfile::tempdir().unwrap();
    let source = library(
        root.path(),
        "demo",
        "stm32",
        "requires:\n  peripherals:\n    - type: i2c\n      count: 0\n",
    );
    let done = libnima(&source, &["--lint"]);

    assert_eq!(code(&done), 1);
    assert!(
        out(&done).contains("error: libnima.yml:10"),
        "{}",
        out(&done)
    );
}

#[test]
fn a_folder_with_no_file_cannot_be_looked_at() {
    let root = tempfile::tempdir().unwrap();
    let done = libnima(root.path(), &["--lint"]);

    assert_eq!(code(&done), 2);
    assert!(err(&done).contains("No libnima.yml"), "{}", err(&done));
}

const OLD: &str = "name: demo\nplatform: stm32\nrequires:\n  lib_installer: 4.0.0\nfiles:\n  headers:\n    - inc/demo.h # the header\n";

#[test]
fn convert_writes_the_new_file_and_keeps_the_old() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("old");

    put(
        &folder,
        &[
            ("installer.yml", OLD),
            ("inc/demo.h", "/**\n * @version 1.0.0\n */\n"),
        ],
    );

    let done = libnima(&folder, &["--convert"]);
    let written = fs::read_to_string(folder.join("libnima.yml")).unwrap();

    assert_eq!(code(&done), 0, "{}\n{}", out(&done), err(&done));
    assert_eq!(
        fs::read_to_string(folder.join("installer.yml")).unwrap(),
        OLD
    );
    assert!(
        written.contains("headers:\n  - inc/demo.h # the header\n"),
        "{written}"
    );
    assert!(!written.contains("lib_installer"), "{written}");
    assert!(
        out(&done).contains("Wrote libnima.yml. installer.yml is left as it was."),
        "{}",
        out(&done)
    );
    assert!(
        out(&done).contains("libnima.yml is good."),
        "{}",
        out(&done)
    );

    // Nothing is written over.
    let again = libnima(&folder, &["--convert"]);

    assert_eq!(code(&again), 2);
    assert!(err(&again).contains("is there already"), "{}", err(&again));
}

#[test]
fn convert_says_what_it_cannot_read() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("older");

    put(
        &folder,
        &[(
            "installer.yml",
            "name: demo\nrequires:\n  lib_installer: 2.0.0\nfiles:\n  headers:\n    - demo.h\n",
        )],
    );

    let done = libnima(&folder, &["--convert"]);

    assert_eq!(code(&done), 2);
    assert!(err(&done).contains("installer.yml:"), "{}", err(&done));
    assert!(!folder.join("libnima.yml").exists());
}

#[test]
fn convert_with_nothing_to_convert_says_so() {
    let root = tempfile::tempdir().unwrap();
    let done = libnima(root.path(), &["--convert"]);

    assert_eq!(code(&done), 2);
    assert!(err(&done).contains("installer.yml"), "{}", err(&done));
}

// The real PATH of whoever runs the tests is left alone: where a start file holds it, HOME points somewhere else. On Windows it is
// the registry, which only a machine that is thrown away afterwards should try, so CI does that with the program itself.
#[cfg(not(windows))]
#[test]
fn the_command_puts_its_folder_on_the_path_once_and_takes_it_off_again() {
    let home = tempfile::tempdir().unwrap();
    let run = |flag: &str| {
        command(home.path(), &[flag])
            .env("HOME", home.path())
            .env("SHELL", "/bin/bash")
            .output()
            .unwrap()
    };
    let profile = home.path().join(".profile");

    assert_eq!(code(&run("--add-to-path")), 0);

    let added = fs::read_to_string(&profile).unwrap();

    assert!(added.contains("# >>> libnima >>>"), "{added}");
    assert!(added.contains("export PATH=\"$PATH:"), "{added}");

    assert_eq!(code(&run("--add-to-path")), 0);
    assert_eq!(
        fs::read_to_string(&profile).unwrap(),
        added,
        "twice writes it once"
    );

    assert_eq!(code(&run("--remove-from-path")), 0);
    assert_eq!(fs::read_to_string(&profile).unwrap(), "");
}

#[test]
fn the_command_writes_the_schema_an_editor_reads() {
    let root = tempfile::tempdir().unwrap();
    let output = libnima(root.path(), &["--schema"]);
    let schema: serde_json::Value = serde_json::from_str(&out(&output)).unwrap();

    assert_eq!(code(&output), 0);
    assert_eq!(schema["title"], "libnima.yml");
    assert!(schema["properties"]["headers"].is_object());
}

#[test]
fn two_libraries_have_a_line_between_them_and_one_has_none() {
    let root = tempfile::tempdir().unwrap();
    let one = library(root.path(), "one", "stm32", "");
    let two = library(root.path(), "two", "stm32", "");
    let folder = project(root.path());
    let rules = |said: &str| {
        said.lines()
            .filter(|line| line.starts_with("-----"))
            .count()
    };

    let alone = libnima(&folder, &[text(&one)]);

    assert_eq!(code(&alone), 0, "{}", err(&alone));
    assert_eq!(rules(&out(&alone)), 0, "{}", out(&alone));

    let other = root.path().join("Other");

    put(&other, &[("Core/Src/main.c", "/* main */\n")]);

    let both = libnima(&other, &[text(&one), text(&two)]);
    let said = out(&both);

    assert_eq!(code(&both), 0, "{}", err(&both));
    assert_eq!(rules(&said), 1, "{said}");

    let at = |wanted: &str| {
        said.find(wanted)
            .unwrap_or_else(|| panic!("no {wanted}: {said}"))
    };

    assert!(at("one 1.2.0") < at("-----"), "{said}");
    assert!(at("-----") < at("two 1.2.0"), "{said}");
}
