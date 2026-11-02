import assert from "node:assert/strict";
import { test } from "node:test";
import { hunks, lineDiff } from "./diff.ts";

const kinds = (before: string, after: string) =>
  lineDiff(before, after)
    .map((line) => `${{ same: " ", added: "+", removed: "-" }[line.kind]}${line.text}`)
    .join("\n");

test("the same text has nothing added or removed", () => {
  assert.equal(kinds("a\nb\n", "a\nb\n"), " a\n b");
});

test("a line added in the middle is the only change", () => {
  assert.equal(kinds("a\nc\n", "a\nb\nc\n"), " a\n+b\n c");
});

test("a line removed is the only change", () => {
  assert.equal(kinds("a\nb\nc\n", "a\nc\n"), " a\n-b\n c");
});

test("a changed line is one removed and one added", () => {
  assert.equal(kinds("a\nb\nc\n", "a\nx\nc\n"), " a\n-b\n+x\n c");
});

test("line endings do not count as a change", () => {
  assert.equal(kinds("a\r\nb\r\n", "a\nb\n"), " a\n b");
});

test("from nothing every line is added, and to nothing every line removed", () => {
  assert.equal(kinds("", "a\nb\n"), "+a\n+b");
  assert.equal(kinds("a\nb\n", ""), "-a\n-b");
});

test("a line moved is found between what stays", () => {
  assert.equal(kinds("a\nb\nc\nd\n", "a\nc\nb\nd\n"), " a\n-b\n c\n+b\n d");
});

test("texts too long to compare say the middle was replaced", () => {
  const many = (prefix: string) => Array.from({ length: 3000 }, (_, index) => `${prefix}${index}`).join("\n");
  const lines = lineDiff(many("a"), many("b"));

  assert.equal(lines.length, 6000);
  assert.equal(lines[0].kind, "removed");
  assert.equal(lines[5999].kind, "added");
});

test("a hunk holds a change and the lines around it, and says what it left out", () => {
  const before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
  const after = "1\n2\n3\n4\n5\nx\n6\n7\n8\n9\n";
  const [only, ...rest] = hunks(lineDiff(before, after), 2);

  assert.equal(rest.length, 0);
  assert.equal(only.skipped, 3);
  assert.deepEqual(
    only.lines.map((line) => line.text),
    ["4", "5", "x", "6", "7"],
  );
});

test("two changes far apart are two hunks", () => {
  const before = Array.from({ length: 30 }, (_, index) => `${index}`).join("\n");
  const after = before.replace("3\n", "three\n").replace("25\n", "twenty five\n");
  const found = hunks(lineDiff(before, after), 1);

  assert.equal(found.length, 2);
  assert.ok(found[1].skipped > 0);
});

test("no change has no hunks", () => {
  assert.deepEqual(hunks(lineDiff("a\n", "a\n")), []);
});
