//! Registering a library with each IDE an STM32 project can use, and checking a project against what a library needs.
//!
//! A project file is read by people and lands in version control, so an added line has to come out looking like the
//! lines around it. Those checks come from real project files that came back from users: the library was on the include
//! path and the build worked, but the line had been written hard against the left margin while every line around it was
//! nine tabs in.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::*;
use libnima_core::manifest::{self, Manifest};
use libnima_core::platform::Platform;
use libnima_core::platform::stm32::checks;
use libnima_core::platform::stm32::ide::{self, cmake, cubeide, iar, keil, makefile};
use libnima_core::target::{Dropped, Level, Outcome, Status};
use regex::Regex;

const FLAT: &str = "headers:\n  - demo.h\nsources:\n  - demo.c\n";
const LFS: &str = "LFS_DEFINES=lfs_defines.h";

const CASES: [(&str, &str); 4] = [
    ("cmake", "CMakeLists.txt"),
    ("cubeide", ".cproject"),
    ("keil", "MDK/Proj.uvprojx"),
    ("iar", "EWARM/Proj.ewp"),
];

const LISTED: [(&str, &str); 2] = [("keil", "MDK/Proj.uvprojx"), ("iar", "EWARM/Proj.ewp")];

fn ides(names: &[&str]) -> Ides {
    let mut chosen = Ides::default();

    for name in names {
        match *name {
            "cmake" => chosen.cmake = true,
            "cubeide" => chosen.cubeide = true,
            "keil" => chosen.keil = true,
            "iar" => chosen.iar = true,
            _ => chosen.makefile = true,
        }
    }

    chosen
}

fn crlf(names: &[&str]) -> Ides {
    Ides {
        crlf: true,
        ..ides(names)
    }
}

/// A version of the library whose code lies at its top, in a folder of its own.
fn demo(root: &Path, tag: &str, more: &str, files: &[(&str, &str)]) -> Manifest {
    let mut all = vec![("demo.h", "/* h */\n"), ("demo.c", "/* c */\n")];

    all.extend_from_slice(files);

    manifest::load(
        &version(root, tag, "demo", &format!("{FLAT}{more}"), &all),
        Some(Platform::Stm32),
    )
    .unwrap()
}

/// A version of the library whose code keeps inc and src.
fn kept_paths(root: &Path, tag: &str, more: &str, files: &[(&str, &str)]) -> Manifest {
    manifest::load(
        &version(root, tag, "demo", &format!("{DEMO_CODE}{more}"), files),
        Some(Platform::Stm32),
    )
    .unwrap()
}

fn asking(root: &Path, tag: &str, defines: &[&str]) -> Manifest {
    let listed: String = defines
        .iter()
        .map(|define| format!("  - {define}\n"))
        .collect();

    if defines.is_empty() {
        demo(root, tag, "", &[])
    } else {
        demo(root, tag, &format!("defines:\n{listed}"), &[])
    }
}

fn nothing_dropped() -> Dropped {
    Dropped {
        files: Vec::new(),
        defines: Vec::new(),
    }
}

fn dropping(project: &Path, files: &[&str]) -> Dropped {
    Dropped {
        files: files
            .iter()
            .map(|file| project.join("demo").join(file))
            .collect(),
        defines: Vec::new(),
    }
}

fn dropping_define(define: &str) -> Dropped {
    Dropped {
        files: Vec::new(),
        defines: vec![define.to_string()],
    }
}

fn where_of(backend: &str) -> &'static str {
    match backend {
        "cmake" => "CMakeLists.txt",
        "cubeide" => ".cproject",
        "keil" => "MDK/Proj.uvprojx",
        "iar" => "EWARM/Proj.ewp",
        _ => "Makefile",
    }
}

fn register_with(backend: &str, project: &Path, library: &Manifest, dropped: &Dropped) -> Outcome {
    let destination = project.join("demo");
    let path = project.join(where_of(backend));

    fs::create_dir_all(&destination).unwrap();

    match backend {
        "cmake" => cmake::integrate(&path, library, &destination, project),
        "cubeide" => cubeide::integrate(&path, library, &destination, project, dropped),
        "keil" => keil::integrate(&path, library, &destination, dropped),
        "iar" => iar::integrate(&path, library, &destination, dropped),
        _ => makefile::integrate(&path, library, &destination, dropped),
    }
    .unwrap()
}

fn register(backend: &str, project: &Path, library: &Manifest) -> Outcome {
    register_with(backend, project, library, &nothing_dropped())
}

/// The lines that appear in after and not in before, in order.
fn added_lines(before: &str, after: &str) -> Vec<String> {
    let mut counts = std::collections::HashMap::new();

    for line in before.lines() {
        *counts.entry(line).or_insert(0) += 1;
    }

    let mut added = Vec::new();

    for line in after.lines() {
        match counts.get_mut(line) {
            Some(count) if *count > 0 => *count -= 1,
            _ => added.push(line.to_string()),
        }
    }

    added
}

fn indent(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn no_bare_line_feed(raw: &[u8]) -> bool {
    raw.iter()
        .enumerate()
        .all(|(at, byte)| *byte != b'\n' || (at > 0 && raw[at - 1] == b'\r'))
}

fn no_bare_carriage_return(raw: &[u8]) -> bool {
    raw.iter()
        .enumerate()
        .all(|(at, byte)| *byte != b'\r' || raw.get(at + 1) == Some(&b'\n'))
}

// ----------------------------------------------------------------------------
// Which IDE this is
// ----------------------------------------------------------------------------

#[test]
fn nothing_is_recognised_in_an_empty_folder() {
    let root = tempfile::tempdir().unwrap();

    assert!(ide::detect(root.path()).is_empty());
}

#[test]
fn a_cubemx_cmake_project_is_both_cmake_and_cubeide() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake", "cubeide"]));
    let names: Vec<&str> = ide::detect(&project)
        .iter()
        .map(|found| found.label)
        .collect();

    assert_eq!(names, ["CMake", "STM32CubeIDE"]);
}

#[test]
fn integrate_runs_every_ide_that_matches() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake", "cubeide", "keil", "iar"]));
    let library = demo(root.path(), "v1", "", &[]);
    let destination = project.join("demo");

    fs::create_dir_all(&destination).unwrap();

    let outcomes = ide::integrate(&project, &library, &destination, None, &nothing_dropped());

    assert_eq!(outcomes.len(), 4);
    assert!(
        outcomes
            .iter()
            .all(|outcome| outcome.status == Status::Changed)
    );
}

#[test]
fn only_one_ide_can_be_selected() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake", "cubeide"]));
    let library = demo(root.path(), "v1", "", &[]);
    let destination = project.join("demo");

    fs::create_dir_all(&destination).unwrap();

    let outcomes = ide::integrate(
        &project,
        &library,
        &destination,
        Some("cmake"),
        &nothing_dropped(),
    );
    let names: Vec<&str> = outcomes
        .iter()
        .map(|outcome| outcome.ide.as_str())
        .collect();

    assert_eq!(names, ["CMake"]);
}

// ----------------------------------------------------------------------------
// CMake
// ----------------------------------------------------------------------------

#[test]
fn the_cmake_block_points_at_the_library_folder() {
    // The project file gets two lines, not a list that grows with the library.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));
    let outcome = register("cmake", &project, &demo(root.path(), "v1", "", &[]));
    let text = read(project.join("CMakeLists.txt"));

    assert_eq!(outcome.status, Status::Changed);
    assert!(text.contains("add_subdirectory(demo)"), "{text}");
    assert!(
        text.contains("target_link_libraries(${CMAKE_PROJECT_NAME} PRIVATE demo_lib)"),
        "{text}"
    );
    assert!(!text.contains("demo.c"), "{text}");
}

#[test]
fn the_library_gets_its_own_cmakelists() {
    // The target must be INTERFACE. A STATIC one would not inherit the application's defines and include paths, so a
    // driver including main.h would fail to compile. That is the whole reason this file is generated.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    register("cmake", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(project.join("demo/CMakeLists.txt"));

    assert!(text.contains("add_library(demo_lib INTERFACE)"), "{text}");
    assert!(!text.contains("add_library(demo_lib STATIC"), "{text}");
    assert!(
        text.contains("${CMAKE_CURRENT_SOURCE_DIR}/demo.c"),
        "{text}"
    );
}

#[test]
fn the_cmake_block_is_replaced_not_repeated() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));
    let library = demo(root.path(), "v1", "", &[]);

    register("cmake", &project, &library);

    let second = register("cmake", &project, &library);

    assert_eq!(second.status, Status::Already);
    assert_eq!(
        read(project.join("CMakeLists.txt"))
            .matches(">>> libnima: demo >>>")
            .count(),
        1
    );
}

#[test]
fn cmake_says_what_to_do_with_a_file_it_does_not_understand() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    write(project.join("CMakeLists.txt"), "# nothing useful here\n");

    let outcome = register("cmake", &project, &demo(root.path(), "v1", "", &[]));

    assert_eq!(outcome.status, Status::Manual);
    assert!(!outcome.steps.is_empty());
}

