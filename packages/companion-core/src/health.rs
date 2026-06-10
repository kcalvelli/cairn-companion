//! Liveness registry for channel adapters and the OpenAI gateway.
//!
//! Channel adapters write their connection state as they connect, drop, and
//! retry; the D-Bus `GetHealth` method reads it. This is a point-in-time
//! mirror of state the adapters already compute in their reconnect loops —
//! not an event log, not a source of truth. `companion doctor` is the only
//! consumer that matters.
//!
//! The registry lives behind an `Arc` on the [`Dispatcher`](crate::dispatcher::Dispatcher)
//! so both the adapters (which hold a `dispatcher` clone) and the D-Bus
//! interface can reach it without extra plumbing. Writes take a short
//! `std::sync::Mutex` and never hold it across an `.await`, so this stays
//! out of the async runtime's way.

use std::collections::BTreeMap;
use std::sync::Mutex;

/// Connection state of a single channel adapter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    /// Connected to the upstream service and serving turns.
    Connected,
    /// Lost the connection and retrying with backoff, or starting up and
    /// not yet connected for the first time.
    Reconnecting,
    /// Hard-failed in a way the adapter does not retry from (e.g. a bad
    /// token that fails at startup).
    Down,
}

impl ChannelState {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelState::Connected => "connected",
            ChannelState::Reconnecting => "reconnecting",
            ChannelState::Down => "down",
        }
    }
}

#[derive(Clone)]
struct ChannelHealth {
    state: ChannelState,
    last_error: Option<String>,
}

#[derive(Clone)]
struct GatewayInfo {
    bind: String,
    port: u16,
}

#[derive(Default)]
struct Inner {
    channels: BTreeMap<String, ChannelHealth>,
    /// `None` means the gateway is disabled. `Some` carries its bind/port
    /// so `doctor` knows where to probe `/health` — the daemon reports
    /// config, the CLI does the liveness probe.
    gateway: Option<GatewayInfo>,
}

/// Shared health state. Construct via `Default`; clone the `Arc`, not this.
#[derive(Default)]
pub struct HealthRegistry {
    inner: Mutex<Inner>,
}

impl HealthRegistry {
    /// Record a channel's current connection state. Called by adapters at
    /// connect / disconnect / retry transitions. Idempotent — last write
    /// wins.
    pub fn set_channel(&self, name: &str, state: ChannelState, last_error: Option<String>) {
        let mut inner = self.inner.lock().expect("health mutex poisoned");
        inner
            .channels
            .insert(name.to_string(), ChannelHealth { state, last_error });
    }

    /// Record the gateway's configured bind address and port. Called once
    /// at startup when the gateway is enabled. Absence = disabled.
    pub fn set_gateway(&self, bind: impl Into<String>, port: u16) {
        let mut inner = self.inner.lock().expect("health mutex poisoned");
        inner.gateway = Some(GatewayInfo {
            bind: bind.into(),
            port,
        });
    }

    /// Snapshot for the D-Bus `GetHealth` method.
    ///
    /// Returns `(channels, gateway)` where each channel is
    /// `(name, state_str, last_error)` (empty string = no error) sorted by
    /// name, and `gateway` is `Some((bind, port))` when enabled.
    #[allow(clippy::type_complexity)]
    pub fn snapshot(&self) -> (Vec<(String, String, String)>, Option<(String, u16)>) {
        let inner = self.inner.lock().expect("health mutex poisoned");
        let channels = inner
            .channels
            .iter()
            .map(|(name, h)| {
                (
                    name.clone(),
                    h.state.as_str().to_string(),
                    h.last_error.clone().unwrap_or_default(),
                )
            })
            .collect();
        let gateway = inner.gateway.as_ref().map(|g| (g.bind.clone(), g.port));
        (channels, gateway)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_reports_nothing() {
        let r = HealthRegistry::default();
        let (channels, gateway) = r.snapshot();
        assert!(channels.is_empty());
        assert!(gateway.is_none());
    }

    #[test]
    fn channel_state_transitions_last_write_wins() {
        let r = HealthRegistry::default();
        r.set_channel("telegram", ChannelState::Reconnecting, None);
        r.set_channel("telegram", ChannelState::Connected, None);
        let (channels, _) = r.snapshot();
        assert_eq!(channels.len(), 1);
        assert_eq!(channels[0], ("telegram".into(), "connected".into(), String::new()));
    }

    #[test]
    fn down_channel_captures_last_error() {
        let r = HealthRegistry::default();
        r.set_channel("email", ChannelState::Down, Some("auth failed".into()));
        let (channels, _) = r.snapshot();
        assert_eq!(
            channels[0],
            ("email".into(), "down".into(), "auth failed".into())
        );
    }

    #[test]
    fn reconnecting_carries_error_string() {
        let r = HealthRegistry::default();
        r.set_channel("xmpp", ChannelState::Reconnecting, Some("stream closed".into()));
        let (channels, _) = r.snapshot();
        assert_eq!(channels[0].1, "reconnecting");
        assert_eq!(channels[0].2, "stream closed");
    }

    #[test]
    fn channels_sorted_by_name() {
        let r = HealthRegistry::default();
        r.set_channel("telegram", ChannelState::Connected, None);
        r.set_channel("discord", ChannelState::Connected, None);
        r.set_channel("email", ChannelState::Connected, None);
        let (channels, _) = r.snapshot();
        let names: Vec<&str> = channels.iter().map(|c| c.0.as_str()).collect();
        assert_eq!(names, ["discord", "email", "telegram"]);
    }

    #[test]
    fn gateway_reports_bind_and_port() {
        let r = HealthRegistry::default();
        r.set_gateway("127.0.0.1", 18789);
        let (_, gateway) = r.snapshot();
        assert_eq!(gateway, Some(("127.0.0.1".into(), 18789)));
    }
}
