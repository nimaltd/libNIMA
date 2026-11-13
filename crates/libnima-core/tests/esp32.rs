//! A library in an ESP-IDF project: a component of its own, and what the esp32 section of its libnima.yml says goes where.
//!
//! Under project, the project's own files: conf, the library's block in sdkconfig.defaults; and cmake, its blocks in the
//! project's CMakeLists.txt, under before-project and after-project. Under library, its component: requires, the
//! components its code uses; and cmake, the lines of the component's CMakeLists.txt.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Watched, backups, put, read, record, run_with, write};
use libnima_core::flow::{self, Request};
use libnima_core::manifest::{self, Manifest};
use libnima_core::platform::esp32::{component, project as prj};
use libnima_core::platform::{Platform, PlatformArgs, target_of};
use libnima_core::target::{Level, Status};

const STARS: &str =
    "# ******************************************************************************";
const CMAKELISTS: &str = "cmake_minimum_required(VERSION 3.16)\n\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\nproject(app)\n";
const MAIN: &str =
    "idf_component_register(SRCS \"main.c\"\n                    INCLUDE_DIRS \".\")\n";
const MINIMAL: &str = "cmake_minimum_required(VERSION 3.22)\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\nidf_build_set_property(MINIMAL_BUILD ON)\nproject(app)\n";

const OPEN: &str = "# >>> libnima: demo >>>\n# Added by libnima, and written again on every install, so set\n# demo's symbols below this block rather than in it.\n";
const CLOSE: &str = "# <<< libnima: demo <<<\n";
const FROM: &str = "# From demo's libnima.yml, and written again on every install.\n";

const GROUPED: &str = "      A faster tick:\n        - FREERTOS_HZ=1000: one tick a millisecond\n      Quiet logs:\n        - LOG_DEFAULT_LEVEL_WARN\n        - LOG_COLORS=n: plain text, for a file\n";

const FILES: [&str; 3] = [
    "CMakeLists.txt",
    "sdkconfig.defaults",
    "main/CMakeLists.txt",
];

/// An ESP-IDF project, as a test states it.
#[derive(Clone, Copy)]
struct App<'a> {
    cmake: &'a str,
    main: Option<&'a str>,
    defaults: Option<&'a str>,
    sdkconfig: Option<&'a str>,
    crlf: bool,
}

impl Default for App<'_> {
    fn default() -> Self {
        App {
            cmake: CMAKELISTS,
            main: Some(MAIN),
            defaults: None,
            sdkconfig: None,
            crlf: false,
        }
    }
}

impl App<'_> {
    fn on(self, root: &Path) -> PathBuf {
        let project = root.join("app");
        let mut files: Vec<(&str, &str)> = vec![
            ("CMakeLists.txt", self.cmake),
            ("main/main.c", "void app_main(void) { }\n"),
        ];

        if let Some(main) = self.main {
            files.push(("main/CMakeLists.txt", main));
        }

        if let Some(defaults) = self.defaults {
            files.push(("sdkconfig.defaults", defaults));
        }

        if let Some(sdkconfig) = self.sdkconfig {
            files.push(("sdkconfig", sdkconfig));
        }

        for (name, text) in files {
            write(
                project.join(name),
                &if self.crlf {
                    text.replace('\n', "\r\n")
                } else {
                    text.to_string()
                },
            );
        }

        project
    }
}

fn plain_app(root: &Path) -> PathBuf {
    App::default().on(root)
}

fn offline() -> libnima_core::download::Client {
    common::Github::serving(&[]).client()
}

/// A library in a folder of its own, with what its libnima.yml says around its name, its code and its platform.
fn lib_at(root: &Path, name: &str, body: &str, files: &[(&str, &str)]) -> PathBuf {
    let folder = root.join(name);

    put(
        &folder,
        &[
            (
                &format!("src/{name}.h"),
                &format!("/**\n * @version 1.2.0\n */\nint {name}_answer(void);\n"),
            ),
            (
                &format!("src/{name}.c"),
                "int answer(void) { return 42; }\n",
            ),
        ],
    );
    put(&folder, files);
    fs::write(
        folder.join("libnima.yml"),
        format!(
            "name: {name}\ndescription: A library for the tests\nrepository: https://github.com/someone/{name}\nplatform: esp32\nheaders:\n  - src/{name}.h\nsources:\n  - src/{name}.c\n{body}"
        ),
    )
    .unwrap();

    folder
}

/// demo, with what its esp32 section holds.
fn demo(root: &Path, esp32: &str) -> PathBuf {
    if esp32.is_empty() {
        lib_at(root, "demo", "", &[])
    } else {
        lib_at(root, "demo", &format!("esp32:\n{esp32}"), &[])
    }
}

fn read_it(folder: &Path) -> Result<Manifest, String> {
    manifest::load(folder, Some(Platform::Esp32)).map_err(|error| error.message)
}

fn refused(esp32: &str) -> String {
    let root = tempfile::tempdir().unwrap();

    read_it(&demo(root.path(), esp32)).unwrap_err()
}

fn loaded(esp32: &str) -> Manifest {
    let root = tempfile::tempdir().unwrap();

    read_it(&demo(root.path(), esp32)).unwrap()
}

fn asked(project: &Path, library: &Path, folder: Option<&str>) -> Request {
    let mut asked = common::request(project, &[library]);

    asked.folder = folder.map(str::to_string);

    asked
}

fn run(asked: &Request) -> (i32, Watched) {
    run_with(offline(), asked)
}

fn install_in(project: &Path, library: &Path, folder: Option<&str>) -> Watched {
    let (code, ui) = run(&asked(project, library, folder));

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());

    ui
}

fn install(project: &Path, library: &Path) -> Watched {
    install_in(project, library, None)
}

fn uninstall(project: &Path, name: &str) -> Watched {
    let (code, ui) = run(&Request {
        uninstall: vec![name.to_string()],
        project: Some(project.to_path_buf()),
        ..Request::default()
    });

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());

    ui
}

fn component_of(project: &Path) -> String {
    read(project.join("components/demo/CMakeLists.txt"))
}

fn snapshot(project: &Path, names: &[&str]) -> Vec<String> {
    names.iter().map(|name| read(project.join(name))).collect()
}

fn all_backups(project: &Path) -> Vec<String> {
    libnima_core::paths::files_below(project)
        .into_iter()
        .filter(|path| path.ends_with(".bak"))
        .collect()
}

fn position(text: &str, wanted: &str) -> usize {
    text.find(wanted)
        .unwrap_or_else(|| panic!("{wanted} is not in:\n{text}"))
}

fn main_of(project: &Path) -> String {
    read(project.join("main/CMakeLists.txt"))
}

fn says(ui: &Watched, wanted: &str) {
    assert!(
        ui.out().contains(wanted),
        "{wanted} is not in:\n{}",
        ui.out()
    );
}

// ----------------------------------------------------------------------------
// The project's CMakeLists.txt
// ----------------------------------------------------------------------------

#[test]
fn a_project_is_known_by_its_include_of_project_cmake() {
    let root = tempfile::tempdir().unwrap();

    assert!(prj::find_project(&plain_app(root.path())).is_some());

    let other = root.path().join("other");

    write(
        other.join("CMakeLists.txt"),
        "# include($ENV{IDF_PATH}/tools/cmake/project.cmake)\n",
    );

    assert!(prj::find_project(&other).is_none());
}

#[test]
fn a_library_in_components_needs_no_cmake_block() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(&project, &demo(root.path(), ""));

    assert_eq!(read(project.join("CMakeLists.txt")), CMAKELISTS);
}

