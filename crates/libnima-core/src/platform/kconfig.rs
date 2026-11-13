//! What Zephyr and ESP-IDF share: Kconfig symbols, the settings a library gives the user,
//! and the lines both write into a CMakeLists.txt.

use std::sync::LazyLock;

use regex::Regex;

use crate::files;
use crate::manifest::ManifestError;
use crate::text::lines_of;
use crate::yaml::{Node, Value};

/// A Kconfig symbol as Zephyr and ESP-IDF write them, without CONFIG_ in front.
static SYMBOL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Z0-9_]+$").expect("a valid pattern"));

/// A value a symbol may be given in a Kconfig file: y or n, a number, a hex number, or a
/// string in quotes. Written into the file as it stands, so nothing else gets through.
static VALUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"^(?:y|n|-?[0-9]+|0x[0-9A-Fa-f]+|"[^"\\\n]*")$"#).expect("a valid pattern")
});

static INT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[0-9]+$").expect("a valid pattern"));
static HEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^0x[0-9A-Fa-f]+$").expect("a valid pattern"));

/// CONFIG_FOO=y, and the form menuconfig writes for a symbol switched off. A file written
/// on Windows ends its lines in CRLF, and $ stops only before the line feed, so the
/// carriage return is allowed for, never taken into a value.
pub static ASSIGNED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*CONFIG_([A-Za-z0-9_]+)[ \t]*=[ \t]*(.*?)[ \t\r]*$")
        .expect("a valid pattern")
});
pub static NOT_SET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*#[ \t]*CONFIG_([A-Za-z0-9_]+) is not set[ \t\r]*$")
        .expect("a valid pattern")
});

/// What a setting can be, as Kconfig names its types.
const TYPES: [&str; 4] = ["bool", "int", "hex", "string"];

/// The Kconfig symbol for a library name: EE24 for ee24, ONE_WIRE for one-wire.
pub fn symbol_of(name: &str) -> String {
    name.chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// A symbol written in libnima.yml, without CONFIG_ in front. `example` is what the error
/// offers, I2C for Zephyr and FREERTOS_HZ for ESP-IDF.
pub fn symbol(text: &Node, where_: &str, example: &str) -> Result<String, ManifestError> {
    let written = text.text();
    let found = written.trim();
    let found = found.strip_prefix("CONFIG_").unwrap_or(found);

    if !SYMBOL.is_match(found) {
        return Err(ManifestError::at(
            text.line,
            format!(
                "{where_} has {}. Write a Kconfig symbol in capitals, digits and _, such as {example} or CONFIG_{example}.",
                text.shown()
            ),
        ));
    }

    Ok(found.to_string())
}

/// A symbol and the value it is set to: BT, which is set to y, or BT_L2CAP_TX_MTU=498.
#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub symbol: String,
    pub value: String,
    /// What it stands behind in the file: CONFIG_, or SB_CONFIG_ in sysbuild.conf.
    pub prefix: &'static str,
}

impl Assignment {
    pub fn read(
        text: &Node,
        key: &str,
        example: &str,
        prefix: &'static str,
    ) -> Result<Assignment, ManifestError> {
        let written = text.text();
        let (name, value) = written.split_once('=').unwrap_or((&written, ""));
        let name = name.trim();
        let name = name.strip_prefix(prefix).unwrap_or(name);
        let symbol = symbol(&Node::string(name, text.line), key, example)?;
        let value = value.trim();
        let value = if value.is_empty() { "y" } else { value };

        if !VALUE.is_match(value) {
            return Err(ManifestError::at(
                text.line,
                format!(
                    "{key} has {}. Give a symbol, which is set to y, or SYMBOL=value, with y, n, a number, 0x and a \
                     hex number, or \"text\".",
                    text.shown()
                ),
            ));
        }

        Ok(Assignment {
            symbol,
            value: value.to_string(),
            prefix,
        })
    }

    /// The line in the file.
    pub fn line(&self) -> String {
        format!("{}{}={}", self.prefix, self.symbol, self.value)
    }
}

