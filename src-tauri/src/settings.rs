//! What the window remembers, in `settings.json` in the config folder. A token is never among it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};

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
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
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
