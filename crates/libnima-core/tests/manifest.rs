//! What a libnima.yml may say, and what it is read as.
//!
//! A manifest arrives over the network before any of it is trusted, so no file it names may be read from outside the
//! library, however it is spelt, and none it writes may land outside the place it is for.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use libnima_core::check;
use libnima_core::manifest::{self, Manifest, ManifestError};
use libnima_core::platform::Platform;

const CODE: &str = "headers:\n  - inc/demo.h\nsources:\n  - src/demo.c\n";

fn folder(root: &Path, body: &str, files: &[(&str, &str)]) -> PathBuf {
    library(root, "demo", body, files)
}

fn read(root: &Path) -> Result<Manifest, ManifestError> {
    manifest::load(root, Some(Platform::Stm32))
}

/// What a body and its files are read as.
fn loaded(body: &str, files: &[(&str, &str)]) -> Manifest {
    let root = tempfile::tempdir().unwrap();

    read(&folder(root.path(), body, files)).unwrap()
}

/// Why a body and its files are refused.
fn refused(body: &str, files: &[(&str, &str)]) -> String {
    let root = tempfile::tempdir().unwrap();

    read(&folder(root.path(), body, files)).unwrap_err().message
}

fn destinations(entries: &[manifest::FileEntry]) -> Vec<&str> {
    entries
        .iter()
        .map(|entry| entry.destination.as_str())
        .collect()
}

// ----------------------------------------------------------------------------
// What every library must have
// ----------------------------------------------------------------------------

#[test]
fn a_manifest_promising_absent_files_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let lib = folder(root.path(), CODE, &[]);
    fs::remove_file(lib.join("src/demo.c")).unwrap();

    assert!(read(&lib).unwrap_err().message.contains("do not exist"));
}

#[test]
fn a_manifest_with_no_code_is_refused() {
    let missing = refused("kind: driver\n", &[]);
    let empty = refused("headers: []\nsources: []\n", &[]);

    assert!(
        missing.contains("It is missing its headers and its sources"),
        "{missing}"
    );
    assert!(empty.contains("nothing to install"), "{empty}");
}

#[test]
fn broken_yaml_is_reported_as_yaml() {
    let root = tempfile::tempdir().unwrap();
    let lib = folder(root.path(), CODE, &[]);
    fs::write(lib.join("libnima.yml"), "name: demo\n  bad indent: [").unwrap();

    let message = read(&lib).unwrap_err().message;

    assert!(message.contains("not valid YAML"), "{message}");
}

#[test]
fn a_key_given_twice_is_refused_when_the_manifest_is_read() {
    let message = refused(&format!("kind: driver\n{CODE}kind: rtos\n"), &[]);

    assert!(message.contains("'kind' is given twice"), "{message}");
}

#[test]
fn an_unknown_kind_is_a_warning_in_a_run_and_something_to_check_in_a_look_over() {
    let found = loaded(&format!("version: 1.0.0\nkind: nonsense\n{CODE}"), &[]);

    assert!(
        found.warnings[0].contains("nonsense"),
        "{:?}",
        found.warnings
    );

    let root = tempfile::tempdir().unwrap();
    let lib = folder(root.path(), &format!("kind: nonsense\n{CODE}"), &[]);
    let (_, problems) = check::lint(&lib).unwrap();

    assert!(
        problems.iter().all(|problem| problem.warning),
        "{problems:?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.message.contains("nonsense")),
        "{problems:?}"
    );
}

#[test]
fn an_rtos_library_provides_rtos_without_saying_so() {
    assert!(
        loaded(&format!("kind: rtos\n{CODE}"), &[])
            .provides
            .contains(&"rtos".to_string())
    );
}

// ----------------------------------------------------------------------------
// Where each file lands
// ----------------------------------------------------------------------------

#[test]
fn the_code_keeps_its_path() {
    let found = loaded(CODE, &[]);

    assert_eq!(destinations(&found.headers), ["inc/demo.h"]);
    assert_eq!(destinations(&found.sources), ["src/demo.c"]);
    assert_eq!(found.include_dirs(), ["inc"]);
}

