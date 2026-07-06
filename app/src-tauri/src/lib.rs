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
    /// live agent session per THREAD (pattern B: one persistent process, many turns)
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
    pinned: bool,
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
                    pinned: t.pinned,
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
        pinned: false,
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
    thread_id: i64,
    kind: String, // "delta" | "tool" | "limit" | "done"
    text: String,
    ok: Option<bool>,
    resets_at: Option<i64>,
}

/// Send a message into a LIVE agent session of the task (pattern B).
/// thread_id=None -> latest thread (created if missing). Returns the thread id.
#[tauri::command]
fn thread_send(
    app_handle: AppHandle,
    app: TState<'_, App>,
    task_id: i64,
    thread_id: Option<i64>,
    prompt: String,
) -> Result<i64, String> {
    app.handle
        .call(move |st| st.try_start_agent(task_id))
        .map_err(err_s)?;

    let thread = match thread_id {
        Some(tid) => app
            .handle
            .call(move |st| st.list_threads(task_id))
            .map_err(err_s)?
            .into_iter()
            .find(|t| t.id == tid)
            .ok_or_else(|| format!("thread #{tid} not found"))?,
        None => {
            let existing = app
                .handle
                .call(move |st| st.list_threads(task_id))
                .map_err(err_s)?
                .into_iter()
                .next();
            match existing {
                Some(t) => t,
                None => {
                    let t = app
                        .handle
                        .call({
                            let title: String = prompt.chars().take(48).collect();
                            move |st| st.add_thread(task_id, "claude", &title)
                        })
                        .map_err(err_s)?;
                    // AI-name the thread from the first message (like chat apps)
                    let handle = app.handle.clone();
                    let ah = app_handle.clone();
                    let p2 = prompt.clone();
                    let tid0 = t.id;
                    std::thread::spawn(move || {
                        let names =
                            namer::suggest_names("claude", &p2, std::time::Duration::from_secs(20));
                        if names.ai {
                            let title = names.title.clone();
                            handle.call(move |st| {
                                let _ = st.set_thread_title(tid0, &title);
                            });
                            let _ = ah.emit("threads-changed", serde_json::json!({ "task_id": task_id }));
                        }
                    });
                    t
                }
            }
        }
    };
    let tid = thread.id;

    let mut sessions = app.sessions.lock().unwrap();
    let need_spawn = match sessions.get_mut(&tid) {
        Some(s) => !s.alive(),
        None => true,
    };
    if need_spawn {
        let root = gcode_core::runner::task_root(&app.handle, task_id).map_err(err_s)?;
        let handle = app.handle.clone();
        let ah = app_handle.clone();
        let session = LiveSession::spawn(
            "claude",
            &root,
            thread.session_id.as_deref(),
            move |ev| {
                let payload = match &ev {
                    AgentEvent::Session(sid) => {
                        let sid2 = sid.clone();
                        handle.call(move |st| {
                            let _ = st.set_thread_session(tid, &sid2);
                        });
                        return;
                    }
                    AgentEvent::TextDelta(t) | AgentEvent::WholeText(t) => ThreadEvent {
                        task_id,
                        thread_id: tid,
                        kind: "delta".into(),
                        text: t.clone(),
                        ok: None,
                        resets_at: None,
                    },
                    AgentEvent::ToolUse(n, d) => ThreadEvent {
                        task_id,
                        thread_id: tid,
                        kind: "tool".into(),
                        text: if d.is_empty() { n.clone() } else { format!("{n} · {d}") },
                        ok: None,
                        resets_at: None,
                    },
                    AgentEvent::RateLimit { kind, resets_at } => ThreadEvent {
                        task_id,
                        thread_id: tid,
                        kind: "limit".into(),
                        text: kind.clone(),
                        ok: None,
                        resets_at: Some(*resets_at),
                    },
                    AgentEvent::Done { ok, error } => {
                        handle.call(move |st| {
                            let _ = st.finish_agent(task_id);
                            let _ = st.touch_thread(tid);
                        });
                        let _ = ah.emit("tasks-changed", serde_json::json!({ "ok": true }));
                        ThreadEvent {
                            task_id,
                            thread_id: tid,
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
        sessions.insert(tid, session);
    }
    if let Some(sess) = sessions.get_mut(&tid) {
        if let Err(e) = sess.send(&prompt) {
            sessions.remove(&tid);
            let _ = app.handle.call(move |st| st.finish_agent(task_id));
            return Err(err_s(e));
        }
    }
    Ok(tid)
}

#[derive(serde::Serialize)]
struct ThreadDto {
    id: i64,
    title: String,
    created_at: String,
}

#[tauri::command]
fn threads_list(app: TState<'_, App>, task_id: i64) -> Result<Vec<ThreadDto>, String> {
    let threads = app
        .handle
        .call(move |st| st.list_threads(task_id))
        .map_err(err_s)?;
    Ok(threads
        .into_iter()
        .map(|t| ThreadDto { id: t.id, title: t.title, created_at: t.created_at })
        .collect())
}

#[tauri::command]
fn thread_new(app: TState<'_, App>, task_id: i64, title: String) -> Result<ThreadDto, String> {
    let t = app
        .handle
        .call(move |st| st.add_thread(task_id, "claude", &title))
        .map_err(err_s)?;
    Ok(ThreadDto { id: t.id, title: t.title, created_at: t.created_at })
}

/// Stop the running turn: soft = control-protocol interrupt (process lives on);
/// force = kill the session (it respawns with --resume on the next send).
#[tauri::command]
fn thread_stop(
    app_handle: AppHandle,
    app: TState<'_, App>,
    task_id: i64,
    thread_id: i64,
    force: bool,
) -> Result<(), String> {
    let mut sessions = app.sessions.lock().unwrap();
    if force {
        if let Some(mut s) = sessions.remove(&thread_id) {
            s.kill();
        }
        let _ = app.handle.call(move |st| st.finish_agent(task_id));
        let _ = app_handle.emit(
            "thread-event",
            ThreadEvent { task_id, thread_id, kind: "done".into(), text: "остановлено".into(), ok: Some(false), resets_at: None },
        );
        let _ = app_handle.emit("tasks-changed", serde_json::json!({ "ok": true }));
        return Ok(());
    }
    if let Some(s) = sessions.get_mut(&thread_id) {
        s.interrupt().map_err(err_s)?;
    } else {
        let _ = app.handle.call(move |st| st.finish_agent(task_id));
    }
    Ok(())
}

/// History of the task's latest thread, read back from Claude's own transcript.
#[tauri::command]
fn thread_history(
    app: TState<'_, App>,
    task_id: i64,
    thread_id: Option<i64>,
) -> Result<Vec<gcode_core::transcript::HistoryItem>, String> {
    let threads = app
        .handle
        .call(move |st| st.list_threads(task_id))
        .map_err(err_s)?;
    let thread = match thread_id {
        Some(tid) => threads.into_iter().find(|t| t.id == tid),
        None => threads.into_iter().next(),
    };
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

/// Working-tree diff of one repo of a task (review loop).
#[tauri::command]
fn task_diff(
    app: TState<'_, App>,
    task_id: i64,
    repo: String,
) -> Result<Vec<gcode_core::diff::DiffFile>, String> {
    gcode_core::diff::task_diff(&app.handle, task_id, &repo).map_err(err_s)
}

#[tauri::command]
fn file_read(app: TState<'_, App>, task_id: i64, repo: String, path: String) -> Result<String, String> {
    gcode_core::files::read_file(&app.handle, task_id, &repo, &path).map_err(err_s)
}

#[tauri::command]
fn file_write(
    app: TState<'_, App>,
    task_id: i64,
    repo: String,
    path: String,
    content: String,
) -> Result<(), String> {
    gcode_core::files::write_file(&app.handle, task_id, &repo, &path, &content).map_err(err_s)
}

#[tauri::command]
fn files_list(app: TState<'_, App>, task_id: i64) -> Result<Vec<String>, String> {
    gcode_core::files::list_files(&app.handle, task_id).map_err(err_s)
}

#[tauri::command]
fn project_dir_list(
    app: TState<'_, App>,
    project_id: i64,
    rel: String,
) -> Result<Vec<gcode_core::files::DirEntry>, String> {
    gcode_core::files::project_list_dir(&app.handle, project_id, &rel).map_err(err_s)
}

/// PROGRESS.md of the task (goal/checklist file in the task root).
/// Read a user-picked file to attach to the prompt (size-capped, text only).
#[tauri::command]
fn attach_read(path: String) -> Result<serde_json::Value, String> {
    let p = std::path::Path::new(&path);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(path.clone());
    let meta = std::fs::metadata(p).map_err(|e| e.to_string())?;
    if meta.len() > 256 * 1024 {
        return Ok(serde_json::json!({ "name": name, "text": null, "reason": "файл больше 256KB — приложи путь" }));
    }
    match std::fs::read_to_string(p) {
        Ok(t) => {
            let capped: String = t.lines().take(400).collect::<Vec<_>>().join("\n");
            Ok(serde_json::json!({ "name": name, "text": capped }))
        }
        Err(_) => Ok(serde_json::json!({ "name": name, "text": null, "reason": "бинарный файл" })),
    }
}

#[tauri::command]
fn progress_read(app: TState<'_, App>, task_id: i64) -> Result<String, String> {
    let root = gcode_core::runner::task_root(&app.handle, task_id).map_err(err_s)?;
    std::fs::read_to_string(root.join("PROGRESS.md")).map_err(|e| e.to_string())
}

#[tauri::command]
fn task_dir_list(
    app: TState<'_, App>,
    task_id: i64,
    rel: String,
) -> Result<Vec<gcode_core::files::DirEntry>, String> {
    gcode_core::files::task_list_dir(&app.handle, task_id, &rel).map_err(err_s)
}

#[tauri::command]
fn project_file_read(app: TState<'_, App>, project_id: i64, rel: String) -> Result<String, String> {
    gcode_core::files::project_read_file(&app.handle, project_id, &rel).map_err(err_s)
}

#[tauri::command]
fn project_file_write(
    app: TState<'_, App>,
    project_id: i64,
    rel: String,
    content: String,
) -> Result<(), String> {
    gcode_core::files::project_write_file(&app.handle, project_id, &rel, &content).map_err(err_s)
}

#[tauri::command]
fn task_pin(app: TState<'_, App>, task_id: i64, pinned: bool) -> Result<(), String> {
    app.handle
        .call(move |st| st.set_task_pinned(task_id, pinned))
        .map_err(err_s)
}

/// Archive with the full core semantics: uncommitted work saved as patches,
/// worktrees removed, branches kept. Kills the live session first.
#[tauri::command]
fn task_archive(app_handle: AppHandle, app: TState<'_, App>, task_id: i64) -> Result<(), String> {
    if let Ok(threads) = app.handle.call(move |st| st.list_threads(task_id)) {
        let mut sessions = app.sessions.lock().unwrap();
        for t in threads {
            if let Some(mut s) = sessions.remove(&t.id) {
                s.kill();
            }
        }
    }
    let handle = app.handle.clone();
    let queues = app.queues.clone();
    std::thread::spawn(move || {
        let res = gcode_core::archive::archive_task_full(&handle, &queues, task_id);
        let payload = match res {
            Ok(rep) => serde_json::json!({ "ok": true, "patches": rep.patches.len() }),
            Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }),
        };
        let _ = app_handle.emit("tasks-changed", payload);
    });
    Ok(())
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
            threads_list,
            thread_new,
            thread_stop,
            thread_history,
            task_context,
            task_pin,
            task_archive,
            task_diff,
            file_read,
            file_write,
            files_list,
            project_dir_list,
            task_dir_list,
            progress_read,
            attach_read,
            project_file_read,
            project_file_write,
            logs_export,
            task_set_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
