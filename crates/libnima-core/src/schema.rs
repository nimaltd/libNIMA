//! Every key a libnima.yml can hold: what it takes, and a line about it.
//!
//! The window's form, its completions and its help all read this table, and so does the JSON Schema an editor elsewhere can
//! use, so each of them says the same. The reader in `manifest` and the platforms stays what decides what a file means: the
//! tests hold this table to that reader and to docs/FORMAT.md, so the three cannot drift apart.

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::manifest::{CATEGORIES, KINDS};
use crate::yaml::split_key;

/// What a key takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// One line of text.
    Text,
    /// A word or a number, which YAML reads as it stands.
    Value,
    /// True or false.
    Flag,
    /// One of its choices.
    Choice,
    /// One of its choices, or a list of them.
    Choices,
    /// A list of words on one line: a program and its arguments.
    Words,
    /// A list of lines of text, which in a file a person reads can come under titles and with notes.
    Lines,
    /// A list of the library's files, each a path or a from and a to.
    Files,
    /// A list of mappings, with the keys below it.
    Rows,
    /// A mapping, with the keys below it.
    Section,
    /// A mapping of names the author gives, each with the keys below `*`.
    Named,
}

/// A key of the file. A path is keys joined by dots: `[]` is an item of a list, as in `run[].label`, and `*` is a name the
/// author gives, as in `options.*.description`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Key {
    pub path: &'static str,
    pub shape: Shape,
    /// What the form calls it.
    pub label: &'static str,
    /// One line about it.
    pub help: &'static str,
    /// What it can be, for the shapes that choose, and what to offer for the others.
    pub choices: &'static [&'static str],
    /// Whether what is not among the choices is fine too.
    pub open: bool,
    /// A value for it, as it stands in a file under the key.
    pub example: &'static str,
    /// What a new row or a new name starts with, as JSON.
    pub fresh: &'static str,
    /// Whether a list is written on one line when it is new.
    pub flow: bool,
    /// Whether a flag is on when it is left out.
    pub on: bool,
}

const BLANK: Key = Key {
    path: "",
    shape: Shape::Text,
    label: "",
    help: "",
    choices: &[],
    open: false,
    example: "",
    fresh: "",
    flow: false,
    on: false,
};

/// A part of the form, which holds these keys and what is below them.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Group {
    pub name: &'static str,
    pub keys: &'static [&'static str],
    /// The platform it is for, when only a library for that platform needs it.
    pub platform: Option<&'static str>,
}

pub const GROUPS: [Group; 10] = [
    Group {
        name: "About",
        keys: &[
            "name",
            "description",
            "platform",
            "repository",
            "license",
            "version",
            "kind",
            "category",
            "provides",
        ],
        platform: None,
    },
    Group {
        name: "Files",
        keys: &[
            "headers",
            "sources",
            "extras",
            "once",
            "include_dirs",
            "defines",
        ],
        platform: None,
    },
    Group {
        name: "Needs",
        keys: &["requires"],
        platform: None,
    },
    Group {
        name: "Options",
        keys: &["options"],
        platform: None,
    },
    Group {
        name: "Steps",
        keys: &["run"],
        platform: None,
    },
    Group {
        name: "STM32",
        keys: &["stm32"],
        platform: Some("stm32"),
    },
    Group {
        name: "Zephyr",
        keys: &["zephyr"],
        platform: Some("zephyr"),
    },
    Group {
        name: "ESP-IDF",
        keys: &["esp32"],
        platform: Some("esp32"),
    },
    Group {
        name: "Arduino",
        keys: &["arduino"],
        platform: Some("arduino"),
    },
    Group {
        name: "PlatformIO",
        keys: &["pio"],
        platform: Some("pio"),
    },
];

const PLATFORMS: &[&str] = &["stm32", "zephyr", "esp32", "arduino", "pio"];
const LICENSES: &[&str] = &[
    "Apache-2.0",
    "MIT",
    "BSD-3-Clause",
    "BSD-2-Clause",
    "GPL-3.0-only",
    "GPL-2.0-only",
    "LGPL-3.0-only",
    "LGPL-2.1-only",
    "MPL-2.0",
    "Zlib",
    "ISC",
    "Unlicense",
];
const PERIPHERALS: &[&str] = &[
    "adc", "can", "dac", "dma", "eth", "fdcan", "gpio", "i2c", "i2s", "qspi", "rtc", "sai", "sdio",
    "spi", "tim", "uart", "usart", "usb",
];
const HAL_MODULES: &[&str] = &[
    "adc", "can", "crc", "dac", "dma", "eth", "fdcan", "flash", "gpio", "i2c", "i2s", "iwdg",
    "pcd", "qspi", "rcc", "rng", "rtc", "sai", "sd", "spi", "tim", "uart", "usart", "wwdg",
];
const RTOS: &[&str] = &[
    "none",
    "optional",
    "any",
    "freertos",
    "cmsis-os2",
    "threadx",
];
const STANDARDS: &[&str] = &["c99", "c11", "c17", "c23", "gnu11", "gnu17"];
const SETTING_TYPES: &[&str] = &["bool", "int", "hex", "string"];
const ARCHITECTURES: &[&str] = &[
    "*",
    "avr",
    "esp32",
    "esp8266",
    "mbed_nano",
    "megaavr",
    "renesas_uno",
    "rp2040",
    "samd",
    "stm32",
];
const ARDUINO_CATEGORIES: &[&str] = &[
    "Display",
    "Communication",
    "Signal Input/Output",
    "Sensors",
    "Device Control",
    "Timing",
    "Data Storage",
    "Data Processing",
    "Other",
];
const FRAMEWORKS: &[&str] = &["*", "arduino", "cmsis", "espidf", "stm32cube", "zephyr"];
const PIO_PLATFORMS: &[&str] = &[
    "*",
    "atmelavr",
    "espressif32",
    "espressif8266",
    "nordicnrf52",
    "raspberrypi",
    "ststm32",
];

/// A row for what a setting of Kconfig holds, under Zephyr and ESP-IDF alike.
macro_rules! settings {
    ($settings:literal, $name:literal, $type:literal, $default:literal, $range:literal, $prompt:literal, $help:literal) => {
        [
            Key {
                path: $settings,
                shape: Shape::Rows,
                label: "Settings",
                help: "The library's own settings, in its Kconfig, which menuconfig shows.",
                example: "- name: EXAMPLE_SIZE\n  type: int\n  default: 8",
                fresh: r#"{"name": "EXAMPLE_SIZE", "type": "int", "default": 8}"#,
                ..BLANK
            },
            Key {
                path: $name,
                label: "Name",
                help: "The setting's name in capitals, without CONFIG_.",
                example: "EXAMPLE_SIZE",
                ..BLANK
            },
            Key {
                path: $type,
                shape: Shape::Choice,
                label: "Type",
                help: "What the setting holds.",
                choices: SETTING_TYPES,
                example: "int",
                ..BLANK
            },
            Key {
                path: $default,
                shape: Shape::Value,
                label: "Default",
                help: "What it is when nobody sets it.",
                example: "8",
                ..BLANK
            },
            Key {
                path: $range,
                shape: Shape::Words,
                label: "Range",
                help: "The lowest and the highest, for an int or a hex.",
                example: "[1, 64]",
                flow: true,
                ..BLANK
            },
            Key {
                path: $prompt,
                label: "Prompt",
                help: "The line menuconfig shows for it.",
                example: "Bytes in one piece",
                ..BLANK
            },
            Key {
                path: $help,
                label: "Help",
                help: "What menuconfig shows when asked about it.",
                example: "How many bytes one piece holds",
                ..BLANK
            },
        ]
    };
}

const ZEPHYR_SETTINGS: [Key; 7] = settings!(
    "zephyr.settings",
    "zephyr.settings[].name",
    "zephyr.settings[].type",
    "zephyr.settings[].default",
    "zephyr.settings[].range",
    "zephyr.settings[].prompt",
    "zephyr.settings[].help"
);

