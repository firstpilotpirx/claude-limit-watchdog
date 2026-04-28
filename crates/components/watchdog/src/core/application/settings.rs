//! User-facing settings persisted to disk (YAML).
//!
//! Pure data — no IO, no defaults that depend on the host (those live in the
//! secondary adapter that picks `~/.claude` on first run). The struct is
//! `serde`-serializable; the on-disk format is the source of truth.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::watch_service::WatchConfig;

/// Persisted user configuration.
///
/// Keep field names stable — they appear verbatim in `config.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Claude Code working directory (the one that contains `projects/`).
    /// Typically `~/.claude` or `~/.claude-personal`.
    pub claude_dir: PathBuf,

    /// How often to scan the tmux pane for a limit message.
    pub poll_interval_secs: u64,

    /// Extra wait after the announced reset time before sending resume.
    pub buffer_secs: u64,

    /// Number of trailing pane lines to capture each poll.
    pub pane_lines: u32,

    /// Substring that marks "limit hit" lines in the pane.
    pub limit_phrase: String,

    /// Text sent to the tmux pane when resuming.
    pub resume_text: String,
}

impl Settings {
    /// Built-in defaults. The wizard pre-fills its fields from these on first run.
    /// `claude_dir` is left to the caller because the right default depends on
    /// `$HOME`, which the application layer doesn't read.
    #[must_use]
    pub fn defaults_with_claude_dir(claude_dir: PathBuf) -> Self {
        Self {
            claude_dir,
            poll_interval_secs: 60,
            buffer_secs: 60,
            pane_lines: 200,
            limit_phrase: "You've hit your limit".to_string(),
            resume_text: "continue the work where you left off".to_string(),
        }
    }

    /// Build the runtime [`WatchConfig`] from persisted settings + a session name.
    #[must_use]
    pub fn into_watch_config(self, session: String) -> WatchConfig {
        WatchConfig {
            session,
            poll_interval: Duration::from_secs(self.poll_interval_secs),
            buffer: Duration::from_secs(self.buffer_secs),
            limit_phrase: self.limit_phrase,
            resume_text: self.resume_text,
            pane_lines: self.pane_lines,
        }
    }
}
