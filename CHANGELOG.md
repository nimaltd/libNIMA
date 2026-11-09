# Changelog

## 1.2.0

- Two text sizes, as in terNIMA and bleNIMA. The menu text size is for the menus, tabs, buttons and dialogs, which grow with it; it starts at 16 pixels, a quarter bigger than the 13 they were, and 13 in Settings gives the old size. The text size is for the libnima.yml editor and the run's report, and starts at 13 as before. Both are in Settings and in the View menu: Bigger text, Smaller text and Normal text size (Ctrl+=, Ctrl+-, Ctrl+0), and Bigger menu text, Smaller menu text and Normal menu text size.
- The menus follow terNIMA and bleNIMA: Settings... is in the File menu with Ctrl+, (Cmd+, on macOS), no longer in Tools; the View menu starts with the command palette and ends with the text sizes and the theme (System theme, Dark theme, Light theme); on macOS File ends with Quit libNIMA, Cmd+Q.

## 1.1.0

- A git repository inside the project is no longer installed where it stands, which removed its `.git` and with it its history: the run stops and says to install from a copy outside the project. A folder installed in place keeps its `libnima.yml`.
- A file of yours where a library's file would go in its folder is never written over: the install stops before writing anything and names it. An uninstall takes only what the record has as the library's.
- An extra changed by hand, the library's `README.md` or a tool it put in `tools/`, is saved as a `.bak` before an install replaces it, or an update or an uninstall removes it, as a source changed outside its `USER CODE` sections is.
- A run step that ran before is not run again by an update when its `unless` file is gone, with `--yes` or without, so a fresh clone without its key does not get a new one. `--run` still runs it.
- `--dir`, or the answer to the folder question, can no longer be another library's folder, or one inside or around it, whose module and files the new library wrote over.
- A library already installed is no longer installed into another folder as well, which left the first copy behind and out of the record: the run stops, names the folder it is in, and says how to move it.
- Every library of a run is checked against the project, and against the others of the run, before the first is written, so a run that is refused writes nothing.
- A `once` file can no longer land in another library's folder or on another library's file, and no install replaces a file made as the user's, nor does an uninstall remove one. `--lint` refuses a `once` file in the folder of the library itself or of one it names.
- `--dir` has to name a folder inside the project, however it is written, and not the project's own folder.
- A `--project` that does not exist is an error, and nothing is made. A folder that is no Zephyr application, ESP-IDF project or PlatformIO project stops the install before anything is written, and a library that went into no build is no longer said to be done.

## 1.0.0

- libNIMA replaces lib-installer: the same installer rewritten in Rust, with a window beside the command line.
- The window: open a project, see which platform it is and which file told, add libraries by repository, zip or folder, see each marked OK, CHECK or NO with the way to put it right, preview what an install would change on a copy of the project, install, update and uninstall. What a run does is shown library by library, each in a part of its own.
- An editor for a library's `libnima.yml`, as a form or as text, which are one file. New fills one in from what is in the folder. The text offers what can go at the cursor and says what a key is, problems come with a fix a click makes, and a summary says what the file comes to. It also converts an `installer.yml`.
- `docs/libnima.schema.json`, which gives other editors the same help on the file.
- `libnima --check`, `--lint`, `--convert`, `--schema` and `--gui`, and `--add-to-path` for the command alone.
- A project moves over from lib-installer, or from one of the five installers before it, on the first run. Its blocks and record keep their contents, and each file is saved first.
