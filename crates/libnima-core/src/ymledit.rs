//! Changing one thing in a libnima.yml's text and nothing else.
//!
//! The window's form and the fixes it offers change a file a person also writes by hand, so its comments, its order and how it
//! is laid out have to stay. So this does not write the file out again: it finds the lines of the one value and changes those.
//! And it does not trust itself: the text it makes is read again with the same reader an install uses, and has to hold what
//! the old text held with that one value changed. When it does not, the file is left as it was and the change is refused.

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::schema::{self, Shape};
use crate::yaml::{self, Entry, Mapping, Node, Value, comment_start};

/// A line is no longer than this when a list is put on one.
const LONGEST_LINE: usize = 100;

/// A step on the way to a value: a key of a mapping, or an item of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Step {
    Key(String),
    Item(usize),
}

impl From<&str> for Step {
    fn from(key: &str) -> Step {
        Step::Key(key.to_string())
    }
}

impl From<usize> for Step {
    fn from(item: usize) -> Step {
        Step::Item(item)
    }
}

/// The path of the table for a place in a file.
fn table_path(at: &[Step]) -> String {
    let mut path = String::new();

    for step in at {
        match step {
            Step::Key(key) => path = schema::below(&path, key),
            Step::Item(_) => path.push_str("[]"),
        }
    }

    path
}

fn shown(at: &[Step]) -> String {
    let parts: Vec<String> = at
        .iter()
        .map(|step| match step {
            Step::Key(key) => key.clone(),
            Step::Item(item) => item.to_string(),
        })
        .collect();

    parts.join(".")
}

/// The text as lines, and how it ends them.
struct Doc {
    lines: Vec<String>,
    crlf: bool,
}

impl Doc {
    fn new(text: &str) -> Doc {
        let crlf = text.contains("\r\n");
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| line.trim_end_matches('\r').to_string())
            .collect();

        // The line after the last line ending is not a line of the file.
        if lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }

        Doc { lines, crlf }
    }

    fn text(&self) -> String {
        let end = if self.crlf { "\r\n" } else { "\n" };
        let mut text = self.lines.join(end);

        if !self.lines.is_empty() {
            text.push_str(end);
        }

        text
    }

    fn indent(&self, index: usize) -> usize {
        let line = &self.lines[index];

        line.len() - line.trim_start_matches(' ').len()
    }

    fn is_quiet(&self, index: usize) -> bool {
        let line = self.lines[index].trim_start();

        line.is_empty() || line.starts_with('#')
    }

    /// Where the key or the value of a line stands, past a dash.
    fn column(&self, index: usize) -> usize {
        let indent = self.indent(index);
        let line = &self.lines[index][indent..];

        match line.strip_prefix("- ") {
            Some(after) => indent + 2 + (after.len() - after.trim_start_matches(' ').len()),
            None => indent,
        }
    }

    fn has_dash(&self, index: usize) -> bool {
        let line = self.lines[index].trim_start();

        line == "-" || line.starts_with("- ")
    }

    /// The lines from `first` up to `next` that belong to what starts on `first`: comments indented deeper than it are its
    /// own, and those at its depth or less, and empty lines at the end, lead into what comes next.
    fn span(&self, first: usize, next: usize, depth: usize) -> (usize, usize) {
        let mut last = next;

        while last > first + 1
            && self.is_quiet(last - 1)
            && (self.lines[last - 1].trim().is_empty() || self.indent(last - 1) <= depth)
        {
            last -= 1;
        }

        (first, last)
    }

    /// The comment at the end of a line, with the space before it.
    fn comment_of(&self, index: usize) -> String {
        let line = &self.lines[index];

        comment_start(line).map_or_else(String::new, |at| {
            let before = line[..at].trim_end().len();

            line[before..].to_string()
        })
    }

    /// A line with what is after its key's colon changed, its comment kept.
    fn set_inline(&mut self, index: usize, written: &str) {
        let comment = self.comment_of(index);
        let line = &self.lines[index];
        let column = self.column(index);
        let body = comment_start(line).map_or(line.as_str(), |at| line[..at].trim_end());
        let colon = colon_at(&body[column..]).map_or(body.len(), |at| column + at + 1);
        let head = &body[..colon];

        self.lines[index] = if written.is_empty() {
            format!("{head}{comment}")
        } else {
            format!("{head} {written}{comment}")
        };
    }

    /// Whether what is after a line's key stands on the line itself.
    fn has_inline(&self, index: usize) -> bool {
        let line = &self.lines[index];
        let column = self.column(index);
        let body = comment_start(line).map_or(line.as_str(), |at| line[..at].trim_end());

        colon_at(&body[column..]).is_some_and(|at| !body[column + at + 1..].trim().is_empty())
    }
}

/// Where the colon after a key is.
fn colon_at(body: &str) -> Option<usize> {
    let (key, _) = yaml::split_key(body)?;
    let start = if body.starts_with(['"', '\'']) {
        body[1..]
            .find(body.as_bytes()[0] as char)
            .map_or(0, |at| at + 2)
    } else {
        key.len()
    };

    body[start..].find(':').map(|at| start + at)
}

/// Whether a scalar of `candidate` reads back as `text`, in the place it would be written.
fn reads_back(candidate: &str, text: &str, flow: bool) -> bool {
    let wanted = Value::Str(text.to_string());
    let holds = |document: String, take: fn(&Node) -> Option<&Node>| {
        yaml::loads(&document)
            .ok()
            .flatten()
            .as_ref()
            .and_then(take)
            .is_some_and(|node| node.value == wanted)
    };

    if flow {
        return holds(format!("k: [{candidate}, x]\n"), |root| {
            root.as_map()?.get("k")?.as_list()?.first()
        });
    }

    holds(format!("k: {candidate}\n"), |root| root.as_map()?.get("k"))
        && holds(format!("- {candidate}\n"), |root| root.as_list()?.first())
}

