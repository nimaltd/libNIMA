// Builds the libnima command and puts it where the bundler takes it from, so it ends up beside the window:
// src-tauri/binaries/libnima-<target>. Run by `npm run package`, before the window is built.

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";

const run = (program, args) => execFileSync(program, args, { stdio: "inherit" });

const host = /^host: (.+)$/m.exec(execFileSync("rustc", ["-vV"], { encoding: "utf8" }))[1];
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host;
const extension = target.includes("windows") ? ".exe" : "";

/** Builds it for one target, and says where it is. */
function build(triple) {
  if (triple === host) {
    run("cargo", ["build", "--release", "-p", "libnima-cli"]);
    return `target/release/libnima${extension}`;
  }
  run("cargo", ["build", "--release", "-p", "libnima-cli", "--target", triple]);
  return `target/${triple}/release/libnima${extension}`;
}

const out = `src-tauri/binaries/libnima-${target}${extension}`;
mkdirSync("src-tauri/binaries", { recursive: true });

if (target === "universal-apple-darwin") {
  // Tauri builds the window for each of the two, each build wanting the command of its own, and then bundles the one made of both.
  const parts = ["aarch64-apple-darwin", "x86_64-apple-darwin"].map((triple) => {
    const part = `src-tauri/binaries/libnima-${triple}`;
    copyFileSync(build(triple), part);
    return part;
  });
  run("lipo", ["-create", "-output", out, ...parts]);
} else {
  copyFileSync(build(target), out);
}

console.log(`The command for the bundle: ${out}`);
