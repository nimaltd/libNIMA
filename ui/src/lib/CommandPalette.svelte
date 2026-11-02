<script lang="ts">
  import { onMount } from "svelte";
  import { key, search, type Command } from "./palette";
  import { shortcutLabel } from "./shortcuts";

  let { commands, onclose }: { commands: Command[]; onclose: () => void } = $props();

  let input: HTMLInputElement;
  let list: HTMLDivElement;
  let query = $state("");
  let selected = $state(0);

  const found = $derived(search(query, commands, recent));

  onMount(() => input.focus());

  function run(command: Command) {
    remember(command);
    onclose();
    command.run();
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      selected = (selected + step + found.length) % Math.max(1, found.length);
      list.querySelector(`[data-index="${selected}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter") {
      event.preventDefault();
      const chosen = found[selected];
      if (chosen) run(chosen.command);
    } else if (event.key === "Escape") {
      event.preventDefault();
      onclose();
    }
  }

  /** The label in pieces, with the letters the query matched marked. */
  function pieces(label: string, positions: number[]): { text: string; hit: boolean }[] {
    const hits = new Set(positions);
    const parts: { text: string; hit: boolean }[] = [];
    for (let at = 0; at < label.length; at++) {
      const hit = hits.has(at);
      const last = parts[parts.length - 1];
      if (last && last.hit === hit) last.text += label[at];
      else parts.push({ text: label[at], hit });
    }
    return parts;
  }
</script>

<script module lang="ts">
  /** The commands run lately, newest first, so an empty palette offers them first. */
  const recent: string[] = [];

  function remember(command: Command) {
    const at = recent.indexOf(key(command));
    if (at >= 0) recent.splice(at, 1);
    recent.unshift(key(command));
    recent.length = Math.min(recent.length, 20);
  }
</script>

<!-- A click outside the palette closes it, as Escape does. -->
<div class="backdrop" role="presentation" onmousedown={onclose}>
  <div
    class="palette"
    role="dialog"
    aria-label="Command palette"
    tabindex="-1"
    onmousedown={(event) => event.stopPropagation()}
  >
    <input
      class="field"
      placeholder="Type a command"
      spellcheck="false"
      bind:this={input}
      bind:value={query}
      oninput={() => (selected = 0)}
      {onkeydown}
    />
    <div class="list" role="listbox" bind:this={list}>
      {#each found as { command, positions }, index (index)}
        <button
          class="item"
          class:selected={index === selected}
          role="option"
          aria-selected={index === selected}
          data-index={index}
          onmousemove={() => (selected = index)}
          onclick={() => run(command)}
        >
          <span class="check">{command.checked ? "✓" : ""}</span>
          <span class="label">
            {#each pieces(command.label, positions) as piece, at (at)}
              {#if piece.hit}<strong>{piece.text}</strong>{:else}{piece.text}{/if}
            {/each}
          </span>
          <span class="group">{command.group}</span>
          <span class="shortcut">{command.shortcut ? shortcutLabel(command.shortcut) : ""}</span>
        </button>
      {:else}
        <p class="empty">No command matches "{query}".</p>
      {/each}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: 72px;
    background: var(--backdrop);
  }

  .palette {
    display: flex;
    flex-direction: column;
    width: min(600px, calc(100vw - 32px));
    max-height: min(480px, calc(100vh - 120px));
    padding: 8px;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
    outline: none;
  }

  .field {
    height: 34px;
    font-size: 14px;
  }

  .list {
    flex: 1;
    min-height: 0;
    margin-top: 6px;
    overflow-y: auto;
  }

  .item {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto 110px;
    gap: 10px;
    align-items: center;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    font: inherit;
    color: var(--text);
    text-align: left;
    background: none;
    border: 0;
    border-radius: 6px;
    cursor: default;
  }

  .item.selected {
    background: var(--accent-soft);
  }

  .check {
    color: var(--accent);
  }

  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .label strong {
    color: var(--accent);
    font-weight: 700;
  }

  .group,
  .shortcut {
    font-size: 12px;
    color: var(--muted);
    white-space: nowrap;
  }

  .shortcut {
    text-align: right;
  }

  .empty {
    padding: 10px;
    color: var(--muted);
  }
</style>