/// Text as a value, in quotes only when it needs them, which the reader itself is asked.
fn text_scalar(text: &str, flow: bool) -> Result<String, String> {
    let single = format!("'{}'", text.replace('\'', "''"));
    let double = format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t")
    );

    [text.to_string(), single, double]
        .into_iter()
        .find(|candidate| reads_back(candidate, text, flow))
        .ok_or_else(|| format!("{} cannot be written in this file.", yaml::quoted(text)))
}

fn scalar(value: &Json, flow: bool) -> Result<String, String> {
    match value {
        Json::Null => Ok("null".to_string()),
        Json::Bool(value) => Ok(value.to_string()),
        Json::Number(value) => Ok(value.to_string()),
        Json::String(text) => text_scalar(text, flow),
        _ => Err("It is not a single value.".to_string()),
    }
}

fn is_scalar(value: &Json) -> bool {
    !matches!(value, Json::Array(_) | Json::Object(_))
}

/// A list of scalars on one line, when it fits.
fn flow_list(items: &[Json]) -> Result<String, String> {
    let written: Result<Vec<String>, String> =
        items.iter().map(|item| scalar(item, true)).collect();

    Ok(format!("[{}]", written?.join(", ")))
}

/// A value as the lines below its key, at a column.
fn block(value: &Json, column: usize, path: &str) -> Result<Vec<String>, String> {
    let pad = " ".repeat(column);
    let mut lines = Vec::new();

    match value {
        Json::Array(items) => {
            for item in items {
                match item {
                    Json::Object(_) => {
                        let inner = block(item, column + 2, &format!("{path}[]"))?;

                        for (index, line) in inner.into_iter().enumerate() {
                            if index == 0 {
                                lines.push(format!("{pad}- {}", &line[column + 2..]));
                            } else {
                                lines.push(line);
                            }
                        }
                    }
                    Json::Array(_) => {
                        return Err("A list in a list cannot be written here.".to_string());
                    }
                    _ => lines.push(format!("{pad}- {}", scalar(item, false)?)),
                }
            }
        }
        Json::Object(map) => {
            for (key, item) in map {
                let name = key_scalar(key)?;

                lines.extend(entry(&name, item, column, &schema::below(path, key))?);
            }
        }
        _ => lines.push(format!("{pad}{}", scalar(value, false)?)),
    }

    Ok(lines)
}

/// A key as it is written.
fn key_scalar(key: &str) -> Result<String, String> {
    let written = text_scalar(key, false)?;

    if yaml::split_key(&format!("{written}: x")).is_some_and(|(found, _)| found == key) {
        Ok(written)
    } else {
        Err(format!(
            "{} cannot be a key in this file.",
            yaml::quoted(key)
        ))
    }
}

/// A key and its value as lines, at a column.
fn entry(key: &str, value: &Json, column: usize, path: &str) -> Result<Vec<String>, String> {
    let pad = " ".repeat(column);

    match value {
        Json::Array(items) if items.is_empty() => Ok(vec![format!("{pad}{key}: []")]),
        Json::Object(map) if map.is_empty() => Ok(vec![format!("{pad}{key}: {{}}")]),
        Json::Array(items) if items.iter().all(is_scalar) && wants_flow(path) => {
            let written = flow_list(items)?;

            if column + key.len() + written.len() + 2 <= LONGEST_LINE {
                return Ok(vec![format!("{pad}{key}: {written}")]);
            }

            let mut lines = vec![format!("{pad}{key}:")];

            lines.extend(block(value, column + 2, path)?);

            Ok(lines)
        }
        Json::Array(_) | Json::Object(_) => {
            let mut lines = vec![format!("{pad}{key}:")];

            lines.extend(block(value, column + 2, path)?);

            Ok(lines)
        }
        _ => Ok(vec![format!("{pad}{key}: {}", scalar(value, false)?)]),
    }
}

fn wants_flow(path: &str) -> bool {
    schema::find(path).is_some_and(|key| key.flow || key.shape == Shape::Words)
}

/// What a value of the file is, as JSON, for a change made to a list as a whole.
pub fn to_json(node: &Node) -> Json {
    match &node.value {
        Value::Null => Json::Null,
        Value::Bool(value) => Json::Bool(*value),
        Value::Int(value) => Json::from(*value),
        Value::Float(value) => Json::from(*value),
        Value::Str(text) => Json::String(text.clone()),
        Value::List(items) => Json::Array(items.iter().map(to_json).collect()),
        Value::Map(map) => Json::Object(
            map.entries
                .iter()
                .map(|entry| (entry.key.clone(), to_json(&entry.value)))
                .collect(),
        ),
    }
}

/// Whether two values hold the same, wherever they stand, a mapping's order aside.
fn same(one: &Json, other: &Json) -> bool {
    match (one, other) {
        (Json::Object(one), Json::Object(other)) => {
            one.len() == other.len()
                && one
                    .iter()
                    .all(|(key, value)| other.get(key).is_some_and(|there| same(value, there)))
        }
        (Json::Array(one), Json::Array(other)) => {
            one.len() == other.len() && one.iter().zip(other).all(|(one, other)| same(one, other))
        }
        (Json::Number(one), Json::Number(other)) => one.as_f64() == other.as_f64(),
        _ => one == other,
    }
}

/// The whole of a file with one value changed, as data: what the text has to read as afterwards.
fn with_change(root: &Json, at: &[Step], value: Option<&Json>) -> Json {
    let mut root = root.clone();

    change_in(&mut root, at, value);

    root
}

