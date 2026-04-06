# HTTP Routes Specification

## Purpose

Defines the HTTP endpoints the openai-gateway exposes inside the companion-core daemon. All routes serve an OpenAI-compatible API surface narrow enough for Home Assistant Conversation and similar consumers, not a full OpenAI API reimplementation.

## Configuration

The gateway reads its configuration from environment variables set by the home-manager module:

| Variable | Default | Description |
|----------|---------|-------------|
| `COMPANION_GATEWAY_ENABLE` | `0` | Set to `1` to start the HTTP server. When `0`, the daemon runs without the gateway (D-Bus only). |
| `COMPANION_GATEWAY_PORT` | `18789` | TCP port to bind. Matches ZeroClaw's port for migration parity. |
| `COMPANION_GATEWAY_BIND` | `0.0.0.0` | Bind address. Default binds all interfaces (Tailscale ACLs are the access control). |
| `COMPANION_GATEWAY_MODEL` | `companion` | Model name returned by `/v1/models` and echoed in completions responses. |
| `COMPANION_GATEWAY_SESSION_POLICY` | `per-conversation-id` | One of `per-conversation-id`, `single-session`, `ephemeral`. See session-policy spec. |

## ADDED Requirements

### Requirement: Health Endpoint

The gateway MUST expose `GET /health` that returns HTTP 200 with a JSON body:

```json
{"status": "ok"}
```

Content-Type: `application/json`. No authentication. This endpoint is for monitoring and load-balancer probes.

#### Scenario: Health check

- **Given**: The gateway is running
- **When**: A client sends `GET /health`
- **Then**: The response is HTTP 200 with `{"status":"ok"}`

### Requirement: Models Endpoint

The gateway MUST expose `GET /v1/models` that returns an OpenAI-format model list containing exactly one entry.

```json
{
  "object": "list",
  "data": [
    {
      "id": "<COMPANION_GATEWAY_MODEL>",
      "object": "model",
      "created": <daemon_start_unix_timestamp>,
      "owned_by": "axios-companion"
    }
  ]
}
```

Content-Type: `application/json`.

#### Scenario: Models list

- **Given**: The gateway is configured with `COMPANION_GATEWAY_MODEL=sid`
- **When**: A client sends `GET /v1/models`
- **Then**: The response contains one model with `"id": "sid"`

### Requirement: Chat Completions Endpoint

The gateway MUST expose `POST /v1/chat/completions` accepting an OpenAI-format request body. See `openai-format/spec.md` for the request and response schemas.

The endpoint MUST:

1. Extract the final user message from the `messages` array
2. Determine the conversation ID per the session policy (see `session-policy/spec.md`)
3. Construct a `TurnRequest` with `surface_id = "openai"` and dispatch through the existing `Dispatcher`
4. If `stream: false` (or absent): collect all `TurnEvent`s, return a single `ChatCompletion` JSON response
5. If `stream: true`: return an SSE stream of `ChatCompletionChunk` objects, terminated with `data: [DONE]`

Content-Type for non-streaming: `application/json`.
Content-Type for streaming: `text/event-stream`.

#### Scenario: Non-streaming completion

- **Given**: The gateway is running
- **When**: A client sends `POST /v1/chat/completions` with `{"messages":[{"role":"user","content":"hello"}],"stream":false}`
- **Then**: The response is HTTP 200 with a `ChatCompletion` JSON body containing the companion's response
- **And**: The `model` field matches the configured model name

#### Scenario: Streaming completion

- **Given**: The gateway is running
- **When**: A client sends `POST /v1/chat/completions` with `{"messages":[{"role":"user","content":"hello"}],"stream":true}`
- **Then**: The response is HTTP 200 with `Content-Type: text/event-stream`
- **And**: Each chunk is an SSE `data: <json>\n\n` line containing a `ChatCompletionChunk`
- **And**: The stream terminates with `data: [DONE]\n\n`

#### Scenario: Empty messages array

- **Given**: The gateway is running
- **When**: A client sends a request with `"messages": []`
- **Then**: The response is HTTP 400 with an OpenAI error envelope

#### Scenario: No user message in messages

- **Given**: The gateway is running
- **When**: A client sends a request where `messages` contains only system messages
- **Then**: The response is HTTP 400 with an OpenAI error envelope

#### Scenario: Dispatcher error

- **Given**: The companion binary is unavailable or a turn fails
- **When**: The dispatcher emits a `TurnEvent::Error`
- **Then**: The gateway returns HTTP 500 with an OpenAI error envelope (non-streaming) or emits an error chunk and terminates the stream (streaming)

### Requirement: Unknown Routes Return 404

Any request to a path not listed above MUST return HTTP 404 with an OpenAI-format error envelope:

```json
{
  "error": {
    "message": "Not found",
    "type": "invalid_request_error",
    "code": "not_found"
  }
}
```

### Requirement: Graceful Shutdown

When the daemon receives SIGTERM/SIGINT:

1. The HTTP listener stops accepting new connections
2. In-flight HTTP requests are allowed to complete (or timeout after the dispatcher's drain period)
3. The axum server shutdown is coordinated with the D-Bus shutdown — both happen concurrently

The gateway does NOT have its own separate shutdown timeout. It inherits the daemon's existing signal handling and systemd `TimeoutStopSec`.
