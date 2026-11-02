//! Copying a library into a project, updating it, and taking it out.

mod common;

use std::fs;

use common::*;
use libnima_core::flow;

/// A configuration header whose settings sit between USER CODE lines.
const CONFIG: &str = "#ifndef DEMO_CONFIG_H\n/* USER CODE BEGIN DEMO_CONFIGURATION */\n#define DEMO_SIZE 8\n/* USER CODE END DEMO_CONFIGURATION */\n#endif\n";

// ----------------------------------------------------------------------------
// The first install
// ----------------------------------------------------------------------------

#[test]
fn every_file_keeps_its_path_in_the_folder() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    assert!(project.join("Libs/demo/inc/demo.h").is_file());
    assert!(project.join("Libs/demo/src/demo.c").is_file());
}

#[test]
fn the_notice_travels_with_the_code() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    assert!(project.join("demo/NOTICE").is_file());
}

#[test]
fn the_record_says_what_was_installed() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("version: 1.0.0\n{DEMO_CODE}"),
        &[],
    );
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    let entry = &record(&project)["demo"];

    assert_eq!(entry["version"], "1.0.0");
    assert_eq!(entry["folder"], "demo");
    assert!(strings(&entry["files"]).contains(&"demo/src/demo.c".to_string()));
    assert_eq!(entry["origin"]["kind"], "folder");
}

#[test]
fn the_record_says_which_platform_the_project_is() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    assert_eq!(
        libnima_core::record::recorded_platform(&project),
        Some(libnima_core::platform::Platform::Stm32)
    );
}

#[test]
fn a_damaged_record_does_not_break_an_install() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());
    write(project.join(".libnima.json"), "{ not json");

    installed(&project, &lib, "demo");

    assert!(record(&project).contains_key("demo"));
}

#[test]
fn nothing_installed_reads_as_nothing() {
    let root = tempfile::tempdir().unwrap();

    assert!(record(root.path()).is_empty());
}

#[test]
fn a_file_where_the_folder_should_be_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());
    write(project.join("demo"), "a file\n");

    let (code, ui) = install(&project, &lib, "demo");

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("is a file, so it cannot hold the library"),
        "{}",
        ui.err()
    );
}

#[test]
fn markers_that_do_not_pair_in_the_library_install_nothing() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        DEMO_CODE,
        &[("src/demo.c", "/* USER CODE BEGIN A */\n")],
    );
    let project = stm32_project(root.path());

    let (code, ui) = install(&project, &lib, "demo");

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("USER CODE markers that do not pair"),
        "{}",
        ui.err()
    );
    assert!(!project.join("demo").exists());
}

// ----------------------------------------------------------------------------
// An update
// ----------------------------------------------------------------------------

#[test]
fn code_is_replaced_on_a_second_install() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");
    write(project.join("demo/src/demo.c"), "/* stale */\n");
    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("demo/src/demo.c")), "/* source */\n");
}

#[test]
fn what_is_between_user_code_lines_stays_and_is_said() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        "headers:\n  - inc/demo.h\n  - inc/demo_config.h\n",
        &[("inc/demo_config.h", CONFIG)],
    );
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    let config = project.join("demo/inc/demo_config.h");
    write(&config, &CONFIG.replace("DEMO_SIZE 8", "DEMO_SIZE 64"));

    let ui = installed(&project, &lib, "demo");

    assert!(read(&config).contains("DEMO_SIZE 64"));
    assert!(
        ui.out().contains("your USER CODE DEMO_CONFIGURATION"),
        "{}",
        ui.out()
    );
    assert!(ui.out().contains("This was an update."), "{}", ui.out());
    assert!(backups(&project.join("demo/inc"), "demo_config.h").is_empty());
}

#[test]
fn a_hand_edit_outside_the_sections_is_saved_before_it_goes() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");
    write(project.join("demo/src/demo.c"), "/* mine */\n");

    let ui = installed(&project, &lib, "demo");
    let saved = backups(&project.join("demo/src"), "demo.c");

    assert_eq!(read(project.join("demo/src/demo.c")), "/* source */\n");
    assert_eq!(saved.len(), 1);
    assert_eq!(read(&saved[0]), "/* mine */\n");
    assert!(
        ui.out()
            .contains("it was changed outside its USER CODE sections"),
        "{}",
        ui.out()
    );
    assert!(
        ui.out()
            .contains("Something of yours could not be carried over"),
        "{}",
        ui.out()
    );
}

#[test]
fn a_section_the_new_version_has_no_place_for_is_saved() {
    let root = tempfile::tempdir().unwrap();
    let first = version(
        root.path(),
        "v1",
        "demo",
        DEMO_CODE,
        &[(
            "src/demo.c",
            "/* USER CODE BEGIN OLD */\n/* USER CODE END OLD */\n",
        )],
    );
    let second = version(root.path(), "v2", "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &first, "demo");
    write(
        project.join("demo/src/demo.c"),
        "/* USER CODE BEGIN OLD */\nmine\n/* USER CODE END OLD */\n",
    );

    let ui = installed(&project, &second, "demo");

    assert!(
        ui.out().contains("the new version has no USER CODE OLD"),
        "{}",
        ui.out()
    );
    assert!(read(&backups(&project.join("demo/src"), "demo.c")[0]).contains("mine"));
}

#[test]
fn markers_the_user_broke_are_said_and_the_file_saved() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");
    write(
        project.join("demo/src/demo.c"),
        "/* USER CODE BEGIN A */\nmine\n",
    );

    let ui = installed(&project, &lib, "demo");

    assert!(
        ui.out().contains("its USER CODE markers do not pair"),
        "{}",
        ui.out()
    );
    assert_eq!(backups(&project.join("demo/src"), "demo.c").len(), 1);
}