fn change_in(here: &mut Json, at: &[Step], value: Option<&Json>) {
    let Some((step, rest)) = at.split_first() else {
        if let Some(value) = value {
            *here = value.clone();
        }

        return;
    };

    match step {
        Step::Key(key) => {
            if !here.is_object() {
                *here = Json::Object(serde_json::Map::new());
            }

            let map = here.as_object_mut().expect("just made one");

            if rest.is_empty() && value.is_none() {
                map.shift_remove(key);
                return;
            }

            change_in(map.entry(key.clone()).or_insert(Json::Null), rest, value);
        }
        Step::Item(item) => {
            if !here.is_array() {
                *here = Json::Array(Vec::new());
            }

            let list = here.as_array_mut().expect("just made one");

            if rest.is_empty() && value.is_none() {
                list.remove(*item);
                return;
            }

            if *item >= list.len() {
                list.push(Json::Null);
            }

            let last = list.len() - 1;

            change_in(&mut list[(*item).min(last)], rest, value);
        }
    }
}

/// Where a value stands, as far as the file has the way to it.
enum Found<'a> {
    /// The entry of a mapping, with the line the mapping's span ends at.
    Entry(&'a Mapping, usize, usize),
    /// The item of a list, with the line the list's span ends at.
    Item(&'a [Node], usize, usize),
    /// The mapping a key is not in yet.
    NewKey(&'a Mapping, usize),
    /// The list one more item goes on.
    NewItem(&'a [Node], usize),
}

/// The line after the last line of an entry of a mapping.
fn entry_end(map: &Mapping, position: usize, limit: usize) -> usize {
    map.entries
        .get(position + 1)
        .map_or(limit, |next| next.line - 1)
}

/// The line an item of a list ends before.
fn item_end(list: &[Node], position: usize, limit: usize) -> usize {
    list.get(position + 1).map_or(limit, |next| next.line - 1)
}

/// Follow the steps through what the file holds: how far they go, what stands there, and what is left of them.
fn locate<'a>(root: &'a Node, at: &[Step], total: usize) -> Result<(Found<'a>, usize), String> {
    let mut node = root;
    let mut limit = total;

    for (taken, step) in at.iter().enumerate() {
        let last = taken + 1 == at.len();

        match (step, &node.value) {
            (Step::Key(key), Value::Map(map)) => {
                let Some(position) = map.entries.iter().position(|entry| entry.key == *key) else {
                    return Ok((Found::NewKey(map, limit), taken));
                };

                let end = entry_end(map, position, limit);

                if last {
                    return Ok((Found::Entry(map, position, limit), taken));
                }

                let value = &map.entries[position].value;

                // Nothing below it yet, or nothing it can hold more in, so the entry is what changes.
                if !matches!(value.value, Value::Map(_) | Value::List(_))
                    || value.as_map().is_some_and(Mapping::is_empty)
                    || value.as_list().is_some_and(<[Node]>::is_empty)
                {
                    return Ok((Found::Entry(map, position, limit), taken));
                }

                limit = end;
                node = value;
            }
            (Step::Item(item), Value::List(list)) => {
                if *item == list.len() {
                    return Ok((Found::NewItem(list, limit), taken));
                }

                let Some(value) = list.get(*item) else {
                    return Err(format!("There is no item {item} here."));
                };

                let end = item_end(list, *item, limit);

                if last {
                    return Ok((Found::Item(list, *item, limit), taken));
                }

                limit = end;
                node = value;
            }
            _ => return Err("It does not hold what this change is for.".to_string()),
        }
    }

    Err("There is nothing to change.".to_string())
}

/// The value the rest of the steps make, around a value at the end of them.
fn nested(rest: &[Step], value: &Json) -> Json {
    let mut found = value.clone();

    for step in rest.iter().rev() {
        found = match step {
            Step::Key(key) => {
                let mut map = serde_json::Map::new();

                map.insert(key.clone(), found);

                Json::Object(map)
            }
            Step::Item(_) => Json::Array(vec![found]),
        };
    }

    found
}

/// Whether a list or a mapping stands on the line of its own key, in brackets.
fn in_brackets(doc: &Doc, line: usize) -> bool {
    doc.has_inline(line - 1)
}

/// Change one value in a file's text, and return the new text. `value` of None takes it out, along with anything it leaves with
/// nothing in it. The text returned has been read again and holds what was asked and everything it held before.
pub fn set(text: &str, at: &[Step], value: Option<&Json>) -> Result<String, String> {
    let root = match yaml::loads(text).map_err(|error| error.to_string())? {
        Some(root) => root,
        None => Node::new(Value::Map(Mapping::default()), 1),
    };

    if root.as_map().is_none() {
        return Err("The file should hold a mapping of settings.".to_string());
    }

    let old = to_json(&root);

    if value.is_none() && value_at(&old, at).is_none() {
        return Ok(text.to_string());
    }

    let at = match value {
        Some(_) => at.to_vec(),
        None => what_goes(&old, at),
    };
    let mut doc = Doc::new(text);

    // What is written on one line changes as a whole.
    if let Some(cut) = in_a_line(&root, &at, &doc) {
        let holder = &at[..cut];
        let mut whole = value_at(&old, holder).cloned().unwrap_or(Json::Null);

        change_in(&mut whole, &at[cut..], value);

        change(&mut doc, &root, holder, Some(&whole))?;
    } else {
        change(&mut doc, &root, &at, value)?;
    }

    let made = doc.text();
    let wanted = with_change(&old, &at, value);
    let read = match yaml::loads(&made).map_err(|error| error.to_string())? {
        Some(read) => to_json(&read),
        None => Json::Object(serde_json::Map::new()),
    };

    if same(&read, &wanted) {
        Ok(made)
    } else {
        Err(format!(
            "{} cannot be changed here without changing something else. Change it in the text.",
            shown(&at)
        ))
    }
}

/// Rename a key where it stands, leaving what it holds. A name given to an option, for one.
pub fn rename(text: &str, at: &[Step], name: &str) -> Result<String, String> {
    let root = yaml::loads(text)
        .map_err(|error| error.to_string())?
        .ok_or("The file holds nothing.")?;
    let old = to_json(&root);
    let (Some(Step::Key(key)), holder) = (at.last(), &at[..at.len().saturating_sub(1)]) else {
        return Err("Only a key has a name to change.".to_string());
    };

    let holding = value_at(&old, holder)
        .and_then(Json::as_object)
        .ok_or("It does not hold what this change is for.")?;

    if holding.contains_key(name) {
        return Err(format!("There is a {name} here already."));
    }

    let (found, _) = locate(&root, at, usize::MAX)?;
    let Found::Entry(map, position, _) = found else {
        return Err(format!("There is no {key} here."));
    };
    let mut doc = Doc::new(text);
    let index = map.entries[position].line - 1;
    let column = doc.column(index);
    let line = &doc.lines[index];
    let colon = column + colon_at(&line[column..]).ok_or("That line holds no key.")?;

    doc.lines[index] = format!("{}{}{}", &line[..column], key_scalar(name)?, &line[colon..]);

    let made = doc.text();
    let read = yaml::loads(&made)
        .map_err(|error| error.to_string())?
        .map(|read| to_json(&read))
        .unwrap_or(Json::Null);
    let mut wanted = old.clone();
    let mut renamed = serde_json::Map::new();

    for (one, value) in holding {
        renamed.insert(
            if one == key {
                name.to_string()
            } else {
                one.clone()
            },
            value.clone(),
        );
    }

    change_in(&mut wanted, holder, Some(&Json::Object(renamed)));

    if same(&read, &wanted) {
        Ok(made)
    } else {
        Err(format!(
            "{} cannot be renamed here. Change it in the text.",
            shown(at)
        ))
    }
}

fn value_at<'a>(root: &'a Json, at: &[Step]) -> Option<&'a Json> {
    let mut here = root;

    for step in at {
        here = match step {
            Step::Key(key) => here.get(key)?,
            Step::Item(item) => here.get(*item)?,
        };
    }

    Some(here)
}

