<script lang="ts">
  import type { Line } from "./project.svelte";

  let { line }: { line: Line } = $props();
</script>

<div class="line mono">{#each line as span, at (at)}<span class={span.style}>{span.text}</span>{/each}</div>

<style>
  .line {
    min-height: 1.5em;
    line-height: 1.5;
    white-space: pre-wrap;
    user-select: text;
  }

  .bold {
    font-weight: 700;
  }

  .title {
    font-weight: 700;
    color: var(--accent);
  }

  .dim,
  .note {
    color: var(--muted);
  }

  .good {
    color: var(--ok);
  }

  .warn {
    color: var(--warn);
  }

  .bad {
    color: var(--bad);
  }

  .accent {
    color: var(--accent);
  }
</style>
