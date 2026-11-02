// What the editor tab holds: a library's file, which it reads and saves through the Rust side, what is wrong with it and what it
// comes to. The form and the text show the same file: the form's changes are made to the text, by the engine, which keeps the rest
// of it as it is.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { confirmFirst } from "./confirm.svelte";
import { addLibrary, view, type Looked } from "./project.svelte";

export type Shape = "text" | "value" | "flag" | "choice" | "choices" | "words" | "lines" | "files" | "rows" | "section" | "named";

/** A key of the file, from the engine's table. */
export type Key = {
  path: string;
  shape: Shape;
  label: string;
  help: string;
  choices: string[];
  open: boolean;
  example: string;
  fresh: string;
  flow: boolean;
  on: boolean;
};

export type Group = { name: string; keys: string[]; platform: string | null };

/** A step on the way to a value: a key of a mapping, or an item of a list. */
export type Step = string | number;

export type Fix = { label: string; text: string };
export type Advice = { line: number | null; message: string; warning: boolean; fixes: Fix[] };

export type Listed = { given: string; line: number; files: string[]; missing: boolean };
export type FileList = { key: string; label: string; entries: Listed[] };
export type PlatformState = { name: string; label: string; problem: string | null; line: number | null };
export type Unlisted = { path: string; belongs: "headers" | "sources" };

/** What a file comes to. */
export type Outline = {
  data: Record<string, unknown> | null;
  error: { line: number | null; message: string } | null;
  lines: Record<string, number>;
  files: string[];
  name: string;
  version: string;
  version_from: string;
  include: string;
  platforms: PlatformState[];
  lists: FileList[];
  unlisted: Unlisted[];
  options: string[];
  steps: string[];
  needs: string[];
  peripherals: string[];
};

type LibraryFile = { path: string; text: string; converted: boolean };

export const editor = $state({
  /** Where saving writes. Empty for a file that came from a repository, which has nowhere to be saved to. */
  path: "",
  /** What the top of the tab says it is. */
  title: "",
  /** The text as it was opened or last saved, which the editor starts from. */
  text: "",
  /** Bumped when a new file is opened, for the editor to take its text. */
  opened: 0,
  /** It is a repository's file, for looking at only. */
  readOnly: false,
  /** It came from an installer.yml, and is not saved as libnima.yml yet. */
  converted: false,
  /** It was made from what is in the folder, and is not saved yet. */
  fresh: false,
  /** It differs from what is on disk. */
  dirty: false,
  /** How it is shown. */
  mode: "form" as "form" | "text",
  /** The part of the form in front. */
  group: "About",
  /** The panel of what the file comes to is showing. */
  panel: true,
  problems: [] as Advice[],
  outline: null as Outline | null,
  keys: [] as Key[],
  groups: [] as Group[],
  /** Why the last thing asked could not be done. */
  problem: "",
  /** Where the editor should go, bumped with each ask. */
  goTo: { line: 0, ask: 0 },
  /** The text in the editor now. */
  current: (() => "") as () => string,
  /** Puts a new text in the editor, as a change that can be undone. */
  replace: (() => {}) as (text: string) => void,
});

/** The engine's table of keys, once. */
export async function loadTable(): Promise<void> {
  if (editor.keys.length === 0) {
    const table = await invoke<{ keys: Key[]; groups: Group[] }>("library_file_table");
    editor.keys = table.keys;
    editor.groups = table.groups;
  }
}

/** The key at a path of the table. */
export function keyAt(path: string): Key | undefined {
  return editor.keys.find((key) => key.path === path);
}

/** The keys right below a path of the table. */
export function keysBelow(path: string): Key[] {
  return editor.keys.filter((key) => key.path.slice(0, Math.max(key.path.lastIndexOf("."), 0)) === path);
}

/** The last word of a key's path. */
export function nameOf(key: Key): string {
  return key.path.slice(key.path.lastIndexOf(".") + 1);
}

/** Reads again what the text comes to. */
export async function refreshOutline(): Promise<void> {
  if (!editor.path || editor.readOnly) {
    editor.outline = null;
    return;
  }
  const opened = editor.opened;
  const outline = await invoke<Outline>("outline_library_file", { path: editor.path, text: editor.current() }).catch(() => null);
  if (opened === editor.opened) {
    editor.outline = outline;
  }
}

/** Puts a text in the editor. */
function show(path: string, title: string, text: string, readOnly: boolean, converted: boolean): void {
  editor.path = path;
  editor.title = title;
  editor.text = text;
  editor.readOnly = readOnly;
  editor.converted = converted;
  editor.fresh = false;
  editor.dirty = converted;
  editor.problems = [];
  editor.problem = "";
  editor.outline = null;
  editor.group = "About";
  editor.opened++;
  if (readOnly) {
    editor.mode = "text";
  }
}

/** Goes ahead, once the changes not saved are agreed to be lost. */
function unlessChanged(then: () => void): void {
  if (editor.dirty && !editor.readOnly) {
    confirmFirst("Discard your changes?", `${editor.title} has changes that are not saved.`, "Discard them", then);
  } else {
    then();
  }
}

