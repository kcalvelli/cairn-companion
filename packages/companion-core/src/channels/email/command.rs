//! Bang commands (`!new`, `!status`, `!help`) for the email channel.
//!
//! Same set as xmpp's slash/bang commands. Email-specific quirk: there is
//! no streaming, no edit-in-place — the command's reply text is just
//! returned to the caller, which packs it into a single SMTP message.
//! That keeps `command::handle` synchronous-ish (no `out_tx` plumbing,
//! no message types) and the test surface smaller.

use crate::dispatcher::Dispatcher;

use crate::channels::util::format_timestamp;

/// Handle a bang command. Returns the reply text the caller should send.
/// Unrecognized commands return a deflection rather than falling through
/// to the dispatcher — same defense-in-depth as xmpp/telegram, prevents
/// typos like `!hep` from leaking into Claude as a one-token turn.
pub async fn handle(
    surface_id: &str,
    conversation_id: &str,
    text: &str,
    dispatcher: &Dispatcher,
) -> String {
    // Strip the leading `!` and pull just the command word. The rest of
    // the line (if any) is ignored — none of the commands take args.
    let cmd = text
        .trim_start_matches('!')
        .split_whitespace()
        .next()
        .unwrap_or("");

    match cmd {
        "new" => {
            let store = dispatcher.store().await;
            let had_session = store
                .delete_session(surface_id, conversation_id)
                .unwrap_or(false);
            drop(store);

            if had_session {
                "Fine. Whatever we just talked about? Gone. Hope it wasn't important.".into()
            } else {
                "There's nothing to forget. We haven't even started yet.".into()
            }
        }
        "status" => {
            let store = dispatcher.store().await;
            let session = store
                .lookup_session(surface_id, conversation_id)
                .ok()
                .flatten();
            drop(store);

            match session {
                Some(s) => {
                    let claude_id = s.claude_session_id.as_deref().unwrap_or("(not yet assigned)");
                    format!(
                        "Session active.\nClaude session: {}\nLast active: {}",
                        claude_id,
                        format_timestamp(s.last_active_at),
                    )
                }
                None => "No active session for this thread. Reply to start one.".into(),
            }
        }
        "help" => "\
!new — drop the session for this thread, start fresh on the next message
!status — show the session info for this thread
!help — this message

Anything else goes straight to the companion."
            .into(),
        _ => "Not a command. Try !help if you're lost.".into(),
    }
}
