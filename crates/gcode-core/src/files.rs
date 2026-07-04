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

// ---- project-level file access ("Show files": the tree shows the WORKING
// COPY on disk — whatever branch each repo has checked out; we surface that
// branch as a badge instead of leaving the human guessing) ----

#[derive(Debug, Clone, serde::Serialize)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    /// current branch when the entry is a git repo root
    pub branch: Option<String>,
}

const HIDDEN: &[&str] = &["node_modules", ".git", ".DS_Store", ".gcode"];

fn project_path(handle: &StateHandle, project_id: i64) -> Result<PathBuf> {
    let projects = handle.call(|st| st.list_projects())?;
    let p = projects
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| CoreError::NotFound(format!("project #{project_id}")))?;
    Ok(PathBuf::from(p.path))
}

/// One directory level (lazy tree). Dirs first, then files, both sorted.
pub fn project_list_dir(handle: &StateHandle, project_id: i64, rel: &str) -> Result<Vec<DirEntry>> {
    let root = project_path(handle, project_id)?;
    let dir = if rel.is_empty() {
        root
    } else {
        safe_join(&root, rel)?
    };
    let mut out = vec![];
    let entries = std::fs::read_dir(&dir).map_err(|e| CoreError::NotFound(e.to_string()))?;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if HIDDEN.contains(&name.as_str()) {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let branch = if is_dir && e.path().join(".git").exists() {
            git::run_git(&e.path(), &["branch", "--show-current"])
                .ok()
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty())
        } else {
            None
        };
        out.push(DirEntry {
            name,
            is_dir,
            branch,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    Ok(out)
}

pub fn project_read_file(handle: &StateHandle, project_id: i64, rel: &str) -> Result<String> {
    let root = project_path(handle, project_id)?;
    let p = safe_join(&root, rel)?;
    std::fs::read_to_string(&p).map_err(|e| CoreError::NotFound(format!("{rel}: {e}")))
}

pub fn project_write_file(
    handle: &StateHandle,
    project_id: i64,
    rel: &str,
    content: &str,
) -> Result<()> {
    let root = project_path(handle, project_id)?;
    let p = safe_join(&root, rel)?;
    std::fs::write(&p, content).map_err(|e| CoreError::Invalid(e.to_string()))
}

/// One directory level of a TASK's worktrees. rel="" lists the repos (each
/// badged with its worktree branch); deeper levels read the worktree fs.
pub fn task_list_dir(handle: &StateHandle, task_id: i64, rel: &str) -> Result<Vec<DirEntry>> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    if rel.is_empty() {
        let mut out = vec![];
        for tr in &trs {
            let wt = Path::new(&tr.worktree_path);
            if !wt.is_dir() {
                continue;
            }
            let branch = git::run_git(wt, &["branch", "--show-current"])
                .ok()
                .map(|b| b.trim().to_string())
                .filter(|b| !b.is_empty());
            out.push(DirEntry {
                name: tr.repo_name.clone(),
                is_dir: true,
                branch,
            });
        }
        return Ok(out);
    }
    let (repo, rest) = rel.split_once('/').unwrap_or((rel, ""));
    let tr = trs
        .iter()
        .find(|t| t.repo_name == repo)
        .ok_or_else(|| CoreError::NotFound(format!("repo '{repo}' in task #{task_id}")))?;
    let root = PathBuf::from(&tr.worktree_path);
    let dir = if rest.is_empty() {
        root
    } else {
        safe_join(&root, rest)?
    };
    let mut out = vec![];
    for e in std::fs::read_dir(&dir)
        .map_err(|e| CoreError::NotFound(e.to_string()))?
        .flatten()
    {
        let name = e.file_name().to_string_lossy().to_string();
        if HIDDEN.contains(&name.as_str()) {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        out.push(DirEntry {
            name,
            is_dir,
            branch: None,
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
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
