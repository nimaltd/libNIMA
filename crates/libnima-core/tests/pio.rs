//! A library in a PlatformIO project: in lib/, with a library.json of its own, and what the pio section of its libnima.yml
//! says goes where. Under project, the project's platformio.ini: build_flags, the flags every file of the build is given,
//! written after the library's defines in its block of build_flags. Under library, its library.json: frameworks, platforms
//! and dependencies.
//!
//! build_flags can be given in groups under their titles, and a flag with a note: the block carries both as comments of
//! platformio.ini, with no blank line. library.json is JSON, which has no comments, so its lists have neither.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Github, Watched, put, read, record, run_with, write};
use libnima_core::flow::{self, Request};
use libnima_core::manifest::{self, Manifest};
use libnima_core::platform::pio::{ini, library_json};
use libnima_core::platform::{Platform, PlatformArgs, target_of};
use libnima_core::target::{Level, Status};
use serde_json::{Value, json};

const INI: &str = "[env:native]\nplatform = native\n";
const STARS: &str =
    "; ******************************************************************************";

/// Two environments: one with build_flags of its own, one that takes them from [env], which the install makes.
const TWO: &str = "[env:native]\nplatform = native\nbuild_flags = -DFROM_YOU=1\n\n[env:plain]\nplatform = native\n";

const GROUPED: &str = "      Printing:\n        - -Wl,-u,_printf_float: printf(\"%f\") prints nothing without it\n      Speed:\n        - -O2\n        - -I include\n";

fn lib_at(root: &Path, name: &str, body: &str, files: &[(&str, &str)]) -> PathBuf {
    let folder = root.join("repos").join(name);

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
            "name: {name}\ndescription: A library for the tests\nrepository: https://github.com/someone/{name}\nlicense: Apache-2.0\nkind: utility\ncategory: timing\nplatform: pio\nheaders:\n  - src/{name}.h\nsources:\n  - src/{name}.c\n{body}"
        ),
    )
    .unwrap();

    folder
}

fn demo(root: &Path, body: &str) -> PathBuf {
    lib_at(root, "demo", body, &[])
}

fn read_it(folder: &Path) -> Result<Manifest, String> {
    manifest::load(folder, Some(Platform::Pio)).map_err(|error| error.message)
}

fn refused(body: &str) -> String {
    let root = tempfile::tempdir().unwrap();

    read_it(&demo(root.path(), body)).unwrap_err()
}

/// A PlatformIO project on disk.
fn project_with(root: &Path, text: &str, crlf: bool) -> PathBuf {
    let project = root.join("app");

    write(project.join("src/main.c"), "int main(void) { return 0; }\n");
    write(
        project.join("platformio.ini"),
        &if crlf {
            text.replace('\n', "\r\n")
        } else {
            text.to_string()
        },
    );

    project
}

fn offline() -> libnima_core::download::Client {
    Github::serving(&[]).client()
}

fn run(asked: &Request) -> (i32, Watched) {
    run_with(offline(), asked)
}

fn install(project: &Path, library: &Path) -> Watched {
    let (code, ui) = run(&common::request(project, &[library]));

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());

    ui
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

fn says(ui: &Watched, wanted: &str) {
    assert!(
        ui.out().contains(wanted),
        "{wanted} is not in:\n{}",
        ui.out()
    );
}

fn ini_of(project: &Path) -> String {
    read(project.join("platformio.ini"))
}

fn all_backups(project: &Path) -> Vec<String> {
    libnima_core::paths::files_below(project)
        .into_iter()
        .filter(|path| path.ends_with(".bak"))
        .collect()
}

/// platformio.ini of TWO with the block of these lines in [env] and in [env:native].
fn both(lines: &[&str]) -> String {
    let body: String = lines.iter().map(|line| format!("    {line}\n")).collect();
    let block = format!("    ; >>> libnima: demo >>>\n{body}    ; <<< libnima: demo <<<\n");

    format!(
        "[env]\nbuild_flags =\n{block}\n[env:native]\nplatform = native\nbuild_flags = -DFROM_YOU=1\n{block}\n[env:plain]\nplatform = native\n"
    )
}

/// The library.json written for a body of libnima.yml.
fn json_of(root: &Path, body: &str, files: &[(&str, &str)]) -> Value {
    let library = read_it(&lib_at(root, "demo", body, files)).unwrap();
    let folder = root.join("written");

    fs::create_dir_all(&folder).unwrap();
    library_json::write(&library, &folder);

    serde_json::from_str(&read(folder.join("library.json"))).unwrap()
}

