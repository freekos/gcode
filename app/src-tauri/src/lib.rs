//! Tauri bridge over gcode-core: thin DTO commands + realtime events.
//! All state access goes through the core's single-writer actor — the UI never
//! touches SQLite or git directly.

use gcode_core::engine::AgentEvent;
use gcode_core::session::LiveSession;
use gcode_core::{namer, provision, scan, KeyedQueues, State, StateHandle, TaskStatus};
use std::collections::HashMap;
use std::sync::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State as TState};

struct App {
    handle: StateHandle,
    queues: Arc<KeyedQueues>,
    /// live agent session per task (pattern B: one persistent process, many turns)
    sessions: Arc<Mutex<HashMap<i64, LiveSession>>>,
}

#[derive(Serialize, Clone)]
struct ProjectDto {
    id: i64,
    name: String,
    path: String,
    repos: usize,
}

#[derive(Serialize, Clone)]
struct TaskDto {
    id: i64,
    title: String,
    slug: String,
    branch: String,
    status: String,
    archived: bool,
    created_at: String,
}

fn err_s(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// State DB location: $GCODE_DB or ~/.gcode/gcode.db (same as the CLI —
/// the app and the CLI see the same world).
fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("GCODE_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".gcode").join("gcode.db")
}

#[tauri::command]
fn projects_list(app: TState<'_, App>) -> Result<Vec<ProjectDto>, String> {
    app.handle
        .call(|st| -> gcode_core::Result<Vec<ProjectDto>> {
            let mut out = vec![];
            for p in st.list_projects()? {
                let repos = st.project_repos(p.id)?.len();
                out.push(ProjectDto {
                    id: p.id,
                    name: p.name,
                    path: p.path,
                    repos,
                });
            }
            Ok(out)
        })
        .map_err(err_s)
}

#[tauri::command]
fn project_add(app: TState<'_, App>, path: String) -> Result<ProjectDto, String> {
    let abs = std::fs::canonicalize(&path).map_err(|e| format!("bad path {path}: {e}"))?;
    let name = abs
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "project".into());
    let repos = scan::discover_repos(&abs).map_err(err_s)?;
    if repos.is_empty() {
        return Err(format!("no git repositories found in {}", abs.display()));
    }
    let n = repos.len();
    let abs_s = abs.to_string_lossy().to_string();
    app.handle
        .call(move |st| st.add_project(&name, &abs_s, &repos))
        .map(|p| ProjectDto {
            id: p.id,
            name: p.name,
            path: p.path,
            repos: n,
        })
        .map_err(err_s)
}

#[tauri::command]
fn tasks_list(
    app: TState<'_, App>,
    project_id: i64,
    include_archived: Option<bool>,
) -> Result<Vec<TaskDto>, String> {
    let inc = include_archived.unwrap_or(false);
    app.handle
        .call(move |st| st.list_tasks(project_id, inc))
        .map(|tasks| {
            tasks
                .into_iter()
                .map(|t| TaskDto {
                    id: t.id,
                    title: t.title,
                    slug: t.slug,
                    branch: t.branch,
                    status: t.status.as_str().to_string(),
                    archived: t.archived_at.is_some(),
                    created_at: t.created_at,
                })
                .collect()
        })
        .map_err(err_s)
}

