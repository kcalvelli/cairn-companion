//! companion-mcp-apps — launch applications and URLs on the desktop.
//!
//! Two thin shell-outs:
//!   `open_url`              → xdg-open <url>
//!   `launch_desktop_entry`  → dex -a <name>
//!
//! Using `dex` rather than `gtk-launch` because gtk-launch only ships
//! inside the full gtk3 package (≈30 MB runtime closure for one
//! binary). `dex` is a tiny purpose-built freedesktop launcher that
//! also supports `-a <name>` for name-based lookup without the user
//! having to know the .desktop filename.
//!
//! Both tools are fire-and-forget. The tool returns as soon as the
//! launcher spawns the child; it does not wait for the application
//! to exit, to show a window, or anything else user-visible. The
//! child is detached from our stdio (stdout + stderr → null) so its
//! lifetime can't hold our JSON-RPC pipe open past MCP-server exit —
//! same lesson as wl-copy in the clipboard tool.

use anyhow::Result;
use companion_spoke::{err_text, ok_text, serve, tool_def, ToolHandler};
use serde_json::{json, Value};

struct Apps;

impl ToolHandler for Apps {
    fn server_name(&self) -> &'static str {
        "companion-apps"
    }

    fn tools(&self) -> Vec<Value> {
        vec![
            tool_def(
                "open_url",
                "Open a URL in the user's default browser (xdg-open). \
                 Fire-and-forget — returns as soon as the browser is \
                 spawned. Runs on the mcp-gateway host, so the URL opens \
                 on THAT host's display, not the caller's.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "The URL or file:// path to open."
                        }
                    },
                    "required": ["url"]
                }),
            ),
            tool_def(
                "launch_desktop_entry",
                "Launch a `.desktop` application entry by name (dex -a). \
                 Pass just the entry name — dex resolves it via the \
                 freedesktop XDG data dirs (~/.local/share/applications \
                 and /usr/share/applications). Fire-and-forget. Runs on \
                 the mcp-gateway host.",
                json!({
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string",
                            "description": "The .desktop entry name, e.g. \"firefox\" or \"com.mitchellh.ghostty\". No .desktop suffix, no path."
                        }
                    },
                    "required": ["name"]
                }),
            ),
        ]
    }

    async fn call(&self, name: &str, args: &Value) -> Value {
        match name {
            "open_url" => open_url(args).await,
            "launch_desktop_entry" => launch_desktop_entry(args).await,
            _ => err_text(format!("unknown tool: {name}")),
        }
    }
}

async fn open_url(args: &Value) -> Value {
    let url = match args.get("url").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return err_text("url is required and must be non-empty"),
    };

    // xdg-open forks and detaches the launched process; inheriting our
    // JSON-RPC stdio would keep the MCP pipe open past our exit. Same
    // bug that bit wl-copy in Phase 3.
    let status = match tokio::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
    {
        Ok(s) => s,
        Err(e) => return err_text(format!("failed to spawn xdg-open: {e}")),
    };

    if !status.success() {
        return err_text(format!(
            "xdg-open exited {}",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
        ));
    }

    ok_text(format!("Opened {url}."))
}

async fn launch_desktop_entry(args: &Value) -> Value {
    let name = match args.get("name").and_then(|v| v.as_str()) {
        Some(s) if !s.is_empty() => s,
        _ => return err_text("name is required and must be non-empty"),
    };

    // dex accepts either the bare name (with `-a`) or a full .desktop
    // path. Strip the suffix if Claude provided it so either form works.
    let entry = name.strip_suffix(".desktop").unwrap_or(name);

    let status = match tokio::process::Command::new("dex")
        .args(["-a", entry])
        .env("XDG_DATA_DIRS", nixos_xdg_data_dirs())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
    {
        Ok(s) => s,
        Err(e) => return err_text(format!("failed to spawn dex: {e}")),
    };

    if !status.success() {
        return err_text(format!(
            "dex could not launch \"{entry}\" (exit {}). \
             Check the entry name — dex looks for .desktop files in the \
             user's local applications dir and NixOS's per-user / system \
             profile share dirs.",
            status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".into())
        ));
    }

    ok_text(format!("Launched {entry}."))
}

/// Build an `XDG_DATA_DIRS` value that covers the NixOS canonical
/// locations for desktop entries:
///   - `$HOME/.local/share`              — user-level (XDG spec)
///   - `$HOME/.nix-profile/share`        — per-user nix profile
///   - `/etc/profiles/per-user/$USER/share` — NixOS per-user profile
///                                          (where user-installed
///                                          home-manager packages land)
///   - `/run/current-system/sw/share`    — NixOS system profile
///   - `/usr/local/share:/usr/share`     — freedesktop fallback
///
/// mcp-gateway's systemd unit does not set XDG_DATA_DIRS at all, so
/// dex would otherwise fall back to just `/usr/share:/usr/local/share`
/// — which on NixOS does not exist, and user-installed apps (ghostty,
/// firefox, whatever you `home.packages` into your profile) are
/// invisible. This function is the reason `launch_desktop_entry
/// ghostty` works on a NixOS box at all.
///
/// If the existing env already has a `XDG_DATA_DIRS` set (e.g.,
/// someone runs the tool interactively from a login shell), we prefer
/// that — their session probably knows better than we do. Otherwise
/// we construct the NixOS-style default.
fn nixos_xdg_data_dirs() -> String {
    if let Ok(existing) = std::env::var("XDG_DATA_DIRS") {
        if !existing.is_empty() {
            return existing;
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".into());
    let user = std::env::var("USER").unwrap_or_else(|_| "root".into());

    [
        format!("{home}/.local/share"),
        format!("{home}/.nix-profile/share"),
        format!("/etc/profiles/per-user/{user}/share"),
        "/run/current-system/sw/share".to_string(),
        "/usr/local/share".to_string(),
        "/usr/share".to_string(),
    ]
    .join(":")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    serve(Apps).await
}
