# Proposal: Model Overrides — Per-Channel Backend Model Selection

> **Status**: Shipped 2026-05-28 — all Nix options, ModelConfig, dispatcher resolution, unit + argv tests, README + ROADMAP entries landed in one commit.

## Tier

Tier 1

## Summary

Add Nix-configured per-channel overrides for the Claude model the dispatcher passes to the `claude` subprocess. Today every channel constructs `TurnRequest { model: None, ... }` and the subprocess uses whatever `~/.claude/settings.json` resolves (Opus, on a default install). The plumbing already exists — `TurnRequest.model: Option<String>` flows to `--model <x>` in `dispatcher.rs` — there's just no way to populate it without editing Rust.

## Motivation

The driver is cost. Email classification, link previews, "did this MUC message mention me" gating — these are short, high-volume, low-stakes turns that have no business running on Opus. Voice and Discord are different: they're interactive, latency-visible, and benefit from the smarter model. Today it's all Opus because there's no knob, and editing channel source per-environment isn't the answer.

The Nix module is already the settings layer for this project — `services.cairn-companion.gateway.openai.modelName` already lowers to `COMPANION_GATEWAY_MODEL` on the systemd unit (`modules/home-manager/default.nix:769-777, 883`). Adding model selection is the same pattern: new `mkOption`, new env var, read at daemon startup, plumbed into `TurnRequest.model` instead of `None`.

A previous commit (9febb5c) stripped the gateway's habit of forwarding the OpenAI client's `model` field as `--model`, because pinning a retired Anthropic model ID would break the voice pipeline on Claude rotations. That decision stands. This proposal is about *operator-configured* overrides set in Nix, not *client-requested* overrides. Clients still don't get to pick the backend model.

## Scope

### In scope

- New Nix options that lower to env vars on the `companion-core` systemd unit:
  - `services.cairn-companion.daemon.defaultModel` — fallback when no channel-specific override is set; null means "let claude-code's own default win" (current behavior)
  - `services.cairn-companion.channels.email.model`
  - `services.cairn-companion.channels.discord.model`
  - `services.cairn-companion.channels.telegram.model`
  - `services.cairn-companion.channels.xmpp.model`
  - `services.cairn-companion.gateway.openai.backendModel` — distinct from the existing cosmetic `modelName` field
- Env var bucket: `COMPANION_MODEL_DEFAULT`, `COMPANION_MODEL_EMAIL`, `COMPANION_MODEL_DISCORD`, `COMPANION_MODEL_TELEGRAM`, `COMPANION_MODEL_XMPP`, `COMPANION_MODEL_GATEWAY`
- New `ModelConfig` struct in `companion-core`, read once at startup and held on `AppState`
- Each channel resolves its model as: channel override → daemon default → `None`, and seeds `TurnRequest.model` accordingly
- D-Bus `SendMessage` path (CLI/TUI) follows the same resolution under a synthetic `"dbus"` channel id, with no Nix option exposed for it — these are owner turns from Keith's terminal and should match the daemon default

### Out of scope

- Per-conversation runtime overrides (e.g., a `/model haiku` slash command in the CLI, or a `companion --model <x> "prompt"` flag). Explicitly punted — Keith won't use it. Operator config only.
- Validating model IDs against Anthropic's live catalog. `claude --model <x>` already fails fast if the ID is bogus; we don't need a second validator that goes stale on every model release.
- Re-enabling the gateway's "forward the OpenAI client's `model` field as `--model`" behavior. That was removed deliberately (9febb5c) and stays gone.
- Per-channel *persona* overrides. Separate axis, separate proposal if it ever matters.

### Non-goals

- Reimplementing claude-code's model selection or building a router. The subprocess does the work; this proposal just sets a flag on it. (Wrapper-Only rule.)
- Cost/usage accounting per channel. If that's wanted it goes through claude-code's own telemetry, not a layer we invent here.
- A new auth or account system for "premium" channels. (No Separate Auth rule.)

## Dependencies

- `daemon-core` (shipped)

## Success criteria

1. Setting `services.cairn-companion.channels.email.model = "claude-haiku-4-5-20251001"` and rebuilding causes email-channel turns to launch `claude --model claude-haiku-4-5-20251001 ...`, verifiable via `journalctl --user -u companion-core` and the dispatcher's existing debug logging.
2. Leaving all model options at their `null` default produces zero behavior change from current main — no `--model` flag passed, claude-code's own default wins.
3. `services.cairn-companion.daemon.defaultModel = "claude-haiku-4-5-20251001"` alone (no per-channel overrides) downgrades every channel to Haiku.
4. A per-channel override takes precedence over the daemon default (set both, channel wins).
5. The existing `services.cairn-companion.gateway.openai.modelName` continues to control only the cosmetic label echoed on `/v1/models` and chat completion responses — operator can set `backendModel = "claude-haiku-4-5-20251001"` while keeping `modelName = "companion"` for client compatibility.
6. Invalid model IDs surface as a clean dispatcher error (whatever `claude --model <bogus>` already produces) rather than a daemon-side validation failure.
