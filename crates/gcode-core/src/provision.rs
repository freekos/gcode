//! Task provisioning: turn "a title + repo names" into a task root with one git
//! worktree per repo (DESIGN.md §4).
//!
//! Ordering is deliberate (race-free by construction):
//! 1. RESERVE the task in state first — the actor makes slug reservation atomic,
//!    so two identical titles can never provision into the same folder.
//! 2. Provision worktrees repo by repo, each under the repo's keyed queue.
//! 3. On any git failure — compensate: remove created worktrees, delete the
//!    reserved task, journal the failure. Disk and state stay clean.

use crate::actor::StateHandle;
use crate::domain::Task;
use crate::error::{CoreError, Result};
use crate::git;
use crate::queues::KeyedQueues;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ProvisionResult {
    pub task: Task,
    pub root: PathBuf,
    /// (repo name, worktree path)
    pub worktrees: Vec<(String, PathBuf)>,
}

/// Create a task: reserve in state, then provision one worktree per repo.
/// `repo_names` empty ⇒ all repos of the project. Names fall back to
/// transliteration; prefer `provision_task_named` with AI-generated names
/// (git branches follow git conventions — see namer.rs).
pub fn provision_task(
    handle: &StateHandle,
    queues: &KeyedQueues,
    project_name: &str,
    title: &str,
    repo_names: &[String],
) -> Result<ProvisionResult> {
    let names = crate::namer::fallback(title);
    provision_task_named(handle, queues, project_name, &names, repo_names)
}

/// Same as `provision_task` but with explicit (usually AI-suggested) names.
pub fn provision_task_named(
    handle: &StateHandle,
    queues: &KeyedQueues,
    project_name: &str,
    names: &crate::namer::TaskNames,
    repo_names: &[String],
) -> Result<ProvisionResult> {
    let title = &names.title;
    let pname = project_name.to_string();
    let (project, all_repos) = handle.call(move |st| -> Result<_> {
        let p = st.project_by_name(&pname)?;
        let repos = st.project_repos(p.id)?;
        Ok((p, repos))
    })?;

    // Resolve requested repos (empty = all), reject unknown names loudly.
    let selected: Vec<_> = if repo_names.is_empty() {
        all_repos.clone()
    } else {
        let mut out = vec![];
        for want in repo_names {
            match all_repos.iter().find(|r| &r.name == want) {
                Some(r) => out.push(r.clone()),
                None => {
                    return Err(CoreError::NotFound(format!(
                        "repo '{want}' in project '{}'",
                        project.name
                    )))
                }
            }
        }
        out
    };
    if selected.is_empty() {
        return Err(CoreError::Invalid("project has no repositories".into()));
    }

    let slug = names.branch.clone();
    let branch = names.branch.clone();
    let root = Path::new(&project.path)
        .join(".gcode")
        .join("tasks")
        .join(&slug);

    // 1. Reserve in state FIRST — unique slug per project is enforced atomically
    //    by the actor+db, so a duplicate fails here before any disk work.
    let repo_worktrees: Vec<(i64, String)> = selected
        .iter()
        .map(|r| (r.id, root.join(&r.name).to_string_lossy().to_string()))
        .collect();
    let (pid, title_s, slug_s, branch_s) =
        (project.id, title.to_string(), slug.clone(), branch.clone());
    let task =
        handle.call(move |st| st.add_task(pid, &title_s, &slug_s, &branch_s, &repo_worktrees))?;

    // 2. Provision worktrees, serialized per repo.
    let mut created: Vec<(String, PathBuf)> = vec![];
    let mut failure: Option<CoreError> = None;
    for repo in &selected {
        let wt = root.join(&repo.name);
        let repo_path = Path::new(&repo.path);
        let res = queues.with(&repo.path, || -> Result<()> {
            if let Some(parent) = wt.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| CoreError::Invalid(format!("mkdir {}: {e}", parent.display())))?;
            }
            // Fetch only when the repo actually has a remote (local-only repos are fine).
            if git::run_git(repo_path, &["remote"])
                .map(|r| !r.is_empty())
                .unwrap_or(false)
            {
                let _ = git::run_git(repo_path, &["fetch", "origin", &repo.default_branch]);
            }
            // New branch from the default branch; if the branch already exists, attach to it.
            let add_new = git::run_git(
                repo_path,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &branch,
                    &wt.to_string_lossy(),
                    &repo.default_branch,
                ],
            );
            if add_new.is_err() {
                git::run_git(
                    repo_path,
                    &["worktree", "add", &wt.to_string_lossy(), &branch],
                )?;
            }
            copy_env_files(repo_path, &wt);
            clone_node_modules(repo_path, &wt);
            Ok(())
        });
        match res {
            Ok(()) => created.push((repo.name.clone(), wt)),
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }

    // 3. Compensate on failure: clean disk, drop the reserved task, journal it.
    if let Some(err) = failure {
        for (repo_name, wt) in &created {
            if let Some(repo) = selected.iter().find(|r| &r.name == repo_name) {
                let repo_path = Path::new(&repo.path).to_path_buf();
                let wt = wt.clone();
                queues.with(&repo.path, || {
                    let _ = git::run_git(
                        &repo_path,
                        &["worktree", "remove", "--force", &wt.to_string_lossy()],
                    );
                });
            }
        }
        let _ = std::fs::remove_dir_all(&root);
        let tid = task.id;
        let slug_j = slug.clone();
        handle.call(move |st| {
            let _ = st.delete_task(tid);
            let _ = st.journal_append("task.provision_failed", Some(&slug_j), None);
        });
        return Err(err);
    }

    // Task context file at the root.
    let repos_list = created
        .iter()
        .map(|(n, _)| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let _ = std::fs::write(
        root.join("TASK.md"),
        format!(
            "# {title}\n\nslug: {slug}\nbranch: {branch}\n\nRepos (worktrees in this folder):\n{repos_list}\n"
        ),
    );
    write_agent_guardrails(&root, title, &repos_list);

    Ok(ProvisionResult {
        task,
        root,
        worktrees: created,
    })
}

