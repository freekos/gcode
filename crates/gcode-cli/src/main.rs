use clap::{Parser, Subcommand};
use gcode_core::archive::{archive_task_full, restore_task_full};
use gcode_core::provision::provision_task;
use gcode_core::{scan, KeyedQueues, State, StateHandle};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "gcode", version, about = "gcode — the agentic IDE engine")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Manage projects (a project = a folder containing your git repos)
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Manage tasks (a task = worktree per affected repo under one root)
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
    /// Show the operation journal
    Journal {
        /// How many recent entries to show
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: i64,
    },
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Create a task: one git worktree per repo, branch created immediately
    New {
        /// Project name
        project: String,
        /// Task title (slug and branch are derived from it)
        title: String,
        /// Comma-separated repo names (default: all repos of the project)
        #[arg(long, value_delimiter = ',')]
        repos: Vec<String>,
    },
    /// List tasks of a project
    Ls {
        project: String,
        /// Include archived tasks
        #[arg(long)]
        all: bool,
    },
    /// Archive: save uncommitted work as a patch, remove worktrees (restorable)
    Archive { project: String, slug: String },
    /// Restore an archived task: worktrees re-attached, saved patches applied
    Restore { project: String, slug: String },
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Register a project: scans the folder for git repositories
    Add {
        /// Path to the project folder
        path: PathBuf,
        /// Project name (defaults to the folder name)
        #[arg(long)]
        name: Option<String>,
    },
    /// List registered projects with their repos
    Ls,
}

/// State DB location: $GCODE_DB or ~/.gcode/gcode.db
fn db_path() -> PathBuf {
    if let Ok(p) = std::env::var("GCODE_DB") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".gcode").join("gcode.db")
}

fn open_state() -> State {
    let path = db_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    State::open(&path).unwrap_or_else(|e| {
        eprintln!("cannot open state db {}: {e}", path.display());
        std::process::exit(1);
    })
}

fn die(msg: impl std::fmt::Display) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Project { cmd } => match cmd {
            ProjectCmd::Add { path, name } => {
                let abs = path
                    .canonicalize()
                    .unwrap_or_else(|e| die(format!("bad path {}: {e}", path.display())));
                let name = name.unwrap_or_else(|| {
                    abs.file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| "project".into())
                });
                let repos = scan::discover_repos(&abs).unwrap_or_else(|e| die(e));
                if repos.is_empty() {
                    die(format!("no git repositories found in {}", abs.display()));
                }
                let mut st = open_state();
                match st.add_project(&name, &abs.to_string_lossy(), &repos) {
                    Ok(p) => {
                        println!("project '{}' registered ({} repo(s)):", p.name, repos.len());
                        for (rname, _, rbranch) in &repos {
                            println!("  {rname}  [{rbranch}]");
                        }
                    }
                    Err(e) => die(e),
                }
            }
            ProjectCmd::Ls => {
                let st = open_state();
                let projects = st.list_projects().unwrap_or_default();
                if projects.is_empty() {
                    println!("no projects yet — try: gcode project add <path>");
                    return;
                }
                for p in projects {
                    let repos = st.project_repos(p.id).unwrap_or_default();
                    println!("{}  ({})", p.name, p.path);
                    for r in repos {
                        println!("  {}  [{}]", r.name, r.default_branch);
                    }
                }
            }
        },
        Command::Task { cmd } => {
            let handle = StateHandle::spawn(open_state());
            let queues = KeyedQueues::new();
            match cmd {
                TaskCmd::New {
                    project,
                    title,
                    repos,
                } => match provision_task(&handle, &queues, &project, &title, &repos) {
                    Ok(res) => {
                        println!(
                            "task '{}' created (branch {}):",
                            res.task.slug, res.task.branch
                        );
                        println!("  root: {}", res.root.display());
                        for (name, wt) in &res.worktrees {
                            println!("  {name}  ->  {}", wt.display());
                        }
                    }
                    Err(e) => die(e),
                },
                TaskCmd::Ls { project, all } => {
                    let p = handle
                        .call(move |st| st.project_by_name(&project))
                        .unwrap_or_else(|e| die(e));
                    let pid = p.id;
                    let tasks = handle
                        .call(move |st| st.list_tasks(pid, all))
                        .unwrap_or_default();
                    if tasks.is_empty() {
                        println!("no tasks — try: gcode task new {} \"<title>\"", p.name);
                        return;
                    }
                    for t in tasks {
                        let arch = if t.archived_at.is_some() {
                            "  [archived]"
                        } else {
                            ""
                        };
                        println!(
                            "{:<24} {:<12} {}{}",
                            t.slug,
                            t.status.as_str(),
                            t.title,
                            arch
                        );
                    }
                }
                TaskCmd::Archive { project, slug } => {
                    let tid = resolve_task(&handle, &project, &slug);
                    match archive_task_full(&handle, &queues, tid) {
                        Ok(rep) => {
                            println!("task '{slug}' archived (worktrees removed, branches kept)");
                            for (repo, patch) in rep.patches {
                                println!("  ⚠ uncommitted work in {repo} saved to {patch}");
                            }
                        }
                        Err(e) => die(e),
                    }
                }
                TaskCmd::Restore { project, slug } => {
                    let tid = resolve_task(&handle, &project, &slug);
                    match restore_task_full(&handle, &queues, tid) {
                        Ok(rep) => {
                            println!("task '{slug}' restored (worktrees re-attached)");
                            for p in rep.applied_patches {
                                println!("  ✓ applied saved patch {p}");
                            }
                            for p in rep.failed_patches {
                                println!("  ✗ patch did NOT apply cleanly, kept for manual recovery: {p}");
                            }
                        }
                        Err(e) => die(e),
                    }
                }
            }
        }
        Command::Journal { limit } => {
            let st = open_state();
            for (ts, action, entity, detail) in st.journal_recent(limit).unwrap_or_default() {
                let entity = entity.unwrap_or_default();
                let detail = detail.unwrap_or_default();
                println!("{ts}  {action:<16} {entity:<20} {detail}");
            }
        }
    }
}

/// Resolve "<project> <slug>" to a task id or exit with a clear error.
fn resolve_task(handle: &StateHandle, project: &str, slug: &str) -> i64 {
    let project = project.to_string();
    let slug = slug.to_string();
    handle
        .call(move |st| {
            let p = st.project_by_name(&project)?;
            st.task_by_slug(p.id, &slug).map(|t| t.id)
        })
        .unwrap_or_else(|e| die(e))
}
