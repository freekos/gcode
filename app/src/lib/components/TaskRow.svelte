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
  <!-- pin sits ON TOP of the left padding (out of flow — no layout shift) -->
  <button
    class="pin"
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
  <span class="dotwrap"><StatusDot {status} size={7} /></span>
  <span class="nm">{title}</span>
  <!-- fixed-width right cell: time and archive swap in place, nothing moves -->
  <span class="right">
    {#if time}<span class="time">{time}</span>{/if}
    <button
      class="arch"
      data-tip="В архив"
      aria-label="В архив"
      onclick={(e) => {
        e.stopPropagation();
        onarchive?.();
      }}
    >
      <svg class="ic" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="3.2" rx="1" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M3.2 6.2V12c0 .7.6 1.3 1.3 1.3h7c.7 0 1.3-.6 1.3-1.3V6.2M6.4 8.8h3.2" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" fill="none"/></svg>
    </button>
  </span>
</div>

<style>
  .row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    padding: 7px 6px 7px 24px;
    cursor: pointer;
    color: var(--text-secondary);
    font: 13px var(--font-ui);
    text-align: left;
    transition: background var(--t-fast) ease-out, color var(--t-fast) ease-out;
  }
  .row:hover { background: var(--surface-2); color: var(--text-primary); }
  .active { background: var(--accent-soft); color: var(--text-primary); }
  .nm { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* everything below is OUT OF FLOW or fixed-size — hover never shifts layout */
  .pin {
    position: absolute;
    left: 4px;
    top: 50%;
    transform: translateY(-50%);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border: 0;
    background: transparent;
    padding: 0;
    cursor: pointer;
    color: var(--text-muted);
    visibility: hidden;
  }
  .row:hover .pin { visibility: visible; }
  .pin.pinned { visibility: visible; color: var(--accent); }
  .pin:hover { color: var(--text-primary); }
  .right { position: relative; width: 34px; height: 18px; flex: none; }
  .time {
    position: absolute;
    inset: 0;
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--text-muted);
  }
  .arch {
    position: absolute;
    inset: 0;
    display: inline-flex;
    align-items: center;
    justify-content: flex-end;
    border: 0;
    background: transparent;
    padding: 0;
    cursor: pointer;
    color: var(--text-muted);
    visibility: hidden;
  }
  .arch:hover { color: var(--text-primary); }
  .row:hover .time { visibility: hidden; }
  .row:hover .arch { visibility: visible; }
  .ic { width: 13px; height: 13px; }
</style>
