//! The library as a Zephyr module: zephyr/module.yml, CMakeLists.txt and Kconfig.
//!
//! Zephyr builds a module's code only when the module says how, in a folder called zephyr
//! inside it. A library written for any project has no such folder, so it is written here, from
//! libnima.yml, into the folder the library was installed in. It is written again on every install
//! and update, so a source file the new version adds is built without anyone editing anything.
//!
//! A library that ships its own zephyr/module.yml knows its build better than this does, and is
//! left alone.
//!
//! A library whose libnima.yml has zephyr.library.conf gets a fourth file there, its Kconfig
//! fragment, zephyr/<name>.conf, a line for each setting, which the application's CMakeLists.txt
//! adds to EXTRA_CONF_FILE. It is written whether the module is this one's or the library's own,
//! and taken out by the update whose libnima.yml no longer has zephyr.library.conf.
//!
//! A manifest can list its settings and lines in groups, each under a title, and give a setting a
//! note. Each file written from them carries both as comments: a group's title between two lines
//! of stars, a note above its setting.
//!
//! The module is named <library>_lib, not after the library itself. Zephyr keeps one module per
//! name, and a module added by the application quietly replaces a module of the workspace with the
//! same name: a library called littlefs would otherwise take the place of Zephyr's own littlefs.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

use super::{CONF_KEY, DEFAULTS_KEY, MODULE};
use crate::blocks::{Group, Marker};
use crate::files::{self, backup, unlink};
use crate::manifest::{LIBRARY, MANIFEST_NAME, Manifest};
use crate::platform::kconfig::{cmake_quote, generated, marks, one_line};
use crate::target::{Outcome, Status};
use crate::titles::{annotated, titled};
use crate::usercode;

pub const NAME: &str = "Zephyr module";

/// A symbol the Kconfig defines: config NAME, or menuconfig NAME.
static DEFINED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^(?:menu)?config[ \t]+([A-Za-z0-9_]+)").expect("a valid pattern")
});

/// Every first line taken as this one's, so a module an installer before wrote is still written
/// again, not refused.
fn our_marks() -> Vec<String> {
    marks("zephyr-installer")
}

/// The library's groups of CMake lines, and those of the options taken, in order.
pub fn groups(library: &Manifest) -> Vec<Group> {
    let mut found = library.part.zephyr().cmake.clone();

    for option in library.chosen_options() {
        found.extend(option.cmake.iter().cloned());
    }

    found
}

fn safe_name(name: &str) -> String {
    name.chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() || char == '_' || char == '-' {
                char
            } else {
                '_'
            }
        })
        .collect()
}

/// The module's name: ee24_lib for ee24.
pub fn module_name(library: &Manifest) -> String {
    format!("{}_lib", safe_name(&library.name))
}

/// A path inside the module, for its CMakeLists.txt.
fn in_module(relative: &str) -> String {
    if relative == "." || relative.is_empty() {
        return "\"${ZEPHYR_CURRENT_MODULE_DIR}\"".to_string();
    }

    format!(
        "\"${{ZEPHYR_CURRENT_MODULE_DIR}}/{}",
        &cmake_quote(relative)[1..]
    )
}

/// The name of the USER CODE section in the module's CMakeLists.txt: EXAMPLE_CMAKE.
fn section(library: &Manifest) -> String {
    format!("{}_CMAKE", library.part.zephyr().symbol)
}

fn label(library: &Manifest) -> String {
    format!("{} {}", library.name, library.version)
        .trim()
        .to_string()
}

/// zephyr/module.yml, which tells Zephyr where the other two files are.
fn module_yml(library: &Manifest) -> String {
    format!(
        "{mark} for {label}. Written again on\n\
         # every install and update, so a change made here does not last.\n\
         \n\
         name: {name}\n\
         build:\n  cmake: zephyr\n  kconfig: zephyr/Kconfig\n",
        mark = crate::MARK,
        label = label(library),
        name = module_name(library),
    )
}