#[test]
fn a_file_nobody_touched_is_left_as_it_is() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    let path = project.join("demo/src/demo.c");
    let old = fs::File::options().write(true).open(&path).unwrap();
    let long_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    old.set_modified(long_ago).unwrap();
    drop(old);

    installed(&project, &lib, "demo");

    let modified = fs::metadata(&path).unwrap().modified().unwrap();

    assert!(modified < std::time::SystemTime::now() - std::time::Duration::from_secs(1800));
}

#[test]
fn an_update_removes_what_the_new_version_no_longer_ships() {
    let root = tempfile::tempdir().unwrap();
    let first = version(
        root.path(),
        "v1",
        "demo",
        "headers: [demo.h]\nsources: [demo.c]\n",
        &[("demo.h", ""), ("demo.c", "")],
    );
    let second = version(root.path(), "v2", "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &first, "demo");
    write(project.join("demo/my_notes.txt"), "mine\n");

    let ui = installed(&project, &second, "demo");

    assert!(!project.join("demo/demo.h").exists());
    assert!(!project.join("demo/demo.c").exists());
    assert!(project.join("demo/src/demo.c").is_file());
    assert!(
        project.join("demo/my_notes.txt").is_file(),
        "a file the user added was removed"
    );
    assert!(
        ui.out()
            .contains("removed demo/demo.c  no longer part of the library"),
        "{}",
        ui.out()
    );

    let entry = &record(&project)["demo"];
    let files = strings(&entry["files"]);

    assert!(!files.contains(&"demo/demo.c".to_string()));
    assert!(files.contains(&"demo/src/demo.c".to_string()));
}

#[test]
fn a_file_the_new_version_drops_with_the_users_work_in_it_is_saved() {
    let root = tempfile::tempdir().unwrap();
    let first = version(
        root.path(),
        "v1",
        "demo",
        "headers: [inc/demo.h]\nsources: [src/old.c]\n",
        &[(
            "src/old.c",
            "/* USER CODE BEGIN S */\n/* USER CODE END S */\n",
        )],
    );
    let second = version(root.path(), "v2", "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &first, "demo");
    write(
        project.join("demo/src/old.c"),
        "/* USER CODE BEGIN S */\nmine\n/* USER CODE END S */\n",
    );

    let ui = installed(&project, &second, "demo");

    assert!(!project.join("demo/src/old.c").exists());
    assert!(read(&backups(&project.join("demo/src"), "old.c")[0]).contains("mine"));
    assert!(
        ui.out()
            .contains("no longer part of the library, and it held your changes"),
        "{}",
        ui.out()
    );
}

#[test]
fn installing_into_another_folder_leaves_the_first_copy_alone() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let other = version(root.path(), "v2", "demo", "headers: [inc/demo.h]\n", &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");
    installed(&project, &other, "Libs/demo");

    assert!(project.join("demo/src/demo.c").is_file());
}

#[test]
fn an_update_never_deletes_outside_the_library_folder() {
    let root = tempfile::tempdir().unwrap();
    let first = version(root.path(), "v1", "demo", DEMO_CODE, &[]);
    let second = version(root.path(), "v2", "demo", "headers: [inc/demo.h]\n", &[]);
    let project = stm32_project(root.path());
    let main = project.join("Core/Src/main.c");

    installed(&project, &first, "demo");

    // The record is a file anyone can edit. Whatever it lists, nothing outside the library's own folder goes.
    let path = project.join(".libnima.json");
    let mut data: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
    data["libraries"]["demo"]["files"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!("Core/Src/main.c"));
    fs::write(&path, data.to_string()).unwrap();

    installed(&project, &second, "demo");

    assert!(main.is_file(), "the user's main.c was deleted");
}

#[test]
fn a_folder_an_update_empties_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let first = version(root.path(), "v1", "demo", DEMO_CODE, &[]);
    let second = version(root.path(), "v2", "demo", "headers: [inc/demo.h]\n", &[]);
    let project = stm32_project(root.path());

    installed(&project, &first, "demo");
    assert!(project.join("demo/src").is_dir());

    installed(&project, &second, "demo");

    assert!(!project.join("demo/src").exists());
}

#[test]
fn the_folder_offered_is_the_one_it_is_already_in() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    let mut ui = Watched::default();
    assert_eq!(flow::run(&request(&project, &[&lib]), &mut ui), 0);

    assert!(ui.asked[0].contains("[Libs/demo]"), "{:?}", ui.asked);
    assert!(
        !project.join("demo").exists(),
        "a second copy was put beside the first"
    );
}

// ----------------------------------------------------------------------------
// A file the new version puts in another folder
// ----------------------------------------------------------------------------

fn config_at(root: &std::path::Path, tag: &str, place: &str) -> std::path::PathBuf {
    version(
        root,
        tag,
        "demo",
        &format!(
            "headers:\n  - inc/demo.h\n  - from: demo_config.h\n    to: {place}\nsources: [src/demo.c]\n"
        ),
        &[("demo_config.h", CONFIG)],
    )
}