/// OPTIMISTIC task creation (review decision): provision INSTANTLY with
/// transliterated names (sub-second), return the task right away; the AI then
/// names the title + git-convention branch in the background and "task-renamed"
/// fires when it lands. The UI shows skeletons meanwhile.
#[tauri::command]
fn task_create(
    app_handle: AppHandle,
    app: TState<'_, App>,
    project_id: i64,
    prompt: String,
) -> Result<TaskDto, String> {
    let project = app
        .handle
        .call(move |st| -> gcode_core::Result<_> {
            let projects = st.list_projects()?;
            projects
                .into_iter()
                .find(|p| p.id == project_id)
                .ok_or_else(|| gcode_core::CoreError::NotFound(format!("project #{project_id}")))
        })
        .map_err(err_s)?;

    // instant path: transliterated names, parallel worktrees (fast)
    let names = namer::fallback(&prompt);
    let res = provision::provision_task_named(&app.handle, &app.queues, &project.name, &names, &[])
        .map_err(err_s)?;
    let task = res.task;
    let dto = TaskDto {
        id: task.id,
        title: task.title.clone(),
        slug: task.slug.clone(),
        branch: task.branch.clone(),
        status: task.status.as_str().to_string(),
        archived: false,
        created_at: task.created_at.clone(),
    };

    // background: AI names it properly, then rename branch + title
    let handle = app.handle.clone();
    let queues = app.queues.clone();
    let task_id = task.id;
    std::thread::spawn(move || {
        let ai = namer::suggest_names("claude", &prompt, std::time::Duration::from_secs(20));
        if ai.ai {
            let title = ai.title.clone();
            handle.call(move |st| {
                let _ = st.set_task_title(task_id, &title);
            });
            let _ = provision::rename_task_branch(&handle, &queues, task_id, &ai.branch);
        }
        let _ = app_handle.emit(
            "task-renamed",
            serde_json::json!({ "id": task_id, "ai": ai.ai }),
        );
        let _ = app_handle.emit("tasks-changed", serde_json::json!({ "ok": true }));
    });
    Ok(dto)
}

#[derive(Serialize, Clone)]
struct ThreadEvent {
    task_id: i64,
    kind: String, // "delta" | "tool" | "limit" | "done"
    text: String,
    ok: Option<bool>,
    resets_at: Option<i64>,
}

/// Send a message into the task's LIVE agent session (pattern B). The first
/// send spawns a persistent claude process (resuming the stored engine session);
/// later sends reuse it — no cold starts. Events stream as "thread-event".
#[tauri::command]
fn thread_send(
    app_handle: AppHandle,
    app: TState<'_, App>,
    task_id: i64,
    prompt: String,
) -> Result<(), String> {
    // one running agent per task (atomic via the actor)
    app.handle
        .call(move |st| st.try_start_agent(task_id))
        .map_err(err_s)?;

    let mut sessions = app.sessions.lock().unwrap();
    let need_spawn = match sessions.get_mut(&task_id) {
        Some(s) => !s.alive(),
        None => true,
    };
    if need_spawn {
        let thread = app
            .handle
            .call(move |st| st.list_threads(task_id))
            .map_err(err_s)?
            .into_iter()
            .next();
        let thread = match thread {
            Some(t) => t,
            None => app
                .handle
                .call({
                    let title: String = prompt.chars().take(48).collect();
                    move |st| st.add_thread(task_id, "claude", &title)
                })
                .map_err(err_s)?,
        };
        let root = gcode_core::runner::task_root(&app.handle, task_id).map_err(err_s)?;
        let handle = app.handle.clone();
        let ah = app_handle.clone();
        let thread_id = thread.id;
        let session = LiveSession::spawn(
            "claude",
            &root,
            thread.session_id.as_deref(),
            move |ev| {
                let payload = match &ev {
                    AgentEvent::Session(sid) => {
                        let sid2 = sid.clone();
                        handle.call(move |st| {
                            let _ = st.set_thread_session(thread_id, &sid2);
                        });
                        return;
                    }
                    AgentEvent::TextDelta(t) | AgentEvent::WholeText(t) => ThreadEvent {
                        task_id,
                        kind: "delta".into(),
                        text: t.clone(),
                        ok: None,
                        resets_at: None,
                    },
                    AgentEvent::ToolUse(n, d) => ThreadEvent {
                        task_id,
                        kind: "tool".into(),
                        text: if d.is_empty() { n.clone() } else { format!("{n} · {d}") },
                        ok: None,
                        resets_at: None,
                    },
                    AgentEvent::RateLimit { kind, resets_at } => ThreadEvent {
                        task_id,
                        kind: "limit".into(),
                        text: kind.clone(),
                        ok: None,
                        resets_at: Some(*resets_at),
                    },
                    AgentEvent::Done { ok, error } => {
                        handle.call(move |st| {
                            let _ = st.finish_agent(task_id);
                            let _ = st.touch_thread(thread_id);
                        });
                        let _ = ah.emit("tasks-changed", serde_json::json!({ "ok": true }));
                        ThreadEvent {
                            task_id,
                            kind: "done".into(),
                            text: error.clone().unwrap_or_default(),
                            ok: Some(*ok),
                            resets_at: None,
                        }
                    }
                };
                let _ = ah.emit("thread-event", payload);
            },
        )
        .map_err(err_s)?;
        sessions.insert(task_id, session);
    }
    if let Some(sess) = sessions.get_mut(&task_id) {
        if let Err(e) = sess.send(&prompt) {
            sessions.remove(&task_id);
            let _ = app.handle.call(move |st| st.finish_agent(task_id));
            return Err(err_s(e));
        }
    }
    Ok(())
}

