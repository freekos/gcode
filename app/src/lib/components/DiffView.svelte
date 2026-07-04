<script lang="ts">
  import DiffStat from "./DiffStat.svelte";
  import Button from "./Button.svelte";
  import type { DiffFile } from "$lib/api";

  // Unified diff with line selection -> batched comments (GitHub-review style).
  export interface PendingComment {
    file: string;
    from: number;
    to: number;
    code: string;
    text: string;
  }

  let {
    files,
    repo,
    onsend,
  }: {
    files: DiffFile[];
    repo: string;
    onsend: (comments: PendingComment[]) => void;
  } = $props();

  let collapsedFiles: Record<string, boolean> = $state({});
  let pending: PendingComment[] = $state([]);

  // selection state: file + line range (new-side numbers where possible)
  let selFile: string | null = $state(null);
  let selFrom: number | null = $state(null);
  let selTo: number | null = $state(null);
  let commentText = $state("");
  let composerAt: string | null = $state(null); // file where the comment box shows

  function lineNo(l: { new_no: number | null; old_no: number | null }): number | null {
    return l.new_no ?? l.old_no;
  }

  function clickLine(file: string, no: number | null, e: MouseEvent) {
    if (no === null) return;
    if (e.shiftKey && selFile === file && selFrom !== null) {
      selTo = no;
      if (selTo < selFrom) [selFrom, selTo] = [selTo, selFrom];
    } else {
      selFile = file;
      selFrom = no;
      selTo = no;
    }
    composerAt = file;
  }

  function inSelection(file: string, no: number | null): boolean {
    return (
      selFile === file && no !== null && selFrom !== null && selTo !== null && no >= selFrom && no <= selTo
    );
  }

  function selectedCode(): string {
    const f = files.find((x) => x.path === selFile);
    if (!f || selFrom === null || selTo === null) return "";
    const out: string[] = [];
    for (const h of f.hunks) {
      for (const l of h.lines) {
        const no = lineNo(l);
        if (no !== null && no >= selFrom && no <= selTo) {
          out.push((l.kind === "add" ? "+" : l.kind === "del" ? "-" : " ") + l.text);
        }
      }
    }
    return out.join("\n");
  }

  function addComment() {
    if (!selFile || selFrom === null || selTo === null || !commentText.trim()) return;
    pending = [
      ...pending,
      { file: selFile, from: selFrom, to: selTo, code: selectedCode(), text: commentText.trim() },
    ];
    commentText = "";
    composerAt = null;
    selFile = null;
    selFrom = selTo = null;
  }

  function sendAll() {
    if (!pending.length) return;
    onsend(pending);
    pending = [];
  }
</script>

