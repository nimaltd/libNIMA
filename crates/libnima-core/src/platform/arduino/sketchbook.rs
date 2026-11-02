//! Where Arduino keeps its libraries, and whether a folder is a sketch.
//!
//! The sketchbook is the folder the Arduino IDE and arduino-cli keep sketches in, and its libraries
//! folder is where Library Manager installs to and where both look for a library a sketch includes.
//! Where it is depends on the machine, and on what the user set, so it is read the way Arduino reads
//! it, first answer wins:
//!
//! ```text
//! --sketchbook                    given on the command line
//! ARDUINO_DIRECTORIES_USER        the variable arduino-cli reads
//! ~/.arduinoIDE/arduino-cli.yaml  Arduino IDE 2's settings, directories.user
//! Arduino15/arduino-cli.yaml      arduino-cli's own settings, the same key
//! Arduino15/preferences.txt       Arduino IDE 1's settings, sketchbook.path
//! the default                     Documents/Arduino on Windows and macOS, ~/Arduino on Linux
//! ```

use std::env;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use crate::files::text_or_empty;
use crate::paths;

/// The two keys that hold the sketchbook, in Arduino IDE 2 and arduino-cli's YAML, and in Arduino IDE
/// 1's preferences.txt.
static DIRECTORIES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^directories:[ \t]*\r?$").expect("a valid pattern"));
static USER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^([ \t]+)user:[ \t]*(.*?)[ \t]*\r?$").expect("a valid pattern")
});
static PREFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^sketchbook\.path=(.*?)[ \t]*\r?$").expect("a valid pattern")
});
static TOP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\S").expect("a valid pattern"));

/// The user's home folder.
fn home() -> PathBuf {
    env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// A path as a shell reads a leading ~.
fn expand_user(path: &str) -> PathBuf {
    match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => home().join(rest),
        None if path == "~" => home(),
        None => PathBuf::from(path),
    }
}

/// The folder Arduino keeps its own data and settings in.
fn arduino15() -> PathBuf {
    if cfg!(windows) {
        env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Local"))
            .join("Arduino15")
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Arduino15")
    } else {
        home().join(".arduino15")
    }
}

/// The user's Documents folder. On Windows it may have been moved, to OneDrive for one, which the
/// registry says.
fn documents() -> PathBuf {
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::HKEY_CURRENT_USER;

        if let Ok(key) = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\User Shell Folders")
            && let Ok(value) = key.get_value::<String, _>("Personal")
            && !value.is_empty()
        {
            return PathBuf::from(expand_variables(&value));
        }
    }

    home().join("Documents")
}

/// %NAME% in a value of the registry's, replaced by the variable.
#[cfg(windows)]
fn expand_variables(value: &str) -> String {
    let mut out = String::new();
    let mut rest = value;

    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];

        match after.find('%') {
            Some(end) => {
                match env::var(&after[..end]) {
                    Ok(found) => out.push_str(&found),
                    Err(_) => {
                        out.push('%');
                        out.push_str(&after[..end]);
                        out.push('%');
                    }
                }

                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }

    out.push_str(rest);

    out
}

/// Where Arduino puts the sketchbook when nothing says otherwise.
fn default_sketchbook() -> PathBuf {
    if cfg!(target_os = "linux") {
        home().join("Arduino")
    } else {
        documents().join("Arduino")
    }
}

/// A YAML scalar without its quotes.
fn unquote(value: &str) -> &str {
    let value = value.trim();
    let bytes = value.as_bytes();

    if bytes.len() >= 2
        && bytes[0] == bytes[bytes.len() - 1]
        && (bytes[0] == b'\'' || bytes[0] == b'"')
    {
        return &value[1..value.len() - 1];
    }

    value
}

/// directories.user from the text of an arduino-cli.yaml, or None.
pub fn user_directory(text: &str) -> Option<String> {
    let found = DIRECTORIES.find(text)?;

    // The keys indented under directories:, up to the next line at the top.
    let block = &text[found.end()..];
    let block = TOP.find(block).map_or(block, |top| &block[..top.start()]);
    let user = USER.captures(block)?;
    let value = unquote(&user[2]);

    (!value.is_empty()).then(|| value.to_string())
}

