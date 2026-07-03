//! Project scanning: discover git repositories inside a project folder.

use crate::error::{CoreError, Result};
use crate::git;
use std::path::Path;

/// A discovered repo: (name, absolute path, default branch).
pub type DiscoveredRepo = (String, String, String);

/// Discover repos under `root`:
/// - if `root` itself is a git repo → it is the single repo of the project;
/// - otherwise every first-level subdirectory that is a git repo counts.
///
/// Hidden directories (starting with '.') are skipped.
pub fn discover_repos(root: &Path) -> Result<Vec<DiscoveredRepo>> {
    if !root.is_dir() {
        return Err(CoreError::Invalid(format!(
            "not a directory: {}",
            root.display()
        )));
    }
    let name_of = |p: &Path| {
        p.file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "repo".to_string())
    };
    if git::is_git_repo(root) {
        return Ok(vec![(
            name_of(root),
            root.to_string_lossy().to_string(),
            git::default_branch(root),
        )]);
    }
    let mut repos = vec![];
    let mut entries: Vec<_> = std::fs::read_dir(root)
        .map_err(|e| CoreError::Invalid(format!("cannot read {}: {e}", root.display())))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = name_of(&path);
        if !path.is_dir() || name.starts_with('.') {
            continue;
        }
        if git::is_git_repo(&path) {
            let branch = git::default_branch(&path);
            repos.push((name, path.to_string_lossy().to_string(), branch));
        }
    }
    Ok(repos)
}
