## 1. Daemon health surface

- [x] 1.1 Add a channel-health registry to the daemon: a shared
  `Arc`-held structure mapping each enabled channel name to its state
  (`connected` / `reconnecting` / `down`) and last-error string. Model it
  as per-channel atomics/enums, not a lock held across awaits.
- [x] 1.2 Have each channel adapter (telegram, xmpp, email, discord) write
  its state into the registry at the same points it already logs
  connection transitions (connect, disconnect, backoff-retry).
- [x] 1.3 Track gateway state (`enabled`, `listening`) so `GetHealth` can
  report it without a self-HTTP call.
- [x] 1.4 Add `GetHealth` method to the `org.cairn.Companion1` interface in
  `packages/companion-core/src/dbus.rs` returning per-channel state and
  gateway state as a structured map. Leave `GetStatus` untouched.
- [x] 1.5 Unit-test the registry: state transitions reflect correctly,
  disabled channels are absent, last-error is captured on `down`.

## 2. CLI D-Bus proxy

- [x] 2.1 Add the `get_health` proxy method to
  `packages/cli-client/src/dbus.rs` mirroring the new daemon method.

## 3. Check framework

- [x] 3.1 Define the uniform `CheckResult { id, status, detail, fields }`
  model with `Status ∈ {Ok, Warn, Fail, Skip}` and serde derives for the
  `--json` path.
- [x] 3.2 Implement the aggregate exit-code rule: non-zero iff any result
  is `Fail`; `Warn` and `Skip` do not fail the run.
- [x] 3.3 Implement the human renderer (one colored line per result, with
  indented sub-results for per-channel and per-spoke entries) and the JSON
  renderer (serialize the result vector as a single document).

## 4. Individual checks

- [x] 4.1 Daemon liveness + identity: confirm `org.cairn.Companion` name
  ownership and that `GetStatus` answers; report version + uptime on OK,
  `FAIL` on owned-but-unresponsive.
- [x] 4.2 Session store: query session count via the proxy; `OK` with
  count, `FAIL` on error, `SKIP` when daemon down.
- [x] 4.3 Channel adapters: read `GetHealth`; report each enabled channel
  `connected`=OK / `reconnecting`=WARN / `down`=FAIL with last error; omit
  disabled channels; `SKIP` section when daemon down.
- [x] 4.4 Gateway: when enabled, probe `GET /health` and require
  `{"status":"ok"}`; `OK` / `FAIL` accordingly; `SKIP` when disabled.
- [x] 4.5 Spoke reachability: locate and parse `spoke-servers.json`; probe
  each spoke's `/mcp` with a minimal JSON-RPC request, concurrently, with a
  short per-spoke timeout; report each spoke `OK` (host + latency) or
  `FAIL` (error); `SKIP` section when no config found.
- [~] 4.6 Workspace: verify directory exists and is writable; report
  memory-index (`MEMORY.md`) freshness vs memory files when present.
  _Done: exists + writability probe (the FAIL-bearing part). Deferred:
  MEMORY.md freshness — the staleness threshold was a design.md open
  question and is a `WARN`-only nicety, not yet implemented._
- [x] 4.7 Persona resolution: resolve the composed persona set using the
  same detection/ordering as the Tier 0 wrapper; `FAIL` naming any
  declared-but-missing file; `OK` with composed-file count otherwise.

## 5. CLI wiring

- [x] 5.1 Add `Command::Doctor { json: bool }` to the clap enum in
  `packages/cli-client/src/main.rs` and an `async fn cmd_doctor(json:
  bool) -> i32` dispatched from `main()`.
- [x] 5.2 Ensure daemon-independent checks (spoke, workspace, persona)
  run and report even when the daemon is unreachable, with daemon-dependent
  checks reported `SKIP`.
- [x] 5.3 Choose the HTTP client for spoke/gateway probes based on the
  existing dependency tree (prefer reuse / minimal feature set); wire it
  in.

## 6. Verification and docs

- [x] 6.1 Add tests: exit-code contract (FAIL→nonzero, WARN/SKIP→zero),
  JSON output validity and parity with human mode, graceful degradation
  with daemon down.
- [~] 6.2 Manual end-to-end on `edge`: healthy all-green run; daemon
  stopped (headline FAIL, spokes still probed); a dead fleet peer in
  `spoke-servers.json` (that spoke FAIL, others OK); a deliberately missing
  persona file (persona check FAIL).
  _Done live against the running daemon: daemon/sessions/workspace/persona
  OK, real dead mini fleet-peer spokes correctly FAIL, exit code 1 (0 when
  only WARN/SKIP), JSON parity. Graceful degradation proven incidentally —
  the running daemon predates `GetHealth`, so channels/gateway SKIP with
  "daemon did not report health". Still to exercise (need the new daemon
  deployed via `nixos-rebuild`, and must not bounce the family's live
  daemon mid-session): connected-channel → OK / reconnecting → WARN path,
  full daemon-stopped headline FAIL, deliberate missing-persona FAIL._
- [x] 6.3 Add a short `companion doctor` section to the README/docs and
  update ROADMAP/IMPROVEMENTS status for this change.
