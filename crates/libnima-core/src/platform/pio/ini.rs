//! platformio.ini: the defines and the build flags a library gives the whole project.
//!
//! A define in a library's library.json reaches only the library's own files, as measured: the
//! project's own code, which includes the library's header, never sees it. A define can change what
//! that header declares, and every file that includes it has to agree, so a library's defines go into
//! platformio.ini instead, where PlatformIO hands build_flags to every file of the build. So do the
//! flags of pio.project.build_flags, after the defines, each group under its title between two lines of
//! stars, and a flag's note above it, as comments of the file's, ; like the markers:
//!
//! ```ini
//! build_flags =
//!     -DFROM_YOU=1
//!     ; >>> libnima: example >>>
//!     -DEXAMPLE_FAST
//!     ; ******************************************************************************
//!     ; Printing
//!     ; ******************************************************************************
//!     ; printf("%f") prints nothing without it
//!     -Wl,-u,_printf_float
//!     ; <<< libnima: example <<<
//! ```
//!
//! A block goes wherever build_flags reaches an environment: into [env], which every [env:name] without
//! build_flags of its own takes, and into each [env:name] that has its own, since it takes nothing from
//! [env]. A project with no [env] section and an environment without build_flags gets an [env] section
//! for them. An uninstall takes out the blocks, and what was added to hold them, so the file is back as
//! it was.
//!
//! The file is the user's, so it is edited as text, never through a parser that would drop its comments:
//! a copy is taken before it changes, its line endings are kept, and a second install adds nothing twice.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::blocks::{self, Marker};
use crate::files::{self, backup};
use crate::manifest::Manifest;
use crate::target::{Outcome, Status};
use crate::text::{line_ending, lines_of};
use crate::titles::annotated;

pub const NAME: &str = "platformio.ini";

const INDENT: &str = "    ";

static SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\[([^\]]+)\][ \t]*$").expect("a valid pattern"));
static FLAGS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^build_flags[ \t]*=(.*)$").expect("a valid pattern"));
static FRAMEWORK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^framework[ \t]*=[ \t]*(.*?)[ \t]*$").expect("a valid pattern"));
static PLATFORM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^platform[ \t]*=[ \t]*(.*?)[ \t]*$").expect("a valid pattern"));

/// The two lines around a library's block, the core's, with ; as the sign.
fn marker(name: &str) -> Marker {
    Marker::signed(name, None, ";")
}

/// Every section: its name, the line of its header, and the line after its last.
fn sections(lines: &[String]) -> Vec<(String, usize, usize)> {
    let found: Vec<(String, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            SECTION
                .captures(line)
                .map(|found| (found[1].trim().to_string(), index))
        })
        .collect();

    found
        .iter()
        .enumerate()
        .map(|(position, (name, start))| {
            let end = found
                .get(position + 1)
                .map_or(lines.len(), |(_, next)| *next);

            (name.clone(), *start, end)
        })
        .collect()
}

fn starts_indented(line: &str) -> bool {
    line.starts_with([' ', '\t'])
}

/// build_flags in a section: its line, and its last value line.
fn flags(lines: &[String], start: usize, end: usize) -> Option<(usize, usize)> {
    for index in start + 1..end {
        if FLAGS.is_match(&lines[index]) {
            let mut last = index;

            while last + 1 < end
                && starts_indented(&lines[last + 1])
                && !lines[last + 1].trim().is_empty()
            {
                last += 1;
            }

            return Some((index, last));
        }
    }

    None
}

/// Each [env:name], with the framework and the platform it builds with. One it does not set itself comes
/// from [env].
pub fn environments(text: &str) -> Vec<(String, Vec<String>, String)> {
    let lines: Vec<String> = lines_of(text).iter().map(ToString::to_string).collect();
    let mut common = (String::new(), String::new());
    let mut found: Vec<(String, Option<String>, Option<String>)> = Vec::new();

    for (name, start, end) in sections(&lines) {
        let mut framework = None;
        let mut platform = None;

        for line in &lines[start + 1..end] {
            if let Some(matched) = FRAMEWORK.captures(line) {
                framework = Some(matched[1].to_string());
            }

            if let Some(matched) = PLATFORM.captures(line) {
                platform = Some(matched[1].to_string());
            }
        }

        if name == "env" {
            if let Some(framework) = framework {
                common.0 = framework;
            }

            if let Some(platform) = platform {
                common.1 = platform;
            }
        } else if let Some(env) = name.strip_prefix("env:") {
            found.push((env.to_string(), framework, platform));
        }
    }

    found
        .into_iter()
        .map(|(env, framework, platform)| {
            let framework = framework.unwrap_or_else(|| common.0.clone());
            let frameworks = framework
                .split(',')
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();

            (
                env,
                frameworks,
                platform.unwrap_or_else(|| common.1.clone()),
            )
        })
        .collect()
}

