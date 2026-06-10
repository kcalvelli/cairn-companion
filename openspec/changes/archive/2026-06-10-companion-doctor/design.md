## Context

cairn-companion has accumulated several surfaces — the Tier 1 daemon, the
OpenAI gateway, four channel adapters, and a fleet of HTTP spoke tools —
but no single way to ask "is all of this healthy?" The existing
`companion status` command calls the daemon's `GetStatus` D-Bus method,
which today returns only `version`, `uptime_seconds`, `active_sessions`,
and `in_flight_turns` (`packages/companion-core/src/dbus.rs`). It says
nothing about channel connections, the gateway, spoke reachability, the
workspace, or persona resolution.

The cost of that gap is documented in the project's own history: a stale
gateway session took 20 minutes to diagnose (the origin of
`tui-session-management`), and a recent "MCP won't connect" scare was two
expired remote-connector OAuth tokens masking the fact that all seven
local spokes were answering `200`.

Relevant existing surfaces this change builds on:

- **D-Bus client**: `packages/cli-client/src/dbus.rs` defines
  `CompanionProxy` against `org.cairn.Companion1` at
  `/org/cairn/Companion`. Adding a method is a one-line proxy addition
  plus the daemon-side impl.
- **CLI dispatch**: `packages/cli-client/src/main.rs` is clap-based with a
  `Command` enum dispatched in `main()`. Each subcommand is an
  `async fn cmd_*` returning an `i32` exit code.
- **Gateway health**: `packages/companion-core/src/gateway/mod.rs` already
  serves `GET /health` → `{"status":"ok"}`.
- **Spoke config**: the home-manager module writes
  `spoke-servers.json` (`modules/home-manager/default.nix`) with local
  HTTP spokes at `localhost:1879x/mcp` and fleet peers at Tailscale
  addresses. The wrapper detects it and passes it to Claude Code as
  `--mcp-config`.

## Goals / Non-Goals

**Goals:**

- One command, `companion doctor`, that gives a complete green/red picture
  of every companion surface in under a few seconds.
- Useful when the daemon is down, not just when it is up — degrade
  gracefully and still report everything that doesn't need the daemon.
- A clean exit-code contract so the command works in scripts and systemd
  hooks, plus `--json` for the TUI and monitoring.
- Reuse existing surfaces (D-Bus proxy, gateway `/health`,
  `spoke-servers.json`) rather than inventing new state stores.

**Non-Goals:**

- No remediation. `doctor` never restarts services, deletes sessions, or
  edits config. (Restart/cleanup belong to `systemctl --user` and
  `tui-session-management`.)
- No probing of Claude Code's own MCP config or claude.ai connectors —
  those live outside this project's boundary (Wrapper-Only rule).
- No new network surface and no auth. Spoke probes hit addresses already
  in `spoke-servers.json` over the existing Tailscale trust boundary.
- No continuous/background health monitoring — `doctor` is a point-in-time
  command. A future `companion-doctor` spoke or scheduled run can layer on
  top, but that is out of scope here.

## Decisions

### Decision: `doctor` lives in `cli-client`, orchestrates checks itself

The command is implemented in `packages/cli-client` as a new
`Command::Doctor { json: bool }` variant and a `doctor` module. The CLI is
the right home because most checks are client-side (HTTP probes,
filesystem, persona resolution) and only some need the daemon. The daemon
stays a *data source*, not the orchestrator.

*Alternative considered:* implement `doctor` as a daemon D-Bus method that
runs all checks server-side. Rejected because the headline requirement is
"works when the daemon is down" — a daemon-side implementation can't
report on its own absence.

### Decision: Add one D-Bus method, `GetHealth`, for channel + gateway state

The daemon gains a `GetHealth` method on `org.cairn.Companion1` returning
a structured map: per-channel `{name, state, last_error}` and gateway
`{enabled, listening}`. This is the only genuinely new daemon surface
area. Channel adapters today are fire-and-forget `tokio::spawn` tasks with
no externally visible state; populating `GetHealth` requires the daemon to
track each adapter's connection status.

*Implementation approach:* a shared `Arc<...>` health registry the
dispatcher holds, where each channel adapter writes its current state
(`connected` / `reconnecting` / `down` + last error) on every connection
transition. The adapters already have reconnect-with-backoff loops; the
write goes at the same points where they log connection state today. This
keeps the registry a passive mirror of state the adapters already compute.

