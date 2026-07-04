//! Working-tree diff for the review loop (phase 4): full `git diff` of a task's
//! worktree parsed into files/hunks/lines with line numbers — the UI renders,
//! the human comments on lines, the agent gets precise context.

use crate::actor::StateHandle;
use crate::error::{CoreError, Result};
use crate::git;
use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffLine {
    /// "ctx" | "add" | "del"
    pub kind: String,
    pub text: String,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DiffFile {
    pub path: String,
    /// "modified" | "added" | "deleted" | "renamed"
    pub status: String,
    pub add: usize,
    pub del: usize,
    pub hunks: Vec<DiffHunk>,
}

/// Full uncommitted diff of one repo worktree of a task (tracked changes vs HEAD
/// + untracked files rendered as additions). Read-only, safe outside queues.
pub fn task_diff(handle: &StateHandle, task_id: i64, repo: &str) -> Result<Vec<DiffFile>> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let tr = trs
        .iter()
        .find(|t| t.repo_name == repo)
        .ok_or_else(|| CoreError::NotFound(format!("repo '{repo}' in task #{task_id}")))?;
    let wt = Path::new(&tr.worktree_path);
    if !wt.is_dir() {
        return Ok(vec![]);
    }
    let raw = git::run_git(wt, &["diff", "HEAD", "--no-color"]).unwrap_or_default();
    let mut files = parse_unified_diff(&raw);

    // untracked files are invisible to `diff HEAD` — show them as pure additions
    let untracked =
        git::run_git(wt, &["ls-files", "--others", "--exclude-standard"]).unwrap_or_default();
    for path in untracked.lines().filter(|l| !l.trim().is_empty()) {
        if path.starts_with("node_modules/") {
            continue;
        }
        let content = std::fs::read_to_string(wt.join(path)).unwrap_or_default();
        let lines: Vec<DiffLine> = content
            .lines()
            .take(2000)
            .enumerate()
            .map(|(i, l)| DiffLine {
                kind: "add".into(),
                text: l.to_string(),
                old_no: None,
                new_no: Some(i + 1),
            })
            .collect();
        let add = lines.len();
        files.push(DiffFile {
            path: path.to_string(),
            status: "added".into(),
            add,
            del: 0,
            hunks: vec![DiffHunk {
                header: format!("@@ -0,0 +1,{add} @@"),
                lines,
            }],
        });
    }
    Ok(files)
}

/// Parse `git diff` unified output into structured files.
pub fn parse_unified_diff(raw: &str) -> Vec<DiffFile> {
    let mut files: Vec<DiffFile> = vec![];
    let mut cur: Option<DiffFile> = None;
    let mut old_no = 0usize;
    let mut new_no = 0usize;

    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if let Some(f) = cur.take() {
                files.push(f);
            }
            // "a/path b/path" — take the b-side path
            let path = rest.split(" b/").nth(1).unwrap_or(rest).trim().to_string();
            cur = Some(DiffFile {
                path,
                status: "modified".into(),
                add: 0,
                del: 0,
                hunks: vec![],
            });
        } else if line.starts_with("new file mode") {
            if let Some(f) = cur.as_mut() {
                f.status = "added".into();
            }
        } else if line.starts_with("deleted file mode") {
            if let Some(f) = cur.as_mut() {
                f.status = "deleted".into();
            }
        } else if line.starts_with("rename from") {
            if let Some(f) = cur.as_mut() {
                f.status = "renamed".into();
            }
        } else if line.starts_with("@@") {
            if let Some(f) = cur.as_mut() {
                // @@ -old_start[,n] +new_start[,n] @@
                let header = line.to_string();
                let nums: Vec<&str> = line.split(' ').collect();
                old_no = nums
                    .get(1)
                    .and_then(|s| s.trim_start_matches('-').split(',').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                new_no = nums
                    .get(2)
                    .and_then(|s| s.trim_start_matches('+').split(',').next())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                f.hunks.push(DiffHunk {
                    header,
                    lines: vec![],
                });
            }
        } else if let Some(f) = cur.as_mut() {
            let Some(hunk) = f.hunks.last_mut() else {
                continue;
            };
            if let Some(t) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine {
                    kind: "add".into(),
                    text: t.to_string(),
                    old_no: None,
                    new_no: Some(new_no),
                });
                f.add += 1;
                new_no += 1;
            } else if let Some(t) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine {
                    kind: "del".into(),
                    text: t.to_string(),
                    old_no: Some(old_no),
                    new_no: None,
                });
                f.del += 1;
                old_no += 1;
            } else if let Some(t) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine {
                    kind: "ctx".into(),
                    text: t.to_string(),
                    old_no: Some(old_no),
                    new_no: Some(new_no),
                });
                old_no += 1;
                new_no += 1;
            }
            // "\\ No newline at end of file" and друг lines are skipped
        }
    }
    if let Some(f) = cur.take() {
        files.push(f);
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modified_file_with_line_numbers() {
        let raw = "diff --git a/src/auth.ts b/src/auth.ts\nindex 111..222 100644\n--- a/src/auth.ts\n+++ b/src/auth.ts\n@@ -10,4 +10,5 @@ fn ctx\n line-a\n-old line\n+new line\n+added line\n line-b\n";
        let files = parse_unified_diff(raw);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.path, "src/auth.ts");
        assert_eq!((f.add, f.del), (2, 1));
        let lines = &f.hunks[0].lines;
        assert_eq!(lines[0].kind, "ctx");
        assert_eq!(lines[0].old_no, Some(10));
        assert_eq!(lines[0].new_no, Some(10));
        assert_eq!(lines[1].kind, "del");
        assert_eq!(lines[1].old_no, Some(11));
        assert_eq!(lines[2].kind, "add");
        assert_eq!(lines[2].new_no, Some(11));
        assert_eq!(lines[3].new_no, Some(12));
        assert_eq!(lines[4].kind, "ctx");
        assert_eq!(lines[4].old_no, Some(12));
        assert_eq!(lines[4].new_no, Some(13));
    }

    #[test]
    fn detects_added_and_deleted_status() {
        let raw = "diff --git a/new.rs b/new.rs\nnew file mode 100644\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1,2 @@\n+fn a() {}\n+fn b() {}\ndiff --git a/gone.rs b/gone.rs\ndeleted file mode 100644\n--- a/gone.rs\n+++ /dev/null\n@@ -1,1 +0,0 @@\n-fn gone() {}\n";
        let files = parse_unified_diff(raw);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].status, "added");
        assert_eq!(files[0].add, 2);
        assert_eq!(files[1].status, "deleted");
        assert_eq!(files[1].del, 1);
    }

    #[test]
    fn empty_diff_is_empty() {
        assert!(parse_unified_diff("").is_empty());
    }
}