#[test]
fn a_file_can_name_its_own_place() {
    let found = loaded("headers:\n  - from: inc/demo.h\n    to: api/demo.h\n", &[]);

    assert_eq!(destinations(&found.headers), ["api/demo.h"]);
    assert_eq!(found.include_dirs(), ["api"]);
}

#[test]
fn a_header_needs_no_from_when_it_is_its_own_place() {
    let found = loaded("headers:\n  - to: inc/demo.h\n", &[]);

    assert_eq!(found.headers[0].source, "inc/demo.h");
    assert_eq!(found.headers[0].destination, "inc/demo.h");
}

#[test]
fn include_dirs_can_be_stated_outright() {
    let found = loaded(
        &format!("{CODE}include_dirs:\n  - inc\n  - inc/port\n"),
        &[],
    );

    assert_eq!(found.include_dirs(), ["inc", "inc/port"]);
}

#[test]
fn a_plain_include_folder_is_taken_as_it_is() {
    let found = loaded(&format!("{CODE}include_dirs: [src, 'inc\\port', .]\n"), &[]);

    assert_eq!(found.include_dirs(), ["src", "inc/port", "."]);
}

#[test]
fn an_include_folder_a_project_file_could_not_hold_is_refused() {
    for (place, says) in [
        ("'src\" onload=\"x'", "it holds a quote, <, >, &, ;"),
        ("'src;inc'", "it holds a quote, <, >, &, ;"),
        ("'src<x>'", "it holds a quote, <, >, &, ;"),
        ("../../outside", "it climbs out with '..'"),
        ("/etc", "it starts at the root of the drive"),
        ("C:/Windows", "it names a drive"),
        ("''", "it is empty"),
    ] {
        let message = refused(&format!("{CODE}include_dirs:\n  - src\n  - {place}\n"), &[]);

        assert!(message.contains(says), "{place}: {message}");
    }
}

#[test]
fn include_dirs_is_a_list() {
    let message = refused(&format!("{CODE}include_dirs: src\n"), &[]);

    assert!(
        message.contains("include_dirs is a list of folders"),
        "{message}"
    );
}

#[test]
fn a_place_escaping_the_folder_is_refused() {
    // A manifest arrives over the network, so this can never be a warning.
    for place in ["../../Core/Src/main.c", "/etc/passwd"] {
        let message = refused(
            &format!("headers:\n  - from: inc/demo.h\n    to: {place}\n"),
            &[],
        );

        assert!(
            message.contains("outside the install folder"),
            "{place}: {message}"
        );
    }
}

#[test]
fn a_file_outside_the_library_is_refused_however_it_is_spelt() {
    for path in [
        "../secret.h",
        "'..\\secret.h'",
        "/etc/passwd",
        "'\\x.h'",
        "C:/x.h",
        "inc/../../x.h",
    ] {
        let message = refused(
            &format!("headers:\n  - inc/demo.h\nsources:\n  - {path}\n"),
            &[],
        );

        assert!(
            message.contains("a file outside the library"),
            "{path}: {message}"
        );
    }
}

#[test]
fn every_kind_of_file_is_checked() {
    for body in [
        format!("{CODE}once:\n  - from: ../x.h\n"),
        format!("{CODE}extras:\n  - ../NOTICE\n"),
        format!(
            "{CODE}options:\n  fs:\n    description: x\n    sources:\n      - from: ../fs.c\n        to: fs.c\n"
        ),
    ] {
        let message = refused(&body, &[]);

        assert!(
            message.contains("a file outside the library"),
            "{body}: {message}"
        );
    }
}

