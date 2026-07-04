<script lang="ts">
  import { onMount } from "svelte";
  import TaskCard from "$lib/components/TaskCard.svelte";
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
    type Project,
    type Task,
    type ThreadEvent,
  } from "$lib/api";

  type ThreadItem = { kind: "user" | "agent" | "tool" | "error"; text: string };
  type ThreadState = { items: ThreadItem[]; running: boolean; queue: string[] };

  let projects: Project[] = $state([]);
  let project: Project | undefined = $state();
  let tasks: Task[] = $state([]);
  let selected: Task | undefined = $state();

  // ⌘N creation modal with a persistent draft (ui-inventory: drafts never die)
  let createOpen = $state(false);
  let prompt = $state("");
  let creating = $state(false);

  // per-task thread view state (history persistence comes later — engine owns transcripts)
  let threads: Record<number, ThreadState> = $state({});
  let msg = $state("");
  let threadBox: HTMLElement | undefined = $state();

  // mutating accessor — handlers only (never call from the template: Svelte
  // forbids state mutation inside template expressions)
  function th(id: number): ThreadState {
    if (!threads[id]) threads[id] = { items: [], running: false, queue: [] };
    return threads[id];
  }
  const EMPTY: ThreadState = { items: [], running: false, queue: [] };
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

  function onEvent(e: ThreadEvent) {
    const t = th(e.task_id);
    if (e.kind === "delta") {
      const last = t.items[t.items.length - 1];
      if (last?.kind === "agent") last.text += e.text;
      else t.items.push({ kind: "agent", text: e.text });
    } else if (e.kind === "tool") {
      t.items.push({ kind: "tool", text: e.text });
    } else if (e.kind === "done") {
      t.running = false;
      if (e.ok === false) t.items.push({ kind: "error", text: e.text || "агент завершился с ошибкой" });
      const next = t.queue.shift();
      if (next) fire(e.task_id, next);
    }
    if (selected?.id === e.task_id) scrollDown();
  }

  const attention = $derived(tasks.filter((t) => t.status === "needs_input" || t.status === "review"));
  const working = $derived(tasks.filter((t) => t.status === "running"));
  const rest = $derived(tasks.filter((t) => !["needs_input", "review", "running"].includes(t.status)));
  const ordered = $derived([...attention, ...working, ...rest]);

  async function reload() {
    projects = await projectsList();
    project = project ?? projects[0];
    if (project) {
      tasks = await tasksList(project.id);
      if (selected) selected = tasks.find((t) => t.id === selected!.id) ?? selected;
    }
  }

  onMount(() => {
    prompt = localStorage.getItem("gcode.draft.newtask") ?? "";
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
</script>

<svelte:head><title>gcode{project ? ` · ${project.name}` : ""}</title></svelte:head>

<div class="layout">
  <aside>
    <div class="proj">
      <span class="pname">{project?.name ?? "—"}</span>
      {#if isDemo}<span class="demo">demo</span>{/if}
    </div>

    {#if tasks.length === 0}
      <div class="empty-side">
        <p>Задач пока нет.</p>
        <p><Kbd keys="⌘N" /> — первая задача</p>
      </div>
    {:else}
      {#if attention.length}
        <div class="grp">⏳ Требуют внимания</div>
        {#each attention as t (t.id)}
          <TaskCard title={t.title} status={t.status} hotkey={hotkeyOf(t)} active={selected?.id === t.id} onclick={() => (selected = t)} />
        {/each}
      {/if}
      {#if working.length}
        <div class="grp">⚙ Работают</div>
        {#each working as t (t.id)}
          <TaskCard title={t.title} status={t.status} hotkey={hotkeyOf(t)} active={selected?.id === t.id} onclick={() => (selected = t)} />
        {/each}
      {/if}
      {#if rest.length}
        <div class="grp">Остальные</div>
        {#each rest as t (t.id)}
          <TaskCard title={t.title} status={t.status} hotkey={hotkeyOf(t)} active={selected?.id === t.id} onclick={() => (selected = t)} />
        {/each}
      {/if}
    {/if}

    <div class="side-bottom">
      <Button variant="primary" onclick={() => (createOpen = true)}>+ Задача</Button>
    </div>
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
      </div>
    {:else}
      <div class="center-empty">
        <p class="logo">g<b>code</b></p>
        <p class="mut">Выбери задачу слева или создай новую — <Kbd keys="⌘N" /></p>
      </div>
    {/if}
  </main>
</div>

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
    height: 100vh;
  }
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
  .pname { font-weight: 700; font-size: 14px; }
  .demo {
    font-family: var(--font-mono);
    font-size: 10px;
    color: var(--status-running);
    border: 1px solid color-mix(in oklab, var(--status-running) 40%, transparent);
    border-radius: 999px;
    padding: 0 7px;
  }
  .grp {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-muted);
    margin: 10px 4px 2px;
  }
  .empty-side { color: var(--text-muted); text-align: center; margin-top: 40px; }
  .side-bottom { margin-top: auto; padding-top: 12px; }
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
  h3 { margin: 0 0 4px; font-size: 15px; }
</style>
