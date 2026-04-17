//! D-Bus proxy for org.cairn.Companion1.
//!
//! Client-side counterpart to companion-core's dbus.rs interface.
//! Generated proxy methods map 1:1 to the daemon's exported methods
//! and signals.

use zbus::Connection;

#[zbus::proxy(
    interface = "org.cairn.Companion1",
    default_service = "org.cairn.Companion",
    default_path = "/org/cairn/Companion"
)]
pub trait Companion {
    /// Send a message and block until the full response arrives.
    async fn send_message(
        &self,
        surface: &str,
        conversation_id: &str,
        message: &str,
    ) -> zbus::Result<String>;

    /// Send a message and return immediately. Response arrives via signals.
    async fn stream_message(
        &self,
        surface: &str,
        conversation_id: &str,
        message: &str,
    ) -> zbus::Result<()>;

    /// List all sessions: (surface, conversation_id, claude_session_id, status, last_active_at).
    async fn list_sessions(&self) -> zbus::Result<Vec<(String, String, String, String, u32)>>;

    /// Fetch one session's full details:
    /// (surface, conversation_id, claude_session_id, status, created_at, last_active_at, metadata).
    async fn get_session(
        &self,
        surface: &str,
        conversation_id: &str,
    ) -> zbus::Result<(String, String, String, String, u32, u32, String)>;

    /// Delete a session. Returns true if a row was removed.
    async fn delete_session(&self, surface: &str, conversation_id: &str) -> zbus::Result<bool>;

    /// Daemon status as a string-keyed property map.
    async fn get_status(
        &self,
    ) -> zbus::Result<std::collections::HashMap<String, zbus::zvariant::OwnedValue>>;

    /// Surfaces with at least one active session.
    async fn get_active_surfaces(&self) -> zbus::Result<Vec<String>>;

    /// Resolved path to Claude Code's project memory directory, or empty
    /// string if the directory doesn't exist yet.
    async fn get_memory_path(&self) -> zbus::Result<String>;

    /// Memory files: (filename, size_bytes, mtime_epoch).
    async fn list_memory_files(&self) -> zbus::Result<Vec<(String, u64, i64)>>;

    /// Read a single memory file by name.
    async fn read_memory_file(&self, name: &str) -> zbus::Result<String>;

    /// MEMORY.md index contents, or empty string if not yet created.
    async fn get_memory_index(&self) -> zbus::Result<String>;

    #[zbus(signal)]
    fn response_chunk(surface: &str, conversation_id: &str, chunk: &str);

    #[zbus(signal)]
    fn response_complete(surface: &str, conversation_id: &str, full_text: &str);

    #[zbus(signal)]
    fn response_error(surface: &str, conversation_id: &str, error: &str);
}

pub async fn connect() -> Result<CompanionProxy<'static>, zbus::Error> {
    let connection = Connection::session().await?;
    CompanionProxy::new(&connection).await
}
