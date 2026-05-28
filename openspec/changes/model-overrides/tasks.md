# Tasks: model-overrides

## Design note

Resolution lives on the **dispatcher**, not in each channel. Channels keep constructing `TurnRequest { model: None, .. }` (or `Some(x)` if they have a specific reason). `Dispatcher::dispatch` resolves `req.model.or_else(|| model_config.for_surface(&req.surface_id))` before forming the `--model` argv. One site, one rule.

Surface ids in production:
- `"openai"` — gateway (channels/../gateway/mod.rs:169)
- `"discord"` — channels/discord/mod.rs:173
- `"email"`  — channels/email/mod.rs `SURFACE_ID`
- `"telegram"` — channels/telegram/mod.rs:351
- `"xmpp"` — channels/xmpp/mod.rs:694, :869
- Any string passed by a CLI/TUI/D-Bus caller — no Nix knob, falls through to default

## Nix module

- [x] `services.cairn-companion.daemon.defaultModel` option (`nullOr str`, default `null`)
- [x] `services.cairn-companion.channels.{email,discord,telegram,xmpp}.model` options (same type/default)
- [x] `services.cairn-companion.gateway.openai.backendModel` option (same type/default), distinct from existing cosmetic `modelName`
- [x] Lower each to its `COMPANION_MODEL_*` env var on the `companion-core` systemd unit using `lib.optionals (cfg.<path> != null)` — don't emit empty env vars. Env var names use the **surface id**: `COMPANION_MODEL_OPENAI`, `_DISCORD`, `_EMAIL`, `_TELEGRAM`, `_XMPP`, plus `COMPANION_MODEL_DEFAULT` for the daemon-wide fallback.
- [x] Update each option's `description` to explain: null = claude-code default, per-surface override > daemon default > None, IDs are passed verbatim to `claude --model`

## companion-core

- [x] `src/model_config.rs` exposing `ModelConfig` with fields `default`, `openai`, `discord`, `email`, `telegram`, `xmpp` (all `Option<String>`)
- [x] `ModelConfig::from_env()` reading `COMPANION_MODEL_*` (empty string = unset, same convention as the rest of the daemon)
- [x] `ModelConfig::for_surface(&self, surface_id: &str) -> Option<String>` — match on surface_id, fall through to `default` on unknown
- [x] `Dispatcher` holds `Arc<ModelConfig>`; constructor takes it; `run_turn` resolves `req.model.clone().or_else(|| model_config.for_surface(&req.surface_id))` immediately before the `--model` arg push
- [x] `Dispatcher::with_command` (cfg(test)) uses `ModelConfig::default()` so existing tests stay green
- [x] `main.rs` builds `let model_config = Arc::new(ModelConfig::from_env())` and passes to `Dispatcher::new`
- [x] Module declared in `main.rs`: `mod model_config;`

## Tests

- [x] Unit test on `ModelConfig::for_surface` covering: no env set, default only, per-surface only, both set (per-surface wins), unknown surface falls through to default
- [ ] Unit test on `ModelConfig::from_env` covering env-var reading. **Deferred** — env mutation is process-global and would need test serialization (cargo runs in parallel by default). `from_env` is five `read_env()` calls over an inline helper; the integration is covered by the manual rebuild test below. If we hit a real bug here, add a serial test gated on a Mutex.
- [x] Dispatcher argv test: `Dispatcher::with_command` + a ModelConfig with `email = Some("X")`, dispatch a turn with `surface_id = "email"` and `model: None`, assert `--model X` lands in argv
- [x] Dispatcher argv test: explicit `req.model = Some("Y")` overrides a configured `ModelConfig::email = Some("X")` — `--model Y` lands, not X
- [x] Dispatcher argv test: unknown surface_id (e.g. `"dbus"`) with `default = Some("D")` set → `--model D` lands
- [ ] Manual (Keith, post-rebuild): set `services.cairn-companion.channels.email.model` to a non-default ID, rebuild, send a test email, grep journal for `--model <id>` in the dispatched argv

## Docs

- [x] README note in the home-manager module section listing the new options and the resolution order
- [x] Mention in ROADMAP.md under Tier 1 that per-channel model selection is now configurable

## Resolved

- **CLI `--model` flag**: out. Keith won't use it. Operator config only.
- **Enumeration test for channel id coverage**: the resolution match in `for_surface` IS the enumeration. New surface ids that need overrides have to be added there — compile-time pressure, no runtime guess.
