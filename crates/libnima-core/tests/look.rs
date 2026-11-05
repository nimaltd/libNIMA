//! What a window reads: a project as data, a library looked at against it, and what an install would change. None of it writes
//! to the project.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use libnima_core::check::Mark;
use libnima_core::flow::Request;
use libnima_core::look::{self, ChangeKind, NeedState};
use libnima_core::platform::{Platform, PlatformArgs};
use libnima_core::target::Level;

fn offline() -> libnima_core::download::Client {
    Github::serving(&[]).client()
}

fn listing(folder: &Path) -> Vec<String> {
    libnima_core::paths::files_below(folder)
}

fn looked_at(project: &Path, source: &Path) -> look::Looked {
    look::look_at(
        offline(),
        project,
        None,
        &PlatformArgs::default(),
        source.to_str().unwrap(),
    )
}

fn with_cmake(root: &Path) -> PathBuf {
    project_with(
        root,
        Ides {
            cmake: true,
            ..Ides::default()
        },
    )
}

// ----------------------------------------------------------------------------
// A project
// ----------------------------------------------------------------------------

#[test]
fn a_project_says_what_it_looks_like_and_which_file_told() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[("boards/x.conf", "")], false);
    let found = look::project(&project, None);

    assert_eq!(found.platform, Some(Platform::Zephyr));
    assert!(!found.recorded);
    assert!(
        found.candidates[0].evidence.contains("CMakeLists.txt"),
        "{found:?}"
    );
    assert_eq!(found.conf_files, ["prj.conf", "boards/x.conf"]);
    assert!(found.installed.is_empty());
}

#[test]
fn a_project_lists_what_it_has_and_its_record_says_which_platform() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!(
            "version: 1.2.0\n{DEMO_CODE}options:\n  x:\n    description: more\n    headers: [extra.h]\n"
        ),
        &[("extra.h", "")],
    );
    let mut asked = request(&project, &[&source]);

    asked.with_options = vec!["x".to_string()];
    assert_eq!(run_with(offline(), &asked).0, 0);

    let found = look::project(&project, None);
    let demo = &found.installed[0];

    assert_eq!(found.platform, Some(Platform::Stm32));
    assert!(found.recorded);
    assert_eq!(demo.name, "demo");
    assert_eq!(demo.version, "1.2.0");
    assert_eq!(demo.folder, "demo");
    assert_eq!(demo.options, ["x"]);
    assert!(
        demo.source.is_some(),
        "a library from a folder can be updated from it"
    );
}

#[test]
fn a_project_of_an_older_installer_lists_what_it_has_and_nothing_is_moved_yet() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let old = project.join(".stm32-installer.json");

    write(
        &old,
        "{\"libraries\": {\"seq\": {\"folder\": \"seq\", \"version\": \"2.0.1\", \"repository\": \"https://github.com/nimaltd/seq\"},\
         \"mine\": {\"folder\": \"mine\", \"version\": \"1.0.0\"}}}",
    );
    let before = listing(&project);

    let found = look::project(&project, None);

    assert_eq!(found.platform, Some(Platform::Stm32));
    assert!(found.recorded);
    assert_eq!(found.installed.len(), 2);

    let named = |name: &str| found.installed.iter().find(|one| one.name == name).unwrap();

    assert_eq!(named("seq").version, "2.0.1");
    assert_eq!(
        named("seq").source.as_deref(),
        Some("nimaltd/seq"),
        "an update knows where it came from"
    );
    assert_eq!(named("seq").origin, "nimaltd/seq");
    assert_eq!(
        named("mine").source,
        None,
        "it does not say where it came from"
    );

    let source = library(
        root.path(),
        "seq",
        &format!("version: 3.0.0\n{DEMO_CODE}"),
        &[],
    );
    let looked = looked_at(&project, &source);

    assert_eq!(looked.installed.as_deref(), Some("2.0.1"));
    assert_eq!(looked.folder, "seq");
    assert_eq!(listing(&project), before, "looking moves nothing over");

    let preview = look::preview(offline(), &request(&project, &[&source]));

    assert_eq!(preview.code, 0, "{:?}", preview.errors);
    assert!(
        !preview
            .changes
            .iter()
            .any(|change| change.path.ends_with(".json")),
        "a record moving over is not a file that goes: {:?}",
        preview
            .changes
            .iter()
            .map(|change| &change.path)
            .collect::<Vec<_>>()
    );
    assert_eq!(listing(&project), before, "a preview moves nothing over");
}

