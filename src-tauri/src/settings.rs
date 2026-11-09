//! What the window remembers, in `settings.json` in the config folder. A token is never among it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

/// Smallest text size of the menus, tabs, buttons and dialogs, in pixels.
pub const MENU_FONT_SIZE_MIN: u8 = 10;
/// Largest text size of the menus, tabs, buttons and dialogs, in pixels.
pub const MENU_FONT_SIZE_MAX: u8 = 26;
/// Text size of the menus, tabs, buttons and dialogs for a new install, in pixels: a quarter bigger than the 13 they were
/// before.
pub const MENU_FONT_SIZE_DEFAULT: u8 = 16;

/// Smallest text size of the editor and the run's report, in pixels.
pub const FONT_SIZE_MIN: u8 = 9;
/// Largest text size of the editor and the run's report, in pixels.
pub const FONT_SIZE_MAX: u8 = 24;
/// Text size of the editor and the run's report for a new install, in pixels.
pub const FONT_SIZE_DEFAULT: u8 = 13;

/// The window's place and size.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// Looking on GitHub for a newer libNIMA.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Updates {
    /// Looks when the window opens.
    pub check: bool,
    /// A version not to tell about again; empty for none.
    pub skipped: String,
}

impl Default for Updates {
    fn default() -> Updates {
        Updates {
            check: true,
            skipped: String::new(),
        }
    }
}

/// The same fields as `Settings` in the UI's settings.svelte.ts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// system, dark or light.
    pub theme: String,
    /// The text size of the menus, tabs, buttons and dialogs, which grow with it.
    pub menu_font_size: u8,
    /// The text size of the editor and the run's report.
    pub font_size: u8,
    pub updates: Updates,
    /// Asks GitHub for newer versions of a project's libraries when it opens.
    pub newer_libraries: bool,
    /// The projects opened lately, the last one first.
    pub recent: Vec<String>,
    pub window: Option<WindowState>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            theme: "system".to_string(),
            menu_font_size: MENU_FONT_SIZE_DEFAULT,
            font_size: FONT_SIZE_DEFAULT,
            updates: Updates::default(),
            newer_libraries: true,
            recent: Vec::new(),
            window: None,
        }
    }
}

impl Settings {
    /// What the file says, or the defaults when it cannot be read.
    pub fn load(path: &Path) -> Settings {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
            .unwrap_or_default()
            .checked()
    }

    /// The same settings with every value moved into its allowed range.
    pub fn checked(mut self) -> Settings {
        self.menu_font_size = self
            .menu_font_size
            .clamp(MENU_FONT_SIZE_MIN, MENU_FONT_SIZE_MAX);
        self.font_size = self.font_size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self
    }

    fn save(&self, path: &Path) -> Result<(), String> {
        let problem =
            |error: &dyn std::fmt::Display| format!("Could not save {}: {error}", path.display());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| problem(&error))?;
        }

        let text = serde_json::to_string_pretty(self).map_err(|error| problem(&error))?;

        fs::write(path, text).map_err(|error| problem(&error))
    }
}

/// The settings in memory and in their file.
pub struct Saved {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl Saved {
    pub fn new(path: PathBuf, settings: Settings) -> Saved {
        Saved {
            path,
            current: Mutex::new(settings),
        }
    }

    pub fn get(&self) -> Settings {
        self.current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Changes them and saves them, under the lock so two saves never cross.
    pub fn change(&self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);

        change(&mut current);
        current.save(&self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_from_before_the_text_sizes_gets_the_defaults() {
        let settings: Settings = serde_json::from_str(r#"{ "theme": "dark" }"#).unwrap();
        assert_eq!(settings.theme, "dark");
        assert_eq!(settings.menu_font_size, MENU_FONT_SIZE_DEFAULT);
        assert_eq!(settings.font_size, FONT_SIZE_DEFAULT);
    }

    #[test]
    fn text_sizes_are_kept_in_range() {
        let small = Settings {
            menu_font_size: 2,
            font_size: 2,
            ..Settings::default()
        }
        .checked();
        let large = Settings {
            menu_font_size: 200,
            font_size: 200,
            ..Settings::default()
        }
        .checked();
        assert_eq!(
            (small.menu_font_size, small.font_size),
            (MENU_FONT_SIZE_MIN, FONT_SIZE_MIN)
        );
        assert_eq!(
            (large.menu_font_size, large.font_size),
            (MENU_FONT_SIZE_MAX, FONT_SIZE_MAX)
        );
    }
}
