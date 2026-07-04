//! Agent engines (Phase 2). An engine runs a coding agent inside a task root and
//! translates its machine output into a small set of `AgentEvent`s.
//!
//! Two patterns by design (phase decision #2): A — process-per-message with
//! `--resume` (implemented here), B — persistent bidirectional process (arrives
//! with the UI phase; the trait already leaves room for it).

use crate::error::{CoreError, Result};
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};

/// What an engine emits while working. Deliberately tiny — the UI/CLI can render
/// these without knowing anything engine-specific.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    /// The engine assigned/confirmed the conversation session id.
    Session(String),
    /// A chunk of assistant text (streamed delta).
    TextDelta(String),
    /// A whole assistant text block (fallback when no deltas were streamed).
    WholeText(String),
    /// The agent invoked a tool (informational): name + best-effort human detail
    /// (command / file path / pattern from the tool input).
    ToolUse(String, String),
    /// Subscription window info (honest facts only: window kind + reset time;
    /// the headless stream does NOT carry used percentages).
    RateLimit { kind: String, resets_at: i64 },
    /// The run finished. `ok=false` means the engine reported an error.
    Done { ok: bool, error: Option<String> },
}

/// One agent run: `prompt` in `cwd`, optionally resuming an existing session.
pub struct RunSpec<'a> {
    pub cwd: &'a Path,
    pub prompt: &'a str,
    pub resume: Option<&'a str>,
}

pub trait Engine {
    fn name(&self) -> &'static str;
    /// Run to completion, invoking `on_event` for every event (pattern A).
    fn run(&self, spec: RunSpec<'_>, on_event: &mut dyn FnMut(AgentEvent)) -> Result<()>;
}

/// Claude Code via headless CLI (`claude -p --output-format stream-json`).
pub struct ClaudeEngine {
    /// Binary to execute — the real `claude`, or a stub in tests.
    pub binary: String,
}

impl Default for ClaudeEngine {
    fn default() -> Self {
        ClaudeEngine {
            binary: "claude".into(),
        }
    }
}

impl Engine for ClaudeEngine {
    fn name(&self) -> &'static str {
        "claude"
    }

    fn run(&self, spec: RunSpec<'_>, on_event: &mut dyn FnMut(AgentEvent)) -> Result<()> {
        let mut cmd = Command::new(&self.binary);
        cmd.arg("-p")
            .arg(spec.prompt)
            .args([
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ])
            .args(["--permission-mode", "bypassPermissions"])
            .current_dir(spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(sid) = spec.resume {
            cmd.args(["--resume", sid]);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| CoreError::Invalid(format!("cannot spawn {}: {e}", self.binary)))?;

        let stdout = child.stdout.take().expect("stdout piped");
        let mut saw_result = false;
        // Deltas AND a whole-message arrive for the same text (cc lesson): emit deltas
        // as they stream; suppress the duplicating WholeText when deltas were seen.
        let mut saw_delta_in_block = false;
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            for ev in parse_line(&line) {
                match &ev {
                    AgentEvent::TextDelta(_) => saw_delta_in_block = true,
                    AgentEvent::WholeText(_) if saw_delta_in_block => continue,
                    AgentEvent::Done { .. } => saw_result = true,
                    _ => {}
                }
                on_event(ev);
            }
        }
        let status = child
            .wait()
            .map_err(|e| CoreError::Invalid(format!("wait on {}: {e}", self.binary)))?;
        if !saw_result {
            // The process died without a result event — surface stderr honestly (cc lesson).
            let mut err = String::new();
            if let Some(mut se) = child.stderr.take() {
                use std::io::Read;
                let _ = se.read_to_string(&mut err);
            }
            on_event(AgentEvent::Done {
                ok: status.success(),
                error: if err.trim().is_empty() {
                    None
                } else {
                    Some(err.lines().next().unwrap_or("").to_string())
                },
            });
        }
        Ok(())
    }
}

