<script lang="ts">
  import { onMount, type Snippet } from "svelte";

  let {
    title,
    onclose,
    onsubmit,
    children,
    buttons,
    wide = false,
  }: {
    title: string;
    onclose: () => void;
    /** Called for Enter, outside a list or a button. */
    onsubmit: () => void;
    children: Snippet;
    buttons: Snippet;
    /** For what has long lines to show. */
    wide?: boolean;
  } = $props();

  let dialog: HTMLDivElement;

  onMount(() => dialog.focus());

  function onkeydown(event: KeyboardEvent) {
    const target = event.target;
    if (event.key === "Escape") {
      event.preventDefault();
      onclose();
    } else if (
      event.key === "Enter" &&
      !(target instanceof HTMLSelectElement || target instanceof HTMLButtonElement)
    ) {
      event.preventDefault();
      onsubmit();
    }
  }
</script>

<!-- A click on the dark area around the dialog closes it, as Escape does. -->
<div class="backdrop" role="presentation" onmousedown={onclose}>
  <div
    class="dialog"
    class:wide
    role="dialog"
    aria-modal="true"
    aria-label={title}
    tabindex="-1"
    bind:this={dialog}
    {onkeydown}
    onmousedown={(event) => event.stopPropagation()}
  >
    <h1>{title}</h1>
    {@render children()}
    <div class="buttons">
      {@render buttons()}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    place-items: center;
    background: var(--backdrop);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: min(640px, calc(100vw - 32px));
    max-height: calc(100vh - 48px);
    padding: 20px 22px 18px;
    overflow-y: auto;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: var(--shadow);
    outline: none;
  }

  .dialog.wide {
    width: min(1040px, calc(100vw - 32px));
  }

  h1 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
  }

  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }

  .buttons :global(.btn) {
    min-width: 88px;
    justify-content: center;
  }
</style>
