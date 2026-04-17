# Proposal: Memory Tier — Surfacing Claude Code's Project Memory

> **Status**: Shipped 2026-04-17. All phases complete. cwd pinning, D-Bus memory methods, CLI subcommand, TUI panel, Syncthing-based cross-machine sync with local index regeneration. Verified live on edge + mini with bidirectional propagation.

## Tier

Tier 2

## Summary

Pin the daemon's spawned Claude Code sessions to the workspace directory
as their working directory, establishing a single canonical location for
Claude Code's native project memory. Then expose that memory read-only
over D-Bus so the TUI can display it and the CLI can inspect it.

This is NOT a custom memory system. Claude Code already writes and
manages project memory (`MEMORY.md` + individual memory files). We just
make it visible outside of an active Claude Code session.

## Motivation

Claude Code's project memory is invisible unless you're inside a Claude
Code conversation. The TUI dashboard shows sessions and streaming
responses but can't tell you *what the agent remembers*. The CLI can
list sessions but can't show you what knowledge persists between them.
Channel adapter users (Telegram, XMPP, Discord, email) have no way to
verify whether the agent retained something from a prior conversation.

The fix is a read window, not a parallel store. Claude Code owns the
memory lifecycle (create, update, delete). We own the visibility.

## Design principle

**If Claude Code does it, we don't redo it — we use it. If Claude Code
doesn't do it, we build it.**

Claude Code does: persistent memory storage, memory indexing
(`MEMORY.md`), reading/writing memory during sessions, deciding what to
remember.

Claude Code does NOT: expose memory outside of an active session, make
memory browsable from a dashboard, or let a CLI inspect it.

## Scope

### In scope

1. **Daemon change**: set `current_dir` on the spawned `companion`
   command to the workspace path. This pins Claude Code's project memory
   slug to a known, stable location:
   `~/.claude/projects/-home-keith--local-share-cairn-companion-workspace/memory/`

2. **New D-Bus methods** on `org.cairn.Companion1`:
   - `GetMemoryPath() → String` — returns the resolved memory directory
   - `ListMemoryFiles() → Vec<(String, u64, i64)>` — name, size bytes, mtime epoch
   - `ReadMemoryFile(name: String) → String` — returns file contents
   - `GetMemoryIndex() → String` — returns MEMORY.md contents

3. **CLI subcommand** (`companion memory`):
   - `companion memory list` — tabular display of memory files
   - `companion memory show <name>` — print file contents to stdout
   - `companion memory index` — print MEMORY.md

4. **TUI memory panel**:
   - Third panel (alongside Sessions and Conversation), toggled via `3` or `m`
   - Left: file list from MEMORY.md index
   - Right: selected file's contents, word-wrapped, scrollable
   - Refreshes on focus or on a configurable interval
   - Read-only — no editing from the TUI

5. **Wrapper change**: when invoked directly (Tier 0 / `companion-code`),
   `cd` to the workspace path before exec-ing claude, OR document that
   direct terminal invocations should be run from the workspace dir for
   memory continuity.

### Out of scope

- Writing, editing, or deleting memory files (Claude Code owns that)
- Memory search or semantic retrieval
- Cross-machine memory sync (distributed-routing territory)
- Injecting memory into channel adapter prompts (stretch goal, separate proposal)
- Any changes to Claude Code's memory format or behavior

### Non-goals

- Replacing Claude Code's memory system with something custom
- Building a vector store or embedding-based retrieval
- Making memory editable from TUI/CLI (opens conflict with Claude Code's index)

## Architecture

```
                     Claude Code (spawned by daemon)
                            │
                            │ writes/reads
                            ▼
~/.claude/projects/-home-keith--local-share-cairn-companion-workspace/memory/
    ├── MEMORY.md              (index)
    ├── user_role.md           (memory file)
    ├── feedback_testing.md    (memory file)
    └── ...
                            ▲
                            │ reads (filesystem)
                            │
                    companion-core daemon
                            │
                            │ D-Bus interface
                            ▼
                ┌───────────┼───────────┐
                │           │           │
          companion     companion   companion-tui
          memory list   memory show  (memory panel)
```

## Key decision: workspace-as-cwd

The daemon sets `current_dir(workspace_path)` on the spawned companion
process. This means:

- Claude Code's project slug is deterministic and stable
- All surfaces (daemon-spawned, terminal, TUI inspection) agree on location
- The workspace dir already exists, is already writable, already passed via `--add-dir`
- Memory files are siblings to whatever else the agent stores in workspace

The slug path (`-home-keith--local-share-cairn-companion-workspace`)
is derived by Claude Code's own path-mangling logic. We don't construct
it ourselves — we discover it at runtime by resolving:
`$HOME/.claude/projects/<slugified workspace path>/memory/`

The slugification algorithm: replace `/` with `-`, prepend `-`.
So `/home/keith/.local/share/cairn-companion/workspace` →
`-home-keith--local-share-cairn-companion-workspace`.

## Dependencies

- `daemon-core` (for D-Bus interface extension)
- `cli-client` (for new subcommand)
- `tui-dashboard` (for new panel)

All three are shipped and active.

## Success criteria

1. [ ] Daemon spawns companion with `current_dir` set to workspace
2. [ ] Memory files accumulate in the expected Claude Code project slug
3. [ ] `companion memory list` shows files with names, sizes, timestamps
4. [ ] `companion memory show <file>` prints contents correctly
5. [ ] `companion memory index` prints MEMORY.md
6. [ ] TUI memory panel renders file list and selected file contents
7. [ ] TUI panel refreshes when re-focused
8. [ ] Direct `companion-code` invocations from workspace dir share the same memory location

## Phases

**Phase 1: cwd pinning + D-Bus methods** — daemon change, verify memory
accumulates in the right place, expose over D-Bus.

**Phase 2: CLI subcommand** — `companion memory list|show|index`.

**Phase 3: TUI panel** — read-only memory browser in the dashboard.

## Risks

- **Claude Code changes its slug algorithm.** Low risk — the algorithm
  has been stable since launch. If it changes, we update one path
  derivation function.
- **Memory dir doesn't exist yet.** If no session has written memory to
  the workspace project, the directory won't exist. D-Bus methods return
  empty results gracefully — no crash, no confusing error.
- **File watching vs. polling.** The TUI could use inotify for live
  updates, but polling on focus is simpler and memory changes are
  infrequent. Start with poll, add inotify later if warranted.
