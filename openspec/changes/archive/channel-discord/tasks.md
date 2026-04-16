# Tasks: Discord Channel Adapter — Tier 1

Fourth channel adapter, after telegram, xmpp, and email. Follows the established pattern: exponential-backoff reconnect loop, per-message handler, bang commands, dispatcher `TurnRequest` with explicit `TrustLevel`. Discord-specific additions: gateway WebSocket via serenity, mention parsing for guild channels, message-edit streaming, 2000-char split limit.

The adapter connects to Discord's Gateway using serenity's client, which handles heartbeating, reconnection, and session resumption internally. The outer serve loop handles the case where the client itself exits (library panic, token invalidation, etc.) with exponential backoff.

## Phase 1: Dependencies and skeleton

- [x] **1.1** Pick the Rust Discord stack. **DECIDED 2026-04-14**: `serenity` 0.12.5 with `client`, `gateway`, `model`, `builder`, `cache`, `rustls_backend` features. `builder` is required by `client` at compile time (serenity doesn't declare the dependency in its own feature graph).
- [x] **1.2** Add `serenity` to `packages/companion-core/Cargo.toml` with `default-features = false` and the required feature set.
- [x] **1.3** Create `src/channels/discord/` directory with three files: `mod.rs`, `config.rs`, `command.rs`.
- [x] **1.4** Register the module in `channels/mod.rs` as `pub mod discord;`.
- [x] **1.5** `cargo check` green, `cargo test` 142/142 passing, `nix build .#companion-core` green, `nix flake check` green.

## Phase 2: Configuration

- [x] **2.1** `DiscordConfig` struct in `config.rs` with fields: `bot_token: String`, `allowed_user_ids: HashSet<u64>`, `mention_only: bool`, `stream_mode: StreamMode`.
- [x] **2.2** `DiscordConfig::from_env()` reads `COMPANION_DISCORD_ENABLE`, `_BOT_TOKEN_FILE`, `_ALLOWED_USER_IDS`, `_MENTION_ONLY`, `_STREAM_MODE`. Returns `None` on missing required fields with error-level logs. Token is read from file (agenix pattern), trimmed.
- [x] **2.3** `is_allowed(&self, user_id: u64) -> bool` checks the allowlist. Empty allowlist = nobody is Owner.
- [x] **2.4** Unit tests for `is_allowed` (populated list, empty list).

## Phase 3: Event handler and message routing

- [x] **3.1** `Handler` struct implementing serenity's `EventHandler` trait. Holds `Arc<Dispatcher>` and `Arc<DiscordConfig>`. Bot user ID comes from the cache (populated on `ready`).
- [x] **3.2** `ready` event handler: logs bot username, bot ID, and guild count.
- [x] **3.3** `message` event handler: drops all bot-authored messages (`msg.author.bot`), branches on `msg.guild_id.is_none()` for DM vs guild routing.
- [x] **3.4** `handle_dm`: trust from `config.is_allowed(author_id)`, `conversation_id` = author ID string, bang command check, dispatch.
- [x] **3.5** `handle_guild_message`: mention check via `msg.mentions` against cached bot ID, mention stripping, always Anonymous trust, `conversation_id` = channel ID string.
- [x] **3.6** Mention stripping: string replace of `<@{bot_id}>` and `<@!{bot_id}>`, trim, drop if empty. Unit test covers both mention formats.

## Phase 4: Bang commands

- [x] **4.1** `command::handle(surface_id, conversation_id, text, dispatcher) -> String` — same shape as email's command handler.
- [x] **4.2** `!new` deletes the session. Reply varies by whether there was one.
- [x] **4.3** `!status` uses `crate::channels::util::format_timestamp`.
- [x] **4.4** `!help` is a static string.
- [x] **4.5** Unrecognized `!`-prefixed messages get a deflection reply.

## Phase 5: Response rendering

- [x] **5.1** `dispatch_and_respond` dispatches to `stream_single_message` or `collect_and_send` based on config.
- [x] **5.2** `stream_single_message`: send on first chunk, edit in place throttled at 1.5s, final edit on `Complete`, error edit on `Error`. Uses `truncate_for_discord` to keep in-progress display under 2000 chars.
- [x] **5.3** If final response exceeds 2000 chars in single_message mode, streaming message is deleted and response is resent as multiple messages.
- [x] **5.4** `collect_and_send`: drain all events, split at 2000 chars, send each chunk.
- [x] **5.5** Message splitting uses `channels::util::split_message(text, 2000)`. Unit test verifies 2000-char cap.
- [ ] **5.6** Code block fence awareness deferred — the util splitter's paragraph-boundary preference naturally avoids mid-block splits in practice.

## Phase 6: Serve loop and shutdown

- [x] **6.1** `serve(dispatcher, config, shutdown)` builds serenity `Client` with the three intents and calls `client.start()`.
- [x] **6.2** Shutdown: spawned task waits on `shutdown.notified()`, calls `shard_manager.shutdown_all()`.
- [x] **6.3** Outer reconnect loop with exponential backoff 1s→60s, shutdown-aware sleep during backoff.
- [x] **6.4** Invalid token retries forever at 60s cap with error-level logs.

## Phase 7: Wiring (main.rs + home-manager)

- [x] **7.1** Added step 6e to `main.rs` with env-gated bootstrap, `discord_shutdown` Notify, info log.
- [x] **7.2** Discord adapter in graceful shutdown sequence.
- [x] **7.3** `channels.discord` options in `default.nix`: `enable`, `botTokenFile`, `allowedUserIds`, `mentionOnly` (default true), `streamMode`.
- [x] **7.4** Assertion: `channels.discord.enable -> daemon.enable`.
- [x] **7.5** `COMPANION_DISCORD_*` env vars marshalled in systemd unit.
- [x] **7.6** `cargo test` 142/142, `nix flake check` green, `nix build .#companion-core` green.

## Phase 8: Deploy, live test, docs, archive

- [x] **8.1** README channel adapters section updated with Discord config block, shared rules updated for four adapters, Discord-specific notes section added.
- [x] **8.2** Operator-side: Discord bot application reused from ZeroClaw, Message Content intent already enabled, token in agenix (`discord-bot-token.age`), `services.cairn-companion.channels.discord` wired in `hosts/mini.nix` with `allowedUserIds = [1005082256878092339]` (Keith).
- [x] **8.3** Live test, DM from allowlisted user: verified 2026-04-16 08:50:50 — journal shows `discord DM` then `turn complete` 3s later.
- [ ] **8.4** Live test, DM from non-allowlisted user: **deferred** — requires a second Discord account not on the allowlist. Dropped from v1 shipping criteria; trust branching is unit-tested and the Anonymous code path is exercised by guild messages (see 8.5).
- [x] **8.5** Live test, guild @mention: verified 2026-04-16 09:01:13 — journal shows `discord guild message` then `turn complete` 5s later, reply dispatched at `TrustLevel::Anonymous`.
- [x] **8.6** Live test, guild non-mention (mentionOnly=true): verified 2026-04-16 — no journal log emitted because `mention_only && !mentioned` returns at `mod.rs:118` before the log line.
- [x] **8.7** Live test, streaming: verified 2026-04-16 08:51:36 — 78-second streamed response via edit-in-place (combined with 8.8).
- [x] **8.8** Live test, long response: verified 2026-04-16 — Commodore 64 essay request produced clean 2000-char splits.
- [x] **8.9** Live test, bang command: verified 2026-04-16 08:51:00 — `!status` DM produced `discord DM` log but no `turn complete`, confirming bang-command early return at `mod.rs:100-102` without dispatch.
- [x] **8.10** Live test, bot loop prevention: implicitly verified — `msg.author.bot` check at `mod.rs:61` is the first statement in `message()`, drops self/other-bot messages before logging or dispatch. 2 hours of uptime with only 4 human-paced events confirms no self-trigger loop.
- [x] **8.11** ROADMAP.md flipped: `channel-discord` marked `[x]` with shipped-on date 2026-04-16.
- [x] **8.12** Archive: `mv openspec/changes/channel-discord openspec/changes/archive/channel-discord`.

## Decisions deferred to follow-up

- **Slash commands.** Discord's application command system (vs `!` bang commands) is a better UX for discovery but adds OAuth2 scope requirements and a registration step. Worth doing eventually, not blocking v1.
- **Image/attachment input.** Passing images to Claude as multimodal input is technically possible and Discord makes it easy to attach files. Deferred because the dispatcher's `TurnRequest` currently only carries text.
- **Embeds.** Rich embed responses (with fields, colors, footers) would look nicer than plain text for structured output like `!status`. Cosmetic, not load-bearing.
- **Reaction commands.** React-to-trigger patterns (e.g., react with :repeat: to regenerate). Novel interaction pattern, not a v1 concern.
- **Per-guild trust configuration.** Currently all guild messages are Anonymous. A `trustedGuilds` config that upgrades specific guilds to Owner would be the Discord equivalent of XMPP's `allowedJids` for DMs, but applied at the guild scope. Not needed until someone actually wants to give a guild Owner-level tool access, which is a risky default anyway.
