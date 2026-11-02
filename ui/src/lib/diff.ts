// What changed between two texts, line by line, for the preview. It has no imports, so its tests run in Node as they are.

export type DiffKind = "same" | "added" | "removed";

export type DiffLine = { kind: DiffKind; text: string };

/** A stretch of a diff to show, with the lines around a change, and how many lines were left out before it. */
export type Hunk = { skipped: number; lines: DiffLine[] };

/** Beyond this many pairs of lines it says the middle was replaced, rather than keep the window waiting. */
const LARGEST = 4_000_000;

/** The lines of a text, a line ending at its end making no empty line after it. */
function linesOf(text: string): string[] {
  const lines = text.split("\n").map((line) => line.replace(/\r$/, ""));
  if (lines.at(-1) === "") {
    lines.pop();
  }
  return lines;
}

/** Every line of both texts in order, each the same, added or removed. */
export function lineDiff(before: string, after: string): DiffLine[] {
  const old = linesOf(before);
  const fresh = linesOf(after);

  // What is the same at either end needs no comparing, and it is most of a file an install edits.
  let start = 0;
  while (start < old.length && start < fresh.length && old[start] === fresh[start]) {
    start++;
  }
  let oldEnd = old.length;
  let freshEnd = fresh.length;
  while (oldEnd > start && freshEnd > start && old[oldEnd - 1] === fresh[freshEnd - 1]) {
    oldEnd--;
    freshEnd--;
  }

  const same = (lines: string[]): DiffLine[] => lines.map((text) => ({ kind: "same", text }));

  return [
    ...same(old.slice(0, start)),
    ...middle(old.slice(start, oldEnd), fresh.slice(start, freshEnd)),
    ...same(old.slice(oldEnd)),
  ];
}

/** The longest run of lines the two have in common, by the usual table of lengths. */
function middle(old: string[], fresh: string[]): DiffLine[] {
  const removed = (text: string): DiffLine => ({ kind: "removed", text });
  const added = (text: string): DiffLine => ({ kind: "added", text });

  if (old.length === 0 || fresh.length === 0 || old.length * fresh.length > LARGEST) {
    return [...old.map(removed), ...fresh.map(added)];
  }

  const width = fresh.length + 1;
  const table = new Uint32Array((old.length + 1) * width);
  for (let i = old.length - 1; i >= 0; i--) {
    for (let j = fresh.length - 1; j >= 0; j--) {
      table[i * width + j] =
        old[i] === fresh[j]
          ? table[(i + 1) * width + j + 1] + 1
          : Math.max(table[(i + 1) * width + j], table[i * width + j + 1]);
    }
  }

  const lines: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < old.length || j < fresh.length) {
    if (i < old.length && j < fresh.length && old[i] === fresh[j]) {
      lines.push({ kind: "same", text: old[i] });
      i++;
      j++;
    } else if (i < old.length && (j === fresh.length || table[(i + 1) * width + j] >= table[i * width + j + 1])) {
      lines.push(removed(old[i]));
      i++;
    } else {
      lines.push(added(fresh[j]));
      j++;
    }
  }
  return lines;
}

/** The changes of a diff with `around` lines of what is the same on each side, and what lies between them left out. */
export function hunks(lines: DiffLine[], around = 3): Hunk[] {
  const shown: boolean[] = lines.map((line) => line.kind !== "same");
  lines.forEach((line, index) => {
    if (line.kind !== "same") {
      for (let near = Math.max(0, index - around); near <= Math.min(lines.length - 1, index + around); near++) {
        shown[near] = true;
      }
    }
  });

  const found: Hunk[] = [];
  let skipped = 0;
  let current: Hunk | null = null;
  lines.forEach((line, index) => {
    if (!shown[index]) {
      skipped++;
      current = null;
      return;
    }
    if (current === null) {
      current = { skipped, lines: [] };
      found.push(current);
      skipped = 0;
    }
    current.lines.push(line);
  });
  return found;
}
