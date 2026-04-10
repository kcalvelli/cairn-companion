//! Outbound mail: build a `lettre::Message` reply, send it via SMTPS, and
//! APPEND a copy to the IMAP Sent folder.
//!
//! SMTP TLS is implicit (port 465 wrapper) by default. STARTTLS support
//! could be added by switching `Tls::Wrapper` to `Tls::Required`, but the
//! initial version only targets SMTPS — if a STARTTLS deployment ever
//! needs to be supported, expose the choice via a new `smtpTls` enum
//! option in the home-manager module and branch here.
//!
//! Sent-folder APPEND opens a fresh IMAP session per reply rather than
//! threading the polling session through the call. The polling session
//! is in the middle of an outer SELECT INBOX loop and reusing it for
//! cross-folder APPEND is more bookkeeping than it's worth at email's
//! traffic levels.

use std::error::Error as StdError;
use std::sync::Arc;

use async_imap::Client;
use lettre::message::header::{Header, HeaderName, HeaderValue};
use lettre::message::{Mailbox, Message as LettreMessage};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::TlsConnector;
use tracing::debug;
use uuid::Uuid;

use super::config::EmailConfig;
use super::fetch::build_tls_config;
use super::parse::ParsedMessage;

/// A built outbound message, ready to be SMTP-sent and APPEND-archived.
/// Holding both the parsed lettre Message and its raw RFC 5322 bytes
/// avoids re-formatting between the two operations.
pub struct OutboundMessage {
    pub message: LettreMessage,
    pub raw: Vec<u8>,
}

/// Build a reply to `inbound`, plain-text body `body`. Sets the threading
/// headers (In-Reply-To, References), the Re: subject prefix (without
/// double-stacking), and `Auto-Submitted: auto-replied` so well-behaved
/// auto-responders on the other side know not to reply to us.
pub fn build_reply(
    config: &EmailConfig,
    inbound: &ParsedMessage,
    body: &str,
) -> Result<OutboundMessage, BuildError> {
    let from: Mailbox = format!("{} <{}>", config.display_name, config.address)
        .parse()
        .map_err(|e: lettre::address::AddressError| BuildError::Address(e.to_string()))?;
    let to: Mailbox = inbound
        .from_raw
        .parse()
        .or_else(|_| inbound.from_address.parse())
        .map_err(|e: lettre::address::AddressError| BuildError::Address(e.to_string()))?;

    let subject = re_prefixed(&inbound.subject);
    let message_id = generate_message_id(&config.address);

    let references = match &inbound.references_raw {
        Some(refs) => format!("{} {}", refs.trim(), inbound.message_id),
        None => inbound.message_id.clone(),
    };

    // lettre's `.header()` only accepts types that implement the `Header`
    // trait. None of Auto-Submitted / In-Reply-To / References / Message-ID
    // ship as typed headers in lettre 0.11, so we define one-line wrappers
    // (see `RawMessageId`, `RawInReplyTo`, etc. below) and pass those.
    let message = LettreMessage::builder()
        .from(from)
        .to(to)
        .subject(subject)
        .header(RawMessageId(message_id.clone()))
        .header(RawInReplyTo(inbound.message_id.clone()))
        .header(RawReferences(references))
        .header(RawAutoSubmitted("auto-replied".to_string()))
        .body(body.to_string())
        .map_err(|e| BuildError::Build(e.to_string()))?;

    let raw = message.formatted();

    debug!(
        to = %inbound.from_address,
        subject = %inbound.subject,
        message_id = %message_id,
        bytes = raw.len(),
        "email: built reply"
    );

    Ok(OutboundMessage { message, raw })
}

/// Submit `outbound` over SMTPS to the configured server. Returns the
/// SMTP response on success.
pub async fn send_smtp(config: &EmailConfig, outbound: &OutboundMessage) -> Result<(), SendError> {
    let creds = Credentials::new(config.address.clone(), config.password.clone());

    let tls_params = TlsParameters::new(config.smtp_host.clone())
        .map_err(|e| SendError::Tls(e.to_string()))?;

    // builder_dangerous is the no-default-TLS constructor — we then add
    // TLS explicitly via the wrapper. The "dangerous" naming is about
    // not assuming TLS is on; we're being explicit about TLS being on,
    // which is the correct path for implicit-TLS port 465.
    let mailer: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&config.smtp_host)
            .port(config.smtp_port)
            .tls(Tls::Wrapper(tls_params))
            .credentials(creds)
            .build();

    let response = mailer
        .send(outbound.message.clone())
        .await
        .map_err(|e| SendError::Smtp(e.to_string()))?;

    debug!(
        code = ?response.code(),
        "email: SMTP send accepted"
    );

    Ok(())
}