#[test]
fn a_moved_file_takes_its_sections_with_it() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());

    installed(
        &project,
        &config_at(root.path(), "v1", "demo_config.h"),
        "demo",
    );

    let old = project.join("demo/demo_config.h");
    write(&old, &CONFIG.replace("DEMO_SIZE 8", "DEMO_SIZE 32"));

    let ui = installed(
        &project,
        &config_at(root.path(), "v2", "src/demo_config.h"),
        "demo",
    );

    assert!(read(project.join("demo/src/demo_config.h")).contains("DEMO_SIZE 32"));
    assert!(!old.exists());
    assert!(
        ui.out()
            .contains("demo/demo_config.h -> demo/src/demo_config.h"),
        "{}",
        ui.out()
    );
    assert!(
        ui.out().contains("DEMO_CONFIGURATION went with it"),
        "{}",
        ui.out()
    );
    assert!(
        !ui.out().contains("saved "),
        "a .bak for a setting that was carried: {}",
        ui.out()
    );
    assert!(
        !ui.out().contains("removed demo/demo_config.h"),
        "{}",
        ui.out()
    );
}

#[test]
fn a_hand_edit_outside_the_sections_of_a_moved_file_is_saved_once() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());

    installed(
        &project,
        &config_at(root.path(), "v1", "demo_config.h"),
        "demo",
    );

    let old = project.join("demo/demo_config.h");
    write(
        &old,
        &CONFIG
            .replace("DEMO_SIZE 8", "DEMO_SIZE 32")
            .replace("#endif", "#endif /* mine */"),
    );

    let ui = installed(
        &project,
        &config_at(root.path(), "v2", "src/demo_config.h"),
        "demo",
    );
    let saved = backups(&project.join("demo"), "demo_config.h");

    assert!(read(project.join("demo/src/demo_config.h")).contains("DEMO_SIZE 32"));
    assert_eq!(saved.len(), 1);
    assert!(read(&saved[0]).contains("/* mine */"));
    assert!(
        ui.out().contains("outside its USER CODE sections"),
        "{}",
        ui.out()
    );
}

#[test]
fn two_old_copies_of_one_name_are_not_a_move() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let first = version(
        root.path(),
        "v1",
        "demo",
        "headers:\n  - inc/demo.h\n  - a/demo_config.h\n  - b/demo_config.h\nsources: [src/demo.c]\n",
        &[("a/demo_config.h", CONFIG), ("b/demo_config.h", CONFIG)],
    );

    installed(&project, &first, "demo");

    for folder in ["a", "b"] {
        write(
            project.join(format!("demo/{folder}/demo_config.h")),
            &CONFIG.replace("DEMO_SIZE 8", "DEMO_SIZE 32"),
        );
    }

    let ui = installed(
        &project,
        &config_at(root.path(), "v2", "src/demo_config.h"),
        "demo",
    );

    assert!(read(project.join("demo/src/demo_config.h")).contains("DEMO_SIZE 8"));
    assert!(!ui.out().contains("went with it"), "{}", ui.out());
    assert_eq!(backups(&project.join("demo/a"), "demo_config.h").len(), 1);
    assert_eq!(backups(&project.join("demo/b"), "demo_config.h").len(), 1);
}

#[test]
fn a_file_still_installed_where_it_was_is_not_a_move() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());

    installed(
        &project,
        &config_at(root.path(), "v1", "demo_config.h"),
        "demo",
    );
    write(
        project.join("demo/demo_config.h"),
        &CONFIG.replace("DEMO_SIZE 8", "DEMO_SIZE 32"),
    );

    let second = version(
        root.path(),
        "v2",
        "demo",
        "headers:\n  - inc/demo.h\n  - demo_config.h\n  - from: demo_config.h\n    to: src/demo_config.h\nsources: [src/demo.c]\n",
        &[("demo_config.h", CONFIG)],
    );
    let ui = installed(&project, &second, "demo");

    assert!(read(project.join("demo/demo_config.h")).contains("DEMO_SIZE 32"));
    assert!(read(project.join("demo/src/demo_config.h")).contains("DEMO_SIZE 8"));
    assert!(!ui.out().contains("went with it"), "{}", ui.out());
}

// ----------------------------------------------------------------------------
// A folder already inside the project
// ----------------------------------------------------------------------------

#[test]
fn in_place_removes_the_repository_scaffolding() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = library(
        &project,
        "demo",
        "headers: [demo.h]\nsources: [demo.c]\nextras: [NOTICE, LICENSE.md]\n",
        &[
            ("demo.h", ""),
            ("demo.c", ""),
            ("LICENSE.md", "Apache\n"),
            ("test/test_demo.c", "int main(void){return 0;}\n"),
            (".github/ci.yml", "on: push\n"),
            ("CMakeLists.txt", "project(demo)\n"),
            ("my_notes.txt", "mine\n"),
        ],
    );

    let mut ui = Watched::default();
    assert_eq!(
        flow::run(&request(&project, &[&folder]), &mut ui),
        0,
        "{}",
        ui.err()
    );

    assert!(!folder.join("test").exists());
    assert!(!folder.join(".github").exists());
    assert!(!folder.join("CMakeLists.txt").exists());
    assert!(!folder.join("libnima.yml").exists());
    assert!(folder.join("LICENSE.md").is_file());
    assert!(folder.join("NOTICE").is_file());
    assert!(
        folder.join("my_notes.txt").is_file(),
        "a file the user added went"
    );
    assert!(
        ui.out()
            .contains("Removed, since they are part of the repository"),
        "{}",
        ui.out()
    );
    assert!(
        ui.asked.is_empty(),
        "no folder to ask about: {:?}",
        ui.asked
    );
}

