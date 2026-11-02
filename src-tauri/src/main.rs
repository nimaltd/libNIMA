//! The libNIMA window, and the commands the UI calls in Rust.
//!
//! The real work lives in `libnima-core`. This file only connects it to the UI.

// No console window next to the app in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod path;
mod run;
mod settings;
mod window;

use std::path::{Path, PathBuf};

use libnima_core::download::Client;
use libnima_core::flow::Request;
use libnima_core::look::{self, Looked, Preview, Project};
use libnima_core::outline::{self, Advice, Outline};
use libnima_core::paths;
use libnima_core::platform::{Platform, PlatformArgs};
use libnima_core::scaffold;
use libnima_core::schema::{self, Completions, Group, Help, Key};
use libnima_core::ymledit::{self, Step};
use serde::{Deserialize, Serialize};
use tauri::{Manager, State, WindowEvent};

use settings::{Saved, Settings};

/// What the status bar shows about the app and the computer it runs on.
#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    os: &'static str,
    arch: &'static str,
}

/// The folder the window was opened on, as `libnima --gui` does.
struct StartFolder(Option<String>);

/// What the window asks of the engine: the same as the command's options.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Asked {
    project: String,
    libraries: Vec<String>,
    folders: Vec<(String, String)>,
    with_options: Vec<String>,
    without_options: Vec<String>,
    run_on: Vec<String>,
    run_off: Vec<String>,
    uninstall: Vec<String>,
    platform: Option<Platform>,
    ide: Option<String>,
    conf: Option<String>,
    into: Option<String>,
    sketchbook: Option<String>,
    /// A token for this run only: never saved, and sent to GitHub and nowhere else.
    key: Option<String>,
}

impl Asked {
    fn args(&self) -> PlatformArgs {
        PlatformArgs {
            ide: self.ide.clone(),
            conf: self.conf.clone(),
            into: self.into.clone(),
            sketchbook: self.sketchbook.clone(),
            uninstalling: Vec::new(),
        }
    }

    fn request(self) -> Request {
        let args = self.args();

        Request {
            libraries: self.libraries,
            folders: self.folders,
            with_options: self.with_options,
            without_options: self.without_options,
            project: Some(PathBuf::from(self.project)),
            run_on: self.run_on,
            run_off: self.run_off,
            key: self.key,
            uninstall: self.uninstall,
            platform: self.platform,
            args,
            ..Request::default()
        }
    }
}

/// Run what takes a while away from the window, so it stays awake.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo {
        version: libnima_core::VERSION,
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

#[tauri::command]
fn start_folder(folder: State<StartFolder>) -> Option<String> {
    folder.0.clone()
}

#[tauri::command]
async fn open_project(folder: String, sketchbook: Option<String>) -> Result<Project, String> {
    if !Path::new(&folder).is_dir() {
        return Err(format!("There is no folder at {folder}."));
    }

    blocking(move || look::project(Path::new(&folder), sketchbook.as_deref())).await
}

#[tauri::command]
async fn look_at_library(asked: Asked, given: String) -> Result<Looked, String> {
    blocking(move || {
        look::look_at(
            Client::new(asked.key.as_deref()),
            Path::new(&asked.project),
            asked.platform,
            &asked.args(),
            &given,
        )
    })
    .await
}

#[tauri::command]
async fn preview_install(asked: Asked) -> Result<Preview, String> {
    blocking(move || {
        let client = Client::new(asked.key.as_deref());

        look::preview(client, &asked.request())
    })
    .await
}

/// Each library GitHub has a newer version of, and that version. Nothing to say when it cannot be reached.
#[tauri::command]
async fn newer_libraries(
    folder: String,
    sketchbook: Option<String>,
    key: Option<String>,
) -> Result<Vec<(String, String)>, String> {
    blocking(move || {
        let project = look::project(Path::new(&folder), sketchbook.as_deref());
        let mut client = Client::new(key.as_deref());

        look::newer_versions(&mut client, &project.installed)
    })
    .await
}

/// A library's file for the editor.
#[derive(Serialize)]
struct LibraryFile {
    /// Where saving it writes.
    path: String,
    text: String,
    /// It came from an installer.yml, and has not been saved as libnima.yml yet.
    converted: bool,
}

