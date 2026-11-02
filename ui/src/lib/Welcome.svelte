<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { logo } from "./logo";
  import PathText from "./PathText.svelte";
  import { openProject, view } from "./project.svelte";
  import { settings } from "./settings.svelte";

  let {
    version,
    onopen,
  }: {
    version: string;
    /** Asks for a folder, and opens it as the project. */
    onopen: () => void;
  } = $props();

  const GITHUB = "https://github.com/nimaltd/libNIMA";
  const FORMAT = `${GITHUB}/blob/main/docs/FORMAT.md`;
  const PLATFORMS = ["STM32", "Zephyr", "ESP-IDF", "Arduino", "PlatformIO"];

  const drawn = logo();

  function nameOf(folder: string): string {
    return folder.replace(/[\\/]+$/, "").split(/[\\/]/).at(-1) ?? folder;
  }
</script>

<div class="welcome">
  <svg
    class="logo"
    width={drawn.width}
    height={drawn.height}
    viewBox="0 0 {drawn.width} {drawn.height}"
    role="img"
    aria-label="libNIMA"
  >
    <path class="lib-shadow" d={drawn.libShadow} />
    <path class="lib-face" d={drawn.libFace} />
    <path class="nima-shadow" d={drawn.nimaShadow} />
    <path class="nima-face" d={drawn.nimaFace} />
  </svg>

  <div class="lines">
    <p><span class="version">v{version}</span>&nbsp; installs embedded C libraries into your project</p>
    <p><button class="link" onclick={() => void openUrl(GITHUB)}>{GITHUB}</button></p>
    <p><span class="dim">Format:</span> <button class="link" onclick={() => void openUrl(FORMAT)}>{FORMAT}</button></p>
  </div>

  <div class="table">
    <span class="label">Platforms</span>
    <span>{PLATFORMS.join("  ")}</span>
    <span class="label">Libraries</span>
    <span>owner/name, a GitHub address, a zip or a folder</span>
    <span class="label">Command</span>
    <span>libnima owner/name, from the root of a project</span>
  </div>

  {#if settings.recent.length > 0}
    <div class="table">
      {#each settings.recent as folder, index (folder)}
        <span class="label">{index === 0 ? "Opened lately" : ""}</span>
        <button class="recent" title={folder} onclick={() => void openProject(folder)}>
          <span class="name">{nameOf(folder)}</span>
          <span class="where"><PathText path={folder} /></span>
        </button>
      {/each}
    </div>
  {/if}

  <div class="lines">
    <p class="dim">Open your project's folder with this button, or File &gt; Open project.</p>
    <p class="dim">Dropping the folder on this window does the same.</p>
  </div>

  <button class="btn primary open" onclick={onopen}>Open folder...</button>

  {#if view.problem}
    <p class="problem">{view.problem}</p>
  {/if}
</div>

<style>
  /* The same colors as the logo in terNIMA's welcome text. */
  .welcome {
    --logo-teal: #2dd4bf;
    --logo-teal-shadow: #135950;
    --logo-orange: #fb923c;
    --logo-orange-shadow: #693d19;

    height: 100%;
    padding: 18px 2ch;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: 14px;
    font-variant-ligatures: none;
    line-height: 18px;
    box-sizing: border-box;
  }

  :global(:root[data-theme="light"]) .welcome {
    --logo-teal: #0d9488;
    --logo-teal-shadow: #99f6e4;
    --logo-orange: #ea580c;
    --logo-orange-shadow: #fed7aa;
  }

  .logo {
    display: block;
    shape-rendering: crispEdges;
  }

  .lib-face {
    fill: var(--logo-teal);
  }

  .lib-shadow {
    fill: var(--logo-teal-shadow);
  }

  .nima-face {
    fill: var(--logo-orange);
  }

  .nima-shadow {
    fill: none;
    stroke: var(--logo-orange-shadow);
    stroke-width: 1;
  }

  .lines {
    margin-top: 18px;
  }

  p {
    margin: 0;
  }

  .version {
    color: var(--accent);
  }

  .dim,
  .link {
    color: var(--muted);
  }

  .link {
    padding: 0;
    font: inherit;
    background: none;
    border: 0;
    cursor: pointer;
  }

  .link:hover {
    color: var(--accent);
    text-decoration: underline;
  }

  .table {
    display: grid;
    grid-template-columns: 14ch minmax(0, 1fr);
    margin-top: 18px;
    white-space: pre-wrap;
  }

  .label {
    color: var(--muted);
    white-space: nowrap;
  }

  .recent {
    display: flex;
    gap: 2ch;
    min-width: 0;
    max-width: 90ch;
    padding: 0;
    font: inherit;
    color: var(--text);
    text-align: left;
    background: none;
    border: 0;
    cursor: pointer;
  }

  .recent:hover {
    color: var(--accent);
  }

  .name {
    flex-shrink: 0;
  }

  .where {
    min-width: 0;
    color: var(--muted);
  }

  .where :global(.path) {
    font: inherit;
  }

  .open {
    margin-top: 18px;
    font-family: var(--font-ui);
    font-size: 13px;
  }

  .problem {
    margin-top: 12px;
    color: var(--bad);
  }
</style>
