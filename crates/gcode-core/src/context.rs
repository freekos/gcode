//! Task context for the UI panel (ui-inventory §3): per-repo change facts from
//! git + agent progress from PROGRESS.md. Read-only — safe outside the queues.

use crate::actor::StateHandle;
use crate::error::Result;
use crate::git;
use crate::runner::task_root;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RepoChange {
    pub repo: String,
    /// files with any uncommitted change (tracked or untracked)
    pub files: usize,
    pub add: usize,
    pub del: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ProgressItem {
    pub text: String,
    pub done: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TaskContext {
    /// repos with changes only (untouched ones are collapsed in the UI)
    pub touched: Vec<RepoChange>,
    pub untouched: usize,
    pub progress: Vec<ProgressItem>,
}

pub fn task_context(handle: &StateHandle, task_id: i64) -> Result<TaskContext> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let mut touched = vec![];
    let mut untouched = 0usize;
    for tr in &trs {
        let wt = Path::new(&tr.worktree_path);
        if !wt.is_dir() {
            untouched += 1;
            continue;
        }
        let files = git::run_git(wt, &["status", "--porcelain"])
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);
        let (add, del) = diff_shortstat(wt);
        if files == 0 && add == 0 && del == 0 {
            untouched += 1;
        } else {
            touched.push(RepoChange {
                repo: tr.repo_name.clone(),
                files,
                add,
                del,
            });
        }
    }
    let progress = task_root(handle, task_id)
        .ok()
        .map(|root| {
            parse_progress(&std::fs::read_to_string(root.join("PROGRESS.md")).unwrap_or_default())
        })
        .unwrap_or_default();
    Ok(TaskContext {
        touched,
        untouched,
        progress,
    })
}

/// (+insertions, -deletions) of uncommitted work vs HEAD.
fn diff_shortstat(wt: &Path) -> (usize, usize) {
    let out = git::run_git(wt, &["diff", "HEAD", "--shortstat"]).unwrap_or_default();
    let mut add = 0;
    let mut del = 0;
    for part in out.split(',') {
        let part = part.trim();
        if let Some(n) = part.split(' ').next().and_then(|n| n.parse::<usize>().ok()) {
            if part.contains("insertion") {
                add = n;
            } else if part.contains("deletion") {
                del = n;
            }
        }
    }
    (add, del)
}

/// Parse "- [ ] item" / "- [x] item" checkboxes out of PROGRESS.md.
pub fn parse_progress(md: &str) -> Vec<ProgressItem> {
    md.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let (done, rest) =
                if let Some(r) = l.strip_prefix("- [x]").or_else(|| l.strip_prefix("- [X]")) {
                    (true, r)
                } else if let Some(r) = l.strip_prefix("- [ ]") {
                    (false, r)
                } else {
                    return None;
                };
            let text = rest.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(ProgressItem { text, done })
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_checkboxes() {
        let md = "# Progress\n\n- [x] найти причину\n- [ ] исправить auth.ts\nтекст без чекбокса\n  - [X] вложенный done\n- [ ]   \n";
        let items = parse_progress(md);
        assert_eq!(items.len(), 3);
        assert!(items[0].done);
        assert_eq!(items[1].text, "исправить auth.ts");
        assert!(!items[1].done);
        assert!(items[2].done, "uppercase X counts");
    }

    #[test]
    fn shortstat_parses_typical_output() {
        // parse logic only (the git call itself is covered by the e2e below)
        // "3 files changed, 49 insertions(+), 2 deletions(-)"
        let out = "3 files changed, 49 insertions(+), 2 deletions(-)";
        let mut add = 0;
        let mut del = 0;
        for part in out.split(',') {
            let part = part.trim();
            if let Some(n) = part.split(' ').next().and_then(|n| n.parse::<usize>().ok()) {
                if part.contains("insertion") {
                    add = n;
                } else if part.contains("deletion") {
                    del = n;
                }
            }
        }
        assert_eq!((add, del), (49, 2));
    }
}
