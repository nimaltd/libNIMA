<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import icon from "../../../src-tauri/icons/icon.svg";

  let { version, onclose }: { version: string; onclose: () => void } = $props();

  let dialog: HTMLDivElement;

  onMount(() => dialog.focus());

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onclose();
    }
  }
</script>

<svelte:window {onkeydown} />

<!-- A click on the dark area around the box closes it, as Escape does. -->
<div class="backdrop" role="presentation" onmousedown={onclose}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="about-title"
    tabindex="-1"
    bind:this={dialog}
    onmousedown={(event) => event.stopPropagation()}
  >
    <img src={icon} alt="" width="80" height="80" />
    <h1 id="about-title">lib<span class="nima">NIMA</span></h1>
    <p class="version">Version {version}</p>
    <p>Installs embedded C libraries into STM32, Zephyr, ESP-IDF, Arduino and PlatformIO projects.</p>
    <p class="muted">
      Copyright 2026 Nima Askari (NimaLTD)<br />
      Apache License 2.0
    </p>
    <div class="buttons">
      <button onclick={() => openUrl("https://github.com/nimaltd/libNIMA")}>GitHub</button>
      <button class="primary" onclick={onclose}>Close</button>
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
    width: 340px;
    padding: 24px 28px 20px;
    text-align: center;
    background: var(--menu-bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    box-shadow: var(--shadow);
    outline: none;
  }

  h1 {
    margin: 8px 0 2px;
    font-size: 20px;
    font-weight: 600;
  }

  /* The same orange as NIMA in the welcome logo. */
  .nima {
    color: #fb923c;
  }

  :global([data-theme="light"]) .nima {
    color: #ea580c;
  }

  p {
    margin: 10px 0;
    line-height: 1.5;
  }

  .version {
    margin-top: 0;
    color: var(--accent);
  }

  .muted {
    font-size: 12px;
    color: var(--muted);
  }

  .buttons {
    display: flex;
    justify-content: center;
    gap: 8px;
    margin-top: 18px;
  }

  button {
    min-width: 88px;
    height: 30px;
    padding: 0 14px;
    font: inherit;
    color: var(--text);
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  button:hover {
    background: var(--hover);
  }

  button.primary {
    color: var(--on-accent);
    background: var(--accent);
    border-color: var(--accent);
  }
</style>
