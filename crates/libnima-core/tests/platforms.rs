//! Which platform a run is: from the library's libnima.yml, --platform, the project's record, or what the folder looks
//! like.

mod common;

use std::fs;
use std::io::Write as _;
use std::path::Path;

use common::*;
use libnima_core::flow::Request;
use libnima_core::platform::Platform;

fn run(asked: &Request) -> (i32, Watched) {
    run_with(Github::serving(&[]).client(), asked)
}

fn kept(project: &Path) -> serde_json::Value {
    serde_json::from_str(&read(project.join(".libnima.json"))).unwrap()
}

fn names_in(project: &Path) -> Vec<String> {
    let mut names: Vec<String> = kept(project)["libraries"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();

    names.sort();

    names
}

#[test]
fn every_platform_has_a_name_and_a_label_and_is_found_by_its_name() {
    for platform in Platform::ALL {
        assert_eq!(Platform::from_name(platform.name()), Some(platform));
        assert_eq!(
            platform.old_installer(),
            format!("{}-installer", platform.name())
        );
        assert!(!platform.label().is_empty());
    }
}

#[test]
fn each_platform_installs_what_names_it() {
    for platform in ["stm32", "zephyr", "esp32", "pio"] {
        let root = tempfile::tempdir().unwrap();
        let project = project_of(root.path(), platform, &[], false);
        let source = library_of(root.path(), "demo", platform, "");
        let (code, ui) = run(&request(&project, &[&source]));

        assert_eq!(code, 0, "{platform}: {}", ui.err());
        assert_eq!(kept(&project)["platform"], platform);
    }
}

#[test]
fn a_library_for_arduino_goes_into_the_libraries_folder() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "arduino", &[], false);
    let book = root.path().join("Arduino");
    let source = library_of(root.path(), "demo", "arduino", "");
    let mut asked = request(&project, &[&source]);

    asked.args.into = Some("libraries".to_string());
    asked.args.sketchbook = Some(book.to_string_lossy().into_owned());

    let (code, ui) = run(&asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(kept(&book.join("libraries"))["platform"], "arduino");
    assert!(book.join("libraries/demo/library.properties").is_file());
}

#[test]
fn the_markers_name_libnima() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let source = library_of(root.path(), "demo", "zephyr", "");

    assert_eq!(run(&request(&project, &[&source])).0, 0);
    assert!(read(project.join("CMakeLists.txt")).contains("# >>> libnima: demo >>>"));
}

#[test]
fn a_library_for_several_platforms_with_nobody_to_ask_needs_platform() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "stm32", &[], false);
    let source = library_of(root.path(), "demo", "[stm32, zephyr]", "");
    let (code, ui) = run(&request(&project, &[&source]));

    assert_eq!(code, 2);
    assert!(ui.err().contains("--platform"), "{}", ui.err());
}

#[test]
fn a_library_for_several_platforms_takes_the_records() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "stm32", &[], false);
    let first = library_of(root.path(), "first", "zephyr", "");
    let second = library_of(root.path(), "demo", "[stm32, zephyr]", "");
    let mut asked = request(&project, &[&first]);

    asked.platform = Some(Platform::Zephyr);
    asked.folder = Some("modules/first".to_string());
    assert_eq!(run(&asked).0, 0);

    let (code, ui) = run(&request(&project, &[&second]));

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        ui.asked.iter().all(|prompt| !prompt.contains("Platform")),
        "{:?}",
        ui.asked
    );
    assert_eq!(names_in(&project), ["demo", "first"]);
}

#[test]
fn the_record_and_platform_must_agree() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let first = library_of(root.path(), "first", "zephyr", "");
    let second = library_of(root.path(), "demo", "[pio, zephyr]", "");

    assert_eq!(run(&request(&project, &[&first])).0, 0);

    let mut asked = request(&project, &[&second]);

    asked.platform = Some(Platform::Pio);

    let (code, ui) = run(&asked);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("This project's record says zephyr, and --platform gives pio."),
        "{}",
        ui.err()
    );
}

