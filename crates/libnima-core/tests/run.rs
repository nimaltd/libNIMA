//! A whole run: what it is given decides where a library comes from, what goes in first, which parts it takes,
//! and what is run once it is in.

mod common;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use common::run_with as run;
use common::*;
use libnima_core::download::Client;
use libnima_core::flow::{self, Request};

/// Stand in for GitHub with libraries on disk. A bare name is under nimaltd, as a run takes it to be.
fn github(repos: &[(&str, &Path)]) -> (Client, Github) {
    let server = Github::serving(repos);

    (server.client(), server)
}

/// A GitHub that holds nothing: what a library names and nobody has is a 404.
fn offline() -> Client {
    github(&[]).0
}

/// A library on disk, with a header named after it, and what it needs.
fn lib(where_: &Path, name: &str, version: &str, needs: &[&str]) -> PathBuf {
    let mut body = format!("version: {version}\nheaders:\n  - inc/{name}.h\n");

    if !needs.is_empty() {
        let listed: Vec<String> = needs.iter().map(|need| format!("    - {need}\n")).collect();
        body.push_str(&format!("requires:\n  libraries:\n{}", listed.concat()));
    }

    library(
        where_,
        name,
        &body,
        &[(&format!("inc/{name}.h"), "/* header */\n")],
    )
}

/// Zip a library the way GitHub's Download ZIP does: under one top folder.
fn zip_of(folder: &Path, archive: &Path, top: &str) -> PathBuf {
    fs::create_dir_all(archive.parent().unwrap()).unwrap();

    let mut bundle = zip::ZipWriter::new(fs::File::create(archive).unwrap());

    for path in libnima_core::paths::files_below(folder) {
        bundle
            .start_file(
                format!("{top}/{path}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        bundle
            .write_all(&fs::read(folder.join(&path)).unwrap())
            .unwrap();
    }

    bundle.finish().unwrap();

    archive.to_path_buf()
}

fn listing(folder: &Path) -> Vec<String> {
    libnima_core::paths::files_below(folder)
}

fn position(out: &str, wanted: &str) -> usize {
    out.find(wanted)
        .unwrap_or_else(|| panic!("{wanted} is not in:\n{out}"))
}

// ----------------------------------------------------------------------------
// Where a library comes from
// ----------------------------------------------------------------------------

#[test]
fn a_zip_with_no_library_in_it_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = root.path().join("other");
    put(&folder, &[("readme.txt", "not a library")]);
    let archive = zip_of(&folder, &root.path().join("other.zip"), "other-main");

    let (code, ui) = run(offline(), &request(&project, &[&archive]));

    assert_eq!(code, 2);
    assert!(ui.err().contains("holds no libnima.yml"), "{}", ui.err());
}

#[test]
fn a_file_that_is_not_a_zip_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let notes = root.path().join("notes.txt");
    write(&notes, "hello\n");

    let (code, ui) = run(offline(), &request(&project, &[&notes]));

    assert_eq!(code, 2);
    assert!(ui.err().contains("not a .zip"), "{}", ui.err());
}

#[test]
fn a_zip_that_cannot_be_read_is_said_to_be_one_not_an_empty_one() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = lib(&root.path().join("disk"), "liba", "1.0.0", &[]);
    let whole = zip_of(&folder, &root.path().join("liba-main.zip"), "liba-main");
    let bytes = fs::read(&whole).unwrap();

    for (name, data) in [
        ("truncated.zip", &bytes[..bytes.len() / 2]),
        ("empty.zip", &b""[..]),
        ("notazip.zip", &b"<html>Not Found</html>"[..]),
    ] {
        let archive = root.path().join(name);
        fs::write(&archive, data).unwrap();

        let (code, ui) = run(offline(), &request(&project, &[&archive]));

        assert_eq!(code, 2, "{name}");
        assert!(
            ui.err().contains("cannot be read as a zip") && ui.err().contains("download it again"),
            "{name}: {}",
            ui.err()
        );
        assert!(!ui.err().contains("holds no"), "{name}: {}", ui.err());
        assert!(record(&project).is_empty());
    }
}

#[test]
fn a_ref_with_nothing_fetched_from_github_is_refused_before_anything() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = lib(&root.path().join("disk"), "liba", "1.0.0", &[]);
    let archive = zip_of(&folder, &root.path().join("liba-main.zip"), "liba-main");

    for given in [&folder, &archive] {
        let mut asked = request(&project, &[given]);
        asked.reference = Some("v9.9.9".to_string());

        let (code, ui) = run(offline(), &asked);

        assert_eq!(code, 2, "{}", ui.out());
        assert!(
            ui.err()
                .contains("--ref v9.9.9 is for a library fetched from GitHub"),
            "{}",
            ui.err()
        );
        assert!(record(&project).is_empty());
    }
}

#[test]
fn an_address_not_on_github_is_refused_and_an_ssh_one_is_read() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let liba = lib(&root.path().join("gh"), "liba", "1.0.0", &[]);

    for given in [
        "https://gitlab.com/tester/liba",
        "git@gitlab.com:tester/liba.git",
    ] {
        let (client, server) = github(&[("tester/liba", &liba)]);
        let mut asked = request(&project, &[]);
        asked.libraries = vec![given.to_string()];

        let (code, ui) = run(client, &asked);

        assert_eq!(code, 2, "{given}");
        assert!(
            ui.err().contains("is not on GitHub"),
            "{given}: {}",
            ui.err()
        );
        assert!(server.repos_asked().is_empty(), "{given} reached GitHub");
    }

    let (client, server) = github(&[("tester/liba", &liba)]);
    let mut asked = request(&project, &[]);
    asked.libraries = vec!["git@github.com:tester/liba.git".to_string()];
    asked.folder = Some("liba".to_string());

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert_eq!(server.repos_asked(), ["tester/liba"]);
    assert!(project.join("liba/inc/liba.h").is_file());
}

#[test]
fn a_zip_goes_in_without_being_unpacked_first() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let archive = zip_of(
        &source,
        &root.path().join("Downloads/demo-main.zip"),
        "demo-main",
    );

    let (code, ui) = run(offline(), &request(&project, &[&archive]));

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("demo/inc/demo.h").is_file());
    assert_eq!(record(&project)["demo"]["origin"]["kind"], "zip");
    assert_eq!(record(&project)["demo"]["origin"]["path"], "demo-main.zip");
}

#[test]
fn a_folder_outside_the_project_is_copied_and_left_alone() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        &root.path().join("Downloads"),
        "demo-master",
        DEMO_CODE,
        &[],
    );
    let before = listing(&source);

    let mut asked = request(&project, &[&source]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("demo/inc/demo.h").is_file());
    assert_eq!(listing(&source), before, "the source folder was changed");
}

#[test]
fn the_folder_windows_extract_all_makes_is_found() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let outer = root.path().join("Downloads/demo-master");
    library(&outer, "demo-master", DEMO_CODE, &[]);

    let mut asked = request(&project, &[&outer]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("demo/inc/demo.h").is_file());
}

#[test]
fn dir_is_refused_for_a_folder_already_in_the_project() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let folder = library(&project, "demo-master", DEMO_CODE, &[]);

    let mut asked = request(&project, &[&folder]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(ui.err().contains("--dir has nothing to do"), "{}", ui.err());
    assert!(
        folder.join("libnima.yml").is_file(),
        "the folder was converted anyway"
    );
}

#[test]
fn a_name_goes_to_github() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let staged = library(root.path(), "demo", DEMO_CODE, &[]);
    let (client, server) = github(&[("nimaltd/demo", &staged)]);

    let asked = Request {
        libraries: vec!["nimaltd/demo".to_string()],
        reference: Some("2.0.0".to_string()),
        folder: Some("demo".to_string()),
        project: Some(project.clone()),
        ..Request::default()
    };

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(server.repos_asked(), ["nimaltd/demo"]);
    assert!(server.seen().iter().all(|seen| seen.reference() == "2.0.0"));
    assert!(project.join("demo/inc/demo.h").is_file());
}

#[test]
fn a_path_that_does_not_exist_is_not_sent_to_github() {
    for given in [
        "D:/Downloads/no-such-library-9f3.zip",
        "./no-such-library-9f3",
        "Libs/no-such/library-9f3",
        "C:\\no-such-folder-9f3\\demo",
    ] {
        let root = tempfile::tempdir().unwrap();
        let project = stm32_project(root.path());
        let (client, server) = github(&[]);
        let asked = Request {
            libraries: vec![given.to_string()],
            project: Some(project),
            ..Request::default()
        };

        let (code, ui) = run(client, &asked);

        assert_eq!(code, 2, "{given}");
        assert!(ui.err().contains("does not exist"), "{}", ui.err());
        assert!(server.seen().is_empty(), "{given} went to GitHub");
    }
}