#[test]
fn every_edit_leaves_a_backup() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));
    let outcome = register("cmake", &project, &demo(root.path(), "v1", "", &[]));
    let saved = outcome.backup.expect("no backup was made");

    assert!(read(saved).contains("add_executable"));
}

#[test]
fn two_backups_at_once_are_both_kept() {
    // A library and the one it needs, installed in one run within a second.
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("CMakeLists.txt");

    fs::write(&file, "before either\n").unwrap();

    let first = libnima_core::files::backup(&file).unwrap();

    fs::write(&file, "after the first\n").unwrap();

    let second = libnima_core::files::backup(&file).unwrap();

    assert_ne!(first, second);
    assert_eq!(read(&first), "before either\n");
    assert_eq!(read(&second), "after the first\n");
}

#[test]
fn the_library_target_does_not_clash_with_the_project() {
    // Found in a real CubeMX project called fsm, testing the fsm library. CMake allows one target per name, so naming the
    // library target after the library collided with the executable and the configure step failed.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    write(
        project.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.22)\nset(CMAKE_PROJECT_NAME demo)\nproject(${CMAKE_PROJECT_NAME})\nadd_executable(${CMAKE_PROJECT_NAME})\n",
    );

    register("cmake", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(project.join("demo/CMakeLists.txt"));

    assert!(text.contains("add_library(demo_lib INTERFACE)"), "{text}");
    assert!(!text.contains("add_library(demo INTERFACE)"), "{text}");
}

const PLAIN_LINK: &str = "cmake_minimum_required(VERSION 3.22)\nproject(Proj C)\nadd_executable(${CMAKE_PROJECT_NAME})\ntarget_link_libraries(${CMAKE_PROJECT_NAME}\n    stm32cubemx\n)\n";

#[test]
fn a_plain_link_signature_is_matched() {
    // The CubeMX template links without a keyword, and CMake refuses to mix the two forms on one target.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    write(project.join("CMakeLists.txt"), PLAIN_LINK);
    register("cmake", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(project.join("CMakeLists.txt"));

    assert!(
        text.contains("target_link_libraries(${CMAKE_PROJECT_NAME} demo_lib)"),
        "{text}"
    );
    assert!(!text.contains("PRIVATE demo_lib"), "{text}");
}

#[test]
fn a_keyword_link_signature_is_matched_too() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    write(
        project.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.22)\nproject(Proj C)\nadd_executable(app main.c)\ntarget_link_libraries(app PRIVATE something)\n",
    );
    register("cmake", &project, &demo(root.path(), "v1", "", &[]));

    assert!(
        read(project.join("CMakeLists.txt"))
            .contains("target_link_libraries(app PRIVATE demo_lib)")
    );
}

#[test]
fn reinstalling_does_not_flip_the_signature() {
    // The tool must not read its own previous block back as evidence.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));
    let library = demo(root.path(), "v1", "", &[]);

    write(project.join("CMakeLists.txt"), PLAIN_LINK);
    register("cmake", &project, &library);
    register("cmake", &project, &library);

    let text = read(project.join("CMakeLists.txt"));

    assert!(!text.contains("PRIVATE demo_lib"), "{text}");
    assert_eq!(text.matches("demo_lib").count(), 1, "{text}");
}

#[test]
fn the_cmake_block_is_recorded_with_its_fingerprint() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);

    installed(&project, &lib, "Libs/demo");

    let blocks = &record(&project)["demo"]["blocks"]["CMakeLists.txt"];

    assert_eq!(
        blocks.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["demo"]
    );
}

#[test]
fn a_half_written_block_leaves_the_file_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));
    let path = project.join("CMakeLists.txt");
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);

    write(&path, &format!("{}# <<< libnima: demo <<<\n", read(&path)));

    let before = read(&path);

    install(&project, &lib, "Libs/demo");

    assert_eq!(read(&path), before);
}

#[test]
fn a_once_file_naming_a_project_file_is_refused() {
    for to in [
        "CMakeLists.txt",
        "Proj.ioc",
        "Core/Inc/stm32f4xx_hal_conf.h",
        "MDK/Proj.uvprojx",
        "Makefile",
        ".cproject",
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["cmake"]));
        let lib = library(
            root.path(),
            "demo",
            &format!("{DEMO_CODE}once:\n  - from: template/demo_config.h\n    to: {to}\n"),
            &[("template/demo_config.h", "#define DEMO_SIZE 8\n")],
        );
        let (code, ui) = install(&project, &lib, "Libs/demo");

        assert_eq!(code, 2, "{to}");
        assert!(
            ui.err().contains("edits or reads as the project's own"),
            "{to}: {}",
            ui.err()
        );
    }
}

// ----------------------------------------------------------------------------
// STM32CubeIDE
// ----------------------------------------------------------------------------

fn define_lists(text: &str) -> Vec<Vec<String>> {
    let option =
        Regex::new(r#"(?s)c\.compiler\.option\.definedsymbols"[^>]*>(.*?)</option>"#).unwrap();
    let value = Regex::new(r#"value="([^"]*)""#).unwrap();

    option
        .captures_iter(text)
        .map(|found| {
            value
                .captures_iter(&found[1])
                .map(|one| one[1].to_string())
                .collect()
        })
        .collect()
}

#[test]
fn cubeide_adds_the_path_to_every_configuration() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let outcome = register("cubeide", &project, &demo(root.path(), "v1", "", &[]));

    assert_eq!(outcome.status, Status::Changed);
    assert_eq!(
        read(project.join(".cproject"))
            .matches("value=\"../demo\"")
            .count(),
        2
    );
}

#[test]
fn cubeide_keeps_the_file_version_line() {
    // A .cproject without it will not open.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));

    register("cubeide", &project, &demo(root.path(), "v1", "", &[]));

    assert!(read(project.join(".cproject")).contains("<?fileVersion 4.0.0?>"));
}

#[test]
fn cubeide_does_not_add_the_path_twice() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let library = demo(root.path(), "v1", "", &[]);

    register("cubeide", &project, &library);

    assert_eq!(
        register("cubeide", &project, &library).status,
        Status::Already
    );
}

#[test]
fn cubeide_lines_up_with_the_paths_already_there() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let before = read(project.join(".cproject"));

    register("cubeide", &project, &demo(root.path(), "v1", "", &[]));

    let after = read(project.join(".cproject"));
    let existing = before
        .lines()
        .find(|line| line.contains("../Core/Inc"))
        .unwrap();
    let added = added_lines(&before, &after);

    assert_eq!(added.len(), 2, "{added:?}");
    assert!(
        added.iter().all(|line| indent(line) == indent(existing)),
        "{added:?}"
    );
    assert_ne!(indent(existing), "");
}

#[test]
fn cubeide_puts_the_path_after_the_ones_already_there() {
    // Where CubeIDE itself puts one added through the Properties dialog.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));

    register("cubeide", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(project.join(".cproject"));

    assert!(text.find("value=\"../Drivers/CMSIS/Include\"") < text.find("value=\"../demo\""));
}

#[test]
fn cubeide_fills_in_a_configuration_the_other_already_has() {
    // Asking "is the path anywhere in this file" says yes as soon as Debug has it, and Release is then left without it.
    // That shows up much later as a build that works in one configuration and not the other.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let library = demo(root.path(), "v1", "", &[]);

    register("cubeide", &project, &library);

    let text = read(project.join(".cproject"));
    let wanted = "<listOptionValue builtIn=\"false\" value=\"../demo\"/>";
    let cut = text.rfind(wanted).unwrap();
    let line = text[..cut].rfind('\n').unwrap();

    write(
        project.join(".cproject"),
        &format!("{}{}", &text[..line], &text[cut + wanted.len()..]),
    );

    let outcome = register("cubeide", &project, &library);

    assert_eq!(outcome.status, Status::Changed);
    assert_eq!(
        read(project.join(".cproject"))
            .matches("value=\"../demo\"")
            .count(),
        2
    );
}

// ----------------------------------------------------------------------------
// Keil and IAR
// ----------------------------------------------------------------------------

#[test]
fn keil_uses_a_relative_path_from_its_own_subfolder() {
    // The .uvprojx lives in MDK/, so the library is one level up from it.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let outcome = register("keil", &project, &demo(root.path(), "v1", "", &[]));
    let text = read(project.join("MDK/Proj.uvprojx"));

    assert_eq!(outcome.status, Status::Changed);
    assert!(text.contains("..\\demo\\demo.c"), "{text}");
    assert!(text.contains("..\\Core\\Inc;..\\demo"), "{text}");
}

#[test]
fn iar_uses_a_relative_path_from_its_own_subfolder() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let outcome = register("iar", &project, &demo(root.path(), "v1", "", &[]));
    let text = read(project.join("EWARM/Proj.ewp"));

    assert_eq!(outcome.status, Status::Changed);
    assert!(text.contains("$PROJ_DIR$\\..\\demo\\demo.c"), "{text}");
    assert!(text.contains("<state>$PROJ_DIR$/../demo</state>"), "{text}");
}

