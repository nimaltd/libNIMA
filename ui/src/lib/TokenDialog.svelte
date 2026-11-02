<script lang="ts">
  import { onMount } from "svelte";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Modal from "./Modal.svelte";

  let {
    why,
    onkey,
    onclose,
  }: {
    /** What GitHub said. */
    why: string;
    /** Takes the token, for this window only. */
    onkey: (key: string) => void;
    onclose: () => void;
  } = $props();

  let field: HTMLInputElement;
  let key = $state("");

  onMount(() => field.focus());

  function submit() {
    if (key.trim()) {
      onkey(key.trim());
      onclose();
    }
  }
</script>

<Modal title="A GitHub token" {onclose} onsubmit={submit}>
  <p class="said">{why}</p>
  <label class="dlg-label">
    Token
    <input class="field mono" type="password" autocomplete="off" spellcheck="false" bind:this={field} bind:value={key} />
  </label>
  <p class="dlg-hint">
    It stays in this window until it closes. It is never saved, and is sent to GitHub and nowhere else. GITHUB_TOKEN works as
    well, set before libNIMA starts. It needs Contents, read-only, on the repository:
    <button class="link" onclick={() => openUrl("https://github.com/settings/personal-access-tokens")}>make one</button>.
  </p>

  {#snippet buttons()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" disabled={!key.trim()} onclick={submit}>Use it</button>
  {/snippet}
</Modal>

<style>
  .said {
    margin: 4px 0 12px;
    line-height: 1.5;
    white-space: pre-wrap;
    user-select: text;
  }

  .dlg-hint {
    margin-top: 8px;
    line-height: 1.5;
  }

  .link {
    padding: 0;
    font: inherit;
    color: var(--accent);
    background: none;
    border: 0;
    cursor: pointer;
  }
</style>
