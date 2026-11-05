# libnima.yml

`libnima.yml` stands at the root of a library's repository and says what the library is and what goes where when it is
installed. Any library can have one, whoever wrote it, and the one file serves every platform: each reads its own section
and leaves the others' alone.

`libnima --lint` checks the file in a folder with no project, and the window's second tab does the same as you type. Each
problem names its line.

The window's second tab is the easy way to write one: **New** fills a file in from what is in the library's folder, the form
has a place for every key below with a line about it, and the text beside it offers what can go where the cursor is. They are
one file, and a change in the form leaves the rest of it, comments included, as it was. Elsewhere,
[libnima.schema.json](libnima.schema.json) gives an editor the same help: with the YAML extension in VS Code, a file that
starts with this line has it.

```text
# yaml-language-server: $schema=https://raw.githubusercontent.com/nimaltd/libNIMA/main/docs/libnima.schema.json
```

## The smallest one

```yaml
name: example
platform: stm32
headers:
  - src/example.h
sources:
  - src/example.c
```

Only `name`, `platform` and `headers` or `sources` are required. Leave out what your library does not need.

## Every key

```yaml
name: example                      # the library's name, and its folder in the project
description: Driver for the X chip # one line, shown while installing
repository: https://github.com/owner/example  # where it comes from
license: Apache-2.0                # the licence's SPDX name
platform: [stm32, zephyr]          # one of stm32, zephyr, esp32, arduino and pio, or a list
version: 1.0.0                     # only when the first header has no @version tag
kind: driver                       # driver, middleware, rtos, protocol, filesystem, utility or bsp
category: sensor                   # sensor, storage, display, communication, wireless,
                                   # rtos, timing, power, input, math or system
provides: []                       # what it gives other libraries: [rtos] for an RTOS

requires:
  libnima: 1.0.0                   # the oldest libNIMA that reads this file
  libraries:                       # other libraries, installed first when missing
    - osal >= 1.0.0                # no owner means nimaltd; >= the oldest that will do
    - owner/lib                    # anyone else's, as owner/name
    - owner/lib@v1.2.0             # a tag, branch or commit, not its default branch
    - source: owner/other@v2.9.3   # a repository with no libnima.yml of its own,
      manifest: manifests/other.yml  # with one this library carries for it
      minimum: 2.9.0               # the oldest version that will do
      name: other                  # its name in the project, when not the repository's
  peripherals:                     # what the board has to have
    - type: spi                    # the peripheral
      count: 1                     # how many the library needs
      note: Master mode, 8 bit     # how to set it up, shown when it is missing
    - i2c                          # or its name alone: one, with no note

headers:                           # .h files; the first is the one to #include
  - src/example.h                  # every file keeps its path: example/src/example.h
  - src/example_config.h           # settings in USER CODE sections, kept on update
  - src/port/*.h                   # a wildcard: * inside one folder, ** across any number
sources:                           # .c files, added to the build
  - src/example.c
  - from: src/port/spi.c           # a file that goes somewhere of its own,
    to: port/spi.c                 # in the library's folder
include_dirs: [src]                # include folders, only when the guess is wrong
defines:                           # given to the whole build, NAME or NAME=value
  - EXAMPLE_FAST

options:                           # parts the user chooses at install
  littlefs:                        # the name typed after --with
    description: LittleFS file system on the flash  # the question asked
    libraries:                     # libraries this part needs, installed first
      - littlefs >= 2.11.0
    headers:
      - src/example_lfs.h
    sources:
      - src/example_lfs.c

once:                              # the project's own files: made when missing,
  - VERSION                        # then the user's, never replaced or removed
  - from: templates/example_port.c # one that lands somewhere else in the project
    to: Core/Src/example_port.c

extras:                            # copied as they are, never compiled
  - LICENSE.md                     # LICENSE.md and NOTICE are the default
  - NOTICE
  - from: docs/guide.md            # to a place in the library's folder
    to: guide.md
  - root: tools                    # into the project's folder, as the library's own

run:                               # steps run once the files are in, each asked about first
  - label: make-key                # the name typed after --run
    description: Make the product's key
    command: [imgtool, keygen, -k, product.key]  # the program, then each argument
    args: [-t, ecdsa-p256]         # more arguments, after the command
    unless: product.key            # skipped when this file is in the project already

stm32: {}                          # what one platform reads, below
zephyr: {}
esp32: {}
arduino: {}
pio: {}
```

An unknown key at the top of the file is a warning, so a newer file still reads. An unknown key under `requires` or in a
platform's section is refused, since a misspelt one would quietly do nothing.

### The version

Leave `version` out. libNIMA reads the version from the `@version` tag in the file comment of the first header listed, so it
is written in the code and nowhere else:

```c
/**
 * @file        example.h
 * @version     2.0.0
 */
```

### requires.libnima

The oldest libNIMA that reads the file correctly. It is checked before anything else in the file is read, so when a file
starts using something only a newer libNIMA understands, raising this makes an older one stop with a message saying to
update, rather than read the file wrongly without a word. Write it as three numbers, `1.0.0`.

