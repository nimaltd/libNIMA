//! Reading and validating a `libnima.yml`.
//!
//! Every library this installs carries one of these at the root of its repository. It
//! answers three questions: what the library is, what it needs from the project, and
//! which files make it up.
//!
//! The "what it is" part matters more than it first appears. A library can itself be an
//! RTOS, so saying only what a library requires is not enough. A library declares its
//! kind, and declares what it provides, and another library's requirement is then
//! satisfied by whatever provides that thing.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value as Json};

use crate::convert;
use crate::paths;
use crate::platform::{self, Part, Platform};
use crate::schema;
use crate::text::{given_twice, short_hash};
use crate::yaml::{self, Mapping, Node, Value, quoted};

pub const MANIFEST_NAME: &str = "libnima.yml";

/// What the file was called in lib-installer. A library that has only this one is read
/// through the converter, when it asks for lib-installer 4.0.0.
pub const INSTALLER_NAME: &str = "installer.yml";

/// What it was called before lib-installer 1.9.0, which is too old to read.
pub const LIBRARY_NAME: &str = "library.yml";

/// Every name a library's file goes by, the one read first.
pub const MANIFEST_NAMES: [&str; 3] = [MANIFEST_NAME, INSTALLER_NAME, LIBRARY_NAME];

/// What a library is. Used for grouping and for satisfying requirements.
pub const KINDS: [&str; 7] = [
    "driver",
    "middleware",
    "rtos",
    "protocol",
    "filesystem",
    "utility",
    "bsp",
];

/// Finer grouping, used when listing libraries to a user.
pub const CATEGORIES: [&str; 11] = [
    "sensor",
    "storage",
    "display",
    "communication",
    "wireless",
    "rtos",
    "timing",
    "power",
    "input",
    "math",
    "system",
];

/// What a compiler takes. Keil, IAR and a Makefile name each of these files one by one,
/// so this also decides which files an update has to tell them about.
pub const COMPILED: [&str; 6] = [".c", ".cpp", ".cc", ".cxx", ".s", ".asm"];

/// Stands for the folder the library is installed in, in a path of the project's.
pub const LIBRARY: &str = "{library}";

/// A define as -D takes it: NAME, or NAME=value. The manifest arrives over the network
/// and each define is written as it stands into XML, a Makefile and a Keil list, so only
/// characters all of them hold safely, with nothing to quote, get through. A space or a
/// quote could otherwise carry a second compiler flag in with it.
pub static DEFINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*(?:=[A-Za-z0-9_.+/-]+)?$").expect("a valid pattern")
});

/// A character a folder of include_dirs cannot hold: each is written as it stands into
/// the project's own files, an IDE's XML, a list Keil separates with ;, a Makefile, so
/// one of these would break the file it lands in.
static UNSAFE_FOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("[\"'<>&;\\x00-\\x1f]").expect("a valid pattern"));

/// A drive letter, as in C:/x or C:x, which Windows reads as a place of its own.
static DRIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z]:").expect("a valid pattern"));

/// Where a library's version lives: the @version tag in its header's file comment.
static VERSION_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@version\s+(\S+)").expect("a valid pattern"));

/// An option's name, as typed after --with. Lower case only, so one word on the command
/// line always means one option.
static OPTION_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_-]*$").expect("a valid pattern"));

/// A library another one needs: name, or owner/name, then @tag, @branch or @commit,
/// then >= the oldest version.
static DEPENDENCY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^([A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)?)(?:@([A-Za-z0-9_.][A-Za-z0-9_./-]*))?\s*(?:>=\s*(\S+))?$")
        .expect("a valid pattern")
});

const DEPENDENCY_HELP: &str = "Write a library as its name, like \"osal\", with an owner for anyone else's, \
     like \"someone/lib\", then @ and a tag, branch or commit if one is wanted, like \"someone/lib@v1.2.0\", \
     and the oldest version that will do, like \"osal >= 1.1.0\".";

/// A manifest is missing, unreadable, or does not describe a usable library.
#[derive(Debug, Clone, PartialEq)]
pub struct ManifestError {
    pub message: String,
    /// The line of the file it is about, when one line is.
    pub line: Option<usize>,
}

impl ManifestError {
    pub fn new(message: impl Into<String>) -> ManifestError {
        ManifestError {
            message: message.into(),
            line: None,
        }
    }

    pub fn at(line: usize, message: impl Into<String>) -> ManifestError {
        ManifestError {
            message: message.into(),
            line: (line > 0).then_some(line),
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ManifestError {}

/// The name a define sets, without its value: HSE_VALUE from HSE_VALUE=8000000.
pub fn define_name(define: &str) -> &str {
    define
        .split_once('=')
        .map_or(define, |(name, _)| name)
        .trim()
}

/// (1, 2, 0) from "1.2.0", or None when it is not three whole numbers.
pub fn version_tuple(text: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = text.trim().split('.').collect();

    if parts.len() != 3
        || !parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }

    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Why a path in a manifest could lead out of the folder it is read in, or None.
///
/// A manifest arrives over the network before any of it is trusted, so a path such as
/// "../../.ssh/id_rsa" or "C:/Windows/win.ini" has to be refused, not followed.
/// Backslashes count as separators wherever the manifest is read: "..\x" leaves the
/// folder on Windows, and has to be refused on Linux as well, so one manifest means the
/// same on both.
pub fn unsafe_path(text: &str) -> Option<&'static str> {
    let text = text.replace('\\', "/");

    if text.trim().is_empty() {
        return Some("it is empty");
    }

    if text.starts_with('/') {
        return Some("it starts at the root of the drive");
    }

    if DRIVE.is_match(&text) {
        return Some("it names a drive");
    }

    if text.split('/').any(|part| part == "..") {
        return Some("it climbs out with ..");
    }

    None
}

/// A folder of include_dirs, as a path inside the library. It is written into the
/// project's own files as it stands, so it has to stay inside the library, and hold
/// nothing those files would read as anything but a path.
fn include_folder(value: &Node) -> Result<String, ManifestError> {
    let text = value.text().replace('\\', "/").trim().to_string();
    let mut why = unsafe_path(&text);

    if why.is_none() && UNSAFE_FOLDER.is_match(&text) {
        why = Some(
            "it holds a quote, <, >, &, ; or a control character, which an IDE's project file would not read as a path",
        );
    }

    if let Some(why) = why {
        return Err(ManifestError::at(
            value.line,
            format!(
                "include_dirs has {}: {why}. Give a folder inside the library, such as src.",
                value.shown()
            ),
        ));
    }

    Ok(text)
}

/// (True, the rest) for a path starting with {library}, which names a place in the
/// library's folder, else (False, the path), which names one from the project's folder.
pub fn in_library(text: &str) -> (bool, String) {
    let text = text.replace('\\', "/");

    if text == LIBRARY {
        return (true, String::new());
    }

    match text.strip_prefix(&format!("{LIBRARY}/")) {
        Some(rest) => (true, rest.trim_start_matches('/').to_string()),
        None => (false, text),
    }
}

/// Whether a manifest entry names more than one file.
pub fn is_pattern(path: &str) -> bool {
    path.contains(['*', '?', '['])
}

/// A test of whether a repository path matches a wildcard, the way a shell matches one:
/// the whole path, with * and ? staying inside one folder, ** for any number of folders,
/// none included, and [...] a set of characters. So *.c is the .c files at the top,
/// src/*.c those in src, and src/**/*.c every one below src, however deep.
pub fn wildcard(pattern: &str) -> Option<Regex> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::from(r"\A");
    let mut index = 0;

    while index < chars.len() {
        let rest = &chars[index..];

        if rest.starts_with(&['*', '*', '/']) {
            out.push_str("(?:[^/]*/)*");
            index += 3;
        } else if rest.starts_with(&['*', '*']) {
            out.push_str("(?s:.*)");
            index += 2;
        } else if chars[index] == '*' {
            out.push_str("[^/]*");
            index += 1;
        } else if chars[index] == '?' {
            out.push_str("[^/]");
            index += 1;
        } else if chars[index] == '['
            && let Some(offset) = chars.iter().skip(index + 2).position(|char| *char == ']')
        {
            // A ] right after the [ is one of the set, as a shell reads it.
            let end = index + 2 + offset;
            let mut body: String = chars[index + 1..end].iter().collect();

            if let Some(rest) = body.strip_prefix('!') {
                body = format!("^{rest}");
            }

            out.push('[');
            out.push_str(
                &body
                    .replace('\\', r"\\")
                    .replace('[', r"\[")
                    .replace(']', r"\]"),
            );
            out.push(']');
            index = end + 1;
        } else {
            out.push_str(&regex::escape(&chars[index].to_string()));
            index += 1;
        }
    }

    out.push_str(r"\z");

    Regex::new(&out).ok()
}

/// One manifest entry, against the paths a listing of the repository gives. An entry is
/// a file, a folder, or a wildcard. Results come back sorted, so a manifest produces the
/// same install every time.
pub fn expand_known(entry: &str, known: &[String]) -> Vec<String> {
    let text = paths::clean_posix(entry);
    let mut found: Vec<String> = if is_pattern(&text) {
        match wildcard(&text) {
            Some(matches) => known
                .iter()
                .filter(|path| matches.is_match(path))
                .cloned()
                .collect(),
            None => Vec::new(),
        }
    } else if known.contains(&text) {
        return vec![text];
    } else {
        let prefix = format!("{}/", text.trim_end_matches('/'));
        known
            .iter()
            .filter(|path| path.starts_with(&prefix))
            .cloned()
            .collect()
    };

    found.sort_by_key(|path| paths::sort_key(path));

    found
}

/// One manifest entry, against a checkout on disk. A wildcard means the same however the
/// library arrived, so it is matched against the checkout's paths exactly as against
/// GitHub's listing.
pub fn expand(root: &Path, entry: &str) -> Vec<String> {
    let text = paths::clean_posix(entry);

    if is_pattern(&text) {
        let Some(matches) = wildcard(&text) else {
            return Vec::new();
        };

        return paths::files_below(root)
            .into_iter()
            .filter(|path| path.split('/').next() != Some(".git") && matches.is_match(path))
            .collect();
    }

    let target = root.join(&text);

    if target.is_file() {
        return vec![text];
    }

    if target.is_dir() {
        return paths::files_below(&target)
            .into_iter()
            .map(|path| {
                if text == "." {
                    path
                } else {
                    format!("{text}/{path}")
                }
            })
            .collect();
    }

    Vec::new()
}

/// One file, where it lives in the repository and where it lands.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub source: String,
    /// Kept as posix text, because it ends up in CMake, Keil and IAR project files, and
    /// every one of them is happy with forward slashes.
    pub destination: String,
    /// The line of the file that names it.
    pub line: usize,
}

impl FileEntry {
    fn new(source: &str, destination: &str, line: usize) -> FileEntry {
        FileEntry {
            source: source.replace('\\', "/"),
            destination: destination.replace('\\', "/"),
            line,
        }
    }