const ESP32_SETTINGS: [Key; 7] = settings!(
    "esp32.settings",
    "esp32.settings[].name",
    "esp32.settings[].type",
    "esp32.settings[].default",
    "esp32.settings[].range",
    "esp32.settings[].prompt",
    "esp32.settings[].help"
);

/// The keys, in the order a file gives them, which is where a new one goes.
pub static KEYS: &[Key] = &[
    Key {
        path: "name",
        label: "Name",
        help: "The library's name, and its folder in the project.",
        example: "example",
        ..BLANK
    },
    Key {
        path: "description",
        label: "Description",
        help: "One line about what it does, shown while it installs.",
        example: "Driver for the X chip",
        ..BLANK
    },
    Key {
        path: "repository",
        label: "Repository",
        help: "Where it comes from.",
        example: "https://github.com/owner/example",
        ..BLANK
    },
    Key {
        path: "license",
        shape: Shape::Choice,
        label: "License",
        help: "The licence's SPDX name.",
        choices: LICENSES,
        open: true,
        example: "Apache-2.0",
        ..BLANK
    },
    Key {
        path: "platform",
        shape: Shape::Choices,
        label: "Platforms",
        help: "The projects it works in.",
        choices: PLATFORMS,
        example: "[stm32, zephyr]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "version",
        label: "Version",
        help: "Only when the first header has no @version tag, which is where it is read from otherwise.",
        example: "1.0.0",
        ..BLANK
    },
    Key {
        path: "kind",
        shape: Shape::Choice,
        label: "Kind",
        help: "What sort of library it is.",
        choices: &KINDS,
        example: "driver",
        ..BLANK
    },
    Key {
        path: "category",
        shape: Shape::Choice,
        label: "Category",
        help: "What it is about, for listing it among others.",
        choices: &CATEGORIES,
        example: "sensor",
        ..BLANK
    },
    Key {
        path: "provides",
        shape: Shape::Lines,
        label: "Provides",
        help: "What it gives other libraries: rtos for an operating system.",
        choices: &["rtos"],
        open: true,
        example: "[rtos]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "requires",
        shape: Shape::Section,
        label: "Needs",
        help: "What it needs from libNIMA, other libraries and the board.",
        example: "libnima: 1.0.0",
        ..BLANK
    },
    Key {
        path: "requires.libnima",
        label: "libNIMA",
        help: "The oldest libNIMA that reads this file, three numbers.",
        example: "1.0.0",
        ..BLANK
    },
    Key {
        path: "requires.libraries",
        shape: Shape::Lines,
        label: "Libraries",
        help: "Other libraries, installed first when missing: osal >= 1.0.0, owner/lib or owner/lib@v1.2.0.",
        example: "- osal >= 1.0.0",
        ..BLANK
    },
    Key {
        path: "requires.libraries[].source",
        label: "Source",
        help: "A repository with no libnima.yml of its own, as owner/name.",
        example: "owner/other@v2.9.3",
        ..BLANK
    },
    Key {
        path: "requires.libraries[].manifest",
        label: "Its file",
        help: "The file this library carries for that repository.",
        example: "manifests/other.yml",
        ..BLANK
    },
    Key {
        path: "requires.libraries[].minimum",
        label: "Oldest version",
        help: "The oldest version of it that will do.",
        example: "2.9.0",
        ..BLANK
    },
    Key {
        path: "requires.libraries[].name",
        label: "Name",
        help: "Its name in the project, when that is not the repository's.",
        example: "other",
        ..BLANK
    },
    Key {
        path: "requires.peripherals",
        shape: Shape::Rows,
        label: "Peripherals",
        help: "What the board has to have, checked against the project where it can be.",
        example: "- type: spi\n  count: 1\n  note: Master mode, 8 bit",
        fresh: r#"{"type": "spi"}"#,
        ..BLANK
    },
    Key {
        path: "requires.peripherals[].type",
        shape: Shape::Choice,
        label: "Peripheral",
        help: "Which peripheral.",
        choices: PERIPHERALS,
        open: true,
        example: "spi",
        ..BLANK
    },
    Key {
        path: "requires.peripherals[].count",
        shape: Shape::Value,
        label: "How many",
        help: "How many the library needs, one when left out.",
        example: "1",
        ..BLANK
    },
    Key {
        path: "requires.peripherals[].note",
        label: "Note",
        help: "How to set it up, shown when it is missing.",
        example: "Master mode, 8 bit",
        ..BLANK
    },
    Key {
        path: "headers",
        shape: Shape::Files,
        label: "Headers",
        help: "Its .h files. The first is the one to #include, and its @version is the library's.",
        example: "- src/example.h",
        ..BLANK
    },
    Key {
        path: "headers[].from",
        label: "From",
        help: "The file, in the library.",
        example: "src/example.h",
        ..BLANK
    },
    Key {
        path: "headers[].to",
        label: "To",
        help: "Where it lands in the library's folder in the project.",
        example: "example.h",
        ..BLANK
    },
    Key {
        path: "sources",
        shape: Shape::Files,
        label: "Sources",
        help: "Its .c files, added to the build.",
        example: "- src/example.c",
        ..BLANK
    },
    Key {
        path: "sources[].from",
        label: "From",
        help: "The file, in the library.",
        example: "src/port/spi.c",
        ..BLANK
    },
    Key {
        path: "sources[].to",
        label: "To",
        help: "Where it lands in the library's folder in the project.",
        example: "port/spi.c",
        ..BLANK
    },
    Key {
        path: "include_dirs",
        shape: Shape::Lines,
        label: "Include folders",
        help: "Only when the guess from where the headers land is wrong.",
        example: "[src]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "defines",
        shape: Shape::Lines,
        label: "Defines",
        help: "Given to the whole build, NAME or NAME=value.",
        example: "- EXAMPLE_FAST",
        ..BLANK
    },
    Key {
        path: "options",
        shape: Shape::Named,
        label: "Options",
        help: "Parts the person installing chooses. Each name is what is typed after --with.",
        example: "littlefs:\n  description: LittleFS file system on the flash",
        fresh: r#"{"description": "What this part adds"}"#,
        ..BLANK
    },
    Key {
        path: "options.*.description",
        label: "Description",
        help: "The question asked about it.",
        example: "LittleFS file system on the flash",
        ..BLANK
    },
    Key {
        path: "options.*.libraries",
        shape: Shape::Lines,
        label: "Libraries",
        help: "Libraries this part needs, installed first.",
        example: "- littlefs >= 2.11.0",
        ..BLANK
    },
    Key {
        path: "options.*.headers",
        shape: Shape::Files,
        label: "Headers",
        help: "The .h files of this part.",
        example: "- src/example_lfs.h",
        ..BLANK
    },
    Key {
        path: "options.*.sources",
        shape: Shape::Files,
        label: "Sources",
        help: "The .c files of this part.",
        example: "- src/example_lfs.c",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr",
        shape: Shape::Section,
        label: "Zephyr",
        help: "This part's CMake lines in a Zephyr project.",
        example: "library:\n  cmake:\n    - zephyr_library_link_libraries(mbedTLS)",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr.project",
        shape: Shape::Section,
        label: "The application's files",
        help: "Lines for the application's CMakeLists.txt.",
        example: "cmake:\n  after-zephyr:\n    - target_sources(app PRIVATE src/extra.c)",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr.project.cmake",
        shape: Shape::Section,
        label: "CMakeLists.txt",
        help: "Its blocks in the application's CMakeLists.txt, by place.",
        example: "after-zephyr:\n  - target_sources(app PRIVATE src/extra.c)",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr.project.cmake.after-zephyr",
        shape: Shape::Lines,
        label: "After find_package(Zephyr)",
        help: "Lines for after Zephyr is found.",
        example: "- target_sources(app PRIVATE src/extra.c)",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr.project.cmake.before-zephyr",
        shape: Shape::Lines,
        label: "Before find_package(Zephyr)",
        help: "Lines for before Zephyr is found.",
        example: "- set(EXAMPLE_ON 1)",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr.library",
        shape: Shape::Section,
        label: "Its own folder",
        help: "Lines for the library's own CMakeLists.txt.",
        example: "cmake:\n  - zephyr_library_link_libraries(mbedTLS)",
        ..BLANK
    },
    Key {
        path: "options.*.zephyr.library.cmake",
        shape: Shape::Lines,
        label: "CMakeLists.txt",
        help: "The lines of the library's own CMakeLists.txt.",
        example: "- zephyr_library_link_libraries(mbedTLS)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32",
        shape: Shape::Section,
        label: "ESP-IDF",
        help: "This part's CMake lines in an ESP-IDF project.",
        example: "library:\n  cmake:\n    - target_link_libraries(${COMPONENT_LIB} PRIVATE m)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32.project",
        shape: Shape::Section,
        label: "The project's files",
        help: "Lines for the project's CMakeLists.txt.",
        example: "cmake:\n  after-project:\n    - idf_component_get_property(main_lib main COMPONENT_LIB)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32.project.cmake",
        shape: Shape::Section,
        label: "CMakeLists.txt",
        help: "Its blocks in the project's CMakeLists.txt, by place.",
        example: "after-project:\n  - idf_component_get_property(main_lib main COMPONENT_LIB)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32.project.cmake.before-project",
        shape: Shape::Lines,
        label: "Before project()",
        help: "Lines for before project().",
        example: "- set(EXAMPLE_ON 1)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32.project.cmake.after-project",
        shape: Shape::Lines,
        label: "After project()",
        help: "Lines for after project().",
        example: "- idf_component_get_property(main_lib main COMPONENT_LIB)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32.library",
        shape: Shape::Section,
        label: "Its component",
        help: "Lines for its component's CMakeLists.txt.",
        example: "cmake:\n  - target_link_libraries(${COMPONENT_LIB} PRIVATE m)",
        ..BLANK
    },
    Key {
        path: "options.*.esp32.library.cmake",
        shape: Shape::Lines,
        label: "CMakeLists.txt",
        help: "The lines of its component's CMakeLists.txt.",
        example: "- target_link_libraries(${COMPONENT_LIB} PRIVATE m)",
        ..BLANK
    },
    Key {
        path: "once",
        shape: Shape::Files,
        label: "The project's own",
        help: "Files made in the project when missing, then the person's: never replaced or removed.",
        example: "- VERSION",
        ..BLANK
    },
    Key {
        path: "once[].from",
        label: "From",
        help: "The file, in the library.",
        example: "templates/example_port.c",
        ..BLANK
    },
    Key {
        path: "once[].to",
        label: "To",
        help: "Where it lands in the project.",
        example: "Core/Src/example_port.c",
        ..BLANK
    },
    Key {
        path: "extras",
        shape: Shape::Files,
        label: "Extras",
        help: "Copied as they are, never compiled. LICENSE.md and NOTICE when it names none.",
        example: "- LICENSE.md",
        ..BLANK
    },
    Key {
        path: "extras[].from",
        label: "From",
        help: "The file or folder, in the library.",
        example: "docs/guide.md",
        ..BLANK
    },
    Key {
        path: "extras[].to",
        label: "To",
        help: "Where it lands in the library's folder in the project.",
        example: "guide.md",
        ..BLANK
    },
    Key {
        path: "extras[].root",
        label: "Into the project",
        help: "A file or folder for the project's folder, as the library's own.",
        example: "tools",
        ..BLANK
    },
    Key {
        path: "run",
        shape: Shape::Rows,
        label: "Steps",
        help: "Commands run once the files are in. Each is shown in full and asked about first.",
        example: "- label: make-key\n  command: [imgtool, keygen, -k, product.key]",
        fresh: r#"{"label": "new-step", "command": ["program"]}"#,
        ..BLANK
    },
    Key {
        path: "run[].label",
        label: "Label",
        help: "The name typed after --run: lower case letters, digits, _ and -.",
        example: "make-key",
        ..BLANK
    },
    Key {
        path: "run[].description",
        label: "Description",
        help: "What it does, shown beside the question.",
        example: "Make the product's key",
        ..BLANK
    },
    Key {
        path: "run[].command",
        shape: Shape::Words,
        label: "Command",
        help: "The program, then each argument. It runs as it is, with no shell.",
        example: "[imgtool, keygen, -k, product.key]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "run[].args",
        shape: Shape::Words,
        label: "More arguments",
        help: "Arguments that go after the command.",
        example: "[-t, ecdsa-p256]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "run[].unless",
        label: "Unless this exists",
        help: "Skipped when this file is in the project already.",
        example: "product.key",
        ..BLANK
    },
    Key {
        path: "stm32",
        shape: Shape::Section,
        label: "STM32",
        help: "What only an STM32 project reads.",
        example: "requires:\n  hal: true",
        ..BLANK
    },
    Key {
        path: "stm32.requires",
        shape: Shape::Section,
        label: "Needs",
        help: "What it needs of the project, checked before it goes in.",
        example: "hal: true",
        ..BLANK
    },
    Key {
        path: "stm32.requires.hal",
        shape: Shape::Flag,
        label: "Uses the HAL",
        help: "It uses the STM32 HAL. True when left out.",
        example: "true",
        on: true,
        ..BLANK
    },
    Key {
        path: "stm32.requires.cmsis",
        shape: Shape::Flag,
        label: "Uses CMSIS",
        help: "It uses CMSIS directly.",
        example: "false",
        ..BLANK
    },
    Key {
        path: "stm32.requires.rtos",
        shape: Shape::Choice,
        label: "RTOS",
        help: "How it relates to an operating system. none when left out.",
        choices: RTOS,
        example: "optional",
        ..BLANK
    },
    Key {
        path: "stm32.requires.hal_modules",
        shape: Shape::Lines,
        label: "HAL modules",
        help: "Must be enabled in stm32xxxx_hal_conf.h, and it is checked.",
        choices: HAL_MODULES,
        open: true,
        example: "[spi]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "stm32.requires.c_standard",
        shape: Shape::Choice,
        label: "C standard",
        help: "The C standard the code is written for. c11 when left out.",
        choices: STANDARDS,
        open: true,
        example: "c11",
        ..BLANK
    },
    Key {
        path: "zephyr",
        shape: Shape::Section,
        label: "Zephyr",
        help: "What only a Zephyr application reads.",
        example: "symbol: EXAMPLE",
        ..BLANK
    },
    Key {
        path: "zephyr.symbol",
        label: "Its switch",
        help: "CONFIG_EXAMPLE=y turns the library on. The name in capitals when left out.",
        example: "EXAMPLE",
        ..BLANK
    },
    ZEPHYR_SETTINGS[0],
    ZEPHYR_SETTINGS[1],
    ZEPHYR_SETTINGS[2],
    ZEPHYR_SETTINGS[3],
    ZEPHYR_SETTINGS[4],
    ZEPHYR_SETTINGS[5],
    ZEPHYR_SETTINGS[6],
    Key {
        path: "zephyr.project",
        shape: Shape::Section,
        label: "The application's files",
        help: "What it writes into the application's own files.",
        example: "conf:\n  - LOG=n",
        ..BLANK
    },
    Key {
        path: "zephyr.project.conf",
        shape: Shape::Lines,
        label: "prj.conf",
        help: "Its block in prj.conf, beside the switch: symbols in capitals, such as LOG=n.",
        example: "- LOG=n",
        ..BLANK
    },
    Key {
        path: "zephyr.project.sysbuild",
        shape: Shape::Lines,
        label: "sysbuild.conf",
        help: "Its block in sysbuild.conf, as SB_CONFIG_.",
        example: "- BOOTLOADER_MCUBOOT",
        ..BLANK
    },
    Key {
        path: "zephyr.project.cmake",
        shape: Shape::Section,
        label: "CMakeLists.txt",
        help: "Its blocks in the application's CMakeLists.txt, by place.",
        example: "after-zephyr:\n  - target_sources(app PRIVATE src/extra.c)",
        ..BLANK
    },
    Key {
        path: "zephyr.project.cmake.before-zephyr",
        shape: Shape::Lines,
        label: "Before find_package(Zephyr)",
        help: "Lines for before Zephyr is found.",
        example: "- list(APPEND EXTRA_DTC_OVERLAY_FILE \"{library}/dts/example.overlay\")",
        ..BLANK
    },
    Key {
        path: "zephyr.project.cmake.after-zephyr",
        shape: Shape::Lines,
        label: "After find_package(Zephyr)",
        help: "Lines for after Zephyr is found.",
        example: "- target_sources(app PRIVATE src/extra.c)",
        ..BLANK
    },
    Key {
        path: "zephyr.library",
        shape: Shape::Section,
        label: "Its own folder",
        help: "What it writes into its own folder in the application.",
        example: "kconfig:\n  - BT",
        ..BLANK
    },
    Key {
        path: "zephyr.library.kconfig",
        shape: Shape::Lines,
        label: "Kconfig defaults",
        help: "Defaults in its Kconfig, while the switch is on.",
        example: "- BT",
        ..BLANK
    },
    Key {
        path: "zephyr.library.conf",
        shape: Shape::Lines,
        label: "Kconfig fragment",
        help: "Its fragment, zephyr/example.conf, read after prj.conf.",
        example: "- BT_AUTO_PHY_PERIPHERAL_2M",
        ..BLANK
    },
    Key {
        path: "zephyr.library.cmake",
        shape: Shape::Lines,
        label: "CMakeLists.txt",
        help: "The lines of its own CMakeLists.txt.",
        example: "- zephyr_library_link_libraries(mbedTLS)",
        ..BLANK
    },
    Key {
        path: "esp32",
        shape: Shape::Section,
        label: "ESP-IDF",
        help: "What only an ESP-IDF project reads.",
        example: "library:\n  requires: [esp_timer]",
        ..BLANK
    },
    ESP32_SETTINGS[0],
    ESP32_SETTINGS[1],
    ESP32_SETTINGS[2],
    ESP32_SETTINGS[3],
    ESP32_SETTINGS[4],
    ESP32_SETTINGS[5],
    ESP32_SETTINGS[6],
    Key {
        path: "esp32.project",
        shape: Shape::Section,
        label: "The project's files",
        help: "What it writes into the project's own files.",
        example: "conf:\n  - FREERTOS_HZ=1000",
        ..BLANK
    },
    Key {
        path: "esp32.project.conf",
        shape: Shape::Lines,
        label: "sdkconfig.defaults",
        help: "Its block in sdkconfig.defaults: symbols in capitals, such as FREERTOS_HZ=1000.",
        example: "- FREERTOS_HZ=1000",
        ..BLANK
    },
    Key {
        path: "esp32.project.cmake",
        shape: Shape::Section,
        label: "CMakeLists.txt",
        help: "Its blocks in the project's CMakeLists.txt, by place.",
        example: "after-project:\n  - idf_component_get_property(main_lib main COMPONENT_LIB)",
        ..BLANK
    },
    Key {
        path: "esp32.project.cmake.before-project",
        shape: Shape::Lines,
        label: "Before project()",
        help: "Lines for before project().",
        example: "- list(APPEND SDKCONFIG_DEFAULTS \"{library}/example.defaults\")",
        ..BLANK
    },
    Key {
        path: "esp32.project.cmake.after-project",
        shape: Shape::Lines,
        label: "After project()",
        help: "Lines for after project().",
        example: "- idf_component_get_property(main_lib main COMPONENT_LIB)",
        ..BLANK
    },
    Key {
        path: "esp32.library",
        shape: Shape::Section,
        label: "Its component",
        help: "What goes into its component.",
        example: "requires: [esp_timer]",
        ..BLANK
    },
    Key {
        path: "esp32.library.requires",
        shape: Shape::Lines,
        label: "REQUIRES",
        help: "REQUIRES of its component, the components it uses.",
        choices: &[
            "driver",
            "esp_timer",
            "freertos",
            "nvs_flash",
            "esp_wifi",
            "esp_event",
        ],
        open: true,
        example: "[esp_timer]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "esp32.library.cmake",
        shape: Shape::Lines,
        label: "CMakeLists.txt",
        help: "Lines of its component's CMakeLists.txt.",
        example: "- target_link_libraries(${COMPONENT_LIB} PRIVATE m)",
        ..BLANK
    },
    Key {
        path: "arduino",
        shape: Shape::Section,
        label: "Arduino",
        help: "What goes into library.properties.",
        example: "architectures: [avr, esp32]",
        ..BLANK
    },
    Key {
        path: "arduino.architectures",
        shape: Shape::Lines,
        label: "Architectures",
        help: "The boards it works on. * when left out.",
        choices: ARCHITECTURES,
        open: true,
        example: "[avr, esp32]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "arduino.category",
        shape: Shape::Choice,
        label: "Category",
        help: "One of Arduino's. The nearest to category above when left out.",
        choices: ARDUINO_CATEGORIES,
        example: "Sensors",
        ..BLANK
    },
    Key {
        path: "arduino.author",
        label: "Author",
        help: "The repository's owner when left out.",
        example: "Someone",
        ..BLANK
    },
    Key {
        path: "arduino.maintainer",
        label: "Maintainer",
        help: "Who looks after it, with an address.",
        example: "Someone <x@example.com>",
        ..BLANK
    },
    Key {
        path: "arduino.depends",
        shape: Shape::Lines,
        label: "Depends on",
        help: "Libraries from Arduino's Library Manager.",
        example: "- ArduinoJson (>=6.0.0)",
        ..BLANK
    },
    Key {
        path: "pio",
        shape: Shape::Section,
        label: "PlatformIO",
        help: "What only a PlatformIO project reads.",
        example: "library:\n  frameworks: [arduino]",
        ..BLANK
    },
    Key {
        path: "pio.project",
        shape: Shape::Section,
        label: "The project's files",
        help: "What it writes into the project's platformio.ini.",
        example: "build_flags:\n  - -O2",
        ..BLANK
    },
    Key {
        path: "pio.project.build_flags",
        shape: Shape::Lines,
        label: "build_flags",
        help: "After its defines, in its block of build_flags.",
        example: "- -O2",
        ..BLANK
    },
    Key {
        path: "pio.library",
        shape: Shape::Section,
        label: "library.json",
        help: "What goes into its library.json.",
        example: "frameworks: [arduino]",
        ..BLANK
    },
    Key {
        path: "pio.library.frameworks",
        shape: Shape::Lines,
        label: "Frameworks",
        help: "The frameworks it works with. * when left out.",
        choices: FRAMEWORKS,
        open: true,
        example: "[arduino, espidf]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "pio.library.platforms",
        shape: Shape::Lines,
        label: "Platforms",
        help: "The PlatformIO platforms it works on. * when left out.",
        choices: PIO_PLATFORMS,
        open: true,
        example: "[espressif32]",
        flow: true,
        ..BLANK
    },
    Key {
        path: "pio.library.dependencies",
        shape: Shape::Lines,
        label: "Dependencies",
        help: "Libraries from the PlatformIO Registry.",
        example: "- bblanchon/ArduinoJson@^7",
        ..BLANK
    },
];

