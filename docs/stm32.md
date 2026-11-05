# libNIMA for STM32

Installs embedded C libraries into an STM32 project.

Copying two files into a project is easy. Doing it so the build actually picks them up, without clobbering the settings you changed last month, is the part this handles.

This page is what libNIMA does in an STM32 project, and what a library's `libnima.yml` can say for STM32. How to install libNIMA, every command, and what is the same on every platform are in the [README](../README.md).

## Installing a library

From the root of your STM32 project, the folder CubeMX made, install a library by its GitHub owner and name:

```bash
libnima owner/example
```

To see it work first, take [lib-installer-example-stm32](https://github.com/nimaltd/lib-installer-example-stm32), a library made for trying libNIMA on. It builds in any STM32 project CubeMX made, with nothing to set up, and blinks an LED without `HAL_Delay()`. Its README walks through an install, an update that keeps a setting you changed, an option, and an uninstall.

### Files that become yours

A library can bring files that belong to your project rather than to the library: a template of a port layer for you to fill in, a linker script section, a note for your own documentation. Each is a start, made for you to change, so it is created in your project's folder only when it is not there:

```
Files
  written example/src/example.h
  created Core/Src/example_port.c  yours from now on
```

From then on it is yours. An update never writes over it, and says so with `kept Core/Src/example_port.c  yours`. An uninstall leaves it where it is, with `kept Core/Src/example_port.c  yours, not removed`. It is not added to any IDE project either: it is your project's own file. STM32CubeIDE builds every file in its source folders, `Core/Src` among them, so there it is built with the rest. In a CMake, Keil or IAR project, which list their files one by one, add it the way you add a file of your own. A library can never put such a file over one libNIMA manages or CubeMX writes, such as `CMakeLists.txt`, `.cproject`, the `.ioc`, the Makefile or `stm32xxxx_hal_conf.h`, nor outside your project's folder, nor into a library's folder, whose next install would replace it.

### Updating a library

Run the same command again. Every file of the library is replaced, so an update updates, and a library you broke by accident is put right. What you wrote between `USER CODE BEGIN` and `USER CODE END` is kept, the way STM32CubeMX keeps your code when it generates again:

```c
/* USER CODE BEGIN EXAMPLE_CONFIGURATION */
#define EXAMPLE_SIZE  32
/* USER CODE END EXAMPLE_CONFIGURATION */
```

```
Files
  written example/example.h
  written example/example_config.h
  written example/example.c
  kept    example/example_config.h  your USER CODE EXAMPLE_CONFIGURATION

This was an update. The library replaced, what was yours kept.
```

Put your settings, and anything else you add to a library file, inside those sections. Outside them, the next install replaces it. When that would lose something of yours, the file as it was is saved beside it first, and the report says why:

```
  saved   example/example.c.20261006-165940.bak  it was changed outside its USER CODE sections
```

That happens when you edited the library outside a section, when its markers no longer pair, or when the new version no longer has a section you had written in. Take what you need from the `.bak` file, then delete it. A file nobody touched is left as it is, so your build does not compile it again for nothing.

When a new version keeps its files somewhere else, say in `src/`, the update moves your project along with it. Your `example_config.h` goes where the new version looks for it, with your settings between its `USER CODE` lines still in it, rather than a fresh default turning up next to the header. Files the old version installed and the new one no longer has are removed, so an old `example.c` cannot end up compiled beside the new one. Keil, IAR and the Makefile are pointed at the new places, and a file the library gained joins its group there. Anything you put in the library's folder yourself is left alone.

```
Files
  written example/src/example.h
  written example/src/example_config.h
  written example/src/example.c
  moved   example/example_config.h -> example/src/example_config.h  your USER CODE EXAMPLE_CONFIGURATION went with it
  removed example/example.h  no longer part of the library
  removed example/example.c  no longer part of the library
```

### Uninstalling a library

```bash
libnima --uninstall example
```

From your project's root, or with `--project`. It takes the library out of every IDE project libNIMA put it in: its block in `CMakeLists.txt` and the `CMakeLists.txt` in its folder, its include path, source folder and defines in STM32CubeIDE's `.cproject`, its group, include path and defines in Keil and IAR, and its lines in the Makefile. Then its files go:

```
example 2.0.0
out of example

Project
  updated CMake  Removed example from CMakeLists.txt.
  updated STM32CubeIDE  Removed example from the include path, source folders and defines in .cproject.

Files
  removed example/example.h
  removed example/example.c
  saved   example/example_config.h.20261014-163211.bak  it was yours to edit

Done. example is out of the project.
```

What was yours is saved before it goes, as a `.bak` beside it: a configuration file the library created, and any file of the library you wrote in between its `USER CODE` lines or changed by hand, its extras and tools among them. A file you put in the library's folder yourself is left where it is, and the folder with it, and so are the [files that became yours](#files-that-become-yours). A define another installed library also asks for stays.

A library another one needs stays, and libNIMA says which needs it:

```
Error: spif needs osal, so it stays. Uninstall spif first, or name them all on one line.
```

Name them all, in any order, and each goes after what needs it: `libnima --uninstall osal spif`. When the library you took out brought another with it that nothing left in the project needs, the last line names it, with the command that takes it out too.

Uninstall works from `.libnima.json`, the record of what was installed and where. For each library it also keeps where it came from, down to the commit, the blocks written into your files, and the steps that ran. A library put in by hand, or a record that was deleted, leaves it nothing to go on.

### A library that needs another

Some libraries build on another one, such as `osal` for the RTOS. You install only the one you want, and what it needs goes in first:

```
example 2.0.0 needs osal >= 1.0.0, which is not in this project. Installing osal first.
...
Done. #include "osal.h" and you are away.
...
Done. #include "example.h" and you are away.
```

You are asked for its folder like any library. Install a second library that needs `osal`, and the one already in your project is used:

```
spif 3.0.0 needs osal >= 1.0.0. osal 1.0.0 is already in this project, kept.
```

Nothing is asked about it then. A library that goes in this way comes from the release the library needing it names, or else from its default branch. If a library needs a newer `osal` than the one you have, it is updated where it is, and your setting in `osal_config.h`, between its `USER CODE` lines, is kept as always. Everything is fetched before anything is installed, so when a library cannot be had, your project is left as it was.

### What you see

A first install from a downloaded zip, into a CubeMX project that builds with both CMake and STM32CubeIDE:

```
Reading example-main.zip ...

example 2.0.0
What this library does, in one line
middleware / system

Files
  written example/example.h
  written example/example.c
  written example/LICENSE.md
  written example/NOTICE
  written example/README.md
  created example/example_config.h  yours to edit

Project
  updated CMake  Added example to CMakeLists.txt, linked to ${CMAKE_PROJECT_NAME}.
          backup: CMakeLists.txt.20260923-193649.bak
  updated STM32CubeIDE  Added example to the include path and source folders in .cproject.
          Refresh the project in CubeIDE (F5) so it picks up the new files.
          backup: .cproject.20260923-193649.bak

Done. #include "example.h" and you are away.
```

The first line says where the library came from: `Fetching owner/example ...` from GitHub, `Reading ...` from a zip or a folder.

In a project with no IDE file libNIMA can read, the files go in all the same, with what to add to your build by hand, and the last line says so in place of `Done`:

```
Installed, and in no build yet: add it as said above, then #include "example.h".
```

---

## What it does for you

**Finds your IDE and wires the library in.** CMake, STM32CubeIDE, Keil MDK, IAR and CubeMX's Makefile are all handled, and a project can be more than one of them at once.

| IDE | What gets changed |
|---|---|
| CMake | The library gets its own `CMakeLists.txt`, and two lines are appended to yours |
| STM32CubeIDE | The include path, and the source folders where the project lists them, in every build configuration of `.cproject` |
| Keil MDK 5 and 6 | A file group and the include path in `.uvprojx` |
| IAR EWARM | A file group and the include path in `.ewp` |
| Makefile from CubeMX | The sources at the end of `C_SOURCES`, the include folder at the end of `C_INCLUDES` |

CubeMX can write its Makefile again when it generates code, and the library's lines go with it. Run libNIMA again after that and they are back.

**Never loses your settings.** What you write inside `USER CODE BEGIN` and `USER CODE END` survives every install. Reinstall as often as you like: the library is replaced, your sections are not, and anything else of yours that would be lost is saved to a `.bak` file first.

**Leaves the file looking like you wrote it.** An added line copies the indentation, the line ending and the path separator of the lines around it. These files go into version control, so a correct edit that shows up as a whole file diff is still a bad edit.

**Keeps your CMakeLists.txt short.** Your project gets two lines that never change, and the library's file list lives with the library:

```cmake
# >>> libnima: example >>>
add_subdirectory(example)
target_link_libraries(${CMAKE_PROJECT_NAME} PRIVATE example_lib)
# <<< libnima: example <<<
```

The target is called `example_lib` rather than `example`, because a project is often named after the library being tried out in it and CMake allows only one target per name. The `PRIVATE` keyword is dropped when your project links its libraries without one, since CMake refuses to mix the two forms.

The generated `example/CMakeLists.txt` declares an INTERFACE target, not a STATIC one. That matters: an INTERFACE target hands its sources to whoever links it, so they are compiled as part of your application and inherit its defines and include paths. That is what lets a driver's `.c` file find `main.h` and the HAL headers. A STATIC library would not see them and would fail to compile.

**Backs up before editing.** Every project file is copied to a timestamped `.bak` first. If the file is not one it recognises, it changes nothing and prints what to click instead.

**Edits the project, not a copy of it.** IAR leaves a `Backup of <name>.ewp` beside the real project when it upgrades one, and that copy is a valid project file whose name sorts first. The workspace file is asked which project is the real one, and backups are skipped.

**Keeps the repository out of your build.** CubeIDE compiles every `.c` under your project, and a library repository ships a test suite with its own `main()`. Left in place, that breaks your build with an error that points nowhere useful. So only the listed files are copied, and a repository that sits inside your project has the rest of what a repository holds removed from it, all but its `libnima.yml`. A git repository in your project is not installed where it stands at all, since its `.git` would go with the rest: install it from a copy outside the project.

**Tells you what the library needs.** It reads your `stm32xxxx_hal_conf.h` and `.ioc` and warns before you hit a confusing compile error:

```
Requirements
  check   HAL SPI module is not enabled
          Uncomment HAL_SPI_MODULE_ENABLED in stm32f4xx_hal_conf.h
  ok      GPIO is configured
  check   This library needs an RTOS and none was found
          It expects freertos. Enable it in CubeMX under Middleware.
```

It cannot switch a peripheral on for you. The `.ioc` belongs to CubeMX, and editing it behind CubeMX's back goes wrong quietly.

---

## For someone writing a library

Every key of `libnima.yml` is on [the format page](FORMAT.md), and what only STM32 reads is under [STM32](FORMAT.md#stm32). The window's second tab writes the file with you: **New** fills one in from what is in the library's folder, and `libnima --lint` checks one with no project. This is what is worth knowing about STM32 beyond the keys.

### Easy to get wrong

**A `.c` file is built only when it is listed.** A port layer under `sources` is built like any other source. Getting this wrong fails at link time with undefined references and no clue why.

**`kind: rtos` implies `provides: [rtos]`.** A library that is an operating system satisfies another library's RTOS requirement, whether or not you remembered to write it down.

**`LICENSE.md` and `NOTICE` travel with the code.** The Apache licence requires it, and the NOTICE file is what carries your attribution into someone else's product. They are never removed during cleanup.

That is everything the repository needs. There is no installer script to copy in: put the install lines from the top of this page in your README, with your library's name in them.