/// What goes when a value is taken out: it, or the highest thing above it that would hold nothing without it.
fn what_goes(root: &Json, at: &[Step]) -> Vec<Step> {
    let mut at = at.to_vec();

    while at.len() > 1 {
        let holder = &at[..at.len() - 1];
        let alone = value_at(root, holder).is_some_and(|value| match value {
            Json::Object(map) => map.len() == 1,
            Json::Array(list) => list.len() == 1,
            _ => false,
        });

        if alone {
            at.pop();
        } else {
            break;
        }
    }

    at
}

/// How many steps lead to a value written on one line that the rest of the steps go inside.
fn in_a_line(root: &Node, at: &[Step], doc: &Doc) -> Option<usize> {
    let mut node = root;

    for (taken, step) in at.iter().enumerate() {
        let (next, line) = match (step, &node.value) {
            (Step::Key(key), Value::Map(map)) => {
                let entry = map.entries.iter().find(|entry| entry.key == *key)?;

                (&entry.value, entry.line)
            }
            (Step::Item(item), Value::List(list)) => {
                let item = list.get(*item)?;

                (item, item.line)
            }
            _ => return None,
        };

        let holds = next.as_map().is_some_and(|map| !map.is_empty())
            || next.as_list().is_some_and(|list| !list.is_empty());

        // A row of a list starts on its own dash, which is not a line of brackets.
        if holds && taken + 1 < at.len() && in_brackets_at(doc, step, line) {
            return Some(taken + 1);
        }

        // One more item for a list in brackets.
        if holds && taken + 1 == at.len() {
            return None;
        }

        node = next;
    }

    None
}

fn in_brackets_at(doc: &Doc, step: &Step, line: usize) -> bool {
    match step {
        Step::Key(_) => in_brackets(doc, line),
        Step::Item(_) => {
            let text = doc.lines[line - 1].trim_start();

            text.strip_prefix("- ")
                .is_some_and(|rest| rest.starts_with(['[', '{']))
        }
    }
}

fn change(doc: &mut Doc, root: &Node, at: &[Step], value: Option<&Json>) -> Result<(), String> {
    let (found, taken) = locate(root, at, doc.lines.len())?;
    let rest = &at[taken..];

    match found {
        Found::Entry(map, position, limit) => {
            let wanted = if rest.len() > 1 {
                value.map(|value| nested(&rest[1..], value))
            } else {
                value.cloned()
            };

            match wanted {
                Some(wanted) => replace_entry(doc, map, position, limit, &wanted, &at[..=taken]),
                None => {
                    remove_entry(doc, map, position, limit);
                    Ok(())
                }
            }
        }
        Found::Item(list, item, limit) => match value {
            Some(value) => replace_item(doc, list, item, limit, value, &at[..=taken]),
            None => {
                remove_item(doc, list, item, limit);
                Ok(())
            }
        },
        Found::NewKey(map, limit) => {
            let Some(value) = value else {
                return Ok(());
            };

            let Step::Key(key) = &rest[0] else {
                return Err("A mapping has keys, not items.".to_string());
            };

            insert_entry(
                doc,
                map,
                limit,
                key,
                &nested(&rest[1..], value),
                &at[..=taken],
            )
        }
        Found::NewItem(list, limit) => {
            let Some(value) = value else {
                return Ok(());
            };

            append_item(doc, list, limit, &nested(&rest[1..], value), &at[..=taken])
        }
    }
}

