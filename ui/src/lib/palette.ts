// The command palette's matching: a few letters find a command, such as "sf"
// for "Send file" or "hex" for "Hex", the best matches first.

/** A command the palette can run. */
export type Command = {
  label: string;
  /** Where it comes from, such as the menu it is in: "File", "Tools". */
  group: string;
  shortcut?: string;
  /** Shown with a check mark, as a menu shows a setting that is on. */
  checked?: boolean;
  run: () => void;
};

/** How well `text` matches, and which of its characters matched, or null for no match. */
export type Match = { score: number; positions: number[] };

/**
 * Matches the letters of `query` in order, anywhere in `text`, ignoring case
 * and the spaces in the query. A match at the start of a word, or right after
 * the last one, counts for more, so "sf" ranks "Send file" over "Save output as".
 */
export function match(query: string, text: string): Match | null {
  const wanted = query.toLowerCase().replace(/\s+/g, "");
  if (wanted === "") {
    return { score: 0, positions: [] };
  }
  const lower = text.toLowerCase();
  // Try starting the match at each place the first letter is, and keep the best.
  let best: Match | null = null;
  for (let first = lower.indexOf(wanted[0]); first >= 0; first = lower.indexOf(wanted[0], first + 1)) {
    // Jumping ahead to word starts can leave too few letters for the rest,
    // so the plain way is tried too.
    for (const words of [true, false]) {
      const found = matchFrom(wanted, text, lower, first, words);
      if (found && (!best || found.score > best.score)) {
        best = found;
      }
    }
  }
  return best;
}

function matchFrom(wanted: string, text: string, lower: string, first: number, words: boolean): Match | null {
  const positions = [first];
  let score = scoreAt(text, first, -2);
  for (let index = 1; index < wanted.length; index++) {
    const last = positions[positions.length - 1];
    // A letter at the start of a word is better than the next one along.
    let at = lower.indexOf(wanted[index], last + 1);
    if (at < 0) {
      return null;
    }
    if (words && at !== last + 1) {
      for (let next = at; next >= 0; next = lower.indexOf(wanted[index], next + 1)) {
        if (isWordStart(text, next)) {
          at = next;
          break;
        }
      }
    }
    positions.push(at);
    score += scoreAt(text, at, last);
  }
  // Shorter names, and matches nearer the start, win ties.
  return { score: score - positions[0] * 0.1 - text.length * 0.01, positions };
}

function scoreAt(text: string, at: number, last: number): number {
  let score = 1;
  if (isWordStart(text, at)) score += 6;
  if (at === last + 1) score += 4;
  return score;
}

/** Whether a letter starts a word: the first one, after a space or a sign, or a capital after a small letter. */
function isWordStart(text: string, at: number): boolean {
  if (at === 0) return true;
  const before = text[at - 1];
  if (/[\s:/._\-(]/.test(before)) return true;
  return /[a-z]/.test(before) && /[A-Z]/.test(text[at]);
}

/** The commands that match, best first; all of them, recent ones first, without a query. */
export function search(
  query: string,
  commands: Command[],
  recent: string[],
): { command: Command; positions: number[] }[] {
  if (query.trim() === "") {
    const rank = (command: Command) => {
      const at = recent.indexOf(key(command));
      return at < 0 ? recent.length : at;
    };
    return [...commands]
      .map((command, index) => ({ command, index }))
      .sort((a, b) => rank(a.command) - rank(b.command) || a.index - b.index)
      .map(({ command }) => ({ command, positions: [] }));
  }
  return commands
    .map((command) => ({ command, found: match(query, command.label) }))
    .filter((entry): entry is { command: Command; found: Match } => entry.found !== null)
    .sort((a, b) => b.found.score - a.found.score)
    .map(({ command, found }) => ({ command, positions: found.positions }));
}

/** What tells a command apart, to remember the ones used lately. */
export function key(command: Command): string {
  return `${command.group}\u0000${command.label}`;
}