/// What says where the sketchbook is on a machine.
struct Places {
    variable: Option<String>,
    home: PathBuf,
    arduino15: PathBuf,
    default: PathBuf,
}

/// The sketchbook folder, and where that answer came from.
pub fn sketchbook(given: Option<&str>) -> (PathBuf, String) {
    sketchbook_in(
        given,
        &Places {
            variable: env::var("ARDUINO_DIRECTORIES_USER").ok(),
            home: home(),
            arduino15: arduino15(),
            default: default_sketchbook(),
        },
    )
}

fn sketchbook_in(given: Option<&str>, places: &Places) -> (PathBuf, String) {
    if let Some(given) = given.filter(|given| !given.is_empty()) {
        return (
            paths::resolve(&expand_user(given)),
            "--sketchbook".to_string(),
        );
    }

    if let Some(variable) = places.variable.as_deref().filter(|found| !found.is_empty()) {
        return (
            paths::resolve(&expand_user(variable)),
            "ARDUINO_DIRECTORIES_USER".to_string(),
        );
    }

    for (config, how) in [
        (
            places.home.join(".arduinoIDE").join("arduino-cli.yaml"),
            "Arduino IDE's settings",
        ),
        (
            places.arduino15.join("arduino-cli.yaml"),
            "arduino-cli's settings",
        ),
    ] {
        if let Some(found) = user_directory(&text_or_empty(&config)) {
            return (paths::resolve(&expand_user(&found)), how.to_string());
        }
    }

    let preferences = text_or_empty(&places.arduino15.join("preferences.txt"));

    if let Some(found) = PREFERENCE.captures(&preferences)
        && !found[1].trim().is_empty()
    {
        return (
            paths::resolve(&expand_user(found[1].trim())),
            "Arduino IDE 1's preferences".to_string(),
        );
    }

    (paths::resolve(&places.default), "the default".to_string())
}

