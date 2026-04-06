# OpenAI Format Specification

## Purpose

Defines the subset of the OpenAI chat completions API that the gateway implements. This is intentionally narrow — only what Home Assistant Conversation and common OpenAI-compatible clients actually send and expect.

## ADDED Requirements

### Requirement: Chat Completion Request Schema

The gateway MUST accept the following request body (JSON):

```
{
  "model": string | null,        // ignored — always routes to companion
  "messages": [
    {
      "role": "system" | "user" | "assistant",
      "content": string
    }
  ],
  "stream": bool | null,          // default false
  "temperature": number | null,   // accepted, ignored
  "max_tokens": number | null,    // accepted, ignored
  "top_p": number | null          // accepted, ignored
}
```

The gateway MUST extract the **last message with `role: "user"`** from the `messages` array as the text to dispatch. System and assistant messages in the array are informational context that the companion's own persona and session history already handle — they are logged at debug level but not forwarded.

Fields beyond those listed above (e.g., `frequency_penalty`, `presence_penalty`, `tools`, `functions`) MUST be silently ignored. The gateway does not reject requests that include unknown fields.

#### Scenario: Model field is ignored

- **Given**: A request with `"model": "gpt-4"`
- **When**: The gateway processes it
- **Then**: The request is routed through the companion as normal
- **And**: The response `model` field contains the configured gateway model name, not `"gpt-4"`

### Requirement: Chat Completion Response Schema (Non-Streaming)

When `stream` is `false` or absent, the gateway MUST return:

```json
{
  "id": "chatcmpl-<uuid>",
  "object": "chat.completion",
  "created": <unix_timestamp>,
  "model": "<configured_model_name>",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": "<full_response_text>"
      },
      "finish_reason": "stop"
    }
  ],
  "usage": {
    "prompt_tokens": 0,
    "completion_tokens": 0,
    "total_tokens": 0
  }
}
```

The `id` MUST be unique per response. The `usage` block is always zeros — the companion does not expose token counts, but the field is required by many clients for parsing.

### Requirement: Chat Completion Chunk Schema (Streaming)

When `stream` is `true`, the gateway MUST return SSE-formatted chunks:

```
data: {"id":"chatcmpl-<uuid>","object":"chat.completion.chunk","created":<ts>,"model":"<model>","choices":[{"index":0,"delta":{"role":"assistant","content":"<chunk>"},"finish_reason":null}]}\n\n
```

The first chunk SHOULD include `"role": "assistant"` in the delta. Subsequent chunks include only `"content"`. The final chunk before `[DONE]` MUST have `"finish_reason": "stop"` and an empty or absent `content`.

The stream terminates with:

```
data: [DONE]\n\n
```

All chunks in a single response share the same `id`.

#### Scenario: Streaming produces well-formed SSE

- **Given**: A streaming request
- **When**: The companion produces text chunks "Hello " and "world."
- **Then**: The SSE stream contains at least 3 data lines: one with "Hello ", one with "world.", and a final stop chunk
- **And**: The stream ends with `data: [DONE]`

### Requirement: Error Response Schema

Errors MUST be returned in the OpenAI error envelope:

```json
{
  "error": {
    "message": "<human_readable_description>",
    "type": "<error_type>",
    "code": "<error_code>"
  }
}
```

Error type mapping:

| Condition | HTTP status | `type` | `code` |
|-----------|------------|--------|--------|
| Malformed JSON body | 400 | `invalid_request_error` | `invalid_json` |
| Missing/empty messages | 400 | `invalid_request_error` | `invalid_messages` |
| No user message found | 400 | `invalid_request_error` | `no_user_message` |
| Dispatcher/companion error | 500 | `server_error` | `companion_error` |
| Route not found | 404 | `invalid_request_error` | `not_found` |

### Requirement: Content-Type Handling

The gateway MUST accept `Content-Type: application/json` on POST requests. It SHOULD also accept requests without a Content-Type header (some clients omit it). It MUST NOT require any other headers.
