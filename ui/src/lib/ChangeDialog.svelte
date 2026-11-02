<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import Modal from "./Modal.svelte";
  import { PLATFORMS, type Choices, type Platform, type Project } from "./project.svelte";

  let {
    project,
    choices,
    onchange,
    onclose,
  }: {
    project: Project;
    choices: Choices;
    /** Takes what was chosen, and checks the list again. */
    onchange: (choices: Choices) => void;
    onclose: () => void;
  } = $props();

  // A copy: nothing changes until OK.
  // svelte-ignore state_referenced_locally
  let platform: Platform | "" = $state(choices.platform ?? "");
  // svelte-ignore state_referenced_locally
  let ide = $state(choices.ide ?? "");
  // svelte-ignore state_referenced_locally
  let conf = $state(choices.conf ?? "");
  // svelte-ignore state_referenced_locally
  let into = $state(choices.into ?? "");
  // svelte-ignore state_referenced_locally
  let sketchbook = $state(choices.sketchbook ?? "");

  const shown = $derived(platform || project.platform);
  const ides = ["cmake", "cubeide", "keil", "iar", "makefile"];

  async function browse() {
    const folder = await open({ title: "The sketchbook folder", directory: true, multiple: false });
    if (typeof folder === "string") {
      sketchbook = folder;
    }
  }

  function submit() {
    onchange({
      platform: platform || null,
      ide: shown === "stm32" && ide ? ide : null,
      conf: shown === "zephyr" && conf.trim() ? conf.trim() : null,
      into: shown === "arduino" && into ? into : null,
      sketchbook: shown === "arduino" && sketchbook.trim() ? sketchbook.trim() : null,
    });
    onclose();
  }
</script>

<Modal title="This project" {onclose} onsubmit={submit}>
  <h2 class="dlg-section">What it looks like</h2>
  {#each project.candidates as candidate (candidate.platform)}
    <p>
      {PLATFORMS.find((one) => one.name === candidate.platform)?.label}, from
      <span class="mono">{candidate.evidence}</span>
    </p>
  {/each}
  {#if project.candidates.length === 0}
    <p class="dlg-muted">Nothing here says which platform it is. Each library names the ones it works on.</p>
  {/if}

  <h2 class="dlg-section">Choose</h2>
  <div class="dlg-grid">
    <label class="dlg-label">
      Platform
      <select class="field" bind:value={platform}>
        <option value="">As found</option>
        {#each PLATFORMS as one (one.name)}
          <option value={one.name}>{one.label}</option>
        {/each}
      </select>
    </label>

    {#if shown === "stm32"}
      <label class="dlg-label">
        Register with
        <select class="field" bind:value={ide}>
          <option value="">Every IDE found</option>
          {#each ides as one (one)}
            <option value={one}>{one}</option>
          {/each}
        </select>
      </label>
    {/if}

    {#if shown === "zephyr"}
      <label class="dlg-label">
        Kconfig file
        <input class="field mono" list="conf-files" placeholder="Asked when there are several" bind:value={conf} />
        <datalist id="conf-files">
          {#each project.conf_files as one (one)}
            <option value={one}></option>
          {/each}
        </datalist>
      </label>
    {/if}

    {#if shown === "arduino"}
      <label class="dlg-label">
        Install into
        <select class="field" bind:value={into}>
          <option value="">Ask, in a sketch</option>
          <option value="libraries">The libraries folder, for every sketch</option>
          <option value="sketch" disabled={!project.sketch}>This sketch's src folder</option>
        </select>
      </label>
    {/if}
  </div>

  {#if shown === "arduino"}
    <label class="dlg-label book">
      Sketchbook
      <span class="file">
        <input class="field mono" placeholder="Found the way Arduino finds it" bind:value={sketchbook} />
        <button class="btn" onclick={browse}>Browse...</button>
      </span>
    </label>
  {/if}

  {#snippet buttons()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={submit}>OK</button>
  {/snippet}
</Modal>

<style>
  p {
    margin: 0 0 6px;
    line-height: 1.5;
  }

  .book {
    margin-top: 12px;
  }

  .file {
    display: flex;
    gap: 8px;
  }

  .file .field {
    flex: 1;
    min-width: 0;
  }
</style>