fn replace_entry(
    doc: &mut Doc,
    map: &Mapping,
    position: usize,
    limit: usize,
    value: &Json,
    at: &[Step],
) -> Result<(), String> {
    let found: &Entry = &map.entries[position];
    let path = table_path(at);
    let index = found.line - 1;
    let column = doc.column(index);
    let dash = doc.has_dash(index);
    let depth = if dash { doc.indent(index) } else { column };
    let (first, last) = doc.span(index, entry_end(map, position, limit), depth);
    let single = last == first + 1;

    // What stands on the key's own line stays there, when it can.
    if single {
        let written = match value {
            Json::Array(items) if items.is_empty() => Some("[]".to_string()),
            Json::Object(items) if items.is_empty() => Some("{}".to_string()),
            Json::Array(items)
                if items.iter().all(is_scalar) && (doc.has_inline(index) || wants_flow(&path)) =>
            {
                let written = flow_list(items)?;
                let line = &doc.lines[index];
                let used = comment_start(line).unwrap_or(line.len());

                // How far the value goes: a comment after it can run on as it likes.
                (column + found.key.len() + written.len() + 2 <= LONGEST_LINE.max(used))
                    .then_some(written)
            }
            Json::Array(_) | Json::Object(_) => None,
            _ => Some(scalar(value, false)?),
        };

        if let Some(written) = written {
            doc.set_inline(index, &written);
            return Ok(());
        }
    }

    // A list on its own lines keeps each of its lines that stays, and the comments with them.
    if let (Value::List(old), Json::Array(items)) = (&found.value.value, value)
        && !doc.has_inline(index)
        && !old.is_empty()
        && !items.is_empty()
        && items.iter().all(is_scalar)
    {
        merge_list(doc, old, last, items)?;
        return Ok(());
    }

    if !doc.has_inline(index) || !single {
        // The block below goes, what it is replaced by comes, and the key's line stays as it is.
        let below: Vec<String> = match value {
            Json::Array(_) | Json::Object(_) => block(value, column + 2, &path)?,
            _ => Vec::new(),
        };

        if matches!(value, Json::Array(_) | Json::Object(_)) {
            doc.set_inline(index, "");
        } else {
            doc.set_inline(index, &scalar(value, false)?);
        }

        doc.lines.splice(first + 1..last, below);

        return Ok(());
    }

    doc.set_inline(index, "");
    doc.lines
        .splice(first + 1..last, block(value, column + 2, &path)?);

    Ok(())
}

/// The lines of a list in the order asked, with each that stays as it was.
fn merge_list(doc: &mut Doc, old: &[Node], last: usize, items: &[Json]) -> Result<(), String> {
    let first_line = old[0].line - 1;
    let dash_column = doc.indent(first_line);
    let pad = " ".repeat(dash_column);
    let mut chunks: Vec<(Json, Vec<String>)> = Vec::new();
    let mut start = first_line;

    for (position, item) in old.iter().enumerate() {
        let end = if position + 1 < old.len() {
            let next = old[position + 1].line - 1;

            // Comments right above the next item are that item's.
            let mut cut = next;

            while cut > start + 1
                && doc.lines[cut - 1].trim_start().starts_with('#')
                && doc.indent(cut - 1) == dash_column
            {
                cut -= 1;
            }

            cut
        } else {
            last
        };

        chunks.push((to_json(item), doc.lines[start..end].to_vec()));
        start = end;
    }

    let mut written: Vec<String> = Vec::new();

    for item in items {
        match chunks.iter().position(|(value, _)| same(value, item)) {
            Some(position) => written.extend(chunks.remove(position).1),
            None => written.push(format!("{pad}- {}", scalar(item, false)?)),
        }
    }

    doc.lines.splice(first_line..last, written);

    Ok(())
}

fn replace_item(
    doc: &mut Doc,
    list: &[Node],
    item: usize,
    limit: usize,
    value: &Json,
    at: &[Step],
) -> Result<(), String> {
    let index = list[item].line - 1;
    let dash_column = doc.indent(index);
    let (first, last) = doc.span(index, item_end(list, item, limit), dash_column);
    let pad = " ".repeat(dash_column);

    if is_scalar(value) {
        let comment = doc.comment_of(index);

        doc.lines.splice(
            first..last,
            [format!("{pad}- {}{comment}", scalar(value, false)?)],
        );

        return Ok(());
    }

    let mut lines = block(
        &Json::Array(vec![value.clone()]),
        dash_column,
        &table_path(&at[..at.len() - 1]),
    )?;

    if lines.is_empty() {
        lines.push(format!("{pad}- {{}}"));
    }

    doc.lines.splice(first..last, lines);

    Ok(())
}

fn append_item(
    doc: &mut Doc,
    list: &[Node],
    limit: usize,
    value: &Json,
    at: &[Step],
) -> Result<(), String> {
    let Some(last_item) = list.last() else {
        return Err("The list is empty, so the key is what changes.".to_string());
    };
    let index = last_item.line - 1;
    let dash_column = doc.indent(index);
    let (_, end) = doc.span(index, limit.min(doc.lines.len()), dash_column);
    let lines = block(
        &Json::Array(vec![value.clone()]),
        dash_column,
        &table_path(&at[..at.len() - 1]),
    )?;

    doc.lines.splice(end..end, lines);

    Ok(())
}

/// Take lines out, along with an empty line that would then be one of two.
fn cut(doc: &mut Doc, first: usize, last: usize) {
    doc.lines.drain(first..last);

    let empty_above = first == 0 || doc.lines[first - 1].trim().is_empty();
    let empty_below = doc
        .lines
        .get(first)
        .is_none_or(|line| line.trim().is_empty());

    if empty_above && empty_below && first < doc.lines.len() {
        doc.lines.remove(first);
    } else if empty_above && first > 0 && first == doc.lines.len() {
        doc.lines.remove(first - 1);
    }
}

