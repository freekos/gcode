<script lang="ts">
  let {
    value = $bindable(""),
    placeholder = "",
    onkeydown,
  }: {
    value?: string;
    placeholder?: string;
    onkeydown?: (e: KeyboardEvent) => void;
  } = $props();
</script>

<input bind:value {placeholder} {onkeydown} />

<style>
  input {
    font: 13px var(--font-ui);
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border-subtle);
    padding: 7px 12px;
    border-radius: var(--r-md);
    width: 100%;
    transition: border-color var(--t-fast) ease-out, box-shadow var(--t-fast) ease-out;
  }
  input::placeholder { color: var(--text-muted); }
  input:focus { outline: none; border-color: var(--accent); box-shadow: none; }
</style>