#[test]
fn a_library_elsewhere_is_added_above_include() {
    for crlf in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let project = App {
            crlf,
            ..App::default()
        }
        .on(root.path());
        let library = demo(root.path(), "");
        let newline = if crlf { "\r\n" } else { "\n" };

        install_in(&project, &library, Some("libs/demo"));

        let text = read(project.join("CMakeLists.txt"));

        assert!(
            text.contains(&format!(
                "list(APPEND EXTRA_COMPONENT_DIRS \"${{CMAKE_CURRENT_SOURCE_DIR}}/libs/demo\"){newline}# <<< libnima: demo <<<{newline}{newline}include($ENV{{IDF_PATH}}"
            )),
            "{text}"
        );
        assert_eq!(
            text.matches("\r\n").count(),
            if crlf { text.matches('\n').count() } else { 0 }
        );

        let ui = install_in(&project, &library, Some("libs/demo"));

        assert_eq!(read(project.join("CMakeLists.txt")), text);
        says(&ui, "CMakeLists.txt already has demo");
    }
}

#[test]
fn a_block_below_include_moves_above_it() {
    let root = tempfile::tempdir().unwrap();
    let late = "include($ENV{IDF_PATH}/tools/cmake/project.cmake)\n# >>> libnima: demo >>>\nSTALE_LINE\n# <<< libnima: demo <<<\nproject(app)\n";
    let project = App {
        cmake: late,
        ..App::default()
    }
    .on(root.path());

    install_in(&project, &demo(root.path(), ""), Some("libs/demo"));

    let text = read(project.join("CMakeLists.txt"));

    assert!(position(&text, "EXTRA_COMPONENT_DIRS") < position(&text, "include("));
    assert!(!text.contains("STALE_LINE"), "{text}");
    assert_eq!(text.matches(">>> libnima: demo").count(), 1, "{text}");
}

#[test]
fn a_library_moved_into_components_loses_its_block() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let library = demo(root.path(), "");

    install_in(&project, &library, Some("libs/demo"));

    // --dir alone would leave the first copy behind, so the move is an uninstall and an install.
    let (code, ui) = run(&asked(&project, &library, Some("components/demo")));

    assert_eq!(code, 2, "{}", ui.out());
    assert!(
        ui.err().contains("libnima --uninstall demo"),
        "{}",
        ui.err()
    );
    assert!(!project.join("components").exists());

    uninstall(&project, "demo");
    install_in(&project, &library, Some("components/demo"));

    assert_eq!(read(project.join("CMakeLists.txt")), CMAKELISTS);
}

#[test]
fn a_half_written_block_is_left_for_the_user() {
    let root = tempfile::tempdir().unwrap();
    let broken = "# >>> libnima: demo >>>\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\n";
    let project = App {
        cmake: broken,
        ..App::default()
    }
    .on(root.path());
    let (_, ui) = run(&asked(&project, &demo(root.path(), ""), Some("libs/demo")));

    says(&ui, "half written");
    assert_eq!(read(project.join("CMakeLists.txt")), broken);
}

#[test]
fn with_no_project_nothing_is_installed() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("empty");

    fs::create_dir_all(&project).unwrap();

    let (code, ui) = run(&asked(&project, &demo(root.path(), ""), Some("libs/demo")));

    assert_eq!(code, 2, "{}", ui.out());
    assert!(
        ui.err().contains("is not an ESP-IDF project"),
        "{}",
        ui.err()
    );
    assert!(fs::read_dir(&project).unwrap().next().is_none());
}

#[test]
fn a_folder_name_with_library_is_where_dir_put_it_and_the_components_folder() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let library = demo(
        root.path(),
        "  project:\n    cmake:\n      before-project:\n        - 'list(APPEND EXTRA_COMPONENT_DIRS \"{library}/extra\")'\n  library:\n    cmake:\n      - 'message(STATUS \"{library}\")'\n",
    );

    install_in(&project, &library, Some("libs/demo"));

    let text = read(project.join("CMakeLists.txt"));

    assert!(
        text.contains("# >>> libnima: demo/before-project >>>"),
        "{text}"
    );
    assert!(
        text.contains(
            "list(APPEND EXTRA_COMPONENT_DIRS \"${CMAKE_CURRENT_SOURCE_DIR}/libs/demo/extra\")"
        ),
        "{text}"
    );
    assert!(
        read(project.join("libs/demo/CMakeLists.txt"))
            .contains("message(STATUS \"${COMPONENT_DIR}\")")
    );
}

// ----------------------------------------------------------------------------
// sdkconfig.defaults
// ----------------------------------------------------------------------------

fn conf(lines: &[&str]) -> String {
    let listed: String = lines
        .iter()
        .map(|line| format!("      - {line}\n"))
        .collect();

    format!("  project:\n    conf:\n{listed}")
}

#[test]
fn a_library_that_needs_no_symbol_adds_no_block() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(&project, &demo(root.path(), ""));

    assert!(!project.join("sdkconfig.defaults").exists());
}

#[test]
fn project_conf_goes_into_sdkconfig_defaults() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(
            root.path(),
            &conf(&["FREERTOS_HZ=1000", "SPI_MASTER_IN_IRAM"]),
        ),
    );

    assert_eq!(
        read(project.join("sdkconfig.defaults")),
        format!("{OPEN}CONFIG_FREERTOS_HZ=1000\nCONFIG_SPI_MASTER_IN_IRAM=y\n{CLOSE}")
    );
}

#[test]
fn the_symbols_go_at_the_end_after_a_blank_line() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_LOG_DEFAULT_LEVEL_INFO=y\n"),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), &conf(&["FREERTOS_HZ=1000"])));

    assert_eq!(
        read(project.join("sdkconfig.defaults")),
        format!("CONFIG_LOG_DEFAULT_LEVEL_INFO=y\n\n{OPEN}CONFIG_FREERTOS_HZ=1000\n{CLOSE}")
    );
}

#[test]
fn a_symbol_the_user_sets_is_left_commented() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_FREERTOS_HZ=100\n# CONFIG_X is not set\n"),
        ..App::default()
    }
    .on(root.path());

    install(
        &project,
        &demo(root.path(), &conf(&["FREERTOS_HZ=1000", "X"])),
    );

    let text = read(project.join("sdkconfig.defaults"));

    assert!(
        text.contains(
            "# CONFIG_FREERTOS_HZ=1000  (left out: sdkconfig.defaults sets CONFIG_FREERTOS_HZ=100)"
        ),
        "{text}"
    );
    assert!(
        text.contains("# CONFIG_X=y  (left out: sdkconfig.defaults sets CONFIG_X=n)"),
        "{text}"
    );
}

#[test]
fn a_value_sdkconfig_holds_is_named_for_menuconfig() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        sdkconfig: Some("CONFIG_FREERTOS_HZ=100\nCONFIG_OTHER=y\n"),
        ..App::default()
    }
    .on(root.path());
    let ui = install(
        &project,
        &demo(root.path(), &conf(&["FREERTOS_HZ=1000", "OTHER"])),
    );

    says(
        &ui,
        "sdkconfig has CONFIG_FREERTOS_HZ=100, which is what ESP-IDF builds with. Set it to 1000 in idf.py menuconfig.",
    );
    assert!(
        !ui.out().contains("Set it to y in idf.py menuconfig"),
        "{}",
        ui.out()
    );
}

#[test]
fn the_block_of_a_library_that_needs_nothing_now_comes_out() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_A=y\n"),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), &conf(&["B"])));
    install(&project, &demo(root.path(), ""));

    assert_eq!(read(project.join("sdkconfig.defaults")), "CONFIG_A=y\n");
}

#[test]
fn crlf_is_kept_in_sdkconfig_defaults() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_A=y\n"),
        crlf: true,
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), &conf(&["B"])));

    assert!(
        !read(project.join("sdkconfig.defaults"))
            .replace("\r\n", "")
            .contains('\n')
    );
}

#[test]
fn titles_and_notes_in_the_block_of_sdkconfig_defaults() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(root.path(), &format!("  project:\n    conf:\n{GROUPED}")),
    );

    assert_eq!(
        read(project.join("sdkconfig.defaults")),
        format!(
            "{OPEN}\n{STARS}\n# A faster tick\n{STARS}\n# one tick a millisecond\nCONFIG_FREERTOS_HZ=1000\n\n{STARS}\n# Quiet logs\n{STARS}\nCONFIG_LOG_DEFAULT_LEVEL_WARN=y\n# plain text, for a file\nCONFIG_LOG_COLORS=n\n{CLOSE}"
        )
    );
}

