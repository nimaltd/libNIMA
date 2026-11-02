// How a list of words is written in one field, and read back from it. It has no imports, so its tests run in Node as they are.

/** A word as it stands in the field, in quotes when it holds a space. */
function shown(word: unknown): string {
  const text = String(word);
  return text.includes(" ") || text === "" ? `"${text}"` : text;
}

/** A list of words as one line of text. */
export function wordsToText(words: unknown): string {
  return Array.isArray(words) ? words.map(shown).join(" ") : "";
}

/** A number where a word is one, as YAML reads it, and otherwise the word. */
export function typed(word: string): string | number {
  const number = Number(word);
  return word !== "" && String(number) === word ? number : word;
}

/** A line of text as a list of words: split at spaces, except inside double quotes. */
export function textToWords(text: string): (string | number)[] {
  const words: (string | number)[] = [];

  for (const found of text.matchAll(/"([^"]*)"|(\S+)/g)) {
    words.push(found[1] !== undefined ? found[1] : typed(found[2]));
  }

  return words;
}

/** What a field of a word or a number holds, read as YAML would read it. */
export function typedValue(text: string): string | number | boolean {
  if (text === "true" || text === "false") {
    return text === "true";
  }
  return typed(text);
}
