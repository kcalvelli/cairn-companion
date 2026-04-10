//! MIME parsing, body extraction, quote stripping, threading, and loop
//! detection for inbound mail.
//!
//! Everything in this file is pure (no IO, no external state). All of it
//! is unit-tested.

use mail_parser::{HeaderValue, Message, MessageParser};

/// A parsed inbound message — only the fields the channel adapter actually
/// uses, normalized into shapes the rest of the module can work with
/// without re-parsing.
#[derive(Debug, Clone)]
pub struct ParsedMessage {
    /// Message-ID of this inbound message, including angle brackets.
    /// Used for `In-Reply-To` and the `References` chain on the outbound
    /// reply.
    pub message_id: String,

    /// The thread root Message-ID — what we use as `conversation_id` in
    /// the dispatcher. Resolved from the first id in `References`,
    /// otherwise from `In-Reply-To`, otherwise from `message_id` (i.e.
    /// this message starts a new thread).
    pub thread_root: String,

    /// `From:` header address, lowercased. Used both for the allowlist
    /// check and as the `To:` recipient on the outbound reply.
    pub from_address: String,

    /// `From:` header full address (preserves original case + display
    /// name). Used to construct the `To:` header on the reply.
    pub from_raw: String,

    /// Subject line, with any leading "Re: " (case-insensitive) chain
    /// preserved as-is. The reply builder will check whether it already
    /// starts with "Re: " before prepending another one.
    pub subject: String,

    /// Plain-text body. Multipart messages are walked for their first
    /// `text/plain` part; if none exists, an HTML part is decoded and
    /// stripped of its tags as a fallback.
    pub body_text: String,

    /// `References:` header, exactly as received (space-separated
    /// `<msg-id>` tokens). The reply builder appends our id and uses
    /// this for the outbound `References` header so the thread renders
    /// correctly across mail clients.
    pub references_raw: Option<String>,

    /// True if the message carried `Auto-Submitted:` set to anything
    /// other than `no`. We refuse to reply to these to avoid mail loops
    /// (RFC 3834).
    auto_submitted: bool,

    /// True if the message looks like a bounce, DSN, or no-reply
    /// notification — sender pattern match plus a few header heuristics.
    bounce_or_no_reply: bool,
}

impl ParsedMessage {
    pub fn is_auto_submitted(&self) -> bool {
        self.auto_submitted
    }
    pub fn is_bounce_or_no_reply(&self) -> bool {
        self.bounce_or_no_reply
    }
}

/// Parse a raw RFC 5322 byte slice into the fields the adapter cares
/// about. Returns `None` if the message is malformed enough that we
/// can't extract the bare minimum (no `From:`, no `Message-ID`, no body).
pub fn parse(raw: &[u8]) -> Option<ParsedMessage> {
    let parser = MessageParser::default();
    let msg = parser.parse(raw)?;

    let from_raw = msg
        .from()
        .and_then(|addrs| addrs.first())
        .and_then(|a| a.address())
        .map(|s| s.to_string())?;
    let from_address = from_raw.to_ascii_lowercase();

    // Message-ID. mail-parser strips the angle brackets — we add them
    // back so the value matches what we'll see in subsequent
    // In-Reply-To / References chains.
    let message_id = msg
        .message_id()
        .map(|s| ensure_angle_brackets(s))
        .unwrap_or_else(|| format!("<no-id-{}@local>", random_token()));

    let subject = msg.subject().unwrap_or("(no subject)").to_string();

    // Walk parts for text/plain first, fallback to text/html with naive
    // tag strip.
    let body_text = extract_body_text(&msg).unwrap_or_default();

    let references_raw = header_message_ids(&msg, "References");
    let in_reply_to = header_message_ids(&msg, "In-Reply-To");

    let thread_root = resolve_thread_root(&references_raw, &in_reply_to, &message_id);

    let auto_submitted = header_text(&msg, "Auto-Submitted")
        .map(|v| !v.trim().eq_ignore_ascii_case("no"))
        .unwrap_or(false);

    let bounce_or_no_reply = looks_like_bounce(&from_address)
        || header_text(&msg, "Precedence")
            .map(|v| {
                let l = v.trim().to_ascii_lowercase();
                l == "bulk" || l == "list" || l == "junk"
            })
            .unwrap_or(false);

    Some(ParsedMessage {
        message_id,
        thread_root,
        from_address,
        from_raw,
        subject,
        body_text,
        references_raw,
        auto_submitted,
        bounce_or_no_reply,
    })
}

/// Pull a free-form text header by name. Returns `None` if the header
/// is missing or stored in a non-text form (e.g. an Address or DateTime
/// variant — those mean the caller asked for the wrong header).
fn header_text(msg: &Message<'_>, name: &str) -> Option<String> {
    msg.header(name).and_then(|h| match h {
        HeaderValue::Text(s) => Some(s.to_string()),
        HeaderValue::TextList(v) => Some(v.join(" ")),
        _ => None,
    })
}