/** The folder a file's path is in. */
export function folderOf(path: string): string {
  return path.replace(/[\\/][^\\/]*$/, "");
}

/** Opens the library's file in a folder. */
async function openLibrary(folder: string): Promise<void> {
  try {
    const file = await invoke<LibraryFile>("read_library_file", { folder });
    show(file.path, file.path, file.text, false, file.converted);
    editor.mode = "form";
  } catch (error) {
    editor.problem = String(error);
  }
}

/** Asks for a folder, and opens the library's file in it. */
export async function pickLibrary(): Promise<void> {
  const folder = await open({ title: "A library's folder", directory: true, multiple: false });
  if (typeof folder === "string") {
    unlessChanged(() => void openLibrary(folder));
  }
}

/** Starts a new file in a folder, from what is in it. One already there is opened instead. */
export async function newLibrary(): Promise<void> {
  const folder = await open({ title: "The library's folder", directory: true, multiple: false });
  if (typeof folder !== "string") {
    return;
  }
  const opened = await invoke<LibraryFile>("read_library_file", { folder }).catch(() => null);
  if (opened) {
    unlessChanged(() => {
      show(opened.path, opened.path, opened.text, false, opened.converted);
      editor.mode = "form";
    });
    return;
  }
  const platform = view.choices.platform ?? view.project?.platform ?? null;
  const fresh = await invoke<LibraryFile>("new_library_file", { folder, platform });
  unlessChanged(() => {
    show(fresh.path, fresh.path, fresh.text, false, false);
    editor.mode = "form";
    editor.fresh = true;
    editor.dirty = true;
  });
}

/** Shows a library's file from the list: to edit, for a folder on this computer, and only to read, from anywhere else. */
export async function showFileOf(looked: Looked): Promise<void> {
  const file = await invoke<LibraryFile>("read_library_file", { folder: looked.given }).catch(() => null);
  unlessChanged(() => {
    if (file) {
      show(file.path, file.path, file.text, false, file.converted);
      editor.mode = "form";
    } else if (looked.manifest) {
      show("", `${looked.manifest.name} of ${looked.given}`, looked.manifest.text, true, false);
    }
  });
}

/** The edits under way, one after another, so each starts from what the one before made. */
let edits: Promise<void> = Promise.resolve();

/** Makes an edit of the text once those before it are done. */
function edit(ask: () => Promise<string>): Promise<void> {
  edits = edits.then(async () => {
    try {
      editor.replace(await ask());
      editor.problem = "";
      await refreshOutline();
    } catch (error) {
      editor.problem = String(error);
    }
  });
  return edits;
}

/** Changes one value in the file, or takes it out with null, as the form asks. */
export function setValue(at: Step[], value: unknown): Promise<void> {
  return edit(() => invoke<string>("edit_library_file", { text: editor.current(), at, value: value ?? null }));
}

/** Gives a key another name where it stands, such as an option's. */
export function renameKey(at: Step[], name: string): Promise<void> {
  return edit(() => invoke<string>("rename_in_library_file", { text: editor.current(), at, name }));
}

/** Puts a problem right the way a fix says. */
export function useFix(fix: Fix): void {
  editor.replace(fix.text);
  void refreshOutline();
}

/** Shows a line in the text. */
export function showLine(line: number | null): void {
  if (line) {
    editor.mode = "text";
    editor.goTo = { line, ask: editor.goTo.ask + 1 };
  }
}

/** The part of the form a line of the file is in. */
export function groupOfLine(line: number): string | null {
  const lines = editor.outline?.lines ?? {};
  let nearest = "";
  let at = 0;

  for (const [path, where] of Object.entries(lines)) {
    if (!path.includes(".") && where <= line && where >= at) {
      nearest = path;
      at = where;
    }
  }

  return editor.groups.find((group) => group.keys.includes(nearest))?.name ?? null;
}

/** Shows where a line is, the way the file is being shown. */
export function showPlace(line: number | null): void {
  if (!line) {
    return;
  }
  const group = editor.mode === "form" ? groupOfLine(line) : null;
  if (group) {
    editor.group = group;
  } else {
    showLine(line);
  }
}

/** Saves the editor's text to its file. */
export async function save(): Promise<boolean> {
  if (!editor.path || editor.readOnly) {
    return false;
  }
  // What is being typed in the form counts: a field gives up its change when it loses the keyboard.
  if (document.activeElement instanceof HTMLInputElement) {
    document.activeElement.blur();
  }
  await edits;
  try {
    await invoke("save_library_file", { path: editor.path, text: editor.current() });
    editor.dirty = false;
    editor.converted = false;
    editor.fresh = false;
    editor.problem = "";
    return true;
  } catch (error) {
    editor.problem = String(error);
    return false;
  }
}

/** Saves the file, and checks the library against the project in the other tab. */
export async function tryOnProject(): Promise<boolean> {
  if (!editor.path || !view.project || !(await save())) {
    return false;
  }
  const folder = folderOf(editor.path);
  // Checked anew, since it was changed.
  view.items = view.items.filter((item) => item.given !== folder);
  addLibrary(folder);
  return true;
}