/// The library's build flags, from pio.project.build_flags, as lines of its block: each group's title
/// between two lines of stars, and a flag's note above it, as comments. No blank line, though: in an ini
/// file one reads as the end of the value, to a person and to many a tool, if not to PlatformIO.
fn flag_lines(library: &Manifest) -> Vec<String> {
    annotated(&library.part.pio().build_flags, String::clone, ";", false)
}

/// What a block holds, in words: the defines, the build flags, or both.
fn what(defines: bool, flags: bool) -> &'static str {
    match (defines, flags) {
        (true, true) => "defines and build flags",
        (false, true) => "build flags",
        _ => "defines",
    }
}

/// What this library's blocks in text hold, in words, read from their lines.
fn held(text: &str, name: &str) -> &'static str {
    let mut lines: Vec<String> = Vec::new();

    for (start, end) in blocks::spans(text, &marker(name)).unwrap_or_default() {
        let found = lines_of(&text[start..end]);

        if found.len() > 2 {
            lines.extend(
                found[1..found.len() - 1]
                    .iter()
                    .map(|line| line.trim().to_string()),
            );
        }
    }

    what(
        lines.iter().any(|line| line.starts_with("-D")),
        lines
            .iter()
            .any(|line| !line.is_empty() && !line.starts_with("-D") && !line.starts_with(';')),
    )
}

fn block(name: &str, defines: &[String], indent: &str, extra: &[String]) -> Vec<String> {
    let found = marker(name);
    let mut lines = vec![format!("{indent}{}", found.open)];

    lines.extend(defines.iter().map(|define| format!("{indent}-D{define}")));
    lines.extend(extra.iter().map(|line| format!("{indent}{line}")));
    lines.push(format!("{indent}{}", found.close));

    lines
}

/// The indent of the value lines already there, or the usual four spaces.
fn indent_of(lines: &[String], key: usize, last: usize) -> String {
    if key < last {
        let line = &lines[key + 1];
        let indent = &line[..line.len() - line.trim_start_matches([' ', '\t']).len()];

        if !indent.is_empty() {
            return indent.to_string();
        }
    }

    INDENT.to_string()
}

/// The lines with this library's blocks taken out, and what was there only to hold them: a build_flags
/// left with no value, and an [env] left empty.
fn without(lines: &[String], name: &str) -> Vec<String> {
    let found = marker(name);
    let mut kept: Vec<String> = Vec::new();
    let mut inside = false;
    let mut held: Vec<usize> = Vec::new();

    for line in lines {
        if line.trim() == found.open {
            inside = true;

            // The build_flags this block was in, which may now hold nothing.
            for back in (0..kept.len()).rev() {
                if FLAGS.is_match(&kept[back]) {
                    held.push(back);
                    break;
                }

                if !starts_indented(&kept[back]) {
                    break;
                }
            }

            continue;
        }

        if inside {
            inside = line.trim() != found.close;
            continue;
        }

        kept.push(line.clone());
    }

    // A build_flags with nothing on its line and nothing after it any more.
    let emptied: Vec<usize> = held
        .into_iter()
        .filter(|index| {
            let on_line = FLAGS
                .captures(&kept[*index])
                .is_some_and(|found| !found[1].trim().is_empty());
            let after = kept
                .get(index + 1)
                .is_some_and(|line| starts_indented(line) && !line.trim().is_empty());

            !on_line && !after
        })
        .collect();

    let had_emptied = !emptied.is_empty();
    let mut kept: Vec<String> = kept
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !emptied.contains(index))
        .map(|(_, line)| line)
        .collect();

    // An [env] that held nothing but the emptied build_flags, with the blank line it brought. A comment
    // of the user's in it keeps it.
    if had_emptied {
        for (section, start, end) in sections(&kept).into_iter().rev() {
            if section == "env"
                && !kept[start + 1..end]
                    .iter()
                    .any(|line| !line.trim().is_empty())
            {
                kept.drain(start..end);
            }
        }
    }

    kept
}