fn tag_of(line: &str) -> String {
    line.split('<')
        .nth(1)
        .unwrap_or_default()
        .split('>')
        .next()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn keil_lines_up_with_the_groups_already_there() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let before = read(project.join("MDK/Proj.uvprojx"));

    register("keil", &project, &demo(root.path(), "v1", "", &[]));

    let after = read(project.join("MDK/Proj.uvprojx"));
    let added = added_lines(&before, &after);

    for tag in ["Group", "Files", "File", "FileName"] {
        let new = added.iter().find(|line| tag_of(line) == tag).unwrap();
        let old = before.lines().find(|line| tag_of(line) == tag).unwrap();

        assert_eq!(
            indent(new),
            indent(old),
            "{tag} does not line up with the one already there"
        );
    }
}

#[test]
fn keil_puts_the_group_after_the_ones_already_there() {
    // Where uVision puts one added through Manage Project Items.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));

    register("keil", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(project.join("MDK/Proj.uvprojx"));

    assert!(
        text.find("<GroupName>Application/User/Core</GroupName>")
            < text.find("<GroupName>demo</GroupName>")
    );
}

#[test]
fn keil_follows_the_slash_the_project_uses() {
    // CubeMX writes forward slashes and uVision writes backslashes. Keil reads either, so the one to write is whichever
    // is already in the file.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let path = project.join("MDK/Proj.uvprojx");

    write(&path, &read(&path).replace('\\', "/"));
    register("keil", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(&path);

    assert!(
        text.contains("<FilePath>../demo/demo.c</FilePath>"),
        "{text}"
    );
    assert!(!text.contains("..\\demo"), "{text}");
}

#[test]
fn keil_names_the_file_without_its_folder() {
    // <FileName> is what uVision shows in the project tree: demo.c, nothing more. It used to be cut from the full path,
    // and Linux and macOS do not split a path at a backslash, so there the whole uVision style path went in.
    for slash in ["\\", "/"] {
        for flat in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let project = project_with(root.path(), ides(&["keil"]));
            let path = project.join("MDK/Proj.uvprojx");
            let library = if flat {
                demo(root.path(), "v1", "", &[])
            } else {
                kept_paths(root.path(), "v1", "", &[])
            };

            write(&path, &read(&path).replace('\\', slash));
            register("keil", &project, &library);

            assert!(
                read(&path).contains("<FileName>demo.c</FileName>"),
                "{slash} {flat}"
            );
        }
    }
}

#[test]
fn iar_lines_up_with_what_is_already_there() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let before = read(project.join("EWARM/Proj.ewp"));

    register("iar", &project, &demo(root.path(), "v1", "", &[]));

    let after = read(project.join("EWARM/Proj.ewp"));
    let added = added_lines(&before, &after);
    let first_of = |lines: &[String], wanted: &str| {
        lines
            .iter()
            .find(|line| line.contains(wanted))
            .cloned()
            .unwrap()
    };
    let kept: Vec<String> = before.lines().map(str::to_string).collect();
    let file_name = added
        .iter()
        .find(|line| line.contains("$PROJ_DIR$") && line.contains("<name>"))
        .unwrap();

    // The shallowest, because the new group is a top level one. Comparing against a file nested three groups deep would
    // demand that depth of it.
    let shallowest = kept
        .iter()
        .filter(|line| line.contains("$PROJ_DIR$") && line.contains("<name>"))
        .map(|line| indent(line))
        .min_by_key(|found| found.len())
        .unwrap();

    assert_eq!(
        indent(&first_of(&added, "<state>")),
        indent(&first_of(&kept, "<state>"))
    );
    assert_eq!(
        indent(&first_of(&added, "<group>")),
        indent(&first_of(&kept, "<group>"))
    );
    assert_eq!(indent(file_name), shallowest);
}

#[test]
fn iar_follows_the_slash_the_project_uses() {
    // Embedded Workbench on Windows writes backslashes throughout.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let path = project.join("EWARM/Proj.ewp");
    let text = read(&path)
        .replace("$PROJ_DIR$/../Core/Inc", "$PROJ_DIR$\\..\\Core\\Inc")
        .replace(
            "$PROJ_DIR$/../Drivers/CMSIS/Include",
            "$PROJ_DIR$\\..\\Drivers\\CMSIS\\Include",
        );

    write(&path, &text);
    register("iar", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(&path);

    assert!(
        text.contains("<state>$PROJ_DIR$\\..\\demo</state>"),
        "{text}"
    );
    assert!(!text.contains("$PROJ_DIR$/../demo"), "{text}");
}

#[test]
fn iar_follows_the_slash_of_the_section_it_is_editing() {
    // CubeMX uses both slashes in one .ewp. On the real G431 project asking the file as a whole put a backslash path among
    // eight forward slash ones.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));

    register("iar", &project, &demo(root.path(), "v1", "", &[]));

    let text = read(project.join("EWARM/Proj.ewp"));

    assert!(text.contains("<state>$PROJ_DIR$/../demo</state>"), "{text}");
    assert!(
        text.contains("<name>$PROJ_DIR$\\..\\demo\\demo.c</name>"),
        "{text}"
    );
}

#[test]
fn iar_lines_up_with_a_nested_group() {
    // Groups nest, and matching Application against the </group> that belongs to Core reads the indentation off the wrong
    // level, so the new group lands at a depth nothing else in the file uses.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let before = read(project.join("EWARM/Proj.ewp"));

    register("iar", &project, &demo(root.path(), "v1", "", &[]));

    let after = read(project.join("EWARM/Proj.ewp"));
    let shallowest = before
        .lines()
        .filter(|line| line.trim() == "<group>")
        .map(indent)
        .min_by_key(|found| found.len());
    let added = added_lines(&before, &after);
    let groups: Vec<&str> = added
        .iter()
        .filter(|line| line.trim() == "<group>")
        .map(|line| indent(line))
        .collect();

    assert_eq!(groups, [shallowest.unwrap()]);
}

#[test]
fn iar_keeps_the_ewt_in_step_with_the_ewp() {
    // IAR mirrors the file tree into the .ewt beside the project.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));

    register("iar", &project, &demo(root.path(), "v1", "", &[]));

    let ewt = read(project.join("EWARM/Proj.ewt"));

    assert!(ewt.contains("<name>demo</name>"), "{ewt}");
    assert!(ewt.contains("$PROJ_DIR$\\..\\demo\\demo.c"), "{ewt}");
}

#[test]
fn iar_is_fine_without_an_ewt() {
    // Older workbenches do not write one, and that is not an error.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));

    fs::remove_file(project.join("EWARM/Proj.ewt")).unwrap();

    assert_eq!(
        register("iar", &project, &demo(root.path(), "v1", "", &[])).status,
        Status::Changed
    );
}

// ----------------------------------------------------------------------------
// Line endings
// ----------------------------------------------------------------------------

#[test]
fn a_crlf_project_stays_crlf() {
    // CubeMX writes CRLF on Windows, and these files are under version control. An integration that folds them rewrites
    // every line ending in the file to add one line to it.
    for (backend, place) in CASES {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), crlf(&[backend]));

        register(backend, &project, &demo(root.path(), "v1", "", &[]));

        let raw = fs::read(project.join(place)).unwrap();

        assert!(
            no_bare_line_feed(&raw),
            "{backend}: a line ends with a bare line feed"
        );
        assert!(
            no_bare_carriage_return(&raw),
            "{backend}: a carriage return lost its line feed"
        );
        assert!(raw.windows(2).any(|pair| pair == b"\r\n"), "{backend}");
    }
}

#[test]
fn a_second_run_on_a_crlf_project_changes_nothing() {
    // A block built with line feeds never compares equal to the same block stored with CRLF, so the integration would
    // decide it had to write the file again, and take a fresh backup, on every single run.
    for (backend, place) in CASES {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), crlf(&[backend]));
        let library = demo(root.path(), "v1", "", &[]);

        register(backend, &project, &library);

        let once = fs::read(project.join(place)).unwrap();
        let outcome = register(backend, &project, &library);

        assert_eq!(outcome.status, Status::Already, "{backend}");
        assert_eq!(fs::read(project.join(place)).unwrap(), once, "{backend}");
    }
}

// ----------------------------------------------------------------------------
// Which file is the project
// ----------------------------------------------------------------------------
//
// From a real G431 board. IAR had upgraded the project and left "Backup of STM32G431CBU6.ewp" beside it. The installer
// took the first by name, put the library into the backup, and said it had succeeded, and the project actually built was
// never touched.