    /// The destination's folder, relative to the install folder. "." at the top.
    pub fn folder(&self) -> String {
        let clean = paths::clean_posix(&self.destination);

        match clean.rfind('/') {
            Some(index) => clean[..index].to_string(),
            None => ".".to_string(),
        }
    }
}

/// Refuse a path to a file of the library that leads out of the library.
///
/// Read from a folder or a zip, a "from" of "../secret.txt" would copy a file from
/// beside the library into the user's project, and from GitHub it would write the
/// download outside the folder it was made in.
fn check_source(text: &str, where_: &str, line: usize) -> Result<(), ManifestError> {
    if let Some(reason) = unsafe_path(text) {
        return Err(ManifestError::at(
            line,
            format!(
                "{where_} lists {}, a file outside the library, which is never read: {reason}. \
                 Name files from the library's own folder.",
                quoted(text)
            ),
        ));
    }

    Ok(())
}

/// A list of a manifest, or an error saying it should be one.
fn list_of<'a>(value: Option<&'a Node>, key: &str) -> Result<&'a [Node], ManifestError> {
    match value {
        None => Ok(&[]),
        Some(node) if node.is_null() => Ok(&[]),
        Some(node) => node
            .as_list()
            .ok_or_else(|| ManifestError::at(node.line, format!("{key} is a list."))),
    }
}

/// Read headers: or sources:, where each item is a path, a folder, a wildcard, or a
/// {from, to} pair.
///
/// A plain entry keeps its path. A pair says exactly where in the library's folder the
/// file goes, and so it can only name one file; its from may be left out when it is the
/// same path as its to. A root, a place in the project's folder, is refused: the code is
/// built in the library's.
fn entries(root: &Path, raw: Option<&Node>, key: &str) -> Result<Vec<FileEntry>, ManifestError> {
    let mut result = Vec::new();

    for item in list_of(raw, key)? {
        let line = item.line;
        let (source, destination) = match item.as_map() {
            Some(map) => {
                if map.has("root") {
                    return Err(ManifestError::at(
                        line,
                        format!(
                            "{key} has {}. Code is built in the library's folder, so it has no root, a place in \
                             the project's: give it a to in the library's folder, or list it under once to give \
                             the project a copy of its own.",
                            item.shown()
                        ),
                    ));
                }

                let to = map.given("to").filter(|to| to.truthy()).map(Node::text);
                let from = map.given("from").map(Node::text).or_else(|| to.clone());

                let Some(from) = from else {
                    return Err(ManifestError::at(
                        line,
                        format!("a {key} entry is missing 'from': {}", item.shown()),
                    ));
                };

                (from, to)
            }
            None => (item.text(), None),
        };

        check_source(&source, key, line)?;

        if let Some(to) = &destination {
            if is_pattern(&source) {
                return Err(ManifestError::at(
                    line,
                    format!(
                        "'{source}' matches many files, so it cannot have a single 'to' of '{to}'. Drop the 'to' \
                         and each keeps its path, or list the files one by one."
                    ),
                ));
            }

            result.push(FileEntry::new(&paths::clean_posix(&source), to, line));
            continue;
        }

        let found = expand(root, &source);

        if found.is_empty() {
            // Kept so load() can refuse it by name rather than silently installing
            // nothing, which is what a wrong pattern feels like.
            let clean = paths::clean_posix(&source);
            result.push(FileEntry::new(&clean, &clean, line));
            continue;
        }

        for path in found {
            result.push(FileEntry::new(&path, &path, line));
        }
    }

    Ok(result)
}

/// Another library this one needs, and the oldest version of it that will do.
///
/// Written as "osal" or "osal >= 1.1.0", with an owner in front when it is not a NimaLTD
/// library: "someone/osal >= 1.1.0". A tag, a branch or a commit after an @ fetches
/// exactly that, "someone/osal@v1.2.0", where without one it comes from the repository's
/// default branch.
///
/// A repository with no libnima.yml of its own, such as a library its authors never
/// wrote one for, is written as a mapping, with one for it that this library carries:
///
/// ```yaml
/// - source: littlefs-project/littlefs@v2.9.3
///   manifest: manifests/littlefs.yml
///   minimum: 2.9.0
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Dependency {
    pub source: String,
    pub reference: Option<String>,
    pub name: String,
    pub minimum_text: Option<String>,
    pub minimum: Option<(u64, u64, u64)>,
    /// The file for a repository that has none, a file of the library that needs it.
    pub manifest: Option<String>,
}

impl Dependency {
    pub fn read(text: &Node) -> Result<Dependency, ManifestError> {
        let line = text.line;
        let mut manifest = None;
        let mut minimum = None;
        let mut name = None;

        let spoken = match text.as_map() {
            Some(map) => {
                let Some(source) = map.given("source").filter(|source| source.truthy()) else {
                    return Err(ManifestError::at(
                        line,
                        format!(
                            "requires.libraries has {}, with no source. {DEPENDENCY_HELP}",
                            text.shown()
                        ),
                    ));
                };

                manifest = map.given("manifest");
                minimum = map.given("minimum");
                name = map
                    .given("name")
                    .filter(|name| name.truthy())
                    .map(Node::text);
                source.text().trim().to_string()
            }
            None => text.text().trim().to_string(),
        };

        let found = DEPENDENCY.captures(&spoken);
        let bad = found.as_ref().is_none_or(|found| {
            found
                .get(2)
                .is_some_and(|reference| reference.as_str().contains(".."))
        });

        if bad {
            return Err(ManifestError::at(
                line,
                format!("requires.libraries has \"{spoken}\". {DEPENDENCY_HELP}"),
            ));
        }

        let found = found.expect("checked above");
        let source = found[1].to_string();
        let minimum_text = found
            .get(3)
            .map(|text| text.as_str().to_string())
            .or_else(|| minimum.map(|minimum| minimum.text().trim().to_string()));

        // Read like any other file of the library, so it cannot be one from outside it either.
        let manifest = match manifest {
            Some(manifest) => {
                let path = manifest.text();
                check_source(
                    &path,
                    &format!("requires.libraries ({source})"),
                    manifest.line,
                )?;
                Some(paths::clean_posix(&path))
            }
            None => None,
        };

        let minimum = match &minimum_text {
            Some(text) => Some(version_tuple(text).ok_or_else(|| {
                ManifestError::at(
                    line,
                    format!("requires.libraries has \"{spoken}\". The version is three numbers, like 1.1.0."),
                )
            })?),
            None => None,
        };

        Ok(Dependency {
            name: name.unwrap_or_else(|| paths::leaf(&source).to_string()),
            reference: found.get(2).map(|text| text.as_str().to_string()),
            source,
            minimum_text,
            minimum,
            manifest,
        })
    }

    /// What to fetch: owner/name, with @ref when one was given.
    pub fn fetch(&self) -> String {
        match &self.reference {
            Some(reference) => format!("{}@{reference}", self.source),
            None => self.source.clone(),
        }
    }

    /// Whether a library at this version will do. Any version will, with no minimum.
    pub fn satisfied_by(&self, version: Option<&str>) -> bool {
        let Some(minimum) = self.minimum else {
            return true;
        };

        version
            .and_then(version_tuple)
            .is_some_and(|have| have >= minimum)
    }
}

impl fmt::Display for Dependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.reference {
            Some(reference) => write!(f, "{}@{reference}", self.name)?,
            None => write!(f, "{}", self.name)?,
        }

        match &self.minimum_text {
            Some(minimum) => write!(f, " >= {minimum}"),
            None => Ok(()),
        }
    }
}

/// A part of a library the user chooses at install, as littlefs is for spif.
///
/// It brings files of the library's own, installed only when it is chosen, and other
/// libraries it needs, installed with it. A file it brings is a library file like any
/// other: replaced on every install, its USER CODE kept, and removed by the update that
/// turns the option off.
#[derive(Debug, Clone)]
pub struct LibOption {
    pub name: String,
    pub description: String,
    pub libraries: Vec<Dependency>,
    pub headers: Vec<FileEntry>,
    pub sources: Vec<FileEntry>,
    /// Its groups of CMake lines on the platform this was read for.
    pub cmake: Vec<crate::blocks::Group>,
    /// The part of it the platform reads, kept until the whole manifest is read.
    pub line: usize,
}

impl LibOption {
    pub fn code_files(&self) -> impl Iterator<Item = &FileEntry> {
        self.headers.iter().chain(&self.sources)
    }
}

/// A command run once the library's files are in place, such as one that makes a signing
/// key.
///
/// It runs on the user's own computer, so the user is always asked first, and is shown
/// exactly what will run. Nothing in the manifest can turn the question off: only the
/// user can, with --run or --yes. A file the step makes can be named with unless, and
/// when it is already in the project's folder the step is skipped without a question.
///
/// ```yaml
/// run:
///   - label: signing-key
///     description: Make MCUboot's signing key, sign_key.pem
///     command: [imgtool, keygen, -k, sign_key.pem, -t, ecdsa-p256]
///     unless: sign_key.pem
/// ```
///
/// {library} in command and args stands for the library's folder, and unless may start
/// with it too.
#[derive(Debug, Clone)]
pub struct RunStep {
    pub label: String,
    pub description: String,
    pub command: Vec<String>,
    pub args: Vec<String>,
    pub unless: Option<String>,
    pub unless_in_library: bool,
}

impl RunStep {
    fn read(data: &Node) -> Result<RunStep, ManifestError> {
        let line = data.line;
        let Some(map) = data.as_map() else {
            return Err(ManifestError::at(
                line,
                format!(
                    "run has {}. Each step is a mapping with a label and a command.",
                    data.shown()
                ),
            ));
        };

        let label = map.given("label").map(Node::text).unwrap_or_default();

        if !OPTION_NAME.is_match(&label) {
            return Err(ManifestError::at(
                line,
                format!(
                    "run has a step labelled {}. Label a step in lower case letters, digits, _ and -, starting \
                     with a letter, since it is typed after --run.",
                    quoted(&label)
                ),
            ));
        }

        let description = map
            .given("description")
            .map(Node::text)
            .unwrap_or_default()
            .trim()
            .to_string();

        if map.has("script") {
            return Err(ManifestError::at(
                map.line_of("script").unwrap_or(line),
                format!(
                    "run.{label} has a script, and libNIMA has no Python of its own to run one with. Give what \
                     runs under command, such as [python, tools/make_key.py]."
                ),
            ));
        }

        let Some(command) = map.given("command") else {
            return Err(ManifestError::at(
                line,
                format!("run.{label} needs a command."),
            ));
        };

        let words =
            |value: &Node| matches!(value.value, Value::Str(_) | Value::Int(_) | Value::Float(_));

        let good = command.as_list().is_some_and(|list| {
            !list.is_empty()
                && list
                    .iter()
                    .all(|part| words(part) && !part.text().is_empty())
        });

        if !good {
            return Err(ManifestError::at(
                command.line,
                format!(
                    "run.{label}.command should be a list: the program, then each argument, like [imgtool, keygen, \
                     -k, sign_key.pem]. It is run as it is, with no shell."
                ),
            ));
        }

        let command = command
            .as_list()
            .unwrap_or_default()
            .iter()
            .map(Node::text)
            .collect();
        let args = match map.given("args") {
            None => Vec::new(),
            Some(args) if !args.truthy() => Vec::new(),
            Some(args) => {
                let good = args.as_list().is_some_and(|list| list.iter().all(words));

                if !good {
                    return Err(ManifestError::at(
                        args.line,
                        format!("run.{label}.args should be a list of words."),
                    ));
                }

                args.as_list()
                    .unwrap_or_default()
                    .iter()
                    .map(Node::text)
                    .collect()
            }
        };

        let mut unless = None;
        let mut unless_in_library = false;

        if let Some(given) = map.given("unless") {
            let (found, rest) = in_library(&given.text());

            if let Some(reason) = unsafe_path(&rest) {
                return Err(ManifestError::at(
                    given.line,
                    format!(
                        "run.{label}.unless is {}, outside the project's folder: {reason}.",
                        given.shown()
                    ),
                ));
            }

            unless_in_library = found;
            unless = Some(rest.replace('\\', "/"));
        }

        Ok(RunStep {
            label,
            description,
            command,
            args,
            unless,
            unless_in_library,
        })
    }

