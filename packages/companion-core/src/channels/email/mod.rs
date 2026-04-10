//! Email channel adapter — connects the companion daemon to an SMTP/IMAP
//! mailbox that belongs to the bot itself. Inbound mail is polled from
//! IMAP, parsed, quote-stripped, allowlisted, dispatched, and replied to
//! via SMTP. Each mail thread is its own dispatcher session, keyed on the
//! RFC 5322 thread root Message-ID — replies in the same thread continue
//! the same Claude session; new threads start fresh ones.
//!
//! Runs as an async task inside companion-core (not a separate process).
//! Env-gated via `COMPANION_EMAIL_ENABLE=1`. Uses `async-imap` for the
//! IMAP poll loop, `lettre` for SMTP submit, and `mail-parser` for MIME
//! decoding plus header inspection. TLS goes through the same
//! `tokio-rustls` + `aws_lc_rs` provider the xmpp connector installs at
//! daemon startup.
//!
//! ## Scope
//!
//! This adapter handles the companion's OWN inbox — the address
//! configured via `services.axios-companion.channels.email.address`. Mail
//! addressed TO that address lands in the dispatcher as a turn. It is not
//! a mechanism for the companion to read other mailboxes on the user's
//! behalf; if you want that, expose it through an MCP tool server, not
//! through this channel adapter. The two have different trust boundaries
//! and different failure domains and must not be conflated.

mod command;
mod config;
mod fetch;
mod parse;
mod send;

pub use config::EmailConfig;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tracing::{debug, error, info, warn};

use crate::dispatcher::{Dispatcher, TrustLevel, TurnEvent, TurnRequest};

use self::parse::ParsedMessage;

/// Surface identifier reported to the dispatcher. Same string the session
/// store uses as the surface column for email rows.
const SURFACE_ID: &str = "email";

/// Start the email adapter. Blocks until `shutdown` fires. On any IMAP
/// connection error the loop reconnects with exponential backoff so the
/// bot survives mail-server restarts and brief network drops.
pub async fn serve(dispatcher: Arc<Dispatcher>, config: EmailConfig, shutdown: Arc<Notify>) {
    let config = Arc::new(config);
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    info!(
        address = %config.address,
        imap = format!("{}:{}", config.imap_host, config.imap_port),
        smtp = format!("{}:{}", config.smtp_host, config.smtp_port),
        poll_secs = config.poll_interval.as_secs(),
        allowed_senders = config.allowed_senders.len(),
        "email adapter starting"
    );

    loop {
        let cfg = config.clone();
        let disp = dispatcher.clone();
        let stop = shutdown.clone();

        let session = run_session(cfg, disp, stop);

        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                info!("email adapter shutting down");
                return;
            }
            outcome = session => {
                match outcome {
                    Ok(()) => {
                        warn!("email session ended cleanly — reconnecting");
                        backoff = Duration::from_secs(1);
                    }
                    Err(e) => {
                        error!(%e, ?backoff, "email session error — reconnecting after backoff");
                        tokio::select! {
                            _ = shutdown.notified() => {
                                info!("email adapter shutting down during backoff");
                                return;
                            }
                            _ = tokio::time::sleep(backoff) => {}
                        }
                        backoff = (backoff * 2).min(max_backoff);
                    }
                }
            }
        }
    }
}

