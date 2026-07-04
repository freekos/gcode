<script module lang="ts">
  export type TaskStatus = "new" | "running" | "needs_input" | "review" | "done";
</script>

<script lang="ts">
  let { status, size = 8 }: { status: TaskStatus; size?: number } = $props();
  const colors: Record<TaskStatus, string> = {
    new: "var(--status-new)",
    running: "var(--status-running)",
    needs_input: "var(--status-needs-input)",
    review: "var(--status-review)",
    done: "var(--status-done)",
  };
</script>

<span
  class="dot {status === 'running' ? 'gc-pulse' : ''}"
  style="background:{colors[status]};width:{size}px;height:{size}px"
></span>

<style>
  .dot { display: inline-block; border-radius: 50%; flex: none; }
</style>