#[test]
fn a_mistyped_path_is_told_from_a_library_name() {
    assert!(flow::looks_like_path("D:/x.zip"));
    assert!(flow::looks_like_path("./x"));
    assert!(flow::looks_like_path("a/b/c"));
    assert!(flow::looks_like_path("C:\\x"));
    assert!(flow::looks_like_path("x.ZIP"));
    assert!(flow::looks_like_path("~/x"));
    assert!(!flow::looks_like_path("owner/name"));
    assert!(!flow::looks_like_path("name"));
    assert!(!flow::looks_like_path("owner/name@feature/x"));
    assert!(!flow::looks_like_path("https://github.com/owner/name"));
}

// ----------------------------------------------------------------------------
// What it says at the end
// ----------------------------------------------------------------------------

#[test]
fn the_last_line_names_the_real_header() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "sequencer",
        "headers: [src/seq.h]\nsources: [src/seq.c]\n",
        &[("src/seq.h", ""), ("src/seq.c", "")],
    );

    let ui = installed(&project, &source, "sequencer");

    assert!(ui.out().contains("#include \"seq.h\""), "{}", ui.out());
    assert!(!ui.out().contains("sequencer.h"), "{}", ui.out());
}

#[test]
fn the_last_line_is_relative_to_the_include_folder() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);

    assert!(
        installed(&project, &source, "demo")
            .out()
            .contains("#include \"demo.h\"")
    );
}

#[test]
fn the_folder_defaults_to_the_library_name() {
    // Not the name of the zip or folder it came from, which carries -master.
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let holder = root.path().join("Downloads");
    let source = holder.join("demo-master");
    library(&holder.join("x"), "demo", DEMO_CODE, &[]);
    fs::rename(holder.join("x/demo"), &source).unwrap();

    let (code, ui) = run(offline(), &request(&project, &[&source]));

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("demo/inc/demo.h").is_file());
}

// ----------------------------------------------------------------------------
// What a library needs
// ----------------------------------------------------------------------------

#[test]
fn what_a_library_needs_is_installed_first() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let gh = root.path().join("gh");
    let ee24 = lib(&gh, "ee24", "1.0.0", &["osal >= 1.0.0"]);
    let osal = lib(&gh, "osal", "1.0.0", &[]);
    let (client, server) = github(&[("nimaltd/ee24", &ee24), ("osal", &osal)]);
    let asked = Request {
        libraries: vec!["nimaltd/ee24".to_string()],
        folder: Some("ee24".to_string()),
        project: Some(project.clone()),
        ..Request::default()
    };

    let (code, ui) = run(client, &asked);
    let out = ui.out();

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(server.repos_asked(), ["nimaltd/ee24", "nimaltd/osal"]);
    assert!(
        server.seen().iter().all(|seen| seen.reference() == "HEAD"),
        "a dependency took a ref"
    );
    assert!(
        project.join("osal/inc/osal.h").is_file(),
        "the folder question defaults to the name"
    );
    assert!(project.join("ee24/inc/ee24.h").is_file());
    assert_eq!(record(&project)["osal"]["folder"], "osal");
    assert_eq!(record(&project)["ee24"]["folder"], "ee24");
    assert!(position(&out, "Installing osal first") < position(&out, "#include \"osal.h\""));
    assert!(position(&out, "#include \"osal.h\"") < position(&out, "#include \"ee24.h\""));
}

#[test]
fn each_library_of_a_run_is_a_part_of_its_own() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let gh = root.path().join("gh");
    let ee24 = lib(&gh, "ee24", "1.0.0", &["osal >= 1.0.0"]);
    let osal = lib(&gh, "osal", "1.0.0", &[]);
    let (client, _server) = github(&[("nimaltd/ee24", &ee24), ("osal", &osal)]);
    let asked = Request {
        libraries: vec!["nimaltd/ee24".to_string()],
        project: Some(project.clone()),
        ..Request::default()
    };

    let (code, ui) = run(client, &asked);
    let parts = ui.parts();

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(parts.len(), 3, "{parts:#?}");
    assert!(parts[0].contains("Fetching nimaltd/ee24"), "{}", parts[0]);
    assert!(!parts[0].contains("#include"), "{}", parts[0]);

    assert!(
        parts[1].contains("Installing osal first"),
        "the reason it came in goes with it: {}",
        parts[1]
    );
    assert!(parts[1].contains("#include \"osal.h\""), "{}", parts[1]);
    assert!(!parts[1].contains("#include \"ee24.h\""), "{}", parts[1]);

    assert!(parts[2].contains("#include \"ee24.h\""), "{}", parts[2]);
    assert!(!parts[2].contains("#include \"osal.h\""), "{}", parts[2]);

    let (client, _server) = github(&[]);
    let (code, ui) = run(
        client,
        &Request {
            uninstall: vec!["ee24".to_string(), "osal".to_string()],
            project: Some(project.clone()),
            ..Request::default()
        },
    );
    let parts = ui.parts();

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(parts.len(), 3, "{parts:#?}");
    assert!(
        parts[1].contains("ee24 is out of the project"),
        "{}",
        parts[1]
    );
    assert!(
        parts[2].contains("osal is out of the project"),
        "{}",
        parts[2]
    );
}

#[test]
fn what_is_already_there_is_kept_and_nobody_is_asked() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let osal = lib(&root.path().join("disk"), "osal", "1.2.0", &[]);

    installed(&project, &osal, "Libs/osal");

    let ee24 = lib(&root.path().join("gh"), "ee24", "1.0.0", &["osal >= 1.1.0"]);
    let (client, server) = github(&[("nimaltd/ee24", &ee24)]);
    let asked = Request {
        libraries: vec!["nimaltd/ee24".to_string()],
        folder: Some("ee24".to_string()),
        project: Some(project.clone()),
        ..Request::default()
    };

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        ui.out()
            .contains("osal 1.2.0 is already in this project, kept"),
        "{}",
        ui.out()
    );
    assert_eq!(server.repos_asked(), ["nimaltd/ee24"]);
    assert!(
        !project.join("osal").exists(),
        "a second copy of osal went in"
    );
    assert_eq!(record(&project)["osal"]["folder"], "Libs/osal");
    assert!(ui.asked.is_empty(), "{:?}", ui.asked);
}

#[test]
fn a_copy_given_in_the_run_is_installed_even_when_one_will_do() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let old = lib(&root.path().join("v1"), "osal", "1.0.0", &[]);

    installed(&project, &old, "Libs/osal");

    let ee24 = lib(&root.path().join("disk"), "ee24", "1.0.0", &["osal"]);
    let newer = lib(&root.path().join("v2"), "osal", "1.1.0", &[]);

    let (code, ui) = run(offline(), &request(&project, &[&ee24, &newer]));

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(
        record(&project)["osal"]["version"],
        "1.1.0",
        "the copy given was not installed"
    );
    assert_eq!(record(&project)["osal"]["folder"], "Libs/osal");
}

#[test]
fn a_copy_given_over_one_that_will_do_is_not_called_an_update() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let have = lib(&root.path().join("v1"), "osal", "1.2.0", &[]);

    installed(&project, &have, "Libs/osal");

    let ee24 = lib(
        &root.path().join("disk"),
        "ee24",
        "1.0.0",
        &["osal >= 1.1.0"],
    );
    let again = lib(&root.path().join("v2"), "osal", "1.2.0", &[]);

    let mut asked = request(&project, &[&ee24, &again]);
    asked.yes = true;

    let (code, ui) = run(offline(), &asked);
    let out = ui.out();

    assert_eq!(code, 0, "{}", ui.err());
    assert!(out.contains("the 1.2.0 in this project will do"), "{out}");
    assert!(
        out.contains("osal 1.2.0 was given on the command line"),
        "{out}"
    );
    assert!(!out.contains("newer than the"), "{out}");
    assert!(!out.contains("Updating osal"), "{out}");
    assert_eq!(record(&project)["osal"]["folder"], "Libs/osal");
}