    /// What the step runs, as a short hash, so a changed step is asked about again.
    pub fn fingerprint(&self) -> String {
        let mut parts: Vec<&str> = vec![""];
        parts.extend(self.command.iter().map(String::as_str));
        parts.push("\0");
        parts.extend(self.args.iter().map(String::as_str));

        short_hash(parts.join("\0").as_bytes())
    }
}

/// A peripheral the library uses, named to the user, and set up in CubeMX on STM32.
#[derive(Debug, Clone, PartialEq)]
pub struct Peripheral {
    pub kind: String,
    pub count: u64,
    pub note: String,
}

impl Peripheral {
    fn read(data: &Node) -> Result<Peripheral, ManifestError> {
        // A word alone, "- spi", or a mapping.
        let (kind, count, note) = match data.as_map() {
            Some(map) => (
                map.given("type").map(Node::text).unwrap_or_default(),
                map.given("count"),
                map.given("note")
                    .filter(|note| note.truthy())
                    .map(Node::text)
                    .unwrap_or_default(),
            ),
            None => (data.text(), None, String::new()),
        };

        let kind = kind.to_lowercase();
        let count = match count {
            None => 1,
            Some(count) => {
                let text = count.text();
                let text = text.trim();
                let number = if matches!(count.value, Value::Bool(_)) {
                    None
                } else {
                    text.parse::<u64>().ok().filter(|number| *number >= 1)
                };

                number.ok_or_else(|| {
                    ManifestError::at(
                        count.line,
                        format!(
                            "requires.peripherals {kind} has a count of {}. Give how many, a whole number from 1.",
                            count.shown()
                        ),
                    )
                })?
            }
        };

        Ok(Peripheral { kind, count, note })
    }

    pub fn describe(&self) -> String {
        let many = if self.count > 1 {
            format!(" x{}", self.count)
        } else {
            String::new()
        };
        let detail = if self.note.is_empty() {
            String::new()
        } else {
            format!(", {}", self.note)
        };

        format!("{}{many}{detail}", self.kind.to_uppercase())
    }
}

/// What a library needs from the project, whatever the project is. What depends on the
/// kind of project is in that platform's own section.
#[derive(Debug, Clone, Default)]
pub struct Requirements {
    pub libraries: Vec<Dependency>,
    pub peripherals: Vec<Peripheral>,
}

impl Requirements {
    fn read(data: Option<&Node>) -> Result<Requirements, ManifestError> {
        let Some(data) = data.filter(|data| !data.is_null()) else {
            return Ok(Requirements::default());
        };

        let Some(map) = data.as_map() else {
            return Err(ManifestError::at(
                data.line,
                "requires holds libnima, libraries and peripherals, each a key of its own under it.",
            ));
        };

        for entry in &map.entries {
            let moved = match entry.key.as_str() {
                "libnima" | "libraries" | "peripherals" => continue,
                "hal" | "cmsis" | "rtos" | "hal_modules" | "c_standard" => {
                    format!("requires.{0} is stm32.requires.{0} in libnima.yml.", entry.key)
                }
                "kconfig" => "requires.kconfig is zephyr.project.conf or esp32.project.conf in libnima.yml: the \
                              settings a library writes into the project's own Kconfig file."
                    .to_string(),
                key if is_installer_key(key) => format!("requires.{key} is requires.libnima in libnima.yml."),
                key => format!("requires has {key}. It holds libnima, libraries and peripherals."),
            };

            return Err(ManifestError::at(entry.line, moved));
        }

        let mut libraries = Vec::new();

        for item in list_of(map.get("libraries"), "requires.libraries")? {
            libraries.push(Dependency::read(item)?);
        }

        let mut peripherals = Vec::new();

        for item in list_of(map.get("peripherals"), "requires.peripherals")? {
            peripherals.push(Peripheral::read(item)?);
        }

        Ok(Requirements {
            libraries,
            peripherals,
        })
    }
}

/// A key under requires naming an installer: lib_installer, and the keys read before
/// lib-installer 3.0.0, installer, stm32_installer, zephyr_installer and the others.
pub fn is_installer_key(key: &str) -> bool {
    match key.strip_suffix("installer") {
        Some("") => true,
        Some(head) => {
            head.ends_with('_')
                && head[..head.len() - 1]
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        }
        None => false,
    }
}

/// The platforms a parsed manifest says it works on, or None when it says nothing.
/// platform is one name, platform: stm32, or a list of them, platform: [stm32, zephyr].
pub fn platforms_of(data: &Mapping) -> Result<Option<Vec<Platform>>, ManifestError> {
    let Some(value) = data.given("platform") else {
        return Ok(None);
    };

    let names: Vec<String> = match value.as_list() {
        Some(list) => list
            .iter()
            .map(|name| name.text().trim().to_lowercase())
            .collect(),
        None => vec![value.text().trim().to_lowercase()],
    };

    let known = Platform::ALL.map(Platform::name).join(", ");
    let wrong = || {
        ManifestError::at(
            value.line,
            format!(
                "platform has {}. Name the platform it works on, one of {known}, such as platform: stm32, or a \
                 list of them, such as platform: [stm32, zephyr].",
                value.shown()
            ),
        )
    };

    if names.is_empty() {
        return Err(wrong());
    }

    let mut found = Vec::new();

    for name in &names {
        let platform = Platform::from_name(name).ok_or_else(wrong)?;

        if !found.contains(&platform) {
            found.push(platform);
        }
    }

    Ok(Some(found))
}

/// Refuse a manifest written for a newer libNIMA than this one.
///
/// Called before anything else in the file is read. A manifest that needs a newer
/// libNIMA may use something this one does not understand, and reading it wrongly
/// without a word is worse than stopping with the way forward.
pub fn check_version(data: &Mapping) -> Result<(), ManifestError> {
    let Some(asked) = data
        .given("requires")
        .and_then(Node::as_map)
        .and_then(|requires| requires.given("libnima"))
    else {
        return Ok(());
    };

    let Some(wanted) = version_tuple(&asked.text()) else {
        return Err(ManifestError::at(
            asked.line,
            format!(
                "requires.libnima is {}. Write it as three numbers, like 1.1.0. YAML reads an unquoted 1.10 as the \
                 decimal number 1.1.",
                asked.shown()
            ),
        ));
    };

    let have = version_tuple(crate::VERSION).unwrap_or((0, 0, 0));

    if wanted > have {
        return Err(ManifestError::at(
            asked.line,
            format!(
                "This library needs libNIMA {}.{}.{} or newer, and this one is {}. Nothing was changed.\n\
                 Get the newest from:\n    {}",
                wanted.0,
                wanted.1,
                wanted.2,
                crate::VERSION,
                crate::RELEASES_PAGE
            ),
        ));
    }

    Ok(())
}

/// What a libnima.yml says about one library.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub root: PathBuf,
    pub name: String,
    pub description: String,
    pub repository: String,
    pub license: String,
    /// The platforms it works on.
    pub platforms: Vec<Platform>,
    pub kind: String,
    pub category: String,
    pub provides: Vec<String>,
    pub requires: Requirements,
    pub headers: Vec<FileEntry>,
    pub sources: Vec<FileEntry>,
    pub version: String,
    /// The project's own files: made when missing, and the user's from then on.
    pub once: Vec<FileEntry>,
    pub declared_include_dirs: Option<Vec<String>>,
    /// Defines the whole project is built with, for a library that reads its settings
    /// through one, as littlefs reads LFS_DEFINES. The whole project and not only the
    /// library's own files, because a define can change what its header declares, and
    /// every file including it has to agree.
    pub defines: Vec<String>,
    pub options: Vec<LibOption>,
    /// Parts the user chose. None until choose() says, so whatever installs a library
    /// without asking gets it without its options rather than with all of them.
    pub chosen: Vec<String>,
    /// Copied verbatim alongside the code. The Apache licence wants LICENSE and NOTICE
    /// to travel with it, and NOTICE is what carries the attribution.
    pub extras: Vec<FileEntry>,
    /// Extras with a root: the library's own files in the project's folder, such as its
    /// tools, written again on every install, and taken out by an uninstall.
    pub placed: Vec<FileEntry>,
    pub empty_extras: Vec<String>,
    pub run: Vec<RunStep>,
    /// Where the library came from, filled in by whoever fetched it: GitHub, with the
    /// ref asked for and the commit it was, or a zip or a folder.
    pub origin: Map<String, Json>,
    /// The section of the platform this was read for.
    pub part: Part,
    pub warnings: Vec<String>,
}

impl Manifest {
    /// Take these options in this install, and no others.
    pub fn choose(&mut self, names: &[String]) -> Result<(), ManifestError> {
        let unknown: Vec<&str> = names
            .iter()
            .filter(|name| !self.options.iter().any(|option| option.name == **name))
            .map(String::as_str)
            .collect();

        if !unknown.is_empty() {
            return Err(ManifestError::new(format!(
                "{} has no option called {}.",
                self.name,
                unknown.join(", ")
            )));
        }

        self.chosen = self
            .options
            .iter()
            .filter(|option| names.contains(&option.name))
            .map(|option| option.name.clone())
            .collect();

        Ok(())
    }

    /// The options taken, in the file's order.
    pub fn chosen_options(&self) -> impl Iterator<Item = &LibOption> {
        self.options
            .iter()
            .filter(|option| self.chosen.contains(&option.name))
    }

    pub fn option(&self, name: &str) -> Option<&LibOption> {
        self.options.iter().find(|option| option.name == name)
    }

    /// Headers and sources together, with those of the options taken. These are replaced
    /// on every install.
    pub fn code_files(&self) -> Vec<&FileEntry> {
        let mut found: Vec<&FileEntry> = self.headers.iter().chain(&self.sources).collect();
        found.extend(self.chosen_options().flat_map(|option| &option.headers));
        found.extend(self.chosen_options().flat_map(|option| &option.sources));

        found
    }

