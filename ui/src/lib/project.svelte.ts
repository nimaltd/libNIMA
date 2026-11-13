// The project in the window, the libraries to install into it, and the run that does. Everything here is asked of the engine
// in src-tauri, which reads and writes the same as the libnima command.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { forgetProject, rememberProject, settings } from "./settings.svelte";

export type Platform = "stm32" | "zephyr" | "esp32" | "arduino" | "pio";

export const PLATFORMS: { name: Platform; label: string }[] = [
  { name: "stm32", label: "STM32" },
  { name: "zephyr", label: "Zephyr" },
  { name: "esp32", label: "ESP-IDF" },
  { name: "arduino", label: "Arduino" },
  { name: "pio", label: "PlatformIO" },
];

export type Style = "plain" | "bold" | "title" | "dim" | "good" | "warn" | "bad" | "note" | "accent";
export type Span = { text: string; style: Style };
/** One line of what a run says. */
export type Line = Span[];

export type QuestionKind =
  | { type: "text" }
  | { type: "yesno"; default: boolean }
  /** One of a list, or several when `many`: answered with the names picked, joined by commas. */
  | { type: "choose"; options: string[]; many: boolean };
export type Question = { prompt: Line; kind: QuestionKind };

export type Candidate = { platform: Platform; evidence: string };

export type InstalledLibrary = {
  name: string;
  version: string;
  folder: string;
  /** What an update fetches it from. Nothing for what came from a zip, which only a new zip can update. */
  source: string | null;
  origin: string;
  options: string[];
  needs: string[];
};

export type Project = {
  folder: string;
  platform: Platform | null;
  recorded: boolean;
  candidates: Candidate[];
  ides: string[];
  conf_files: string[];
  overlay_files: string[];
  sketch: boolean;
  installed: InstalledLibrary[];
};

export type Mark = "ok" | "check" | "no";
export type Level = "ok" | "warn" | "info";
export type Found = { level: Level; message: string; hint: string };
export type OptionOffered = { name: string; description: string; on: boolean; needs: string[] };
export type StepOffered = { label: string; description: string; command: string };
export type Need = { name: string; wanted: string; state: "here" | "update" | "get"; have: string };

export type Looked = {
  given: string;
  mark: Mark;
  why: string;
  name: string;
  version: string;
  description: string;
  repository: string;
  license: string;
  platform: Platform | null;
  findings: Found[];
  options: OptionOffered[];
  steps: StepOffered[];
  needs: Need[];
  folder: string;
  in_place: boolean;
  installed: string | null;
  manifest: { name: string; text: string } | null;
  needs_token: boolean;
};

export type Change = {
  path: string;
  kind: "added" | "changed" | "removed";
  before: string | null;
  after: string | null;
};
export type Preview = { code: number; changes: Change[]; said: Line[]; errors: string[] };

/** A library in the list to install. */
export type Item = {
  id: number;
  /** As it was given: owner/name, a GitHub address, a zip or a folder. */
  given: string;
  /** What the check found, once it has. */
  looked: Looked | null;
  /** The folder it goes into. */
  folder: string;
  /** The options to take, by name. */
  options: Record<string, boolean>;
  /** The steps to run, by label. None is ticked until the person has read what it runs. */
  steps: Record<string, boolean>;
};

/** What the project's Change dialog can say, in place of what the engine would find. */
export type Choices = {
  platform: Platform | null;
  ide: string | null;
  conf: string | null;
  overlay: string | null;
  into: string | null;
  sketchbook: string | null;
};

/** How many lines of the log are kept. */
const LOG_KEPT = 5000;

/** How many lines of what led up to a question are shown with it. */
const QUESTION_LINES = 12;

export const view = $state({
  project: null as Project | null,
  /** Reading a folder. */
  opening: false,
  /** Why the folder could not be read. */
  problem: "",
  choices: { platform: null, ide: null, conf: null, overlay: null, into: null, sketchbook: null } as Choices,
  items: [] as Item[],
  /** The libraries GitHub has newer versions of, by name. */
  newer: {} as Record<string, string>,
  /** What runs said, in parts: each library is one, and so is what a run says before it gets to them. */
  log: [] as Line[][],
  /** What a run is doing, while one is under way. */
  running: "",
  /** The question a run is waiting on. */
  question: null as Question | null,
  /** A token for this window, for private repositories. It is never saved. */
  key: null as string | null,
  /** A preview being made. */
  previewing: false,
});

let nextId = 1;

/** The options of the engine that every request carries. */
function asked(): Record<string, unknown> {
  const { platform, ide, conf, overlay, into, sketchbook } = view.choices;
  return { project: view.project?.folder ?? "", platform, ide, conf, overlay, into, sketchbook, key: view.key };
}

function isBlank(line: Line): boolean {
  return line.every((span) => span.text.trim() === "");
}

/** How many lines the log holds. */
let logged = 0;

