# libNIMA for Arduino

Installs embedded C and C++ libraries for Arduino, from GitHub, from a zip, or from a folder.

It puts a library where Arduino finds it, the way you choose: in the libraries folder of your sketchbook, where Library Manager installs to, for every sketch, or in the `src` folder of one sketch, for that sketch alone. A library written for any project gets the `library.properties` Arduino knows it by, the libraries it needs go in first, and its settings are kept through every update.

This page is what libNIMA does in an Arduino sketch, and what a library's `libnima.yml` can say for Arduino. How to install libNIMA, every command, and what is the same on every platform are in the [README](../README.md).

## Installing a library

Install a library by its GitHub owner and name:

```bash
libnima owner/example
```

To see it work first, take [lib-installer-example-arduino](https://github.com/nimaltd/lib-installer-example-arduino), a library made for trying libNIMA on. It builds for any board, and prints on Serial. Its README walks through both places a library can go, a setting kept through an update, an option, and an uninstall.

libNIMA finds Arduino's libraries folder by itself, so it runs from anywhere. Run from a sketch's folder, it asks where the library goes, as below.

### Where a library goes

Run from a sketch's folder, the one holding `blink.ino` for a sketch called `blink`, libNIMA asks:

```
This is the sketch blink. Where do the libraries go?
  1  the libraries folder, for every sketch: C:/Users/you/Documents/Arduino/libraries
  2  this sketch's src folder, for this sketch only
Install into [1]:
```

Press Enter for the libraries folder. Run from anywhere else, that is where it goes without a question. `--into libraries` or `--into sketch` answers it on the command line, and in a script, where nobody can answer, the libraries folder is taken.

| | The libraries folder | A sketch's src folder |
|---|---|---|
| Which sketches see it | every sketch | that sketch alone |
| You include it as | `#include "example.h"` | `#include "src/example/src/example.h"` |
| Its settings | one set, for every sketch | that sketch's own |
| It goes into | `Arduino/libraries/example/` | `blink/src/example/` |
| Library Manager lists it | yes | no |

In a sketch where libNIMA has put a library into `src` before, `2` is the answer offered instead, so an update goes where the library already is.

### The libraries folder

It is the `libraries` folder of your sketchbook, the folder the IDE keeps sketches in, and libNIMA finds it the way Arduino does. The first of these that is set wins:

1. `--sketchbook D:/Arduino`, on the command line.
2. `ARDUINO_DIRECTORIES_USER`, the variable arduino-cli reads.
3. Arduino IDE 2's settings, the sketchbook location in its Preferences.
4. arduino-cli's settings, `directories.user` in `arduino-cli.yaml`.
5. Arduino IDE 1's preferences, the sketchbook location in its Preferences.
6. Where Arduino puts it on a new machine: `Documents/Arduino` on Windows and macOS, `~/Arduino` on Linux.

Every install says which folder it used, and why:

```
Libraries folder: C:/Users/you/Documents/Arduino/libraries  (from Arduino IDE's settings)
```

### What happens

Take `example`, installed into the libraries folder:

```
Arduino/libraries/
  .libnima.json          what is installed here, and where
  example/
    library.properties       written by libNIMA: how Arduino knows the library
    src/
      example.h              the library's files, as its libnima.yml lists them
      example_config.h       its settings, between USER CODE lines
      example.cpp
```

The same library installed into a sketch called `blink`:

```
blink/
  blink.ino
  .libnima.json          what is installed in this sketch
  src/
    example/
      src/
        example.h
        example_config.h
        example.cpp
```

Arduino builds every file of a sketch's `src` folder with the sketch, so nothing else is needed there. It puts none of them on the include path, though, which is why the sketch includes the library by its path, and the last line of the install says exactly which:

```
Done. #include "src/example/src/example.h" and you are away.
```

### A library that needs another

What a library needs always goes into the libraries folder, also when the library itself goes into a sketch. In a sketch's `src` folder one library cannot find another: Arduino looks for `#include "osal.h"` in the libraries folder and nowhere else in the sketch. So libNIMA says so before it goes in:

```
Requirements
  note    osal, which it needs, goes into the libraries folder
          In a sketch's src folder one library cannot find another. In the libraries folder every library and sketch can.
```

It comes from the release the library names for it, or else from its default branch. One already in the libraries folder at a version that will do is left alone. Everything is fetched before anything is installed, so when a library cannot be had, nothing is changed.

### Settings

Arduino has no settings menu of its own for a library, so a library's settings are in its configuration header, between `USER CODE BEGIN` and `USER CODE END`:

```c
/* USER CODE BEGIN EXAMPLE_CONFIGURATION */
#define EXAMPLE_PERIOD_MS           250U
/* USER CODE END EXAMPLE_CONFIGURATION */
```

Change them there. Every install and update keeps what is between those lines. In the libraries folder there is one set, for every sketch; installed into a sketch, the sketch has its own.

### Files that become yours

A library installed into a sketch can bring files that belong to the sketch rather than to the library, such as an `arduino_secrets.h` for your network's name and password: a start, made for you to fill in. Each is created in the sketch's folder only when it is not there:

```
Files
  written src/example/src/example.h
  created arduino_secrets.h  yours from now on
```

From then on it is yours. An update never writes over it, and says so with `kept arduino_secrets.h  yours`. An uninstall leaves it where it is, with `kept arduino_secrets.h  yours, not removed`. A library can never put such a file over the sketch's `.ino` files, nor outside the sketch's folder, nor into a library's folder, whose next install would replace it.

The libraries folder is shared by every sketch, so it has no place for such files. A library that brings them, installed there, stops before anything is written and says to install it into a sketch, with `--into sketch`.

### What you see

A first install from GitHub, run from a sketch, into the libraries folder:

```
This is the sketch blink. Where do the libraries go?
  1  the libraries folder, for every sketch: C:/Users/you/Documents/Arduino/libraries
  2  this sketch's src folder, for this sketch only
Install into [1]:
Libraries folder: C:/Users/you/Documents/Arduino/libraries  (from the default)
Fetching owner/example ...

example 1.2.0
Driver for the X chip
driver / storage

Requirements
  note    Needs I2C, 400 kHz
          Wire it up and set it up in your sketch.

Folder to install example into [example]:

Files
  written example/src/example.h
  written example/src/example_config.h
  written example/src/example.cpp

Project
  updated library.properties  Wrote example's library.properties, which Arduino knows it by.

Done. #include "example.h" and you are away.
```

### Updating a library

Run the same command again. Every file of the library is replaced, so an update updates, and `library.properties` is written again from the new `libnima.yml`. What you wrote between `USER CODE BEGIN` and `USER CODE END` is kept. Outside those lines, the next install replaces it, and when that would lose something of yours the file is saved beside it first, as a `.bak`.

The record of what is installed, and where, is `.libnima.json`: in the libraries folder for what is there, and in a sketch for what is in it. For each library it also keeps where it came from, down to the commit, and the steps that ran.

### Uninstalling a library

```bash
libnima --uninstall example
```

Run from a sketch, it is taken out of the sketch when the sketch has it, and out of the libraries folder otherwise. `--into` says which, when it is in both. Its files go, and its `library.properties`. What was yours is saved before it goes, as a `.bak` beside it: a configuration header with your settings in it, or any file of the library you changed by hand, its extras among them. A file you put in the library's folder yourself is left where it is, and the folder with it, and so are the [files that became yours](#files-that-become-yours).

A library another one needs stays, and libNIMA says which needs it. One that came into the libraries folder because a library in a sketch needed it stays there too, since other sketches may use it.

## For someone writing a library

Every key of `libnima.yml` is on [the format page](FORMAT.md), and what only Arduino reads is under [Arduino](FORMAT.md#arduino). The window's second tab writes the file with you: **New** fills one in from what is in the library's folder, and `libnima --lint` checks one with no project. This is what is worth knowing about Arduino beyond the keys.

### What Arduino needs of a library

- **Keep the code in `src/`, since every file keeps its path.** Arduino builds `src/` and every folder in it, and puts `src/` on the include path. A library with no `src/` folder is built from the files at its top instead.
- **Headers beside the code that includes them.** In a sketch's `src` folder nothing is on the include path, so `#include "example.h"` in `example.cpp` works only when the two are in the same folder. A header in another folder of the library is included by its path from there.
- **`extern "C"` in every header of C code.** A sketch is C++, and without it the linker cannot find a function of a `.c` file.
- **Settings in a configuration header**, between `USER CODE` lines libNIMA keeps, since Arduino has no other place for them.
- **No `defines`.** Arduino has no defines for a whole build, so libNIMA warns about them and sets nothing. Put them in the configuration header instead.

A library made for Arduino that ships its own `library.properties`, listed under `extras`, `headers` or `sources` so it is installed, keeps it: libNIMA writes one only for a library that has none.
