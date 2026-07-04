<script lang="ts">
  import type { Snippet } from "svelte";
  // Modal behavior per ui-inventory.md: overlay click closes (drafts persist at
  // the caller level); `guarded` — dangerous confirmations ignore overlay clicks.
  let {
    open = $bindable(false),
    guarded = false,
    width = "440px",
    children,
  }: {
    open?: boolean;
    guarded?: boolean;
    width?: string;
    children: Snippet;
  } = $props();

  let el: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  });

  function onclick(e: MouseEvent) {
    if (guarded || !el) return;
    if (e.target === el) open = false; // click on ::backdrop hits the dialog itself
  }
</script>

<dialog
  class="glass-rim"
  bind:this={el}
  style="width:min({width}, 94vw)"
  {onclick}
  onclose={() => (open = false)}
>
  {@render children()}
</dialog>

<style>
  dialog {
    /* Tailwind preflight zeroes margins — restore the UA centering of <dialog> */
    margin: auto;
    background: var(--surface-3);
    color: var(--text-primary);
    border: 0;
    border-radius: var(--r-xl);
    padding: 20px 22px;
    box-shadow: 0 24px 60px oklch(0% 0 0 / 0.5);
  }
  dialog::backdrop {
    background: oklch(10% 0.01 262 / 0.6);
    backdrop-filter: blur(2px);
  }
</style>
