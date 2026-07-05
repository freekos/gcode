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
    taskPin,
    taskArchive,
    threadSend,
    threadStop,
    threadsList,
    threadNew,
    type ThreadInfo,
    onThreadEvent,
    onTasksChanged,
    onTaskRenamed,
    isDemo,
    taskContext,
    threadHistory,
    taskDiff,
    fileRead,
    fileWrite,
    filesList,
    projectFileRead,
    projectFileWrite,
    projectDirList,
    taskDirList,
    progressRead,
    type DirEntry,
    projectAdd,
    pickFolder,
    revealProject,
    checkUpdate,
    openUrl,
    exportLogs,
    type UpdateInfo,
    type Project,
    type Task,
    type ThreadEvent,
    type TaskContext,
    ago,
  } from "$lib/api";
  import DiffStat from "$lib/components/DiffStat.svelte";
  import DiffView, { type PendingComment, type DiffGroup } from "$lib/components/DiffView.svelte";
  import { autogrow } from "$lib/actions";
  import Editor from "$lib/components/Editor.svelte";
  import FileTree from "$lib/components/FileTree.svelte";
  import Md from "$lib/components/Md.svelte";

  type ThreadItem = { kind: "user" | "agent" | "tool" | "error" | "turn" | "review" | "stopped" | "uquote" | "reply"; text: string };
  type ThreadState = {
    items: ThreadItem[];
    running: boolean;
    queue: string[];
    waiting?: boolean;
    turnStart?: number;
  };

  type ProjectNode = { project: Project; tasks: Task[] };
  let tree: ProjectNode[] = $state([]);
  let projectId: number | undefined = $state(); // context for ⌘N / hub
  const project = $derived(tree.find((n) => n.project.id === projectId)?.project);
  let selected: Task | undefined = $state();

  const PRIORITY: Record<string, number> = { needs_input: 0, review: 1, running: 2, new: 3, done: 4 };
  function sortTasks(ts: Task[]): Task[] {
    const pin = (t: Task) => (t.pinned ? 0 : 1);
    if (sortBy === "created") {
      return [...ts].sort((a, b) => pin(a) - pin(b) || b.created_at.localeCompare(a.created_at));
    }
    return [...ts].sort(
      (a, b) => pin(a) - pin(b) || (PRIORITY[a.status] ?? 9) - (PRIORITY[b.status] ?? 9),
    );
  }

  let creating = $state(false);
  let pendingPrompt: string | null = $state(null);
  // tasks whose AI name is still cooking -> skeleton shimmer on the title
  let naming = $state(new Set<number>());

  // thread view state, keyed by thread key: "t<threadId>" | "new:<taskId>"
  let threads: Record<string, ThreadState> = $state({});
  // threads of each task (for tabs + navigator)
  let taskThreads: Record<number, ThreadInfo[]> = $state({});
  let ctx: TaskContext | undefined = $state();
  let limit: { kind: string; resetsAt: number } | undefined = $state();
  let paletteOpen = $state(false);
  let collapsed: Record<number, boolean> = $state({});
  let sbw = $state(260);
  let ctxw = $state(230); // right panel width (resizable)

  function startCtxResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = ctxw;
    const move = (ev: PointerEvent) => {
      const w = startW + (startX - ev.clientX);
      ctxw = Math.min(420, Math.max(210, w));
    };
    const up = () => {
      localStorage.setItem("gcode.ctx.width", String(ctxw));
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
  let showArchived = $state(false);
  let sortBy: "status" | "created" = $state("status");
  let viewMenuOpen = $state(false);
  let upd: UpdateInfo | undefined = $state();
  let updOpen = $state(false);
  let helpOpen = $state(false);
  let diffSelecting = $state(false); // a line range is being commented

  // ---- tabbed center (phase 4.5): thread / diff / file / md ----
  type Tab =
    | { id: string; kind: "thread"; title: string }
    | { id: string; kind: "diff"; title: string }
    | { id: string; kind: "file"; scope: "task" | "project"; repo: string; path: string; title: string }
    | { id: string; kind: "md"; scope: "task" | "project"; repo: string; path: string; title: string };
  let tabsByTask: Record<number, Tab[]> = $state({});
  let activeByTask: Record<number, string> = $state({});
  // file/md tab contents, keyed by tab id
  let tabContent: Record<string, string> = $state({});

  function tabsOf(taskId: number): Tab[] {
    if (!tabsByTask[taskId]) {
      tabsByTask[taskId] = [{ id: "thread", kind: "thread", title: "Тред" }];
      activeByTask[taskId] = "thread";
    }
    return tabsByTask[taskId];
  }
  const curTabs = $derived(selected ? (tabsByTask[selected.id] ?? [{ id: "thread", kind: "thread", title: "Тред" } as Tab]) : []);
  const activeTab = $derived.by(() => {
    if (!selected) return undefined;
    const id = activeByTask[selected.id] ?? "thread";
    return curTabs.find((t) => t.id === id) ?? curTabs[0];
  });

  function openTab(taskId: number, tab: Tab) {
    const tabs = tabsOf(taskId);
    if (!tabs.some((t) => t.id === tab.id)) tabs.push(tab);
    activeByTask[taskId] = tab.id;
  }
  function closeTab(taskId: number, id: string) {
    if (id === "thread") return; // the thread tab stays
    const tabs = tabsOf(taskId);
    const i = tabs.findIndex((t) => t.id === id);
    if (i >= 0) tabs.splice(i, 1);
    if (activeByTask[taskId] === id) activeByTask[taskId] = tabs[Math.max(0, i - 1)]?.id ?? "thread";
  }
  async function newThread() {
    if (!selected) return;
    const info = await threadNew(selected.id, `Тред ${(taskThreads[selected.id]?.length ?? 0) + 1}`);
    taskThreads[selected.id] = await threadsList(selected.id);
    openTab(selected.id, { id: `t${info.id}`, kind: "thread", title: info.title });
  }

  async function openThreadTab(info: ThreadInfo) {
    if (!selected) return;
    const key = `t${info.id}`;
    openTab(selected.id, { id: key, kind: "thread", title: info.title });
    const st = th(key);
    if (st.items.length === 0 && !st.running) {
      const items = await threadHistory(selected.id, info.id);
      if (st.items.length === 0) st.items = items.map((i) => ({ kind: i.kind, text: i.text }));
    }
  }

  function cycleTab(dir: 1 | -1) {
    if (!selected) return;
    const sid = selected.id;
    const tabs = tabsOf(sid);
    const i = tabs.findIndex((t) => t.id === (activeByTask[sid] ?? "thread"));
    activeByTask[sid] = tabs[(i + dir + tabs.length) % tabs.length].id;
  }
  // sidebar "Show files" mode (ZCode-style): the tree shows the WORKING COPY
  let sbMode: "tasks" | "files" = $state("tasks");
  let filesScope: "project" | "task" = $state("project");
  let filesProject: Project | undefined = $state();
  let filePaletteOpen = $state(false);
  let fileQ = $state("");
  let fileList: string[] = $state([]);

  async function openEditor(repo: string, path: string) {
    if (!selected) return;
    const id = `file:task:${repo}/${path}`;
    let content = "";
    try {
      content = await fileRead(selected.id, repo, path);
    } catch { /* new file */ }
    tabContent[id] = content;
    openTab(selected.id, { id, kind: "file", scope: "task", repo, path, title: path.split("/").pop() ?? path });
  }

  async function openProjectFile(rel: string) {
    if (!filesProject || !selected) return;
    const id = `file:project:${rel}`;
    let content = "";
    try {
      content = await projectFileRead(filesProject.id, rel);
    } catch { /* missing */ }
    tabContent[id] = content;
    openTab(selected.id, { id, kind: "file", scope: "project", repo: "", path: rel, title: rel.split("/").pop() ?? rel });
  }

  // cmd-L attachments: chips above the composer (Cursor-style), payload on send
  let attachments: { loc: string; code: string }[] = $state([]);
  // per-task history of attached quotes (navigator section -> jump to message)
  let attLog: Record<number, { loc: string; midx: number }[]> = $state({});
  // project knowledge files for the navigator (root .md + docs/plan/*)
  let knowledgeFiles: string[] = $state([]);

  async function loadKnowledge() {
    if (!project) { knowledgeFiles = []; return; }
    try {
      const root = await projectDirList(project.id, "");
      const mds = root.filter((e: DirEntry) => !e.is_dir && e.name.toLowerCase().endsWith(".md")).map((e: DirEntry) => e.name);
      let plans: string[] = [];
      if (root.some((e: DirEntry) => e.is_dir && e.name === "docs")) {
        try {
          const docs = await projectDirList(project.id, "docs/plan");
          plans = docs.filter((e: DirEntry) => !e.is_dir && e.name.endsWith(".md")).map((e: DirEntry) => `docs/plan/${e.name}`);
        } catch { /* no docs/plan */ }
      }
      knowledgeFiles = [...mds, ...plans];
    } catch {
      knowledgeFiles = [];
    }
  }

  async function openProgressTab() {
    if (!selected) return;
    const id = "md:progress";
    try {
      tabContent[id] = await progressRead(selected.id);
    } catch {
      tabContent[id] = "PROGRESS.md пока пуст.";
    }
    openTab(selected.id, { id, kind: "md", scope: "task", repo: "", path: "PROGRESS.md", title: "PROGRESS.md" });
  }

  async function openMdTab(scope: "task" | "project", repo: string, path: string) {
    if (!selected) return;
    const id = `md:${scope}:${repo}/${path}`;
    try {
      tabContent[id] =
        scope === "task" ? await fileRead(selected.id, repo, path) : await projectFileRead(project?.id ?? 0, path);
    } catch {
      tabContent[id] = "";
    }
    openTab(selected.id, { id, kind: "md", scope, repo, path, title: path.split("/").pop() ?? path });
  }

  async function openKnowledgeTab(rel: string) {
    if (!selected || !project) return;
    const id = `md:project:${rel}`;
    try {
      tabContent[id] = await projectFileRead(project.id, rel);
    } catch {
      tabContent[id] = "";
    }
    openTab(selected.id, { id, kind: "md", scope: "project", repo: "", path: rel, title: rel.split("/").pop() ?? rel });
  }

  const FILE_RE = /^[\w.@-]+(\/[\w.@-]+)+\.[a-z0-9]+$/i;

  async function tryOpenPath(raw: string) {
    if (!selected) return;
    const text = raw.replace(/:[\d–-]+$/, "").trim();
    if (!FILE_RE.test(text)) return;
    const isMd = text.toLowerCase().endsWith(".md");
    const repos = ctx?.touched.map((r) => r.repo) ?? [];
    const [first, ...rest] = text.split("/");
    const tryOpen = async (repo: string, path: string) => {
      try {
        await fileRead(selected!.id, repo, path);
        if (isMd) await openMdTab("task", repo, path);
        else await openEditor(repo, path);
        return true;
      } catch {
        return false;
      }
    };
    if (repos.includes(first) && (await tryOpen(first, rest.join("/")))) return;
    for (const r of repos) if (await tryOpen(r, text)) return;
    // project-level fallback
    try {
      if (!project) return;
      await projectFileRead(project.id, text);
      if (isMd) await openKnowledgeTab(text);
      else await openProjectFile(text);
    } catch { /* not found anywhere — ignore */ }
  }

  function onThreadClick(e: MouseEvent) {
    const code = (e.target as HTMLElement).closest("code");
    if (code?.textContent) tryOpenPath(code.textContent);
  }

  function jumpToMessage(midx: number) {
    if (!selected) return;
    activeByTask[selected.id] = "thread";
    requestAnimationFrame(() => {
      const el = document.querySelector(`[data-midx="${midx}"]`);
      el?.scrollIntoView({ block: "center" });
      el?.classList.add("flash");
      setTimeout(() => el?.classList.remove("flash"), 1200);
    });
  }

  let replyTo: string | null = $state(null);

  function quoteReply(text: string) {
    replyTo = text;
    if (selected) activeByTask[selected.id] = "thread";
    requestAnimationFrame(() =>
      document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus(),
    );
  }

  // cmd-L: attach a code quote as a chip (the prompt field stays clean)
  function quoteToComposer(loc: string, code: string) {
    attachments = [...attachments, { loc, code }];
    if (selected) activeByTask[selected.id] = "thread";
    requestAnimationFrame(() =>
      document.querySelector<HTMLTextAreaElement>(".composer textarea")?.focus(),
    );
  }

  function saveEditor(tab: Tab, text: string) {
    if (tab.kind !== "file" || !selected) return;
    tabContent[tab.id] = text;
    if (tab.scope === "project") {
      if (filesProject) projectFileWrite(filesProject.id, tab.path, text);
    } else {
      fileWrite(selected.id, tab.repo, tab.path, text);
    }
  }

  async function openFilePalette() {
    if (!selected) return;
    fileList = await filesList(selected.id);
    fileQ = "";
    filePaletteOpen = true;
  }

  const fileMatches = $derived.by(() => {
    const q = fileQ.toLowerCase();
    const scored = fileList.filter((f) => {
      // simple fuzzy: all query chars appear in order
      let i = 0;
      const lf = f.toLowerCase();
      for (const ch of q) {
        i = lf.indexOf(ch, i);
        if (i < 0) return false;
        i++;
      }
      return true;
    });
    return scored.slice(0, 12);
  });
  let diffRepo: string | null = $state(null); // null = все репо
  let diffGroups: DiffGroup[] = $state([]);

  async function openDiff(repo?: string | null) {
    if (!selected) return;
    diffRepo = repo ?? null;
    openTab(selected.id, { id: "diff", kind: "diff", title: "Изменения" });
    const repos = diffRepo ? [diffRepo] : (ctx?.touched.map((r) => r.repo) ?? []);
    const sel = selected;
    diffGroups = await Promise.all(
      repos.map(async (r) => ({ repo: r, files: await taskDiff(sel.id, r) })),
    );
  }

  function sendReview(comments: PendingComment[]) {
    if (!selected) return;
    const fence = "```";
    const parts = comments.map(
      (c) =>
        `**${c.repo}/${c.file}:${c.from}${c.to !== c.from ? `–${c.to}` : ""}**\n${fence}\n${c.code}\n${fence}\n${c.text}`,
    );
    const msg = `Ревью изменений (${comments.length} комм.):\n\n${parts.join("\n\n")}\n\nПоправь по комментариям.`;
    activeByTask[selected.id] = "thread";
    const t = th(activeThreadKey);
    // the human sees a structured card; the agent receives the full markdown
    t.items.push({ kind: "review", text: JSON.stringify(comments) });
    if (t.running) {
      t.queue.push(msg);
    } else {
      t.running = true;
      t.waiting = true;
      t.turnStart = Date.now();
      scrollDown();
      threadSend(selected.id, threadIdOfKey(activeThreadKey), msg);
    }
  }

  function revealCurrent() {
    if (!project) return;
    const path = selected
      ? `${project.path}/.gcode/tasks/${selected.slug}`
      : project.path;
    revealProject(path);
  }

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
  function th(key: string): ThreadState {
    if (!threads[key]) threads[key] = { items: [], running: false, queue: [] };
    return threads[key];
  }

  // active thread key for the current tab (thread tabs carry their key)
  const activeThreadKey = $derived.by(() => {
    if (!selected) return "";
    const t = activeTab;
    if (t && t.kind === "thread") return t.id === "thread" ? defaultThreadKey() : t.id;
    return defaultThreadKey();
  });
  // the main "Тред" tab is pinned to ONE thread (the task's oldest); new
  // threads from "+" get their own tabs — the main tab never jumps around
  let mainThread: Record<number, string> = $state({});
  function defaultThreadKey(): string {
    if (!selected) return "";
    return mainThread[selected.id] ?? `new:${selected.id}`;
  }
  function threadIdOfKey(key: string): number | null {
    return key.startsWith("t") ? Number(key.slice(1)) : null;
  }
  const EMPTY: ThreadState = { items: [], running: false, queue: [] };
  $effect(() => {
    if (selected) {
      loadCtx();
      loadKnowledge();
    }
  });

  function fmtReset(ts: number): string {
    const d = new Date(ts * 1000);
    return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }
  const cur = $derived(activeThreadKey ? (threads[activeThreadKey] ?? EMPTY) : EMPTY);

  type Block = { kind: "msg"; item: ThreadItem; idx: number } | { kind: "tools"; tools: string[] };

  // consecutive tool events fold into one collapsible block (review: tool spam)
  const blocks = $derived.by(() => {
    const out: Block[] = [];
    for (const it of cur.items) {
      if (it.kind === "tool") {
        const last = out[out.length - 1];
        if (last?.kind === "tools") last.tools.push(it.text);
        else out.push({ kind: "tools", tools: [it.text] });
      } else {
        out.push({ kind: "msg", item: it, idx: cur.items.indexOf(it) });
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
    if (!selected || (!msg.trim() && attachments.length === 0)) return;
    const t = th(activeThreadKey);
    const text = msg.trim();
    if (replyTo !== null) {
      const quote = replyTo.split("\n").slice(0, 8).map((l) => `> ${l}`).join("\n");
      const full = `${quote}\n\n${text}`;
      const item: ThreadItem = { kind: "reply", text: JSON.stringify({ quote: replyTo, text }) };
      replyTo = null;
      msg = "";
      if (t.running) {
        t.queue.push(full);
      } else {
        t.items.push(item);
        t.running = true;
        t.waiting = true;
        t.turnStart = Date.now();
        stopRequested = false;
        scrollDown();
        threadSend(selected.id, threadIdOfKey(activeThreadKey), full);
      }
      return;
    }
    if (attachments.length === 0) {
      if (t.running) t.queue.push(text);
      else fire(selected.id, text);
      msg = "";
      return;
    }
    const fence = "```";
    const full = [text, ...attachments.map((a) => `${a.loc}\n${fence}\n${a.code}\n${fence}`)]
      .filter(Boolean)
      .join("\n\n");
    if (t.running) {
      t.queue.push(full);
    } else {
      const midx = t.items.length;
      if (!attLog[selected.id]) attLog[selected.id] = [];
      for (const a of attachments) attLog[selected.id].push({ loc: a.loc, midx });
      t.items.push({
        kind: "uquote",
        text: JSON.stringify({ text, atts: attachments.map((a) => a.loc) }),
      });
      t.running = true;
      t.waiting = true;
      t.turnStart = Date.now();
      stopRequested = false;
      scrollDown();
      const k = activeThreadKey;
      threadSend(selected.id, threadIdOfKey(k), full).then(async (tid) => {
        if (k.startsWith("new:")) {
          threads[`t${tid}`] = threads[k];
          delete threads[k];
          if (selected) {
            mainThread[selected.id] = `t${tid}`;
            taskThreads[selected.id] = await threadsList(selected.id);
          }
        }
      });
    }
    attachments = [];
    msg = "";
  }

  let stopRequested = $state(false);

  function stopTurn() {
    if (!selected) return;
    const tid = threadIdOfKey(activeThreadKey);
    if (tid === null) return;
    threadStop(selected.id, tid, stopRequested); // 2nd click = force kill
    stopRequested = true;
  }

  async function fire(taskId: number, prompt: string, key?: string) {
    const k = key ?? activeThreadKey ?? `new:${taskId}`;
    const t = th(k);
    t.items.push({ kind: "user", text: prompt });
    t.running = true;
    t.waiting = true; // skeleton "agent is connecting" until the first event
    t.turnStart = Date.now();
    stopRequested = false;
    scrollDown();
    const tid = await threadSend(taskId, threadIdOfKey(k), prompt);
    if (k.startsWith("new:")) {
      // the backend created the thread — rebind the state to its real id
      threads[`t${tid}`] = threads[k];
      delete threads[k];
      mainThread[taskId] = `t${tid}`;
      taskThreads[taskId] = await threadsList(taskId);
    }
  }

  async function loadCtx() {
    if (selected) ctx = await taskContext(selected.id);
  }

  function onEvent(e: ThreadEvent) {
    const t = th(`t${e.thread_id}`);
    if (e.kind === "limit") {
      if (e.resets_at) limit = { kind: e.text, resetsAt: e.resets_at };
      return;
    }
    t.waiting = false;
    if (e.kind === "delta") {
      const last = t.items[t.items.length - 1];
      if (last?.kind === "agent") last.text += e.text;
      else t.items.push({ kind: "agent", text: e.text });
    } else if (e.kind === "tool") {
      t.items.push({ kind: "tool", text: e.text });
    } else if (e.kind === "done") {
      t.running = false;
      const wasStopped = stopRequested || e.text === "остановлено";
      stopRequested = false;
      if (t.turnStart) {
        const sec = Math.round((Date.now() - t.turnStart) / 1000);
        const dur = sec >= 60 ? `${Math.floor(sec / 60)}м ${sec % 60}с` : `${sec}с`;
        t.items.push({ kind: "turn", text: dur });
        t.turnStart = undefined;
      }
      if (e.ok === false) {
        if (wasStopped) t.items.push({ kind: "stopped", text: "" });
        else t.items.push({ kind: "error", text: e.text || "агент завершился с ошибкой" });
      }
      loadCtx();
      const next = t.queue.shift();
      if (next) fire(e.task_id, next, `t${e.thread_id}`);
    }
    if (selected?.id === e.task_id) scrollDown();
  }

  const ordered = $derived(tree.flatMap((n) => n.tasks));

  async function reload() {
    const projects = await projectsList();
    tree = await Promise.all(
      projects.map(async (p) => ({ project: p, tasks: sortTasks(await tasksList(p.id, showArchived)) })),
    );
    projectId = projectId ?? projects[0]?.id;
    if (selected) {
      const all = tree.flatMap((n) => n.tasks);
      selected = all.find((t) => t.id === selected!.id) ?? selected;
    }
  }

  onMount(() => {
    hubPrompt = localStorage.getItem("gcode.draft.newtask") ?? "";
    try {
      collapsed = JSON.parse(localStorage.getItem("gcode.sidebar.collapsed") ?? "{}");
    } catch {
      collapsed = {};
    }
    sbw = Number(localStorage.getItem("gcode.sidebar.width") ?? 260) || 260;
    ctxw = Number(localStorage.getItem("gcode.ctx.width") ?? 230) || 230;
    checkUpdate().then((u) => (upd = u));
    reload();
    let un: (() => void) | undefined;
    let unThread: (() => void) | undefined;
    onTasksChanged(async () => {
      creating = false;
      await reload();
    }).then((u) => (un = u));
    onThreadEvent(onEvent).then((u) => (unThread = u));
    let unRen: (() => void) | undefined;
    onTaskRenamed(({ id }) => {
      naming.delete(id);
      naming = new Set(naming);
      reload();
    }).then((u) => (unRen = u));

    const onkey = (e: KeyboardEvent) => {
      if (e.metaKey && e.key.toLowerCase() === "n") {
        e.preventDefault();
        goHub();
      }
      if (e.metaKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        if (selected) openDiff();
      }
      if (e.metaKey && e.shiftKey && (e.key === "[" || e.key === "{")) {
        e.preventDefault();
        cycleTab(-1);
      }
      if (e.metaKey && e.shiftKey && (e.key === "]" || e.key === "}")) {
        e.preventDefault();
        cycleTab(1);
      }
      if (e.metaKey && !e.shiftKey && e.key.toLowerCase() === "w") {
        e.preventDefault();
        if (selected && activeTab) closeTab(selected.id, activeTab.id);
      }
      if (e.key === "Escape" && sbMode === "files" && !paletteOpen && !addProjOpen && !filePaletteOpen) {
        sbMode = "tasks";
      }
      if (e.metaKey && e.key.toLowerCase() === "p") {
        e.preventDefault();
        if (selected) openFilePalette();
      }
      if (e.metaKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        palQ = "";
        paletteOpen = true;
      }
    };
    window.addEventListener("keydown", onkey);
    return () => {
      window.removeEventListener("keydown", onkey);
      un?.();
      unThread?.();
      unRen?.();
    };
  });

  // central hub composer = the only task-creation flow (⌘N leads here)
  let hubPrompt = $state("");
  let hubTa: HTMLTextAreaElement | undefined = $state();

  function goHub(pid?: number) {
    if (pid !== undefined) projectId = pid;
    selected = undefined;
    requestAnimationFrame(() => hubTa?.focus());
  }

  function saveHubDraft() {
    localStorage.setItem("gcode.draft.newtask", hubPrompt);
  }

  async function submitHub() {
    if (!project || !hubPrompt.trim()) return;
    const text = hubPrompt.trim();
    hubPrompt = "";
    localStorage.removeItem("gcode.draft.newtask");
    const p = project;
    const t = await taskCreate(p.id, text); // instant (optimistic naming)
    naming.add(t.id);
    naming = new Set(naming);
    await reload();
    const node = tree.find((n) => n.project.id === p.id);
    const created = node?.tasks.find((x) => x.id === t.id);
    if (created && node) {
      pick(created, node.project);
      fire(created.id, text); // prompt = first agent message, right away
    }
  }
  function greet(): string {
    const h = new Date().getHours();
    if (h < 6) return "Поздняя смена?";
    if (h < 12) return "Доброе утро";
    if (h < 18) return "Что делаем?";
    return "Добрый вечер";
  }

  function pick(t: Task, p: Project) {
    selected = t;
    projectId = p.id;
    diffRepo = null;
    // load the task's threads, then restore the latest conversation
    threadsList(t.id).then((list) => {
      taskThreads[t.id] = list;
      if (!list.length) return;
      const oldest = [...list].sort((a, b) => a.created_at.localeCompare(b.created_at))[0];
      if (!mainThread[t.id]) mainThread[t.id] = `t${oldest.id}`;
      const tid = Number(mainThread[t.id].slice(1));
      const st = th(mainThread[t.id]);
      if (st.items.length === 0 && !st.running) {
        threadHistory(t.id, tid).then((items) => {
          if (st.items.length === 0 && items.length) {
            st.items = items.map((i) => ({ kind: i.kind, text: i.text }));
            if (selected?.id === t.id) scrollDown();
          }
        });
      }
    });
  }

  async function addProject() {
    // native folder picker; the path-input modal is only the demo/error fallback
    const picked = await pickFolder();
    if (picked === null && !isDemo) return; // cancelled
    if (picked) {
      try {
        const p = await projectAdd(picked);
        projectId = p.id;
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
      projectId = p.id;
      await reload();
    } catch (e) {
      projErr = String(e);
    }
  }
</script>

<svelte:window
  onclick={(e) => {
    if (viewMenuOpen && !(e.target as HTMLElement).closest(".viewmenu-wrap")) viewMenuOpen = false;
    if (helpOpen && !(e.target as HTMLElement).closest(".help-wrap")) helpOpen = false;
  }}
/>

<svelte:head><title>gcode{project ? ` · ${project.name}` : ""}</title></svelte:head>

<div class="layout" class:with-ctx={!!selected} style="--sbw:{sbw}px; --ctxw:{ctxw}px">
  <aside>
    <div class="drag-strip" data-tauri-drag-region>
      {#if upd}
        <div
          class="upd-wrap"
          role="status"
          onmouseenter={() => (updOpen = upd?.available ?? false)}
          onmouseleave={() => (updOpen = false)}
        >
          {#if upd.available}
            <button class="upd-btn" onclick={() => openUrl(upd!.url)}>Update</button>
            {#if updOpen}
              <div class="upd-pop">
                <b>v{upd.version}</b>
                <span class="mut" style="font-size:11.5px">{upd.date}</span>
                <div class="upd-notes">
                  {#each upd.notes.split("\n").filter((l) => l.trim()) as line, i (i)}
                    <p>{line.replace(/^[-*] /, "• ")}</p>
                  {/each}
                </div>
                <span class="mut" style="font-size:11px">клик по Update — открыть релиз · автоустановка в фазе 9</span>
              </div>
            {/if}
          {:else}
            <span class="upd-ver">v{upd.current}</span>
          {/if}
        </div>
      {/if}
    </div>

    {#if sbMode === "files" && (filesScope === "task" ? !!selected : !!filesProject)}
      <button class="newtask" onclick={() => (sbMode = "tasks")}>
        <svg class="ic" viewBox="0 0 16 16"><path d="M9.5 3.5 5 8l4.5 4.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/></svg>
        К задачам
      </button>
      <div class="sb-head">
        <span class="seg">
          <svg class="ic" style="width:12px;height:12px" viewBox="0 0 16 16"><path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h3l1.4 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
          {filesScope === "task" ? `${selected?.title ?? ""} · worktrees` : `${filesProject?.name} · файлы`}
        </span>
        <span style="flex:1"></span>
      </div>
      <div class="ft-wrap">
        {#if filesScope === "task" && selected}
          {#key selected.id}
            <FileTree
              lister={(rel) => taskDirList(selected!.id, rel)}
              active={activeTab?.kind === "file" && activeTab.scope === "task" ? `${activeTab.repo}/${activeTab.path}` : null}
              onopen={(rel) => { const [r, ...rest] = rel.split("/"); openEditor(r, rest.join("/")); }}
            />
          {/key}
        {:else if filesProject}
          <FileTree
            lister={(rel) => projectDirList(filesProject!.id, rel)}
            active={activeTab?.kind === "file" && activeTab.scope === "project" ? activeTab.path : null}
            onopen={openProjectFile}
          />
        {/if}
      </div>
    {:else}
    <button class="newtask" onclick={() => goHub()}>
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
      <button class="iconbtn" data-tip="Добавить проект" aria-label="Добавить проект" onclick={addProject}>
        <svg class="ic" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
      </button>
      <span style="flex:1"></span>
      <div class="viewmenu-wrap">
        <button class="iconbtn" data-tip="Вид и сортировка" aria-label="Вид и сортировка" onclick={() => (viewMenuOpen = !viewMenuOpen)}>
          <svg class="ic" viewBox="0 0 16 16"><path d="M2.5 4.5h11M4.5 8h7M6.5 11.5h3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
        </button>
        {#if viewMenuOpen}
          <div class="viewmenu glass-rim" role="menu">
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
      <button class="iconbtn" class:on={showArchived} data-tip="Архивные задачи" aria-label="Архивные задачи" onclick={() => { showArchived = !showArchived; reload(); }}>
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
          <div class="phead-wrap">
            <button class="phead" onclick={() => toggleProject(node.project.id)}>
              {#if collapsed[node.project.id]}
                <svg class="ic pic" viewBox="0 0 16 16"><path d="M1.8 4.4c0-.7.5-1.2 1.2-1.2h2.7l1.3 1.5h5.2c.7 0 1.2.5 1.2 1.2v6c0 .7-.5 1.2-1.2 1.2H3c-.7 0-1.2-.5-1.2-1.2z" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/></svg>
              {:else}
                <svg class="ic pic" viewBox="0 0 16 16"><path d="M1.8 12V4.4c0-.7.5-1.2 1.2-1.2h2.7l1.3 1.5h4.9c.7 0 1.2.5 1.2 1.2v.9" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M1.8 12l1.6-4.1c.2-.5.6-.8 1.1-.8h8.9c.8 0 1.4.8 1.1 1.6l-1.1 2.9c-.2.5-.6.8-1.1.8H2.2" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/></svg>
              {/if}
              <span class="pname2" class:pactive={project?.id === node.project.id}>{node.project.name}</span>
              <span class="pmeta">{node.tasks.length}</span>
            </button>
            <span class="p-actions">
              <button class="iconbtn sm" data-tip="Открыть папку" aria-label="Открыть папку" onclick={() => revealProject(node.project.path)}>
                <svg class="ic" viewBox="0 0 16 16"><circle cx="3.5" cy="8" r="1.1" fill="currentColor"/><circle cx="8" cy="8" r="1.1" fill="currentColor"/><circle cx="12.5" cy="8" r="1.1" fill="currentColor"/></svg>
              </button>
              <button class="iconbtn sm" data-tip="Показать файлы" aria-label="Показать файлы" onclick={() => { filesProject = node.project; filesScope = "project"; sbMode = "files"; }}>
                <svg class="ic" viewBox="0 0 16 16"><path d="M3 3.5h4M3 8h2.5M3 12.5h2.5M8 8h5M8 12.5h5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><path d="M5.5 3.5v9" stroke="currentColor" stroke-width="1" stroke-linecap="round" opacity="0.5"/></svg>
              </button>
              <button class="iconbtn sm" data-tip="Новая задача" aria-label="Новая задача" onclick={() => goHub(node.project.id)}>
                <svg class="ic" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.2" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M8 5.4v5.2M5.4 8h5.2" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>
              </button>
            </span>
          </div>
          {#if !collapsed[node.project.id]}
            <div class="plist">
              {#if node.tasks.length === 0}
                <p class="mut" style="margin:2px 8px 6px">нет задач</p>
              {:else}
                {#each node.tasks as t (t.id)}
                  <div class:dim-arch={t.archived}>
                    <TaskRow
                      title={t.title}
                      status={t.status}
                      time={ago(t.created_at)}
                      pinned={t.pinned}
                      active={selected?.id === t.id}
                      onclick={() => pick(t, node.project)}
                      onpin={() => taskPin(t.id, !t.pinned)}
                      onarchive={() => {
                        if (selected?.id === t.id) selected = undefined;
                        taskArchive(t.id);
                      }}
                    />
                  </div>
                {/each}
              {/if}
            </div>
          {/if}
        </div>
      {/each}
    {/if}
    {/if}
    <div class="sb-bottom">
      <button class="user" data-tip="Вход через Google — скоро · v0.1" aria-label="Вход через Google — скоро · v0.1">
        <span class="avatar">G</span>
        <span class="uname">Газиз</span>
        {#if isDemo}<span class="demo">demo</span>{/if}
      </button>
      <span style="flex:1"></span>
      <button class="iconbtn" data-tip="Remote с телефона — скоро" aria-label="Remote с телефона — скоро">
        <svg class="ic" viewBox="0 0 16 16"><rect x="4.6" y="1.8" width="6.8" height="12.4" rx="1.6" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M7 12.4h2" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>
      </button>
      <button class="iconbtn" data-tip="Настройки · ⌘," aria-label="Настройки · ⌘,">
        <svg class="ic" viewBox="0 0 16 16"><circle cx="8" cy="8" r="2.1" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M8 1.6v2.1M8 12.3v2.1M1.6 8h2.1M12.3 8h2.1M3.5 3.5l1.5 1.5M11 11l1.5 1.5M12.5 3.5 11 5M5 11l-1.5 1.5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>
      </button>
    </div>
    <div class="sb-resize" role="separator" aria-orientation="vertical" aria-label="Ширина сайдбара" onpointerdown={startResize}></div>
  </aside>

  <div class="card">
    <div class="card-actions">
      <button class="iconbtn" data-tip={selected ? "Открыть задачу в Finder" : "Открыть проект в Finder"} aria-label="Открыть в Finder" onclick={revealCurrent}>
        <svg class="ic" viewBox="0 0 16 16"><path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h3l1.4 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
      </button>
      <div class="help-wrap">
        <button class="iconbtn" data-tip="Помощь" aria-label="Помощь" onclick={() => (helpOpen = !helpOpen)}>
          <svg class="ic" viewBox="0 0 16 16"><circle cx="8" cy="8" r="6.3" fill="none" stroke="currentColor" stroke-width="1.1"/><path d="M6.3 6.2c.2-1 1-1.6 1.9-1.5.9 0 1.7.7 1.7 1.6 0 1.2-1.6 1.4-1.9 2.4v.5" fill="none" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/><circle cx="8" cy="11.6" r=".7" fill="currentColor"/></svg>
        </button>
        {#if helpOpen}
          <div class="viewmenu help-menu glass-rim" role="menu">
            <button class="vm-item" onclick={() => { helpOpen = false; openUrl("https://github.com/freekos/gcode/issues/new?labels=bug"); }}>Сообщить об ошибке</button>
            <button class="vm-item" onclick={() => { helpOpen = false; openUrl("https://github.com/freekos/gcode/issues/new?labels=enhancement"); }}>Предложить фичу</button>
            <button class="vm-item" onclick={() => { helpOpen = false; openUrl("https://github.com/freekos/gcode#readme"); }}>Документация</button>
            <button class="vm-item" onclick={async () => { helpOpen = false; await exportLogs(); }}>Экспорт логов</button>
          </div>
        {/if}
      </div>
    </div>
  <main>
    {#if creating}
      <div class="center-empty">
        <p class="spin">◐</p>
        <p>Готовлю worktrees…</p>
      </div>
    {:else if selected}
      <div class="thread-head" data-tauri-drag-region>
        {#if naming.has(selected.id)}
          <b>{selected.title}</b>
          <span class="skel skel-pill" data-tip="ИИ придумывает имя и ветку" aria-label="Имя генерится"></span>
        {:else}
          <b>{selected.title}</b>
          <span class="branch">{selected.branch}</span>
        {/if}
        <Badge status={cur.running ? "running" : selected.status} />
      </div>
      {#if curTabs.length > 0}
        <div class="tabbar">
          {#each curTabs as t (t.id)}
            <div class="tab" class:on={activeTab?.id === t.id}>
              <button class="tab-main" onclick={() => selected && (activeByTask[selected.id] = t.id)}>
                <span class="tab-ic">{t.kind === "thread" ? "💬" : t.kind === "diff" ? "±" : t.kind === "md" ? "📄" : "✎"}</span>
                {t.title}
              </button>
              {#if t.id !== "thread"}
                <button class="tab-x" aria-label="Закрыть вкладку" onclick={() => selected && closeTab(selected.id, t.id)}>×</button>
              {/if}
            </div>
          {/each}
          <button class="tab-plus" data-tip="Новый тред" aria-label="Новый тред" onclick={newThread}>+</button>
          <span class="tab-hint">⌘⇧[ ] · ⌘W</span>
        </div>
      {/if}
      {#if !activeTab || activeTab.kind === "thread"}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
      <div class="thread-box" bind:this={threadBox} onclick={onThreadClick} role="presentation">
        {#if cur.items.length === 0}
          <div class="center-empty">
            <p class="mut">Скажи агенту, что делать — worktrees уже готовы:</p>
            <p class="branch">.gcode/tasks/{selected.slug}/</p>
          </div>
        {:else}
          {#if cur.waiting}
            <div class="agent-wait">
              <span class="skel skel-line" style="width:56%"></span>
              <span class="skel skel-line" style="width:38%"></span>
              <span class="mut" style="font-size:11.5px">◐ агент подключается…</span>
            </div>
          {/if}
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
              <div class="m-user-wrap">
                <span class="msg-acts">
                  <button class="ma" data-tip="Повторить промпт" aria-label="Повторить" onclick={() => selected && !cur.running && fire(selected.id, b.item.text)}>
                    <svg class="ic-xs" viewBox="0 0 16 16"><path d="M13 8a5 5 0 1 1-1.5-3.6M13 2.8v2.6h-2.6" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
                  </button>
                  <button class="ma" data-tip="Цитировать в ответ" aria-label="Цитировать" onclick={() => quoteReply(b.item.text)}>
                    <svg class="ic-xs" viewBox="0 0 16 16"><path d="M3 4.5h10M3 8h10M3 11.5h6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
                  </button>
                </span>
                <div class="m-user">{b.item.text}</div>
              </div>
            {:else if b.item.kind === "agent"}
              <div class="m-agent-wrap">
                <span class="msg-acts left">
                  <button class="ma" data-tip="Цитировать в ответ" aria-label="Цитировать" onclick={() => quoteReply(b.item.text)}>
                    <svg class="ic-xs" viewBox="0 0 16 16"><path d="M3 4.5h10M3 8h10M3 11.5h6" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
                  </button>
                </span>
              <div class="m-agent"><Md text={b.item.text} /></div>
              </div>
            {:else if b.item.kind === "review"}
              {@const revs = JSON.parse(b.item.text)}
              <div class="m-review">
                <div class="rv-head">Ревью · {revs.length} комм.</div>
                {#each revs as rc, ri (ri)}
                  <div class="rv-item">
                    <span class="mono rv-loc">{rc.repo}/{rc.file}:{rc.from}{rc.to !== rc.from ? `–${rc.to}` : ""}</span>
                    <p class="rv-text">{rc.text}</p>
                    <details class="rv-code">
                      <summary>код</summary>
                      <pre>{rc.code}</pre>
                    </details>
                  </div>
                {/each}
              </div>
            {:else if b.item.kind === "uquote"}
              {@const uq = JSON.parse(b.item.text)}
              <div class="m-user-wrap" data-midx={b.idx}>
                <div class="m-user">
                  {#if uq.text}{uq.text}{/if}
                  <div class="uq-atts">
                    {#each uq.atts as a (a)}<span class="uq-chip mono">📄 {a}</span>{/each}
                  </div>
                </div>
              </div>
            {:else if b.item.kind === "reply"}
              {@const rq = JSON.parse(b.item.text)}
              <div class="m-user-wrap" data-midx={b.idx}>
                <div class="m-user">
                  <div class="rq-quote">{rq.quote.split("\n").slice(0, 4).join("\n")}</div>
                  {rq.text}
                </div>
              </div>
            {:else if b.item.kind === "stopped"}
              <div class="m-stop">⏹ Остановлено</div>
            {:else if b.item.kind === "turn"}
              <div class="m-turn"><span>Работал {b.item.text}</span><span class="tline"></span></div>
            {:else}
              <div class="m-err">{b.item.text}</div>
            {/if}
          {/each}
        {/if}
      </div>
      <div class="composer">
        <div class="c-inner glass-rim">
          {#if replyTo !== null}
            <div class="att-row">
              <span class="reply-card">
                <span class="rq-bar"></span>
                <span class="rq-text">{replyTo.split("\n")[0].slice(0, 90)}</span>
                <button class="x" aria-label="Убрать цитату" onclick={() => (replyTo = null)}>×</button>
              </span>
            </div>
          {/if}
          {#if attachments.length}
            <div class="att-row">
              {#each attachments as a, i (i)}
                <span class="uq-chip mono" data-tip={a.code.split("\n").slice(0, 3).join(" ⏎ ").slice(0, 120)}>
                  📄 {a.loc.split("/").slice(-1)[0]}
                  <button class="x" aria-label="Убрать" onclick={() => (attachments = attachments.filter((_, j) => j !== i))}>×</button>
                </span>
              {/each}
            </div>
          {/if}
          <textarea
            use:autogrow
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
          <div class="c-row">
            <button class="iconbtn" data-tip="Вложения — скоро" aria-label="Вложения — скоро">
              <svg class="ic" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
            </button>
            <button class="perm" data-tip="Права агента: авто-правки кода, git запрещён технически" aria-label="Права агента">
              <svg class="ic" style="color:inherit" viewBox="0 0 16 16"><path d="M8 1.8 3 3.6v4.1c0 3 2.1 5.2 5 6.5 2.9-1.3 5-3.5 5-6.5V3.6z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
              Автопилот · без git
              <span class="chev2">⌄</span>
            </button>
            {#if cur.running}
              <span class="queue-note">◐ работает{cur.queue.length ? ` · очередь: ${cur.queue.length}` : ""}</span>
            {/if}
            <span style="flex:1"></span>
            {#if limit}
              <span class="mut mono-s" data-tip="Окно лимита подписки" aria-label="Лимит">↻ {limit.kind === "five_hour" ? "5h" : limit.kind} · {fmtReset(limit.resetsAt)}</span>
            {/if}
            <button class="engine-chip" data-tip="Движок треда" aria-label="Движок треда">◆ Claude <span class="chev2">⌄</span></button>
            {#if cur.running}
              <button class="send stop" data-tip={stopRequested ? "Ещё раз — жёсткая остановка" : "Остановить ход"} aria-label="Остановить" onclick={stopTurn}>
                <svg class="ic" viewBox="0 0 16 16"><rect x="4.4" y="4.4" width="7.2" height="7.2" rx="1.4" fill="currentColor"/></svg>
              </button>
            {/if}
            <button class="send" onclick={sendMsg} data-tip={cur.running ? "В очередь · ⏎" : "Отправить · ⏎"} aria-label="Отправить">
              <svg class="ic" style="color:inherit" viewBox="0 0 16 16"><path d="M8 12.5v-9M4.5 7 8 3.5 11.5 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/></svg>
            </button>
          </div>
        </div>
      </div>
      {:else if activeTab.kind === "diff"}
        <div class="dp-head">
          <div class="dp-chips">
            <button class="repo-chip" class:on={diffRepo === null} onclick={() => openDiff(null)}>Все</button>
            {#if ctx}
              {#each ctx.touched as r (r.repo)}
                <button class="repo-chip" class:on={diffRepo === r.repo} onclick={() => openDiff(r.repo)}>
                  {r.repo} <DiffStat add={r.add} del={r.del} />
                </button>
              {/each}
            {/if}
          </div>
        </div>
        <DiffView
          groups={diffGroups}
          onsend={sendReview}
          onselchange={(b) => (diffSelecting = b)}
          onopen={openEditor}
          onquote={(repo, file, from, to, code) => quoteToComposer(`${repo}/${file}:${from}${to !== from ? `–${to}` : ""}`, code)}
        />
      {:else if activeTab.kind === "file"}
        {#key activeTab.id}
          <Editor
            content={tabContent[activeTab.id] ?? ""}
            path={activeTab.path}
            label={activeTab.scope === "task" ? `${activeTab.repo}/${activeTab.path}` : activeTab.path}
            onsave={(text) => activeTab && saveEditor(activeTab, text)}
            onquote={(from, to, code) => {
              if (!activeTab || activeTab.kind !== "file") return;
              quoteToComposer(
                `${activeTab.scope === "task" ? activeTab.repo + "/" : ""}${activeTab.path}:${from}${to !== from ? `–${to}` : ""}`,
                code,
              );
              if (selected) activeByTask[selected.id] = "thread";
            }}
          />
        {/key}
      {:else if activeTab.kind === "md"}
        <div class="md-page">
          <Md text={tabContent[activeTab.id] ?? ""} />
        </div>
      {/if}
    {:else}
      <div class="hub">
        <p class="hub-greet">{greet()}</p>
        <div class="hub-box glass-rim">
          <div class="hub-proj">
            <span class="proj-chip">
              <svg class="ic" style="width:12px;height:12px" viewBox="0 0 16 16"><path d="M1.8 4.2c0-.8.6-1.4 1.4-1.4h3l1.4 1.6h5.2c.8 0 1.4.6 1.4 1.4v6c0 .8-.6 1.4-1.4 1.4H3.2c-.8 0-1.4-.6-1.4-1.4z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
              <select bind:value={projectId} class="proj-pick">
                {#each tree as n (n.project.id)}
                  <option value={n.project.id}>{n.project.name}</option>
                {/each}
              </select>
              <span class="chev2">⌄</span>
            </span>
          </div>
          <div class="c-inner glass-rim">
            <textarea
              use:autogrow
              bind:this={hubTa}
              bind:value={hubPrompt}
              oninput={saveHubDraft}
              rows="2"
              placeholder="Что сделать? Опиши задачу — имя, ветка и worktrees появятся сами"
              onkeydown={(e) => {
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  submitHub();
                }
              }}
            ></textarea>
            <div class="c-row">
              <button class="iconbtn" data-tip="Вложения — скоро" aria-label="Вложения — скоро">
                <svg class="ic" viewBox="0 0 16 16"><path d="M8 3.5v9M3.5 8h9" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/></svg>
              </button>
              <button class="perm" data-tip="Права агента: авто-правки кода, git запрещён технически" aria-label="Права агента">
                <svg class="ic" style="color:inherit" viewBox="0 0 16 16"><path d="M8 1.8 3 3.6v4.1c0 3 2.1 5.2 5 6.5 2.9-1.3 5-3.5 5-6.5V3.6z" fill="none" stroke="currentColor" stroke-width="1.1"/></svg>
                Автопилот · без git
                <span class="chev2">⌄</span>
              </button>
              <span style="flex:1"></span>
              <button class="engine-chip" data-tip="Движок треда" aria-label="Движок треда">◆ Claude <span class="chev2">⌄</span></button>
              <button class="send" onclick={submitHub} data-tip="Создать · ⏎" aria-label="Создать">
                <svg class="ic" style="color:inherit" viewBox="0 0 16 16"><path d="M8 12.5v-9M4.5 7 8 3.5 11.5 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" fill="none"/></svg>
              </button>
            </div>
          </div>
        </div>
        <p class="mut" style="font-size:11px">или выбери задачу слева · <Kbd keys="⌘K" /> — всё остальное</p>
      </div>
    {/if}
  </main>
  {#if selected}
    <aside class="ctx">
      <div class="ctx-resize" role="separator" aria-orientation="vertical" aria-label="Ширина панели" onpointerdown={startCtxResize}></div>
      <div class="grp" style="margin-top:2px">Цель</div>
      {#if ctx && ctx.progress.length}
        {@const done = ctx.progress.filter((p) => p.done).length}
        <button class="nav-link" onclick={openProgressTab}>
          {ctx.progress.find((p) => !p.done)?.text ?? "всё отмечено"}
          <span class="mut">{done}/{ctx.progress.length} · открыть →</span>
        </button>
      {:else}
        <button class="nav-link" onclick={openProgressTab}><span class="mut">PROGRESS.md · открыть →</span></button>
      {/if}

      {#if selected && (taskThreads[selected.id]?.length ?? 0) > 1}
        <div class="grp">Треды</div>
        {#each taskThreads[selected.id] as ti (ti.id)}
          <button class="nav-link" onclick={() => openThreadTab(ti)}>💬 {ti.title}<span class="mut">{ago(ti.created_at)}</span></button>
        {/each}
      {/if}

      <div class="grp">Изменения</div>
      {#if ctx && ctx.touched.length > 0}
        <button class="all-diff" onclick={() => openDiff(null)}>Все изменения задачи →</button>
        {#each ctx.touched as r (r.repo)}
          <button class="repo repo-btn" onclick={() => openDiff(r.repo)} data-tip="Открыть дифф" aria-label="Открыть дифф">
            <div class="rn">{r.repo}</div>
            <div class="rrow">
              <span class="mut">✎ {r.files}</span>
              <DiffStat add={r.add} del={r.del} />
            </div>
          </button>
        {/each}
        {#if ctx.untouched}
          <p class="mut" style="margin:4px 2px">ещё {ctx.untouched} не тронуты ›</p>
        {/if}
      {:else}
        <p class="mut" style="margin:4px 2px">пока без изменений</p>
      {/if}

      {#if knowledgeFiles.length}
        <div class="grp">Файлы проекта</div>
        {#each knowledgeFiles as f (f)}
          <button class="nav-link mono-sm" onclick={() => openKnowledgeTab(f)}>{f}</button>
        {/each}
      {/if}

      {#if selected && attLog[selected.id]?.length}
        <div class="grp">Вложения</div>
        {#each attLog[selected.id] as a, i (i)}
          <button class="nav-link mono-sm" data-tip="К сообщению в треде" onclick={() => jumpToMessage(a.midx)}>📄 {a.loc}</button>
        {/each}
      {/if}
    </aside>
  {/if}
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

<Modal bind:open={filePaletteOpen} width="520px">
  <!-- svelte-ignore a11y_autofocus -->
  <input
    class="pal-input"
    autofocus
    placeholder="Файл задачи… (fuzzy, Enter — открыть)"
    bind:value={fileQ}
    onkeydown={(e) => {
      if (e.key === "Enter" && fileMatches[0]) {
        const [repo, ...rest] = fileMatches[0].split("/");
        filePaletteOpen = false;
        openEditor(repo, rest.join("/"));
      }
    }}
  />
  <div class="pal-list">
    {#each fileMatches as f (f)}
      <button
        class="pal-item"
        onclick={() => {
          const [repo, ...rest] = f.split("/");
          filePaletteOpen = false;
          openEditor(repo, rest.join("/"));
        }}
      >{f}</button>
    {/each}
    {#if fileMatches.length === 0}
      <div class="mut" style="padding:8px 4px">Ничего не найдено</div>
    {/if}
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
      { label: "Новая задача", key: "cmd-new", hint: "⌘N", act: () => { paletteOpen = false; goHub(); } },
      { label: "Добавить проект", key: "cmd-addproj", hint: "", act: () => { paletteOpen = false; addProject(); } },
      { label: "Styleguide", key: "cmd-styleguide", hint: "", act: () => { paletteOpen = false; window.location.href = "/styleguide"; } },
      ...ordered.map((t) => ({ label: t.title, key: `task-${t.id}`, hint: "", act: () => { paletteOpen = false; selected = t; } })),
    ].filter((c) => c.label.toLowerCase().includes(palQ.toLowerCase())) as c, ci (c.key ?? `cmd-${ci}`)}
      <button class="pal-item" onclick={c.act}>
        <span>{c.label}</span>
        {#if c.hint}<Kbd keys={c.hint} />{/if}
      </button>
    {/each}
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
  /* content card: the CENTER is the floating opaque panel (ZCode layout) —
     the sidebar lives directly on the window glass */
  .card {
    display: grid;
    grid-template-columns: 1fr;
    min-width: 0;
    height: 100%;
  }
  .with-ctx .card { grid-template-columns: 1fr var(--ctxw, 230px); }
  .card { position: relative; border: 1px solid var(--border-strong); border-radius: 16px; overflow: hidden; }
  .card-actions {
    position: absolute;
    top: 10px;
    right: 12px;
    z-index: 30;
    display: inline-flex;
    gap: 4px;
  }
  .help-wrap { position: relative; }
  .help-menu { top: 30px; right: 0; left: auto; min-width: 210px; }
  :global(:root.native) .card {
    margin: 10px 14px 14px 0;
    height: calc(100vh - 24px);
    background: var(--surface-0);
    /* flat even border — no gradient rim, no depth "shadow" (Gaziz's call) */
    border: 1px solid var(--border-strong);
    border-radius: 16px;
    overflow: hidden;
  }
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
    border: 0;
    border-radius: var(--r-lg);
    box-shadow: 0 16px 40px oklch(0% 0 0 / 0.45);
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
  .tabbar {
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 8px 14px 8px;
    overflow-x: auto;
  }
  .tab {
    display: inline-flex;
    align-items: center;
    border-radius: 8px;
    background: transparent;
    color: var(--text-muted);
    flex: none;
  }
  .tab.on { background: var(--surface-2); color: var(--text-primary); }
  .tab:hover:not(.on) { background: var(--surface-1); }
  .tab-main {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border: 0;
    background: transparent;
    color: inherit;
    font: 500 12px var(--font-ui);
    padding: 4px 4px 4px 10px;
    cursor: pointer;
    max-width: 200px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .tab-ic { font-size: 10px; opacity: 0.8; }
  .tab-x {
    border: 0;
    background: transparent;
    color: var(--text-muted);
    font-size: 13px;
    cursor: pointer;
    padding: 2px 8px 2px 2px;
  }
  .tab-x:hover { color: var(--text-primary); }
  .tab-plus {
    border: 0;
    background: transparent;
    color: var(--text-muted);
    font-size: 15px;
    cursor: pointer;
    padding: 2px 8px;
    border-radius: 6px;
    flex: none;
  }
  .tab-plus:hover { background: var(--surface-2); color: var(--text-primary); }
  .tab-hint { margin-left: auto; font-size: 10px; color: var(--text-disabled); flex: none; padding-right: 2px; }
  .md-page { flex: 1; overflow-y: auto; padding: 20px 26px 40px; }
  .md-page > :global(.md) { max-width: 860px; margin: 0 auto; }
  .m-user-wrap { display: flex; align-items: flex-start; justify-content: flex-end; gap: 6px; }
  .m-user-wrap .m-user { margin-right: 0 !important; }
  .m-agent-wrap { display: flex; align-items: flex-start; gap: 6px; }
  .msg-acts { display: inline-flex; gap: 2px; flex: none; padding-top: 6px; visibility: hidden; }
  .m-user-wrap:hover .msg-acts, .m-agent-wrap:hover .msg-acts { visibility: visible; }
  .msg-acts.left { order: 2; }
  .ma {
    display: inline-flex; align-items: center; justify-content: center;
    width: 20px; height: 20px; border: 0; border-radius: 5px;
    background: var(--surface-2); color: var(--text-muted); cursor: pointer; padding: 0;
  }
  .ma:hover { color: var(--text-primary); }
  .ic-xs { width: 11px; height: 11px; }
  .send.stop { background: var(--surface-3); color: var(--status-error, oklch(65% 0.15 25)); }
  .send.stop:hover { filter: brightness(1.2); }
  .ft-wrap { overflow-y: auto; flex: 1; padding: 2px 4px; }
  .repo-chip {
    display: inline-flex; align-items: center; gap: 8px;
    background: var(--surface-1); border: 0; border-radius: 999px;
    font: 500 12px var(--font-mono); color: var(--text-secondary);
    padding: 4px 12px; cursor: pointer;
  }
  .repo-chip.on { background: var(--accent-soft); color: var(--text-primary); }
  .repo-btn { width: 100%; border: 0; text-align: left; cursor: pointer; }
  .thread-box :global(code.md-ic) { cursor: pointer; }
  .thread-box :global(code.md-ic:hover) { background: var(--accent-soft); color: var(--text-primary); }
  .nav-link {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    width: 100%;
    border: 0;
    background: transparent;
    color: var(--text-secondary);
    font: 12.5px var(--font-ui);
    text-align: left;
    padding: 5px 8px;
    border-radius: var(--r-md);
    cursor: pointer;
  }
  .nav-link:hover { background: var(--surface-2); color: var(--text-primary); }
  .nav-link .mut { font-size: 11px; }
  .mono-sm { font-family: var(--font-mono); font-size: 11px; }
  :global(.flash) { animation: gc-flash 1.2s ease-out; }
  @keyframes gc-flash {
    0% { background: var(--accent-soft); border-radius: var(--r-md); }
    100% { background: transparent; }
  }
  .all-diff {
    width: 100%;
    background: var(--accent-soft);
    border: 0;
    border-radius: var(--r-md);
    color: var(--text-primary);
    font: 500 12px var(--font-ui);
    padding: 7px 10px;
    cursor: pointer;
    margin-bottom: 8px;
    text-align: left;
    transition: filter var(--t-fast) ease-out;
  }
  .all-diff:hover { filter: brightness(1.15); }
  .repo-btn:hover { background: var(--surface-3); }
  .skel {
    display: inline-block;
    border-radius: 6px;
    background: linear-gradient(90deg, var(--surface-2) 25%, var(--surface-3) 50%, var(--surface-2) 75%);
    background-size: 200% 100%;
    animation: gc-shimmer 1.4s ease-in-out infinite;
  }
  .skel-pill { width: 120px; height: 14px; }
  .skel-line { height: 12px; margin: 3px 0; }
  .agent-wait { display: flex; flex-direction: column; gap: 2px; order: 999; }
  @keyframes gc-shimmer {
    to { background-position: -200% 0; }
  }
  .mono-input { font-family: var(--font-mono); font-size: 12.5px; }
  .drag-strip {
    height: 44px;
    flex: none;
    margin: -12px -12px 0;
    display: flex;
    align-items: center;
    justify-content: flex-end;
    padding-right: 12px;
  }
  /* the traffic lights sit at (26,26): their visual center is ~32px from the
     window top — nudge the pill so both share one axis */
  .upd-wrap { margin-top: -3px; }
  .upd-wrap { position: relative; display: inline-flex; align-items: center; }
  .upd-btn {
    background: oklch(62% 0.14 150);
    color: oklch(15% 0.03 150);
    border: 0;
    border-radius: 999px;
    font: 700 12px var(--font-ui);
    padding: 4px 14px;
    cursor: pointer;
    transition: filter var(--t-fast) ease-out;
  }
  .upd-btn:hover { filter: brightness(1.08); }
  .upd-ver {
    font: 500 11px var(--font-mono);
    color: var(--text-muted);
    background: var(--surface-2);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    padding: 2px 10px;
  }
  .upd-pop {
    position: fixed;
    top: 52px;
    left: 14px;
    z-index: 200;
    width: min(430px, 80vw);
    background: var(--surface-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--r-lg);
    padding: 14px 16px;
    box-shadow: 0 20px 50px oklch(0% 0 0 / 0.5), inset 0 1px 0 var(--glass-highlight);
  }
  .upd-pop b { font-size: 14px; margin-right: 8px; }
  .upd-notes { margin: 10px 0; max-height: 300px; overflow-y: auto; }
  .upd-notes p { margin: 4px 0; color: var(--text-secondary); font-size: 12.5px; }
  .sb-bottom {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: 4px;
    padding-top: 8px;
    border-top: 1px solid var(--border-subtle);
  }
  .user {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: transparent;
    border: 0;
    cursor: pointer;
    padding: 4px 6px;
    border-radius: var(--r-md);
    color: var(--text-primary);
    font: 600 12.5px var(--font-ui);
  }
  .user:hover { background: var(--surface-2); }
  .avatar {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: oklch(58% 0.15 40);
    color: white;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font: 700 12px var(--font-ui);
  }
  .uname { letter-spacing: 0.01em; }
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
    width: min(660px, 100%);
    background: var(--surface-1);
    border: 0;
    border-radius: 18px;
    padding: 8px;
  }
  :global(:root.native) .hub-box { background: var(--surface-2); }
  .hub-box { box-shadow: inset 0 1px 0 var(--glass-highlight); }
  .hub-proj { margin: 4px 6px 8px; }
  .c-inner {
    background: var(--surface-2);
    border: 0;
    border-radius: 12px;
    padding: 10px 10px 8px;
    box-shadow: inset 0 1px 0 var(--glass-highlight);
  }
  .c-row { display: flex; align-items: center; gap: 8px; margin-top: 6px; }
  .perm {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: 0;
    cursor: pointer;
    color: var(--status-running);
    font: 500 12.5px var(--font-ui);
    padding: 4px 6px;
    border-radius: var(--r-md);
  }
  .perm:hover { background: color-mix(in oklab, var(--status-running) 10%, transparent); }
  .engine-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: transparent;
    border: 0;
    cursor: pointer;
    color: var(--text-secondary);
    font: 500 12.5px var(--font-ui);
    padding: 4px 8px;
    border-radius: var(--r-md);
  }
  .engine-chip:hover { background: var(--surface-3); color: var(--text-primary); }
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
  .proj-chip:focus-within { border-color: var(--accent); }
  .chev2 { font-size: 10px; color: var(--text-muted); margin-top: -2px; }
  .hub-box textarea,
  .composer textarea {
    width: 100%;
    border: 0;
    background: transparent;
    resize: none;
    color: var(--text-primary);
    font: 13.5px var(--font-ui);
    outline: none;
    padding: 2px;
  }
  .send {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 34px;
    height: 34px;
    border-radius: 11px;
    border: 1px solid var(--border-subtle);
    background: var(--surface-3);
    color: var(--text-primary);
    cursor: pointer;
    transition: background var(--t-fast) ease-out, transform var(--t-fast) ease-out;
    box-shadow: inset 0 1px 0 var(--glass-highlight);
  }
  .send:hover { background: oklch(38% 0.006 280); }
  .send:active { transform: translateY(1px); }
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
  /* native window: the sidebar is FULLY transparent — pure window glass,
     no own background/borders (Gaziz's call: don't fight the vibrancy) */
  :global(:root.native) aside {
    background: transparent;
    border-right: 0;
  }
  :global(:root.native) main { background: transparent; }
  .sb-resize {
    position: absolute;
    top: 0;
    right: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    z-index: 20;
  }
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
  .phead-wrap { display: flex; align-items: center; position: relative; border-radius: var(--r-sm); }
  .phead-wrap:hover { background: var(--surface-2); }
  .phead-wrap .phead { flex: 1; }
  .p-actions {
    position: absolute;
    right: 2px;
    display: inline-flex;
    gap: 1px;
    opacity: 0;
    transition: opacity var(--t-fast) ease-out;
  }
  .phead-wrap:hover .p-actions { opacity: 1; }
  .phead-wrap:hover .pmeta { opacity: 0; }
  .iconbtn.sm { width: 22px; height: 22px; }
  .phead {
    display: flex; align-items: center; gap: 6px;
    background: transparent; border: 0; cursor: pointer; text-align: left;
    padding: 5px 6px; border-radius: var(--r-sm);
    color: var(--text-primary); width: 100%;
  }

  .pic { width: 14px; height: 14px; }
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
  .modal-bar { display: flex; align-items: center; gap: 8px; margin-top: 10px; }
  .thread-box { flex: 1; overflow-y: auto; padding: 18px 22px; display: flex; flex-direction: column; gap: 10px; }
  .thread-box > :global(*) { width: 100%; max-width: 860px; margin-left: auto; margin-right: auto; }
  .m-user {
    background: var(--surface-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-lg);
    padding: 9px 13px;
    max-width: 66% !important;
    margin-right: 0 !important;
  }
  .m-review {
    background: var(--surface-2);
    border: 1px solid var(--border-subtle);
    border-radius: var(--r-lg);
    padding: 10px 14px;
    max-width: 66% !important;
    margin-right: 0 !important;
  }
  .rv-head { font-size: 11px; text-transform: uppercase; letter-spacing: 0.08em; color: var(--text-muted); margin-bottom: 6px; }
  .rv-item { margin-bottom: 8px; }
  .rv-item:last-child { margin-bottom: 0; }
  .rv-loc { font-size: 11px; color: var(--accent); }
  .rv-text { margin: 3px 0 4px; }
  .rv-code summary { font-size: 11px; color: var(--text-muted); cursor: pointer; }
  .rv-code pre {
    font: 11px var(--font-mono);
    background: var(--surface-0);
    border-radius: var(--r-md);
    padding: 8px 10px;
    overflow-x: auto;
    margin: 4px 0 0;
  }
  .m-stop { color: var(--text-muted); font-size: 12px; }
  .att-row { display: flex; flex-wrap: wrap; gap: 5px; padding: 8px 12px 0; }
  .reply-card {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    background: var(--surface-2);
    border-radius: var(--r-md);
    padding: 5px 10px;
    font-size: 12px;
    color: var(--text-secondary);
    max-width: 100%;
  }
  .reply-card .rq-bar { width: 2.5px; align-self: stretch; border-radius: 2px; background: var(--accent); flex: none; }
  .reply-card .rq-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .reply-card .x { background: none; border: 0; color: var(--text-muted); cursor: pointer; font-size: 13px; padding: 0; flex: none; }
  .reply-card .x:hover { color: var(--diff-del); }
  .rq-quote {
    border-left: 2.5px solid var(--accent);
    padding: 2px 0 2px 10px;
    margin-bottom: 6px;
    color: var(--text-muted);
    font-size: 12px;
    white-space: pre-wrap;
  }
  .uq-atts { display: flex; flex-wrap: wrap; gap: 5px; margin-top: 6px; }
  .uq-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: var(--surface-3);
    border-radius: 999px;
    padding: 2px 9px;
    font-size: 10.5px;
    color: var(--text-secondary);
  }
  .uq-chip .x { background: none; border: 0; color: var(--text-muted); cursor: pointer; font-size: 12px; padding: 0; }
  .uq-chip .x:hover { color: var(--diff-del); }
  .m-turn {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text-muted);
    font-size: 12px;
    margin: 6px 0;
  }
  .m-turn .tline { flex: 1; height: 1px; background: var(--border-subtle); }
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
  .composer { padding: 6px 22px 16px; }
  .composer .c-inner { max-width: 860px; margin: 0 auto; border-radius: 16px; }
  .queue-note { font-size: 11.5px; color: var(--status-running); }
  .mono-s { font-family: var(--font-mono); font-size: 11px; }
  .ctx { position: relative; background: var(--surface-1); border-left: 1px solid var(--border-subtle); padding: 12px; overflow-y: auto; }
  .ctx-resize {
    position: absolute;
    top: 0;
    left: 0;
    width: 7px;
    height: 100%;
    cursor: col-resize;
    z-index: 40;
  }
  :global(:root.native) .ctx { background: color-mix(in oklab, var(--surface-1) 70%, transparent); }
  .repo { background: var(--surface-2); border-radius: var(--r-md); padding: 8px 10px; margin-bottom: 8px; }
  .rn { font-family: var(--font-mono); font-size: 11.5px; }
  .rrow { display: flex; gap: 10px; margin-top: 3px; align-items: center; }
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
