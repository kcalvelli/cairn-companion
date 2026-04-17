//! Application state and update logic for companion-tui.

/// Which panel has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Sessions,
    Conversation,
    Memory,
}

/// Per-session conversation buffer.
#[derive(Debug, Clone)]
pub struct ConversationBuffer {
    pub surface: String,
    pub conversation_id: String,
    /// Accumulated text from chunks.
    pub text: String,
    /// Whether the current turn has completed.
    pub turn_complete: bool,
}

/// Daemon status snapshot from get_status().
#[derive(Debug, Clone, Default)]
pub struct DaemonStatus {
    pub version: String,
    pub uptime_seconds: u32,
    pub active_sessions: u32,
    pub in_flight_turns: u32,
}

/// A session row from list_sessions().
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub surface: String,
    pub conversation_id: String,
    pub claude_session_id: String,
    pub status: String,
    pub last_active_at: u32,
}

/// A memory file entry from the daemon.
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub name: String,
    pub size: u64,
    pub mtime: i64,
}

/// Top-level application state.
pub struct App {
    pub running: bool,
    pub connected: bool,
    pub focus: Focus,
    pub show_help: bool,

    // Sessions panel state.
    pub sessions: Vec<SessionRow>,
    pub selected_session: usize,

    // Status bar state.
    pub daemon_status: DaemonStatus,

    // Conversation panel state — keyed by (surface, conversation_id).
    pub conversations: Vec<ConversationBuffer>,
    /// Scroll offset from the bottom of the conversation view.
    pub conversation_scroll: u16,

    // Memory panel state.
    pub memory_files: Vec<MemoryEntry>,
    pub selected_memory: usize,
    pub memory_content: String,
    pub memory_scroll: u16,

    /// Errors or status messages shown briefly.
    pub flash_message: Option<String>,
}

impl App {
    pub fn new() -> Self {
        Self {
            running: true,
            connected: false,
            focus: Focus::Sessions,
            show_help: false,
            sessions: Vec::new(),
            selected_session: 0,
            daemon_status: DaemonStatus::default(),
            conversations: Vec::new(),
            conversation_scroll: 0,
            memory_files: Vec::new(),
            selected_memory: 0,
            memory_content: String::new(),
            memory_scroll: 0,
            flash_message: None,
        }
    }

    /// The currently selected session, if any.
    pub fn selected_session_key(&self) -> Option<(&str, &str)> {
        self.sessions
            .get(self.selected_session)
            .map(|s| (s.surface.as_str(), s.conversation_id.as_str()))
    }

    /// Get or create a conversation buffer for the given session.
    pub fn conversation_buf_mut(
        &mut self,
        surface: &str,
        conversation_id: &str,
    ) -> &mut ConversationBuffer {
        let pos = self
            .conversations
            .iter()
            .position(|c| c.surface == surface && c.conversation_id == conversation_id);

        match pos {
            Some(i) => &mut self.conversations[i],
            None => {
                self.conversations.push(ConversationBuffer {
                    surface: surface.to_string(),
                    conversation_id: conversation_id.to_string(),
                    text: String::new(),
                    turn_complete: false,
                });
                self.conversations.last_mut().unwrap()
            }
        }
    }

    /// Move session selection up.
    pub fn select_prev(&mut self) {
        if self.selected_session > 0 {
            self.selected_session -= 1;
            self.conversation_scroll = 0;
        }
    }

    /// Move session selection down.
    pub fn select_next(&mut self) {
        if !self.sessions.is_empty() && self.selected_session < self.sessions.len() - 1 {
            self.selected_session += 1;
            self.conversation_scroll = 0;
        }
    }

    /// Scroll conversation up (towards older text).
    pub fn scroll_up(&mut self) {
        self.conversation_scroll = self.conversation_scroll.saturating_add(1);
    }

    /// Scroll conversation down (towards newer text).
    pub fn scroll_down(&mut self) {
        self.conversation_scroll = self.conversation_scroll.saturating_sub(1);
    }

