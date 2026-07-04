<script lang="ts">
  import "$lib/design/app.css";
  import { onMount } from "svelte";
  let { children } = $props();
  let fatal = $state("");

  onMount(() => {
    if ("__TAURI_INTERNALS__" in window) document.documentElement.classList.add("native");

    // app zoom: cmd+= / cmd+- / cmd+0 (persisted)
    let zoom = Number(localStorage.getItem("gcode.zoom") ?? 1) || 1;
    const applyZoom = () => {
      (document.body.style as CSSStyleDeclaration & { zoom: string }).zoom = String(zoom);
      localStorage.setItem("gcode.zoom", String(zoom));
    };
    applyZoom();
    const onZoomKey = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      if (e.key === "=" || e.key === "+") {
        e.preventDefault();
        zoom = Math.min(1.5, Math.round((zoom + 0.1) * 10) / 10);
        applyZoom();
      } else if (e.key === "-") {
        e.preventDefault();
        zoom = Math.max(0.7, Math.round((zoom - 0.1) * 10) / 10);
        applyZoom();
      } else if (e.key === "0") {
        e.preventDefault();
        zoom = 1;
        applyZoom();
      }
    };
    window.addEventListener("keydown", onZoomKey);

    // global tooltip: one fixed pill, clamped to the viewport (panels clip ::after)
    const tip = document.createElement("div");
    tip.id = "gc-tip";
    document.body.appendChild(tip);
    let timer = 0;
    const onOver = (e: MouseEvent) => {
      const el = (e.target as HTMLElement).closest("[data-tip]") as HTMLElement | null;
      if (!el) return;
      clearTimeout(timer);
      timer = window.setTimeout(() => {
        const r = el.getBoundingClientRect();
        // the trigger may have vanished (hover-revealed buttons) — a zero rect
        // would pin the tip to the window corner, over the traffic lights
        if (!el.isConnected || (r.width === 0 && r.height === 0)) return;
        tip.textContent = el.dataset.tip ?? "";
        tip.classList.add("show");
        const tw = tip.offsetWidth;
        const x = Math.min(Math.max(8, r.left + r.width / 2 - tw / 2), window.innerWidth - tw - 8);
        let y = r.top - tip.offsetHeight - 8;
        if (y < 8) y = r.bottom + 8;
        tip.style.left = `${x}px`;
        tip.style.top = `${y}px`;
      }, 450);
    };
    const onOut = (e: MouseEvent) => {
      const el = (e.target as HTMLElement).closest("[data-tip]");
      if (!el) return;
      clearTimeout(timer);
      tip.classList.remove("show");
    };
    document.addEventListener("mouseover", onOver);
    document.addEventListener("mouseout", onOut);
    document.addEventListener("mousedown", () => tip.classList.remove("show"));
    // (zoom listener cleanup)
    const cleanupZoom = () => window.removeEventListener("keydown", onZoomKey);
    void cleanupZoom;
    const onErr = (e: ErrorEvent) => (fatal = `${e.message}\n${e.filename}:${e.lineno}`);
    const onRej = (e: PromiseRejectionEvent) => (fatal = `unhandled rejection: ${e.reason}`);
    window.addEventListener("error", onErr);
    window.addEventListener("unhandledrejection", onRej);
    return () => {
      window.removeEventListener("error", onErr);
      window.removeEventListener("unhandledrejection", onRej);
    };
  });
</script>

<div class="win-frame" data-tauri-drag-region></div>
<div class="window-rim glass-rim" aria-hidden="true"></div>

{#if fatal}
  <div class="fatal">
    <b>Ошибка приложения</b>
    <pre>{fatal}</pre>
  </div>
{/if}
{@render children()}

<style>
  /* drag zone: only the TOP strip of the window (mac convention), a bit tall
     so it's easy to grab — the 8px rim plus a slice of the top edge */
  /* topmost thin band: the transparent .layout grid swallowed clicks meant for
     a frame that sat BELOW it — the drag band must sit ABOVE the content */
  .win-frame {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 12px;
    z-index: 500;
  }
  .window-rim {
    display: none;
  }
  :global(:root.native) .window-rim {
    display: block;
    position: fixed;
    inset: 0;
    border-radius: 14px;
    z-index: 998;
    pointer-events: none;
  }
  .fatal {
    position: fixed;
    inset: auto 12px 12px 12px;
    z-index: 999;
    background: var(--diff-del-bg);
    border: 1px solid var(--diff-del);
    border-radius: var(--r-lg);
    padding: 12px 14px;
    color: var(--text-primary);
  }
  .fatal pre { white-space: pre-wrap; font-size: 11.5px; margin: 6px 0 0; }
</style>
