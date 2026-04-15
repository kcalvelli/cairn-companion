# cairn-companion

A persistent, customizable persona wrapper around [Claude Code](https://docs.claude.com/en/docs/claude-code) — turn your Claude subscription into an AI agent that lives with you across your Linux machines.

> **Part of the cairn ecosystem, but cairn is not required.** This project ships as a home-manager module that works on any NixOS system with Claude Code installed. It integrates naturally with [cairn](https://github.com/kcalvelli/cairn) and [mcp-gateway](https://github.com/kcalvelli/mcp-gateway), but neither is a hard dependency.

## What this is

cairn-companion is a thin wrapper that gives Claude Code three things it doesn't have out of the box:

1. **A persistent persona.** Response-format rules, user context, and (optionally) a full character voice that the agent adopts in every session. Your agent feels like the same person every time, not a stateless assistant.
2. **A home on your filesystem.** A workspace directory for memory, reference data, and long-lived state that the agent can read, write, and evolve across sessions.
3. **A path to distributed agency.** Optional tiers add a persistent daemon, channel adapters (Telegram/Discord/email/XMPP), a terminal dashboard, and — ultimately — multi-machine tool routing so the agent can act on whichever machine you're currently using.

## What this is NOT

- **Not a new AI model.** Claude Code does all the actual thinking. This is a wrapper.
- **Not a multi-tenant service.** Each user on each machine runs their own isolated instance using their own Claude subscription. There is no shared server, no accounts, no API tokens managed by this project.
- **Not a replacement for Claude Code.** If you want a chat interface to Claude, use Claude Code directly. This project exists for people who want Claude Code to feel like a persistent AI agent with a consistent identity and local agency.
- **Not cairn-specific.** Despite the name, nothing here requires cairn. The name reflects that this is the canonical companion for cairn users — not an cairn dependency.

## Core commitments

These are enforced in `openspec/config.yaml` as architectural rules and apply to every change proposal:

- **Wrapper around claude-code, nothing more.** If Claude Code already does it, cairn-companion doesn't reimplement it. Auth, tool execution, the agent loop, permission prompts, streaming — all belong to Claude Code.
- **Per-user, home-manager only.** No system-level services, no `sid` system user, no multi-tenant infrastructure. Everything runs under the user's systemd slice with the user's own credentials.
- **Tailscale for network trust.** When machines talk to each other (Tier 2), access control is network-level via Tailscale. No application-level auth, no tokens, no OAuth layer.
- **Personality is opt-in.** The default persona that ships with this repo is deliberately character-free — only response format and citation rules. Users who want a voice bring their own persona files.

## Tiers

cairn-companion ships in opt-in tiers. You can stop at any tier and still have a working agent.

| Tier | What you get | What runs |
|------|--------------|-----------|
| **0 — Shell wrapper** | A `companion` command that runs Claude with your persona files and workspace pre-loaded | Nothing persistent. Just a binary. |
| **1 — Single-machine daemon** | Persistent sessions, channel adapters (Telegram/Discord/email/XMPP), CLI with subcommands, TUI dashboard | A user-level systemd daemon |
| **2 — Distributed agency** | The agent can act on whichever machine you're currently using via mcp-gateway + Tailscale; active-spoke routing follows your presence | Hub daemon on one machine + mcp-gateway with companion tool servers on every machine |
| **(optional) GUI** | GTK4/libadwaita desktop app for visual dashboards and memory browsing | Opt-in GUI client |

See [ROADMAP.md](./ROADMAP.md) for the full build order and which OpenSpec proposals ship each tier.

## Getting started

> **Tier 0 is complete and most of Tier 1 is live.** The `companion` wrapper, the home-manager module, and the default character-free persona all work today. The Tier 1 daemon (`companion-core`) is running — systemd user service, D-Bus control plane, persistent session routing, streaming support, an OpenAI-compatible HTTP gateway for Home Assistant voice integration, a Rust CLI client, a ratatui TUI dashboard, and **three channel adapters: Telegram, XMPP, and email**. Remaining Tier 1 work: `channel-discord`, voice (STT+TTS), and a handful of deferred CLI subcommands. Layering your own character on top is covered in [Authoring a persona](#authoring-a-persona) below. See [ROADMAP.md](./ROADMAP.md) for what's next.

Adding cairn-companion to a NixOS + home-manager system looks like this:

```nix
# flake.nix
{
  inputs = {
    cairn-companion.url = "github:kcalvelli/cairn-companion";
    # ... your other inputs
  };
}

# home-manager configuration
{
  imports = [ inputs.cairn-companion.homeManagerModules.default ];

  services.cairn-companion = {
    enable = true;

    # Optional: start the Tier 1 daemon (D-Bus, session routing, streaming)
    daemon.enable = true;

    # Optional: layer your own persona files on top of the minimal default
    persona.userFile = ./my-user-context.md;
    persona.extraFiles = [ ./my-persona-voice.md ];
  };
}
```

Then, from any terminal:

```bash
companion                       # interactive session with persona + workspace + mcp pre-loaded
companion "quick question"      # interactive session, seeded with a first message
companion -p "one-shot prompt"  # non-interactive, prints response, exits
companion --resume              # continue the last session
```

Everything after the wrapper's own injections is passthrough — any flag Claude Code accepts, `companion` accepts. See [the wrapper contract in the bootstrap spec](./openspec/changes/bootstrap/specs/wrapper/spec.md) for the full list of supported invocation shapes and guarantees.

### First run

The first time you invoke `companion` after enabling the module, the wrapper scaffolds your workspace:

1. **Workspace directory created** at `$XDG_DATA_HOME/cairn-companion/workspace` (typically `~/.local/share/cairn-companion/workspace`) unless you set `services.cairn-companion.workspaceDir` to a different path.
2. **`README.md` written** into the workspace explaining what the directory is for — long-lived notes, reference material, memory the agent can read across sessions.
3. **`USER.md` template written** — but only if you have *not* set `services.cairn-companion.persona.userFile`. If you supplied your own user file via the module option, the wrapper skips scaffolding the template because your file is already the source of truth. If you didn't, the template lands with placeholder sections (`<your name>`, `<your role>`, etc.) that you fill in to give the agent context.
4. **Claude launches** with the composed persona as its system prompt, the workspace attached via `--add-dir`, and — if detected — your mcp-gateway config loaded.

Subsequent invocations skip scaffolding entirely and do not touch anything in the workspace. You can edit `USER.md`, add new files, delete things, or rearrange the directory freely between runs — the wrapper treats the workspace as yours after that first invocation.

### The daemon (Tier 1)

When `daemon.enable = true`, the home-manager module installs and starts `companion-core` — a systemd user service that turns the one-shot wrapper into a live service:

- **D-Bus control plane** on `org.cairn.Companion` with methods for sending messages, streaming responses, listing sessions, and querying status
- **Persistent session routing** — maps `(surface, conversation_id)` pairs to Claude sessions in SQLite, survives daemon restarts
- **Turn serialization** — one subprocess per session at a time, concurrent sessions run in parallel
- **Streaming** — `StreamMessage` returns immediately and emits `ResponseChunk` / `ResponseComplete` / `ResponseError` signals

The daemon invokes the same `companion` wrapper for every turn — it doesn't bypass persona resolution or workspace injection. Think of it as a persistent supervisor that knows how to route conversations.

```bash
# Check daemon status
systemctl --user status companion-core

# Send a message via D-Bus
busctl --user call org.cairn.Companion /org/cairn/Companion \
  org.cairn.Companion1 SendMessage sss "dbus" "test" "hello"

# List active sessions
busctl --user call org.cairn.Companion /org/cairn/Companion \
  org.cairn.Companion1 ListSessions

# Watch the journal
journalctl --user -u companion-core -f
```

The daemon is the foundation for all Tier 1 features — channel adapters, CLI client, and TUI dashboard all connect through its dispatcher.

### OpenAI gateway

When `gateway.openai.enable = true`, the daemon starts an OpenAI-compatible HTTP server alongside the D-Bus interface. This restores the voice integration that Home Assistant's Conversation integration depends on — HA points at the gateway's `/v1/chat/completions` endpoint and uses it as the LLM backend for Assist voice pipelines.

```nix
services.cairn-companion = {
  enable = true;
  daemon.enable = true;
  gateway.openai = {
    enable = true;
    port = 18789;           # default — matches ZeroClaw's port
    modelName = "sid";      # cosmetic — shown in /v1/models
    sessionPolicy = "per-conversation-id";  # or "single-session" / "ephemeral"
  };
};
```

Endpoints:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Returns `{"status":"ok"}` — for monitoring |
| `/v1/models` | GET | Returns configured model in OpenAI format |
| `/v1/chat/completions` | POST | OpenAI-compatible chat completions (streaming + non-streaming) |

Session control: send an `X-Conversation-ID` header to route requests to distinct sessions (e.g., one per room satellite). Without the header, all requests share a default session.

```bash
# Non-streaming
curl -s http://localhost:18789/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"hello"}]}'

# Streaming
curl -sN http://localhost:18789/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"messages":[{"role":"user","content":"hello"}],"stream":true}'

# With conversation ID
curl -s http://localhost:18789/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Conversation-ID: kitchen" \
  -d '{"messages":[{"role":"user","content":"hello"}]}'
```

No authentication — Tailscale network trust is the access boundary, same as every other Tier 1+ network surface.

### Channel adapters

When the daemon is running, channel adapters let outside-the-terminal surfaces talk to the companion the same way the CLI does — they all dispatch through the same session router. Each adapter is opt-in and runs as an async task inside `companion-core`. Today the daemon ships four: **Telegram** (long-poll bot via teloxide), **XMPP** (native client for self-hosted Prosody/ejabberd via tokio-xmpp), **email** (IMAP poll + SMTP via async-imap + lettre, with RFC 5322 thread-root session keying), and **Discord** (Gateway WebSocket + REST via serenity, with DM and guild channel support).

```nix
services.cairn-companion = {
  enable = true;
  daemon.enable = true;

  # Telegram bot
  channels.telegram = {
    enable = true;
    botTokenFile = "/run/agenix/telegram-bot-token";
    allowedUsers = [ 123456789 ];           # Telegram user IDs (deny by default)
    mentionOnly = false;                    # group chats only — DMs always handled
    streamMode = "single_message";          # edit-in-place via Telegram message edits
  };

  # XMPP client (DMs working today; MUC support is on the way)
  channels.xmpp = {
    enable = true;
    jid = "sid@chat.example.org";
    passwordFile = "/run/agenix/xmpp-bot-password";
    server = "127.0.0.1";                   # or a Tailscale Serve TCP host
    port = 5222;
    allowedJids = [ "keith@chat.example.org" ];
    mentionOnly = true;                     # MUC default — DMs always handled
    streamMode = "single_message";          # XEP-0308 Last Message Correction (Phase 4 pending)
    mucRooms = [
      { jid = "household@muc.chat.example.org"; nick = "Sid"; }
    ];
  };

  # Discord bot
  channels.discord = {
    enable = true;
    botTokenFile = "/run/agenix/discord-bot-token";
    allowedUserIds = [ 123456789012345678 ]; # Discord snowflake IDs (deny by default)
    mentionOnly = true;                     # guild channels only — DMs always handled
    streamMode = "single_message";          # edit-in-place via Discord message edits
  };

  # Email channel — the bot's own inbox
  channels.email = {
    enable = true;
    address = "bot@example.com";
    displayName = "Bot";
    passwordFile = "/run/agenix/email-bot-password";
    imapHost = "imap.example.com";          # IMAPS on 993 by default
    smtpHost = "smtp.example.com";          # SMTPS (implicit TLS) on 465 by default
    allowedSenders = [ "alice@example.com" ];  # Owner trust; everyone else is Anonymous
    pollIntervalSecs = 30;                  # poll, not IDLE — IDLE is a follow-up
  };
};
```

**Shared design rules across all channel adapters:**

- **Empty allowlist = deny everyone (or Anonymous trust).** There is no implicit-trust mode. Telegram and Discord use numeric user IDs; XMPP uses bare JIDs; email uses addresses. If you forget the allowlist, the bot connects but everyone gets Anonymous trust at best.
- **Sessions are keyed by `(surface_id, conversation_id)`.** A given human gets a continuous session per channel — but Telegram, XMPP, Discord, and email are separate sessions by design, because the same person on different channels often *means* different things in each.
- **Three commands across all adapters:** `new` resets the session, `status` reports session state, `help` lists them. Telegram uses the `/` prefix (Telegram registers commands server-side). XMPP, Discord, and email use `!` because their clients intercept `/` locally.
- **Unrecognized commands get a deflection reply,** not a forward to the dispatcher. This prevents Claude Code skill leakage from typos like `/skil` or `!skil`.
- **`streamMode = "single_message"` is the default for all.** Telegram uses native message edits. XMPP uses XEP-0308 Last Message Correction. Discord uses message edits via the REST API. `multi_message` collects the full response and splits it: 4096 chars for Telegram, ~3000 chars for XMPP, 2000 chars for Discord.
- **All adapters reconnect with backoff** if the upstream service goes away.

**Telegram-specific notes:**

- One bot token = one long-poller, so the adapter is single-host (run it on whichever machine the bot belongs on). The daemon does not support running two adapters against the same token.
- `mentionOnly = true` only affects group chats; private DMs are always handled regardless of the setting.

**XMPP-specific notes:**

- Connects directly to the address you give it — **no SRV lookups.** This is intentional so that a Tailscale Serve TCP-passthrough endpoint (`tailscale serve --tcp=5222 ...`, *not* `--tls-terminated-tcp`) just works without DNS gymnastics.
- Currently accepts the server's TLS cert without verification. The architectural reason is documented in the header comment of `packages/companion-core/src/channels/xmpp/connector.rs`; the short version is that `tokio-xmpp` 5's shipped connector hardcodes its rustls config with no override hook, and our chat infra presents a self-signed cert behind Tailscale. When real certs land (e.g. ACME-issued or Tailscale-issued certs for chat), a `tlsVerify` option will land alongside the verified branch in the same commit.
- `mentionOnly = true` is the **default** for XMPP (inverted from Telegram's default of `false`), because high-volume household MUCs would otherwise burn tokens on every message that drifts past.
- MUC support is wired in the module options (`mucRooms`) but the actual join is deferred to a later phase. The daemon currently logs `MUC join deferred to Phase 5` and skips. When that phase ships, configured rooms activate automatically on the next rebuild — no further config needed.
- Bare-JID conversation routing means a user gets the same session whether they message you from Conversations on their phone, Gajim on a desktop, or Dino on a different desktop. Resource roaming is handled correctly.

**Discord-specific notes:**

- **Requires the Message Content privileged intent.** Enable it in the Discord Developer Portal under your bot's settings. Without it, guild messages arrive with empty content and the adapter silently drops them. DMs always have content regardless of this intent.
- **Guild messages are always Anonymous trust.** Room membership is not identity — same reasoning as XMPP MUC. DMs from `allowedUserIds` get Owner trust; everyone else in DMs gets Anonymous.
- **Mention stripping is automatic.** When someone @mentions the bot, the `<@BOT_ID>` tag is stripped from the message before dispatch so the model doesn't see its own ping as the first token. A bare ping (just the mention, nothing else) is dropped.
- **Bot messages are always dropped.** The adapter ignores all messages with the `bot` flag set, including its own. This is the loop prevention — no self-response, no cross-bot chattering.
- **Thread messages get their own session.** Discord threads use the thread's channel ID as `conversation_id`, so a thread runs as an independent session parallel to its parent channel.
- **`mentionOnly = true` is the default** (same as XMPP, inverted from Telegram), because guild channels would otherwise burn tokens on every message.
- **2000-character message limit.** Discord's API cap is 2000 chars. In single_message mode, responses that exceed the limit during streaming are deleted and resent as multiple messages. In multi_message mode, responses are split at 2000-char boundaries before sending.

**Email-specific notes:**

- **The email channel is the companion's OWN inbox**, not a path to read another mailbox on your behalf. Mail addressed to `address` lands in the dispatcher as a turn, the reply goes back out via SMTPS from the same address. If you want the companion to query your personal inboxes during a tool-using turn, expose that through an MCP tool server (e.g. via mcp-gateway), not through this channel adapter — they have different trust boundaries and different failure domains.
- **Session identity is the RFC 5322 thread root**, not the sender. The adapter resolves `conversation_id` from `References[0]` → `In-Reply-To` → the message's own `Message-ID`, so every distinct thread is its own Claude session. Reply to an old thread and you continue that conversation; start a new thread and you get a fresh one. Same threading model your mail client already uses.
- **Allowlist gates trust level, not delivery.** `allowedSenders` addresses get `TrustLevel::Owner` (curated MCP tool allowlist via Claude Code's `bypassPermissions`). Anyone else still gets a reply — at `TrustLevel::Anonymous`, same tool-free conversational path the XMPP MUC uses. An empty `allowedSenders` list just means everyone is anonymous; it does not deny delivery. If you want hard delivery rejection, do it at the mail server's level (Postfix filter, milter) before the message ever reaches the adapter.
- **Loop prevention is automatic.** Inbound messages with `Auto-Submitted:` set to anything but `no` are dropped at debug level. Same for `Precedence: bulk/list`, bounces, `mailer-daemon`, `postmaster`, and `no-reply`/`noreply` senders. Outbound replies carry `Auto-Submitted: auto-replied` so well-behaved auto-responders on the other side know not to reply to the bot either. This is how you avoid turning one out-of-office into an infinite regress between two tarpit'd mailservers.
- **Quote stripping is defensive.** Lines starting with `>` are dropped before dispatch. "On X, Y wrote:" and `-----Original Message-----` separators truncate everything after them. A 500-line quoted reply chain becomes a three-line turn request.
- **Polling, not IDLE.** The adapter runs a 30-second IMAP poll loop (configurable via `pollIntervalSecs`, floored at 5). IMAP IDLE is a follow-up — poll is simpler, there's no persistent-connection edge case to manage, and for a low-traffic channel the 15-second average latency is fine. If email volume justifies IDLE later, it's a localized refactor in `fetch.rs`.
- **Outbound replies are filed in the IMAP Sent folder via APPEND.** The adapter tries `Sent`, then `INBOX.Sent`, then `Sent Items` — the three folder names that cover essentially every IMAP server people actually run. Failing to file is logged as a warning; the reply still went out over SMTP.
- **TLS is strict.** Unlike the XMPP adapter's no-verify path (which exists for self-signed Prosody behind Tailscale), the email adapter always uses Mozilla's CA bundle via `webpki-roots` and enforces standard certificate verification. Public mail servers have real certs; if yours doesn't, fix that before enabling this channel.

### MCP tool integration

The wrapper auto-detects an mcp-gateway configuration at runtime and passes it to Claude Code as `--mcp-config=<path>`, making every tool your gateway exposes available in the session. Detection is purely file-existence-based; the wrapper checks these paths in order and uses the first one it finds:

1. `$XDG_CONFIG_HOME/mcp-gateway/claude_config.json`
2. `$XDG_CONFIG_HOME/mcp/mcp_servers.json`
3. `$HOME/.mcp.json`

If none exist, the wrapper invokes Claude Code without `--mcp-config` and produces no warning — MCP tools are optional, not required. If you keep your mcp-gateway config somewhere else, set `services.cairn-companion.mcpConfigFile` to the absolute path and the wrapper will use it exclusively, bypassing auto-detection.

## Authoring a persona

cairn-companion is intentionally **Nix-declarative on the outside and rich-markdown on the inside**. You declare *which* files make up your persona in your home-manager config; the files themselves are plain markdown that you author and edit by hand. Nix is poor at holding paragraphs of character voice as attribute sets; markdown is poor at being reproducibly wired into a system configuration. This split lets each do what it's good at.

### The recommended five-file layout

Nothing in the module enforces a specific file structure — the wrapper concatenates whatever files you list. But after porting a production persona to this module we converged on a five-file layout that separates concerns cleanly and keeps each file short enough to reason about. Start here:

```
personas/<name>/
├── USER.md       # Facts about the user and household — names, roles,
│                 # machines, safeguards. No voice guidance.
├── voice.md      # How the agent talks — tone rules, signature phrases,
│                 # "what you never do," the aesthetic target.
├── beliefs.md    # The agent's worldview — cultural era, tech opinions,
│                 # the interior lens. Not rules; texture.
├── family.md     # Per-person interaction guide — one section per
│                 # person the agent interacts with. Skip this file
│                 # entirely if only one person uses the agent.
└── context.md    # Operating rules the user has taught the agent —
                  # technical preferences, writing-with-personality
                  # rules, discipline around specific failure modes.
```

### What goes in each file

**`USER.md`** — strictly factual. Who the user is, who else is in the household (if multiple people interact with the agent), what machines exist, what to check before destructive actions. No voice guidance lives here. When the agent needs to answer *"who am I talking to?"*, this is where it reads.

**`voice.md`** — the load-bearing persona file. Tone register, how sentences are shaped, what the agent never says, the one-line aesthetic target. A single concrete cultural reference does more work than paragraphs of tone adjectives — *"Randal from Clerks, not burnt-out help desk guy"* is clearer than fifteen bullets about sarcasm and brevity. This is the file that makes the agent sound like a specific character rather than a generic assistant.

**`beliefs.md`** — worldview backdrop. What the agent remembers culturally, what the agent finds ridiculous, what the agent believes about the present. Written as context, not instructions — *"it's the water you swim in, not a costume."* The agent does not recite this file; it thinks through it.

**`family.md`** — per-person handling for users beyond the primary. Each person gets a section with their quirks, running bits, and the specific tone adjustments that make them feel known rather than flattened into a generic "other user." Skip this file entirely if only one person uses your agent.

**`context.md`** — the lessons-learned file. Things the user has explicitly taught the agent through experience: technical preferences, writing style for code artifacts, and discipline around specific failure modes ("don't retry crashed tools," "don't fake broken," "check before guessing"). Specific failure modes make better instructions than abstract principles — a rule that starts *"when you claimed email was dead and the logs showed continuous activity…"* is more actionable than "be thorough."

### Wiring it into your config

Drop the files wherever you keep your home-manager config — typically `~/.config/your-nix-config/personas/<name>/` — then reference them from `services.cairn-companion.persona`:

```nix
services.cairn-companion = {
  enable = true;
  persona = {
    userFile   = ../personas/sid/USER.md;
    extraFiles = [
      ../personas/sid/voice.md
      ../personas/sid/beliefs.md
      ../personas/sid/family.md
      ../personas/sid/context.md
    ];
  };
};
```

Paths are relative to the `.nix` file doing the referencing, and Nix copies them into the store at build time — so each rebuild pins the exact persona content, and edits to the markdown files only take effect after `nixos-rebuild switch` (or `home-manager switch`). That's the declarative trade-off.

> **Gotcha — commit new persona files before rebuilding.** Nix flakes determine what's in the source using git, and **uncommitted files may not be included** in the store copy even when staged. The first time you create a new `personas/` directory, `git add` and `git commit` the files in your config repo before rebuilding, or nix will copy your modified `.nix` files into the store and leave the fresh markdown files behind. When this happens, `companion` launches with warnings like *"persona file not found: /nix/store/XXX-source/personas/sid/USER.md"* and the agent speaks in a generic voice because only the shipped character-free base loaded.

### Order matters

The wrapper assembles the agent's system prompt in this specific order:

1. The shipped character-free base `AGENT.md` — format rules only (*"be concise," "cite files with path:line," "lead with the answer not the preamble"*). Always loaded. You cannot opt out of this without replacing `persona.basePackage` entirely.
2. `persona.userFile` — your `USER.md`, replacing the default template.
3. Each file in `persona.extraFiles`, in list order.

Files earlier in the composition set context that later files build on. We recommend putting **`voice.md` first in `extraFiles`** because it locks the speech register immediately after the factual user context, so every subsequent file is read "in character" by the model. `beliefs.md`, `family.md`, and `context.md` can then extend or specialize without needing to re-establish tone.

### Multiple personas

If you want different personas for different machines, different roles, or different moods, put each in its own subdirectory (`personas/sid/`, `personas/work/`, `personas/public/`) and wire each host's `hosts/<hostname>.nix` to reference the set it wants. Nothing in the module prevents this — the files are just paths, and each host can point at whichever set is active there.

### Keeping the default

If you enable `services.cairn-companion` without setting `persona.userFile` or `persona.extraFiles`, you get a fully working agent with only the character-free default: response-format rules, citation conventions, and a placeholder `USER.md` template scaffolded into your workspace on first run for you to fill in. That's a perfectly valid deployment — the five-file layout is a recommended starting point for users who want a distinct character voice, not a requirement.

## Repository layout

```
cairn-companion/
├── flake.nix                   # Nix flake exposing the home-manager module
├── ROADMAP.md                  # Tiered build order with links to proposals
├── openspec/
│   ├── config.yaml             # Context, non-goals, and architectural rules
│   └── changes/
│       ├── archive/            # Completed proposals (bootstrap, daemon-core,
│       │                       # openai-gateway, channel-xmpp, ...)
│       ├── cli-client/         # Tier 1 CLI subcommands
│       ├── tui-dashboard/      # Tier 1 terminal dashboard
│       ├── channel-telegram/   # Tier 1 first channel adapter
│       ├── channel-email/      # Tier 1 email adapter
│       ├── channel-discord/    # Tier 1 Discord adapter
│       ├── spoke-tools/        # Tier 2 machine-local MCP tool servers
│       ├── distributed-routing/# Tier 2 hub/spoke multi-machine routing
│       ├── gui-gtk4/           # Optional GUI
│       └── cairn-integration/  # Thin consumer-side proposal (lives in cairn)
```

Each change is a self-contained proposal with `proposal.md`, `specs/` describing behavior, and `tasks.md` with an implementation checklist. Shipped changes (`bootstrap`, `daemon-core`, `openai-gateway`, `cli-client`, `tui-dashboard`, `channel-telegram`, `channel-xmpp`, `channel-email`) are either fully drafted or archived. The remaining proposals are skeletons that get fleshed out when picked up.

## Development workflow

This project follows spec-driven development via [OpenSpec](https://github.com/openspec-dev/openspec):

1. Changes start as proposals in `openspec/changes/<name>/proposal.md`
2. Behavior is specified in `openspec/changes/<name>/specs/`
3. Implementation steps are tracked in `openspec/changes/<name>/tasks.md`
4. Only after proposal + specs + tasks are reviewed is any code written
5. Completed changes are archived to `openspec/changes/archive/`

To work on this project:

```bash
nix develop                     # enter devshell with nixfmt, git, gh
cat ROADMAP.md                  # see what's next
cd openspec/changes/bootstrap   # start with the bootstrap proposal
```

## Related projects

- **[cairn](https://github.com/kcalvelli/cairn)** — The NixOS-based distribution that cairn-companion is primarily designed to integrate with
- **[mcp-gateway](https://github.com/kcalvelli/mcp-gateway)** — MCP server aggregator; serves as the spoke daemon in Tier 2 distributed mode
- **[Claude Code](https://docs.claude.com/en/docs/claude-code)** — The underlying agent runtime this project wraps

## License

MIT — see [LICENSE](./LICENSE).