#[test]
fn what_is_too_old_is_updated_where_it_is() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let config = "/* USER CODE BEGIN C */\n/* USER CODE END C */\n";
    let old = library(
        &root.path().join("v1"),
        "osal",
        "version: 1.0.0\nheaders: [inc/osal.h]\n",
        &[("inc/osal.h", config)],
    );

    installed(&project, &old, "Libs/osal");
    write(
        project.join("Libs/osal/inc/osal.h"),
        "/* USER CODE BEGIN C */\n#define MINE 1\n/* USER CODE END C */\n",
    );

    let newer = library(
        &root.path().join("v2"),
        "osal",
        "version: 1.1.0\nheaders: [inc/osal.h]\n",
        &[("inc/osal.h", config)],
    );
    let ee24 = lib(
        &root.path().join("disk"),
        "ee24",
        "1.0.0",
        &["osal >= 1.1.0"],
    );
    let (client, _) = github(&[("osal", &newer)]);
    let mut asked = request(&project, &[&ee24]);
    asked.folder = Some("ee24".to_string());

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(ui.out().contains("Updating osal first"), "{}", ui.out());
    assert_eq!(record(&project)["osal"]["version"], "1.1.0");
    assert_eq!(
        record(&project)["osal"]["folder"],
        "Libs/osal",
        "the update went somewhere else"
    );
    assert!(!project.join("osal").exists());
    assert!(
        read(project.join("Libs/osal/inc/osal.h")).contains("#define MINE 1"),
        "the user's settings were overwritten"
    );
}

#[test]
fn what_cannot_be_had_stops_the_run_before_anything_is_written() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let before = listing(&project);
    let ee24 = lib(&root.path().join("disk"), "ee24", "1.0.0", &["osal"]);
    let mut asked = request(&project, &[&ee24]);
    asked.folder = Some("ee24".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(ui.err().contains("ee24 needs osal"), "{}", ui.err());
    assert!(
        ui.err().contains("give both zips on one line"),
        "{}",
        ui.err()
    );
    assert_eq!(
        listing(&project),
        before,
        "the project was changed by a run that could not finish"
    );
}

#[test]
fn the_newest_to_be_had_must_be_new_enough() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let osal = lib(&root.path().join("gh"), "osal", "1.0.0", &[]);
    let ee24 = lib(
        &root.path().join("disk"),
        "ee24",
        "1.0.0",
        &["osal >= 2.0.0"],
    );
    let (client, _) = github(&[("osal", &osal)]);
    let mut asked = request(&project, &[&ee24]);
    asked.folder = Some("ee24".to_string());

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("the newest osal to be had is 1.0.0"),
        "{}",
        ui.err()
    );
    assert!(record(&project).is_empty());
}

#[test]
fn zips_given_together_install_without_the_internet() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let src = root.path().join("src");
    let downloads = root.path().join("Downloads");
    let ee24 = zip_of(
        &lib(&src, "ee24", "1.0.0", &["osal"]),
        &downloads.join("ee24-master.zip"),
        "ee24-master",
    );
    let osal = zip_of(
        &lib(&src, "osal", "1.0.0", &[]),
        &downloads.join("osal-master.zip"),
        "osal-master",
    );
    let (client, server) = github(&[]);

    let (code, ui) = run(client, &request(&project, &[&ee24, &osal]));
    let out = ui.out();

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        server.seen().is_empty(),
        "went online for a library given as a zip"
    );
    assert!(project.join("osal/inc/osal.h").is_file());
    assert!(project.join("ee24/inc/ee24.h").is_file());
    assert!(
        position(&out, "#include \"osal.h\"") < position(&out, "#include \"ee24.h\""),
        "osal did not go in first"
    );
    assert_eq!(
        out.matches("#include \"osal.h\"").count(),
        1,
        "osal went in twice"
    );
}

#[test]
fn what_a_needed_library_needs_goes_in_before_it() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let gh = root.path().join("gh");
    let osal = lib(&gh, "osal", "1.0.0", &["base"]);
    let base = lib(&gh, "base", "1.0.0", &[]);
    let ee24 = lib(&root.path().join("disk"), "ee24", "1.0.0", &["osal"]);
    let (client, _) = github(&[("osal", &osal), ("base", &base)]);
    let mut asked = request(&project, &[&ee24]);
    asked.folder = Some("ee24".to_string());

    let (code, ui) = run(client, &asked);
    let out = ui.out();

    assert_eq!(code, 0, "{}", ui.err());
    assert!(position(&out, "#include \"base.h\"") < position(&out, "#include \"osal.h\""));
    assert!(position(&out, "#include \"osal.h\"") < position(&out, "#include \"ee24.h\""));
}

#[test]
fn a_library_two_others_need_goes_in_once() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let osal = lib(&root.path().join("gh"), "osal", "1.0.0", &[]);
    let disk = root.path().join("disk");
    let ee24 = lib(&disk, "ee24", "1.0.0", &["osal"]);
    let spif = lib(&disk, "spif", "1.0.0", &["osal"]);
    let (client, server) = github(&[("osal", &osal)]);

    let (code, ui) = run(client, &request(&project, &[&ee24, &spif]));

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(
        server
            .seen()
            .iter()
            .filter(|seen| seen.file() == "libnima.yml")
            .count(),
        1
    );
    assert_eq!(ui.out().matches("#include \"osal.h\"").count(), 1);
    assert!(project.join("spif/inc/spif.h").is_file());
}

#[test]
fn libraries_that_need_each_other_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let disk = root.path().join("disk");
    let first = lib(&disk, "first", "1.0.0", &["second"]);
    let second = lib(&disk, "second", "1.0.0", &["first"]);

    let (code, ui) = run(offline(), &request(&project, &[&first, &second]));

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("first -> second -> first"),
        "{}",
        ui.err()
    );
    assert!(record(&project).is_empty());
}

#[test]
fn dir_is_refused_with_more_than_one_library() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let disk = root.path().join("disk");
    let one = lib(&disk, "one", "1.0.0", &[]);
    let two = lib(&disk, "two", "1.0.0", &[]);
    let mut asked = request(&project, &[&one, &two]);
    asked.folder = Some("x".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(
        ui.err().contains("--dir names the folder for one library"),
        "{}",
        ui.err()
    );
}

#[test]
fn a_dependency_with_no_manifest_comes_with_the_one_carried_for_it() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let bare = root.path().join("gh/lfs");
    put(&bare, &[("lfs.h", "")]);
    let needs = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}requires:\n  libraries:\n    - source: someone/lfs@v2\n      manifest: manifests/lfs.yml\n      minimum: 2.0.0\n"
        ),
        &[(
            "manifests/lfs.yml",
            "name: lfs\nplatform: stm32\nversion: 2.1.0\nheaders: [lfs.h]\n",
        )],
    );
    let (client, server) = github(&[("someone/lfs", &bare)]);
    let mut asked = request(&project, &[&needs]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert_eq!(server.repos_asked(), ["someone/lfs"]);
    assert!(server.seen().iter().all(|seen| seen.reference() == "v2"));
    assert!(project.join("lfs/lfs.h").is_file());
    assert_eq!(record(&project)["lfs"]["version"], "2.1.0");
}

/// A library of a version, in a folder of its own under where_/tag, with more of its libnima.yml.
fn lib_at(where_: &Path, tag: &str, name: &str, version: &str, more: &str) -> PathBuf {
    library(
        &where_.join(tag),
        name,
        &format!("version: {version}\nheaders:\n  - inc/{name}.h\n{more}"),
        &[(&format!("inc/{name}.h"), "/* header */\n")],
    )
}

/// A copy of a folder, file by file.
fn copy_tree(from: &Path, to: &Path) {
    for path in listing(from) {
        let target = to.join(&path);

        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::copy(from.join(&path), target).unwrap();
    }
}

/// What a library needs, as its libnima.yml lists it.
fn needing(lines: &[&str]) -> String {
    let listed: Vec<String> = lines.iter().map(|line| format!("    {line}\n")).collect();

    format!("requires:\n  libraries:\n{}", listed.concat())
}

/// The record's list of what a library needs.
fn needs_of(project: &Path, name: &str) -> Vec<String> {
    strings(&record(project)[name]["needs"])
}

/// core 0.2.0, in the repository tester/fw-core.
fn core_from_repository(root: &Path, version: &str) -> PathBuf {
    lib_at(
        root,
        &format!("core-{version}"),
        "core",
        version,
        "repository: https://github.com/tester/fw-core\n",
    )
}

