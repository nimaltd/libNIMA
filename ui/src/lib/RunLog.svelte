<script lang="ts">
  import { tick } from "svelte";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import LogLine from "./LogLine.svelte";
  import { clearLog, view, type Line } from "./project.svelte";

  type State = "working" | "problem" | "done" | "";

  /** What is said beside a library's name, once it is known. */
  const WORDS = { working: "working...", problem: "a problem, see below", done: "done" };

  let box: HTMLDivElement | undefined = $state();

  const busy = $derived(view.running !== "");
  const empty = $derived(view.log.every((part) => part.length === 0));

  /** The line with a library's name and version, which its part is shown under. A part without one is about no library. */
  function headOf(part: Line[]): Line | undefined {
    return part.find((line) => line[0]?.style === "title");
  }

  /** How a library's part went. */
  function stateOf(part: Line[], last: boolean): State {
    const has = (style: string) => part.some((line) => line.some((span) => span.style === style));
    if (busy && last) {
      return "working";
    }
    return has("bad") ? "problem" : has("good") ? "done" : "";
  }

  function asText(parts: Line[][]): string {
    return parts
      .filter((part) => part.length > 0)
      .map((part) => part.map((line) => line.map((span) => span.text).join("")).join("\n"))
      .join(`\n\n${"-".repeat(60)}\n\n`);
  }

  // The newest line stays in view.
  $effect(() => {
    void view.log.length;
    void view.log.at(-1)?.length;
    void tick().then(() => {
      if (box) {
        box.scrollTop = box.scrollHeight;
      }
    });
  });
</script>

<section class="log">
  <div class="bar">
    <span>{view.running || "What the last run did"}</span>
    <span class="spacer"></span>
    <button class="btn small" disabled={empty} onclick={() => void writeText(asText(view.log))}>Copy</button>
    <button class="btn small" disabled={empty || busy} onclick={clearLog}>Clear</button>
  </div>
  <div class="lines" bind:this={box}>
    {#each view.log as part, index (index)}
      {@const head = headOf(part)}
      {#if head}
        {@const state = stateOf(part, index === view.log.length - 1)}
        <details class="part" open>
          <summary>
            <LogLine line={head} />
            {#if state}
              <span class="state {state}">{WORDS[state]}</span>
            {/if}
          </summary>
          <div class="inside">
            {#each part as line, at (at)}
              {#if line !== head}
                <LogLine {line} />
              {/if}
            {/each}
          </div>
        </details>
      {:else}
        {#each part as line, at (at)}
          <LogLine {line} />
        {/each}
      {/if}
    {/each}
  </div>
</section>

<style>
  .log {
    display: flex;
    flex: 0 0 32%;
    flex-direction: column;
    min-height: 90px;
    background: var(--panel);
    border-top: 1px solid var(--border);
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 12px;
    font-size: 12px;
    color: var(--muted);
  }

  .spacer {
    flex: 1;
  }

  .small {
    height: 22px;
    padding: 0 8px;
    font-size: 12px;
  }

  .lines {
    flex: 1;
    min-height: 0;
    padding: 4px 12px 8px;
    overflow: auto;
    /* A step under the editor's text, as it was at 12px beside the editor's 13px. */
    font-size: calc(var(--view-size) * 12 / 13);
    background: var(--bg);
  }

  .part {
    margin: 8px 0;
    border: 1px solid var(--border);
    border-radius: 6px;
  }

  summary {
    position: sticky;
    top: -4px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
    list-style: none;
    background: var(--panel);
    border-radius: 6px;
    cursor: default;
  }

  summary::-webkit-details-marker {
    display: none;
  }

  summary::before {
    color: var(--muted);
    content: "\25B8";
  }

  details[open] > summary::before {
    content: "\25BE";
  }

  .inside {
    padding: 6px 10px 8px;
  }

  .state {
    margin-left: auto;
    font-family: var(--font-ui);
    white-space: nowrap;
  }

  .state.working {
    color: var(--muted);
  }

  .state.problem {
    color: var(--bad);
  }

  .state.done {
    color: var(--ok);
  }
</style>