#[test]
fn iar_ignores_the_backup_the_ide_left_behind() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            iar_backup: true,
            ..ides(&["iar"])
        },
    );
    let stale = project.join("EWARM/Backup of Proj.ewp");
    let before = fs::read(&stale).unwrap();
    let found = iar::detect(&project).unwrap();

    assert_eq!(found.file_name().unwrap(), "Proj.ewp");

    register("iar", &project, &demo(root.path(), "v1", "", &[]));

    assert_eq!(fs::read(&stale).unwrap(), before, "the backup was edited");
    assert!(read(project.join("EWARM/Proj.ewp")).contains("demo"));
}

#[test]
fn keil_ignores_the_backup_the_ide_left_behind() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            keil_backup: true,
            ..ides(&["keil"])
        },
    );

    assert_eq!(
        keil::detect(&project).unwrap().file_name().unwrap(),
        "Proj.uvprojx"
    );
}

#[test]
fn iar_asks_the_workspace_which_project_is_real() {
    // Renaming the real project to sort last proves the answer comes from the workspace rather than from the order the
    // files happen to be found in.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let ewarm = project.join("EWARM");

    fs::rename(ewarm.join("Proj.ewp"), ewarm.join("zzz.ewp")).unwrap();
    write(
        ewarm.join("Project.eww"),
        &read(ewarm.join("Project.eww")).replace("Proj.ewp", "zzz.ewp"),
    );
    write(ewarm.join("Aaa.ewp"), "<project></project>\n");

    assert_eq!(
        iar::detect(&project).unwrap().file_name().unwrap(),
        "zzz.ewp"
    );
}

// ----------------------------------------------------------------------------
// What goes in a group
// ----------------------------------------------------------------------------

#[test]
fn only_compilable_files_reach_an_ide_file_group() {
    // Keil and IAR name every file in the project, so the group is the file tree. A README in there is confusing at best.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil", "iar"]));
    let lib = manifest::load(
        &library(
            root.path(),
            "demo",
            "headers:\n  - inc/demo.h\nsources:\n  - src/demo.c\n  - README.md\n",
            &[("README.md", "# demo\n")],
        ),
        Some(Platform::Stm32),
    )
    .unwrap();

    assert_eq!(lib.build_sources(), ["src/demo.c"]);

    register("keil", &project, &lib);
    register("iar", &project, &lib);

    assert!(!read(project.join("MDK/Proj.uvprojx")).contains("README"));
    assert!(!read(project.join("EWARM/Proj.ewp")).contains("README"));
}

#[test]
fn the_group_is_named_after_the_library_and_nothing_else() {
    // That name is read by the user in the project tree every day.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil", "iar"]));
    let library = demo(root.path(), "v1", "", &[]);

    register("keil", &project, &library);
    register("iar", &project, &library);

    let uvprojx = read(project.join("MDK/Proj.uvprojx"));
    let ewp = read(project.join("EWARM/Proj.ewp"));

    assert!(uvprojx.contains("<GroupName>demo</GroupName>"));
    assert!(ewp.contains("<name>demo</name>"));
    assert!(!uvprojx.contains("libnima") && !ewp.contains("libnima"));
}

#[test]
fn a_renamed_group_is_still_recognised() {
    // The user owns that name once the files are in their project. A marker in the name would make a rename look like a
    // fresh install, and the second run would add every file a second time.
    for (backend, place) in LISTED {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil", "iar"]));
        let library = demo(root.path(), "v1", "", &[]);
        let path = project.join(place);

        register(backend, &project, &library);
        write(&path, &read(&path).replace(">demo<", ">My own name<"));

        let before = read(&path);
        let outcome = register(backend, &project, &library);

        assert_eq!(outcome.status, Status::Already, "{backend}");
        assert_eq!(read(&path), before, "{backend}");

        // The path, not the bare name: Keil writes the name in <FileName> as well.
        assert_eq!(before.matches("demo\\demo.c").count(), 1, "{backend}");
    }
}

#[test]
fn a_kept_path_has_one_kind_of_slash() {
    // A source with a folder of its own: joined on as it stood, a project written with backslashes got a path with both
    // kinds of slash in it.
    for (backend, place, written) in [
        (
            "keil",
            "MDK/Proj.uvprojx",
            "<FilePath>..\\demo\\src\\demo.c</FilePath>",
        ),
        (
            "iar",
            "EWARM/Proj.ewp",
            "<name>$PROJ_DIR$\\..\\demo\\src\\demo.c</name>",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil", "iar"]));

        register(backend, &project, &kept_paths(root.path(), "v1", "", &[]));

        let text = read(project.join(place));

        assert!(text.contains(written), "{text}");
        assert!(!text.contains("src/demo.c"), "{text}");
    }
}

#[test]
fn the_library_under_the_other_slash_is_still_recognised() {
    // The slash written is whichever the project uses most, and that can change: files the user adds through the IDE can
    // tip the count. An update must add neither a second group nor a second include path.
    let paths = Regex::new(r"[^<>;]*demo[^<>;]*").unwrap();

    for (backend, place) in LISTED {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil", "iar"]));
        let library = demo(root.path(), "v1", "", &[]);
        let path = project.join(place);

        register(backend, &project, &library);

        let done = read(&path);
        let swapped = paths.replace_all(&done, |found: &regex::Captures| {
            found[0]
                .chars()
                .map(|one| match one {
                    '\\' => '/',
                    '/' => '\\',
                    other => other,
                })
                .collect::<String>()
        });

        assert_ne!(swapped, done, "the library's paths did not change slash");
        write(&path, &swapped);

        let before = read(&path);
        let outcome = register(backend, &project, &library);

        assert_eq!(outcome.status, Status::Already, "{backend}");
        assert_eq!(read(&path), before, "{backend}");
    }
}

#[test]
fn a_header_only_library_gets_no_empty_group() {
    // With nothing to compile there is no file to recognise the library by, so an empty group would be added again on
    // every single run.
    for (backend, place) in LISTED {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil", "iar"]));
        let lib = manifest::load(
            &library(root.path(), "demo", "headers:\n  - inc/demo.h\n", &[]),
            Some(Platform::Stm32),
        )
        .unwrap();
        let first = register(backend, &project, &lib);
        let second = register(backend, &project, &lib);
        let text = read(project.join(place));

        assert_eq!(
            first.status,
            Status::Changed,
            "the include path still has to be added"
        );
        assert_eq!(second.status, Status::Already);
        assert!(text.contains("demo"), "the include path is there");
        assert_eq!(
            text.matches(">demo<").count(),
            0,
            "but no group was created for it"
        );
    }
}

// ----------------------------------------------------------------------------
// Updates: Keil and IAR name every file, so they have to follow a move
// ----------------------------------------------------------------------------

#[test]
fn an_update_moves_an_entry_with_its_file() {
    // The library's code moved into src/. The entry follows it, in place. Adding a second group for the new path would
    // leave the old entry pointing at a file that is gone, and a per-file setting the user made on it lost.
    for (backend, place, anchor, option, gone, moved, group) in [
        (
            "keil",
            "MDK/Proj.uvprojx",
            "<FilePath>..\\demo\\demo.c</FilePath>",
            "<IncludeInBuild>0</IncludeInBuild>",
            "<FilePath>..\\demo\\demo.c</FilePath>",
            "<FilePath>..\\demo\\src\\demo.c</FilePath>",
            "<GroupName>demo</GroupName>",
        ),
        (
            "iar",
            "EWARM/Proj.ewp",
            "<name>$PROJ_DIR$\\..\\demo\\demo.c</name>",
            "<configuration>Debug</configuration>",
            "<name>$PROJ_DIR$\\..\\demo\\demo.c</name>",
            "<name>$PROJ_DIR$\\..\\demo\\src\\demo.c</name>",
            "<name>demo</name>",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil", "iar"]));
        let old = demo(root.path(), "v1", "", &[]);
        let new = kept_paths(root.path(), "v2", "", &[]);
        let path = project.join(place);
        let setting = if backend == "keil" {
            format!("{anchor}\n              <FileOption>{option}</FileOption>")
        } else {
            format!("{anchor}\n            <excluded>{option}</excluded>")
        };

        register(backend, &project, &old);
        write(&path, &read(&path).replace(anchor, &setting));

        let outcome = register_with(backend, &project, &new, &dropping(&project, &["demo.c"]));
        let text = read(&path);

        assert_eq!(outcome.status, Status::Changed, "{backend}");
        assert_eq!(text.matches(moved).count(), 1, "{backend}: {text}");
        assert!(!text.contains(gone), "{backend}");
        assert_eq!(
            text.matches(group).count(),
            1,
            "{backend}: a second group was added"
        );
        assert!(
            text.contains(option),
            "{backend}: the per-file setting was lost"
        );
        assert_eq!(
            register(backend, &project, &new).status,
            Status::Already,
            "{backend}"
        );
    }
}

#[test]
fn iar_moves_the_ewt_entry_too() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));

    register("iar", &project, &demo(root.path(), "v1", "", &[]));
    register_with(
        "iar",
        &project,
        &kept_paths(root.path(), "v2", "", &[]),
        &dropping(&project, &["demo.c"]),
    );

    let ewt = read(project.join("EWARM/Proj.ewt"));

    assert!(ewt.contains("$PROJ_DIR$\\..\\demo\\src\\demo.c"), "{ewt}");
    assert!(!ewt.contains("$PROJ_DIR$\\..\\demo\\demo.c"), "{ewt}");
}