// ----------------------------------------------------------------------------
// A whole install
// ----------------------------------------------------------------------------

#[test]
fn a_library_goes_into_lib_with_its_library_json() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let ui = install(&project, &demo(root.path(), "defines:\n  - DEMO_FAST\n"));
    let written: Value =
        serde_json::from_str(&read(project.join("lib/demo/library.json"))).unwrap();

    assert!(project.join("lib/demo/src/demo.h").is_file());
    assert_eq!(
        written["build"]["srcFilter"],
        json!(["-<*>", "+<src/demo.c>"])
    );
    assert!(ini_of(&project).contains("    -DDEMO_FAST\n"));
    assert_eq!(record(&project)["demo"]["folder"], "lib/demo");
    says(&ui, "#include \"demo.h\"");
}

#[test]
fn a_library_with_no_defines_leaves_platformio_ini_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);

    install(&project, &demo(root.path(), ""));

    assert_eq!(ini_of(&project), INI);
    assert!(common::backups(&project, "platformio.ini").is_empty());
}

#[test]
fn what_a_library_needs_goes_into_lib_too() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let helper = lib_at(root.path(), "helper", "", &[]);
    let library = demo(root.path(), "requires:\n  libraries:\n    - helper\n");
    let (code, ui) = run(&common::request(&project, &[&library, &helper]));

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("lib/helper/library.json").is_file());
    assert!(project.join("lib/demo/library.json").is_file());
}

#[test]
fn the_folder_offered_is_in_lib() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let library = demo(root.path(), "");
    let mut ui = Watched::answering(&[""]);
    let code = flow::run_with(offline(), &common::request(&project, &[&library]), &mut ui);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(ui.asked[0].contains("[lib/demo]"), "{:?}", ui.asked);
}

#[test]
fn a_folder_outside_lib_is_named_for_the_user() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let library = demo(root.path(), "");
    let mut asked = common::request(&project, &[&library]);

    asked.folder = Some("vendor/demo".to_string());

    let (code, ui) = run(&asked);

    assert_eq!(code, 0, "{}", ui.err());
    says(
        &ui,
        "PlatformIO looks for libraries in lib/, and demo is not in it.",
    );
    says(&ui, "lib_extra_dirs");
}

// ----------------------------------------------------------------------------
// What is checked before the library goes in
// ----------------------------------------------------------------------------

fn checked(library: &Path, project: &Path) -> Vec<(Level, String)> {
    let manifest = read_it(library).unwrap();
    let mut target = target_of(Platform::Pio, &PlatformArgs::default());

    target
        .check(&manifest, project, &mut Watched::default())
        .into_iter()
        .map(|finding| (finding.level, finding.message))
        .collect()
}

#[test]
fn with_no_platformio_ini_nothing_is_installed() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty");

    fs::create_dir_all(&empty).unwrap();

    let (code, ui) = run(&common::request(&empty, &[&demo(root.path(), "")]));

    assert_eq!(code, 2, "{}", ui.out());
    assert!(
        ui.err()
            .contains("is not a PlatformIO project: it has no platformio.ini"),
        "{}",
        ui.err()
    );
    assert!(fs::read_dir(&empty).unwrap().next().is_none());
}

#[test]
fn no_platformio_ini_is_one_warning() {
    let root = tempfile::tempdir().unwrap();
    let empty = root.path().join("empty");

    fs::create_dir_all(&empty).unwrap();

    assert_eq!(
        checked(&demo(root.path(), ""), &empty),
        [(Level::Warn, "No platformio.ini here".to_string())]
    );
}

#[test]
fn an_environment_the_library_does_not_build_with_is_named() {
    let root = tempfile::tempdir().unwrap();
    let text = "[env]\nframework = arduino\n\n[env:uno]\nplatform = atmelavr\n\n[env:esp]\nplatform = platformio/espressif32@^6.5\nframework = espidf\n\n[env:pc]\nplatform = native\nframework =\n";
    let project = project_with(root.path(), text, false);
    let library = demo(
        root.path(),
        "pio:\n  library:\n    frameworks: [arduino]\n    platforms: [atmelavr, native]\n",
    );

    assert_eq!(
        checked(&library, &project),
        [(Level::Warn, "[env:esp] builds with espidf, which demo does not list, so PlatformIO leaves it out of that build".to_string())]
    );
}