#[test]
fn the_library_from_the_repository_a_requirement_names_meets_it_whatever_its_name() {
    let root = tempfile::tempdir().unwrap();
    let disk = root.path().join("disk");
    let core = core_from_repository(&disk, "0.2.0");

    for need in [
        "- tester/fw-core@v0.2.0",
        "- tester/fw-core",
        "- source: tester/fw-core\n      minimum: 0.2.0",
    ] {
        let project = stm32_project(&root.path().join(need.len().to_string()));
        let addon = lib_at(
            &disk,
            &need.len().to_string(),
            "addon",
            "1.0.0",
            &needing(&[need]),
        );

        installed(&project, &core, "core");

        let (code, ui) = run(offline(), &request(&project, &[&addon]));

        assert_eq!(code, 0, "{need}: {}\n{}", ui.out(), ui.err());
        assert!(
            ui.out()
                .contains("core 0.2.0 is already in this project, kept"),
            "{need}: {}",
            ui.out()
        );
        assert!(!ui.out().contains("Fetching"), "{need}: {}", ui.out());
        // What it needs is what is installed, so core cannot go out from under it.
        assert_eq!(needs_of(&project, "addon"), ["core"], "{need}");

        let (code, ui) = uninstall(&project, &["core"]);

        assert_eq!(code, 2, "{need}: {}", ui.out());
        assert!(
            ui.err().contains("addon needs core"),
            "{need}: {}",
            ui.err()
        );
    }
}

#[test]
fn the_library_from_the_repository_given_on_the_line_meets_a_requirement() {
    let root = tempfile::tempdir().unwrap();
    let disk = root.path().join("disk");
    let core = core_from_repository(&disk, "0.2.0");
    let addon = lib_at(
        &disk,
        "addon",
        "addon",
        "1.0.0",
        &needing(&["- tester/fw-core@v0.2.0"]),
    );

    for (tag, given) in [("a", [&addon, &core]), ("b", [&core, &addon])] {
        let project = stm32_project(&root.path().join(tag));
        let given: Vec<&Path> = given.iter().map(|path| path.as_path()).collect();
        let (code, ui) = run(offline(), &request(&project, &given));

        assert_eq!(code, 0, "{tag}: {}\n{}", ui.out(), ui.err());
        assert!(project.join("core/inc/core.h").is_file(), "{tag}");
        assert_eq!(needs_of(&project, "addon"), ["core"], "{tag}");
    }
}

#[test]
fn a_library_found_through_its_repository_still_has_to_be_new_enough() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let disk = root.path().join("disk");
    let addon = lib_at(
        &disk,
        "addon",
        "addon",
        "1.0.0",
        &needing(&["- source: tester/fw-core\n      minimum: 0.3.0"]),
    );

    installed(&project, &core_from_repository(&disk, "0.2.0"), "core");

    let (code, ui) = run(offline(), &request(&project, &[&addon]));

    assert_eq!(code, 2, "{}", ui.out());
    // Too old, which is not missing: named with its version, and a newer one looked for.
    assert!(ui.out().contains("Fetching tester/fw-core"), "{}", ui.out());
    assert!(
        ui.err().contains("core 0.2.0 is the one in this project"),
        "{}",
        ui.err()
    );
    assert!(!ui.err().contains("not in this project"), "{}", ui.err());
    assert!(!record(&project).contains_key("addon"));
}

#[test]
fn a_fetched_library_already_installed_under_its_name_is_kept_not_put_back() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let disk = root.path().join("disk");
    // Installed from somewhere else, at a version newer than the repository's.
    let mine = lib_at(
        &disk,
        "mine",
        "example",
        "9.9.9",
        "repository: https://github.com/me/fork\n",
    );
    let theirs = lib_at(&root.path().join("gh"), "x", "example", "1.4.0", "");
    let addon = lib_at(
        &disk,
        "addon",
        "addon",
        "1.0.0",
        &needing(&["- someone/lib-example"]),
    );
    let (client, server) = github(&[("someone/lib-example", &theirs)]);

    installed(&project, &mine, "example");

    let (code, ui) = run(client, &request(&project, &[&addon]));

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert_eq!(server.repos_asked(), ["someone/lib-example"]);
    assert!(
        ui.out()
            .contains("example 9.9.9 is already in this project, kept"),
        "{}",
        ui.out()
    );
    assert_eq!(record(&project)["example"]["version"], "9.9.9");
    assert_eq!(needs_of(&project, "addon"), ["example"]);
}

#[test]
fn a_record_that_needs_a_repository_by_name_still_keeps_the_library_from_it() {
    // libNIMA 1.0.0 recorded the repository's name as what was needed.
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let gh = lib_at(&root.path().join("gh"), "x", "example", "1.4.0", "");
    let addon = lib_at(
        &root.path().join("disk"),
        "addon",
        "addon",
        "1.0.0",
        &needing(&["- someone/lib-example"]),
    );
    let (client, _server) = github(&[("someone/lib-example", &gh)]);

    assert_eq!(run(client, &request(&project, &[&addon])).0, 0);

    let path = project.join(".libnima.json");
    let text = read(&path).replace("\"example\"\n", "\"lib-example\"\n");
    write(&path, &text);
    assert_eq!(needs_of(&project, "addon"), ["lib-example"]);

    let (code, ui) = uninstall(&project, &["example"]);

    assert_eq!(code, 2, "{}", ui.out());
    assert!(ui.err().contains("addon needs example"), "{}", ui.err());
}

#[test]
fn what_is_needed_is_said_by_name_and_version_and_the_zips_by_repository() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let pmic = lib_at(
        &root.path().join("disk"),
        "pmic",
        "gpmic",
        "1.0.0",
        &format!(
            "repository: https://github.com/tester/fw-pmic\n{}",
            needing(&["- source: tester/core-fw@v0.2.0\n      minimum: 0.2.0\n      name: gcore"])
        ),
    );

    let (code, ui) = run(offline(), &request(&project, &[&pmic]));
    let said = format!("{}\n{}", ui.out(), ui.err());

    assert_eq!(code, 2);
    assert!(said.contains("gpmic needs gcore >= 0.2.0"), "{said}");
    assert!(!said.contains("gcore@v0.2.0"), "{said}");
    assert!(said.contains("Fetching tester/core-fw@v0.2.0"), "{said}");
    assert!(
        said.contains("libnima fw-pmic-main.zip core-fw-0.2.0.zip"),
        "{said}"
    );
}

#[test]
fn an_update_that_closes_a_circle_is_refused_and_a_circle_named_in_full_goes() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let disk = root.path().join("disk");
    let alone = lib_at(&disk, "ca0", "ca", "1.0.0", "");
    let ca = lib_at(&disk, "ca", "ca", "1.0.0", &needing(&["- cb >= 1.0.0"]));
    let cb = lib_at(&disk, "cb", "cb", "1.0.0", &needing(&["- ca >= 1.0.0"]));

    installed(&project, &alone, "ca");
    installed(&project, &cb, "cb");

    let before = read(project.join(".libnima.json"));
    let (code, ui) = run(offline(), &request(&project, &[&ca]));

    assert_eq!(code, 2, "{}", ui.out());
    assert!(ui.err().contains("ca -> cb -> ca"), "{}", ui.err());
    assert_eq!(read(project.join(".libnima.json")), before);

    // As 1.0.0 let it in: ca needs cb, and cb needs ca.
    let path = project.join(".libnima.json");
    let mut data: serde_json::Value = serde_json::from_str(&before).unwrap();
    data["libraries"]["ca"]["needs"] = serde_json::json!(["cb"]);
    write(&path, &serde_json::to_string_pretty(&data).unwrap());

    let (code, ui) = uninstall(&project, &["ca"]);

    assert_eq!(code, 2, "{}", ui.out());
    assert!(ui.err().contains("cb needs ca"), "{}", ui.err());

    let (code, ui) = uninstall(&project, &["cb", "ca"]);

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert!(record(&project).is_empty());
}

#[test]
fn one_library_given_twice_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let disk = root.path().join("disk");
    let old = lib_at(&disk, "a9", "liba", "0.9.0", "");
    let new = lib_at(&disk, "a10", "liba", "0.10.0", "");
    let user = lib_at(&disk, "b", "libb", "1.0.0", &needing(&["- liba >= 0.10.0"]));

    for check_only in [false, true] {
        let mut asked = request(&project, &[&user, &old, &new]);
        asked.check_only = check_only;

        let (code, ui) = run(offline(), &asked);

        assert_ne!(code, 0, "{}", ui.out());
        assert!(
            ui.err()
                .contains("liba is given 2 times in this run, at 0.9.0 and 0.10.0"),
            "{}",
            ui.err()
        );
        assert!(!ui.err().contains("newest"), "{}", ui.err());
        assert!(record(&project).is_empty());
    }

    assert_eq!(run(offline(), &request(&project, &[&user, &new])).0, 0);
}

