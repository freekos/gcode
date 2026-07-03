//! State layer: SQLite (WAL) with migrations, transactions per operation, and a journal.
//!
//! Concurrency contract (see DESIGN.md §3): this struct is NOT shared across threads —
//! it is owned by the single state actor. Everything here is synchronous by design.

use crate::domain::{Group, Project, Repo, Task, TaskRepo, TaskStatus, Thread};
use crate::error::{CoreError, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// Current schema version. Bump together with a new `migrate_to_*` step.
const SCHEMA_VERSION: i64 = 2;

/// One journal line: (ts, action, entity, detail).
pub type JournalEntry = (String, String, Option<String>, Option<String>);

pub struct State {
    conn: Connection,
}

impl State {
    /// Open (and migrate) the state database at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// In-memory state — used by tests.
    pub fn in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut st = State { conn };
        st.migrate()?;
        Ok(st)
    }

    fn migrate(&mut self) -> Result<()> {
        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v < 1 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                r#"
                CREATE TABLE projects (
                    id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL UNIQUE,
                    path TEXT NOT NULL,
                    created_at TEXT NOT NULL DEFAULT (datetime('now'))
                );
                CREATE TABLE repos (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    name TEXT NOT NULL,
                    path TEXT NOT NULL,
                    default_branch TEXT NOT NULL,
                    UNIQUE(project_id, name)
                );
                CREATE TABLE task_groups (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    name TEXT NOT NULL,
                    branch TEXT,
                    UNIQUE(project_id, name)
                );
                CREATE TABLE tasks (
                    id INTEGER PRIMARY KEY,
                    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                    group_id INTEGER REFERENCES task_groups(id) ON DELETE SET NULL,
                    title TEXT NOT NULL,
                    slug TEXT NOT NULL,
                    branch TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'new',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    archived_at TEXT,
                    UNIQUE(project_id, slug)
                );
                CREATE TABLE task_repos (
                    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    repo_id INTEGER NOT NULL REFERENCES repos(id) ON DELETE CASCADE,
                    worktree_path TEXT NOT NULL,
                    PRIMARY KEY (task_id, repo_id)
                );
                CREATE TABLE journal (
                    id INTEGER PRIMARY KEY,
                    ts TEXT NOT NULL DEFAULT (datetime('now')),
                    action TEXT NOT NULL,
                    entity TEXT,
                    detail TEXT
                );
                "#,
            )?;
            tx.pragma_update(None, "user_version", 1)?;
            tx.commit()?;
        }
        let v: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v < 2 {
            let tx = self.conn.transaction()?;
            tx.execute_batch(
                r#"
                CREATE TABLE threads (
                    id INTEGER PRIMARY KEY,
                    task_id INTEGER NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
                    engine TEXT NOT NULL,
                    session_id TEXT,
                    title TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL DEFAULT (datetime('now')),
                    last_activity TEXT NOT NULL DEFAULT (datetime('now'))
                );
                "#,
            )?;
            tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            tx.commit()?;
        }
        Ok(())
    }

    // ---- projects ----

    pub fn add_project(
        &mut self,
        name: &str,
        path: &str,
        repos: &[(String, String, String)],
    ) -> Result<Project> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO projects (name, path) VALUES (?1, ?2)",
            params![name, path],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.extended_code == 2067 || f.extended_code == 1555 =>
            {
                CoreError::AlreadyExists(format!("project '{name}'"))
            }
            other => CoreError::Db(other),
        })?;
        let pid = tx.last_insert_rowid();
        for (rname, rpath, rbranch) in repos {
            tx.execute(
                "INSERT INTO repos (project_id, name, path, default_branch) VALUES (?1, ?2, ?3, ?4)",
                params![pid, rname, rpath, rbranch],
            )?;
        }
        tx.execute(
            "INSERT INTO journal (action, entity, detail) VALUES ('project.add', ?1, ?2)",
            params![name, format!("{} repo(s)", repos.len())],
        )?;
        tx.commit()?;
        Ok(Project {
            id: pid,
            name: name.into(),
            path: path.into(),
        })
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, path FROM projects ORDER BY name")?;
        let rows = stmt
            .query_map([], |r| {
                Ok(Project {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: r.get(2)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn project_by_name(&self, name: &str) -> Result<Project> {
        self.conn
            .query_row(
                "SELECT id, name, path FROM projects WHERE name = ?1",
                params![name],
                |r| {
                    Ok(Project {
                        id: r.get(0)?,
                        name: r.get(1)?,
                        path: r.get(2)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("project '{name}'")))
    }

    pub fn project_repos(&self, project_id: i64) -> Result<Vec<Repo>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, name, path, default_branch FROM repos WHERE project_id = ?1 ORDER BY name",
        )?;
        let rows = stmt
            .query_map(params![project_id], |r| {
                Ok(Repo {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    name: r.get(2)?,
                    path: r.get(3)?,
                    default_branch: r.get(4)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    // ---- tasks ----

    /// Create a task with its per-repo worktree records. `repo_worktrees` pairs repo_id
    /// with the (future) worktree path — provisioning happens outside the state layer.
    pub fn add_task(
        &mut self,
        project_id: i64,
        title: &str,
        slug: &str,
        branch: &str,
        repo_worktrees: &[(i64, String)],
    ) -> Result<Task> {
        if repo_worktrees.is_empty() {
            return Err(CoreError::Invalid("a task needs at least one repo".into()));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO tasks (project_id, title, slug, branch) VALUES (?1, ?2, ?3, ?4)",
            params![project_id, title, slug, branch],
        )
        .map_err(|e| match e {
            rusqlite::Error::SqliteFailure(f, _)
                if f.extended_code == 2067 || f.extended_code == 1555 =>
            {
                CoreError::AlreadyExists(format!("task slug '{slug}'"))
            }
            other => CoreError::Db(other),
        })?;
        let tid = tx.last_insert_rowid();
        for (repo_id, wt) in repo_worktrees {
            tx.execute(
                "INSERT INTO task_repos (task_id, repo_id, worktree_path) VALUES (?1, ?2, ?3)",
                params![tid, repo_id, wt],
            )?;
        }
        tx.execute(
            "INSERT INTO journal (action, entity, detail) VALUES ('task.new', ?1, ?2)",
            params![slug, title],
        )?;
        tx.commit()?;
        self.task_by_id(tid)
    }

    pub fn task_by_id(&self, id: i64) -> Result<Task> {
        self.conn
            .query_row(
                "SELECT id, project_id, group_id, title, slug, branch, status, created_at, archived_at
                 FROM tasks WHERE id = ?1",
                params![id],
                Self::row_to_task,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("task #{id}")))
    }

    pub fn task_by_slug(&self, project_id: i64, slug: &str) -> Result<Task> {
        self.conn
            .query_row(
                "SELECT id, project_id, group_id, title, slug, branch, status, created_at, archived_at
                 FROM tasks WHERE project_id = ?1 AND slug = ?2",
                params![project_id, slug],
                Self::row_to_task,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("task '{slug}'")))
    }

    fn row_to_task(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
        let status_s: String = r.get(6)?;
        Ok(Task {
            id: r.get(0)?,
            project_id: r.get(1)?,
            group_id: r.get(2)?,
            title: r.get(3)?,
            slug: r.get(4)?,
            branch: r.get(5)?,
            status: TaskStatus::parse(&status_s).unwrap_or(TaskStatus::New),
            created_at: r.get(7)?,
            archived_at: r.get(8)?,
        })
    }

    /// Active (non-archived) tasks of a project.
    pub fn list_tasks(&self, project_id: i64, include_archived: bool) -> Result<Vec<Task>> {
        let sql = if include_archived {
            "SELECT id, project_id, group_id, title, slug, branch, status, created_at, archived_at
             FROM tasks WHERE project_id = ?1 ORDER BY created_at DESC"
        } else {
            "SELECT id, project_id, group_id, title, slug, branch, status, created_at, archived_at
             FROM tasks WHERE project_id = ?1 AND archived_at IS NULL ORDER BY created_at DESC"
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt
            .query_map(params![project_id], Self::row_to_task)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn task_repos(&self, task_id: i64) -> Result<Vec<TaskRepo>> {
        let mut stmt = self.conn.prepare(
            "SELECT tr.task_id, tr.repo_id, r.name, tr.worktree_path
             FROM task_repos tr JOIN repos r ON r.id = tr.repo_id
             WHERE tr.task_id = ?1 ORDER BY r.name",
        )?;
        let rows = stmt
            .query_map(params![task_id], |r| {
                Ok(TaskRepo {
                    task_id: r.get(0)?,
                    repo_id: r.get(1)?,
                    repo_name: r.get(2)?,
                    worktree_path: r.get(3)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn set_task_status(&mut self, task_id: i64, status: TaskStatus) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE tasks SET status = ?2 WHERE id = ?1",
            params![task_id, status.as_str()],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("task #{task_id}")));
        }
        Ok(())
    }

    /// Mark archived (idempotent errors: archiving an archived task fails loudly).
    pub fn archive_task(&mut self, task_id: i64) -> Result<()> {
        let t = self.task_by_id(task_id)?;
        if t.archived_at.is_some() {
            return Err(CoreError::Invalid(format!(
                "task '{}' is already archived",
                t.slug
            )));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE tasks SET archived_at = datetime('now') WHERE id = ?1",
            params![task_id],
        )?;
        tx.execute(
            "INSERT INTO journal (action, entity) VALUES ('task.archive', ?1)",
            params![t.slug],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Hard-delete a task record (compensation path of provisioning ONLY — user-facing
    /// removal is archive). Cascades task_repos via FK.
    pub fn delete_task(&mut self, task_id: i64) -> Result<()> {
        let n = self
            .conn
            .execute("DELETE FROM tasks WHERE id = ?1", params![task_id])?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("task #{task_id}")));
        }
        Ok(())
    }

    /// Absolute path of a repo by id.
    pub fn repo_path(&self, repo_id: i64) -> Result<String> {
        self.conn
            .query_row(
                "SELECT path FROM repos WHERE id = ?1",
                params![repo_id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("repo #{repo_id}")))
    }

    pub fn restore_task(&mut self, task_id: i64) -> Result<()> {
        let t = self.task_by_id(task_id)?;
        if t.archived_at.is_none() {
            return Err(CoreError::Invalid(format!(
                "task '{}' is not archived",
                t.slug
            )));
        }
        let tx = self.conn.transaction()?;
        tx.execute(
            "UPDATE tasks SET archived_at = NULL WHERE id = ?1",
            params![task_id],
        )?;
        tx.execute(
            "INSERT INTO journal (action, entity) VALUES ('task.restore', ?1)",
            params![t.slug],
        )?;
        tx.commit()?;
        Ok(())
    }

    // ---- groups ----

    pub fn add_group(
        &mut self,
        project_id: i64,
        name: &str,
        branch: Option<&str>,
    ) -> Result<Group> {
        self.conn
            .execute(
                "INSERT INTO task_groups (project_id, name, branch) VALUES (?1, ?2, ?3)",
                params![project_id, name, branch],
            )
            .map_err(|e| match e {
                rusqlite::Error::SqliteFailure(f, _) if f.extended_code == 2067 => {
                    CoreError::AlreadyExists(format!("group '{name}'"))
                }
                other => CoreError::Db(other),
            })?;
        let id = self.conn.last_insert_rowid();
        Ok(Group {
            id,
            project_id,
            name: name.into(),
            branch: branch.map(String::from),
        })
    }

    /// Assign a task to a group (or clear with None). Membership is just a label —
    /// allowed at any moment regardless of task status. Cross-project is rejected.
    pub fn assign_group(&mut self, task_id: i64, group_id: Option<i64>) -> Result<()> {
        let t = self.task_by_id(task_id)?;
        if let Some(gid) = group_id {
            let g_project: Option<i64> = self
                .conn
                .query_row(
                    "SELECT project_id FROM task_groups WHERE id = ?1",
                    params![gid],
                    |r| r.get(0),
                )
                .optional()?;
            match g_project {
                None => return Err(CoreError::NotFound(format!("group #{gid}"))),
                Some(p) if p != t.project_id => {
                    return Err(CoreError::Invalid(
                        "group belongs to another project".into(),
                    ))
                }
                _ => {}
            }
        }
        self.conn.execute(
            "UPDATE tasks SET group_id = ?2 WHERE id = ?1",
            params![task_id, group_id],
        )?;
        Ok(())
    }

    // ---- threads ----

    /// Register a new thread of a task (metadata only — the transcript lives with the engine).
    pub fn add_thread(&mut self, task_id: i64, engine: &str, title: &str) -> Result<Thread> {
        // FK is enforced, but give a clean error instead of a bare constraint failure.
        let _ = self.task_by_id(task_id)?;
        self.conn.execute(
            "INSERT INTO threads (task_id, engine, title) VALUES (?1, ?2, ?3)",
            params![task_id, engine, title],
        )?;
        let id = self.conn.last_insert_rowid();
        self.thread_by_id(id)
    }

    pub fn thread_by_id(&self, id: i64) -> Result<Thread> {
        self.conn
            .query_row(
                "SELECT id, task_id, engine, session_id, title, created_at, last_activity
                 FROM threads WHERE id = ?1",
                params![id],
                Self::row_to_thread,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("thread #{id}")))
    }

    fn row_to_thread(r: &rusqlite::Row<'_>) -> rusqlite::Result<Thread> {
        Ok(Thread {
            id: r.get(0)?,
            task_id: r.get(1)?,
            engine: r.get(2)?,
            session_id: r.get(3)?,
            title: r.get(4)?,
            created_at: r.get(5)?,
            last_activity: r.get(6)?,
        })
    }

    /// Threads of a task, most recently active first.
    pub fn list_threads(&self, task_id: i64) -> Result<Vec<Thread>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, task_id, engine, session_id, title, created_at, last_activity
             FROM threads WHERE task_id = ?1 ORDER BY last_activity DESC",
        )?;
        let rows = stmt
            .query_map(params![task_id], Self::row_to_thread)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Record the engine-assigned session id (first run of a thread) + touch activity.
    pub fn set_thread_session(&mut self, thread_id: i64, session_id: &str) -> Result<()> {
        let n = self.conn.execute(
            "UPDATE threads SET session_id = ?2, last_activity = datetime('now') WHERE id = ?1",
            params![thread_id, session_id],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("thread #{thread_id}")));
        }
        Ok(())
    }

    pub fn touch_thread(&mut self, thread_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE threads SET last_activity = datetime('now') WHERE id = ?1",
            params![thread_id],
        )?;
        Ok(())
    }

    // ---- journal ----

    pub fn journal_append(
        &mut self,
        action: &str,
        entity: Option<&str>,
        detail: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO journal (action, entity, detail) VALUES (?1, ?2, ?3)",
            params![action, entity, detail],
        )?;
        Ok(())
    }

    /// Recent journal lines, newest first: (ts, action, entity, detail).
    pub fn journal_recent(&self, limit: i64) -> Result<Vec<JournalEntry>> {
        let mut stmt = self
            .conn
            .prepare("SELECT ts, action, entity, detail FROM journal ORDER BY id DESC LIMIT ?1")?;
        let rows = stmt
            .query_map(params![limit], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> State {
        State::in_memory().unwrap()
    }

    fn proj(s: &mut State) -> Project {
        s.add_project(
            "azi",
            "/tmp/azi",
            &[
                ("server".into(), "/tmp/azi/server".into(), "main".into()),
                ("crm".into(), "/tmp/azi/crm".into(), "master".into()),
            ],
        )
        .unwrap()
    }

    #[test]
    fn project_add_list_repos() {
        let mut s = st();
        let p = proj(&mut s);
        assert_eq!(s.list_projects().unwrap().len(), 1);
        let repos = s.project_repos(p.id).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].default_branch, "master"); // crm sorts first
    }

    #[test]
    fn duplicate_project_name_rejected() {
        let mut s = st();
        proj(&mut s);
        let err = s.add_project("azi", "/elsewhere", &[]).unwrap_err();
        assert!(matches!(err, CoreError::AlreadyExists(_)), "{err}");
    }

    #[test]
    fn task_lifecycle_add_archive_restore() {
        let mut s = st();
        let p = proj(&mut s);
        let repos = s.project_repos(p.id).unwrap();
        let wt = vec![(repos[0].id, "/tmp/azi/.gcode/tasks/fix/crm".to_string())];
        let t = s
            .add_task(p.id, "Fix login", "fix-login", "fix-login", &wt)
            .unwrap();
        assert_eq!(t.status, TaskStatus::New);
        assert_eq!(s.list_tasks(p.id, false).unwrap().len(), 1);

        s.archive_task(t.id).unwrap();
        assert_eq!(
            s.list_tasks(p.id, false).unwrap().len(),
            0,
            "archived hidden by default"
        );
        assert_eq!(s.list_tasks(p.id, true).unwrap().len(), 1);
        // status survives archive (orthogonal flag)
        assert_eq!(s.task_by_id(t.id).unwrap().status, TaskStatus::New);

        let err = s.archive_task(t.id).unwrap_err();
        assert!(
            matches!(err, CoreError::Invalid(_)),
            "double archive must fail loudly"
        );

        s.restore_task(t.id).unwrap();
        assert_eq!(s.list_tasks(p.id, false).unwrap().len(), 1);
    }

    #[test]
    fn task_needs_at_least_one_repo() {
        let mut s = st();
        let p = proj(&mut s);
        let err = s.add_task(p.id, "x", "x", "x", &[]).unwrap_err();
        assert!(matches!(err, CoreError::Invalid(_)));
    }

    #[test]
    fn duplicate_slug_rejected_per_project() {
        let mut s = st();
        let p = proj(&mut s);
        let repos = s.project_repos(p.id).unwrap();
        let wt = vec![(repos[0].id, "/x".to_string())];
        s.add_task(p.id, "A", "same", "same", &wt).unwrap();
        let err = s.add_task(p.id, "B", "same", "same", &wt).unwrap_err();
        assert!(matches!(err, CoreError::AlreadyExists(_)));
    }

    #[test]
    fn groups_membership_rules() {
        let mut s = st();
        let p = proj(&mut s);
        let repos = s.project_repos(p.id).unwrap();
        let wt = vec![(repos[0].id, "/x".to_string())];
        let t = s.add_task(p.id, "A", "a", "a", &wt).unwrap();
        let g = s.add_group(p.id, "auth-rework", None).unwrap();

        // join and leave at any moment — membership is a label
        s.assign_group(t.id, Some(g.id)).unwrap();
        assert_eq!(s.task_by_id(t.id).unwrap().group_id, Some(g.id));
        s.assign_group(t.id, None).unwrap();
        assert_eq!(s.task_by_id(t.id).unwrap().group_id, None);

        // cross-project group is rejected
        let p2 = s
            .add_project(
                "other",
                "/tmp/o",
                &[("r".into(), "/tmp/o/r".into(), "main".into())],
            )
            .unwrap();
        let g2 = s.add_group(p2.id, "g2", None).unwrap();
        let err = s.assign_group(t.id, Some(g2.id)).unwrap_err();
        assert!(matches!(err, CoreError::Invalid(_)));

        // unknown group
        let err = s.assign_group(t.id, Some(9999)).unwrap_err();
        assert!(matches!(err, CoreError::NotFound(_)));
    }

    #[test]
    fn threads_metadata_lifecycle() {
        let mut s = st();
        let p = proj(&mut s);
        let repos = s.project_repos(p.id).unwrap();
        let wt = vec![(repos[0].id, "/x".to_string())];
        let t = s.add_task(p.id, "A", "a", "a", &wt).unwrap();

        let th = s.add_thread(t.id, "claude", "первый разговор").unwrap();
        assert_eq!(th.engine, "claude");
        assert!(
            th.session_id.is_none(),
            "session id arrives from the engine later"
        );

        s.set_thread_session(th.id, "sess-123").unwrap();
        let th2 = s.thread_by_id(th.id).unwrap();
        assert_eq!(th2.session_id.as_deref(), Some("sess-123"));

        let list = s.list_threads(t.id).unwrap();
        assert_eq!(list.len(), 1);

        // unknown task/thread fail loudly
        assert!(matches!(
            s.add_thread(9999, "claude", "x").unwrap_err(),
            CoreError::NotFound(_)
        ));
        assert!(matches!(
            s.set_thread_session(9999, "s").unwrap_err(),
            CoreError::NotFound(_)
        ));
    }

    #[test]
    fn schema_migrates_to_v2() {
        let s = st();
        let v: i64 = s
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 2);
    }

    #[test]
    fn journal_records_operations() {
        let mut s = st();
        let p = proj(&mut s);
        let repos = s.project_repos(p.id).unwrap();
        let wt = vec![(repos[0].id, "/x".to_string())];
        let t = s.add_task(p.id, "A", "a", "a", &wt).unwrap();
        s.archive_task(t.id).unwrap();
        let j = s.journal_recent(10).unwrap();
        let actions: Vec<&str> = j.iter().map(|(_, a, _, _)| a.as_str()).collect();
        assert!(actions.contains(&"project.add"));
        assert!(actions.contains(&"task.new"));
        assert!(actions.contains(&"task.archive"));
    }

    #[test]
    fn state_survives_reopen_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("state.db");
        {
            let mut s = State::open(&db).unwrap();
            proj(&mut s);
        }
        let s2 = State::open(&db).unwrap();
        assert_eq!(
            s2.list_projects().unwrap().len(),
            1,
            "data persists across reopen"
        );
    }
}
