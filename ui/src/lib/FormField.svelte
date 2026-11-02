<script lang="ts">
  import FormField from "./FormField.svelte";
  import FormFiles from "./FormFiles.svelte";
  import { editor, keysBelow, nameOf, renameKey, setValue, showLine, type Key, type Step } from "./editor.svelte";
  import { textToWords, typedValue, wordsToText } from "./words";

  let {
    key,
    at,
    value,
    set = (found: unknown) => void setValue(at, found),
  }: {
    key: Key;
    at: Step[];
    value: unknown;
    /** How a change is made, where it is not simply this value that changes. */
    set?: (found: unknown) => void;
  } = $props();

  /** The lines being typed that are not in the file yet. */
  let adding = $state(false);

  const given = $derived(value !== undefined && value !== null);
  const line = $derived(editor.outline?.lines[at.join(".")] ?? null);
  const isMap = (found: unknown) => typeof found === "object" && found !== null && !Array.isArray(found);
  const isPlain = (found: unknown) => !Array.isArray(found) && !isMap(found);
  const list = $derived(Array.isArray(value) ? (value as unknown[]) : []);
  /** A list of lines the form can edit: one under titles or with notes is for the text. */
  const plainList = $derived(!given || (Array.isArray(value) && list.every(isPlain)));
  const rowKeys = $derived(keysBelow(`${key.path}[]`));
  const nameKeys = $derived(keysBelow(`${key.path}.*`));
  const inner = $derived(keysBelow(key.path));
  const names = $derived(isMap(value) ? Object.keys(value as object) : []);

  function setText(text: string) {
    const found = text.trim();
    set(found === "" ? null : key.shape === "value" ? typedValue(found) : found);
  }

  function setChoices(choice: string, on: boolean) {
    const now = Array.isArray(value) ? (value as string[]) : typeof value === "string" ? [value] : [];
    const chosen = key.choices.filter((one) => (one === choice ? on : now.includes(one)));
    set(chosen.length === 0 ? null : chosen.length === 1 ? chosen[0] : chosen);
  }

  function putList(items: unknown[]) {
    set(items.length > 0 ? items : null);
  }

  function setLine(index: number, text: string) {
    const found = text.trim();
    putList(found === "" ? list.filter((_, at) => at !== index) : list.map((one, at) => (at === index ? found : one)));
  }

  function addRow() {
    const fresh = key.fresh ? JSON.parse(key.fresh) : {};
    void setValue([...at, list.length], fresh);
  }

  /** A row's value for a key of it: a row can be its first key's value alone, as a peripheral can. */
  function rowValue(row: unknown, child: Key, first: boolean): unknown {
    if (isMap(row)) {
      return (row as Record<string, unknown>)[nameOf(child)];
    }
    return first ? row : undefined;
  }

  function setRow(index: number, row: unknown, child: Key, first: boolean, found: unknown) {
    const name = nameOf(child);
    if (isMap(row)) {
      void setValue([...at, index, name], found);
    } else if (first) {
      void setValue([...at, index], found);
    } else {
      const firstName = nameOf(rowKeys[0]);
      void setValue([...at, index], found === null ? row : { [firstName]: row, [name]: found });
    }
  }

  function addName() {
    const fresh = key.fresh ? JSON.parse(key.fresh) : {};
    let name = "new-option";
    for (let count = 2; names.includes(name); count++) {
      name = `new-option-${count}`;
    }
    void setValue([...at, name], fresh);
  }
</script>