#[test]
fn a_platform_the_library_does_not_list_is_named() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), "[env:esp]\nplatform = espressif32\n", false);
    let found = checked(
        &demo(root.path(), "pio:\n  library:\n    platforms: [native]\n"),
        &project,
    );

    assert_eq!(found[0].0, Level::Warn);
    assert!(
        found[0].1.contains("[env:esp] builds for espressif32"),
        "{found:?}"
    );
}

#[test]
fn a_platform_from_a_url_is_not_judged() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        "[env:x]\nplatform = https://github.com/someone/platform-x.git\n",
        false,
    );

    assert!(
        checked(
            &demo(root.path(), "pio:\n  library:\n    platforms: [native]\n"),
            &project
        )
        .is_empty()
    );
}

#[test]
fn a_platform_with_its_owner_and_version_is_the_platform() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        "[env:esp]\nplatform = platformio/espressif32@^6.5\n",
        false,
    );

    assert!(
        checked(
            &demo(
                root.path(),
                "pio:\n  library:\n    platforms: [espressif32]\n"
            ),
            &project
        )
        .is_empty()
    );
}

#[test]
fn defines_dependencies_and_hardware_are_said() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let library = demo(
        root.path(),
        "defines:\n  - DEMO_FAST\npio:\n  library:\n    dependencies:\n      - bblanchon/ArduinoJson@^7.0.0\n      - Wire\nrequires:\n  peripherals:\n    - type: i2c\n      note: 400 kHz\n    - spi\n",
    );

    assert_eq!(
        checked(&library, &project),
        [
            (
                Level::Info,
                "DEMO_FAST goes into build_flags in platformio.ini".to_string()
            ),
            (
                Level::Info,
                "Needs bblanchon/ArduinoJson ^7.0.0, from the PlatformIO Registry".to_string()
            ),
            (
                Level::Info,
                "Needs Wire, from the PlatformIO Registry".to_string()
            ),
            (Level::Info, "Needs I2C, 400 kHz".to_string()),
            (Level::Info, "Needs SPI".to_string()),
        ]
    );
}

#[test]
fn an_unreadable_platformio_ini_gives_the_flags_by_hand() {
    let root = tempfile::tempdir().unwrap();
    let library = read_it(&demo(root.path(), "defines:\n  - DEMO_FAST\n")).unwrap();
    let empty = root.path().join("empty");

    fs::create_dir_all(&empty).unwrap();

    let outcome = ini::integrate(&empty, &library).expect("nothing was said");

    assert_eq!(outcome.status, Status::Manual);
    assert_eq!(outcome.steps.last().unwrap(), "-DDEMO_FAST");
}

// ----------------------------------------------------------------------------
// library.json
// ----------------------------------------------------------------------------

#[test]
fn with_no_pio_part_it_builds_anywhere() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(
        json_of(root.path(), "", &[]),
        json!({
            "name": "demo",
            "version": "1.2.0",
            "description": "A library for the tests",
            "keywords": ["utility", "timing"],
            "repository": {"type": "git", "url": "https://github.com/someone/demo"},
            "authors": [{"name": "someone"}],
            "license": "Apache-2.0",
            "frameworks": "*",
            "platforms": "*",
            "build": {"srcDir": ".", "srcFilter": ["-<*>", "+<src/demo.c>"], "includeDir": "src"},
        })
    );
}

#[test]
fn library_goes_into_library_json() {
    let root = tempfile::tempdir().unwrap();
    let written = json_of(
        root.path(),
        "pio:\n  library:\n    frameworks: [arduino, espidf]\n    platforms: espressif32\n    dependencies:\n      - bblanchon/ArduinoJson@^7.0.0\n      - Wire\n",
        &[],
    );

    assert_eq!(written["frameworks"], json!(["arduino", "espidf"]));
    assert_eq!(written["platforms"], json!(["espressif32"]));
    assert_eq!(
        written["dependencies"],
        json!({"bblanchon/ArduinoJson": "^7.0.0", "Wire": "*"})
    );
}

