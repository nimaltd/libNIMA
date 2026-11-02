<script lang="ts">
  import Modal from "./Modal.svelte";

  let {
    title,
    text,
    yes,
    onyes,
    onclose,
  }: {
    title: string;
    text: string;
    /** What the button that goes ahead says. */
    yes: string;
    onyes: () => void;
    onclose: () => void;
  } = $props();

  function submit() {
    onyes();
    onclose();
  }
</script>

<Modal {title} {onclose} onsubmit={submit}>
  <p>{text}</p>

  {#snippet buttons()}
    <button class="btn" onclick={onclose}>Cancel</button>
    <button class="btn primary" onclick={submit}>{yes}</button>
  {/snippet}
</Modal>

<style>
  p {
    margin: 4px 0 0;
    line-height: 1.5;
  }
</style>