/// The key at a path of the table.
pub fn find(path: &str) -> Option<&'static Key> {
    KEYS.iter().find(|key| key.path == path)
}

/// Where a key stands among those a file gives, for putting a new one in its place.
pub fn order_of(path: &str) -> usize {
    KEYS.iter()
        .position(|key| key.path == path)
        .unwrap_or(usize::MAX)
}

/// The path above a path: `run[]` for `run[].label`, and nothing for a key at the top.
fn parent_of(path: &str) -> &str {
    path.rfind('.').map_or("", |at| &path[..at])
}

/// The keys right below a path.
pub fn children(path: &str) -> Vec<&'static Key> {
    KEYS.iter()
        .filter(|key| parent_of(key.path) == path)
        .collect()
}

/// Whether a key at the top of a file is one this table has, the same as the reader takes.
pub fn is_top_level(key: &str) -> bool {
    KEYS.iter().any(|known| known.path == key)
}

/// The path of a key below a path, with a name the author gave read as `*`.
pub fn below(path: &str, key: &str) -> String {
    if find(path).is_some_and(|found| found.shape == Shape::Named) {
        format!("{path}.*")
    } else if path.is_empty() {
        key.to_string()
    } else {
        format!("{path}.{key}")
    }
}

/// How many edits make one word the other.
fn distance(one: &str, other: &str) -> usize {
    let other: Vec<char> = other.chars().collect();
    let mut row: Vec<usize> = (0..=other.len()).collect();

    for (at, char) in one.chars().enumerate() {
        let mut last = row[0];

        row[0] = at + 1;

        for (index, against) in other.iter().enumerate() {
            let here = row[index + 1];

            row[index + 1] = if char == *against {
                last
            } else {
                1 + last.min(here).min(row[index])
            };
            last = here;
        }
    }

    row[other.len()]
}

