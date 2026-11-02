//! A first libnima.yml for a library's folder, from what is in it.
//!
//! Most of what the file says is already in the folder: the files, the licence, where it comes from, and often which platform
//! it is for. So a new file starts from that rather than from nothing, and what it could not find is left for the person.
//! It only ever offers: nothing here is written until the person saves it.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Value as Json, json};

use crate::manifest::{self, MANIFEST_NAME};
use crate::paths;
use crate::platform::Platform;
use crate::ymledit::{self, Step};

/// What is at the top of a new file: where its format is told, for a person and for an editor.
const TOP: &str = "# The format of this file: https://github.com/nimaltd/libNIMA/blob/main/docs/FORMAT.md\n\
                   # yaml-language-server: $schema=https://raw.githubusercontent.com/nimaltd/libNIMA/main/docs/libnima.schema.json\n\n";

/// What a new file says when the folder does not say what the library does.
const NO_DESCRIPTION: &str = "What it does, in one line";

/// Folders that hold nothing of a library's: what tools and builds leave.
const NEVER: [&str; 13] = [
    ".git",
    ".github",
    ".idea",
    ".pio",
    ".svn",
    ".vs",
    ".vscode",
    "__pycache__",
    "build",
    "Debug",
    "node_modules",
    "Release",
    "target",
];

/// Folders whose code is about the library rather than of it.
const NOT_ITS_CODE: [&str; 9] = [
    "doc", "docs", "example", "examples", "extras", "scripts", "test", "tests", "tools",
];

/// How much of a file is read for what it mentions.
const READ_OF_A_FILE: usize = 64 * 1024;

/// How many code files are read for what they mention.
const FILES_READ: usize = 64;

static STM32_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*#\s*include\s*[<"]stm32|\bHAL_[A-Z0-9]+_|\bHAL_Delay\b|\bGPIO_TypeDef\b"#)
        .expect("a valid pattern")
});

static ZEPHYR_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*#\s*include\s*[<"]zephyr[/.]"#).expect("a valid pattern")
});

static ESP32_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*#\s*include\s*[<"](esp_|driver/|freertos/FreeRTOS\.h)"#)
        .expect("a valid pattern")
});

static ARDUINO_CODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s*#\s*include\s*[<"]Arduino\.h"#).expect("a valid pattern")
});

/// The HAL's modules a project has to switch on, as its calls name them. GPIO, RCC and the like are always on.
static HAL_MODULE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:HAL_|__HAL_)(ADC|CAN|CRC|DAC|ETH|FDCAN|I2C|I2S|IWDG|PCD|QSPI|RNG|RTC|SAI|SD|SPI|TIM|UART|USART|WWDG)_")
        .expect("a valid pattern")
});

static GIT_REMOTE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)\[remote "origin"\][^\[]*?url\s*=\s*(\S+)"#).expect("a valid pattern")
});

/// What was found in a library's folder.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Found {
    pub name: String,
    pub description: String,
    pub repository: String,
    pub license: String,
    pub version: String,
    pub platforms: Vec<Platform>,
    pub headers: Vec<String>,
    pub sources: Vec<String>,
    pub extras: Vec<String>,
    pub hal_modules: Vec<String>,
    /// What library.properties says, for the Arduino section.
    pub arduino_category: String,
    pub arduino_architectures: Vec<String>,
    pub arduino_depends: Vec<String>,
    /// What library.json says, for the PlatformIO section.
    pub pio_frameworks: Vec<String>,
    pub pio_platforms: Vec<String>,
}

/// A folder's name, as a library's name can be.
pub fn name_for(folder: &Path) -> String {
    let name: String = paths::resolve(folder)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase()
        .chars()
        .map(|one| {
            if one.is_ascii_alphanumeric() || one == '_' || one == '-' {
                one
            } else {
                '_'
            }
        })
        .collect();

    if name.starts_with(|first: char| first.is_ascii_alphabetic()) {
        name
    } else {
        format!("lib_{name}")
    }
}