/// Parse ONE line of Claude Code stream-json into zero or more events.
/// Unknown event types are ignored on purpose — the stream contains noise
/// (rate_limit_event, system/status, …) that must never break the run.
pub fn parse_line(line: &str) -> Vec<AgentEvent> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return vec![];
    };
    let typ = v["type"].as_str().unwrap_or("");
    match typ {
        "system" => {
            if v["subtype"] == "init" {
                if let Some(sid) = v["session_id"].as_str() {
                    return vec![AgentEvent::Session(sid.to_string())];
                }
            }
            vec![]
        }
        "stream_event" => {
            let ev = &v["event"];
            match ev["type"].as_str().unwrap_or("") {
                "content_block_delta" => {
                    let d = &ev["delta"];
                    if d["type"] == "text_delta" {
                        if let Some(t) = d["text"].as_str() {
                            return vec![AgentEvent::TextDelta(t.to_string())];
                        }
                    }
                    vec![]
                }
                _ => vec![],
            }
        }
        "assistant" => {
            let mut out = vec![];
            if let Some(blocks) = v["message"]["content"].as_array() {
                for b in blocks {
                    match b["type"].as_str().unwrap_or("") {
                        "text" => {
                            if let Some(t) = b["text"].as_str() {
                                if !t.is_empty() {
                                    out.push(AgentEvent::WholeText(t.to_string()));
                                }
                            }
                        }
                        "tool_use" => {
                            if let Some(n) = b["name"].as_str() {
                                let inp = &b["input"];
                                let detail = inp["command"]
                                    .as_str()
                                    .or_else(|| inp["file_path"].as_str())
                                    .or_else(|| inp["path"].as_str())
                                    .or_else(|| inp["pattern"].as_str())
                                    .or_else(|| inp["url"].as_str())
                                    .unwrap_or("");
                                let detail: String = detail.chars().take(80).collect();
                                out.push(AgentEvent::ToolUse(n.to_string(), detail));
                            }
                        }
                        _ => {}
                    }
                }
            }
            out
        }
        "rate_limit_event" => {
            let info = &v["rate_limit_info"];
            match (info["rateLimitType"].as_str(), info["resetsAt"].as_i64()) {
                (Some(k), Some(t)) => vec![AgentEvent::RateLimit {
                    kind: k.to_string(),
                    resets_at: t,
                }],
                _ => vec![],
            }
        }
        "result" => {
            let ok = !v["is_error"].as_bool().unwrap_or(false);
            let error = v["result"].as_str().filter(|_| !ok).map(|s| s.to_string());
            vec![AgentEvent::Done { ok, error }]
        }
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture is REAL Claude Code output, recorded once (paths sanitized).
    const FIXTURE: &str = include_str!("../tests/fixtures/claude_stream_real.jsonl");

    fn events() -> Vec<AgentEvent> {
        FIXTURE.lines().flat_map(parse_line).collect()
    }

    #[test]
    fn real_stream_yields_session_id_first() {
        let evs = events();
        assert!(
            matches!(&evs[0], AgentEvent::Session(s) if !s.is_empty()),
            "first event must be the session id, got {:?}",
            evs.first()
        );
    }

    #[test]
    fn real_stream_deltas_reassemble_the_reply() {
        let text: String = events()
            .iter()
            .filter_map(|e| match e {
                AgentEvent::TextDelta(t) => Some(t.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            text.contains("hello from gcode fixture"),
            "deltas must reassemble the reply, got: {text}"
        );
    }

    #[test]
    fn real_stream_ends_with_success() {
        let evs = events();
        assert!(
            matches!(evs.last(), Some(AgentEvent::Done { ok: true, .. })),
            "stream must end with a successful Done, got {:?}",
            evs.last()
        );
    }

    #[test]
    fn whole_text_also_present_for_delta_free_consumers() {
        // The raw stream carries BOTH deltas and the whole assistant message;
        // the runner dedupes at run() level, the parser must surface both.
        let evs = events();
        assert!(evs.iter().any(
            |e| matches!(e, AgentEvent::WholeText(t) if t.contains("hello from gcode fixture"))
        ));
    }

    #[test]
    fn garbage_and_unknown_events_are_ignored() {
        assert!(parse_line("not json at all").is_empty());
        assert!(
            parse_line(r#"{"type":"rate_limit_event","foo":1}"#).is_empty(),
            "malformed limit event ignored"
        );
        assert!(parse_line(r#"{"type":"system","subtype":"status"}"#).is_empty());
    }

    #[test]
    fn real_stream_surfaces_rate_limit_window() {
        let evs = events();
        assert!(
            evs.iter().any(|e| matches!(e, AgentEvent::RateLimit { kind, resets_at } if kind == "five_hour" && *resets_at > 0)),
            "the real fixture contains a five_hour rate_limit_event"
        );
    }

    #[test]
    fn error_result_is_surfaced() {
        let evs = parse_line(
            r#"{"type":"result","subtype":"error","is_error":true,"result":"boom happened"}"#,
        );
        assert_eq!(
            evs,
            vec![AgentEvent::Done {
                ok: false,
                error: Some("boom happened".into())
            }]
        );
    }
}
