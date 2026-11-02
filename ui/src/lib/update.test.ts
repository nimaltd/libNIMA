// Run with npm test: Node runs TypeScript as it is.

import assert from "node:assert/strict";
import { test } from "node:test";
import { isNewer } from "./update.ts";

test("a higher version is newer, part by part as numbers", () => {
  assert.equal(isNewer("1.0.1", "1.0.0"), true);
  assert.equal(isNewer("1.1.0", "1.0.9"), true);
  assert.equal(isNewer("2.0.0", "1.9.9"), true);
  // 10 is more than 9, though "10" sorts before "9" as text.
  assert.equal(isNewer("1.10.0", "1.9.0"), true);
});

test("the same or an older version is not newer", () => {
  assert.equal(isNewer("1.0.0", "1.0.0"), false);
  assert.equal(isNewer("1.0.0", "1.0.1"), false);
  assert.equal(isNewer("1.0", "1.0.0"), false);
});

test("a v in front of a tag, and what follows a dash, are left out", () => {
  assert.equal(isNewer("v1.0.1", "1.0.0"), true);
  assert.equal(isNewer("v1.0.0", "1.0.0"), false);
  assert.equal(isNewer("1.0.1-beta", "1.0.0"), true);
});
