<script lang="ts">
  import FormField from "./FormField.svelte";
  import { editor, groupOfLine, keyAt, showLine, useFix } from "./editor.svelte";

  let { onundo, onredo }: { onundo: () => void; onredo: () => void } = $props();

  /** Undo and redo for what the form changed, unless a field is being typed in, which has its own. */
  function onkeydown(event: KeyboardEvent) {
    const typing = event.target instanceof HTMLInputElement && event.target.type !== "checkbox";
    if (typing || !(event.ctrlKey || event.metaKey)) {
      return;
    }
    if (event.key.toLowerCase() === "z") {
      event.preventDefault();
      if (event.shiftKey) {
        onredo();
      } else {
        onundo();
      }
    } else if (event.key.toLowerCase() === "y") {
      event.preventDefault();
      onredo();
    }
  }

  const data = $derived(editor.outline?.data ?? null);
  const platforms = $derived.by(() => {
    const given = data?.platform;
    return Array.isArray(given) ? (given as string[]) : typeof given === "string" ? [given] : [];
  });
  /** A platform's part shows only for a library that is for it. */
  const groups = $derived(editor.groups.filter((group) => group.platform === null || platforms.includes(group.platform)));
  const shown = $derived(groups.find((group) => group.name === editor.group) ?? groups[0]);

  /** The problems of a part of the form. */
  function problemsOf(name: string): number {
    return editor.problems.filter((problem) => {
      if (problem.line === null) {
        return name === "About";
      }
      return (groupOfLine(problem.line) ?? "About") === name;
    }).length;
  }
</script>

{#if editor.outline?.error}
  <div class="unread">
    <p>The text has a problem, so the form cannot read it:</p>
    <p class="why">{editor.outline.error.message}</p>
    {#each editor.problems.flatMap((problem) => problem.fixes) as fix (fix.label)}
      <button class="btn" onclick={() => useFix(fix)}>{fix.label}</button>
    {/each}
    <button class="btn primary" onclick={() => showLine(editor.outline?.error?.line ?? 1)}>Show it in the text</button>
  </div>
{:else if data && shown}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="form" {onkeydown}>
    <nav>
      {#each groups as group (group.name)}
        {@const problems = problemsOf(group.name)}
        <button class="part" class:front={group.name === shown.name} onclick={() => (editor.group = group.name)}>
          <span>{group.name}</span>
          {#if problems > 0}
            <span class="count">{problems}</span>
          {/if}
        </button>
      {/each}
    </nav>
    <div class="fields">
      {#each shown.keys as name (name)}
        {@const key = keyAt(name)}
        {#if key}
          <FormField {key} at={[name]} value={data[name]} />
        {/if}
      {/each}
    </div>
  </div>
{/if}

<style>
  .form {
    display: grid;
    grid-template-columns: 124px minmax(0, 1fr);
    height: 100%;
    min-height: 0;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 10px 8px;
    overflow-y: auto;
    border-right: 1px solid var(--border);
  }

  .part {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 5px 8px;
    font: inherit;
    color: var(--muted);
    text-align: left;
    background: none;
    border: 0;
    border-radius: 6px;
    cursor: default;
  }

  .part:hover {
    color: var(--text);
    background: var(--hover);
  }

  .part.front {
    color: var(--text);
    background: var(--accent-soft);
  }

  .count {
    min-width: 16px;
    font-size: 11px;
    line-height: 16px;
    color: var(--on-accent);
    text-align: center;
    background: var(--warn);
    border-radius: 8px;
  }

  .fields {
    padding: 14px 18px;
    overflow-y: auto;
  }

  .unread {
    padding: 18px;
  }

  .unread p {
    margin: 0 0 8px;
  }

  .why {
    color: var(--bad);
    user-select: text;
  }

  .unread .btn {
    margin-right: 8px;
  }
</style>
