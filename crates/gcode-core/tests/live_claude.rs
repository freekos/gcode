//! LIVE tests against the real Claude Code binary on the local subscription.
//! Ignored by default (CI has no Claude login). Run locally:
//!
//!   cargo test -p gcode-core --test live_claude -- --ignored --nocapture
//!
//! These are the "catch real cases" tests (phase 2 decision #3): they exercise the
//! actual wire format, the deny-git guardrails and session resume end to end.

use gcode_core::engine::ClaudeEngine;
use gcode_core::provision::provision_task;
use gcode_core::runner::run_thread;
use gcode_core::{scan, KeyedQueues, State, StateHandle};
use std::path::Path;
use std::process::Command;

fn sh(dir: &Path, cmd: &str, args: &[&str]) {
    let out = Command::new(cmd)
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "{cmd} {args:?}");
}

fn setup() -> (StateHandle, KeyedQueues, tempfile::TempDir, i64) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("server");
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, "git", &["init", "-q", "-b", "main"]);
    sh(&dir, "git", &["config", "user.email", "t@t.t"]);
    sh(&dir, "git", &["config", "user.name", "t"]);
    std::fs::write(dir.join("README.md"), "# server\n").unwrap();
    sh(&dir, "git", &["add", "."]);
    sh(&dir, "git", &["commit", "-q", "-m", "init"]);

    let repos = scan::discover_repos(tmp.path()).unwrap();
    let mut st = State::in_memory().unwrap();
    st.add_project("live", &tmp.path().to_string_lossy(), &repos)
        .unwrap();
    let h = StateHandle::spawn(st);
    let q = KeyedQueues::new();
    let res = provision_task(&h, &q, "live", "live check", &[]).unwrap();
    (h, q, tmp, res.task.id)
}

#[test]
#[ignore = "needs a logged-in claude binary; run locally"]
fn live_roundtrip_session_resume_and_deny_git() {
    let (h, _q, _tmp, task_id) = setup();
    let eng = ClaudeEngine::default();

    // 1. New thread: reply arrives, session id persisted.
    let out = run_thread(
        &h,
        &eng,
        task_id,
        None,
        "Reply with exactly: live-ok",
        &mut |_| {},
    )
    .unwrap();
    assert!(out.ok, "agent failed: {:?}", out.error);
    assert!(out.text.contains("live-ok"), "got: {}", out.text);
    let sid = out.thread.session_id.clone().expect("session id persisted");

    // 2. Continue: the SAME session is resumed (Claude keeps the transcript).
    let out2 = run_thread(
        &h,
        &eng,
        task_id,
        Some(out.thread.id),
        "What exact phrase did I ask you to reply with in my previous message? Answer with the phrase only.",
        &mut |_| {},
    )
    .unwrap();
    assert!(out2.ok);
    assert_eq!(
        out2.thread.session_id.as_deref(),
        Some(sid.as_str()),
        "same session"
    );
    assert!(
        out2.text.contains("live-ok"),
        "resume lost context: {}",
        out2.text
    );

    // 3. Deny-git: the agent must be REFUSED when trying git (guardrails from provisioning).
    let out3 = run_thread(
        &h,
        &eng,
        task_id,
        Some(out.thread.id),
        "Run 'git status' via Bash inside the server folder. If the command is blocked or denied by permissions, reply exactly: GIT-DENIED. If it executed, reply exactly: GIT-WORKED.",
        &mut |_| {},
    )
    .unwrap();
    assert!(
        out3.text.contains("GIT-DENIED"),
        "agent must not be able to run git, got: {}",
        out3.text
    );
}
