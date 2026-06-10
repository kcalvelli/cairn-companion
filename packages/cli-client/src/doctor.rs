//! `companion doctor` — one-screen health check across every companion
//! surface.
//!
//! Most checks are client-side (HTTP probes, filesystem, persona
//! resolution); only the daemon-state checks need D-Bus. When the daemon is
//! down that becomes the headline failure, but the daemon-independent checks
//! still run — which is exactly when you want them. The daemon is a data
//! source here, never the orchestrator: a daemon-side implementation
//! couldn't report on its own absence.

use std::collections::BTreeMap;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::dbus::CompanionProxy;

/// Per-spoke / per-gateway probe timeout. Short and per-target so one dead
/// fleet peer can't stall the whole report.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// The daemon's `GetHealth` reply: `(channels, gateway_enabled,
/// gateway_bind, gateway_port)` where each channel is
/// `(name, state, last_error)`.
type HealthReply = (Vec<(String, String, String)>, bool, String, u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    /// Human glyph + label, optionally ANSI-colored.
    fn render(self, color: bool) -> String {
        let (glyph, label, ansi) = match self {
            Status::Ok => ("✓", "OK", "32"),    // green
            Status::Warn => ("!", "WARN", "33"), // yellow
            Status::Fail => ("✗", "FAIL", "31"), // red
            Status::Skip => ("-", "SKIP", "90"), // bright-black
        };
        if color {
            format!("\x1b[{ansi}m{glyph} {label}\x1b[0m")
        } else {
            format!("{glyph} {label}")
        }
    }
}

/// One check result. Checks may nest sub-results (per channel, per spoke).
#[derive(Serialize)]
struct Check {
    id: String,
    status: Status,
    detail: String,
    #[serde(skip_serializing_if = "serde_json::Map::is_empty")]
    fields: serde_json::Map<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<Check>,
}

impl Check {
    fn new(id: impl Into<String>, status: Status, detail: impl Into<String>) -> Self {
        Check {
            id: id.into(),
            status,
            detail: detail.into(),
            fields: serde_json::Map::new(),
            children: Vec::new(),
        }
    }

    fn field(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.fields.insert(key.to_string(), value.into());
        self
    }

    /// The worst status anywhere in this check's subtree. A parent with OK
    /// status but a FAIL child counts as FAIL for exit-code purposes.
    fn worst(&self) -> Status {
        let mut worst = self.status;
        for child in &self.children {
            let c = child.worst();
            if rank(&c) > rank(&worst) {
                worst = c;
            }
        }
        worst
    }
}

/// Severity ordering for aggregation. Skip and Warn never fail the run.
fn rank(s: &Status) -> u8 {
    match s {
        Status::Skip => 0,
        Status::Ok => 1,
        Status::Warn => 2,
        Status::Fail => 3,
    }
}

/// Entry point. Returns the process exit code: non-zero iff any check
/// anywhere reported FAIL (WARN and SKIP do not fail the run).
pub async fn run(json: bool) -> i32 {
    // Connect to the session bus and probe the daemon once. Everything that
    // needs the daemon shares this proxy; everything that doesn't ignores it.
    let proxy = crate::dbus::connect().await.ok();
    let status_map = match &proxy {
        Some(p) => p.get_status().await.ok(),
        None => None,
    };
    let daemon_up = status_map.is_some();
    let health = if daemon_up {
        proxy.as_ref().unwrap().get_health().await.ok()
    } else {
        None
    };

    let mut checks = Vec::new();
    checks.push(check_daemon(&status_map));
    checks.push(check_sessions(proxy.as_ref(), daemon_up).await);
    checks.push(check_channels(&health, daemon_up));
    checks.push(check_gateway(&health, daemon_up).await);
    checks.push(check_spokes().await);
    checks.push(check_workspace());
    checks.push(check_persona());

    let exit = if checks.iter().any(|c| c.worst() == Status::Fail) {
        1
    } else {
        0
    };

    if json {
        // Single document; exit-code contract identical to human mode.
        println!(
            "{}",
            serde_json::to_string_pretty(&checks).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
        );
    } else {
        render_human(&checks);
    }

    exit
}

