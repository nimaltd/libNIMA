//! A library for Arduino: in the sketchbook's libraries folder, or in a sketch's src folder, and what is written for it.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{Github, Watched, put, read, run_with, write};
use libnima_core::flow::{self, Request};
use libnima_core::manifest::{self, Manifest};
use libnima_core::platform::arduino::properties;
use libnima_core::platform::{Platform, PlatformArgs, target_of};
use libnima_core::record::installed_libraries;
use libnima_core::target::{Level, Status};

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
                &format!("int {name}_answer(void) {{ return 42; }}\n"),
            ),
        ],
    );
    put(&folder, files);
    fs::write(
        folder.join("libnima.yml"),
        format!(
            "name: {name}\ndescription: A library for the tests\nrepository: https://github.com/someone/{name}\ncategory: timing\nplatform: arduino\nheaders:\n  - src/{name}.h\nsources:\n  - src/{name}.c\n{body}"
        ),
    )
    .unwrap();

    folder
}

fn demo(root: &Path, body: &str) -> PathBuf {
    lib_at(root, "demo", body, &[])
}

fn read_it(folder: &Path) -> Result<Manifest, String> {
    manifest::load(folder, Some(Platform::Arduino)).map_err(|error| error.message)
}

/// The sketchbook of a test, so no test reads or writes the machine's real one.
fn book(root: &Path) -> PathBuf {
    root.join("Arduino")
}

/// A sketch in the sketchbook: a folder with a .ino named after it.
fn sketch(root: &Path) -> PathBuf {
    let folder = book(root).join("blink");

    write(
        folder.join("blink.ino"),
        "void setup() { }\nvoid loop() { }\n",
    );

    folder
}

/// A folder that is no sketch.
fn elsewhere(root: &Path) -> PathBuf {
    let folder = root.join("elsewhere");

    fs::create_dir_all(&folder).unwrap();

    folder
}

fn asked(root: &Path, project: &Path, libraries: &[&Path], into: Option<&str>) -> Request {
    let mut asked = common::request(project, libraries);

    asked.args.sketchbook = Some(book(root).to_string_lossy().into_owned());
    asked.args.into = into.map(str::to_string);

    asked
}

fn offline() -> libnima_core::download::Client {
    Github::serving(&[]).client()
}

fn run(request: &Request) -> (i32, Watched) {
    run_with(offline(), request)
}

fn install(root: &Path, project: &Path, libraries: &[&Path], into: Option<&str>) -> Watched {
    let (code, ui) = run(&asked(root, project, libraries, into));

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());

    ui
}

fn taking_out(root: &Path, project: &Path, names: &[&str]) -> Request {
    Request {
        uninstall: names.iter().map(ToString::to_string).collect(),
        project: Some(project.to_path_buf()),
        args: PlatformArgs {
            sketchbook: Some(book(root).to_string_lossy().into_owned()),
            ..PlatformArgs::default()
        },
        // Nothing else here says which platform this is, outside a sketch.
        platform: Some(Platform::Arduino),
        ..Request::default()
    }
}

fn uninstall(root: &Path, project: &Path, names: &[&str]) -> Watched {
    let (code, ui) = run(&taking_out(root, project, names));

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

// ----------------------------------------------------------------------------
// The libraries folder
// ----------------------------------------------------------------------------

#[test]
fn outside_a_sketch_it_goes_into_the_libraries_folder() {
    let root = tempfile::tempdir().unwrap();
    let ui = install(
        root.path(),
        &elsewhere(root.path()),
        &[&demo(root.path(), "")],
        None,
    );
    let libraries = book(root.path()).join("libraries");

    assert!(libraries.join("demo/src/demo.h").is_file());
    assert!(read(libraries.join("demo/library.properties")).contains("name=demo\n"));
    assert_eq!(installed_libraries(&libraries)["demo"]["folder"], "demo");
    says(
        &ui,
        &format!(
            "Libraries folder: {}  (from --sketchbook)",
            libnima_core::paths::posix(&libnima_core::paths::resolve(&libraries))
        ),
    );
    says(&ui, "#include \"demo.h\"");
}

#[test]
fn from_a_sketch_the_user_is_asked_and_the_libraries_folder_is_offered() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());
    let library = demo(root.path(), "");
    let mut ui = Watched::answering(&["", ""]);
    let code = flow::run_with(
        offline(),
        &asked(root.path(), &folder, &[&library], None),
        &mut ui,
    );

    assert_eq!(code, 0, "{}", ui.err());
    assert!(ui.asked[0].contains("Install into [1]"), "{:?}", ui.asked);
    assert!(folder.parent().unwrap().join("libraries/demo").is_dir());
    assert!(!folder.join("src").exists());
}