fn with_extra(root: &Path, tag: &str) -> Manifest {
    manifest::load(
        &version(
            root,
            tag,
            "demo",
            "headers:\n  - demo.h\nsources:\n  - demo.c\n  - extra.c\n",
            &[("demo.h", ""), ("demo.c", ""), ("extra.c", "")],
        ),
        Some(Platform::Stm32),
    )
    .unwrap()
}

#[test]
fn an_update_removes_the_entry_of_a_file_that_is_gone() {
    // Left in, the entry names a file that no longer exists, and the build fails.
    for (backend, place) in LISTED {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil", "iar"]));

        register(backend, &project, &with_extra(root.path(), "v1"));
        assert!(read(project.join(place)).contains("extra.c"));

        let outcome = register_with(
            backend,
            &project,
            &demo(root.path(), "v2", "", &[]),
            &dropping(&project, &["extra.c"]),
        );
        let text = read(project.join(place));

        assert_eq!(outcome.status, Status::Changed);
        assert!(!text.contains("extra.c"), "{text}");
        assert!(text.contains("demo.c"), "{text}");
    }
}

#[test]
fn a_file_the_library_gained_joins_its_group() {
    // Only a library the project has none of gets a new group, so an update used to see its old file, add nothing, and
    // leave the new one out of the build.
    for (backend, place) in LISTED {
        for line_ending in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let project = project_with(
                root.path(),
                Ides {
                    crlf: line_ending,
                    ..ides(&["keil", "iar"])
                },
            );
            let path = project.join(place);

            register(backend, &project, &demo(root.path(), "v1", "", &[]));
            fs::write(&path, read(&path).replace(">demo<", ">My own name<")).unwrap();
            register(backend, &project, &with_extra(root.path(), "v2"));

            let text = read(&path);

            assert_eq!(
                text.matches("extra.c<").count(),
                if backend == "keil" { 2 } else { 1 },
                "{backend}"
            );
            assert!(
                !text.contains(">demo<"),
                "a new group was added instead of using the renamed one"
            );
            assert!(text.find("extra.c") > text.find("demo.c"));

            if line_ending {
                assert!(
                    no_bare_line_feed(text.as_bytes()),
                    "a bare line feed went into a CRLF file"
                );
            }
        }
    }
}

// ----------------------------------------------------------------------------
// A CubeMX Makefile
// ----------------------------------------------------------------------------

/// The entries of one list in a Makefile, in order, and the raw lines they came from.
fn makefile_list(text: &str, name: &str) -> (Vec<String>, Vec<String>) {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.starts_with(&format!("{name} =")))
        .unwrap();
    let mut end = start;

    while lines[end].trim_end().ends_with('\\') {
        end += 1;
    }

    let body: Vec<String> = lines[start + 1..=end]
        .iter()
        .map(ToString::to_string)
        .collect();
    let entries = body
        .iter()
        .map(|line| line.trim_end().trim_end_matches('\\').trim().to_string())
        .collect();

    (entries, body)
}

fn labels_of(found: &[ide::Found]) -> Vec<&str> {
    found.iter().map(|one| one.label).collect()
}

#[test]
fn a_cubemx_makefile_is_recognised_whatever_its_line_endings() {
    // With CRLF it was not, and the library never reached the Makefile.
    for chosen in [ides(&["makefile"]), crlf(&["makefile"])] {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), chosen);

        assert_eq!(labels_of(&ide::detect(&project)), ["Makefile"]);
    }
}

#[test]
fn a_makefile_that_cubemx_did_not_write_is_left_alone() {
    // Plenty of projects have a Makefile. Only one with C_SOURCES is CubeMX's.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), Ides::default());

    write(project.join("Makefile"), "all:\n\tgcc main.c\n");

    assert!(ide::detect(&project).is_empty());
}

#[test]
fn the_makefile_gets_the_sources_and_the_include_folder() {
    for chosen in [ides(&["makefile"]), crlf(&["makefile"])] {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), chosen);
        let outcome = register("makefile", &project, &demo(root.path(), "v1", "", &[]));
        let raw = read(project.join("Makefile"));
        let text = raw.replace("\r\n", "\n");
        let (sources, lines) = makefile_list(&text, "C_SOURCES");
        let (includes, _) = makefile_list(&text, "C_INCLUDES");

        assert_eq!(outcome.status, Status::Changed);
        assert_eq!(sources.last().unwrap(), "demo/demo.c");
        assert_eq!(includes.last().unwrap(), "-Idemo");

        // The old last line gained a backslash and the new one has none, or make would join the list to whatever follows.
        assert!(lines[lines.len() - 2].ends_with('\\'));
        assert!(!lines[lines.len() - 1].ends_with('\\'));

        if chosen.crlf {
            assert!(no_bare_line_feed(raw.as_bytes()));
        }
    }
}

#[test]
fn the_makefile_is_left_alone_on_a_second_run() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));
    let library = demo(root.path(), "v1", "", &[]);

    register("makefile", &project, &library);

    let before = read(project.join("Makefile"));

    assert_eq!(
        register("makefile", &project, &library).status,
        Status::Already
    );
    assert_eq!(read(project.join("Makefile")), before);
}

#[test]
fn a_makefile_entry_follows_its_file_on_an_update() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));

    register("makefile", &project, &demo(root.path(), "v1", "", &[]));
    register_with(
        "makefile",
        &project,
        &kept_paths(root.path(), "v2", "", &[]),
        &dropping(&project, &["demo.c"]),
    );

    let text = read(project.join("Makefile"));
    let (sources, _) = makefile_list(&text, "C_SOURCES");
    let (includes, _) = makefile_list(&text, "C_INCLUDES");

    assert!(
        sources.contains(&"demo/src/demo.c".to_string()),
        "{sources:?}"
    );
    assert!(!sources.contains(&"demo/demo.c".to_string()), "{sources:?}");
    assert!(includes.contains(&"-Idemo/inc".to_string()), "{includes:?}");
}

#[test]
fn a_makefile_entry_moves_where_it_stands() {
    // The line changes in place. Removing it and adding the new one at the end would move it past the lines the user added
    // after it, and reorder the list.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));
    let path = project.join("Makefile");

    register("makefile", &project, &demo(root.path(), "v1", "", &[]));
    write(
        &path,
        &read(&path).replace("demo/demo.c\n", "demo/demo.c \\\nCore/Src/app.c\n"),
    );
    register_with(
        "makefile",
        &project,
        &kept_paths(root.path(), "v2", "", &[]),
        &dropping(&project, &["demo.c"]),
    );

    let (sources, _) = makefile_list(&read(&path), "C_SOURCES");

    assert_eq!(
        sources[sources.len() - 2..],
        ["demo/src/demo.c", "Core/Src/app.c"]
    );
}

#[test]
fn a_makefile_list_stays_whole_when_its_last_entry_goes() {
    // The library's file was the last in the list. With its line gone, the line now last must lose its backslash, or the
    // list runs on into the next line.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));

    register("makefile", &project, &with_extra(root.path(), "v1"));
    register_with(
        "makefile",
        &project,
        &demo(root.path(), "v2", "", &[]),
        &dropping(&project, &["extra.c"]),
    );

    let text = read(project.join("Makefile"));
    let (sources, lines) = makefile_list(&text, "C_SOURCES");

    assert_eq!(sources.last().unwrap(), "demo/demo.c");
    assert!(!sources.contains(&"demo/extra.c".to_string()));
    assert!(!lines.last().unwrap().ends_with('\\'));
    assert!(text.contains("# ASM sources"));
}

// ----------------------------------------------------------------------------
// Defines the whole project is built with
// ----------------------------------------------------------------------------
//
// littlefs is the reason they exist. It reads its settings from a header named by LFS_DEFINES, and only when the compiler
// is given -DLFS_DEFINES=lfs_defines.h. The define has to reach every file that includes lfs.h, since LFS_THREADSAFE adds
// two members to struct lfs_config: two files that disagree about it disagree about where every later member sits, and
// nothing reports it. So each IDE gets the define where USE_HAL_DRIVER already is, for the whole project, in every
// configuration.

#[test]
fn the_manifest_reads_the_defines() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(
        asking(root.path(), "v1", &[LFS, "DEMO_FAST"]).defines,
        [LFS, "DEMO_FAST"]
    );
    assert!(asking(root.path(), "v2", &[]).defines.is_empty());
}