fn remove_entry(doc: &mut Doc, map: &Mapping, position: usize, limit: usize) {
    let index = map.entries[position].line - 1;
    let dash = doc.has_dash(index);
    let column = doc.column(index);
    let depth = if dash { doc.indent(index) } else { column };
    let (first, last) = doc.span(index, entry_end(map, position, limit), depth);

    // The first key of a row has the row's dash, which the next key takes over.
    if dash && let Some(next) = map.entries.get(position + 1) {
        let next_index = next.line - 1;
        let dash_column = doc.indent(index);
        let line = doc.lines[next_index].clone();

        doc.lines[next_index] = format!("{}- {}", " ".repeat(dash_column), line.trim_start());
        doc.lines.drain(first..next_index);

        return;
    }

    cut(doc, first, last);
}

fn remove_item(doc: &mut Doc, list: &[Node], item: usize, limit: usize) {
    let index = list[item].line - 1;
    let dash_column = doc.indent(index);
    let (first, last) = doc.span(index, item_end(list, item, limit), dash_column);

    cut(doc, first, last);
}

/// Where a new key goes among those of a mapping: after the last that comes before it in the table.
fn place_for(doc: &Doc, map: &Mapping, limit: usize, at: &[Step]) -> usize {
    let holder = &at[..at.len() - 1];
    let wanted = schema::order_of(&table_path(at));
    let mut after = None;

    for (position, found) in map.entries.iter().enumerate() {
        let mut path = holder.to_vec();

        path.push(Step::Key(found.key.clone()));

        // What the table has no place for, a name the author gives, goes after the others.
        if wanted == usize::MAX || schema::order_of(&table_path(&path)) < wanted {
            after = Some(position);
        }
    }

    match after {
        Some(position) => {
            let index = map.entries[position].line - 1;
            let dash = doc.has_dash(index);
            let depth = if dash {
                doc.indent(index)
            } else {
                doc.column(index)
            };

            doc.span(index, entry_end(map, position, limit), depth).1
        }
        None => map
            .entries
            .first()
            .map_or(doc.lines.len(), |first| first.line - 1),
    }
}