#[test]
fn nobody_there_means_the_libraries_folder() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(root.path(), &folder, &[&demo(root.path(), "")], None);

    assert!(folder.parent().unwrap().join("libraries/demo").is_dir());
}

// ----------------------------------------------------------------------------
// A sketch's src folder
// ----------------------------------------------------------------------------

#[test]
fn answered_sketch_it_goes_into_src() {
    for answer in ["2", "sketch", "S"] {
        let root = tempfile::tempdir().unwrap();
        let folder = sketch(root.path());
        let library = demo(root.path(), "");
        let mut ui = Watched::answering(&[answer, ""]);
        let code = flow::run_with(
            offline(),
            &asked(root.path(), &folder, &[&library], None),
            &mut ui,
        );

        assert_eq!(code, 0, "{answer}: {}", ui.err());
        assert!(folder.join("src/demo/src/demo.h").is_file(), "{answer}");
        assert!(
            !folder.join("src/demo/library.properties").exists(),
            "{answer}"
        );
        assert!(
            installed_libraries(&folder).contains_key("demo"),
            "{answer}"
        );
        says(&ui, "#include \"src/demo/src/demo.h\"");
        says(&ui, "nothing to register");
    }
}

#[test]
fn a_sketch_already_used_offers_its_src_folder() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(
        root.path(),
        &folder,
        &[&demo(root.path(), "")],
        Some("sketch"),
    );

    let other = lib_at(root.path(), "other", "", &[]);
    let mut ui = Watched::answering(&["", ""]);
    let code = flow::run_with(
        offline(),
        &asked(root.path(), &folder, &[&other], None),
        &mut ui,
    );

    assert_eq!(code, 0, "{}", ui.err());
    assert!(ui.asked[0].contains("Install into [2]"), "{:?}", ui.asked);
    assert!(folder.join("src/other/src/other.h").is_file());
}

#[test]
fn with_nobody_to_ask_the_default_is_taken_in_a_sketch_already_used() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(
        root.path(),
        &folder,
        &[&demo(root.path(), "")],
        Some("sketch"),
    );
    install(
        root.path(),
        &folder,
        &[&lib_at(root.path(), "other", "", &[])],
        None,
    );

    assert!(folder.join("src/other/src/other.h").is_file());
}

#[test]
fn into_sketch_outside_a_sketch_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let (code, ui) = run(&asked(
        root.path(),
        &elsewhere(root.path()),
        &[&demo(root.path(), "")],
        Some("sketch"),
    ));

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("is not a sketch: it has no"),
        "{}",
        ui.err()
    );
}

#[test]
fn what_a_library_in_a_sketch_needs_goes_into_the_libraries_folder() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());
    let helper = lib_at(root.path(), "helper", "", &[]);
    let library = demo(root.path(), "requires:\n  libraries:\n    - helper\n");
    let ui = install(root.path(), &folder, &[&library, &helper], Some("sketch"));

    assert!(folder.join("src/demo/src/demo.h").is_file());
    assert!(
        folder
            .parent()
            .unwrap()
            .join("libraries/helper/library.properties")
            .is_file()
    );
    assert!(!folder.join("src/helper").exists());
    says(
        &ui,
        "helper, which it needs, goes into the libraries folder",
    );
    assert!(installed_libraries(&book(root.path()).join("libraries")).contains_key("helper"));
    assert_eq!(
        installed_libraries(&folder).keys().collect::<Vec<_>>(),
        ["demo"]
    );
}