/// zephyr/CMakeLists.txt: the library's files, built when its symbol is set.
fn cmakelists(library: &Manifest) -> String {
    let symbol = &library.part.zephyr().symbol;
    let sources = library.build_sources();
    let mut lines = vec![
        format!("{} for {}.", crate::MARK, label(library)),
        "#".to_string(),
        "# Zephyr reads this because the application's CMakeLists.txt adds this folder".to_string(),
        format!("# to ZEPHYR_EXTRA_MODULES, and builds the library only when CONFIG_{symbol} is"),
        "# set, which prj.conf does. Written again on every install and update, so a".to_string(),
        "# change made here does not last. The files come from the library's".to_string(),
        format!("# {MANIFEST_NAME}."),
        String::new(),
        format!("if(CONFIG_{symbol})"),
    ];

    // On the include path of the whole build, the application's code too, which is what includes
    // the library's header.
    lines.push("  zephyr_include_directories(".to_string());
    lines.extend(
        library
            .include_dirs()
            .iter()
            .map(|folder| format!("    {}", in_module(folder))),
    );
    lines.push("  )".to_string());

    // A library of headers only has nothing to compile, and Zephyr warns about a library with no
    // sources.
    if !sources.is_empty() {
        lines.push(String::new());
        lines.push(format!("  zephyr_library_named({})", module_name(library)));
        lines.push("  zephyr_library_sources(".to_string());
        lines.extend(
            sources
                .iter()
                .map(|name| format!("    {}", in_module(name))),
        );
        lines.push("  )".to_string());
    }

    // For the whole build, not only the library's own files: a define can change what the
    // library's header declares, and every file including it has to agree.
    if !library.defines.is_empty() {
        lines.push(String::new());
        lines.push("  zephyr_compile_definitions(".to_string());
        lines.extend(library.defines.iter().map(|define| format!("    {define}")));
        lines.push("  )".to_string());
    }

    // The groups whose place is the module, each between markers that name it, so a reader finds
    // each again.
    for group in groups(library) {
        if group.place != MODULE {
            continue;
        }

        lines.push(String::new());

        let found = Marker::new(&library.name, Some(&group.label));
        lines.push(format!("  {}", found.open));
        lines.push(format!(
            "  # From {}'s {MANIFEST_NAME}, and written again on every install.",
            library.name
        ));

        // {library} in a manifest's lines is the module's own folder.
        for line in &group.lines {
            let line = line.replace(LIBRARY, "${ZEPHYR_CURRENT_MODULE_DIR}");

            lines.push(if line.is_empty() {
                line
            } else {
                format!("  {line}")
            });
        }

        lines.push(format!("  {}", found.close));
    }

    // The user's own lines, which every install carries over. Empty to start with, so anything in
    // it on an uninstall is the user's. A library of headers only has no library of its own to
    // give settings to.
    let example = if sources.is_empty() {
        "zephyr_compile_definitions()"
    } else {
        "zephyr_library_compile_definitions()"
    };

    lines.extend([
        String::new(),
        format!("  # Lines of your own for {}, such as", library.name),
        format!("  # {example}, go between these two, and every"),
        "  # install keeps them.".to_string(),
        format!("  # USER CODE BEGIN {}", section(library)),
        format!("  # USER CODE END {}", section(library)),
        "endif()".to_string(),
    ]);

    format!("{}\n", lines.join("\n"))
}

/// A Kconfig help block, each line indented under it.
fn help(lines: &[String]) -> Vec<String> {
    let mut out = vec!["\thelp".to_string()];

    out.extend(lines.iter().map(|line| {
        if line.trim().is_empty() {
            String::new()
        } else {
            format!("\t  {line}")
        }
    }));

    out
}