#[test]
fn only_a_library_that_came_in_for_the_one_taken_out_is_named() {
    let root = tempfile::tempdir().unwrap();
    let disk = root.path().join("disk");
    let core = lib_at(&disk, "core", "core", "1.0.0", "");
    let addon = lib_at(
        &disk,
        "addon",
        "addon",
        "1.0.0",
        &needing(&["- core >= 1.0.0"]),
    );
    let names_core = |ui: &Watched| ui.out().contains("--uninstall core");

    // Installed on its own: the user's, never offered to go.
    let first = stm32_project(&root.path().join("first"));
    installed(&first, &core, "core");
    installed(&first, &addon, "addon");
    let (code, ui) = uninstall(&first, &["addon"]);
    assert_eq!(code, 0);
    assert!(!names_core(&ui), "{}", ui.out());

    // Given with it to meet its requirement: it came in for it.
    let second = stm32_project(&root.path().join("second"));
    assert_eq!(run(offline(), &request(&second, &[&addon, &core])).0, 0);
    assert_eq!(strings(&record(&second)["core"]["brought_by"]), ["addon"]);
    let third = root.path().join("third");
    copy_tree(&second, &third);

    let (code, ui) = uninstall(&second, &["addon"]);
    assert_eq!(code, 0);
    assert!(names_core(&ui), "{}", ui.out());

    // Taken out in the same run: nothing is left to name.
    let (code, ui) = uninstall(&third, &["core", "addon"]);
    assert_eq!(code, 0, "{}", ui.err());
    assert!(!names_core(&ui), "{}", ui.out());
}

#[test]
fn an_install_below_what_an_installed_library_needs_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let disk = root.path().join("disk");
    let new = lib_at(&disk, "a1", "liba", "1.0.0", "");
    let old = lib_at(&disk, "a09", "liba", "0.9.0", "");
    let user = lib_at(&disk, "b", "libb", "1.0.0", &needing(&["- liba >= 1.0.0"]));

    let project = stm32_project(&root.path().join("needed"));
    assert_eq!(run(offline(), &request(&project, &[&user, &new])).0, 0);
    let before = read(project.join(".libnima.json"));

    for check_only in [true, false] {
        let mut asked = request(&project, &[&old]);
        asked.check_only = check_only;

        let (code, ui) = run(offline(), &asked);

        assert_ne!(code, 0, "{}", ui.out());
        assert!(
            ui.err()
                .contains("libb needs liba >= 1.0.0, and liba 0.9.0 would not do"),
            "{}",
            ui.err()
        );
        assert_eq!(read(project.join(".libnima.json")), before);
    }

    // Nothing needs it: it goes back, and that is said.
    let alone = stm32_project(&root.path().join("alone"));
    installed(&alone, &new, "liba");
    let ui = installed(&alone, &old, "liba");
    assert_eq!(record(&alone)["liba"]["version"], "0.9.0");
    assert!(
        ui.out()
            .contains("liba 1.0.0 is in this project, and liba 0.9.0 goes in over it"),
        "{}",
        ui.out()
    );
}

// ----------------------------------------------------------------------------
// Parts a library has for the choosing
// ----------------------------------------------------------------------------

const OPTION: &str = "options:\n  lfs:\n    description: LittleFS file system on the flash\n    libraries: [fsx]\n    headers: [inc/demo_lfs.h]\n    sources: [src/demo_lfs.c]\n";
const OPTION_FILES: [(&str, &str); 2] = [("inc/demo_lfs.h", ""), ("src/demo_lfs.c", "")];

fn with_option(root: &Path, tag: &str) -> PathBuf {
    version(
        root,
        tag,
        "demo",
        &format!("{DEMO_CODE}{OPTION}"),
        &OPTION_FILES,
    )
}

fn fsx(root: &Path) -> PathBuf {
    lib(root, "fsx", "1.0.0", &[])
}

#[test]
fn the_option_alone_brings_its_library() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let library_of_it = fsx(root.path());
    let (client, server) = github(&[("fsx", &library_of_it)]);
    let mut asked = request(&project, &[&with_option(root.path(), "v1")]);
    asked.folder = Some("demo".to_string());
    asked.with_options = vec!["lfs".to_string()];

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(server.repos_asked(), ["nimaltd/fsx"]);
    assert!(project.join("fsx/inc/fsx.h").is_file());
    assert!(project.join("demo/src/demo_lfs.c").is_file());
    assert_eq!(record(&project)["demo"]["options"]["lfs"], true);
}

#[test]
fn anything_but_yes_leaves_the_option_out() {
    for reply in ["", "n", "no", "maybe"] {
        let root = tempfile::tempdir().unwrap();
        let project = stm32_project(root.path());
        let mut asked = request(&project, &[&with_option(root.path(), "v1")]);
        asked.folder = Some("demo".to_string());

        let mut ui = Watched::answering(&[reply]);
        let code = flow::run_with(offline(), &asked, &mut ui);

        assert_eq!(code, 0, "{}", ui.err());
        assert!(!project.join("demo/src/demo_lfs.c").exists(), "{reply}");
        assert!(!project.join("fsx").exists());
        assert_eq!(record(&project)["demo"]["options"]["lfs"], false);
    }
}

#[test]
fn yes_takes_the_option() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let library_of_it = fsx(root.path());
    let (client, _) = github(&[("fsx", &library_of_it)]);
    let mut asked = request(&project, &[&with_option(root.path(), "v1")]);
    asked.folder = Some("demo".to_string());

    let mut ui = Watched::answering(&["y", ""]);
    let code = flow::run_with(client, &asked, &mut ui);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("demo/src/demo_lfs.c").is_file());
    assert!(
        ui.asked[0].contains("LittleFS file system on the flash. Add it? [y/N]"),
        "{:?}",
        ui.asked
    );
}

#[test]
fn with_nobody_to_ask_the_option_is_left_out_and_it_says_how() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let mut asked = request(&project, &[&with_option(root.path(), "v1")]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!project.join("demo/src/demo_lfs.c").exists());
    assert!(
        ui.out()
            .contains("no, nobody was asked. Add it with --with lfs"),
        "{}",
        ui.out()
    );
}

#[test]
fn an_update_keeps_the_answer_without_asking_again() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let library_of_it = fsx(root.path());
    let mut asked = request(&project, &[&with_option(root.path(), "v1"), &library_of_it]);
    asked.with_options = vec!["lfs".to_string()];

    let (code, ui) = run(offline(), &asked);
    assert_eq!(code, 0, "{}", ui.err());

    let mut again = request(&project, &[&with_option(root.path(), "v2")]);
    again.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &again);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        ui.asked.is_empty(),
        "asked again on an update: {:?}",
        ui.asked
    );
    assert!(project.join("demo/src/demo_lfs.c").is_file());
    assert!(
        ui.out()
            .contains("yes, as before. Change it with --without lfs"),
        "{}",
        ui.out()
    );
}

#[test]
fn without_takes_the_option_out_and_leaves_its_library() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            keil: true,
            ..Ides::default()
        },
    );
    let library_of_it = fsx(root.path());
    let mut asked = request(&project, &[&with_option(root.path(), "v1"), &library_of_it]);
    asked.with_options = vec!["lfs".to_string()];

    assert_eq!(run(offline(), &asked).0, 0);

    let mut again = request(&project, &[&with_option(root.path(), "v2")]);
    again.without_options = vec!["lfs".to_string()];
    again.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &again);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!project.join("demo/src/demo_lfs.c").exists());
    assert!(
        project.join("fsx/inc/fsx.h").is_file(),
        "the library it brought was removed"
    );
    assert!(!read(project.join("MDK/Proj.uvprojx")).contains("demo_lfs.c"));
    assert!(ui.out().contains("option lfs is off"), "{}", ui.out());
    assert!(
        ui.out().contains("fsx stays in the project"),
        "{}",
        ui.out()
    );
}

#[test]
fn an_option_new_in_an_update_is_asked_about() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let first = library(root.path(), "demo", DEMO_CODE, &[]);

    installed(&project, &first, "demo");

    let mut asked = request(&project, &[&with_option(root.path(), "v2")]);
    asked.folder = Some("demo".to_string());

    let (_, ui) = run(offline(), &asked);

    assert_eq!(ui.asked.len(), 1, "{:?}", ui.asked);
    assert!(ui.asked[0].contains("Add it?"), "{:?}", ui.asked);
}

#[test]
fn an_option_no_library_has_is_refused_before_anything() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let mut asked = request(&project, &[&with_option(root.path(), "v1")]);
    asked.folder = Some("demo".to_string());
    asked.with_options = vec!["lsf".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(!project.join("demo").exists());
    // Said of the library by its name, as it is not installed here yet.
    assert!(
        ui.err().contains("No option called lsf. demo has lfs."),
        "{}",
        ui.err()
    );
}

#[test]
fn with_and_without_the_same_option_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let mut asked = request(&project, &[&with_option(root.path(), "v1")]);
    asked.folder = Some("demo".to_string());
    asked.with_options = vec!["lfs".to_string()];
    asked.without_options = vec!["lfs".to_string()];

    let (code, _) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(!project.join("demo").exists());
}

