# libNIMA plan

libNIMA puts an embedded C library into an STM32, Zephyr, ESP-IDF, Arduino or
PlatformIO project, from a window or from the `libnima` command, the same on
Windows, Linux and macOS. It replaces lib-installer, which was written in Python,
and starts at v1.0.0.

This file is the plan: what we build, what we build it with, and in which order. It
will change as we learn. The checklist at the bottom shows where we are.

---

## Decisions

| Topic | Decision |
|---|---|
| Language | Rust for everything that is not the window's own drawing. No Python anywhere in what people get |
| Window | Tauri 2, Svelte 5 and TypeScript, the look of terNIMA |
| Programs | `libnima`, the command, and `libnima-app`, the window, around one engine, `libnima-core`. Windows sees `libnima.exe` and `libNIMA.exe` as one file, so the window's is not called after the product |
| The engine | Never prints and never asks. What it has to say or ask goes through a trait the two programs each fill in |
| Platforms | Windows 10/11, Linux x86_64, macOS (Apple Silicon and Intel) |
| The file | `libnima.yml` at the root of a library's repository, see [FORMAT.md](FORMAT.md) |
| Old libraries | A repository with only an `installer.yml` asking for lib-installer 3.x or 4.x is read through a built-in converter, `libnima --convert`. Anything older is refused, with the reason |
| Old projects | A project lib-installer installed into moves over on the first run: `.lib-installer.json` becomes `.libnima.json`, and the markers of its blocks are renamed. Each file is saved first |
| Finding the platform | New. lib-installer took it from the library. libNIMA reads the project, says which file told it, and lets you change it |
| Tokens | Asked and never saved, or read from `GITHUB_TOKEN` or `GH_TOKEN` |
| License | Apache 2.0, like the other NimaLTD projects |

---

## How a library gets in

1. Open the project's folder. libNIMA finds which platform it is.
2. Give a library: `owner/name`, a GitHub address, a zip, or a folder.
3. libNIMA finds its `libnima.yml`, fetches what it lists, and checks it against
   the project. Nothing in the project is touched by any of this.
4. The library joins the list to install, marked with what the check found:

   | Mark | Meaning |
   |---|---|
   | OK | Nothing stands in its way |
   | CHECK | It installs, and there is something to do, with the way to do it. Never a refusal, because a project can be laid out in ways this cannot read |
   | NO | It is not installed, and the reason is beside it: a library for another platform, or one that needs a newer libNIMA |

5. Install takes the whole list, what each library needs first.

The command does the same from the project's root: `libnima owner/name`.

---

## The window

```
 File  Edit  View  Tools  Help
 [ Project ]  [ libnima.yml ]
+--------------------------------------------------------------------------+
| Project:  D:/Work/MyBoard                               [Open folder]    |
| Platform: STM32, from MyBoard.ioc      IDE: CubeIDE, CMake     [Change]  |
+--------------------------------------------------------------------------+
| Add library: [ nimaltd/ee24                                 ]  [Add]     |
+--------------------------------------------------------------------------+
| To install                                                               |
|  OK     ee24   3.1.0   needs osal >= 1.0.0, already here                 |
|  CHECK  spif   2.0.0   HAL SPI module is not enabled      [How to fix]   |
|  NO     gcore  1.0.0   Only for Zephyr, this project is STM32            |
|                                             [Preview]  [Install all]     |
+--------------------------------------------------------------------------+
| Installed                                                                |
|  osal   1.0.0   newer: 1.0.2                     [Update]  [Uninstall]   |
+--------------------------------------------------------------------------+
| what the last run did, line by line                                      |
+--------------------------------------------------------------------------+
```

The second tab edits a library's `libnima.yml` in a local folder, as a form or as text,
which are one file: the form changes only the lines of what it changes, and the text it
makes is read again before it is kept, so its comments and its order stay. **New** fills a
first file in from what is in the folder. The check runs as you type, with the same code
an install reads the file with, each problem names its line, and many come with a fix a
click makes. The text offers what can go at the cursor and says what a key is, a summary
beside either says what the file comes to, and all of it reads one table of the keys,
which the tests hold to the reader and to the format page. A repository's file can be
looked at there, and not saved, since it has nowhere to be saved to.

```
 [ Project ]  [ libnima.yml ]
+--------------------------------------------------------------------------+
| D:/Work/ee24/libnima.yml                     [Save]  [Try on project]    |
+--------------------------------------------------------------------------+
| 24  requires:                                                            |
| 25    libnima: 1.0.0                                                     |
| 26    peripherals:                                                       |
| 27      - type: i2c                                                      |
| 28        count: 0                                                       |
+--------------------------------------------------------------------------+
| Problems                                                                 |
|  line 28  count is 0. Give how many, a whole number from 1.              |
+--------------------------------------------------------------------------+
```

---

## The command

Everything lib-installer took stays: `--ref`, `--dir`, `--with`, `--without`,
`--project`, `--run`, `--no-run`, `-y`, `--key`, `--uninstall`, `--platform`,
`--ide`, `--conf`, `--into`, `--sketchbook`, `--no-update-check` and `-V`. New:

| Command | What it does |
|---|---|
| `libnima --gui` | Opens the window |
| `libnima --convert` | Writes a folder's `installer.yml` out as `libnima.yml` |
| `libnima --check owner/name` | Checks a library against this project and writes nothing |
| `libnima --lint` | Checks a folder's `libnima.yml` with no project, for a library's own CI |
| `libnima --add-to-path` | Puts its own folder on your PATH, for the command alone from its zip. `--remove-from-path` takes it off |
| `libnima --schema` | Writes the JSON Schema of `libnima.yml`, for an editor's help on the file |

---

## Layout

```
crates/libnima-core/   the engine: find the platform, read the yml, check, install
crates/libnima-cli/    the libnima command
src-tauri/             the libnima-app window: connects the engine to the UI
ui/                    the UI: Svelte and TypeScript
scripts/               what packaging runs: builds the command for the installers
docs/                  this plan, the format of libnima.yml, and a guide for each platform
```

---

## Where we are

- [x] 1. Skeleton: the workspace, CI on all three, an empty window, `libnima --version`
- [x] 2. The new file: its format, a reader that keeps line numbers, its checks, the converter
- [x] 3. The engine: the shared parts, then STM32 with its five IDEs, Zephyr, ESP-IDF, Arduino
      and PlatformIO. A platform is done when its tests are in Rust and what it writes
      matches lib-installer 4.0.1 byte for byte, apart from the name
- [x] 4. The command, with every option above
- [x] 5. The window: the project tab, then the editor
- [x] 6. Packaging: the installers, adding the command to PATH, a zip of the command alone,
      asking GitHub for a newer version
- [ ] 7. Release: the 11 libraries' files, the READMEs, the help, the site, a last Python
      release that points here, and v1.0.0
