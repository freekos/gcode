<script lang="ts">
  import StatusDot, { type TaskStatus } from "./StatusDot.svelte";
  // ZCode-style light row: dot + name; on hover — pin (left) and archive (right).
  let {
    title,
    status,
    hotkey,
    time,
    pinned = false,
    active = false,
    onclick,
    onpin,
    onarchive,
  }: {
    title: string;
    status: TaskStatus;
    hotkey?: string;
    time?: string;
    pinned?: boolean;
    active?: boolean;
    onclick?: () => void;
    onpin?: () => void;
    onarchive?: () => void;
  } = $props();
</script>

<div class="row" class:active role="button" tabindex="0" onclick={onclick} onkeydown={(e) => e.key === "Enter" && onclick?.()} title={hotkey ? `Открыть · ${hotkey}` : undefined}>
  <button
    class="side pin"
    class:pinned
    data-tip={pinned ? "Открепить" : "Закрепить"}
    aria-label="Закрепить"
    onclick={(e) => {
      e.stopPropagation();
      onpin?.();
    }}
  >
    <svg class="ic" viewBox="0 0 16 16"><path d="M9.5 2.2 13.8 6.5M10.6 3.3 6.9 5.1c-.3.1-.6.1-.9 0l-1.3-.4c-.6-.2-1.1.5-.7 1l5.3 5.3c.5.4 1.2-.1 1-.7l-.4-1.3c-.1-.3-.1-.6 0-.9l1.8-3.7M5.2 10.8 2.4 13.6" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/></svg>
  </button>
  <span class="dotwrap" class:hide-hover={true}><StatusDot {status} size={7} /></span>
  <span class="nm">{title}</span>
  {#if time}<span class="time">{time}</span>{/if}
  <button
    class="side arch"
    data-tip="В архив"
    aria-label="В архив"
    onclick={(e) => {
      e.stopPropagation();
      onarchive?.();
    }}
  >
    <svg class="ic" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="3.2" rx="1" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M3.2 6.2V12c0 .7.6 1.3 1.3 1.3h7c.7 0 1.3-.6 1.3-1.3V6.2M6.4 8.8h3.2" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" fill="none"/></svg>
  </button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    padding: 7px 6px;
    cursor: pointer;
    color: var(--text-secondary);
    font: 13px var(--font-ui);
    text-align: left;
    transition: background var(--t-fast) ease-out, color var(--t-fast) ease-out;
  }
  .row:hover { background: var(--surface-2); color: var(--text-primary); }
  .active { background: var(--accent-soft); color: var(--text-primary); }
  .nm { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .time { font-family: var(--font-mono); font-size: 10.5px; color: var(--text-muted); flex: none; }
  .row:hover .time { display: none; }
  .side {
    display: none;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    background: transparent;
    border: 0;
    padding: 0;
    cursor: pointer;
    color: var(--text-muted);
    flex: none;
  }
  .side:hover { color: var(--text-primary); }
  .row:hover .side { display: inline-flex; }
  .pin.pinned { display: inline-flex; color: var(--accent); }
  .row:hover .dotwrap { display: none; }
  .ic { width: 13px; height: 13px; }
</style>