#[test]
fn a_library_updated_because_another_needs_it_keeps_its_options() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let extra = "options:\n  x:\n    description: extra\n    sources: [src/fsx_x.c]\n";
    let code_of = |tag: &str, number: &str| {
        library(
            &root.path().join(tag),
            "fsx",
            &format!("version: {number}\nheaders: [inc/fsx.h]\n{extra}"),
            &[("inc/fsx.h", ""), ("src/fsx_x.c", "")],
        )
    };
    let old = code_of("v1", "1.0.0");
    let new = code_of("v2", "2.0.0");

    let mut asked = request(&project, &[&old]);
    asked.folder = Some("fsx".to_string());
    asked.with_options = vec!["x".to_string()];

    assert_eq!(run(offline(), &asked).0, 0);

    let demo = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}requires:\n  libraries: [fsx >= 2.0.0]\n"),
        &[],
    );
    let (client, _) = github(&[("fsx", &new)]);
    let mut again = request(&project, &[&demo]);
    again.folder = Some("demo".to_string());

    let (code, ui) = run(client, &again);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(record(&project)["fsx"]["version"], "2.0.0");
    assert!(
        project.join("fsx/src/fsx_x.c").is_file(),
        "the update dropped the option"
    );
    assert!(ui.asked.is_empty(), "{:?}", ui.asked);
}

#[test]
fn a_library_brought_in_says_how_to_take_its_option() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let extra = library(
        &root.path().join("gh"),
        "fsx",
        "version: 1.0.0\nheaders: [inc/fsx.h]\noptions:\n  x:\n    description: extra\n    sources: [src/x.c]\n",
        &[("inc/fsx.h", ""), ("src/x.c", "")],
    );
    let demo = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}requires:\n  libraries: [fsx]\n"),
        &[],
    );
    let (client, _) = github(&[("fsx", &extra)]);
    let mut asked = request(&project, &[&demo]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        ui.out().contains("Add it with: libnima fsx --with x"),
        "{}",
        ui.out()
    );
}

// ----------------------------------------------------------------------------
// Steps run once the library is in
// ----------------------------------------------------------------------------

/// A step that writes a file, by the machine's own shell, so a test needs nothing else.
fn writes(file: &str) -> String {
    if cfg!(windows) {
        format!("[cmd, /c, \"echo %LIBNIMA_NAME% %LIBNIMA%> {file}\"]")
    } else {
        format!("[sh, -c, \"echo $LIBNIMA_NAME $LIBNIMA > {file}\"]")
    }
}

fn appends(file: &str) -> String {
    if cfg!(windows) {
        format!("[cmd, /c, \"echo 1>> {file}\"]")
    } else {
        format!("[sh, -c, \"echo 1 >> {file}\"]")
    }
}

fn fails() -> String {
    if cfg!(windows) {
        "[cmd, /c, \"exit 3\"]".to_string()
    } else {
        "[sh, -c, \"exit 3\"]".to_string()
    }
}

fn with_key_step(root: &Path) -> PathBuf {
    library(
        root,
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: key\n    description: Make a key\n    command: {}\n    unless: sign_key.pem\n",
            writes("sign_key.pem")
        ),
        &[],
    )
}

#[test]
fn the_flags_reach_a_library_that_comes_in_as_a_requirement() {
    let root = tempfile::tempdir().unwrap();
    let example = library(
        &root.path().join("gh"),
        "example",
        &format!(
            "version: 1.0.0\n{DEMO_CODE}options:\n  uptime:\n    description: Count the time up\n    headers: [inc/up.h]\n\
             run:\n  - label: make-id\n    description: Make an id\n    command: {}\n    unless: example_id.h\n",
            writes("example_id.h")
        ),
        &[("inc/up.h", "")],
    );
    let addy = lib_at(
        &root.path().join("disk"),
        "addy",
        "addy",
        "1.0.0",
        &needing(&["- source: someone/example\n      name: example"]),
    );

    // --no-run and --with name the step and the option of the library it needs.
    let project = stm32_project(&root.path().join("one"));
    let (client, _server) = github(&[("someone/example", &example)]);
    let mut asked = request(&project, &[&addy]);
    asked.yes = true;
    asked.run_off = vec!["make-id".to_string()];
    asked.with_options = vec!["uptime".to_string()];

    let (code, ui) = run(client, &asked);

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert!(
        ui.out().contains("skipped, as asked with --no-run make-id"),
        "{}",
        ui.out()
    );
    assert!(!project.join("example_id.h").exists());
    assert_eq!(record(&project)["example"]["options"]["uptime"], true);

    // A name no library of the run has is still refused, by the install and by the check.
    for check_only in [false, true] {
        let project = stm32_project(&root.path().join(format!("two-{check_only}")));
        let (client, _server) = github(&[("someone/example", &example)]);
        let mut asked = request(&project, &[&addy]);
        asked.with_options = vec!["nothing-like-it".to_string()];
        asked.check_only = check_only;

        let (code, ui) = run(client, &asked);

        assert_ne!(code, 0, "{}", ui.out());
        assert!(
            ui.err().contains("No option called nothing-like-it"),
            "{}",
            ui.err()
        );
        assert!(record(&project).is_empty());
    }
}

fn folder_of(asked: &mut Request) {
    asked.folder = Some("demo".to_string());
}

fn ran_entry(project: &Path) -> serde_json::Value {
    record(project)["demo"]["ran"].clone()
}

#[test]
fn a_step_answered_yes_runs_in_the_project_folder() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);

    let mut ui = Watched::answering(&["y"]);
    let code = flow::run_with(offline(), &asked, &mut ui);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(read(project.join("sign_key.pem")).trim(), "demo libnima");
    assert!(
        ui.asked.iter().any(|prompt| prompt.contains("Run it?")),
        "{:?}",
        ui.asked
    );
    assert!(ran_entry(&project)["key"].is_string());
}

#[test]
fn the_command_is_shown_before_the_question() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);

    let mut ui = Watched::answering(&["n"]);
    let code = flow::run_with(offline(), &asked, &mut ui);

    assert_eq!(code, 0);
    assert!(ui.out().contains("sign_key.pem"), "{}", ui.out());
    assert!(ui.out().contains("not run, as answered"), "{}", ui.out());
    assert!(!project.join("sign_key.pem").exists());
}

#[test]
fn its_file_already_there_skips_it_without_a_question() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    write(project.join("sign_key.pem"), "mine");
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0);
    assert_eq!(read(project.join("sign_key.pem")), "mine");
    assert!(
        !ui.asked.iter().any(|prompt| prompt.contains("Run it?")),
        "{:?}",
        ui.asked
    );
    assert!(
        ui.out().contains("skipped, sign_key.pem is already there"),
        "{}",
        ui.out()
    );
}

#[test]
fn with_nobody_to_ask_it_is_not_run_and_says_how() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0);
    assert!(!project.join("sign_key.pem").exists());
    assert!(ui.out().contains("Run it with --run key"), "{}", ui.out());
}

#[test]
fn run_given_runs_it_without_a_question() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["key".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(project.join("sign_key.pem").is_file());
    assert!(ui.asked.is_empty(), "{:?}", ui.asked);
}

#[test]
fn yes_runs_every_step() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.yes = true;

    assert_eq!(run(offline(), &asked).0, 0);
    assert!(project.join("sign_key.pem").is_file());
}

#[test]
fn no_run_skips_it() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_off = vec!["key".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0);
    assert!(!project.join("sign_key.pem").exists());
    assert!(
        ui.out().contains("skipped, as asked with --no-run key"),
        "{}",
        ui.out()
    );
}

#[test]
fn an_unknown_step_is_refused_before_anything_goes_in() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["kye".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(ui.err().contains("No run step called kye"), "{}", ui.err());
    assert!(!project.join("demo").exists());
}

#[test]
fn one_step_given_both_ways_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["key".to_string()];
    asked.run_off = vec!["key".to_string()];

    assert_eq!(run(offline(), &asked).0, 2);
}

#[test]
fn a_step_that_fails_leaves_the_library_in_and_says_so() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: bad\n    command: {}\n",
            fails()
        ),
        &[],
    );
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["bad".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 1);
    assert!(project.join("demo/inc/demo.h").is_file());
    assert!(
        ui.out().contains("failed, it ended with code 3"),
        "{}",
        ui.out()
    );
    assert!(ui.out().contains("bad did not finish"), "{}", ui.out());
    assert_eq!(ran_entry(&project), serde_json::json!({}));
}

