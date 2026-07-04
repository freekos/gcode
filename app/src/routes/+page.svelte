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
    ago,
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
    if (sortBy === "created") {
      return [...ts].sort((a, b) => b.created_at.localeCompare(a.created_at));
    }
    return [...ts].sort((a, b) => (PRIORITY[a.status] ?? 9) - (PRIORITY[b.status] ?? 9));
  }

  // ⌘N creation modal with a persistent draft (ui-inventory: drafts never die)
  let createOpen = $state(false);
  let prompt = $state("");
  let creating = $state(false);
  let pendingPrompt: string | null = $state(null);

  // per-task thread view state (history persistence comes later — engine owns transcripts)
  let threads: Record<number, ThreadState> = $state({});
  let ctx: TaskContext | undefined = $state();
  let limit: { kind: string; resetsAt: number } | undefined = $state();
  let paletteOpen = $state(false);
  let collapsed: Record<number, boolean> = $state({});
  let sbw = $state(260);
  let showArchived = $state(false);
  let sortBy: "status" | "created" = $state("status");
  let viewMenuOpen = $state(false);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = sbw;
    const move = (ev: PointerEvent) => {
      sbw = Math.min(420, Math.max(200, startW + ev.clientX - startX));
    };
    const up = () => {
      localStorage.setItem("gcode.sidebar.width", String(sbw));
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

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

  type Block = { kind: "msg"; item: ThreadItem } | { kind: "tools"; tools: string[] };
  // consecutive tool events fold into one collapsible block (review: tool spam)
  const blocks = $derived.by(() => {
    const out: Block[] = [];
    for (const it of cur.items) {
      if (it.kind === "tool") {
        const last = out[out.length - 1];
        if (last?.kind === "tools") last.tools.push(it.text);
        else out.push({ kind: "tools", tools: [it.text] });
      } else {
        out.push({ kind: "msg", item: it });
      }
    }
    return out;
  });

  function toolSummary(tools: string[]): string {
    const counts = new Map<string, number>();
    for (const t of tools) {
      const name = t.split(" · ")[0];
      counts.set(name, (counts.get(name) ?? 0) + 1);
    }
    const parts = [...counts.entries()].map(([n, c]) => (c > 1 ? `${n} ×${c}` : n));
    return `${tools.length} шаг${tools.length === 1 ? "" : tools.length < 5 ? "а" : "ов"} · ${parts.join(" · ")}`;
  }

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
      projects.map(async (p) => ({ project: p, tasks: sortTasks(await tasksList(p.id, showArchived)) })),
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
    sbw = Number(localStorage.getItem("gcode.sidebar.width") ?? 260) || 260;
    reload();
    let un: (() => void) | undefined;
    let unThread: (() => void) | undefined;
    onTasksChanged(async (payload) => {
      creating = false;
      await reload();
      // the creation prompt IS the first agent message (review comment #2)
      if (pendingPrompt && payload?.ok && payload?.slug) {
        const node = tree.find((n) => n.tasks.some((t) => t.slug === payload.slug));
        const t = node?.tasks.find((t) => t.slug === payload.slug);
        if (t && node) {
          pick(t, node.project);
          fire(t.id, pendingPrompt);
        }
        pendingPrompt = null;
      }
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

  // central hub composer (ZCode-style empty state): create a task right here
  let hubPrompt = $state("");
  async function submitHub() {
    if (!project || !hubPrompt.trim()) return;
    creating = true;
    pendingPrompt = hubPrompt.trim();
    await taskCreate(project.id, hubPrompt.trim());
    hubPrompt = "";
  }
  function greet(): string {
    const h = new Date().getHours();
    if (h < 6) return "Поздняя смена?";
    if (h < 12) return "Доброе утро";
    if (h < 18) return "Что делаем?";
    return "Добрый вечер";
  }

  async function submitCreate() {
    if (!project || !prompt.trim()) return;
    creating = true;
    pendingPrompt = prompt.trim();
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

<svelte:window
  onclick={(e) => {
    if (viewMenuOpen && !(e.target as HTMLElement).closest(".viewmenu-wrap")) viewMenuOpen = false;
  }}
/>

<svelte:head><title>gcode{project ? ` · ${project.name}` : ""}</title></svelte:head>

<div class="layout" class:with-ctx={!!selected} style="--sbw:{sbw}px">
  <aside>
    <div class="drag-strip" data-tauri-drag-region></div>

    <button class="newtask" onclick={() => (createOpen = true)}>
      <svg class="ic" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.4" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M8 5.2v5.6M5.2 8h5.6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
      Новая задача
      <span class="hk-static"><Kbd keys="⌘N" /></span>
    </button>
    <button class="newtask" onclick={() => { palQ = ""; paletteOpen = true; }}>
      <svg class="ic" viewBox="0 0 16 16"><circle cx="7" cy="7" r="4.6" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="m10.5 10.5 3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
      Поиск
      <span class="hk-static"><Kbd keys="⌘K" /></span>
    </button>

    <div class="sb-head">
      <span class="seg">
        <svg class="ic" style="width:12px;height:12px" viewBox="0 0 16 16"><path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h3l1.4 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
        Проекты
      </span>
      <button class="iconbtn" title="Добавить проект" onclick={addProject}>
        <svg class="ic" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
      </button>
      <span style="flex:1"></span>
      <div class="viewmenu-wrap">
        <button class="iconbtn" title="Вид и сортировка" onclick={() => (viewMenuOpen = !viewMenuOpen)}>
          <svg class="ic" viewBox="0 0 16 16"><path d="M2.5 4.5h11M4.5 8h7M6.5 11.5h3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
        </button>
        {#if viewMenuOpen}
          <div class="viewmenu" role="menu">
            <div class="vm-sec">Вид</div>
            <button class="vm-item" onclick={() => (viewMenuOpen = false)}>
              По проектам <span class="vm-check">✓</span>
            </button>
            <button class="vm-item" disabled title="скоро">Таймлайн</button>
            <div class="vm-sec">Сортировка</div>
            <button class="vm-item" onclick={() => { sortBy = "status"; viewMenuOpen = false; reload(); }}>
              По статусу {#if sortBy === "status"}<span class="vm-check">✓</span>{/if}
            </button>
            <button class="vm-item" onclick={() => { sortBy = "created"; viewMenuOpen = false; reload(); }}>
              По созданию {#if sortBy === "created"}<span class="vm-check">✓</span>{/if}
            </button>
          </div>
        {/if}
      </div>
      <button class="iconbtn" class:on={showArchived} title="Архивные задачи" onclick={() => { showArchived = !showArchived; reload(); }}>
        <svg class="ic" viewBox="0 0 16 16"><rect x="2" y="3" width="12" height="3.4" rx="1" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M3.2 6.4V12c0 .7.6 1.3 1.3 1.3h7c.7 0 1.3-.6 1.3-1.3V6.4M6.4 9h3.2" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" fill="none"/></svg>
      </button>
    </div>

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
                  <div class:dim-arch={t.archived}>
                    <TaskRow title={t.title} status={t.status} hotkey={hotkeyOf(t)} time={ago(t.created_at)} active={selected?.id === t.id} onclick={() => pick(t, node.project)} />
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
    <div class="sb-bottom">
      <button class="newtask" style="margin:0" title="Настройки · ⌘, (скоро)">
        <svg class="ic" viewBox="0 0 16 16"><circle cx="8" cy="8" r="2.2" fill="none" stroke="currentColor" stroke-width="1.2"/><path d="M8 1.8v2M8 12.2v2M1.8 8h2M12.2 8h2M3.6 3.6l1.4 1.4M11 11l1.4 1.4M12.4 3.6 11 5M5 11l-1.4 1.4" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>
        Настройки
      </button>
      {#if isDemo}<span class="demo">demo</span>{/if}
      <span class="tok" style="margin-left:auto">0.1</span>
    </div>
    <div class="sb-resize" role="separator" aria-orientation="vertical" aria-label="Ширина сайдбара" onpointerdown={startResize}></div>
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
          {#each blocks as b, i (i)}
            {#if b.kind === "tools"}
              <details class="tools">
                <summary>⚙ {toolSummary(b.tools)}</summary>
                <div class="tool-list">
                  {#each b.tools as t, j (j)}
                    <div class="m-tool">{t}</div>
                  {/each}
                </div>
              </details>
            {:else if b.item.kind === "user"}
              <div class="m-user">{b.item.text}</div>
            {:else if b.item.kind === "agent"}
              <div class="m-agent">{b.item.text}</div>
            {:else}
              <div class="m-err">{b.item.text}</div>
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
          <button class="send" onclick={sendMsg} title={cur.running ? "В очередь · ⏎" : "Отправить · ⏎"}>↑</button>
        </div>
        <div class="e-bar">
          <span class="engine">◆ Claude</span>
          {#if limit}
            <span class="mut mono-s">↻ {limit.kind === "five_hour" ? "5h" : limit.kind} · сброс {fmtReset(limit.resetsAt)}</span>
          {/if}
        </div>
      </div>
    {:else}
      <div class="hub">
        <p class="hub-greet">{greet()}</p>
        <div class="hub-box">
          <div class="hub-proj">
            <span class="proj-chip">
              <svg class="ic" style="width:12px;height:12px" viewBox="0 0 16 16"><path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h3l1.4 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
              <select bind:value={project} class="proj-pick">
                {#each tree as n (n.project.id)}
                  <option value={n.project}>{n.project.name}</option>
                {/each}
              </select>
              <span class="chev2">⌄</span>
            </span>
          </div>
          <textarea
            bind:value={hubPrompt}
            rows="3"
            placeholder="Что сделать? Опиши задачу — имя, ветка и worktrees появятся сами"
            onkeydown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                submitHub();
              }
            }}
          ></textarea>
          <div class="hub-bar">
            <span class="mut" style="font-size:11px">worktrees: все репо · git агенту запрещён</span>
            <span style="flex:1"></span>
            <button class="send" onclick={submitHub} title="Создать задачу · ⏎">↑</button>
          </div>
        </div>
        <p class="mut" style="font-size:11px">или выбери задачу слева · <Kbd keys="⌘K" /> — всё остальное</p>
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
      { label: "Новая задача", key: "cmd-new", hint: "⌘N", act: () => { paletteOpen = false; createOpen = true; } },
      { label: "Добавить проект", key: "cmd-addproj", hint: "", act: () => { paletteOpen = false; addProject(); } },
      { label: "Styleguide", key: "cmd-styleguide", hint: "", act: () => { paletteOpen = false; window.location.href = "/styleguide"; } },
      ...ordered.map((t) => ({ label: t.title, key: `task-${t.id}`, hint: hotkeyOf(t) ?? "", act: () => { paletteOpen = false; selected = t; } })),
    ].filter((c) => c.label.toLowerCase().includes(palQ.toLowerCase())) as c, ci (c.key ?? `cmd-${ci}`)}
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
    position: relative;
    z-index: 1;
    display: grid;
    grid-template-columns: var(--sbw, 260px) 1fr;
    grid-template-rows: 1fr;
    height: 100vh;
  }
  /* native: ZCode-style window frame — content inset from the window edge,
     the 8px rim behind is a drag region (win-frame in the layout root) */
  :global(:root.native) .layout {
    margin: 8px;
    height: calc(100vh - 16px);
    border: 1px solid var(--border-subtle);
    border-radius: 14px;
    overflow: hidden;
    box-shadow:
      inset 0 1px 0 var(--glass-highlight),
      0 12px 40px oklch(0% 0 0 / 0.35);
  }
  .layout.with-ctx { grid-template-columns: var(--sbw, 260px) 1fr 230px; }
  .sb-head {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 8px 0 4px;
    position: relative;
  }
  .seg {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: 600 12px var(--font-ui);
    color: var(--text-primary);
    background: var(--surface-3);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 3px 11px;
  }
  .iconbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    background: transparent;
    border: 0;
    border-radius: var(--r-md);
    cursor: pointer;
    color: var(--text-muted);
    transition: background var(--t-fast) ease-out, color var(--t-fast) ease-out;
  }
  .iconbtn:hover { background: var(--surface-2); color: var(--text-primary); }
  .iconbtn.on { color: var(--accent); background: var(--accent-soft); }
  .viewmenu-wrap { position: relative; display: inline-flex; }
  .viewmenu {
    position: absolute;
    top: 30px;
    right: 0;
    z-index: 40;
    min-width: 190px;
    background: var(--surface-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-lg);
    box-shadow: 0 16px 40px oklch(0% 0 0 / 0.45), inset 0 1px 0 var(--glass-highlight);
    padding: 6px;
  }
  .vm-sec {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--text-muted);
    padding: 6px 8px 2px;
  }
  .vm-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    background: transparent;
    border: 0;
    color: var(--text-primary);
    font: 13px var(--font-ui);
    padding: 7px 8px;
    border-radius: var(--r-md);
    cursor: pointer;
    text-align: left;
  }
  .vm-item:hover { background: var(--surface-2); }
  .vm-item:disabled { color: var(--text-disabled); cursor: default; }
  .vm-check { color: var(--accent); }
  .dim-arch { opacity: 0.5; }
  .mono-input { font-family: var(--font-mono); font-size: 12.5px; }
  .drag-strip { height: 26px; flex: none; margin: -12px -12px 0; }
  .sb-bottom {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-top: 8px;
    border-top: 1px solid var(--border-subtle);
  }
  .tok { font-family: var(--font-mono); font-size: 10.5px; color: var(--text-muted); }
  .ic { width: 15px; height: 15px; color: var(--text-muted); flex: none; }
  .newtask:hover .ic { color: var(--text-secondary); }
  .hub {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 18px;
    padding: 0 24px;
  }
  .hub-greet { font-size: 26px; font-weight: 600; letter-spacing: -0.02em; color: var(--text-primary); margin: 0; }
  .hub-box {
    width: min(640px, 100%);
    background: var(--surface-1);
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-xl);
    padding: 12px 14px;
  }
  :global(:root.native) .hub-box { background: var(--surface-2); }
  .hub-box { box-shadow: inset 0 1px 0 var(--glass-highlight); }
  .hub-proj { margin-bottom: 6px; }
  .proj-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--surface-3);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 3px 10px;
    color: var(--text-secondary);
  }
  .proj-pick {
    font: 500 12px var(--font-ui);
    color: var(--text-secondary);
    background: transparent;
    border: 0;
    appearance: none;
    -webkit-appearance: none;
    cursor: pointer;
    padding-right: 2px;
  }
  .proj-pick:focus-visible { outline: none; }
  .proj-chip:focus-within { border-color: var(--accent); }
  .chev2 { font-size: 10px; color: var(--text-muted); margin-top: -2px; }
  .hub-box textarea {
    width: 100%;
    border: 0;
    background: transparent;
    resize: none;
    color: var(--text-primary);
    font: 13.5px var(--font-ui);
    outline: none;
    padding: 4px 2px;
  }
  .hub-bar { display: flex; align-items: center; gap: 10px; margin-top: 4px; }
  .send {
    width: 30px;
    height: 30px;
    border-radius: 8px;
    border: 0;
    background: var(--accent);
    color: var(--on-accent);
    font-size: 15px;
    font-weight: 700;
    cursor: pointer;
    transition: filter var(--t-fast) ease-out, transform var(--t-fast) ease-out;
  }
  .send:hover { filter: brightness(1.1); }
  .send:active { transform: translateY(1px); }
  .send:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .perr { color: var(--diff-del); font-size: 12px; margin: 6px 0 0; }
  aside {
    position: relative;
    background: var(--surface-1);
    border-right: 1px solid var(--border-subtle);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow-y: auto;
  }
  /* native window: vibrancy glass shows through the translucent sidebar */
  :global(:root.native) aside {
    background: color-mix(in oklab, var(--surface-1) 55%, transparent);
  }
  :global(:root.native) main { background: var(--surface-0); }
  .sb-resize {
    position: absolute;
    top: 0;
    right: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    z-index: 20;
  }
  .sb-resize:hover { background: var(--accent-soft); }
  .demo {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--status-running);
    border: 1px solid color-mix(in oklab, var(--status-running) 40%, transparent);
    border-radius: 999px;
    padding: 0 7px;
  }
  .newtask {
    display: flex; align-items: center; gap: 9px;
    background: transparent; border: 0; cursor: pointer;
    color: var(--text-secondary); font: 500 13px var(--font-ui);
    padding: 8px 9px; border-radius: var(--r-md); text-align: left;
    transition: background var(--t-fast) ease-out, color var(--t-fast) ease-out;
    margin-bottom: 2px;
  }
  .newtask:hover { background: var(--surface-2); color: var(--text-primary); }
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
  .m-tool { font-family: var(--font-mono); font-size: 11.5px; color: var(--text-muted); padding: 1px 0; }
  .tools summary {
    cursor: pointer;
    font-family: var(--font-mono);
    font-size: 11.5px;
    color: var(--text-muted);
    padding: 3px 8px;
    background: var(--surface-1);
    border-radius: var(--r-md);
    display: inline-block;
    transition: color var(--t-fast) ease-out, background var(--t-fast) ease-out;
    user-select: none;
  }
  .tools summary:hover { color: var(--text-secondary); background: var(--surface-2); }
  .tools[open] summary { color: var(--text-secondary); }
  .tool-list { padding: 6px 10px 2px; border-left: 2px solid var(--border-subtle); margin: 4px 0 0 8px; }
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
