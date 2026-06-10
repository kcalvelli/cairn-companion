# Improvements roadmap

Queue of proposal-ready work items from the 2026-06-09 whole-repo review.
Each entry has enough context to seed `/opsx:propose` directly — name,
tier, motivation, scope sketch, dependencies, and success criteria. Items
graduate into [ROADMAP.md](./ROADMAP.md) when their proposal lands in
`openspec/changes/`; until then this file is the holding pen.

Ordered by leverage, not effort. Top of the list pays for itself fastest.

---

## 1. companion-doctor — ✅ SHIPPED 2026-06-10

> Done and archived at
> `openspec/changes/archive/2026-06-10-companion-doctor/`. Deployed and
> verified live on edge + mini: `GetHealth` D-Bus method + channel
> `HealthRegistry`, `companion doctor` with `--json`, session-scoped
> spokes WARN instead of FAIL. The `diagnostics` capability is now a
> baseline spec in `openspec/specs/`.

**Tier:** 1 (CLI subcommand; probes Tier 2 surfaces when present)
**Depends on:** daemon-core, cli-client. Spoke/gateway probes degrade
gracefully when those tiers aren't enabled.

**Motivation.** Every observed failure of this system so far has been
diagnosable but slow: a stale gateway session took 20 minutes of log
diving (`tui-session-management`'s origin story), and an apparent "MCP
servers not connecting" outage was two expired OAuth tokens on remote
connectors while every local spoke answered 200. The system is fine;
*finding out* the system is fine is the expensive part.

**Scope sketch.** `companion doctor` — one screen, green/red per check,
nonzero exit if anything is red:

- daemon: unit active, D-Bus name `org.cairn.Companion` acquired,
  `Status` method answers
- session store: SQLite opens, schema version matches, session count
- OpenAI gateway: `/health` returns ok (when enabled)
- channels: each enabled adapter's connection state (connected /
  reconnecting / dead, last error)
- spokes: HTTP POST to every entry in `spoke-servers.json` — local and
  fleet peers — with per-host latency
- workspace: exists, writable, MEMORY.md index fresh
- persona: every composed file resolved (catches the
  uncommitted-flake-source footgun at runtime instead of via generic
  voice)

**Non-goals.** No remediation (`doctor` reports, never repairs). No
probing of claude-code's own MCP config or claude.ai connectors — out
of this project's boundary.

**Success criteria.** Both historical incidents above would have been
diagnosed by `companion doctor` in under 10 seconds.

---

## 2. gateway-trust-level

**Tier:** 1
**Depends on:** openai-gateway, model-overrides (env-var lowering pattern).

**Motivation.** The OpenAI gateway hardcodes `TrustLevel::Anonymous`
(`gateway/mod.rs`), so HA voice — the most-used surface in the house —
gets the tool-stripped companion. The only clients that can reach the
port are on the tailnet. Voice that can fire notifications and check
the fleet is a categorically better product than voice that can only
chat.

**Scope sketch.** `gateway.openai.trustLevel = "anonymous" | "owner"`,
default `"anonymous"` (zero behavior change unset). Lowered via env var
like the model overrides; dispatcher applies the same Owner path the
allowlisted channels use. Document loudly that `"owner"` means anything
that can reach the port speaks as you — Tailscale ACL accordingly.

**Non-goals.** No per-request auth, no API keys, no token exchange
(No-Separate-Auth rule). Trust is port-level and binary, same as every
other surface.

**Success criteria.** Voice turn through HA can invoke MCP tools when
`trustLevel = "owner"`; default config behaves exactly as today.

---

## 3. hardening-pass

**Tier:** 1 + 2 (touches daemon, modules, and one spoke)
**Depends on:** daemon-core, spoke-tools, memory-tier.

**Motivation.** Four small correctness/safety findings from the review,
none big enough to carry a proposal alone:

1. **Session lock leak** — `dispatcher.rs` `session_locks:
   HashMap<SessionKey, Arc<Mutex<()>>>` grows forever; no eviction on a
   daemon designed to run for months. TTL-based cleanup on an existing
   timer.
2. **Missing module assertions** — nothing prevents
   `spoke.tools.X.http.enable` without `spoke.tools.X.enable`; nothing
   warns when `channels.*` are enabled without `daemon.enable`. Cheap
   `assertions` blocks that fail the build instead of failing at
   runtime.