#[test]
fn a_line_the_user_sets_keeps_its_note_and_its_place_in_its_group() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_FREERTOS_HZ=100\n"),
        ..App::default()
    }
    .on(root.path());

    install(
        &project,
        &demo(
            root.path(),
            "  project:\n    conf:\n      A faster tick:\n        - FREERTOS_HZ=1000: one tick a millisecond\n        - FREERTOS_USE_TRACE_FACILITY\n",
        ),
    );

    assert!(read(project.join("sdkconfig.defaults")).contains(&format!(
        "# A faster tick\n{STARS}\n# one tick a millisecond\n# CONFIG_FREERTOS_HZ=1000  (left out: sdkconfig.defaults sets CONFIG_FREERTOS_HZ=100)\nCONFIG_FREERTOS_USE_TRACE_FACILITY=y\n{CLOSE}"
    )));
}

#[test]
fn project_cmake_goes_into_the_projects_cmakelists_by_its_place() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(
            root.path(),
            "  project:\n    cmake:\n      before-project:\n        Its own defaults:\n          - 'list(APPEND SDKCONFIG_DEFAULTS \"{library}/demo.defaults\")'\n      after-project:\n        - 'message(STATUS \"after\")'\n",
        ),
    );

    assert_eq!(
        read(project.join("CMakeLists.txt")),
        format!(
            "cmake_minimum_required(VERSION 3.16)\n\n# >>> libnima: demo/before-project >>>\n{FROM}{STARS}\n# Its own defaults\n{STARS}\nlist(APPEND SDKCONFIG_DEFAULTS \"${{CMAKE_CURRENT_SOURCE_DIR}}/components/demo/demo.defaults\")\n# <<< libnima: demo/before-project <<<\n\ninclude($ENV{{IDF_PATH}}/tools/cmake/project.cmake)\nproject(app)\n\n# >>> libnima: demo/after-project >>>\n{FROM}message(STATUS \"after\")\n# <<< libnima: demo/after-project <<<\n"
        )
    );
}

#[test]
fn an_update_keeps_every_librarys_cmake_blocks_where_they_are() {
    // Each block went right above include(project.cmake), or to the end, on every install, below the other libraries'
    // ones, so the order followed the last install, and an install that changed nothing still left a .bak.
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let cmake_of = |name: &str| {
        format!(
            "esp32:\n  project:\n    cmake:\n      before-project:\n        - set({name}_ON 1)\n      after-project:\n        - 'message(STATUS \"{name}\")'\n"
        )
    };
    let first = lib_at(root.path(), "demo", &cmake_of("demo"), &[]);
    let second = lib_at(root.path(), "other", &cmake_of("other"), &[]);

    install(&project, &first);
    install(&project, &second);

    let text = read(project.join("CMakeLists.txt"));
    let saved = backups(&project, "CMakeLists.txt");

    install(&project, &first);

    assert_eq!(read(project.join("CMakeLists.txt")), text);
    assert_eq!(backups(&project, "CMakeLists.txt"), saved);

    for place in ["before-project", "after-project"] {
        assert!(
            text.find(&format!("libnima: demo/{place} >>>"))
                < text.find(&format!("libnima: other/{place} >>>")),
            "{text}"
        );
    }
}

#[test]
fn a_block_of_after_project_above_project_goes_below_it() {
    // The project's targets exist from project() on, not from include(project.cmake).
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let library = demo(
        root.path(),
        "  project:\n    cmake:\n      after-project:\n        - 'message(STATUS \"after\")'\n",
    );
    let block = format!(
        "# >>> libnima: demo/after-project >>>\n{FROM}message(STATUS \"after\")\n# <<< libnima: demo/after-project <<<\n"
    );

    install(&project, &library);

    let path = project.join("CMakeLists.txt");
    let text = read(&path);

    assert!(
        text.ends_with(&format!("project(app)\n\n{block}")),
        "{text}"
    );

    write(
        &path,
        &text.replacen(&format!("\n{block}"), "", 1).replacen(
            "project(app)\n",
            &format!("{block}project(app)\n"),
            1,
        ),
    );
    install(&project, &library);

    assert_eq!(read(&path), text);
}

// ----------------------------------------------------------------------------
// main/CMakeLists.txt
// ----------------------------------------------------------------------------

#[test]
fn main_with_no_list_of_its_own_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(&project, &demo(root.path(), ""));

    assert_eq!(main_of(&project), MAIN);
}

#[test]
fn main_with_a_list_gets_the_component() {
    for (main, expected) in [
        (
            "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES esp_timer)\n",
            "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES demo esp_timer)\n",
        ),
        (
            "idf_component_register(SRCS \"main.c\" REQUIRES driver)\n",
            "idf_component_register(SRCS \"main.c\" REQUIRES demo driver)\n",
        ),
        (
            "idf_component_register(SRCS \"main.c\"\n    REQUIRES driver\n    PRIV_REQUIRES nvs_flash)\n",
            "idf_component_register(SRCS \"main.c\"\n    REQUIRES driver\n    PRIV_REQUIRES demo nvs_flash)\n",
        ),
        (
            "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES)\n",
            "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES demo)\n",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = App {
            main: Some(main),
            ..App::default()
        }
        .on(root.path());
        let library = demo(root.path(), "");

        install(&project, &library);
        assert_eq!(main_of(&project), expected);

        install(&project, &library);
        assert_eq!(main_of(&project), expected, "the second install changed it");
    }
}

#[test]
fn a_word_in_a_comment_or_a_string_is_not_a_keyword() {
    let root = tempfile::tempdir().unwrap();
    let main = "idf_component_register(SRCS \"REQUIRES.c\" # REQUIRES nothing\n    INCLUDE_DIRS \"$<IF:1,(a),b>\")\n";
    let project = App {
        main: Some(main),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));

    assert_eq!(main_of(&project), main);
}

#[test]
fn a_name_already_listed_in_quotes_is_not_added_again() {
    let root = tempfile::tempdir().unwrap();
    let main = "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES \"demo\")\n";
    let project = App {
        main: Some(main),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));

    assert_eq!(main_of(&project), main);
}

#[test]
fn main_that_cannot_be_read_is_left_with_steps() {
    let root = tempfile::tempdir().unwrap();
    let main = "idf_component_register(SRCS main.c\n";
    let project = App {
        main: Some(main),
        ..App::default()
    }
    .on(root.path());
    let ui = install(&project, &demo(root.path(), ""));

    assert_eq!(main_of(&project), main);
    says(&ui, "main/CMakeLists.txt");
}

#[test]
fn the_component_comes_out_of_main_again() {
    let root = tempfile::tempdir().unwrap();
    let main = "idf_component_register(SRCS \"main.c\"\n    PRIV_REQUIRES\n        demo\n        esp_timer\n    REQUIRES demo)\n";
    let project = App {
        main: Some(main),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));
    uninstall(&project, "demo");

    assert_eq!(
        main_of(&project),
        "idf_component_register(SRCS \"main.c\"\n    PRIV_REQUIRES\n        esp_timer\n)\n"
    );
}

#[test]
fn a_parenthesis_inside_an_argument_does_not_close_the_call() {
    let root = tempfile::tempdir().unwrap();
    let main = "idf_component_register(SRCS \"main.c\" INCLUDE_DIRS $<IF:1,(a),b>\n    PRIV_REQUIRES esp_timer)\n";
    let project = App {
        main: Some(main),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));

    assert!(
        main_of(&project).contains("PRIV_REQUIRES demo esp_timer)"),
        "{}",
        main_of(&project)
    );
}