#[test]
fn what_is_in_the_libraries_folder_already_is_kept() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());
    let helper = lib_at(root.path(), "helper", "", &[]);
    let library = demo(root.path(), "requires:\n  libraries:\n    - helper\n");

    install(root.path(), &folder, &[&helper], Some("libraries"));

    let ui = install(root.path(), &folder, &[&library], Some("sketch"));

    says(&ui, "helper 1.2.0 is already in this project, kept");
    assert!(!folder.join("src/helper").exists());
}

#[test]
fn a_fetched_dependency_keeps_the_options_recorded_where_it_is_kept() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());
    let option = "version: VERSION\noptions:\n  extra:\n    description: More\n    headers:\n      - src/helper_extra.h\n";
    let make = |tag: &str, number: &str| {
        lib_at(
            &root.path().join(tag),
            "helper",
            &option.replace("VERSION", number),
            &[("src/helper_extra.h", "int x;\n")],
        )
    };
    let old = make("v1", "1.0.0");
    let new = make("v2", "2.0.0");

    // Versions in a header come first, and these say it in the file alone.
    for version in [&old, &new] {
        fs::write(version.join("src/helper.h"), "int helper_answer(void);\n").unwrap();
    }

    let mut first = asked(root.path(), &folder, &[&old], Some("libraries"));

    first.with_options = vec!["extra".to_string()];
    assert_eq!(run(&first).0, 0);

    let library = demo(
        root.path(),
        "requires:\n  libraries:\n    - helper >= 2.0.0\n",
    );
    let server = Github::serving(&[("helper", &new)]);
    let (code, ui) = run_with(
        server.client(),
        &asked(root.path(), &folder, &[&library], Some("sketch")),
    );
    let libraries = book(root.path()).join("libraries");

    assert_eq!(code, 0, "{}", ui.err());
    assert!(ui.out().contains("yes, as before"), "{}", ui.out());
    assert!(libraries.join("helper/src/helper_extra.h").is_file());
    assert_eq!(
        installed_libraries(&libraries)["helper"]["version"],
        "2.0.0"
    );
}

// ----------------------------------------------------------------------------
// What is checked before the library goes in
// ----------------------------------------------------------------------------

fn checked(root: &Path, library: &Path) -> Vec<(Level, String)> {
    let manifest = read_it(library).unwrap();
    let mut target = target_of(
        Platform::Arduino,
        &PlatformArgs {
            sketchbook: Some(book(root).to_string_lossy().into_owned()),
            ..PlatformArgs::default()
        },
    );
    let mut ui = Watched::default();
    let where_ = target.project_root(&elsewhere(root), &mut ui).unwrap();

    target
        .check(&manifest, &where_, &mut ui)
        .into_iter()
        .map(|finding| (finding.level, finding.message))
        .collect()
}

#[test]
fn defines_are_said_to_have_no_place() {
    let root = tempfile::tempdir().unwrap();
    let library = demo(root.path(), "defines:\n  - DEMO_FAST\n  - DEMO_N=2\n");

    assert_eq!(
        checked(root.path(), &library),
        [(
            Level::Warn,
            "Arduino has no defines for a whole build, so these are not set: DEMO_FAST, DEMO_N=2"
                .to_string()
        )]
    );
}

#[test]
fn library_manager_libraries_are_looked_for() {
    let root = tempfile::tempdir().unwrap();

    fs::create_dir_all(book(root.path()).join("libraries/Adafruit_BusIO")).unwrap();

    let library = demo(
        root.path(),
        "arduino:\n  depends:\n    - Adafruit BusIO\n    - ArduinoJson (>=6.0.0)\nrequires:\n  peripherals:\n    - type: i2c\n      note: 400 kHz\n    - spi\n",
    );

    assert_eq!(
        checked(root.path(), &library),
        [
            (
                Level::Ok,
                "Adafruit BusIO, from Library Manager, is installed".to_string()
            ),
            (
                Level::Warn,
                "Needs ArduinoJson, from Library Manager".to_string()
            ),
            (Level::Info, "Needs I2C, 400 kHz".to_string()),
            (Level::Info, "Needs SPI".to_string()),
        ]
    );
}

