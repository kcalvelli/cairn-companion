//! Per-surface model overrides read from `COMPANION_MODEL_*` env vars.
//!
//! Resolution order, applied by the dispatcher:
//!   1. `req.model` if `Some(x)` — explicit caller override always wins.
//!   2. Per-surface override, keyed by `req.surface_id`.
//!   3. Daemon-wide default (`COMPANION_MODEL_DEFAULT`).
//!   4. `None` — fall through to whatever Claude Code's own default resolves
//!      to (typically Opus on a default install).
//!
//! Nothing here validates the model id against Anthropic's catalog. The
//! claude subprocess fails fast on a bogus `--model` arg and that's a
//! better surface for the error than re-implementing the catalog here.

use std::env;

/// One env-var bucket worth of model overrides. Construct via `from_env()`.
#[derive(Debug, Clone, Default)]
pub struct ModelConfig {
    pub default: Option<String>,
    pub openai: Option<String>,
    pub discord: Option<String>,
    pub email: Option<String>,
    pub telegram: Option<String>,
    pub xmpp: Option<String>,
}

impl ModelConfig {
    /// Read every `COMPANION_MODEL_*` env var. Missing or empty → `None`.
    pub fn from_env() -> Self {
        Self {
            default: read_env("COMPANION_MODEL_DEFAULT"),
            openai: read_env("COMPANION_MODEL_OPENAI"),
            discord: read_env("COMPANION_MODEL_DISCORD"),
            email: read_env("COMPANION_MODEL_EMAIL"),
            telegram: read_env("COMPANION_MODEL_TELEGRAM"),
            xmpp: read_env("COMPANION_MODEL_XMPP"),
        }
    }

    /// Resolve the model for a given surface_id. Returns `None` when no
    /// override applies, leaving the dispatcher to omit `--model` entirely
    /// and let Claude Code's own default win.
    ///
    /// Unknown surface ids (D-Bus callers passing arbitrary strings, future
    /// channels not yet wired into the match) fall through to the daemon
    /// default. This is intentional — adding `--model` to the daemon-wide
    /// fallback for a CLI turn from Keith is the right behavior; the only
    /// way to opt out is to leave `default` unset.
    pub fn for_surface(&self, surface_id: &str) -> Option<String> {
        let per_surface = match surface_id {
            "openai" => self.openai.as_deref(),
            "discord" => self.discord.as_deref(),
            "email" => self.email.as_deref(),
            "telegram" => self.telegram.as_deref(),
            "xmpp" => self.xmpp.as_deref(),
            _ => None,
        };
        per_surface.or(self.default.as_deref()).map(String::from)
    }
}

fn read_env(key: &str) -> Option<String> {
    match env::var(key) {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(default: Option<&str>, per_surface: &[(&str, &str)]) -> ModelConfig {
        let mut c = ModelConfig {
            default: default.map(String::from),
            ..Default::default()
        };
        for (surface, model) in per_surface {
            match *surface {
                "openai" => c.openai = Some((*model).into()),
                "discord" => c.discord = Some((*model).into()),
                "email" => c.email = Some((*model).into()),
                "telegram" => c.telegram = Some((*model).into()),
                "xmpp" => c.xmpp = Some((*model).into()),
                other => panic!("unknown surface id in test fixture: {other}"),
            }
        }
        c
    }

    #[test]
    fn nothing_set_returns_none_for_every_surface() {
        let c = ModelConfig::default();
        for s in ["openai", "discord", "email", "telegram", "xmpp", "dbus", "what"] {
            assert_eq!(c.for_surface(s), None, "surface {s} should resolve to None");
        }
    }

    #[test]
    fn default_only_applies_to_every_surface_including_unknowns() {
        let c = cfg(Some("haiku"), &[]);
        for s in ["openai", "discord", "email", "telegram", "xmpp", "dbus", "what"] {
            assert_eq!(
                c.for_surface(s).as_deref(),
                Some("haiku"),
                "surface {s} should fall back to default"
            );
        }
    }

    #[test]
    fn per_surface_only_isolates_to_that_surface() {
        let c = cfg(None, &[("email", "haiku")]);
        assert_eq!(c.for_surface("email").as_deref(), Some("haiku"));
        for s in ["openai", "discord", "telegram", "xmpp", "dbus"] {
            assert_eq!(c.for_surface(s), None, "surface {s} should not pick up email's override");
        }
    }

    #[test]
    fn per_surface_wins_over_default() {
        let c = cfg(Some("opus"), &[("email", "haiku")]);
        assert_eq!(c.for_surface("email").as_deref(), Some("haiku"));
        assert_eq!(c.for_surface("discord").as_deref(), Some("opus"));
        assert_eq!(c.for_surface("dbus").as_deref(), Some("opus"));
    }

    #[test]
    fn unknown_surface_falls_through_to_default() {
        let c = cfg(Some("opus"), &[("email", "haiku")]);
        assert_eq!(c.for_surface("dbus").as_deref(), Some("opus"));
        assert_eq!(c.for_surface("cli").as_deref(), Some("opus"));
        assert_eq!(c.for_surface("").as_deref(), Some("opus"));
    }

    #[test]
    fn every_known_surface_has_a_match_arm() {
        // Belt-and-suspenders against a future channel author adding an
        // override field without wiring the match arm. If this test
        // starts failing, somebody added a public field to ModelConfig
        // and forgot to surface it. Add the arm in `for_surface`.
        let c = cfg(None, &[
            ("openai", "M1"),
            ("discord", "M2"),
            ("email", "M3"),
            ("telegram", "M4"),
            ("xmpp", "M5"),
        ]);
        assert_eq!(c.for_surface("openai").as_deref(), Some("M1"));
        assert_eq!(c.for_surface("discord").as_deref(), Some("M2"));
        assert_eq!(c.for_surface("email").as_deref(), Some("M3"));
        assert_eq!(c.for_surface("telegram").as_deref(), Some("M4"));
        assert_eq!(c.for_surface("xmpp").as_deref(), Some("M5"));
    }
}
