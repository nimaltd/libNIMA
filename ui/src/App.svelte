<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import About from "./lib/About.svelte";
  import ChangeDialog from "./lib/ChangeDialog.svelte";
  import CommandPalette from "./lib/CommandPalette.svelte";
  import ConfirmDialog from "./lib/ConfirmDialog.svelte";
  import EditorTab from "./lib/EditorTab.svelte";
  import FindingsDialog from "./lib/FindingsDialog.svelte";
  import MenuBar from "./lib/MenuBar.svelte";
  import PreviewDialog from "./lib/PreviewDialog.svelte";
  import ProjectTab from "./lib/ProjectTab.svelte";
  import QuestionDialog from "./lib/QuestionDialog.svelte";
  import SettingsDialog from "./lib/SettingsDialog.svelte";
  import TokenDialog from "./lib/TokenDialog.svelte";
  import UpdateDialog from "./lib/UpdateDialog.svelte";
  import { asking, confirmFirst } from "./lib/confirm.svelte";
  import { editor, showFileOf, tryOnProject } from "./lib/editor.svelte";
  import { buildMenus, isEnabled, itemForKey, menuCommands, menuForAltKey } from "./lib/menus";
  import type { Command } from "./lib/palette";
  import {
    addLibrary,
    answer,
    check,
    checkAll,
    installAll,
    listenToRuns,
    makePreview,
    openProject,
    reopen,
    saidJustNow,
    view,
    type Item,
    type Preview,
  } from "./lib/project.svelte";
  import { loadSettings, settings, unzoomed, updateSettings } from "./lib/settings.svelte";
  import { appScheme } from "./lib/theme.svelte";
  import { checkForUpdate, type UpdateCheck } from "./lib/update";

  type AppInfo = { version: string; os: string; arch: string };
  type PathState = { found: boolean; folder: string };

  let info = $state<AppInfo>();
  let tab: "project" | "editor" = $state("project");
  let aboutOpen = $state(false);
  let settingsOpen = $state(false);
  let changeOpen = $state(false);
  let palette: Command[] | null = $state(null);
  /** What looking for a newer version found, while it is shown. */
  let updateCheck: UpdateCheck | null = $state(null);
  /** The library whose findings are shown. */
  let fixing: Item | null = $state(null);
  /** The library a token is being asked for. */
  let tokenFor: Item | null = $state(null);
  let preview: Preview | null = $state(null);
  /** What adding the command to the PATH said. */
  let pathSaid: { title: string; text: string } | null = $state(null);

  let menuBar: MenuBar;
  let projectTab: ProjectTab;
  let editorTab: EditorTab;

  const dialogOpen = $derived(
    aboutOpen ||
      settingsOpen ||
      changeOpen ||
      palette !== null ||
      updateCheck !== null ||
      fixing !== null ||
      tokenFor !== null ||
      preview !== null ||
      asking.now !== null ||
      pathSaid !== null ||
      view.question !== null,
  );

  const menus = buildMenus({
    about: () => (aboutOpen = true),
    exit: () => void getCurrentWindow().close(),
    openProject: () => {
      tab = "project";
      void projectTab.pickProject();
    },
    addZip: () => {
      tab = "project";
      void projectTab.pickZip();
    },
    addFolder: () => {
      tab = "project";
      void projectTab.pickFolder();
    },
    showTab: (shown) => (tab = shown),
    inEditor: () => tab === "editor",
    undo: () => editorTab.runUndo(),
    redo: () => editorTab.runRedo(),
    find: () => editorTab.find(),
    preview: () => void showPreview(),
    changeProject: () => (changeOpen = true),
    addToPath: () => void addToPath(),
    settings: () => (settingsOpen = true),
    commandPalette: () => (palette = menuCommands(menus)),
    checkUpdates: () => void lookForUpdate(true),
  });

  /**
   * Looks on GitHub for a newer libNIMA. Asked from the Help menu, it says what it found; when the window opens, it speaks
   * only of a newer version that was not skipped, and says nothing when GitHub cannot be reached.
   */
  async function lookForUpdate(asked: boolean) {
    if (!info) {
      return;
    }
    const found = await checkForUpdate(info.version);
    if (asked || (found.kind === "newer" && found.release.version !== settings.updates.skipped)) {
      updateCheck = found;
    }
  }

  async function showPreview() {
    preview = await makePreview();
  }

  async function tryIt() {
    if (await tryOnProject()) {
      tab = "project";
    }
  }

  async function addToPath() {
    const state = await invoke<PathState>("path_state");
    if (state.found) {
      pathSaid = { title: "libnima is on the PATH", text: "A terminal finds libnima already. Run it from the root of a project." };
      return;
    }
    if (!state.folder) {
      pathSaid = {
        title: "The command is not here",
        text: "This install has the window alone. The command comes with the installer, or as a zip of its own, from the releases page.",
      };
      return;
    }
    confirmFirst(
      "Add libnima to the PATH?",
      `So a terminal finds it from the root of any project. It adds ${state.folder} to your own PATH, and needs no administrator.`,
      "Add it",
      () => {
        invoke<string>("add_to_path")
          .then((said) => (pathSaid = { title: "Added to the PATH", text: said }))
          .catch((error) => (pathSaid = { title: "It could not be added", text: String(error) }));
      },
    );
  }

  onMount(() => {
    listenToRuns();
    const appInfo = invoke<AppInfo>("app_info").then((result) => (info = result));
    loadSettings()
      .catch((error) => console.error(error))
      .then(() => invoke<string | null>("start_folder"))
      .then((folder) => (folder ? openProject(folder) : undefined))
      .then(() => appInfo)
      .then(() => (settings.updates.check ? lookForUpdate(false) : undefined))
      .catch((error) => console.error(error));

    // A zip or a folder dropped on the window: a library on the list, or with no project open, the project.
    const dropped = getCurrentWebview().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") {
        return;
      }
      for (const path of event.payload.paths) {
        if (view.project) {
          tab = "project";
          addLibrary(path);
        } else if (!path.toLowerCase().endsWith(".zip")) {
          void openProject(path);
        }
      }
    });

    // Asked before a run is cut off, or an edit not saved is lost.
    const closing = getCurrentWindow().onCloseRequested((event) => {
      const lost = view.running
        ? `${view.running} is still under way, and closing cuts it off.`
        : editor.dirty && !editor.readOnly
          ? `${editor.title} has changes that are not saved.`
          : "";
      if (lost) {
        event.preventDefault();
        confirmFirst("Close libNIMA?", lost, "Close", () => void getCurrentWindow().destroy());
      }
    });

    return () => {
      void dropped.then((stop) => stop());
      void closing.then((stop) => stop());
    };
  });

  // The page and the window frame follow the theme.
  $effect(() => {
    document.documentElement.dataset.theme = appScheme();
  });
  $effect(() => {
    const theme = settings.theme;
    void getCurrentWindow().setTheme(theme === "system" ? null : theme);
  });

  function onkeydown(event: KeyboardEvent) {
    if (dialogOpen) {
      return;
    }
    const item = itemForKey(menus, event);
    if (item) {
      event.preventDefault();
      if (isEnabled(item)) {
        item.run?.();
      }
      return;
    }
    const menu = menuForAltKey(menus, event);
    if (menu !== null) {
      event.preventDefault();
      void menuBar.openMenu(menu);
    }
  }

  /** The web view's own menu only where text is typed. */
  function oncontextmenu(event: MouseEvent) {
    const target = event.target instanceof Element ? event.target : null;
    if (!target?.closest("input, textarea, [contenteditable]")) {
      event.preventDefault();
    }
  }