#[test]
fn in_the_libraries_folder_a_dependency_is_not_mentioned() {
    let root = tempfile::tempdir().unwrap();
    let library = demo(root.path(), "requires:\n  libraries:\n    - helper\n");

    assert!(checked(root.path(), &library).is_empty());
}

#[test]
fn a_look_at_a_library_makes_no_folder() {
    // --check writes nothing, and the libraries folder of a sketchbook that has none is no exception.
    let root = tempfile::tempdir().unwrap();
    let mut request = asked(
        root.path(),
        &elsewhere(root.path()),
        &[&demo(root.path(), "")],
        None,
    );

    request.check_only = true;

    let (code, ui) = run(&request);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!book(root.path()).exists(), "a check made the sketchbook");
}

// ----------------------------------------------------------------------------
// library.properties
// ----------------------------------------------------------------------------

fn properties_of(root: &Path, body: &str) -> String {
    let library = read_it(&demo(root, body)).unwrap();
    let folder = root.join("written");

    fs::create_dir_all(&folder).unwrap();
    properties::write(&library, &folder);

    read(folder.join("library.properties"))
}

#[test]
fn with_no_arduino_part_everything_has_a_default() {
    let root = tempfile::tempdir().unwrap();
    let text = properties_of(root.path(), "");

    assert!(text.starts_with("# Generated by libnima"), "{text}");
    assert!(text.contains("\nname=demo\nversion=1.2.0\nauthor=someone\nmaintainer=someone\nsentence=A library for the tests\n"), "{text}");
    assert!(text.contains("\nparagraph=\ncategory=Timing\nurl=https://github.com/someone/demo\narchitectures=*\nincludes=demo.h\n"), "{text}");
    assert!(!text.contains("depends="), "{text}");
}

#[test]
fn the_arduino_part_is_written_as_given() {
    let root = tempfile::tempdir().unwrap();
    let text = properties_of(
        root.path(),
        "arduino:\n  architectures: [avr, esp32]\n  category: Sensors\n  author: Nima Askari\n  maintainer: Someone Else <x@example.com>\n  depends:\n    - Adafruit BusIO\n    - ArduinoJson (>=6.0.0)\n",
    );

    assert!(
        text.contains("author=Nima Askari\nmaintainer=Someone Else <x@example.com>\n"),
        "{text}"
    );
    assert!(text.contains("category=Sensors\n"), "{text}");
    assert!(text.contains("architectures=avr,esp32\n"), "{text}");
    assert!(
        text.contains("depends=Adafruit BusIO,ArduinoJson (>=6.0.0)\n"),
        "{text}"
    );
}

#[test]
fn the_category_is_the_nearest_of_arduinos() {
    for (category, expected) in [
        ("sensor", "Sensors"),
        ("storage", "Data Storage"),
        ("wireless", "Communication"),
        ("input", "Signal Input/Output"),
        ("math", "Data Processing"),
        ("rtos", "Other"),
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = demo(root.path(), "");
        let text = read(source.join("libnima.yml"))
            .replace("category: timing", &format!("category: {category}"));

        fs::write(source.join("libnima.yml"), text).unwrap();

        let folder = root.path().join("written");

        fs::create_dir_all(&folder).unwrap();
        properties::write(&read_it(&source).unwrap(), &folder);

        assert!(
            read(folder.join("library.properties")).contains(&format!("category={expected}\n")),
            "{category}"
        );
    }
}