#[test]
fn a_command_that_is_not_installed_says_so() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: tool\n    command: [surely-no-such-program-here, --x]\n"
        ),
        &[],
    );
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["tool".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 1);
    assert!(
        ui.out()
            .contains("surely-no-such-program-here is not installed, or not on the PATH"),
        "{}",
        ui.out()
    );
}

#[test]
fn an_update_does_not_run_a_step_that_ran_before() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: once\n    command: {}\n",
            appends("count.txt")
        ),
        &[],
    );
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["once".to_string()];

    assert_eq!(run(offline(), &asked).0, 0);

    let mut plain = request(&project, &[&source]);
    folder_of(&mut plain);

    let (_, ui) = run(offline(), &plain);

    assert_eq!(read(project.join("count.txt")).lines().count(), 1);
    assert!(
        ui.out()
            .contains("ran before. Run it again with --run once"),
        "{}",
        ui.out()
    );

    assert_eq!(run(offline(), &asked).0, 0);
    assert_eq!(read(project.join("count.txt")).lines().count(), 2);
}

#[test]
fn a_step_that_ran_before_is_not_run_again_when_its_file_is_gone() {
    // A fresh clone of a product that keeps its key out of git. A new key would not be the one the devices know.
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = with_key_step(root.path());
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.yes = true;

    assert_eq!(run(offline(), &asked).0, 0);
    assert!(project.join("sign_key.pem").is_file());

    fs::remove_file(project.join("sign_key.pem")).unwrap();

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!project.join("sign_key.pem").exists(), "it ran again");
    assert!(
        ui.out().contains("ran before. Run it again with --run key"),
        "{}",
        ui.out()
    );

    asked.run_on = vec!["key".to_string()];

    assert_eq!(run(offline(), &asked).0, 0);
    assert!(project.join("sign_key.pem").is_file());
}

#[test]
fn a_changed_step_is_asked_about_again() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let first = version(
        root.path(),
        "v1",
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: s\n    command: {}\n",
            appends("a.txt")
        ),
        &[],
    );
    let second = version(
        root.path(),
        "v2",
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: s\n    command: {}\n",
            appends("b.txt")
        ),
        &[],
    );

    let mut asked = request(&project, &[&first]);
    folder_of(&mut asked);
    asked.run_on = vec!["s".to_string()];
    assert_eq!(run(offline(), &asked).0, 0);

    let mut again = request(&project, &[&second]);
    folder_of(&mut again);

    let (_, ui) = run(offline(), &again);

    assert!(
        ui.asked.iter().any(|prompt| prompt.contains("Run it?")),
        "{:?}",
        ui.asked
    );
}

#[test]
fn a_step_sees_where_things_are() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let print = if cfg!(windows) {
        "[cmd, /c, \"echo %INSTALLER_NAME% %LIBNIMA_PLATFORM%> seen.txt\"]"
    } else {
        "[sh, -c, \"echo $INSTALLER_NAME $LIBNIMA_PLATFORM > seen.txt\"]"
    };
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}run:\n  - label: s\n    command: {print}\n"),
        &[],
    );
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.yes = true;

    assert_eq!(run(offline(), &asked).0, 0);
    assert_eq!(read(project.join("seen.txt")).trim(), "demo stm32");
}

#[test]
fn library_names_the_folder_it_was_installed_in() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let print = if cfg!(windows) {
        "[cmd, /c, \"echo {library}> seen.txt\"]"
    } else {
        "[sh, -c, \"echo {library} > seen.txt\"]"
    };
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}run:\n  - label: s\n    command: {print}\n"),
        &[],
    );
    let mut asked = request(&project, &[&source]);
    asked.folder = Some("modules/demo".to_string());
    asked.yes = true;

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(read(project.join("seen.txt")).trim(), "modules/demo");
    assert!(ui.out().contains("modules/demo"), "{}", ui.out());
}

#[test]
fn unless_may_name_the_library_folder() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(
        root.path(),
        "demo",
        &format!(
            "{DEMO_CODE}run:\n  - label: s\n    command: {}\n    unless: '{{library}}/inc/demo.h'\n",
            writes("ran.txt")
        ),
        &[],
    );
    let mut asked = request(&project, &[&source]);
    folder_of(&mut asked);
    asked.run_on = vec!["s".to_string()];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(!project.join("ran.txt").exists());
    assert!(ui.out().contains("skipped"), "{}", ui.out());
}

// ----------------------------------------------------------------------------
// Which platform
// ----------------------------------------------------------------------------

#[test]
fn a_library_for_a_platform_goes_in_with_nothing_more_said() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);

    assert_eq!(install(&project, &source, "demo").0, 0);
}

#[test]
fn a_library_for_another_platform_than_the_project_looks_like_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    write(project.join("Board.ioc"), "");
    let source = library_for("zephyr", root.path(), "demo", DEMO_CODE, &[]);

    let (code, ui) = install(&project, &source, "demo");

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("this looks like a STM32 project, from Board.ioc"),
        "{}",
        ui.err()
    );
    assert!(ui.err().contains("--platform zephyr"), "{}", ui.err());
    assert!(!project.join("demo").exists());
}

#[test]
fn platform_given_puts_right_what_the_folder_looks_like() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    put(
        &project,
        &[
            ("Board.ioc", ""),
            ("CMakeLists.txt", "find_package(Zephyr)\n"),
        ],
    );
    let source = library_for("zephyr", root.path(), "demo", DEMO_CODE, &[]);

    let mut asked = request(&project, &[&source]);
    asked.platform = Some(libnima_core::platform::Platform::Zephyr);
    asked.folder = Some("modules/demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(read(project.join("prj.conf")).contains("CONFIG_DEMO=y"));
}

#[test]
fn platform_given_that_the_library_does_not_name_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let mut asked = request(&project, &[&source]);
    asked.platform = Some(libnima_core::platform::Platform::Zephyr);

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("demo is for stm32, and --platform gives zephyr"),
        "{}",
        ui.err()
    );
}

#[test]
fn a_library_for_several_platforms_takes_the_one_the_project_is() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    put(&project, &[("CMakeLists.txt", "find_package(Zephyr)\n")]);
    let source = library_for("[stm32, zephyr]", root.path(), "demo", DEMO_CODE, &[]);

    let mut asked = request(&project, &[&source]);
    asked.folder = Some("modules/demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}", ui.err());
    assert!(
        ui.asked.iter().all(|prompt| !prompt.contains("Platform")),
        "{:?}",
        ui.asked
    );
    assert_eq!(
        libnima_core::record::recorded_platform(&project),
        Some(libnima_core::platform::Platform::Zephyr)
    );
}

#[test]
fn a_library_for_several_in_a_folder_that_says_nothing_asks() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library_for("[zephyr, stm32]", root.path(), "demo", DEMO_CODE, &[]);
    let mut asked = request(&project, &[&source]);
    asked.folder = Some("demo".to_string());

    let mut ui = Watched::answering(&["2"]);
    let code = flow::run_with(offline(), &asked, &mut ui);

    assert_eq!(code, 0, "{}", ui.err());
    assert_eq!(
        libnima_core::record::recorded_platform(&project),
        Some(libnima_core::platform::Platform::Stm32)
    );

    // With nobody to answer, it says what to give.
    let other = tempfile::tempdir().unwrap();
    let project = stm32_project(other.path());
    let mut asked = request(&project, &[&source]);
    asked.folder = Some("demo".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("Say which this project is with --platform"),
        "{}",
        ui.err()
    );
}

#[test]
fn libraries_for_different_platforms_are_refused_together() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let one = library(root.path(), "one", DEMO_CODE, &[]);
    let two = library_for("zephyr", root.path(), "two", DEMO_CODE, &[]);

    let (code, ui) = run(offline(), &request(&project, &[&one, &two]));

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("These libraries are for different platforms"),
        "{}",
        ui.err()
    );
}

#[test]
fn the_record_says_one_platform_and_the_library_another() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    write(
        project.join(".libnima.json"),
        "{\"platform\": \"stm32\", \"libraries\": {}}",
    );
    let source = library_for("zephyr", root.path(), "demo", DEMO_CODE, &[]);

    let (code, ui) = install(&project, &source, "demo");

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("This project's record says stm32, and demo is for zephyr"),
        "{}",
        ui.err()
    );
}

#[test]
fn an_option_of_another_platform_is_refused_rather_than_ignored() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let mut asked = request(&project, &[&source]);
    asked.args.conf = Some("prj.conf".to_string());

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2);
    assert!(
        ui.err()
            .contains("--conf is for Zephyr projects, and this one is for STM32"),
        "{}",
        ui.err()
    );
}

// ----------------------------------------------------------------------------
// A project lib-installer installed into
// ----------------------------------------------------------------------------

