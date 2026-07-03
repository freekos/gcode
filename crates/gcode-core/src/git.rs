//! Thin wrapper around the system `git` binary.
//!
//! We deliberately shell out to real git instead of linking a git library: worktree
//! semantics, credentials, hooks and edge cases behave exactly like the user's
//! terminal. Callers serialize commands per repository via `KeyedQueues` (DESIGN.md §3).

use crate::error::{CoreError, Result};
use std::path::Path;
use std::process::Command;

/// Run a git command in `repo`, returning trimmed stdout on success.
pub fn run_git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .map_err(|e| CoreError::Invalid(format!("cannot spawn git: {e}")))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(CoreError::Invalid(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            repo.display(),
            err.lines().next().unwrap_or("unknown error")
        )))
    }
}

/// Is this directory a git repository checkout (has `.git`)?
pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists()
}

/// Best-effort default branch of a repo:
/// origin/HEAD if set, else the currently checked-out branch, else "main".
pub fn default_branch(repo: &Path) -> String {
    if let Ok(s) = run_git(
        repo,
        &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
    ) {
        if let Some(b) = s.strip_prefix("origin/") {
            return b.to_string();
        }
    }
    if let Ok(s) = run_git(repo, &["symbolic-ref", "--short", "HEAD"]) {
        if !s.is_empty() {
            return s;
        }
    }
    "main".to_string()
}
