//! Channel adapters — surfaces that connect external chat platforms to the
//! companion daemon. Each adapter runs as an async task inside companion-core,
//! receives messages from its platform, dispatches them through the shared
//! `Dispatcher`, and renders responses back to the platform.
//!
//! Adapters share text-manipulation helpers via [`util`]. Beyond that they
//! are independent — telegram knows nothing about xmpp and vice versa.

pub mod telegram;
pub mod util;
pub mod xmpp;