#[test]
fn a_minimal_build_gives_main_a_list_of_its_own() {
    for cmake in [
        MINIMAL,
        "include($ENV{IDF_PATH}/tools/cmake/project.cmake)\nset(COMPONENTS main)\nproject(app)\n",
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = App {
            cmake,
            ..App::default()
        }
        .on(root.path());
        let library = demo(root.path(), "");
        let ui = install(&project, &library);
        let wanted = "idf_component_register(SRCS \"main.c\"\n                    INCLUDE_DIRS \".\" PRIV_REQUIRES demo)\n";

        says(&ui, "builds only the components main names");
        assert_eq!(main_of(&project), wanted);

        install(&project, &library);
        assert_eq!(main_of(&project), wanted);
    }
}

#[test]
fn a_list_added_for_a_minimal_build_comes_out_whole() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        cmake: MINIMAL,
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));
    uninstall(&project, "demo");

    assert_eq!(main_of(&project), MAIN);
}

#[test]
fn minimal_build_off_is_not_minimal() {
    let root = tempfile::tempdir().unwrap();
    let cmake = "include($ENV{IDF_PATH}/tools/cmake/project.cmake)\nidf_build_set_property(MINIMAL_BUILD OFF)\n# set(COMPONENTS main)\nproject(app)\n";
    let project = App {
        cmake,
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));

    assert_eq!(main_of(&project), MAIN);
}

#[test]
fn a_main_with_no_arguments_gets_its_list_inside_the_call() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        cmake: MINIMAL,
        main: Some("idf_component_register()\n"),
        ..App::default()
    }
    .on(root.path());

    install(&project, &demo(root.path(), ""));

    assert_eq!(
        main_of(&project),
        "idf_component_register( PRIV_REQUIRES demo)\n"
    );
}

// ----------------------------------------------------------------------------
// The component
// ----------------------------------------------------------------------------

fn written_component(library: &Path, root: &Path, requires: &[&str]) -> (PathBuf, String) {
    let manifest = read_it(library).unwrap();
    let folder = root.join("components").join(&manifest.name);
    let requires: Vec<String> = requires.iter().map(ToString::to_string).collect();

    fs::create_dir_all(&folder).unwrap();
    component::write(&manifest, &folder, &requires);

    let text = read(folder.join("CMakeLists.txt"));

    (folder, text)
}

const ONE_SETTING: &str = "  settings:\n    - {name: DEMO_N, type: int, default: 1, prompt: N}\n";

#[test]
fn the_component_lists_its_sources_includes_and_requires() {
    let root = tempfile::tempdir().unwrap();
    let (_, text) = written_component(&demo(root.path(), ""), root.path(), &["esp_timer", "osal"]);

    assert!(text.starts_with("# Generated by libnima"), "{text}");
    assert!(text.contains("SRCS\n        \"src/demo.c\"\n"), "{text}");
    assert!(text.contains("INCLUDE_DIRS\n        \"src\"\n"), "{text}");
    assert!(
        text.contains("REQUIRES\n        esp_timer\n        osal\n)"),
        "{text}"
    );
    assert!(
        text.contains("# USER CODE BEGIN DEMO_CMAKE\n# USER CODE END DEMO_CMAKE\n"),
        "{text}"
    );
}

fn header_only(root: &Path, more: &str) -> PathBuf {
    let folder = root.join("headers");

    put(&folder, &[("src/demo.h", "")]);
    fs::write(
        folder.join("libnima.yml"),
        format!("name: demo\nplatform: esp32\nheaders:\n  - src/demo.h\n{more}"),
    )
    .unwrap();

    folder
}

#[test]
fn a_library_of_headers_only_has_no_srcs_and_interface_defines() {
    let root = tempfile::tempdir().unwrap();
    let library = header_only(root.path(), "defines:\n  - DEMO_FAST\n  - DEMO_N=2\n");
    let (_, text) = written_component(&library, root.path(), &[]);

    assert!(
        !text.contains("SRCS") && !text.contains("REQUIRES"),
        "{text}"
    );
    assert!(
        text.contains(
            "target_compile_definitions(${COMPONENT_LIB} INTERFACE\n    DEMO_FAST\n    DEMO_N=2\n)"
        ),
        "{text}"
    );
    assert!(
        text.contains("target_compile_options(${COMPONENT_LIB} INTERFACE -O2)"),
        "{text}"
    );
}

#[test]
fn a_library_with_sources_gives_its_defines_to_every_user() {
    let root = tempfile::tempdir().unwrap();
    let library = lib_at(root.path(), "demo", "defines:\n  - DEMO_FAST\n", &[]);
    let (_, text) = written_component(&library, root.path(), &[]);

    assert!(
        text.contains("target_compile_definitions(${COMPONENT_LIB} PUBLIC\n    DEMO_FAST\n)"),
        "{text}"
    );
}

#[test]
fn a_file_at_the_top_is_found_from_the_component() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("top");

    put(&folder, &[("demo.h", ""), ("demo.c", "")]);
    fs::write(
        folder.join("libnima.yml"),
        "name: demo\nplatform: esp32\nheaders:\n  - demo.h\nsources:\n  - demo.c\n",
    )
    .unwrap();

    let (_, text) = written_component(&folder, root.path(), &[]);

    assert!(text.contains("INCLUDE_DIRS\n        \".\"\n"), "{text}");
    assert!(text.contains("SRCS\n        \"demo.c\"\n"), "{text}");
}

#[test]
fn an_odd_file_name_stays_one_argument() {
    // Such a name cannot even be made on Windows, so it is quoted on its own.
    assert_eq!(
        libnima_core::platform::kconfig::cmake_quote("src/a \"b\";$c.c"),
        "\"src/a \\\"b\\\"\\;\\$c.c\""
    );
}

#[test]
fn library_requires_goes_into_the_components_requires() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(
            root.path(),
            "  library:\n    requires:\n      - esp_timer\n      - esp_driver_i2c\n      - esp_timer\n",
        ),
    );

    assert!(
        component_of(&project)
            .contains("    REQUIRES\n        esp_timer\n        esp_driver_i2c\n)\n"),
        "{}",
        component_of(&project)
    );
}

#[test]
fn library_cmake_goes_into_the_components_cmakelists() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(
            root.path(),
            "  library:\n    cmake:\n      Math:\n        - target_link_libraries(${COMPONENT_LIB} PRIVATE m)\n      Its private headers:\n        - 'target_include_directories(${COMPONENT_LIB} PRIVATE \"{library}/private\")'\n",
        ),
    );

    let text = component_of(&project);

    assert!(text.contains(&format!(
        "# >>> libnima: demo/component >>>\n{FROM}{STARS}\n# Math\n{STARS}\ntarget_link_libraries(${{COMPONENT_LIB}} PRIVATE m)\n\n{STARS}\n# Its private headers\n{STARS}\ntarget_include_directories(${{COMPONENT_LIB}} PRIVATE \"${{COMPONENT_DIR}}/private\")\n# <<< libnima: demo/component <<<\n"
    )), "{text}");
    assert!(position(&text, ")\n") < position(&text, "PRIVATE m)"));
    assert!(position(&text, "PRIVATE m)") < position(&text, "USER CODE BEGIN"));
}

#[test]
fn no_settings_means_no_kconfig() {
    let root = tempfile::tempdir().unwrap();
    let (folder, _) = written_component(&demo(root.path(), ""), root.path(), &[]);

    assert!(!folder.join("Kconfig").exists());
}