#[test]
fn what_cannot_be_written_into_library_properties_is_refused() {
    for (body, says) in [
        ("arduino:\n  architectures:\n    - a b\n", "architectures"),
        ("arduino:\n  architectures: avr esp32\n", "architectures"),
        ("arduino:\n  category: Sensor\n", "Arduino's"),
        ("arduino:\n  author: \"two\\nlines\"\n", "one line"),
        ("arduino:\n  depends:\n    - bad;name\n", "depends"),
        ("arduino:\n  depends: Adafruit BusIO\n", "depends"),
        ("arduino: everything\n", "arduino should hold"),
        (
            "arduino:\n  architecture: avr\n",
            "arduino has architecture. It holds architectures, category",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let message = read_it(&demo(root.path(), body)).unwrap_err();

        assert!(message.contains(says), "{body}: {message}");
    }
}

#[test]
fn a_single_architecture_may_be_written_alone() {
    let root = tempfile::tempdir().unwrap();
    let found = read_it(&demo(root.path(), "arduino:\n  architectures: avr\n")).unwrap();

    assert_eq!(found.part.arduino().architectures, ["avr"]);
}

#[test]
fn a_repository_not_on_github_has_an_unknown_author() {
    let root = tempfile::tempdir().unwrap();
    let source = demo(root.path(), "");
    let text = read(source.join("libnima.yml")).replace(
        "https://github.com/someone/demo",
        "https://example.com/demo",
    );

    fs::write(source.join("libnima.yml"), text).unwrap();

    let folder = root.path().join("written");

    fs::create_dir_all(&folder).unwrap();
    properties::write(&read_it(&source).unwrap(), &folder);

    assert!(read(folder.join("library.properties")).contains("author=Unknown\n"));
}

#[test]
fn a_header_at_the_top_is_included_as_it_is() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("top");

    put(&folder, &[("demo.h", ""), ("demo.c", "")]);
    fs::write(
        folder.join("libnima.yml"),
        "name: demo\nplatform: arduino\nheaders:\n  - demo.h\nsources:\n  - demo.c\n",
    )
    .unwrap();

    assert_eq!(
        properties::include_name(&read_it(&folder).unwrap()).as_deref(),
        Some("demo.h")
    );
}

#[test]
fn a_library_of_no_headers_has_no_includes() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("sources");

    put(&folder, &[("src/demo.c", "")]);
    fs::write(
        folder.join("libnima.yml"),
        "name: demo\nplatform: arduino\nsources:\n  - src/demo.c\n",
    )
    .unwrap();

    let written = root.path().join("written");

    fs::create_dir_all(&written).unwrap();
    properties::write(&read_it(&folder).unwrap(), &written);

    assert!(!read(written.join("library.properties")).contains("includes="));
}

#[test]
fn it_is_written_then_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let library = read_it(&demo(root.path(), "")).unwrap();
    let folder = root.path().join("written");

    fs::create_dir_all(&folder).unwrap();

    assert_eq!(properties::write(&library, &folder).status, Status::Changed);
    assert_eq!(properties::write(&library, &folder).status, Status::Already);
}

#[test]
fn a_librarys_own_is_used_as_it_is_and_never_removed() {
    let root = tempfile::tempdir().unwrap();
    let library = read_it(&demo(root.path(), "")).unwrap();
    let folder = root.path().join("written");
    let own = "name=Demo\nversion=9.9.9\n";

    write(folder.join("library.properties"), own);

    assert_eq!(properties::write(&library, &folder).status, Status::Already);

    properties::remove(&folder);

    assert_eq!(read(folder.join("library.properties")), own);
}

#[test]
fn remove_takes_only_what_write_made() {
    let root = tempfile::tempdir().unwrap();
    let library = read_it(&demo(root.path(), "")).unwrap();
    let folder = root.path().join("written");

    fs::create_dir_all(&folder).unwrap();
    properties::write(&library, &folder);
    properties::remove(&folder);

    assert!(!folder.join("library.properties").exists());
}

// ----------------------------------------------------------------------------
// Taking it out
// ----------------------------------------------------------------------------

#[test]
fn out_of_the_libraries_folder_with_its_library_properties() {
    let root = tempfile::tempdir().unwrap();
    let project = elsewhere(root.path());
    let libraries = book(root.path()).join("libraries");

    install(root.path(), &project, &[&demo(root.path(), "")], None);
    uninstall(root.path(), &project, &["demo"]);

    assert!(!libraries.join("demo").exists());
    assert!(installed_libraries(&libraries).is_empty());
}

#[test]
fn out_of_a_sketch_without_being_told_where() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(
        root.path(),
        &folder,
        &[&demo(root.path(), "")],
        Some("sketch"),
    );
    uninstall(root.path(), &folder, &["demo"]);

    assert!(!folder.join("src/demo").exists());
    assert!(installed_libraries(&folder).is_empty());
}

