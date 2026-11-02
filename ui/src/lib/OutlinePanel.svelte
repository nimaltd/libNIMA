<script lang="ts">
  import { editor, setValue, showPlace, type Unlisted } from "./editor.svelte";

  const outline = $derived(editor.outline);

  /** Puts a file on the list it belongs on. */
  function addFile(file: Unlisted) {
    const now = editor.outline?.data?.[file.belongs];
    const list = Array.isArray(now) ? now : [];
    void setValue([file.belongs], [...list, file.path]);
  }
</script>

<aside class="panel">
  <h2>What this file does</h2>

  {#if !outline || outline.error}
    <p class="muted">Nothing yet: the file has to read first.</p>
  {:else}
    <div class="block">
      <p class="name">
        <span class="mono">{outline.name || "no name yet"}</span>
        {#if outline.version}
          <span class="mono version">{outline.version}</span>
        {/if}
      </p>
      {#if outline.version_from}
        <p class="muted small">from {outline.version_from}</p>
      {:else}
        <p class="warn small">No version: give the first header a @version tag</p>
      {/if}
      {#if outline.include}
        <p class="mono small include">#include "{outline.include}"</p>
      {/if}
    </div>

    <div class="block">
      <h3>Installs on</h3>
      {#each outline.platforms as platform (platform.name)}
        <button class="row" onclick={() => showPlace(platform.line)} title={platform.problem ?? "Nothing stands in its way"}>
          <span class="mark" class:bad={platform.problem}>{platform.problem ? "NO" : "OK"}</span>
          <span>{platform.label}</span>
        </button>
      {:else}
        <p class="warn small">No platform yet</p>
      {/each}
    </div>

    {#each outline.lists as list (list.key)}
      <div class="block">
        <h3>{list.label} <span class="muted">{list.entries.reduce((sum, entry) => sum + entry.files.length, 0)}</span></h3>
        {#each list.entries as entry, index (index)}
          <button class="row" class:gone={entry.missing} onclick={() => showPlace(entry.line)} title={entry.files.join("\n") || entry.given}>
            <span class="mono path">{entry.given || "an empty line"}</span>
            {#if entry.missing}
              <span class="tag">missing</span>
            {:else if entry.files.length > 1}
              <span class="muted small">{entry.files.length}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/each}

    {#if outline.unlisted.length > 0}
      <div class="block">
        <h3>In the folder, not listed</h3>
        {#each outline.unlisted.slice(0, 20) as file (file.path)}
          <div class="row still">
            <span class="mono path" title={file.path}>{file.path}</span>
            <button class="add" title={`Add it to ${file.belongs}`} onclick={() => addFile(file)}>Add</button>
          </div>
        {/each}
        {#if outline.unlisted.length > 20}
          <p class="muted small">and {outline.unlisted.length - 20} more</p>
        {/if}
      </div>
    {/if}

    {#if outline.needs.length > 0 || outline.peripherals.length > 0}
      <div class="block">
        <h3>Needs</h3>
        {#each outline.needs as need (need)}
          <p class="mono small">{need}</p>
        {/each}
        {#if outline.peripherals.length > 0}
          <p class="small">{outline.peripherals.join(", ")}</p>
        {/if}
      </div>
    {/if}

    {#if outline.options.length > 0}
      <div class="block">
        <h3>Options</h3>
        <p class="mono small">{outline.options.join(", ")}</p>
      </div>
    {/if}

    {#if outline.steps.length > 0}
      <div class="block">
        <h3>Steps, each asked about first</h3>
        <p class="mono small">{outline.steps.join(", ")}</p>
      </div>
    {/if}
  {/if}
</aside>

<style>
  .panel {
    width: 270px;
    flex-shrink: 0;
    padding: 10px 12px;
    overflow-y: auto;
    background: var(--panel);
    border-left: 1px solid var(--border);
  }

  /* Where there is no room for it beside the form, Summary has nothing to take room from. */
  @media (max-width: 860px) {
    .panel {
      display: none;
    }
  }

  h2 {
    margin: 0 0 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }

  h3 {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
  }

  p {
    margin: 0 0 2px;
  }

  .block {
    margin-bottom: 12px;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
    user-select: text;
  }

  .version {
    margin-left: 4px;
    font-weight: 400;
    color: var(--muted);
  }

  .small {
    font-size: 11px;
  }

  .muted {
    color: var(--muted);
  }

  .warn {
    color: var(--warn);
  }

  .include {
    margin-top: 4px;
    color: var(--accent);
    user-select: text;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: 20px;
    padding: 1px 4px;
    font: inherit;
    color: var(--text);
    text-align: left;
    background: none;
    border: 0;
    border-radius: 4px;
    cursor: default;
  }

  button.row:hover {
    background: var(--hover);
  }

  .row.still {
    padding-right: 0;
  }

  .row.gone .path {
    color: var(--bad);
  }

  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mark {
    width: 26px;
    font-size: 10px;
    font-weight: 700;
    color: var(--ok);
    text-align: center;
    border: 1px solid currentColor;
    border-radius: 3px;
  }

  .mark.bad {
    color: var(--bad);
  }

  .tag {
    flex-shrink: 0;
    font-size: 10px;
    color: var(--bad);
  }

  .add {
    flex-shrink: 0;
    height: 18px;
    padding: 0 6px;
    font: inherit;
    font-size: 11px;
    color: var(--accent);
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
  }
</style>
