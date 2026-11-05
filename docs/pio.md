# libNIMA for PlatformIO

Installs embedded C and C++ libraries into a PlatformIO project, from GitHub, from a zip, or from a folder, for any framework.

It puts a library into your project's `lib` folder, which PlatformIO builds from for every framework: Arduino, ESP-IDF, STM32Cube, or none at all on the native platform. A library written for any project gets the `library.json` PlatformIO builds it by, the defines it needs go into `platformio.ini`, the libraries it needs go in first, and its settings are kept through every update.

This page is what libNIMA does in a PlatformIO project, and what a library's `libnima.yml` can say for PlatformIO. How to install libNIMA, every command, and what is the same on every platform are in the [README](../README.md).

## Installing a library

From your project's folder, the one holding `platformio.ini`, install a library by its GitHub owner and name:

```bash
libnima owner/example
```

To see it work first, take [libnima-example-pio](https://github.com/nimaltd/libnima-example-pio), a library made for trying libNIMA on. It builds with every framework, and on the native platform too, so you can run it on your computer with no board at all. Its README walks through a setting kept through an update, an option, and an uninstall.

Run from a folder with no `platformio.ini`, it stops before writing anything, since a library put there would be in no build.

### Where a library goes

Into `lib`, in a folder of its own:

```
my-project/
  platformio.ini            the defines of a library that has some, in build_flags
  .libnima.json             what is installed here, and where
  src/
    main.c
  lib/
    example/
      library.json          written by libNIMA: how PlatformIO builds the library
      src/
        example.h           the library's files, as its libnima.yml lists them
        example_config.h    its settings, between USER CODE lines
        example.c
```

PlatformIO builds a library of `lib` once your code includes one of its headers, for every environment in `platformio.ini`, so the library is your project's own, with your project's settings. You include it by its name:

```c
#include "example.h"
```

libNIMA asks for the folder, with `lib/example` as the answer when you just press Enter. A folder outside `lib` works too, given with `--dir`, but PlatformIO does not look there, so libNIMA says what to add to `platformio.ini`:

```
Project
  manual  lib_extra_dirs  PlatformIO looks for libraries in lib/, and example is not in it.
          Add its folder's parent to lib_extra_dirs in the [env] section of platformio.ini, or install it into lib/ with --dir lib/example.
```

### library.json

PlatformIO builds a library by its `library.json`. A library written for any project has none, so libNIMA writes one from the library's `libnima.yml`, and writes it again on every install:

```json
{
  "name": "example",
  "version": "1.0.0",
  "frameworks": "*",
  "platforms": "*",
  "build": {
    "srcDir": ".",
    "srcFilter": ["-<*>", "+<src/example.c>"],
    "includeDir": "src"
  }
}
```

It names the library's source files one by one, so PlatformIO builds those and nothing else: a test or an example in the library's folder is left alone. Its description, licence, repository and author go in too, and the frameworks, platforms and PlatformIO Registry libraries the library lists, as [below](#for-someone-writing-a-library).

A library made for PlatformIO that ships its own `library.json`, listed in its `libnima.yml` so it is installed, keeps it: libNIMA writes one only for a library that has none.

### Defines

A library that needs a define for the whole build, such as littlefs's `LFS_DEFINES`, lists it in its `libnima.yml`, and libNIMA puts it into `build_flags` in `platformio.ini`, between two lines that say whose it is:

```ini
[env]
build_flags =
    ; >>> libnima: example >>>
    -DEXAMPLE_FAST
    ; <<< libnima: example <<<

[env:uno]
platform = atmelavr
board = uno
framework = arduino
```

`[env]` is shared by every environment, so that is where it goes, and libNIMA adds the section when an environment needs it and the file has none. An environment with `build_flags` of its own does not take them from `[env]`, so it gets the same lines in its own. Everything else in the file is left as you wrote it.

Not in `library.json`: PlatformIO gives the `build_flags` of a `library.json` to the library's own files and to nothing else, so your code, which includes the library's header, would not see the define, and the two would not agree on what the header declares.

Every install writes the lines again from the library's new `libnima.yml`, and an uninstall takes them out, with the section libNIMA added, so `platformio.ini` is back to the byte it was.

### Build flags

A library can need a flag of its own for the whole build, such as `-Wl,-u,_printf_float`, without which `printf("%f")` prints nothing on many chips. It goes into the same block, after the defines, and a title or a note the library gives it is written as a comment of `platformio.ini`'s, `;`:

```ini
[env]
build_flags =
    ; >>> libnima: example >>>
    -DEXAMPLE_FAST
    ; ******************************************************************************
    ; Printing
    ; ******************************************************************************
    ; printf("%f") prints nothing without it
    -Wl,-u,_printf_float
    ; <<< libnima: example <<<
```

PlatformIO reads the comments as comments, and gives every environment the flags, as it does the defines. There is no blank line in the block: in an ini file one reads as the end of the value, to a person and to many a tool, if not to PlatformIO.

### Frameworks and platforms

PlatformIO leaves a library out of any build whose framework or platform the library does not list, without a word, and your `#include` then simply finds nothing. So before anything is written, libNIMA reads every environment in `platformio.ini` and names any the library will not build in:

```
Requirements
  check   [env:esp32] builds with espidf, which example does not list, so PlatformIO leaves it out of that build
          example lists: arduino.
```

It is a warning and nothing more. The library still goes in, for the environments it does build in.

### A library that needs another

What a library needs goes into `lib` too, beside it, and installs first. In `lib` every library finds the others' headers, so nothing else is needed. It comes from the release the library names for it, or else from its default branch. One already there at a version that will do is left alone. Everything is fetched before anything is installed, so when a library cannot be had, nothing is changed.

A library from the [PlatformIO Registry](https://registry.platformio.org), such as ArduinoJson, is a different thing: the library lists it, libNIMA writes it into the library's `library.json`, and PlatformIO installs it by itself at the next build.

### Settings

A library's settings are in its configuration header, between `USER CODE BEGIN` and `USER CODE END`:

```c
/* USER CODE BEGIN EXAMPLE_CONFIGURATION */
#define EXAMPLE_PERIOD_MS           250U
/* USER CODE END EXAMPLE_CONFIGURATION */
```

Change them there. Every install and update keeps what is between those lines. The library is in your project's `lib`, so they are your project's settings, for every environment it builds.

### Files that become yours

A library can bring files that belong to your project rather than to the library: a partition table, a file of secrets for you to fill in. Each is a start, made for you to change, so it is created in your project's folder only when it is not there:

```
Files
  written lib/example/src/example.h
  created partitions.csv  yours from now on
```

From then on it is yours. An update never writes over it, and says so with `kept partitions.csv  yours`. An uninstall leaves it where it is, with `kept partitions.csv  yours, not removed`. A library can never put such a file over `platformio.ini`, nor outside your project's folder, nor into a library's folder, whose next install would replace it.

### What you see

A first install from GitHub, of a library that needs I2C and a define:

```
Fetching owner/example ...

example 1.2.0
Driver for the X chip
driver / storage

Requirements
  note    EXAMPLE_FAST goes into build_flags in platformio.ini
  note    Needs I2C, 400 kHz
          Set it up in your code.

Folder to install example into [lib/example]:

Files
  written lib/example/src/example.h
  written lib/example/src/example_config.h
  written lib/example/src/example.c

Project
  updated library.json  Wrote example's library.json, which PlatformIO builds it by.
  updated platformio.ini  Added example's defines to platformio.ini, for every environment.
          backup: platformio.ini.20261017-091500.bak

Done. #include "example.h" and you are away.
```

### Updating a library

Run the same command again. Every file of the library is replaced, so an update updates, and `library.json` and the lines in `platformio.ini` are written again from the new `libnima.yml`. What you wrote between `USER CODE BEGIN` and `USER CODE END` is kept. Outside those lines, the next install replaces it, and when that would lose something of yours the file is saved beside it first, as a `.bak`. `platformio.ini` is saved the same way, before each change, as `platformio.ini.<date>-<time>.bak`.

The record of what is installed, and where, is `.libnima.json`, beside `platformio.ini`. For each library it also keeps where it came from, down to the commit, the block written into `platformio.ini`, and the steps that ran.

### Uninstalling a library

```bash
libnima --uninstall example
```

Its files go, its `library.json`, and its lines in `platformio.ini`. What was yours is saved before it goes, as a `.bak` beside it: a configuration header with your settings in it, or any file of the library you changed by hand, its extras and tools among them. A file you put in the library's folder yourself is left where it is, and the folder with it, and so are the [files that became yours](#files-that-become-yours).

A library another one needs stays, and libNIMA says which needs it. One that came in only because the library needed it stays too, and libNIMA says how to take it out.

## For someone writing a library

Every key of `libnima.yml` is on [the format page](FORMAT.md), and what only PlatformIO reads is under [PlatformIO](FORMAT.md#platformio). The window's second tab writes the file with you: **New** fills one in from what is in the library's folder, and `libnima --lint` checks one with no project. This is what is worth knowing about PlatformIO beyond the keys.

### What PlatformIO needs of a library

- **The header to include first in `headers`.** `library.json` names one include folder, the one the first header listed is in, unless `include_dirs` names the folders. A header in any other folder is found too: libNIMA adds that folder to `library.json` as `-I`, which PlatformIO gives the library and your project alike.
- **`extern "C"` in every header of C code**, when a project in C++ may use it, as one with the Arduino framework does. Without it the linker cannot find a function of a `.c` file.
- **Settings in a configuration header**, between `USER CODE` lines libNIMA keeps.
- **A define in `defines` only when the whole build needs it.** It goes into every environment of the project. A setting that only the library reads belongs in its configuration header.
- **`frameworks` and `platforms` that are true.** Listing too few hides the library from a build that could use it, and libNIMA warns about it; listing too many lets a build try and fail.

A library made for PlatformIO that ships its own `library.json`, listed under `extras`, `headers` or `sources` so it is installed, keeps it: libNIMA writes one only for a library that has none.