#[test]
fn the_kconfig_is_a_menu_of_the_settings() {
    let root = tempfile::tempdir().unwrap();
    let library = demo(
        root.path(),
        "  settings:\n    - name: DEMO_SIZE\n      type: int\n      default: 8\n      range: [1, 64]\n      prompt: 'Size \"in\" bytes'\n      help: |\n        How big.\n\n        In bytes.\n    - {name: DEMO_MASK, type: hex, default: '0x10', range: ['0x0', '0xFF'], prompt: Mask}\n",
    );
    let (folder, _) = written_component(&library, root.path(), &[]);
    let text = read(folder.join("Kconfig"));

    assert!(text.starts_with("# Generated by libnima"), "{text}");
    assert!(
        text.contains("menu \"demo: A library for the tests\""),
        "{text}"
    );
    assert!(text.contains("    config DEMO_SIZE\n        int \"Size \\\"in\\\" bytes\"\n        default 8\n        range 1 64\n"), "{text}");
    assert!(
        text.contains("        help\n            How big.\n\n            In bytes.\n"),
        "{text}"
    );
    assert!(text.contains("        range 0x0 0xff\n"), "{text}");
    assert!(text.ends_with("endmenu\n"), "{text}");
}

#[test]
fn both_files_are_written_then_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let manifest = read_it(&demo(root.path(), ONE_SETTING)).unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();

    let first = component::write(&manifest, &folder, &[]);
    let second = component::write(&manifest, &folder, &[]);

    assert_eq!(first.status, Status::Changed);
    assert!(
        first.message.contains("wrote CMakeLists.txt, Kconfig"),
        "{}",
        first.message
    );
    assert_eq!(second.status, Status::Already);
    assert!(folder.join("Kconfig").is_file());
}

#[test]
fn a_kconfig_for_settings_that_went_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();
    component::write(
        &read_it(&demo(root.path(), ONE_SETTING)).unwrap(),
        &folder,
        &[],
    );

    let outcome = component::write(&read_it(&demo(root.path(), "")).unwrap(), &folder, &[]);

    assert!(!folder.join("Kconfig").exists());
    assert!(
        outcome.message.contains("removed its Kconfig"),
        "{}",
        outcome.message
    );
}

#[test]
fn a_kconfig_of_the_librarys_own_is_never_removed() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("components/demo");
    let own = "menu \"mine\"\nendmenu\n";

    write(folder.join("Kconfig"), own);
    component::write(&read_it(&demo(root.path(), "")).unwrap(), &folder, &[]);

    assert_eq!(read(folder.join("Kconfig")), own);
}

#[test]
fn the_users_lines_are_carried_into_the_new_file() {
    let root = tempfile::tempdir().unwrap();
    let manifest = read_it(&demo(root.path(), "")).unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();
    component::write(&manifest, &folder, &[]);

    let path = folder.join("CMakeLists.txt");

    write(
        &path,
        &read(&path).replace(
            "# USER CODE BEGIN DEMO_CMAKE\n",
            "# USER CODE BEGIN DEMO_CMAKE\nmessage(STATUS mine)\n",
        ),
    );

    let outcome = component::write(&manifest, &folder, &["esp_timer".to_string()]);

    assert!(read(&path).contains("message(STATUS mine)"));
    assert!(read(&path).contains("esp_timer"));
    assert!(
        outcome
            .steps
            .join(" ")
            .contains("Kept your USER CODE DEMO_CMAKE"),
        "{:?}",
        outcome.steps
    );
}

#[test]
fn broken_markers_put_the_old_file_aside() {
    let root = tempfile::tempdir().unwrap();
    let manifest = read_it(&demo(root.path(), "")).unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();
    component::write(&manifest, &folder, &[]);

    let path = folder.join("CMakeLists.txt");

    write(
        &path,
        &read(&path).replace("# USER CODE END DEMO_CMAKE\n", "mine\n"),
    );

    let outcome = component::write(&manifest, &folder, &["esp_timer".to_string()]);

    assert!(read(outcome.backup.expect("nothing was saved")).contains("mine"));
    assert!(read(&path).contains("# USER CODE END DEMO_CMAKE"));
}

#[test]
fn a_library_with_its_own_cmakelists_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("components/demo");
    let own = "idf_component_register(SRCS mine.c)\n";

    write(folder.join("CMakeLists.txt"), own);

    let outcome = component::write(&read_it(&demo(root.path(), "")).unwrap(), &folder, &[]);

    assert_eq!(outcome.status, Status::Already);
    assert_eq!(read(folder.join("CMakeLists.txt")), own);
    assert!(!folder.join("Kconfig").exists());
}

#[test]
fn remove_takes_only_what_write_made() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();
    component::write(
        &read_it(&demo(root.path(), ONE_SETTING)).unwrap(),
        &folder,
        &[],
    );

    assert_eq!(component::symbols(&folder), ["DEMO_N"]);
    assert_eq!(component::remove(&folder), None);
    assert!(!folder.join("CMakeLists.txt").exists() && !folder.join("Kconfig").exists());
}

#[test]
fn remove_keeps_a_copy_of_the_users_lines() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();
    component::write(&read_it(&demo(root.path(), "")).unwrap(), &folder, &[]);

    let path = folder.join("CMakeLists.txt");

    write(
        &path,
        &read(&path).replace(
            "# USER CODE BEGIN DEMO_CMAKE\n",
            "# USER CODE BEGIN DEMO_CMAKE\nmine()\n",
        ),
    );

    let saved = component::remove(&folder).expect("nothing was saved");

    assert!(read(saved).contains("mine()"));
    assert!(!path.exists());
}

#[test]
fn remove_leaves_a_kconfig_of_the_librarys_own() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("components/demo");

    fs::create_dir_all(&folder).unwrap();
    component::write(&read_it(&demo(root.path(), "")).unwrap(), &folder, &[]);
    write(
        folder.join("Kconfig"),
        "menu \"mine\"\n    config MINE\n        bool \"Mine\"\nendmenu\n",
    );

    assert!(component::symbols(&folder).is_empty());

    component::remove(&folder);

    assert!(!folder.join("CMakeLists.txt").exists());
    assert!(folder.join("Kconfig").is_file());
}

// ----------------------------------------------------------------------------
// What the esp32 section says
// ----------------------------------------------------------------------------

#[test]
fn a_library_with_no_esp32_part_has_nothing_extra() {
    let found = loaded("");
    let part = found.part.esp32();

    assert!(
        part.requires.is_empty()
            && part.settings.is_empty()
            && part.cmake.is_empty()
            && part.conf.is_empty()
    );
}

#[test]
fn project_conf_takes_a_symbol_or_a_value() {
    let found = loaded(&conf(&[
        "FREERTOS_HZ=1000",
        "CONFIG_ESP_TIMER_SUPPORTS_ISR_DISPATCH_METHOD",
        "'X=\"text\"'",
    ]));
    let lines: Vec<String> = found
        .part
        .esp32()
        .conf
        .iter()
        .map(|one| one.item.line())
        .collect();

    assert_eq!(
        lines,
        [
            "CONFIG_FREERTOS_HZ=1000",
            "CONFIG_ESP_TIMER_SUPPORTS_ISR_DISPATCH_METHOD=y",
            "CONFIG_X=\"text\""
        ]
    );
}

#[test]
fn project_conf_refuses_what_cannot_be_written_safely() {
    for bad in [
        "lower",
        "A B",
        "X=two words",
        "\"X=y\\nCONFIG_Y=y\"",
        "'X=\"un\"closed\"'",
    ] {
        let message = refused(&conf(&[bad]));

        assert!(message.contains("esp32.project.conf"), "{bad}: {message}");
    }
}

#[test]
fn library_requires_lists_idf_components_once() {
    let found = loaded("  library:\n    requires: [esp_timer, esp-tls, esp_timer]\n");

    assert_eq!(found.part.esp32().requires, ["esp_timer", "esp-tls"]);
}

#[test]
fn library_requires_refuses_what_is_not_a_component_name() {
    for bad in ["esp timer", "a;b", "'${X}'", "'\"q\"'"] {
        let message = refused(&format!("  library:\n    requires:\n      - {bad}\n"));

        assert!(
            message.contains("esp32.library.requires"),
            "{bad}: {message}"
        );
    }
}

