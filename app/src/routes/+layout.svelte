<script lang="ts">
  import "$lib/design/app.css";
  import { onMount } from "svelte";
  let { children } = $props();
  let fatal = $state("");

  onMount(() => {
    if ("__TAURI_INTERNALS__" in window) document.documentElement.classList.add("native");
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

{#if fatal}
  <div class="fatal">
    <b>Ошибка приложения</b>
    <pre>{fatal}</pre>
  </div>
{/if}
{@render children()}

<style>
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
