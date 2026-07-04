//! Domain types. Kept plain (no DB details) — the state layer maps rows into these.

/// A registered project: a folder that contains git repositories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    /// Absolute path to the project root folder.
    pub path: String,
}

/// A git repository inside a project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    /// Absolute path to the repo checkout.
    pub path: String,
    pub default_branch: String,
}

/// Task lifecycle. `archived` is intentionally NOT a status — it's an orthogonal
/// flag (`archived_at`), so a task keeps its last status through archive/restore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// Created; no agent run yet.
    New,
    /// An agent thread is actively working.
    Running,
    /// The agent asked a question and is blocked on the human.
    NeedsInput,
    /// Agent finished; waiting for human review.
    Review,
    /// Work delivered (merged/PR'd) — terminal.
    Done,
}

impl TaskStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskStatus::New => "new",
            TaskStatus::Running => "running",
            TaskStatus::NeedsInput => "needs_input",
            TaskStatus::Review => "review",
            TaskStatus::Done => "done",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "new" => TaskStatus::New,
            "running" => TaskStatus::Running,
            "needs_input" => TaskStatus::NeedsInput,
            "review" => TaskStatus::Review,
            "done" => TaskStatus::Done,
            _ => return None,
        })
    }
}

/// A unit of work spanning one or more repos (one worktree per repo).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub id: i64,
    pub project_id: i64,
    /// Optional group membership (a task lives in at most one group).
    pub group_id: Option<i64>,
    pub title: String,
    /// Filesystem/branch-safe identifier derived from the title; unique per project.
    pub slug: String,
    /// Branch name created in every worktree of this task.
    pub branch: String,
    pub status: TaskStatus,
    pub created_at: String,
    pub archived_at: Option<String>,
}

/// A visual grouping of tasks + an optional integration branch (target for explicit merges).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub id: i64,
    pub project_id: i64,
    pub name: String,
    pub branch: Option<String>,
}

/// One agent conversation inside a task. The transcript itself is stored by the
/// engine (cwd-scoped); we keep metadata + the session id needed to resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    pub id: i64,
    pub task_id: i64,
    pub engine: String,
    pub session_id: Option<String>,
    pub title: String,
    pub created_at: String,
    pub last_activity: String,
}

/// Per-repo worktree of a task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRepo {
    pub task_id: i64,
    pub repo_id: i64,
    pub repo_name: String,
    pub worktree_path: String,
}

/// Derive a filesystem/branch-safe slug from a human title.
/// Cyrillic is transliterated (prompts are often Russian — "почини логин" must
/// give a meaningful slug, not a fallback). Lowercase ASCII letters/digits/dashes,
/// max 40 chars, never empty.
pub fn slugify(title: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    let push = |s: &str, out: &mut String, prev_dash: &mut bool| {
        for c in s.chars() {
            out.push(c);
        }
        *prev_dash = false;
    };
    for ch in title.chars() {
        if out.len() >= 40 {
            break;
        }
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if let Some(tr) = translit(ch.to_lowercase().next().unwrap_or(ch)) {
            push(tr, &mut out, &mut prev_dash);
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "task".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

/// RU -> latin, GOST-style simplified.
fn translit(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'ё' => "e",
        'ж' => "zh",
        'з' => "z",
        'и' => "i",
        'й' => "y",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "h",
        'ц' => "ts",
        'ч' => "ch",
        'ш' => "sh",
        'щ' => "sch",
        'ы' => "y",
        'э' => "e",
        'ю' => "yu",
        'я' => "ya",
        'ъ' | 'ь' => "",
        // Kazakh specifics
        'қ' => "k",
        'ғ' => "g",
        'ң' => "n",
        'ү' | 'ұ' => "u",
        'ө' => "o",
        'һ' => "h",
        'і' => "i",
        'ә' => "a",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Fix Login Redirect!"), "fix-login-redirect");
    }

    #[test]
    fn slugify_cyrillic_transliterates() {
        assert_eq!(slugify("почини логин"), "pochini-login");
        assert_eq!(
            slugify("Почини редирект после логина"),
            "pochini-redirekt-posle-logina"
        );
    }

    #[test]
    fn slugify_mixed_keeps_ascii_and_translit() {
        assert_eq!(
            slugify("починить login redirect"),
            "pochinit-login-redirect"
        );
    }

    #[test]
    fn slugify_emoji_only_falls_back() {
        assert_eq!(slugify("🔥🔥🔥"), "task");
    }

    #[test]
    fn slugify_caps_length_and_trims_dashes() {
        let s = slugify(&"a b ".repeat(40));
        assert!(s.len() <= 40);
        assert!(!s.starts_with('-') && !s.ends_with('-'));
    }

    #[test]
    fn status_roundtrip() {
        for st in [
            TaskStatus::New,
            TaskStatus::Running,
            TaskStatus::NeedsInput,
            TaskStatus::Review,
            TaskStatus::Done,
        ] {
            assert_eq!(TaskStatus::parse(st.as_str()), Some(st));
        }
        assert_eq!(TaskStatus::parse("bogus"), None);
    }
}
