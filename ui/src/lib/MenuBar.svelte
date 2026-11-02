<script lang="ts">
  import { tick } from "svelte";
  import { isEnabled, type Item, type Menu } from "./menus";
  import { shortcutLabel } from "./shortcuts";

  let { menus, onclose }: { menus: Menu[]; onclose: () => void } = $props();

  let bar: HTMLDivElement;
  /** The open menu, or null when all are closed. */
  let open: number | null = $state(null);
  /** The highlighted item in the open menu. */
  let active: number | null = $state(null);

  /** Opens a menu and gives it the keyboard. The app calls this for Alt+F and the like. */
  export async function openMenu(index: number) {
    open = index;
    active = null;
    await tick();
    bar.focus();
  }

  function close() {
    open = null;
    active = null;
    onclose();
  }

  function run(item: Item) {
    if (!isEnabled(item)) {
      return;
    }
    close();
    item.run?.();
  }

  /** Moves the highlight to the next item that can be used, round past the ends. */
  function move(step: 1 | -1) {
    if (open === null) {
      return;
    }
    const items = menus[open].items;
    let index = active ?? (step === 1 ? -1 : items.length);
    for (let tries = 0; tries < items.length; tries++) {
      index = (index + step + items.length) % items.length;
      const item = items[index];
      if (item && typeof item !== "string" && isEnabled(item)) {
        active = index;
        return;
      }
    }
  }

  function onkeydown(event: KeyboardEvent) {
    if (open === null) {
      return;
    }
    switch (event.key) {
      case "ArrowDown":
        move(1);
        break;
      case "ArrowUp":
        move(-1);
        break;
      case "ArrowRight":
        void openMenu((open + 1) % menus.length);
        break;
      case "ArrowLeft":
        void openMenu((open - 1 + menus.length) % menus.length);
        break;
      case "Enter":
      case " ": {
        const item = active === null ? null : menus[open].items[active];
        if (item && typeof item !== "string") {
          run(item);
        }
        break;
      }
      case "Escape":
        close();
        break;
      default:
        return;
    }
    // The key was for the menu: the app's own shortcuts must not see it too.
    event.preventDefault();
    event.stopPropagation();
  }

  // A click anywhere outside the menus closes them.
  function onWindowMouseDown(event: MouseEvent) {
    if (open !== null && !bar.contains(event.target as Node)) {
      close();
    }
  }

  function onTitleMouseDown(event: MouseEvent, index: number) {
    // Keep the focus where it is until the menu takes it.
    event.preventDefault();
    if (open === index) {
      close();
    } else {
      void openMenu(index);
    }
  }
</script>

<svelte:window onmousedown={onWindowMouseDown} onblur={() => open !== null && close()} />

<div class="menubar" role="menubar" tabindex="-1" bind:this={bar} {onkeydown}>
  {#each menus as menu, i (menu.title)}
    <div class="menu">
      <button
        class="title"
        class:open={open === i}
        role="menuitem"
        aria-haspopup="menu"
        aria-expanded={open === i}
        tabindex="-1"
        onmousedown={(event) => onTitleMouseDown(event, i)}
        onmouseenter={() => open !== null && open !== i && openMenu(i)}
      >
        {menu.title}
      </button>

      {#if open === i}
        <div class="dropdown" role="menu">
          {#each menu.items as item, j (j)}
            {#if item === null}
              <div class="separator" role="separator"></div>
            {:else if typeof item === "string"}
              <div class="heading" role="presentation">{item}</div>
            {:else}
              {@const enabled = isEnabled(item)}
              <button
                class="item"
                class:active={active === j && enabled}
                class:disabled={!enabled}
                role={item.checked ? "menuitemradio" : "menuitem"}
                aria-checked={item.checked ? item.checked() : undefined}
                aria-disabled={!enabled}
                tabindex="-1"
                onmouseenter={() => (active = j)}
                onclick={() => run(item)}
              >
                <span class="check">{item.checked?.() ? "✓" : ""}</span>
                <span class="label">{item.label}</span>
                <span class="hint">
                  {#if item.shortcut}
                    {shortcutLabel(item.shortcut)}
                  {:else if item.hint}
                    {item.hint}
                  {/if}
                </span>
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .menubar {
    display: flex;
    align-items: stretch;
    height: 30px;
    padding: 0 4px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
    outline: none;
  }

  .menu {
    position: relative;
    display: flex;
  }

  button {
    font: inherit;
    color: var(--text);
    background: none;
    border: 0;
    cursor: default;
  }

  .title {
    margin: 3px 0;
    padding: 0 10px;
    border-radius: 4px;
  }

  .title:hover,
  .title.open {
    background: var(--hover);
  }

  .dropdown {
    position: absolute;
    top: calc(100% + 2px);
    left: 0;
    z-index: 10;
    /* As wide as its longest item, each on one line. */
    width: max-content;
    min-width: 270px;
    padding: 4px;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: var(--shadow);
  }

  .item {
    display: grid;
    grid-template-columns: 20px 1fr auto;
    align-items: center;
    width: 100%;
    height: 28px;
    padding: 0 10px 0 4px;
    text-align: left;
    border-radius: 4px;
  }

  .item.active {
    background: var(--accent-soft);
  }

  .item.disabled {
    color: var(--muted);
    opacity: 0.6;
  }

  .check {
    color: var(--accent);
    text-align: center;
  }

  .label {
    white-space: nowrap;
  }

  .hint {
    padding-left: 32px;
    font-size: 12px;
    color: var(--muted);
  }

  .separator {
    height: 1px;
    margin: 4px 8px;
    background: var(--border);
  }

  .heading {
    padding: 6px 10px 2px 24px;
    font-size: 12px;
    color: var(--muted);
  }
</style>