    /// Jump to bottom of conversation.
    pub fn scroll_bottom(&mut self) {
        self.conversation_scroll = 0;
    }

    /// Cycle focus through panels.
    pub fn toggle_focus(&mut self) {
        self.focus = match self.focus {
            Focus::Sessions => Focus::Conversation,
            Focus::Conversation => Focus::Memory,
            Focus::Memory => Focus::Sessions,
        };
    }

    /// Move memory file selection up.
    pub fn select_memory_prev(&mut self) {
        if self.selected_memory > 0 {
            self.selected_memory -= 1;
            self.memory_scroll = 0;
        }
    }

    /// Move memory file selection down.
    pub fn select_memory_next(&mut self) {
        if !self.memory_files.is_empty() && self.selected_memory < self.memory_files.len() - 1 {
            self.selected_memory += 1;
            self.memory_scroll = 0;
        }
    }

    /// Update memory file list. Preserves selection if possible.
    pub fn update_memory_files(&mut self, files: Vec<MemoryEntry>) {
        let prev_name = self.memory_files.get(self.selected_memory).map(|e| e.name.clone());
        self.memory_files = files;
        if let Some(ref name) = prev_name {
            if let Some(idx) = self.memory_files.iter().position(|e| e.name == *name) {
                self.selected_memory = idx;
            } else {
                self.selected_memory = self.selected_memory.min(
                    self.memory_files.len().saturating_sub(1),
                );
            }
        }
    }

    /// The currently selected memory file name, if any.
    pub fn selected_memory_name(&self) -> Option<&str> {
        self.memory_files.get(self.selected_memory).map(|e| e.name.as_str())
    }

    /// Update sessions list from daemon. Preserves selection if possible.
    pub fn update_sessions(&mut self, rows: Vec<SessionRow>) {
        let prev_key = self.selected_session_key().map(|(s, c)| (s.to_string(), c.to_string()));

        self.sessions = rows;

        // Try to re-select the previously selected session.
        if let Some((prev_surface, prev_conv)) = prev_key {
            if let Some(idx) = self
                .sessions
                .iter()
                .position(|s| s.surface == prev_surface && s.conversation_id == prev_conv)
            {
                self.selected_session = idx;
            } else {
                self.selected_session = self.selected_session.min(
                    self.sessions.len().saturating_sub(1),
                );
            }
        }
    }
}

