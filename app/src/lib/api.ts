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
}

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

// ---- demo data (browser only) ----
const demoProjects: Project[] = [{ id: 1, name: "azi", path: "~/Codebase/work/azi", repos: 4 }];
let demoTasks: Task[] = [
  { id: 1, title: "Почини редирект после логина", slug: "pochini-redirekt", branch: "pochini-redirekt", status: "review", archived: false },
  { id: 2, title: "Добавь тесты на webview-куку", slug: "dobav-testy-webview", branch: "dobav-testy-webview", status: "running", archived: false },
  { id: 3, title: "Разберись с ценой в KG", slug: "razberis-s-tsenoy", branch: "razberis-s-tsenoy", status: "needs_input", archived: false },
  { id: 4, title: "Рефактор API клиента", slug: "refaktor-api", branch: "refaktor-api", status: "new", archived: false },
];
let demoId = 5;

export async function projectsList(): Promise<Project[]> {
  if (!inTauri) return demoProjects;
  return invoke<Project[]>("projects_list");
}

export async function projectAdd(path: string): Promise<Project> {
  if (!inTauri) throw new Error("demo mode: запусти через Tauri");
  return invoke<Project>("project_add", { path });
}

export async function tasksList(projectId: number): Promise<Task[]> {
  if (!inTauri) return demoTasks;
  return invoke<Task[]>("tasks_list", { projectId });
}

export async function taskCreate(projectId: number, prompt: string): Promise<void> {
  if (!inTauri) {
    const slug = prompt.toLowerCase().slice(0, 24).replace(/[^a-zа-яё0-9]+/gi, "-");
    demoTasks = [
      { id: demoId++, title: prompt, slug, branch: slug, status: "new", archived: false },
      ...demoTasks,
    ];
    setTimeout(() => window.dispatchEvent(new CustomEvent("demo-tasks-changed")), 400);
    return;
  }
  return invoke<void>("task_create", { projectId, prompt });
}

/** Subscribe to "tasks changed" from the core; returns unsubscribe. */
export async function onTasksChanged(cb: () => void): Promise<() => void> {
  if (!inTauri) {
    const h = () => cb();
    window.addEventListener("demo-tasks-changed", h);
    return () => window.removeEventListener("demo-tasks-changed", h);
  }
  const { listen } = await import("@tauri-apps/api/event");
  const un = await listen("tasks-changed", () => cb());
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
    fire({ task_id: taskId, kind: "tool", text: "Read", ok: null }, 500);
    chunks.forEach((c, i) => fire({ task_id: taskId, kind: "delta", text: c, ok: null }, 900 + i * 500));
    fire({ task_id: taskId, kind: "tool", text: "Edit", ok: null }, 1600);
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

export const isDemo = !inTauri;