#[tauri::command]
fn read_library_file(folder: String) -> Result<LibraryFile, String> {
    let (path, text, converted) = look::file_for_editing(Path::new(&folder))?;

    Ok(LibraryFile {
        path: paths::posix(&path),
        text,
        converted,
    })
}

/// The folder a library's file is in.
fn folder_of(path: &str) -> PathBuf {
    Path::new(path)
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

/// What is wrong with the text in the editor, read against the library's own folder, with what can be done about it.
#[tauri::command]
fn lint_library_file(path: String, text: String, platform: Option<Platform>) -> Vec<Advice> {
    outline::advise(&folder_of(&path), Path::new(&path), &text, platform)
}

/// What the text in the editor comes to.
#[tauri::command]
fn outline_library_file(path: String, text: String) -> Outline {
    outline::outline(&folder_of(&path), Path::new(&path), &text)
}

/// The engine's table of what a file can hold, for the form.
#[derive(Serialize)]
struct Table {
    keys: &'static [Key],
    groups: &'static [Group],
}

#[tauri::command]
fn library_file_table() -> Table {
    Table {
        keys: schema::KEYS,
        groups: &schema::GROUPS,
    }
}

/// The text with one value changed, or taken out when there is none.
#[tauri::command]
fn edit_library_file(
    text: String,
    at: Vec<Step>,
    value: Option<serde_json::Value>,
) -> Result<String, String> {
    ymledit::set(&text, &at, value.as_ref())
}

/// The text with a key given another name where it stands.
#[tauri::command]
fn rename_in_library_file(text: String, at: Vec<Step>, name: String) -> Result<String, String> {
    ymledit::rename(&text, &at, &name)
}

/// What can go at a place in the text.
#[tauri::command]
fn complete_library_file(path: String, text: String, line: usize, column: usize) -> Completions {
    let files = scaffold::files_of(&folder_of(&path));

    schema::complete(&text, line, column, &files)
}

/// What the key at a place in the text is.
#[tauri::command]
fn help_in_library_file(text: String, line: usize, column: usize) -> Option<Help> {
    schema::help_at(&text, line, column)
}

#[tauri::command]
fn save_library_file(path: String, text: String) -> Result<(), String> {
    std::fs::write(&path, text).map_err(|error| format!("Could not save {path}: {error}"))
}

/// A new file for the library in a folder, from what is in it. The platform is the one for a folder that does not tell.
#[tauri::command]
fn new_library_file(folder: String, platform: Option<Platform>) -> LibraryFile {
    let folder = PathBuf::from(folder);
    let found = scaffold::scan(&folder, platform);

    LibraryFile {
        path: scaffold::path_for(&folder),
        text: scaffold::first_file(&found, libnima_core::VERSION),
        converted: false,
    }
}

#[tauri::command]
fn get_settings(file: State<Saved>) -> Settings {
    file.get()
}

/// Saves new settings at once.
#[tauri::command]
fn set_settings(settings: Settings, file: State<Saved>) -> Result<(), String> {
    file.change(|current| {
        // The window's place and size come from the window, not from the UI.
        *current = Settings {
            window: current.window,
            ..settings
        };
    })
}

fn main() {
    // A folder on the command line, from `libnima --gui`, or from a file manager.
    let start = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .map(|path| paths::resolve(&path).to_string_lossy().into_owned());

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let path = app.path().app_config_dir()?.join("settings.json");
            let settings = Settings::load(&path);
            let before = settings.window;

            app.manage(Saved::new(path, settings));
            app.manage(StartFolder(start.clone()));
            app.manage(run::Running::default());
            app.manage(window::Unsaved::default());

            if let Some(main) = app.get_webview_window("main") {
                window::restore(&main, before);
            }

            window::keep_saving(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Moved(_) | WindowEvent::Resized(_) => window::changed(window),
            WindowEvent::CloseRequested { .. } => window::keep(window),
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            start_folder,
            open_project,
            look_at_library,
            preview_install,
            newer_libraries,
            read_library_file,
            lint_library_file,
            outline_library_file,
            library_file_table,
            edit_library_file,
            rename_in_library_file,
            complete_library_file,
            help_in_library_file,
            save_library_file,
            new_library_file,
            get_settings,
            set_settings,
            run::start_run,
            run::answer_run,
            path::path_state,
            path::add_to_path,
        ])
        .run(tauri::generate_context!())
        .expect("libNIMA could not start");
}