#[test]
fn a_folder_of_nothing_is_only_a_folder() {
    let root = tempfile::tempdir().unwrap();
    let found = look::project(root.path(), None);

    assert_eq!(found.platform, None);
    assert!(found.candidates.is_empty() && found.installed.is_empty());
}

#[test]
fn an_arduino_project_has_what_the_sketchbook_has() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Arduino");
    let sketch = book.join("blink");
    let source = library_of(root.path(), "demo", "arduino", "");

    write(
        sketch.join("blink.ino"),
        "void setup() { }\nvoid loop() { }\n",
    );

    let mut asked = request(&sketch, &[&source]);

    asked.args.sketchbook = Some(book.to_string_lossy().into_owned());
    asked.args.into = Some("libraries".to_string());
    assert_eq!(run_with(offline(), &asked).0, 0);

    let found = look::project(&sketch, Some(&book.to_string_lossy()));

    assert_eq!(found.platform, Some(Platform::Arduino));
    assert!(found.sketch);
    assert_eq!(found.installed[0].name, "demo");
}

// ----------------------------------------------------------------------------
// A library looked at
// ----------------------------------------------------------------------------

#[test]
fn a_library_that_fits_is_ok_and_nothing_is_written() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!("description: The demo\nlicense: MIT\nversion: 1.0.0\n{DEMO_CODE}"),
        &[],
    );
    let before = listing(&project);
    let found = looked_at(&project, &source);

    assert_eq!(found.mark, Mark::Ok, "{found:?}");
    assert_eq!(found.name, "demo");
    assert_eq!(found.version, "1.0.0");
    assert_eq!(found.description, "The demo");
    assert_eq!(found.license, "MIT");
    assert_eq!(found.platform, Some(Platform::Stm32));
    assert_eq!(found.folder, "demo");
    assert!(found.manifest.as_ref().unwrap().text.contains("name: demo"));
    assert_eq!(listing(&project), before, "looking wrote something");
}

#[test]
fn something_to_do_is_a_check_with_the_way_to_do_it() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());

    write(project.join("Proj.ioc"), "Mcu.IP0=NVIC\n");

    let source = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}requires:\n  peripherals:\n    - type: spi\n      note: Master mode\n"
        ),
        &[],
    );
    let found = looked_at(&project, &source);
    let warned = found
        .findings
        .iter()
        .find(|one| one.level == Level::Warn)
        .unwrap();

    assert_eq!(found.mark, Mark::Check);
    assert_eq!(found.why, warned.message);
    assert!(warned.hint.contains("Master mode"), "{warned:?}");
}

#[test]
fn a_library_for_another_platform_is_no_with_the_reason() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());

    write(project.join("Proj.ioc"), "");

    let found = looked_at(
        &project,
        &library_for("zephyr", root.path(), "demo", DEMO_CODE, &[]),
    );

    assert_eq!(found.mark, Mark::No);
    assert!(
        found.why.contains("this looks like a STM32 project"),
        "{}",
        found.why
    );
    assert!(!found.needs_token);
}