/// Every file of a library's folder worth offering, leaving out what tools and builds leave. The file itself is not one of them.
pub fn files_of(folder: &Path) -> Vec<String> {
    paths::files_below(folder)
        .into_iter()
        .filter(|path| {
            !path
                .split('/')
                .any(|part| NEVER.contains(&part) || part.starts_with("cmake-build"))
                && !manifest::MANIFEST_NAMES.contains(&path.as_str())
        })
        .collect()
}

fn is_code_folder(path: &str) -> bool {
    !path
        .split('/')
        .rev()
        .skip(1)
        .any(|part| NOT_ITS_CODE.contains(&part.to_lowercase().as_str()))
}

/// The header to #include: the one named after the library, or one with a version in it, ahead of the rest.
fn main_header_first(headers: &mut [String], name: &str, folder: &Path) {
    let wanted = format!("{name}.h");

    headers.sort_by_key(|path| {
        let leaf = paths::leaf(path).to_lowercase();
        let named = leaf == wanted;
        let settings = leaf.contains("config") || leaf.contains("conf") || leaf.contains("port");
        let versioned = read_some(&folder.join(path))
            .as_deref()
            .and_then(manifest::version_in)
            .is_some();

        (!named, !versioned, settings, path.clone())
    });
}

fn read_some(path: &Path) -> Option<String> {
    use std::io::Read;

    let mut buffer = vec![0; READ_OF_A_FILE];
    let read = fs::File::open(path).ok()?.read(&mut buffer).ok()?;

    Some(String::from_utf8_lossy(&buffer[..read]).into_owned())
}

/// The licence a licence file's text is, by its SPDX name.
fn license_in(text: &str) -> Option<&'static str> {
    let text = text.to_lowercase();
    let has = |word: &str| text.contains(word);

    if has("apache license") && has("version 2.0") {
        Some("Apache-2.0")
    } else if has("gnu lesser general public license") {
        Some(if has("version 3") {
            "LGPL-3.0-only"
        } else {
            "LGPL-2.1-only"
        })
    } else if has("gnu general public license") {
        Some(if has("version 3") {
            "GPL-3.0-only"
        } else {
            "GPL-2.0-only"
        })
    } else if has("mozilla public license") {
        Some("MPL-2.0")
    } else if has("permission is hereby granted, free of charge") {
        Some("MIT")
    } else if has("redistribution and use in source and binary forms") {
        Some(if has("neither the name") {
            "BSD-3-Clause"
        } else {
            "BSD-2-Clause"
        })
    } else if has("this software is provided 'as-is'") {
        Some("Zlib")
    } else if has("permission to use, copy, modify, and") {
        Some("ISC")
    } else if has("this is free and unencumbered software") {
        Some("Unlicense")
    } else {
        None
    }
}

/// A remote's address as one a browser opens.
fn web_address(remote: &str) -> Option<String> {
    let remote = remote.trim().trim_end_matches('/').trim_end_matches(".git");

    if let Some(rest) = remote.strip_prefix("git@github.com:") {
        return Some(format!("https://github.com/{rest}"));
    }

    if let Some(rest) = remote.strip_prefix("ssh://git@github.com/") {
        return Some(format!("https://github.com/{rest}"));
    }

    remote.starts_with("https://").then(|| remote.to_string())
}