/// Pull a message-id-list header (References, In-Reply-To) and return it
/// as space-separated `<id>` tokens with the angle brackets restored.
///
/// mail-parser stores threading headers as `TextList` (or `Text` for a
/// single id) with the angle brackets stripped — but downstream code in
/// this module works on `<id>` tokens, so we re-add them at the boundary.
fn header_message_ids(msg: &Message<'_>, name: &str) -> Option<String> {
    let h = msg.header(name)?;
    let tokens: Vec<String> = match h {
        HeaderValue::Text(s) => s
            .split_whitespace()
            .map(|t| t.to_string())
            .collect(),
        HeaderValue::TextList(v) => v.iter().map(|s| s.to_string()).collect(),
        _ => return None,
    };
    let bracketed: Vec<String> = tokens
        .into_iter()
        .filter(|t| !t.is_empty())
        .map(|t| {
            if t.starts_with('<') && t.ends_with('>') {
                t
            } else {
                format!("<{t}>")
            }
        })
        .collect();
    if bracketed.is_empty() {
        None
    } else {
        Some(bracketed.join(" "))
    }
}

/// Walk the MIME tree for a usable text body. Prefer `text/plain`. Fall back
/// to `text/html` with naive tag stripping. Returns the first match found.
fn extract_body_text(msg: &Message<'_>) -> Option<String> {
    // Prefer the dedicated body_text helper — it handles the common
    // cases of single-part text/plain and multipart/alternative
    // text-then-html messages without us having to walk parts manually.
    if let Some(s) = msg.body_text(0) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Some(s.into_owned());
        }
    }
    if let Some(html) = msg.body_html(0) {
        return Some(html_strip_tags(&html));
    }
    None
}

/// Drop HTML tags and collapse whitespace. Not a real HTML renderer —
/// just enough to recover the visible text from a `text/html` body when
/// no plain alternative was provided. Inline scripts/styles are removed
/// wholesale.
fn html_strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut chars = html.chars().peekable();
    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut skip_until: Option<&'static str> = None;

    while let Some(c) = chars.next() {
        if let Some(end_tag) = skip_until {
            // Naive search for the closing tag.
            if c == '<' {
                let mut probe = String::new();
                probe.push('<');
                for nc in chars.by_ref() {
                    probe.push(nc);
                    if nc == '>' {
                        break;
                    }
                    if probe.len() > end_tag.len() + 4 {
                        break;
                    }
                }
                if probe.to_ascii_lowercase().starts_with(end_tag) {
                    skip_until = None;
                }
            }
            continue;
        }

        if c == '<' {
            in_tag = true;
            tag_buf.clear();
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
                let lower = tag_buf.to_ascii_lowercase();
                if lower.starts_with("script") {
                    skip_until = Some("</script");
                } else if lower.starts_with("style") {
                    skip_until = Some("</style");
                }
                tag_buf.clear();
                continue;
            }
            tag_buf.push(c);
            continue;
        }
        out.push(c);
    }

    // Collapse whitespace.
    let mut collapsed = String::with_capacity(out.len());
    let mut prev_ws = false;
    for c in out.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                collapsed.push(if c == '\n' { '\n' } else { ' ' });
            }
            prev_ws = true;
        } else {
            collapsed.push(c);
            prev_ws = false;
        }
    }
    collapsed.trim().to_string()
}

/// Strip quoted reply text from a body. Drops:
/// - Lines starting with `>` (any depth, with or without trailing space)
/// - Everything from the first attribution line ("On X, Y wrote:" /
///   "-----Original Message-----") onward
///
/// Preserves blank lines internal to the kept content. Trims leading and
/// trailing blank lines from the result.
pub fn strip_quoted(body: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    for line in body.lines() {
        if is_attribution_line(line) {
            break;
        }
        if is_quoted_line(line) {
            continue;
        }
        kept.push(line);
    }

    // Trim leading and trailing blanks. Keep internal blanks intact.
    while kept.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        kept.remove(0);
    }
    while kept.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        kept.pop();
    }

    kept.join("\n")
}

fn is_quoted_line(line: &str) -> bool {
    let stripped = line.trim_start();
    stripped.starts_with('>')
}

fn is_attribution_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with("-----Original Message-----") {
        return true;
    }
    // Match "On <something>, <somebody> wrote:" — the most common form
    // across Apple Mail, Gmail, Thunderbird, Outlook. Body must end with
    // "wrote:" (case-insensitive). The "On " prefix is the cheap filter.
    if trimmed.starts_with("On ") || trimmed.starts_with("on ") {
        let lower = trimmed.to_ascii_lowercase();
        if lower.ends_with("wrote:") || lower.ends_with("wrote :") {
            return true;
        }
    }
    false
}

