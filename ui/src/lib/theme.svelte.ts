import { settings } from "./settings.svelte";

export type Scheme = "dark" | "light";

const lightQuery = window.matchMedia("(prefers-color-scheme: light)");
const osScheme = (): Scheme => (lightQuery.matches ? "light" : "dark");

// The OS's scheme, kept up to date when the OS switches between dark and light.
const os = $state({ scheme: osScheme() });
lightQuery.addEventListener("change", () => (os.scheme = osScheme()));

/** The color scheme in use: the one picked in the View menu, or else the OS's. */
export function appScheme(): Scheme {
  return settings.theme === "system" ? os.scheme : settings.theme;
}