#[test]
fn a_define_that_is_not_plain_is_refused() {
    for bad in [
        "'LFS_DEFINES=lfs defines.h'",
        "'LFS_DEFINES=\"lfs_defines.h\"'",
        "'LFS_DEFINES=a.h -include evil.h'",
        "1LFS=1",
        "'LFS_DEFINES=<x>'",
        "LFS_DEFINES=",
        "-DLFS_DEFINES=lfs_defines.h",
    ] {
        let root = tempfile::tempdir().unwrap();
        let lib = library(
            root.path(),
            "demo",
            &format!("{DEMO_CODE}defines:\n  - {bad}\n"),
            &[],
        );
        let message = manifest::load(&lib, Some(Platform::Stm32))
            .unwrap_err()
            .message;

        assert!(message.contains("defines"), "{bad}: {message}");
    }
}

#[test]
fn one_name_set_twice_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}defines:\n  - {LFS}\n  - LFS_DEFINES=other.h\n"),
        &[],
    );
    let message = manifest::load(&lib, Some(Platform::Stm32))
        .unwrap_err()
        .message;

    assert!(message.contains("LFS_DEFINES"), "{message}");
}

#[test]
fn defines_written_as_one_line_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}defines: LFS\n"),
        &[],
    );
    let message = manifest::load(&lib, Some(Platform::Stm32))
        .unwrap_err()
        .message;

    assert!(message.contains("list"), "{message}");
}

#[test]
fn cmake_hands_the_define_to_the_whole_project() {
    // INTERFACE, so it reaches every file of the application, not only lfs.c.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    register("cmake", &project, &asking(root.path(), "v1", &[LFS]));

    let text = read(project.join("demo/CMakeLists.txt"));

    assert!(
        text.contains(&format!(
            "target_compile_definitions(demo_lib INTERFACE\n    {LFS}\n)\n"
        )),
        "{text}"
    );
}

#[test]
fn cmake_writes_nothing_new_for_a_library_without_defines() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cmake"]));

    register("cmake", &project, &asking(root.path(), "v1", &[]));

    let text = read(project.join("demo/CMakeLists.txt"));

    assert!(!text.contains("target_compile_definitions"), "{text}");
    assert!(
        text.ends_with(
            "target_include_directories(demo_lib INTERFACE ${CMAKE_CURRENT_SOURCE_DIR})\n"
        ),
        "{text}"
    );
}

#[test]
fn cubeide_adds_the_define_to_every_configuration() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let outcome = register("cubeide", &project, &asking(root.path(), "v1", &[LFS]));
    let text = read(project.join(".cproject"));

    assert_eq!(outcome.status, Status::Changed);
    assert!(outcome.message.contains("defines"), "{}", outcome.message);
    assert_eq!(
        define_lists(&text),
        [
            vec!["DEBUG", "USE_HAL_DRIVER", "STM32G431xx", LFS],
            vec!["USE_HAL_DRIVER", "STM32G431xx", LFS]
        ]
    );
}

#[test]
fn cubeide_leaves_the_assembler_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));

    register("cubeide", &project, &asking(root.path(), "v1", &[LFS]));

    let text = read(project.join(".cproject"));
    let assembler =
        Regex::new(r#"(?s)assembler\.option\.definedsymbols"[^>]*>(.*?)</option>"#).unwrap();
    let found = assembler.captures(&text).unwrap();

    assert!(found[1].contains("value=\"DEBUG\""));
    assert!(!found[1].contains(LFS));
}

#[test]
fn cubeide_lines_the_define_up_with_the_ones_there() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));

    register("cubeide", &project, &asking(root.path(), "v1", &[LFS]));

    let wanted = format!(
        "\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"STM32G431xx\"/>\n\t\t\t\t\t\t<listOptionValue builtIn=\"false\" value=\"{LFS}\"/>\n\t\t\t\t\t</option>\n"
    );

    assert!(read(project.join(".cproject")).contains(&wanted));
}

#[test]
fn cubeide_does_not_add_the_define_twice() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let library = asking(root.path(), "v1", &[LFS]);

    register("cubeide", &project, &library);

    assert_eq!(
        register("cubeide", &project, &library).status,
        Status::Already
    );
    assert_eq!(read(project.join(".cproject")).matches(LFS).count(), 2);
}

#[test]
fn cubeide_follows_a_value_the_library_changed() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));

    register(
        "cubeide",
        &project,
        &asking(root.path(), "v1", &["LFS_DEFINES=old.h"]),
    );
    register_with(
        "cubeide",
        &project,
        &asking(root.path(), "v2", &[LFS]),
        &dropping_define("LFS_DEFINES=old.h"),
    );

    let text = read(project.join(".cproject"));

    assert!(!text.contains("old.h"));
    assert_eq!(
        define_lists(&text)[1],
        ["USE_HAL_DRIVER", "STM32G431xx", LFS]
    );
}

#[test]
fn cubeide_takes_out_a_define_the_library_no_longer_has() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));

    register("cubeide", &project, &asking(root.path(), "v1", &[LFS]));
    register_with(
        "cubeide",
        &project,
        &asking(root.path(), "v2", &[]),
        &dropping_define(LFS),
    );

    let text = read(project.join(".cproject"));

    assert!(!text.contains(LFS));
    assert_eq!(define_lists(&text)[1], ["USE_HAL_DRIVER", "STM32G431xx"]);
}

#[test]
fn cubeide_keeps_a_value_the_user_set_and_says_so() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let path = project.join(".cproject");

    write(
        &path,
        &read(&path).replace("value=\"STM32G431xx\"", "value=\"LFS_DEFINES=mine.h\""),
    );

    let outcome = register("cubeide", &project, &asking(root.path(), "v1", &[LFS]));
    let text = read(&path);

    assert!(!text.contains(LFS));
    assert_eq!(text.matches("LFS_DEFINES=mine.h").count(), 2);
    assert!(
        outcome
            .steps
            .iter()
            .any(|step| step.contains("LFS_DEFINES=mine.h") && step.contains("left as it is")),
        "{:?}",
        outcome.steps
    );
}

#[test]
fn cubeide_without_a_define_list_says_what_to_add() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["cubeide"]));
    let path = project.join(".cproject");
    let lists = Regex::new(r#"(?s)[ \t]*<option id="d[ab]".*?</option>\n"#).unwrap();

    write(&path, &lists.replace_all(&read(&path), ""));

    let outcome = register("cubeide", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(outcome.status, Status::Manual);
    assert!(
        outcome.steps.iter().any(|step| step.contains(LFS)),
        "{:?}",
        outcome.steps
    );
    assert!(
        read(&path).contains("value=\"../demo\""),
        "the rest was not done"
    );
}

fn keil_defines(text: &str) -> Vec<String> {
    let found = Regex::new(r"<Define>(.*?)</Define>").unwrap();

    found
        .captures_iter(text)
        .map(|one| one[1].to_string())
        .collect()
}

#[test]
fn keil_adds_the_define_to_the_c_compiler_only() {
    // The <Define> under <Aads> is the assembler's.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let outcome = register("keil", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(outcome.status, Status::Changed);
    assert_eq!(
        keil_defines(&read(project.join("MDK/Proj.uvprojx"))),
        [format!("USE_HAL_DRIVER,STM32G431xx,{LFS}"), String::new()]
    );
}

#[test]
fn keil_adds_the_define_to_every_target() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let path = project.join("MDK/Proj.uvprojx");
    let text = read(&path);
    let target = Regex::new(r"(?s)    <Target>.*?</Target>\n")
        .unwrap()
        .find(&text)
        .unwrap()
        .as_str()
        .to_string();

    write(&path, &text.replace(&target, &format!("{target}{target}")));
    register("keil", &project, &asking(root.path(), "v1", &[LFS]));

    let one = format!("USE_HAL_DRIVER,STM32G431xx,{LFS}");

    assert_eq!(
        keil_defines(&read(&path)),
        [one.clone(), String::new(), one, String::new()]
    );
}

#[test]
fn keil_follows_the_separator_already_used() {
    for (written, expected) in [
        (
            "USE_HAL_DRIVER STM32G431xx",
            format!("USE_HAL_DRIVER STM32G431xx {LFS}"),
        ),
        (
            "USE_HAL_DRIVER, STM32G431xx",
            format!("USE_HAL_DRIVER, STM32G431xx, {LFS}"),
        ),
        ("USE_HAL_DRIVER", format!("USE_HAL_DRIVER,{LFS}")),
        ("", LFS.to_string()),
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&["keil"]));
        let path = project.join("MDK/Proj.uvprojx");

        write(
            &path,
            &read(&path).replace("USE_HAL_DRIVER,STM32G431xx", written),
        );
        register("keil", &project, &asking(root.path(), "v1", &[LFS]));

        assert_eq!(keil_defines(&read(&path))[0], expected);
    }
}