fn render_human(checks: &[Check]) {
    let color = std::io::stdout().is_terminal();
    for check in checks {
        println!(
            "{}  {}  {}",
            check.status.render(color),
            check.id,
            check.detail
        );
        for child in &check.children {
            println!(
                "    {}  {}  {}",
                child.status.render(color),
                child.id,
                child.detail
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Individual checks
// ---------------------------------------------------------------------------

/// Daemon liveness + identity. The daemon owning `org.cairn.Companion` and
/// answering `GetStatus` is the precondition for every daemon-dependent
/// check below.
fn check_daemon(
    status_map: &Option<std::collections::HashMap<String, zbus::zvariant::OwnedValue>>,
) -> Check {
    match status_map {
        Some(map) => {
            let version = map
                .get("version")
                .and_then(|v| <&str>::try_from(v).ok())
                .unwrap_or("unknown")
                .to_string();
            let uptime = map
                .get("uptime_seconds")
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0);
            Check::new(
                "daemon",
                Status::Ok,
                format!("companion-core v{version}, up {}", fmt_duration(uptime)),
            )
            .field("version", version)
            .field("uptime_seconds", uptime)
        }
        None => Check::new(
            "daemon",
            Status::Fail,
            "not reachable on org.cairn.Companion — is companion-core running? (systemctl --user status companion-core)",
        ),
    }
}

/// Session store: confirm the daemon can enumerate sessions.
async fn check_sessions(proxy: Option<&CompanionProxy<'_>>, daemon_up: bool) -> Check {
    if !daemon_up {
        return Check::new("sessions", Status::Skip, "daemon down");
    }
    let proxy = proxy.expect("daemon_up implies proxy");
    match proxy.list_sessions().await {
        Ok(sessions) => {
            let active = sessions.iter().filter(|s| s.3 == "active").count();
            Check::new(
                "sessions",
                Status::Ok,
                format!("{} session(s), {active} active", sessions.len()),
            )
            .field("total", sessions.len() as u64)
            .field("active", active as u64)
        }
        Err(e) => Check::new("sessions", Status::Fail, format!("session query failed: {e}")),
    }
}

/// Channel adapters: one sub-result per enabled channel.
/// connected→OK, reconnecting→WARN, down→FAIL. Disabled channels are absent
/// from the registry and so never appear.
fn check_channels(
    health: &Option<HealthReply>,
    daemon_up: bool,
) -> Check {
    if !daemon_up {
        return Check::new("channels", Status::Skip, "daemon down");
    }
    let channels = match health {
        Some((channels, _, _, _)) => channels,
        None => return Check::new("channels", Status::Skip, "daemon did not report health"),
    };
    if channels.is_empty() {
        return Check::new("channels", Status::Ok, "no channel adapters enabled");
    }

    let mut parent = Check::new("channels", Status::Ok, format!("{} enabled", channels.len()));
    for (name, state, last_error) in channels {
        let status = match state.as_str() {
            "connected" => Status::Ok,
            "reconnecting" => Status::Warn,
            _ => Status::Fail, // "down" or anything unexpected
        };
        let detail = if last_error.is_empty() {
            state.clone()
        } else {
            format!("{state} — {last_error}")
        };
        let mut child = Check::new(name.clone(), status, detail).field("state", state.clone());
        if !last_error.is_empty() {
            child = child.field("last_error", last_error.clone());
        }
        parent.children.push(child);
    }
    // Parent glyph reflects the worst channel so the summary line isn't
    // green over red children.
    parent.status = parent.worst();
    parent
}

/// OpenAI gateway: probe `/health` when enabled. SKIP (not FAIL) when
/// disabled. The daemon tells us bind/port; the probe is ours.
async fn check_gateway(
    health: &Option<HealthReply>,
    daemon_up: bool,
) -> Check {
    if !daemon_up {
        return Check::new("gateway", Status::Skip, "daemon down");
    }
    let (enabled, bind, port) = match health {
        Some((_, enabled, bind, port)) => (*enabled, bind.clone(), *port),
        None => return Check::new("gateway", Status::Skip, "daemon did not report health"),
    };
    if !enabled {
        return Check::new("gateway", Status::Skip, "gateway disabled");
    }

    // A 0.0.0.0 bind isn't a connectable address — probe loopback.
    let host = if bind.is_empty() || bind == "0.0.0.0" {
        "127.0.0.1".to_string()
    } else {
        bind
    };
    let url = format!("http://{host}:{port}/health");
    match probe_get_json(&url).await {
        Ok(body) if body.get("status").and_then(|v| v.as_str()) == Some("ok") => {
            Check::new("gateway", Status::Ok, format!("{host}:{port} healthy")).field("url", url)
        }
        Ok(_) => Check::new(
            "gateway",
            Status::Fail,
            format!("{host}:{port} answered but not {{\"status\":\"ok\"}}"),
        ),
        Err(e) => Check::new("gateway", Status::Fail, format!("{url}: {e}")),
    }
}

/// Spoke reachability: probe each spoke in `spoke-servers.json` concurrently.
async fn check_spokes() -> Check {
    let path = match spoke_config_path() {
        Some(p) => p,
        None => return Check::new("spokes", Status::Skip, "no spoke-servers.json found"),
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(e) => {
            return Check::new(
                "spokes",
                Status::Fail,
                format!("{}: {e}", path.display()),
            )
        }
    };
    let config: SpokeConfig = match serde_json::from_str(&raw) {
        Ok(c) => c,
        Err(e) => {
            return Check::new(
                "spokes",
                Status::Fail,
                format!("{} is not valid JSON: {e}", path.display()),
            )
        }
    };
    if config.mcp_servers.is_empty() {
        return Check::new("spokes", Status::Skip, "spoke-servers.json has no spokes");
    }

    // Probe all spokes concurrently; total time bounded by the slowest one.
    let futures = config.mcp_servers.into_iter().map(|(name, entry)| async move {
        let url = entry.url.unwrap_or_default();
        if url.is_empty() {
            return Check::new(name, Status::Fail, "no url in spoke-servers.json");
        }
        let started = Instant::now();
        match probe_mcp(&url).await {
            Ok(()) => {
                let ms = started.elapsed().as_millis() as u64;
                Check::new(name, Status::Ok, format!("{url} ({ms}ms)"))
                    .field("url", url)
                    .field("latency_ms", ms)
            }
            Err(e) => Check::new(name, Status::Fail, format!("{url}: {e}")).field("url", url),
        }
    });
    let mut children = futures_util::future::join_all(futures).await;
    children.sort_by(|a, b| a.id.cmp(&b.id));

    let failed = children.iter().filter(|c| c.status == Status::Fail).count();
    let detail = if failed == 0 {
        format!("{} spoke(s) reachable", children.len())
    } else {
        format!("{}/{} spoke(s) unreachable", failed, children.len())
    };
    let mut parent = Check::new("spokes", Status::Ok, detail);
    parent.children = children;
    // Parent glyph reflects the worst spoke (a dead fleet peer is a FAIL).
    parent.status = parent.worst();
    parent
}

/// Workspace: the default `$XDG_DATA_HOME/cairn-companion/workspace` exists
/// and is writable. (A custom `workspaceDir` is not yet exposed by the
/// daemon — this checks the default location.)
fn check_workspace() -> Check {
    let dir = workspace_dir();
    if !dir.is_dir() {
        return Check::new(
            "workspace",
            Status::Fail,
            format!("{} does not exist", dir.display()),
        );
    }
    // Probe writability without leaving a turd behind.
    let probe = dir.join(".doctor-write-probe");
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Check::new("workspace", Status::Ok, dir.display().to_string())
                .field("path", dir.display().to_string())
        }
        Err(e) => Check::new(
            "workspace",
            Status::Fail,
            format!("{} not writable: {e}", dir.display()),
        ),
    }
}

/// Persona resolution: read the Tier 0 wrapper (`companion-code`) and verify
/// every baked persona file resolves on disk. This reads the wrapper's own
/// `PERSONA_PATHS` array, so it tests exactly what a real session loads —
/// catching the uncommitted-flake-source footgun at diagnosis time instead
/// of via a silently generic-voiced session.
fn check_persona() -> Check {
    let wrapper = match which("companion-code") {
        Some(p) => p,
        None => {
            return Check::new(
                "persona",
                Status::Skip,
                "companion-code not on PATH (Tier 0 wrapper not installed?)",
            )
        }
    };
    let script = match std::fs::read_to_string(&wrapper) {
        Ok(s) => s,
        Err(e) => {
            return Check::new(
                "persona",
                Status::Fail,
                format!("{}: {e}", wrapper.display()),
            )
        }
    };
    let paths = parse_persona_paths(&script);
    if paths.is_empty() {
        return Check::new(
            "persona",
            Status::Warn,
            "could not find PERSONA_PATHS in companion-code",
        );
    }
    let missing: Vec<&String> = paths.iter().filter(|p| !PathBuf::from(p).is_file()).collect();
    if missing.is_empty() {
        Check::new(
            "persona",
            Status::Ok,
            format!("{} persona file(s) resolve", paths.len()),
        )
        .field("count", paths.len() as u64)
    } else {
        let names: Vec<String> = missing.iter().map(|p| (*p).clone()).collect();
        Check::new(
            "persona",
            Status::Fail,
            format!("{} missing: {}", missing.len(), names.join(", ")),
        )
    }
}

// ---------------------------------------------------------------------------
// Probes and helpers
// ---------------------------------------------------------------------------

/// GET a URL expecting a small JSON body (gateway `/health`).
async fn probe_get_json(url: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client.get(url).send().await.map_err(short_err)?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    resp.json::<serde_json::Value>().await.map_err(|e| e.to_string())
}

/// POST an MCP `initialize` to a spoke's `/mcp` endpoint. A 2xx response
/// means the HTTP MCP handler is actually serving — deeper than a bare TCP
/// connect, which would pass even on a wedged handler. (We don't parse the
/// streamable-HTTP/SSE body; a served 2xx is sufficient signal here.)
async fn probe_mcp(url: &str) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "companion-doctor", "version": "0.1" }
        }
    });
    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        .json(&body)
        .send()
        .await
        .map_err(short_err)?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", resp.status().as_u16()))
    }
}