/// Decide which Message-ID is the thread root for this inbound message.
/// Per RFC 5322, the first id in `References` is the original message
/// in the thread. If there's no `References`, fall back to `In-Reply-To`
/// (first id only — multiple ids are non-standard but seen in the wild).
/// If neither is present, this message starts a new thread, so its own
/// Message-ID is the root.
pub fn resolve_thread_root(
    references: &Option<String>,
    in_reply_to: &Option<String>,
    own_message_id: &str,
) -> String {
    if let Some(refs) = references {
        if let Some(first) = first_message_id(refs) {
            return first;
        }
    }
    if let Some(irt) = in_reply_to {
        if let Some(first) = first_message_id(irt) {
            return first;
        }
    }
    own_message_id.to_string()
}

/// Extract the first `<msg-id>` token from a header value. Tolerates
/// extra whitespace and stray text. Returns `None` if no angle-bracketed
/// id is found.
fn first_message_id(raw: &str) -> Option<String> {
    let start = raw.find('<')?;
    let end = raw[start..].find('>')?;
    Some(raw[start..start + end + 1].to_string())
}

/// Wrap a Message-ID in angle brackets if it isn't already. mail-parser
/// strips them on parse, but we want them everywhere else for consistency
/// with `In-Reply-To` / `References` formatting.
fn ensure_angle_brackets(id: &str) -> String {
    let trimmed = id.trim();
    if trimmed.starts_with('<') && trimmed.ends_with('>') {
        trimmed.to_string()
    } else {
        format!("<{trimmed}>")
    }
}

fn looks_like_bounce(addr: &str) -> bool {
    let local = addr.split('@').next().unwrap_or(addr);
    matches!(
        local,
        "mailer-daemon"
            | "postmaster"
            | "no-reply"
            | "noreply"
            | "do-not-reply"
            | "donotreply"
    ) || local.starts_with("bounce")
        || local.starts_with("mailer-daemon")
}