/** Adds a line to the part of the log under way. */
function addLine(line: Line): void {
  if (view.log.length === 0) {
    view.log.push([]);
  }
  const part = view.log[view.log.length - 1];
  // An empty line at the top of a part only kept it from what came before, which the part does by itself, and one is enough.
  const previous = part.at(-1);
  if (isBlank(line) && (previous === undefined || isBlank(previous))) {
    return;
  }
  part.push(line);
  logged++;
  if (logged > LOG_KEPT) {
    view.log[0].shift();
    logged--;
    if (view.log[0].length === 0) {
      view.log.shift();
    }
  }
}

/** Says a line in the log. */
export function say(text: string, style: Style = "note"): void {
  addLine([{ text, style }]);
}

export function clearLog(): void {
  view.log = [];
  logged = 0;
}

/** Opens a folder as the project. */
export async function openProject(folder: string): Promise<void> {
  view.opening = true;
  view.problem = "";
  try {
    const project = await invoke<Project>("open_project", { folder, sketchbook: null });
    if (project.folder !== view.project?.folder) {
      clearLog();
    }
    view.project = project;
    view.choices = { platform: null, ide: null, conf: null, overlay: null, into: null, sketchbook: null };
    view.items = [];
    view.newer = {};
    rememberProject(project.folder);
    void lookForNewer();
  } catch (error) {
    view.problem = String(error);
    say(view.problem, "bad");
    forgetProject(folder);
  } finally {
    view.opening = false;
  }
}

/** Reads the project again, as after a run. */
export async function reopen(): Promise<void> {
  if (view.project) {
    view.project = await invoke<Project>("open_project", {
      folder: view.project.folder,
      sketchbook: view.choices.sketchbook,
    });
  }
}

/** Asks GitHub whether the project's libraries have newer versions. Saying nothing when it cannot be reached. */
export async function lookForNewer(asked_for = false): Promise<void> {
  const project = view.project;
  if (!project || project.installed.length === 0 || (!settings.newer_libraries && !asked_for)) {
    return;
  }
  const found = await invoke<[string, string][]>("newer_libraries", {
    folder: project.folder,
    sketchbook: view.choices.sketchbook,
    key: view.key,
  }).catch(() => []);
  if (view.project?.folder === project.folder) {
    view.newer = Object.fromEntries(found);
  }
}

/** Puts a library on the list, and checks it against the project. */
export function addLibrary(given: string): void {
  const text = given.trim();
  if (!text || !view.project) {
    return;
  }
  if (view.items.some((item) => item.given === text)) {
    return;
  }
  const item: Item = { id: nextId++, given: text, looked: null, folder: "", options: {}, steps: {} };
  view.items.push(item);
  void check(item.id);
}

/** Checks a library on the list against the project, and writes nothing. */
export async function check(id: number): Promise<void> {
  const item = view.items.find((one) => one.id === id);
  if (!item) {
    return;
  }
  item.looked = null;
  const looked = await invoke<Looked>("look_at_library", { asked: asked(), given: item.given }).catch(
    (error) =>
      ({
        given: item.given,
        mark: "no",
        why: String(error),
        name: "",
        version: "",
        description: "",
        repository: "",
        license: "",
        platform: null,
        findings: [],
        options: [],
        steps: [],
        needs: [],
        folder: "",
        in_place: false,
        installed: null,
        manifest: null,
        needs_token: false,
      }) as Looked,
  );
  // It may have left the list while GitHub was asked.
  const now = view.items.find((one) => one.id === id);
  if (!now) {
    return;
  }
  now.looked = looked;
  now.folder = looked.folder;
  now.options = Object.fromEntries(looked.options.map((option) => [option.name, option.on]));
  now.steps = Object.fromEntries(looked.steps.map((step) => [step.label, false]));
}

/** Checks the whole list again, as when the platform was changed. */
export function checkAll(): void {
  for (const item of view.items) {
    void check(item.id);
  }
}

export function removeItem(id: number): void {
  view.items = view.items.filter((item) => item.id !== id);
}

/** What can go in: what the check has finished with, and did not refuse. */
export function installable(): Item[] {
  return view.items.filter((item) => item.looked !== null && item.looked.mark !== "no");
}

/** Whether every library on the list has been checked. */
export function allChecked(): boolean {
  return view.items.every((item) => item.looked !== null);
}

/** The request for the libraries that can go in, with what was chosen for each. */
export function installRequest(): Record<string, unknown> {
  const items = installable();
  const named = (choose: (item: Item) => Record<string, boolean>, wanted: boolean) => [
    ...new Set(
      items.flatMap((item) =>
        Object.entries(choose(item))
          .filter(([, on]) => on === wanted)
          .map(([name]) => name),
      ),
    ),
  ];
  const withOptions = named((item) => item.options, true);
  const runOn = named((item) => item.steps, true);

  return {
    ...asked(),
    libraries: items.map((item) => item.given),
    folders: items
      .filter((item) => !item.looked?.in_place && item.folder.trim() !== "")
      .map((item) => [item.looked?.name, item.folder.trim()]),
    // Two libraries can have an option of one name: what is ticked in either wins.
    with_options: withOptions,
    without_options: named((item) => item.options, false).filter((name) => !withOptions.includes(name)),
    run_on: runOn,
    run_off: named((item) => item.steps, false).filter((name) => !runOn.includes(name)),
  };
}