/// Remove the task's worktrees from disk (used by archive; state stays intact).
pub fn remove_worktrees(handle: &StateHandle, queues: &KeyedQueues, task_id: i64) -> Result<()> {
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    let repo_ids: Vec<i64> = trs.iter().map(|t| t.repo_id).collect();
    let repos = handle.call(move |st| -> Result<Vec<(i64, String)>> {
        let mut out = vec![];
        for rid in repo_ids {
            // Task repos always reference existing repos (FK) — resolve their paths.
            let path: String = st.repo_path(rid)?;
            out.push((rid, path));
        }
        Ok(out)
    })?;
    for tr in &trs {
        if let Some((_, repo_path)) = repos.iter().find(|(rid, _)| *rid == tr.repo_id) {
            let rp = repo_path.clone();
            let wt = tr.worktree_path.clone();
            queues.with(repo_path, || {
                let _ = git::run_git(Path::new(&rp), &["worktree", "remove", "--force", &wt]);
                let _ = git::run_git(Path::new(&rp), &["worktree", "prune"]);
            });
        }
    }
    Ok(())
}

/// Agent guardrails (Phase 2, "agents never touch git" — enforced, not requested):
/// `.claude/settings.json` with DENY rules for git/dangerous commands, and CLAUDE.md
/// with the task rules. Claude Code loads both from the task root (agent cwd).
pub fn write_agent_guardrails(root: &Path, title: &str, repos_list: &str) {
    let claude_dir = root.join(".claude");
    let _ = std::fs::create_dir_all(&claude_dir);
    let settings = r#"{
  "permissions": {
    "deny": [
      "Bash(git:*)",
      "Bash(sudo:*)",
      "Bash(gh:*)",
      "Bash(glab:*)"
    ]
  }
}
"#;
    let _ = std::fs::write(claude_dir.join("settings.json"), settings);
    // Goal/progress live as FILES in the task root — engine-agnostic: switching the
    // agent (Claude -> Codex) keeps the goal and progress readable by the next one.
    if !root.join("PROGRESS.md").exists() {
        let _ = std::fs::write(
            root.join("PROGRESS.md"),
            format!(
                "# Progress: {title}\n\n- [ ] (агент заполняет план здесь и отмечает сделанное)\n"
            ),
        );
    }
    let claude_md = format!(
        "# Task: {title}\n\n\
         You are working on this task inside gcode. The repos below are YOUR worktrees — \
         each subfolder is a separate repository checked out on the task branch:\n{repos_list}\n\n\
         ## Rules\n\
         - Work ONLY inside these worktree folders.\n\
         - Maintain PROGRESS.md in the task root: write your plan as a checklist and tick \
           items as you complete them. It is the single source of progress for the human \
           and for any agent that continues this task after you.\n\
         - Do NOT run git (or gh/glab). Commits, branches, merges and PRs are done by the human \
           through gcode — git commands are technically denied to you.\n\
         - Do NOT install dependencies unless the task explicitly requires it.\n\
         - When you finish, summarize what you changed per repo.\n"
    );
    let _ = std::fs::write(root.join("CLAUDE.md"), claude_md);
}

/// Copy `.env*` files (except tracked `.env.example`) from the main checkout.
fn copy_env_files(repo_path: &Path, wt: &Path) {
    if let Ok(entries) = std::fs::read_dir(repo_path) {
        for e in entries.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(".env") && name != ".env.example" && e.path().is_file() {
                let _ = std::fs::copy(e.path(), wt.join(&name));
            }
        }
    }
}

/// Give the worktree its own `node_modules` as a copy-on-write clone of the main
/// checkout's (DESIGN.md §4): instant and ~0 extra disk on APFS/btrfs, fully isolated
/// (a symlink would share caches and break Vite/esbuild). Never runs an install.
/// Cascade: `cp -c` (macOS clonefile) → `cp --reflink=auto` (Linux CoW) → plain copy.
fn clone_node_modules(repo_path: &Path, wt: &Path) {
    let src = repo_path.join("node_modules");
    if !src.is_dir() || wt.join("node_modules").exists() {
        return;
    }
    let src_s = src.to_string_lossy().to_string();
    let dst_s = wt.join("node_modules").to_string_lossy().to_string();
    let attempts: [&[&str]; 3] = [
        &["-Rc", &src_s, &dst_s],                  // macOS APFS clonefile
        &["-a", "--reflink=auto", &src_s, &dst_s], // Linux CoW (btrfs/xfs)
        &["-R", &src_s, &dst_s],                   // plain copy — correctness over speed
    ];
    for args in attempts {
        let ok = std::process::Command::new("cp")
            .args(args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            return;
        }
        // a failed attempt may leave a partial tree — clear it before the next strategy
        let _ = std::fs::remove_dir_all(wt.join("node_modules"));
    }
}
