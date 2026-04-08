# Tasks: XMPP Channel Adapter — Tier 1

Second channel adapter, after telegram. Stress-tests the channel pattern against a genuinely different protocol (XML streams, presence, MUC, no native message editing). Connects to the existing Prosody server on mini at `127.0.0.1:5222`, JID `sid@chat.taile0fb4.ts.net`, password already in agenix at `secrets/xmpp-bot-password.age`. Deploys mini-only.

## Phase 1: Dependencies and skeleton

- [x] **1.1** ~~Pick the Rust XMPP stack~~ **DECIDED 2026-04-08**: use `tokio-xmpp` 5.0.0 + `xmpp-parsers` 0.22.0 directly. **Skip** the high-level `xmpp` crate — it's at 0.6.0 (last released July 2024), self-describes as "very much WIP," and its `Event` enum exposes only `ChatMessage`/`RoomMessage`/`RoomJoined`-style variants, with no surface for XEP-0308 corrections or XEP-0085 chat states. We'd be hand-rolling those stanzas via `xmpp-parsers` regardless, so the wrapper provides no value while adding a stale dependency. `xmpp-parsers` confirmed to have `message_correct` (XEP-0308), `chatstates` (XEP-0085), `muc` (XEP-0045), and `message` (RFC 6120) modules.
- [ ] **1.2** Add `tokio-xmpp = "5"` and `xmpp-parsers = "0.22"` to `packages/companion-core/Cargo.toml`. Use the `starttls` and `aws_lc_rs`/`rustls` features on `tokio-xmpp` to match the existing rustls posture in the workspace. **Do not** add the `xmpp` crate.
- [ ] **1.3** Update `packages/companion-core/default.nix` if any new native build inputs are needed (likely none — xmpp-rs is pure Rust + rustls).
- [ ] **1.4** Verify `cargo check` passes with new dependencies.
- [ ] **1.5** **Reorg into `channels/` namespace** (Option A, decided):
  - Create `packages/companion-core/src/channels/mod.rs`.
  - Move `packages/companion-core/src/telegram/` → `packages/companion-core/src/channels/telegram/`.
  - Create `packages/companion-core/src/channels/util.rs` with `pub fn split_message(text: &str, max_chars: usize) -> Vec<String>` — lift the algorithm from `channels/telegram/mod.rs`, parameterize the cap.
  - Update `channels/telegram/mod.rs` to call `crate::channels::util::split_message(text, 4096)` and delete its private copy.
  - Update `main.rs` to declare `mod channels;` instead of `mod telegram;` and update `use` paths.
  - Run `cargo test -p companion-core` — every existing telegram unit test must still pass with zero edits to test logic. If a test fails, the move broke something; fix before continuing.
- [ ] **1.6** Create `packages/companion-core/src/channels/xmpp/mod.rs` skeleton with module-level doc comment matching telegram's style. Wire it into `channels/mod.rs`.
- [ ] **1.7** **Pre-XMPP regression check**: build with the channels reorg but no xmpp logic yet, deploy to a scratch shell or `cargo run` locally, verify telegram still connects and responds. The reorg lands cleanly *before* any xmpp code exists, so a regression here can only be the move itself. Do not start Phase 2 until this is green.

## Phase 2: Configuration and connection

- [ ] **2.1** Define `XmppConfig` struct in `xmpp/mod.rs`: `jid`, `password`, `server`, `port`, `tls_verify`, `allowed_jids: HashSet<BareJid>`, `muc_rooms: Vec<(BareJid, String)>` (room JID + nick), `mention_only`, `stream_mode`.
- [ ] **2.2** Implement `XmppConfig::from_env()` mirroring `TelegramConfig::from_env()`. Read from `COMPANION_XMPP_*` env vars. Return `None` when `COMPANION_XMPP_ENABLE != 1`.
- [ ] **2.3** Read password from `COMPANION_XMPP_PASSWORD_FILE` (agenix-managed file path). Empty file → error → return None.
- [ ] **2.4** Define `StreamMode` enum identical to telegram's: `SingleMessage` (uses XEP-0308 corrections) and `MultiMessage` (splits).
- [ ] **2.5** Implement `connect()` — establish XMPP client connection to `server:port`, accept self-signed cert when `tls_verify = false`, authenticate with SASL PLAIN (we're on localhost — PLAIN over TLS-less loopback is fine, but verify the `xmpp` crate's behavior here and document it).
- [ ] **2.6** Send initial presence (online) on successful connection.
- [ ] **2.7** Implement reconnect-with-backoff loop for transient failures. The Prosody server restarts during nixos-rebuild — the bot should survive that without a daemon restart.

