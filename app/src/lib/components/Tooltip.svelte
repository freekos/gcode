<script lang="ts">
  import type { Snippet } from "svelte";
  // Tooltip policy (ui-inventory.md): only for icon-only controls and packed
  // facts; shows on hover AND keyboard focus; ~500ms delay via CSS.
  let { text, children }: { text: string; children: Snippet } = $props();
</script>

<span class="tip-wrap">
  {@render children()}
  <span class="tip" role="tooltip">{text}</span>
</span>

<style>
  .tip-wrap { position: relative; display: inline-flex; }
  .tip {
    position: absolute;
    bottom: calc(100% + 6px);
    left: 50%;
    transform: translateX(-50%);
    background: var(--surface-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-md);
    color: var(--text-secondary);
    font-size: 11.5px;
    padding: 3px 8px;
    white-space: nowrap;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast) ease-out;
    transition-delay: 0ms;
    z-index: 50;
  }
  .tip-wrap:hover .tip,
  .tip-wrap:focus-within .tip {
    opacity: 1;
    transition-delay: 500ms;
  }
</style>
