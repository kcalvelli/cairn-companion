# Tasks — spoke-http-transport

## Phase 1: HTTP transport in spoke-tools lib

- [x] Make `handle_request` pub in lib.rs
- [x] Add `serve_http()` — axum server, single `POST /mcp` endpoint,
      `Mcp-Session-Id` on initialize, 202 for notifications, 405 on GET
- [x] Add `run()` helper — branches on `MCP_TRANSPORT` env var
- [x] Add axum + tokio `net` to Cargo.toml
- [x] Add reqwest (dev) for HTTP transport tests
- [x] 5 HTTP transport tests (initialize+session-id, tools/call, notification→202, GET→405, session-id echo)

## Phase 2: Binary wiring

- [x] Replace `serve(H)` with `run(H)` in all 7 binaries
- [x] Handle shell.rs name collision (`run` local fn vs import)
- [x] All 11 tests pass (6 existing + 5 new)

## Phase 3: Home-manager module

- [x] Add `http.enable` + `http.port` sub-options to all 7 tool definitions
- [x] Generate systemd user services for HTTP-mode spokes (long-running, Restart=on-failure)
- [x] Shell HTTP service includes COMPANION_SHELL_ALLOWLIST in Environment
- [x] `nix flake check` passes

## Phase 4: Verification

- [x] End-to-end: start a spoke in HTTP mode, hit it with curl
- [x] End-to-end: edge→mini and mini→edge spoke calls over HTTP
- [x] Update proposal.md status
- [x] Update ROADMAP.md

## Phase 5: Direct HTTP config (bypass gateway)

- [x] Fleet config option (`services.cairn-companion.fleet`) — peers, domain, hostname
- [x] Generate `spoke-servers.json` — local HTTP spokes + fleet peer entries
- [x] Companion wrapper passes spoke-servers.json as second `--mcp-config`
- [x] Session-dependent HTTP services use `After=graphical-session.target`
- [x] Remove spoke stdio registration on edge (spokes no longer route through gateway)
- [x] Verified: mini Sid launches Brave on edge, sends notifications cross-host
