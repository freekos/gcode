<script lang="ts">
  let { add = 0, del = 0 }: { add?: number; del?: number } = $props();
</script>

<span class="ds">
  {#if add}<span class="add">+{add}</span>{/if}
  {#if del}<span class="del">−{del}</span>{/if}
</span>

<style>
  .ds { font-family: var(--font-mono); font-size: 11px; display: inline-flex; gap: 6px; }
  .add { color: var(--diff-add); }
  .del { color: var(--diff-del); }
</style>