/// The lines with this library's defines, and the lines of its build flags, added wherever build_flags
/// reaches an environment.
fn with_block(
    mut lines: Vec<String>,
    name: &str,
    defines: &[String],
    extra: &[String],
) -> Vec<String> {
    let found = sections(&lines);
    let common = found.iter().find(|(section, _, _)| section == "env");
    let envs: Vec<&(String, usize, usize)> = found
        .iter()
        .filter(|(section, _, _)| section.starts_with("env:"))
        .collect();
    let mut inserts: Vec<(usize, Vec<String>)> = Vec::new();

    for (_, start, end) in &envs {
        if let Some((key, last)) = flags(&lines, *start, *end) {
            inserts.push((
                last + 1,
                block(name, defines, &indent_of(&lines, key, last), extra),
            ));
        }
    }

    if let Some((_, start, end)) = common {
        if let Some((key, last)) = flags(&lines, *start, *end) {
            inserts.push((
                last + 1,
                block(name, defines, &indent_of(&lines, key, last), extra),
            ));
        } else {
            let last = (*start + 1..*end)
                .filter(|index| !lines[*index].trim().is_empty())
                .max()
                .unwrap_or(*start);
            let mut added = vec!["build_flags =".to_string()];
            added.extend(block(name, defines, INDENT, extra));
            inserts.push((last + 1, added));
        }
    } else if envs
        .iter()
        .any(|(_, start, end)| flags(&lines, *start, *end).is_none())
    {
        let first = envs[0].1;
        let mut added = vec!["[env]".to_string(), "build_flags =".to_string()];
        added.extend(block(name, defines, INDENT, extra));
        added.push(String::new());
        inserts.push((first, added));
    }

    inserts.sort_by_key(|(index, _)| std::cmp::Reverse(*index));

    for (index, added) in inserts {
        lines.splice(index..index, added);
    }

    lines
}

/// The file's text with this library's defines and the lines of its build flags, or none of them when it
/// has none.
fn updated(text: &str, name: &str, defines: &[String], extra: &[String]) -> String {
    let newline = line_ending(text);
    let lines: Vec<String> = text.split(newline).map(str::to_string).collect();
    let mut lines = without(&lines, name);

    if !defines.is_empty() || !extra.is_empty() {
        lines = with_block(lines, name, defines, extra);
    }

    lines.join(newline)
}

/// The flags to add by hand when the file cannot be written: its defines, then its build flags.
fn by_hand(library: &Manifest) -> Vec<String> {
    let mut found: Vec<String> = library
        .defines
        .iter()
        .map(|define| format!("-D{define}"))
        .collect();

    found.extend(
        library
            .part
            .pio()
            .build_flags
            .iter()
            .map(|flag| flag.item.clone()),
    );

    found
}

/// Give the project the library's defines and build flags. None when there is nothing to do.
pub fn integrate(project_root: &Path, library: &Manifest) -> Option<Outcome> {
    let path = project_root.join(NAME);
    let extra = flag_lines(library);
    let wanted = !library.defines.is_empty() || !extra.is_empty();
    let words = what(!library.defines.is_empty(), !extra.is_empty());
    let name = &library.name;

    let text = match files::read(&path) {
        Ok(text) => text,
        Err(error) => {
            if !wanted {
                return None;
            }

            let mut steps = vec!["Add these to build_flags of every environment:".to_string()];
            steps.extend(by_hand(library));

            return Some(
                Outcome::new(
                    NAME,
                    Status::Manual,
                    format!("Could not read {NAME}: {}", files::reason(&error)),
                )
                .with_steps(steps),
            );
        }
    };

    // A block with one of its markers missing: whatever was written could take the user's lines with it,
    // so the file is left as it is.
    if blocks::spans(&text, &marker(name)).is_err() {
        let mut steps = vec!["Then add these to build_flags of every environment:".to_string()];
        steps.extend(by_hand(library));

        return Some(
            Outcome::new(
                NAME,
                Status::Manual,
                format!(
                    "{NAME} has a half written libnima block for {name}. Fix it by hand first."
                ),
            )
            .with_steps(steps),
        );
    }

    let new = updated(&text, name, &library.defines, &extra);
    let with_blocks = |outcome: Outcome| {
        if wanted {
            outcome.with_blocks(NAME, blocks::fingerprints(&new, &[marker(name)]))
        } else {
            outcome
        }
    };

    if new == text {
        if !wanted {
            return None;
        }

        return Some(with_blocks(Outcome::new(
            NAME,
            Status::Already,
            format!("{NAME} already has {name}'s {words}."),
        )));
    }

    let saved = backup(&path).ok();

    if let Err(error) = files::write(&path, &new) {
        return Some(
            Outcome::new(
                NAME,
                Status::Manual,
                format!("Could not write {NAME}: {}", files::reason(&error)),
            )
            .with_backup(saved),
        );
    }

    let done = if wanted {
        format!("Added {name}'s {words} to {NAME}, for every environment.")
    } else {
        format!("Took {name}'s old {} out of {NAME}.", held(&text, name))
    };

    Some(with_blocks(
        Outcome::new(NAME, Status::Changed, done).with_backup(saved),
    ))
}

