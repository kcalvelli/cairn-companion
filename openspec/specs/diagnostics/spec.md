## Purpose

The diagnostics capability provides the `companion doctor` command: a
self-contained health-check battery that inspects the daemon, session
store, channel adapters, OpenAI gateway, spoke reachability, workspace,
and persona resolution, reporting an aggregate pass/fail suitable for
humans, scripts, and monitoring.

## Requirements

### Requirement: Doctor command exists and is invocable without the daemon

The CLI SHALL provide a `companion doctor` subcommand that runs a fixed
battery of health checks and prints a report. The command SHALL run to
completion and produce a report even when the daemon is not running, so
that it is useful precisely when the daemon has failed to start.

#### Scenario: Daemon is running and healthy

- **WHEN** the user runs `companion doctor` with the daemon active and all
  surfaces healthy
- **THEN** every check is printed with an `OK` status
- **AND** the process exits with code `0`

#### Scenario: Daemon is not running

- **WHEN** the user runs `companion doctor` and the daemon's D-Bus name
  `org.cairn.Companion` is not owned by any process
- **THEN** the daemon-liveness check is reported as `FAIL`
- **AND** the daemon-dependent checks (sessions, channels, gateway state)
  are reported as `SKIP` with a reason referencing the down daemon
- **AND** the daemon-independent checks (spoke reachability, workspace,
  persona resolution) still run and report their own status
- **AND** the process exits with a non-zero code

### Requirement: Exit code reflects aggregate health

The `doctor` command SHALL exit `0` if and only if no check reported
`FAIL`. A `WARN` status SHALL NOT by itself cause a non-zero exit. This
makes the command usable in scripts, monitoring, and systemd hooks.

#### Scenario: A single check fails

- **WHEN** any check reports `FAIL`
- **THEN** the process exits with a non-zero code

#### Scenario: Only warnings, no failures

- **WHEN** one or more checks report `WARN` and none report `FAIL`
- **THEN** the process exits with code `0`

### Requirement: Daemon liveness and identity check

The `doctor` command SHALL verify that the daemon process is reachable by
confirming ownership of the D-Bus name `org.cairn.Companion` and that the
`org.cairn.Companion1` interface answers a status call. It SHALL report
the daemon version and uptime when reachable.

#### Scenario: Daemon owns the name and answers

- **WHEN** the daemon owns `org.cairn.Companion` and `GetStatus` returns
- **THEN** the check reports `OK` with the daemon version and uptime

#### Scenario: Name owned but interface unresponsive

- **WHEN** the D-Bus name is owned but the status call errors or times out
- **THEN** the check reports `FAIL` with the underlying error

### Requirement: Session store check

The `doctor` command SHALL report the session-store health by querying the
daemon for the active session count and confirming the query succeeds. A
failure to read sessions SHALL be reported as `FAIL`.

#### Scenario: Sessions queryable

- **WHEN** the daemon answers a session listing
- **THEN** the check reports `OK` with the session count

#### Scenario: Session query fails

- **WHEN** the session listing call errors
- **THEN** the check reports `FAIL` with the error

### Requirement: Channel adapter state check

The daemon SHALL expose, over D-Bus, the connection state of every
enabled channel adapter (Telegram, XMPP, email, Discord). The `doctor`
command SHALL report each enabled channel's state as one of `connected`,
`reconnecting`, or `down`, including the last error string when not
connected. Channels that are not enabled SHALL NOT appear in the report.

#### Scenario: An enabled channel is connected

- **WHEN** an enabled channel adapter holds a live connection to its
  upstream service
- **THEN** that channel is reported `OK` with state `connected`

#### Scenario: An enabled channel is retrying

- **WHEN** an enabled channel adapter has lost its connection and is in
  backoff-retry
- **THEN** that channel is reported `WARN` with state `reconnecting` and
  the last error

#### Scenario: A disabled channel

- **WHEN** a channel adapter is not enabled in configuration
- **THEN** that channel does not appear in the report

### Requirement: OpenAI gateway check

When the gateway is enabled, the `doctor` command SHALL probe the
gateway's `/health` endpoint and report `OK` only when it returns the
expected `{"status":"ok"}` body. When the gateway is not enabled, the
check SHALL be reported as `SKIP`, not `FAIL`.

#### Scenario: Gateway enabled and healthy