#[test]
fn two_libraries_take_the_platform_they_share() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "stm32", &[], false);
    let one = library_of(root.path(), "one", "[stm32, zephyr]", "");
    let two = library_of(root.path(), "two", "zephyr", "");
    let mut asked = request(&project, &[&one, &two]);

    // STM32 by its look alone, and the two say otherwise: the libraries are not asked about what they agree on.
    asked.platform = Some(Platform::Zephyr);

    let (code, ui) = run(&asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        ui.asked.iter().all(|prompt| !prompt.contains("Platform")),
        "{:?}",
        ui.asked
    );
    assert_eq!(names_in(&project), ["one", "two"]);
}

#[test]
fn libraries_of_different_platforms_are_refused_together() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "stm32", &[], false);
    let one = library_of(root.path(), "one", "stm32", "");
    let two = library_of(root.path(), "two", "zephyr", "");
    let (code, ui) = run(&request(&project, &[&one, &two]));

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("different platforms: one for stm32; two for zephyr"),
        "{}",
        ui.err()
    );
}

#[test]
fn an_option_of_another_platform_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let source = library_of(root.path(), "demo", "zephyr", "");
    let mut asked = request(&project, &[&source]);

    asked.args.ide = Some("cmake".to_string());

    let (code, ui) = run(&asked);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("--ide is for STM32 projects, and this one is for Zephyr."),
        "{}",
        ui.err()
    );
    assert!(!project.join(".libnima.json").exists());
}

#[test]
fn an_option_of_this_platform_is_taken() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(
        root.path(),
        "zephyr",
        &[("debug.conf", "CONFIG_DEBUG=y\n")],
        false,
    );
    let source = library_of(root.path(), "demo", "zephyr", "");
    let mut asked = request(&project, &[&source]);

    asked.args.conf = Some("debug.conf".to_string());

    assert_eq!(run(&asked).0, 0);
    assert!(read(project.join("debug.conf")).contains("libnima: demo"));
    assert!(!read(project.join("prj.conf")).contains("libnima"));
}

#[test]
fn an_uninstall_takes_the_platform_from_the_record() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let source = library_of(root.path(), "demo", "zephyr", "");

    assert_eq!(run(&request(&project, &[&source])).0, 0);

    let (code, ui) = run(&Request {
        uninstall: vec!["demo".to_string()],
        project: Some(project.clone()),
        ..Request::default()
    });

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!read(project.join("CMakeLists.txt")).contains("libnima"));
    assert!(names_in(&project).is_empty());
}

#[test]
fn a_platform_that_is_none_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let source = library_of(root.path(), "demo", "avr", "");
    let (code, ui) = run(&request(&project, &[&source]));

    assert_eq!(code, 2);
    assert!(ui.err().contains("platform has 'avr'"), "{}", ui.err());
}

#[test]
fn a_platform_named_in_capitals_is_read() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let source = library_of(root.path(), "demo", "Zephyr", "");
    let (code, ui) = run(&request(&project, &[&source]));

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(kept(&project)["platform"], "zephyr");
}

#[test]
fn a_folder_with_no_manifest_is_said() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let empty = root.path().join("empty");

    fs::create_dir_all(&empty).unwrap();

    let (code, ui) = run(&request(&project, &[&empty]));

    assert_eq!(code, 2);
    assert!(ui.err().contains("holds no libnima.yml"), "{}", ui.err());
}

#[test]
fn a_message_naming_the_command_names_platform_when_it_was_given() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let mut asked = Request {
        project: Some(project),
        ..Request::default()
    };

    let (code, ui) = run(&asked);

    assert_eq!(code, 2);
    assert!(ui.err().contains("libnima owner/example"), "{}", ui.err());

    asked.platform = Some(Platform::Zephyr);

    let (code, ui) = run(&asked);

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("libnima --platform zephyr owner/example"),
        "{}",
        ui.err()
    );
}

