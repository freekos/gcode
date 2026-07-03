# gcode — Core Design Document

> Status: **skeleton** — sections marked ⏳ are being written in Phase 1a and reviewed before any core code lands.
> The Python engine of the predecessor project ("cc") is the living spec: where its behavior was proven in daily use, gcode-core must match it or improve on it deliberately.

## 1. Product decisions (fixed)

- **Task = multi-repo.** Creating a task provisions one git worktree per affected repo under a single task root (`<tasks-dir>/<task>/<repo>/…`), plus a task-level context file. Agents run with the task root as cwd, so nested per-repo agent context is picked up naturally.
- **Agents never touch git.** Enforced via deny permissions written into each worktree's agent settings, not by prompt politeness. Review happens on the **working-tree diff**; line annotations are batched back into the agent thread.
- **No automerge, no combined magic.** When a task is done, the human picks: **create a PR/MR** or **merge locally into a chosen branch**. Both are explicit ops actions.
- **Group = visual grouping + optional integration branch.** Any task can join/leave a group at any time (membership is a label). The group's integration branch is a normal long-lived branch; tasks are merged into it explicitly. No deterministic rebuild.
- **A task without a group is just a project task.** No "loose" concept.
- **Threads.** A task has one or more agent threads (industry term), each with its own history, resumable; engine switching (Claude ↔ Codex) carries a context digest.
- **Ops from facts.** Test/stage/release run as headless agents, but their status comes from exit codes + CI pipeline facts (git/glab/gh), never from agent prose.
- **Jira/trackers: pull-only.** Import tasks/epics to start work; never write back.
- **Global AI assistant** (hotkey): explains the IDE itself (built-in docs as knowledge), answers over live state + operation journal ("what's running", "what did I do yesterday"), and doubles as a scratch chat.
- **Native-light.** Tauri + Svelte 5; heavy views virtualized. GPUI is the escape hatch if we ever outgrow WebView.

## 2. Architecture

```
┌────────────────────────────────────────────┐
│ app/ (Tauri + Svelte 5)                    │  board, threads, diff review, modals
├────────────────────────────────────────────┤
│ crates/gcode-core                          │
│  domain: Project / Task / Thread / Group   │
│  state:  SQLite (WAL), single writer      │
│  git:    system git, serialized per repo  │
│  agents: spawn CLIs, stream-json          │
│  ops:    actions + CI fact verification   │
│  journal: append-only audit               │
├────────────────────────────────────────────┤
│ crates/gcode-cli — thin CLI over the core  │  scripting, telegram bot later
└────────────────────────────────────────────┘
```

## 3. Concurrency model (no races by construction) ⏳

Invariants to be specified precisely in Phase 1a, with tests per invariant:

- **Single writer for state:** all mutations flow through one actor/queue; SQLite in WAL mode, transactions per operation.
- **Git serialized per repository:** at most one git command per repo at a time (per-repo queue); different repos proceed in parallel.
- **Agents isolated per worktree** and denied git — nothing to race on.
- **Heavy provisioning is queued** (dependency installs never run unbounded in parallel).

## 4. Provisioning (don't kill the user's machine) ⏳

- `git fetch origin <branch>` (only what's needed) → `git worktree add` (shared history, no clone).
- Copy `.env*` (except tracked `.env.example`) from the main checkout.
- Dependencies cascade: pnpm repo → pnpm install against a shared store; otherwise **CoW-clone** `node_modules` (APFS `clonefile` / Linux reflink); symlink only as last resort; never a silent full `npm install`.
- Lazy: install deps when actually needed (dev run / tests), through the provisioning queue.
- Cleanup built in: archiving a task removes worktrees (state preserved, restorable); `git worktree prune`; disk usage surfaced in UI.
- User hook for custom setup (Zed-style `create_worktree` hook).

## 5. Testing policy ⏳

- **Unit** tests for domain logic (all branches: conflicts, busy targets, archived entities, cross-project moves, corrupted state).
- **e2e** tests against real temporary git repos (multi-repo fixtures): provisioning, diff, merge into group branch, archive/restore.
- **Concurrency** tests: parallel task creation / merges / state mutations hammering the core.
- CI runs fmt + clippy + the full suite on every PR.

## 6. Roadmap (phases)

0. Skeleton: workspace, CLI stub, CI. ✅
1. **1a: this document completed & reviewed** → domain + state + provisioning with full tests.
2. Agent layer: threads, stream-json, resume, deny-git enforcement.
3. Minimal UI: board, prompt-first task creation, thread chat, fact-based statuses. (Dogfood starts here.)
4. Diff review with line annotations → back to agent.
5. Git-ops layer: PR/MR or local merge, confirmations.
6. Groups + integration branches.
7. Ops with CI fact verification + global ops modal.
8. Polish + distribution: signing, auto-update, Homebrew.
9. Remote: Telegram bot over the CLI.
