//! Putting the `libnima` command on the PATH, so it runs from the root of any project.
//!
//! The installers do this where they can. On macOS, and for an AppImage, nothing can, so the window offers it from the Tools menu.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

use libnima_core::shell;
use serde::Serialize;

/// Where the command stands.
#[derive(Serialize)]
pub struct PathState {
    /// A terminal finds it.
    pub found: bool,
    /// The folder it is in, beside the window's program. Empty when this install has no command beside it.
    pub folder: String,
}

/// The folder of the window's program, when the command stands beside it.
fn beside() -> Option<PathBuf> {
    let folder = std::env::current_exe().ok()?.parent()?.to_path_buf();

    folder
        .join(shell::command_name())
        .is_file()
        .then_some(folder)
}

/// The folder to put on the PATH, once the command is somewhere it will stay.
fn lasting_folder(beside: PathBuf) -> Result<PathBuf, String> {
    // A macOS app run from its disk image, or from a folder macOS has moved it to for now, is gone once it ends.
    if cfg!(target_os = "macos") {
        let shown = beside.to_string_lossy();

        if shown.starts_with("/Volumes/") || shown.contains("/AppTranslocation/") {
            return Err(
                "Move libNIMA to Applications first, so the command stays where this puts it."
                    .to_string(),
            );
        }
    }

    // An AppImage is mounted only while it runs, so the command is copied out of it.
    #[cfg(unix)]
    if let Some(appimage) = std::env::var_os("APPIMAGE") {
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let folder = PathBuf::from(home).join(".local").join("bin");

        shell::copy_out_of_appimage(
            &beside.join(shell::command_name()),
            Path::new(&appimage),
            &folder,
        )
        .map_err(|error| {
            format!(
                "Could not copy the command to {}: {error}",
                folder.display()
            )
        })?;

        return Ok(folder);
    }

    Ok(beside)
}

#[tauri::command]
pub fn path_state() -> PathState {
    let folder = beside();

    PathState {
        found: shell::command_found(),
        folder: folder.map_or_else(String::new, |folder| folder.to_string_lossy().into_owned()),
    }
}

/// Adds the command's folder to the user's PATH. Returns what was done, for saying.
#[tauri::command]
pub fn add_to_path() -> Result<String, String> {
    let Some(folder) = beside() else {
        return Err(
            "The command is not beside this program, so there is nothing to add.".to_string(),
        );
    };

    shell::add_to_path(&lasting_folder(folder)?).map_err(|error| error.to_string())
}