- **WHEN** the gateway is enabled and `GET /health` returns
  `{"status":"ok"}`
- **THEN** the check reports `OK`

#### Scenario: Gateway enabled but not answering

- **WHEN** the gateway is enabled but `/health` is unreachable or returns
  a non-ok body
- **THEN** the check reports `FAIL`

#### Scenario: Gateway disabled

- **WHEN** the gateway is not enabled
- **THEN** the check reports `SKIP`

### Requirement: Spoke reachability check

The `doctor` command SHALL read the spoke configuration
(`spoke-servers.json`) and probe each declared spoke — local and fleet
peer — with an MCP request to its `/mcp` endpoint. Each spoke SHALL be
reported individually with its host and round-trip latency on success.
When no spoke configuration exists, the entire spoke section SHALL be
reported as `SKIP`.

An unreachable spoke SHALL be classified by whether its tool requires a
graphical session. Spokes whose tool is **session-scoped** (those the
home-manager module starts `After=graphical-session.target`: `notify`,
`screenshot`, `clipboard`, `apps`, `niri`) SHALL be reported as `WARN`
when unreachable, because a host that is currently headless is expected
to have them down — this is a normal steady state, not a fault. Spokes
whose tool is **session-independent** (`journal`, `shell`) SHALL be
reported as `FAIL` when unreachable, because those have no graphical
dependency and being down indicates a real problem. The tool class is
determined from the spoke's name suffix (the segment after the final
`companion-`), so it applies uniformly to local and fleet-peer spokes.

#### Scenario: A local spoke answers

- **WHEN** a spoke declared in `spoke-servers.json` answers its `/mcp`
  endpoint
- **THEN** that spoke is reported `OK` with its host and latency

#### Scenario: A session-independent fleet peer spoke is unreachable

- **WHEN** a fleet-peer spoke whose tool is `journal` or `shell` does not
  answer within the probe timeout
- **THEN** that spoke is reported `FAIL` with the connection error
- **AND** other spokes are still probed and reported independently

#### Scenario: A session-scoped spoke is unreachable on a headless host

- **WHEN** a spoke whose tool is `notify`, `screenshot`, `clipboard`,
  `apps`, or `niri` does not answer within the probe timeout
- **THEN** that spoke is reported `WARN`, not `FAIL`
- **AND** the aggregate exit code is not failed by that spoke alone

#### Scenario: No spoke configuration present

- **WHEN** no `spoke-servers.json` is found at any detected path
- **THEN** the spoke section reports `SKIP`

### Requirement: Workspace check

The `doctor` command SHALL verify that the workspace directory exists and
is writable, and SHALL report the freshness of the memory index
(`MEMORY.md`) relative to the memory files when a memory tier is present.

#### Scenario: Workspace exists and is writable

- **WHEN** the workspace directory exists and the process can write to it
- **THEN** the check reports `OK`

#### Scenario: Workspace missing or read-only

- **WHEN** the workspace directory does not exist or is not writable
- **THEN** the check reports `FAIL` with the path and the reason

### Requirement: Persona resolution check

The `doctor` command SHALL resolve the composed persona file set using the
same detection and ordering the Tier 0 wrapper uses, and SHALL report
`FAIL` listing any persona file that is declared but does not exist on
disk. This catches the uncommitted-flake-source failure mode at diagnosis
time rather than via a session that silently speaks in the default voice.

#### Scenario: All persona files resolve

- **WHEN** every persona file in the composed set exists on disk
- **THEN** the check reports `OK` with the count of files composed

#### Scenario: A declared persona file is missing

- **WHEN** a persona file referenced by configuration does not exist at
  its resolved store path
- **THEN** the check reports `FAIL` and names the missing file(s)

### Requirement: Machine-readable output

The `doctor` command SHALL accept a `--json` flag that emits the full
report as a single JSON document instead of the human-readable rendering.
The JSON SHALL include, for every check, its identifier, status
(`ok` / `warn` / `fail` / `skip`), a human-readable detail string, and any
structured fields relevant to that check (e.g. latency for spokes,
last-error for channels).

#### Scenario: JSON output requested

- **WHEN** the user runs `companion doctor --json`
- **THEN** the command prints a single valid JSON document and no
  human-formatted lines
- **AND** the exit-code contract is identical to the human-readable mode
