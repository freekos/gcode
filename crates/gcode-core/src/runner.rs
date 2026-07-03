//! Thread runner: orchestrates one agent run inside a task (Phase 2).
//!
//! Guarantees:
//! - ONE running agent per task (atomic lock via the state actor) — two agents
//!   writing into the same worktrees would wreck each other's files.
//! - The task status reflects FACTS: running while the process lives, review when
//!   it exited — set in the same code path that owns the process, never from prose.
//! - The engine-assigned session id is persisted on first sight, so the thread can
//!   be resumed later (by gcode or by a plain `claude -c` in the task root).

use crate::actor::StateHandle;
use crate::domain::Thread;
use crate::engine::{AgentEvent, Engine, RunSpec};
use crate::error::{CoreError, Result};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ThreadRunOutcome {
    pub thread: Thread,
    /// Full assistant text of this run (deltas assembled).
    pub text: String,
    pub ok: bool,
    pub error: Option<String>,
}

/// The task root = parent of any of its worktrees (where TASK.md lives).
pub fn task_root(handle: &StateHandle, task_id: i64) -> Result<PathBuf> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let first = trs
        .first()
        .ok_or_else(|| CoreError::Invalid("task has no repos".into()))?;
    Path::new(&first.worktree_path)
        .parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| CoreError::Invalid("cannot derive task root".into()))
}

