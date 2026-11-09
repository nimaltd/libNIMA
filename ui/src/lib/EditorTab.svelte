<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { autocompletion, closeBrackets, startCompletion, type CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
  import { defaultKeymap, history, historyKeymap, indentWithTab, redo, undo } from "@codemirror/commands";
  import { yaml } from "@codemirror/lang-yaml";
  import {
    HighlightStyle,
    bracketMatching,
    foldGutter,
    foldKeymap,
    indentOnInput,
    syntaxHighlighting,
  } from "@codemirror/language";
  import { lintGutter, linter, lintKeymap, type Diagnostic } from "@codemirror/lint";
  import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from "@codemirror/search";
  import { Annotation, Compartment, EditorState } from "@codemirror/state";
  import {
    EditorView,
    drawSelection,
    highlightActiveLine,
    highlightActiveLineGutter,
    hoverTooltip,
    keymap,
    lineNumbers,
  } from "@codemirror/view";
  import { tags } from "@lezer/highlight";
  import { editor, loadTable, newLibrary, pickLibrary, refreshOutline, save, showLine, useFix, type Advice } from "./editor.svelte";
  import OutlinePanel from "./OutlinePanel.svelte";
  import PathText from "./PathText.svelte";
  import { view as project } from "./project.svelte";
  import YmlForm from "./YmlForm.svelte";

  let {
    active,
    ontry,
  }: {
    active: boolean;
    /** Saves the file and checks it against the project. */
    ontry: () => void;
  } = $props();

  let host: HTMLDivElement;
  let view: EditorView;
  const editable = new Compartment();
  /** Marks the change that puts a new file in, which is no edit of the person's. */
  const loading = Annotation.define<boolean>();
  /** The file the text in the editor came from, for a check that finishes after another was opened. */
  let openedAs = 0;

  type Offered = { label: string; detail: string; info: string; apply: string };
  type Help = { path: string; label: string; help: string; example: string; from: number; to: number };

  // The colors come from app.css, so the editor follows the app's theme by itself.
  const look = EditorView.theme({
    "&": { height: "100%", color: "var(--text)", backgroundColor: "var(--bg)" },
    ".cm-content": { fontFamily: "var(--font-mono)", caretColor: "var(--accent)", userSelect: "text" },
    ".cm-scroller": { fontFamily: "var(--font-mono)", fontSize: "var(--view-size)", lineHeight: "1.55" },
    ".cm-gutters": { color: "var(--muted)", backgroundColor: "var(--panel)", border: "none" },
    ".cm-activeLine": { backgroundColor: "var(--hover)" },
    ".cm-activeLineGutter": { backgroundColor: "var(--hover)" },
    "&.cm-focused": { outline: "none" },
    "&.cm-focused .cm-cursor": { borderLeftColor: "var(--accent)" },
    ".cm-selectionBackground, &.cm-focused .cm-selectionBackground": { backgroundColor: "var(--accent-soft)" },
    ".cm-selectionMatch": { backgroundColor: "var(--accent-soft)" },
    ".cm-panels": { color: "var(--text)", backgroundColor: "var(--panel)" },
    ".cm-panels input, .cm-panels button": { color: "var(--text)" },
    ".cm-tooltip": { color: "var(--text)", backgroundColor: "var(--menu-bg)", border: "1px solid var(--border)", borderRadius: "6px" },
    ".cm-tooltip.cm-tooltip-autocomplete > ul": { fontFamily: "var(--font-mono)", fontSize: "12px" },
    ".cm-tooltip.cm-tooltip-autocomplete > ul > li[aria-selected]": { color: "var(--on-accent)", backgroundColor: "var(--accent)" },
    ".cm-completionDetail": { marginLeft: "12px", fontStyle: "normal", opacity: "0.7" },
    ".cm-completionInfo": { maxWidth: "340px", fontFamily: "var(--font-ui)", fontSize: "12px", lineHeight: "1.4" },
    ".key-help": { maxWidth: "400px", padding: "8px 10px", fontFamily: "var(--font-ui)", fontSize: "12px", lineHeight: "1.4" },
    ".key-help b": { display: "block", marginBottom: "2px", fontWeight: "600" },
    ".key-help pre": { margin: "6px 0 0", padding: "4px 6px", fontFamily: "var(--font-mono)", fontSize: "11px", backgroundColor: "var(--bg)", borderRadius: "4px" },
  });

  const colors = HighlightStyle.define([
    { tag: [tags.propertyName, tags.definition(tags.propertyName)], color: "var(--accent)" },
    { tag: [tags.string, tags.special(tags.string)], color: "var(--text)" },
    { tag: tags.comment, color: "var(--muted)", fontStyle: "italic" },
    { tag: [tags.number, tags.bool, tags.null], color: "var(--accent-2)" },
    { tag: [tags.keyword, tags.meta, tags.punctuation], color: "var(--muted)" },
  ]);

  /** What is wrong with the text, by the same code an install reads the file with, and what it comes to. */
  const check = linter(
    async (found) => {
      if (!editor.path || editor.readOnly) {
        editor.problems = [];
        return [];
      }
      const asked = openedAs;
      const text = found.state.doc.toString();
      const platform = project.choices.platform ?? project.project?.platform ?? null;
      const problems = await invoke<Advice[]>("lint_library_file", { path: editor.path, text, platform }).catch(() => []);
      if (asked !== openedAs) {
        return [];
      }
      editor.problems = problems;
      void refreshOutline();
      return problems.map((problem): Diagnostic => {
        const line = found.state.doc.line(Math.min(problem.line ?? 1, found.state.doc.lines));
        return {
          from: line.from,
          to: line.to,
          severity: problem.warning ? "warning" : "error",
          message: problem.message,
          actions: problem.fixes.map((fix) => ({ name: fix.label, apply: () => useFix(fix) })),
        };
      });
    },
    { delay: 250 },
  );

  /** What can go where the cursor is, from the engine's table and the library's folder. */
  async function offer(context: CompletionContext): Promise<CompletionResult | null> {
    if (!editor.path || editor.readOnly) {
      return null;
    }
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    // On an empty line it waits to be asked, so plain typing is left alone.
    if (!context.explicit && before.trim() === "") {
      return null;
    }
    const found = await invoke<{ from: number; items: Offered[] }>("complete_library_file", {
      path: editor.path,
      text: context.state.doc.toString(),
      line: line.number,
      column: context.pos - line.from,
    }).catch(() => null);
    if (!found || found.items.length === 0) {
      return null;
    }
    return {
      from: line.from + found.from,
      options: found.items.map((item) => ({
        label: item.label,
        detail: item.detail,
        info: item.info,
        apply: (editing, _completion, from, to) => {
          editing.dispatch({
            changes: { from, to, insert: item.apply },
            selection: { anchor: from + item.apply.length },
          });
          // What a key can be comes next, where there is a choice.
          if (item.apply.endsWith(" ")) {
            startCompletion(editing);
          }
        },
      })),
    };
  }

  /** What a key is, when the pointer rests on it. */
  const keyHelp = hoverTooltip(async (editing, position) => {
    const line = editing.state.doc.lineAt(position);
    const found = await invoke<Help | null>("help_in_library_file", {
      text: editing.state.doc.toString(),
      line: line.number,
      column: position - line.from,
    }).catch(() => null);
    if (!found) {
      return null;
    }
    return {
      pos: line.from + found.from,
      end: line.from + found.to,
      above: true,
      create() {
        const dom = document.createElement("div");
        dom.className = "key-help";
        const title = document.createElement("b");
        title.textContent = found.label;
        const about = document.createElement("span");
        about.textContent = found.help;
        const example = document.createElement("pre");
        example.textContent = found.example;
        dom.append(title, about, example);
        return { dom };
      },
    };
  });

  /** A new text, changing only what is different, so a change can be undone and the cursor keeps its place. */
  function replaceText(text: string) {
    const old = view.state.doc.toString();
    if (old === text) {
      return;
    }
    let start = 0;
    while (start < old.length && start < text.length && old[start] === text[start]) {
      start++;
    }
    let oldEnd = old.length;
    let newEnd = text.length;
    while (oldEnd > start && newEnd > start && old[oldEnd - 1] === text[newEnd - 1]) {
      oldEnd--;
      newEnd--;
    }
    view.dispatch({ changes: { from: start, to: oldEnd, insert: text.slice(start, newEnd) } });
  }

  onMount(() => {
    void loadTable();
    view = new EditorView({
      parent: host,
      state: EditorState.create({
        doc: editor.text,
        extensions: [
          lineNumbers(),
          highlightActiveLine(),
          highlightActiveLineGutter(),
          foldGutter(),
          lintGutter(),
          history(),
          drawSelection(),
          indentOnInput(),
          bracketMatching(),
          closeBrackets(),
          highlightSelectionMatches(),
          search({ top: true }),
          autocompletion({ override: [offer], icons: false }),
          keyHelp,
          syntaxHighlighting(colors),
          yaml(),
          look,
          check,
          editable.of(EditorState.readOnly.of(true)),
          keymap.of([
            indentWithTab,
            ...defaultKeymap,
            ...historyKeymap,
            ...searchKeymap,
            ...foldKeymap,
            ...lintKeymap,
          ]),
          EditorView.updateListener.of((update) => {
            if (update.docChanged && update.transactions.some((one) => !one.annotation(loading))) {
              editor.dirty = true;
            }
          }),
        ],
      }),
    });
    editor.current = () => view.state.doc.toString();
    editor.replace = replaceText;
    return () => view.destroy();
  });

  // A new file in the editor.
  $effect(() => {
    const opened = editor.opened;
    if (!view || opened === openedAs) {
      return;
    }
    openedAs = opened;
    view.dispatch({
      changes: { from: 0, to: view.state.doc.length, insert: editor.text },
      selection: { anchor: 0 },
      annotations: loading.of(true),
      effects: editable.reconfigure(EditorState.readOnly.of(editor.readOnly || !editor.path)),
      scrollIntoView: true,
    });
    void refreshOutline();
    if (editor.mode === "text") {
      view.focus();
    }
  });

  // A click on a problem goes to its line.
  $effect(() => {
    const { line, ask } = editor.goTo;
    if (view && ask > 0 && line > 0 && line <= view.state.doc.lines) {
      const place = view.state.doc.line(line);
      view.dispatch({ selection: { anchor: place.from }, scrollIntoView: true });
      view.focus();
    }
  });

  // CodeMirror measures itself when it is drawn; a hidden one has no size until it comes in front.
  $effect(() => {
    if (active && editor.mode === "text" && view) {
      view.requestMeasure();
    }
  });

  /** For the Edit menu. */
  export function runUndo() {
    undo(view);
    void refreshOutline();
  }

  export function runRedo() {
    redo(view);
    void refreshOutline();
  }

  export function find() {
    editor.mode = "text";
    openSearchPanel(view);
  }

  const errors = $derived(editor.problems.filter((problem) => !problem.warning).length);
  const hasFile = $derived(editor.title !== "");
  const editing = $derived(editor.path !== "" && !editor.readOnly);
</script>

<div class="tab">
  <div class="bar">
    <span class="title">
      {#if editor.title}
        <PathText path={`${editor.title}${editor.dirty ? " *" : ""}`} />
      {:else}
        <span class="dlg-muted">No file open</span>
      {/if}
    </span>
    {#if editing}
      <span class="switch" role="tablist">
        <button role="tab" aria-selected={editor.mode === "form"} class:front={editor.mode === "form"} onclick={() => (editor.mode = "form")}>Form</button>
        <button role="tab" aria-selected={editor.mode === "text"} class:front={editor.mode === "text"} onclick={() => (editor.mode = "text")}>Text</button>
      </span>
      <button class="btn" class:on={editor.panel} title="What this file does" onclick={() => (editor.panel = !editor.panel)}>Summary</button>
    {/if}
    <button class="btn" onclick={() => void pickLibrary()}>Open library...</button>
    <button class="btn" onclick={() => void newLibrary()}>New...</button>
    <button class="btn" disabled={!editing || !editor.dirty} onclick={() => void save()}>Save</button>
    <button
      class="btn primary"
      disabled={!editing || !project.project}
      title={project.project ? "Saves the file, and checks the library against the project" : "Open a project first"}
      onclick={ontry}
    >
      Try on project
    </button>
  </div>

  {#if editor.readOnly}
    <p class="note">From a repository, so it can be read here and not saved.</p>
  {:else if editor.fresh}
    <p class="note accent">Filled in from what is in the folder. Look it over, then Save: nothing is written until you do.</p>
  {:else if editor.converted}
    <p class="note warn">
      Read from installer.yml and converted. Save writes libnima.yml beside it, and installer.yml stays as it was.
      {#if editor.text.includes("python")}
        A script became a command that runs python: look over the steps under run.
      {/if}
    </p>
  {/if}
  {#if editor.problem}
    <p class="note bad">{editor.problem}</p>
  {/if}

  <div class="work">
    <div class="main">
      {#if editing && editor.mode === "form"}
        <YmlForm onundo={runUndo} onredo={runRedo} />
      {/if}
      <div class="code" class:hidden={editing && editor.mode === "form"} bind:this={host}>
        {#if !hasFile}
          <p class="empty">
            Open a library's folder to edit its libnima.yml, or to convert its installer.yml. New starts one from what is in a
            folder. A library on the project's list opens here with its yml button.
          </p>
        {/if}
      </div>
    </div>
    {#if editing && editor.panel}
      <OutlinePanel />
    {/if}
  </div>

  {#if editing}
    <section class="problems">
      <h2>
        Problems
        {#if editor.problems.length === 0}
          <span class="fine">none</span>
        {:else if errors === 0}
          <span class="warned">{editor.problems.length}, none that stops an install</span>
        {:else}
          <span class="wrong">{errors}: it would not install like this</span>
        {/if}
      </h2>
      <div class="rows">
        {#each editor.problems as problem, index (index)}
          <div class="row" class:warning={problem.warning}>
            <button class="where mono" onclick={() => showLine(problem.line)}>{problem.line ? `line ${problem.line}` : ""}</button>
            <span class="message">{problem.message}</span>
            {#each problem.fixes as fix (fix.label)}
              <button class="btn fix" onclick={() => useFix(fix)}>{fix.label}</button>
            {/each}
          </div>
        {/each}
      </div>
    </section>
  {/if}
</div>

<style>
  .tab {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }

  .title {
    flex: 1;
    min-width: 0;
  }

  .switch {
    display: inline-flex;
    flex-shrink: 0;
    overflow: hidden;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .switch button {
    height: 26px;
    padding: 0 12px;
    font: inherit;
    color: var(--muted);
    background: none;
    border: 0;
    cursor: default;
  }

  .switch button.front {
    color: var(--text);
    background: var(--accent-soft);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .btn.on {
    color: var(--accent);
  }

  .note {
    margin: 0;
    padding: 6px 12px;
    color: var(--muted);
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }

  .note.warn {
    color: var(--warn);
  }

  .note.bad {
    color: var(--bad);
  }

  .note.accent {
    color: var(--accent);
  }

  .work {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .main {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
  }

  .code {
    position: relative;
    height: 100%;
    min-height: 0;
  }

  .code.hidden {
    display: none;
  }

  .code :global(.cm-editor) {
    height: 100%;
  }

  .empty {
    position: absolute;
    top: 30%;
    left: 50%;
    z-index: 1;
    width: min(460px, 80%);
    margin: 0;
    line-height: 1.5;
    color: var(--muted);
    text-align: center;
    pointer-events: none;
    transform: translateX(-50%);
  }

  .problems {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    max-height: 30%;
    min-height: 32px;
    background: var(--panel);
    border-top: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    padding: 6px 12px;
    font-size: 11px;
    font-weight: 600;
    color: var(--muted);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  h2 span {
    margin-left: 6px;
    text-transform: none;
    letter-spacing: 0;
  }

  .fine {
    color: var(--ok);
  }

  .warned {
    color: var(--warn);
  }

  .wrong {
    color: var(--bad);
  }

  .rows {
    overflow-y: auto;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 12px;
  }

  .row:hover {
    background: var(--hover);
  }

  .where {
    flex-shrink: 0;
    width: 62px;
    padding: 0;
    font-size: 12px;
    color: var(--bad);
    text-align: left;
    background: none;
    border: 0;
    cursor: pointer;
  }

  .row.warning .where {
    color: var(--warn);
  }

  .message {
    flex: 1;
    min-width: 0;
    user-select: text;
  }

  .fix {
    flex-shrink: 0;
    height: 24px;
    font-size: 12px;
    color: var(--accent);
  }
</style>