#[test]
fn a_link_out_of_the_library_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let lib = folder(root.path(), CODE, &[]);
    let outside = root.path().join("elsewhere");
    put(&outside, &[("demo.c", "/* not the library's */\n")]);
    fs::remove_dir_all(lib.join("src")).unwrap();

    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&outside, lib.join("src")).is_ok();

    // Windows lets anyone make a junction, where a symbolic link needs rights a user may not have.
    #[cfg(windows)]
    let made = std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(lib.join("src"))
        .arg(&outside)
        .output()
        .is_ok_and(|done| done.status.success());

    if !made {
        return;
    }

    let message = read(&lib).unwrap_err().message;

    assert!(message.contains("leads outside the library"), "{message}");
}

#[test]
fn code_has_no_root() {
    let message = refused(
        "headers:\n  - from: inc/demo.h\n    root: inc/demo.h\n",
        &[],
    );

    assert!(
        message.contains("Code is built in the library's folder"),
        "{message}"
    );
}

// ----------------------------------------------------------------------------
// Wildcards
// ----------------------------------------------------------------------------

#[test]
fn a_wildcard_expands_to_every_match() {
    let found = loaded(
        "headers:\n  - lib/*.h\nsources:\n  - lib/*.c\n",
        &[
            ("lib/a.h", ""),
            ("lib/b.h", ""),
            ("lib/a.c", ""),
            ("lib/b.c", ""),
        ],
    );

    assert_eq!(destinations(&found.headers), ["lib/a.h", "lib/b.h"]);
    assert_eq!(found.build_sources(), ["lib/a.c", "lib/b.c"]);
}

#[test]
fn a_folder_expands_and_keeps_its_shape() {
    // Flattening a folder would collide the moment two subfolders agreed on a name.
    let found = loaded(
        &format!("{CODE}extras:\n  - docs\n"),
        &[("docs/guide.md", ""), ("docs/api/ref.md", "")],
    );

    assert_eq!(
        destinations(&found.extras),
        ["docs/api/ref.md", "docs/guide.md"]
    );
}

#[test]
fn a_wildcard_means_the_same_on_disk_and_in_the_listing() {
    let tree = [
        "top.c",
        "top.h",
        "src/a.c",
        "src/b.h",
        "src/ble/c.c",
        "src/ble/deep/d.c",
        "test/t.c",
    ];
    let known: Vec<String> = tree.iter().map(ToString::to_string).collect();
    let root = tempfile::tempdir().unwrap();

    for file in tree {
        put(root.path(), &[(file, "x")]);
    }

    for (pattern, wanted) in [
        ("*.c", vec!["top.c"]),
        ("src/*.c", vec!["src/a.c"]),
        (
            "src/**/*.c",
            vec!["src/a.c", "src/ble/c.c", "src/ble/deep/d.c"],
        ),
        (
            "**/*.c",
            vec![
                "src/a.c",
                "src/ble/c.c",
                "src/ble/deep/d.c",
                "test/t.c",
                "top.c",
            ],
        ),
        (
            "src/**",
            vec!["src/a.c", "src/b.h", "src/ble/c.c", "src/ble/deep/d.c"],
        ),
        ("src/?.[ch]", vec!["src/a.c", "src/b.h"]),
        ("src/[!a].h", vec!["src/b.h"]),
    ] {
        assert_eq!(
            manifest::expand(root.path(), pattern),
            wanted,
            "{pattern} on disk"
        );
        assert_eq!(
            manifest::expand_known(pattern, &known),
            wanted,
            "{pattern} in the listing"
        );
    }
}

#[test]
fn a_listing_finds_a_file_a_folder_or_nothing() {
    // This is the path the install from GitHub takes, with no copy on disk to look at.
    let known: Vec<String> = [
        "src/a.h",
        "src/b.h",
        "src/a.c",
        "docs/guide.md",
        "docs/api/ref.md",
    ]
    .iter()
    .map(ToString::to_string)
    .collect();

    assert_eq!(
        manifest::expand_known("src/*.h", &known),
        ["src/a.h", "src/b.h"]
    );
    assert_eq!(
        manifest::expand_known("docs", &known),
        ["docs/api/ref.md", "docs/guide.md"]
    );
    assert_eq!(manifest::expand_known("src/a.c", &known), ["src/a.c"]);
    assert!(manifest::expand_known("nothing/*.x", &known).is_empty());
}