#[test]
fn from_a_sketch_a_library_of_the_libraries_folder_is_found_there() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(
        root.path(),
        &folder,
        &[&demo(root.path(), "")],
        Some("libraries"),
    );
    uninstall(root.path(), &folder, &["demo"]);

    assert!(!folder.parent().unwrap().join("libraries/demo").exists());
}

#[test]
fn a_library_with_its_own_library_properties_comes_out_too() {
    let root = tempfile::tempdir().unwrap();
    let project = elsewhere(root.path());
    let library = lib_at(
        root.path(),
        "demo",
        "extras:\n  - library.properties\n",
        &[("library.properties", "name=demo\nversion=1.2.0\n")],
    );

    install(root.path(), &project, &[&library], None);
    uninstall(root.path(), &project, &["demo"]);

    assert!(!book(root.path()).join("libraries/demo/src").exists());
}

#[test]
fn names_in_both_places_are_refused_until_split() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(
        root.path(),
        &folder,
        &[&demo(root.path(), "")],
        Some("sketch"),
    );
    install(
        root.path(),
        &folder,
        &[&lib_at(root.path(), "helper", "", &[])],
        Some("libraries"),
    );

    let (code, ui) = run(&taking_out(root.path(), &folder, &["demo", "helper"]));

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("Take them out in two runs"),
        "{}",
        ui.err()
    );
    assert!(folder.join("src/demo").exists());
    assert!(folder.parent().unwrap().join("libraries/helper").exists());
}

// ----------------------------------------------------------------------------
// Files of the project's own, and steps
// ----------------------------------------------------------------------------

const ONCE: &str = "once:\n  - from: project/notes.txt\n    to: notes.txt\n";
const NOTES: [(&str, &str); 1] = [("project/notes.txt", "mine to edit\n")];

#[test]
fn in_a_sketch_a_once_file_lands_beside_the_ino() {
    let root = tempfile::tempdir().unwrap();
    let folder = sketch(root.path());

    install(
        root.path(),
        &folder,
        &[&lib_at(root.path(), "demo", ONCE, &NOTES)],
        Some("sketch"),
    );

    assert_eq!(read(folder.join("notes.txt")), "mine to edit\n");
}

#[test]
fn in_the_libraries_folder_a_once_file_is_refused_naming_sketch_mode() {
    let root = tempfile::tempdir().unwrap();
    let (code, ui) = run(&asked(
        root.path(),
        &elsewhere(root.path()),
        &[&lib_at(root.path(), "demo", ONCE, &NOTES)],
        Some("libraries"),
    ));

    assert_eq!(code, 2);
    assert!(ui.err().contains("--into sketch"), "{}", ui.err());
    assert!(!book(root.path()).join("libraries/demo").exists());
}

#[test]
fn in_the_libraries_folder_a_run_step_is_left_unrun() {
    let root = tempfile::tempdir().unwrap();
    let library = demo(root.path(), "run:\n  - label: k\n    command: [x]\n");
    let mut request = asked(
        root.path(),
        &elsewhere(root.path()),
        &[&library],
        Some("libraries"),
    );

    request.yes = true;

    let (code, ui) = run(&request);

    assert_eq!(code, 0, "{}", ui.err());
    says(&ui, "not run, there is no project folder here to run it in");
    says(&ui, "--into sketch");
    assert!(
        book(root.path())
            .join("libraries/demo/library.properties")
            .is_file()
    );
}

#[test]
fn a_once_file_naming_a_sketch_file_is_refused() {
    for to in ["blink.ino", "other.ino"] {
        let root = tempfile::tempdir().unwrap();
        let folder = sketch(root.path());
        let library = lib_at(
            root.path(),
            "demo",
            &format!("once:\n  - from: project/notes.txt\n    to: {to}\n"),
            &NOTES,
        );
        let (code, ui) = run(&asked(root.path(), &folder, &[&library], Some("sketch")));

        assert_eq!(code, 2, "{to}");
        assert!(
            ui.err().contains("edits or reads as the project's own"),
            "{to}: {}",
            ui.err()
        );
    }
}
