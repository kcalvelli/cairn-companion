# Proposal: Discord Channel Adapter

> **Status**: Spec complete, ready to implement.

## Tier

Tier 1

## Summary

Add a Discord channel adapter to the companion daemon using `serenity` (Rust Discord library, gateway + REST). The adapter connects to Discord's Gateway WebSocket, receives message events, dispatches them through the shared `Dispatcher`, and renders responses back to the channel. Supports DMs (with allowlist-based trust) and guild channels (with mention-gated, anonymous-trust dispatch). Streaming responses use Discord's message-edit API; long responses split at Discord's 2000-character limit.

## Motivation

Discord is where communities live. Users who run gaming servers, family servers, or project-coordination servers want their companion accessible from the same client they're already in. Discord also supports richer formatting than most chat platforms (code blocks with syntax highlighting, proper markdown) that suit technical conversations.

## Scope clarification

**Channel-discord is a single-purpose message adapter.** It listens for messages, dispatches them, and sends replies. It does not manage servers, assign roles, moderate channels, or do anything a "Discord bot framework" would do.

## Scope

### In scope

- Discord adapter running inside the daemon as an async task via serenity's gateway client
- Home-manager options under `services.axios-companion.channels.discord`:
  - `enable`
  - `botTokenFile` — agenix-compatible token file
  - `allowedUserIds` — list of Discord user snowflake IDs (u64), controls Owner trust in DMs
  - `mentionOnly` — for guild channels, only respond when @mentioned (default true)
  - `streamMode` — `single_message` (edit-in-place) or `multi_message` (collect + send)
- DM handling: allowlisted users → `TrustLevel::Owner`, everyone else → `TrustLevel::Anonymous`. Conversation key is the user's snowflake ID.
- Guild channel handling: all guild messages → `TrustLevel::Anonymous` (matching XMPP MUC precedent). Conversation key is the channel ID. `mentionOnly` gates whether non-mentioned messages are processed.
- Discord thread support: thread messages use the thread's channel ID as conversation key (one session per thread, parallel to the parent channel's session)
- Mention parsing: strip `<@BOT_ID>` from message content before dispatch so the model doesn't see its own ping as the first token of every turn
- Streaming responses via message edit (single_message mode): send initial message on first chunk, edit in place as chunks arrive, throttled to respect Discord's edit rate limit (~5/5s per message)
- Message splitting at 2000 characters via `channels::util::split_message` for multi_message mode and for single responses that exceed the limit
- Code block awareness: don't split inside a triple-backtick fence
- Bang commands: `!new`, `!status`, `!help` — same command set as xmpp/email, `!` prefix
- Loop prevention: drop all messages from the bot's own user ID
- Exponential-backoff reconnect: serenity handles gateway reconnection internally, but if the client exits unexpectedly, the outer serve loop reconnects with 1s→60s backoff
- Graceful shutdown via the standard `Notify` pattern

### Out of scope

- **Slash commands.** Discord's application command system is a richer UX but requires OAuth2 scope setup and adds registration complexity. Deferred — bang commands cover the same functionality.
- **Voice channel integration.** Belongs in the voice roadmap slot, not here.
- **Server management.** No kick, ban, role, or permission management.
- **Embeds / rich responses.** Plain text with markdown. If embed support is ever wanted, it's a follow-up.
- **Image/attachment input.** Passing images to Claude as multimodal input is interesting but adds complexity. Deferred.
- **Reaction-based interaction.** No reaction commands, polls, or emoji-driven flows.

### Non-goals

- A Discord bot framework — this is a single-purpose adapter
- Replacing Discord's UI or client functionality
- Managing multiple bot accounts — one token per adapter instance

## Dependencies

- `bootstrap`
- `daemon-core`
- `channel-telegram` (pattern reference — streaming, allowlist, main.rs wiring)
- `channel-xmpp` (pattern reference — MUC/guild handling, mention parsing, loop prevention)

## Discord-specific deployment requirements

1. **Bot application**: Create a bot in the [Discord Developer Portal](https://discord.com/developers/applications). Copy the bot token to an agenix-managed file.
2. **Privileged intents**: Enable the **Message Content** intent in the bot's settings. Without it, guild messages arrive with empty content. DMs always have content regardless.
3. **Gateway intents requested**: `GUILD_MESSAGES`, `DIRECT_MESSAGES`, `MESSAGE_CONTENT`. The adapter requests exactly these three — no presence, no member lists, no voice.
4. **Bot invite**: Invite the bot to target guilds with the `Send Messages` and `Read Message History` permissions. `Read Messages`/`View Channels` is implicit.

## Success criteria

1. User configures `botTokenFile` and `allowedUserIds` via home-manager. After rebuild, `systemctl --user status companion-core` shows `active (running)` with the Discord adapter initialized, journal shows `starting Discord adapter` and `Discord gateway ready`.
2. A DM from an allowlisted user ID gets a response. Journal shows `trust=Owner`.
3. A DM from a non-allowlisted user gets a response at `TrustLevel::Anonymous`. Journal shows `trust=Anonymous`, no tool calls in the reply.
4. An @mention in a guild channel (with `mentionOnly=true`) gets a response. The mention prefix is stripped from the dispatch body.
5. A non-mentioned message in a guild channel (with `mentionOnly=true`) is ignored.
6. Long responses split cleanly at 2000-char boundaries without breaking code blocks.
7. In single_message mode, the user sees the response being "typed" via edits.
8. `!new` clears the session, `!status` reports session info, `!help` lists commands.
9. The bot does not respond to its own messages.
10. `nix build .#companion-core` is green; unit tests pass.

## Specs

No separate `specs/` subdirectory. Same precedent as channel-xmpp and channel-email — a single-purpose adapter doesn't need per-concern specs when the proposal + tasks cover the behavior.