### Where each file lands

Every file keeps its path in the library's folder. A `to` is another place in that folder, and a `to` alone names the file by
that path. A plain path under `headers`, `sources` or `extras` can also be a folder, which keeps its shape.

| Under | A `from` and a `to` | A `root` |
|---|---|---|
| `headers`, `sources` | a place in the library's folder | refused: code is built in the library's folder |
| `once` | a place in the project's folder | refused |
| `extras` | a place in the library's folder | a place in the project's folder, written again on every install and taken out by an uninstall |

No path may climb out of the library, or of the folder it lands in, however it is spelt. `{library}` names the folder the
library was installed in, wherever it was put, in a step's command, `unless` and a platform's CMake lines.

A `once` file cannot land in a library's folder, which every install of that library replaces: not in the folder this library
goes into, nor in that of a library it names, which `--lint` checks with the folders the question offers, nor in that of a
library the project has, which the install checks. Nor can it, or an extra with a `root`, land on a file another library has
in the project.

### USER CODE sections

Whatever the user writes between these two comments, in a file of the library's, survives every update. The rest of the file
is the library's and is replaced.

```c
/* USER CODE BEGIN EXAMPLE_CONFIGURATION */
#define EXAMPLE_SIZE  8
/* USER CODE END EXAMPLE_CONFIGURATION */
```

A section the new version no longer has, a hand edit outside the sections and markers that no longer pair are each saved
beside the file before it is replaced, and the run says so. An extra has no sections, even one that shows the markers: one
changed by hand is saved beside it the same way, before an install replaces it, or an update or an uninstall removes it.

### run

A step is shown in full and asked about before it runs, and `--run` and `--yes` say yes in advance. It runs in the project's
folder, as it is written, with no shell. It finds where things are in its environment:

| Variable | Holds |
|---|---|
| `LIBNIMA_PROJECT` | the project's folder |
| `LIBNIMA_LIBRARY` | the library's folder |
| `LIBNIMA_NAME`, `LIBNIMA_VERSION` | the library's |
| `LIBNIMA_PLATFORM` | the platform of the run, such as `zephyr` |
| `LIBNIMA` | `libnima` |

The same are there under `INSTALLER_`, for a program written for lib-installer. A step that ran before is not run again by an
update, unless it changed or `--run` asks, and that holds when its `unless` file is gone too: `--yes` does not make a second
key.

## Titles and notes

A list that goes into a file a person reads can be given in groups under their titles, and a line of it with a note after a
colon. The file written carries both as comments:

```yaml
zephyr:
  project:
    conf:
      What the release build leaves out:
        - LOG=n: no messages, and nowhere to send them
        - ASSERT=n
```

```
# ******************************************************************************
# What the release build leaves out
# ******************************************************************************
# no messages, and nowhere to send them
CONFIG_LOG=n
CONFIG_ASSERT=n
```

`library.json` and `library.properties` have no comments, so what goes into them has neither. A line of CMake takes a title
and no note: a line holding `: ` is CMake's own, and goes in quotes.

## STM32

```yaml
stm32:
  requires:
    hal: true                      # uses the STM32 HAL
    cmsis: false                   # uses CMSIS directly
    rtos: optional                 # none, optional, any, freertos, cmsis-os2 or threadx
    hal_modules: [spi]             # must be enabled in stm32xxxx_hal_conf.h, checked
    c_standard: c11                # the C standard the code is written for
```

`requires.peripherals` is checked against the `.ioc`. Nothing here refuses an install: what is missing is said, with what to do
about it. The library is registered with every IDE found in the project: CMake, STM32CubeIDE, Keil MDK, IAR Embedded Workbench
and a CubeMX Makefile, and `defines` reach each of them where `USE_HAL_DRIVER` already is.

## Zephyr

```yaml
zephyr:
  symbol:                          # the library's switch, CONFIG_EXAMPLE=y, the name in capitals when left out
    EXAMPLE: the driver, and its defaults while it is on  # with a note, or as symbol: EXAMPLE
  settings:                        # the library's own, in its Kconfig
    - name: EXAMPLE_SIZE
      type: int                    # bool, int, hex or string
      default: 8
      range: [1, 64]               # an int's or a hex's
      prompt: Bytes in one piece
      help: What menuconfig shows
  project:                         # the application's own files
    conf:                          # its block in prj.conf, beside the switch
      - LOG=n
    sysbuild:                      # its block in sysbuild.conf, as SB_CONFIG_
      - BOOTLOADER_MCUBOOT
    cmake:                         # its blocks in CMakeLists.txt, by place
      before-zephyr:
        - list(APPEND EXTRA_DTC_OVERLAY_FILE "{library}/dts/example.overlay")
      after-zephyr:
        - target_sources(app PRIVATE src/extra.c)
  library:                         # its own folder
    kconfig:                       # defaults in its Kconfig, while the switch is on
      - BT
    conf:                          # its Kconfig fragment, zephyr/example.conf, read after prj.conf
      - BT_AUTO_PHY_PERIPHERAL_2M
    cmake:                         # the lines of its own CMakeLists.txt
      - zephyr_library_link_libraries(mbedTLS)
```