/// zephyr/Kconfig: the library's switch, and its settings under it.
fn kconfig(library: &Manifest) -> String {
    let part = library.part.zephyr();
    let prompt = if library.description.is_empty() {
        library.name.clone()
    } else {
        format!("{}: {}", library.name, library.description)
    };
    let about: Vec<String> = [&library.description, &library.repository]
        .into_iter()
        .filter(|line| !line.is_empty())
        .cloned()
        .collect();
    let keyword = if part.settings.is_empty() {
        "config"
    } else {
        "menuconfig"
    };

    let mut lines = vec![
        format!("{} for {}. Written again on", crate::MARK, label(library)),
        "# every install and update, so a change made here does not last.".to_string(),
        String::new(),
        format!("{keyword} {}", part.symbol),
        format!("\tbool \"{}\"", one_line(&prompt)),
    ];

    if !about.is_empty() {
        lines.extend(help(&about));
    }

    if !part.settings.is_empty() {
        lines.push(String::new());
        lines.push(format!("if {}", part.symbol));

        for setting in &part.settings {
            lines.push(String::new());
            lines.push(format!("config {}", setting.name));
            lines.push(format!("\t{} \"{}\"", setting.kind, setting.prompt));
            lines.push(format!("\tdefault {}", setting.default));

            if let Some((low, high)) = setting.range {
                lines.push(format!(
                    "\trange {} {}",
                    setting.format(low),
                    setting.format(high)
                ));
            }

            if !setting.help.is_empty() {
                lines.extend(help(&setting.help));
            }
        }

        lines.push(String::new());
        lines.push(format!("endif # {}", part.symbol));
    }

    // The library's settings of other symbols, from zephyr.library.kconfig. Defaults, not
    // assignments: they hold only while the library is on, and a line of the application's own
    // Kconfig files that sets the same symbol wins. This file is read before Zephyr's subsystems,
    // so they come before the subsystems' own defaults, and win over those.
    if !part.defaults.is_empty() {
        lines.push(String::new());
        lines.push(format!(
            "# {}'s settings of other symbols, from {DEFAULTS_KEY} in its {MANIFEST_NAME}.",
            library.name
        ));
        lines.push(format!(
            "# They hold while {} is on. A line in prj.conf that sets one wins.",
            part.symbol
        ));
        lines.push(format!("if {}", part.symbol));

        let mut title: Option<&str> = None;

        // Each group's title between two lines of stars above its first setting, and a setting's
        // note above it, as a comment.
        for default in &part.defaults {
            if let Some(group) = default.group.as_deref()
                && Some(group) != title
            {
                lines.push(String::new());
                lines.extend(titled(group, "#"));
                title = Some(group);
            }

            lines.push(String::new());

            if let Some(note) = &default.note {
                lines.push(format!("# {note}"));
            }

            lines.push(format!("configdefault {}", default.item.symbol));
            lines.push(format!("\tdefault {}", default.item.value));
        }

        lines.push(String::new());
        lines.push(format!("endif # {}", part.symbol));
    }

    format!("{}\n", lines.join("\n"))
}

/// The Kconfig fragment's name in the zephyr folder: gcore.conf for gcore.
pub fn fragment_name(library: &str) -> String {
    format!("{}.conf", safe_name(library))
}

/// zephyr/<name>.conf: the settings of the library's zephyr.library.conf, a line each, under
/// their groups' titles and with their notes, or None when it has none. Zephyr reads it after
/// prj.conf, so a line here wins over one there.
fn fragment(library: &Manifest) -> Option<String> {
    let settings = &library.part.zephyr().conf;

    if settings.is_empty() {
        return None;
    }

    let mut lines = vec![
        format!("{} for {}. Written again on", crate::MARK, label(library)),
        "# every install and update, so a change made here does not last.".to_string(),
        "#".to_string(),
        format!(
            "# {}'s settings, from {CONF_KEY} in its {MANIFEST_NAME}. The",
            library.name
        ),
        "# application's CMakeLists.txt adds this file to EXTRA_CONF_FILE, so Zephyr".to_string(),
        "# reads it after prj.conf, and a line here wins over one there.".to_string(),
        String::new(),
    ];

    lines.extend(annotated(settings, |setting| setting.line(), "#", true));

    Some(format!("{}\n", lines.join("\n")))
}

/// Whether the library brought a zephyr/module.yml of its own.
fn ships_its_own(destination: &Path) -> bool {
    let path = destination.join("zephyr").join("module.yml");

    match files::read(&path) {
        Ok(text) => !our_marks()
            .iter()
            .any(|mark| text.starts_with(mark.as_str())),
        Err(_) => path.is_file(),
    }
}

