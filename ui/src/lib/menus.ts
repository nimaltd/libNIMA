// What is in the menu bar.

import { openUrl } from "@tauri-apps/plugin-opener";
import { settings, updateSettings, type ThemeChoice } from "./settings.svelte";
import type { Command } from "./palette";
import { editor, newLibrary, pickLibrary, save } from "./editor.svelte";
import { allChecked, checkAll, installAll, installable, lookForNewer, openProject, view } from "./project.svelte";
import { isMac, keys, matches } from "./shortcuts";

export type Item = {
  label: string;
  shortcut?: string;
  /** Shown on the right where a shortcut would be. */
  hint?: string;
  enabled?: () => boolean;
  checked?: () => boolean;
  run?: () => void;
};

/** A menu's items, with `null` for a separator line and a string for a heading over the items after it. */
export type Menu = { title: string; items: (Item | string | null)[] };

/** What the menus ask of the app around them. */
export type AppActions = {
  about(): void;
  exit(): void;
  /** Asks for a folder, and opens it as the project. */
  openProject(): void;
  /** Asks for a zip, and adds it to the list. */
  addZip(): void;
  /** Asks for a folder, and adds it to the list. */
  addFolder(): void;
  /** Shows a tab. */
  showTab(tab: "project" | "editor"): void;
  /** Whether the editor's tab is in front. */
  inEditor(): boolean;
  undo(): void;
  redo(): void;
  find(): void;
  /** Tries the list on a copy of the project. */
  preview(): void;
  /** Shows what was found of the project, and lets it be changed. */
  changeProject(): void;
  addToPath(): void;
  settings(): void;
  commandPalette(): void;
  /** Looks on GitHub for a newer libNIMA, and says what it found. */
  checkUpdates(): void;
};

const GITHUB = "https://github.com/nimaltd/libNIMA";
/** A path longer than this is shown by its end, which is what tells two projects apart. */
const LONGEST_PATH = 60;
const FORMAT = `${GITHUB}/blob/main/docs/FORMAT.md`;

