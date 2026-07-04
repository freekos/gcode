<script lang="ts">
  import FileTree from "./FileTree.svelte";
  import { projectDirList, type DirEntry } from "$lib/api";

  // Lazy directory tree of the project's WORKING COPY (branch badges on repos).
  let {
    projectId,
    rel = "",
    depth = 0,
    onopen,
  }: {
    projectId: number;
    rel?: string;
    depth?: number;
    onopen: (rel: string) => void;
  } = $props();

  let entries: DirEntry[] | null = $state(null);
  let expanded: Record<string, boolean> = $state({});

  $effect(() => {
    projectDirList(projectId, rel).then((e) => (entries = e));
  });
</script>

{#if entries === null}
  <div class="ft-load" style="padding-left:{depth * 14 + 10}px">…</div>
{:else}
  {#each entries as e (e.name)}
    {@const childRel = rel ? `${rel}/${e.name}` : e.name}
    {#if e.is_dir}
      <button
        class="ft-row"
        style="padding-left:{depth * 14 + 8}px"
        onclick={() => (expanded[e.name] = !expanded[e.name])}
      >
        <span class="chev">{expanded[e.name] ? "▾" : "▸"}</span>
        <span class="nm">{e.name}</span>
        {#if e.branch}<span class="br">{e.branch}</span>{/if}
      </button>
      {#if expanded[e.name]}
        <FileTree {projectId} rel={childRel} depth={depth + 1} {onopen} />
      {/if}
    {:else}
      <button class="ft-row" style="padding-left:{depth * 14 + 22}px" onclick={() => onopen(childRel)}>
        <span class="nm file">{e.name}</span>
      </button>
    {/if}
  {/each}
{/if}

<style>
  .ft-row {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--text-secondary);
    font: 12.5px var(--font-ui);
    padding: 3.5px 8px;
    border-radius: var(--r-sm);
    cursor: pointer;
    text-align: left;
  }
  .ft-row:hover { background: var(--surface-2); color: var(--text-primary); }
  .chev { font-size: 9px; color: var(--text-muted); width: 10px; flex: none; }
  .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .nm.file { color: var(--text-secondary); }
  .br {
    margin-left: auto;
    font: 10px var(--font-mono);
    color: var(--text-muted);
    background: var(--surface-2);
    border-radius: 999px;
    padding: 1px 7px;
    flex: none;
  }
  .ft-load { color: var(--text-disabled); font-size: 11px; }
</style>