#[test]
fn what_the_install_would_refuse_is_no_with_the_reason() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());

    // A copy of the user's own where the library's file would go, in the folder it offers.
    write(project.join("demo/src/demo.c"), "/* mine */\n");

    let found = looked_at(&project, &library(root.path(), "demo", DEMO_CODE, &[]));

    assert_eq!(found.mark, Mark::No);
    assert!(found.why.contains("a file of yours"), "{}", found.why);
    assert_eq!(found.name, "demo");
    assert_eq!(read(project.join("demo/src/demo.c")), "/* mine */\n");

    // A git repository in the project, which would lose its .git where it stands.
    let clone = library(
        &project,
        "clone",
        DEMO_CODE,
        &[(".git/HEAD", "ref: refs/heads/main\n")],
    );
    let found = looked_at(&project, &clone);

    assert_eq!(found.mark, Mark::No);
    assert!(found.why.contains("is a git repository"), "{}", found.why);
    assert!(clone.join(".git/HEAD").is_file());
}

#[test]
fn a_file_that_cannot_be_read_is_no_with_the_reason() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let found = looked_at(
        &project,
        &library(root.path(), "demo", "headers: [missing.h]\n", &[]),
    );

    assert_eq!(found.mark, Mark::No);
    assert!(found.why.contains("do not exist"), "{}", found.why);
}

#[test]
fn a_path_that_is_not_there_is_no_with_the_reason() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let found = look::look_at(
        offline(),
        &project,
        None,
        &PlatformArgs::default(),
        "./no-such-library-9f3",
    );

    assert_eq!(found.mark, Mark::No);
    assert!(found.why.contains("does not exist"), "{}", found.why);
}

#[test]
fn a_repository_that_cannot_be_seen_may_want_a_token() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let found = look::look_at(
        offline(),
        &project,
        Some(Platform::Stm32),
        &PlatformArgs::default(),
        "someone/hidden",
    );

    assert_eq!(found.mark, Mark::No);
    assert!(found.needs_token, "{}", found.why);
}

#[test]
fn what_it_offers_and_needs_is_listed() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let here = library(
        &root.path().join("v1"),
        "osal",
        &format!("version: 1.0.0\n{DEMO_CODE}"),
        &[],
    );

    installed(&project, &here, "osal");

    let body = format!(
        "{DEMO_CODE}requires:\n  libraries:\n    - osal >= 1.0.0\n    - ee24\noptions:\n  lfs:\n    description: LittleFS\n    libraries: [fsx]\n    headers: [lfs.h]\nrun:\n  - label: key\n    description: Make a key\n    command: [imgtool, keygen, '{{library}}/key.pem']\n"
    );
    let source = library(root.path(), "demo", &body, &[("lfs.h", "")]);
    let found = looked_at(&project, &source);

    assert_eq!(found.options[0].name, "lfs");
    assert_eq!(found.options[0].description, "LittleFS");
    assert!(!found.options[0].on);
    assert_eq!(found.options[0].needs, ["fsx"]);
    assert_eq!(found.steps[0].label, "key");
    assert!(
        found.steps[0]
            .command
            .contains("imgtool keygen demo/key.pem"),
        "{}",
        found.steps[0].command
    );
    assert_eq!(found.needs[0].name, "osal");
    assert_eq!(found.needs[0].state, NeedState::Here);
    assert_eq!(found.needs[0].have, "1.0.0");
    assert_eq!(found.needs[1].name, "ee24");
    assert_eq!(found.needs[1].state, NeedState::Get);
}

#[test]
fn a_library_too_old_is_to_be_updated() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let here = library(
        &root.path().join("v1"),
        "osal",
        &format!("version: 1.0.0\n{DEMO_CODE}"),
        &[],
    );

    installed(&project, &here, "osal");

    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}requires:\n  libraries:\n    - osal >= 2.0.0\n"),
        &[],
    );
    let found = looked_at(&project, &source);

    assert_eq!(found.needs[0].state, NeedState::Update);
}

#[test]
fn a_library_already_in_says_where_and_which_version() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!(
            "version: 1.0.0\n{DEMO_CODE}options:\n  x:\n    description: more\n    headers: [e.h]\n"
        ),
        &[("e.h", "")],
    );
    let mut asked = request(&project, &[&source]);

    asked.folder = Some("Libs/demo".to_string());
    asked.with_options = vec!["x".to_string()];
    assert_eq!(run_with(offline(), &asked).0, 0);

    let found = looked_at(&project, &source);

    assert_eq!(found.installed.as_deref(), Some("1.0.0"));
    assert_eq!(found.folder, "Libs/demo");
    assert!(found.options[0].on);
}

