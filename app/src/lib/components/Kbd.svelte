<script lang="ts">
  let { keys }: { keys: string } = $props();
</script>

<kbd>{keys}</kbd>

<style>
  kbd {
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-secondary);
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-bottom-width: 2px;
    border-radius: var(--r-sm);
    padding: 1px 6px;
  }
</style>
