//! library.json: how a library in the project's lib folder introduces itself to PlatformIO.
//!
//! PlatformIO's Library Dependency Finder reads it for which files to build, the folder a project includes
//! from, the frameworks and platforms the library builds with, and the libraries of the PlatformIO Registry
//! it needs, which PlatformIO then installs itself. A library written for any project has none, so it is
//! written here, from libnima.yml, and written again on every install.
//!
//! It names the library's files one by one, from libnima.yml, so PlatformIO builds those and nothing else: a
//! test or an example in the library's folder is left alone, and a source file a new version adds is built
//! with no one editing anything.
//!
//! A library made for PlatformIO ships its own, which knows the library better than this does, and is left
//! alone. JSON has no comments to mark this one with, so the two are told apart by whether libnima.yml lists
//! a library.json among the library's own files.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};

use crate::files;
use crate::installer::Installed;
use crate::manifest::Manifest;
use crate::paths;
use crate::record::json_text;
use crate::target::{Outcome, Status};

const NAME: &str = "library.json";

static OWNER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)github\.com[/:]([^/\s]+)/").expect("a valid pattern"));

/// Whether libnima.yml lists a library.json among the library's files.
fn ships_its_own(library: &Manifest) -> bool {
    library
        .headers
        .iter()
        .chain(&library.sources)
        .chain(&library.extras)
        .chain(&library.once)
        .any(|entry| paths::clean_posix(&entry.destination) == NAME)
}

/// The include folder, and any other folder the headers are in.
///
/// The include folder is the first one include_dirs names, or else the folder of the first header, the one a
/// project includes. library.json has room for one, so the others go in as -I flags, which PlatformIO gives
/// the library and every file that uses it alike (measured). The top of the library is on the include path
/// already, as its srcDir.
fn include_folders(library: &Manifest) -> (Option<String>, Vec<String>) {
    let first = match (&library.declared_include_dirs, library.headers.first()) {
        (Some(dirs), _) if !dirs.is_empty() => Some(dirs[0].clone()),
        (_, Some(header)) => Some(header.folder()),
        _ => None,
    };

    let main = first.filter(|folder| folder != "." && !folder.is_empty());
    let others = library
        .include_dirs()
        .into_iter()
        .filter(|folder| folder != "." && !folder.is_empty() && Some(folder) != main.as_ref())
        .collect();

    (main, others)
}

/// library.json for this library, in the order it is written.
fn manifest(library: &Manifest) -> Value {
    let part = library.part.pio();
    let mut data = Map::new();

    data.insert("name".to_string(), json!(library.name));

    if !library.version.is_empty() {
        data.insert("version".to_string(), json!(library.version));
    }

    if !library.description.is_empty() {
        data.insert("description".to_string(), json!(library.description));
    }

    let keywords: Vec<&str> = [library.kind.as_str(), library.category.as_str()]
        .into_iter()
        .filter(|word| !word.is_empty())
        .collect();

    if !keywords.is_empty() {
        data.insert("keywords".to_string(), json!(keywords));
    }

    if !library.repository.is_empty() {
        data.insert(
            "repository".to_string(),
            json!({"type": "git", "url": library.repository}),
        );
    }

    if let Some(owner) = OWNER.captures(&library.repository) {
        data.insert("authors".to_string(), json!([{"name": &owner[1]}]));
    }

    if !library.license.is_empty() {
        data.insert("license".to_string(), json!(library.license));
    }

    let any_or = |names: &[String]| {
        if names.is_empty() {
            json!("*")
        } else {
            json!(names)
        }
    };

    data.insert("frameworks".to_string(), any_or(&part.frameworks));
    data.insert("platforms".to_string(), any_or(&part.platforms));

    if !part.dependencies.is_empty() {
        let mut dependencies = Map::new();

        for (name, version) in &part.dependencies {
            dependencies.insert(name.clone(), json!(version));
        }

        data.insert("dependencies".to_string(), Value::Object(dependencies));
    }

    // Built from the top of its folder, with every file but its own sources filtered out. Its include folder
    // is the one a project includes it from.
    let mut filter = vec!["-<*>".to_string()];
    filter.extend(
        library
            .build_sources()
            .iter()
            .map(|name| format!("+<{name}>")),
    );

    let mut build = Map::new();
    build.insert("srcDir".to_string(), json!("."));
    build.insert("srcFilter".to_string(), json!(filter));

    let (main, others) = include_folders(library);

    if let Some(main) = main {
        build.insert("includeDir".to_string(), json!(main));
    }

    if !others.is_empty() {
        let flags: Vec<String> = others.iter().map(|folder| format!("-I {folder}")).collect();
        build.insert("flags".to_string(), json!(flags));
    }

    data.insert("build".to_string(), Value::Object(build));

    Value::Object(data)
}

/// Write library.json, only when it changed.
pub fn write(library: &Manifest, destination: &Path) -> Outcome {
    let path = destination.join(NAME);
    let name = &library.name;

    if ships_its_own(library) {
        return Outcome::new(
            NAME,
            Status::Already,
            format!("{name} has its own library.json, used as it is."),
        );
    }

    let data = json_text(&manifest(library)).into_bytes();

    if path.is_file() && fs::read(&path).is_ok_and(|old| old == data) {
        return Outcome::new(
            NAME,
            Status::Already,
            format!("{name}'s library.json is up to date."),
        );
    }

    if let Err(error) = fs::write(&path, &data) {
        return Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "Could not write {}: {}",
                path.display(),
                files::reason(&error)
            ),
        );
    }

    Outcome::new(
        NAME,
        Status::Changed,
        format!("Wrote {name}'s library.json, which PlatformIO builds it by."),
    )
}

/// Delete the library.json write() made, before the library's files go. One the record lists as the
/// library's own is left to go with them.
pub fn remove(installed: &Installed) {
    let Some(destination) = &installed.destination else {
        return;
    };

    let path = destination.join(NAME);

    if installed.files.contains(&path) || installed.once.contains(&path) {
        return;
    }

    if path.is_file() {
        let _ = fs::remove_file(path);
    }
}
