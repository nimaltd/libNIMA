# libNIMA for Zephyr

Installs embedded C libraries into a Zephyr application, nRF Connect SDK ones included.

A library written for any project has no idea what a Zephyr module is. This copies it into your application, makes it a module, adds it to the build, and switches it on in `prj.conf`, so `west build` picks it up and its settings live in Kconfig like everything else in Zephyr.

This page is what libNIMA does in a Zephyr application, and what a library's `libnima.yml` can say for Zephyr. How to install libNIMA, every command, and what is the same on every platform are in the [README](../README.md).

## Installing a library

From your application's folder, the one holding `CMakeLists.txt` and `prj.conf`, install a library by its GitHub owner and name, then build as always:

```bash
libnima owner/example
west build -b nrf52840dk/nrf52840
```

To see it work first, take [lib-installer-example-zephyr](https://github.com/nimaltd/lib-installer-example-zephyr), a library made for trying libNIMA on. It builds in any Zephyr application, `native_sim` included, with its settings in Kconfig. Its README walks through an install, a setting changed in `prj.conf`, an option, and an uninstall. nRF Connect SDK applications are Zephyr applications, and install the same way.

Run from a folder whose `CMakeLists.txt` does not find Zephyr, or given a `--project` that is not there, libNIMA stops before writing anything and says so: a library put there would be in no build.

The library goes into `modules/<name>` when you press Enter at the folder question, or into the folder `--dir` gives. That folder is inside your application and its own: not the application's folder itself, and not another library's, nor one inside or around it. A library already installed stays where it is: to move it, take it out with `libnima --uninstall example`, then install it again into the new folder. A file of yours where a library's file would go is never written over: the install stops before anything is written, and names it.

### What happens

Take `example`, installed into an application called `app`:

```
app/
  CMakeLists.txt         one block added, above find_package(Zephyr)
  prj.conf               one block added, CONFIG_EXAMPLE=y and what it needs
  modules/example/
    src/example.h        the library's files, at the paths they have in its repository
    src/example.c
    zephyr/
      module.yml         written by libNIMA: the library as a Zephyr module
      CMakeLists.txt     its files, built when CONFIG_EXAMPLE is set
      Kconfig            CONFIG_EXAMPLE, the library's settings, and its defaults for others
      example.conf       its settings for every build, when its libnima.yml has zephyr.library.conf
```

The block in your `CMakeLists.txt` adds the library's folder to `ZEPHYR_EXTRA_MODULES`. That is how Zephyr is told about a module outside the west workspace, and it has to come before `find_package(Zephyr)`, which is when Zephyr looks:

```cmake
cmake_minimum_required(VERSION 3.20.0)
# >>> libnima: example >>>
# Added by libnima. Delete this block, and example's block in
# the Kconfig file, to drop the library from the build.
list(APPEND ZEPHYR_EXTRA_MODULES "${CMAKE_CURRENT_SOURCE_DIR}/modules/example")
# <<< libnima: example <<<

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
```

The block in `prj.conf` switches the library on, with the symbols it needs and a list of its settings:

```
# >>> libnima: example >>>
# Added by libnima, and written again on every install, so set
# example's settings below this block rather than in it.
# Its settings, as they are unless you set them:
#   CONFIG_EXAMPLE_POLL_CHUNK=32

CONFIG_I2C=y

# ******************************************************************************
# example
# ******************************************************************************
CONFIG_EXAMPLE=y
# <<< libnima: example <<<
```

A symbol you already set yourself, on or off, is not set again: in Kconfig the line read last wins, and it would quietly undo your choice. It is still written in the block, commented out, with the line that set it, so the block shows everything the library asked for:

```
# CONFIG_I2C=y  (left out: prj.conf sets CONFIG_I2C=n)
```

And libNIMA warns about each one before it writes anything:

```
Requirements
  check   prj.conf sets CONFIG_I2C=n, and example needs y. The block has it commented out
          Your line is the one used. Change it if the library is to work.
```

Change your line, or delete it and install again, and the block sets it.

A library can need more than these two blocks, and then it says so in its `libnima.yml`:

- **Sysbuild settings**, such as MCUboot switched on, go into a block of their own in `sysbuild.conf`, made when the application has none, with the same rules as the block in `prj.conf`. A library without them never touches `sysbuild.conf`.
- **Lines of CMake** it needs in your `CMakeLists.txt` go above `find_package(Zephyr)` or at the end of the file, wherever the library says, each place between two lines that name it:

```cmake
# >>> libnima: example/before-zephyr >>>
# From example's libnima.yml, and written again on every install.
list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_SOURCE_DIR}/example.conf")
# <<< libnima: example/before-zephyr <<<
```

- **Defaults for other symbols**, such as Bluetooth switched on, go into the library's own Kconfig rather than your `prj.conf`: they hold while the library is on, and a line of yours that sets the same symbol wins.
- **Settings for every build** that a default cannot make, such as a member of a choice, go into a Kconfig fragment of the library's own, `zephyr/example.conf` in its folder, which a block of its own in your `CMakeLists.txt` adds to `EXTRA_CONF_FILE`. Zephyr reads it after `prj.conf`, so a line of it wins over yours, and the install says so:

```cmake
# >>> libnima: example/zephyr.conf >>>
# example's Kconfig settings, from zephyr.library.conf in its libnima.yml,
# read after prj.conf. Written again on every install.
list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_SOURCE_DIR}/modules/example/zephyr/example.conf")
# <<< libnima: example/zephyr.conf <<<
```

A library that lists its lines in groups has each group's title written above it, between two lines of stars, and a note it gives a line above that line, so each file says why its lines are there:

```
# ******************************************************************************
# Bluetooth as fast as it goes
# ******************************************************************************
CONFIG_BT_AUTO_PHY_PERIPHERAL_2M=y
# one notification in each packet
CONFIG_BT_L2CAP_TX_MTU=247
```

Every block is written again on every install, so put your own lines outside them. An edit made inside one is noticed on the next install, and the file as it was is in the `.bak`.

Both files are copied aside before they change, as `CMakeLists.txt.20261013-170013.bak` and the like, and the files keep their own line endings. Installing twice changes nothing the second time.

The module is called `example_lib`, not `example`. Zephyr keeps one module per name, and one added by the application quietly takes the place of a module of the same name in the workspace: a library called `littlefs` would otherwise replace Zephyr's own littlefs.

### Settings

A library's settings are Kconfig symbols, set in `prj.conf` like any other, below libNIMA's block:

```
CONFIG_EXAMPLE_POLL_CHUNK=64
```

`west build -t menuconfig` shows them too, under the library's name. The C code reads them as `CONFIG_EXAMPLE_POLL_CHUNK`, which Zephyr gives every file of the build.

### Which Kconfig file

`prj.conf` is the one most applications have. When yours has more, such as a board's own file in `boards/`, libNIMA asks once which one gets the libraries:

```
This application has more than one Kconfig file: prj.conf, boards/nrf52840dk_nrf52840.conf
Kconfig file for the libraries [prj.conf]:
```

Press Enter for `prj.conf`. `--conf boards/nrf52840dk_nrf52840.conf` answers it on the command line, and a file that does not exist yet is created. One answer covers every library of that run, a library it needs included. On an update, a library stays in the file that already has its block, without a question. In a script, where nobody can answer, `prj.conf` is taken.

`sysbuild.conf` is never one of the choices. It holds sysbuild's settings, not the application's, so libNIMA neither asks about it nor reads it as one, and `--conf sysbuild.conf` is refused.

### Files that become yours

A library can bring files that belong to your application rather than to the library: a `VERSION`, a `CHANGELOG.md`, an overlay in `socs/` for each chip. Each is a start, made for you to change, so it is created in your application's folder only when it is not there:

```
Files
  written modules/example/src/example.h
  created VERSION  yours from now on
  created socs/nrf54l15_cpuapp.overlay  yours from now on
```

From then on it is yours. An update never writes over it, and says so with `kept VERSION  yours`. An uninstall leaves it where it is, with `kept VERSION  yours, not removed`. A library can never put such a file over one libNIMA manages, such as `CMakeLists.txt` or `prj.conf`, nor outside your application's folder, nor into a library's folder, whose next install would replace it.

### What you see

A first install from GitHub, into an application whose `prj.conf` turned printk off:

```
Fetching owner/example ...

example 1.2.0
Driver for the X chip
driver / storage

Requirements
  note    CONFIG_I2C=y goes into prj.conf
  check   prj.conf sets CONFIG_PRINTK=n, and example needs y
          Your line is kept. Change it if the library is to work.
  note    Needs I2C, 400 kHz
          Enable it in your board's devicetree, or in an overlay of the application.

Folder to install example into [modules/example]:

Files
  written modules/example/src/example.h
  written modules/example/src/example.c

Project
  updated Zephyr module  Wrote example_lib: zephyr/module.yml, zephyr/CMakeLists.txt, zephyr/Kconfig.
  updated CMakeLists.txt  Added example to CMakeLists.txt.
          backup: CMakeLists.txt.20261013-170013.bak
  updated prj.conf  Added example to prj.conf.
          backup: prj.conf.20261013-170014.bak

Done. #include "example.h" and you are away.
```

The hardware a library uses is yours to give it in the devicetree, which libNIMA names but does not change.

### Updating a library

Run the same command again. Every file of the library is replaced, so an update updates, and the module's files are written again from the new `libnima.yml`, so a source file the new version adds is built without you editing anything. A file that did not change is left alone, so the build does not configure again for nothing.

What you wrote between `USER CODE BEGIN` and `USER CODE END` in a library's file is kept. Outside those lines, the next install replaces it, and when that would lose something of yours the file is saved beside it first, as a `.bak`. So is an extra you changed by hand, such as the library's `README.md` or a tool it put in `tools/`, which has no such lines.

The record of what is installed, and where, is `.libnima.json` in your application's folder. For each library it also keeps where it came from, down to the commit, the blocks written into your files, and the steps that ran. Keep it in version control with the rest.

### Uninstalling a library

```bash
libnima --uninstall example
```

From your application's folder, or with `--project`. Its blocks go from `CMakeLists.txt`, from the Kconfig file that has its block and from `sysbuild.conf`, the module files and the Kconfig fragment libNIMA wrote go from its folder, and then the library's own files. Installed and then uninstalled, `CMakeLists.txt` and `prj.conf` are back to the byte they were. `sysbuild.conf` stays, even when the block was all it held, and so do the [files that became yours](#files-that-become-yours).

What was yours is saved before it goes, as a `.bak` beside it: any file of the library you wrote in between its `USER CODE` lines or changed by hand, its extras and tools among them. A file you put in the library's folder yourself is left where it is, and the folder with it.

A setting you set yourself, below libNIMA's block, is your line, and it stays. The symbol it sets went with the library, though, and Zephyr stops on a symbol it does not know, so libNIMA names every such line:

```
  manual  prj.conf  prj.conf still sets what came with example, and Zephyr stops on a symbol it does not know. Take these lines out:
          CONFIG_EXAMPLE_POLL_CHUNK=64
```

A library another one needs stays, and libNIMA says which needs it. Name them all, in any order, and each goes after what needs it: `libnima --uninstall osal ee24`. When the library you took out brought another with it that nothing left needs, the last line names it, with the command that takes it out too. Uninstall works from `.libnima.json`, the record of what was installed and where.

### A library that needs another

You install only the one you want, and what it needs goes in first, each as a module of its own with its own blocks. It comes from the release the library names for it, or else from its default branch. One already in the application at a version that will do is left alone. Everything is fetched before anything is installed, so when a library cannot be had, your application is left as it was.

## For someone writing a library

Every key of `libnima.yml` is on [the format page](FORMAT.md), and what only Zephyr reads is under [Zephyr](FORMAT.md#zephyr). The window's second tab writes the file with you: **New** fills one in from what is in the library's folder, and `libnima --lint` checks one with no project. This is what is worth knowing about Zephyr beyond the keys.

### Settings of other symbols

A library that needs Bluetooth, MCUmgr or anything else of Zephyr switched on, or set to a value of its own, lists those settings under `zephyr.library.kconfig`, written as under `zephyr.project.conf`: a symbol alone is set to `y`, or `SYMBOL=value`.

```yaml
platform: zephyr
requires:
  libnima: 1.0.0
zephyr:
  symbol: GCORE
  library:
    kconfig:
      - BT
      - BT_PERIPHERAL
      - BT_L2CAP_TX_MTU=498
      - BT_DEVICE_NAME="gcore"
      - BT_CTLR_LE_PING=n
```

libNIMA writes each into the library's Kconfig as a `configdefault`, inside `if GCORE`, and the application's `prj.conf` gets only `CONFIG_GCORE=y`:

```
if GCORE

configdefault BT
	default y

configdefault BT_L2CAP_TX_MTU
	default 498
...
endif # GCORE
```

So the settings hold while the library is switched on, and only then. A line of the application's own that sets one of them wins: `CONFIG_BT_L2CAP_TX_MTU=247` in `prj.conf` is the value used, and the install says so. A setting whose dependencies are not met stays off, without a warning. The library's Kconfig is read before Zephyr's subsystems, so its defaults come before theirs, and win over them; only a board's or a SoC's `Kconfig.defconfig`, and a symbol Kconfig reads before the library's file, such as several of the nRF Connect SDK's, can come before it. Those go under [`zephyr.library.conf`](#settings-for-every-build).

A value is `y`, `n`, a number, `0x` and a hex number, or `"text"` with no quote or backslash in it. A symbol given twice, one also under `zephyr.project.conf`, or the library's own switch, is refused when the file is read. A symbol this Zephyr does not define stops the build, as it would in a `prj.conf`. A choice, such as which of several backends is used, cannot be set this way: it goes under `zephyr.library.conf`, into the library's Kconfig fragment, or under `zephyr.project.conf`, into the application's Kconfig file.

`zephyr.project.conf` and `zephyr.library.kconfig` are written alike and differ in where the line goes. `zephyr.project.conf` writes it into the application's Kconfig file, for the user to see and keep; `zephyr.library.kconfig` keeps it in the library, as a default. What neither can set goes under `zephyr.library.conf`, below.

### Settings for every build

Some settings cannot be defaults. A member of a choice, such as `BT_AUTO_PHY_PERIPHERAL_2M`, takes no `configdefault`. And a symbol the nRF Connect SDK defines before Kconfig reads the library's file keeps the default it was given there, which comes first: `BT_CTLR_SDC_MAX_CONN_EVENT_LEN_DEFAULT`, `SOC_FLASH_NRF_RADIO_SYNC_MPSL_NORMAL_PRIORITY_TIMEOUT_US` and `NCS_MCUBOOT_IMGTOOL_SET_ROM_FIXED_ADDRESS` are three. A library lists such settings under `zephyr.library.conf`, written as under `zephyr.library.kconfig`: a symbol alone is set to `y`, or `SYMBOL=value`:

```yaml
platform: zephyr
requires:
  libnima: 1.0.0
zephyr:
  symbol: GCORE
  library:
    conf:
      - BT_AUTO_PHY_PERIPHERAL_2M
      - BT_CTLR_SDC_MAX_CONN_EVENT_LEN_DEFAULT=4000000
      - SOC_FLASH_NRF_RADIO_SYNC_MPSL_NORMAL_PRIORITY_TIMEOUT_US=0
      - NCS_MCUBOOT_IMGTOOL_SET_ROM_FIXED_ADDRESS=n
```

libNIMA writes them into a Kconfig fragment of the library's own, `zephyr/gcore.conf` in its folder, a line each:

```
# Generated by libnima for gcore 1.0.0. Written again on
# every install and update, so a change made here does not last.
#
# gcore's settings, from zephyr.library.conf in its libnima.yml. The
# application's CMakeLists.txt adds this file to EXTRA_CONF_FILE, so Zephyr
# reads it after prj.conf, and a line here wins over one there.

CONFIG_BT_AUTO_PHY_PERIPHERAL_2M=y
CONFIG_BT_CTLR_SDC_MAX_CONN_EVENT_LEN_DEFAULT=4000000
CONFIG_SOC_FLASH_NRF_RADIO_SYNC_MPSL_NORMAL_PRIORITY_TIMEOUT_US=0
CONFIG_NCS_MCUBOOT_IMGTOOL_SET_ROM_FIXED_ADDRESS=n
```

And it adds the file to the build with a block of its own in the application's `CMakeLists.txt`, above `find_package(Zephyr)`, below the block that adds the library's folder. The file is named as `{library}` names the library's folder in a line of `zephyr.project.cmake`, wherever `--dir` put it:

```cmake
# >>> libnima: gcore/zephyr.conf >>>
# gcore's Kconfig settings, from zephyr.library.conf in its libnima.yml,
# read after prj.conf. Written again on every install.
list(APPEND EXTRA_CONF_FILE "${CMAKE_CURRENT_SOURCE_DIR}/modules/gcore/zephyr/gcore.conf")
# <<< libnima: gcore/zephyr.conf <<<
```

Zephyr merges the application's Kconfig files first, `prj.conf` and a board's own `.conf`, then what `EXTRA_CONF_FILE` lists, and of that what the application's `CMakeLists.txt` adds comes before a file given with `-DEXTRA_CONF_FILE`, on the command line or in `CMakePresets.json`, with sysbuild or without. So the fragment is read after `prj.conf` and before a debug build's `prj_debug.conf`, as the build's `Merged configuration` lines show:

```bash
west build -b nrf54l15dk/nrf54l15/cpuapp -d build-debug -- -DEXTRA_CONF_FILE=prj_debug.conf
```

```
Merged configuration '.../prj.conf'
Merged configuration '.../boards/nrf54l15dk_nrf54l15_cpuapp.conf'
Merged configuration '.../modules/gcore/zephyr/gcore.conf'
Merged configuration '.../prj_debug.conf'
```

Which means:

- Each line is an assignment. It holds over every default, those of the library's Kconfig and the nRF Connect SDK's alike, and sets a member of a choice.
- A line of the application's Kconfig files that sets the same symbol loses to it. Nothing in the build says so, so the install names each one before it writes anything:

```
Requirements
  check   prj.conf sets CONFIG_BT_CTLR_SDC_MAX_CONN_EVENT_LEN_DEFAULT=7500, and gcore's zephyr.library.conf sets 4000000
          zephyr/gcore.conf is read after prj.conf, so its line is the one used. A file given with -DEXTRA_CONF_FILE is read after it.
```

- A file given with `-DEXTRA_CONF_FILE`, such as `prj_debug.conf` above, is read after it, and can still change any of its lines for that build.
- The lines hold for as long as the library is installed, not only while it is switched on: the block stays in `CMakeLists.txt` with `CONFIG_GCORE` off in `prj.conf`.

The fragment and its block are written again on every install. An update whose `libnima.yml` no longer has `zephyr.library.conf` takes both out, and so does an uninstall, which gives `CMakeLists.txt` back as it was. A library with its own `zephyr/module.yml` gets the fragment too.

A value is `y`, `n`, a number, `0x` and a hex number, or `"text"` with no quote or backslash in it. A symbol given twice, the library's own switch, a symbol also under `zephyr.project.conf` or `zephyr.library.kconfig`, which `zephyr.library.conf` would quietly undo, and a file of the library's own at the fragment's place are refused when the file is read. A symbol this Zephyr does not define stops the build, as it would in a `prj.conf`.

| Key | Where the line goes | A line of the application's own that sets the symbol |
|---|---|---|
| `zephyr.project.conf` | the application's Kconfig file, in the library's block | wins: the block has the library's line commented out |
| `zephyr.library.kconfig` | the library's `zephyr/Kconfig`, a `configdefault` while it is switched on | wins, and so does a default Kconfig reads before the library's |
| `zephyr.library.conf` | the library's `zephyr/<name>.conf`, read after `prj.conf` | loses, unless it is in a file given with `-DEXTRA_CONF_FILE` |

So a setting the application is to keep and change goes under `zephyr.project.conf` or `zephyr.library.kconfig`, and one the library needs in every build, whatever `prj.conf` says, under `zephyr.library.conf`.

### Titles and notes

Every list under `zephyr.project` and `zephyr.library` can be given in groups, each under its title, and a setting with a note. libNIMA writes both into the file the list goes into, as comments, so whoever reads the application's `prj.conf`, or the library's fragment, sees why each line is there:

```yaml
zephyr:
  symbol:
    GCORE: builds gcore, and its defaults hold while it is on
  project:
    conf:
      The smallest code, with nothing to print:
        - SIZE_OPTIMIZATIONS
        - LOG=n: no messages, and nowhere to send them
        - PRINTK=n
  library:
    conf:
      Bluetooth as fast as it goes:
        - BT_AUTO_PHY_PERIPHERAL_2M
        - BT_L2CAP_TX_MTU=247: one notification in each packet
```

The block in `prj.conf`:

```
# >>> libnima: gcore >>>
# Added by libnima, and written again on every install, so set
# gcore's settings below this block rather than in it.

# ******************************************************************************
# The smallest code, with nothing to print
# ******************************************************************************
CONFIG_SIZE_OPTIMIZATIONS=y
# no messages, and nowhere to send them
CONFIG_LOG=n
CONFIG_PRINTK=n

# ******************************************************************************
# gcore
# ******************************************************************************
# builds gcore, and its defaults hold while it is on
CONFIG_GCORE=y
# <<< libnima: gcore <<<
```

And `zephyr/gcore.conf`, below its own lines of comment:

```
# ******************************************************************************
# Bluetooth as fast as it goes
# ******************************************************************************
CONFIG_BT_AUTO_PHY_PERIPHERAL_2M=y
# one notification in each packet
CONFIG_BT_L2CAP_TX_MTU=247
```

- A list is either plain, or a mapping of titles to their lists. A plain list is written with no title.
- A title is one line, written between two lines of stars, 80 columns wide, above its group, and a group after the first starts after a blank line. The same holds in `sysbuild.conf`, in the library's `Kconfig`, above the `configdefault`s of its group, and in the lines of CMake, in the application's file and the module's.
- A note follows its setting after a colon, `LOG=n: no messages`, and is written as a comment of its own above the setting's line, since Kconfig takes no comment at the end of one. A setting the application sets itself keeps its note, above the line left out.
- The library's switch, `CONFIG_GCORE=y`, is always last in the block, a group of its own under the library's name, between two lines of stars. Its note comes from `zephyr.symbol`, given as a setting's is, after a colon, on a line of its own below `symbol:`, as above, since YAML takes no second `: ` on the line of `symbol:` itself. The switch is never listed among the settings, under `zephyr.project` or `zephyr.library`: such a file is refused.
- A line of CMake takes a title but no note: a line holding `: ` is CMake's own, and goes in quotes, so that YAML reads it as a line.
- A title or a note holding `: ` goes in quotes too, as any YAML text does.
- A title or a note is one line, and none may hold what libNIMA marks its blocks with, `>>>`, `<<<` or `USER CODE`, nor read as a setting, as `CONFIG_X is not set` does. Such a file is refused when it is read.
- Two groups with one title are refused, as is any key given twice: YAML would keep only the last, and drop the other's lines without a word.
- A comment of YAML's own, after a `#`, stays in `libnima.yml`, for whoever reads that file.

### More CMake for a library

A library that needs more than its files and its defines, a library of Zephyr's linked in, an option for its compiler, or a line in the application's own `CMakeLists.txt`, lists the lines by the file they go into: those of its own `CMakeLists.txt` under `zephyr.library.cmake`, and those of the application's under `zephyr.project.cmake`, under the place each goes:

```yaml
zephyr:
  project:
    cmake:
      before-zephyr:
        - list(APPEND EXTRA_DTC_OVERLAY_FILE "{library}/dts/example.overlay")
      after-zephyr:
        - 'message(STATUS "example: built for ${BOARD}")'
  library:
    cmake:
      - zephyr_library_link_libraries(mbedTLS)
      - zephyr_library_compile_options(-O2)
```

| Key | Written into | For |
|---|---|---|
| `zephyr.library.cmake` | the module's `zephyr/CMakeLists.txt`, inside `if(CONFIG_EXAMPLE)`, after the library's files | `zephyr_library_...` calls on the library itself, counted only while it is switched on |
| `zephyr.project.cmake.before-zephyr` | the application's `CMakeLists.txt`, above `find_package(Zephyr)`, below the library's `ZEPHYR_EXTRA_MODULES` block | what Zephyr reads as it starts: `EXTRA_CONF_FILE`, `EXTRA_DTC_OVERLAY_FILE`, `SNIPPET`. No `CONFIG_` value exists yet |
| `zephyr.project.cmake.after-zephyr` | the application's `CMakeLists.txt`, at its end | what needs the application's own targets, such as `app` |

The lines of each place are written as one block, between two lines that name the library and the place, `module`, `before-zephyr` or `after-zephyr`, `# >>> libnima: example/before-zephyr >>>` and its pair, in the file's order. A list given in groups has each group's title among its lines, between two lines of stars, see [Titles and notes](#titles-and-notes):

```cmake
# >>> libnima: example/before-zephyr >>>
# From example's libnima.yml, and written again on every install.
list(APPEND EXTRA_DTC_OVERLAY_FILE "${CMAKE_CURRENT_SOURCE_DIR}/modules/example/dts/example.overlay")
# <<< libnima: example/before-zephyr <<<
```

Every install writes them again: a place whose lines changed is updated, and one the file no longer has is taken out. A line keeps the spaces it starts with, so the body of an `if()` can be indented. `{library}` in a line names the library's folder, as `${CMAKE_CURRENT_SOURCE_DIR}/modules/example` in the application's file and `${ZEPHYR_CURRENT_MODULE_DIR}` in the module's, wherever `--dir` put it; anything else in a line is CMake's own, written as it stands. A line holding a colon, as `message()` often does, goes in quotes, or YAML reads it as a mapping.

An option brings lines of its own under `options.<name>.zephyr.project.cmake` and `options.<name>.zephyr.library.cmake`, the same way, written while it is chosen and taken out by the update that turns it off. Its blocks are named after it and the place, `example/littlefs-module`, so they never meet the library's.

`zephyr_library_...` commands act on the library's own files only. A library of headers alone has no library of its own, so it uses the `zephyr_...` ones, which act on the whole build. The lines run when the user configures the build, the same as a module the library ships itself would, so they are the library's to answer for.

The user has a place in the module's file too. Its last lines are a `USER CODE` section, empty to start with, and every install keeps what is written in it, the way a configuration header keeps settings:

```cmake
  # USER CODE BEGIN EXAMPLE_CMAKE
  zephyr_library_compile_definitions(EXAMPLE_DEBUG=1)
  # USER CODE END EXAMPLE_CMAKE
endif()
```

On an uninstall, a section with something in it is saved as a `.bak` before the file goes.

### Sysbuild settings

MCUboot is an image of its own, built by sysbuild, and sysbuild does not see a module added through `ZEPHYR_EXTRA_MODULES`.

```yaml
zephyr:
  project:
    sysbuild:
      - BOOTLOADER_MCUBOOT
      - BOOT_SIGNATURE_TYPE_ECDSA_P256
      - BOOT_SIGNATURE_KEY_FILE="${APP_DIR}/sign_key.pem"
```

```
# >>> libnima: example >>>
# Added by libnima, and written again on every install, so set
# example's sysbuild settings below this block rather than in it.
SB_CONFIG_BOOTLOADER_MCUBOOT=y
SB_CONFIG_BOOT_SIGNATURE_TYPE_ECDSA_P256=y
SB_CONFIG_BOOT_SIGNATURE_KEY_FILE="${APP_DIR}/sign_key.pem"
# <<< libnima: example <<<
```

Each is read like a line of `zephyr.project.conf`, written without `SB_CONFIG_`, which libNIMA adds, and can be in groups, each under its title, with a note. A value is written as given: sysbuild expands `${APP_DIR}` itself. The block follows the rules of the one in `prj.conf`: a symbol the user sets outside it is left commented out in it, an uninstall takes it out, and `sysbuild.conf` is made when the application has none.

### Settings in the code

Zephyr hands every file of the build its configuration, so the library's code reads a setting as a macro. One header can serve Zephyr and any other project, taking the setting from Kconfig when it is there:

```c
#ifdef CONFIG_EXAMPLE_POLL_CHUNK
#define EXAMPLE_POLL_CHUNK          CONFIG_EXAMPLE_POLL_CHUNK
#endif

#ifndef EXAMPLE_POLL_CHUNK
#define EXAMPLE_POLL_CHUNK          32U
#endif
```

A `bool` setting that is off is not defined at all, which is how Kconfig does it, so test it with `#ifdef` or `IS_ENABLED()`. A `string` arrives in quotes, ready for C.

Each setting is checked at install, since all of it is written into a Kconfig file: a default of the wrong type, one outside its range, or a name Kconfig would refuse stops the install with the reason, before anything is written.

### A library with its own module

A library that ships a `zephyr/module.yml` of its own, listed under `extras`, `headers` or `sources` so it is installed, knows its build better than libNIMA does. Its module is used as it is: nothing is written into its `zephyr` folder but the Kconfig fragment of its `zephyr.library.conf`, when it has one, and only the blocks in the application are added.