/// One IMAP-connect → login → SELECT INBOX → poll-loop cycle. Returns
/// `Ok(())` on graceful shutdown, `Err(_)` on any I/O or protocol failure
/// (caller decides whether to reconnect).
async fn run_session(
    config: Arc<EmailConfig>,
    dispatcher: Arc<Dispatcher>,
    shutdown: Arc<Notify>,
) -> Result<(), EmailError> {
    let mut session = fetch::connect_and_login(&config).await?;
    info!(address = %config.address, "email IMAP authenticated");

    loop {
        // Pull all currently-unseen messages, oldest first.
        let messages = match fetch::fetch_unseen(&mut session).await {
            Ok(m) => m,
            Err(e) => {
                // SEARCH/FETCH failure is session-fatal. Log and bail so
                // the outer loop reconnects.
                let _ = session.logout().await;
                return Err(e);
            }
        };

        if !messages.is_empty() {
            info!(count = messages.len(), "email: processing unseen messages");
        }

        for raw in messages {
            let parsed = match parse::parse(&raw.body) {
                Some(p) => p,
                None => {
                    warn!(uid = raw.uid, "email: failed to parse message, marking seen");
                    let _ = fetch::mark_seen(&mut session, raw.uid).await;
                    continue;
                }
            };
            handle_message(&config, &dispatcher, &parsed, &raw.body).await;
            if let Err(e) = fetch::mark_seen(&mut session, raw.uid).await {
                warn!(uid = raw.uid, %e, "email: failed to STORE \\Seen, will reprocess next poll");
            }
        }

        // Sleep until next poll, but bail out promptly on shutdown.
        tokio::select! {
            biased;
            _ = shutdown.notified() => {
                let _ = session.logout().await;
                return Ok(());
            }
            _ = tokio::time::sleep(config.poll_interval) => {}
        }
    }
}

/// Process a single inbound message: filter loops, allowlist, parse, dispatch,
/// reply via SMTP, append to Sent. Errors are logged here, never propagated —
/// one bad message must not kill the poll loop.
async fn handle_message(
    config: &EmailConfig,
    dispatcher: &Dispatcher,
    parsed: &ParsedMessage,
    raw: &[u8],
) {
    // Loop prevention: drop anything that smells like an auto-reply or
    // bounce. Without this guard the bot would happily reply to its own
    // out-of-office, vacation responders, mailing list digests, and DSNs —
    // which is how you turn one stranger's spam into a thousand-message
    // tarpit between the bot and a hapless mailserver.

    // Self-address check. Defense in depth on top of the Auto-Submitted
    // header (which we set on outbound and filter on inbound below) —
    // some intermediate mail gateways strip headers they don't
    // recognize, and a forged inbound claiming to be from us shouldn't
    // get a reply either. The bot never legitimately receives mail
    // from itself, so dropping is always correct.
    if is_from_self(&parsed.from_address, &config.address) {
        debug!(
            from = %parsed.from_address,
            "email: dropping message that claims to be from the bot itself"
        );
        return;
    }
    if parsed.is_auto_submitted() {
        debug!(
            from = %parsed.from_address,
            "email: dropping auto-submitted message"
        );
        return;
    }
    if parsed.is_bounce_or_no_reply() {
        debug!(
            from = %parsed.from_address,
            "email: dropping bounce / no-reply message"
        );
        return;
    }

    let trust = if config.is_allowed(&parsed.from_address) {
        TrustLevel::Owner
    } else {
        TrustLevel::Anonymous
    };

    let stripped_body = parse::strip_quoted(&parsed.body_text);
    if stripped_body.trim().is_empty() {
        debug!(
            from = %parsed.from_address,
            "email: dropping message with empty body after quote stripping"
        );
        return;
    }

    let conversation_id = parsed.thread_root.clone();

    info!(
        from = %parsed.from_address,
        subject = %parsed.subject,
        thread_root = %conversation_id,
        ?trust,
        body_len = stripped_body.len(),
        "email message received"
    );

    // Bang commands short-circuit the dispatcher. Same convention as xmpp
    // — slash commands collide with mail clients that interpret them
    // locally (Apple Mail, Outlook), bang is the safer prefix.
    let trimmed = stripped_body.trim();
    if trimmed.starts_with('!') {
        let reply_text = command::handle(SURFACE_ID, &conversation_id, trimmed, dispatcher).await;
        send_reply(config, parsed, &reply_text).await;
        return;
    }

    let turn_req = TurnRequest {
        surface_id: SURFACE_ID.into(),
        conversation_id,
        message_text: stripped_body,
        trust,
    };

    let mut rx = dispatcher.dispatch(turn_req).await;
    let reply_text = collect_reply(&mut rx).await;

    if reply_text.is_empty() {
        warn!(from = %parsed.from_address, "email: dispatcher produced empty reply, skipping send");
        return;
    }

    send_reply(config, parsed, &reply_text).await;
    let _ = raw; // currently unused; reserved for future raw-archive features
}

