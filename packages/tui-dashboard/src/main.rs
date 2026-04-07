//! companion-tui — terminal dashboard for the axios-companion daemon.

mod app;
mod dbus;
mod ui;

use std::io;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use futures_util::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::time::interval;

use app::{App, DaemonStatus, Focus, SessionRow};
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_POLL: Duration = Duration::from_millis(50);

/// Events flowing through the main loop.
enum AppEvent {
    Key(event::KeyEvent),
    StatusUpdate(DaemonStatus),
    SessionsUpdate(Vec<SessionRow>),
    Chunk {
        surface: String,
        conversation_id: String,
        text: String,
    },
    TurnComplete {
        surface: String,
        conversation_id: String,
        full_text: String,
    },
    TurnError {
        surface: String,
        conversation_id: String,
        error: String,
    },
    Connected,
    Disconnected,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let mut app = App::new();

    // Terminal setup.
    enable_raw_mode()?;
    io::stdout().execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<AppEvent>();

    // Keyboard input task.
    let tx_keys = tx.clone();
    tokio::spawn(async move {
        loop {
            if event::poll(EVENT_POLL).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if tx_keys.send(AppEvent::Key(key)).is_err() {
                        break;
                    }
                }
            }
        }
    });

    // D-Bus poller + signal subscriber task.
    let tx_dbus = tx.clone();
    tokio::spawn(async move {
        loop {
            // Try to connect.
            match dbus::connect().await {
                Ok(proxy) => {
                    let _ = tx_dbus.send(AppEvent::Connected);

                    // Subscribe to signals before starting the poll loop.
                    let chunks_stream = proxy.receive_response_chunk().await;
                    let complete_stream = proxy.receive_response_complete().await;
                    let error_stream = proxy.receive_response_error().await;

                    let (mut chunks, mut completions, mut errors) =
                        match (chunks_stream, complete_stream, error_stream) {
                            (Ok(c), Ok(co), Ok(e)) => (c, co, e),
                            _ => {
                                let _ = tx_dbus.send(AppEvent::Disconnected);
                                tokio::time::sleep(POLL_INTERVAL).await;
                                continue;
                            }
                        };

                    let mut tick = interval(POLL_INTERVAL);
                    // Send initial tick immediately.
                    tick.tick().await;

                    loop {
                        tokio::select! {
                            _ = tick.tick() => {
                                // Poll status + sessions.
                                match proxy.get_status().await {
                                    Ok(map) => {
                                        let status = parse_status(&map);
                                        let _ = tx_dbus.send(AppEvent::StatusUpdate(status));
                                    }
                                    Err(_) => {
                                        let _ = tx_dbus.send(AppEvent::Disconnected);
                                        break;
                                    }
                                }
                                match proxy.list_sessions().await {
                                    Ok(rows) => {
                                        let sessions = rows
                                            .into_iter()
                                            .map(|(surface, conv, claude, status, ts)| SessionRow {
                                                surface,
                                                conversation_id: conv,
                                                claude_session_id: claude,
                                                status,
                                                last_active_at: ts,
                                            })
                                            .collect();
                                        let _ = tx_dbus.send(AppEvent::SessionsUpdate(sessions));
                                    }
                                    Err(_) => {
                                        let _ = tx_dbus.send(AppEvent::Disconnected);
                                        break;
                                    }
                                }
                            }
                            Some(signal) = chunks.next() => {
                                if let Ok(args) = signal.args() {
                                    let _ = tx_dbus.send(AppEvent::Chunk {
                                        surface: args.surface.to_string(),
                                        conversation_id: args.conversation_id.to_string(),
                                        text: args.chunk.to_string(),
                                    });
                                }
                            }
                            Some(signal) = completions.next() => {
                                if let Ok(args) = signal.args() {
                                    let _ = tx_dbus.send(AppEvent::TurnComplete {
                                        surface: args.surface.to_string(),
                                        conversation_id: args.conversation_id.to_string(),
                                        full_text: args.full_text.to_string(),
                                    });
                                }
                            }
                            Some(signal) = errors.next() => {
                                if let Ok(args) = signal.args() {
                                    let _ = tx_dbus.send(AppEvent::TurnError {
                                        surface: args.surface.to_string(),
                                        conversation_id: args.conversation_id.to_string(),
                                        error: args.error.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }
                Err(_) => {
                    let _ = tx_dbus.send(AppEvent::Disconnected);
                    tokio::time::sleep(POLL_INTERVAL).await;
                }
            }
        }
    });

    // Main render + event loop.
    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        if let Some(event) = rx.recv().await {
            match event {
                AppEvent::Key(key) => handle_key(&mut app, key),
                AppEvent::Connected => {
                    app.connected = true;
                    app.flash_message = None;
                }
                AppEvent::Disconnected => {
                    app.connected = false;
                    app.daemon_status = DaemonStatus::default();
                    app.sessions.clear();
                }
                AppEvent::StatusUpdate(status) => {
                    app.daemon_status = status;
                }
                AppEvent::SessionsUpdate(sessions) => {
                    app.update_sessions(sessions);
                }
                AppEvent::Chunk {
                    surface,
                    conversation_id,
                    text,
                } => {
                    let buf = app.conversation_buf_mut(&surface, &conversation_id);
                    buf.text.push_str(&text);
                    buf.turn_complete = false;
                    // Auto-scroll to bottom when new chunks arrive (if already at bottom).
                    if app.conversation_scroll == 0 {
                        // Already at bottom — stays there.
                    }
                }
                AppEvent::TurnComplete {
                    surface,
                    conversation_id,
                    full_text,
                } => {
                    let buf = app.conversation_buf_mut(&surface, &conversation_id);
                    // If chunks were missed, set the full text.
                    if buf.text.is_empty() {
                        buf.text = full_text;
                    }
                    buf.text.push_str("\n\n---\n\n");
                    buf.turn_complete = true;
                }
                AppEvent::TurnError {
                    surface,
                    conversation_id,
                    error,
                } => {
                    let buf = app.conversation_buf_mut(&surface, &conversation_id);
                    buf.text.push_str(&format!("\n\n[error: {}]\n\n", error));
                    buf.turn_complete = true;
                }
            }
        }

        if !app.running {
            break;
        }
    }

    // Restore terminal and exit. process::exit kills the spawned tasks
    // that would otherwise block on crossterm's synchronous event::poll.
    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    std::process::exit(0);
}

fn handle_key(app: &mut App, key: event::KeyEvent) {
    // Global keys (always active).
    match key.code {
        KeyCode::Char('q') if !app.show_help => {
            app.running = false;
            return;
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.running = false;
            return;
        }
        KeyCode::Char('?') => {
            app.show_help = !app.show_help;
            return;
        }
        KeyCode::Esc if app.show_help => {
            app.show_help = false;
            return;
        }
        KeyCode::Tab => {
            app.toggle_focus();
            return;
        }
        KeyCode::Char('1') => {
            app.focus = Focus::Sessions;
            return;
        }
        KeyCode::Char('2') => {
            app.focus = Focus::Conversation;
            return;
        }
        _ => {}
    }

    if app.show_help {
        return; // Absorb all other keys when help is open.
    }

    // Panel-specific keys.
    match app.focus {
        Focus::Sessions => match key.code {
            KeyCode::Char('j') | KeyCode::Down => app.select_next(),
            KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
            KeyCode::Enter => app.focus = Focus::Conversation,
            _ => {}
        },
        Focus::Conversation => match key.code {
            KeyCode::Char('j') | KeyCode::Down => app.scroll_up(),
            KeyCode::Char('k') | KeyCode::Up => app.scroll_down(),
            KeyCode::Char('G') => app.scroll_bottom(),
            KeyCode::Char('g') => {
                // Would need double-tap for gg, but single g goes to top for now.
                app.conversation_scroll = u16::MAX;
            }
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    fn key_with_mod(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent {
            code,
            modifiers,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    #[test]
    fn q_quits() {
        let mut app = App::new();
        handle_key(&mut app, key(KeyCode::Char('q')));
        assert!(!app.running);
    }

    #[test]
    fn ctrl_c_quits() {
        let mut app = App::new();
        handle_key(&mut app, key_with_mod(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert!(!app.running);
    }

    #[test]
    fn q_does_not_quit_in_help() {
        let mut app = App::new();
        app.show_help = true;
        handle_key(&mut app, key(KeyCode::Char('q')));
        assert!(app.running); // Help absorbs it.
    }

    #[test]
    fn question_mark_toggles_help() {
        let mut app = App::new();
        assert!(!app.show_help);
        handle_key(&mut app, key(KeyCode::Char('?')));
        assert!(app.show_help);
        handle_key(&mut app, key(KeyCode::Char('?')));
        assert!(!app.show_help);
    }

    #[test]
    fn esc_closes_help() {
        let mut app = App::new();
        app.show_help = true;
        handle_key(&mut app, key(KeyCode::Esc));
        assert!(!app.show_help);
    }

    #[test]
    fn tab_toggles_focus() {
        let mut app = App::new();
        assert_eq!(app.focus, Focus::Sessions);
        handle_key(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Conversation);
        handle_key(&mut app, key(KeyCode::Tab));
        assert_eq!(app.focus, Focus::Sessions);
    }

    #[test]
    fn number_keys_set_focus() {
        let mut app = App::new();
        handle_key(&mut app, key(KeyCode::Char('2')));
        assert_eq!(app.focus, Focus::Conversation);
        handle_key(&mut app, key(KeyCode::Char('1')));
        assert_eq!(app.focus, Focus::Sessions);
    }

    #[test]
    fn j_k_navigate_sessions() {
        let mut app = App::new();
        app.sessions = vec![
            app::SessionRow {
                surface: "cli".into(),
                conversation_id: "a".into(),
                claude_session_id: String::new(),
                status: "active".into(),
                last_active_at: 1000,
            },
            app::SessionRow {
                surface: "cli".into(),
                conversation_id: "b".into(),
                claude_session_id: String::new(),
                status: "active".into(),
                last_active_at: 1000,
            },
        ];
        app.focus = Focus::Sessions;

        handle_key(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.selected_session, 1);
        handle_key(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.selected_session, 0);
    }

    #[test]
    fn enter_switches_to_conversation() {
        let mut app = App::new();
        app.focus = Focus::Sessions;
        handle_key(&mut app, key(KeyCode::Enter));
        assert_eq!(app.focus, Focus::Conversation);
    }

    #[test]
    fn j_k_scroll_in_conversation() {
        let mut app = App::new();
        app.focus = Focus::Conversation;
        handle_key(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.conversation_scroll, 1);
        handle_key(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.conversation_scroll, 0);
    }

    #[test]
    fn g_scrolls_to_top_and_shift_g_to_bottom() {
        let mut app = App::new();
        app.focus = Focus::Conversation;
        handle_key(&mut app, key(KeyCode::Char('g')));
        assert_eq!(app.conversation_scroll, u16::MAX);
        handle_key(&mut app, key(KeyCode::Char('G')));
        assert_eq!(app.conversation_scroll, 0);
    }

    #[test]
    fn help_absorbs_panel_keys() {
        let mut app = App::new();
        app.sessions = vec![app::SessionRow {
            surface: "cli".into(),
            conversation_id: "a".into(),
            claude_session_id: String::new(),
            status: "active".into(),
            last_active_at: 1000,
        }, app::SessionRow {
            surface: "cli".into(),
            conversation_id: "b".into(),
            claude_session_id: String::new(),
            status: "active".into(),
            last_active_at: 1000,
        }];
        app.show_help = true;
        app.focus = Focus::Sessions;

        handle_key(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.selected_session, 0); // Absorbed by help.
    }
}

fn parse_status(
    map: &std::collections::HashMap<String, zbus::zvariant::OwnedValue>,
) -> DaemonStatus {
    let get_u32 = |key: &str| -> u32 {
        map.get(key)
            .and_then(|v| <u32>::try_from(v).ok())
            .unwrap_or(0)
    };

    let version = map
        .get("version")
        .and_then(|v| <&str>::try_from(v).ok())
        .unwrap_or_default()
        .to_string();

    DaemonStatus {
        version,
        uptime_seconds: get_u32("uptime_seconds"),
        active_sessions: get_u32("active_sessions"),
        in_flight_turns: get_u32("in_flight_turns"),
    }
}
