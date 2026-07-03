//! The single-writer state actor (DESIGN.md §3).
//!
//! All state mutations flow through ONE thread that owns the `State`. Callers hold a
//! cloneable `StateHandle` and submit closures; the actor runs them strictly one at a
//! time, so two mutations can never interleave — races are impossible by construction,
//! not by discipline.

use crate::state::State;
use std::sync::mpsc;

type Job = Box<dyn FnOnce(&mut State) + Send>;

/// Cloneable handle to the state actor. Cheap to clone; safe to share across threads.
#[derive(Clone)]
pub struct StateHandle {
    tx: mpsc::Sender<Job>,
}

impl StateHandle {
    /// Take ownership of the `State` and run it on a dedicated actor thread.
    /// The thread exits when every handle has been dropped.
    pub fn spawn(mut state: State) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("gcode-state".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    job(&mut state);
                }
            })
            .expect("failed to spawn state actor thread");
        StateHandle { tx }
    }

    /// Run `f` on the actor thread and wait for its result.
    ///
    /// This is the ONLY way to touch the state — there is no way to get a reference
    /// to `State` outside the actor.
    pub fn call<R, F>(&self, f: F) -> R
    where
        R: Send + 'static,
        F: FnOnce(&mut State) -> R + Send + 'static,
    {
        let (rtx, rrx) = mpsc::sync_channel::<R>(1);
        self.tx
            .send(Box::new(move |st: &mut State| {
                // Receiver dropped => caller vanished; the result is simply discarded.
                let _ = rtx.send(f(st));
            }))
            .expect("state actor is gone");
        rrx.recv().expect("state actor dropped the reply")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use std::sync::{Arc, Barrier};

    fn handle() -> StateHandle {
        StateHandle::spawn(State::in_memory().unwrap())
    }

    fn add_project(h: &StateHandle) -> (i64, i64) {
        h.call(|st| {
            let p = st
                .add_project(
                    "azi",
                    "/tmp/azi",
                    &[("server".into(), "/tmp/azi/server".into(), "main".into())],
                )
                .unwrap();
            let repo = st.project_repos(p.id).unwrap().remove(0);
            (p.id, repo.id)
        })
    }

    #[test]
    fn calls_return_values() {
        let h = handle();
        let n = h.call(|st| st.list_projects().unwrap().len());
        assert_eq!(n, 0);
    }

    #[test]
    fn parallel_task_creation_all_succeed() {
        let h = handle();
        let (pid, rid) = add_project(&h);
        let barrier = Arc::new(Barrier::new(10));
        let mut joins = vec![];
        for i in 0..10 {
            let h = h.clone();
            let b = barrier.clone();
            joins.push(std::thread::spawn(move || {
                b.wait(); // maximize contention: everyone fires at once
                h.call(move |st| {
                    st.add_task(
                        pid,
                        &format!("Task {i}"),
                        &format!("task-{i}"),
                        &format!("task-{i}"),
                        &[(rid, format!("/wt/{i}"))],
                    )
                    .map(|t| t.id)
                })
            }));
        }
        let ids: Vec<i64> = joins
            .into_iter()
            .map(|j| {
                j.join()
                    .unwrap()
                    .expect("every distinct-slug task must succeed")
            })
            .collect();
        let unique: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(unique.len(), 10, "no lost updates under contention");
        let listed = h.call(move |st| st.list_tasks(pid, false).unwrap().len());
        assert_eq!(listed, 10);
    }

    #[test]
    fn same_slug_race_exactly_one_winner() {
        let h = handle();
        let (pid, rid) = add_project(&h);
        let barrier = Arc::new(Barrier::new(8));
        let mut joins = vec![];
        for _ in 0..8 {
            let h = h.clone();
            let b = barrier.clone();
            joins.push(std::thread::spawn(move || {
                b.wait();
                h.call(move |st| {
                    st.add_task(pid, "Same", "same", "same", &[(rid, "/wt".into())])
                        .map(|_| ())
                })
            }));
        }
        let results: Vec<_> = joins.into_iter().map(|j| j.join().unwrap()).collect();
        let ok = results.iter().filter(|r| r.is_ok()).count();
        let dup = results
            .iter()
            .filter(|r| matches!(r, Err(CoreError::AlreadyExists(_))))
            .count();
        assert_eq!(ok, 1, "exactly one winner");
        assert_eq!(dup, 7, "everyone else gets a clean AlreadyExists");
    }

    #[test]
    fn parallel_archive_restore_is_serialized() {
        let h = handle();
        let (pid, rid) = add_project(&h);
        let tid = h.call(move |st| {
            st.add_task(pid, "A", "a", "a", &[(rid, "/wt".into())])
                .unwrap()
                .id
        });
        // Hammer archive/restore from many threads; the actor serializes them, so the
        // final state must be consistent and every op either succeeded or failed loudly.
        let mut joins = vec![];
        for i in 0..20 {
            let h = h.clone();
            joins.push(std::thread::spawn(move || {
                h.call(move |st| {
                    if i % 2 == 0 {
                        st.archive_task(tid).is_ok()
                    } else {
                        st.restore_task(tid).is_ok()
                    }
                })
            }));
        }
        for j in joins {
            j.join().unwrap();
        }
        // Whatever the interleaving, the task still exists and is either archived or not.
        let t = h.call(move |st| st.task_by_id(tid).unwrap());
        assert_eq!(t.slug, "a");
    }
}
