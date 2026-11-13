<script lang="ts">
  import { onMount } from "svelte";
  import LogLine from "./LogLine.svelte";
  import Modal from "./Modal.svelte";
  import type { Line, Question } from "./project.svelte";

  let {
    question,
    context,
    onanswer,
  }: {
    question: Question;
    /** What was said just before it, such as the command a step would run. */
    context: Line[];
    /** The answer, or nothing for what the question offers. */
    onanswer: (answer: string | null) => void;
  } = $props();

  let field: HTMLInputElement | undefined = $state();
  let typed = $state("");
  /** For a choice: what the search leaves shown, and what is ticked. */
  let search = $state("");
  let picked: string[] = $state([]);

  /** The question as a person reads it: the way a terminal offers its answers belongs to a terminal. */
  const text = $derived(
    question.prompt
      .map((span) => span.text)
      .join("")
      .replace(/\s*\[y\/n\]:?\s*$/i, "")
      .replace(/\s*\[([^\]]*)\]:?\s*$/, "")
      .replace(/:\s*$/, ""),
  );
  /** What is in brackets at the end, which is what nothing answered takes. */
  const offered = $derived(/\[([^\]]*)\]:?\s*$/.exec(question.prompt.map((span) => span.text).join(""))?.[1] ?? "");
  /** The options of a choice the search leaves, in the list's order. */
  const shown = $derived(
    question.kind.type === "choose"
      ? question.kind.options.filter((one) => one.toLowerCase().includes(search.trim().toLowerCase()))
      : [],
  );

  onMount(() => field?.focus());

  function toggle(one: string) {
    if (question.kind.type === "choose" && !question.kind.many) {
      picked = [one];
    } else {
      picked = picked.includes(one) ? picked.filter((other) => other !== one) : [...picked, one];
    }
  }

  function tickShown() {
    picked = [...picked, ...shown.filter((one) => !picked.includes(one))];
  }

  function submit() {
    if (question.kind.type === "text") {
      onanswer(typed.trim());
    } else if (question.kind.type === "choose") {
      // The names, in the list's order, which the engine reads as the command reads them typed.
      const options = question.kind.options;
      onanswer(options.filter((one) => picked.includes(one)).join(", "));
    } else {
      onanswer(question.kind.default ? "y" : "n");
    }
  }
</script>

<Modal title="A question" onclose={() => onanswer(null)} onsubmit={submit}>
  <!-- A choice's list is the context: the command says it as lines, which would show it twice here. -->
  {#if context.length > 0 && question.kind.type !== "choose"}
    <div class="context">
      {#each context as line, index (index)}
        <LogLine {line} />
      {/each}
    </div>
  {/if}
  <p>{text}</p>
  {#if question.kind.type === "text"}
    <input class="field mono" spellcheck="false" placeholder={offered} bind:this={field} bind:value={typed} />
    {#if offered}
      <p class="dlg-hint">Left empty, it takes {offered}.</p>
    {/if}
  {:else if question.kind.type === "choose"}
    <input class="field mono" spellcheck="false" placeholder="Search" bind:this={field} bind:value={search} />
    <div class="choices" role="group" aria-label={text}>
      {#each shown as one (one)}
        <label class="choice">
          <input
            type={question.kind.many ? "checkbox" : "radio"}
            checked={picked.includes(one)}
            onchange={() => toggle(one)}
          />
          <span class="mono">{one}</span>
        </label>
      {:else}
        <p class="dlg-muted">Nothing matches the search.</p>
      {/each}
    </div>
    <p class="dlg-hint">
      {picked.length === 0 ? "Nothing ticked takes none." : `Ticked: ${picked.join(", ")}`}
    </p>
  {/if}

  {#snippet buttons()}
    {#if question.kind.type === "text"}
      <button class="btn primary" onclick={submit}>OK</button>
    {:else if question.kind.type === "choose"}
      {#if question.kind.many}
        <button class="btn" onclick={tickShown} disabled={shown.length === 0}>Tick all shown</button>
      {/if}
      <button class="btn primary" onclick={submit}>OK</button>
    {:else}
      <button class="btn" class:primary={!question.kind.default} onclick={() => onanswer("n")}>No</button>
      <button class="btn" class:primary={question.kind.default} onclick={() => onanswer("y")}>Yes</button>
    {/if}
  {/snippet}
</Modal>

<style>
  p {
    margin: 4px 0 10px;
    line-height: 1.5;
  }

  .field {
    width: 100%;
    box-sizing: border-box;
  }

  .context {
    max-height: 240px;
    margin: 4px 0 10px;
    padding: 6px 10px;
    overflow: auto;
    font-size: 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .choices {
    max-height: 240px;
    margin: 8px 0;
    padding: 4px 10px;
    overflow: auto;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  .choice {
    display: flex;
    gap: 8px;
    align-items: center;
    padding: 3px 0;
    cursor: pointer;
  }
</style>
