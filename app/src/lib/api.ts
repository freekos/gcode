// UI ↔ core bridge. In the Tauri app this calls real commands; in a plain
// browser (vite dev / screenshots) it serves DEMO data so the UI is workable
// without the native shell.

export type TaskStatus = "new" | "running" | "needs_input" | "review" | "done";

export interface Project {
  id: number;
  name: string;
  path: string;
  repos: number;
}

export interface Task {
  id: number;
  title: string;
  slug: string;
  branch: string;
  status: TaskStatus;
  archived: boolean;
  created_at: string;
  pinned: boolean;
}

/** "2026-07-04 08:12:00" (UTC, from SQLite) -> "3d" / "5h" / "12m" / "сейчас" */
export function ago(createdAt: string): string {
  const t = Date.parse(createdAt.replace(" ", "T") + "Z");
  if (Number.isNaN(t)) return "";
  const s = Math.max(0, (Date.now() - t) / 1000);
  if (s < 90) return "сейчас";
  if (s < 3600) return `${Math.round(s / 60)}м`;
  if (s < 86400) return `${Math.round(s / 3600)}ч`;
  return `${Math.round(s / 86400)}д`;
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

// ---- demo data (browser only) ----
const demoProjects: Project[] = [
  { id: 1, name: "azi", path: "~/Codebase/work/azi", repos: 4 },
  { id: 2, name: "invictus", path: "~/Codebase/work/invictus", repos: 12 },
];
const demoTaskSets: Record<number, Task[]> = {
  1: [
    { id: 1, title: "Почини редирект после логина", slug: "fix-login-redirect", branch: "fix-login-redirect", status: "review", archived: false, pinned: false, created_at: "2026-07-03 10:00:00" },
    { id: 2, title: "Добавь тесты на webview-куку", slug: "add-webview-tests", branch: "add-webview-tests", status: "running", archived: false, pinned: false, created_at: "2026-07-03 10:00:00" },
    { id: 4, title: "Рефактор API клиента", slug: "refactor-api-client", branch: "refactor-api-client", status: "new", archived: false, pinned: false, created_at: "2026-07-03 10:00:00" },
  ],
  2: [
    { id: 3, title: "Разберись с ценой в KG", slug: "fix-kg-price", branch: "fix-kg-price", status: "needs_input", archived: false, pinned: false, created_at: "2026-07-03 10:00:00" },
    { id: 5, title: "Лендинг лояльности", slug: "loyalty-landing", branch: "loyalty-landing", status: "done", archived: false, pinned: false, created_at: "2026-07-03 10:00:00" },
    { id: 6, title: "Старая миграция БД", slug: "old-db-migration", branch: "old-db-migration", status: "done", archived: true, pinned: false, created_at: "2026-06-20 10:00:00" },
  ],
};
let demoId = 5;

export async function projectsList(): Promise<Project[]> {
  if (!inTauri) return demoProjects;
  return invoke<Project[]>("projects_list");
}

export async function projectAdd(path: string): Promise<Project> {
  if (!inTauri) {
    const name = path.split("/").filter(Boolean).pop() ?? "project";
    const p = { id: demoProjects.length + 1, name, path, repos: 1 };
    demoProjects.push(p);
    demoTaskSets[p.id] = [];
    return p;
  }
  return invoke<Project>("project_add", { path });
}

export async function tasksList(projectId: number, includeArchived = false): Promise<Task[]> {
  if (!inTauri) {
    const ts = demoTaskSets[projectId] ?? [];
    return includeArchived ? ts : ts.filter((t) => !t.archived);
  }
  return invoke<Task[]>("tasks_list", { projectId, includeArchived });
}

export async function taskCreate(projectId: number, prompt: string): Promise<Task> {
  if (!inTauri) {
    const slug = prompt.toLowerCase().slice(0, 24).replace(/[^a-zа-яё0-9]+/gi, "-");
    const t: Task = { id: demoId++, title: prompt, slug, branch: slug, status: "new", archived: false, pinned: false, created_at: new Date().toISOString().slice(0, 19).replace("T", " ") };
    demoTaskSets[projectId] = [t, ...(demoTaskSets[projectId] ?? [])];
    // demo: pretend the AI renamed it a bit later
    setTimeout(() => {
      t.title = "Красивое имя от ИИ";
      t.branch = "ai-branch-name";
      window.dispatchEvent(new CustomEvent("demo-task-renamed", { detail: { id: t.id, ai: true } }));
      window.dispatchEvent(new CustomEvent("demo-tasks-changed", { detail: { ok: true } }));
    }, 2500);
    return t;
  }
  return invoke<Task>("task_create", { projectId, prompt });
}

export async function onTaskRenamed(cb: (e: { id: number; ai: boolean }) => void): Promise<() => void> {
  if (!inTauri) {
    const h = (e: Event) => cb((e as CustomEvent<{ id: number; ai: boolean }>).detail);
    window.addEventListener("demo-task-renamed", h);
    return () => window.removeEventListener("demo-task-renamed", h);
  }
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<{ id: number; ai: boolean }>("task-renamed", (ev) => cb(ev.payload));
  return un;
}

export interface TasksChangedPayload {
  ok?: boolean;
  slug?: string;
  error?: string;
}

/** Subscribe to "tasks changed" from the core; returns unsubscribe. */
export async function onTasksChanged(
  cb: (payload?: TasksChangedPayload) => void,
): Promise<() => void> {
  if (!inTauri) {
    const h = (e: Event) => cb((e as CustomEvent<TasksChangedPayload>).detail);
    window.addEventListener("demo-tasks-changed", h);
    return () => window.removeEventListener("demo-tasks-changed", h);
  }
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<TasksChangedPayload>("tasks-changed", (ev) => cb(ev.payload));
  return un;
}

export interface ThreadEvent {
  task_id: number;
  thread_id: number;
  kind: "delta" | "tool" | "limit" | "done";
  text: string;
  ok: boolean | null;
  resets_at?: number | null;
}

export interface RepoChange { repo: string; files: number; add: number; del: number; }
export interface ProgressItem { text: string; done: boolean; }
export interface TaskContext { touched: RepoChange[]; untouched: number; progress: ProgressItem[]; }

export async function taskContext(taskId: number): Promise<TaskContext> {
  if (!inTauri) {
    return {
      touched: [
        { repo: "server", files: 3, add: 49, del: 2 },
        { repo: "crm", files: 1, add: 12, del: 1 },
      ],
      untouched: 2,
      progress: [
        { text: "найти причину редиректа", done: true },
        { text: "исправить auth.ts", done: true },
        { text: "тест на webview-куку", done: false },
      ],
    };
  }
  return invoke<TaskContext>("task_context", { taskId });
}

export interface ThreadInfo { id: number; title: string; created_at: string; }

export async function threadsList(taskId: number): Promise<ThreadInfo[]> {
  if (!inTauri) return demoThreads[taskId] ?? [];
  return invoke<ThreadInfo[]>("threads_list", { taskId });
}

export async function threadNew(taskId: number, title: string): Promise<ThreadInfo> {
  if (!inTauri) {
    const t = { id: demoId++, title, created_at: new Date().toISOString() };
    demoThreads[taskId] = [t, ...(demoThreads[taskId] ?? [])];
    return t;
  }
  return invoke<ThreadInfo>("thread_new", { taskId, title });
}

const demoThreads: Record<number, ThreadInfo[]> = {};

export async function threadSend(taskId: number, threadId: number | null, prompt: string): Promise<number> {
  if (!inTauri) {
    let tid = threadId ?? demoThreads[taskId]?.[0]?.id;
    if (tid === undefined) { tid = demoId++; demoThreads[taskId] = [{ id: tid, title: prompt.slice(0, 30), created_at: new Date().toISOString() }]; }
    // demo: stream a scripted reply
    const chunks = [
      "## План редизайна\n\n",
      "Смотрю код… **нашёл причину**:\n",
      "- фронт: `svelte`-компоненты в `src/screens`\n- бэк: пересмотреть `api.ts`\n\n",
      "```ts\nconst url = withWebviewCookie(base);\nreturn url;\n```\n\nИсправил и добавил тест.",
    ];
    const fire = (detail: ThreadEvent, delay: number) =>
      setTimeout(() => window.dispatchEvent(new CustomEvent("demo-thread-event", { detail })), delay);
    fire({ task_id: taskId, thread_id: tid, kind: "limit", text: "five_hour", ok: null, resets_at: Math.floor(Date.now() / 1000) + 7200 }, 300);
    fire({ task_id: taskId, thread_id: tid, kind: "tool", text: "Read · server/src/auth.ts", ok: null }, 500);
    fire({ task_id: taskId, thread_id: tid, kind: "tool", text: "Bash · pnpm test auth", ok: null }, 650);
    fire({ task_id: taskId, thread_id: tid, kind: "tool", text: "Read · crm/src/session.ts", ok: null }, 800);
    chunks.forEach((c, i) => fire({ task_id: taskId, thread_id: tid, kind: "delta", text: c, ok: null }, 900 + i * 500));
    fire({ task_id: taskId, thread_id: tid, kind: "tool", text: "Edit · server/src/auth.ts", ok: null }, 1600);
    fire({ task_id: taskId, thread_id: tid, kind: "done", text: "", ok: true }, 900 + chunks.length * 500 + 300);
    return tid;
  }
  return invoke<number>("thread_send", { taskId, threadId, prompt });
}

export async function onThreadEvent(cb: (e: ThreadEvent) => void): Promise<() => void> {
  if (!inTauri) {
    const h = (e: Event) => cb((e as CustomEvent<ThreadEvent>).detail);
    window.addEventListener("demo-thread-event", h);
    return () => window.removeEventListener("demo-thread-event", h);
  }
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen<ThreadEvent>("thread-event", (ev) => cb(ev.payload));
  return un;
}

/** Native folder picker (Tauri). Returns null when cancelled or in demo mode. */
export async function pickFolder(): Promise<string | null> {
  if (!inTauri) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const res = await open({ directory: true, multiple: false, title: "Папка проекта (с git-репозиториями)" });
  return typeof res === "string" ? res : null;
}

/** Reveal the project folder in Finder (native only). */
export async function revealProject(path: string): Promise<void> {
  if (!inTauri) return;
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

export interface UpdateInfo {
  version: string;      // e.g. "0.2.0"
  current: string;
  notes: string;
  date: string;
  url: string;
  available: boolean;
}

/** Check GitHub Releases for a newer version (updater install lands in phase 9). */
export async function checkUpdate(): Promise<UpdateInfo> {
  let current = "0.1.0";
  if (inTauri) {
    const { getVersion } = await import("@tauri-apps/api/app");
    current = await getVersion();
  }
  const fallback: UpdateInfo = { version: current, current, notes: "", date: "", url: "", available: false };
  try {
    const r = await fetch("https://api.github.com/repos/freekos/gcode/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
    });
    if (!r.ok) return fallback;
    const j = await r.json();
    const version = String(j.tag_name ?? "").replace(/^v/, "");
    if (!version) return fallback;
    const newer = version.localeCompare(current, undefined, { numeric: true }) > 0;
    return {
      version,
      current,
      notes: String(j.body ?? ""),
      date: String(j.published_at ?? "").slice(0, 10),
      url: String(j.html_url ?? ""),
      available: newer,
    };
  } catch {
    return fallback;
  }
}

export async function openUrl(url: string): Promise<void> {
  if (!inTauri) {
    window.open(url, "_blank");
    return;
  }
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

/** Help -> Export logs: save the core journal to a chosen file. */
export async function exportLogs(): Promise<string | null> {
  if (!inTauri) {
    const blob = new Blob(["gcode demo logs\n"], { type: "text/plain" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = "gcode-logs.txt";
    a.click();
    return "gcode-logs.txt";
  }
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({ defaultPath: "gcode-logs.txt", title: "Экспорт логов gcode" });
  if (!path) return null;
  await invoke<number>("logs_export", { path });
  return path;
}

export interface HistoryItem { kind: "user" | "agent" | "tool"; text: string; }

/** History of the task's latest thread (from Claude's own transcript). */
export async function threadHistory(taskId: number, threadId: number | null): Promise<HistoryItem[]> {
  if (!inTauri) return [];
  return invoke<HistoryItem[]>("thread_history", { taskId, threadId });
}

export async function taskPin(taskId: number, pinned: boolean): Promise<void> {
  if (!inTauri) {
    for (const ts of Object.values(demoTaskSets)) {
      const t = ts.find((x) => x.id === taskId);
      if (t) t.pinned = pinned;
    }
    window.dispatchEvent(new CustomEvent("demo-tasks-changed", { detail: { ok: true } }));
    return;
  }
  return invoke<void>("task_pin", { taskId, pinned });
}

export async function taskArchive(taskId: number): Promise<void> {
  if (!inTauri) {
    for (const ts of Object.values(demoTaskSets)) {
      const t = ts.find((x) => x.id === taskId);
      if (t) t.archived = true;
    }
    window.dispatchEvent(new CustomEvent("demo-tasks-changed", { detail: { ok: true } }));
    return;
  }
  return invoke<void>("task_archive", { taskId });
}

export interface DiffLine { kind: "ctx" | "add" | "del"; text: string; old_no: number | null; new_no: number | null; }
export interface DiffHunk { header: string; lines: DiffLine[]; }
export interface DiffFile { path: string; status: string; add: number; del: number; hunks: DiffHunk[]; }

export async function taskDiff(taskId: number, repo: string): Promise<DiffFile[]> {
  if (!inTauri) {
    return [
      {
        path: "src/auth.ts", status: "modified", add: 2, del: 1,
        hunks: [{ header: "@@ -38,6 +38,7 @@", lines: [
          { kind: "ctx", text: "function redirect(user: User) {", old_no: 38, new_no: 38 },
          { kind: "del", text: "  return url;", old_no: 39, new_no: null },
          { kind: "add", text: "  const url = withWebviewCookie(base);", old_no: null, new_no: 39 },
          { kind: "add", text: "  return url;", old_no: null, new_no: 40 },
          { kind: "ctx", text: "}", old_no: 40, new_no: 41 },
        ]}],
      },
      {
        path: "tests/auth.test.ts", status: "added", add: 3, del: 0,
        hunks: [{ header: "@@ -0,0 +1,3 @@", lines: [
          { kind: "add", text: "it('keeps webview cookie', () => {", old_no: null, new_no: 1 },
          { kind: "add", text: "  expect(redirect(u)).toContain('wv=1');", old_no: null, new_no: 2 },
          { kind: "add", text: "});", old_no: null, new_no: 3 },
        ]}],
      },
    ];
  }
  return invoke<DiffFile[]>("task_diff", { taskId, repo });
}

export async function fileRead(taskId: number, repo: string, path: string): Promise<string> {
  if (!inTauri) return `// demo ${repo}/${path}\nexport function redirect(user: User) {\n  const url = withWebviewCookie(base);\n  return url;\n}\n`;
  return invoke<string>("file_read", { taskId, repo, path });
}

export async function fileWrite(taskId: number, repo: string, path: string, content: string): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("file_write", { taskId, repo, path, content });
}

export async function filesList(taskId: number): Promise<string[]> {
  if (!inTauri) return ["server/src/auth.ts", "server/README.md", "crm/src/api.ts"];
  return invoke<string[]>("files_list", { taskId });
}

export interface DirEntry { name: string; is_dir: boolean; branch: string | null; }

export async function projectDirList(projectId: number, rel: string): Promise<DirEntry[]> {
  if (!inTauri) {
    if (rel === "") return [
      { name: "server", is_dir: true, branch: "main" },
      { name: "crm", is_dir: true, branch: "develop" },
      { name: "docs", is_dir: true, branch: null },
      { name: "README.md", is_dir: false, branch: null },
    ];
    if (rel === "server") return [
      { name: "src", is_dir: true, branch: null },
      { name: "README.md", is_dir: false, branch: null },
    ];
    if (rel === "server/src") return [{ name: "auth.ts", is_dir: false, branch: null }];
    return [];
  }
  return invoke<DirEntry[]>("project_dir_list", { projectId, rel });
}

export async function taskDirList(taskId: number, rel: string): Promise<DirEntry[]> {
  if (!inTauri) {
    if (rel === "") return [
      { name: "server", is_dir: true, branch: "fix-login-redirect" },
      { name: "crm", is_dir: true, branch: "fix-login-redirect" },
    ];
    if (rel === "server") return [
      { name: "src", is_dir: true, branch: null },
      { name: "README.md", is_dir: false, branch: null },
    ];
    if (rel === "server/src") return [{ name: "auth.ts", is_dir: false, branch: null }];
    return [];
  }
  return invoke<DirEntry[]>("task_dir_list", { taskId, rel });
}

export async function projectFileRead(projectId: number, rel: string): Promise<string> {
  if (!inTauri) return `// demo project file ${rel}\n`;
  return invoke<string>("project_file_read", { projectId, rel });
}

export async function projectFileWrite(projectId: number, rel: string, content: string): Promise<void> {
  if (!inTauri) return;
  return invoke<void>("project_file_write", { projectId, rel, content });
}

export async function threadStop(taskId: number, threadId: number, force: boolean): Promise<void> {
  if (!inTauri) {
    window.dispatchEvent(new CustomEvent("demo-thread-event", { detail: { task_id: taskId, thread_id: threadId, kind: "done", text: "остановлено", ok: false } }));
    return;
  }
  return invoke<void>("thread_stop", { taskId, threadId, force });
}

export async function progressRead(taskId: number): Promise<string> {
  if (!inTauri) return "# Goal\nПочинить редирект после логина.\n\n## Чеклист\n- [x] найти причину редиректа\n- [x] исправить auth.ts\n- [ ] тест на webview-куку\n";
  return invoke<string>("progress_read", { taskId });
}

export async function onThreadsChanged(cb: (e: { task_id: number }) => void): Promise<() => void> {
  if (!inTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return await listen<{ task_id: number }>("threads-changed", (ev) => cb(ev.payload));
}

/** [+] in the composer: system file picker -> attachment chip {loc, code}. */
export async function pickAttachment(): Promise<{ loc: string; code: string } | null> {
  if (!inTauri) return { loc: "LOYALTY_SPEC.md", code: "# демо-вложение\n…" };
  const { open } = await import("@tauri-apps/plugin-dialog");
  const path = await open({ multiple: false, directory: false, title: "Файл к промпту" });
  if (typeof path !== "string") return null;
  const r = await invoke<{ name: string; text: string | null; reason?: string }>("attach_read", { path });
  return { loc: r.name, code: r.text ?? `[${r.reason}]\nпуть: ${path}` };
}

export const isDemo = !inTauri;