/// Whether a folder is a sketch: one with a .ino named after it, as Arduino needs.
pub fn is_sketch(folder: &Path) -> bool {
    let Some(name) = folder
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
    else {
        return false;
    };

    folder.join(format!("{name}.ino")).is_file() || folder.join(format!("{name}.pde")).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sketchbook_is_read_from_arduino_clis_settings() {
        let text = "board_manager:\n  additional_urls: []\ndirectories:\n  data: /x\n  user: \"/home/me/Sketches\"\nother: 1\n";

        assert_eq!(user_directory(text).as_deref(), Some("/home/me/Sketches"));
        assert_eq!(
            user_directory("directories:\n  data: /x\nuser: /no\n"),
            None
        );
        assert_eq!(user_directory("nothing: here\n"), None);
    }

    #[test]
    fn a_sketch_has_a_ino_named_after_it() {
        let root = tempfile::tempdir().unwrap();
        let sketch = root.path().join("Blink");
        std::fs::create_dir_all(&sketch).unwrap();

        assert!(!is_sketch(&sketch));

        std::fs::write(sketch.join("Blink.ino"), "").unwrap();

        assert!(is_sketch(&sketch));
    }

    #[test]
    fn a_folder_given_comes_first() {
        let root = tempfile::tempdir().unwrap();
        let (found, how) = sketchbook(Some(&root.path().to_string_lossy()));

        assert_eq!(found, paths::resolve(root.path()));
        assert_eq!(how, "--sketchbook");
    }

    /// A machine of the test's own, with nothing set, as a fresh one has.
    fn fresh(root: &Path) -> Places {
        Places {
            variable: None,
            home: root.join("home"),
            arduino15: root.join("home/Arduino15"),
            default: root.join("home/Documents/Arduino"),
        }
    }

    fn put(path: PathBuf, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn at(root: &Path, name: &str) -> String {
        paths::posix(&root.join(name))
    }

    #[test]
    fn the_variable_arduino_cli_reads_comes_next() {
        let root = tempfile::tempdir().unwrap();
        let mut places = fresh(root.path());

        put(
            places.home.join(".arduinoIDE/arduino-cli.yaml"),
            "directories:\n  user: /elsewhere\n",
        );
        places.variable = Some(at(root.path(), "from_env"));

        assert_eq!(
            sketchbook_in(None, &places),
            (
                paths::resolve(&root.path().join("from_env")),
                "ARDUINO_DIRECTORIES_USER".to_string()
            )
        );
    }

    #[test]
    fn a_variable_set_to_nothing_says_nothing() {
        let root = tempfile::tempdir().unwrap();
        let mut places = fresh(root.path());

        places.variable = Some(String::new());

        assert_eq!(sketchbook_in(None, &places).1, "the default");
    }

    #[test]
    fn arduino_ide_2_settings_come_before_arduino_cli() {
        let root = tempfile::tempdir().unwrap();
        let places = fresh(root.path());
        let ide = at(root.path(), "ide");
        let cli = at(root.path(), "cli");

        put(
            places.home.join(".arduinoIDE/arduino-cli.yaml"),
            &format!(
                "board_manager:\n  additional_urls: []\ndirectories:\n  data: /x\n  user: '{ide}'\nlogging:\n  level: info\n"
            ),
        );
        put(
            places.arduino15.join("arduino-cli.yaml"),
            &format!("directories:\n  user: {cli}\n"),
        );

        assert_eq!(
            sketchbook_in(None, &places),
            (
                paths::resolve(&root.path().join("ide")),
                "Arduino IDE's settings".to_string()
            )
        );
    }

    #[test]
    fn arduino_cli_settings_then_ide_1_preferences() {
        let root = tempfile::tempdir().unwrap();
        let places = fresh(root.path());
        let ide1 = at(root.path(), "ide1");
        let cli = at(root.path(), "cli");

        put(
            places.arduino15.join("preferences.txt"),
            &format!("sketchbook.path={ide1}\r\n"),
        );

        assert_eq!(
            sketchbook_in(None, &places),
            (
                paths::resolve(&root.path().join("ide1")),
                "Arduino IDE 1's preferences".to_string()
            )
        );

        put(
            places.arduino15.join("arduino-cli.yaml"),
            &format!("directories:\r\n  user: \"{cli}\"\r\n"),
        );

        assert_eq!(
            sketchbook_in(None, &places),
            (
                paths::resolve(&root.path().join("cli")),
                "arduino-cli's settings".to_string()
            )
        );
    }

    #[test]
    fn a_user_key_outside_directories_is_not_the_sketchbook() {
        let root = tempfile::tempdir().unwrap();
        let places = fresh(root.path());

        put(
            places.home.join(".arduinoIDE/arduino-cli.yaml"),
            "directories:\n  data: /x\nnetwork:\n  user: /not/this\n",
        );

        assert_eq!(sketchbook_in(None, &places).1, "the default");
    }

    #[test]
    fn with_nothing_set_it_is_the_default() {
        let root = tempfile::tempdir().unwrap();
        let places = fresh(root.path());

        assert_eq!(
            sketchbook_in(None, &places),
            (paths::resolve(&places.default), "the default".to_string())
        );
    }

    #[test]
    fn a_ino_of_another_name_makes_no_sketch() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("blink");

        put(folder.join("other.ino"), "");

        assert!(!is_sketch(&folder));

        put(folder.join("blink.ino"), "");

        assert!(is_sketch(&folder));
    }

    #[test]
    fn arduino_keeps_its_settings_where_this_system_wants() {
        let found = paths::posix(&arduino15());

        if cfg!(target_os = "linux") {
            assert!(found.ends_with("/.arduino15"), "{found}");
        } else {
            assert!(found.ends_with("Arduino15"), "{found}");
        }
    }
}