/** Tries an install on a copy of the project, and says what it would change. */
export async function makePreview(): Promise<Preview> {
  view.previewing = true;
  try {
    return await invoke<Preview>("preview_install", { asked: installRequest() });
  } catch (error) {
    return { code: 2, changes: [], said: [], errors: [String(error)] };
  } finally {
    view.previewing = false;
  }
}

/** Starts a run of the engine. What it says arrives as events, which `listenToRuns` takes. */
async function start(what: string, request: Record<string, unknown>, then: (code: number) => void): Promise<void> {
  if (view.running) {
    return;
  }
  view.running = what;
  afterRun = then;
  view.log.push([]);
  addLine([{ text: what, style: "bold" }]);
  try {
    await invoke("start_run", { asked: request });
  } catch (error) {
    view.running = "";
    afterRun = null;
    say(String(error), "bad");
  }
}

/** Whether a run did what it was for: 1 is a step that did not finish, with the library in. */
function wentIn(code: number): boolean {
  return code === 0 || code === 1;
}

/** Installs what can go in. */
export function installAll(): void {
  const items = installable();
  const names = items.map((item) => item.looked?.name).join(", ");
  void start(`Installing ${names}`, installRequest(), (code) => {
    // Anything else stopped the run, so the list stays to try again.
    if (wentIn(code)) {
      const gone = items.map((item) => item.id);
      view.items = view.items.filter((item) => !gone.includes(item.id));
    }
  });
}

/** Fetches a library's newest version, into the folder it is in. */
export function updateLibrary(library: InstalledLibrary): void {
  if (library.source) {
    void start(
      `Updating ${library.name}`,
      { ...asked(), libraries: [library.source], folders: [[library.name, library.folder]] },
      (code) => {
        if (wentIn(code)) {
          delete view.newer[library.name];
        }
      },
    );
  }
}

/** Takes a library out of the project. */
export function uninstallLibrary(name: string): void {
  void start(`Taking ${name} out`, { ...asked(), uninstall: [name] }, (code) => {
    if (wentIn(code)) {
      delete view.newer[name];
    }
  });
}

/** Answers the question the run is waiting on. Nothing takes what it offers. */
export function answer(reply: string | null): void {
  view.question = null;
  void invoke("answer_run", { answer: reply });
}

let afterRun: ((code: number) => void) | null = null;

/** Takes what a run says from here on. Once, when the window opens. */
export function listenToRuns(): void {
  void listen<Line>("run-line", (event) => addLine(event.payload));
  void listen("run-part", () => view.log.push([]));
  void listen<Question>("run-ask", (event) => {
    view.question = event.payload;
  });
  void listen<number>("run-done", (event) => {
    view.running = "";
    view.question = null;
    afterRun?.(event.payload);
    afterRun = null;
    void reopen();
  });
}

/** What was said since the last empty line, which is what a question that follows is about. */
export function saidJustNow(): Line[] {
  const part = view.log.at(-1) ?? [];
  const lines: Line[] = [];
  for (let index = part.length - 1; index >= 0 && lines.length < QUESTION_LINES; index--) {
    const line = part[index];
    if (isBlank(line)) {
      break;
    }
    lines.unshift(line);
  }
  return lines;
}

function platformLabel(platform: Platform | null): string | undefined {
  return PLATFORMS.find((one) => one.name === platform)?.label;
}

/** What the project is, for its line at the top: the platform, and which file told. */
export function platformLine(project: Project, choices: Choices): string {
  const platform = choices.platform ?? project.platform;
  const label = platformLabel(platform);
  if (!label) {
    return "Nothing here says yet. A library names what it works on, and Change says which this is.";
  }
  if (choices.platform) {
    return `${label}, as you chose`;
  }
  if (project.recorded) {
    return `${label}, from its record of what is installed`;
  }
  const evidence = project.candidates.find((one) => one.platform === platform)?.evidence;
  const others = project.candidates
    .filter((one) => one.platform !== platform)
    .map((one) => platformLabel(one.platform))
    .join(" and ");
  return `${label}${evidence ? `, from ${evidence}` : ""}${others ? `. It looks like ${others} too` : ""}`;
}

/** What a library on the list needs, in words. */
export function needsLine(looked: Looked): string {
  const said: Record<Need["state"], string> = {
    here: "already here",
    update: "updated first",
    get: "installed first",
  };
  return looked.needs.map((need) => `needs ${need.wanted}, ${said[need.state]}`).join("; ");
}
