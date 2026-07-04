//! gcode-core — the engine behind the gcode agentic IDE.
//!
//! Design principles (see DESIGN.md):
//! - A Task spans MULTIPLE repos: one worktree per repo under a single task root.
//! - Agents write code only — git is denied to them; all git ops are explicit human actions.
//! - Single writer for state; git operations serialized per repository; no races by construction.

pub mod actor;
pub mod archive;
pub mod context;
pub mod diff;
pub mod domain;
pub mod engine;
pub mod error;
pub mod git;
pub mod namer;
pub mod provision;
pub mod queues;
pub mod runner;
pub mod scan;
pub mod session;
pub mod state;
pub mod transcript;

pub use actor::StateHandle;
pub use domain::{slugify, Group, Project, Repo, Task, TaskRepo, TaskStatus, Thread};
pub use error::{CoreError, Result};
pub use queues::KeyedQueues;
pub use state::State;

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_is_semver_like() {
        assert!(super::version().split('.').count() >= 3);
    }
}
