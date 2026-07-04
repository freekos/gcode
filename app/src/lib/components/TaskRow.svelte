<script lang="ts">
  import StatusDot, { type TaskStatus } from "./StatusDot.svelte";
  import Kbd from "./Kbd.svelte";
  // ZCode-style light row: dot + name + (hover) hotkey. No card chrome.
  let {
    title,
    status,
    hotkey,
    time,
    active = false,
    onclick,
  }: {
    title: string;
    status: TaskStatus;
    hotkey?: string;
    time?: string;
    active?: boolean;
    onclick?: () => void;
  } = $props();
</script>

<button class="row" class:active {onclick} title={hotkey ? `Открыть · ${hotkey}` : undefined}>
  <StatusDot {status} size={7} />
  <span class="nm">{title}</span>
  {#if hotkey}<span class="hk"><Kbd keys={hotkey} /></span>{/if}
  {#if time}<span class="time">{time}</span>{/if}
</button>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    padding: 7px 9px;
    cursor: pointer;
    color: var(--text-secondary);
    font: 13px var(--font-ui);
    text-align: left;
    transition: background var(--t-fast) ease-out, color var(--t-fast) ease-out;
  }
  .row:hover { background: var(--surface-2); color: var(--text-primary); }
  .row:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .active { background: var(--accent-soft); color: var(--text-primary); }
  .nm { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hk { opacity: 0; transition: opacity var(--t-fast) ease-out; }
  .row:hover .hk { opacity: 1; }
  .time { font-family: var(--font-mono); font-size: 10.5px; color: var(--text-muted); flex: none; }
  .row:hover .time { display: none; } /* время уступает место хоткею на hover */
</style>
