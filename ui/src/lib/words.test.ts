import assert from "node:assert/strict";
import { test } from "node:test";
import { textToWords, typed, typedValue, wordsToText } from "./words.ts";

test("a line of words is split at its spaces", () => {
  assert.deepEqual(textToWords("imgtool keygen -k  product.key"), ["imgtool", "keygen", "-k", "product.key"]);
});

test("a word with a space stays whole in double quotes", () => {
  assert.deepEqual(textToWords('python "my tools/x.py" -v'), ["python", "my tools/x.py", "-v"]);
  assert.equal(wordsToText(["python", "my tools/x.py", "-v"]), 'python "my tools/x.py" -v');
});

test("what is written comes back the same", () => {
  const words = ["a", "b c", 5, "-k"];
  assert.deepEqual(textToWords(wordsToText(words)), words);
});

test("a number stays a number only when it reads back the same", () => {
  assert.equal(typed("64"), 64);
  assert.equal(typed("1.5"), 1.5);
  assert.equal(typed("007"), "007");
  assert.equal(typed("0x10"), "0x10");
  assert.equal(typed("1.0.0"), "1.0.0");
  assert.equal(typed(""), "");
});

test("a value is true, false, a number or text", () => {
  assert.equal(typedValue("true"), true);
  assert.equal(typedValue("false"), false);
  assert.equal(typedValue("8"), 8);
  assert.equal(typedValue("text"), "text");
});

test("nothing given is no words", () => {
  assert.equal(wordsToText(undefined), "");
  assert.deepEqual(textToWords("   "), []);
});
