<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { confirmFirst } from "./confirm.svelte";
  import PathText from "./PathText.svelte";
  import RunLog from "./RunLog.svelte";
  import Welcome from "./Welcome.svelte";
  import {
    addLibrary,
    allChecked,
    check,
    installAll,
    installable,
    needsLine,
    openProject,
    platformLine,
    removeItem,
    updateLibrary,
    uninstallLibrary,
    view,
    type InstalledLibrary,
    type Item,
  } from "./project.svelte";

  let {
    version,
    onchange,
    onfix,
    onyml,
    onpreview,
    ontoken,
  }: {
    /** The program's version, for the welcome page. */
    version: string;
    /** Opens the dialog for what the engine found: the platform, the IDE and the rest. */
    onchange: () => void;
    /** Shows what the check found, and how to put it right. */
    onfix: (item: Item) => void;
    /** Shows a library's file. */
    onyml: (item: Item) => void;
    onpreview: () => void;
    /** Asks for a GitHub token, and checks the library again. */
    ontoken: (item: Item) => void;
  } = $props();

  let adding = $state("");
  /** What was opened or closed of the rows, by id. A row with choices to make starts opened. */
  let opened: Record<number, boolean> = $state({});

  const project = $derived(view.project);
  const busy = $derived(view.running !== "");

  export async function pickProject() {
    const folder = await open({ title: "Your project's folder", directory: true, multiple: false });
    if (typeof folder === "string") {
      await openProject(folder);
    }
  }

  function add() {
    addLibrary(adding);
    adding = "";
  }

  export async function pickZip() {
    const file = await open({
      title: "A library's zip",
      multiple: false,
      directory: false,
      filters: [{ name: "Zip", extensions: ["zip"] }],
    });
    if (typeof file === "string") {
      addLibrary(file);
    }
  }

  export async function pickFolder() {
    const folder = await open({ title: "A library's folder", directory: true, multiple: false });
    if (typeof folder === "string") {
      addLibrary(folder);
    }
  }

  function confirmUninstall(library: InstalledLibrary) {
    confirmFirst(
      `Take ${library.name} out?`,
      `Its files and its place in the build go. What you wrote in them is saved beside them first, and files that became yours stay.`,
      "Take it out",
      () => uninstallLibrary(library.name),
    );
  }

  /** What the row says beside the name. */
  function rowText(item: Item): string {
    const looked = item.looked;
    if (!looked) {
      return "Checking...";
    }
    if (looked.why) {
      return looked.why.split("\n")[0];
    }
    const needs = needsLine(looked);
    const over = looked.installed !== null ? `over the ${looked.installed || "copy"} in this project` : "";
    return [over, needs].filter(Boolean).join("; ") || looked.description;
  }

  function hasMore(item: Item): boolean {
    const looked = item.looked;
    return looked !== null && looked.mark !== "no";
  }

  function isOpen(item: Item): boolean {
    return opened[item.id] ?? (item.looked !== null && (item.looked.options.length > 0 || item.looked.steps.length > 0));
  }

</script>