A symbol is written in capitals, with or without `CONFIG_`: `I2C`, which is set to `y`, or `I2C=n`, `SIZE=8`, `SIZE=0x10`,
`NAME="text"`. An option takes `zephyr.project.cmake` and `zephyr.library.cmake`.

## ESP-IDF

```yaml
esp32:
  settings:                        # the library's own, in its Kconfig, as for Zephyr
    - name: EXAMPLE_SIZE
      type: int
      default: 8
  project:                         # the project's own files
    conf:                          # its block in sdkconfig.defaults
      - FREERTOS_HZ=1000
    cmake:                         # its blocks in CMakeLists.txt, by place
      before-project:
        - list(APPEND SDKCONFIG_DEFAULTS "{library}/example.defaults")
      after-project:
        - idf_component_get_property(main_lib main COMPONENT_LIB)
  library:                         # its component
    requires: [esp_timer]          # REQUIRES of its component, with no titles or notes
    cmake:                         # its component's CMakeLists.txt
      - target_link_libraries(${COMPONENT_LIB} PRIVATE m)
```

An option takes `esp32.project.cmake` and `esp32.library.cmake`.

## Arduino

```yaml
arduino:                           # what goes into library.properties
  architectures: [avr, esp32]      # * when left out
  category: Sensors                # one of Arduino's, or the nearest to category above
  author: Someone                  # the repository's owner when left out
  maintainer: Someone <x@example.com>
  depends:                         # from Library Manager
    - Adafruit BusIO
    - ArduinoJson (>=6.0.0)
```

A library goes into the sketchbook's libraries folder, or a sketch's `src` folder. Arduino has no defines for a whole build, so
`defines` are said to have no place. In the libraries folder there is no project of a sketch's own, so `once` is refused and a
step is left unrun, with the way to get them: `--into sketch`.

## PlatformIO

```yaml
pio:
  project:                         # the project's platformio.ini
    build_flags:                   # after its defines, in its block of build_flags
      - -O2
  library:                         # its library.json, with no titles or notes
    frameworks: [arduino, espidf]  # * when left out
    platforms: [espressif32]       # * when left out
    dependencies:                  # from the PlatformIO Registry
      - bblanchon/ArduinoJson@^7
      - Wire
```

## How the file is read

A file is read as YAML 1.1 reads it, on what it holds:

- mappings and lists, by indentation, a list at the depth of its key included
- `[a, b]` and `{a: b}`, which have to close on their line
- `'it''s'` and `"a\tb"`, each with what its kind of quote escapes
- `|` and `>` for a block of text, with `-` and `+` for what happens at its end

What it does not hold is an error naming the line, rather than a guess: anchors, aliases and tags, a second document, quotes
or brackets that go on over several lines. A key given twice in one place is refused, where YAML would keep the last without a
word. A value that starts with `*`, `&`, `!`, `%`, `@`, `{` or `[`, such as `'*.c'` or `'{library}/tools'`, goes in quotes.

## From lib-installer

A repository with only an `installer.yml` asking for lib-installer 3.x or 4.x is read through a built-in converter, so nothing
has to change on the day libNIMA arrives. `libnima --convert` writes the same out as `libnima.yml`, comments and order kept, and
leaves `installer.yml` for whoever still installs with lib-installer. Anything older is refused, with the reason.

Each old key is refused in a `libnima.yml`, with where it went:

| In `installer.yml` | In `libnima.yml` |
|---|---|
| `requires.lib_installer`, `requires.stm32_installer` and the like | `requires.libnima` |
| `files.headers`, `files.sources` | `headers`, `sources` |
| `install.defines`, `install.include_dirs` | `defines`, `include_dirs` |
| `install.layout` | gone: every file keeps its path |
| `requires.hal`, `cmsis`, `rtos`, `hal_modules`, `c_standard` | `stm32.requires` |
| `requires.kconfig` | `zephyr.project.conf`, `esp32.project.conf` |
| `run.script` | `run.command`, as `[python, tools/make_key.py]`: libNIMA has no Python of its own to run one with, so it needs Python on that machine |
| `zephyr.kconfig`, as one symbol | `zephyr.symbol` |
| `zephyr.defaults` | `zephyr.library.kconfig` |
| `zephyr.sysbuild` | `zephyr.project.sysbuild` |
| `zephyr.cmake` | `zephyr.project.cmake`, `zephyr.library.cmake` |
| `esp32.requires` | `esp32.library.requires` |
| `esp32.cmake` | `esp32.project.cmake`, `esp32.library.cmake` |
| `pio.frameworks`, `pio.platforms`, `pio.dependencies` | `pio.library` |
