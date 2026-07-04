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
    { id: 1, title: "Почини редирект после логина", slug: "fix-login-redirect", branch: "fix-login-redirect", status: "review", archived: false, created_at: "2026-07-03 10:00:00" },
    { id: 2, title: "Добавь тесты на webview-куку", slug: "add-webview-tests", branch: "add-webview-tests", status: "running", archived: false, created_at: "2026-07-03 10:00:00" },
    { id: 4, title: "Рефактор API клиента", slug: "refactor-api-client", branch: "refactor-api-client", status: "new", archived: false, created_at: "2026-07-03 10:00:00" },
  ],
  2: [
    { id: 3, title: "Разберись с ценой в KG", slug: "fix-kg-price", branch: "fix-kg-price", status: "needs_input", archived: false, created_at: "2026-07-03 10:00:00" },
    { id: 5, title: "Лендинг лояльности", slug: "loyalty-landing", branch: "loyalty-landing", status: "done", archived: false, created_at: "2026-07-03 10:00:00" },
    { id: 6, title: "Старая миграция БД", slug: "old-db-migration", branch: "old-db-migration", status: "done", archived: true, created_at: "2026-06-20 10:00:00" },
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

export async function taskCreate(projectId: number, prompt: string): Promise<void> {
  if (!inTauri) {
    const slug = prompt.toLowerCase().slice(0, 24).replace(/[^a-zа-яё0-9]+/gi, "-");
    demoTaskSets[projectId] = [
      { id: demoId++, title: prompt, slug, branch: slug, status: "new", archived: false, created_at: new Date().toISOString().slice(0, 19).replace("T", " ") },
      ...(demoTaskSets[projectId] ?? []),
    ];
    setTimeout(
      () => window.dispatchEvent(new CustomEvent("demo-tasks-changed", { detail: { ok: true, slug } })),
      400,
    );
    return;
  }
  return invoke<void>("task_create", { projectId, prompt });
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

export const isDemo = !inTauri;