#[test]
fn a_required_pattern_matching_nothing_is_refused() {
    let message = refused("sources:\n  - src/*.cpp\n", &[]);

    assert!(message.contains("do not exist"), "{message}");
}

#[test]
fn an_extras_pattern_matching_nothing_only_warns() {
    let found = loaded(&format!("{CODE}extras:\n  - docs/*.md\n"), &[]);

    assert!(
        found
            .warnings
            .iter()
            .any(|warning| warning.contains("matched no file")),
        "{:?}",
        found.warnings
    );
}

#[test]
fn a_wildcard_cannot_be_given_one_place() {
    let message = refused("sources:\n  - from: src/*.c\n    to: one.c\n", &[]);

    assert!(message.contains("matches many files"), "{message}");
}

#[test]
fn a_place_that_is_a_wildcard_is_refused() {
    for key in ["to", "root"] {
        let message = refused(&format!("{CODE}extras:\n  - {key}: tools/*.py\n"), &[]);

        assert!(message.contains("never a wildcard"), "{key}: {message}");
    }
}

// ----------------------------------------------------------------------------
// Two files in one place
// ----------------------------------------------------------------------------

#[test]
fn a_file_claimed_as_both_code_and_once_is_refused() {
    // A wildcard sweeping up the config header would have the user's settings overwritten on every update, while the
    // output still said they were kept.
    let message = refused(
        "headers:\n  - lib/*.h\nonce:\n  - from: lib/demo_config.h\n",
        &[("lib/demo.h", ""), ("lib/demo_config.h", "")],
    );

    assert!(message.contains("both code and configuration"), "{message}");
}

#[test]
fn kept_paths_keep_two_files_of_one_name_apart() {
    let found = loaded(
        "sources:\n  - lib/ble/util.c\n  - lib/dfu/util.c\n",
        &[("lib/ble/util.c", ""), ("lib/dfu/util.c", "")],
    );

    assert_eq!(
        destinations(&found.sources),
        ["lib/ble/util.c", "lib/dfu/util.c"]
    );
}

#[test]
fn one_file_listed_twice_is_no_clash() {
    assert!(
        !loaded("sources:\n  - src/demo.c\n  - src/*.c\n", &[])
            .sources
            .is_empty()
    );
}

#[test]
fn two_extras_landing_in_one_place_are_refused() {
    let message = refused(
        &format!(
            "{CODE}extras:\n  - from: tools/a.md\n    root: x.md\n  - from: NOTICE\n    root: x.md\n"
        ),
        &[("tools/a.md", "")],
    );

    assert!(message.contains("both land"), "{message}");
}

#[test]
fn an_extras_to_cannot_land_on_the_code() {
    let message = refused(
        &format!("{CODE}extras:\n  - from: NOTICE\n    to: src/demo.c\n"),
        &[],
    );

    assert!(
        message.contains("land at src/demo.c in the library's folder"),
        "{message}"
    );
}

// ----------------------------------------------------------------------------
// Options
// ----------------------------------------------------------------------------

#[test]
fn an_options_files_keep_their_paths_too() {
    let found = loaded(
        &format!(
            "{CODE}options:\n  extra:\n    description: More\n    sources:\n      - src/more.c\n"
        ),
        &[("src/more.c", "")],
    );

    assert_eq!(destinations(&found.options[0].sources), ["src/more.c"]);
}

// ----------------------------------------------------------------------------
// A library another needs
// ----------------------------------------------------------------------------

#[test]
fn a_carried_manifest_from_outside_the_library_is_refused() {
    let message = refused(
        &format!("{CODE}requires:\n  libraries:\n    - source: x/y\n      manifest: ../y.yml\n"),
        &[],
    );

    assert!(message.contains("a file outside the library"), "{message}");
}

