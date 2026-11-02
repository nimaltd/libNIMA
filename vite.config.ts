import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// The UI lives in ui/ and is built into dist/, where Tauri picks it up.
export default defineConfig({
  root: "ui",
  plugins: [svelte({ configFile: "../svelte.config.js" })],
  // Keep Rust's compile errors on screen, and use the fixed port Tauri expects.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    outDir: "../dist",
    emptyOutDir: true,
    // WebView2 on Windows, WebKit on macOS and Linux.
    target: ["es2022", "chrome105", "safari15"],
    // The UI loads from disk, not over a network, so one large file is fine.
    chunkSizeWarningLimit: 2048,
  },
});