/// Take this library's defines and build flags out of platformio.ini. None when it has none there.
pub fn uninstall(project_root: &Path, name: &str) -> Option<Outcome> {
    let path = project_root.join(NAME);
    let text = files::read(&path).ok()?;

    if blocks::spans(&text, &marker(name)).is_err() {
        return Some(Outcome::new(
            NAME,
            Status::Manual,
            format!("{NAME} has a half written libnima block for {name}. Take it out by hand."),
        ));
    }

    let new = updated(&text, name, &[], &[]);

    if new == text {
        return None;
    }

    let saved = backup(&path).ok();

    if let Err(error) = files::write(&path, &new) {
        return Some(
            Outcome::new(
                NAME,
                Status::Manual,
                format!("Could not write {NAME}: {}", files::reason(&error)),
            )
            .with_backup(saved),
        );
    }

    Some(
        Outcome::new(
            NAME,
            Status::Changed,
            format!("Removed {name}'s {} from {NAME}.", held(&text, name)),
        )
        .with_backup(saved),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_named(text: &str, name: &str, defines: &[&str]) -> String {
        let defines: Vec<String> = defines.iter().map(ToString::to_string).collect();

        updated(text, name, &defines, &[])
    }

    fn add(text: &str) -> String {
        add_named(text, "demo", &["DEMO_FAST"])
    }

    /// Added, then taken out: the text it comes back as.
    fn round(text: &str) -> String {
        updated(&add(text), "demo", &[], &[])
    }

    const BLOCK: &str =
        "    ; >>> libnima: demo >>>\n    -DDEMO_FAST\n    ; <<< libnima: demo <<<\n";

    #[test]
    fn an_env_with_no_build_flags_gets_an_env_section_above_it() {
        assert_eq!(
            add("[platformio]\ndefault_envs = native\n\n[env:native]\nplatform = native\n"),
            format!(
                "[platformio]\ndefault_envs = native\n\n[env]\nbuild_flags =\n{BLOCK}\n[env:native]\nplatform = native\n"
            )
        );
    }

    #[test]
    fn an_env_with_its_own_build_flags_gets_the_block_in_them() {
        assert_eq!(
            add("[env:native]\nplatform = native\nbuild_flags =\n\t-DYOURS=1\nlib_deps = x\n"),
            "[env:native]\nplatform = native\nbuild_flags =\n\t-DYOURS=1\n\t; >>> libnima: demo >>>\n\t-DDEMO_FAST\n\t; <<< libnima: demo <<<\nlib_deps = x\n"
        );
    }

    #[test]
    fn a_build_flags_on_one_line_takes_the_block_under_it() {
        assert_eq!(
            add("[env:native]\nbuild_flags = -DYOURS=1\n"),
            format!("[env:native]\nbuild_flags = -DYOURS=1\n{BLOCK}")
        );
    }

    #[test]
    fn an_env_section_without_build_flags_gets_them_and_no_second_env() {
        assert_eq!(
            add("[env]\nframework = arduino\n\n[env:uno]\nplatform = atmelavr\n"),
            format!(
                "[env]\nframework = arduino\nbuild_flags =\n{BLOCK}\n[env:uno]\nplatform = atmelavr\n"
            )
        );
    }

    #[test]
    fn both_env_and_an_env_with_its_own_get_a_block() {
        let added = add(
            "[env]\nbuild_flags = -DCOMMON\n\n[env:a]\nplatform = native\n\n[env:b]\nbuild_flags = -DB\n",
        );

        assert_eq!(added.matches(">>> libnima: demo >>>").count(), 2, "{added}");
        assert!(
            added.starts_with("[env]\nbuild_flags = -DCOMMON\n    ; >>> libnima: demo >>>"),
            "{added}"
        );
        assert!(added.contains("[env:a]\nplatform = native\n\n"), "{added}");
    }

    #[test]
    fn installing_twice_writes_one_block() {
        let text = "[env:native]\nplatform = native\n";

        assert_eq!(add(&add(text)), add(text));
    }

    #[test]
    fn new_defines_replace_the_old() {
        let text = "[env:native]\nplatform = native\n";
        let old = add_named(text, "demo", &["OLD=1", "GONE"]);

        assert_eq!(
            add_named(&old, "demo", &["NEW=2"]),
            add_named(text, "demo", &["NEW=2"])
        );
    }

    #[test]
    fn no_defines_now_takes_the_block_out() {
        let text = "[env:native]\nplatform = native\n";

        assert_eq!(updated(&add(text), "demo", &[], &[]), text);
    }

    #[test]
    fn two_libraries_keep_a_block_each() {
        let text = "[env:native]\nplatform = native\n";
        let both = add_named(&add_named(text, "one", &["ONE"]), "two", &["TWO"]);

        assert_eq!(both.matches(">>> libnima: one >>>").count(), 1, "{both}");
        assert_eq!(both.matches(">>> libnima: two >>>").count(), 1, "{both}");
        assert_eq!(
            updated(&updated(&both, "two", &[], &[]), "one", &[], &[]),
            text
        );
    }

    #[test]
    fn taken_out_again_the_file_is_back_to_the_byte() {
        for text in [
            "[env:native]\nplatform = native\n",
            "[env:native]\nplatform = native\nbuild_flags =\n    -DYOURS\n",
            "[env:native]\nbuild_flags = -DYOURS\n",
            "[env]\nframework = arduino\n\n[env:uno]\nplatform = atmelavr\n",
            "[env]\nbuild_flags = -DC\n\n[env:a]\nplatform = native\n\n[env:b]\nbuild_flags = -DB\n",
            "; mine\n[platformio]\ndefault_envs = a\n\n[env:a]\nplatform = native\n\n[env:b]\nplatform = native\n",
            "[env:native]\nplatform = native",
            "[env]\n; my notes\n\n[env:a]\nplatform = native\n",
        ] {
            let crlf = text.replace('\n', "\r\n");

            assert_eq!(round(text), text);
            assert_eq!(round(&crlf), crlf);
        }
    }

    #[test]
    fn crlf_is_kept_in_what_is_added() {
        assert!(
            !add("[env:native]\r\nplatform = native\r\n")
                .replace("\r\n", "")
                .contains('\n')
        );
    }

    #[test]
    fn a_block_of_another_library_is_left_alone() {
        let text = add_named("[env:native]\nplatform = native\n", "other", &["OTHER"]);

        assert_eq!(updated(&text, "demo", &[], &[]), text);
    }

    #[test]
    fn the_environments_and_what_they_build_with() {
        let text = "[env]\nframework = arduino\nplatform = atmelavr\n\n[env:uno]\nboard = uno\n\n[env:esp]\nplatform = platformio/espressif32@^6.5\nframework = arduino, espidf\n";

        assert_eq!(
            environments(text),
            [
                (
                    "uno".to_string(),
                    vec!["arduino".to_string()],
                    "atmelavr".to_string()
                ),
                (
                    "esp".to_string(),
                    vec!["arduino".to_string(), "espidf".to_string()],
                    "platformio/espressif32@^6.5".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_line_of_spaces_ends_the_value() {
        assert_eq!(
            add("[env:native]\nbuild_flags = -DA\n   \nlib_deps = x\n"),
            format!("[env:native]\nbuild_flags = -DA\n{BLOCK}   \nlib_deps = x\n")
        );
    }
}