    /// Every code file the manifest names, whatever is chosen, for checking it.
    pub fn all_code_files(&self) -> Vec<&FileEntry> {
        let mut found: Vec<&FileEntry> = self.headers.iter().chain(&self.sources).collect();
        found.extend(self.options.iter().flat_map(LibOption::code_files));

        found
    }

    /// The folders that go on the IDE's include path.
    ///
    /// Where the headers land, those of the options taken included, unless the manifest
    /// names them. An option that is not taken adds nothing, so no include path points at
    /// a folder that was never installed.
    pub fn include_dirs(&self) -> Vec<String> {
        if let Some(declared) = self
            .declared_include_dirs
            .as_ref()
            .filter(|dirs| !dirs.is_empty())
        {
            return declared.clone();
        }

        let mut found: Vec<String> = self
            .headers
            .iter()
            .chain(self.chosen_options().flat_map(|option| &option.headers))
            .map(FileEntry::folder)
            .collect();

        found.sort();
        found.dedup();

        if found.is_empty() {
            found.push(".".to_string());
        }

        found
    }

    /// The libraries this one needs, with those the options taken bring.
    pub fn dependencies(&self) -> Vec<&Dependency> {
        self.requires
            .libraries
            .iter()
            .chain(self.chosen_options().flat_map(|option| &option.libraries))
            .collect()
    }

    /// File names the IDE has to compile, as they end up in the install folder.
    ///
    /// Only files a compiler can actually take are listed. Keil and IAR name every file
    /// in the project explicitly, so anything that gets in here shows up in the IDE's
    /// file tree and is handed to the compiler. A README that reached it by way of a
    /// mistyped manifest would be visible, confusing, and a build error.
    pub fn build_sources(&self) -> Vec<String> {
        self.sources
            .iter()
            .chain(self.chosen_options().flat_map(|option| &option.sources))
            .filter(|entry| COMPILED.contains(&paths::suffix(&entry.destination).as_str()))
            .map(|entry| entry.destination.clone())
            .collect()
    }

    /// Every repository path the manifest promises, those of every option included,
    /// chosen or not: its code, its once files, and the file it carries for a library
    /// that has none. Extras are optional.
    pub fn required_files(&self) -> Vec<String> {
        let mut found: Vec<String> = self
            .all_code_files()
            .iter()
            .map(|entry| entry.source.clone())
            .collect();
        found.extend(self.once.iter().map(|entry| entry.source.clone()));
        found.extend(
            self.requires
                .libraries
                .iter()
                .chain(self.options.iter().flat_map(|option| &option.libraries))
                .filter_map(|dependency| dependency.manifest.clone()),
        );

        found
    }

    /// Files claimed as both code and once.
    ///
    /// Code is overwritten on every install and a once file never is, so one claimed as
    /// both would have the user's edits quietly replaced while the output still reported
    /// it as kept. A wildcard that happens to sweep up the config header is the usual way
    /// this happens.
    pub fn clashes(&self) -> Vec<String> {
        let code = self.all_code_files();
        let mut found: Vec<String> = self
            .once
            .iter()
            .filter(|entry| code.iter().any(|file| file.source == entry.source))
            .map(|entry| entry.source.clone())
            .collect();

        found.sort();
        found.dedup();

        found
    }

    /// Files an option brings that the library, or another option, also lists.
    ///
    /// A file belongs to one place. One that is the library's and an option's both would
    /// be removed by the update that turns the option off, while the library still needs
    /// it.
    pub fn shared_option_files(&self) -> Vec<String> {
        let mut owner: Vec<(String, Option<&str>)> = self
            .headers
            .iter()
            .chain(&self.sources)
            .map(|entry| (entry.source.clone(), None))
            .collect();
        let mut shared = Vec::new();

        for option in &self.options {
            for entry in option.code_files() {
                match owner.iter().find(|(path, _)| *path == entry.source) {
                    Some((_, holder)) if *holder != Some(option.name.as_str()) => {
                        shared.push(entry.source.clone())
                    }
                    Some(_) => {}
                    None => owner.push((entry.source.clone(), Some(&option.name))),
                }
            }
        }

        shared.sort();
        shared.dedup();

        shared
    }

    /// A one line description of what this library is, for listings.
    pub fn summary(&self) -> String {
        if !self.category.is_empty() && self.category != self.kind {
            format!("{} / {}", self.kind, self.category)
        } else {
            self.kind.clone()
        }
    }
}

/// The headers and the sources of a manifest or one of its options.
fn code_lists<'a>(
    data: &'a Mapping,
    where_: &str,
) -> Result<(Option<&'a Node>, Option<&'a Node>), ManifestError> {
    if data.has("files") {
        return Err(ManifestError::at(
            data.line_of("files").unwrap_or(0),
            format!(
                "{where_} has files. In libnima.yml headers and sources stand on their own, with no files around them."
            ),
        ));
    }

    Ok((data.get("headers"), data.get("sources")))
}

/// The version a header's text says of itself, in the @version tag of its file comment.
pub fn version_in(text: &str) -> Option<String> {
    VERSION_TAG.captures(text).map(|found| found[1].to_string())
}

/// The @version tag in the first header's file comment, or None.
fn header_version(root: &Path, headers: &[FileEntry]) -> Option<String> {
    use std::io::Read;

    let first = headers.first()?;
    let mut buffer = vec![0; 4096];
    let mut file = fs::File::open(root.join(&first.source)).ok()?;
    let read = file.read(&mut buffer).ok()?;

    version_in(&String::from_utf8_lossy(&buffer[..read]))
}

/// A to or a root, checked and written with forward slashes: one place, never a
/// wildcard, inside the folder it names a place in, the project's or the library's, and
/// so never with {library}.
fn landing(
    place: &Node,
    source: &str,
    where_: &str,
    key: &str,
    in_project: bool,
) -> Result<String, ManifestError> {
    let text = place.text();
    let line = place.line;
    let folder = if in_project {
        "the project's folder"
    } else {
        "the library's folder"
    };

    if is_pattern(&text) {
        return Err(ManifestError::at(
            line,
            format!(
                "{where_} puts {} at {}. A {key} is one place, never a wildcard.",
                quoted(source),
                quoted(&text)
            ),
        ));
    }

    if in_library(&text).0 {
        let why = if !in_project {
            format!("A to is a place in the library's folder already, so leave {LIBRARY} out.")
        } else if where_ == "extras" {
            "A root is a place in the project's folder: for one in the library's, give it a to."
                .to_string()
        } else {
            "A once file is the project's own, so it lands in the project's folder, never the library's.".to_string()
        };

        return Err(ManifestError::at(
            line,
            format!(
                "{where_} puts {} at {}. {why}",
                quoted(source),
                quoted(&text)
            ),
        ));
    }

    if let Some(reason) = unsafe_path(&text) {
        return Err(ManifestError::at(
            line,
            format!(
                "{where_} puts {} at {}, outside {folder}: {reason}.",
                quoted(source),
                quoted(&text)
            ),
        ));
    }

    Ok(text.replace('\\', "/").trim_end_matches('/').to_string())
}

/// A once entry, as the file of the library it copies and where in the project that lands.
fn once_file(entry: &Node) -> Result<FileEntry, ManifestError> {
    let line = entry.line;
    let map = entry.as_map();

    if map.is_some_and(|map| map.has("root")) {
        return Err(ManifestError::at(
            line,
            format!(
                "once has {}. A once file always lands in the project's folder, so another place there is a to. A \
                 root is for an extra, which would otherwise go in the library's folder.",
                entry.shown()
            ),
        ));
    }

    let given = match map {
        Some(map) => map
            .given("from")
            .filter(|from| from.truthy())
            .or_else(|| map.given("to").filter(|to| to.truthy()))
            .map(Node::text),
        None => entry.truthy().then(|| entry.text()),
    };

    let Some(given) = given else {
        return Err(ManifestError::at(
            line,
            format!(
                "once has {}. Each entry is a path, or a mapping with a from and a to.",
                entry.shown()
            ),
        ));
    };

    check_source(&given, "once", line)?;

    let source = given.replace('\\', "/");
    let to = map.and_then(|map| map.given("to")).filter(|to| to.truthy());

    Ok(match to {
        Some(to) => FileEntry::new(&source, &landing(to, &source, "once", "to", true)?, line),
        None => FileEntry::new(&source, &source, line),
    })
}

/// An extra written as a mapping: a file, a folder or a wildcard of the library, with a
/// to, a place in the library's folder, or a root, one in the project's, where it is the
/// library's own, such as its tools. A file lands at the place itself; the files of a
/// folder keep their path below it; those a wildcard matches keep their names, under it.
/// from may be left out when it is the same path: "root: tools" takes the library's
/// tools/ to the project's tools/.
fn extra_mapping(
    root: &Path,
    item: &Node,
    map: &Mapping,
    extras: &mut Vec<FileEntry>,
    placed: &mut Vec<FileEntry>,
    empty: &mut Vec<String>,
) -> Result<(), ManifestError> {
    let line = item.line;

    if map.has("to") && map.has("root") {
        return Err(ManifestError::at(
            line,
            format!(
                "extras has {}, with both a to and a root. An extra lands in one place: a to is one in the library's \
                 folder, and a root one in the project's.",
                item.shown()
            ),
        ));
    }

    let key = if map.has("root") { "root" } else { "to" };
    let Some(place) = map.given(key).filter(|place| place.truthy()) else {
        return Err(ManifestError::at(
            line,
            format!(
                "extras has {}. An extra written as a mapping has a to, a place in the library's folder, or a root, a \
                 place in the project's, and a from when the library's path is not the same.",
                item.shown()
            ),
        ));
    };

    let source = map
        .given("from")
        .filter(|from| from.truthy())
        .map_or_else(|| place.text(), Node::text);

    check_source(&source, "extras", line)?;

    let place = landing(place, &source, "extras", key, key == "root")?;
    let source = source.replace('\\', "/").trim_end_matches('/').to_string();
    let found = expand(root, &source);

    for path in &found {
        let destination = if is_pattern(&source) {
            format!("{place}/{}", paths::leaf(path))
        } else if *path == paths::clean_posix(&source) {
            place.clone()
        } else {
            let rest = path
                .strip_prefix(&format!("{}/", paths::clean_posix(&source)))
                .unwrap_or(path);
            format!("{place}/{rest}")
        };

        let entry = FileEntry::new(path, &destination, line);

        if key == "root" {
            placed.push(entry);
        } else {
            extras.push(entry);
        }
    }

    if found.is_empty() {
        empty.push(source);
    }

    Ok(())
}

/// The manifest in a folder, or None when it holds none: libnima.yml, or the name
/// lib-installer read.
pub fn manifest_path(folder: &Path) -> Option<PathBuf> {
    MANIFEST_NAMES
        .iter()
        .map(|name| folder.join(name))
        .find(|path| path.is_file())
}

