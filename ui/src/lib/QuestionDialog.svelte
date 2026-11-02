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

  onMount(() => field?.focus());

  function submit() {
    if (question.kind.type === "text") {
      onanswer(typed.trim());
    } else {
      onanswer(question.kind.default ? "y" : "n");
    }
  }
</script>

<Modal title="A question" onclose={() => onanswer(null)} onsubmit={submit}>
  {#if context.length > 0}
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
  {/if}

  {#snippet buttons()}
    {#if question.kind.type === "text"}
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
</style>
