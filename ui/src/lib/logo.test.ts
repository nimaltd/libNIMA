import assert from "node:assert/strict";
import { test } from "node:test";
import { CELL_HEIGHT, CELL_WIDTH, LIB, LIB_TOP, LIB_WIDTH, LINES, NIMA, logo } from "./logo.ts";

test("every row of a letter is as wide as the rest of it, so the letters stand straight", () => {
  for (const letter of NIMA) {
    const widths = new Set(letter.map((row) => [...row].length));
    assert.equal(widths.size, 1, letter.join("\n"));
    assert.equal(letter.length, 6);
  }
});

test("every character of the letters has a shape", () => {
  for (const character of NIMA.flat().join("")) {
    assert.ok(character === " " || character === "█" || character in LINES, `no shape for ${character}`);
  }
});

test("the small letters stand on the same line as the big ones, whose letters end on the fifth line", () => {
  assert.equal(LIB_TOP + LIB.length, 2 * 5);
});

test("the small letters and their shadow fit the room they are given", () => {
  const widest = Math.max(...LIB.map((row) => row.trimEnd().length));
  assert.ok(widest + 1 < LIB_WIDTH);
  assert.equal(new Set(LIB.map((row) => row.length)).size, 1);
});

test("the picture is as big as its six lines and all its columns", () => {
  const drawn = logo();
  const letters = NIMA.reduce((sum, letter) => sum + [...letter[0]].length, 0);

  assert.equal(drawn.width, (LIB_WIDTH + letters) * CELL_WIDTH);
  assert.equal(drawn.height, 6 * CELL_HEIGHT);
});

test("the paths hold numbers only, and stay inside the picture", () => {
  const drawn = logo();

  for (const path of [drawn.libFace, drawn.libShadow, drawn.nimaFace, drawn.nimaShadow]) {
    assert.ok(path.length > 0);
    assert.ok(!path.includes("NaN") && !path.includes("undefined"), path);
  }

  for (const [, x, y] of drawn.nimaShadow.matchAll(/M(-?[\d.]+) (-?[\d.]+)/g)) {
    assert.ok(Number(x) >= 0 && Number(x) <= drawn.width, x);
    assert.ok(Number(y) >= 0 && Number(y) <= drawn.height, y);
  }
});