/// Read a manifest's text, as libnima.yml writes it: converted, when it has a name from
/// lib-installer. The bool says it was.
pub fn new_form(name: &str, text: &str) -> Result<(String, bool), ManifestError> {
    match name {
        MANIFEST_NAME => Ok((text.to_string(), false)),
        INSTALLER_NAME => Ok((convert::convert(text)?, true)),
        _ => Err(ManifestError::new(format!(
            "This library has only a {LIBRARY_NAME}, from before lib-installer 1.9.0, which libNIMA does not read. \
             It reads {MANIFEST_NAME}, and an {INSTALLER_NAME} asking for lib-installer 4.0.0."
        ))),
    }
}

/// Parse the text of a libnima.yml as far as its platform and whether this libNIMA can
/// read it. A file holding no mapping is an error.
pub fn parse(text: &str) -> Result<Node, ManifestError> {
    let parsed = yaml::loads(text).map_err(|error| ManifestError {
        message: format!("It is not valid YAML: {error}"),
        line: error.line,
    })?;

    match parsed {
        Some(node) if node.as_map().is_some() => Ok(node),
        _ => Err(ManifestError::new(
            "It should hold a mapping of settings, not a bare value.",
        )),
    }
}

/// Why a library's name cannot stand in a folder's name and a project's files, or None.
fn unsafe_name(name: &str) -> bool {
    name.trim().is_empty()
        || name.starts_with('.')
        || name
            .chars()
            .any(|char| char.is_control() || "/\\<>\"';&|:*?".contains(char))
}

/// What a whole manifest says, once its text is known to be readable.
fn read_library(
    root: &Path,
    data: &Mapping,
    running: Option<Platform>,
) -> Result<Manifest, ManifestError> {
    let name_node = data
        .given("name")
        .ok_or_else(|| ManifestError::new("It is missing the required 'name' field."))?;
    let name = name_node.text();

    if unsafe_name(&name) {
        return Err(ManifestError::at(
            name_node.line,
            format!(
                "name is {}. A library's name is its folder's and stands in the project's own files, so it holds no \
                 path, quote or control character.",
                name_node.shown()
            ),
        ));
    }

    let text_of = |key: &str| data.given(key).map(Node::text).unwrap_or_default();

    // The platforms it works on, checked against the platform of the run, so a library
    // for STM32 is refused in a Zephyr application here, not after it went in and the
    // build failed on the HAL.
    let platforms = platforms_of(data)?;
    let known = Platform::ALL.map(Platform::name).join(", ");

    let Some(platforms) = platforms else {
        return Err(ManifestError::new(format!(
            "{name} names no platform. Add the one it works on, such as platform: stm32, one of {known}."
        )));
    };

    if let Some(running) = running
        && !platforms.contains(&running)
    {
        let names: Vec<&str> = platforms.iter().map(|platform| platform.name()).collect();

        return Err(ManifestError::at(
            data.line_of("platform").unwrap_or(0),
            format!(
                "{name} works on {}, as platform in its {MANIFEST_NAME} says, not on {}, so it was not installed.",
                names.join(", "),
                running.name()
            ),
        ));
    }

    let kind = data
        .get("kind")
        .map_or_else(|| "driver".to_string(), |kind| kind.text().to_lowercase());
    let category = data
        .given("category")
        .map(Node::text)
        .unwrap_or_default()
        .to_lowercase();
    let mut provides: Vec<String> = list_of(data.get("provides"), "provides")?
        .iter()
        .map(|item| item.text().to_lowercase())
        .collect();

    // A library that is an operating system satisfies an RTOS requirement, whether or
    // not whoever wrote the manifest remembered to say so.
    if kind == "rtos" && !provides.iter().any(|item| item == "rtos") {
        provides.push("rtos".to_string());
    }

    let requires = Requirements::read(data.get("requires"))?;

    if data.has("install") {
        return Err(ManifestError::at(
            data.line_of("install").unwrap_or(0),
            "install has gone from libnima.yml: include_dirs and defines stand on their own, beside headers.",
        ));
    }

    let (headers, sources) = code_lists(data, MANIFEST_NAME)?;
    let headers = entries(root, headers, "headers")?;
    let sources = entries(root, sources, "sources")?;

    // The version lives in the code, in the @version tag of the first header's file
    // comment, so it cannot drift from what it describes. A version written here is only
    // a fallback, for a header that does not carry one.
    let version = header_version(root, &headers).unwrap_or_else(|| text_of("version"));

    let mut once = Vec::new();

    for entry in list_of(data.get("once"), "once")? {
        once.push(once_file(entry)?);
    }

    // Which folders go on the IDE's include path. Worked out from where the headers
    // land unless the manifest says.
    let declared_include_dirs = match data.given("include_dirs") {
        Some(dirs) if dirs.truthy() => {
            let Some(list) = dirs.as_list() else {
                return Err(ManifestError::at(
                    dirs.line,
                    "include_dirs is a list of folders inside the library, such as [src].",
                ));
            };

            Some(
                list.iter()
                    .map(include_folder)
                    .collect::<Result<Vec<_>, _>>()?,
            )
        }
        _ => None,
    };

    let mut defines = Vec::new();

    if let Some(given) = data.given("defines").filter(|given| given.truthy()) {
        let Some(list) = given.as_list() else {
            return Err(ManifestError::at(
                given.line,
                "defines should be a list, one define per line.",
            ));
        };

        // Refused like an escaping path, and for the same reason: what is written here
        // lands in the user's project files as it stands.
        for item in list {
            let define = item.text().trim().to_string();

            if !DEFINE.is_match(&define) {
                return Err(ManifestError::at(
                    item.line,
                    format!(
                        "defines has {}. Write a define as NAME or NAME=value, using letters, digits and _ . + - / only.",
                        quoted(&define)
                    ),
                ));
            }

            defines.push(define);
        }

        let names: Vec<&str> = defines.iter().map(|define| define_name(define)).collect();
        let twice = given_twice(names);

        if !twice.is_empty() {
            return Err(ManifestError::at(
                given.line,
                format!("defines sets {} more than once.", twice.join(", ")),
            ));
        }
    }

    let mut options = Vec::new();

    if let Some(given) = data.given("options").filter(|given| given.truthy()) {
        let Some(map) = given.as_map() else {
            return Err(ManifestError::at(
                given.line,
                "options should name each option, with what it brings under it.",
            ));
        };

        for entry in &map.entries {
            if !OPTION_NAME.is_match(&entry.key) {
                return Err(ManifestError::at(
                    entry.line,
                    format!(
                        "options has {}. Name an option in lower case letters, digits, _ and -, starting with a \
                         letter, since it is typed after --with.",
                        quoted(&entry.key)
                    ),
                ));
            }

            let Some(body) = entry.value.as_map() else {
                return Err(ManifestError::at(
                    entry.line,
                    format!(
                        "options.{} should hold a description, and the files and libraries it brings.",
                        entry.key
                    ),
                ));
            };

            let mut libraries = Vec::new();

            for item in list_of(
                body.get("libraries"),
                &format!("options.{}.libraries", entry.key),
            )? {
                libraries.push(Dependency::read(item)?);
            }

            let (option_headers, option_sources) =
                code_lists(body, &format!("options.{}", entry.key))?;

            options.push(LibOption {
                name: entry.key.clone(),
                description: body
                    .given("description")
                    .map(Node::text)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                libraries,
                headers: entries(
                    root,
                    option_headers,
                    &format!("options.{}.headers", entry.key),
                )?,
                sources: entries(
                    root,
                    option_sources,
                    &format!("options.{}.sources", entry.key),
                )?,
                cmake: Vec::new(),
                line: entry.line,
            });
        }
    }

    let mut extras = Vec::new();
    let mut placed = Vec::new();
    let mut empty_extras = Vec::new();
    let given_extras = data.given("extras").filter(|extras| extras.truthy());

    if let Some(given) = given_extras {
        for item in list_of(Some(given), "extras")? {
            if let Some(map) = item.as_map() {
                extra_mapping(root, item, map, &mut extras, &mut placed, &mut empty_extras)?;
                continue;
            }

            let text = item.text();
            check_source(&text, "extras", item.line)?;
            let found = expand(root, &text);

            for path in &found {
                extras.push(FileEntry::new(path, path, item.line));
            }

            if found.is_empty() && text != "LICENSE.md" && text != "NOTICE" {
                empty_extras.push(text);
            }
        }
    } else {
        // LICENSE.md and NOTICE, when it names none.
        for text in ["LICENSE.md", "NOTICE"] {
            for path in expand(root, text) {
                extras.push(FileEntry::new(&path, &path, 0));
            }
        }
    }

    // Commands run once the files are in place. Each is asked about first.
    let mut run = Vec::new();

    if let Some(given) = data.given("run").filter(|given| given.truthy()) {
        let Some(list) = given.as_list() else {
            return Err(ManifestError::at(
                given.line,
                "run should be a list of steps, each with a label and a command.",
            ));
        };

        for step in list {
            run.push(RunStep::read(step)?);
        }

        let twice = given_twice(run.iter().map(|step: &RunStep| step.label.as_str()));

        if !twice.is_empty() {
            return Err(ManifestError::at(
                given.line,
                format!(
                    "run has two steps labelled {}. Give each its own label.",
                    twice.join(", ")
                ),
            ));
        }
    }

    let mut manifest = Manifest {
        root: root.to_path_buf(),
        name,
        description: text_of("description"),
        repository: text_of("repository"),
        license: text_of("license"),
        platforms,
        kind,
        category,
        provides,
        requires,
        headers,
        sources,
        version,
        once,
        declared_include_dirs,
        defines,
        options,
        chosen: Vec::new(),
        extras,
        placed,
        empty_extras,
        run,
        origin: Map::new(),
        part: Part::None,
        warnings: Vec::new(),
    };

    // Last, so the platform sees the library as read: a section of its own comes from here.
    if let Some(running) = running {
        platform::read_part(running, &mut manifest, data)?;
    }

    Ok(manifest)
}

fn escapes(destination: &str) -> bool {
    destination.starts_with(['/', '\\'])
        || Path::new(destination).is_absolute()
        || destination.split(['/', '\\']).any(|part| part == "..")
}

/// Things worth saying out loud that are not bad enough to refuse the install.
fn warnings_of(manifest: &Manifest, data: &Mapping, running: Option<Platform>) -> Vec<String> {
    let mut found = Vec::new();

    if !KINDS.contains(&manifest.kind.as_str()) {
        found.push(format!(
            "kind '{}' is not one of: {}",
            manifest.kind,
            KINDS.join(", ")
        ));
    }

    if !manifest.category.is_empty() && !CATEGORIES.contains(&manifest.category.as_str()) {
        found.push(format!(
            "category '{}' is not one of: {}",
            manifest.category,
            CATEGORIES.join(", ")
        ));
    }

    if let Some(running) = running {
        found.extend(platform::warnings(running, manifest));
    }

    for entry in &manifest.empty_extras {
        found.push(format!("extras entry '{entry}' matched no file"));
    }

    for option in &manifest.options {
        if option.description.is_empty() {
            found.push(format!(
                "option '{}' has no description, so the question about it is only its name",
                option.name
            ));
        }
    }

    for key in data.keys() {
        if !schema::is_top_level(key) {
            found.push(format!("{key} is no key libNIMA reads, so it was left out"));
        }
    }

    found
}

