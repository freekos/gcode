//! Keyed execution queues (DESIGN.md §3): at most ONE operation per key at a time.
//!
//! Used for git: the key is the repository path, so git commands against one repo are
//! strictly serialized while different repos proceed in parallel. Later the same
//! primitive backs the heavy-provisioning queue (dependency installs).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct KeyedQueues {
    inner: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl KeyedQueues {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run `f` while holding the exclusive lock for `key`.
    /// Same key ⇒ strictly one at a time; different keys ⇒ parallel.
    pub fn with<R>(&self, key: &str, f: impl FnOnce() -> R) -> R {
        let slot = {
            let mut map = self.inner.lock().expect("queues map poisoned");
            map.entry(key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = slot.lock().expect("queue slot poisoned");
        f()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Barrier;
    use std::time::Duration;

    #[test]
    fn same_key_never_overlaps() {
        let q = Arc::new(KeyedQueues::new());
        let current = Arc::new(AtomicUsize::new(0));
        let max_seen = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(8));
        let mut joins = vec![];
        for _ in 0..8 {
            let (q, cur, max, b) = (
                q.clone(),
                current.clone(),
                max_seen.clone(),
                barrier.clone(),
            );
            joins.push(std::thread::spawn(move || {
                b.wait();
                q.with("repoA", || {
                    let now = cur.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(now, Ordering::SeqCst);
                    std::thread::sleep(Duration::from_millis(5));
                    cur.fetch_sub(1, Ordering::SeqCst);
                });
            }));
        }
        for j in joins {
            j.join().unwrap();
        }
        assert_eq!(
            max_seen.load(Ordering::SeqCst),
            1,
            "two operations on the same repo must never run concurrently"
        );
    }

    #[test]
    fn different_keys_do_not_deadlock() {
        // Both sections run to completion even when entered from parallel threads —
        // per-key locks are independent (no global serialization, no deadlock).
        let q = Arc::new(KeyedQueues::new());
        let barrier = Arc::new(Barrier::new(2));
        let a = {
            let (q, b) = (q.clone(), barrier.clone());
            std::thread::spawn(move || {
                b.wait();
                q.with("repoA", || std::thread::sleep(Duration::from_millis(20)));
            })
        };
        let b_ = {
            let (q, b) = (q.clone(), barrier.clone());
            std::thread::spawn(move || {
                b.wait();
                q.with("repoB", || std::thread::sleep(Duration::from_millis(20)));
            })
        };
        a.join().unwrap();
        b_.join().unwrap();
    }

    #[test]
    fn reentrant_use_after_completion() {
        let q = KeyedQueues::new();
        let x = q.with("k", || 1) + q.with("k", || 2);
        assert_eq!(x, 3);
    }
}