#[test]
fn keil_follows_a_value_the_library_changed() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));

    register(
        "keil",
        &project,
        &asking(root.path(), "v1", &["LFS_DEFINES=old.h"]),
    );
    register_with(
        "keil",
        &project,
        &asking(root.path(), "v2", &[LFS]),
        &dropping_define("LFS_DEFINES=old.h"),
    );

    assert_eq!(
        keil_defines(&read(project.join("MDK/Proj.uvprojx")))[0],
        format!("USE_HAL_DRIVER,STM32G431xx,{LFS}")
    );
}

#[test]
fn keil_keeps_a_value_the_user_set() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let path = project.join("MDK/Proj.uvprojx");

    write(
        &path,
        &read(&path).replace("STM32G431xx", "LFS_DEFINES=mine.h"),
    );

    let outcome = register("keil", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(
        keil_defines(&read(&path))[0],
        "USE_HAL_DRIVER,LFS_DEFINES=mine.h"
    );
    assert!(
        outcome
            .steps
            .iter()
            .any(|step| step.contains("left as it is")),
        "{:?}",
        outcome.steps
    );
}

#[test]
fn keil_without_a_define_list_says_what_to_add() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let path = project.join("MDK/Proj.uvprojx");
    let lists = Regex::new(r"[ \t]*<Define>[^<]*</Define>\n").unwrap();

    write(&path, &lists.replace_all(&read(&path), ""));

    let outcome = register("keil", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(outcome.status, Status::Manual);
    assert!(
        outcome.steps.iter().any(|step| step.contains(LFS)),
        "{:?}",
        outcome.steps
    );
}

fn iar_defines(text: &str) -> Vec<String> {
    let option = Regex::new(r"(?s)<name>CCDefines</name>(.*?)</option>").unwrap();
    let state = Regex::new(r"<state>([^<]*)</state>").unwrap();
    let body = option.captures(text).unwrap();

    state
        .captures_iter(&body[1])
        .map(|one| one[1].to_string())
        .collect()
}

#[test]
fn iar_adds_the_define_in_line_with_the_others() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let outcome = register("iar", &project, &asking(root.path(), "v1", &[LFS]));
    let wanted = format!(
        "                    <state>STM32G431xx</state>\n                    <state>{LFS}</state>\n                </option>\n"
    );

    assert_eq!(outcome.status, Status::Changed);
    assert!(read(project.join("EWARM/Proj.ewp")).contains(&wanted));
    assert!(!read(project.join("EWARM/Proj.ewt")).contains(LFS));
}

#[test]
fn iar_does_not_add_the_define_twice() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let library = asking(root.path(), "v1", &[LFS]);

    register("iar", &project, &library);

    assert_eq!(register("iar", &project, &library).status, Status::Already);
    assert_eq!(
        iar_defines(&read(project.join("EWARM/Proj.ewp"))),
        ["USE_HAL_DRIVER", "STM32G431xx", LFS]
    );
}

#[test]
fn iar_follows_a_value_the_library_changed() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));

    register(
        "iar",
        &project,
        &asking(root.path(), "v1", &["LFS_DEFINES=old.h"]),
    );
    register_with(
        "iar",
        &project,
        &asking(root.path(), "v2", &[LFS]),
        &dropping_define("LFS_DEFINES=old.h"),
    );

    let text = read(project.join("EWARM/Proj.ewp"));

    assert_eq!(iar_defines(&text), ["USE_HAL_DRIVER", "STM32G431xx", LFS]);
    assert!(!text.contains("old.h"));
}

#[test]
fn iar_keeps_a_value_the_user_set() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let path = project.join("EWARM/Proj.ewp");

    write(
        &path,
        &read(&path).replace("<state>STM32G431xx", "<state>LFS_DEFINES=mine.h"),
    );

    let outcome = register("iar", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(
        iar_defines(&read(&path)),
        ["USE_HAL_DRIVER", "LFS_DEFINES=mine.h"]
    );
    assert!(
        outcome
            .steps
            .iter()
            .any(|step| step.contains("left as it is")),
        "{:?}",
        outcome.steps
    );
}

#[test]
fn iar_without_a_define_list_says_what_to_add() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["iar"]));
    let path = project.join("EWARM/Proj.ewp");
    let list = Regex::new(r"(?s)[ \t]*<option>\s*<name>CCDefines</name>.*?</option>\n").unwrap();

    write(&path, &list.replace_all(&read(&path), ""));

    let outcome = register("iar", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(outcome.status, Status::Manual);
    assert!(
        outcome.steps.iter().any(|step| step.contains(LFS)),
        "{:?}",
        outcome.steps
    );
}

#[test]
fn the_makefile_gets_the_define_in_c_defs() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));
    let outcome = register("makefile", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(outcome.status, Status::Changed);
    assert!(outcome.message.contains("C_DEFS"), "{}", outcome.message);
    assert!(read(project.join("Makefile")).contains(&format!(
        "C_DEFS =  \\\n-DUSE_HAL_DRIVER \\\n-DSTM32G431xx \\\n-D{LFS}\n\n# C includes\n"
    )));
}

#[test]
fn the_makefile_does_not_get_the_define_twice() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));
    let library = asking(root.path(), "v1", &[LFS]);

    register("makefile", &project, &library);

    assert_eq!(
        register("makefile", &project, &library).status,
        Status::Already
    );
}

#[test]
fn the_makefile_list_stays_whole_when_the_library_takes_its_define_back() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));

    register("makefile", &project, &asking(root.path(), "v1", &[LFS]));
    register_with(
        "makefile",
        &project,
        &asking(root.path(), "v2", &[]),
        &dropping_define(LFS),
    );

    assert!(
        read(project.join("Makefile"))
            .contains("C_DEFS =  \\\n-DUSE_HAL_DRIVER \\\n-DSTM32G431xx\n\n# C includes\n")
    );
}

#[test]
fn the_makefile_follows_a_value_the_library_changed() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));

    register(
        "makefile",
        &project,
        &asking(root.path(), "v1", &["LFS_DEFINES=old.h", "DEMO_FAST"]),
    );
    register_with(
        "makefile",
        &project,
        &asking(root.path(), "v2", &[LFS, "DEMO_FAST"]),
        &dropping_define("LFS_DEFINES=old.h"),
    );

    assert!(
        read(project.join("Makefile"))
            .contains(&format!("-DSTM32G431xx \\\n-DDEMO_FAST \\\n-D{LFS}\n\n"))
    );
}

#[test]
fn the_makefile_keeps_a_value_the_user_set() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));
    let path = project.join("Makefile");

    write(
        &path,
        &read(&path).replace("-DSTM32G431xx", "-DLFS_DEFINES=mine.h"),
    );

    let outcome = register("makefile", &project, &asking(root.path(), "v1", &[LFS]));

    assert!(!read(&path).contains(&format!("-D{LFS}")));
    assert!(
        outcome
            .steps
            .iter()
            .any(|step| step.contains("left as it is")),
        "{:?}",
        outcome.steps
    );
}

#[test]
fn a_makefile_without_c_defs_says_what_to_add() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["makefile"]));
    let path = project.join("Makefile");

    write(
        &path,
        &read(&path).replace("C_DEFS =  \\\n-DUSE_HAL_DRIVER \\\n-DSTM32G431xx\n", ""),
    );

    let outcome = register("makefile", &project, &asking(root.path(), "v1", &[LFS]));

    assert_eq!(outcome.status, Status::Manual);
    assert!(
        outcome
            .steps
            .iter()
            .any(|step| step.contains(&format!("-D{LFS}"))),
        "{:?}",
        outcome.steps
    );
}

const WITH_DEFINES: [(&str, &str); 4] = [
    ("cubeide", ".cproject"),
    ("keil", "MDK/Proj.uvprojx"),
    ("iar", "EWARM/Proj.ewp"),
    ("makefile", "Makefile"),
];

#[test]
fn a_crlf_project_stays_crlf_with_a_define() {
    for (backend, place) in WITH_DEFINES {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), crlf(&[backend]));
        let library = asking(root.path(), "v1", &[LFS]);

        register(backend, &project, &library);

        let raw = fs::read(project.join(place)).unwrap();
        let second = register(backend, &project, &library);

        assert!(String::from_utf8_lossy(&raw).contains(LFS), "{backend}");
        assert!(
            no_bare_line_feed(&raw),
            "{backend}: a line ends with a bare line feed"
        );
        assert_eq!(second.status, Status::Already, "{backend}");
        assert_eq!(fs::read(project.join(place)).unwrap(), raw, "{backend}");
    }
}

#[test]
fn a_library_without_defines_leaves_the_define_list_alone() {
    for (backend, place) in WITH_DEFINES {
        let root = tempfile::tempdir().unwrap();
        let project = project_with(root.path(), ides(&[backend]));

        register(backend, &project, &asking(root.path(), "v1", &[]));

        let text = read(project.join(place));

        assert!(text.contains("STM32G431xx"), "{backend}");
        assert!(!text.contains("LFS"), "{backend}");
    }
}

