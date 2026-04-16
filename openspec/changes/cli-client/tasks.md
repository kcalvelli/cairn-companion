# Tasks: cli-client

## Completed

- [x] Create `packages/cli-client/` Rust package with clap + zbus
- [x] Implement D-Bus proxy matching daemon's `org.cairn.Companion1` interface
- [x] Default send mode: `companion "prompt"` streams via D-Bus signals
- [x] Interactive REPL: `companion chat` with streaming responses
- [x] Stdin mode: `companion -` reads from stdin
- [x] `companion status` — daemon health display
- [x] `companion sessions list` — tabular session listing
- [x] `companion surfaces` — active surface listing
- [x] Nix package (`default.nix`, flake output, Nix build verified)
- [x] Home-manager integration: `cli.enable`, `cli.package` options
- [x] PATH conflict resolution: CLI as `companion`, wrapper as `companion-raw`
- [x] Assertion: `cli.enable → daemon.enable`

## Second batch (2026-04-16)

- [x] `companion logs [-f] [--surface <pat>]` — shells out to `journalctl --user -u companion-core`, `--follow` maps to `-f`, `--surface` maps to `--grep <pat>` (journalctl's PCRE filter). No daemon-side log streaming method needed after all — the daemon already writes to the user journal, so client-side journalctl is both simpler and more featureful.
- [x] `companion sessions show <surface> <conversation_id>` — new D-Bus method `GetSession(surface, conv_id)` returning `(surface, conv_id, claude_session_id, status, created_at, last_active_at, metadata)`. Prints a formatted block. Returns exit 1 + stripped FileNotFound message if missing.
- [x] `companion sessions delete <surface> <conversation_id>` — new D-Bus method `DeleteSession(surface, conv_id)` wrapping the existing store method. Returns bool.
- [x] Shell completions via `clap_complete` — `companion completions <bash|fish|zsh|elvish|powershell>` emits to stdout. One-shot `Shell` enum argument, no file writing. (Minor: clap_complete panics on SIGPIPE when piped to `head`; does not affect real usage like `companion completions fish > ~/.config/fish/completions/companion.fish`.)

**Semantic decision:** subcommands take `<surface> <conversation_id>` as a positional pair, not a numeric SQLite PK. Matches what `sessions list` displays and what the data model uses as the natural key. Numeric row IDs are an internal detail.

## Still deferred

- [ ] `companion sessions resume <id>` — works today via `COMPANION_CONVERSATION_ID=<conv_id> companion chat`. Not worth a dedicated subcommand until there's a UX argument for one.
- [ ] `companion memory list|show|edit` — workspace/memory tier isn't landed yet (Tier 0 has no memory per the roadmap). Premature.
- [ ] Passthrough mode: unknown flags forwarded to daemon's SendMessage — ill-specified. The existing wrapper `companion-raw` exec-path handles a full Claude Code session when there are no args. If a specific passthrough use case emerges, reopen.