/// Start a NEW thread with `prompt`, or continue an existing one (`thread_id`).
/// Streams events to `on_event` as they happen; returns the assembled outcome.
pub fn run_thread(
    handle: &StateHandle,
    engine: &dyn Engine,
    task_id: i64,
    thread_id: Option<i64>,
    prompt: &str,
    on_event: &mut dyn FnMut(&AgentEvent),
) -> Result<ThreadRunOutcome> {
    let root = task_root(handle, task_id)?;
    if !root.is_dir() {
        return Err(CoreError::Invalid(format!(
            "task root missing on disk: {} (archived?)",
            root.display()
        )));
    }

    // Resolve or create the thread BEFORE taking the run lock (cheap metadata).
    let thread = match thread_id {
        Some(id) => {
            let t = handle.call(move |st| st.thread_by_id(id))?;
            if t.task_id != task_id {
                return Err(CoreError::Invalid("thread belongs to another task".into()));
            }
            t
        }
        None => {
            let title: String = prompt.chars().take(48).collect();
            let engine_name = engine.name().to_string();
            handle.call(move |st| st.add_thread(task_id, &engine_name, &title))?
        }
    };

    // ONE agent per task: atomic check-and-set through the actor.
    handle.call(move |st| st.try_start_agent(task_id))?;

    // From here on the lock is held — release it on EVERY path (incl. engine errors).
    let mut text = String::new();
    let mut ok = false;
    let mut error = None;
    let tid_for_session = thread.id;
    let run_res = engine.run(
        RunSpec {
            cwd: &root,
            prompt,
            resume: thread.session_id.as_deref(),
        },
        &mut |ev| {
            match &ev {
                AgentEvent::Session(sid) => {
                    let sid = sid.clone();
                    handle.call(move |st| {
                        let _ = st.set_thread_session(tid_for_session, &sid);
                    });
                }
                AgentEvent::TextDelta(t) => text.push_str(t),
                AgentEvent::WholeText(t) if text.is_empty() => text.push_str(t),
                AgentEvent::Done { ok: o, error: e } => {
                    ok = *o;
                    error = e.clone();
                }
                _ => {}
            }
            on_event(&ev);
        },
    );

    // Release the lock (facts: the process is gone) and touch activity.
    handle.call(move |st| {
        let _ = st.finish_agent(task_id);
        let _ = st.touch_thread(tid_for_session);
        let _ = st.journal_append(
            "thread.run",
            Some(&format!("task#{task_id}/thread#{tid_for_session}")),
            None,
        );
    });
    run_res?;

    let thread = handle.call(move |st| st.thread_by_id(tid_for_session))?;
    Ok(ThreadRunOutcome {
        thread,
        text,
        ok,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TaskStatus;
    use crate::state::State;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    /// Mock engine: emits a scripted stream, optionally sleeping to simulate work.
    struct MockEngine {
        session: String,
        reply: String,
        work_ms: u64,
        concurrent: Arc<AtomicUsize>,
        max_concurrent: Arc<AtomicUsize>,
    }

    impl Engine for MockEngine {
        fn name(&self) -> &'static str {
            "mock"
        }
        fn run(&self, spec: RunSpec<'_>, on_event: &mut dyn FnMut(AgentEvent)) -> Result<()> {
            let now = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_concurrent.fetch_max(now, Ordering::SeqCst);
            on_event(AgentEvent::Session(self.session.clone()));
            // echo whether we were resumed — lets tests assert resume plumbing
            if let Some(sid) = spec.resume {
                on_event(AgentEvent::TextDelta(format!("[resumed:{sid}] ")));
            }
            std::thread::sleep(Duration::from_millis(self.work_ms));
            on_event(AgentEvent::TextDelta(self.reply.clone()));
            on_event(AgentEvent::Done {
                ok: true,
                error: None,
            });
            self.concurrent.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn mock(session: &str, reply: &str, work_ms: u64) -> MockEngine {
        MockEngine {
            session: session.into(),
            reply: reply.into(),
            work_ms,
            concurrent: Arc::new(AtomicUsize::new(0)),
            max_concurrent: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// A task whose "root" really exists on disk (runner checks it).
    fn setup(tmp: &tempfile::TempDir) -> (StateHandle, i64) {
        let root = tmp.path().join(".gcode/tasks/a");
        std::fs::create_dir_all(root.join("server")).unwrap();
        let mut st = State::in_memory().unwrap();
        let p = st
            .add_project(
                "azi",
                &tmp.path().to_string_lossy(),
                &[("server".into(), "/tmp/x".into(), "main".into())],
            )
            .unwrap();
        let rid = st.project_repos(p.id).unwrap()[0].id;
        let t = st
            .add_task(
                p.id,
                "A",
                "a",
                "a",
                &[(rid, root.join("server").to_string_lossy().to_string())],
            )
            .unwrap();
        (StateHandle::spawn(st), t.id)
    }

    #[test]
    fn new_thread_records_session_and_finishes_in_review() {
        let tmp = tempfile::tempdir().unwrap();
        let (h, task_id) = setup(&tmp);
        let eng = mock("sess-1", "hello", 0);
        let mut events = vec![];
        let out = run_thread(&h, &eng, task_id, None, "do it", &mut |e| {
            events.push(e.clone())
        })
        .unwrap();
        assert!(out.ok);
        assert_eq!(out.text, "hello");
        assert_eq!(out.thread.session_id.as_deref(), Some("sess-1"));
        let t = h.call(move |st| st.task_by_id(task_id).unwrap());
        assert_eq!(
            t.status,
            TaskStatus::Review,
            "facts: process exited → review"
        );
    }

    #[test]
    fn continue_resumes_with_stored_session() {
        let tmp = tempfile::tempdir().unwrap();
        let (h, task_id) = setup(&tmp);
        let eng = mock("sess-A", "first", 0);
        let out1 = run_thread(&h, &eng, task_id, None, "start", &mut |_| {}).unwrap();
        let out2 =
            run_thread(&h, &eng, task_id, Some(out1.thread.id), "more", &mut |_| {}).unwrap();
        assert!(
            out2.text.starts_with("[resumed:sess-A]"),
            "second run must resume the stored session, got: {}",
            out2.text
        );
    }

    #[test]
    fn one_agent_per_task_second_start_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let (h, task_id) = setup(&tmp);
        let concurrent = Arc::new(AtomicUsize::new(0));
        let max_c = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(4));
        let mut joins = vec![];
        for i in 0..4 {
            let h = h.clone();
            let (c, m, b) = (concurrent.clone(), max_c.clone(), barrier.clone());
            joins.push(std::thread::spawn(move || {
                let eng = MockEngine {
                    session: format!("s{i}"),
                    reply: "r".into(),
                    work_ms: 60,
                    concurrent: c,
                    max_concurrent: m,
                };
                b.wait();
                run_thread(&h, &eng, task_id, None, "go", &mut |_| {}).map(|o| o.ok)
            }));
        }
        let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
        let ok = results.iter().filter(|r| r.is_ok()).count();
        let busy = results
            .iter()
            .filter(|r| matches!(r, Err(CoreError::Invalid(m)) if m.contains("already working")))
            .count();
        assert!(ok >= 1, "at least one run wins");
        assert_eq!(
            ok + busy,
            4,
            "everyone else got a clean busy error: {results:?}"
        );
        assert_eq!(
            max_c.load(Ordering::SeqCst),
            1,
            "two agents never ran concurrently on one task"
        );
        // lock released afterwards: a new run succeeds
        let eng = mock("s-after", "again", 0);
        assert!(run_thread(&h, &eng, task_id, None, "again", &mut |_| {}).is_ok());
    }

    #[test]
    fn archived_task_refuses_agents() {
        let tmp = tempfile::tempdir().unwrap();
        let (h, task_id) = setup(&tmp);
        h.call(move |st| st.archive_task(task_id).unwrap());
        let eng = mock("s", "r", 0);
        let err = run_thread(&h, &eng, task_id, None, "go", &mut |_| {}).unwrap_err();
        assert!(err.to_string().contains("archived"), "{err}");
    }
}