#[test]
fn only_its_own_sources_are_built() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("own");

    put(
        &folder,
        &[
            ("src/demo.h", ""),
            ("src/demo.c", ""),
            ("src/port/spi.c", ""),
            ("test/test_demo.c", "x\n"),
        ],
    );
    fs::write(
        folder.join("libnima.yml"),
        "name: demo\nplatform: pio\nheaders:\n  - src/demo.h\nsources:\n  - src/demo.c\n  - src/port/spi.c\n",
    )
    .unwrap();

    let written = root.path().join("written");

    fs::create_dir_all(&written).unwrap();
    library_json::write(&read_it(&folder).unwrap(), &written);

    let found: Value = serde_json::from_str(&read(written.join("library.json"))).unwrap();

    assert_eq!(
        found["build"]["srcFilter"],
        json!(["-<*>", "+<src/demo.c>", "+<src/port/spi.c>"])
    );
}

fn headers_json(root: &Path, headers: &[&str], more: &str) -> Value {
    let folder = root.join("headers");
    let listed: String = headers
        .iter()
        .map(|header| format!("  - {header}\n"))
        .collect();

    for header in headers {
        put(&folder, &[(header, "")]);
    }

    fs::write(
        folder.join("libnima.yml"),
        format!("name: demo\nplatform: pio\nheaders:\n{listed}{more}"),
    )
    .unwrap();

    let written = root.join("written");

    fs::create_dir_all(&written).unwrap();
    library_json::write(&read_it(&folder).unwrap(), &written);

    serde_json::from_str(&read(written.join("library.json"))).unwrap()
}

#[test]
fn a_library_at_the_top_has_no_include_folder() {
    let root = tempfile::tempdir().unwrap();
    let written = headers_json(root.path(), &["demo.h"], "");

    assert!(written["build"].get("includeDir").is_none(), "{written}");
    assert!(written["build"].get("flags").is_none(), "{written}");
}

#[test]
fn the_first_headers_folder_is_the_include_folder_and_the_others_are_flags() {
    let root = tempfile::tempdir().unwrap();
    let written = headers_json(
        root.path(),
        &[
            "inc/demo.h",
            "api/demo_api.h",
            "inc/port/demo_port.h",
            "top.h",
        ],
        "",
    );

    assert_eq!(written["build"]["includeDir"], "inc");
    assert_eq!(written["build"]["flags"], json!(["-I api", "-I inc/port"]));
}

#[test]
fn include_dirs_named_in_the_file_come_first() {
    let root = tempfile::tempdir().unwrap();
    let written = headers_json(
        root.path(),
        &["src/demo.h", "include/demo_port.h"],
        "include_dirs:\n  - include\n  - src\n",
    );

    assert_eq!(written["build"]["includeDir"], "include");
    assert_eq!(written["build"]["flags"], json!(["-I src"]));
}

#[test]
fn what_cannot_be_written_is_refused() {
    for (body, says) in [
        (
            "pio:\n  library:\n    frameworks:\n      - Arduino IDE\n",
            "pio.library.frameworks",
        ),
        (
            "pio:\n  library:\n    platforms: [1.5]\n",
            "pio.library.platforms",
        ),
        (
            "pio:\n  library:\n    dependencies: ArduinoJson\n",
            "pio.library.dependencies",
        ),
        (
            "pio:\n  library:\n    dependencies:\n      - bad;name\n",
            "pio.library.dependencies",
        ),
        ("pio: everything\n", "pio should hold"),
    ] {
        let message = refused(body);

        assert!(message.contains(says), "{body}: {message}");
    }
}

#[test]
fn any_is_written_as_a_star() {
    for value in ["'*'", "['*']", ""] {
        let root = tempfile::tempdir().unwrap();
        let written = json_of(
            root.path(),
            &format!("pio:\n  library:\n    frameworks: {value}\n"),
            &[],
        );

        assert_eq!(written["frameworks"], "*", "{value}");
    }
}

#[test]
fn frameworks_may_be_one_line_with_commas() {
    let root = tempfile::tempdir().unwrap();

    assert_eq!(
        json_of(
            root.path(),
            "pio:\n  library:\n    frameworks: arduino, espidf\n",
            &[]
        )["frameworks"],
        json!(["arduino", "espidf"])
    );
}

#[test]
fn it_is_written_as_json_then_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let library = read_it(&demo(root.path(), "")).unwrap();
    let folder = root.path().join("written");

    fs::create_dir_all(&folder).unwrap();

    assert_eq!(
        library_json::write(&library, &folder).status,
        Status::Changed
    );
    assert!(read(folder.join("library.json")).ends_with("}\n"));
    assert_eq!(
        library_json::write(&library, &folder).status,
        Status::Already
    );
}