#[test]
fn folders_the_files_stay_in_survive_the_cleanup() {
    // inc/ and src/ are on the cleanup list, so going by name alone would install four files and then delete all four.
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = library(
        &project,
        "big",
        "headers:\n  - inc/demo.h\n  - inc/port/spi.h\nsources:\n  - src/demo.c\n  - src/port/spi.c\n",
        &[
            ("inc/port/spi.h", ""),
            ("src/port/spi.c", ""),
            ("test/test_big.c", "int main(void){}\n"),
        ],
    );

    let mut ui = Watched::default();
    assert_eq!(
        flow::run(&request(&project, &[&folder]), &mut ui),
        0,
        "{}",
        ui.err()
    );

    assert!(folder.join("inc/demo.h").is_file());
    assert!(folder.join("inc/port/spi.h").is_file());
    assert!(folder.join("src/demo.c").is_file());
    assert!(folder.join("src/port/spi.c").is_file());
    assert!(!folder.join("test").exists());
    assert!(!folder.join("libnima.yml").exists());
}

#[test]
fn an_option_not_taken_goes_from_a_folder_in_place() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = library(
        &project,
        "demo",
        &format!(
            "{DEMO_CODE}options:\n  extra:\n    description: More\n    sources: [opt/extra.c]\n"
        ),
        &[("opt/extra.c", "")],
    );

    let mut ui = Watched::default();
    assert_eq!(
        flow::run(&request(&project, &[&folder]), &mut ui),
        0,
        "{}",
        ui.err()
    );

    assert!(!folder.join("opt/extra.c").exists());
    assert!(!folder.join("opt").exists());
}

#[test]
fn a_folder_installed_once_already_says_so() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = library(&project, "demo", DEMO_CODE, &[]);
    let manifest = read(folder.join("libnima.yml"));

    let mut ui = Watched::default();
    assert_eq!(
        flow::run(&request(&project, &[&folder]), &mut ui),
        0,
        "{}",
        ui.err()
    );

    // The file that says which platform is gone now, so the next run is told it. The record still knows the folder.
    let mut again = request(&project, &[&folder]);
    again.platform = Some(libnima_core::platform::Platform::Stm32);

    let mut ui = Watched::default();
    let code = flow::run(&again, &mut ui);

    assert_ne!(code, 0, "{}", ui.out());
    assert!(
        ui.err().contains("no libnima.yml") || ui.err().contains("holds no libnima.yml"),
        "{}",
        ui.err()
    );

    let _ = manifest;
}

// ----------------------------------------------------------------------------
// Files that become the project's own
// ----------------------------------------------------------------------------

const ONCE: &str = "once:\n  - project/VERSION\n  - project/CHANGELOG.md\n  - from: boards/x.overlay\n    to: socs/x_cpuapp.overlay\n";
const ONCE_FILES: [(&str, &str); 3] = [
    ("project/VERSION", "VERSION_MAJOR = 1\n"),
    ("project/CHANGELOG.md", "# Changelog\n"),
    ("boards/x.overlay", "/ { };\n"),
];

#[test]
fn once_files_land_in_the_project_folder() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}{ONCE}"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());

    let ui = installed(&project, &lib, "modules/demo");

    assert_eq!(read(project.join("project/VERSION")), "VERSION_MAJOR = 1\n");
    assert!(project.join("project/CHANGELOG.md").is_file());
    assert!(project.join("socs/x_cpuapp.overlay").is_file());
    assert!(!project.join("modules/demo/project/VERSION").exists());
    assert!(
        ui.out()
            .contains("created project/VERSION  yours from now on"),
        "{}",
        ui.out()
    );

    let entry = &record(&project)["demo"];

    assert_eq!(
        strings(&entry["project_files"]),
        [
            "project/CHANGELOG.md",
            "project/VERSION",
            "socs/x_cpuapp.overlay"
        ]
    );
    assert!(strings(&entry["once"]).is_empty());
}

#[test]
fn a_once_file_already_there_is_kept_as_it_is() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}{ONCE}"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());
    write(project.join("project/VERSION"), "VERSION_MAJOR = 7\n");

    let ui = installed(&project, &lib, "demo");

    assert_eq!(read(project.join("project/VERSION")), "VERSION_MAJOR = 7\n");
    assert!(
        ui.out().contains("kept    project/VERSION  yours"),
        "{}",
        ui.out()
    );
}

#[test]
fn an_update_never_replaces_a_once_file_the_user_edited() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}{ONCE}"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");
    write(project.join("socs/x_cpuapp.overlay"), "/* mine */\n");
    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("socs/x_cpuapp.overlay")), "/* mine */\n");
}

#[test]
fn a_once_file_the_new_version_drops_stays_and_leaves_the_record() {
    let root = tempfile::tempdir().unwrap();
    let first = version(
        root.path(),
        "v1",
        "demo",
        &format!("{DEMO_CODE}{ONCE}"),
        &ONCE_FILES,
    );
    let second = version(
        root.path(),
        "v2",
        "demo",
        &format!("{DEMO_CODE}once:\n  - project/VERSION\n"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());

    installed(&project, &first, "demo");
    installed(&project, &second, "demo");

    assert!(project.join("project/CHANGELOG.md").is_file());
    assert_eq!(
        strings(&record(&project)["demo"]["project_files"]),
        ["project/VERSION"]
    );
}

#[test]
fn a_c_file_among_once_files_is_not_built() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}once:\n  - port/port.c\n"),
        &[("port/port.c", "")],
    );
    let manifest =
        libnima_core::manifest::load(&lib, Some(libnima_core::platform::Platform::Stm32)).unwrap();

    assert_eq!(manifest.build_sources(), ["src/demo.c"]);
}

