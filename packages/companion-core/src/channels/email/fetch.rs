//! IMAP connect, login, fetch, and flag helpers.
//!
//! All TLS goes through `tokio-rustls` with the Mozilla root CA bundle from
//! `webpki-roots`. We deliberately do not reuse the xmpp connector's
//! no-verify path — public mail servers always have valid certs and we
//! want the real verifier so a misconfigured deployment fails loudly
//! instead of silently trusting an attacker.

use std::sync::Arc;

use async_imap::Client;
use futures::TryStreamExt;
use tokio::net::TcpStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;
use tracing::debug;

use super::config::EmailConfig;
use super::EmailError;

/// Concrete session type with TLS already terminated. Pinning the type
/// here keeps it out of mod.rs (which would otherwise need to thread the
/// generic everywhere).
pub type ImapSession = async_imap::Session<tokio_rustls::client::TlsStream<TcpStream>>;

/// One unseen message pulled from the inbox.
pub struct RawMessage {
    pub uid: u32,
    pub body: Vec<u8>,
}

/// Connect to the configured IMAP host, complete the TLS handshake,
/// authenticate, and SELECT the inbox. Returns a ready-to-poll session.
pub async fn connect_and_login(config: &EmailConfig) -> Result<ImapSession, EmailError> {
    // rustls 0.23+ requires a crypto provider be installed before any
    // ClientConfig is built. The xmpp adapter installs aws_lc_rs at
    // startup; this call is a no-op if it's already installed.
    let _ = tokio_rustls::rustls::crypto::aws_lc_rs::default_provider().install_default();

    let tls_config = build_tls_config();
    let connector = TlsConnector::from(Arc::new(tls_config));

    let addr = (config.imap_host.as_str(), config.imap_port);
    let tcp = TcpStream::connect(addr).await.map_err(EmailError::Connect)?;
    // TCP_NODELAY: IMAP commands are small and reply latency dominates.
    let _ = tcp.set_nodelay(true);

    let server_name = ServerName::try_from(config.imap_host.clone())
        .map_err(|_| EmailError::InvalidServerName(config.imap_host.clone()))?;
    let tls = connector
        .connect(server_name, tcp)
        .await
        .map_err(EmailError::Tls)?;

    let mut client = Client::new(tls);
    // Read the server greeting before issuing LOGIN. async-imap requires
    // this — without it, the first command races the unsolicited OK and
    // the parser gets unhappy.
    let _ = client
        .read_response()
        .await
        .ok_or_else(|| EmailError::Protocol("IMAP server closed before greeting".into()))?;

    let mut session = client
        .login(&config.address, &config.password)
        .await
        .map_err(|(e, _)| EmailError::Login(e.to_string()))?;

    session
        .select("INBOX")
        .await
        .map_err(|e| EmailError::Protocol(format!("SELECT INBOX failed: {e}")))?;

    Ok(session)
}

/// Build a rustls `ClientConfig` that trusts the Mozilla root CAs from
/// webpki-roots and enforces standard certificate verification.
pub fn build_tls_config() -> ClientConfig {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth()
}

/// SEARCH for unseen messages, then FETCH each one's body.peek + uid.
/// Returns them oldest-first (the order the IMAP server returned them,
/// which for SEARCH UNSEEN is ascending UID).
pub async fn fetch_unseen(session: &mut ImapSession) -> Result<Vec<RawMessage>, EmailError> {
    let uids = session
        .uid_search("UNSEEN")
        .await
        .map_err(|e| EmailError::Protocol(format!("UID SEARCH failed: {e}")))?;

    if uids.is_empty() {
        return Ok(Vec::new());
    }

    let mut sorted: Vec<u32> = uids.into_iter().collect();
    sorted.sort_unstable();

    let uid_set = sorted
        .iter()
        .map(|u| u.to_string())
        .collect::<Vec<_>>()
        .join(",");

    debug!(count = sorted.len(), "email: fetching unseen UIDs");

    // BODY.PEEK[] avoids implicitly setting \Seen — we mark seen
    // explicitly only after handle_message succeeds.
    let mut stream = session
        .uid_fetch(&uid_set, "(UID BODY.PEEK[])")
        .await
        .map_err(|e| EmailError::Protocol(format!("UID FETCH failed: {e}")))?;

    let mut out: Vec<RawMessage> = Vec::with_capacity(sorted.len());
    while let Some(fetch) = stream
        .try_next()
        .await
        .map_err(|e| EmailError::Protocol(format!("FETCH stream error: {e}")))?
    {
        let uid = match fetch.uid {
            Some(u) => u,
            None => continue,
        };
        let body = match fetch.body() {
            Some(b) => b.to_vec(),
            None => continue,
        };
        out.push(RawMessage { uid, body });
    }

    Ok(out)
}

/// Mark a single UID as seen via `UID STORE +FLAGS (\Seen)`. Drains the
/// response stream so subsequent commands don't trip on leftover data.
pub async fn mark_seen(session: &mut ImapSession, uid: u32) -> Result<(), EmailError> {
    let mut stream = session
        .uid_store(uid.to_string(), "+FLAGS (\\Seen)")
        .await
        .map_err(|e| EmailError::Protocol(format!("UID STORE failed: {e}")))?;

    while let Some(_resp) = stream
        .try_next()
        .await
        .map_err(|e| EmailError::Protocol(format!("STORE stream error: {e}")))?
    {}

    Ok(())
}
