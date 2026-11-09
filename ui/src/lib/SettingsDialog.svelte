<script lang="ts">
  import Modal from "./Modal.svelte";
  import {
    FONT_SIZE_MAX,
    FONT_SIZE_MIN,
    MENU_FONT_SIZE_MAX,
    MENU_FONT_SIZE_MIN,
    settings,
    updateSettings,
    type ThemeChoice,
    type UpdateSettings,
  } from "./settings.svelte";

  let { onclose }: { onclose: () => void } = $props();

  // A copy of the settings: nothing changes until Save.
  let theme: ThemeChoice = $state(settings.theme);
  let menuFontSize = $state(settings.menu_font_size);
  let fontSize = $state(settings.font_size);
  let updates: UpdateSettings = $state({ ...settings.updates });
  let newer = $state(settings.newer_libraries);
  let error = $state("");

  function submit() {
    if (!Number.isInteger(menuFontSize) || menuFontSize < MENU_FONT_SIZE_MIN || menuFontSize > MENU_FONT_SIZE_MAX) {
      error = `The menu text size must be a whole number from ${MENU_FONT_SIZE_MIN} to ${MENU_FONT_SIZE_MAX}.`;
      return;
    }
    if (!Number.isInteger(fontSize) || fontSize < FONT_SIZE_MIN || fontSize > FONT_SIZE_MAX) {
      error = `The text size must be a whole number from ${FONT_SIZE_MIN} to ${FONT_SIZE_MAX}.`;
      return;
    }
    updateSettings({
      theme,
      menu_font_size: menuFontSize,
      font_size: fontSize,
      updates: { ...updates },
      newer_libraries: newer,
    });
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
    <label class="dlg-label">
      Menu text size
      <input
        class="field mono"
        type="number"
        min={MENU_FONT_SIZE_MIN}
        max={MENU_FONT_SIZE_MAX}
        bind:value={menuFontSize}
      />
    </label>
    <label class="dlg-label">
      Text size
      <input class="field mono" type="number" min={FONT_SIZE_MIN} max={FONT_SIZE_MAX} bind:value={fontSize} />
    </label>
  </div>
  <p class="dlg-hint">
    The menu text size is for the menus, tabs, buttons and dialogs, which grow with it. The text size is for the
    libnima.yml editor and the run's report.
  </p>

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

  {#if error}
    <p class="dlg-error">{error}</p>
  {/if}

  {#snippet buttons()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={submit}>Save</button>
  {/snippet}
</Modal>
