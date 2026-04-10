//! Configuration for the email channel adapter.
//!
//! All fields are populated from `COMPANION_EMAIL_*` environment variables,
//! same pattern as the xmpp and telegram adapters. Returns `None` from
//! [`EmailConfig::from_env`] if `COMPANION_EMAIL_ENABLE != 1` or any
//! required field is missing/invalid.

use std::collections::HashSet;
use std::time::Duration;

use tracing::{error, warn};

/// Email channel configuration.
#[derive(Debug, Clone)]
pub struct EmailConfig {
    /// The bot's own address — both the IMAP login username and the
    /// outbound `From:` address. Example: `bot@example.com`.
    pub address: String,

    /// Display name for the outbound `From:` header (e.g. the bot's
    /// character name, or just `"Bot"`). Defaults to the local part of
    /// `address` if unset.
    pub display_name: String,

    /// IMAP/SMTP password (single line, no trailing newline). Read from
    /// `COMPANION_EMAIL_PASSWORD_FILE` so it doesn't have to live in
    /// process environment.
    pub password: String,

    pub imap_host: String,
    pub imap_port: u16,

    pub smtp_host: String,
    pub smtp_port: u16,

    /// How often to poll IMAP for unseen messages.
    pub poll_interval: Duration,

    /// Lowercased email addresses allowed to talk to the bot at
    /// [`TrustLevel::Owner`](crate::dispatcher::TrustLevel::Owner). Anyone
    /// else gets [`TrustLevel::Anonymous`](crate::dispatcher::TrustLevel::Anonymous).
    /// An empty set means everyone is anonymous — that's a valid (if
    /// unusual) deployment, not an error.
    pub allowed_senders: HashSet<String>,
}

impl EmailConfig {
    /// Build config from environment variables. Returns `None` if the
    /// channel is not enabled or any required field is missing/invalid.
    /// Logs the failure reason at error level so a misconfigured deploy
    /// is loud, not silent.
    ///
    /// Env vars:
    /// - `COMPANION_EMAIL_ENABLE` — required, must be `"1"`
    /// - `COMPANION_EMAIL_ADDRESS` — required, the bot's full mail address
    /// - `COMPANION_EMAIL_DISPLAY_NAME` — optional, defaults to local-part of address
    /// - `COMPANION_EMAIL_PASSWORD_FILE` — required, path to a file containing the password
    /// - `COMPANION_EMAIL_IMAP_HOST` — required, IMAP server hostname
    /// - `COMPANION_EMAIL_IMAP_PORT` — optional, defaults to `993`
    /// - `COMPANION_EMAIL_SMTP_HOST` — required, SMTP server hostname
    /// - `COMPANION_EMAIL_SMTP_PORT` — optional, defaults to `465`
    /// - `COMPANION_EMAIL_POLL_INTERVAL_SECS` — optional, defaults to `30`
    /// - `COMPANION_EMAIL_ALLOWED_SENDERS` — optional, comma-separated allowlist
    pub fn from_env() -> Option<Self> {
        if std::env::var("COMPANION_EMAIL_ENABLE").ok()?.as_str() != "1" {
            return None;
        }

        let address = match std::env::var("COMPANION_EMAIL_ADDRESS") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                error!("COMPANION_EMAIL_ADDRESS not set");
                return None;
            }
        };
        if !address.contains('@') {
            error!(address = %address, "COMPANION_EMAIL_ADDRESS missing '@' — not a valid mail address");
            return None;
        }

        let display_name = std::env::var("COMPANION_EMAIL_DISPLAY_NAME")
            .ok()
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| {
                address
                    .split('@')
                    .next()
                    .unwrap_or(&address)
                    .to_string()
            });

        let password_file = match std::env::var("COMPANION_EMAIL_PASSWORD_FILE") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                error!("COMPANION_EMAIL_PASSWORD_FILE not set");
                return None;
            }
        };
        let password = match std::fs::read_to_string(&password_file) {
            Ok(p) => p.trim().to_string(),
            Err(e) => {
                error!(path = %password_file, %e, "failed to read email password file");
                return None;
            }
        };
        if password.is_empty() {
            error!(path = %password_file, "email password file is empty");
            return None;
        }

        let imap_host = match std::env::var("COMPANION_EMAIL_IMAP_HOST") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                error!("COMPANION_EMAIL_IMAP_HOST not set");
                return None;
            }
        };
        let imap_port: u16 = std::env::var("COMPANION_EMAIL_IMAP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(993);

        let smtp_host = match std::env::var("COMPANION_EMAIL_SMTP_HOST") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                error!("COMPANION_EMAIL_SMTP_HOST not set");
                return None;
            }
        };
        let smtp_port: u16 = std::env::var("COMPANION_EMAIL_SMTP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(465);

        let poll_secs: u64 = std::env::var("COMPANION_EMAIL_POLL_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        // Floor at 5s — anything tighter and we're hammering the IMAP
        // server pointlessly. Anything looser is fine.
        let poll_interval = Duration::from_secs(poll_secs.max(5));

        let allowed_senders = parse_allowed_senders(
            std::env::var("COMPANION_EMAIL_ALLOWED_SENDERS")
                .unwrap_or_default()
                .as_str(),
        );

        Some(Self {
            address,
            display_name,
            password,
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            poll_interval,
            allowed_senders,
        })
    }

    /// Returns true if `from_address` (case-insensitive) is in the allowlist.
    /// An empty allowlist returns false for everyone — those messages still
    /// get processed, just at `TrustLevel::Anonymous` instead of `Owner`.
    pub fn is_allowed(&self, from_address: &str) -> bool {
        let needle = from_address.to_ascii_lowercase();
        self.allowed_senders.contains(&needle)
    }
}