/// Write the three files and the Kconfig fragment, each only when it changed, and take out a
/// fragment the library no longer has. A library with its own module.yml gets the fragment alone.
pub fn write(library: &Manifest, destination: &Path) -> Outcome {
    let folder = destination.join("zephyr");
    let own = ships_its_own(destination);
    let mut wanted: Vec<(String, Option<String>)> = Vec::new();

    if !own {
        wanted.push(("module.yml".to_string(), Some(module_yml(library))));
        wanted.push(("CMakeLists.txt".to_string(), Some(cmakelists(library))));
        wanted.push(("Kconfig".to_string(), Some(kconfig(library))));
    }

    // None with no zephyr.library.conf, so the fragment of a version that had one goes.
    wanted.push((fragment_name(&library.name), fragment(library)));

    let mut written = Vec::new();
    let mut removed = Vec::new();
    let mut steps = Vec::new();
    let mut saved = None;

    let mut each = || -> std::io::Result<()> {
        for (name, text) in &wanted {
            let path = folder.join(name);

            let Some(text) = text else {
                if generated(&path, &our_marks()) {
                    fs::remove_file(&path)?;
                    removed.push(name.clone());
                }

                continue;
            };

            fs::create_dir_all(&folder)?;

            let mut data = text.clone().into_bytes();
            let old = if path.is_file() {
                Some(fs::read(&path)?)
            } else {
                None
            };

            // What the user wrote between the USER CODE lines of the last copy goes into the new
            // one. Markers that no longer pair leave nothing to carry, so the copy is put aside
            // first, whole.
            if let Some(old) = old.as_ref().filter(|_| name == "CMakeLists.txt")
                && let Ok(merged) = usercode::merge(&data, old)
            {
                if merged.broken.is_some() || !merged.lost.is_empty() {
                    let copy = backup(&path)?;
                    let shown = copy
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    steps.push(format!("Your lines in zephyr/{name} could not be carried over: they are in {shown}."));
                    saved = Some(copy);
                } else {
                    data = merged.data;
                }

                if !merged.kept.is_empty() {
                    steps.push(format!(
                        "Kept your USER CODE {} in zephyr/{name}.",
                        merged.kept.join(", ")
                    ));
                }
            }

            // Left alone when nothing changed, so the build does not see a newer file and configure
            // again for nothing.
            if old.as_ref() == Some(&data) {
                continue;
            }

            fs::write(&path, &data)?;
            written.push(name.clone());
        }

        Ok(())
    };

    if let Err(error) = each() {
        return Outcome::new(
            NAME,
            Status::Manual,
            format!(
                "Could not write {}: {}",
                folder.display(),
                files::reason(&error)
            ),
        );
    }

    let mut said: Vec<String> = if own {
        vec![format!(
            "{} has its own zephyr/module.yml, used as it is.",
            library.name
        )]
    } else {
        Vec::new()
    };

    if written.is_empty() && removed.is_empty() {
        let message = if own {
            said.remove(0)
        } else {
            format!("{} is up to date.", module_name(library))
        };

        return Outcome::new(NAME, Status::Already, message).with_steps(steps);
    }

    if !written.is_empty() {
        let names = format!("zephyr/{}", written.join(", zephyr/"));

        said.push(if own {
            format!("Wrote {names}.")
        } else {
            format!("Wrote {}: {names}.", module_name(library))
        });
    }

    if let Some(first) = removed.first() {
        said.push(format!(
            "Removed zephyr/{first}: {} has no zephyr.library.conf now.",
            library.name
        ));
    }

    Outcome::new(NAME, Status::Changed, said.join(" "))
        .with_steps(steps)
        .with_backup(saved)
}

/// Whether a CMakeLists.txt write() made has anything between its USER CODE lines.
fn holds_yours(path: &Path) -> bool {
    let Ok(data) = fs::read(path) else {
        return false;
    };

    usercode::parse(&data).map_or(true, |parsed| parsed.holds_something())
}

/// Delete the files write() made, the three and the Kconfig fragment, and the zephyr folder when
/// they were all it held. A library's own module is not touched.
///
/// Returns the copy put aside of a CMakeLists.txt that had the user's lines in it, or None.
pub fn remove(destination: &Path) -> Option<PathBuf> {
    let folder = destination.join("zephyr");
    let mut saved = None;

    // The fragment is named after the library, which the folder does not say, so any .conf there
    // this wrote goes.
    let mut found: Vec<PathBuf> = ["module.yml", "CMakeLists.txt", "Kconfig"]
        .iter()
        .map(|name| folder.join(name))
        .collect();

    if let Ok(entries) = fs::read_dir(&folder) {
        let mut confs: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "conf")
            })
            .collect();
        confs.sort();
        found.extend(confs);
    }

    for path in found {
        if !generated(&path, &our_marks()) {
            continue;
        }

        if path
            .file_name()
            .is_some_and(|name| name == "CMakeLists.txt")
            && holds_yours(&path)
        {
            saved = backup(&path).ok();
        }

        let _ = unlink(&path);
    }

    let _ = fs::remove_dir(&folder);

    saved
}

/// Every symbol the Kconfig write() made defines, read before it is removed.
pub fn symbols(destination: &Path) -> Vec<String> {
    let Ok(text) = files::read(&destination.join("zephyr").join("Kconfig")) else {
        return Vec::new();
    };

    if !our_marks()
        .iter()
        .any(|mark| text.starts_with(mark.as_str()))
    {
        return Vec::new();
    }

    DEFINED
        .captures_iter(&text)
        .map(|found| found[1].to_string())
        .collect()
}
