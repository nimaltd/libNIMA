<script lang="ts">
  let { path }: { path: string } = $props();
</script>

<!-- Cut from the front when it does not fit, since the end is what tells two paths apart. -->
<span class="path mono" title={path}><bdi>{path}</bdi></span>

<style>
  .path {
    display: block;
    min-width: 0;
    overflow: hidden;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
  }
</style>
