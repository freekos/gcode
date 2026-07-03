//! gcode-core — the engine behind the gcode agentic IDE.
//!
//! Design principles (see DESIGN.md):
//! - A Task spans MULTIPLE repos: one worktree per repo under a single task root.
//! - Agents write code only — git is denied to them; all git ops are explicit human actions.
//! - Single writer for state; git operations serialized per repository; no races by construction.

/// Placeholder for the first domain type; replaced by the real domain model in Phase 1.
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
