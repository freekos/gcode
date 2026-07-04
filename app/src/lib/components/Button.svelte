<script lang="ts">
  import type { Snippet } from "svelte";
  let {
    variant = "default",
    disabled = false,
    onclick,
    children,
  }: {
    variant?: "primary" | "default" | "ghost" | "danger";
    disabled?: boolean;
    onclick?: (e: MouseEvent) => void;
    children: Snippet;
  } = $props();
</script>

<button class="btn {variant}" {disabled} {onclick}>
  {@render children()}
</button>

<style>
  .btn {
    font: 500 13px var(--font-ui);
    color: var(--text-primary);
    background: var(--surface-2);
    border: 1px solid var(--border-subtle);
    padding: 6px 14px;
    border-radius: var(--r-md);
    cursor: pointer;
    transition:
      background var(--t-fast) ease-out,
      border-color var(--t-fast) ease-out,
      transform var(--t-fast) ease-out;
  }
  .btn:hover { background: var(--surface-3); border-color: var(--border-strong); }
  .btn:active { transform: translateY(1px); }
  .btn:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .primary { background: var(--accent); border-color: transparent; color: var(--on-accent); font-weight: 600; }
  .primary:hover { filter: brightness(1.08); background: var(--accent); }
  .ghost { background: transparent; border-color: transparent; color: var(--text-secondary); }
  .ghost:hover { background: var(--surface-2); color: var(--text-primary); }
  .danger { color: var(--diff-del); background: transparent; border-color: color-mix(in oklab, var(--diff-del) 40%, transparent); }
  .danger:hover { background: var(--diff-del-bg); }
  .btn:disabled { color: var(--text-disabled); background: var(--surface-1); border-color: var(--border-subtle); cursor: not-allowed; transform: none; }
</style>
