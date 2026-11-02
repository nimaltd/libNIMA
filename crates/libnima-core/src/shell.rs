//! Putting the `libnima` command on the PATH, so it runs from the root of any project.
//!
//! On Windows the user's PATH is a value in the registry. On Linux and macOS it is a line in the shell's start file, so that is
//! what is added to, once, between markers that say who wrote it. Nothing here needs an administrator.

use std::env;
#[cfg(not(windows))]
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// The line's markers in a shell's start file.
const OPEN: &str = "# >>> libnima >>>";
const CLOSE: &str = "# <<< libnima <<<";

/// The command's name on this system.
pub fn command_name() -> &'static str {
    if cfg!(windows) {
        "libnima.exe"
    } else {
        "libnima"
    }
}

/// Whether the command is in one of these folders.
fn on(folders: impl Iterator<Item = PathBuf>) -> bool {
    folders
        .map(|folder| folder.join(command_name()))
        .any(|path| path.is_file())
}

/// Whether a terminal started now finds the command. An AppImage's own mount is on the PATH it runs with, and is gone when it
/// ends, so that does not count.
pub fn command_found() -> bool {
    let mount = env::var_os("APPDIR").map(PathBuf::from);
    let lasting = |folder: &PathBuf| {
        mount
            .as_ref()
            .is_none_or(|mount| !folder.starts_with(mount))
    };

    env::var_os("PATH").is_some_and(|path| on(env::split_paths(&path).filter(lasting)))
        || saved_path().is_some_and(|path| on(env::split_paths(&path)))
}

/// Two folders of a list being the same, by the rule the system reads it: on Windows neither case nor a slash at the end tells
/// them apart.
fn same_folder(one: &str, other: &str) -> bool {
    let one = one.trim().trim_end_matches(['/', '\\']);
    let other = other.trim().trim_end_matches(['/', '\\']);

    if cfg!(windows) {
        one.eq_ignore_ascii_case(other)
    } else {
        one == other
    }
}

/// A list of folders with one more on it, unless it is there already.
pub fn with_folder(list: &str, folder: &str, separator: char) -> Option<String> {
    if list.split(separator).any(|part| same_folder(part, folder)) {
        return None;
    }

    Some(if list.trim().is_empty() {
        folder.to_string()
    } else {
        format!("{}{separator}{folder}", list.trim_end_matches(separator))
    })
}

/// A list of folders with one taken off it, when it is there.
pub fn without_folder(list: &str, folder: &str, separator: char) -> Option<String> {
    let kept: Vec<&str> = list
        .split(separator)
        .filter(|part| !same_folder(part, folder))
        .collect();

    (kept.len() != list.split(separator).count()).then(|| kept.join(&separator.to_string()))
}

/// A shell's start file with the line that puts a folder on the PATH: one block, replaced when it is there, so running this
/// twice writes it once.
pub fn with_line(text: &str, folder: &str) -> String {
    let line = format!("export PATH=\"$PATH:{folder}\"");
    let block = format!("{OPEN}\n{line}\n{CLOSE}\n");

    if let (Some(start), Some(end)) = (text.find(OPEN), text.find(CLOSE))
        && start < end
    {
        let after = end + CLOSE.len();
        let after = after + usize::from(text[after..].starts_with('\n'));

        return format!("{}{block}{}", &text[..start], &text[after..]);
    }

    if text.is_empty() {
        block
    } else if text.ends_with('\n') {
        format!("{text}\n{block}")
    } else {
        format!("{text}\n\n{block}")
    }
}

/// A shell's start file with the block taken out, along with the empty line that was put before it.
pub fn without_line(text: &str) -> String {
    let (Some(start), Some(end)) = (text.find(OPEN), text.find(CLOSE)) else {
        return text.to_string();
    };

    if start >= end {
        return text.to_string();
    }

    let before = &text[..start];
    let before = before
        .strip_suffix('\n')
        .filter(|kept| kept.ends_with('\n'))
        .unwrap_or(before);
    let after = end + CLOSE.len();
    let after = after + usize::from(text[after..].starts_with('\n'));

    format!("{before}{}", &text[after..])
}

/// Copy the command out of a mounted AppImage into a folder that lasts, with the way back to the window beside it, for
/// `libnima --gui`: the AppImage is what runs it. Returns that folder.
#[cfg(unix)]
pub fn copy_out_of_appimage(command: &Path, appimage: &Path, folder: &Path) -> io::Result<()> {
    fs::create_dir_all(folder)?;
    fs::copy(command, folder.join("libnima"))?;

    let window = folder.join("libnima-app");
    let _ = fs::remove_file(&window);

    std::os::unix::fs::symlink(appimage, window)
}

/// The start files of the shells that are on this machine, the one for a login shell of each.
#[cfg(not(windows))]
fn start_files(home: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = [".zshrc", ".bashrc", ".profile"]
        .iter()
        .map(|name| home.join(name))
        .filter(|path| path.is_file())
        .collect();

    if found.is_empty() {
        let shell = env::var("SHELL").unwrap_or_default();

        found.push(home.join(if shell.ends_with("zsh") {
            ".zshrc"
        } else {
            ".profile"
        }));
    }

    found
}