/// The one of these that is nearest to a word, when one is near enough to be what was meant.
pub fn nearest<'a>(word: &str, among: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let lower = word.to_lowercase();

    among
        .into_iter()
        .map(|candidate| (distance(&lower, &candidate.to_lowercase()), candidate))
        .filter(|(far, candidate)| *far <= (candidate.len().max(lower.len()) / 3).max(1))
        .min_by_key(|(far, _)| *far)
        .map(|(_, candidate)| candidate)
}

/// A line of a file, as far as its keys go.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    indent: usize,
    dash: bool,
    /// Where the key stands, past the dash.
    key_indent: usize,
    key: Option<String>,
    /// What is after the colon, or the whole of what is after the dash.
    rest: String,
}

/// What a line holds, leaving out a line with nothing on it and a comment.
fn row_of(line: &str) -> Option<Row> {
    let line = line.trim_end_matches('\r');
    let indent = line.len() - line.trim_start_matches(' ').len();
    let body = &line[indent..];

    if body.trim().is_empty() || body.starts_with('#') {
        return None;
    }

    let (dash, key_indent, body) = if body == "-" {
        (true, indent + 2, "")
    } else if let Some(after) = body.strip_prefix("- ") {
        let inner = after.len() - after.trim_start_matches(' ').len();

        (true, indent + 2 + inner, after.trim_start_matches(' '))
    } else {
        (false, indent, body)
    };

    let body = crate::yaml::comment_start(body).map_or(body, |at| body[..at].trim_end());

    Some(match split_key(body) {
        Some((key, rest)) => Row {
            indent,
            dash,
            key_indent,
            key: Some(key),
            rest,
        },
        None => Row {
            indent,
            dash,
            key_indent,
            key: None,
            rest: body.to_string(),
        },
    })
}