3. **Apps spoke path traversal** — desktop entry names aren't checked
   for `/` before file search (`spoke-tools/src/bin/apps.rs`). Low real
   risk, two-line reject.
4. **Memory slug coupling** — `modules/nixos/default.nix` re-implements
   claude-code's workspace-path slugification for the Syncthing folder.
   If upstream changes the algorithm, sync silently points at a dead
   directory. Add a pre-flight existence check (service-level
   `ExecStartPre` or assertion-adjacent warning).

**Non-goals.** No new features, no API changes, no refactors beyond the
four items.

**Success criteria.** Lock map bounded under churn (test with synthetic
session keys); misconfigured module combinations fail at `nix build`;
`launch_desktop_entry("../x")` rejected; missing memory dir surfaces a
loud error before Syncthing starts.

---

## 4. scheduler

**Tier:** 1 (new capability; the largest item here — full proposal with
design doc warranted)
**Depends on:** daemon-core, model-overrides.

**Motivation.** The companion is purely reactive — no heartbeat, no
"check the mail queue every morning," no scheduled anything. ZeroClaw
had this and its cost discipline with it. This is the biggest
functional regression vs the system this project replaced.

**Scope sketch.** Daemon-side schedule table. Each entry: cron
expression, prompt, surface to deliver output on (or discard), and a
per-entry model override so scheduled turns run on Haiku instead of
burning Opus by default. Declared in Nix
(`services.cairn-companion.schedules.<name> = { cron, prompt, surface,
model }`), surfaced read-only via D-Bus / `companion schedules list` /
TUI panel. Fired turns dispatch through the existing dispatcher as
their own surface (`schedule:<name>`) with Owner trust.

**Design constraints to encode in the spec:**

- Every schedule justifies its cost each firing or it gets deleted —
  the ZeroClaw rule, stated in the proposal as a usage norm.
- Declarative-only creation (Nix), no runtime API to create schedules —
  prevents the companion scheduling work for itself.
- Per-entry model override defaults to the cheapest viable model, not
  the daemon default.

**Non-goals.** No runtime schedule mutation, no agent-initiated
scheduling, no retry/queue semantics beyond "missed firings are
skipped, not replayed."

**Success criteria.** A Nix-declared schedule fires on cadence, runs on
its configured model, delivers to its configured surface, and survives
daemon restart. Zero schedules declared → zero new code paths active.

---

## 5. persona-dev-mode

**Tier:** 0
**Depends on:** bootstrap. No dependencies above Tier 0.

**Motivation.** Iterating on a persona today is edit → `git add` →
commit → `nixos-rebuild switch` — a brutal loop for prose tuning, and
the source of the documented uncommitted-files footgun (flake copies
stale source, persona silently falls back to generic voice). Persona
authoring is the thing this product is *for*; it should be the pleasant
part.

**Scope sketch.** `companion --persona-dir <path>`: compose the persona
from live files in `<path>` (same ordering rules: AGENT.md base, then
USER.md, then extras sorted or manifest-listed), print a loud
`DEV PERSONA — not your deployed config` banner to stderr, and skip
workspace scaffolding side effects. Declarative path stays canonical
for deployment; dev mode is for authoring sessions only.

**Non-goals.** No hot-reload mid-session (claude-code reads the system
prompt once), no persistence of the dev path, no module option — this
is a CLI flag, deliberately ephemeral.

**Success criteria.** Edit voice.md, re-run `companion --persona-dir`,
new voice is live — no commit, no rebuild, and the banner makes it
impossible to mistake a dev session for the deployed one.

---

## 6. spoke-bind-hardening

**Tier:** 2
**Depends on:** spoke-http-transport.

**Motivation.** Every HTTP spoke — including `companion-shell`, which
is arbitrary command execution — defaults to binding `0.0.0.0`, with a
comment asserting Tailscale ACLs are the boundary. Nothing verifies
Tailscale is running, and `0.0.0.0` answers the LAN too. The
architecture rule is Tailscale-first networking; the bind default
should enforce it instead of assuming it.