/// Text for a Kconfig prompt: one line, its quotes and backslashes escaped.
pub fn one_line(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Text as one quoted CMake argument.
///
/// A file name lands in CMakeLists.txt as it stands, so a quote, a dollar or a semicolon
/// in it could otherwise end the argument, expand a variable or split it into two.
pub fn cmake_quote(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace(';', "\\;");

    format!("\"{escaped}\"")
}

/// One setting of the library, a Kconfig symbol the user sets in menuconfig or prj.conf.
///
/// Written into the library's Kconfig, so the C code reads it as CONFIG_<name>. Everything
/// is checked here, since all of it is written into a file Kconfig reads, and the manifest
/// comes over the network.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting {
    pub name: String,
    pub kind: String,
    pub prompt_given: bool,
    pub prompt: String,
    pub help: Vec<String>,
    pub range: Option<(i128, i128)>,
    pub default: String,
}

impl Setting {
    /// `section` is zephyr or esp32, and `example` what an error about a symbol offers.
    pub fn read(data: &Node, section: &str, example: &str) -> Result<Setting, ManifestError> {
        let Some(map) = data.as_map() else {
            return Err(ManifestError::at(
                data.line,
                format!(
                    "{section}.settings holds one entry per setting, each with a name and a type."
                ),
            ));
        };

        let empty = Node::string("", data.line);
        let name = symbol(
            map.get("name").unwrap_or(&empty),
            &format!("{section}.settings"),
            example,
        )?;
        let where_ = format!("{section}.settings {name}");

        crate::manifest::only_keys(
            map,
            &where_,
            &["name", "type", "default", "range", "prompt", "help"],
        )?;

        let kind = map
            .given("type")
            .map(Node::text)
            .unwrap_or_default()
            .to_lowercase();

        if !TYPES.contains(&kind.as_str()) {
            return Err(ManifestError::at(
                map.line_of("type").unwrap_or(data.line),
                format!(
                    "{where_} has type {}. Use one of: {}.",
                    map.get("type")
                        .map_or_else(|| "None".to_string(), Node::shown),
                    TYPES.join(", ")
                ),
            ));
        }

        let prompt = map.given("prompt").filter(|prompt| prompt.truthy());
        let help = map
            .given("help")
            .filter(|help| help.truthy())
            .map(Node::text)
            .unwrap_or_default();
        let range = read_range(map.given("range"), &kind, &where_)?;

        let Some(default) = map.given("default") else {
            return Err(ManifestError::at(
                data.line,
                format!("{where_} has no default. Every setting needs one, so it builds as it is."),
            ));
        };

        let default = default_value(default, &kind, range, &where_)?;

        Ok(Setting {
            prompt_given: prompt.is_some(),
            prompt: one_line(&prompt.map_or_else(|| name.clone(), Node::text)),
            help: lines_of(help.trim())
                .iter()
                .map(|line| line.trim_end().to_string())
                .collect(),
            name,
            kind,
            range,
            default,
        })
    }

    /// A number written the way this setting's type writes it.
    pub fn format(&self, number: i128) -> String {
        format_number(&self.kind, number)
    }

    /// The setting at its default, as a line of prj.conf.
    pub fn line(&self) -> String {
        format!("CONFIG_{}={}", self.name, self.default)
    }
}

fn format_number(kind: &str, number: i128) -> String {
    if kind != "hex" {
        return number.to_string();
    }

    if number < 0 {
        format!("-0x{:x}", -number)
    } else {
        format!("0x{number:x}")
    }
}

/// An int or hex value as a number, for comparing with the range.
fn number_of(value: &Node, where_: &str) -> Result<i128, ManifestError> {
    let written = value.text();
    let text = written.trim();
    let not_number = || {
        ManifestError::at(
            value.line,
            format!("{where_}: {} is not a number.", value.shown()),
        )
    };

    if matches!(value.value, Value::Bool(_)) {
        return Err(not_number());
    }

    if INT.is_match(text) {
        return text.parse().map_err(|_| not_number());
    }

    if HEX.is_match(text) {
        return i128::from_str_radix(&text[2..], 16).map_err(|_| not_number());
    }

    Err(not_number())
}

