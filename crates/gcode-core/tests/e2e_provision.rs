//! e2e: multi-repo task provisioning against REAL temporary git repos.

use gcode_core::provision::{provision_task, remove_worktrees};
use gcode_core::{scan, CoreError, KeyedQueues, State, StateHandle};
use std::path::Path;
use std::process::Command;

fn sh(dir: &Path, cmd: &str, args: &[&str]) {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("spawn {cmd}: {e}"));
    assert!(
        out.status.success(),
        "{cmd} {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn mk_repo(root: &Path, name: &str, branch: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, "git", &["init", "-q", "-b", branch]);
    sh(&dir, "git", &["config", "user.email", "t@t.t"]);
    sh(&dir, "git", &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), format!("# {name}\n")).unwrap();
    sh(&dir, "git", &["add", "."]);
    sh(&dir, "git", &["commit", "-q", "-m", "init"]);
    dir
}

/// Register a two-repo project in a fresh state; returns (handle, queues, project root).
fn setup() -> (StateHandle, KeyedQueues, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let server = mk_repo(tmp.path(), "server", "main");
    mk_repo(tmp.path(), "crm", "master");
    // a secret that must follow the worktree + a tracked example that must NOT
    std::fs::write(server.join(".env"), "SECRET=1\n").unwrap();
    std::fs::write(server.join(".env.example"), "SECRET=\n").unwrap();
    // deps dir cloned in background by provisioning
    std::fs::create_dir_all(server.join("node_modules/somelib")).unwrap();
    std::fs::write(server.join("node_modules/somelib/index.js"), "x\n").unwrap();

    let repos = scan::discover_repos(tmp.path()).unwrap();
    let mut st = State::in_memory().unwrap();
    st.add_project("azi", &tmp.path().to_string_lossy(), &repos)
        .unwrap();
    (StateHandle::spawn(st), KeyedQueues::new(), tmp)
}