</script>

<svelte:window {onkeydown} {oncontextmenu} />

<div class="app">
  <MenuBar bind:this={menuBar} {menus} onclose={() => {}} />

  <div class="tabs" role="tablist">
    <button class="tab" class:active={tab === "project"} role="tab" aria-selected={tab === "project"} onclick={() => (tab = "project")}>
      <span class="dot" class:open={view.project !== null}></span>
      {view.project ? "Project" : "Welcome"}
    </button>
    <button class="tab" class:active={tab === "editor"} role="tab" aria-selected={tab === "editor"} onclick={() => (tab = "editor")}>
      <span class="dot" class:open={editor.title !== "" && !editor.dirty} class:unsaved={editor.dirty}></span>
      libnima.yml{editor.dirty ? " *" : ""}
    </button>
  </div>

  <main class="pane" style:--view-size={`${unzoomed(settings.font_size)}px`}>
    <div class="page" class:hidden={tab !== "project"}>
      <ProjectTab
        bind:this={projectTab}
        version={info?.version ?? ""}
        onchange={() => (changeOpen = true)}
        onfix={(item) => (fixing = item)}
        onyml={(item) => {
          if (item.looked) {
            tab = "editor";
            void showFileOf(item.looked);
          }
        }}
        onpreview={() => void showPreview()}
        ontoken={(item) => (tokenFor = item)}
      />
    </div>
    <!-- The editor stays while the other tab is shown, with its text, its place and its undo. -->
    <div class="page" class:hidden={tab !== "editor"}>
      <EditorTab bind:this={editorTab} active={tab === "editor"} ontry={() => void tryIt()} />
    </div>
  </main>

  <footer class="status">
    <span class:busy={view.running !== "" || view.previewing}>
      {view.running || (view.previewing ? "Trying the install on a copy..." : "Ready")}
    </span>
    <span class="spacer"></span>
    {#if info}
      <span>libNIMA v{info.version}</span>
      <span class="divider"></span>
      <span>{info.os} {info.arch}</span>
    {/if}
  </footer>
</div>

{#if view.question}
  <!-- Anew for each question, so nothing typed for one is there for the next. -->
  {#key view.question}
    <QuestionDialog question={view.question} context={saidJustNow()} onanswer={answer} />
  {/key}
{/if}

{#if fixing}
  <FindingsDialog item={fixing} onclose={() => (fixing = null)} />
{/if}

{#if tokenFor}
  {@const item = tokenFor}
  <TokenDialog
    why={item.looked?.why ?? ""}
    onkey={(key) => {
      view.key = key;
      void check(item.id);
    }}
    onclose={() => (tokenFor = null)}
  />
{/if}

{#if preview}
  <PreviewDialog {preview} oninstall={installAll} onclose={() => (preview = null)} />
{/if}

{#if changeOpen && view.project}
  <ChangeDialog
    project={view.project}
    choices={view.choices}
    onchange={(choices) => {
      view.choices = choices;
      void reopen();
      checkAll();
    }}
    onclose={() => (changeOpen = false)}
  />
{/if}

{#if settingsOpen}
  <SettingsDialog onclose={() => (settingsOpen = false)} />
{/if}

{#if palette}
  <CommandPalette commands={palette} onclose={() => (palette = null)} />
{/if}

{#if asking.now}
  <ConfirmDialog
    title={asking.now.title}
    text={asking.now.text}
    yes={asking.now.yes}
    onyes={asking.now.then}
    onclose={() => (asking.now = null)}
  />
{/if}

{#if pathSaid}
  <ConfirmDialog title={pathSaid.title} text={pathSaid.text} yes="OK" onyes={() => {}} onclose={() => (pathSaid = null)} />
{/if}

{#if updateCheck && info}
  <UpdateDialog
    check={updateCheck}
    current={info.version}
    onskip={(version) => {
      updateSettings({ updates: { ...settings.updates, skipped: version } });
      updateCheck = null;
    }}
    onclose={() => (updateCheck = null)}
  />
{/if}

{#if aboutOpen && info}
  <About version={info.version} onclose={() => (aboutOpen = false)} />
{/if}

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
  }

  .tabs {
    display: flex;
    flex-shrink: 0;
    align-items: stretch;
    height: 36px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    font: inherit;
    color: var(--muted);
    background: none;
    border: 0;
    border-right: 1px solid var(--border);
    cursor: default;
  }

  .tab:hover {
    color: var(--text);
  }

  /* The active tab covers the bar's bottom border, so it joins what is under it. */
  .tab.active {
    margin-bottom: -1px;
    color: var(--text);
    background: var(--bg);
    box-shadow: inset 0 2px 0 var(--accent);
  }

  .dot {
    flex-shrink: 0;
    width: 8px;
    height: 8px;
    border: 1.5px solid var(--muted);
    border-radius: 50%;
    box-sizing: border-box;
  }

  .dot.open {
    background: var(--ok);
    border-color: var(--ok);
  }

  .dot.unsaved {
    background: var(--warn);
    border-color: var(--warn);
  }

  .pane {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--bg);
  }

  .page {
    position: absolute;
    inset: 0;
  }

  .page.hidden {
    display: none;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 24px;
    padding: 0 12px;
    font-size: 12px;
    color: var(--muted);
    background: var(--panel);
    border-top: 1px solid var(--border);
  }

  .busy {
    color: var(--accent);
  }

  .spacer {
    flex: 1;
  }

  .divider {
    width: 1px;
    height: 12px;
    background: var(--border);
  }
</style>