/// The smallest and largest the setting may be, or None.
fn read_range(
    value: Option<&Node>,
    kind: &str,
    where_: &str,
) -> Result<Option<(i128, i128)>, ManifestError> {
    let Some(value) = value else {
        return Ok(None);
    };

    if kind != "int" && kind != "hex" {
        return Err(ManifestError::at(
            value.line,
            format!("{where_} has a range, which only an int or a hex setting can have."),
        ));
    }

    let Some([low, high]) = value.as_list() else {
        return Err(ManifestError::at(
            value.line,
            format!("{where_}: write the range as [smallest, largest]."),
        ));
    };

    let (low, high) = (number_of(low, where_)?, number_of(high, where_)?);

    if low > high {
        return Err(ManifestError::at(
            value.line,
            format!("{where_}: the range starts above where it ends."),
        ));
    }

    Ok(Some((low, high)))
}

/// The default as Kconfig writes it.
fn default_value(
    value: &Node,
    kind: &str,
    range: Option<(i128, i128)>,
    where_: &str,
) -> Result<String, ManifestError> {
    match kind {
        "bool" => {
            let lower = value.text().to_lowercase();

            if matches!(value.value, Value::Bool(true))
                || matches!(lower.as_str(), "y" | "yes" | "true")
            {
                return Ok("y".to_string());
            }

            if matches!(value.value, Value::Bool(false))
                || matches!(lower.as_str(), "n" | "no" | "false")
            {
                return Ok("n".to_string());
            }

            Err(ManifestError::at(
                value.line,
                format!(
                    "{where_}: the default of a bool is true or false, not {}.",
                    value.shown()
                ),
            ))
        }
        "string" => {
            let text = value.text();

            if text.contains('\n') {
                return Err(ManifestError::at(
                    value.line,
                    format!("{where_}: the default is one line of text."),
                ));
            }

            Ok(format!(
                "\"{}\"",
                text.replace('\\', "\\\\").replace('"', "\\\"")
            ))
        }
        _ => {
            let number = number_of(value, where_)?;

            if range.is_some_and(|(low, high)| number < low || number > high) {
                return Err(ManifestError::at(
                    value.line,
                    format!(
                        "{where_}: the default {} is outside its range.",
                        value.text()
                    ),
                ));
            }

            Ok(format_number(kind, number))
        }
    }
}

/// The settings of a section: a list, one entry per setting, each named once.
pub fn read_settings(
    value: Option<&Node>,
    section: &str,
    example: &str,
) -> Result<Vec<Setting>, ManifestError> {
    let Some(value) = value.filter(|value| value.truthy()) else {
        return Ok(Vec::new());
    };

    let Some(list) = value.as_list() else {
        return Err(ManifestError::at(
            value.line,
            format!("{section}.settings is a list, one entry per setting."),
        ));
    };

    let settings = list
        .iter()
        .map(|entry| Setting::read(entry, section, example))
        .collect::<Result<Vec<_>, _>>()?;
    let twice = crate::text::given_twice(settings.iter().map(|setting| setting.name.as_str()));

    if !twice.is_empty() {
        return Err(ManifestError::at(
            value.line,
            format!(
                "{section}.settings has {} more than once.",
                twice.join(", ")
            ),
        ));
    }

    Ok(settings)
}

/// Whether a file is one libNIMA or an installer before it wrote, by its first line.
pub fn generated(path: &std::path::Path, marks: &[String]) -> bool {
    files::read(path).is_ok_and(|text| marks.iter().any(|mark| text.starts_with(mark.as_str())))
}

