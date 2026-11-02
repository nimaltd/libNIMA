<script lang="ts">
  import { editor, setValue, showLine, type Key, type Step } from "./editor.svelte";

  let { key, at, value }: { key: Key; at: Step[]; value: unknown } = $props();

  /** How many files of a folder are offered at once. */
  const OFFERED = 200;

  let adding = $state("");

  const items = $derived(Array.isArray(value) ? (value as unknown[]) : []);
  const place = $derived(at.join("."));
  const leaf = $derived(key.path.slice(key.path.lastIndexOf(".") + 1));
  const listed = $derived(editor.outline?.lists.find((list) => list.key === place)?.entries ?? []);

  /** Every file the entries come to. */
  const named = $derived(new Set(editor.outline?.lists.flatMap((list) => list.entries.flatMap((entry) => entry.files)) ?? []));

  /** Files of the folder this list could take, and does not name. */
  const offered = $derived.by(() => {
    const outline = editor.outline;
    if (!outline) {
      return [];
    }
    if (leaf === "headers" || leaf === "sources") {
      return outline.unlisted.filter((file) => file.belongs === leaf).map((file) => file.path);
    }
    if (leaf === "extras") {
      return outline.files.filter((file) => !file.includes("/") && !file.startsWith(".") && !named.has(file) && !/\.(c|cc|cpp|cxx|h|hh|hpp|hxx|s|asm)$/i.test(file));
    }
    return [];
  });

  function line(index: number): number | undefined {
    return editor.outline?.lines[`${place}.${index}`];
  }

  /** What an entry says, a mapping's by its parts. */
  function textOf(item: unknown): string {
    if (typeof item === "string") {
      return item;
    }
    const map = item as Record<string, string>;
    if (map.root) {
      return map.from ? `${map.from} into the project at ${map.root}` : `${map.root} into the project`;
    }
    return map.to ? `${map.from ?? map.to} to ${map.to}` : (map.from ?? "");
  }

  function put(list: unknown[]) {
    void setValue(at, list.length > 0 ? list : null);
  }

  function addFile(file: string) {
    if (file.trim() !== "") {
      put([...items, file.trim()]);
    }
  }

  function remove(index: number) {
    put(items.filter((_, at) => at !== index));
  }

  function makeFirst(index: number) {
    put([items[index], ...items.filter((_, at) => at !== index)]);
  }
</script>

<div class="files">
  {#each items as item, index (index)}
    {@const entry = listed[index]}
    <div class="file">
      <input type="checkbox" checked title="Take it off the list" onchange={() => remove(index)} />
      <span class="path" class:gone={entry?.missing}>{textOf(item)}</span>
      {#if entry?.missing}
        <span class="note bad">not in the folder</span>
      {:else if entry && entry.files.length > 1}
        <span class="note">{entry.files.length} files</span>
      {/if}
      {#if leaf === "headers" && index === 0 && place === "headers"}
        <span class="note main">the one to #include</span>
      {:else if leaf === "headers" && index > 0 && place === "headers"}
        <button class="small" title="The first header is the one to #include, and its @version is the library's" onclick={() => makeFirst(index)}>
          Make it first
        </button>
      {/if}
      {#if typeof item !== "string"}
        <button class="small" onclick={() => showLine(line(index) ?? null)}>Edit in the text</button>
      {/if}
    </div>
  {/each}

  {#each offered.slice(0, OFFERED) as file (file)}
    <div class="file spare">
      <input type="checkbox" title="Put it on the list" onchange={() => addFile(file)} />
      <span class="path">{file}</span>
      {#if leaf === "headers" || leaf === "sources"}
        <span class="note warn">in the folder, not listed</span>
      {/if}
    </div>
  {/each}

  <div class="add">
    <input
      class="field mono"
      list={`files-${place}`}
      placeholder={leaf === "headers" ? "A path, or a wildcard such as src/port/*.h" : leaf === "sources" ? "A path, or a wildcard such as src/port/*.c" : "A file or a folder"}
      spellcheck="false"
      bind:value={adding}
      onkeydown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          addFile(adding);
          adding = "";
        }
      }}
    />
    <datalist id={`files-${place}`}>
      {#each editor.outline?.files ?? [] as file (file)}
        <option value={file}></option>
      {/each}
    </datalist>
    <button
      class="btn"
      disabled={adding.trim() === ""}
      onclick={() => {
        addFile(adding);
        adding = "";
      }}
    >
      Add
    </button>
  </div>
</div>

<style>
  .files {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
  }

  .path {
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .path.gone {
    color: var(--bad);
  }

  .spare .path {
    color: var(--muted);
  }

  .note {
    flex-shrink: 0;
    font-size: 11px;
    color: var(--muted);
  }

  .note.bad {
    color: var(--bad);
  }

  .note.warn {
    color: var(--warn);
  }

  .note.main {
    color: var(--accent);
  }

  .small {
    flex-shrink: 0;
    height: 20px;
    padding: 0 6px;
    font: inherit;
    font-size: 11px;
    color: var(--muted);
    background: none;
    border: 1px solid var(--border);
    border-radius: 4px;
    cursor: pointer;
  }

  .small:hover {
    color: var(--text);
  }

  .add {
    display: flex;
    gap: 8px;
    margin-top: 6px;
  }

  .add .field {
    flex: 1;
    min-width: 0;
    font-size: 12px;
  }
</style>