/// Open a one-shot IMAP session, APPEND the raw outbound bytes to the
/// Sent folder, and log out. Tries `Sent` first, falls back to
/// `INBOX.Sent` and `Sent Items` for servers that use those names.
pub async fn append_to_sent(
    config: &EmailConfig,
    outbound: &OutboundMessage,
) -> Result<(), SendError> {
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let tls_config = build_tls_config();
    let connector = TlsConnector::from(Arc::new(tls_config));

    let tcp = TcpStream::connect((config.imap_host.as_str(), config.imap_port))
        .await
        .map_err(|e| SendError::Imap(format!("connect: {e}")))?;

    let server_name = ServerName::try_from(config.imap_host.clone())
        .map_err(|_| SendError::Imap("invalid server name".into()))?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(|e| SendError::Imap(format!("tls: {e}")))?;

    let mut client = Client::new(tls);
    let _ = client
        .read_response()
        .await
        .ok_or_else(|| SendError::Imap("no greeting".into()))?;

    let mut session = client
        .login(&config.address, &config.password)
        .await
        .map_err(|(e, _)| SendError::Imap(format!("login: {e}")))?;

    let mut last_err: Option<String> = None;
    for folder in ["Sent", "INBOX.Sent", "Sent Items"] {
        // append(mailbox, flags, datetime, body) — we don't need flags
        // (\Seen would be wrong for an outbound copy in some clients) or
        // explicit datetime (server uses INTERNALDATE = now, which is
        // what we want).
        match session
            .append(folder, None::<&str>, None::<&str>, &outbound.raw)
            .await
        {
            Ok(_) => {
                debug!(folder, bytes = outbound.raw.len(), "email: APPEND to Sent ok");
                last_err = None;
                break;
            }
            Err(e) => {
                debug!(folder, %e, "email: APPEND failed, trying next folder");
                last_err = Some(e.to_string());
            }
        }
    }

    let _ = session.logout().await;

    if let Some(e) = last_err {
        return Err(SendError::Imap(format!("append: {e}")));
    }
    Ok(())
}

// One-line wrappers around lettre's `Header` trait so we can set the four
// raw RFC 5322 headers we need (Message-ID, In-Reply-To, References,
// Auto-Submitted). lettre 0.11 ships typed headers for things like From,
// To, Subject, ContentType, etc., but not for these — and `.header()` on
// `MessageBuilder` requires the typed `Header` trait, not a raw HeaderValue.
//
// `parse` is a no-op for our purposes (we never round-trip through
// lettre's parser; we always construct these ourselves) but the trait
// requires it. `display()` is what gets serialized into the message.
macro_rules! raw_header_struct {
    ($name:ident, $header_name:literal) => {
        #[derive(Debug, Clone)]
        struct $name(String);

        impl Header for $name {
            fn name() -> HeaderName {
                HeaderName::new_from_ascii_str($header_name)
            }
            fn parse(s: &str) -> Result<Self, Box<dyn StdError + Send + Sync>> {
                Ok(Self(s.to_string()))
            }
            fn display(&self) -> HeaderValue {
                HeaderValue::new(Self::name(), self.0.clone())
            }
        }
    };
}

raw_header_struct!(RawMessageId, "Message-ID");
raw_header_struct!(RawInReplyTo, "In-Reply-To");
raw_header_struct!(RawReferences, "References");
raw_header_struct!(RawAutoSubmitted, "Auto-Submitted");

/// Generate an RFC 5322 Message-ID rooted at the bot's domain. Uses a
/// UUID for uniqueness — the bot is single-threaded per reply on a per-
/// thread basis, so collision is theoretical, but it's free to be safe.
fn generate_message_id(address: &str) -> String {
    let domain = address.split('@').nth(1).unwrap_or("local");
    format!("<{}@{}>", Uuid::new_v4().simple(), domain)
}

/// Add a `Re: ` prefix unless the subject already has one. Case-insensitive.
/// Multiple stacked `Re: Re: Re:` prefixes are NOT collapsed — that's a
/// mail-client choice and we shouldn't second-guess it (some users care
/// about depth, others don't, mailing-list software cares a lot).
fn re_prefixed(subject: &str) -> String {
    let trimmed = subject.trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("re:") {
        subject.to_string()
    } else {
        format!("Re: {trimmed}")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("invalid email address: {0}")]
    Address(String),
    #[error("message build error: {0}")]
    Build(String),
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
    #[error("SMTP error: {0}")]
    Smtp(String),
    #[error("TLS configuration error: {0}")]
    Tls(String),
    #[error("IMAP APPEND error: {0}")]
    Imap(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn re_prefix_added_when_missing() {
        assert_eq!(re_prefixed("hello"), "Re: hello");
    }

    #[test]
    fn re_prefix_not_double_stacked() {
        assert_eq!(re_prefixed("Re: hello"), "Re: hello");
        assert_eq!(re_prefixed("re: hello"), "re: hello");
        assert_eq!(re_prefixed("RE: hello"), "RE: hello");
    }

    #[test]
    fn re_prefix_keeps_existing_chain() {
        // Don't try to be clever about Re: Re: Re: stacks. Whatever the
        // sender did, preserve it.
        assert_eq!(re_prefixed("Re: Re: hello"), "Re: Re: hello");
    }

    #[test]
    fn generate_message_id_uses_address_domain() {
        let id = generate_message_id("bot@example.com");
        assert!(id.starts_with('<'));
        assert!(id.ends_with("@example.com>"));
    }

    #[test]
    fn generate_message_id_falls_back_to_local() {
        let id = generate_message_id("malformed");
        assert!(id.ends_with("@local>"));
    }
}