#[test]
fn a_folder_of_the_project_becomes_the_library_where_it_stands() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(&project, "demo", DEMO_CODE, &[]);
    let found = looked_at(&project, &source);

    assert!(found.in_place);
    assert_eq!(found.mark, Mark::Ok);
    assert!(
        source.join("libnima.yml").is_file(),
        "looking took its file away"
    );
}

#[test]
fn a_look_does_not_move_a_project_over() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);

    installed(&project, &source, "demo");
    fs::rename(
        project.join(".libnima.json"),
        project.join(".lib-installer.json"),
    )
    .unwrap();

    let found = looked_at(&project, &source);

    assert_eq!(found.mark, Mark::Ok, "{found:?}");
    assert!(project.join(".lib-installer.json").is_file());
    assert!(!project.join(".libnima.json").exists());
}

// ----------------------------------------------------------------------------
// The folder a window names for each library
// ----------------------------------------------------------------------------

#[test]
fn each_library_goes_into_the_folder_it_was_given() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let one = version(root.path(), "a", "one", DEMO_CODE, &[]);
    let two = version(root.path(), "b", "two", DEMO_CODE, &[]);
    let mut asked = request(&project, &[&one, &two]);

    asked.folders = vec![
        ("one".to_string(), "Libs/one".to_string()),
        ("two".to_string(), "Middle/two".to_string()),
    ];

    let (code, ui) = run_with(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("Libs/one/inc/demo.h").is_file());
    assert!(project.join("Middle/two/inc/demo.h").is_file());
    assert!(ui.asked.is_empty(), "{:?}", ui.asked);
}

// ----------------------------------------------------------------------------
// A preview
// ----------------------------------------------------------------------------

#[test]
fn a_preview_says_what_an_install_would_change_and_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let before = listing(&project);
    let cmake = read(project.join("CMakeLists.txt"));
    let found = look::preview(offline(), &request(&project, &[&source]));

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert_eq!(listing(&project), before, "the preview touched the project");
    assert_eq!(read(project.join("CMakeLists.txt")), cmake);

    let edited = found
        .changes
        .iter()
        .find(|change| change.path == "CMakeLists.txt")
        .expect("no CMakeLists.txt");
    let added = found
        .changes
        .iter()
        .find(|change| change.path == "demo/inc/demo.h")
        .expect("no header");

    assert_eq!(edited.kind, ChangeKind::Changed);
    assert_eq!(edited.before.as_deref(), Some(cmake.as_str()));
    assert!(
        edited
            .after
            .as_deref()
            .unwrap()
            .contains("add_subdirectory(demo)")
    );
    assert_eq!(added.kind, ChangeKind::Added);
    assert_eq!(added.before, None);
    assert!(
        !found
            .changes
            .iter()
            .any(|change| change.path.ends_with(".bak") || change.path.ends_with(".libnima.json"))
    );
}