<div class="tab">
  {#if !project}
    <Welcome {version} onopen={() => void pickProject()} />
  {:else}
    <div class="body">
      <section class="card">
        <div class="line">
          <span class="key">Project</span>
          <span class="value"><PathText path={project.folder} /></span>
          <button class="btn" onclick={() => void pickProject()}>Open folder...</button>
        </div>
        <div class="line">
          <span class="key">Platform</span>
          <span class="value">
            {platformLine(project, view.choices)}
            {#if project.ides.length > 0}
              <span class="dlg-muted">&nbsp; IDE: {project.ides.join(", ")}</span>
            {/if}
          </span>
          <button class="btn" onclick={onchange}>Change...</button>
        </div>
      </section>

      <section class="card">
        <div class="line">
          <span class="key">Add library</span>
          <input
            class="field mono value"
            placeholder="owner/name, a GitHub address, a .zip or a folder"
            spellcheck="false"
            bind:value={adding}
            onkeydown={(event) => {
              if (event.key === "Enter") {
                add();
              }
            }}
          />
          <button class="btn primary" disabled={!adding.trim()} onclick={add}>Add</button>
          <button class="btn" title="A zip from GitHub's Download ZIP" onclick={() => void pickZip()}>Zip...</button>
          <button class="btn" title="A library's folder on this computer" onclick={() => void pickFolder()}>Folder...</button>
        </div>
      </section>

      {#if view.items.length > 0}
        <section class="card">
          <h2 class="dlg-section">To install</h2>
          {#each view.items as item (item.id)}
            {@const looked = item.looked}
            <div class="item">
              <div class="row">
                <span class="mark {looked?.mark ?? 'wait'}">{looked ? looked.mark.toUpperCase() : "..."}</span>
                <span class="name">{looked?.name || item.given}</span>
                <span class="version mono">{looked?.version ?? ""}</span>
                <span class="why" title={looked?.why || rowText(item)}>{rowText(item)}</span>
                {#if looked?.needs_token}
                  <button class="btn" onclick={() => ontoken(item)}>Give a token...</button>
                {/if}
                {#if looked?.mark === "check"}
                  <button class="btn" onclick={() => onfix(item)}>How to fix</button>
                {:else if looked?.mark === "no"}
                  <button class="btn" onclick={() => onfix(item)}>Why</button>
                {/if}
                {#if looked?.manifest}
                  <button class="btn" title="Shows its file" onclick={() => onyml(item)}>yml</button>
                {/if}
                {#if hasMore(item)}
                  <button
                    class="btn"
                    title="Its folder, options and steps"
                    onclick={() => (opened[item.id] = !isOpen(item))}
                  >
                    {isOpen(item) ? "Less" : "More"}
                  </button>
                {/if}
                {#if looked?.mark === "no"}
                  <button class="btn" title="Checks it again" onclick={() => void check(item.id)}>Again</button>
                {/if}
                <button class="btn" title="Takes it off the list" onclick={() => removeItem(item.id)}>Remove</button>
              </div>
              {#if looked && hasMore(item) && isOpen(item)}
                <div class="more">
                  {#if looked.description}
                    <p class="dlg-muted">{looked.description}</p>
                  {/if}
                  {#if !looked.in_place}
                    <label class="choice">
                      <span>Folder</span>
                      <input class="field mono" spellcheck="false" bind:value={item.folder} />
                    </label>
                  {/if}
                  {#each looked.options as option (option.name)}
                    <label class="dlg-check">
                      <input type="checkbox" bind:checked={item.options[option.name]} />
                      <span>
                        <b>{option.name}</b>{option.description ? `: ${option.description}` : ""}
                        {#if option.needs.length > 0}
                          <span class="dlg-muted">(brings {option.needs.join(", ")})</span>
                        {/if}
                      </span>
                    </label>
                  {/each}
                  {#each looked.steps as step (step.label)}
                    <label class="dlg-check step">
                      <input type="checkbox" bind:checked={item.steps[step.label]} />
                      <span>
                        <b>Run {step.label}</b>{step.description ? `: ${step.description}` : ""}
                        <code class="mono">{step.command}</code>
                      </span>
                    </label>
                  {/each}
                </div>
              {/if}
            </div>
          {/each}
          <div class="actions">
            <button
              class="btn"
              disabled={busy || view.previewing || !allChecked() || installable().length === 0}
              title="Tries it on a copy of the project, and shows what it would change"
              onclick={onpreview}
            >
              {view.previewing ? "Trying it..." : "Preview"}
            </button>
            <button
              class="btn primary"
              disabled={busy || !allChecked() || installable().length === 0}
              onclick={installAll}
            >
              Install all
            </button>
          </div>
        </section>
      {/if}

      {#if project.installed.length > 0}
        <section class="card">
          <h2 class="dlg-section">Installed</h2>
          {#each project.installed as library (library.name)}
            <div class="row installed">
              <span class="name">{library.name}</span>
              <span class="version mono">{library.version}</span>
              {#if view.newer[library.name]}
                <span class="newer">newer: {view.newer[library.name]}</span>
              {/if}
              <span class="why mono" title={library.origin}>{library.folder}</span>
              <button
                class="btn"
                disabled={busy || !library.source}
                title={library.source ? `Fetches it again from ${library.source}` : "It came from a zip: add the new zip"}
                onclick={() => updateLibrary(library)}
              >
                Update
              </button>
              <button class="btn" disabled={busy} onclick={() => confirmUninstall(library)}>Uninstall</button>
            </div>
          {/each}
        </section>
      {/if}
    </div>

    <RunLog />
  {/if}
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .body {
    display: flex;
    flex: 1;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
    padding: 12px;
    overflow-y: auto;
  }

  .card {
    flex-shrink: 0;
    padding: 8px 12px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .card h2 {
    margin: 4px 0 6px;
  }

  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
  }

  .key {
    flex-shrink: 0;
    width: 78px;
    color: var(--muted);
  }

  .value {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .item + .item {
    border-top: 1px solid var(--border);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 36px;
  }

  .installed + .installed {
    border-top: 1px solid var(--border);
  }

  .mark {
    flex-shrink: 0;
    width: 50px;
    padding: 1px 0;
    font-size: 11px;
    font-weight: 700;
    text-align: center;
    border: 1px solid currentColor;
    border-radius: 4px;
  }

  .mark.ok {
    color: var(--ok);
  }

  .mark.check {
    color: var(--warn);
  }

  .mark.no {
    color: var(--bad);
  }

  .mark.wait {
    color: var(--muted);
  }

  .name {
    flex-shrink: 0;
    max-width: 240px;
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .version {
    flex-shrink: 0;
    color: var(--muted);
  }

  .newer {
    flex-shrink: 0;
    padding: 1px 6px;
    font-size: 12px;
    color: var(--accent);
    border: 1px solid var(--accent);
    border-radius: 4px;
  }

  .why {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .more {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 0 0 10px 58px;
  }

  .more p {
    margin: 0;
  }

  .choice {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
  }

  .choice .field {
    flex: 1;
    max-width: 380px;
  }

  .step {
    align-items: flex-start;
  }

  .step input {
    margin-top: 3px;
  }

  code {
    display: block;
    margin-top: 2px;
    padding: 2px 6px;
    color: var(--text);
    word-break: break-all;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    user-select: text;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 8px 0 4px;
  }
</style>