#[test]
fn the_record_is_never_a_once_file() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}once:\n  - from: project/VERSION\n    to: .libnima.json\n"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());

    let (code, ui) = install(&project, &lib, "demo");

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("edits or reads as the project's own"),
        "{}",
        ui.err()
    );
    assert!(!project.join("demo").exists());
}

#[test]
fn a_file_libnima_manages_is_never_a_once_file() {
    for to in [
        "CMakeLists.txt",
        "board/app.ioc",
        "Core/Inc/stm32g4xx_hal_conf.h",
    ] {
        let root = tempfile::tempdir().unwrap();
        let lib = library(
            root.path(),
            "demo",
            &format!("{DEMO_CODE}once:\n  - from: project/VERSION\n    to: {to}\n"),
            &ONCE_FILES,
        );
        let project = stm32_project(root.path());

        let (code, ui) = install(&project, &lib, "demo");

        assert_eq!(code, 2, "{to}");
        assert!(
            ui.err().contains("edits or reads as the project's own"),
            "{}",
            ui.err()
        );
        assert!(!project.join(to).exists());
    }
}

#[test]
fn a_once_file_inside_the_library_folder_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}once:\n  - from: project/VERSION\n    to: demo/VERSION\n"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());

    let (code, ui) = install(&project, &lib, "demo");

    assert_eq!(code, 2);
    assert!(ui.err().contains("inside its own folder"), "{}", ui.err());
    assert!(!project.join("demo").exists());
}

// ----------------------------------------------------------------------------
// The library's own files in the project's folder
// ----------------------------------------------------------------------------

#[test]
fn an_extra_with_a_root_lands_in_the_project_and_is_written_again() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        &[("tools/a.py", "new\n")],
    );
    let project = stm32_project(root.path());

    let ui = installed(&project, &lib, "demo");

    assert!(ui.out().contains("written tools/a.py"), "{}", ui.out());
    assert_eq!(strings(&record(&project)["demo"]["placed"]), ["tools/a.py"]);

    write(project.join("tools/a.py"), "changed\n");
    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("tools/a.py")), "new\n");
}

#[test]
fn a_file_of_the_users_is_never_written_over_by_an_extra() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        &[("tools/a.py", "new\n")],
    );
    let project = stm32_project(root.path());
    write(project.join("tools/a.py"), "mine\n");

    let (code, ui) = install(&project, &lib, "demo");

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("where a file of yours already is"),
        "{}",
        ui.err()
    );
    assert_eq!(read(project.join("tools/a.py")), "mine\n");
    assert!(!project.join("demo").exists());
}

#[test]
fn an_extra_the_new_version_no_longer_has_goes() {
    let root = tempfile::tempdir().unwrap();
    let first = version(
        root.path(),
        "v1",
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        &[("tools/a.py", ""), ("tools/b.py", "")],
    );
    let second = version(
        root.path(),
        "v2",
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools/a.py\n"),
        &[("tools/a.py", "")],
    );
    let project = stm32_project(root.path());

    installed(&project, &first, "demo");
    installed(&project, &second, "demo");

    assert!(!project.join("tools/b.py").exists());
    assert!(project.join("tools/a.py").is_file());
}

#[test]
fn an_uninstall_takes_the_extras_in_the_project_too() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        &[("tools/a.py", "")],
    );
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    assert_eq!(uninstall(&project, &["demo"]).0, 0);
    assert!(!project.join("tools").exists());
}

// ----------------------------------------------------------------------------
// Taking a library out
// ----------------------------------------------------------------------------

#[test]
fn the_files_the_record_and_the_folder_go() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    let (code, ui) = uninstall(&project, &["demo"]);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!project.join("Libs/demo").exists());
    assert!(project.join("Libs").is_dir());
    assert!(!record(&project).contains_key("demo"));
    assert!(
        ui.out().contains("removed Libs/demo/src/demo.c"),
        "{}",
        ui.out()
    );
    assert!(
        ui.out().contains("Done. demo is out of the project."),
        "{}",
        ui.out()
    );
}

#[test]
fn what_was_written_in_a_library_file_is_saved_first() {
    let header = "/* USER CODE BEGIN SETTINGS */\n/* USER CODE END SETTINGS */\nint demo(void);\n";
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[("inc/demo.h", header)]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    let path = project.join("Libs/demo/inc/demo.h");
    write(
        &path,
        &header.replace("SETTINGS */\n/*", "SETTINGS */\n#define MINE 1\n/*"),
    );

    let (code, ui) = uninstall(&project, &["demo"]);
    let saved = backups(&project.join("Libs/demo/inc"), "demo.h");

    assert_eq!(code, 0);
    assert!(!path.exists());
    assert_eq!(saved.len(), 1);
    assert!(read(&saved[0]).contains("#define MINE 1"));
    assert!(
        ui.out()
            .contains("it has something between its USER CODE lines"),
        "{}",
        ui.out()
    );
    assert!(
        ui.out().contains("stays, for what was saved above"),
        "{}",
        ui.out()
    );
}