/// The path of what a line holds, read from the indentation of the lines above it, so it works on a file that does not read
/// yet, as one being typed does not.
fn path_above(rows: &[Option<Row>], index: usize, level: usize, equal_counts: bool) -> String {
    let mut level = level;
    let mut equal_counts = equal_counts;
    let mut names: Vec<String> = Vec::new();

    for row in rows[..index].iter().rev().flatten() {
        if row.dash {
            match &row.key {
                Some(key) if row.key_indent < level => {
                    names.push(key.clone());
                    names.push("[]".to_string());
                    level = row.indent;
                    equal_counts = true;
                }
                _ if row.indent < level => {
                    names.push("[]".to_string());
                    level = row.indent;
                    equal_counts = true;
                }
                _ => {}
            }

            continue;
        }

        if let Some(key) = &row.key
            && (row.indent < level || (equal_counts && row.indent == level))
        {
            names.push(key.clone());
            level = row.indent;
            equal_counts = false;

            if level == 0 {
                break;
            }
        }
    }

    let mut path = String::new();

    for name in names.iter().rev() {
        if name == "[]" {
            path.push_str("[]");
        } else {
            path = below(&path, name);
        }
    }

    path
}

/// One thing to offer where the cursor is.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Completion {
    pub label: String,
    /// A word beside it.
    pub detail: String,
    /// The line about it.
    pub info: String,
    /// What is written for it.
    pub apply: String,
}

/// What to offer where the cursor is, and the column of the line it replaces from.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Completions {
    pub from: usize,
    pub items: Vec<Completion>,
}

fn is_header(path: &str) -> bool {
    matches!(
        crate::paths::suffix(path).as_str(),
        ".h" | ".hh" | ".hpp" | ".hxx" | ".inc"
    )
}

fn is_source(path: &str) -> bool {
    crate::manifest::COMPILED.contains(&crate::paths::suffix(path).as_str())
}

/// The files to offer for a key: what suits it, from those in the library's folder.
pub fn files_for(path: &str, files: &[String]) -> Vec<String> {
    let last = path.rsplit('.').next().unwrap_or(path);

    files
        .iter()
        .filter(|file| match last {
            "headers" => is_header(file),
            "sources" => is_source(file),
            _ => !file.starts_with('.'),
        })
        .cloned()
        .collect()
}

fn value_item(label: &str, key: &Key) -> Completion {
    Completion {
        label: label.to_string(),
        detail: String::new(),
        info: key.help.to_string(),
        apply: label.to_string(),
    }
}

/// What a value of a key can be.
fn values_of(key: &Key, files: &[String]) -> Vec<Completion> {
    match key.shape {
        Shape::Flag => ["true", "false"]
            .iter()
            .map(|word| value_item(word, key))
            .collect(),
        Shape::Files => files_for(key.path, files)
            .iter()
            .map(|file| value_item(file, key))
            .collect(),
        _ => key
            .choices
            .iter()
            .map(|word| value_item(word, key))
            .collect(),
    }
}

/// How a key is written when it is chosen, with the cursor left where its value goes.
fn key_text(key: &Key, name: &str, indent: usize) -> String {
    let inner = " ".repeat(indent + 2);

    match key.shape {
        Shape::Section | Shape::Named => format!("{name}:\n{inner}"),
        Shape::Lines | Shape::Files | Shape::Rows if !key.flow => format!("{name}:\n{inner}- "),
        _ => format!("{name}: "),
    }
}

fn shape_word(shape: Shape) -> &'static str {
    match shape {
        Shape::Text | Shape::Value => "text",
        Shape::Flag => "true or false",
        Shape::Choice | Shape::Choices => "choice",
        Shape::Words | Shape::Lines => "list",
        Shape::Files => "files",
        Shape::Rows => "rows",
        Shape::Section | Shape::Named => "section",
    }
}

/// What is already given beside a key in its mapping, so it is not offered again.
fn given_beside(rows: &[Option<Row>], index: usize, indent: usize) -> Vec<String> {
    let mut found = Vec::new();

    for row in rows[..index].iter().rev().flatten() {
        if row.key_indent == indent
            && let Some(key) = &row.key
        {
            found.push(key.clone());
        }

        if row.indent < indent {
            break;
        }
    }

    for row in rows[index + 1..].iter().flatten() {
        if row.indent < indent {
            break;
        }

        if row.key_indent == indent
            && !row.dash
            && let Some(key) = &row.key
        {
            found.push(key.clone());
        }
    }

    found
}

