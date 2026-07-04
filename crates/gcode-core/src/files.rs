//! Worktree file access for the built-in editor (phase 4): read, save, list.
//! Paths are always repo-relative and validated to stay inside the worktree.

use crate::actor::StateHandle;
use crate::error::{CoreError, Result};
use crate::git;
use std::path::{Path, PathBuf};

fn worktree_of(handle: &StateHandle, task_id: i64, repo: &str) -> Result<PathBuf> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let tr = trs
        .iter()
        .find(|t| t.repo_name == repo)
        .ok_or_else(|| CoreError::NotFound(format!("repo '{repo}' in task #{task_id}")))?;
    Ok(PathBuf::from(&tr.worktree_path))
}

/// Resolve a repo-relative path, rejecting escapes ("../", absolute paths).
fn safe_join(wt: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    if rel_path.is_absolute() || rel.split('/').any(|c| c == "..") {
        return Err(CoreError::Invalid(format!("bad path: {rel}")));
    }
    Ok(wt.join(rel_path))
}

pub fn read_file(handle: &StateHandle, task_id: i64, repo: &str, path: &str) -> Result<String> {
    let wt = worktree_of(handle, task_id, repo)?;
    let p = safe_join(&wt, path)?;
    std::fs::read_to_string(&p).map_err(|e| CoreError::NotFound(format!("{path}: {e}")))
}

pub fn write_file(
    handle: &StateHandle,
    task_id: i64,
    repo: &str,
    path: &str,
    content: &str,
) -> Result<()> {
    let wt = worktree_of(handle, task_id, repo)?;
    let p = safe_join(&wt, path)?;
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CoreError::Invalid(e.to_string()))?;
    }
    std::fs::write(&p, content).map_err(|e| CoreError::Invalid(e.to_string()))
}

/// All files of every worktree of the task (for cmd-P fuzzy jump):
/// tracked + untracked, node_modules excluded, as "repo/relative/path".
pub fn list_files(handle: &StateHandle, task_id: i64) -> Result<Vec<String>> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let mut out = vec![];
    for tr in &trs {
        let wt = Path::new(&tr.worktree_path);
        if !wt.is_dir() {
            continue;
        }
        let listed = git::run_git(
            wt,
            &["ls-files", "--cached", "--others", "--exclude-standard"],
        )
        .unwrap_or_default();
        for l in listed.lines() {
            if l.is_empty() || l.starts_with("node_modules/") {
                continue;
            }
            out.push(format!("{}/{}", tr.repo_name, l));
        }
    }
    out.sort();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escaping_paths() {
        assert!(safe_join(Path::new("/wt"), "../../etc/passwd").is_err());
        assert!(safe_join(Path::new("/wt"), "/etc/passwd").is_err());
        assert!(safe_join(Path::new("/wt"), "src/../../x").is_err());
        assert_eq!(
            safe_join(Path::new("/wt"), "src/a.rs").unwrap(),
            Path::new("/wt/src/a.rs")
        );
    }
}