*Alternative considered:* extend the existing `GetStatus` map with channel
keys. Rejected — `GetStatus` is consumed by `companion status` and the TUI
with a fixed set of scalar keys; overloading it with nested per-channel
state muddies a stable interface. A separate `GetHealth` keeps concerns
clean and lets `doctor` and a future TUI health panel share one method.

### Decision: Spoke probe is an MCP `initialize`-style POST, not a raw TCP check

Each spoke is probed by POSTing a minimal JSON-RPC request to its `/mcp`
endpoint and checking for a well-formed response, not merely opening a
socket. A spoke process can be listening while its MCP handler is wedged;
a real request distinguishes "port open" from "actually serving." Latency
is measured around the request. Probe timeout is short (default ~2s) and
per-spoke, so one dead fleet peer doesn't stall the whole report.

*Alternative considered:* TCP connect only. Rejected — too shallow; the
recent incident was about whether spokes *serve*, not whether ports are
open (they were).

### Decision: Persona + workspace checks reuse the wrapper's detection logic

The persona-resolution and workspace checks must use the *same* file
detection and ordering as the Tier 0 wrapper, or they'd test a different
thing than what actually runs. The wrapper's detection order (persona
files, `mcp-config` paths, workspace dir) is the source of truth; `doctor`
factors that resolution into a shared helper both can call, or replicates
it with a test asserting parity. The whole point is to catch the
uncommitted-flake-source footgun, so it must resolve persona files exactly
as the wrapper does.

### Decision: Check result model is uniform

Every check produces a `CheckResult { id, status, detail, fields }` where
`status ∈ {Ok, Warn, Fail, Skip}`. The human renderer prints one line per
result with a colored status glyph and indented sub-results (per channel,
per spoke). The `--json` renderer serializes the same vector. Aggregate
exit code is non-zero iff any result is `Fail` (`Warn` and `Skip` do not
fail the run). This uniform model is what makes the exit-code contract and
the JSON shape fall out for free.

## Risks / Trade-offs

- **[Channel state tracking adds shared mutable state to the daemon]** →
  Keep the registry write-only from adapters and read-only from
  `GetHealth`; model it as a per-channel atomic/enum behind an `Arc`, not
  a lock held across awaits. Mirrors state the adapters already compute,
  so no new failure modes in the turn path.
- **[Spoke probes could slow `doctor` if many fleet peers are down]** →
  Per-spoke short timeout and concurrent probing; total spoke-section time
  is bounded by the slowest single probe, not the sum.
- **[Persona detection drift]** → If `doctor` and the wrapper diverge, the
  check tests the wrong thing. Mitigate by extracting one shared
  resolution helper, or a parity test pinning them together.
- **[`doctor` reports a false FAIL during a transient channel reconnect]**
  → Reconnecting is `WARN`, not `FAIL`, and does not affect exit code; only
  a hard-down or erroring surface is `FAIL`.
- **[Adding `reqwest` bloats the CLI binary]** → Use a minimal HTTP client
  configured without unnecessary TLS/features, consistent with how spokes
  are already contacted; or reuse whatever HTTP dependency the workspace
  already pulls in. Decide at implementation time based on the existing
  dependency tree.

## Migration Plan

Purely additive. New CLI subcommand and one new D-Bus method; no existing
command, method, or config option changes behavior. Ship order:

1. Add `GetHealth` to the daemon D-Bus interface and the channel-state
   registry; existing `GetStatus` untouched.
2. Add the `GetHealth` proxy method to the CLI `dbus.rs`.
3. Implement the `doctor` module and `Command::Doctor` dispatch.
4. Wire docs.

Rollback is removing the subcommand; the `GetHealth` method is harmless if
unused. No data migration, no state changes.

## Open Questions

- **HTTP client choice** — `reqwest` vs a hand-rolled minimal probe.
  Defer to implementation; pick whatever keeps the CLI binary lean given
  the existing dependency tree.
- **Should `doctor` also check that `mcp-gw --json list` succeeds** (the
  daemon uses it at startup to build the deny list)? Leaning yes as a
  `WARN`-level check, since a broken gateway-list breaks anonymous-trust
  turns — but it is daemon-internal and may belong in `GetHealth` rather
  than a client-side probe. Resolve during spec review.
- **Workspace memory-index freshness** — exact staleness threshold (mtime
  comparison vs content hash). Start with mtime; refine if it produces
  false WARNs.
