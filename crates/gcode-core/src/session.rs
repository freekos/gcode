//! Pattern B (phase 2 decision, promoted after the chat review): a PERSISTENT
//! bidirectional Claude process per active thread. One spawn, many turns —
//! no cold start between messages, tokens stream immediately.
//!
//! Input: NDJSON user messages on stdin (`--input-format stream-json`);
//! output: the same stream-json we already parse. A turn ends with a `result`
//! event, the process stays alive for the next `send`.

use crate::engine::{parse_line, AgentEvent};
use crate::error::{CoreError, Result};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

pub struct LiveSession {
    child: Child,
    stdin: ChildStdin,
}

impl LiveSession {
    /// Spawn a persistent session in `cwd` (optionally resuming an engine session).
    /// Events from ALL turns flow into `on_event` from a reader thread.
    pub fn spawn(
        binary: &str,
        cwd: &Path,
        resume: Option<&str>,
        mut on_event: impl FnMut(AgentEvent) + Send + 'static,
    ) -> Result<Self> {
        let mut cmd = Command::new(binary);
        cmd.args([
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
        ])
        .args(["--permission-mode", "bypassPermissions"])
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
        if let Some(sid) = resume {
            cmd.args(["--resume", sid]);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| CoreError::Invalid(format!("cannot spawn {binary}: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| CoreError::Invalid("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| CoreError::Invalid("no stdout".into()))?;

        std::thread::spawn(move || {
            // suppress duplicate whole-messages when deltas streamed (cc lesson)
            let mut saw_delta = false;
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                for ev in parse_line(&line) {
                    match &ev {
                        AgentEvent::TextDelta(_) => saw_delta = true,
                        AgentEvent::WholeText(_) if saw_delta => continue,
                        AgentEvent::Done { .. } => saw_delta = false,
                        _ => {}
                    }
                    on_event(ev);
                }
            }
            // process ended (killed, crashed, or logged out) — tell the consumer
            on_event(AgentEvent::Done {
                ok: false,
                error: Some("session closed".into()),
            });
        });

        Ok(LiveSession { child, stdin })
    }

    /// Send one user turn into the live session.
    pub fn send(&mut self, text: &str) -> Result<()> {
        let msg = serde_json::json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "text", "text": text }] }
        });
        writeln!(self.stdin, "{msg}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| CoreError::Invalid(format!("session write: {e}")))
    }

    /// Interrupt the CURRENT turn (control protocol); the process stays alive
    /// and the turn ends with a result event. Hard fallback is kill().
    pub fn interrupt(&mut self) -> Result<()> {
        let msg = serde_json::json!({
            "type": "control_request",
            "request_id": format!("int-{}", std::process::id()),
            "request": { "subtype": "interrupt" }
        });
        writeln!(self.stdin, "{msg}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| CoreError::Invalid(format!("interrupt: {e}")))
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for LiveSession {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Stub engine binary: replies to EVERY stdin line with a full turn.
    fn stub() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("claude-live.sh");
        std::fs::write(&p, r#"#!/bin/sh
echo '{"type":"system","subtype":"init","session_id":"live-sess-1"}'
while read -r line; do
  echo '{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"echo-turn"}}}'
  echo '{"type":"result","subtype":"success","is_error":false}'
done
"#).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        let s = p.to_string_lossy().to_string();
        (dir, s)
    }

    #[test]
    fn multiple_turns_one_process() {
        let (dir, bin) = stub();
        let (tx, rx) = mpsc::channel::<AgentEvent>();
        let mut s = LiveSession::spawn(&bin, dir.path(), None, move |ev| {
            let _ = tx.send(ev);
        })
        .unwrap();

        // session id arrives once
        let first = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(first, AgentEvent::Session("live-sess-1".into()));

        for _ in 0..3 {
            s.send("hi").unwrap();
            // delta then done, per turn
            let d = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert_eq!(d, AgentEvent::TextDelta("echo-turn".into()));
            let done = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            assert!(matches!(done, AgentEvent::Done { ok: true, .. }));
        }
        assert!(s.alive(), "process persists across turns");
        s.kill();
    }
}