## Phase 3: Direct message handling

- [ ] **3.1** Implement message stanza handler: receive `<message type="chat">`, extract sender bare JID + body.
- [ ] **3.2** Allowlist filter: drop messages from JIDs not in `allowed_jids`. Empty allowlist = nobody (deny by default, matches telegram).
- [ ] **3.3** Map sender bare JID → session ID via the session store. Reuse the same `delete_session()` path telegram uses for `/new`.
- [ ] **3.4** Build a `TurnRequest` and dispatch through the shared `Arc<Dispatcher>`.
- [ ] **3.5** Send the assembled response back as a `<message type="chat">` stanza to the sender's bare JID.
- [ ] **3.6** Unit test: allowlist enforcement (allowed JID passes, unknown JID drops, empty allowlist drops everyone).

## Phase 4: Streaming, corrections, and chat states

- [ ] **4.1** XMPP message splitting: call `crate::channels::util::split_message(text, 3000)` (3000-char cap is the empirical comfortable size for Conversations/Gajim/Dino — XMPP has no protocol limit, but clients get unhappy past a few thousand chars). Make the cap a constant in `channels/xmpp/mod.rs`, not a magic number.
- [ ] **4.2** *(folded into 1.5 — `split_message` already lives in `channels/util.rs` by this phase)*
- [ ] **4.3** Implement `MultiMessage` stream mode: collect dispatcher events into a buffer, on `Complete` split and send N stanzas in order.
- [ ] **4.4** Implement `SingleMessage` stream mode using XEP-0308: send the first chunk as a normal message, then on each subsequent chunk send a correction stanza referencing the previous message's `id` via `<replace xmlns="urn:xmpp:message-correct:0" id="..."/>`. Throttle corrections to ~1.5s like telegram.
- [ ] **4.5** Verify XEP-0308 behavior in Conversations (Android), Gajim (Linux), and Dino (Linux) — all three are clients in the household. If any of them ignore corrections, document the fallback expectation in the spec.
- [ ] **4.6** Implement XEP-0085 Chat States: send `<composing/>` when dispatch starts, `<active/>` when dispatch completes. This is the typing-indicator equivalent. Do NOT send `<paused/>` or `<inactive/>` — overkill for a bot.
- [ ] **4.7** Unit tests for `split_message()` (paragraph/line/sentence/word/hard-cut paths).

## Phase 5: MUC support

- [ ] **5.1** Implement MUC auto-join on connection: for each `(room_jid, nick)` in config, send a presence stanza to `room_jid/nick` to join.
- [ ] **5.2** Handle MUC message stanzas (`<message type="groupchat">`). Extract room JID, sender nick, body.
- [ ] **5.3** **Loop prevention**: drop any groupchat message whose sender nick equals our own nick in that room. The ZeroClaw incident (`# Disabled: MUC loop issue with zeroclaw` comment in mini.nix) was almost certainly this — verify by testing once integration is up.
- [ ] **5.4** **Mention parsing**: in `mention_only` mode, only respond when the body starts with our nick followed by `:`, `,`, or whitespace, OR contains an `@nick` reference. Strip the mention from the body before dispatching.
- [ ] **5.5** Map room JID → session ID (separate session per room, not per user-in-room — the bot has one conversation with the room as a whole).
- [ ] **5.6** Send responses as groupchat stanzas to the room JID. SingleMessage corrections work in MUC the same way as DMs.
- [ ] **5.7** Allowlist behavior in MUC: trust everyone in a room the bot has been told to join. Room membership is the access control boundary, not per-JID allowlists. Document this decision.
- [ ] **5.8** Unit test: own-nick loop prevention. Unit test: mention parsing edge cases.