/// Drain the dispatcher channel for a single turn, accumulating text into
/// one final string. Email is not interactive — there's no streaming, no
/// edit-in-place. We collect everything and send one SMTP message at the
/// end.
async fn collect_reply(rx: &mut tokio::sync::mpsc::Receiver<TurnEvent>) -> String {
    let mut accumulated = String::new();
    while let Some(event) = rx.recv().await {
        match event {
            TurnEvent::TextChunk(chunk) => accumulated.push_str(&chunk),
            TurnEvent::Complete(text) => {
                // Complete carries the canonical full response — prefer it
                // over the streamed accumulation in case the model emitted
                // a final correction.
                return text;
            }
            TurnEvent::Error(e) => {
                return format!("Something went sideways on this end: {e}");
            }
        }
    }
    accumulated
}

/// Send a reply via SMTP and append it to the IMAP Sent folder. Errors are
/// logged but not propagated — failing to file in Sent is annoying but not
/// fatal, and failing to SMTP-send the reply leaves the inbound message
/// already marked seen (slightly worse, but the alternative is reprocessing
/// the same inbound forever).
async fn send_reply(config: &EmailConfig, parsed: &ParsedMessage, reply_text: &str) {
    let outbound = match send::build_reply(config, parsed, reply_text) {
        Ok(m) => m,
        Err(e) => {
            error!(%e, "email: failed to build reply message");
            return;
        }
    };

    if let Err(e) = send::send_smtp(config, &outbound).await {
        error!(%e, "email: SMTP send failed");
        return;
    }

    if let Err(e) = send::append_to_sent(config, &outbound).await {
        warn!(%e, "email: failed to APPEND to Sent folder (reply was still delivered)");
    }
}

/// Returns true if the inbound `From:` address resolves to the bot's
/// own configured address. Comparison is case-insensitive on the whole
/// address — RFC 5321 says local-parts MAY be case-sensitive but in
/// practice nobody enforces it, and treating `Bot@Example.Com` and
/// `bot@example.com` as the same identity is what every operator
/// actually expects.
///
/// `parsed_from` is expected to already be lowercased at parse time
/// (see `parse::parse`); we lowercase `bot_address` here to match.
fn is_from_self(parsed_from: &str, bot_address: &str) -> bool {
    parsed_from == bot_address.to_ascii_lowercase()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_address_check_matches_exact() {
        assert!(is_from_self("bot@example.com", "bot@example.com"));
    }

    #[test]
    fn self_address_check_is_case_insensitive_on_config() {
        // parse::parse lowercases the inbound from_address. The config
        // value can be in any case the operator typed it.
        assert!(is_from_self("bot@example.com", "Bot@Example.Com"));
        assert!(is_from_self("bot@example.com", "BOT@EXAMPLE.COM"));
    }

    #[test]
    fn self_address_check_rejects_other_senders() {
        assert!(!is_from_self("alice@example.com", "bot@example.com"));
        assert!(!is_from_self("bot@other.example", "bot@example.com"));
        // Different local-part, same domain — common in shared-domain
        // deployments where the bot lives on the same host as humans.
        assert!(!is_from_self("notbot@example.com", "bot@example.com"));
    }

    #[test]
    fn self_address_check_rejects_substring_match() {
        // The check is full-string equality, not substring. A sender
        // with the bot's address as a suffix of its own should NOT
        // match — that would be the kind of subtle false positive
        // that's worth catching at test time.
        assert!(!is_from_self("evil-bot@example.com", "bot@example.com"));
    }
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Session-fatal errors that bubble up to `serve()`'s reconnect loop. Most
/// per-message failures are logged inside `handle_message` and never reach
/// this type — only IMAP connect/auth/protocol errors that invalidate the
/// session as a whole.
#[derive(Debug, thiserror::Error)]
pub enum EmailError {
    #[error("IMAP connect failed: {0}")]
    Connect(#[source] std::io::Error),
    #[error("TLS handshake failed: {0}")]
    Tls(#[source] std::io::Error),
    #[error("IMAP login failed: {0}")]
    Login(String),
    #[error("IMAP protocol error: {0}")]
    Protocol(String),
    #[error("invalid server name: {0}")]
    InvalidServerName(String),
}