#[test]
fn provisions_worktree_per_repo_with_branch_env_and_context() {
    let (h, q, tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Fix login flow", &[]).unwrap();

    // one worktree per repo under the hidden task root
    let root = tmp
        .path()
        .join(".gcode")
        .join("tasks")
        .join("fix-login-flow");
    assert_eq!(res.root, root);
    assert_eq!(res.worktrees.len(), 2);
    for (name, wt) in &res.worktrees {
        assert!(wt.exists(), "worktree {name} exists on disk");
        // the task branch is checked out in every worktree
        assert_eq!(git_out(wt, &["branch", "--show-current"]), "fix-login-flow");
    }
    // .env copied, .env.example NOT copied
    assert!(root.join("server").join(".env").exists());
    assert!(!root.join("server").join(".env.example").exists());
    // node_modules clone happens in the BACKGROUND (off the critical path) — poll
    let nm = root.join("server").join("node_modules").join("somelib");
    for _ in 0..50 {
        if nm.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    // context file at the root
    let task_md = std::fs::read_to_string(root.join("TASK.md")).unwrap();
    assert!(task_md.contains("Fix login flow"));
    // state agrees with disk
    let repos = h.call(move |st| st.task_repos(res.task.id).unwrap());
    assert_eq!(repos.len(), 2);
}

#[test]
fn subset_of_repos_and_unknown_repo_rejected() {
    let (h, q, tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Server only", &["server".into()]).unwrap();
    assert_eq!(res.worktrees.len(), 1);
    assert!(tmp.path().join(".gcode/tasks/server-only/server").exists());
    assert!(!tmp.path().join(".gcode/tasks/server-only/crm").exists());

    let err = provision_task(&h, &q, "azi", "Bad", &["nope".into()]).unwrap_err();
    assert!(matches!(err, CoreError::NotFound(_)), "{err}");
}

#[test]
fn duplicate_title_rejected_before_any_disk_work() {
    let (h, q, tmp) = setup();
    provision_task(&h, &q, "azi", "Same Thing", &[]).unwrap();
    let err = provision_task(&h, &q, "azi", "Same thing", &[]).unwrap_err();
    assert!(matches!(err, CoreError::AlreadyExists(_)), "{err}");
    // exactly one task on disk and in state
    let tasks_dir = tmp.path().join(".gcode").join("tasks");
    assert_eq!(std::fs::read_dir(&tasks_dir).unwrap().count(), 1);
}

#[test]
fn parallel_provisioning_of_distinct_tasks_all_land() {
    let (h, q, tmp) = setup();
    let q = std::sync::Arc::new(q);
    let mut joins = vec![];
    for i in 0..4 {
        let h = h.clone();
        let q = q.clone();
        joins.push(std::thread::spawn(move || {
            provision_task(&h, &q, "azi", &format!("Task number {i}"), &[]).map(|r| r.task.id)
        }));
    }
    let ids: Vec<i64> = joins
        .into_iter()
        .map(|j| j.join().unwrap().unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    let tasks_dir = tmp.path().join(".gcode").join("tasks");
    assert_eq!(
        std::fs::read_dir(&tasks_dir).unwrap().count(),
        4,
        "4 task roots on disk"
    );
    // every worktree of every task has its branch checked out — no cross-task interference
    for i in 0..4 {
        let wt = tasks_dir.join(format!("task-number-{i}")).join("server");
        assert_eq!(
            git_out(&wt, &["branch", "--show-current"]),
            format!("task-number-{i}")
        );
    }
}

#[test]
fn git_failure_compensates_state_and_disk() {
    let (h, q, tmp) = setup();
    // Sabotage: register a project whose second repo path doesn't exist on disk.
    let ghost = tmp.path().join("ghost");
    let repos = vec![
        (
            "server".to_string(),
            tmp.path().join("server").to_string_lossy().to_string(),
            "main".to_string(),
        ),
        (
            "ghost".to_string(),
            ghost.to_string_lossy().to_string(),
            "main".to_string(),
        ),
    ];
    h.call(move |st| {
        st.add_project("broken", "/tmp/broken-root", &repos)
            .unwrap()
    });

    let err = provision_task(&h, &q, "broken", "Doomed", &[]).unwrap_err();
    assert!(
        err.to_string().contains("git") || err.to_string().contains("mkdir"),
        "{err}"
    );

    // state: task is gone; journal recorded the failure
    let (n_tasks, has_fail) = h.call(|st| {
        let p = st.project_by_name("broken").unwrap();
        let n = st.list_tasks(p.id, true).unwrap().len();
        let j = st.journal_recent(10).unwrap();
        (n, j.iter().any(|(_, a, _, _)| a == "task.provision_failed"))
    });
    assert_eq!(n_tasks, 0, "reserved task removed by compensation");
    assert!(has_fail, "failure journaled");
    // disk: the partially created worktree was cleaned up
    let leftover = tmp.path().join("server");
    let wt_list = git_out(&leftover, &["worktree", "list"]);
    assert!(
        !wt_list.contains("doomed"),
        "no dangling worktree registration: {wt_list}"
    );
}

#[test]
fn remove_worktrees_frees_disk_but_keeps_branches() {
    let (h, q, tmp) = setup();
    let res = provision_task(&h, &q, "azi", "To archive", &[]).unwrap();
    let tid = res.task.id;
    remove_worktrees(&h, &q, tid).unwrap();
    assert!(!res.root.join("server").exists(), "worktree gone from disk");
    // the branch survives — restore can re-attach later
    let server = tmp.path().join("server");
    let branches = git_out(&server, &["branch", "--list", "to-archive"]);
    assert!(branches.contains("to-archive"), "branch kept: {branches}");
}

#[test]
fn task_context_reports_touched_repos_and_progress() {
    use gcode_core::context::task_context;
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Context probe", &[]).unwrap();
    // dirty one repo
    std::fs::write(res.root.join("server").join("new.rs"), "fn x() {}\n").unwrap();
    // agent-maintained progress
    std::fs::write(
        res.root.join("PROGRESS.md"),
        "- [x] понять\n- [ ] сделать\n",
    )
    .unwrap();

    let ctx = task_context(&h, res.task.id).unwrap();
    assert_eq!(
        ctx.touched.len(),
        1,
        "only the dirty repo is touched: {ctx:?}"
    );
    assert_eq!(ctx.touched[0].repo, "server");
    assert!(ctx.touched[0].files >= 1);
    assert_eq!(ctx.untouched, 1, "crm untouched");
    assert_eq!(ctx.progress.len(), 2);
    assert!(ctx.progress[0].done);
}

#[test]
fn optimistic_branch_rename_updates_worktrees_and_state() {
    use gcode_core::provision::rename_task_branch;
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "временное имя", &[]).unwrap();
    assert_eq!(
        res.task.branch, "vremennoe-imya",
        "transliterated instantly"
    );

    rename_task_branch(&h, &q, res.task.id, "fix-temp-name").unwrap();
    // branch renamed in every worktree
    for (_, wt) in &res.worktrees {
        assert_eq!(git_out(wt, &["branch", "--show-current"]), "fix-temp-name");
    }
    // and in state
    let t = h.call({
        let id = res.task.id;
        move |st| st.task_by_id(id).unwrap()
    });
    assert_eq!(t.branch, "fix-temp-name");
}

#[test]
fn task_diff_reports_modified_and_untracked() {
    use gcode_core::diff::task_diff;
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Diff probe", &[]).unwrap();
    let wt = res.root.join("server");
    // modify tracked + add untracked
    std::fs::write(wt.join("README.md"), "# changed\nline2\n").unwrap();
    std::fs::write(wt.join("brand_new.rs"), "fn hello() {}\n").unwrap();

    let files = task_diff(&h, res.task.id, "server").unwrap();
    let readme = files
        .iter()
        .find(|f| f.path == "README.md")
        .expect("README in diff");
    assert_eq!(readme.status, "modified");
    assert!(readme.add >= 1 && readme.del >= 1);
    assert!(readme.hunks[0]
        .lines
        .iter()
        .any(|l| l.kind == "add" && l.new_no.is_some()));
    let newf = files
        .iter()
        .find(|f| f.path == "brand_new.rs")
        .expect("untracked in diff");
    assert_eq!(newf.status, "added");
    assert_eq!(newf.add, 1);
    // unknown repo fails loudly
    assert!(task_diff(&h, res.task.id, "nope").is_err());
}