export function buildMenus(app: AppActions): Menu[] {
  const theme = (label: string, choice: ThemeChoice): Item => ({
    label,
    checked: () => settings.theme === choice,
    run: () => updateSettings({ theme: choice }),
  });
  const hasProject = () => view.project !== null;
  const free = () => view.running === "";
  const canInstall = () => hasProject() && free() && allChecked() && installable().length > 0;

  return [
    {
      title: "File",
      // Projects opened lately come and go, so the menu is made each time it is shown.
      get items() {
        const recent: (Item | string | null)[] =
          settings.recent.length === 0
            ? []
            : [
                null,
                "Opened lately",
                ...settings.recent.map((folder) => ({
                  label: folder.length > LONGEST_PATH ? `...${folder.slice(-LONGEST_PATH)}` : folder,
                  run: () => void openProject(folder),
                })),
              ];
        return [
          { label: "Open project...", shortcut: keys("Ctrl+O", "Cmd+O"), run: app.openProject },
          ...recent,
          null,
          { label: "Add library from a zip...", enabled: hasProject, run: app.addZip },
          { label: "Add library from a folder...", enabled: hasProject, run: app.addFolder },
          null,
          {
            label: "Open a library's file...",
            run: () => {
              app.showTab("editor");
              void pickLibrary();
            },
          },
          {
            label: "New library's file...",
            run: () => {
              app.showTab("editor");
              void newLibrary();
            },
          },
          {
            label: "Save",
            shortcut: keys("Ctrl+S", "Cmd+S"),
            enabled: () => editor.dirty && !editor.readOnly && editor.path !== "",
            run: () => void save(),
          },
          null,
          { label: "Exit", run: app.exit },
        ];
      },
    },
    {
      title: "Edit",
      items: [
        { label: "Undo", hint: keys("Ctrl+Z", "Cmd+Z"), enabled: app.inEditor, run: app.undo },
        { label: "Redo", hint: keys("Ctrl+Y", "Cmd+Shift+Z"), enabled: app.inEditor, run: app.redo },
        null,
        { label: "Find...", hint: keys("Ctrl+F", "Cmd+F"), enabled: app.inEditor, run: app.find },
      ],
    },
    {
      title: "View",
      items: [
        { label: "Project", shortcut: keys("Ctrl+1", "Cmd+1"), checked: () => !app.inEditor(), run: () => app.showTab("project") },
        { label: "libnima.yml", shortcut: keys("Ctrl+2", "Cmd+2"), checked: app.inEditor, run: () => app.showTab("editor") },
        null,
        "libnima.yml",
        {
          label: "Form",
          checked: () => editor.mode === "form",
          run: () => {
            app.showTab("editor");
            editor.mode = "form";
          },
        },
        {
          label: "Text",
          checked: () => editor.mode === "text",
          run: () => {
            app.showTab("editor");
            editor.mode = "text";
          },
        },
        { label: "Summary", checked: () => editor.panel, run: () => (editor.panel = !editor.panel) },
        null,
        "Theme",
        theme("Follow the OS", "system"),
        theme("Dark", "dark"),
        theme("Light", "light"),
        null,
        { label: "Command palette...", shortcut: keys("Ctrl+Shift+P", "Cmd+Shift+P"), run: app.commandPalette },
      ],
    },
    {
      title: "Tools",
      items: [
        { label: "Check the list again", enabled: () => hasProject() && view.items.length > 0, run: checkAll },
        { label: "Preview", enabled: canInstall, run: app.preview },
        { label: "Install all", enabled: canInstall, run: installAll },
        null,
        { label: "Change the platform...", enabled: hasProject, run: app.changeProject },
        { label: "Look for newer versions of its libraries", enabled: hasProject, run: () => void lookForNewer(true) },
        null,
        {
          label: "Convert a library's installer.yml...",
          run: () => {
            app.showTab("editor");
            void pickLibrary();
          },
        },
        { label: "Add libnima to the PATH...", run: app.addToPath },
        { label: "Settings...", run: app.settings },
      ],
    },
    {
      title: "Help",
      items: [
        { label: "libNIMA on GitHub", run: () => void openUrl(GITHUB) },
        { label: "The format of libnima.yml", run: () => void openUrl(FORMAT) },
        { label: "Report a bug...", run: () => void openUrl(`${GITHUB}/issues/new`) },
        { label: "Check for updates...", run: app.checkUpdates },
        null,
        { label: "About libNIMA", run: app.about },
      ],
    },
  ];
}

/** Every menu item that can be used now, as commands for the palette. */
export function menuCommands(menus: Menu[]): Command[] {
  const commands: Command[] = [];
  for (const menu of menus) {
    // Items under a heading are named with it: "Theme: Dark".
    let heading = "";
    for (const item of menu.items) {
      if (typeof item === "string" || item === null) {
        heading = item ?? "";
      } else if (isEnabled(item) && item.run && !item.label.startsWith("Command palette")) {
        commands.push({
          label: `${heading ? `${heading}: ` : ""}${item.label.replace(/\.\.\.$/, "")}`,
          group: menu.title,
          shortcut: item.shortcut,
          checked: item.checked?.() ?? false,
          run: item.run,
        });
      }
    }
  }
  return commands;
}

/** Whether an item can be used right now. */
export function isEnabled(item: Item): boolean {
  return item.run !== undefined && (item.enabled?.() ?? true);
}

/** The item whose shortcut this key press is, if any. */
export function itemForKey(menus: Menu[], event: KeyboardEvent): Item | null {
  for (const menu of menus) {
    for (const item of menu.items) {
      if (item && typeof item !== "string" && item.shortcut && matches(item.shortcut, event)) {
        return item;
      }
    }
  }
  return null;
}

/** Alt+F opens File, Alt+E opens Edit and so on; not on macOS, where Option types characters. */
export function menuForAltKey(menus: Menu[], event: KeyboardEvent): number | null {
  if (isMac || !event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return null;
  }
  const index = menus.findIndex((menu) => event.code === `Key${menu.title[0]}`);
  return index >= 0 ? index : null;
}
