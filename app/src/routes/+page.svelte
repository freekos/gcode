<script lang="ts">
  import { onMount } from "svelte";
  import TaskRow from "$lib/components/TaskRow.svelte";
  import Button from "$lib/components/Button.svelte";
  import Badge from "$lib/components/Badge.svelte";
  import Kbd from "$lib/components/Kbd.svelte";
  import Modal from "$lib/components/Modal.svelte";
  import {
    projectsList,
    tasksList,
    taskCreate,
    threadSend,
    onThreadEvent,
    onTasksChanged,
    isDemo,
    taskContext,
    projectAdd,
    pickFolder,
    type Project,
    type Task,
    type ThreadEvent,
    type TaskContext,
  } from "$lib/api";
  import DiffStat from "$lib/components/DiffStat.svelte";

  type ThreadItem = { kind: "user" | "agent" | "tool" | "error"; text: string };
  type ThreadState = { items: ThreadItem[]; running: boolean; queue: string[] };

  type ProjectNode = { project: Project; tasks: Task[] };
  let tree: ProjectNode[] = $state([]);
  let project: Project | undefined = $state(); // context for ⌘N
  let selected: Task | undefined = $state();

  const PRIORITY: Record<string, number> = { needs_input: 0, review: 1, running: 2, new: 3, done: 4 };
  function sortTasks(ts: Task[]): Task[] {
    return [...ts].sort((a, b) => (PRIORITY[a.status] ?? 9) - (PRIORITY[b.status] ?? 9));
  }

  // ⌘N creation modal with a persistent draft (ui-inventory: drafts never die)
  let createOpen = $state(false);
  let prompt = $state("");
  let creating = $state(false);

  // per-task thread view state (history persistence comes later — engine owns transcripts)
  let threads: Record<number, ThreadState> = $state({});
  let ctx: TaskContext | undefined = $state();
  let limit: { kind: string; resetsAt: number } | undefined = $state();
  let paletteOpen = $state(false);
  let collapsed: Record<number, boolean> = $state({});

  function toggleProject(id: number) {
    collapsed[id] = !collapsed[id];
    localStorage.setItem("gcode.sidebar.collapsed", JSON.stringify(collapsed));
  }
  let palQ = $state("");
  let addProjOpen = $state(false);
  let projPath = $state("");
  let projErr = $state("");
  let msg = $state("");
  let threadBox: HTMLElement | undefined = $state();

  // mutating accessor — handlers only (never call from the template: Svelte
  // forbids state mutation inside template expressions)
  function th(id: number): ThreadState {
    if (!threads[id]) threads[id] = { items: [], running: false, queue: [] };
    return threads[id];
  }
  const EMPTY: ThreadState = { items: [], running: false, queue: [] };
  $effect(() => {
    if (selected) loadCtx();
  });

  function fmtReset(ts: number): string {
    const d = new Date(ts * 1000);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }
  const cur = $derived(selected ? (threads[selected.id] ?? EMPTY) : EMPTY);

  function scrollDown() {
    requestAnimationFrame(() => threadBox?.scrollTo({ top: threadBox.scrollHeight }));
  }

  function sendMsg() {
    if (!selected || !msg.trim()) return;
    const t = th(selected.id);
    if (t.running) {
      t.queue.push(msg.trim());
    } else {
      fire(selected.id, msg.trim());
    }
    msg = "";
  }

  function fire(taskId: number, prompt: string) {
    const t = th(taskId);
    t.items.push({ kind: "user", text: prompt });
    t.running = true;
    scrollDown();
    threadSend(taskId, prompt);
  }

  async function loadCtx() {
    if (selected) ctx = await taskContext(selected.id);
  }

  function onEvent(e: ThreadEvent) {
    const t = th(e.task_id);
    if (e.kind === "limit") {
      if (e.resets_at) limit = { kind: e.text, resetsAt: e.resets_at };
      return;
    }
    if (e.kind === "delta") {
      const last = t.items[t.items.length - 1];
      if (last?.kind === "agent") last.text += e.text;
      else t.items.push({ kind: "agent", text: e.text });
    } else if (e.kind === "tool") {
      t.items.push({ kind: "tool", text: e.text });
    } else if (e.kind === "done") {
      t.running = false;
      if (e.ok === false) t.items.push({ kind: "error", text: e.text || "агент завершился с ошибкой" });
      loadCtx();
      const next = t.queue.shift();
      if (next) fire(e.task_id, next);
    }
    if (selected?.id === e.task_id) scrollDown();
  }

  const ordered = $derived(tree.flatMap((n) => n.tasks));

  async function reload() {
    const projects = await projectsList();
    tree = await Promise.all(
      projects.map(async (p) => ({ project: p, tasks: sortTasks(await tasksList(p.id)) })),
    );
    project = project ?? projects[0];
    if (selected) {
      const all = tree.flatMap((n) => n.tasks);
      selected = all.find((t) => t.id === selected!.id) ?? selected;
    }
  }

  onMount(() => {
    prompt = localStorage.getItem("gcode.draft.newtask") ?? "";
    try {
      collapsed = JSON.parse(localStorage.getItem("gcode.sidebar.collapsed") ?? "{}");
    } catch {
      collapsed = {};
    }
    reload();
    let un: (() => void) | undefined;
    let unThread: (() => void) | undefined;
    onTasksChanged(() => {
      creating = false;
      reload();
    }).then((u) => (un = u));
    onThreadEvent(onEvent).then((u) => (unThread = u));

    const onkey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        createOpen = true;
      }
      if (e.metaKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        palQ = "";
        paletteOpen = true;
      }
      if (e.metaKey && /^[1-9]$/.test(e.key)) {
        e.preventDefault();
        const t = ordered[Number(e.key) - 1];
        if (t) selected = t;
      }
    };
    window.addEventListener("keydown", onkey);
    return () => {
      window.removeEventListener("keydown", onkey);
      un?.();
      unThread?.();
    };
  });

  function saveDraft() {
    localStorage.setItem("gcode.draft.newtask", prompt);
  }

  async function submitCreate() {
    if (!project || !prompt.trim()) return;
    creating = true;
    await taskCreate(project.id, prompt.trim());
    prompt = "";
    localStorage.removeItem("gcode.draft.newtask");
    createOpen = false;
  }

  function hotkeyOf(t: Task): string | undefined {
    const i = ordered.findIndex((x) => x.id === t.id);
    return i >= 0 && i < 9 ? `⌘${i + 1}` : undefined;
  }

  function pick(t: Task, p: Project) {
    selected = t;
    project = p;
  }

  async function addProject() {
    // native folder picker; the path-input modal is only the demo/error fallback
    const picked = await pickFolder();
    if (picked === null && !isDemo) return; // cancelled
    if (picked) {
      try {
        project = await projectAdd(picked);
        await reload();
        return;
      } catch (e) {
        projErr = String(e);
        projPath = picked;
        addProjOpen = true;
        return;
      }
    }
    projErr = "";
    addProjOpen = true;
  }

  async function submitAddProject() {
    projErr = "";
    try {
      const p = await projectAdd(projPath.trim());
      addProjOpen = false;
      projPath = "";
      project = p;
      await reload();
    } catch (e) {
      projErr = String(e);
    }
  }
