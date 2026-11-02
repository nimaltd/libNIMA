<script lang="ts">
  import Modal from "./Modal.svelte";
  import { hunks, lineDiff } from "./diff";
  import type { Change, Preview } from "./project.svelte";

  let {
    preview,
    oninstall,
    onclose,
  }: {
    preview: Preview;
    oninstall: () => void;
    onclose: () => void;
  } = $props();

  let chosen = $state<Change | null>(null);

  const edited = $derived(preview.changes.filter((change) => change.kind === "changed"));
  const added = $derived(preview.changes.filter((change) => change.kind === "added"));
  const removed = $derived(preview.changes.filter((change) => change.kind === "removed"));
  const failed = $derived(preview.code !== 0 && preview.code !== 1);
  const shown = $derived(chosen ? hunks(lineDiff(chosen.before ?? "", chosen.after ?? "")) : []);
  const tooBig = $derived(
    chosen !== null &&
      ((chosen.kind !== "added" && chosen.before === null) || (chosen.kind !== "removed" && chosen.after === null)),
  );
  const sign = { same: " ", added: "+", removed: "-" };

  function submit() {
    if (!failed) {
      onclose();
      oninstall();
    }
  }
</script>

{#snippet group(title: string, changes: Change[])}
  {#if changes.length > 0}
    <h2 class="dlg-section">{title} ({changes.length})</h2>
    <div class="files">
      {#each changes as change (change.path)}
        <button
          class="file mono"
          class:chosen={chosen?.path === change.path}
          title={change.path}
          onclick={() => (chosen = change)}
        >
          {change.path}
        </button>
      {/each}
    </div>
  {/if}
{/snippet}

<Modal title="What the install would change" wide {onclose} onsubmit={submit}>
  {#if failed}
    <p class="dlg-error">It could not go in, and nothing was changed:</p>
    {#each preview.errors as error, index (index)}
      <p class="said">{error}</p>
    {/each}
  {:else}
    <p class="dlg-hint">
      Tried on a copy of the project, and the project itself is as it was. Questions took the answer they offer, and no step
      was run. Each file edited is saved as a .bak first.
    </p>
    <div class="split">
      <div class="list">
        {@render group("Files of the project edited", edited)}
        {@render group("New files", added)}
        {@render group("Files that go", removed)}
        {#if preview.changes.length === 0}
          <p class="dlg-muted">Nothing would change: it is all there already.</p>
        {/if}
      </div>
      <div class="diff mono">
        {#if !chosen}
          <p class="dlg-muted">Click a file to see what changes in it.</p>
        {:else if tooBig}
          <p class="dlg-muted">It is too big to show here, or is not text.</p>
        {:else}
          {#each shown as hunk, index (index)}
            {#if hunk.skipped > 0}
              <div class="skipped">{hunk.skipped} lines the same</div>
            {/if}
            {#each hunk.lines as line, at (at)}
              <div class="dline {line.kind}">{sign[line.kind]} {line.text}</div>
            {/each}
          {/each}
        {/if}
      </div>
    </div>
  {/if}

  {#snippet buttons()}
    <button class="btn" onclick={onclose}>Close</button>
    <button class="btn primary" disabled={failed} onclick={submit}>Install all</button>
  {/snippet}
</Modal>

<style>
  p {
    margin: 4px 0 8px;
    line-height: 1.5;
  }

  .said {
    white-space: pre-wrap;
    user-select: text;
  }

  .split {
    display: grid;
    grid-template-columns: 300px 1fr;
    gap: 10px;
    height: min(58vh, 520px);
    margin-top: 8px;
  }

  .list {
    min-width: 0;
    padding-right: 4px;
    overflow-y: auto;
  }

  .list h2:first-child {
    margin-top: 0;
  }

  .files {
    display: flex;
    flex-direction: column;
  }

  .file {
    overflow: hidden;
    padding: 2px 6px;
    font-size: 12px;
    color: var(--text);
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
    background: none;
    border: 1px solid transparent;
    border-radius: 4px;
  }

  .file:hover {
    background: var(--hover);
  }

  .file.chosen {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  .diff {
    min-width: 0;
    padding: 6px 0;
    overflow: auto;
    font-size: 12px;
    line-height: 1.5;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    user-select: text;
  }

  .diff p {
    margin: 8px 12px;
    font-family: var(--font-ui);
  }

  .dline {
    min-height: 1.5em;
    padding: 0 10px;
    white-space: pre;
  }

  .dline.added {
    color: var(--ok);
    background: color-mix(in srgb, var(--ok) 12%, transparent);
  }

  .dline.removed {
    color: var(--bad);
    background: color-mix(in srgb, var(--bad) 12%, transparent);
  }

  .skipped {
    padding: 2px 10px;
    color: var(--muted);
    font-family: var(--font-ui);
    background: var(--panel);
  }
</style>