**Scope sketch.** Local-only spokes default to `127.0.0.1`. Fleet-
exposed spokes bind the host's Tailscale interface address specifically
(resolved at service start — `tailscale ip -4` in `ExecStartPre` or a
systemd `BindToDevice=tailscale0`-style constraint), not the wildcard.
`bindAddress` stays overridable for users who know what they're doing.
Migration note in the module: existing fleet configs keep working, the
wildcard just stops being the silent default.

**Non-goals.** No application-level auth on spokes (architecture rule),
no per-tool firewalling, no rate limiting (single-user fleet; revisit
only if it ever hurts).

**Success criteria.** Default config: spokes unreachable from LAN,
reachable from tailnet peers exactly as today. `nmap` from a non-
tailnet host on the same LAN shows nothing listening.

---

## 7. channel-shared-commands

**Tier:** 1
**Depends on:** all four shipped channel adapters.

**Motivation.** The `/new` / `!new`, `status`, `help` handlers are
byte-identical across four adapters, and Telegram/Discord each
reimplement single-message edit-streaming. ~400 extractable lines. Not
painful at four adapters; very painful the day a fifth (Matrix, Signal)
shows up and inherits four divergent copies.

**Scope sketch.** Phase 1 (cheap, do now): extract
`channels::commands::handle(surface_id, conversation_id, text,
dispatcher) -> String`; adapters keep their prefix conventions (`/` vs
`!`) and pass the stripped command in. Phase 2 (only when adapter five
is real): a `ChannelAdapter` trait + shared edit-streaming helper
parameterized on message-length cap and edit call.

**Non-goals.** No behavior change visible from any channel. Phase 2
does not ship until a fifth adapter proposal exists — no speculative
abstraction.

**Success criteria.** One copy of command logic; all existing channel
tests (and the new ones from item 8) pass unchanged.

---

## 8. channel-test-coverage

**Tier:** 1
**Depends on:** daemon-core. Pairs well with channel-shared-commands
(shared command handler is the first thing worth testing once).

**Motivation.** Dispatcher and store have ~500 lines of solid tests.
The four channel adapters, the OpenAI gateway, and the D-Bus memory
index have zero. The gateway is the one surface that parses untrusted
HTTP, which makes it the worst place for the test gap to live.

**Scope sketch.** Priority order:

1. Gateway: request parsing, SSE streaming, error envelopes,
   conversation-ID resolution — `tower` service tests, no real port.
2. Channel filtering logic: loop prevention (email auto-submitted /
   precedence), allowlist → trust mapping, mention stripping, archive-
   replay detection — pure functions, mock the dispatcher with mpsc.
3. Memory index: frontmatter parsing, index regeneration, `.stignore`
   handling.

**Non-goals.** No live-service integration tests against real
Telegram/Discord/IMAP — those stay manual, as the archived proposals'
verification sections already do them.

**Success criteria.** Gateway and channel filter paths covered;
`cargo test` still runs in seconds; a regression in loop prevention or
trust mapping fails CI instead of failing in production.

---

## 9. docs-restructure

**Tier:** n/a (docs only — may not need a full OpenSpec change; a
lightweight proposal keeps the workflow honest)
**Depends on:** nothing.

**Motivation.** README.md is 456 lines; the channel-adapter section
alone is 130. A reader deciding whether to use this project drowns
before reaching "Getting started."

**Scope sketch.** README shrinks to: what it is, what it is not, the
tiers table, a 20-line quickstart, links out. Everything else moves to
`docs/`: `channels.md`, `persona-authoring.md`, `fleet.md`,
`gateway.md`, `development.md`. Also: commit or `.gitignore` the
`.claude/` and `.grok/` skill mirrors currently sitting untracked.

**Non-goals.** No content rewrites beyond relocation and a tightened
README — the prose is good, there's just too much of it in one file.

**Success criteria.** README under 150 lines; no information lost; all
internal links resolve.

---

## Suggested batching

- **One sitting:** hardening-pass items 2–4, channel-shared-commands
  phase 1.
- **One proposal each, in order:** companion-doctor,
  gateway-trust-level, persona-dev-mode, spoke-bind-hardening.
- **The big one:** scheduler — full proposal + design doc, worth the
  ceremony.
- **Background:** channel-test-coverage and docs-restructure, whenever
  the mood strikes.