fn insert_entry(
    doc: &mut Doc,
    map: &Mapping,
    limit: usize,
    key: &str,
    value: &Json,
    at: &[Step],
) -> Result<(), String> {
    let column = map
        .entries
        .first()
        .map_or(0, |first| doc.column(first.line - 1));
    let place = place_for(doc, map, limit.min(doc.lines.len()), at);
    let mut lines = entry(&key_scalar(key)?, value, column, &table_path(at))?;

    // Before the first key of a row is under the row's dash, which stays first.
    if let Some(first) = map.entries.first()
        && place == first.line - 1
        && doc.has_dash(place)
    {
        let dash_column = doc.indent(place);
        let line = doc.lines[place].clone();

        doc.lines[place] = format!("{}{}", " ".repeat(column), line[column..].trim_start());
        lines[0] = format!("{}- {}", " ".repeat(dash_column), lines[0].trim_start());
    }

    // At the top of a file, a block has an empty line on each side, as the ones around it have.
    if column == 0 && !doc.lines.is_empty() {
        let is_block = lines.len() > 1;
        let above = place
            .checked_sub(1)
            .map(|index| doc.lines[index].trim().is_empty());
        let block_above = place > 0 && doc.indent(place - 1) > 0;

        if (is_block || block_above) && above == Some(false) {
            lines.insert(0, String::new());
        }

        if is_block
            && doc
                .lines
                .get(place)
                .is_some_and(|line| !line.trim().is_empty())
        {
            lines.push(String::new());
        }
    }

    doc.lines.splice(place..place, lines);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FILE: &str = "# The file's own comment\n\
                        \n\
                        name: demo          # its name\n\
                        platform: stm32\n\
                        kind: driver\n\
                        \n\
                        # The headers\n\
                        headers:\n\
                        \x20 - src/demo.h       # the main one\n\
                        \x20 # the settings\n\
                        \x20 - src/demo_config.h\n\
                        \n\
                        sources:\n\
                        \x20 - src/demo.c\n\
                        \n\
                        run:\n\
                        \x20 - label: one       # first\n\
                        \x20   command: [a, b]\n\
                        \x20 - label: two\n\
                        \x20   description: The second\n\
                        \x20   command: [c]\n\
                        \n\
                        stm32:\n\
                        \x20 requires:\n\
                        \x20   hal: true\n\
                        \x20   hal_modules: [spi, i2c]   # both\n\
                        # the end\n";

    fn steps(path: &[&str]) -> Vec<Step> {
        path.iter()
            .map(|step| match step.parse::<usize>() {
                Ok(item) => Step::Item(item),
                Err(_) => Step::Key((*step).to_string()),
            })
            .collect()
    }

    fn change(text: &str, path: &[&str], value: Json) -> String {
        set(text, &steps(path), Some(&value)).unwrap()
    }

    fn take_out(text: &str, path: &[&str]) -> String {
        set(text, &steps(path), None).unwrap()
    }

    #[test]
    fn a_value_changes_on_its_line_and_its_comment_stays() {
        let made = change(FILE, &["name"], json!("other"));

        assert!(
            made.contains("name: other          # its name\n")
                || made.contains("name: other # its name\n"),
            "{made}"
        );
        assert!(made.contains("# The file's own comment"), "{made}");
        assert_eq!(made.lines().count(), FILE.lines().count());
    }

    #[test]
    fn a_value_is_put_in_quotes_only_when_it_needs_them() {
        for (given, written) in [
            ("plain words", "plain words"),
            ("a: b", "'a: b'"),
            ("it's", "it's"),
            ("true", "'true'"),
            ("1.5", "'1.5'"),
            ("1.2.3", "1.2.3"),
            ("*.c", "'*.c'"),
            ("{library}/tools", "'{library}/tools'"),
            ("a # b", "'a # b'"),
            ("line\nbreak", "\"line\\nbreak\""),
        ] {
            let made = change("description: x\n", &["description"], json!(given));

            assert_eq!(made, format!("description: {written}\n"), "{given}");
        }
    }

    #[test]
    fn a_new_key_goes_where_the_table_has_it() {
        let made = change(FILE, &["description"], json!("A demo"));
        let lines: Vec<&str> = made.lines().collect();

        assert_eq!(lines[2], "name: demo          # its name");
        assert_eq!(lines[3], "description: A demo");
        assert_eq!(lines[4], "platform: stm32");

        let made = change(FILE, &["category"], json!("sensor"));
        let lines: Vec<&str> = made.lines().collect();

        assert_eq!(lines[4], "kind: driver");
        assert_eq!(lines[5], "category: sensor");
    }

    #[test]
    fn a_new_block_stands_apart_at_the_top_of_a_file() {
        let made = change(FILE, &["defines"], json!(["FAST"]));

        assert!(
            made.contains("  - src/demo.c\n\ndefines:\n  - FAST\n\nrun:\n"),
            "{made}"
        );
    }

    #[test]
    fn a_new_key_goes_inside_what_holds_it() {
        let made = change(FILE, &["stm32", "requires", "rtos"], json!("optional"));

        assert!(
            made.contains(
                "    hal: true\n    rtos: optional\n    hal_modules: [spi, i2c]   # both\n"
            ),
            "{made}"
        );
    }

    #[test]
    fn what_holds_a_new_key_is_made_when_it_is_not_there() {
        let made = change("name: x\n", &["requires", "libnima"], json!("1.0.0"));

        assert_eq!(made, "name: x\n\nrequires:\n  libnima: 1.0.0\n");

        let made = change(
            "name: x\nstm32: {}\n",
            &["stm32", "requires", "hal"],
            json!(true),
        );

        assert_eq!(made, "name: x\nstm32:\n  requires:\n    hal: true\n");

        let made = change(
            "name: x\nstm32:\n",
            &["stm32", "requires", "hal"],
            json!(false),
        );

        assert_eq!(made, "name: x\nstm32:\n  requires:\n    hal: false\n");
    }

    #[test]
    fn a_list_on_its_own_lines_keeps_each_line_that_stays() {
        let made = change(
            FILE,
            &["headers"],
            json!(["src/demo.h", "src/demo_config.h", "src/more.h"]),
        );

        assert!(
            made.contains("  - src/demo.h       # the main one\n"),
            "{made}"
        );
        assert!(
            made.contains("  # the settings\n  - src/demo_config.h\n  - src/more.h\n"),
            "{made}"
        );

        let made = change(
            FILE,
            &["headers"],
            json!(["src/demo_config.h", "src/demo.h"]),
        );

        assert!(
            made.contains("headers:\n  # the settings\n  - src/demo_config.h\n  - src/demo.h       # the main one\n"),
            "{made}"
        );

        let made = change(FILE, &["headers"], json!(["src/demo.h"]));

        assert!(
            made.contains("headers:\n  - src/demo.h       # the main one\n\nsources:"),
            "{made}"
        );
    }

    #[test]
    fn a_list_on_one_line_stays_on_one_line_with_its_comment() {
        let made = change(FILE, &["stm32", "requires", "hal_modules"], json!(["spi"]));

        assert!(made.contains("    hal_modules: [spi]   # both\n"), "{made}");

        let made = change(
            FILE,
            &["stm32", "requires", "hal_modules", "1"],
            json!("uart"),
        );

        assert!(
            made.contains("    hal_modules: [spi, uart]   # both\n"),
            "{made}"
        );

        let made = change(
            FILE,
            &["stm32", "requires", "hal_modules", "2"],
            json!("uart"),
        );

        assert!(
            made.contains("    hal_modules: [spi, i2c, uart]   # both\n"),
            "{made}"
        );

        let made = take_out(FILE, &["stm32", "requires", "hal_modules", "0"]);

        assert!(made.contains("    hal_modules: [i2c]   # both\n"), "{made}");
    }

    #[test]
    fn one_platform_is_a_word_and_several_a_list() {
        let made = change(
            "name: x\nplatform: stm32  # for now\n",
            &["platform"],
            json!(["stm32", "zephyr"]),
        );

        assert_eq!(made, "name: x\nplatform: [stm32, zephyr]  # for now\n");

        let made = change(&made, &["platform"], json!("zephyr"));

        assert_eq!(made, "name: x\nplatform: zephyr  # for now\n");
    }

    #[test]
    fn a_key_of_a_row_changes_where_it_stands() {
        let made = change(FILE, &["run", "0", "label"], json!("first"));

        assert!(
            made.contains("  - label: first       # first\n    command: [a, b]\n"),
            "{made}"
        );

        let made = change(FILE, &["run", "1", "unless"], json!("made.txt"));

        assert!(
            made.contains("    command: [c]\n    unless: made.txt\n"),
            "{made}"
        );

        let made = change(FILE, &["run", "0", "description"], json!("The first"));

        assert!(
            made.contains(
                "  - label: one       # first\n    description: The first\n    command: [a, b]\n"
            ),
            "{made}"
        );
    }

    #[test]
    fn a_new_row_goes_at_the_end_of_its_list() {
        let made = change(
            FILE,
            &["run", "2"],
            json!({"label": "three", "command": ["d", "e"]}),
        );

        assert!(
            made.contains("    command: [c]\n  - label: three\n    command: [d, e]\n\nstm32:"),
            "{made}"
        );
    }

    #[test]
    fn a_new_item_goes_on_a_list_on_its_own_lines() {
        let made = change(FILE, &["sources", "1"], json!("src/more.c"));

        assert!(
            made.contains("sources:\n  - src/demo.c\n  - src/more.c\n\nrun:"),
            "{made}"
        );
    }

    #[test]
    fn a_key_goes_with_its_block_and_the_comments_in_it() {
        let made = take_out(FILE, &["headers"]);

        assert!(!made.contains("demo.h"), "{made}");
        assert!(!made.contains("# the settings"), "{made}");
        assert!(
            made.contains("# The headers\n"),
            "a comment above it may be about anything: {made}"
        );
        assert!(made.contains("sources:\n"), "{made}");
    }

    #[test]
    fn a_row_goes_and_the_others_stay() {
        let made = take_out(FILE, &["run", "0"]);

        assert!(made.contains("run:\n  - label: two\n"), "{made}");
        assert!(!made.contains("label: one"), "{made}");

        let made = take_out(FILE, &["run", "1"]);

        assert!(made.contains("    command: [a, b]\n\nstm32:"), "{made}");
    }

    #[test]
    fn what_is_left_holding_nothing_goes_too() {
        let made = take_out(FILE, &["sources", "0"]);

        assert!(!made.contains("sources"), "{made}");

        let text = "name: x\nstm32:\n  requires:\n    hal: true\n";

        assert_eq!(take_out(text, &["stm32", "requires", "hal"]), "name: x\n");
    }

    #[test]
    fn the_first_key_of_a_row_gives_its_dash_to_the_next() {
        let made = take_out(FILE, &["run", "1", "label"]);

        assert!(
            made.contains("  - description: The second\n    command: [c]\n"),
            "{made}"
        );
    }

    #[test]
    fn a_comment_at_the_end_of_the_file_stays() {
        let made = take_out(FILE, &["stm32"]);

        assert!(made.ends_with("# the end\n"), "{made}");
    }

    #[test]
    fn taking_out_what_is_not_there_changes_nothing() {
        assert_eq!(take_out(FILE, &["once"]), FILE);
        assert_eq!(take_out(FILE, &["zephyr", "project", "conf"]), FILE);
        assert_eq!(take_out(FILE, &["run", "7"]), FILE);
    }

    #[test]
    fn line_endings_stay_as_the_file_has_them() {
        let text = "name: x\r\nplatform: stm32\r\n";
        let made = change(text, &["description"], json!("y"));

        assert_eq!(made, "name: x\r\ndescription: y\r\nplatform: stm32\r\n");
    }

    #[test]
    fn an_empty_file_gets_its_first_key() {
        assert_eq!(change("", &["name"], json!("x")), "name: x\n");
        assert_eq!(
            change("# only a comment\n", &["name"], json!("x")),
            "# only a comment\nname: x\n"
        );
    }

    #[test]
    fn a_file_that_does_not_read_is_left_as_it_is() {
        assert!(set("a: [\n", &steps(&["name"]), Some(&json!("x"))).is_err());
    }

    #[test]
    fn a_mapping_in_brackets_changes_as_a_whole() {
        let made = change(
            "name: x\nstm32: {requires: {hal: true}}\n",
            &["stm32", "requires", "cmsis"],
            json!(true),
        );
        let read = yaml::loads(&made).unwrap().unwrap();

        assert_eq!(
            to_json(&read),
            json!({"name": "x", "stm32": {"requires": {"hal": true, "cmsis": true}}}),
            "{made}"
        );
    }

    #[test]
    fn whatever_is_changed_the_rest_of_the_file_reads_the_same() {
        let before = to_json(&yaml::loads(FILE).unwrap().unwrap());

        for (path, value) in [
            (vec!["name"], json!("a")),
            (vec!["version"], json!("1.0.0")),
            (vec!["headers"], json!(["x.h"])),
            (vec!["run", "1", "command"], json!(["p", "q"])),
            (vec!["once"], json!(["VERSION"])),
            (
                vec!["options", "lfs"],
                json!({"description": "d", "headers": ["l.h"]}),
            ),
            (
                vec!["requires", "peripherals"],
                json!([{"type": "spi", "count": 2}]),
            ),
            (
                vec!["zephyr", "project", "cmake", "after-zephyr"],
                json!(["line(1)"]),
            ),
        ] {
            let made = change(FILE, &path, value.clone());
            let read = to_json(&yaml::loads(&made).unwrap().unwrap());

            assert_eq!(
                read,
                with_change(&before, &steps(&path), Some(&value)),
                "{path:?}\n{made}"
            );
            assert!(made.contains("# the end\n"), "{made}");
            assert!(made.contains("# The file's own comment\n"), "{made}");
        }
    }

    #[test]
    fn a_comment_after_a_value_does_not_count_for_how_long_its_line_is() {
        let comment = "# ".to_string() + &"a long comment ".repeat(10);
        let text = format!("platform: stm32   {comment}\n");
        let made = change(&text, &["platform"], json!(["stm32", "zephyr"]));

        assert_eq!(made, format!("platform: [stm32, zephyr]   {comment}\n"));
    }

    #[test]
    fn a_new_name_goes_after_the_others() {
        let text = "options:\n  one:\n    description: a\n";
        let made = change(text, &["options", "two"], json!({"description": "b"}));

        assert_eq!(
            made,
            "options:\n  one:\n    description: a\n  two:\n    description: b\n"
        );
    }

    #[test]
    fn a_name_changes_where_it_stands() {
        let text = "options:\n  one:          # the first\n    description: a\n  two:\n    description: b\n";
        let made = rename(text, &steps(&["options", "one"]), "uno").unwrap();

        assert_eq!(
            made,
            "options:\n  uno:          # the first\n    description: a\n  two:\n    description: b\n"
        );
        assert!(
            rename(text, &steps(&["options", "one"]), "two").is_err(),
            "taken"
        );
    }

    #[test]
    fn a_step_is_a_key_or_a_number_as_the_window_sends_it() {
        let found: Vec<Step> = serde_json::from_value(json!(["run", 0, "label"])).unwrap();

        assert_eq!(
            found,
            vec![Step::from("run"), Step::from(0), Step::from("label")]
        );
    }
}