#[test]
fn a_carried_manifest_must_be_in_the_library() {
    let message = refused(
        &format!(
            "{CODE}requires:\n  libraries:\n    - source: x/y\n      manifest: manifests/y.yml\n"
        ),
        &[],
    );

    assert!(message.contains("do not exist"), "{message}");
}

#[test]
fn a_dependency_that_says_nothing_usable_is_refused() {
    for text in [
        "'osal@'",
        "osal@../x",
        "osal >= 1.2",
        "a/b/c",
        "osal@v1 junk",
        "osal > 1.0.0",
        "osal >= one",
        "osal 1.0.0",
    ] {
        let message = refused(
            &format!("{CODE}requires:\n  libraries:\n    - {text}\n"),
            &[],
        );

        assert!(!message.is_empty(), "{text}");
    }
}

#[test]
fn a_dependency_keeps_its_owner_its_ref_and_its_oldest_version() {
    let found = loaded(
        &format!("{CODE}requires:\n  libraries:\n    - someone/osal@v1.2.0 >= 1.2.0\n"),
        &[],
    );
    let needed = found.dependencies();

    assert_eq!(needed[0].source, "someone/osal");
    assert_eq!(needed[0].reference.as_deref(), Some("v1.2.0"));
    assert_eq!(needed[0].name, "osal");
    assert_eq!(needed[0].minimum, Some((1, 2, 0)));
    assert_eq!(needed[0].fetch(), "someone/osal@v1.2.0");
}

#[test]
fn a_branch_with_a_slash_is_one_ref() {
    let found = loaded(
        &format!("{CODE}requires:\n  libraries:\n    - someone/lib@feature/x\n"),
        &[],
    );

    assert_eq!(
        found.dependencies()[0].reference.as_deref(),
        Some("feature/x")
    );
}

// ----------------------------------------------------------------------------
// Which libNIMA it is for
// ----------------------------------------------------------------------------

#[test]
fn a_library_for_this_libnima_loads() {
    let found = loaded(
        &format!("{CODE}requires:\n  libnima: {}\n", libnima_core::VERSION),
        &[],
    );

    assert_eq!(found.name, "demo");
}

#[test]
fn a_library_for_a_newer_libnima_is_refused_before_anything_else_is_read() {
    // A manifest for a newer libNIMA may not have the fields this one expects.
    let message = refused("requires:\n  libnima: 99.0.0\nsources_v9:\n  - x.c\n", &[]);

    assert!(
        message.contains("needs libNIMA 99.0.0 or newer"),
        "{message}"
    );
}

#[test]
fn a_two_part_version_is_refused() {
    // Unquoted, YAML reads 1.10 as a number, and the minor part is lost.
    let message = refused(&format!("{CODE}requires:\n  libnima: 1.1\n"), &[]);

    assert!(message.contains("three numbers"), "{message}");
}

#[test]
fn a_zip_for_a_newer_libnima_leaves_the_project_as_it_was() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            cmake: true,
            ..Ides::default()
        },
    );
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}requires:\n  libnima: 99.0.0\n"),
        &[],
    );
    let archive = root.path().join("demo-main.zip");
    let mut bundle = zip::ZipWriter::new(fs::File::create(&archive).unwrap());

    for path in libnima_core::paths::files_below(&source) {
        bundle
            .start_file(
                format!("demo-main/{path}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        std::io::Write::write_all(&mut bundle, &fs::read(source.join(&path)).unwrap()).unwrap();
    }

    bundle.finish().unwrap();

    let before = common::read(project.join("CMakeLists.txt"));
    let (code, ui) = install(&project, &archive, "demo");

    assert_eq!(code, 2);
    assert!(ui.err().contains("needs libNIMA 99.0.0"), "{}", ui.err());
    assert!(!project.join("demo").exists());
    assert_eq!(common::read(project.join("CMakeLists.txt")), before);
}

// ----------------------------------------------------------------------------
// The version
// ----------------------------------------------------------------------------

#[test]
fn the_header_wins_over_a_version_left_in_the_file() {
    let found = loaded(
        "version: 1.0.0\nheaders:\n  - inc/demo.h\n",
        &[("inc/demo.h", "/**\n * @version 2.3.4\n */\n")],
    );

    assert_eq!(found.version, "2.3.4");
}

#[test]
fn the_file_is_read_for_a_header_without_a_version() {
    assert_eq!(
        loaded(&format!("version: 1.0.0\n{CODE}"), &[]).version,
        "1.0.0"
    );
}

#[test]
fn no_version_anywhere_is_none() {
    assert_eq!(loaded(CODE, &[]).version, "");
}

#[test]
fn the_record_keeps_the_version_from_the_header() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "demo",
        DEMO_CODE,
        &[("inc/demo.h", "// @version 3.1.4\n")],
    );

    installed(&project, &source, "demo");

    assert_eq!(record(&project)["demo"]["version"], "3.1.4");
}