/// What each kind of first line a file of this platform can start with, the first the one
/// written now, and those of lib-installer and of the installer before it after it.
pub fn marks(old_installer: &str) -> Vec<String> {
    vec![
        crate::MARK.to_string(),
        "# Generated by lib-installer".to_string(),
        format!("# Generated by {old_installer}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml::loads;

    fn setting(text: &str) -> Result<Setting, ManifestError> {
        Setting::read(&loads(text).unwrap().unwrap(), "zephyr", "I2C")
    }

    #[test]
    fn a_symbol_is_read_with_or_without_config() {
        let read = |text: &str| Assignment::read(&Node::string(text, 1), "k", "I2C", "CONFIG_");

        assert_eq!(read("I2C").unwrap().line(), "CONFIG_I2C=y");
        assert_eq!(read("CONFIG_I2C=n").unwrap().line(), "CONFIG_I2C=n");
        assert_eq!(read("A=0x10").unwrap().value, "0x10");
        assert_eq!(read("A=\"x y\"").unwrap().value, "\"x y\"");
        assert!(read("a").is_err());
        assert!(read("A=x y").is_err());
        assert!(read("A=\"x\ny\"").is_err());
    }

    #[test]
    fn a_setting_of_each_type_reads() {
        let int = setting("name: A\ntype: int\ndefault: 5\nrange: [1, 10]\nprompt: The A\nhelp: |\n  One.\n  Two.\n").unwrap();
        assert_eq!(int.default, "5");
        assert_eq!(int.range, Some((1, 10)));
        assert_eq!(int.help, ["One.", "Two."]);
        assert_eq!(int.line(), "CONFIG_A=5");
        assert!(int.prompt_given);

        let hex = setting("name: A\ntype: hex\ndefault: 0x10\n").unwrap();
        assert_eq!(hex.default, "0x10");
        assert_eq!(hex.prompt, "A");
        assert!(!hex.prompt_given);

        assert_eq!(
            setting("name: A\ntype: bool\ndefault: true\n")
                .unwrap()
                .default,
            "y"
        );
        assert_eq!(
            setting("name: A\ntype: bool\ndefault: no\n")
                .unwrap()
                .default,
            "n"
        );
        assert_eq!(
            setting("name: A\ntype: string\ndefault: a \"b\"\n")
                .unwrap()
                .default,
            "\"a \\\"b\\\"\""
        );
    }

    #[test]
    fn a_bad_setting_is_refused() {
        for (text, wanted) in [
            ("name: a\ntype: int\ndefault: 1\n", "Write a Kconfig symbol"),
            ("name: A\ntype: float\ndefault: 1\n", "Use one of"),
            ("name: A\ntype: int\n", "has no default"),
            ("name: A\ntype: int\ndefault: x\n", "is not a number"),
            (
                "name: A\ntype: int\ndefault: 11\nrange: [1, 10]\n",
                "outside its range",
            ),
            (
                "name: A\ntype: int\ndefault: 1\nrange: [10, 1]\n",
                "starts above where it ends",
            ),
            (
                "name: A\ntype: int\ndefault: 1\nrange: [1]\n",
                "[smallest, largest]",
            ),
            (
                "name: A\ntype: bool\ndefault: 1\nrange: [0, 1]\n",
                "only an int or a hex",
            ),
            ("name: A\ntype: bool\ndefault: maybe\n", "true or false"),
        ] {
            let error = setting(text).unwrap_err();
            assert!(
                error.message.contains(wanted),
                "{text:?}: {}",
                error.message
            );
        }
    }

    #[test]
    fn names_become_symbols() {
        assert_eq!(symbol_of("ee24"), "EE24");
        assert_eq!(symbol_of("one-wire"), "ONE_WIRE");
    }

    #[test]
    fn a_cmake_argument_cannot_end_early_or_expand() {
        assert_eq!(cmake_quote("a b"), "\"a b\"");
        assert_eq!(cmake_quote("a\"b$c;d\\e"), "\"a\\\"b\\$c\\;d\\\\e\"");
    }

    #[test]
    fn a_prompt_is_one_line() {
        assert_eq!(one_line("a\n  b \"c\""), "a b \\\"c\\\"");
    }
}
