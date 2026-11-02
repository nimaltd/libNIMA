<script lang="ts">
  import Modal from "./Modal.svelte";
  import { settings, updateSettings, type ThemeChoice, type UpdateSettings } from "./settings.svelte";

  let { onclose }: { onclose: () => void } = $props();

  // A copy of the settings: nothing changes until Save.
  let theme: ThemeChoice = $state(settings.theme);
  let updates: UpdateSettings = $state({ ...settings.updates });
  let newer = $state(settings.newer_libraries);

  function submit() {
    updateSettings({ theme, updates: { ...updates }, newer_libraries: newer });
    onclose();
  }
</script>

<Modal title="Settings" {onclose} onsubmit={submit}>
  <h2 class="dlg-section">Appearance</h2>
  <div class="dlg-grid">
    <label class="dlg-label">
      Theme
      <select class="field" bind:value={theme}>
        <option value="system">Follow the OS</option>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
      </select>
    </label>
  </div>

  <h2 class="dlg-section">Libraries</h2>
  <div class="dlg-checks">
    <label class="dlg-check">
      <input type="checkbox" bind:checked={newer} /> Look on GitHub for newer versions of a project's libraries when it opens
    </label>
  </div>

  <h2 class="dlg-section">Updates</h2>
  <div class="dlg-checks">
    <label class="dlg-check">
      <input type="checkbox" bind:checked={updates.check} /> Look on GitHub for a new version when libNIMA starts
    </label>
  </div>

  {#snippet buttons()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={submit}>Save</button>
  {/snippet}
</Modal>
