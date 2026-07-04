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
    onTasksChanged,
    isDemo,
    type Project,
    type Task,
  } from "$lib/api";

  let projects: Project[] = $state([]);
  let project: Project | undefined = $state();
  let tasks: Task[] = $state([]);
  let selected: Task | undefined = $state();

  // ⌘N creation modal with a persistent draft (ui-inventory: drafts never die)
  let createOpen = $state(false);
  let prompt = $state("");
  let creating = $state(false);

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
    onTasksChanged(() => {
      creating = false;
      reload();
    }).then((u) => (un = u));

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
        <Badge status={selected.status} />
        <span class="branch">{selected.branch}</span>
      </div>
      <div class="center-empty">
        <p>Тред агента — следующий шаг фазы 3b.</p>
        <p class="mut">Worktrees на месте: <span class="branch">.gcode/tasks/{selected.slug}/</span></p>
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
  h3 { margin: 0 0 4px; font-size: 15px; }
</style>
