//! Task archive/restore (Phase 1 decision #3):
//! archive removes the worktrees from disk but keeps state, branches, TASK.md and —
//! when a worktree had uncommitted changes — an auto-saved patch per repo, so nothing
//! is ever lost. Restore re-attaches worktrees to the task branches and applies the
//! saved patches back.

use crate::actor::StateHandle;
use crate::error::{CoreError, Result};
use crate::git;
use crate::queues::KeyedQueues;
use std::path::Path;

/// Outcome of an archive: which repos had uncommitted changes saved into patches.
#[derive(Debug, Default)]
pub struct ArchiveReport {
    /// (repo name, patch file path)
    pub patches: Vec<(String, String)>,
}

/// Outcome of a restore: which saved patches were applied back.
#[derive(Debug, Default)]
pub struct RestoreReport {
    pub applied_patches: Vec<String>,
    /// Patches that failed to apply cleanly — kept on disk for manual recovery.
    pub failed_patches: Vec<String>,
}

/// Patch file location for a repo of a task: `<task-root>/<repo>.unsaved.patch`.
/// The task root survives archive (only worktree subfolders are removed).
fn patch_path(worktree: &Path, repo_name: &str) -> std::path::PathBuf {
    worktree
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| Path::new(".").to_path_buf())
        .join(format!("{repo_name}.unsaved.patch"))
}

/// Archive: save uncommitted work as patches, remove worktrees, flag state.
pub fn archive_task_full(
    handle: &StateHandle,
    queues: &KeyedQueues,
    task_id: i64,
) -> Result<ArchiveReport> {
    // Validate state first (double-archive fails loudly before touching disk).
    let task = handle.call(move |st| st.task_by_id(task_id))?;
    if task.archived_at.is_some() {
        return Err(CoreError::Invalid(format!(
            "task '{}' is already archived",
            task.slug
        )));
    }
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let mut report = ArchiveReport::default();

    for tr in &trs {
        let repo_path = handle.call({
            let rid = tr.repo_id;
            move |st| st.repo_path(rid)
        })?;
        let wt = Path::new(&tr.worktree_path).to_path_buf();
        let repo_name = tr.repo_name.clone();
        let patch_file = patch_path(&wt, &repo_name);
        let saved = queues.with(&repo_path, || -> Result<Option<String>> {
            if !wt.is_dir() {
                return Ok(None); // worktree already gone — nothing to save
            }
            // Any uncommitted work? (tracked or untracked)
            let dirty = git::run_git(&wt, &["status", "--porcelain"])?;
            let mut saved = None;
            if !dirty.is_empty() {
                // `add -A` + `diff --cached` captures modified AND new files in one patch.
                // node_modules is excluded explicitly: it's provisioning artifact, not work —
                // and in a repo without .gitignore it would balloon the patch to gigabytes.
                git::run_git(&wt, &["add", "-A", "--", ":(exclude)node_modules"])?;
                let patch = git::run_git(&wt, &["diff", "--cached", "--binary", "HEAD"])?;
                if !patch.is_empty() {
                    std::fs::write(&patch_file, format!("{patch}\n")).map_err(|e| {
                        CoreError::Invalid(format!(
                            "cannot save patch {}: {e}",
                            patch_file.display()
                        ))
                    })?;
                    saved = Some(patch_file.to_string_lossy().to_string());
                }
            }
            let repo = Path::new(&repo_path);
            git::run_git(
                repo,
                &["worktree", "remove", "--force", &wt.to_string_lossy()],
            )?;
            let _ = git::run_git(repo, &["worktree", "prune"]);
            Ok(saved)
        })?;
        if let Some(p) = saved {
            report.patches.push((repo_name, p));
        }
    }

    handle.call(move |st| st.archive_task(task_id))?;
    Ok(report)
}

/// Restore: flag state, re-attach worktrees to task branches, apply saved patches.
pub fn restore_task_full(
    handle: &StateHandle,
    queues: &KeyedQueues,
    task_id: i64,
) -> Result<RestoreReport> {
    let task = handle.call(move |st| st.task_by_id(task_id))?;
    if task.archived_at.is_none() {
        return Err(CoreError::Invalid(format!(
            "task '{}' is not archived",
            task.slug
        )));
    }
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let mut report = RestoreReport::default();

    for tr in &trs {
        let repo_path = handle.call({
            let rid = tr.repo_id;
            move |st| st.repo_path(rid)
        })?;
        let wt = Path::new(&tr.worktree_path).to_path_buf();
        let branch = task.branch.clone();
        let patch_file = patch_path(&wt, &tr.repo_name);
        let applied = queues.with(&repo_path, || -> Result<Option<(String, bool)>> {
            let repo = Path::new(&repo_path);
            if !wt.exists() {
                if let Some(parent) = wt.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                git::run_git(repo, &["worktree", "add", &wt.to_string_lossy(), &branch])?;
            }
            if patch_file.is_file() {
                let ok =
                    git::run_git(&wt, &["apply", "--index", &patch_file.to_string_lossy()]).is_ok();
                if ok {
                    let _ = std::fs::remove_file(&patch_file);
                }
                return Ok(Some((patch_file.to_string_lossy().to_string(), ok)));
            }
            Ok(None)
        })?;
        if let Some((p, ok)) = applied {
            if ok {
                report.applied_patches.push(p);
            } else {
                report.failed_patches.push(p);
            }
        }
    }

    handle.call(move |st| st.restore_task(task_id))?;
    Ok(report)
}