/// The first sentence of a README that says something: past its headings, badges and pictures.
fn first_sentence(text: &str) -> Option<String> {
    let paragraph: Vec<&str> = text
        .lines()
        .map(str::trim)
        .skip_while(|line| {
            line.is_empty()
                || line.starts_with('#')
                || line.starts_with('[')
                || line.starts_with('!')
                || line.starts_with('<')
                || line.starts_with("===")
                || line.starts_with("---")
        })
        .take_while(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let joined = paragraph.join(" ");
    let cut = joined
        .find(". ")
        .map_or(joined.as_str(), |at| &joined[..at])
        .trim_end_matches('.')
        .replace(['*', '`'], "");

    (!cut.is_empty() && cut.len() <= 120).then_some(cut)
}

fn add_platform(platforms: &mut Vec<Platform>, platform: Platform) {
    if !platforms.contains(&platform) {
        platforms.push(platform);
    }
}

fn list_in(value: Option<&Json>) -> Vec<String> {
    match value {
        Some(Json::Array(items)) => items
            .iter()
            .filter_map(Json::as_str)
            .map(str::to_string)
            .collect(),
        Some(Json::String(text)) => text
            .split(',')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

/// What Arduino's own file says of the library.
fn read_properties(folder: &Path, found: &mut Found) {
    let Ok(text) = fs::read_to_string(folder.join("library.properties")) else {
        return;
    };
    let get = |key: &str| {
        text.lines()
            .find_map(|line| {
                line.trim()
                    .strip_prefix(key)?
                    .trim_start()
                    .strip_prefix('=')
            })
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };

    add_platform(&mut found.platforms, Platform::Arduino);

    if found.description.is_empty() {
        found.description = get("sentence");
    }

    if found.repository.is_empty() && get("url").contains("github.com") {
        found.repository = get("url");
    }

    if found.version.is_empty() {
        found.version = get("version");
    }

    found.arduino_category = get("category");
    found.arduino_architectures = list_in(Some(&Json::String(get("architectures"))));
    found.arduino_depends = list_in(Some(&Json::String(get("depends"))));

    // A file of its own says this for every architecture, which needs no saying.
    found.arduino_architectures.retain(|one| one != "*");
}

/// What PlatformIO's own file says of the library.
fn read_library_json(folder: &Path, found: &mut Found) {
    let Some(data) = fs::read_to_string(folder.join("library.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Json>(&text).ok())
    else {
        return;
    };
    let text_of =
        |value: Option<&Json>| value.and_then(Json::as_str).unwrap_or_default().to_string();

    add_platform(&mut found.platforms, Platform::Pio);

    if found.description.is_empty() {
        found.description = text_of(data.get("description"));
    }

    if found.repository.is_empty() {
        found.repository = text_of(
            data.get("repository")
                .and_then(|repository| repository.get("url")),
        )
        .trim_end_matches(".git")
        .to_string();
    }

    if found.license.is_empty() {
        found.license = text_of(data.get("license"));
    }

    if found.version.is_empty() {
        found.version = text_of(data.get("version"));
    }

    found.pio_frameworks = list_in(data.get("frameworks"));
    found.pio_platforms = list_in(data.get("platforms"));
    found.pio_frameworks.retain(|one| one != "*");
    found.pio_platforms.retain(|one| one != "*");
}

/// What a library's folder says about it. `fallback` is the platform when nothing in it tells.
pub fn scan(folder: &Path, fallback: Option<Platform>) -> Found {
    let files = files_of(folder);
    let mut found = Found {
        name: name_for(folder),
        ..Found::default()
    };

    if let Some(remote) = fs::read_to_string(folder.join(".git").join("config"))
        .ok()
        .and_then(|config| {
            GIT_REMOTE
                .captures(&config)
                .map(|captured| captured[1].to_string())
        })
    {
        found.repository = web_address(&remote).unwrap_or_default();
    }

    read_properties(folder, &mut found);
    read_library_json(folder, &mut found);

    let code: Vec<&String> = files.iter().filter(|path| is_code_folder(path)).collect();

    found.headers = code
        .iter()
        .filter(|path| manifest_header(path))
        .map(|path| (*path).clone())
        .collect();
    found.sources = code
        .iter()
        .filter(|path| manifest::COMPILED.contains(&paths::suffix(path).as_str()))
        .map(|path| (*path).clone())
        .collect();
    main_header_first(&mut found.headers, &found.name.clone(), folder);

    // What the folder's own files tell of the platform.
    if files
        .iter()
        .any(|path| path == "zephyr/module.yml" || path == "module.yml")
    {
        add_platform(&mut found.platforms, Platform::Zephyr);
    }

    if files.iter().any(|path| path == "idf_component.yml") {
        add_platform(&mut found.platforms, Platform::Esp32);
    }

    // And what the code mentions.
    let mut modules: Vec<String> = Vec::new();
    let mut uses_hal = false;

    for path in found.headers.iter().chain(&found.sources).take(FILES_READ) {
        let Some(text) = read_some(&folder.join(path)) else {
            continue;
        };

        if STM32_CODE.is_match(&text) {
            uses_hal = true;
            add_platform(&mut found.platforms, Platform::Stm32);
        }

        if ZEPHYR_CODE.is_match(&text) {
            add_platform(&mut found.platforms, Platform::Zephyr);
        }

        if ESP32_CODE.is_match(&text) {
            add_platform(&mut found.platforms, Platform::Esp32);
        }

        if ARDUINO_CODE.is_match(&text) {
            add_platform(&mut found.platforms, Platform::Arduino);
        }

        for module in HAL_MODULE.captures_iter(&text) {
            let module = module[1].to_lowercase();

            if !modules.contains(&module) {
                modules.push(module);
            }
        }
    }

    modules.sort();

    if uses_hal {
        found.hal_modules = modules;
    }

    if found.platforms.is_empty()
        && let Some(fallback) = fallback
    {
        found.platforms.push(fallback);
    }

    // What is copied as it is, from the top of the folder.
    let mut license_file = None;

    for path in &files {
        if path.contains('/') {
            continue;
        }

        let upper = path.to_uppercase();

        if upper.starts_with("LICENSE")
            || upper.starts_with("LICENCE")
            || upper.starts_with("COPYING")
        {
            license_file.get_or_insert_with(|| path.clone());
            found.extras.push(path.clone());
        } else if upper == "NOTICE" || upper.starts_with("README") || upper.starts_with("CHANGELOG")
        {
            found.extras.push(path.clone());
        }
    }

    if found.license.is_empty()
        && let Some(text) = license_file.and_then(|file| read_some(&folder.join(file)))
    {
        found.license = license_in(&text).unwrap_or_default().to_string();
    }

    if found.description.is_empty() {
        found.description = files
            .iter()
            .find(|path| !path.contains('/') && path.to_uppercase().starts_with("README"))
            .and_then(|path| read_some(&folder.join(path)))
            .and_then(|text| first_sentence(&text))
            .unwrap_or_default();
    }

    // The first header's @version is read every time, so it is written nowhere else.
    if found
        .headers
        .first()
        .and_then(|first| read_some(&folder.join(first)))
        .as_deref()
        .and_then(manifest::version_in)
        .is_some()
    {
        found.version.clear();
    }

    found
}

fn manifest_header(path: &str) -> bool {
    matches!(
        paths::suffix(path).as_str(),
        ".h" | ".hh" | ".hpp" | ".hxx" | ".inc"
    )
}

/// A first file from what was found. Written with the code the form changes a file with, so it comes out in the order and the
/// layout of any other.
pub fn first_file(found: &Found, oldest_libnima: &str) -> String {
    let platform: Json = match found.platforms.as_slice() {
        [] => json!(Platform::Stm32.name()),
        [only] => json!(only.name()),
        many => json!(
            many.iter()
                .map(|platform| platform.name())
                .collect::<Vec<_>>()
        ),
    };
    let description = if found.description.is_empty() {
        NO_DESCRIPTION
    } else {
        &found.description
    };
    let headers = if found.headers.is_empty() && found.sources.is_empty() {
        vec![format!("src/{}.h", found.name)]
    } else {
        found.headers.clone()
    };

    let mut text = TOP.to_string();
    let mut put = |at: &[&str], value: Json| {
        let at: Vec<Step> = at.iter().map(|key| Step::from(*key)).collect();

        if let Ok(made) = ymledit::set(&text, &at, Some(&value)) {
            text = made;
        }
    };

    put(&["name"], json!(found.name));
    put(&["description"], json!(description));

    if !found.repository.is_empty() {
        put(&["repository"], json!(found.repository));
    }

    if !found.license.is_empty() {
        put(&["license"], json!(found.license));
    }

    put(&["platform"], platform);

    if !found.version.is_empty() {
        put(&["version"], json!(found.version));
    }

    put(&["requires", "libnima"], json!(oldest_libnima));

    if !headers.is_empty() {
        put(&["headers"], json!(headers));
    }

    if !found.sources.is_empty() {
        put(&["sources"], json!(found.sources));
    }

    if !found.extras.is_empty() {
        put(&["extras"], json!(found.extras));
    }

    if !found.hal_modules.is_empty() {
        put(
            &["stm32", "requires", "hal_modules"],
            json!(found.hal_modules),
        );
    }

    if !found.arduino_architectures.is_empty() {
        put(
            &["arduino", "architectures"],
            json!(found.arduino_architectures),
        );
    }

    if !found.arduino_category.is_empty() {
        put(&["arduino", "category"], json!(found.arduino_category));
    }

    if !found.arduino_depends.is_empty() {
        put(&["arduino", "depends"], json!(found.arduino_depends));
    }

    if !found.pio_frameworks.is_empty() {
        put(
            &["pio", "library", "frameworks"],
            json!(found.pio_frameworks),
        );
    }

    if !found.pio_platforms.is_empty() {
        put(&["pio", "library", "platforms"], json!(found.pio_platforms));
    }

    text
}

/// The path a new file for a folder is saved at.
pub fn path_for(folder: &Path) -> String {
    paths::posix(&folder.join(MANIFEST_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::lint_text;

    fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::Builder::new().prefix("ee24-").tempdir().unwrap();

        for (name, text) in files {
            let path = root.path().join(name);

            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }

        root
    }

    fn scan_named(root: &Path) -> Found {
        Found {
            name: "ee24".to_string(),
            ..scan(root, None)
        }
    }

    const APACHE: &str = "                                 Apache License\n                           Version 2.0, January 2004\n";

    #[test]
    fn it_finds_what_a_library_folder_says() {
        let root = folder(&[
            (
                "src/ee24.h",
                "/**\n * @version 4.0.0\n */\n#include \"main.h\"\n",
            ),
            ("src/ee24_config.h", "#define EE24_SIZE 8\n"),
            (
                "src/ee24.c",
                "#include \"stm32h5xx_hal.h\"\nHAL_I2C_Master_Transmit(a);\nHAL_GPIO_WritePin(b);\n",
            ),
            ("tests/test_ee24.c", "#include <zephyr/kernel.h>\n"),
            ("examples/demo/demo.h", ""),
            ("build/out.c", ""),
            ("LICENSE.md", APACHE),
            ("NOTICE", "x"),
            (
                "README.md",
                "# ee24\n\n[![CI](x)](y)\n\nDriver for 24xx I2C EEPROMs. It does a lot.\n\n## More\n",
            ),
            ("CHANGELOG.md", "x"),
            (
                ".git/config",
                "[core]\n\tbare = false\n[remote \"origin\"]\n\turl = git@github.com:nimaltd/ee24.git\n\tfetch = x\n",
            ),
        ]);
        let found = scan(root.path(), None);

        assert_eq!(found.description, "Driver for 24xx I2C EEPROMs");
        assert_eq!(found.repository, "https://github.com/nimaltd/ee24");
        assert_eq!(found.license, "Apache-2.0");
        assert_eq!(found.platforms, [Platform::Stm32], "not from the tests");
        assert_eq!(found.version, "", "the first header says it, every time");
        assert_eq!(found.headers, ["src/ee24.h", "src/ee24_config.h"]);
        assert_eq!(found.sources, ["src/ee24.c"]);
        assert_eq!(
            found.extras,
            ["CHANGELOG.md", "LICENSE.md", "NOTICE", "README.md"]
        );
        assert_eq!(found.hal_modules, ["i2c"], "GPIO is always on");
    }

    #[test]
    fn the_file_it_writes_reads_clean_and_says_what_it_found() {
        let root = folder(&[
            ("src/ee24.h", "/**\n * @version 4.0.0\n */\n"),
            (
                "src/ee24.c",
                "#include \"stm32h5xx_hal.h\"\nHAL_SPI_Init(a);\n",
            ),
            ("LICENSE.md", APACHE),
            ("README.md", "Driver for a chip.\n"),
        ]);
        let found = scan_named(root.path());
        let text = first_file(&found, "1.0.0");

        assert_eq!(
            text,
            format!(
                "{TOP}name: ee24\ndescription: Driver for a chip\nlicense: Apache-2.0\nplatform: stm32\n\n\
                 requires:\n  libnima: 1.0.0\n\n\
                 headers:\n  - src/ee24.h\n\nsources:\n  - src/ee24.c\n\n\
                 extras:\n  - LICENSE.md\n  - README.md\n\n\
                 stm32:\n  requires:\n    hal_modules: [spi]\n"
            )
        );

        let problems = lint_text(root.path(), &root.path().join(MANIFEST_NAME), &text);

        assert!(problems.is_empty(), "{problems:#?}");
    }

    #[test]
    fn a_header_named_after_the_library_comes_first() {
        let root = folder(&[
            ("a_port.h", ""),
            ("aaa.h", ""),
            ("ee24.h", ""),
            ("ee24_config.h", ""),
        ]);

        assert_eq!(scan_named(root.path()).headers.len(), 4);

        let mut headers: Vec<String> = ["a_port.h", "aaa.h", "ee24.h", "ee24_config.h"]
            .map(str::to_string)
            .to_vec();

        main_header_first(&mut headers, "ee24", root.path());

        assert_eq!(headers, ["ee24.h", "aaa.h", "a_port.h", "ee24_config.h"]);
    }

    #[test]
    fn a_header_with_a_version_comes_ahead_of_one_without() {
        let root = folder(&[("b.h", "/** @version 1.0.0 */\n"), ("a.h", "")]);
        let mut headers = vec!["a.h".to_string(), "b.h".to_string()];

        main_header_first(&mut headers, "x", root.path());

        assert_eq!(headers, ["b.h", "a.h"]);
    }

    #[test]
    fn arduino_and_platformio_say_a_lot_in_their_own_files() {
        let root = folder(&[
            ("src/x.h", ""),
            (
                "library.properties",
                "name=X\nversion=1.2.3\nsentence=A driver.\ncategory=Sensors\nurl=https://github.com/a/x\narchitectures=avr, esp32\ndepends=Wire, SPI\n",
            ),
            (
                "library.json",
                "{\"name\": \"x\", \"license\": \"MIT\", \"frameworks\": [\"arduino\"], \"platforms\": \"espressif32\"}",
            ),
        ]);
        let found = scan(root.path(), None);

        assert_eq!(found.platforms, [Platform::Arduino, Platform::Pio]);
        assert_eq!(found.description, "A driver.");
        assert_eq!(found.repository, "https://github.com/a/x");
        assert_eq!(found.license, "MIT");
        assert_eq!(found.version, "1.2.3");
        assert_eq!(found.arduino_category, "Sensors");
        assert_eq!(found.arduino_architectures, ["avr", "esp32"]);
        assert_eq!(found.arduino_depends, ["Wire", "SPI"]);
        assert_eq!(found.pio_frameworks, ["arduino"]);
        assert_eq!(found.pio_platforms, ["espressif32"]);

        let text = first_file(&found, "1.0.0");
        let problems: Vec<_> = lint_text(root.path(), &root.path().join(MANIFEST_NAME), &text)
            .into_iter()
            .filter(|problem| !problem.warning)
            .collect();

        assert!(problems.is_empty(), "{text}\n{problems:#?}");
        assert!(text.contains("platform: [arduino, pio]\n"), "{text}");
        assert!(text.contains("arduino:\n  architectures: [avr, esp32]\n  category: Sensors\n  depends:\n    - Wire\n    - SPI\n"), "{text}");
        assert!(
            text.contains(
                "pio:\n  library:\n    frameworks: [arduino]\n    platforms: [espressif32]\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn the_code_tells_of_the_other_platforms_too() {
        let zephyr = folder(&[("x.c", "#include <zephyr/kernel.h>\n")]);
        let esp = folder(&[("main.c", "#include \"esp_log.h\"\n")]);
        let by_file = folder(&[("x.h", ""), ("zephyr/module.yml", "")]);

        assert_eq!(scan(zephyr.path(), None).platforms, [Platform::Zephyr]);
        assert_eq!(scan(esp.path(), None).platforms, [Platform::Esp32]);
        assert_eq!(scan(by_file.path(), None).platforms, [Platform::Zephyr]);
    }

    #[test]
    fn a_folder_that_tells_nothing_takes_the_platform_it_is_given() {
        let root = folder(&[("x.h", "int x;\n")]);

        assert_eq!(
            scan(root.path(), Some(Platform::Pio)).platforms,
            [Platform::Pio]
        );
        assert!(scan(root.path(), None).platforms.is_empty());
    }

    #[test]
    fn an_empty_folder_still_gets_a_file_to_start_from() {
        let root = folder(&[]);
        let found = scan_named(root.path());
        let text = first_file(&found, "1.0.0");

        assert!(text.contains("name: ee24\n"), "{text}");
        assert!(
            text.contains("description: What it does, in one line\n"),
            "{text}"
        );
        assert!(text.contains("platform: stm32\n"), "{text}");
        assert!(text.contains("headers:\n  - src/ee24.h\n"), "{text}");
    }

    #[test]
    fn a_name_is_made_from_the_folder() {
        assert_eq!(name_for(Path::new("/x/My Lib")), "my_lib");
        assert_eq!(name_for(Path::new("/x/ee24")), "ee24");
        assert_eq!(name_for(Path::new("/x/24xx")), "lib_24xx");
    }

    #[test]
    fn the_licences_are_told_apart() {
        for (text, name) in [
            ("Apache License\nVersion 2.0", "Apache-2.0"),
            (
                "MIT License\nPermission is hereby granted, free of charge, to any",
                "MIT",
            ),
            (
                "Redistribution and use in source and binary forms\nNeither the name of",
                "BSD-3-Clause",
            ),
            (
                "Redistribution and use in source and binary forms",
                "BSD-2-Clause",
            ),
            ("GNU GENERAL PUBLIC LICENSE\nVersion 3", "GPL-3.0-only"),
            (
                "GNU LESSER GENERAL PUBLIC LICENSE\nVersion 2.1",
                "LGPL-2.1-only",
            ),
            ("Mozilla Public License Version 2.0", "MPL-2.0"),
        ] {
            assert_eq!(license_in(text), Some(name), "{text}");
        }

        assert_eq!(license_in("Do what you like"), None);
    }

    #[test]
    fn a_remote_becomes_an_address_a_browser_opens() {
        assert_eq!(
            web_address("git@github.com:a/b.git").as_deref(),
            Some("https://github.com/a/b")
        );
        assert_eq!(
            web_address("https://github.com/a/b.git").as_deref(),
            Some("https://github.com/a/b")
        );
        assert_eq!(
            web_address("ssh://git@github.com/a/b").as_deref(),
            Some("https://github.com/a/b")
        );
        assert_eq!(web_address("/some/folder"), None);
    }

    #[test]
    fn what_a_folder_leaves_about_is_not_offered() {
        let root = folder(&[
            ("src/x.h", ""),
            (".git/config", ""),
            ("build/a.c", ""),
            ("cmake-build-debug/b.c", ""),
            ("libnima.yml", ""),
            ("README.md", ""),
        ]);

        assert_eq!(files_of(root.path()), ["README.md", "src/x.h"]);
    }
}