#[test]
fn a_librarys_own_is_used_as_it_is() {
    let root = tempfile::tempdir().unwrap();
    let own = "{\"name\": \"mine\"}\n";
    let library = read_it(&lib_at(
        root.path(),
        "demo",
        "extras:\n  - library.json\n",
        &[("library.json", own)],
    ))
    .unwrap();
    let folder = root.path().join("written");

    write(folder.join("library.json"), own);

    assert_eq!(
        library_json::write(&library, &folder).status,
        Status::Already
    );
    assert_eq!(read(folder.join("library.json")), own);
}

// ----------------------------------------------------------------------------
// project: platformio.ini
// ----------------------------------------------------------------------------

#[test]
fn build_flags_go_after_the_defines_wherever_build_flags_reach() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), TWO, false);

    install(
        &project,
        &demo(
            root.path(),
            "defines:\n  - DEMO_FAST\npio:\n  project:\n    build_flags:\n      - -O2\n      - -Wl,-u,_printf_float\n",
        ),
    );

    assert_eq!(
        ini_of(&project),
        both(&["-DDEMO_FAST", "-O2", "-Wl,-u,_printf_float"])
    );
}

#[test]
fn titles_and_notes_are_comments_of_platformio_ini_with_no_blank_line() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), TWO, false);

    install(
        &project,
        &demo(
            root.path(),
            &format!("pio:\n  project:\n    build_flags:\n{GROUPED}"),
        ),
    );

    let text = ini_of(&project);

    assert_eq!(
        text,
        both(&[
            STARS,
            "; Printing",
            STARS,
            "; printf(\"%f\") prints nothing without it",
            "-Wl,-u,_printf_float",
            STARS,
            "; Speed",
            STARS,
            "-O2",
            "-I include"
        ])
    );
    assert!(!text.contains("\n\n    ;"), "{text}");
}

#[test]
fn what_a_run_says_names_the_flags() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let ui = install(
        &project,
        &demo(
            root.path(),
            "pio:\n  project:\n    build_flags:\n      - -O2\n      - -Wl,-u,_printf_float\n",
        ),
    );

    says(
        &ui,
        "-O2, -Wl,-u,_printf_float go into build_flags in platformio.ini",
    );
}

#[test]
fn a_second_install_changes_nothing_and_an_uninstall_gives_the_file_back() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), TWO, false);
    let library = demo(
        root.path(),
        &format!("defines:\n  - DEMO_FAST\npio:\n  project:\n    build_flags:\n{GROUPED}"),
    );

    install(&project, &library);

    let done = ini_of(&project);
    let saved = all_backups(&project);

    install(&project, &library);

    assert_eq!(ini_of(&project), done);
    assert_eq!(all_backups(&project), saved);

    let ui = uninstall(&project, "demo");

    assert_eq!(ini_of(&project), TWO);
    says(
        &ui,
        "Removed demo's defines and build flags from platformio.ini.",
    );
}

#[test]
fn an_update_without_build_flags_takes_its_block_out() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), TWO, false);

    install(
        &project,
        &demo(
            root.path(),
            "pio:\n  project:\n    build_flags:\n      - -O2\n",
        ),
    );

    let ui = install(&project, &demo(root.path(), ""));

    assert_eq!(ini_of(&project), TWO);
    says(&ui, "Took demo's old build flags out of platformio.ini.");
}

const HALF: &str =
    "[env:native]\nbuild_flags =\n    ; >>> libnima: demo >>>\n    -DOLD\n\nlib_deps = mine\n";

#[test]
fn a_half_written_block_is_left_alone_with_the_flags_to_add() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), HALF, false);
    let library = read_it(&demo(
        root.path(),
        &format!("defines:\n  - DEMO_FAST\npio:\n  project:\n    build_flags:\n{GROUPED}"),
    ))
    .unwrap();
    let outcome = ini::integrate(&project, &library).expect("nothing was said");

    assert_eq!(outcome.status, Status::Manual);
    assert_eq!(
        outcome.steps[1..],
        ["-DDEMO_FAST", "-Wl,-u,_printf_float", "-O2", "-I include"]
    );
    assert_eq!(ini_of(&project), HALF);
}

