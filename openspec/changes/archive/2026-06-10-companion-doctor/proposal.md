## Why

Every operational failure of cairn-companion observed so far has been
*diagnosable but slow*. A stale OpenAI-gateway session that broke after a
model retirement took 20 minutes of log diving to find what one SQLite
`DELETE` would have fixed (this is the documented motivation for
`tui-session-management`). An apparent "MCP servers won't connect" outage
turned out to be two expired OAuth tokens on remote claude.ai connectors
while every local spoke was answering `200` — but confirming that meant
hand-probing seven ports, reading the daemon journal, and inspecting
`spoke-servers.json` by hand. The system is almost always fine; finding
out that it is fine is the expensive part.

There is no single command that answers "is my companion healthy, and if
not, which piece is broken?" `companion status` reports daemon uptime and
session counts but says nothing about channel connections, the gateway,
spoke reachability, the workspace, or whether the persona files even
resolve. This change adds that command.

## What Changes

- **New `companion doctor` CLI subcommand** that runs a fixed battery of
  health checks and prints a one-screen, color-coded (OK / WARN / FAIL)
  report. Exits `0` when everything is green, non-zero when any check
  fails — so it is usable in scripts and `ExecStartPost` hooks, not just
  by a human reading the terminal.
- **Daemon-independent checks run even when the daemon is down.** If the
  daemon is unreachable that becomes the headline FAIL, but spoke probes,
  gateway probe, workspace checks, and persona resolution still run and
  report, because those are exactly the things you want to see when the
  daemon won't start.
- **New D-Bus method on `org.cairn.Companion1`** that reports per-channel
  connection state and gateway-enabled/listening state. The current
  `GetStatus` exposes version, uptime, session count, and in-flight turns
  but nothing about whether Telegram is connected or the gateway is
  serving. `doctor` needs that, so the daemon must expose it.
- **`--json` flag** on `doctor` for machine-readable output (TUI panel,
  monitoring, future `companion-doctor`-as-a-spoke).
- **No remediation.** `doctor` reports; it never restarts, deletes, or
  repairs. Diagnosis and action stay separate commands.

## Capabilities

### New Capabilities

- `diagnostics`: The `companion doctor` command and the daemon-side
  health surface that backs it — the full set of checks (daemon liveness,
  D-Bus name ownership, session store, gateway, channel adapters, spoke
  reachability, workspace, persona resolution), their OK/WARN/FAIL
  semantics, exit-code contract, graceful degradation when the daemon is
  down, and the `--json` output shape. Includes the new D-Bus health
  method that exposes channel and gateway connection state to any client.

### Modified Capabilities

<!-- None. The daemon D-Bus health method is new surface area folded into
     the diagnostics capability rather than a requirement change to an
     existing shipped spec; openspec/specs/ has no baseline dbus-interface
     spec to delta against. -->

## Impact

- **`packages/cli-client`** — new `Command::Doctor` variant, a new
  `doctor` module implementing the checks, dispatch in `main()`. Reuses
  the existing `dbus::CompanionProxy`. Adds an HTTP client dependency for
  spoke/gateway probes (lightweight — `reqwest` or a hand-rolled probe
  consistent with how spokes are already POSTed elsewhere).
- **`packages/companion-core`** — new method on the `org.cairn.Companion1`
  interface (`packages/companion-core/src/dbus.rs`) returning channel and
  gateway state; the dispatcher/daemon must track per-channel connection
  status to populate it (today channel adapters are fire-and-forget
  `tokio::spawn` tasks with no reported state).
- **`spoke-servers.json`** — read by `doctor` (not written) to enumerate
  which spokes to probe and at which addresses. Same file the wrapper
  already passes to Claude Code via `--mcp-config`.
- **Persona/workspace resolution** — `doctor` reuses the same config-file
  detection order the Tier 0 wrapper uses, so the persona-file-resolution
  check catches the documented uncommitted-flake-source footgun at
  diagnosis time instead of via a generic-voice session.
- **Docs** — README/`docs` gain a short `companion doctor` section.
- **No new dependencies between machines, no new network surface, no auth
  layer.** Spoke probes go to addresses already in `spoke-servers.json`
  over the existing Tailscale trust boundary.