/// reqwest errors are verbose; collapse to the useful part.
fn short_err(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".to_string()
    } else if e.is_connect() {
        "connection refused".to_string()
    } else {
        e.to_string()
    }
}

#[derive(serde::Deserialize)]
struct SpokeConfig {
    #[serde(rename = "mcpServers", default)]
    mcp_servers: BTreeMap<String, SpokeEntry>,
}

#[derive(serde::Deserialize)]
struct SpokeEntry {
    #[serde(default)]
    url: Option<String>,
}

/// `$XDG_CONFIG_HOME/cairn-companion/spoke-servers.json`, falling back to
/// `$HOME/.config/...` — the same path the wrapper passes to Claude Code.
fn spoke_config_path() -> Option<PathBuf> {
    let path = config_home().join("cairn-companion").join("spoke-servers.json");
    path.is_file().then_some(path)
}

fn config_home() -> PathBuf {
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME") {
        if !x.is_empty() {
            return PathBuf::from(x);
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/root".into())).join(".config")
}

/// Default workspace, mirroring companion-core's main.rs derivation.
fn workspace_dir() -> PathBuf {
    let data = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
        format!("{home}/.local/share")
    });
    PathBuf::from(data).join("cairn-companion").join("workspace")
}

/// Find an executable by name on `$PATH`.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(dir).join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Extract the persona file paths from the wrapper's baked
/// `PERSONA_PATHS=( ... )` bash array. Each entry is a single-quoted path,
/// possibly with `'\''` escapes for embedded quotes.
fn parse_persona_paths(script: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_array = false;
    for line in script.lines() {
        let trimmed = line.trim();
        if !in_array {
            if trimmed.starts_with("PERSONA_PATHS=(") {
                in_array = true;
            }
            continue;
        }
        if trimmed == ")" {
            break;
        }
        if let Some(path) = unquote_bash(trimmed) {
            out.push(path);
        }
    }
    out
}