#[test]
fn uninstall_leaves_a_half_written_block_alone() {
    let root = tempfile::tempdir().unwrap();
    let text =
        "[env:native]\nbuild_flags =\n    ; >>> libnima: demo >>>\n    -DOLD\nlib_deps = mine\n";
    let project = project_with(root.path(), text, false);
    let outcome = ini::uninstall(&project, "demo").expect("nothing was said");

    assert_eq!(outcome.status, Status::Manual);
    assert_eq!(ini_of(&project), text);
}

#[test]
fn the_block_is_recorded_with_its_fingerprint() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);

    install(&project, &demo(root.path(), "defines:\n  - DEMO_FAST\n"));

    assert_eq!(
        record(&project)["demo"]["blocks"]["platformio.ini"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["demo"]
    );
}

#[test]
fn flags_of_a_second_library_go_in_after_the_first_and_both_come_back_out() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), TWO, false);
    let first = demo(root.path(), "defines:\n  - DEMO_FAST\n");
    let second = lib_at(
        root.path(),
        "flags",
        "pio:\n  project:\n    build_flags:\n      Its level:\n        - -DFLAGS_LEVEL=7: flags.h stops the build without it\n  library:\n    platforms: [native]\n",
        &[],
    );

    install(&project, &first);
    install(&project, &second);

    let text = ini_of(&project);

    assert_eq!(
        text.matches(&format!(
            "    ; >>> libnima: flags >>>\n    {STARS}\n    ; Its level\n    {STARS}\n    ; flags.h stops the build without it\n    -DFLAGS_LEVEL=7\n"
        ))
        .count(),
        2,
        "{text}"
    );
    assert!(
        text.find("libnima: demo <<<") < text.find("libnima: flags >>>"),
        "{text}"
    );

    let written: Value =
        serde_json::from_str(&read(project.join("lib/flags/library.json"))).unwrap();

    assert_eq!(written["platforms"], json!(["native"]));

    uninstall(&project, "demo");
    uninstall(&project, "flags");

    assert_eq!(ini_of(&project), TWO);
}

// ----------------------------------------------------------------------------
// What is refused
// ----------------------------------------------------------------------------

#[test]
fn the_keys_from_before_are_refused_with_where_they_went() {
    for (body, says) in [
        (
            "pio:\n  frameworks: [arduino]\n",
            "pio.frameworks is pio.library.frameworks",
        ),
        (
            "pio:\n  platforms: [native]\n",
            "pio.platforms is pio.library.platforms",
        ),
        (
            "pio:\n  dependencies:\n    - bblanchon/ArduinoJson@^7\n",
            "pio.dependencies is pio.library.dependencies",
        ),
        (
            "pio:\n  framework: arduino\n",
            "pio has framework. It holds project, library.",
        ),
    ] {
        let message = refused(body);

        assert!(message.contains(says), "{body}: {message}");
    }
}

#[test]
fn what_a_part_does_not_hold_is_refused() {
    for (body, says) in [
        (
            "pio:\n  project:\n    flags: [-O2]\n",
            "pio.project has flags. It holds build_flags.",
        ),
        (
            "pio:\n  library:\n    build_flags: [-O2]\n",
            "pio.library has build_flags. It holds frameworks, platforms, dependencies.",
        ),
        (
            "pio:\n  project:\n    - -O2\n",
            "pio.project holds build_flags",
        ),
    ] {
        let message = refused(body);

        assert!(message.contains(says), "{body}: {message}");
    }
}

#[test]
fn what_library_json_cannot_hold_is_refused() {
    for (body, says) in [
        (
            "    platforms:\n      Boards:\n        - native\n",
            "pio.library.platforms has a title or a note",
        ),
        (
            "    frameworks:\n      - arduino: the one it is tested with\n",
            "pio.library.frameworks has a title or a note",
        ),
        (
            "    dependencies:\n      - bblanchon/ArduinoJson@^7: for the settings file\n",
            "pio.library.dependencies has a title or a note, which library.json, being JSON, has no room for",
        ),
        (
            "    dependencies:\n      - bblanchon/ArduinoJson@^6\n      - bblanchon/ArduinoJson@^7\n",
            "pio.library.dependencies has bblanchon/ArduinoJson more than once",
        ),
    ] {
        let message = refused(&format!("pio:\n  library:\n{body}"));

        assert!(message.contains(says), "{body}: {message}");
    }
}