</script>

<svelte:head><title>gcode{project ? ` · ${project.name}` : ""}</title></svelte:head>

<div class="layout" class:with-ctx={!!selected}>
  <aside>
    <div class="proj">
      <span class="pname">g<b style="color:var(--accent)">code</b></span>
      {#if isDemo}<span class="demo">demo</span>{/if}
    </div>

    <button class="newtask" onclick={() => (createOpen = true)}>
      <span class="plus">＋</span> Новая задача
      <span class="hk-static"><Kbd keys="⌘N" /></span>
    </button>

    {#if tree.length === 0}
      <div class="empty-side">
        <p>Проектов пока нет.</p>
        <Button variant="primary" onclick={addProject}>+ Добавить проект</Button>
      </div>
    {:else}
      {#each tree as node (node.project.id)}
        <div class="pnode">
          <button class="phead" onclick={() => toggleProject(node.project.id)}>
            <span class="chev" class:closed={collapsed[node.project.id]}>▾</span>
            <span class="pname2" class:pactive={project?.id === node.project.id}>{node.project.name}</span>
            <span class="pmeta">{node.tasks.length}</span>
          </button>
          {#if !collapsed[node.project.id]}
            <div class="plist">
              {#if node.tasks.length === 0}
                <p class="mut" style="margin:2px 8px 6px">нет задач</p>
              {:else}
                {#each node.tasks as t (t.id)}
                  <TaskRow title={t.title} status={t.status} hotkey={hotkeyOf(t)} active={selected?.id === t.id} onclick={() => pick(t, node.project)} />
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      <button class="newtask addproj" onclick={addProject}>
        <span class="plus">＋</span> проект
      </button>
    {/if}
  </aside>

  <main>
    {#if creating}
      <div class="center-empty">
        <p class="spin">◐</p>
        <p>Готовлю worktrees…</p>
      </div>
    {:else if selected}
      <div class="thread-head">
        <b>{selected.title}</b>
        <Badge status={cur.running ? "running" : selected.status} />
        <span class="branch">{selected.branch}</span>
      </div>
      <div class="thread-box" bind:this={threadBox}>
        {#if cur.items.length === 0}
          <div class="center-empty">
            <p class="mut">Скажи агенту, что делать — worktrees уже готовы:</p>
            <p class="branch">.gcode/tasks/{selected.slug}/</p>
          </div>
        {:else}
          {#each cur.items as it, i (i)}
            {#if it.kind === "user"}
              <div class="m-user">{it.text}</div>
            {:else if it.kind === "agent"}
              <div class="m-agent">{it.text}</div>
            {:else if it.kind === "tool"}
              <div class="m-tool">[{it.text}]</div>
            {:else}
              <div class="m-err">{it.text}</div>
            {/if}
          {/each}
        {/if}
      </div>
      <div class="composer">
        <textarea
          bind:value={msg}
          rows="2"
          placeholder={cur.running ? "Агент работает — сообщение уйдёт следующим…" : "Сообщение агенту…"}
          onkeydown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              sendMsg();
            }
          }}
        ></textarea>
        <div class="c-bar">
          {#if cur.running}
            <span class="queue-note">◐ агент работает{cur.queue.length ? ` · в очереди: ${cur.queue.length}` : ""}</span>
          {/if}
          <span style="flex:1"></span>
          <Button variant="primary" onclick={sendMsg}>{cur.running ? "В очередь" : "Отправить"} ⏎</Button>
        </div>
        <div class="e-bar">
          <span class="engine">◆ Claude</span>
          {#if limit}
            <span class="mut mono-s">↻ {limit.kind === "five_hour" ? "5h" : limit.kind} · сброс {fmtReset(limit.resetsAt)}</span>
          {/if}
        </div>
      </div>
    {:else}
      <div class="center-empty">
        <p class="logo">g<b>code</b></p>
        <p class="mut">Выбери задачу слева или создай новую — <Kbd keys="⌘N" /></p>
      </div>
    {/if}
  </main>
  {#if selected}
    <aside class="ctx">
      <div class="grp" style="margin-top:2px">Worktrees · тронутые</div>
      {#if ctx && ctx.touched.length}
        {#each ctx.touched as r (r.repo)}
          <div class="repo">
            <div class="rn">{r.repo}</div>
            <div class="rrow">
              <span class="mut">✎ {r.files}</span>
              <DiffStat add={r.add} del={r.del} />
            </div>
          </div>
        {/each}
      {:else}
        <p class="mut" style="margin:4px 2px">пока без изменений</p>
      {/if}
      {#if ctx && ctx.untouched}
        <p class="mut" style="margin:4px 2px">ещё {ctx.untouched} не тронуты ›</p>
      {/if}

      {#if ctx && ctx.progress.length}
        <div class="grp">Goal · из PROGRESS.md</div>
        <ul class="goal">
          {#each ctx.progress as p, i (i)}
            <li class:done={p.done}>{p.text}</li>
          {/each}
        </ul>
      {/if}
    </aside>
  {/if}
  <div class="statusbar">
    <span>{tree.length} проектов · {ordered.length} задач</span>
    <span>● {Object.values(threads).filter((t) => t.running).length} агентов работают</span>
    <span style="margin-left:auto">gcode 0.1</span>
  </div>
</div>

<Modal bind:open={addProjOpen} width="520px">
  <h3>Добавить проект</h3>
  <p class="mut" style="margin:0 0 12px">Путь к папке, где лежат git-репозитории проекта (или к одному репо).</p>
  <input
    class="pal-input mono-input"
    placeholder="/Users/you/Codebase/work/azi"
    bind:value={projPath}
    onkeydown={(e) => {
      if (e.key === "Enter") submitAddProject();
    }}
  />
  {#if projErr}<p class="perr">{projErr}</p>{/if}
  <div class="modal-bar">
    <span style="flex:1"></span>
    <Button variant="ghost" onclick={() => (addProjOpen = false)}>Закрыть</Button>
    <Button variant="primary" onclick={submitAddProject}>Добавить</Button>
  </div>
</Modal>

<Modal bind:open={paletteOpen} width="480px">
  <input
    class="pal-input"
    placeholder="Команда или задача…"
    bind:value={palQ}
  />
  <div class="pal-list">
    {#each [
      { label: "Новая задача", hint: "⌘N", act: () => { paletteOpen = false; createOpen = true; } },
      { label: "Добавить проект", hint: "", act: () => { paletteOpen = false; addProject(); } },
      { label: "Styleguide", hint: "", act: () => { paletteOpen = false; window.location.href = "/styleguide"; } },
      ...ordered.map((t) => ({ label: t.title, hint: hotkeyOf(t) ?? "", act: () => { paletteOpen = false; selected = t; } })),
    ].filter((c) => c.label.toLowerCase().includes(palQ.toLowerCase())) as c (c.label)}
      <button class="pal-item" onclick={c.act}>
        <span>{c.label}</span>
        {#if c.hint}<Kbd keys={c.hint} />{/if}
      </button>
    {/each}
  </div>
</Modal>

<Modal bind:open={createOpen} width="560px">
  <h3>Новая задача{project ? ` · ${project.name}` : ""}</h3>
  <p class="mut" style="margin:0 0 12px">Опиши, что сделать — имя придумается само. Worktrees: все репо проекта.</p>
  <textarea
    bind:value={prompt}
    oninput={saveDraft}
    placeholder="Например: почини редирект после логина, ломается на вебвью"
    rows="3"
    onkeydown={(e) => {
      if (e.metaKey && e.key === "Enter") submitCreate();
    }}
  ></textarea>
  <div class="modal-bar">
    <span class="mut" style="font-size:11.5px">черновик сохраняется · ⌘⏎ — создать</span>
    <span style="flex:1"></span>
    <Button variant="ghost" onclick={() => (createOpen = false)}>Закрыть</Button>
    <Button variant="primary" onclick={submitCreate}>Создать</Button>
  </div>
</Modal>

<style>
  .layout {
    display: grid;
    grid-template-columns: 260px 1fr;
    grid-template-rows: 1fr auto;
    height: 100vh;
  }
  .layout.with-ctx { grid-template-columns: 260px 1fr 230px; }
  .statusbar {
    grid-column: 1 / -1;
    display: flex;
    gap: 16px;
    align-items: center;
    border-top: 1px solid var(--border-subtle);
    padding: 4px 14px;
    font-family: var(--font-mono);
    font-size: 11px;
    color: var(--text-muted);
  }
  .addproj { margin-top: 4px; }
  .mono-input { font-family: var(--font-mono); font-size: 12.5px; }
  .perr { color: var(--diff-del); font-size: 12px; margin: 6px 0 0; }
  aside {
    background: var(--surface-1);
    border-right: 1px solid var(--border-subtle);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
  }
  .proj { display: flex; align-items: center; gap: 8px; padding: 2px 4px 10px; }
  .pname { font-weight: 700; font-size: 14px; font-family: var(--font-mono); }
  .demo {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--status-running);
    border: 1px solid color-mix(in oklab, var(--status-running) 40%, transparent);
    border-radius: 999px;
    padding: 0 7px;
  }
  .newtask {
    display: flex; align-items: center; gap: 8px;
    background: transparent; border: 0; cursor: pointer;
    color: var(--text-secondary); font: 500 12.5px var(--font-ui);
    padding: 6px 8px; border-radius: var(--r-md); text-align: left;
    transition: background var(--t-fast) ease-out, color var(--t-fast) ease-out;
    margin-bottom: 6px;
  }
  .newtask:hover { background: var(--surface-2); color: var(--text-primary); }
  .newtask .plus { color: var(--accent); font-weight: 700; }
  .hk-static { margin-left: auto; }
  .pnode { display: flex; flex-direction: column; margin-bottom: 4px; }
  .phead {
    display: flex; align-items: center; gap: 6px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    padding: 5px 6px; border-radius: var(--r-sm);
    color: var(--text-primary); width: 100%;
  }
  .phead:hover { background: var(--surface-2); }
  .chev {
    font-size: 9px; color: var(--text-muted); width: 12px;
    transition: transform var(--t-fast) ease-out;
  }
  .chev.closed { transform: rotate(-90deg); }
  .pname2 { font-weight: 600; font-size: 12px; letter-spacing: .01em; }
  .pname2.pactive { color: var(--accent); }
  .pmeta { margin-left: auto; font-size: 10.5px; color: var(--text-muted); font-family: var(--font-mono); }
  .plist { display: flex; flex-direction: column; gap: 1px; padding-left: 10px; margin-top: 2px; }
  .empty-side { color: var(--text-muted); text-align: center; margin-top: 40px; }
  main { display: flex; flex-direction: column; overflow: hidden; }
  .thread-head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 20px;
    border-bottom: 1px solid var(--border-subtle);
  }
  .branch { font-family: var(--font-mono); font-size: 11px; color: var(--text-muted); }
  .center-empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-secondary);
  }
  .logo { font-family: var(--font-mono); font-weight: 700; font-size: 22px; color: var(--text-primary); }
  .logo b { color: var(--accent); }
  .mut { color: var(--text-muted); font-size: 12px; }
  .spin { font-size: 20px; animation: gc-spin 1s linear infinite; }
  @keyframes gc-spin { to { transform: rotate(360deg); } }
  textarea {
    width: 100%;
    font: 13px var(--font-ui);
    color: var(--text-primary);
    background: var(--surface-1);
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-md);
    padding: 10px 12px;
    resize: vertical;
  }
  textarea:focus { outline: none; border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
  .modal-bar { display: flex; align-items: center; gap: 8px; margin-top: 10px; }
  .thread-box { flex: 1; overflow-y: auto; padding: 18px 22px; display: flex; flex-direction: column; gap: 10px; }
  .m-user {
    align-self: flex-end;
    background: var(--surface-2);
    border-radius: var(--r-lg);
    padding: 8px 12px;
    max-width: 80%;
  }
  .m-agent { color: var(--text-primary); max-width: 92%; white-space: pre-wrap; }
  .m-tool { font-family: var(--font-mono); font-size: 12px; color: var(--text-muted); }
  .m-err { color: var(--diff-del); font-size: 12.5px; }
  .composer { border-top: 1px solid var(--border-subtle); padding: 12px 16px; }
  .composer textarea { margin-bottom: 0; }
  .c-bar { display: flex; align-items: center; gap: 10px; margin-top: 8px; }
  .queue-note { font-size: 11.5px; color: var(--status-running); }
  .e-bar { display: flex; gap: 12px; align-items: center; margin-top: 8px; padding-top: 8px; border-top: 1px dashed var(--border-subtle); }
  .engine {
    font-size: 11.5px; color: var(--text-secondary);
    background: var(--surface-2); border: 1px solid var(--border-subtle);
    border-radius: 999px; padding: 2px 10px;
  }
  .mono-s { font-family: var(--font-mono); font-size: 11px; }
  .ctx { background: var(--surface-1); border-left: 1px solid var(--border-subtle); padding: 12px; overflow-y: auto; }
  .repo { background: var(--surface-2); border-radius: var(--r-md); padding: 8px 10px; margin-bottom: 8px; }
  .rn { font-family: var(--font-mono); font-size: 11.5px; }
  .rrow { display: flex; gap: 10px; margin-top: 3px; align-items: center; }
  .goal { margin: 4px 0 0; padding-left: 18px; }
  .goal li { color: var(--text-secondary); margin: 3px 0; font-size: 12.5px; }
  .goal li.done { color: var(--text-muted); text-decoration: line-through; }
  .pal-input {
    width: 100%; font: 14px var(--font-ui); color: var(--text-primary);
    background: var(--surface-2); border: 1px solid var(--border-subtle);
    border-radius: var(--r-md); padding: 9px 12px; margin-bottom: 10px;
  }
  .pal-input:focus { outline: none; border-color: var(--accent); }
  .pal-list { display: flex; flex-direction: column; gap: 2px; max-height: 300px; overflow-y: auto; }
  .pal-item {
    display: flex; justify-content: space-between; align-items: center;
    background: transparent; border: 0; color: var(--text-primary);
    font: 13px var(--font-ui); padding: 8px 10px; border-radius: var(--r-md);
    cursor: pointer; text-align: left;
  }
  .pal-item:hover { background: var(--surface-2); }
  h3 { margin: 0 0 4px; font-size: 15px; }
</style>
