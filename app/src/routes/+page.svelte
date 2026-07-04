<script lang="ts">
  import Kbd from "$lib/components/Kbd.svelte";
</script>

<svelte:head><title>gcode</title></svelte:head>

<div class="empty">
  <span class="logo">g<b>code</b></span>
  <p>Экраны приложения собираются (фаза 3b).</p>
  <p class="hint"><a href="/styleguide">/styleguide</a> — живая витрина дизайн-системы · <Kbd keys="⌘N" /> скоро</p>
</div>

<style>
  .empty {
    height: 100vh;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-secondary);
  }
  .logo { font-family: var(--font-mono); font-weight: 700; font-size: 22px; color: var(--text-primary); }
  .logo b { color: var(--accent); }
  .hint { color: var(--text-muted); font-size: 12px; }
  a { color: var(--accent); }
</style>
