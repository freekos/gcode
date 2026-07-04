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

export const isDemo = !inTauri;