#[test]
fn a_project_of_lib_installer_moves_over_and_finds_its_blocks() {
    let root = tempfile::tempdir().unwrap();
    let project = project_with(
        root.path(),
        Ides {
            cmake: true,
            ..Ides::default()
        },
    );
    let source = library(root.path(), "demo", DEMO_CODE, &[]);

    installed(&project, &source, "demo");

    // Put back as lib-installer left it.
    let cmake =
        read(project.join("CMakeLists.txt")).replace("libnima: demo", "lib-installer: demo");
    write(project.join("CMakeLists.txt"), &cmake);
    fs::rename(
        project.join(".libnima.json"),
        project.join(".lib-installer.json"),
    )
    .unwrap();

    let ui = installed(&project, &source, "demo");

    assert!(
        ui.out()
            .contains("Moved over from lib-installer to libnima"),
        "{}",
        ui.out()
    );
    assert!(project.join(".libnima.json").is_file());
    assert!(!project.join(".lib-installer.json").exists());

    let after = read(project.join("CMakeLists.txt"));

    assert_eq!(after.matches(">>> libnima: demo >>>").count(), 1, "{after}");
    assert!(!after.contains("lib-installer: demo"), "{after}");
    assert_eq!(backups(&project, ".lib-installer.json").len(), 1);
}

#[test]
fn a_project_of_one_of_the_five_installers_moves_over_too() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("app");
    put(
        &project,
        &[
            (
                "CMakeLists.txt",
                "# >>> zephyr-installer: demo >>>\nlist(APPEND ZEPHYR_EXTRA_MODULES x)\n# <<< zephyr-installer: demo <<<\n\nfind_package(Zephyr)\n",
            ),
            (
                "prj.conf",
                "# >>> zephyr-installer: demo >>>\nCONFIG_DEMO=y\n# <<< zephyr-installer: demo <<<\n",
            ),
            ("conf/debug.conf", "CONFIG_X=y\n"),
            (
                ".zephyr-installer.json",
                "{\"libraries\": {\"demo\": {\"folder\": \"modules/demo\", \"files\": []}}}",
            ),
        ],
    );

    let (code, ui) = {
        let asked = Request {
            uninstall: vec!["demo".to_string()],
            project: Some(project.clone()),
            ..Request::default()
        };
        run(offline(), &asked)
    };

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert!(
        ui.out()
            .contains("Moved over from zephyr-installer to libnima"),
        "{}",
        ui.out()
    );
    assert!(!read(project.join("prj.conf")).contains("demo"));
    assert!(!read(project.join("CMakeLists.txt")).contains("ZEPHYR_EXTRA_MODULES"));
}

// ----------------------------------------------------------------------------
// Only looking
// ----------------------------------------------------------------------------

#[test]
fn a_check_writes_nothing_and_says_what_it_finds() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    write(project.join("Board.ioc"), "Mcu.IP0=NVIC\n");
    let source = library(
        root.path(),
        "demo",
        &format!("{DEMO_CODE}requires:\n  peripherals: [spi]\n"),
        &[],
    );
    let before = listing(&project);
    let mut asked = request(&project, &[&source]);
    asked.check_only = true;

    let (code, ui) = run(offline(), &asked);

    // A CHECK is something to do first, so a script can stop on it: 1, apart from the 2 of a run that went wrong.
    assert_eq!(code, 1, "{}", ui.err());
    assert_eq!(listing(&project), before, "a check wrote something");
    assert!(
        ui.out()
            .contains("No SPI found in your CubeMX configuration"),
        "{}",
        ui.out()
    );
    assert!(ui.out().contains("CHECK"), "{}", ui.out());
}

#[test]
fn a_check_with_nothing_to_do_exits_0() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", DEMO_CODE, &[]);
    let mut asked = request(&project, &[&source]);
    asked.check_only = true;

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 0, "{}\n{}", ui.out(), ui.err());
    assert!(ui.out().contains("OK"), "{}", ui.out());
}

#[test]
fn a_check_of_what_cannot_go_in_says_no() {
    let root = tempfile::tempdir().unwrap();
    let project = stm32_project(root.path());
    let source = library(root.path(), "demo", "headers: [missing.h]\n", &[]);
    let mut asked = request(&project, &[&source]);
    asked.check_only = true;

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 1);
    assert!(ui.out().contains("NO"), "{}", ui.out());
    assert!(ui.err().contains("do not exist"), "{}", ui.err());
}

// ----------------------------------------------------------------------------
// Several libraries, all or nothing
// ----------------------------------------------------------------------------

/// A library with its tools for the project's folder.
fn with_tools(root: &Path, name: &str, tools: &[(&str, &str)]) -> PathBuf {
    library(
        root,
        name,
        &format!("{DEMO_CODE}extras:\n  - root: tools\n"),
        tools,
    )
}

#[test]
fn a_run_that_is_refused_writes_nothing_at_all() {
    // Every check of every library is made before the first is written, so a refusal of the second leaves no trace of
    // the first.
    let root = tempfile::tempdir().unwrap();
    let liba = library(root.path(), "liba", DEMO_CODE, &[]);
    let refused = [
        (
            // A once file on a file libNIMA edits.
            library(
                root.path(),
                "libc",
                &format!("{DEMO_CODE}once:\n  - from: x.txt\n    to: CMakeLists.txt\n"),
                &[("x.txt", "x\n")],
            ),
            None,
            "edits or reads as the project's own",
        ),
        (
            // The user's own file where a tool of the library's goes.
            with_tools(root.path(), "libt", &[("tools/libt.py", "the library's\n")]),
            Some(("tools/libt.py", "mine\n")),
            "libt would write tools/libt.py in the project, where a file of yours already is",
        ),
    ];

    for (index, (second, mine, said)) in refused.iter().enumerate() {
        let project = stm32_project(&root.path().join(format!("p{index}")));

        if let Some((path, text)) = mine {
            write(project.join(path), text);
        }

        let before = listing(&project);
        let (code, ui) = run(offline(), &request(&project, &[&liba, second]));

        assert_eq!(code, 2, "{}", ui.out());
        assert!(ui.err().contains(said), "{}", ui.err());
        assert_eq!(listing(&project), before, "liba went in all the same");
        assert!(!ui.out().contains("liba 1"), "{}", ui.out());
    }
}

#[test]
fn two_libraries_of_a_run_never_share_a_file() {
    let root = tempfile::tempdir().unwrap();
    let libx = with_tools(root.path(), "libx", &[("tools/shared.py", "# libx\n")]);
    let liby = with_tools(root.path(), "liby", &[("tools/shared.py", "# liby\n")]);
    let project = stm32_project(root.path());
    let before = listing(&project);
    let (code, ui) = run(offline(), &request(&project, &[&libx, &liby]));

    assert_eq!(code, 2, "{}", ui.out());
    assert!(
        ui.err().contains(
            "liby would put tools/shared.py in the project, where libx has a file of its own"
        ),
        "{}",
        ui.err()
    );
    assert_eq!(listing(&project), before);

    // --check of the two says the same of the second.
    let mut checked = request(&project, &[&libx, &liby]);
    checked.check_only = true;

    let (code, ui) = run(offline(), &checked);

    assert_eq!(code, 1, "{}", ui.out());
    assert!(ui.out().contains("NO      liby"), "{}", ui.out());
    assert!(!ui.out().contains("NO      libx"), "{}", ui.out());

    // One at a time, the second is refused the same way, naming the first.
    assert_eq!(run(offline(), &request(&project, &[&libx])).0, 0);

    let (code, ui) = run(offline(), &request(&project, &[&liby]));

    assert_eq!(code, 2, "{}", ui.out());
    assert!(
        ui.err().contains("where libx has a file of its own"),
        "{}",
        ui.err()
    );
    assert_eq!(read(project.join("tools/shared.py")), "# libx\n");
}

#[test]
fn two_libraries_of_a_run_never_share_a_folder() {
    let root = tempfile::tempdir().unwrap();
    let one = library(root.path(), "one", DEMO_CODE, &[]);
    let two = library(root.path(), "two", DEMO_CODE, &[]);
    let project = stm32_project(root.path());
    let before = listing(&project);
    let mut asked = request(&project, &[&one, &two]);

    asked.folders = vec![
        ("one".to_string(), "Libs/both".to_string()),
        ("two".to_string(), "Libs/both".to_string()),
    ];

    let (code, ui) = run(offline(), &asked);

    assert_eq!(code, 2, "{}", ui.out());
    assert!(
        ui.err().contains("Libs/both is where one is installed"),
        "{}",
        ui.err()
    );
    assert_eq!(listing(&project), before);
}