/// A once file is the project's own from the moment it lands, so it cannot land in a library's folder, which every
/// install of that library replaces and its uninstall takes away: neither in the folder this library goes into nor in
/// that of a library it names, as the platform offers them. Where each really is is known only in a project, and the
/// install checks that against the project's record.
fn refuse_once_in_a_library(manifest: &Manifest, platform: Platform) -> Result<(), ManifestError> {
    let mut folders = vec![(
        manifest.name.as_str(),
        platform::default_folder(platform, &manifest.name),
    )];

    // On Arduino, a library another one needs goes into the libraries folder, never into the sketch the once files are in.
    if platform != Platform::Arduino {
        folders.extend(
            manifest
                .requires
                .libraries
                .iter()
                .chain(manifest.options.iter().flat_map(|option| &option.libraries))
                .map(|dependency| {
                    (
                        dependency.name.as_str(),
                        platform::default_folder(platform, &dependency.name),
                    )
                }),
        );
    }

    for entry in &manifest.once {
        let landing = paths::clean_posix(&entry.destination).to_lowercase();
        let found = folders.iter().find(|(_, folder)| {
            let folder = folder.to_lowercase();

            landing == folder || landing.starts_with(&format!("{folder}/"))
        });

        if let Some((name, folder)) = found {
            return Err(ManifestError::at(
                entry.line,
                if *name == manifest.name {
                    format!(
                        "once puts {} inside its own folder, {folder}, which every install replaces. A file of the \
                         library's own belongs under headers or sources, with USER CODE sections for what is the user's.",
                        entry.destination
                    )
                } else {
                    format!(
                        "once puts {} inside {folder}, where {name} goes, which every install of {name} replaces. A \
                         file of the project's own belongs in a folder of the project's.",
                        entry.destination
                    )
                },
            ));
        }
    }

    Ok(())
}

/// What a library's file says, checked against the library's own files, for the platform
/// of the run.
fn checked(
    root: &Path,
    path: &Path,
    data: &Mapping,
    running: Option<Platform>,
) -> Result<Manifest, ManifestError> {
    // Before anything else is read, since a manifest for a newer libNIMA may not even
    // have the fields this one expects.
    check_version(data)?;

    let in_file = |error: ManifestError| ManifestError {
        message: format!("{}: {}", path.display(), error.message),
        line: error.line,
    };

    if !data.has("name") {
        return Err(in_file(ManifestError::new(
            "It is missing the required 'name' field.",
        )));
    }

    if !data.has("headers") && !data.has("sources") && !data.has("files") {
        return Err(in_file(ManifestError::new(
            "It is missing its headers and its sources.",
        )));
    }

    let manifest = read_library(root, data, running)?;

    if manifest.code_files().is_empty() {
        return Err(in_file(ManifestError::new(
            "It lists no headers and no sources, so there is nothing to install.",
        )));
    }

    let missing: Vec<&FileEntry> = manifest
        .all_code_files()
        .into_iter()
        .chain(&manifest.once)
        .filter(|entry| !root.join(&entry.source).is_file())
        .collect();

    if let Some(first) = missing.first() {
        let listed: Vec<&str> = missing.iter().map(|entry| entry.source.as_str()).collect();

        return Err(in_file(ManifestError::at(
            first.line,
            format!(
                "It lists files that do not exist in the repository: {}",
                listed.join(", ")
            ),
        )));
    }

    for dependency in manifest
        .requires
        .libraries
        .iter()
        .chain(manifest.options.iter().flat_map(|option| &option.libraries))
    {
        if let Some(carried) = &dependency.manifest
            && !root.join(carried).is_file()
        {
            return Err(in_file(ManifestError::new(format!(
                "It lists files that do not exist in the repository: {carried}"
            ))));
        }
    }

    // Two files of the code that land in one place: the second would quietly replace
    // the first. Checked for the files installed together, the library's and each
    // option's, since two options may well bring a port.c each. One file listed twice,
    // by a wildcard and by name, is no clash. An extra's to can put it anywhere in the
    // library's folder, so the extras are in each.
    let base: Vec<&FileEntry> = manifest.headers.iter().chain(&manifest.sources).collect();
    let mut groups = vec![base.clone()];

    for option in &manifest.options {
        let mut group = base.clone();
        group.extend(option.code_files());
        groups.push(group);
    }

    for group in groups {
        let mut landings: Vec<(&str, &str, usize)> = group
            .iter()
            .chain(manifest.extras.iter().collect::<Vec<_>>().iter())
            .map(|entry| {
                (
                    entry.destination.as_str(),
                    entry.source.as_str(),
                    entry.line,
                )
            })
            .collect();

        landings.sort();
        landings.dedup_by_key(|(destination, source, _)| (*destination, *source));

        let clashing = given_twice(landings.iter().map(|(destination, _, _)| *destination));

        if !clashing.is_empty() {
            let line = landings
                .iter()
                .find(|(destination, _, _)| *destination == clashing[0])
                .map_or(0, |(_, _, line)| *line);

            return Err(in_file(ManifestError::at(
                line,
                format!(
                    "It lists two files that land at {} in the library's folder, so one would replace the other. \
                     Give one a to of its own.",
                    clashing.join(", ")
                ),
            )));
        }
    }

    // A file found where the manifest says, that is a link to somewhere else: a folder
    // or a zip can hold one, and following it would copy a file from outside the library.
    let resolved_root = paths::resolve(root);

    for source in manifest.required_files().iter().chain(
        manifest
            .extras
            .iter()
            .chain(&manifest.placed)
            .map(|entry| &entry.source),
    ) {
        if !paths::within(&paths::resolve(&root.join(source)), &resolved_root) {
            return Err(in_file(ManifestError::new(format!(
                "It lists {source}, which leads outside the library, so it is never read. A link to a file \
                 elsewhere has to be replaced by the file itself."
            ))));
        }
    }

    let destinations: Vec<&str> = manifest
        .once
        .iter()
        .map(|entry| entry.destination.as_str())
        .collect();
    let twice = given_twice(destinations.iter().copied());

    if !twice.is_empty() {
        return Err(in_file(ManifestError::new(format!(
            "It lists two once files that both land in {}.",
            twice.join(", ")
        ))));
    }

    // An extra with a root is written on every install, a once file never is, so one
    // place cannot be both; nor two extras.
    let landings: Vec<&str> = manifest
        .placed
        .iter()
        .map(|entry| entry.destination.as_str())
        .collect();
    let mut both: Vec<String> = given_twice(landings.iter().copied());
    both.extend(
        landings
            .iter()
            .filter(|landing| destinations.contains(landing))
            .map(|landing| landing.to_string()),
    );
    both.sort();
    both.dedup();

    if !both.is_empty() {
        return Err(in_file(ManifestError::new(format!(
            "It lists two files that both land in the project at {}.",
            both.join(", ")
        ))));
    }

    let shared = manifest.shared_option_files();

    if !shared.is_empty() {
        return Err(in_file(ManifestError::new(format!(
            "It lists these both in an option and somewhere else, so turning the option off would remove a file \
             still needed: {}",
            shared.join(", ")
        ))));
    }

    let clashing = manifest.clashes();

    if !clashing.is_empty() {
        return Err(in_file(ManifestError::new(format!(
            "It lists these as both code and configuration, so an update would overwrite the user's settings: {}. \
             Narrow the pattern, or list the files one by one.",
            clashing.join(", ")
        ))));
    }

    // Always refused, never merely warned about. A manifest can arrive over the network,
    // and a destination like "../../Core/Src/main.c" would write into the user's own code
    // rather than the folder they agreed to.
    let escaping: Vec<&str> = manifest
        .all_code_files()
        .into_iter()
        .chain(&manifest.once)
        .chain(&manifest.placed)
        .filter(|entry| escapes(&entry.destination) || unsafe_path(&entry.destination).is_some())
        .map(|entry| entry.destination.as_str())
        .collect();

    if !escaping.is_empty() {
        return Err(in_file(ManifestError::new(format!(
            "It sends files outside the install folder, which is never allowed: {}",
            escaping.join(", ")
        ))));
    }

    if let Some(platform) = running {
        refuse_once_in_a_library(&manifest, platform).map_err(in_file)?;
    }

    let mut manifest = manifest;
    manifest.warnings = warnings_of(&manifest, data, running);

    Ok(manifest)
}

/// Read the libnima.yml at the root of a library, for the platform of the run. A library
/// with only an installer.yml is read through the converter, and is told about it.
///
/// None for the platform reads what every platform shares and no platform's own section.
pub fn load(library_root: &Path, running: Option<Platform>) -> Result<Manifest, ManifestError> {
    let root = paths::resolve(library_root);

    let Some(path) = manifest_path(&root) else {
        return Err(ManifestError::new(format!(
            "No {MANIFEST_NAME} in {}, so there is nothing that says how to install it.",
            root.display()
        )));
    };

    let in_file = |error: ManifestError| ManifestError {
        message: format!("{}: {}", path.display(), error.message),
        line: error.line,
    };

    let raw = fs::read(&path).map_err(|error| in_file(ManifestError::new(error.to_string())))?;
    let text =
        String::from_utf8(raw).map_err(|_| in_file(ManifestError::new("It is not UTF-8 text.")))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (text, converted) = new_form(&name, &text).map_err(in_file)?;

    load_text(&root, &path, &text, running).map(|mut manifest| {
        if converted {
            manifest.warnings.push(format!(
                "it has an {INSTALLER_NAME} from lib-installer, read through the converter. libnima --convert writes \
                 it out as {MANIFEST_NAME}"
            ));
        }

        manifest
    })
}

/// Read a libnima.yml's text against the library's own folder.
pub fn load_text(
    root: &Path,
    path: &Path,
    text: &str,
    running: Option<Platform>,
) -> Result<Manifest, ManifestError> {
    let in_file = |error: ManifestError| ManifestError {
        message: format!("{}: {}", path.display(), error.message),
        line: error.line,
    };

    let node = parse(text).map_err(in_file)?;
    let data = node.as_map().expect("parse() gives a mapping");

    checked(root, path, data, running).map_err(|error| {
        // An error from checked() already names the file when it is about the whole of it.
        if error.message.contains(&path.display().to_string()) {
            error
        } else {
            in_file(error)
        }
    })
}

#[cfg(test)]
pub(crate) mod testing {
    //! A throwaway library on disk: a test states only the part it cares about.

    use std::fs;
    use std::path::{Path, PathBuf};

    /// Write a library's files, and its libnima.yml with what every one needs around `body`.
    pub fn library(root: &Path, name: &str, body: &str, files: &[&str]) -> PathBuf {
        let folder = root.join(name);

        for file in files {
            let path = folder.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("/* {file} */\n")).unwrap();
        }

        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("libnima.yml"),
            format!("name: {name}\nplatform: stm32\n{body}"),
        )
        .unwrap();

        folder
    }
}