/// What to offer at a place in a file's text, the line counted from 1 and the column from 0. `files` are those in the
/// library's folder.
pub fn complete(text: &str, line: usize, column: usize, files: &[String]) -> Completions {
    let lines: Vec<&str> = text.split('\n').collect();
    let nothing = Completions {
        from: column,
        items: Vec::new(),
    };

    let Some(whole) = line.checked_sub(1).and_then(|at| lines.get(at)) else {
        return nothing;
    };

    let upto: String = whole.chars().take(column).collect();
    let rows: Vec<Option<Row>> = lines.iter().map(|one| row_of(one)).collect();
    let index = line - 1;
    let indent = upto.len() - upto.trim_start_matches(' ').len();
    let body = &upto[indent..];

    if body.starts_with('#') {
        return nothing;
    }

    let (dash, key_indent, body) = if let Some(after) = body.strip_prefix("- ") {
        let inner = after.len() - after.trim_start_matches(' ').len();

        (true, indent + 2 + inner, after.trim_start_matches(' '))
    } else {
        (false, indent, body)
    };
    let context = path_above(&rows, index, if dash { indent } else { key_indent }, dash);

    // After "key: ", a value of that key, the next of a list on one line included.
    if let Some((name, value)) = split_key(body) {
        let path = below(
            &if dash {
                format!("{context}[]")
            } else {
                context
            },
            &name,
        );
        let Some(key) = find(&path) else {
            return nothing;
        };
        let word = value
            .rfind(['[', ','])
            .map_or(value.as_str(), |at| value[at + 1..].trim_start());
        let from = upto.chars().count() - word.chars().count();

        return Completions {
            from,
            items: values_of(key, files),
        };
    }

    let from = upto.chars().count() - body.chars().count();

    // An item of a list: what the list holds, or the keys of a row.
    if dash {
        let list = find(&context);
        let row = format!("{context}[]");
        let mut items = list.map(|list| values_of(list, files)).unwrap_or_default();

        if items.is_empty() {
            items = key_items(&row, &[], key_indent);
        }

        return Completions { from, items };
    }

    let given = given_beside(&rows, index, key_indent);

    Completions {
        from,
        items: key_items(&context, &given, key_indent),
    }
}

/// The keys to offer below a path, leaving out those given.
fn key_items(path: &str, given: &[String], indent: usize) -> Vec<Completion> {
    let named = find(path).is_some_and(|key| key.shape == Shape::Named);

    if named {
        return Vec::new();
    }

    children(path)
        .into_iter()
        .filter_map(|key| {
            let name = key.path.rsplit('.').next().unwrap_or(key.path);

            (!given.iter().any(|one| one == name)).then(|| Completion {
                label: name.to_string(),
                detail: shape_word(key.shape).to_string(),
                info: key.help.to_string(),
                apply: key_text(key, name, indent),
            })
        })
        .collect()
}

/// What to say about the key at a place in a file's text.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Help {
    pub path: String,
    pub label: String,
    pub help: String,
    pub example: String,
    /// Where the key starts and ends on its line.
    pub from: usize,
    pub to: usize,
}

/// The help for the key under the cursor, the line counted from 1 and the column from 0.
pub fn help_at(text: &str, line: usize, column: usize) -> Option<Help> {
    let lines: Vec<&str> = text.split('\n').collect();
    let rows: Vec<Option<Row>> = lines.iter().map(|one| row_of(one)).collect();
    let index = line.checked_sub(1)?;
    let row = rows.get(index)?.as_ref()?;
    let name = row.key.as_ref()?;
    let (from, to) = (row.key_indent, row.key_indent + name.chars().count());

    if column < from || column > to {
        return None;
    }

    let context = path_above(
        &rows,
        index,
        if row.dash { row.indent } else { row.key_indent },
        row.dash,
    );
    let path = below(
        &if row.dash {
            format!("{context}[]")
        } else {
            context
        },
        name,
    );
    let key = find(&path)?;

    Some(Help {
        path,
        label: key.label.to_string(),
        help: key.help.to_string(),
        example: snippet(key),
        from,
        to,
    })
}

/// A key with its example, as it stands at its own depth in a file.
pub fn snippet(key: &Key) -> String {
    let last = key.path.rsplit('.').next().unwrap_or(key.path);
    let name = last.trim_end_matches("[]");

    if key.example.contains('\n')
        || key.example.starts_with("- ")
        || !matches!(
            key.shape,
            Shape::Text
                | Shape::Value
                | Shape::Flag
                | Shape::Choice
                | Shape::Choices
                | Shape::Words
                | Shape::Lines
        )
    {
        let body: Vec<String> = key
            .example
            .lines()
            .map(|line| format!("  {line}"))
            .collect();

        format!("{name}:\n{}", body.join("\n"))
    } else {
        format!("{name}: {}", key.example)
    }
}

/// The table as a JSON Schema, which editors elsewhere read for the same help as the window's.
pub fn json_schema() -> Value {
    let mut root = schema_below("");

    root["$schema"] = json!("http://json-schema.org/draft-07/schema#");
    root["$id"] =
        json!("https://raw.githubusercontent.com/nimaltd/libNIMA/main/docs/libnima.schema.json");
    root["title"] = json!("libnima.yml");
    root["description"] =
        json!("What a library is and what goes where when it is installed. See docs/FORMAT.md.");
    root["required"] = json!(["name", "platform"]);
    // A key it does not know at the top is only a warning, so a newer file still reads.
    root["additionalProperties"] = json!(true);

    root
}

/// An object with the keys below a path.
fn schema_below(path: &str) -> Value {
    let mut properties = Map::new();

    for key in children(path) {
        let name = key.path.rsplit('.').next().unwrap_or(key.path);

        properties.insert(name.to_string(), schema_of(key));
    }

    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": properties,
    })
}

/// A list of text, which can come under titles, each line of it with a note.
fn lines_schema(item: Value) -> Value {
    json!({
        "oneOf": [
            {
                "type": "array",
                "items": { "oneOf": [item, { "type": "object" }] },
            },
            { "type": "object" },
        ],
    })
}

fn choice_schema(key: &Key) -> Value {
    if key.open || key.choices.is_empty() {
        json!({ "type": "string" })
    } else {
        json!({ "enum": key.choices })
    }
}

fn schema_of(key: &Key) -> Value {
    let row = format!("{}[]", key.path);
    let mut found = match key.shape {
        Shape::Text => json!({ "type": ["string", "number", "boolean"] }),
        Shape::Value => json!({ "type": ["string", "number", "boolean"] }),
        Shape::Flag => json!({ "type": "boolean" }),
        Shape::Choice => choice_schema(key),
        Shape::Choices => {
            json!({ "oneOf": [choice_schema(key), { "type": "array", "items": choice_schema(key) }] })
        }
        Shape::Words => json!({ "type": "array", "items": { "type": ["string", "number"] } }),
        Shape::Lines => {
            let mut item = choice_schema(key);

            if !children(&row).is_empty() {
                item = json!({ "oneOf": [item, schema_below(&row)] });
            }

            lines_schema(item)
        }
        Shape::Files => json!({
            "type": "array",
            "items": { "oneOf": [{ "type": "string" }, schema_below(&row)] },
        }),
        Shape::Rows => {
            let mut item = schema_below(&row);

            // A peripheral can be its name alone.
            if key.path == "requires.peripherals" {
                item = json!({ "oneOf": [{ "type": "string" }, item] });
            }

            json!({ "type": "array", "items": item })
        }
        Shape::Section => schema_below(key.path),
        Shape::Named => json!({
            "type": "object",
            "additionalProperties": schema_below(&format!("{}.*", key.path)),
        }),
    };

    found["description"] = json!(key.help);

    found
}

