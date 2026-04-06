# Proposal: OpenAI Gateway — OpenAI-Compatible Chat Completions Endpoint

> **Status**: Complete — shipped 2026-04-06. Gateway runs inside companion-core, 32/32 tests passing, Nix build clean.

## Tier

Tier 1 (and **required for ZeroClaw decommissioning** — see Motivation)

## Summary

Expose an OpenAI-compatible `POST /v1/chat/completions` endpoint from the companion daemon so that non-interactive API consumers — most importantly Home Assistant's Conversation integration — can use the companion as their LLM backend. The endpoint accepts OpenAI-format requests (messages array, streaming flag, model hint), forwards them to a managed `claude -p` invocation with persona and session context applied, and returns OpenAI-format responses (streaming via SSE or one-shot JSON). This restores parity with the ZeroClaw gateway feature at `gateway.http.endpoints.chatCompletions.enabled` on port 18789 that is currently load-bearing for voice interaction via HA.

## Motivation

### Current state in ZeroClaw

Keith's current Sid deployment on mini exposes an OpenAI-compatible chat completions endpoint on port 18789 (configured via `services.zeroclaw.settings.gateway.http.endpoints.chatCompletions.enabled = true`). This endpoint is the integration point for **Home Assistant's Conversation integration**, which is the primary voice interface to Sid: satellites in rooms transcribe speech via Assist, forward the text to Sid's chat completions endpoint, receive a response, and speak it back through the satellite's TTS pipeline. Without this endpoint, voice interaction with Sid across the house stops working entirely.

### Why this was not in the initial roadmap

The initial axios-companion roadmap was drafted under the assumption that all inbound channels were interactive chat-style (Telegram, Discord, email, XMPP). The HA voice integration was overlooked because it's a *pull* interface exposed via HTTP, not a *push* interface that receives messages from a bot platform. When the initial ZeroClaw replacement audit was done, the gateway's chat completions endpoint was flagged as "verify whether anything uses it" — a subsequent check confirmed HA Conversation is a heavy consumer.

This proposal closes that gap. It is **required** before ZeroClaw can be decommissioned on mini, because disabling ZeroClaw without a replacement for this endpoint would break voice interaction across every room in the house.

### Why this is a separate proposal from channel-* adapters

Channel adapters (`channel-telegram`, `channel-discord`, `channel-email`, `channel-xmpp`) are push-interactive: a user sends a message through their preferred bot platform, the companion responds. The API model is "platform-specific bot receiving events, daemon-held session per conversation."

The OpenAI gateway is fundamentally different: it's a **pull API** that speaks a standardized protocol (OpenAI chat completions, as defined by OpenAI's public API and implemented by dozens of local LLM proxies — llama.cpp server, Ollama, vLLM, etc.). Any tool that can point at an OpenAI-compatible endpoint can use the companion as its backend — Home Assistant Conversation is today's consumer, but the same endpoint will work for future integrations without any additional work.

Keeping this separate from channel adapters makes the distinction clear and avoids overloading the `channel-*` namespace with something that isn't really a channel.

### Why implement in axios-companion, not in mcp-gateway

A tempting alternative would be to add an OpenAI chat completions endpoint to mcp-gateway (which is also Python/FastAPI and already Tailscale-aware). Rejected because:

1. **mcp-gateway's job is MCP server aggregation.** Adding an unrelated OpenAI proxy to it violates its single-purpose design.
2. **The companion daemon already manages claude-code subprocesses, session state, and persona resolution.** mcp-gateway would have to reimplement or duplicate all of that.
3. **Session continuity matters for voice.** When you ask Sid a follow-up question by voice, the second request should see the first's context. The daemon has session state; mcp-gateway does not.
4. **Persona loading is daemon-local.** Running this from mcp-gateway would require reaching into the user's persona configuration from a different process.

The gateway lives in the companion daemon, reuses the dispatcher and session store, and presents a stable HTTP surface on a configurable port.

## Scope

### In scope

- New component inside `companion-core`: an HTTP server (axum) exposing:
  - `POST /v1/chat/completions` — OpenAI chat completions API
  - `GET /v1/models` — returns a single model entry describing "companion" as the available model (HA and other clients often probe this)
  - `GET /health` — liveness endpoint for monitoring