{#snippet head()}
  <div class="head">
    <span class="label">{key.label}</span>
    {#if line}
      <button class="line" title="Show it in the text" onclick={() => showLine(line)}>line {line}</button>
    {/if}
  </div>
  <p class="help">{key.help}</p>
{/snippet}

{#snippet forText()}
  <button class="btn fortext" onclick={() => showLine(line)}>
    It is written in a way the form cannot show, such as with titles or notes. Edit it in the text
  </button>
{/snippet}

{#if key.shape === "section"}
  <details class="section" open={given || at.length === 1}>
    <summary>
      <span class="label">{key.label}</span>
      <span class="help inline">{key.help}</span>
    </summary>
    <div class="inside">
      {#each inner as child (child.path)}
        <FormField
          key={child}
          at={[...at, nameOf(child)]}
          value={isMap(value) ? (value as Record<string, unknown>)[nameOf(child)] : undefined}
        />
      {/each}
    </div>
  </details>
{:else if key.shape === "rows"}
  <div class="field-block">
    {@render head()}
    {#if !plainListOfRows(value)}
      {@render forText()}
    {:else}
      {#each list as row, index (index)}
        <div class="card">
          {#each rowKeys as child, place (child.path)}
            <FormField
              key={child}
              at={[...at, index, nameOf(child)]}
              value={rowValue(row, child, place === 0)}
              set={(found) => setRow(index, row, child, place === 0, found)}
            />
          {/each}
          <div class="card-end">
            <button class="btn" onclick={() => void setValue([...at, index], null)}>Remove</button>
          </div>
        </div>
      {/each}
      <button class="btn" onclick={addRow}>Add</button>
    {/if}
  </div>
{:else if key.shape === "named"}
  <div class="field-block">
    {@render head()}
    {#each names as name (name)}
      <div class="card">
        <label class="row-label">
          <span>Name</span>
          <input
            class="field mono"
            spellcheck="false"
            value={name}
            onchange={(event) => {
              const found = event.currentTarget.value.trim();
              if (found && found !== name) {
                void renameKey([...at, name], found);
              }
            }}
          />
        </label>
        {#each nameKeys as child (child.path)}
          <FormField
            key={child}
            at={[...at, name, nameOf(child)]}
            value={(((value as Record<string, unknown>)[name] ?? {}) as Record<string, unknown>)[nameOf(child)]}
          />
        {/each}
        <div class="card-end">
          <button class="btn" onclick={() => void setValue([...at, name], null)}>Remove</button>
        </div>
      </div>
    {/each}
    <button class="btn" onclick={addName}>Add an option</button>
  </div>
{:else if key.shape === "files"}
  <div class="field-block">
    {@render head()}
    <FormFiles {key} {at} {value} />
  </div>
{:else if key.shape === "lines"}
  <div class="field-block">
    {@render head()}
    {#if !plainList}
      {@render forText()}
    {:else}
      {#each list as item, index (index)}
        <div class="line-row">
          <input
            class="field mono"
            spellcheck="false"
            list={key.choices.length > 0 ? `choices-${key.path}` : undefined}
            value={String(item)}
            onchange={(event) => setLine(index, event.currentTarget.value)}
          />
          <button class="btn" title="Take it off the list" onclick={() => putList(list.filter((_, at) => at !== index))}>Remove</button>
        </div>
      {/each}
      {#if adding}
        <div class="line-row">
          <!-- svelte-ignore a11y_autofocus -->
          <input
            class="field mono"
            spellcheck="false"
            autofocus
            list={key.choices.length > 0 ? `choices-${key.path}` : undefined}
            placeholder={key.example.replace(/^- /, "").replace(/^\[|\]$/g, "")}
            onblur={(event) => {
              const found = event.currentTarget.value.trim();
              adding = false;
              if (found) {
                putList([...list, found]);
              }
            }}
            onkeydown={(event) => {
              if (event.key === "Enter") {
                event.currentTarget.blur();
              }
            }}
          />
        </div>
      {:else}
        <button class="btn" onclick={() => (adding = true)}>Add</button>
      {/if}
      {#if key.choices.length > 0}
        <datalist id={`choices-${key.path}`}>
          {#each key.choices as choice (choice)}
            <option value={choice}></option>
          {/each}
        </datalist>
      {/if}
    {/if}
  </div>
{:else if key.shape === "flag"}
  <div class="field-block">
    <label class="check">
      <input
        type="checkbox"
        checked={given ? value === true : key.on}
        onchange={(event) => set(event.currentTarget.checked)}
      />
      <span class="label">{key.label}</span>
      {#if line}
        <button class="line" title="Show it in the text" onclick={() => showLine(line)}>line {line}</button>
      {/if}
    </label>
    <p class="help">{key.help}</p>
  </div>
{:else if key.shape === "choices"}
  <div class="field-block">
    {@render head()}
    <div class="choices">
      {#each key.choices as choice (choice)}
        <label class="check">
          <input
            type="checkbox"
            checked={Array.isArray(value) ? value.includes(choice) : value === choice}
            onchange={(event) => setChoices(choice, event.currentTarget.checked)}
          />
          <span>{choice}</span>
        </label>
      {/each}
    </div>
  </div>
{:else if key.shape === "choice" && !key.open}
  <div class="field-block">
    {@render head()}
    <select
      class="field"
      value={given ? String(value) : ""}
      onchange={(event) => set(event.currentTarget.value === "" ? null : event.currentTarget.value)}
    >
      <option value="">Not set</option>
      {#each key.choices as choice (choice)}
        <option value={choice}>{choice}</option>
      {/each}
      {#if given && !key.choices.includes(String(value))}
        <option value={String(value)}>{String(value)}</option>
      {/if}
    </select>
  </div>
{:else if key.shape === "words"}
  <div class="field-block">
    {@render head()}
    <input
      class="field mono wide"
      spellcheck="false"
      placeholder={key.example.replace(/^\[|\]$/g, "").replaceAll(",", "")}
      value={wordsToText(value)}
      onchange={(event) => {
        const words = textToWords(event.currentTarget.value);
        set(words.length > 0 ? words : null);
      }}
    />
  </div>
{:else if !isPlain(value)}
  <div class="field-block">
    {@render head()}
    {@render forText()}
  </div>
{:else}
  <div class="field-block">
    {@render head()}
    <input
      class="field wide"
      class:mono={key.shape === "value"}
      spellcheck="false"
      list={key.choices.length > 0 ? `choices-${key.path}` : undefined}
      placeholder={key.example}
      value={given ? String(value) : ""}
      onchange={(event) => setText(event.currentTarget.value)}
    />
    {#if key.choices.length > 0}
      <datalist id={`choices-${key.path}`}>
        {#each key.choices as choice (choice)}
          <option value={choice}></option>
        {/each}
      </datalist>
    {/if}
  </div>
{/if}

<script lang="ts" module>
  /** Whether a list of rows is one the form can show: each a mapping, or a word as a peripheral can be. */
  function plainListOfRows(value: unknown): boolean {
    return value === undefined || value === null || Array.isArray(value);
  }
</script>

<style>
  .field-block {
    margin-bottom: 16px;
  }

  .head {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .label {
    font-weight: 600;
  }

  .line {
    padding: 0;
    font: inherit;
    font-size: 11px;
    color: var(--muted);
    background: none;
    border: 0;
    cursor: pointer;
  }

  .line:hover {
    color: var(--accent);
    text-decoration: underline;
  }

  .help {
    margin: 1px 0 6px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--muted);
  }

  .help.inline {
    display: inline;
    margin-left: 8px;
  }

  .wide {
    width: 100%;
    max-width: 560px;
    box-sizing: border-box;
  }

  select.field {
    width: 240px;
  }

  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 16px;
  }

  .check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .line-row {
    display: flex;
    gap: 8px;
    max-width: 560px;
    margin-bottom: 4px;
  }

  .line-row .field {
    flex: 1;
    min-width: 0;
    font-size: 12px;
  }

  .card {
    max-width: 640px;
    margin-bottom: 10px;
    padding: 12px 14px 10px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .card-end {
    display: flex;
    justify-content: flex-end;
  }

  .row-label {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    font-weight: 600;
  }

  .row-label .field {
    flex: 1;
    max-width: 320px;
    font-weight: 400;
  }

  .section {
    margin-bottom: 12px;
  }

  summary {
    padding: 2px 0;
    cursor: default;
  }

  .inside {
    margin-top: 8px;
    padding-left: 14px;
    border-left: 1px solid var(--border);
  }

  .fortext {
    height: auto;
    min-height: 28px;
    white-space: normal;
  }
</style>
