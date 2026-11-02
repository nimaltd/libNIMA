<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Modal from "./Modal.svelte";
  import type { UpdateCheck } from "./update";

  let {
    check,
    current,
    onskip,
    onclose,
  }: {
    /** What looking for a newer version found. */
    check: UpdateCheck;
    /** The version running. */
    current: string;
    /** Says not to tell about this version again. */
    onskip: (version: string) => void;
    onclose: () => void;
  } = $props();

  /** Opens the release's page, with the downloads for each OS and what changed. */
  function download() {
    if (check.kind === "newer") {
      void openUrl(check.release.url);
    }
    onclose();
  }
</script>

<Modal title={check.kind === "newer" ? "A new version of libNIMA" : "Updates"} {onclose} onsubmit={download}>
  {#if check.kind === "newer"}
    <p>libNIMA {check.release.version} is out. You have {current}.</p>
    <p class="dlg-hint">Its page on GitHub has the downloads for each OS and what changed.</p>
  {:else if check.kind === "newest"}
    <p>You have the newest version, {current}.</p>
  {:else}
    <p>Could not look for a new version.</p>
    <p class="dlg-error">{check.problem}</p>
  {/if}

  {#snippet buttons()}
    {#if check.kind === "newer"}
      {@const version = check.release.version}
      <button class="btn" onclick={() => onskip(version)}>Skip this version</button>
      <button class="btn" onclick={onclose}>Not now</button>
      <button class="btn primary" onclick={download}>Download</button>
    {:else}
      <button class="btn primary" onclick={onclose}>Close</button>
    {/if}
  {/snippet}
</Modal>

<style>
  p {
    margin: 0 0 8px;
    line-height: 1.5;
  }
</style>