// ----------------------------------------------------------------------------
// The name of the file
// ----------------------------------------------------------------------------

const OLD: &str =
    "name: demo\nplatform: stm32\nrequires:\n  lib_installer: 4.0.0\nheaders:\n  - inc/demo.h\n";

#[test]
fn a_folder_with_only_the_old_name_still_loads() {
    let root = tempfile::tempdir().unwrap();
    let lib = folder(root.path(), CODE, &[]);
    fs::remove_file(lib.join("libnima.yml")).unwrap();
    fs::write(lib.join("installer.yml"), OLD).unwrap();

    assert_eq!(read(&lib).unwrap().name, "demo");
}

#[test]
fn the_new_name_wins_when_both_are_there() {
    let root = tempfile::tempdir().unwrap();
    let lib = folder(root.path(), &format!("version: 7.7.7\n{CODE}"), &[]);
    fs::write(
        lib.join("installer.yml"),
        OLD.replace("name: demo", "name: other"),
    )
    .unwrap();

    let found = read(&lib).unwrap();

    assert_eq!(found.name, "demo");
    assert_eq!(found.version, "7.7.7");
}

#[test]
fn a_folder_with_neither_names_the_new_one() {
    let root = tempfile::tempdir().unwrap();
    let message = read(root.path()).unwrap_err().message;

    assert!(message.contains("libnima.yml"), "{message}");
}