/// The user's own PATH as it is saved, which a terminal started now reads and this process may not have yet.
#[cfg(windows)]
fn saved_path() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Environment")
        .ok()?
        .get_value("Path")
        .ok()
}

#[cfg(not(windows))]
fn saved_path() -> Option<String> {
    let home = env::var_os("HOME").map(PathBuf::from)?;
    let mut folders: Vec<String> = Vec::new();

    for path in start_files(&home) {
        let text = fs::read_to_string(path).unwrap_or_default();

        if let (Some(start), Some(end)) = (text.find(OPEN), text.find(CLOSE))
            && start < end
        {
            for line in text[start..end].lines() {
                if let Some(rest) = line.strip_prefix("export PATH=\"$PATH:") {
                    folders.push(rest.trim_end_matches('"').to_string());
                }
            }
        }
    }

    (!folders.is_empty()).then(|| folders.join(":"))
}

/// Add a folder to the user's PATH. Returns what was done, for saying.
#[cfg(windows)]
pub fn add_to_path(folder: &Path) -> io::Result<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    let shown = folder.to_string_lossy().into_owned();
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    let list: String = key.get_value("Path").unwrap_or_default();

    let Some(longer) = with_folder(&list, &shown, ';') else {
        return Ok(format!("{shown} is on your PATH already."));
    };

    save_path(&key, &longer)?;

    Ok(format!(
        "Added {shown} to your PATH. A terminal opened from now on finds libnima."
    ))
}

/// Take a folder off the user's PATH. Returns what was done, for saying.
#[cfg(windows)]
pub fn remove_from_path(folder: &Path) -> io::Result<String> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};

    let shown = folder.to_string_lossy().into_owned();
    let key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
    let list: String = key.get_value("Path").unwrap_or_default();

    let Some(shorter) = without_folder(&list, &shown, ';') else {
        return Ok(format!("{shown} is not on your PATH."));
    };

    save_path(&key, &shorter)?;

    Ok(format!("Took {shown} off your PATH."))
}

/// Write the user's PATH as the system keeps it, so a %VARIABLE% in it goes on being read as one, and say so to what is running.
#[cfg(windows)]
fn save_path(key: &winreg::RegKey, list: &str) -> io::Result<()> {
    let value = winreg::RegValue {
        bytes: list
            .encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect(),
        vtype: winreg::enums::RegType::REG_EXPAND_SZ,
    };

    key.set_raw_value("Path", &value)?;
    tell_windows();

    Ok(())
}

/// Tell what is running that the environment changed, so a terminal opened from Explorer sees it without signing out.
#[cfg(windows)]
fn tell_windows() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };

    let name: Vec<u16> = "Environment"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: the text lives through the call, and the answer is not written anywhere it is not asked for.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            name.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            1000,
            std::ptr::null_mut(),
        );
    }
}

/// Add a folder to the user's PATH. Returns what was done, for saying.
#[cfg(not(windows))]
pub fn add_to_path(folder: &Path) -> io::Result<String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("HOME is not set"))?;
    let shown = folder.to_string_lossy().into_owned();
    let mut written = Vec::new();

    for path in start_files(&home) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let updated = with_line(&text, &shown);

        if updated != text {
            fs::write(&path, updated)?;
            written.push(path.to_string_lossy().into_owned());
        }
    }

    Ok(if written.is_empty() {
        format!("{shown} is on your PATH already.")
    } else {
        format!(
            "Added {shown} to your PATH in {}. A terminal opened from now on finds libnima.",
            written.join(", ")
        )
    })
}