/// Parse a comma-separated allowlist into a lowercase HashSet. Whitespace
/// around entries is trimmed; empty entries are dropped; entries without
/// `@` are dropped with a warning (likely a misconfiguration).
fn parse_allowed_senders(raw: &str) -> HashSet<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .filter_map(|s| {
            if !s.contains('@') {
                warn!(entry = %s, "skipping invalid sender in COMPANION_EMAIL_ALLOWED_SENDERS (no '@')");
                return None;
            }
            Some(s.to_ascii_lowercase())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_allowlist_lowercases() {
        let set = parse_allowed_senders("Alice@Example.com, BOB@example.org");
        assert!(set.contains("alice@example.com"));
        assert!(set.contains("bob@example.org"));
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn parse_allowlist_drops_invalid_entries() {
        let set = parse_allowed_senders("alice@example.com, garbage, , another@host");
        assert_eq!(set.len(), 2);
        assert!(set.contains("alice@example.com"));
        assert!(set.contains("another@host"));
    }

    #[test]
    fn parse_allowlist_empty() {
        let set = parse_allowed_senders("");
        assert!(set.is_empty());
    }

    #[test]
    fn is_allowed_case_insensitive() {
        let cfg = EmailConfig {
            address: "bot@example.com".into(),
            display_name: "Bot".into(),
            password: "x".into(),
            imap_host: "h".into(),
            imap_port: 993,
            smtp_host: "h".into(),
            smtp_port: 465,
            poll_interval: Duration::from_secs(30),
            allowed_senders: ["alice@example.com".to_string()].into_iter().collect(),
        };
        assert!(cfg.is_allowed("Alice@Example.com"));
        assert!(cfg.is_allowed("alice@example.com"));
        assert!(!cfg.is_allowed("nobody@example.com"));
    }

    #[test]
    fn is_allowed_empty_allowlist() {
        let cfg = EmailConfig {
            address: "bot@example.com".into(),
            display_name: "Bot".into(),
            password: "x".into(),
            imap_host: "h".into(),
            imap_port: 993,
            smtp_host: "h".into(),
            smtp_port: 465,
            poll_interval: Duration::from_secs(30),
            allowed_senders: HashSet::new(),
        };
        // Empty allowlist = nobody is Owner. is_allowed returns false for
        // all senders; the channel hands them off as Anonymous, not denied.
        assert!(!cfg.is_allowed("anyone@anywhere"));
    }
}
