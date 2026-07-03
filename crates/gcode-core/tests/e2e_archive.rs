//! e2e: archive/restore with the uncommitted-work patch, on REAL git repos.

use gcode_core::archive::{archive_task_full, restore_task_full};
use gcode_core::provision::provision_task;
use gcode_core::{scan, CoreError, KeyedQueues, State, StateHandle};
use std::path::Path;
use std::process::Command;

fn sh(dir: &Path, cmd: &str, args: &[&str]) {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{cmd} {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn mk_repo(root: &Path, name: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, "git", &["init", "-q", "-b", "main"]);
    sh(&dir, "git", &["config", "user.email", "t@t.t"]);
    sh(&dir, "git", &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), format!("# {name}\n")).unwrap();
    sh(&dir, "git", &["add", "."]);
    sh(&dir, "git", &["commit", "-q", "-m", "init"]);
    dir
}

fn setup() -> (StateHandle, KeyedQueues, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    mk_repo(tmp.path(), "server");
    mk_repo(tmp.path(), "crm");
    let repos = scan::discover_repos(tmp.path()).unwrap();
    let mut st = State::in_memory().unwrap();
    st.add_project("azi", &tmp.path().to_string_lossy(), &repos)
        .unwrap();
    (StateHandle::spawn(st), KeyedQueues::new(), tmp)
}

#[test]
fn dirty_archive_saves_patch_and_restore_brings_everything_back() {
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Feature X", &[]).unwrap();
    let tid = res.task.id;
    let server_wt = res.root.join("server");

    // uncommitted work: modify a tracked file AND add a brand-new one
    std::fs::write(server_wt.join("README.md"), "# changed\n").unwrap();
    std::fs::write(server_wt.join("new_file.rs"), "fn new_code() {}\n").unwrap();

    let report = archive_task_full(&h, &q, tid).unwrap();
    assert_eq!(report.patches.len(), 1, "only the dirty repo gets a patch");
    assert!(report.patches[0].1.ends_with("server.unsaved.patch"));
    assert!(!server_wt.exists(), "worktree removed from disk");
    assert!(
        res.root.join("TASK.md").exists(),
        "task root survives archive"
    );
    assert!(res.root.join("server.unsaved.patch").exists());

    let restored = restore_task_full(&h, &q, tid).unwrap();
    assert_eq!(restored.applied_patches.len(), 1);
    assert!(restored.failed_patches.is_empty());
    // both the modification and the new file are back
    assert_eq!(
        std::fs::read_to_string(server_wt.join("README.md")).unwrap(),
        "# changed\n"
    );
    assert!(
        server_wt.join("new_file.rs").exists(),
        "untracked file came back via patch"
    );
    assert!(
        !res.root.join("server.unsaved.patch").exists(),
        "applied patch is consumed"
    );
}

#[test]
fn clean_archive_produces_no_patches() {
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Clean task", &[]).unwrap();
    let report = archive_task_full(&h, &q, res.task.id).unwrap();
    assert!(
        report.patches.is_empty(),
        "clean worktrees → no patch files"
    );
    let restored = restore_task_full(&h, &q, res.task.id).unwrap();
    assert!(restored.applied_patches.is_empty());
    assert!(res.root.join("server").join("README.md").exists());
}

#[test]
fn double_archive_and_double_restore_fail_loudly_without_touching_disk() {
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "Twice", &[]).unwrap();
    let tid = res.task.id;
    archive_task_full(&h, &q, tid).unwrap();
    let err = archive_task_full(&h, &q, tid).unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)), "{err}");

    restore_task_full(&h, &q, tid).unwrap();
    let err = restore_task_full(&h, &q, tid).unwrap_err();
    assert!(matches!(err, CoreError::Invalid(_)), "{err}");
}

#[test]
fn archive_restore_cycle_preserves_branch_history() {
    let (h, q, _tmp) = setup();
    let res = provision_task(&h, &q, "azi", "With commit", &[]).unwrap();
    let server_wt = res.root.join("server");
    // commit work on the task branch, then archive/restore
    std::fs::write(server_wt.join("done.txt"), "committed work\n").unwrap();
    sh(&server_wt, "git", &["add", "."]);
    sh(&server_wt, "git", &["commit", "-q", "-m", "work"]);

    archive_task_full(&h, &q, res.task.id).unwrap();
    restore_task_full(&h, &q, res.task.id).unwrap();
    // the committed file is back because the BRANCH carried it (not a patch)
    assert!(
        server_wt.join("done.txt").exists(),
        "committed work survives via the branch"
    );
}