#[cfg(test)]
mod tests {
    use super::testing::library;
    use super::*;

    const ONE: &str = "headers:\n  - src/demo.h\nsources:\n  - src/demo.c\n";

    fn load_body(body: &str, files: &[&str]) -> Result<Manifest, ManifestError> {
        let root = tempfile::tempdir().unwrap();
        let folder = library(root.path(), "demo", body, files);

        load(&folder, Some(Platform::Stm32))
    }

    fn error_of(body: &str, files: &[&str]) -> String {
        load_body(body, files).unwrap_err().message
    }

    #[test]
    fn the_smallest_library_reads() {
        let manifest = load_body(ONE, &["src/demo.h", "src/demo.c"]).unwrap();

        assert_eq!(manifest.name, "demo");
        assert_eq!(manifest.kind, "driver");
        assert_eq!(manifest.platforms, [Platform::Stm32]);
        assert_eq!(manifest.headers[0].destination, "src/demo.h");
        assert_eq!(manifest.include_dirs(), ["src"]);
        assert_eq!(manifest.build_sources(), ["src/demo.c"]);
        assert!(manifest.warnings.is_empty(), "{:?}", manifest.warnings);
    }

    #[test]
    fn the_version_comes_from_the_first_header() {
        let root = tempfile::tempdir().unwrap();
        let folder = library(
            root.path(),
            "demo",
            &format!("version: 9.9.9\n{ONE}"),
            &["src/demo.c"],
        );
        fs::create_dir_all(folder.join("src")).unwrap();

        fs::write(folder.join("src/demo.h"), "/** @version 2.1.0 */\n").unwrap();
        assert_eq!(
            load(&folder, Some(Platform::Stm32)).unwrap().version,
            "2.1.0"
        );

        fs::write(folder.join("src/demo.h"), "no tag\n").unwrap();
        assert_eq!(
            load(&folder, Some(Platform::Stm32)).unwrap().version,
            "9.9.9"
        );
    }

    #[test]
    fn a_folder_with_no_file_says_so() {
        let root = tempfile::tempdir().unwrap();
        let error = load(root.path(), None).unwrap_err();

        assert!(
            error.message.contains("No libnima.yml"),
            "{}",
            error.message
        );
    }

