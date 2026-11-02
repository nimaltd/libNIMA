// Keyboard shortcuts, written as "Ctrl+Shift+C" or "Cmd+K".

export const isMac = navigator.userAgent.includes("Mac");

/** Picks the shortcut for this OS: macOS uses Cmd where the others use Ctrl+Shift. */
export function keys(others: string, mac: string): string {
  return isMac ? mac : others;
}

/** Whether a key press is the given shortcut. */
export function matches(shortcut: string, event: KeyboardEvent): boolean {
  const parts = shortcut.split("+");
  const key = parts.pop()!.toLowerCase();

  // Ctrl+= and Ctrl++ both make text bigger, as in browsers, so Shift does not
  // matter for = and -.
  const shiftFree = key === "=" || key === "-";
  if (
    event.ctrlKey !== parts.includes("Ctrl") ||
    event.altKey !== parts.includes("Alt") ||
    event.metaKey !== parts.includes("Cmd") ||
    (!shiftFree && event.shiftKey !== parts.includes("Shift"))
  ) {
    return false;
  }

  // Letters and digits by key position, so shortcuts also work with other
  // keyboard layouts, such as Persian or Russian.
  if (/^[a-z]$/.test(key)) {
    return event.code === `Key${key.toUpperCase()}`;
  }
  if (/^[0-9]$/.test(key)) {
    return event.code === `Digit${key}` || event.code === `Numpad${key}`;
  }
  return normalKey(event.key) === key;
}

/** How a shortcut is shown in a menu: as written, or with macOS's key symbols. */
export function shortcutLabel(shortcut: string): string {
  if (!isMac) {
    return shortcut;
  }
  return shortcut
    .replace("Ctrl+", "⌃")
    .replace("Alt+", "⌥")
    .replace("Shift+", "⇧")
    .replace("Cmd+", "⌘");
}

/** The key as written in shortcuts: + counts as =, and _ as -. */
function normalKey(key: string): string {
  const lower = key.toLowerCase();
  if (lower === "+") return "=";
  if (lower === "_") return "-";
  return lower;
}