// ----------------------------------------------------------------------------
// The manifest, read first
// ----------------------------------------------------------------------------

const ON_GITHUB: &str =
    "name: demo\nplatform: zephyr\nheaders:\n  - src/demo.h\nsources:\n  - src/demo.c\n";

fn on_github(root: &Path, text: &str) -> std::path::PathBuf {
    let folder = root.join("github");

    put(
        &folder,
        &[
            ("libnima.yml", text),
            ("src/demo.h", "int demo(void);\n"),
            ("src/demo.c", "int demo(void);\n"),
        ],
    );

    folder
}

fn named(project: &Path, source: &str) -> Request {
    Request {
        libraries: vec![source.to_string()],
        project: Some(project.to_path_buf()),
        ..Request::default()
    }
}

#[test]
fn the_manifest_on_github_is_asked_for_once() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let folder = on_github(root.path(), ON_GITHUB);
    let server = Github::serving(&[("someone/demo", &folder)]);

    server
        .holding()
        .commits
        .insert("HEAD".to_string(), "a".repeat(40));

    let (code, ui) = run_with(server.client(), &named(&project, "someone/demo"));
    let seen = server.seen();

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(kept(&project)["platform"], "zephyr");
    assert_eq!(
        seen.iter()
            .filter(|one| one.file() == "libnima.yml")
            .count(),
        1
    );
    assert_eq!(server.holding().lookups.len(), 1);
}

#[test]
fn a_manifest_on_github_naming_no_platform_stops_before_any_file() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let folder = on_github(root.path(), &ON_GITHUB.replace("platform: zephyr\n", ""));
    let server = Github::serving(&[("someone/demo", &folder)]);
    let (code, ui) = run_with(server.client(), &named(&project, "someone/demo"));

    assert_eq!(code, 2);
    assert!(ui.err().contains("names no platform"), "{}", ui.err());
    assert!(
        server.seen().iter().all(|one| one.file() == "libnima.yml"),
        "{:?}",
        server.seen()
    );
}

#[test]
fn a_library_needed_that_is_for_another_platform_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = project_of(root.path(), "zephyr", &[], false);
    let folder = on_github(
        root.path(),
        &ON_GITHUB
            .replace("name: demo", "name: dep")
            .replace("platform: zephyr", "platform: stm32"),
    );
    let needer = library_of(
        root.path(),
        "needer",
        "zephyr",
        "requires:\n  libraries:\n    - someone/dep\n",
    );
    let server = Github::serving(&[("someone/dep", &folder)]);
    let (code, ui) = run_with(server.client(), &request(&project, &[&needer]));

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("dep works on stm32, as platform in its libnima.yml says, not on zephyr"),
        "{}",
        ui.err()
    );
    assert!(!project.join(".libnima.json").exists());
}

fn zip_of(path: &Path, top: &str, manifest: &str) {
    let mut archive = zip::ZipWriter::new(fs::File::create(path).unwrap());

    for (name, text) in [
        ("libnima.yml", manifest),
        ("src/demo.h", "int demo(void);\n"),
        ("src/demo.c", "int demo(void) { return 1; }\n"),
    ] {
        archive
            .start_file(
                format!("{top}{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.write_all(text.as_bytes()).unwrap();
    }

    archive.finish().unwrap();
}

#[test]
fn the_platform_is_read_from_a_zip_with_a_top_folder_or_without() {
    for (name, top) in [("demo-main.zip", "demo-main/"), ("demo.zip", "")] {
        let root = tempfile::tempdir().unwrap();
        let project = project_of(root.path(), "zephyr", &[], false);
        let archive = root.path().join(name);

        zip_of(&archive, top, ON_GITHUB);

        let (code, ui) = run(&request(&project, &[&archive]));

        assert_eq!(code, 0, "{name}: {}", ui.err());
        assert_eq!(kept(&project)["platform"], "zephyr");
    }
}