/// Strip the outer single quotes from a bash-escaped token and undo the
/// `'\''` escape sequence.
fn unquote_bash(token: &str) -> Option<String> {
    let token = token.trim();
    let inner = token.strip_prefix('\'')?.strip_suffix('\'')?;
    Some(inner.replace("'\\''", "'"))
}

fn fmt_duration(secs: u32) -> String {
    let d = secs / 86400;
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if d > 0 {
        format!("{d}d{h}h")
    } else if h > 0 {
        format!("{h}h{m}m")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worst_status_bubbles_up_from_children() {
        let mut parent = Check::new("channels", Status::Ok, "");
        parent.children.push(Check::new("telegram", Status::Ok, ""));
        parent.children.push(Check::new("email", Status::Fail, ""));
        assert_eq!(parent.worst(), Status::Fail);
    }

    #[test]
    fn warn_and_skip_never_outrank_to_fail() {
        let mut parent = Check::new("channels", Status::Ok, "");
        parent.children.push(Check::new("a", Status::Warn, ""));
        parent.children.push(Check::new("b", Status::Skip, ""));
        assert_eq!(parent.worst(), Status::Warn);
    }

    #[test]
    fn parse_persona_paths_extracts_quoted_entries() {
        let script = "\
HAS_USER_FILE=1
PERSONA_PATHS=(
  '/nix/store/abc-persona/AGENT.md'
  '/nix/store/def-persona/USER.md'
  '/home/keith/personas/sid/voice.md'
)
WORKSPACE=foo";
        let paths = parse_persona_paths(script);
        assert_eq!(paths.len(), 3);
        assert_eq!(paths[0], "/nix/store/abc-persona/AGENT.md");
        assert_eq!(paths[2], "/home/keith/personas/sid/voice.md");
    }

    #[test]
    fn parse_persona_paths_handles_embedded_quote_escape() {
        let script = "PERSONA_PATHS=(\n  '/path/o'\\''brien/USER.md'\n)";
        let paths = parse_persona_paths(script);
        assert_eq!(paths, vec!["/path/o'brien/USER.md".to_string()]);
    }

    #[test]
    fn parse_persona_paths_empty_when_absent() {
        assert!(parse_persona_paths("no array here").is_empty());
    }

    #[test]
    fn spoke_config_parses_url_map() {
        let raw = r#"{"mcpServers":{"companion-shell":{"type":"http","url":"http://localhost:18790/mcp"}}}"#;
        let cfg: SpokeConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(cfg.mcp_servers.len(), 1);
        assert_eq!(
            cfg.mcp_servers["companion-shell"].url.as_deref(),
            Some("http://localhost:18790/mcp")
        );
    }

    #[test]
    fn json_serialization_includes_fields_and_children() {
        let mut parent = Check::new("channels", Status::Ok, "1 enabled");
        parent
            .children
            .push(Check::new("telegram", Status::Ok, "connected").field("state", "connected"));
        let v = serde_json::to_value(&parent).unwrap();
        assert_eq!(v["id"], "channels");
        assert_eq!(v["status"], "ok");
        assert_eq!(v["children"][0]["fields"]["state"], "connected");
    }

    #[test]
    fn fmt_duration_scales() {
        assert_eq!(fmt_duration(45), "45s");
        assert_eq!(fmt_duration(150), "2m30s");
        assert_eq!(fmt_duration(3700), "1h1m");
        assert_eq!(fmt_duration(90000), "1d1h");
    }
}
