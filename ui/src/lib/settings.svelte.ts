import { invoke } from "@tauri-apps/api/core";

export type ThemeChoice = "system" | "dark" | "light";

/** Looking on GitHub for a newer libNIMA. */
export type UpdateSettings = {
  /** Looks when the window opens. */
  check: boolean;
  /** A version not to tell about again, such as "1.0.2"; empty for none. */
  skipped: string;
};

/** The same fields as `Settings` in src-tauri's settings.rs. A token is never among them. */
export type Settings = {
  theme: ThemeChoice;
  updates: UpdateSettings;
  /** Asks GitHub for newer versions of a project's libraries when it opens. */
  newer_libraries: boolean;
  /** The projects opened lately, the last one first. */
  recent: string[];
};

/** How many projects the File menu lists. */
const RECENT_KEPT = 8;

/** The settings in use. Read them anywhere; change them only with `updateSettings`. */
export const settings: Settings = $state({
  theme: "system",
  updates: { check: true, skipped: "" },
  newer_libraries: true,
  recent: [],
});

/** Reads the settings from their file, through the Rust side. */
export async function loadSettings(): Promise<void> {
  Object.assign(settings, await invoke<Settings>("get_settings"));
}

/** Changes settings at once, and saves them. */
export function updateSettings(change: Partial<Settings>): void {
  Object.assign(settings, change);
  invoke("set_settings", { settings: $state.snapshot(settings) }).catch((error) => console.error(error));
}

/** Puts a project at the top of those opened lately. */
export function rememberProject(folder: string): void {
  updateSettings({
    recent: [folder, ...settings.recent.filter((one) => one !== folder)].slice(0, RECENT_KEPT),
  });
}

/** Takes a project off the list, once it turns out to be gone. */
export function forgetProject(folder: string): void {
  updateSettings({ recent: settings.recent.filter((one) => one !== folder) });
}