#[test]
fn settings_of_every_type() {
    let found = loaded(
        "  settings:\n    - {name: DEMO_ON, type: bool, default: true, prompt: On}\n    - {name: DEMO_SIZE, type: int, default: 8, range: [1, 64], prompt: Size}\n    - {name: DEMO_MASK, type: hex, default: '0x0F', prompt: Mask}\n    - name: DEMO_TEXT\n      type: string\n      default: 'say \"hi\"'\n      prompt: Text\n",
    );
    let lines: Vec<String> = found
        .part
        .esp32()
        .settings
        .iter()
        .map(|one| one.line())
        .collect();

    assert_eq!(
        lines,
        [
            "CONFIG_DEMO_ON=y",
            "CONFIG_DEMO_SIZE=8",
            "CONFIG_DEMO_MASK=0xf",
            "CONFIG_DEMO_TEXT=\"say \\\"hi\\\"\""
        ]
    );
}

#[test]
fn a_setting_that_cannot_build_is_refused() {
    for (setting, says) in [
        ("{name: DEMO_X, type: float, default: 1}", "type"),
        ("{name: DEMO_X, type: int}", "no default"),
        (
            "{name: DEMO_X, type: int, default: 99, range: [1, 10]}",
            "outside its range",
        ),
        (
            "{name: DEMO_X, type: int, default: 5, range: [10, 1]}",
            "starts above",
        ),
        (
            "{name: DEMO_X, type: bool, default: maybe}",
            "true or false",
        ),
        (
            "{name: DEMO_X, type: string, default: \"two\\nlines\"}",
            "one line",
        ),
        (
            "{name: DEMO_X, type: bool, default: true, range: [0, 1]}",
            "only an int or a hex",
        ),
        ("{name: lower, type: int, default: 1}", "capitals"),
    ] {
        let message = refused(&format!("  settings:\n    - {setting}\n"));

        assert!(message.contains(says), "{setting}: {message}");
    }
}

#[test]
fn a_setting_named_twice_is_refused() {
    let one = "    - {name: DEMO_X, type: int, default: 1}\n";

    assert!(refused(&format!("  settings:\n{one}{one}")).contains("more than once"));
}

#[test]
fn a_setting_without_the_prefix_or_a_prompt_is_warned_about() {
    let found = loaded("  settings:\n    - {name: OTHER, type: int, default: 1}\n");

    assert!(
        found
            .warnings
            .iter()
            .any(|one| one.contains("does not start with DEMO_")),
        "{:?}",
        found.warnings
    );
    assert!(
        found
            .warnings
            .iter()
            .any(|one| one.contains("has no prompt")),
        "{:?}",
        found.warnings
    );
}

#[test]
fn the_keys_from_before_are_refused_with_where_they_went() {
    for (esp32, says) in [
        (
            "  requires:\n    - esp_timer\n",
            "esp32.requires is esp32.library.requires",
        ),
        (
            "  cmake:\n    component:\n      - a()\n",
            "esp32.cmake is esp32.project.cmake and esp32.library.cmake",
        ),
        (
            "  setings: []\n",
            "esp32 has setings. It holds settings, project, library.",
        ),
    ] {
        let message = refused(esp32);

        assert!(message.contains(says), "{esp32}: {message}");
    }
}

#[test]
fn requires_kconfig_is_refused_with_where_it_went() {
    let root = tempfile::tempdir().unwrap();
    let message = read_it(&lib_at(
        root.path(),
        "demo",
        "requires:\n  kconfig:\n    - FREERTOS_HZ=1000\n",
        &[],
    ))
    .unwrap_err();

    assert!(message.contains("esp32.project.conf"), "{message}");
}

#[test]
fn what_a_part_does_not_hold_is_refused() {
    for (esp32, says) in [
        (
            "  project:\n    requires:\n      - esp_timer\n",
            "esp32.project has requires. It holds conf, cmake.",
        ),
        (
            "  library:\n    conf:\n      - FREERTOS_HZ=1000\n",
            "esp32.library has conf. It holds requires, cmake.",
        ),
        (
            "  project:\n    cmake:\n      component:\n        - a()\n",
            "esp32.project.cmake has component. It holds before-project, after-project.",
        ),
        (
            "  project:\n    - FREERTOS_HZ=1000\n",
            "esp32.project holds conf, cmake",
        ),
    ] {
        let message = refused(esp32);

        assert!(message.contains(says), "{esp32}: {message}");
    }
}

#[test]
fn what_cannot_be_written_is_refused() {
    for (esp32, says) in [
        (
            "  project:\n    conf:\n      - freertos_hz\n",
            "esp32.project.conf has 'freertos_hz'. Write a Kconfig symbol",
        ),
        (
            "  project:\n    conf:\n      - FREERTOS_HZ=fast\n",
            "esp32.project.conf has 'FREERTOS_HZ=fast'. Give a symbol",
        ),
        (
            "  project:\n    conf:\n      A:\n        - FREERTOS_HZ=1000\n      B:\n        - CONFIG_FREERTOS_HZ=100\n",
            "esp32.project.conf has FREERTOS_HZ more than once",
        ),
        (
            "  library:\n    requires:\n      Timers:\n        - esp_timer\n",
            "esp32.library.requires is a list of ESP-IDF components",
        ),
        (
            "  library:\n    requires:\n      - esp_timer: for the time\n",
            "with no titles or notes",
        ),
        (
            "  library:\n    requires:\n      - esp timer\n",
            "esp32.library.requires has 'esp timer'",
        ),
    ] {
        let message = refused(esp32);

        assert!(message.contains(says), "{esp32}: {message}");
    }
}

#[test]
fn titles_and_notes_that_cannot_be_written_are_refused() {
    for (esp32, says) in [
        (
            "    conf:\n      \"Two\\nlines\":\n        - FREERTOS_HZ=1000\n",
            "Give it one line of text",
        ),
        (
            "    conf:\n      \">>> libnima: x >>>\":\n        - FREERTOS_HZ=1000\n",
            "what libNIMA marks its blocks with",
        ),
        (
            "    conf:\n      - FREERTOS_HZ=1000: CONFIG_FREERTOS_HZ is not set\n",
            "Kconfig would read as a setting",
        ),
        (
            "    conf:\n      Empty: []\n",
            "has the title 'Empty' with no list under it",
        ),
    ] {
        let message = refused(&format!("  project:\n{esp32}"));

        assert!(message.contains(says), "{esp32}: {message}");
    }
}

// ----------------------------------------------------------------------------
// Every file written
// ----------------------------------------------------------------------------

#[test]
fn a_plain_list_has_no_titles() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(
            root.path(),
            "  project:\n    conf:\n      - FREERTOS_HZ=1000\n    cmake:\n      after-project:\n        - a()\n  library:\n    cmake:\n      - b()\n",
        ),
    );

    for name in [
        "sdkconfig.defaults",
        "CMakeLists.txt",
        "components/demo/CMakeLists.txt",
    ] {
        assert!(!read(project.join(name)).contains("*****"), "{name}");
    }
}

#[test]
fn a_second_install_with_titles_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let library = demo(
        root.path(),
        &format!(
            "  project:\n    conf:\n{GROUPED}    cmake:\n      before-project:\n        First:\n          - a()\n  library:\n    requires:\n      - esp_timer\n    cmake:\n      Second:\n        - b()\n"
        ),
    );
    let names = [
        "sdkconfig.defaults",
        "CMakeLists.txt",
        "components/demo/CMakeLists.txt",
    ];

    install(&project, &library);

    let before = snapshot(&project, &names);
    let saved = all_backups(&project);

    install(&project, &library);

    assert_eq!(snapshot(&project, &names), before);
    assert_eq!(all_backups(&project), saved);
}

#[test]
fn a_second_install_of_a_plain_library_changes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        main: Some("idf_component_register(SRCS \"main.c\" PRIV_REQUIRES esp_timer)\n"),
        ..App::default()
    }
    .on(root.path());
    let library = demo(root.path(), &conf(&["FREERTOS_HZ=1000"]));
    let names = [
        "CMakeLists.txt",
        "sdkconfig.defaults",
        "main/CMakeLists.txt",
        "components/demo/CMakeLists.txt",
    ];

    install(&project, &library);

    let first = snapshot(&project, &names);

    install(&project, &library);

    assert_eq!(snapshot(&project, &names), first);
}

