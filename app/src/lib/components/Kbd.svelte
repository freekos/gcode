<script lang="ts">
  // ZCode-style: a hotkey is quiet text, not a boxed key (the boxed look read "wooden").
  let { keys }: { keys: string } = $props();
  // thin space between the modifier and the key: "⌘N" -> "⌘ N"
  const pretty = $derived(keys.replace(/([⌘⌥⇧⌃])(?=\S)/g, "$1\u2009"));
</script>

<kbd>{pretty}</kbd>

<style>
  kbd {
    font-family: var(--font-ui);
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    white-space: nowrap;
  }
</style>