/// History of the task's latest thread, read back from Claude's own transcript.
#[tauri::command]
fn thread_history(
    app: TState<'_, App>,
    task_id: i64,
) -> Result<Vec<gcode_core::transcript::HistoryItem>, String> {
    let thread = app
        .handle
        .call(move |st| st.list_threads(task_id))
        .map_err(err_s)?
        .into_iter()
        .next();
    let Some(thread) = thread else {
        return Ok(vec![]);
    };
    let Some(sid) = thread.session_id else {
        return Ok(vec![]);
    };
    let root = gcode_core::runner::task_root(&app.handle, task_id).map_err(err_s)?;
    Ok(gcode_core::transcript::load_history(&root, &sid))
}

#[tauri::command]
fn task_context(app: TState<'_, App>, task_id: i64) -> Result<gcode_core::context::TaskContext, String> {
    gcode_core::context::task_context(&app.handle, task_id).map_err(err_s)
}

/// Export the core journal to a text file (Help -> Export logs).
#[tauri::command]
fn logs_export(app: TState<'_, App>, path: String) -> Result<usize, String> {
    let lines = app
        .handle
        .call(|st| st.journal_recent(2000))
        .map_err(err_s)?;
    let mut out = String::from("gcode journal export\n\n");
    for (ts, action, entity, detail) in &lines {
        out.push_str(&format!(
            "{ts}  {action:<20} {:<24} {}\n",
            entity.clone().unwrap_or_default(),
            detail.clone().unwrap_or_default()
        ));
    }
    std::fs::write(&path, &out).map_err(|e| format!("cannot write {path}: {e}"))?;
    Ok(lines.len())
}

#[tauri::command]
fn task_set_status(app: TState<'_, App>, task_id: i64, status: String) -> Result<(), String> {
    let st = TaskStatus::parse(&status).ok_or_else(|| format!("unknown status {status}"))?;
    app.handle
        .call(move |s| s.set_task_status(task_id, st))
        .map_err(err_s)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let path = db_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let state = State::open(&path).expect("cannot open gcode state db");
    let app = App {
        handle: StateHandle::spawn(state),
        queues: Arc::new(KeyedQueues::new()),
        sessions: Arc::new(Mutex::new(HashMap::new())),
    };
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // macOS: translucent sidebar material behind transparent regions
            #[cfg(target_os = "macos")]
            if let Some(w) = app.get_webview_window("main") {
                // rounded corners for the vibrancy layer itself — a transparent
                // window loses the system rounding, leaving square glass corners
                let _ = window_vibrancy::apply_vibrancy(
                    &w,
                    window_vibrancy::NSVisualEffectMaterial::Sidebar,
                    None,
                    Some(14.0),
                );
            }
            Ok(())
        })
        .manage(app)
        .invoke_handler(tauri::generate_handler![
            projects_list,
            project_add,
            tasks_list,
            task_create,
            thread_send,
            thread_history,
            task_context,
            logs_export,
            task_set_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
