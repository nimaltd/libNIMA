# Changelog

## Unreleased

- A Zephyr library can include its devicetree in the application's overlay: `zephyr.project.overlay` lists the files, such as a `.dtsi` that a `once` file puts in `boards/`, and libNIMA writes a block of `#include` lines, marked with `//`, into the overlay, each line naming its file from the overlay's folder. A file alone goes into the application's one overlay, into `app.overlay` when it has none, or into those picked when it has several, asked once for every library of a run; a `from` and a `to` name the overlay in full, or every one a wildcard matches. `--overlay`, and the window's This project dialog, say it in place of what the libraries say. A file the overlay includes already, or one that is not there, is written commented out, with why. An update keeps the lines where they are, and an uninstall takes them out. An option brings its own. A library using it asks for `requires.libnima: 1.3.0`.
- A question can offer a list to pick one or several from: on the command line by a number, several as 1,3, a name, a part of one, a wildcard, or * for all, and in the window with a search and a box to tick for each.
- A library's blocks in `CMakeLists.txt` stay where they are: the one adding its Kconfig fragment to `EXTRA_CONF_FILE`, and its groups of `zephyr.project.cmake` and `esp32.project.cmake`, were taken out on every install and put back right above `find_package(Zephyr)` or `include(project.cmake)`, or at the end, below every other library's. The order followed the last install, and with it which fragment's line won, and an install that changed nothing still wrote the file and left a `.bak`. A block on its side of the line is now written again in its place, one you moved stays there, and one on the wrong side is still moved. An ESP-IDF group of `after-project` goes below `project()`, where the project's targets exist.
- A requirement written `owner/repo`, `owner/repo@ref`, or as a `source` with no `name`, is met by a library in the project, or on the command line, that comes from that repository, whatever it is called: by the `repository` of its `libnima.yml`, or where it was fetched from. It is no longer fetched again over it, nor put back to an older version, and the record's `needs` names the library as it is installed, so an uninstall keeps it while another needs it, in a project 1.0.0 recorded too.
- `--check` works out `requires.libraries` as the install does, fetching what the install would, and says NO, exit 1, where the install would be refused: a library that cannot be had, one too old, one given twice, or one an installed library needs a newer version of.
- A required library that is installed but too old is named with its version, `core 0.1.0 is the one in this project`, and is no longer said to be missing.
- `--run`, `--no-run`, `--with` and `--without` reach a library that comes in as a requirement; a name no library of the run has is still refused, by `--check` as well.
- A requirement is said by its name and minimum, `gcore >= 0.2.0`: a ref is no version, and is said only where it is fetched from. The advice for a machine without the internet names the zips as GitHub's Download ZIP names them, after the repositories.
- An install or update that would close a circle with libraries already in the project is refused before anything is written, and libraries that need each other, named together after `--uninstall`, go out together.
- One library given twice in one run is refused, by `--check` as well, rather than the first taken without a word.
- An uninstall offers to take out a library only when it came in for the one taken out, never one installed on its own, nor one the same run takes out.
- An install below the version an installed library needs is refused, naming that library; going back to an older version that nothing objects to goes in, and is said.
- An address on a host other than GitHub is refused before anything is fetched, rather than fetched from GitHub as the same owner and name; an SSH clone address of GitHub, `git@github.com:owner/name.git`, is read as `owner/name`.
- A zip that cannot be read, an incomplete download above all, is said to be one, no longer to hold no `libnima.yml`; a fetch that fails gives one hint, with this run's zips.
- A misspelt key inside an entry, a run step, a `once` or `extras` entry, a `headers` or `sources` mapping, a requirement, a peripheral, an option or a setting, is refused by `--lint` and the install, by name and line. One at the top of the file is still only said, so a newer file still reads.
- `--lint` refuses what every install refuses or breaks: a `once` file or an extra with a `root` on a file libNIMA edits, a file of the library's where its Zephyr module goes when it ships none of its own, a library that needs itself, and a name no folder on every computer can have, such as `con` or one ending in a dot.
- The JSON Schema takes `zephyr.symbol` with a note, the mapping form of a requirement, and an option's `{from, to}` files, asks for headers or sources, and refuses a misspelt requirement key.
- `--lint` says when a library's version is not three numbers, or it has none, and when `version:` says another than the `@version` tag; a requirement it cannot meet says what is wrong with that version.
- `requires.libnima` is read before `platform` everywhere, so a library for a newer libNIMA gets the message to update even when it names a platform this one does not know.
- `--lint`, `--schema` and `--convert` refuse an argument they would leave unused, and a folder given before `--lint` is the one it reads, named in its verdict; `--ref` with nothing fetched from GitHub is refused.
- `--conf` takes only a `.conf` file of the application: no file of sysbuild's, `sysbuild_<suffix>.conf` and `sysbuild/` among them, no other file, nothing outside the application. A once file of the library's is refused by `--check` as by the install, and neither is offered by the question.
- `--check` of a project lib-installer left checks it as the install will, moved over, on a copy, and writes nothing; a run keeps one `.bak` of each file it changes, the file as it was before the run, moving over included, and names it.
- An install that changes nothing leaves `.libnima.json` as it was, and a file that holds what the install has for it already is left as it was, said to be the same.
- An install and an uninstall give back to the byte a file whose last line had no line ending.
- An uninstall says when a change of the user's inside a block went with it, and which copy keeps it; it names each copy by its path from the project's folder, calls the module's USER CODE it saved the user's, and takes out an empty `modules/`.
- `--uninstall` where nothing was installed says so, with `--platform` or without.
- An update says once when the template of a `once` file it keeps changed since the file was made.
- `--check` says CHECK when a block was changed by hand, as the install writes it again.
- The exit codes are in `--help` and the README, and `--check` exits 1 when it says CHECK or NO.
- Every run names the platform and what told it; errors and warnings about `libnima.yml` name its line, with forward slashes; the Kconfig file taken with nobody to ask is said; a library with no kind shows none; an unknown option is said of the libraries by name; a run whose step failed no longer ends with Done.
- The window's libnima.yml tab no longer says of a title, or of a line with its note, that it is no key libNIMA reads, as it did of every note in a plain list, `- BT: on`, and of a title with a dot in it, such as `For gled_config.h`. A misspelt key of a row under a title is offered the right one.

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
