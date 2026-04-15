# Tasks: OpenAI Gateway — Tier 1 HTTP Surface

## Phase 1: Specs, tasks, and dependencies

- [x] **1.1** Write HTTP routes spec (`specs/http-routes/spec.md`)
- [x] **1.2** Write OpenAI format spec (`specs/openai-format/spec.md`)
- [x] **1.3** Write session policy spec (`specs/session-policy/spec.md`)
- [x] **1.4** Write this tasks file
- [x] **1.5** Add `axum`, `uuid`, `tokio-stream` dependencies to `Cargo.toml`
- [x] **1.6** Update `default.nix` if new native build inputs are needed — no new native inputs, axum is pure Rust
- [x] **1.7** Verify `cargo check` passes with new dependencies

## Phase 2: OpenAI types and format mapping

- [x] **2.1** Create `src/gateway/types.rs` with request types: `ChatCompletionRequest`, `Message`
- [x] **2.2** Create response types: `ChatCompletion`, `ChatCompletionChoice`, `ChatCompletionMessage`, `Usage`
- [x] **2.3** Create streaming types: `ChatCompletionChunk`, `ChunkChoice`, `ChunkDelta`
- [x] **2.4** Create error type: `OpenAIError`, `OpenAIErrorEnvelope`
- [x] **2.5** Implement `GatewayConfig` (parsed from env vars)
- [x] **2.6** Unit tests for serialization round-trips (8 tests)

## Phase 3: HTTP server core

- [x] **3.1** Create `src/gateway/mod.rs` with axum router and shared state
- [x] **3.2** Implement `GET /health` handler
- [x] **3.3** Implement `GET /v1/models` handler
- [x] **3.4** Implement `POST /v1/chat/completions` non-streaming path: extract user message, dispatch, collect Complete, return ChatCompletion
- [x] **3.5** Implement `POST /v1/chat/completions` streaming path: dispatch, map TextChunk events to SSE ChatCompletionChunk, terminate with [DONE]
- [x] **3.6** Implement session policy: conversation ID resolution from header + policy config
- [x] **3.7** Implement error handling: validation errors → 400, dispatcher errors → 500, unknown routes → 404
- [x] **3.8** Wire gateway startup into `main.rs` — conditional on `COMPANION_GATEWAY_ENABLE=1`, shares `Arc<Dispatcher>`, runs alongside D-Bus via tokio::Notify shutdown signal
- [x] **3.9** Implement graceful shutdown: gateway server uses a shutdown signal tied to the daemon's SIGTERM/SIGINT handling
- [x] **3.10** Integration tests with mock companion (10 tests: health, models, 404, validation errors, non-streaming, streaming, error handling, conversation ID header)

## Phase 4: Home-manager module

- [x] **4.1** Add `services.cairn-companion.gateway.openai.enable` option
- [x] **4.2** Add `port`, `bindAddress`, `modelName`, `sessionPolicy` options
- [x] **4.3** Wire `Environment` entries into the systemd unit when gateway is enabled
- [x] **4.4** Verify: `nix flake check` passes, `nix build .#companion-core` succeeds

## Phase 5: Testing, docs, archive

- [x] **5.1** Test: health endpoint returns ok (integration test `health_returns_ok`)
- [x] **5.2** Test: models endpoint returns configured model (integration test `models_returns_configured_model`)
- [x] **5.3** Test: non-streaming chat completion returns valid response (integration test `non_streaming_completion`)
- [x] **5.4** Test: streaming chat completion returns valid SSE stream (integration test `streaming_completion`)
- [x] **5.5** Test: error turn returns 500 (integration test `error_turn_returns_500`)
- [x] **5.6** Test: X-Conversation-ID header creates distinct sessions (integration test `conversation_id_from_header`)
- [x] **5.7** Update `README.md` with gateway setup instructions
- [x] **5.8** Update `ROADMAP.md` to mark `openai-gateway` complete
- [x] **5.9** Archive to `openspec/changes/archive/openai-gateway/`

**Note:** Live curl tests (5.1–5.6 in original plan) are covered by integration tests using axum's `tower::ServiceExt::oneshot` — no running daemon needed. Live E2E testing against HA will happen when the gateway is deployed on mini.
