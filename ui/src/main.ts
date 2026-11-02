import { mount } from "svelte";

// Fonts ship with the app, so text looks the same on every OS.
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/700.css";
import "@fontsource-variable/inter";
import "./app.css";

import App from "./App.svelte";

const target = document.getElementById("app");
if (!target) {
  throw new Error("index.html has no #app element");
}

export default mount(App, { target });