/// The path a key in a mapping stands at, from the path of the mapping, for a walk over a file that reads.
pub fn walk_keys(
    node: &crate::yaml::Node,
    path: &str,
    visit: &mut dyn FnMut(&str, &crate::yaml::Entry),
) {
    use crate::yaml::Value as Yaml;

    match &node.value {
        Yaml::Map(map) => {
            for entry in &map.entries {
                let here = below(path, &entry.key);

                visit(&here, entry);

                if find(&here).is_some() || here.ends_with(".*") {
                    walk_keys(&entry.value, &here, visit);
                }
            }
        }
        Yaml::List(items) => {
            let row = format!("{path}[]");

            for item in items {
                walk_keys(item, &row, visit);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::lint_text;
    use crate::manifest;
    use crate::yaml::{self, Node, Value as Yaml};
    use std::collections::BTreeSet;

    /// A library's folder with every file the examples of the table name.
    fn folder() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();

        for file in [
            "src/example.h",
            "src/example.c",
            "src/example_lfs.h",
            "src/example_lfs.c",
            "src/port/spi.c",
            "src/extra.c",
            "VERSION",
            "templates/example_port.c",
            "docs/guide.md",
            "tools/x.py",
            "LICENSE.md",
            "manifests/other.yml",
        ] {
            let path = root.path().join(file);

            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        }

        root
    }

    /// What a row of a list has to hold whichever of its keys is being tried.
    fn rest_of_row(list: &str) -> &'static [&'static str] {
        match list {
            "requires.libraries" => &["source: owner/other@v2.9.3"],
            "requires.peripherals" => &["type: spi"],
            "run" => &["label: make-key", "command: [imgtool, keygen]"],
            "zephyr.settings" | "esp32.settings" => {
                &["name: EXAMPLE_SIZE", "type: int", "default: 8"]
            }
            "headers" => &["from: src/example.h"],
            "sources" => &["from: src/port/spi.c"],
            "once" => &["from: templates/example_port.c"],
            "extras" => &["from: docs/guide.md", "to: guide.md"],
            _ => &[],
        }
    }

    /// The whole of a file around one key's example, at the depth of its path.
    fn around(key: &Key) -> String {
        let mut text = snippet(key);
        let mut path = key.path;

        // A key of a row comes with the rest of what the row has to hold, unless it is one of them.
        if let Some((list, name)) = path.rsplit_once("[].") {
            for needed in rest_of_row(list) {
                let wanted = needed.split(':').next().unwrap();

                if wanted != name && !(list == "extras" && name == "root") {
                    text.push('\n');
                    text.push_str(needed);
                }
            }
        }

        // Up through the keys above it.
        while let Some(at) = path.rfind('.') {
            let parent = &path[..at];
            let item = parent.ends_with("[]");
            let name = parent
                .rsplit('.')
                .next()
                .unwrap_or(parent)
                .trim_end_matches("[]");
            let name = if name == "*" { "example" } else { name };
            let body: Vec<String> = text
                .lines()
                .enumerate()
                .map(|(index, line)| match (item, index) {
                    (true, 0) => format!("  - {line}"),
                    (true, _) => format!("    {line}"),
                    (false, _) => format!("  {line}"),
                })
                .collect();

            text = format!("{name}:\n{}", body.join("\n"));
            path = parent;
        }

        let top = key.path.split(['.', '[']).next().unwrap();
        let mut whole = String::new();

        if top != "name" {
            whole.push_str("name: example\n");
        }

        if top != "platform" {
            whole.push_str("platform: [stm32, zephyr, esp32, arduino, pio]\n");
        }

        if top != "headers" {
            whole.push_str("headers:\n  - src/example.h\n");
        }

        whole.push_str(&text);
        whole.push('\n');

        whole
    }

    #[test]
    fn every_example_of_the_table_reads_as_it_stands() {
        let root = folder();
        let path = root.path().join("libnima.yml");

        for key in KEYS {
            let text = around(key);
            let refused: Vec<String> = lint_text(root.path(), &path, &text)
                .into_iter()
                .filter(|problem| !problem.warning)
                .map(|problem| problem.message)
                .collect();

            assert!(refused.is_empty(), "{}:\n{text}\n{refused:#?}", key.path);
        }
    }

    #[test]
    fn a_key_at_the_top_is_known_to_the_table_and_the_reader_alike() {
        let root = folder();
        let path = root.path().join("libnima.yml");

        for key in KEYS.iter().filter(|key| parent_of(key.path).is_empty()) {
            let warned = lint_text(root.path(), &path, &around(key))
                .into_iter()
                .any(|problem| problem.message.contains("is no key"));

            assert!(!warned, "{} is read as unknown", key.path);
        }

        let text = "name: x\nplatform: stm32\nheaders:\n  - src/example.h\nsomething: 1\n";
        let warned = lint_text(root.path(), &path, text);

        assert!(
            warned
                .iter()
                .any(|problem| problem.message.contains("something is no key")),
            "{warned:#?}"
        );
        assert!(!is_top_level("something"));
    }

    /// Each key on the format page, as a path of the table.
    fn walk_page(node: &Node, path: &str, seen: &mut BTreeSet<String>, strange: &mut Vec<String>) {
        match &node.value {
            Yaml::Map(map) => {
                for entry in &map.entries {
                    let here = below(path, &entry.key);

                    match find(&here) {
                        Some(key) => {
                            seen.insert(here.clone());

                            // What is below a list of lines is its titles and notes, and a name given to a switch is the
                            // author's.
                            if key.shape != Shape::Lines
                                || matches!(entry.value.value, Yaml::List(_))
                            {
                                walk_page(&entry.value, &here, seen, strange);
                            }
                        }
                        // A name the author gives holds keys of the table below it.
                        None if find(path).is_some_and(|key| key.shape == Shape::Named) => {
                            walk_page(&entry.value, &here, seen, strange);
                        }
                        None if path == "zephyr.symbol" => {}
                        None => strange.push(here),
                    }
                }
            }
            Yaml::List(items) => {
                let row = format!("{path}[]");

                for item in items {
                    if item.as_map().is_some() && !children(&row).is_empty() {
                        walk_page(item, &row, seen, strange);
                    }
                }
            }
            _ => {}
        }
    }

    #[test]
    fn the_table_and_the_format_page_name_the_same_keys() {
        let page = include_str!("../../../docs/FORMAT.md");
        let mut seen = BTreeSet::new();
        let mut strange = Vec::new();

        for block in page.split("```yaml\n").skip(1) {
            let text = block.split("```").next().unwrap();
            let node = yaml::loads(text).unwrap().unwrap();

            walk_page(&node, "", &mut seen, &mut strange);
        }

        assert!(
            strange.is_empty(),
            "on the page and not in the table: {strange:#?}"
        );

        // What the page says in a sentence, as being the same as what it shows beside it.
        let in_a_sentence = [
            "options.*.zephyr",
            "options.*.esp32",
            "headers[]",
            "esp32.settings[]",
        ];
        let missing: Vec<&str> = KEYS
            .iter()
            .map(|key| key.path)
            .filter(|path| {
                !seen.contains(*path) && !in_a_sentence.iter().any(|said| path.starts_with(said))
            })
            .collect();

        assert!(
            missing.is_empty(),
            "in the table and not on the page: {missing:#?}"
        );
    }

    /// What a guide's example has for keys that the table does not: each line of a list of lines can be a title, so only what
    /// the table has a place for is held to it.
    #[test]
    fn the_examples_in_the_guides_use_the_keys_of_the_table() {
        let mut strange = Vec::new();

        for (name, page) in [
            ("stm32", include_str!("../../../docs/stm32.md")),
            ("zephyr", include_str!("../../../docs/zephyr.md")),
            ("esp32", include_str!("../../../docs/esp32.md")),
            ("arduino", include_str!("../../../docs/arduino.md")),
            ("pio", include_str!("../../../docs/pio.md")),
        ] {
            for block in page.split("```yaml\n").skip(1) {
                let text = block.split("```").next().unwrap();
                let node = match yaml::loads(text) {
                    Ok(Some(node)) => node,
                    Ok(None) => continue,
                    Err(error) => panic!("{name}: {error}\n{text}"),
                };
                let mut seen = BTreeSet::new();
                let mut found = Vec::new();

                // An example can start part of the way down: below a key the text before it names.
                if node.as_map().is_some() || node.as_list().is_some() {
                    walk_page(&node, "", &mut seen, &mut found);
                }

                strange.extend(found.into_iter().map(|key| format!("{name}: {key}")));
            }
        }

        assert!(
            strange.is_empty(),
            "keys that are not in the table: {strange:#?}"
        );
    }

    #[test]
    fn the_choices_of_the_table_are_those_the_reader_takes() {
        let root = folder();
        let path = root.path().join("libnima.yml");

        // A setting's default depends on its type, which has a test of its own below.
        for key in KEYS.iter().filter(|key| {
            key.shape == Shape::Choice && !key.open && !key.path.ends_with("settings[].type")
        }) {
            for choice in key.choices {
                let found = Key {
                    example: choice,
                    ..*key
                };
                let problems = lint_text(root.path(), &path, &around(&found));

                assert!(problems.is_empty(), "{} {choice}: {problems:#?}", key.path);
            }
        }
    }

    #[test]
    fn a_setting_can_be_each_of_the_types_the_table_offers() {
        let root = folder();
        let path = root.path().join("libnima.yml");

        for (kind, default) in [
            ("bool", "true"),
            ("int", "8"),
            ("hex", "0x10"),
            ("string", "text"),
        ] {
            assert!(SETTING_TYPES.contains(&kind));

            let text = format!(
                "name: x\nplatform: [zephyr, esp32]\nheaders:\n  - src/example.h\n\
                 zephyr:\n  settings:\n    - name: X_SIZE\n      type: {kind}\n      default: {default}\n"
            );
            let refused: Vec<_> = lint_text(root.path(), &path, &text)
                .into_iter()
                .filter(|problem| !problem.warning)
                .collect();

            assert!(refused.is_empty(), "{kind}: {refused:#?}");
        }

        assert_eq!(SETTING_TYPES.len(), 4);
    }

    #[test]
    fn the_path_of_a_line_is_read_from_the_lines_above_it() {
        let text = "name: x\n\
                    # a comment\n\
                    requires:\n  libraries:\n    - osal\n  peripherals:\n    - type: spi\n      count: 1\n\
                    headers:\n- a.h\n- from: b.h\n  to: c.h\n\
                    options:\n  lfs:\n    headers:\n      - x.h\n\
                    zephyr:\n  project:\n    cmake:\n      after-zephyr:\n        - y\n";
        let lines: Vec<&str> = text.lines().collect();
        let rows: Vec<Option<Row>> = lines.iter().map(|one| row_of(one)).collect();
        let at = |line: usize| {
            let row = rows[line - 1].as_ref().unwrap();

            path_above(
                &rows,
                line - 1,
                if row.dash { row.indent } else { row.key_indent },
                row.dash,
            )
        };

        assert_eq!(at(1), "");
        assert_eq!(at(4), "requires");
        assert_eq!(at(5), "requires.libraries");
        assert_eq!(at(7), "requires.peripherals");
        assert_eq!(at(8), "requires.peripherals[]");
        assert_eq!(at(10), "headers");
        assert_eq!(at(11), "headers");
        assert_eq!(at(12), "headers[]");
        assert_eq!(at(15), "options.*");
        assert_eq!(at(16), "options.*.headers");
        assert_eq!(at(21), "zephyr.project.cmake.after-zephyr");
    }

    fn labels(found: &Completions) -> Vec<&str> {
        found.items.iter().map(|item| item.label.as_str()).collect()
    }

    #[test]
    fn at_the_start_of_a_line_it_offers_the_keys_not_given_yet() {
        let text = "name: x\nhe";
        let found = complete(text, 2, 2, &[]);

        assert_eq!(found.from, 0);
        assert!(labels(&found).contains(&"headers"), "{found:#?}");
        assert!(!labels(&found).contains(&"name"), "{found:#?}");

        let headers = found
            .items
            .iter()
            .find(|item| item.label == "headers")
            .unwrap();

        assert_eq!(headers.apply, "headers:\n  - ");
    }

    #[test]
    fn below_a_key_it_offers_what_that_key_holds() {
        let text = "stm32:\n  requires:\n    ";
        let found = complete(text, 3, 4, &[]);

        assert_eq!(found.from, 4);
        assert!(labels(&found).contains(&"hal_modules"), "{found:#?}");

        let text = "run:\n  - label: x\n    ";
        let found = complete(text, 3, 4, &[]);

        assert!(labels(&found).contains(&"command"), "{found:#?}");
        assert!(
            !labels(&found).contains(&"label"),
            "it is given: {found:#?}"
        );
    }

    #[test]
    fn after_a_key_it_offers_what_that_key_can_be() {
        let found = complete("kind: dr", 1, 8, &[]);

        assert_eq!(found.from, 6);
        assert!(labels(&found).contains(&"driver"), "{found:#?}");

        let found = complete("platform: [stm32, ze", 1, 20, &[]);

        assert_eq!(found.from, 18);
        assert!(labels(&found).contains(&"zephyr"), "{found:#?}");

        let found = complete("stm32:\n  requires:\n    rtos: ", 3, 10, &[]);

        assert!(labels(&found).contains(&"freertos"), "{found:#?}");
    }

    #[test]
    fn in_a_list_of_files_it_offers_the_files_that_suit_it() {
        let files = vec![
            "src/a.h".to_string(),
            "src/a.c".to_string(),
            "README.md".to_string(),
        ];
        let found = complete("headers:\n  - sr", 2, 6, &files);

        assert_eq!(found.from, 4);
        assert_eq!(labels(&found), ["src/a.h"]);

        let found = complete("sources:\n  - ", 2, 4, &files);

        assert_eq!(labels(&found), ["src/a.c"]);

        let found = complete("extras:\n  - ", 2, 4, &files);

        assert_eq!(labels(&found), ["src/a.h", "src/a.c", "README.md"]);
    }

    #[test]
    fn in_a_comment_it_offers_nothing() {
        assert!(complete("# he", 1, 4, &[]).items.is_empty());
    }

    #[test]
    fn on_a_key_it_says_what_the_key_is() {
        let text = "stm32:\n  requires:\n    rtos: any\n";
        let found = help_at(text, 3, 6).unwrap();

        assert_eq!(found.path, "stm32.requires.rtos");
        assert_eq!(found.from, 4);
        assert_eq!(found.to, 8);
        assert!(found.help.contains("operating system"), "{found:#?}");
        assert_eq!(found.example, "rtos: optional");

        assert_eq!(help_at(text, 3, 12), None, "on the value");
        assert_eq!(help_at("something: 1\n", 1, 2), None, "no such key");

        let found = help_at("run:\n  - label: x\n", 2, 5).unwrap();

        assert_eq!(found.path, "run[].label");
    }

    #[test]
    fn the_nearest_word_is_offered_only_when_it_is_near() {
        let keys = ["sources", "headers", "name"];

        assert_eq!(nearest("source", keys), Some("sources"));
        assert_eq!(nearest("Headers", keys), Some("headers"));
        assert_eq!(nearest("hedaers", keys), Some("headers"));
        assert_eq!(nearest("xyzzy", keys), None);
    }

    #[test]
    fn the_schema_file_is_the_table() {
        let written = include_str!("../../../docs/libnima.schema.json");
        let made = serde_json::to_string_pretty(&json_schema()).unwrap();

        assert_eq!(
            written.trim().replace('\r', ""),
            made,
            "docs/libnima.schema.json is out of date: run libnima --schema > docs/libnima.schema.json"
        );
    }

    #[test]
    fn the_schema_says_what_the_keys_are() {
        let found = json_schema();

        assert_eq!(found["required"], json!(["name", "platform"]));
        assert_eq!(found["additionalProperties"], json!(true));
        assert_eq!(found["properties"]["kind"]["enum"], json!(KINDS));
        assert_eq!(
            found["properties"]["requires"]["additionalProperties"],
            json!(false)
        );
        assert!(
            found["properties"]["options"]["additionalProperties"]["properties"]["description"]
                .is_object()
        );
    }

    #[test]
    fn a_walk_goes_over_the_keys_of_a_file_by_their_paths() {
        let node = manifest::parse(
            "name: x\noptions:\n  lfs:\n    description: y\nrun:\n  - label: z\nnope: 1\n",
        )
        .unwrap();
        let mut seen = Vec::new();

        walk_keys(&node, "", &mut |path, _| seen.push(path.to_string()));

        assert_eq!(
            seen,
            [
                "name",
                "options",
                "options.*",
                "options.*.description",
                "run",
                "run[].label",
                "nope"
            ]
        );
    }
}
