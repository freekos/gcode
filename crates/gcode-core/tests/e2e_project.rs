//! e2e: project scanning + registration against REAL temporary git repos.

use gcode_core::{scan, State};
use std::path::Path;
use std::process::Command;

fn sh(dir: &Path, cmd: &str, args: &[&str]) {
    let ok = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap_or_else(|e| panic!("spawn {cmd}: {e}"))
        .status
        .success();
    assert!(ok, "{cmd} {args:?} failed in {}", dir.display());
}

/// Create a real git repo with one commit on the given branch.
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

#[test]
fn discovers_multi_repo_project_with_branches() {
    let tmp = tempfile::tempdir().unwrap();
    mk_repo(tmp.path(), "server", "main");
    mk_repo(tmp.path(), "crm", "master");
    std::fs::create_dir(tmp.path().join("not-a-repo")).unwrap(); // must be ignored
    std::fs::create_dir(tmp.path().join(".hidden")).unwrap(); // must be ignored

    let repos = scan::discover_repos(tmp.path()).unwrap();
    assert_eq!(repos.len(), 2, "plain and hidden dirs are not repos");
    assert_eq!(repos[0].0, "crm");
    assert_eq!(repos[0].2, "master", "per-repo default branch detected");
    assert_eq!(repos[1].0, "server");
    assert_eq!(repos[1].2, "main");
}

#[test]
fn root_that_is_itself_a_repo_counts_as_single_repo_project() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = mk_repo(tmp.path(), "solo", "main");
    let repos = scan::discover_repos(&dir).unwrap();
    assert_eq!(repos.len(), 1);
    assert_eq!(repos[0].0, "solo");
}

#[test]
fn scan_of_missing_dir_fails_loudly() {
    let err = scan::discover_repos(Path::new("/definitely/not/here")).unwrap_err();
    assert!(err.to_string().contains("not a directory"));
}

#[test]
fn scanned_project_registers_into_state() {
    let tmp = tempfile::tempdir().unwrap();
    mk_repo(tmp.path(), "server", "main");
    mk_repo(tmp.path(), "crm", "master");

    let repos = scan::discover_repos(tmp.path()).unwrap();
    let mut st = State::in_memory().unwrap();
    let p = st
        .add_project("azi", &tmp.path().to_string_lossy(), &repos)
        .unwrap();
    let stored = st.project_repos(p.id).unwrap();
    assert_eq!(stored.len(), 2);
    assert_eq!(stored[0].name, "crm");
    assert_eq!(stored[0].default_branch, "master");
}
