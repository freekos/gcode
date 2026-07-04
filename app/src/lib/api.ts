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

export async function threadSend(taskId: number, prompt: string): Promise<void> {
  if (!inTauri) {
    // demo: stream a scripted reply
    const chunks = ["Смотрю код… ", "нашёл причину: ", "редирект собирался без webview-куки. ", "Исправил и добавил тест."];
    const fire = (detail: ThreadEvent, delay: number) =>
      setTimeout(() => window.dispatchEvent(new CustomEvent("demo-thread-event", { detail })), delay);
    fire({ task_id: taskId, kind: "limit", text: "five_hour", ok: null, resets_at: Math.floor(Date.now() / 1000) + 7200 }, 300);
    fire({ task_id: taskId, kind: "tool", text: "Read · server/src/auth.ts", ok: null }, 500);
    fire({ task_id: taskId, kind: "tool", text: "Bash · pnpm test auth", ok: null }, 650);
    fire({ task_id: taskId, kind: "tool", text: "Read · crm/src/session.ts", ok: null }, 800);
    chunks.forEach((c, i) => fire({ task_id: taskId, kind: "delta", text: c, ok: null }, 900 + i * 500));
    fire({ task_id: taskId, kind: "tool", text: "Edit · server/src/auth.ts", ok: null }, 1600);
    fire({ task_id: taskId, kind: "done", text: "", ok: true }, 900 + chunks.length * 500 + 300);
    return;
  }
  return invoke<void>("thread_send", { taskId, prompt });
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
export async function threadHistory(taskId: number): Promise<HistoryItem[]> {
  if (!inTauri) return [];
  return invoke<HistoryItem[]>("thread_history", { taskId });
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

export const isDemo = !inTauri;
