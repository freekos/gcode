# gcode

**An agentic IDE: a control deck for a fleet of AI coding agents.**

gcode is not a text editor. It orchestrates AI coding agents (Claude Code, Codex — bring your own subscription) across **multi-repo tasks**, keeps them isolated in git worktrees, and puts a human firmly in charge of everything that touches git.

## Why another tool?

Every agent orchestrator today lives in the same template: *one repo → one worktree → PR to GitHub*. Real work often doesn't fit it. gcode is built around four ideas none of the existing tools cover:

1. **A task spans multiple repos.** One task = worktrees for every affected repo under a single task root, with the agent aware of all of them.
2. **Agents write code, never git.** No commits, no pushes, no merges by agents — enforced, not requested. You review the working-tree diff, annotate lines, send comments back to the agent.
3. **Groups with an integration branch.** Group related tasks visually, and merge them — explicitly, by hand — into the group's integration branch to test a feature as a whole.
4. **Ops from facts.** Test / stage / release actions report ✓/✗ from git and CI facts, never from an agent's prose.

Plus a global AI assistant that can explain the IDE itself and answer "what's going on / what did I do yesterday" from live state.

## Status

Early. Phase 0 (skeleton). See [DESIGN.md](DESIGN.md) for the core design document.

## Stack

- **Core:** Rust (`gcode-core` crate) — domain, git layer, agent layer, SQLite state.
- **CLI:** `gcode` — thin wrapper over the core for scripting and bots.
- **App:** Tauri + Svelte 5 (native-light desktop; no Electron).

## License

MIT
