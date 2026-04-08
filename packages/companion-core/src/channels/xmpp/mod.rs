//! XMPP channel adapter — connects the companion daemon to a self-hosted
//! XMPP server (Prosody, ejabberd, etc.) as a native client. Handles direct
//! messages and Multi-User Chat rooms, streams responses with XEP-0308
//! Last Message Correction, and signals presence via XEP-0085 Chat States.
//!
//! Runs as an async task inside companion-core (not a separate process).
//! Env-gated via `COMPANION_XMPP_ENABLE=1`. Uses `tokio-xmpp` for stream
//! management and `xmpp-parsers` for stanza construction.
//!
//! Implementation lands in Phase 2+ of the channel-xmpp openspec change.
//! This file exists at Phase 1.6 only so the module hierarchy compiles
//! and the Phase 1.7 regression check on mini exercises the channels/
//! reorg without any xmpp logic in the way.