#[test]
fn a_preview_is_what_the_install_then_does() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            cmake: true,
            cubeide: true,
            keil: true,
            iar: true,
            makefile: true,
            ..Ides::default()
        },
    );
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}defines:\n  - DEMO_FAST\n"),
        &[],
    );
    let found = look::preview(offline(), &request(&project, &[&source]));

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert_eq!(run_with(offline(), &request(&project, &[&source])).0, 0);

    for change in &found.changes {
        assert_eq!(
            Some(read(project.join(&change.path))),
            change.after,
            "{}",
            change.path
        );
    }

    assert!(
        found.changes.len() > 5,
        "{:?}",
        found
            .changes
            .iter()
            .map(|change| &change.path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_preview_of_an_update_says_what_goes() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let old = version(
        root.path(),
        "v1",
        "demo",
        &format!("{DEMO_CODE}extras:\n  - extra.md\n"),
        &[("extra.md", "x\n")],
    );
    let new = version(root.path(), "v2", "demo", DEMO_CODE, &[]);

    installed(&project, &old, "demo");

    let found = look::preview(offline(), &request(&project, &[&new]));
    let gone = found
        .changes
        .iter()
        .find(|change| change.path == "demo/extra.md")
        .expect("nothing was said of it");

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert_eq!(gone.kind, ChangeKind::Removed);
    assert!(project.join("demo/extra.md").is_file());
}

#[test]
fn a_preview_runs_no_step() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let make = if cfg!(windows) {
        "[cmd, /c, \"echo x> made.txt\"]"
    } else {
        "[sh, -c, \"echo x > made.txt\"]"
    };
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}run:\n  - label: s\n    command: {make}\n"),
        &[],
    );
    let mut asked = request(&project, &[&source]);

    asked.run_on = vec!["s".to_string()];

    let found = look::preview(offline(), &asked);

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert!(
        !found.changes.iter().any(|change| change.path == "made.txt"),
        "{:?}",
        found.changes
    );
}

#[test]
fn a_preview_that_cannot_go_in_says_why() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let found = look::preview(
        offline(),
        &request(
            &project,
            &[&library(root.path(), "demo", "headers: [missing.h]\n", &[])],
        ),
    );

    assert_eq!(found.code, 2);
    assert!(found.changes.is_empty());
    assert!(
        found.errors.join("\n").contains("do not exist"),
        "{:?}",
        found.errors
    );
}

#[test]
fn a_preview_leaves_out_what_no_install_reads() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());

    write(project.join("build/huge.bin"), "built");
    write(project.join("Drivers/CMSIS/x.h"), "hal");

    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let found = look::preview(offline(), &request(&project, &[&source]));

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert!(
        !found
            .changes
            .iter()
            .any(|change| change.path.starts_with("build") || change.path.starts_with("Drivers"))
    );
}

#[test]
fn a_preview_of_arduino_tries_the_sketchbook_too() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("Arduino");
    let source = library_of(root.path(), "demo", "arduino", "");
    let here = root.path().join("here");

    fs::create_dir_all(&here).unwrap();

    let mut asked = request(&here, &[&source]);

    asked.platform = Some(Platform::Arduino);
    asked.args.sketchbook = Some(book.to_string_lossy().into_owned());

    let found = look::preview(offline(), &asked);

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert!(
        found
            .changes
            .iter()
            .any(|change| change.path == "libraries/demo/library.properties"),
        "{:?}",
        found.changes
    );
    assert!(!book.exists(), "the preview made the sketchbook");
}

#[test]
fn a_preview_of_a_folder_of_the_project_leaves_it_where_it_is() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(&project, "demo", DEMO_CODE, &[]);
    let found = look::preview(
        offline(),
        &Request {
            libraries: vec![source.to_string_lossy().into_owned()],
            project: Some(project.clone()),
            ..Request::default()
        },
    );

    assert_eq!(found.code, 0, "{:?}", found.errors);
    assert!(source.join("libnima.yml").is_file());
    assert!(!source.join("CMakeLists.txt").exists());
    // On the copy, the folder became the library where it stands, and kept its libnima.yml.
    assert!(
        found
            .changes
            .iter()
            .any(|change| change.path == "demo/CMakeLists.txt" && change.kind == ChangeKind::Added),
        "{:?}",
        found.changes
    );
    assert!(
        !found
            .changes
            .iter()
            .any(|change| change.path == "demo/libnima.yml"),
        "{:?}",
        found.changes
    );
}

#[test]
fn a_preview_of_a_git_repository_in_the_project_is_refused_as_the_install_is() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(
        &project,
        "demo",
        DEMO_CODE,
        &[(".git/HEAD", "ref: refs/heads/main\n")],
    );
    let before = listing(&project);
    let found = look::preview(
        offline(),
        &Request {
            libraries: vec![source.to_string_lossy().into_owned()],
            project: Some(project.clone()),
            ..Request::default()
        },
    );

    assert_eq!(found.code, 2, "{:?}", found.said);
    assert!(
        found
            .errors
            .iter()
            .any(|error| error.contains("demo is a git repository")),
        "{:?}",
        found.errors
    );
    assert!(found.changes.is_empty(), "{:?}", found.changes);
    assert_eq!(listing(&project), before);
}