#[test]
fn a_file_changed_outside_its_sections_is_saved_first() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");
    write(project.join("Libs/demo/src/demo.c"), "/* mine now */\n");

    let (code, ui) = uninstall(&project, &["demo"]);

    assert_eq!(code, 0);
    assert_eq!(
        read(&backups(&project.join("Libs/demo/src"), "demo.c")[0]),
        "/* mine now */\n"
    );
    assert!(
        ui.out()
            .contains("it was changed outside its USER CODE sections"),
        "{}",
        ui.out()
    );
}

#[test]
fn an_extra_that_shows_user_code_lines_is_not_taken_for_the_users() {
    let readme = "Set it:\n/* USER CODE BEGIN X */\n#define X 1\n/* USER CODE END X */\n";
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras: [README.md]\n"),
        &[("README.md", readme)],
    );
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    assert_eq!(uninstall(&project, &["demo"]).0, 0);
    assert!(!project.join("Libs/demo").exists());
}

#[test]
fn a_file_the_library_never_installed_stays_with_its_folder() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    let notes = project.join("Libs/demo/notes.txt");
    write(&notes, "mine\n");

    let (code, ui) = uninstall(&project, &["demo"]);

    assert_eq!(code, 0);
    assert_eq!(read(&notes), "mine\n");
    assert!(!project.join("Libs/demo/src/demo.c").exists());
    assert!(
        ui.out().contains("kept    Libs/demo/notes.txt"),
        "{}",
        ui.out()
    );
    assert!(ui.out().contains("Libs/demo stays"), "{}", ui.out());
}

#[test]
fn a_record_naming_a_file_outside_the_folder_deletes_nothing_there() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    let outside = project.join("main.c");
    write(&outside, "int main(void) { return 0; }\n");

    let path = project.join(".libnima.json");
    let mut data: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();

    for wrong in [
        "main.c".to_string(),
        "Libs/demo/../../main.c".to_string(),
        outside.to_string_lossy().into_owned(),
    ] {
        data["libraries"]["demo"]["files"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!(wrong));
    }

    fs::write(&path, data.to_string()).unwrap();

    assert_eq!(uninstall(&project, &["demo"]).0, 0);
    assert!(outside.is_file());
}

#[test]
fn once_files_stay_through_an_uninstall_and_it_says_so() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}{ONCE}"),
        &ONCE_FILES,
    );
    let project = stm32_project(root.path());

    installed(&project, &lib, "demo");

    let (code, ui) = uninstall(&project, &["demo"]);

    assert_eq!(code, 0);
    assert!(project.join("project/VERSION").is_file());
    assert!(project.join("socs/x_cpuapp.overlay").is_file());
    assert!(!project.join("demo").exists());
    assert!(
        ui.out().contains("project/VERSION  yours, not removed"),
        "{}",
        ui.out()
    );
}

fn with_needs(root: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let holder = root.join("src");
    let osal = library(&holder, "osal", "headers: [osal.h]\n", &[("osal.h", "")]);
    let ee24 = library(
        &holder,
        "ee24",
        "requires:\n  libraries: [osal]\nheaders: [ee24.h]\nsources: [ee24.c]\n",
        &[("ee24.h", ""), ("ee24.c", "")],
    );

    (ee24, osal)
}

fn install_both(project: &std::path::Path, ee24: &std::path::Path, osal: &std::path::Path) {
    let mut ui = Watched::default();

    assert_eq!(
        flow::run(&request(project, &[ee24, osal]), &mut ui),
        0,
        "{}",
        ui.err()
    );
}

#[test]
fn what_a_library_needs_is_recorded() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let (ee24, osal) = with_needs(root.path());

    install_both(&project, &ee24, &osal);

    assert_eq!(strings(&record(&project)["ee24"]["needs"]), ["osal"]);
    assert!(strings(&record(&project)["osal"]["needs"]).is_empty());
}

#[test]
fn a_library_another_needs_stays() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let (ee24, osal) = with_needs(root.path());

    install_both(&project, &ee24, &osal);

    let (code, ui) = uninstall(&project, &["osal"]);

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("ee24 needs osal, so it stays"),
        "{}",
        ui.err()
    );
    assert!(project.join("osal/osal.h").is_file());
    assert!(record(&project).contains_key("osal"));
}

#[test]
fn both_on_one_line_go_in_the_right_order() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let (ee24, osal) = with_needs(root.path());

    install_both(&project, &ee24, &osal);

    assert_eq!(uninstall(&project, &["osal", "ee24"]).0, 0);
    assert!(record(&project).is_empty());
    assert!(!project.join("osal").exists());
    assert!(!project.join("ee24").exists());
}

#[test]
fn a_library_nothing_needs_any_more_is_named() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let (ee24, osal) = with_needs(root.path());

    install_both(&project, &ee24, &osal);

    let (code, ui) = uninstall(&project, &["ee24"]);

    assert_eq!(code, 0);
    assert!(
        ui.out()
            .contains("osal came in for ee24, and nothing here needs it now"),
        "{}",
        ui.out()
    );
    assert!(
        ui.out().contains("libnima --uninstall osal"),
        "{}",
        ui.out()
    );
    assert!(record(&project).contains_key("osal"));
}

#[test]
fn a_library_not_installed_is_refused_naming_what_is() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());

    installed(&project, &lib, "Libs/demo");

    let (code, ui) = uninstall(&project, &["dmeo"]);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("dmeo is not installed in this project. .libnima.json has demo."),
        "{}",
        ui.err()
    );
}

