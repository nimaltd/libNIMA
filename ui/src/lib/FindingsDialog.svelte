<script lang="ts">
  import Modal from "./Modal.svelte";
  import type { Item } from "./project.svelte";

  let { item, onclose }: { item: Item; onclose: () => void } = $props();

  const looked = $derived(item.looked);
  const words = { ok: "Fine", warn: "To do", info: "Worth knowing" };
  /** The engine says the same to the command as to this window. */
  const namesAnOption = $derived(/--(platform|ide|conf|overlay|into|sketchbook)\b/.test(looked?.why ?? ""));
</script>

<Modal title={looked?.name || item.given} {onclose} onsubmit={onclose}>
  {#if looked?.description}
    <p class="dlg-muted">{looked.description}</p>
  {/if}

  {#if looked?.mark === "no"}
    <h2 class="dlg-section">Why it cannot go in</h2>
    <p class="said">{looked.why}</p>
    {#if namesAnOption}
      <p class="dlg-hint">Here, that option is under Change, beside Platform.</p>
    {/if}
  {:else if looked?.findings.length}
    <h2 class="dlg-section">What the check found</h2>
    <p class="dlg-hint">It installs either way: a project can be laid out in ways this cannot read.</p>
    <div class="found">
      {#each looked.findings as finding, index (index)}
        <div class="row">
          <span class="level {finding.level}">{words[finding.level]}</span>
          <div>
            <p class="said">{finding.message}</p>
            {#if finding.hint}
              <p class="said hint">{finding.hint}</p>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <p>The check found nothing to do.</p>
  {/if}

  {#snippet buttons()}
    <button class="btn primary" onclick={onclose}>Close</button>
  {/snippet}
</Modal>

<style>
  p {
    margin: 0 0 8px;
    line-height: 1.5;
  }

  .said {
    white-space: pre-wrap;
    user-select: text;
  }

  .hint {
    color: var(--muted);
  }

  .found {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 10px;
  }

  .row {
    display: grid;
    grid-template-columns: 96px 1fr;
    gap: 8px;
  }

  .level {
    font-size: 12px;
    font-weight: 600;
    line-height: 1.7;
  }

  .level.ok {
    color: var(--ok);
  }

  .level.warn {
    color: var(--warn);
  }

  .level.info {
    color: var(--muted);
  }
</style>