- Home-manager options under `services.axios-companion.gateway.openai`:
  - `enable` — boolean
  - `port` — integer (default: 18789 for parity with current ZeroClaw config)
  - `bindAddress` — string (default: Tailscale interface via hostname, or `127.0.0.1` if not on a tailnet)
  - `modelName` — string shown in `/v1/models` responses (default: `"companion"`)
  - `defaultSystemPrompt` — optional override to append to the gateway-routed persona
  - `sessionPolicy` — one of `per-conversation-id` (honor any client-supplied conversation identifier), `single-session` (all gateway traffic shares one session), `ephemeral` (every request is a fresh `claude -p` with no resumption). Default: `per-conversation-id` with a fallback ID if the client doesn't supply one
- Request handling:
  - Accept OpenAI-format `messages` array, extract the final user message and route through the dispatcher
  - Honor `stream: true` with SSE-chunked OpenAI-format responses (`data: {...}\n\n` per chunk, `[DONE]` terminator)
  - Honor `stream: false` with a single JSON response
  - Map claude stream-json events to OpenAI chunk format
  - Return sensible error responses in OpenAI error envelope format
- Session mapping: gateway requests create and resume sessions via the same dispatcher/session-store that channel adapters use
- Persona application: gateway-routed requests use the configured persona, same as any other dispatcher path
- No application-level auth — Tailscale network trust is the sole boundary, matching the architectural rule for Tier 1+ networking

### Out of scope

- Token-level authentication or API keys (architectural rule: no application-level auth)
- Function calling / tool use via the OpenAI function-call API (claude-code handles its own tools; the gateway just streams the conversational response)
- Image or multimodal input via the OpenAI vision API (deferred — claude-code handles multimodal via workspace file references, not via OpenAI-style base64 image payloads)
- Embeddings endpoint (`/v1/embeddings`) — out of scope, companion does not provide embeddings
- Fine-tuning endpoints, assistants endpoints, threads endpoints, anything else in the OpenAI API surface that isn't `/v1/chat/completions` and `/v1/models`
- OpenAI-specific headers beyond what HA Conversation and common clients actually send (don't implement untested surface)

### Non-goals

- Being a full drop-in replacement for OpenAI's API across all features — the scope is deliberately narrow to what HA and similar consumers actually use
- Multi-tenancy within the gateway — one daemon, one user, one gateway instance per user
- Rate limiting or quotas — subscription billing makes this moot; if the user needs limits they add them at the reverse-proxy layer
- Any caching layer — each request produces a fresh claude invocation (with session resumption for context)

## Dependencies

- `bootstrap`
- `daemon-core` (the gateway runs inside the companion daemon and shares the dispatcher and session store)

This proposal is a peer of the `channel-*` proposals in Tier 1 — it does not depend on any of them. It can be implemented immediately after `daemon-core` ships.

## Success criteria

1. `services.axios-companion.gateway.openai.enable = true` in a home-manager config produces a daemon that listens on the configured port after `home-manager switch`
2. `curl http://localhost:18789/v1/models` returns a valid OpenAI-format models list with the configured model name
3. `curl -X POST http://localhost:18789/v1/chat/completions -d '{"messages":[{"role":"user","content":"hello"}],"stream":false}'` returns a valid non-streaming chat completion with the companion's response
4. Same request with `"stream": true` returns a valid SSE stream of OpenAI-format delta chunks terminated with `data: [DONE]`
5. Home Assistant's Conversation integration, configured with `http://mini.tail0efb4.ts.net:18789/v1/chat/completions` as the endpoint, can send a voice transcription and receive a spoken response through the Assist pipeline end-to-end
6. Follow-up voice requests that include conversation history in the `messages` array produce contextually coherent responses (the daemon resumes the prior session)
7. Tailscale ACLs are the sole access control — the gateway is reachable only from nodes on the user's tailnet
8. Gateway requests appear in `companion status` output and in the TUI dashboard's session list, indistinguishable from other session sources except by their surface identifier (`gateway` or `openai`)

## Priority note

**This proposal should be implemented before or concurrently with `channel-telegram`.** Rationale: `channel-telegram` is a new capability (Keith does not rely on Telegram for the home environment today in a way that would break), whereas the OpenAI gateway is restoring an existing critical capability (voice interaction with Sid across the house). Missing a Telegram capability is "a feature hasn't shipped yet"; missing the OpenAI gateway is "voice interaction is broken on every satellite in the house during the migration." The ROADMAP should reflect this priority when picking the next Tier 1 proposal after `daemon-core` lands.