#[test]
fn a_zip_with_the_old_name_is_found() {
    let root = tempfile::tempdir().unwrap();
    let archive = root.path().join("demo-main.zip");
    let mut bundle = zip::ZipWriter::new(fs::File::create(&archive).unwrap());

    bundle
        .start_file(
            "demo-main/installer.yml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    std::io::Write::write_all(&mut bundle, OLD.as_bytes()).unwrap();
    bundle.finish().unwrap();

    let (bytes, name) = libnima_core::download::peek_zip(&archive).unwrap();

    assert_eq!(name, "installer.yml");
    assert_eq!(String::from_utf8(bytes).unwrap(), OLD);
}

#[test]
fn a_once_file_in_a_librarys_folder_is_refused_with_no_project() {
    // A file of the project's own, made once, cannot be one that a library's next install replaces. With no project, the
    // folders known are the one this library goes into and those of the libraries it names.
    let root = tempfile::tempdir().unwrap();
    let lib = root.path().join("libf");
    let named = "name: libf\nplatform: zephyr\nrequires:\n  libraries:\n    - liba\nheaders: [src/libf.h]\nonce:\n  - from: templates/x.h\n    to: modules/liba/src/liba.h\n";

    put(
        &lib,
        &[
            ("src/libf.h", ""),
            ("templates/x.h", ""),
            ("libnima.yml", named),
        ],
    );

    let (_, problems) = check::lint(&lib).unwrap();

    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(!problems[0].warning);
    assert!(
        problems[0]
            .message
            .contains("once puts modules/liba/src/liba.h inside modules/liba, where liba goes"),
        "{}",
        problems[0].message
    );

    fs::write(
        lib.join("libnima.yml"),
        "name: libf\nplatform: zephyr\nheaders: [src/libf.h]\nonce:\n  - from: templates/x.h\n    to: modules/libf/x.h\n",
    )
    .unwrap();

    let (_, problems) = check::lint(&lib).unwrap();

    assert!(
        problems[0]
            .message
            .contains("once puts modules/libf/x.h inside its own folder, modules/libf"),
        "{problems:?}"
    );

    // Anywhere else in the project is the project's own.
    fs::write(
        lib.join("libnima.yml"),
        "name: libf\nplatform: zephyr\nversion: 1.0.0\nheaders: [src/libf.h]\nonce:\n  - from: templates/x.h\n    to: modules/x.h\n",
    )
    .unwrap();

    let (_, problems) = check::lint(&lib).unwrap();

    assert!(problems.is_empty(), "{problems:?}");
}

#[test]
fn installing_in_place_keeps_either_name() {
    for name in ["libnima.yml", "installer.yml"] {
        let root = tempfile::tempdir().unwrap();
        let project = stm32_project(root.path());
        let lib = library(&project, "demo", DEMO_CODE, &[]);

        if name == "installer.yml" {
            fs::remove_file(lib.join("libnima.yml")).unwrap();
            fs::write(
                lib.join(name),
                OLD.replace(
                    "  - inc/demo.h\n",
                    "  - inc/demo.h\nsources:\n  - src/demo.c\n",
                ),
            )
            .unwrap();
        }

        let (code, ui) = run_with(Github::serving(&[]).client(), &request(&project, &[&lib]));

        assert_eq!(code, 0, "{name}: {}", ui.err());
        assert!(lib.join(name).is_file(), "{name} was removed");
        assert!(lib.join("inc/demo.h").is_file());
    }
}

// ----------------------------------------------------------------------------
// The page that says all this
// ----------------------------------------------------------------------------

#[test]
fn every_example_on_the_format_page_reads() {
    let page = include_str!("../../../docs/FORMAT.md");
    let root = tempfile::tempdir().unwrap();
    let mut examples = 0;

    for block in page.split("```yaml\n").skip(1) {
        let text = block.split("```").next().unwrap();
        let section = ["stm32", "zephyr", "esp32", "arduino", "pio"]
            .into_iter()
            .find(|name| text.starts_with(&format!("{name}:")));
        let whole = match section {
            Some(name) => {
                format!("name: example\nplatform: {name}\nheaders:\n  - src/example.h\n{text}")
            }
            None => text.to_string(),
        };
        let folder = root.path().join(format!("example{examples}"));

        put(
            &folder,
            &[
                ("libnima.yml", &whole),
                ("src/example.h", ""),
                ("src/example_config.h", ""),
                ("src/port/x.h", ""),
                ("src/example.c", ""),
                ("src/port/spi.c", ""),
                ("src/example_lfs.h", ""),
                ("src/example_lfs.c", ""),
                (
                    "manifests/other.yml",
                    "name: other\nplatform: stm32\nheaders:\n  - other.h\n",
                ),
                ("VERSION", "1\n"),
                ("templates/example_port.c", ""),
                ("LICENSE.md", ""),
                ("NOTICE", ""),
                ("docs/guide.md", ""),
                ("tools/make_key.py", ""),
            ],
        );

        // What a page shows on its own is a part of a file, as a platform's section is.
        if text.starts_with("name:") || section.is_some() {
            let running = section
                .and_then(Platform::from_name)
                .unwrap_or(Platform::Stm32);

            if let Err(error) = read_as(&folder, running) {
                panic!("{text}\n{error}");
            }
        }

        examples += 1;
    }

    assert!(examples >= 7, "{examples}");
}

fn read_as(folder: &Path, platform: Platform) -> Result<Manifest, String> {
    manifest::load(folder, Some(platform)).map_err(|error| error.message)
}