#[test]
fn an_uninstall_gives_the_project_back_as_it_was() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_FREERTOS_HZ=100\n"),
        ..App::default()
    }
    .on(root.path());
    let names = ["sdkconfig.defaults", "CMakeLists.txt"];
    let before = snapshot(&project, &names);

    install(
        &project,
        &demo(
            root.path(),
            &format!(
                "  project:\n    conf:\n{GROUPED}    cmake:\n      before-project:\n        First:\n          - a()\n      after-project:\n        - b()\n"
            ),
        ),
    );
    uninstall(&project, "demo");

    assert_eq!(snapshot(&project, &names), before);
    assert!(!project.join("components/demo").exists());
}

// ----------------------------------------------------------------------------
// Options
// ----------------------------------------------------------------------------

#[test]
fn an_options_cmake_is_under_project_and_library() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let source = lib_at(
        root.path(),
        "demo",
        "options:\n  fast:\n    description: Built for speed\n    esp32:\n      project:\n        cmake:\n          after-project:\n            - 'message(STATUS \"fast\")'\n      library:\n        cmake:\n          Speed:\n            - target_compile_options(${COMPONENT_LIB} PRIVATE -O2)\n",
        &[],
    );
    let mut request = asked(&project, &source, None);

    request.with_options = vec!["fast".to_string()];
    assert_eq!(run(&request).0, 0);

    assert!(read(project.join("CMakeLists.txt")).contains("demo/fast-after-project >>>"));
    assert!(component_of(&project).contains(&format!(
        "demo/fast-component >>>\n{FROM}{STARS}\n# Speed\n"
    )));

    let mut request = asked(&project, &source, None);

    request.without_options = vec!["fast".to_string()];
    assert_eq!(run(&request).0, 0);

    assert!(!read(project.join("CMakeLists.txt")).contains("demo/fast"));
    assert!(!component_of(&project).contains("demo/fast"));
}

#[test]
fn an_options_cmake_in_the_wrong_place_is_refused() {
    for (esp32, says) in [
        (
            "      cmake:\n        component:\n          - a()\n",
            "options.fast.esp32.cmake is options.fast.esp32.project.cmake and options.fast.esp32.library.cmake",
        ),
        (
            "      project:\n        conf:\n          - LOG_COLORS=n\n",
            "options.fast.esp32.project has conf. It holds cmake.",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let library = lib_at(
            root.path(),
            "demo",
            &format!("options:\n  fast:\n    description: x\n    esp32:\n{esp32}"),
            &[],
        );
        let message = read_it(&library).unwrap_err();

        assert!(message.contains(says), "{esp32}: {message}");
    }
}

// ----------------------------------------------------------------------------
// SDKCONFIG_DEFAULTS
// ----------------------------------------------------------------------------

const EXTRA_BLOCK: &str = "# >>> libnima: extra/defaults >>>\n# From extra's libnima.yml, and written again on every install.\nif(NOT SDKCONFIG_DEFAULTS AND EXISTS \"${CMAKE_CURRENT_SOURCE_DIR}/sdkconfig.defaults\")\n    set(SDKCONFIG_DEFAULTS \"${CMAKE_CURRENT_SOURCE_DIR}/sdkconfig.defaults\")\nendif()\nlist(APPEND SDKCONFIG_DEFAULTS \"${CMAKE_CURRENT_SOURCE_DIR}/extra.defaults\")\n# <<< libnima: extra/defaults <<<\n";

fn project_cmake(middle: &str) -> String {
    format!(
        "cmake_minimum_required(VERSION 3.16)\n\n{middle}\ninclude($ENV{{IDF_PATH}}/tools/cmake/project.cmake)\nproject(app)\n"
    )
}

#[test]
fn a_librarys_block_setting_sdkconfig_defaults_is_not_the_projects_line() {
    let root = tempfile::tempdir().unwrap();
    let cmake = project_cmake(EXTRA_BLOCK);

    assert!(!prj::names_its_defaults(
        &App {
            cmake: &cmake,
            ..App::default()
        }
        .on(root.path())
    ));
}

#[test]
fn the_projects_own_line_setting_sdkconfig_defaults_still_counts() {
    let root = tempfile::tempdir().unwrap();
    let cmake = project_cmake(&format!(
        "set(SDKCONFIG_DEFAULTS \"sdkconfig.defaults;sdkconfig.release\")\n{EXTRA_BLOCK}"
    ));

    assert!(prj::names_its_defaults(
        &App {
            cmake: &cmake,
            ..App::default()
        }
        .on(root.path())
    ));
}

#[test]
fn installing_again_after_a_block_like_that_gives_no_warning_about_sdkconfig_defaults() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let extra = lib_at(
        root.path(),
        "extra",
        "esp32:\n  project:\n    cmake:\n      before-project:\n        - 'if(NOT SDKCONFIG_DEFAULTS AND EXISTS \"${CMAKE_CURRENT_SOURCE_DIR}/sdkconfig.defaults\")'\n        - '    set(SDKCONFIG_DEFAULTS \"${CMAKE_CURRENT_SOURCE_DIR}/sdkconfig.defaults\")'\n        - endif()\n",
        &[],
    );
    let library = demo(root.path(), &conf(&["FREERTOS_HZ=1000"]));

    install(&project, &extra);
    install(&project, &library);

    assert!(
        !install(&project, &library)
            .out()
            .contains("sets SDKCONFIG_DEFAULTS")
    );
}

// ----------------------------------------------------------------------------
// A whole install
// ----------------------------------------------------------------------------

#[test]
fn a_library_becomes_a_component() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let ui = install(
        &project,
        &demo(
            root.path(),
            "  project:\n    conf:\n      - FREERTOS_HZ=1000\n  library:\n    requires:\n      - esp_timer\n",
        ),
    );

    assert!(project.join("components/demo/src/demo.h").is_file());
    assert!(component_of(&project).contains("esp_timer"));
    assert!(read(project.join("sdkconfig.defaults")).contains("CONFIG_FREERTOS_HZ=1000"));
    assert!(!read(project.join("CMakeLists.txt")).contains("EXTRA_COMPONENT_DIRS"));
    assert_eq!(record(&project)["demo"]["folder"], "components/demo");
    says(&ui, "#include \"demo.h\"");
}

#[test]
fn a_dependency_is_required_by_its_folders_name() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let osal = lib_at(root.path(), "osal", "", &[]);
    let library = lib_at(
        root.path(),
        "demo",
        "requires:\n  libraries:\n    - osal\nesp32:\n  library:\n    requires:\n      - esp_timer\n",
        &[],
    );

    install_in(&project, &osal, Some("components/osal_lib"));
    install(&project, &library);

    assert!(
        component_of(&project).contains("REQUIRES\n        esp_timer\n        osal_lib\n)"),
        "{}",
        component_of(&project)
    );
}

#[test]
fn a_folder_outside_components_is_added_to_the_project() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install_in(&project, &demo(root.path(), ""), Some("libs/demo"));

    assert!(
        read(project.join("CMakeLists.txt")).contains("\"${CMAKE_CURRENT_SOURCE_DIR}/libs/demo\"")
    );
    assert!(project.join("libs/demo/CMakeLists.txt").is_file());
}

#[test]
fn the_folder_offered_is_in_components() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let mut ui = Watched::answering(&[""]);
    let code = flow::run_with(
        offline(),
        &asked(&project, &demo(root.path(), ""), None),
        &mut ui,
    );

    assert_eq!(code, 0, "{}", ui.err());
    assert!(ui.asked[0].contains("[components/demo]"), "{:?}", ui.asked);
}

// ----------------------------------------------------------------------------
// What is checked before the library goes in
// ----------------------------------------------------------------------------