<div class="diffwrap">
  {#if files.length === 0}
    <p class="empty">Изменений в <span class="mono">{repo}</span> пока нет.</p>
  {:else}
    {#each files as f (f.path)}
      <div class="dfile">
        <button class="dhead" onclick={() => (collapsedFiles[f.path] = !collapsedFiles[f.path])}>
          <span class="chev">{collapsedFiles[f.path] ? "▸" : "▾"}</span>
          <span class="path">{f.path}</span>
          {#if f.status !== "modified"}<span class="st-{f.status}">{f.status}</span>{/if}
          <span class="grow"></span>
          <DiffStat add={f.add} del={f.del} />
        </button>
        {#if !collapsedFiles[f.path]}
          <div class="hunks">
            {#each f.hunks as h, hi (hi)}
              <div class="hh">{h.header}</div>
              {#each h.lines as l, li (li)}
                <div
                  class="dl {l.kind}"
                  class:sel={inSelection(f.path, lineNo(l))}
                  role="button"
                  tabindex="-1"
                  onclick={(e) => clickLine(f.path, lineNo(l), e)}
                  onkeydown={() => {}}
                >
                  <span class="plus" aria-hidden="true">+</span>
                  <span class="no">{l.old_no ?? ""}</span>
                  <span class="no">{l.new_no ?? ""}</span>
                  <span class="sign">{l.kind === "add" ? "+" : l.kind === "del" ? "−" : " "}</span>
                  <span class="txt">{l.text}</span>
                </div>
              {/each}
            {/each}
          </div>
          {#if composerAt === f.path && selFrom !== null}
            <div class="cbox">
              <div class="crange mono">{f.path}:{selFrom}{selTo !== selFrom ? `–${selTo}` : ""} · shift+клик — диапазон</div>
              <textarea
                bind:value={commentText}
                rows="2"
                placeholder="Комментарий агенту к этим строкам…"
                onkeydown={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    addComment();
                  }
                }}
              ></textarea>
              <div class="cbar">
                <Button variant="ghost" onclick={() => { composerAt = null; selFile = null; selFrom = selTo = null; }}>Отмена</Button>
                <Button variant="primary" onclick={addComment}>Добавить в пачку ⏎</Button>
              </div>
            </div>
          {/if}
        {/if}
      </div>
    {/each}
  {/if}

  {#if pending.length}
    <div class="batch glass-rim">
      <span><b>{pending.length}</b> комм. в пачке</span>
      {#each pending as c, i (i)}
        <span class="chip mono" data-tip={c.text} aria-label={c.text}>{c.file.split("/").pop()}:{c.from}{c.to !== c.from ? `–${c.to}` : ""}
          <button class="x" aria-label="Убрать" onclick={() => (pending = pending.filter((_, j) => j !== i))}>×</button>
        </span>
      {/each}
      <span class="grow"></span>
      <Button variant="ghost" onclick={() => (pending = [])}>Очистить</Button>
      <Button variant="primary" onclick={sendAll}>Отправить всё агенту</Button>
    </div>
  {/if}
</div>

<style>
  .diffwrap { display: flex; flex-direction: column; gap: 10px; padding: 14px 16px 90px; overflow-y: auto; flex: 1; }
  .empty { color: var(--text-muted); }
  .mono { font-family: var(--font-mono); font-size: 11.5px; }
  .dfile { border-radius: var(--r-lg); background: var(--surface-1); overflow: hidden; }
  .dhead {
    display: flex; align-items: center; gap: 8px; width: 100%;
    background: var(--surface-2); border: 0; cursor: pointer;
    padding: 7px 10px; color: var(--text-primary);
    font: 500 12.5px var(--font-mono); text-align: left;
  }
  .chev { font-size: 9px; color: var(--text-muted); }
  .path { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .grow { flex: 1; }
  .st-added { color: var(--diff-add); font-size: 11px; }
  .st-deleted { color: var(--diff-del); font-size: 11px; }
  .st-renamed { color: var(--text-muted); font-size: 11px; }
  .hunks { display: flex; flex-direction: column; }
  .hh { font: 11px var(--font-mono); color: var(--text-muted); background: var(--surface-0); padding: 3px 10px; }
  .dl {
    display: grid;
    grid-template-columns: 20px 40px 40px 14px 1fr;
    gap: 6px;
    padding: 0 10px 0 4px;
    font: 12px var(--font-mono);
    line-height: 1.7;
    color: var(--text-secondary);
    white-space: pre;
  }
  .dl .no { color: var(--text-disabled); text-align: right; user-select: none; }
  .dl .sign { color: var(--text-muted); user-select: none; }
  .dl.add { background: var(--diff-add-bg); }
  .dl.add .txt { color: var(--diff-add); }
  .dl.del { background: var(--diff-del-bg); }
  .dl.del .txt { color: var(--diff-del); }
  /* GitHub-style: a blue "+" appears in the gutter on hover — the row itself
     is plain text (selectable); selection is a soft merged band, not per-row rings */
  .dl { cursor: pointer; }
  .plus {
    visibility: hidden;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 4px;
    background: var(--accent);
    color: var(--on-accent);
    font: 700 12px var(--font-ui);
    line-height: 1;
    width: 16px;
    height: 16px;
    align-self: center;
  }
  .dl:hover .plus { visibility: visible; }
  .dl.sel {
    background: var(--accent-soft);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .dl.sel .plus { visibility: visible; }
  .txt { overflow: hidden; text-overflow: ellipsis; }
  .cbox { padding: 10px; background: var(--surface-2); }
  .crange { color: var(--text-muted); margin-bottom: 6px; }
  .cbox textarea {
    width: 100%; border: 0; border-radius: var(--r-md);
    background: var(--surface-1); color: var(--text-primary);
    font: 13px var(--font-ui); padding: 8px 10px; resize: vertical; outline: none;
  }
  .cbar { display: flex; justify-content: flex-end; gap: 8px; margin-top: 8px; }
  .batch {
    position: sticky;
    bottom: 10px;
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    background: var(--surface-3);
    border-radius: var(--r-lg);
    padding: 8px 12px;
    box-shadow: 0 12px 32px oklch(0% 0 0 / 0.45);
  }
  .chip {
    display: inline-flex; align-items: center; gap: 5px;
    background: var(--surface-2); border-radius: 999px; padding: 2px 9px;
    color: var(--text-secondary);
  }
  .chip .x { background: none; border: 0; color: var(--text-muted); cursor: pointer; font-size: 13px; padding: 0; }
  .chip .x:hover { color: var(--diff-del); }
</style>
