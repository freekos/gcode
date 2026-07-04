<script lang="ts">
  import StatusDot, { type TaskStatus } from "./StatusDot.svelte";
  import DiffStat from "./DiffStat.svelte";
  import Kbd from "./Kbd.svelte";
  let {
    title,
    status,
    branch,
    add = 0,
    del = 0,
    hotkey,
    active = false,
    ask,
    onclick,
  }: {
    title: string;
    status: TaskStatus;
    branch?: string;
    add?: number;
    del?: number;
    hotkey?: string;
    active?: boolean;
    ask?: string;
    onclick?: () => void;
  } = $props();
</script>

<button class="task" class:active {onclick}>
  <span class="row1">
    <StatusDot {status} />
    <span class="title">{title}</span>
    {#if hotkey}<Kbd keys={hotkey} />{/if}
  </span>
  {#if branch || add || del}
    <span class="row2">
      {#if branch}<span class="branch">{branch}</span>{/if}
      <span class="grow"></span>
      <DiffStat {add} {del} />
    </span>
  {/if}
  {#if ask}
    <span class="ask">{ask}</span>
  {/if}
</button>

<style>
  .task {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    text-align: left;
    background: var(--surface-1);
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-lg);
    padding: 12px 14px;
    cursor: pointer;
    color: var(--text-primary);
    font-family: var(--font-ui);
    transition: background var(--t-base) ease-out, border-color var(--t-base) ease-out;
  }
  .task:hover { background: var(--surface-2); }
  .task:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .active { border-color: var(--accent); background: var(--surface-2); }
  .row1 { display: flex; align-items: center; gap: 8px; }
  .title { font-weight: 600; font-size: 13px; flex: 1; }
  .row2 { display: flex; align-items: center; gap: 10px; width: 100%; }
  .grow { flex: 1; }
  .branch { font-family: var(--font-mono); font-size: 11px; color: var(--text-muted); }
  .ask {
    font-size: 12px;
    color: var(--status-needs-input);
    background: color-mix(in oklab, var(--status-needs-input) 12%, transparent);
    border-radius: var(--r-sm);
    padding: 5px 8px;
  }
</style>