#[test]
fn uninstall_and_install_are_not_mixed() {
    let root = tempfile::tempdir().unwrap();
    let lib = library(root.path(), "demo", DEMO_CODE, &[]);
    let project = stm32_project(root.path());
    let mut asked = request(&project, &[&lib]);
    asked.uninstall = vec!["demo".to_string()];
    asked.platform = Some(libnima_core::platform::Platform::Stm32);

    let mut ui = Watched::default();

    assert_eq!(flow::run(&asked, &mut ui), 2);
    assert!(
        ui.err().contains("Install on a run of its own"),
        "{}",
        ui.err()
    );
}

#[test]
fn nothing_to_say_which_platform_an_uninstall_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let asked = libnima_core::flow::Request {
        uninstall: vec!["demo".to_string()],
        project: Some(root.path().to_path_buf()),
        ..Default::default()
    };
    let mut ui = Watched::default();

    assert_eq!(flow::run(&asked, &mut ui), 2);
    assert!(
        ui.err().contains("Nothing here says which platform"),
        "{}",
        ui.err()
    );
}

#[test]
fn a_define_another_library_asks_for_stays_in_the_project() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            makefile: true,
            ..Ides::default()
        },
    );
    let holder = root.path().join("src");
    let first = library(
        &holder,
        "first",
        "defines: [SHARED, FIRST]\nheaders: [first.h]\n",
        &[("first.h", "")],
    );
    let second = library(
        &holder,
        "second",
        "defines: [SHARED]\nheaders: [second.h]\n",
        &[("second.h", "")],
    );

    let mut ui = Watched::default();
    assert_eq!(
        flow::run(&request(&project, &[&first, &second]), &mut ui),
        0,
        "{}",
        ui.err()
    );
    assert!(read(project.join("Makefile")).contains("-DSHARED"));
    assert!(read(project.join("Makefile")).contains("-DFIRST"));

    assert_eq!(uninstall(&project, &["first"]).0, 0);

    let makefile = read(project.join("Makefile"));

    assert!(makefile.contains("-DSHARED"), "{makefile}");
    assert!(!makefile.contains("-DFIRST"), "{makefile}");
}

// ----------------------------------------------------------------------------
// What the user wrote, past the first update
// ----------------------------------------------------------------------------

const CONFIG_V1: &str = "/* demo_config.h, version 1 */\n/* USER CODE BEGIN DEMO_CONFIGURATION */\n#define DEMO_SIZE 8\n/* USER CODE END DEMO_CONFIGURATION */\n";
const CONFIG_V2: &str = "/* demo_config.h, version 2 */\n/* USER CODE BEGIN DEMO_CONFIGURATION */\n#define DEMO_SIZE 16\n/* USER CODE END DEMO_CONFIGURATION */\n/* USER CODE BEGIN DEMO_EXTRA */\n/* USER CODE END DEMO_EXTRA */\n";

fn with_config(root: &std::path::Path, tag: &str, config: &str) -> std::path::PathBuf {
    version(
        root,
        tag,
        "demo",
        "headers:\n  - inc/demo.h\n  - inc/demo_config.h\nsources:\n  - src/demo.c\n",
        &[("inc/demo_config.h", config)],
    )
}

#[test]
fn a_config_that_was_once_the_users_keeps_its_section_and_is_saved_when_it_differs() {
    // The step a library takes when it drops once: its config becomes an ordinary file.
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());

    installed(
        &project,
        &with_config(root.path(), "old", CONFIG_V1),
        "demo",
    );

    // As lib-installer left it, for a library whose config was then copied once into its own folder.
    let path = project.join(".libnima.json");
    let mut written: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
    let entry = &mut written["libraries"]["demo"];

    entry["fingerprints"]
        .as_object_mut()
        .unwrap()
        .remove("demo/inc/demo_config.h");
    entry["once"] = serde_json::json!(["demo/inc/demo_config.h"]);
    write(&path, &written.to_string());

    let config = project.join("demo/inc/demo_config.h");
    write(
        &config,
        &format!(
            "{}#define MINE 1\n",
            CONFIG_V1.replace("DEMO_SIZE 8", "DEMO_SIZE 32")
        ),
    );

    let ui = installed(&project, &with_config(root.path(), "v2", CONFIG_V2), "demo");

    assert!(
        read(&config).contains("#define DEMO_SIZE 32"),
        "{}",
        read(&config)
    );
    assert!(ui.out().contains("yours to edit until now"), "{}", ui.out());

    let saved = backups(&project.join("demo/inc"), "demo_config.h");

    assert_eq!(saved.len(), 1);
    assert!(read(&saved[0]).contains("#define MINE 1"));
}

#[test]
fn without_a_fingerprint_in_the_record_code_is_replaced_and_nothing_is_saved() {
    // A project last installed before the record kept them. Code was always replaced then too.
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = with_config(root.path(), "v1", CONFIG_V1);

    installed(&project, &lib, "demo");

    let path = project.join(".libnima.json");
    let mut written: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();

    written["libraries"]["demo"]
        .as_object_mut()
        .unwrap()
        .remove("fingerprints");
    write(&path, &written.to_string());
    write(project.join("demo/inc/demo.h"), "/* edited */\n");

    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("demo/inc/demo.h")), "/* header */\n");
    assert!(backups(&project.join("demo/inc"), "demo.h").is_empty());
}