// ----------------------------------------------------------------------------
// The editor's files
// ----------------------------------------------------------------------------

#[test]
fn a_folder_with_a_file_gives_it_as_it_is() {
    let root = tempfile::tempdir().unwrap();
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let (path, text, converted) = look::file_for_editing(&source).unwrap();

    assert_eq!(path, source.join("libnima.yml"));
    assert!(text.contains("name: demo"));
    assert!(!converted);
}

#[test]
fn a_folder_with_only_an_installer_yml_gives_it_converted_to_save_as_the_new_name() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("old");

    put(
        &folder,
        &[(
            "installer.yml",
            "name: demo\nplatform: stm32\nrequires:\n  lib_installer: 4.0.0\nfiles:\n  headers:\n    - a.h\n",
        )],
    );

    let (path, text, converted) = look::file_for_editing(&folder).unwrap();

    assert_eq!(path, folder.join("libnima.yml"));
    assert!(converted);
    assert!(text.contains("headers:\n  - a.h\n"), "{text}");
    assert!(!folder.join("libnima.yml").exists(), "looking wrote it");
}

#[test]
fn a_folder_with_neither_says_to_make_one() {
    let root = tempfile::tempdir().unwrap();

    assert!(
        look::file_for_editing(root.path())
            .unwrap_err()
            .contains("Make one with New")
    );
}

#[test]
fn a_new_file_starts_as_one_that_reads() {
    let root = tempfile::tempdir().unwrap();

    for platform in Platform::ALL {
        let folder = root.path().join(platform.name()).join("demo");

        put(&folder, &[("src/demo.h", ""), ("src/demo.c", "")]);

        let found = libnima_core::scaffold::scan(&folder, Some(platform));
        let text = libnima_core::scaffold::first_file(&found, libnima_core::VERSION);
        let problems = libnima_core::check::lint_text(&folder, &folder.join("libnima.yml"), &text);

        assert!(problems.is_empty(), "{}: {problems:?}", platform.name());
    }
}

// ----------------------------------------------------------------------------
// Newer versions
// ----------------------------------------------------------------------------

#[test]
fn a_newer_version_on_github_is_found_from_its_first_header() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let repo = root.path().join("github");

    put(
        &repo,
        &[
            (
                "libnima.yml",
                "name: osal\nplatform: stm32\nheaders:\n  - inc/osal.h\n",
            ),
            ("inc/osal.h", "// @version 1.2.0\n"),
        ],
    );

    let server = Github::serving(&[("someone/osal", &repo)]);
    let mut client = server.client();
    let mut asked = Request {
        libraries: vec!["someone/osal".to_string()],
        project: Some(project.clone()),
        ..Request::default()
    };

    asked.folder = Some("osal".to_string());
    assert_eq!(run_with(server.client(), &asked).0, 0);

    assert!(look::newer_versions(&mut client, &look::project(&project, None).installed).is_empty());

    fs::write(repo.join("inc/osal.h"), "// @version 1.3.0\n").unwrap();

    let mut fresh = server.client();
    let found = look::newer_versions(&mut fresh, &look::project(&project, None).installed);

    assert_eq!(found, [("osal".to_string(), "1.3.0".to_string())]);
}

#[test]
fn a_library_from_a_folder_has_no_newer_version_to_ask_about() {
    let root = tempfile::tempdir().unwrap();
    let project = with_cmake(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let server = Github::serving(&[]);

    installed(&project, &source, "demo");

    let found = look::newer_versions(
        &mut server.client(),
        &look::project(&project, None).installed,
    );

    assert!(found.is_empty());
    assert!(server.seen().is_empty(), "{:?}", server.seen());
}