## Phase 6: Slash commands

- [ ] **6.1** Implement the same command set as telegram: `/new` (delete and recreate session), `/status` (show session info), `/help` (list commands).
- [ ] **6.2** Catch unrecognized `/commands` in the adapter — do NOT forward them to the dispatcher. Same reason as telegram (prevents Claude Code skill leakage).
- [ ] **6.3** In MUC, slash commands only fire when the bot is being addressed (same `mention_only` rules). A `/new` floating in the room without addressing the bot does nothing.
- [ ] **6.4** Unit test: command parsing, unknown command rejection.

## Phase 7: Wiring

- [ ] **7.1** Add the xmpp adapter as step 5c in `packages/companion-core/src/main.rs`, env-gated, shared `Arc<Dispatcher>`, shutdown via the existing `Notify`.
- [ ] **7.2** Add `services.axios-companion.channels.xmpp` options to `modules/home-manager/default.nix`: `enable`, `jid`, `passwordFile`, `server` (default `127.0.0.1`), `port` (default `5222`), `tlsVerify` (default `false`), `allowedJids`, `mucRooms` (list of `{ jid, nick }`), `mentionOnly` (default `true`), `streamMode` (default `single_message`).
- [ ] **7.3** Add an assertion: `channels.xmpp.enable -> daemon.enable`.
- [ ] **7.4** Wire the env vars into the systemd unit, mirroring telegram's block.
- [ ] **7.5** Enable on mini in `~/.config/nixos_config/hosts/mini.nix` via the existing `home-manager.users.keith` host override. Reuse `secrets/xmpp-bot-password.age`. Configure `xojabo@muc.chat.taile0fb4.ts.net` as a MUC room with nick `Sid`.
- [ ] **7.6** Verify `nix flake check` passes for both the companion repo and `~/.config/nixos_config`.

## Phase 8: Live test, docs, archive

- [ ] **8.1** Deploy to mini: `sudo nixos-rebuild switch --flake .#mini`.
- [ ] **8.2** Live DM test from Conversations on Keith's phone to `sid@chat.taile0fb4.ts.net`. Verify: response arrives, streaming works (single-message corrections render correctly in Conversations), `/new` resets session, `/status` reports correctly.
- [ ] **8.3** Live MUC test in `xojabo@muc.chat.taile0fb4.ts.net`. Verify: bot is present in room, ignores ambient chatter, responds when addressed by `Sid:` or `@Sid`, does not loop on its own messages. **Built-in test fixture**: John types "xojabo" in `xojabo` constantly because he likes the way it sounds. The bot must NOT respond to a bare "xojabo" message — that's the canonical false-positive case for `mention_only`. If Sid responds to John's xojabo spam even once, the mention parser is broken.
- [ ] **8.4** Watch `journalctl --user -u axios-companion -f` during the test for warnings/errors. Address anything noisy.
- [ ] **8.5** Update `README.md` with an XMPP setup section (briefly — link to channel-telegram's section as the model since the patterns rhyme).
- [ ] **8.6** Update `ROADMAP.md` to mark `channel-xmpp` complete.
- [ ] **8.7** Write a session handoff memory note matching the channel-telegram precedent (`project_session_handoff_<date>_xmpp.md`).
- [ ] **8.8** Archive: `mv openspec/changes/channel-xmpp openspec/changes/archive/channel-xmpp`.
- [ ] **8.9** Commit the archive move.

## Decisions deferred to implementation

- **OMEMO**: out of scope. Self-hosted Prosody on Tailscale, federation off — the trust model doesn't need it. Revisit only if a household member asks.
- **File transfer (XEP-0363)**: out of scope for v1. The Prosody server already has it enabled for human users; the bot can ignore it. Revisit when voice/image input lands.
- **Carbons (XEP-0280)**: not relevant for a bot — the bot has only one resource, it's not syncing across devices.
- **Smacks (XEP-0198)**: nice to have for connection resilience, but the high-level `xmpp` crate may handle it transparently. Decide during 1.1.