#[test]
fn the_record_keeps_a_fingerprint_of_every_file_written() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());

    installed(&project, &with_config(root.path(), "v1", CONFIG_V1), "demo");

    let entry = &record(&project)["demo"]["fingerprints"];
    let mut names: Vec<&String> = entry.as_object().unwrap().keys().collect();

    names.sort();

    assert_eq!(
        names,
        [
            "demo/inc/demo.h",
            "demo/inc/demo_config.h",
            "demo/src/demo.c"
        ]
    );
    assert_eq!(
        entry["demo/inc/demo_config.h"],
        libnima_core::usercode::fingerprint(CONFIG_V1.as_bytes())
    );
}

#[test]
fn a_readme_that_shows_the_markers_installs_and_is_copied_as_it_is() {
    // A README can show one example twice, or mention a marker in a sentence.
    let example = "```c\n/* USER CODE BEGIN DEMO_CONFIGURATION */\n#define DEMO_SIZE 8\n/* USER CODE END DEMO_CONFIGURATION */\n```\n";
    let readme = format!(
        "Keep it between `USER CODE BEGIN DEMO_CONFIGURATION` and its END.\n{example}{example}"
    );
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras:\n  - README.md\n"),
        &[("README.md", &readme)],
    );

    installed(&project, &lib, "demo");
    write(project.join("demo/README.md"), "my notes\n");

    let ui = installed(&project, &lib, "demo");

    assert_eq!(read(project.join("demo/README.md")), readme);
    assert!(backups(&project.join("demo"), "README.md").is_empty());
    assert!(!ui.out().contains("your USER CODE"), "{}", ui.out());
}

#[test]
fn the_report_says_what_was_kept_and_what_was_saved() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = with_config(root.path(), "v1", CONFIG_V1);

    installed(&project, &lib, "demo");
    write(
        project.join("demo/inc/demo_config.h"),
        &CONFIG_V1.replace("DEMO_SIZE 8", "DEMO_SIZE 32"),
    );
    write(project.join("demo/inc/demo.h"), "/* edited */\n");

    let out = installed(&project, &lib, "demo").out();

    assert!(out.contains("your USER CODE DEMO_CONFIGURATION"), "{out}");
    assert!(
        out.contains("saved") && out.contains("demo.h.") && out.contains("changed outside"),
        "{out}"
    );
    assert!(out.contains("This was an update"), "{out}");
}

// ----------------------------------------------------------------------------
// Places in the project
// ----------------------------------------------------------------------------

#[test]
fn a_file_lands_at_its_root_and_a_wildcard_keeps_the_names() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}extras:\n  - from: tools/keygen.md\n    root: doc/keys.md\n  - from: tools/*.py\n    root: scripts\n"
        ),
        &[
            ("tools/keygen.md", "# keygen\n"),
            ("tools/keygen.py", "# py\n"),
        ],
    );

    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("doc/keys.md")), "# keygen\n");
    assert!(project.join("scripts/keygen.py").is_file());
}

#[test]
fn the_same_file_already_there_is_taken_over() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    write(project.join("tools/keygen.py"), "# py\n");
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        &[("tools/keygen.py", "# py\n")],
    );

    installed(&project, &lib, "demo");

    assert!(strings(&record(&project)["demo"]["placed"]).contains(&"tools/keygen.py".to_string()));
}

#[test]
fn an_extra_needs_no_from_when_it_is_its_root() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        &[("tools/keygen.py", ""), ("tools/keygen.md", "")],
    );

    installed(&project, &lib, "demo");

    assert!(project.join("tools/keygen.py").is_file());
    assert_eq!(
        strings(&record(&project)["demo"]["placed"]),
        ["tools/keygen.md", "tools/keygen.py"]
    );
}

#[test]
fn an_extras_to_is_a_place_in_the_library_folder() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}extras:\n  - from: tools/keygen.md\n    to: doc/keys.md\n  - to: tools\n"
        ),
        &[("tools/keygen.md", "# keygen\n"), ("tools/keygen.py", "")],
    );

    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("demo/doc/keys.md")), "# keygen\n");
    assert!(project.join("demo/tools/keygen.py").is_file());
    assert!(!project.join("doc").exists() && !project.join("tools").exists());
    assert!(strings(&record(&project)["demo"]["placed"]).is_empty());
}

#[test]
fn a_once_file_lands_at_its_own_path_in_the_project() {
    for entry in ["from: boards/chip.overlay", "to: boards/chip.overlay"] {
        let root = tempfile::tempdir().unwrap();
        let project = stm32_project(root.path());
        let lib = library(
            root.path(),
            "demo",
            &format!("{DEMO_CODE}once:\n  - {entry}\n"),
            &[("boards/chip.overlay", "/ { };\n")],
        );

        installed(&project, &lib, "demo");

        assert!(project.join("boards/chip.overlay").is_file(), "{entry}");
        assert!(!project.join("chip.overlay").exists(), "{entry}");
    }
}

#[test]
fn a_once_file_may_be_its_path_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}once:\n  - boards/chip.overlay\n"),
        &[("boards/chip.overlay", "/ { };\n")],
    );

    installed(&project, &lib, "demo");

    assert!(project.join("boards/chip.overlay").is_file());
}

#[test]
fn a_once_files_to_is_its_place_in_the_project() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let lib = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}once:\n  - from: templates/CHANGELOG.md\n    to: CHANGELOG.md\n"),
        &[("templates/CHANGELOG.md", "# Changelog\n")],
    );

    installed(&project, &lib, "demo");

    assert_eq!(read(project.join("CHANGELOG.md")), "# Changelog\n");
    assert!(!project.join("templates").exists());
    assert!(!project.join("demo/CHANGELOG.md").exists());
}
