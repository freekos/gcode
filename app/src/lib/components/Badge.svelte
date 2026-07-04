<script lang="ts">
  import StatusDot, { type TaskStatus } from "./StatusDot.svelte";
  let { status, label }: { status: TaskStatus; label?: string } = $props();
  const names: Record<TaskStatus, string> = {
    new: "new",
    running: "running",
    needs_input: "needs input",
    review: "review",
    done: "done",
  };
</script>

<span class="badge">
  <StatusDot {status} />
  {label ?? names[status]}
</span>

<style>
  .badge {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 3px 10px;
    border-radius: 999px;
    font-size: 12px;
    font-weight: 500;
    background: var(--surface-2);
    border: 1px solid var(--border-subtle);
  }
</style>