fn defining(root: &Path, tag: &str, defines: &[&str]) -> PathBuf {
    let listed: String = defines
        .iter()
        .map(|define| format!("  - {define}\n"))
        .collect();
    let body = if defines.is_empty() {
        DEMO_CODE.to_string()
    } else {
        format!("{DEMO_CODE}defines:\n{listed}")
    };

    version(root, tag, "demo", &body, &[])
}

#[test]
fn the_record_keeps_the_defines() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());

    installed(&project, &defining(root.path(), "v1", &[LFS]), "demo");

    assert_eq!(strings(&record(&project)["demo"]["defines"]), [LFS]);
}

#[test]
fn a_define_edited_into_the_record_is_never_used() {
    // The record is a file in the user's project, so it is not trusted.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));
    let lib = defining(root.path(), "v1", &[LFS]);
    let uvprojx = project.join("MDK/Proj.uvprojx");

    installed(&project, &lib, "demo");
    write(
        &uvprojx,
        &read(&uvprojx).replace("STM32G431xx", "OLD_ONE,STM32G431xx"),
    );

    let path = project.join(".libnima.json");
    let mut written: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();

    written["libraries"]["demo"]["defines"] = serde_json::json!(["X\"/><evil", 7, "OLD_ONE"]);
    write(&path, &written.to_string());

    installed(&project, &lib, "demo");

    let text = read(&uvprojx);

    assert!(!text.contains("OLD_ONE"), "{text}");
    assert!(!text.contains("evil"), "{text}");
}

#[test]
fn a_copy_in_another_folder_drops_nothing() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), ides(&["keil"]));

    installed(&project, &defining(root.path(), "v1", &[LFS]), "demo");
    installed(&project, &defining(root.path(), "v2", &[]), "other");

    assert!(read(project.join("MDK/Proj.uvprojx")).contains(LFS));
}

#[test]
fn an_update_from_start_to_finish_moves_the_define() {
    // The whole path: record, result, and every IDE told what to take out.
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        ides(&["cmake", "cubeide", "keil", "iar", "makefile"]),
    );

    installed(
        &project,
        &defining(root.path(), "v1", &["LFS_DEFINES=old.h"]),
        "demo",
    );
    installed(&project, &defining(root.path(), "v2", &[LFS]), "demo");

    for place in [
        ".cproject",
        "MDK/Proj.uvprojx",
        "EWARM/Proj.ewp",
        "Makefile",
        "demo/CMakeLists.txt",
    ] {
        let text = read(project.join(place));

        assert!(text.contains(LFS), "{place}");
        assert!(!text.contains("old.h"), "{place}");
    }
}

// ----------------------------------------------------------------------------
// Checking a project against what a library needs
// ----------------------------------------------------------------------------

const HAL_CONF: &str = "#define HAL_MODULE_ENABLED\n#define HAL_GPIO_MODULE_ENABLED\n/*#define HAL_SPI_MODULE_ENABLED   */\n";
const IOC: &str = "Mcu.IP0=NVIC\nMcu.IP1=RCC\nMcu.IP2=GPIO\nMcu.IP3=USART2\n";

fn cubemx_project(root: &Path, ioc: &str) -> PathBuf {
    let project = stm32_project(root);

    write(project.join("Core/Inc/stm32g4xx_hal_conf.h"), HAL_CONF);
    write(project.join("Proj.ioc"), ioc);

    project
}

fn needing(root: &Path, more: &str) -> Manifest {
    manifest::load(
        &library(root, "demo", &format!("{DEMO_CODE}{more}"), &[]),
        Some(Platform::Stm32),
    )
    .unwrap()
}

#[test]
fn a_library_that_needs_nothing_reports_nothing() {
    let root = tempfile::tempdir().unwrap();

    assert!(checks::check(&needing(root.path(), ""), &cubemx_project(root.path(), IOC)).is_empty());
}

#[test]
fn a_commented_out_hal_module_counts_as_disabled() {
    // CubeMX leaves the define in place, commented, which is easy to misread.
    let root = tempfile::tempdir().unwrap();
    let project = cubemx_project(root.path(), IOC);
    let enabled = checks::enabled_hal_modules(checks::find_hal_conf(&project).as_deref());

    assert!(enabled.contains(&"gpio".to_string()), "{enabled:?}");
    assert!(!enabled.contains(&"spi".to_string()), "{enabled:?}");
}

#[test]
fn a_disabled_hal_module_is_flagged() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(root.path(), "stm32:\n  requires:\n    hal_modules: [spi]\n");
    let found = checks::check(&library, &cubemx_project(root.path(), IOC));

    assert!(
        found
            .iter()
            .any(|one| one.level == Level::Warn && one.message.contains("SPI")),
        "{found:?}"
    );
}

#[test]
fn an_enabled_hal_module_passes() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(
        root.path(),
        "stm32:\n  requires:\n    hal_modules: [gpio]\n",
    );
    let found = checks::check(&library, &cubemx_project(root.path(), IOC));

    assert!(found.iter().any(|one| one.level == Level::Ok), "{found:?}");
}

#[test]
fn a_peripheral_matches_any_numbered_instance() {
    // A library asking for usart is satisfied by USART2.
    let root = tempfile::tempdir().unwrap();
    let library = needing(
        root.path(),
        "requires:\n  peripherals:\n    - type: usart\n",
    );
    let found = checks::check(&library, &cubemx_project(root.path(), IOC));

    assert!(
        found.iter().all(|one| one.level != Level::Warn),
        "{found:?}"
    );
}

#[test]
fn a_missing_peripheral_is_flagged_with_what_is_needed() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(
        root.path(),
        "requires:\n  peripherals:\n    - type: spi\n      note: Master mode\n",
    );
    let found = checks::check(&library, &cubemx_project(root.path(), IOC));
    let warned = found
        .iter()
        .find(|one| one.level == Level::Warn)
        .expect("nothing was flagged");

    assert!(warned.hint.contains("Master mode"), "{warned:?}");
}

#[test]
fn a_missing_rtos_is_flagged() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(root.path(), "stm32:\n  requires:\n    rtos: freertos\n");
    let found = checks::check(&library, &cubemx_project(root.path(), IOC));

    assert!(
        found
            .iter()
            .any(|one| one.level == Level::Warn && one.message.contains("RTOS")),
        "{found:?}"
    );
}

#[test]
fn freertos_in_the_ioc_satisfies_the_requirement() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(root.path(), "stm32:\n  requires:\n    rtos: freertos\n");
    let found = checks::check(
        &library,
        &cubemx_project(root.path(), &format!("{IOC}Mcu.IP4=FREERTOS\n")),
    );

    assert!(
        found
            .iter()
            .any(|one| one.level == Level::Ok && one.message.contains("freertos")),
        "{found:?}"
    );
}

#[test]
fn a_different_rtos_is_a_warning_not_a_refusal() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(root.path(), "stm32:\n  requires:\n    rtos: threadx\n");
    let found = checks::check(
        &library,
        &cubemx_project(root.path(), &format!("{IOC}Mcu.IP4=FREERTOS\n")),
    );

    assert!(
        found
            .iter()
            .any(|one| one.level == Level::Warn && one.message.contains("threadx")),
        "{found:?}"
    );
}

#[test]
fn any_rtos_is_satisfied_by_whatever_is_there() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(root.path(), "stm32:\n  requires:\n    rtos: any\n");
    let found = checks::check(
        &library,
        &cubemx_project(root.path(), &format!("{IOC}Mcu.IP4=FREERTOS\n")),
    );

    assert!(
        found.iter().all(|one| one.level != Level::Warn),
        "{found:?}"
    );
}

#[test]
fn a_project_with_no_cubemx_files_is_reported_as_unverified() {
    // Being unable to check must never read as everything being fine.
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty");
    let library = needing(root.path(), "stm32:\n  requires:\n    hal_modules: [spi]\n");

    fs::create_dir_all(&empty).unwrap();

    let found = checks::check(&library, &empty);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].level, Level::Info);
}

#[test]
fn a_peripheral_given_as_a_word_is_taken() {
    let root = tempfile::tempdir().unwrap();
    let library = needing(
        root.path(),
        "requires:\n  peripherals:\n    - spi\n    - type: i2c\n      count: 2\n      note: 400 kHz\n",
    );
    let shown: Vec<String> = library
        .requires
        .peripherals
        .iter()
        .map(|one| one.describe())
        .collect();

    assert_eq!(shown, ["SPI", "I2C x2, 400 kHz"]);
}

#[test]
fn a_count_that_is_no_whole_number_is_refused() {
    for count in ["two", "0", "-1", "true", "'1.5'"] {
        let root = tempfile::tempdir().unwrap();
        let lib = library(
            root.path(),
            "demo",
            &format!(
                "{DEMO_CODE}requires:\n  peripherals:\n    - type: spi\n      count: {count}\n"
            ),
            &[],
        );
        let message = manifest::load(&lib, Some(Platform::Stm32))
            .unwrap_err()
            .message;

        assert!(
            message.contains("requires.peripherals spi has a count of"),
            "{count}: {message}"
        );
    }
}
