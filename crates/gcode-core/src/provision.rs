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

    // 2. Provision worktrees — repos IN PARALLEL (per-repo queues still serialize
    //    against other operations on the same repo). Fast path only: worktree from
    //    the LOCAL ref (no network), .env copy. Fetch and node_modules cloning are
    //    off the critical path (background, below) — review finding: sequential
    //    provisioning with fetch+deps made task creation take tens of seconds.
    let started = std::time::Instant::now();
    let results: Vec<(String, PathBuf, Result<()>)> = std::thread::scope(|scope| {
        let handles: Vec<_> = selected
            .iter()
            .map(|repo| {
                let wt = root.join(&repo.name);
                let branch = branch.clone();
                scope.spawn(move || {
                    let repo_path = Path::new(&repo.path);
                    let res = queues.with(&repo.path, || -> Result<()> {
                        if let Some(parent) = wt.parent() {
                            std::fs::create_dir_all(parent).map_err(|e| {
                                CoreError::Invalid(format!("mkdir {}: {e}", parent.display()))
                            })?;
                        }
                        // New branch from the LOCAL default ref; attach if the branch exists.
                        // Fetch only as a fallback when the local ref is missing.
                        let wt_s = wt.to_string_lossy().to_string();
                        let add_new = git::run_git(
                            repo_path,
                            &[
                                "worktree",
                                "add",
                                "-b",
                                &branch,
                                &wt_s,
                                &repo.default_branch,
                            ],
                        );
                        if add_new.is_err() {
                            let attach =
                                git::run_git(repo_path, &["worktree", "add", &wt_s, &branch]);
                            if attach.is_err() {
                                let _ = git::run_git(
                                    repo_path,
                                    &["fetch", "origin", &repo.default_branch],
                                );
                                git::run_git(
                                    repo_path,
                                    &[
                                        "worktree",
                                        "add",
                                        "-b",
                                        &branch,
                                        &wt_s,
                                        &repo.default_branch,
                                    ],
                                )?;
                            }
                        }
                        copy_env_files(repo_path, &wt);
                        Ok(())
                    });
                    (repo.name.clone(), wt, res)
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut created: Vec<(String, PathBuf)> = vec![];
    let mut failure: Option<CoreError> = None;
    for (name, wt, res) in results {
        match res {
            Ok(()) => created.push((name, wt)),
            Err(e) => failure = Some(failure.take().unwrap_or(e)),
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

    // Heavy extras run in the BACKGROUND — the task is usable immediately:
    // node_modules CoW clones + a freshness fetch per repo. Facts land in the journal.
    let elapsed = started.elapsed();
    {
        let deps: Vec<(PathBuf, PathBuf)> = selected
            .iter()
            .filter_map(|r| {
                created
                    .iter()
                    .find(|(n, _)| n == &r.name)
                    .map(|(_, wt)| (PathBuf::from(&r.path), wt.clone()))
            })
            .collect();
        let handle_bg = handle.clone();
        let slug_bg = slug.clone();
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            for (repo_path, wt) in &deps {
                clone_node_modules(repo_path, wt);
                let _ = git::run_git(repo_path, &["fetch", "origin", "--quiet"]);
            }
            let detail = format!(
                "deps+fetch for {} repo(s) in {:.1}s",
                deps.len(),
                t0.elapsed().as_secs_f32()
            );
            handle_bg.call(move |st| {
                let _ = st.journal_append("task.deps_ready", Some(&slug_bg), Some(&detail));
            });
        });
    }
    let slug_j = slug.clone();
    let detail = format!(
        "{} worktree(s) in {:.2}s; deps in background",
        created.len(),
        elapsed.as_secs_f32()
    );
    handle.call(move |st| {
        let _ = st.journal_append("task.provisioned", Some(&slug_j), Some(&detail));
    });

    Ok(ProvisionResult {
        task,
        root,
        worktrees: created,
    })
}

/// Rename the task branch in every worktree (safe before the first push) and in
/// state — the optimistic transliterated branch becomes the AI convention name.
pub fn rename_task_branch(
    handle: &StateHandle,
    queues: &KeyedQueues,
    task_id: i64,
    new_branch: &str,
) -> Result<()> {
    let task = handle.call(move |st| st.task_by_id(task_id))?;
    if task.branch == new_branch {
        return Ok(());
    }
    let trs = handle.call(move |st| st.task_repos(task_id))?;
    for tr in &trs {
        let rid = tr.repo_id;
        let repo_path = handle.call(move |st| st.repo_path(rid))?;
        let wt = Path::new(&tr.worktree_path).to_path_buf();
        let old = task.branch.clone();
        let newb = new_branch.to_string();
        queues.with(&repo_path, || {
            if wt.is_dir() {
                let _ = git::run_git(&wt, &["branch", "-m", &old, &newb]);
            }
        });
    }
    let nb = new_branch.to_string();
    handle.call(move |st| {
        st.set_task_branch(task_id, &nb)?;
        let _ = st.journal_append("task.branch_renamed", Some(&nb), None);
        Ok::<_, crate::error::CoreError>(())
    })?;
    Ok(())
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
         - Long plans, analyses and design docs go into FILES (docs/*.md inside the \
           relevant repo worktree, or PROGRESS.md) — NOT into the chat. In the chat give \
           a short summary plus the file path in backticks, e.g. `server/docs/plan.md` — \
           the human opens it as a tab.\n\
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
