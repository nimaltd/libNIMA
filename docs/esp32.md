# libNIMA for ESP-IDF

Installs embedded C libraries into an ESP-IDF project, for any ESP32 chip.

A library written for any project has no idea what an ESP-IDF component is. This copies it into your project's `components` folder and makes it a component: its own `CMakeLists.txt`, the ESP-IDF components it uses, its settings in menuconfig, and the symbols it needs in `sdkconfig.defaults`. Then `idf.py build` picks it up like any other component.

This page is what libNIMA does in an ESP-IDF project, and what a library's `libnima.yml` can say for ESP-IDF. How to install libNIMA, every command, and what is the same on every platform are in the [README](../README.md).

## Installing a library

From your project's folder, the one holding `CMakeLists.txt` and `main/`, install a library by its GitHub owner and name, then build as always:

```bash
libnima owner/example
idf.py build
```

To see it work first, take [lib-installer-example-esp32](https://github.com/nimaltd/lib-installer-example-esp32), a library made for trying libNIMA on. It builds for any ESP32 chip, with its settings in menuconfig. Its README walks through an install, a setting changed, an option, and an uninstall.

It runs from any shell, ESP-IDF's own included: it needs nothing of ESP-IDF's.

### What happens

Take `example`, installed into a project called `app`:

```
app/
  CMakeLists.txt           not touched, for a library in components/ that
                           adds no lines of its own here
  sdkconfig.defaults       one block added, with the symbols the library needs
  main/
    CMakeLists.txt         example added to PRIV_REQUIRES, when main lists its own
                           or the project builds only what main names
  components/example/
    src/example.h          the library's files, at the paths they have in its repository
    src/example.c
    CMakeLists.txt         written by libNIMA: idf_component_register()
    Kconfig                written by libNIMA: the library's settings
```

ESP-IDF builds every folder of `components/` that has a `CMakeLists.txt` calling `idf_component_register()`, so libNIMA writes one, from the library's `libnima.yml`:

```cmake
idf_component_register(
    SRCS
        "src/example.c"
    INCLUDE_DIRS
        "src"
    REQUIRES
        esp_timer
)
```

`REQUIRES` names the ESP-IDF components the library uses, and the libraries it needs, so their headers are on its include path.

The component is named after its folder, `example`. A component of the project takes the place of one of ESP-IDF's with the same name, so a library called `log` would replace ESP-IDF's own logging. When ESP-IDF is set up in the shell you run from, so `IDF_PATH` is set, libNIMA checks for that before anything is written, and says to install it into a folder of another name:

```
Requirements
  check   ESP-IDF has a component called log, and this one would take its place
          Install it into a folder of another name, such as --dir components/log_lib.
```

### main

`main` is special in ESP-IDF: as long as its `idf_component_register()` lists no `REQUIRES` or `PRIV_REQUIRES` of its own, it uses every component of the project by itself, and nothing in it changes. Once it lists its own, it sees only those, so libNIMA adds the library to them:

```cmake
idf_component_register(SRCS "main.c"
                       PRIV_REQUIRES example esp_timer)
```

Into `PRIV_REQUIRES` when main has one, otherwise `REQUIRES`.

A project that builds only what `main` names leaves out every other component, so there `main` gets a list even when it had none. ESP-IDF's own examples are built that way, `hello_world` and `blink` among them, with `idf_build_set_property(MINIMAL_BUILD ON)` in the project's `CMakeLists.txt`, and `set(COMPONENTS ...)` does the same:

```cmake
idf_component_register(SRCS "main.c"
                    INCLUDE_DIRS "." PRIV_REQUIRES example)
```

Without it, `#include "example.h"` in main would not be found. A component of your own that uses the library lists it the same way, by its folder's name.

### sdkconfig.defaults

The Kconfig symbols the library needs go into a block at the end of `sdkconfig.defaults`, created when the project has none:

```
# >>> libnima: example >>>
# Added by libnima, and written again on every install, so set
# example's symbols below this block rather than in it.
CONFIG_FREERTOS_HZ=1000
# <<< libnima: example <<<
```

A library that needs no symbol gets no block. A symbol you already set yourself, on or off, is not set again, since the line read last would quietly undo your choice. It is still written in the block, commented out, with the line that set it:

```
# CONFIG_FREERTOS_HZ=1000  (left out: sdkconfig.defaults sets CONFIG_FREERTOS_HZ=100)
```

A library can give its symbols in groups, each under a title, and a symbol with a note. The block carries both as comments, so you see why each line is there:

```
# >>> libnima: example >>>
# Added by libnima, and written again on every install, so set
# example's symbols below this block rather than in it.

# ******************************************************************************
# A faster tick
# ******************************************************************************
# one tick a millisecond
CONFIG_FREERTOS_HZ=1000
# <<< libnima: example <<<
```

ESP-IDF reads `sdkconfig.defaults` only for a symbol your `sdkconfig` does not have yet, and `sdkconfig` is what it builds with. So libNIMA also reads your `sdkconfig`, and names every symbol it holds at another value:

```
Requirements
  check   sdkconfig has CONFIG_FREERTOS_HZ=100, and example needs 1000
          sdkconfig is what ESP-IDF builds with. Set it in idf.py menuconfig.
```

Set it in `idf.py menuconfig`, or delete `sdkconfig` to start again from `sdkconfig.defaults`. libNIMA never edits `sdkconfig` itself: ESP-IDF writes that file, and it is yours.

Every file is copied aside before it changes, as `sdkconfig.defaults.20261016-190827.bak` and the like, and the files keep their own line endings. Installing twice changes nothing the second time.

### Settings

A library's settings are Kconfig symbols, in `idf.py menuconfig` under the library's name, from the `Kconfig` libNIMA wrote into its component. Set one there, or in `sdkconfig.defaults` for a fresh `sdkconfig`:

```
CONFIG_EXAMPLE_PERIOD_MS=250
```

The C code reads it as `CONFIG_EXAMPLE_PERIOD_MS`, from `sdkconfig.h`.

### A folder outside components

Pressed Enter at the folder question, a library goes into `components/<name>`, where ESP-IDF finds it by itself. Anywhere else, given with `--dir` or typed as the answer, ESP-IDF has to be told, so a block goes into your project's `CMakeLists.txt`, above `include(project.cmake)`, which is when ESP-IDF looks:

```cmake
cmake_minimum_required(VERSION 3.16)

# >>> libnima: example >>>
# Added by libnima, since example is not in the components folder,
# where ESP-IDF looks by itself. Delete this block to drop it from the build.
list(APPEND EXTRA_COMPONENT_DIRS "${CMAKE_CURRENT_SOURCE_DIR}/libs/example")
# <<< libnima: example <<<

include($ENV{IDF_PATH}/tools/cmake/project.cmake)
project(app)
```

### Lines a library adds to your CMakeLists.txt

A library can need lines of CMake in your project's `CMakeLists.txt`, and then it says so in its `libnima.yml`: above `include(project.cmake)`, for what ESP-IDF reads as the project starts, or at the end of the file, for what needs the project's own targets. The lines of each place go between two lines that name it:

```cmake
# >>> libnima: example/before-project >>>
# From example's libnima.yml, and written again on every install.
list(APPEND SDKCONFIG_DEFAULTS "${CMAKE_CURRENT_SOURCE_DIR}/example.defaults")
# <<< libnima: example/before-project <<<
```

Every block is written again on every install, so put your own lines outside them. An edit made inside one is noticed on the next install, and the file as it was is in the `.bak`. A title the library gives a group of its lines is written among them, between two lines of stars, as in `sdkconfig.defaults`.

### Files that become yours

A library can bring files that belong to your project rather than to the library, such as a `version.txt`, which ESP-IDF reads the project's version from, or a file of settings: a start, made for you to change. Each is created in your project's folder only when it is not there:

```
Files
  written components/example/src/example.h
  created version.txt  yours from now on
```

From then on it is yours. An update never writes over it, and says so with `kept version.txt  yours`. An uninstall leaves it where it is, with `kept version.txt  yours, not removed`. A library can never put such a file over one libNIMA manages or reads, `CMakeLists.txt`, `main/CMakeLists.txt`, `sdkconfig` and `sdkconfig.defaults`, nor outside your project's folder.

### What you see

A first install from GitHub, into a project whose `main` lists its own components:

```
Fetching owner/example ...

example 1.2.0
Driver for the X chip
driver / storage

Requirements
  note    CONFIG_FREERTOS_HZ=1000 goes into sdkconfig.defaults
  note    Needs I2C, 400 kHz
          Set it up in your code, or in idf.py menuconfig.

Folder to install example into [components/example]:

Files
  written components/example/src/example.h
  written components/example/src/example.c

Project
  updated ESP-IDF component  The component example: wrote CMakeLists.txt, Kconfig.
  updated sdkconfig.defaults  Added example to sdkconfig.defaults.
          backup: sdkconfig.defaults.20261016-190827.bak
  updated main/CMakeLists.txt  Added example to PRIV_REQUIRES in main/CMakeLists.txt, since main lists the components it uses.
          backup: CMakeLists.txt.20261016-190827.bak

Done. #include "example.h" and you are away.
```

The hardware a library uses is yours to set up, which libNIMA names but does not do.

### Updating a library

Run the same command again. Every file of the library is replaced, so an update updates, and the component's `CMakeLists.txt` and `Kconfig` are written again from the new `libnima.yml`, so a source file the new version adds is built without you editing anything. A file that did not change is left alone, so the build does not configure again for nothing.

What you wrote between `USER CODE BEGIN` and `USER CODE END` in a library's file is kept. Outside those lines, the next install replaces it, and when that would lose something of yours the file is saved beside it first, as a `.bak`.

The record of what is installed, and where, is `.libnima.json` in your project's folder. For each library it also keeps where it came from, down to the commit, the blocks written into your files, and the steps that ran. Keep it in version control with the rest.

### Uninstalling a library

```bash
libnima --uninstall example
```

From your project's folder, or with `--project`. Its block goes from `sdkconfig.defaults`, and its blocks from `CMakeLists.txt` when it has any, its name goes from main's `REQUIRES` and `PRIV_REQUIRES`, and a list left with nothing in it goes too, the files libNIMA wrote go from its component, and then the library's own files. Installed and then uninstalled, your project's files are back to the byte they were, except the [files that became yours](#files-that-become-yours), which stay.

What was yours is saved before it goes, as a `.bak` beside it: any file of the library you wrote in between its `USER CODE` lines or changed by hand. A file you put in the library's folder yourself is left where it is, and the folder with it.

A setting you set yourself in `sdkconfig.defaults` is your line, and it stays. The symbol it sets went with the library, though, so libNIMA names every such line, for you to take out:

```
  manual  sdkconfig.defaults  sdkconfig.defaults still sets what came with example, which ESP-IDF no longer knows. Take these lines out:
          CONFIG_EXAMPLE_PERIOD_MS=250
```

Your `sdkconfig` still has the library's settings too. ESP-IDF drops a symbol it no longer knows the next time it writes `sdkconfig`, so there is nothing to do there.

A library another one needs stays, and libNIMA says which needs it. Name them all, in any order, and each goes after what needs it: `libnima --uninstall osal ee24`. When the library you took out brought another with it that nothing left needs, the last line names it, with the command that takes it out too.

### A library that needs another

You install only the one you want, and what it needs goes in first, each as a component of its own, and the one that needs it lists it in its `REQUIRES`. It comes from the release the library names for it, or else from its default branch. One already in the project at a version that will do is left alone. Everything is fetched before anything is installed, so when a library cannot be had, your project is left as it was.

## For someone writing a library

Every key of `libnima.yml` is on [the format page](FORMAT.md), and what only ESP-IDF reads is under [ESP-IDF](FORMAT.md#esp-idf). The window's second tab writes the file with you: **New** fills one in from what is in the library's folder, and `libnima --lint` checks one with no project. This is what is worth knowing about ESP-IDF beyond the keys.

### Titles and notes

Every list under `esp32.project` and `esp32.library` but `requires` can be given in groups, each under its title, and a symbol with a note. libNIMA writes both into the file the list goes into, as comments, so whoever reads the project's `sdkconfig.defaults`, or the component's `CMakeLists.txt`, sees why each line is there:

```yaml
esp32:
  project:
    conf:
      A faster tick:
        - FREERTOS_HZ=1000: one tick a millisecond
      Quiet logs:
        - LOG_DEFAULT_LEVEL_WARN
        - LOG_COLORS=n: plain text, for a file
  library:
    cmake:
      Math:
        - target_link_libraries(${COMPONENT_LIB} PRIVATE m)
```

The block in `sdkconfig.defaults`:

```
# >>> libnima: example >>>
# Added by libnima, and written again on every install, so set
# example's symbols below this block rather than in it.

# ******************************************************************************
# A faster tick
# ******************************************************************************
# one tick a millisecond
CONFIG_FREERTOS_HZ=1000

# ******************************************************************************
# Quiet logs
# ******************************************************************************
CONFIG_LOG_DEFAULT_LEVEL_WARN=y
# plain text, for a file
CONFIG_LOG_COLORS=n
# <<< libnima: example <<<
```

And the component's `CMakeLists.txt`:

```cmake
# >>> libnima: example/component >>>
# From example's libnima.yml, and written again on every install.
# ******************************************************************************
# Math
# ******************************************************************************
target_link_libraries(${COMPONENT_LIB} PRIVATE m)
# <<< libnima: example/component <<<
```

- A list is either plain, or a mapping of titles to their lists. A plain list is written with no title.
- A title is one line, written between two lines of stars, 80 columns wide, above its group, and a group after the first starts after a blank line. The same holds in the lines of CMake, in the project's file and the component's.
- A note follows its symbol after a colon, `LOG_COLORS=n: plain text`, and is written as a comment of its own above the symbol's line. A symbol the project sets itself keeps its note, above the line left out.
- A line of CMake takes a title but no note: a line holding `: ` is CMake's own, and goes in quotes, so that YAML reads it as a line. A title or a note holding `: ` goes in quotes too, as any YAML text does.
- `esp32.library.requires` is a plain list of names, with no title and no note.
- A title or a note is one line, and none may hold what libNIMA marks its blocks with, `>>>`, `<<<` or `USER CODE`, nor read as a setting, as `CONFIG_X is not set` does. Such a file is refused when it is read.
- Two groups with one title are refused, as is any key given twice, and so is one symbol given twice: the line read last would quietly undo the first.

### More CMake for a library

A library that needs more than its files and its defines, a library linked in, an option for its compiler, or a line in the project's own `CMakeLists.txt`, lists the lines by the file each goes into: the project's under `esp32.project.cmake`, by their place, and the component's own under `esp32.library.cmake`:

```yaml
esp32:
  project:
    cmake:
      before-project:
        - list(APPEND SDKCONFIG_DEFAULTS "${CMAKE_CURRENT_SOURCE_DIR}/example.defaults")
      after-project:
        - idf_component_get_property(main_lib main COMPONENT_LIB)
        - target_compile_definitions(${main_lib} PRIVATE EXAMPLE_IN_MAIN)
  library:
    cmake:
      - target_link_libraries(${COMPONENT_LIB} PRIVATE m)
      - target_compile_options(${COMPONENT_LIB} PRIVATE -O2)
```

| Key | Written into | For |
|---|---|---|
| `esp32.library.cmake` | the component's `CMakeLists.txt`, after `idf_component_register()`, where `${COMPONENT_LIB}` is its own library | the component itself |
| `esp32.project.cmake`, `before-project` | the project's `CMakeLists.txt`, above `include($ENV{IDF_PATH}/tools/cmake/project.cmake)` | what ESP-IDF reads as the project starts, such as `EXTRA_COMPONENT_DIRS` or `SDKCONFIG_DEFAULTS` |
| `esp32.project.cmake`, `after-project` | the project's `CMakeLists.txt`, at its end | what needs the project's own targets, every component's library among them |

The lines of each place are written as one block, between two lines that name the library and the place, `# >>> libnima: example/before-project >>>` and its pair, and `example/component` for the component's own, in the file's order. Every install writes them again: a place whose lines changed is updated, and one the file no longer has is taken out. A line keeps the spaces it starts with, so the body of an `if()` can be indented. `{library}` in a line names the library's folder, as `${CMAKE_CURRENT_SOURCE_DIR}/components/example` in the project's file and `${COMPONENT_DIR}` in the component's, wherever `--dir` put it; anything else in a line is CMake's own, written as it stands. A line holding a colon, as `message()` often does, goes in quotes, or YAML reads it as a mapping.

An option brings lines of its own under `options.<name>.esp32.project.cmake` and `options.<name>.esp32.library.cmake`, the same way, written while it is chosen and taken out by the update that turns it off. Its blocks are named after it and the place, `example/littlefs-component`, so they never meet the library's.

One thing about `SDKCONFIG_DEFAULTS`: once it is set, ESP-IDF reads only the files it lists, and no longer `sdkconfig.defaults` by itself. A group that adds a file to it names `sdkconfig.defaults` first, when the project has one and nothing set the list yet:

```cmake
if(NOT SDKCONFIG_DEFAULTS AND EXISTS "${CMAKE_CURRENT_SOURCE_DIR}/sdkconfig.defaults")
    set(SDKCONFIG_DEFAULTS "${CMAKE_CURRENT_SOURCE_DIR}/sdkconfig.defaults")
endif()
list(APPEND SDKCONFIG_DEFAULTS "${CMAKE_CURRENT_SOURCE_DIR}/example.defaults")
```

A component of headers alone is an interface library, so its lines use `INTERFACE` where one with sources uses `PRIVATE` or `PUBLIC`. The lines run when the user configures the build, the same as a `CMakeLists.txt` the library ships itself would, so they are the library's to answer for.

The user has a place in the component's file too. Its last lines are a `USER CODE` section, empty to start with, and every install keeps what is written in it, the way a configuration header keeps settings:

```cmake
# USER CODE BEGIN EXAMPLE_CMAKE
target_compile_definitions(${COMPONENT_LIB} PRIVATE EXAMPLE_DEBUG=1)
# USER CODE END EXAMPLE_CMAKE
```

On an uninstall, a section with something in it is saved as a `.bak` before the file goes.

### Settings in the code

ESP-IDF writes every setting into `sdkconfig.h`, so the library's code reads it as a macro once it includes that. One header can serve ESP-IDF and any other project, taking the setting from Kconfig when it is there:

```c
#if __has_include("sdkconfig.h")
#include "sdkconfig.h"
#endif

#ifdef CONFIG_EXAMPLE_POLL_CHUNK
#define EXAMPLE_POLL_CHUNK          CONFIG_EXAMPLE_POLL_CHUNK
#endif

#ifndef EXAMPLE_POLL_CHUNK
#define EXAMPLE_POLL_CHUNK          32U
#endif
```

A `bool` setting that is off is not defined at all, which is how Kconfig does it, so test it with `#ifdef`. A `string` arrives in quotes, ready for C.

Each setting is checked at install, since all of it is written into a Kconfig file: a default of the wrong type, one outside its range, or a name Kconfig would refuse stops the install with the reason, before anything is written.

### A library with its own component

A library that ships a `CMakeLists.txt` of its own, listed under `extras`, `headers` or `sources` so it is installed, knows its build better than libNIMA does. It is used as it is: nothing is written into its folder, and only the project's own files get what they need.
