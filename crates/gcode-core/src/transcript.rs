//! Read conversation history back from Claude Code's own transcripts.
//!
//! Claude Code stores each session as JSONL at
//! `~/.claude/projects/<encoded cwd>/<session id>.jsonl`, where the cwd is
//! encoded by replacing every char outside [A-Za-z0-9-] with '-'. Reusing the
//! native transcript (instead of keeping our own copy) means the same thread
//! keeps working from a plain `claude -c` in the terminal.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct HistoryItem {
    /// "user" | "agent" | "tool"
    pub kind: String,
    pub text: String,
}

/// Claude Code's cwd -> project-dir encoding.
pub fn encode_cwd(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn transcript_path(cwd: &Path, session_id: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    Some(
        PathBuf::from(home)
            .join(".claude")
            .join("projects")
            .join(encode_cwd(cwd))
            .join(format!("{session_id}.jsonl")),
    )
}

/// Load the visible history of a session (missing file -> empty, never errors).
pub fn load_history(cwd: &Path, session_id: &str) -> Vec<HistoryItem> {
    let Some(path) = transcript_path(cwd, session_id) else {
        return vec![];
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return vec![];
    };
    parse_transcript(&raw)
}

/// Parse transcript JSONL into displayable items. Unknown/meta lines are skipped.
pub fn parse_transcript(raw: &str) -> Vec<HistoryItem> {
    let mut out = vec![];
    for line in raw.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // meta noise (summaries, hooks, system) is not conversation
        if v["isMeta"].as_bool().unwrap_or(false) {
            continue;
        }
        match v["type"].as_str().unwrap_or("") {
            "user" => {
                let content = &v["message"]["content"];
                if let Some(text) = content.as_str() {
                    if !text.trim().is_empty() && !looks_like_noise(text) {
                        out.push(HistoryItem {
                            kind: "user".into(),
                            text: text.trim().to_string(),
                        });
                    }
                } else if let Some(blocks) = content.as_array() {
                    for b in blocks {
                        if b["type"] == "text" {
                            if let Some(t) = b["text"].as_str() {
                                if !t.trim().is_empty() && !looks_like_noise(t) {
                                    out.push(HistoryItem {
                                        kind: "user".into(),
                                        text: t.trim().to_string(),
                                    });
                                }
                            }
                        }
                        // tool_result blocks are skipped — the tool line itself was shown
                    }
                }
            }
            "assistant" => {
                if let Some(blocks) = v["message"]["content"].as_array() {
                    for b in blocks {
                        match b["type"].as_str().unwrap_or("") {
                            "text" => {
                                if let Some(t) = b["text"].as_str() {
                                    if !t.trim().is_empty() {
                                        out.push(HistoryItem {
                                            kind: "agent".into(),
                                            text: t.trim().to_string(),
                                        });
                                    }
                                }
                            }
                            "tool_use" => {
                                let name = b["name"].as_str().unwrap_or("tool");
                                let inp = &b["input"];
                                let detail = inp["command"]
                                    .as_str()
                                    .or_else(|| inp["file_path"].as_str())
                                    .or_else(|| inp["path"].as_str())
                                    .or_else(|| inp["pattern"].as_str())
                                    .unwrap_or("");
                                let detail: String = detail.chars().take(80).collect();
                                let text = if detail.is_empty() {
                                    name.to_string()
                                } else {
                                    format!("{name} · {detail}")
                                };
                                out.push(HistoryItem {
                                    kind: "tool".into(),
                                    text,
                                });
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// System-injected user content that should not render as a human message.
fn looks_like_noise(t: &str) -> bool {
    t.starts_with("<system-reminder>")
        || t.starts_with("Caveat:")
        || t.starts_with("[Request interrupted")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_cwd_like_claude() {
        assert_eq!(
            encode_cwd(Path::new("/Users/dev/work/azi")),
            "-Users-dev-work-azi"
        );
        assert_eq!(
            encode_cwd(Path::new("/tmp/.gcode/tasks/fix_it")),
            "-tmp--gcode-tasks-fix-it"
        );
    }

    #[test]
    fn parses_conversation_and_skips_noise() {
        let raw = r#"{"type":"user","message":{"role":"user","content":"почини логин"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Смотрю код."},{"type":"tool_use","name":"Read","input":{"file_path":"src/auth.ts"}}]}}
{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"file contents"}]}}
{"type":"user","isMeta":true,"message":{"role":"user","content":"meta stuff"}}
{"type":"user","message":{"role":"user","content":"<system-reminder>noise</system-reminder>"}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Готово."}]}}
not json"#;
        let items = parse_transcript(raw);
        assert_eq!(
            items,
            vec![
                HistoryItem {
                    kind: "user".into(),
                    text: "почини логин".into()
                },
                HistoryItem {
                    kind: "agent".into(),
                    text: "Смотрю код.".into()
                },
                HistoryItem {
                    kind: "tool".into(),
                    text: "Read · src/auth.ts".into()
                },
                HistoryItem {
                    kind: "agent".into(),
                    text: "Готово.".into()
                },
            ]
        );
    }

    #[test]
    fn missing_file_is_empty() {
        assert!(load_history(Path::new("/nope"), "no-session").is_empty());
    }
}