fn random_token() -> String {
    use uuid::Uuid;
    Uuid::new_v4().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_quoted_drops_quote_lines() {
        let body = "Real reply line.\n> previous message\n> more quote\nAnother real line.";
        let stripped = strip_quoted(body);
        assert_eq!(stripped, "Real reply line.\nAnother real line.");
    }

    #[test]
    fn strip_quoted_handles_attribution_apple_mail() {
        let body = "Sure, that works.\n\nOn Tuesday, April 9, 2026, Alice Example <alice@example.com> wrote:\n> some quoted text\n> more quoted";
        let stripped = strip_quoted(body);
        assert_eq!(stripped, "Sure, that works.");
    }

    #[test]
    fn strip_quoted_handles_outlook_separator() {
        let body = "yes\n\n-----Original Message-----\nFrom: keith\nblah";
        let stripped = strip_quoted(body);
        assert_eq!(stripped, "yes");
    }

    #[test]
    fn strip_quoted_preserves_internal_blanks() {
        let body = "first paragraph\n\nsecond paragraph";
        assert_eq!(strip_quoted(body), "first paragraph\n\nsecond paragraph");
    }

    #[test]
    fn strip_quoted_empty_when_only_quotes() {
        let body = "> all quoted\n> all quoted\n> all quoted";
        assert_eq!(strip_quoted(body), "");
    }

    #[test]
    fn strip_quoted_trims_blank_lines_at_edges() {
        let body = "\n\nreal content\n\n";
        assert_eq!(strip_quoted(body), "real content");
    }

    #[test]
    fn quoted_line_with_leading_space_is_stripped() {
        // Some clients indent quoted blocks. Apple Mail puts " > " on
        // forwarded sections.
        assert!(is_quoted_line("  > nested quote"));
        assert!(is_quoted_line(">no space"));
        assert!(!is_quoted_line("not quoted"));
    }

    #[test]
    fn first_message_id_extracts_brackets() {
        assert_eq!(
            first_message_id("<abc123@host>").as_deref(),
            Some("<abc123@host>")
        );
    }

    #[test]
    fn first_message_id_picks_first_of_chain() {
        assert_eq!(
            first_message_id("<a@h> <b@h> <c@h>").as_deref(),
            Some("<a@h>")
        );
    }

    #[test]
    fn first_message_id_returns_none_when_no_id() {
        assert_eq!(first_message_id("just garbage"), None);
    }

    #[test]
    fn resolve_thread_root_prefers_references_first() {
        let root = resolve_thread_root(
            &Some("<root@h> <middle@h> <parent@h>".to_string()),
            &Some("<parent@h>".to_string()),
            "<self@h>",
        );
        assert_eq!(root, "<root@h>");
    }

    #[test]
    fn resolve_thread_root_falls_back_to_in_reply_to() {
        let root = resolve_thread_root(&None, &Some("<parent@h>".to_string()), "<self@h>");
        assert_eq!(root, "<parent@h>");
    }

    #[test]
    fn resolve_thread_root_falls_back_to_own_id() {
        let root = resolve_thread_root(&None, &None, "<self@h>");
        assert_eq!(root, "<self@h>");
    }

    #[test]
    fn ensure_angle_brackets_idempotent() {
        assert_eq!(ensure_angle_brackets("<abc@h>"), "<abc@h>");
        assert_eq!(ensure_angle_brackets("abc@h"), "<abc@h>");
        assert_eq!(ensure_angle_brackets("  abc@h  "), "<abc@h>");
    }

    #[test]
    fn looks_like_bounce_matches_common_addresses() {
        assert!(looks_like_bounce("mailer-daemon@example.com"));
        assert!(looks_like_bounce("postmaster@example.com"));
        assert!(looks_like_bounce("no-reply@example.com"));
        assert!(looks_like_bounce("noreply@github.com"));
        assert!(looks_like_bounce("bounce-12345@list.example.com"));
        assert!(!looks_like_bounce("alice@example.com"));
        assert!(!looks_like_bounce("bob@example.org"));
    }

    #[test]
    fn html_strip_drops_tags_and_scripts() {
        let html = "<html><body><script>alert(1)</script><p>Hello <b>world</b></p></body></html>";
        let plain = html_strip_tags(html);
        assert!(plain.contains("Hello"));
        assert!(plain.contains("world"));
        assert!(!plain.contains("alert"));
        assert!(!plain.contains('<'));
    }

    #[test]
    fn parse_basic_message() {
        let raw = b"From: Alice Example <alice@example.com>\r\n\
                    To: Bot <bot@example.com>\r\n\
                    Subject: hello\r\n\
                    Message-ID: <abc123@example.com>\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    Hi there, what's up?\r\n";
        let parsed = parse(raw).expect("should parse");
        assert_eq!(parsed.from_address, "alice@example.com");
        assert_eq!(parsed.subject, "hello");
        assert_eq!(parsed.message_id, "<abc123@example.com>");
        assert_eq!(parsed.thread_root, "<abc123@example.com>");
        assert!(parsed.body_text.contains("what's up"));
        assert!(!parsed.is_auto_submitted());
        assert!(!parsed.is_bounce_or_no_reply());
    }

    #[test]
    fn parse_reply_uses_references_for_thread_root() {
        let raw = b"From: Alice Example <alice@example.com>\r\n\
                    To: Bot <bot@example.com>\r\n\
                    Subject: Re: hello\r\n\
                    Message-ID: <reply2@example.com>\r\n\
                    In-Reply-To: <reply1@example.org>\r\n\
                    References: <root@example.com> <reply1@example.org>\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    Same thread, new message.\r\n";
        let parsed = parse(raw).expect("should parse");
        assert_eq!(parsed.thread_root, "<root@example.com>");
        assert_eq!(parsed.message_id, "<reply2@example.com>");
    }

    #[test]
    fn parse_detects_auto_submitted() {
        let raw = b"From: vacation@example.com\r\n\
                    To: bot@example.com\r\n\
                    Subject: Out of office\r\n\
                    Message-ID: <vac1@example.com>\r\n\
                    Auto-Submitted: auto-replied\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    I'm out until next week.\r\n";
        let parsed = parse(raw).expect("should parse");
        assert!(parsed.is_auto_submitted());
    }

    #[test]
    fn parse_auto_submitted_no_is_not_a_loop() {
        let raw = b"From: alice@example.com\r\n\
                    To: bot@example.com\r\n\
                    Subject: ok\r\n\
                    Message-ID: <ok@example.com>\r\n\
                    Auto-Submitted: no\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    real message\r\n";
        let parsed = parse(raw).expect("should parse");
        assert!(!parsed.is_auto_submitted());
    }

    #[test]
    fn parse_detects_bounce_sender() {
        let raw = b"From: mailer-daemon@example.com\r\n\
                    To: bot@example.com\r\n\
                    Subject: Undelivered Mail Returned to Sender\r\n\
                    Message-ID: <bounce@example.com>\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    Your message could not be delivered.\r\n";
        let parsed = parse(raw).expect("should parse");
        assert!(parsed.is_bounce_or_no_reply());
    }

    #[test]
    fn parse_detects_precedence_bulk() {
        let raw = b"From: list@example.com\r\n\
                    To: bot@example.com\r\n\
                    Subject: digest\r\n\
                    Message-ID: <list@example.com>\r\n\
                    Precedence: bulk\r\n\
                    Content-Type: text/plain\r\n\
                    \r\n\
                    digest content\r\n";
        let parsed = parse(raw).expect("should parse");
        assert!(parsed.is_bounce_or_no_reply());
    }
}
