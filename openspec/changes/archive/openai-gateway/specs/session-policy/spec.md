# Session Policy Specification

## Purpose

Defines how the openai-gateway maps incoming HTTP requests to dispatcher sessions. The OpenAI chat completions API has no native concept of persistent conversations — each request is nominally independent. The gateway bridges this gap with configurable session policies.

## ADDED Requirements

### Requirement: Conversation ID Resolution

The gateway MUST determine a `conversation_id` for each request using the following resolution order:

1. **`X-Conversation-ID` header** — if present and non-empty, use its value as the conversation ID. This is the primary mechanism for clients that want explicit session control (e.g., Home Assistant configured with a static conversation identifier per room/satellite).

2. **Fallback per session policy** — if no header is present, the configured `COMPANION_GATEWAY_SESSION_POLICY` determines behavior:

| Policy | Behavior |
|--------|----------|
| `per-conversation-id` | Use `"openai-default"` as the conversation ID. All headerless requests share one session. Clients that want distinct sessions must send the header. |
| `single-session` | Always use `"openai-default"` regardless of headers. All gateway traffic shares one session. The `X-Conversation-ID` header is ignored. |
| `ephemeral` | Generate a UUID v4 for each request. Every request starts a fresh companion session. The `X-Conversation-ID` header is ignored. |

The `surface_id` is always `"openai"` for all gateway requests.

#### Scenario: Client sends X-Conversation-ID with per-conversation-id policy

- **Given**: Policy is `per-conversation-id`
- **When**: A request arrives with `X-Conversation-ID: kitchen-satellite`
- **Then**: The dispatcher receives `surface_id="openai"`, `conversation_id="kitchen-satellite"`
- **And**: Follow-up requests with the same header resume the same session

#### Scenario: Client omits header with per-conversation-id policy

- **Given**: Policy is `per-conversation-id`
- **When**: A request arrives without `X-Conversation-ID`
- **Then**: The dispatcher receives `conversation_id="openai-default"`

#### Scenario: Single-session policy ignores header

- **Given**: Policy is `single-session`
- **When**: Requests arrive with different `X-Conversation-ID` values
- **Then**: All requests use `conversation_id="openai-default"`

#### Scenario: Ephemeral policy generates unique IDs

- **Given**: Policy is `ephemeral`
- **When**: Two requests arrive (with or without headers)
- **Then**: Each request gets a unique UUID conversation ID
- **And**: Neither resumes a prior session

### Requirement: Session Lifetime

Gateway sessions follow the same lifecycle as any other surface's sessions in the dispatcher. They are created on first request, persist in SQLite, and can be resumed on subsequent requests (unless ephemeral policy is in effect).

There is no session expiry or cleanup in this proposal. Session cleanup is a daemon-wide concern deferred to a future proposal.

### Requirement: Home Assistant Integration Pattern

Home Assistant's OpenAI Conversation integration sends requests that look like standard OpenAI chat completions — a `messages` array with conversation history. By default HA does not send custom headers.

The recommended configuration for HA voice with per-room sessions:

1. Set gateway policy to `per-conversation-id`
2. Configure each HA Conversation agent (one per room/satellite) with a distinct URL or use HA's conversation ID passthrough if available
3. Alternatively, set policy to `single-session` if all voice interactions should share one continuous conversation with Sid

For most deployments, `single-session` is the pragmatic default — Keith talks to Sid from whichever room he's in, and wants Sid to remember what was said regardless of which satellite picked it up.