/// Take the command's folder off the user's PATH. Returns what was done, for saying.
#[cfg(not(windows))]
pub fn remove_from_path(_folder: &Path) -> io::Result<String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("HOME is not set"))?;
    let mut written = Vec::new();

    for path in start_files(&home) {
        let text = fs::read_to_string(&path).unwrap_or_default();
        let updated = without_line(&text);

        if updated != text {
            fs::write(&path, updated)?;
            written.push(path.to_string_lossy().into_owned());
        }
    }

    Ok(if written.is_empty() {
        "libnima was not added to your PATH here, so there is nothing to take off.".to_string()
    } else {
        format!("Took libnima off your PATH in {}.", written.join(", "))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn a_folder_goes_on_the_end_of_the_list_once() {
        assert_eq!(
            with_folder("C:/a;C:/b", "C:/c", ';').as_deref(),
            Some("C:/a;C:/b;C:/c")
        );
        assert_eq!(
            with_folder("C:/a;C:/b;", "C:/c", ';').as_deref(),
            Some("C:/a;C:/b;C:/c")
        );
        assert_eq!(with_folder("", "C:/c", ';').as_deref(), Some("C:/c"));
        assert_eq!(with_folder("C:/a;C:/c", "C:/c", ';'), None);
        assert_eq!(with_folder("C:/a;C:/c/", "C:/c", ';'), None);
    }

    #[cfg(windows)]
    #[test]
    fn case_does_not_tell_two_folders_apart_on_windows() {
        assert_eq!(
            with_folder(
                "C:\\Program Files\\libNIMA",
                "c:\\program files\\libnima\\",
                ';'
            ),
            None
        );
    }

    #[test]
    fn a_folder_comes_off_the_list_and_nothing_else_does() {
        assert_eq!(
            without_folder("C:/a;C:/c;C:/b", "C:/c", ';').as_deref(),
            Some("C:/a;C:/b")
        );
        assert_eq!(
            without_folder("C:/a;C:/c/", "C:/c", ';').as_deref(),
            Some("C:/a")
        );
        assert_eq!(without_folder("C:/c", "C:/c", ';').as_deref(), Some(""));
        assert_eq!(without_folder("C:/a;C:/b", "C:/c", ';'), None);
        assert_eq!(without_folder("", "C:/c", ';'), None);
        assert_eq!(
            without_folder("%A%;C:/c;;C:/b", "C:/c", ';').as_deref(),
            Some("%A%;;C:/b"),
            "what is not ours stays as it was"
        );
    }

    #[test]
    fn adding_a_folder_and_taking_it_off_leaves_the_list_as_it_was() {
        let list = "C:/a;%B%\\bin";
        let longer = with_folder(list, "C:/c", ';').unwrap();

        assert_eq!(without_folder(&longer, "C:/c", ';').as_deref(), Some(list));
    }

    #[test]
    fn the_block_comes_out_and_what_was_around_it_stays() {
        for text in ["alias ll='ls -l'\n", "", "a\n\nb\n\n"] {
            assert_eq!(without_line(&with_line(text, "/opt/libnima")), text);
        }

        assert_eq!(without_line("x\n"), "x\n", "there is no block to take out");
        assert_eq!(without_line(&with_line("x", "/opt/libnima")), "x\n");

        let more = format!("{}alias y=z\n", with_line("x\n", "/opt/libnima"));

        assert_eq!(without_line(&more), "x\nalias y=z\n");
    }

    #[cfg(unix)]
    #[test]
    fn the_command_is_copied_out_of_an_appimage_with_the_way_back_beside_it() {
        let root = tempfile::tempdir().unwrap();
        let mount = root.path().join("mount");
        let appimage = root.path().join("libnima.AppImage");
        let folder = root.path().join("home/.local/bin");

        fs::create_dir_all(&mount).unwrap();
        fs::write(mount.join("libnima"), "the command").unwrap();
        fs::write(&appimage, "the window").unwrap();

        copy_out_of_appimage(&mount.join("libnima"), &appimage, &folder).unwrap();
        copy_out_of_appimage(&mount.join("libnima"), &appimage, &folder).unwrap();

        assert_eq!(
            fs::read_to_string(folder.join("libnima")).unwrap(),
            "the command"
        );
        assert_eq!(
            fs::read_to_string(folder.join("libnima-app")).unwrap(),
            "the window"
        );
    }

    #[test]
    fn the_line_goes_at_the_end_in_a_block_of_its_own() {
        assert_eq!(
            with_line("alias ll='ls -l'\n", "/opt/libnima"),
            "alias ll='ls -l'\n\n# >>> libnima >>>\nexport PATH=\"$PATH:/opt/libnima\"\n# <<< libnima <<<\n"
        );
        assert_eq!(
            with_line("", "/opt/libnima"),
            "# >>> libnima >>>\nexport PATH=\"$PATH:/opt/libnima\"\n# <<< libnima <<<\n"
        );
        assert_eq!(
            with_line("x", "/opt/libnima"),
            "x\n\n# >>> libnima >>>\nexport PATH=\"$PATH:/opt/libnima\"\n# <<< libnima <<<\n"
        );
    }

    #[test]
    fn adding_it_twice_writes_it_once_and_a_new_place_replaces_the_old() {
        let once = with_line("x\n", "/opt/libnima");

        assert_eq!(with_line(&once, "/opt/libnima"), once);

        let moved = with_line(&once, "/usr/local/libnima");

        assert_eq!(moved.matches(OPEN).count(), 1, "{moved}");
        assert!(moved.contains("/usr/local/libnima"), "{moved}");
        assert!(!moved.contains("/opt/libnima"), "{moved}");
    }

    #[test]
    fn what_the_user_wrote_after_it_stays() {
        let once = with_line("x\n", "/opt/libnima");
        let more = format!("{once}alias y=z\n");

        assert!(with_line(&more, "/opt/libnima").ends_with("# <<< libnima <<<\nalias y=z\n"));
    }

    #[test]
    fn the_command_is_found_in_a_folder_that_holds_it() {
        let root = tempfile::tempdir().unwrap();

        assert!(!on(std::iter::once(root.path().to_path_buf())));

        fs::write(root.path().join(command_name()), "").unwrap();

        assert!(on(std::iter::once(root.path().to_path_buf())));
    }
}