fn checked(library: &Path, project: &Path) -> Vec<(Level, String, String)> {
    let manifest = read_it(library).unwrap();
    let mut target = target_of(Platform::Esp32, &PlatformArgs::default());

    target
        .check(&manifest, project, &mut Watched::default())
        .into_iter()
        .map(|finding| (finding.level, finding.message, finding.hint))
        .collect()
}

#[test]
fn no_project_is_one_warning() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty");

    fs::create_dir_all(&empty).unwrap();

    let found = checked(&demo(root.path(), ""), &empty);

    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, Level::Warn);
    assert!(
        found[0].1.contains("include(.../project.cmake)"),
        "{found:?}"
    );
}

#[test]
fn each_symbol_is_said_once_for_what_it_is() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        defaults: Some("CONFIG_A=y\nCONFIG_B=1\n"),
        sdkconfig: Some("CONFIG_C=n\nCONFIG_D=y\n"),
        ..App::default()
    }
    .on(root.path());
    let library = lib_at(
        root.path(),
        "demo",
        "requires:\n  peripherals:\n    - type: i2c\n      note: 400 kHz\nesp32:\n  project:\n    conf:\n      - A\n      - B=2\n      - C\n      - D\n      - E\n",
        &[],
    );
    let said: Vec<(Level, String)> = checked(&library, &project)
        .into_iter()
        .map(|(level, message, _)| (level, message))
        .collect();

    assert_eq!(
        said,
        [
            (Level::Warn, "sdkconfig.defaults already sets CONFIG_A=y, so the block has it commented out".to_string()),
            (Level::Warn, "sdkconfig.defaults sets CONFIG_B=1, and demo needs 2. The block has it commented out".to_string()),
            (Level::Warn, "sdkconfig has CONFIG_C=n, and demo needs y".to_string()),
            (Level::Info, "CONFIG_D=y goes into sdkconfig.defaults".to_string()),
            (Level::Info, "CONFIG_E=y goes into sdkconfig.defaults".to_string()),
            (Level::Info, "Needs I2C, 400 kHz".to_string()),
        ]
    );
}

#[test]
fn a_symbol_already_in_the_block_is_ok() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let library = demo(root.path(), &conf(&["A"]));

    install(&project, &library);

    let said: Vec<(Level, String)> = checked(&library, &project)
        .into_iter()
        .map(|(level, message, _)| (level, message))
        .collect();

    assert_eq!(
        said,
        [(Level::Ok, "CONFIG_A=y is in sdkconfig.defaults".to_string())]
    );
}

#[test]
fn a_project_naming_its_own_defaults_is_warned() {
    let root = tempfile::tempdir().unwrap();
    let cmake = "set(SDKCONFIG_DEFAULTS \"my.defaults\")\ninclude($ENV{IDF_PATH}/tools/cmake/project.cmake)\n";
    let project = App {
        cmake,
        ..App::default()
    }
    .on(root.path());
    let found = checked(&demo(root.path(), &conf(&["A"])), &project);

    assert!(found[0].1.contains("SDKCONFIG_DEFAULTS"), "{found:?}");
}

// ----------------------------------------------------------------------------
// Taking it out
// ----------------------------------------------------------------------------

#[test]
fn the_project_is_back_as_it_was() {
    for crlf in [false, true] {
        for folder in ["components/demo", "libs/demo"] {
            for (cmake, main) in [
                (
                    CMAKELISTS,
                    "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES esp_timer)\n",
                ),
                (MINIMAL, MAIN),
            ] {
                let root = tempfile::tempdir().unwrap();
                let project = App {
                    cmake,
                    main: Some(main),
                    defaults: Some("CONFIG_A=y\n"),
                    sdkconfig: None,
                    crlf,
                }
                .on(root.path());
                let before = snapshot(&project, &FILES);
                let library = demo(
                    root.path(),
                    "  settings:\n    - {name: DEMO_SIZE, type: int, default: 8, prompt: Size}\n  project:\n    conf:\n      - FREERTOS_HZ=1000\n",
                );

                install_in(&project, &library, Some(folder));
                assert_ne!(snapshot(&project, &FILES), before);

                let ui = uninstall(&project, "demo");

                assert_eq!(snapshot(&project, &FILES), before, "{folder} {crlf}");
                assert!(!project.join(folder).exists());
                says(&ui, "Removed demo from sdkconfig.defaults.");
                says(&ui, "Removed demo from the components main requires.");
            }
        }
    }
}

#[test]
fn one_of_two_libraries_comes_out() {
    let root = tempfile::tempdir().unwrap();
    let project = App {
        main: Some("idf_component_register(SRCS \"main.c\" PRIV_REQUIRES esp_timer)\n"),
        ..App::default()
    }
    .on(root.path());
    let first = lib_at(
        root.path(),
        "first",
        &format!("esp32:\n{}", conf(&["A"])),
        &[],
    );
    let second = lib_at(
        root.path(),
        "second",
        &format!("esp32:\n{}", conf(&["B"])),
        &[],
    );

    install(&project, &second);

    let only_second = snapshot(&project, &FILES);

    install(&project, &first);
    uninstall(&project, "first");

    assert_eq!(snapshot(&project, &FILES), only_second);
    assert!(project.join("components/second/CMakeLists.txt").is_file());
}

#[test]
fn a_setting_the_user_set_is_named() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(
        &project,
        &demo(
            root.path(),
            "  settings:\n    - {name: DEMO_SIZE, type: int, default: 8, prompt: Size}\n",
        ),
    );
    write(project.join("sdkconfig.defaults"), "CONFIG_DEMO_SIZE=16\n");

    let ui = uninstall(&project, "demo");

    says(&ui, "sdkconfig.defaults still sets what came with demo");
    says(&ui, "CONFIG_DEMO_SIZE=16");
    assert_eq!(
        read(project.join("sdkconfig.defaults")),
        "CONFIG_DEMO_SIZE=16\n"
    );
}

#[test]
fn the_users_cmake_lines_are_saved() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());

    install(&project, &demo(root.path(), ""));

    let path = project.join("components/demo/CMakeLists.txt");

    write(
        &path,
        &read(&path).replace(
            "# USER CODE BEGIN DEMO_CMAKE\n",
            "# USER CODE BEGIN DEMO_CMAKE\nmine()\n",
        ),
    );

    let ui = uninstall(&project, "demo");
    let saved = backups(&project.join("components/demo"), "CMakeLists.txt");

    assert_eq!(saved.len(), 1);
    assert!(read(&saved[0]).contains("mine()"));
    says(&ui, "Your lines in its CMakeLists.txt are saved");
}

// ----------------------------------------------------------------------------
// Files of the project's own
// ----------------------------------------------------------------------------

#[test]
fn once_files_land_in_the_project_and_a_managed_one_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = plain_app(root.path());
    let source = lib_at(
        root.path(),
        "demo",
        "once:\n  - from: project/VERSION\n    to: VERSION\n",
        &[("project/VERSION", "1.0.0\n")],
    );

    install(&project, &source);

    assert_eq!(read(project.join("VERSION")), "1.0.0\n");

    for to in ["sdkconfig.defaults", "main/CMakeLists.txt", "sdkconfig"] {
        let bad = lib_at(
            &root.path().join(to.replace('/', "-")),
            "bad",
            &format!("once:\n  - from: project/VERSION\n    to: {to}\n"),
            &[("project/VERSION", "1.0.0\n")],
        );
        let (code, ui) = run(&asked(&project, &bad, Some("components/bad")));

        assert_eq!(code, 2, "{to}");
        assert!(
            ui.err().contains("edits or reads as the project's own"),
            "{to}: {}",
            ui.err()
        );
    }
}

#[test]
fn an_empty_list_of_the_users_own_is_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let main = "idf_component_register(SRCS \"main.c\" PRIV_REQUIRES REQUIRES driver)\n";
    let project = App {
        main: Some(main),
        ..App::default()
    }
    .on(root.path());

    prj::uninstall(&project, "demo", Some("demo"));

    assert_eq!(main_of(&project), main);
}