#[test]
fn a_flag_that_could_be_read_as_more_than_a_flag_is_refused() {
    for flag in [
        "'!echo -DX'", // PlatformIO runs a command for a line starting with !
        "-DX=$(HOME)", // SCons expands $
        "'-DX=${sysenv.HOME}'",
        "-DX;rm",      // ; starts a comment
        "'-O2 #fast'", // and so does #
        "'-DMSG=\"hi\"'",
        "O2", // no option
        "\"-DA\\n-DB\"",
        "''",
        "5",
    ] {
        let message = refused(&format!(
            "pio:\n  project:\n    build_flags:\n      - {flag}\n"
        ));

        assert!(
            message.contains("pio.project.build_flags has"),
            "{flag}: {message}"
        );
        assert!(
            message.contains("Give a flag as the compiler takes it"),
            "{flag}: {message}"
        );
    }
}

#[test]
fn build_flags_that_cannot_be_written_are_refused() {
    for (body, says) in [
        (
            "      A:\n        - -O2\n      B:\n        - -O2\n",
            "pio.project.build_flags has -O2 more than once",
        ),
        (
            "      \">>> libnima: x >>>\":\n        - -O2\n",
            "what libNIMA marks its blocks with",
        ),
        (
            "      - -O2: \"first\\nsecond\"\n",
            "Give it one line of text",
        ),
        (
            "      Empty: []\n",
            "has the title 'Empty' with no list under it",
        ),
    ] {
        let message = refused(&format!("pio:\n  project:\n    build_flags:\n{body}"));

        assert!(message.contains(says), "{body}: {message}");
    }
}

// ----------------------------------------------------------------------------
// Taking it out
// ----------------------------------------------------------------------------

#[test]
fn the_project_is_back_as_it_was() {
    for crlf in [false, true] {
        for text in [
            INI,
            "[env]\nbuild_flags = -DC\n\n[env:a]\nplatform = native\n\n[env:b]\nbuild_flags =\n    -DB\n",
        ] {
            let root = tempfile::tempdir().unwrap();
            let project = project_with(root.path(), text, crlf);
            let before = ini_of(&project);

            install(
                &project,
                &demo(root.path(), "defines:\n  - DEMO_FAST\n  - DEMO_N=2\n"),
            );
            assert_ne!(ini_of(&project), before);

            let ui = uninstall(&project, "demo");

            assert_eq!(ini_of(&project), before);
            assert!(!project.join("lib/demo").exists());
            says(&ui, "Removed demo's defines from platformio.ini.");
        }
    }
}

#[test]
fn a_librarys_own_library_json_goes_with_its_files_and_is_said_to() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);
    let library = lib_at(
        root.path(),
        "demo",
        "extras:\n  - library.json\n",
        &[("library.json", "{\"name\": \"demo\"}\n")],
    );

    install(&project, &library);

    let ui = uninstall(&project, "demo");

    assert!(!project.join("lib/demo").exists());
    says(&ui, "removed lib/demo/library.json");
}

#[test]
fn one_of_two_libraries_comes_out() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);

    install(
        &project,
        &lib_at(root.path(), "two", "defines:\n  - TWO\n", &[]),
    );

    let only_two = ini_of(&project);

    install(&project, &demo(root.path(), "defines:\n  - ONE\n"));
    uninstall(&project, "demo");

    assert_eq!(ini_of(&project), only_two);
    assert!(project.join("lib/two/library.json").is_file());
}

// ----------------------------------------------------------------------------
// Files of the project's own
// ----------------------------------------------------------------------------

#[test]
fn once_files_land_in_the_project_and_never_in_platformio_ini() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(root.path(), INI, false);

    install(
        &project,
        &lib_at(
            root.path(),
            "demo",
            "once:\n  - from: project/VERSION\n    to: VERSION\n",
            &[("project/VERSION", "1\n")],
        ),
    );

    assert_eq!(read(project.join("VERSION")), "1\n");

    let bad = lib_at(
        root.path(),
        "bad",
        "once:\n  - from: project/VERSION\n    to: platformio.ini\n",
        &[("project/VERSION", "1\n")],
    );
    let (code, ui) = run(&common::request(&project, &[&bad]));

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("edits or reads as the project's own"),
        "{}",
        ui.err()
    );
}