#[cfg(test)]
fn make_session(surface: &str, conv_id: &str) -> SessionRow {
    SessionRow {
        surface: surface.to_string(),
        conversation_id: conv_id.to_string(),
        claude_session_id: String::new(),
        status: "active".to_string(),
        last_active_at: 1000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_app_defaults() {
        let app = App::new();
        assert!(app.running);
        assert!(!app.connected);
        assert_eq!(app.focus, Focus::Sessions);
        assert!(!app.show_help);
        assert!(app.sessions.is_empty());
        assert_eq!(app.selected_session, 0);
        assert_eq!(app.conversation_scroll, 0);
    }

    #[test]
    fn selected_session_key_empty() {
        let app = App::new();
        assert!(app.selected_session_key().is_none());
    }

    #[test]
    fn selected_session_key_returns_current() {
        let mut app = App::new();
        app.sessions = vec![make_session("cli", "abc-123")];
        let key = app.selected_session_key().unwrap();
        assert_eq!(key, ("cli", "abc-123"));
    }

    #[test]
    fn select_next_and_prev() {
        let mut app = App::new();
        app.sessions = vec![
            make_session("cli", "a"),
            make_session("cli", "b"),
            make_session("cli", "c"),
        ];

        assert_eq!(app.selected_session, 0);
        app.select_next();
        assert_eq!(app.selected_session, 1);
        app.select_next();
        assert_eq!(app.selected_session, 2);
        // Can't go past the end.
        app.select_next();
        assert_eq!(app.selected_session, 2);

        app.select_prev();
        assert_eq!(app.selected_session, 1);
        app.select_prev();
        assert_eq!(app.selected_session, 0);
        // Can't go below zero.
        app.select_prev();
        assert_eq!(app.selected_session, 0);
    }

    #[test]
    fn select_next_on_empty_is_noop() {
        let mut app = App::new();
        app.select_next();
        assert_eq!(app.selected_session, 0);
    }

    #[test]
    fn selection_resets_scroll() {
        let mut app = App::new();
        app.sessions = vec![make_session("cli", "a"), make_session("cli", "b")];
        app.conversation_scroll = 5;
        app.select_next();
        assert_eq!(app.conversation_scroll, 0);
    }

    #[test]
    fn scroll_up_down_bottom() {
        let mut app = App::new();
        assert_eq!(app.conversation_scroll, 0);

        app.scroll_up();
        assert_eq!(app.conversation_scroll, 1);
        app.scroll_up();
        assert_eq!(app.conversation_scroll, 2);

        app.scroll_down();
        assert_eq!(app.conversation_scroll, 1);

        app.scroll_bottom();
        assert_eq!(app.conversation_scroll, 0);

        // scroll_down at 0 stays at 0.
        app.scroll_down();
        assert_eq!(app.conversation_scroll, 0);
    }

    #[test]
    fn toggle_focus_cycles() {
        let mut app = App::new();
        assert_eq!(app.focus, Focus::Sessions);
        app.toggle_focus();
        assert_eq!(app.focus, Focus::Conversation);
        app.toggle_focus();
        assert_eq!(app.focus, Focus::Memory);
        app.toggle_focus();
        assert_eq!(app.focus, Focus::Sessions);
    }

    #[test]
    fn conversation_buf_creates_on_first_access() {
        let mut app = App::new();
        assert!(app.conversations.is_empty());

        let buf = app.conversation_buf_mut("cli", "conv-1");
        buf.text.push_str("hello");

        assert_eq!(app.conversations.len(), 1);
        assert_eq!(app.conversations[0].surface, "cli");
        assert_eq!(app.conversations[0].text, "hello");
    }

    #[test]
    fn conversation_buf_reuses_existing() {
        let mut app = App::new();

        app.conversation_buf_mut("cli", "conv-1").text.push_str("first");
        app.conversation_buf_mut("cli", "conv-1").text.push_str(" second");

        assert_eq!(app.conversations.len(), 1);
        assert_eq!(app.conversations[0].text, "first second");
    }

    #[test]
    fn conversation_buf_separates_sessions() {
        let mut app = App::new();

        app.conversation_buf_mut("cli", "a").text.push_str("one");
        app.conversation_buf_mut("cli", "b").text.push_str("two");

        assert_eq!(app.conversations.len(), 2);
    }

    #[test]
    fn update_sessions_preserves_selection() {
        let mut app = App::new();
        app.sessions = vec![
            make_session("cli", "a"),
            make_session("cli", "b"),
            make_session("cli", "c"),
        ];
        app.selected_session = 1; // selected "b"

        // New list has the same sessions in different order.
        app.update_sessions(vec![
            make_session("cli", "c"),
            make_session("cli", "a"),
            make_session("cli", "b"),
        ]);

        // Should still be pointing at "b", now at index 2.
        assert_eq!(app.selected_session, 2);
        assert_eq!(app.selected_session_key().unwrap(), ("cli", "b"));
    }

    #[test]
    fn update_sessions_clamps_when_selected_removed() {
        let mut app = App::new();
        app.sessions = vec![
            make_session("cli", "a"),
            make_session("cli", "b"),
            make_session("cli", "c"),
        ];
        app.selected_session = 2; // selected "c"

        // "c" is gone.
        app.update_sessions(vec![
            make_session("cli", "a"),
            make_session("cli", "b"),
        ]);

        // Should clamp to last valid index.
        assert!(app.selected_session <= 1);
    }

    #[test]
    fn update_sessions_empty_list() {
        let mut app = App::new();
        app.sessions = vec![make_session("cli", "a")];
        app.selected_session = 0;

        app.update_sessions(vec![]);

        assert_eq!(app.selected_session, 0);
        assert!(app.sessions.is_empty());
    }
}
