use clap::{Parser, Subcommand};
use gcode_core::{scan, State};
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
    /// Show the operation journal
    Journal {
        /// How many recent entries to show
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: i64,
    },
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
