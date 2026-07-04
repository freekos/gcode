//! AI naming (Gaziz's rule: humans write prompts, the AI names things — and git
//! artifacts follow git conventions, not transliteration).
//!
//! One fast engine call turns a task prompt into BOTH names:
//! - `title` — short human title (prompt language is fine),
//! - `branch` — English kebab-case git branch (e.g. "fix-login-redirect").
//!
//! Transliterated slugify is the FALLBACK only (engine missing/slow/garbage).
//! The branch template (prefixes like feat/) will live in settings — phase 5.

use crate::domain::slugify;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct TaskNames {
    pub title: String,
    pub branch: String,
    /// false when the fallback (transliteration) produced the branch
    pub ai: bool,
}

/// Ask the engine for names; fall back to transliteration on any failure.
/// `binary` is the claude executable ("claude" in production, a stub in tests).
pub fn suggest_names(binary: &str, prompt: &str, timeout: Duration) -> TaskNames {
    match ai_names(binary, prompt, timeout) {
        Some((title, branch)) => TaskNames {
            title,
            branch,
            ai: true,
        },
        None => fallback(prompt),
    }
}

pub fn fallback(prompt: &str) -> TaskNames {
    let slug = slugify(prompt);
    TaskNames {
        title: prompt.trim().chars().take(64).collect(),
        branch: slug,
        ai: false,
    }
}

fn ai_names(binary: &str, prompt: &str, timeout: Duration) -> Option<(String, String)> {
    let ask = format!(
        "Task prompt: \"{}\"\n\nReply with ONLY a JSON object, no prose:\n\
         {{\"title\": \"<short human title in the prompt's language, max 6 words>\", \
         \"branch\": \"<english kebab-case git branch name, conventional style like fix-login-redirect, max 5 words, lowercase ascii>\"}}",
        prompt.replace('"', "'")
    );
    let mut child = Command::new(binary)
        .args(["-p", &ask, "--output-format", "text", "--model", "haiku"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;

    // Poll with a hard deadline — a hung engine must not block task creation.
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let mut out = String::new();
    child.stdout.take()?.read_to_string(&mut out).ok()?;
    parse_names(&out)
}

/// Parse and VALIDATE the model reply — garbage falls back, never panics.
pub fn parse_names(raw: &str) -> Option<(String, String)> {
    // the model may wrap JSON in ```json fences or prose — find the object
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    let v: serde_json::Value = serde_json::from_str(&raw[start..=end]).ok()?;
    let title = v["title"].as_str()?.trim().to_string();
    let branch_raw = v["branch"].as_str()?.trim().to_lowercase();
    // sanitize branch to strict kebab-case ascii
    let branch = slugify(&branch_raw);
    if title.is_empty() || branch.is_empty() || branch == "task" {
        return None;
    }
    Some((title.chars().take(64).collect(), branch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json() {
        let (t, b) =
            parse_names(r#"{"title":"Починить редирект логина","branch":"fix-login-redirect"}"#)
                .unwrap();
        assert_eq!(t, "Починить редирект логина");
        assert_eq!(b, "fix-login-redirect");
    }

    #[test]
    fn parses_fenced_json_and_sanitizes_branch() {
        let raw = "```json\n{\"title\":\"Fix prices\",\"branch\":\"Fix Login_Redirect!!\"}\n```";
        let (_, b) = parse_names(raw).unwrap();
        assert_eq!(b, "fix-login-redirect", "branch sanitized to kebab-case");
    }

    #[test]
    fn garbage_returns_none() {
        assert!(parse_names("sorry, I cannot").is_none());
        assert!(parse_names(r#"{"title":"x"}"#).is_none());
        // a cyrillic branch is not garbage — it gets transliterated by sanitize
        let (_, b) = parse_names(r#"{"title":"x","branch":"котики"}"#).unwrap();
        assert_eq!(b, "kotiki");
    }

    #[test]
    fn fallback_transliterates() {
        let n = fallback("почини редирект после логина");
        assert!(!n.ai);
        assert_eq!(n.branch, "pochini-redirekt-posle-logina");
        assert_eq!(n.title, "почини редирект после логина");
    }

    #[test]
    fn missing_binary_falls_back() {
        let n = suggest_names(
            "/definitely/not/claude",
            "fix the login",
            Duration::from_secs(1),
        );
        assert!(!n.ai);
        assert_eq!(n.branch, "fix-the-login");
    }

    #[test]
    fn stub_engine_names_are_used() {
        // a stub "claude" that instantly replies with valid JSON
        let dir = tempfile::tempdir().unwrap();
        let stub = dir.path().join("claude-stub.sh");
        std::fs::write(&stub, "#!/bin/sh\necho '{\"title\":\"Fix login redirect\",\"branch\":\"fix-login-redirect\"}'\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        let n = suggest_names(
            &stub.to_string_lossy(),
            "почини редирект",
            Duration::from_secs(5),
        );
        assert!(n.ai);
        assert_eq!(n.branch, "fix-login-redirect");
        assert_eq!(n.title, "Fix login redirect");
    }
}