    #[test]
    fn a_file_from_before_lib_installer_1_9_is_refused_with_the_reason() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("library.yml"), "name: x\n").unwrap();

        assert!(
            load(root.path(), None)
                .unwrap_err()
                .message
                .contains("from before lib-installer 1.9.0")
        );
    }

    #[test]
    fn an_installer_yml_of_lib_installer_4_reads_through_the_converter() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("demo.h"), "").unwrap();
        fs::write(
            root.path().join("installer.yml"),
            "name: x\nplatform: stm32\nrequires:\n  lib_installer: 4.0.0\n  hal_modules: [spi]\nheaders: [demo.h]\n",
        )
        .unwrap();

        let manifest = load(root.path(), Some(Platform::Stm32)).unwrap();

        assert_eq!(manifest.part.stm32().hal_modules, ["spi"]);
        assert!(
            manifest
                .warnings
                .iter()
                .any(|warning| warning.contains("libnima --convert"))
        );
    }

    #[test]
    fn a_library_for_another_platform_is_refused_with_the_line() {
        let error = load_body(ONE, &["src/demo.h", "src/demo.c"])
            .map(|_| ())
            .err();
        assert!(error.is_none());

        let root = tempfile::tempdir().unwrap();
        let folder = library(root.path(), "demo", ONE, &["src/demo.h", "src/demo.c"]);
        let error = load(&folder, Some(Platform::Zephyr)).unwrap_err();

        assert!(
            error.message.contains("works on stm32"),
            "{}",
            error.message
        );
        assert_eq!(error.line, Some(2));
    }

    #[test]
    fn a_library_naming_no_platform_is_refused() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("a.h"), "").unwrap();
        fs::write(root.path().join("libnima.yml"), "name: x\nheaders: [a.h]\n").unwrap();

        assert!(
            load(root.path(), None)
                .unwrap_err()
                .message
                .contains("names no platform")
        );
    }

    #[test]
    fn a_platform_that_does_not_exist_is_refused() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("libnima.yml"),
            "name: x\nplatform: avr\nheaders: [a.h]\n",
        )
        .unwrap();

        assert!(
            load(root.path(), None)
                .unwrap_err()
                .message
                .contains("platform has 'avr'")
        );
    }

    #[test]
    fn a_newer_libnima_is_asked_for_before_anything_else() {
        let body = format!("requires:\n  libnima: 99.0.0\n  bad: key\n{ONE}");
        let error = load_body(&body, &[]).unwrap_err();

        assert!(
            error.message.contains("needs libNIMA 99.0.0 or newer"),
            "{}",
            error.message
        );
        assert_eq!(error.line, Some(4));
    }

    #[test]
    fn a_version_that_is_not_three_numbers_is_refused() {
        let error = error_of(&format!("requires:\n  libnima: 1.10\n{ONE}"), &[]);

        assert!(error.contains("three numbers"), "{error}");
    }

    #[test]
    fn keys_from_lib_installer_are_refused_with_where_they_went() {
        for (body, wanted) in [
            ("requires:\n  hal: true\n", "stm32.requires.hal"),
            ("requires:\n  lib_installer: 4.0.0\n", "requires.libnima"),
            ("requires:\n  zephyr_installer: 2.0.0\n", "requires.libnima"),
            ("requires:\n  kconfig: [I2C]\n", "zephyr.project.conf"),
            ("requires:\n  typo: 1\n", "It holds libnima"),
            (
                "install:\n  defines: [A]\n",
                "include_dirs and defines stand on their own",
            ),
            ("files:\n  headers: [a.h]\n", "with no files around them"),
        ] {
            let error = error_of(&format!("{body}{ONE}"), &["src/demo.h", "src/demo.c"]);
            assert!(error.contains(wanted), "{error}");
        }
    }

    #[test]
    fn a_file_the_repository_does_not_have_is_named_with_its_line() {
        let error = load_body(ONE, &["src/demo.h"]).unwrap_err();

        assert!(
            error
                .message
                .contains("do not exist in the repository: src/demo.c"),
            "{}",
            error.message
        );
        assert_eq!(error.line, Some(6));
    }

    #[test]
    fn a_path_out_of_the_library_is_refused() {
        for path in [
            "../../secret.h",
            "/etc/passwd",
            "C:/Windows/win.ini",
            "..\\x.h",
        ] {
            let error = error_of(&format!("headers:\n  - '{path}'\n"), &[]);
            assert!(
                error.contains("a file outside the library"),
                "{path}: {error}"
            );
        }
    }

    #[test]
    fn a_destination_out_of_the_library_is_refused() {
        let error = error_of(
            "headers:\n  - from: a.h\n    to: ../../Core/Src/main.c\n",
            &["a.h"],
        );

        assert!(error.contains("outside the install folder"), "{error}");
    }

    #[test]
    fn a_name_that_is_a_path_is_refused() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("libnima.yml"),
            "name: ../../x\nplatform: stm32\nheaders: [a.h]\n",
        )
        .unwrap();

        assert!(
            load(root.path(), None)
                .unwrap_err()
                .message
                .contains("holds no path")
        );
    }

    #[test]
    fn wildcards_match_the_way_a_shell_does() {
        let known: Vec<String> = ["a.c", "src/b.c", "src/x/c.c", "src/x/d.h", "other/e.c"]
            .iter()
            .map(ToString::to_string)
            .collect();

        assert_eq!(expand_known("*.c", &known), ["a.c"]);
        assert_eq!(expand_known("src/*.c", &known), ["src/b.c"]);
        assert_eq!(expand_known("src/**/*.c", &known), ["src/b.c", "src/x/c.c"]);
        assert_eq!(expand_known("src/x", &known), ["src/x/c.c", "src/x/d.h"]);
        assert_eq!(expand_known("src/[bq].c", &known), ["src/b.c"]);
        assert_eq!(expand_known("src/[!b].c", &known), Vec::<String>::new());
        assert_eq!(expand_known("a.c", &known), ["a.c"]);
    }

    #[test]
    fn a_wildcard_means_the_same_on_disk() {
        let root = tempfile::tempdir().unwrap();

        for file in ["a.c", "src/b.c", "src/x/c.c", ".git/hooks/d.c"] {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }

        assert_eq!(expand(root.path(), "*.c"), ["a.c"]);
        assert_eq!(expand(root.path(), "src/**/*.c"), ["src/b.c", "src/x/c.c"]);
        assert_eq!(
            expand(root.path(), "**/*.c"),
            ["a.c", "src/b.c", "src/x/c.c"]
        );
        assert_eq!(expand(root.path(), "src"), ["src/b.c", "src/x/c.c"]);
    }

    #[test]
    fn dependencies_read_every_way_they_are_written() {
        let read = |text: &str| Dependency::read(&Node::string(text, 1));

        let plain = read("osal").unwrap();
        assert_eq!(
            (plain.source.as_str(), plain.name.as_str(), plain.minimum),
            ("osal", "osal", None)
        );

        let full = read("someone/osal@v1.2.0 >= 1.1.0").unwrap();
        assert_eq!(full.source, "someone/osal");
        assert_eq!(full.reference.as_deref(), Some("v1.2.0"));
        assert_eq!(full.minimum, Some((1, 1, 0)));
        assert_eq!(full.fetch(), "someone/osal@v1.2.0");
        assert_eq!(full.to_string(), "osal@v1.2.0 >= 1.1.0");

        assert!(full.satisfied_by(Some("1.1.0")));
        assert!(!full.satisfied_by(Some("1.0.9")));
        assert!(!full.satisfied_by(None));

        assert!(read("a b c").is_err());
        assert!(read("osal@../x").is_err());
        assert!(read("osal >= 1.1").is_err());
    }

    #[test]
    fn a_dependency_can_carry_its_own_manifest() {
        let node = yaml::loads("source: littlefs-project/littlefs@v2.9.3\nmanifest: manifests/littlefs.yml\nminimum: 2.9.0\n")
            .unwrap()
            .unwrap();
        let found = Dependency::read(&node).unwrap();

        assert_eq!(found.name, "littlefs");
        assert_eq!(found.manifest.as_deref(), Some("manifests/littlefs.yml"));
        assert_eq!(found.minimum, Some((2, 9, 0)));

        let node = yaml::loads("source: a/b\nmanifest: ../x.yml\n")
            .unwrap()
            .unwrap();
        assert!(Dependency::read(&node).is_err());
    }

    #[test]
    fn a_define_that_could_carry_a_flag_is_refused() {
        for define in ["A B", "A=\"x\"", "A;B", "-DA"] {
            let body = format!("defines:\n  - '{define}'\n{ONE}");
            let error = error_of(&body, &["src/demo.h", "src/demo.c"]);
            assert!(
                error.contains("Write a define as NAME"),
                "{define}: {error}"
            );
        }
    }

    #[test]
    fn a_define_given_twice_is_refused() {
        let body = format!("defines: [A=1, A=2]\n{ONE}");

        assert!(error_of(&body, &["src/demo.h", "src/demo.c"]).contains("sets A more than once"));
    }

    #[test]
    fn a_folder_on_the_include_path_is_checked() {
        for folder in ["../x", "a;b", "a\"b"] {
            let body = format!("include_dirs:\n  - '{folder}'\n{ONE}");
            let error = error_of(&body, &["src/demo.h", "src/demo.c"]);
            assert!(error.contains("include_dirs has"), "{folder}: {error}");
        }
    }

    #[test]
    fn options_read_and_add_to_what_is_built_when_taken() {
        let body = format!(
            "{ONE}options:\n  fast:\n    description: Go fast\n    headers: [opt/f.h]\n    sources: [opt/f.c]\n    libraries: [osal >= 1.0.0]\n"
        );
        let mut manifest =
            load_body(&body, &["src/demo.h", "src/demo.c", "opt/f.h", "opt/f.c"]).unwrap();

        assert_eq!(manifest.code_files().len(), 2);
        assert!(manifest.dependencies().is_empty());

        manifest.choose(&["fast".to_string()]).unwrap();

        assert_eq!(manifest.code_files().len(), 4);
        assert_eq!(manifest.include_dirs(), ["opt", "src"]);
        assert_eq!(manifest.build_sources(), ["src/demo.c", "opt/f.c"]);
        assert_eq!(manifest.dependencies()[0].name, "osal");

        assert!(manifest.choose(&["slow".to_string()]).is_err());
    }

    #[test]
    fn an_option_with_a_bad_name_is_refused() {
        let body = format!("{ONE}options:\n  Bad Name:\n    description: x\n");

        assert!(error_of(&body, &["src/demo.h", "src/demo.c"]).contains("Name an option"));
    }

    #[test]
    fn a_file_both_an_option_and_the_library_list_is_refused() {
        let body = format!("{ONE}options:\n  a:\n    sources: [src/demo.c]\n");

        assert!(error_of(&body, &["src/demo.h", "src/demo.c"]).contains("both in an option"));
    }

    #[test]
    fn a_file_both_code_and_once_is_refused() {
        let body = format!("{ONE}once:\n  - src/demo.c\n");

        assert!(
            error_of(&body, &["src/demo.h", "src/demo.c"]).contains("both code and configuration")
        );
    }

    #[test]
    fn two_files_landing_in_one_place_are_refused() {
        let body = "headers:\n  - from: a.h\n    to: x.h\n  - from: b.h\n    to: x.h\n";

        assert!(error_of(body, &["a.h", "b.h"]).contains("land at x.h"));
    }

    #[test]
    fn one_file_listed_twice_is_no_clash() {
        let body = "headers:\n  - a.h\n  - '*.h'\n";

        assert!(load_body(body, &["a.h"]).is_ok());
    }

    #[test]
    fn extras_land_where_they_say() {
        let body = format!(
            "{ONE}extras:\n  - LICENSE.md\n  - to: docs/N.txt\n    from: NOTICE\n  - root: tools\n  - from: t/*.py\n    root: scripts\n  - missing.txt\n"
        );
        let manifest = load_body(
            &body,
            &[
                "src/demo.h",
                "src/demo.c",
                "LICENSE.md",
                "NOTICE",
                "tools/a.py",
                "tools/sub/b.py",
                "t/c.py",
            ],
        )
        .unwrap();

        let extras: Vec<&str> = manifest
            .extras
            .iter()
            .map(|entry| entry.destination.as_str())
            .collect();
        let placed: Vec<&str> = manifest
            .placed
            .iter()
            .map(|entry| entry.destination.as_str())
            .collect();

        assert_eq!(extras, ["LICENSE.md", "docs/N.txt"]);
        assert_eq!(placed, ["tools/a.py", "tools/sub/b.py", "scripts/c.py"]);
        assert_eq!(manifest.empty_extras, ["missing.txt"]);
        assert!(
            manifest
                .warnings
                .iter()
                .any(|warning| warning.contains("matched no file"))
        );
    }

    #[test]
    fn extras_default_to_the_licence_and_the_notice() {
        let manifest = load_body(ONE, &["src/demo.h", "src/demo.c", "NOTICE"]).unwrap();

        assert_eq!(manifest.extras.len(), 1);
        assert_eq!(manifest.extras[0].destination, "NOTICE");
        assert!(manifest.empty_extras.is_empty());
    }

    #[test]
    fn a_bad_extra_is_refused() {
        for (extra, wanted) in [
            ("  - to: a\n    root: b\n", "both a to and a root"),
            ("  - from: a\n", "has a to"),
            (
                "  - root: '{library}/x'\n",
                "A root is a place in the project's folder",
            ),
            ("  - to: '{library}/x'\n", "leave {library} out"),
            ("  - root: ../x\n", "a file outside the library"),
            (
                "  - from: NOTICE\n    root: ../x\n",
                "outside the project's folder",
            ),
            ("  - from: NOTICE\n    to: 'a*'\n", "never a wildcard"),
        ] {
            let body = format!("{ONE}extras:\n{extra}");
            let error = error_of(&body, &["src/demo.h", "src/demo.c", "NOTICE"]);
            assert!(error.contains(wanted), "{extra}: {error}");
        }
    }

    #[test]
    fn once_files_read_every_way_they_are_written() {
        let body = format!(
            "{ONE}once:\n  - VERSION\n  - from: t/port.c\n    to: Core/Src/port.c\n  - to: notes.md\n"
        );
        let manifest = load_body(
            &body,
            &[
                "src/demo.h",
                "src/demo.c",
                "VERSION",
                "t/port.c",
                "notes.md",
            ],
        )
        .unwrap();
        let once: Vec<(&str, &str)> = manifest
            .once
            .iter()
            .map(|entry| (entry.source.as_str(), entry.destination.as_str()))
            .collect();

        assert_eq!(
            once,
            [
                ("VERSION", "VERSION"),
                ("t/port.c", "Core/Src/port.c"),
                ("notes.md", "notes.md")
            ]
        );
    }

    #[test]
    fn a_bad_once_file_is_refused() {
        for (entry, wanted) in [
            ("  - root: x\n", "A root is for an extra"),
            (
                "  - from: a\n    to: '{library}/x'\n",
                "never the library's",
            ),
            (
                "  - from: a\n    to: ../x\n",
                "outside the project's folder",
            ),
            ("  - ../x\n", "a file outside the library"),
        ] {
            let body = format!("{ONE}once:\n{entry}");
            let error = error_of(&body, &["src/demo.h", "src/demo.c", "a"]);
            assert!(error.contains(wanted), "{entry}: {error}");
        }
    }

    #[test]
    fn run_steps_read_and_a_script_is_refused() {
        let body = format!(
            "{ONE}run:\n  - label: make-key\n    description: Make it\n    command: [imgtool, keygen, 5]\n    args: [a, 1]\n    unless: '{{library}}/k.pem'\n"
        );
        let manifest = load_body(&body, &["src/demo.h", "src/demo.c"]).unwrap();
        let step = &manifest.run[0];

        assert_eq!(step.command, ["imgtool", "keygen", "5"]);
        assert_eq!(step.args, ["a", "1"]);
        assert_eq!(step.unless.as_deref(), Some("k.pem"));
        assert!(step.unless_in_library);

        for (run, wanted) in [
            ("  - label: a\n    script: x.py\n", "no Python of its own"),
            ("  - label: Bad\n    command: [a]\n", "Label a step"),
            ("  - label: a\n", "needs a command"),
            ("  - label: a\n    command: x\n", "should be a list"),
            (
                "  - label: a\n    command: [x]\n    args: y\n",
                "a list of words",
            ),
            (
                "  - label: a\n    command: [x]\n    unless: ../y\n",
                "outside the project's folder",
            ),
            (
                "  - label: a\n    command: [x]\n  - label: a\n    command: [y]\n",
                "two steps labelled a",
            ),
        ] {
            let body = format!("{ONE}run:\n{run}");
            let error = error_of(&body, &["src/demo.h", "src/demo.c"]);
            assert!(error.contains(wanted), "{run}: {error}");
        }
    }

    #[test]
    fn a_run_fingerprint_is_the_one_lib_installer_kept() {
        let step = RunStep {
            label: "a".to_string(),
            description: String::new(),
            command: vec!["imgtool".to_string(), "keygen".to_string()],
            args: vec!["x".to_string()],
            unless: None,
            unless_in_library: false,
        };

        // hashlib.sha256("\0".join(["", "imgtool", "keygen", "\0", "x"]).encode()).hexdigest()[:16]
        assert_eq!(
            step.fingerprint(),
            short_hash("\0imgtool\0keygen\0\0\0x".as_bytes())
        );
    }

    #[test]
    fn peripherals_read_as_a_word_or_a_mapping() {
        let body = format!(
            "requires:\n  peripherals:\n    - spi\n    - type: I2C\n      count: 2\n      note: fast\n{ONE}"
        );
        let manifest = load_body(&body, &["src/demo.h", "src/demo.c"]).unwrap();

        assert_eq!(manifest.requires.peripherals[0].describe(), "SPI");
        assert_eq!(manifest.requires.peripherals[1].describe(), "I2C x2, fast");
    }

    #[test]
    fn a_count_of_nothing_is_refused_with_its_line() {
        let body = format!("requires:\n  peripherals:\n    - type: i2c\n      count: 0\n{ONE}");
        let error = load_body(&body, &[]).unwrap_err();

        assert!(
            error.message.contains("has a count of 0"),
            "{}",
            error.message
        );
        assert_eq!(error.line, Some(6));
    }

    #[test]
    fn an_rtos_provides_itself() {
        let body = format!("kind: rtos\n{ONE}");
        let manifest = load_body(&body, &["src/demo.h", "src/demo.c"]).unwrap();

        assert_eq!(manifest.provides, ["rtos"]);
    }

    #[test]
    fn what_is_odd_but_harmless_is_warned_about() {
        let body = format!(
            "kind: gadget\ncategory: fun\nsomething: else\n{ONE}options:\n  a:\n    sources: [o.c]\n"
        );
        let manifest = load_body(&body, &["src/demo.h", "src/demo.c", "o.c"]).unwrap();
        let said = manifest.warnings.join("\n");

        assert!(said.contains("kind 'gadget'"), "{said}");
        assert!(said.contains("category 'fun'"), "{said}");
        assert!(said.contains("something is no key"), "{said}");
        assert!(said.contains("option 'a' has no description"), "{said}");
    }

    #[test]
    fn only_what_a_compiler_takes_is_built() {
        let manifest = load_body(
            "sources:\n  - a.c\n  - b.CPP\n  - c.s\n  - readme.txt\n",
            &["a.c", "b.CPP", "c.s", "readme.txt"],
        )
        .unwrap();

        assert_eq!(manifest.build_sources(), ["a.c", "b.CPP", "c.s"]);
    }
}
