//! Driven ports — traits the infrastructure adapters implement.

use std::path::PathBuf;
use std::time::Duration;

use crate::application::settings::Settings;
use crate::domain::{ModelStats, SessionWindow};

/// Access to a named "pane" — the terminal-like surface where Claude Code is
/// running. The watchdog needs three operations on it: existence check,
/// recent-output capture, and key/text injection. Implemented by the tmux
/// adapter; could be implemented for other process surfaces that expose the
/// same shape (screen, kitty, etc.).
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait Pane: Send + Sync {
    fn exists(&self, name: &str) -> bool;
    fn capture(&self, name: &str, lines: u32) -> Result<String, PaneError>;
    fn send(&self, name: &str, text: &str) -> Result<(), PaneError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PaneError {
    #[error("pane '{0}' not found")]
    NotFound(String),
    #[error("pane operation failed: {0}")]
    OperationFailed(String),
}

/// Wall-clock + sleep abstraction so the watch loop is testable without real time.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait Clock: Send + Sync {
    /// Unix epoch seconds.
    fn now_epoch(&self) -> i64;
    fn sleep(&self, duration: Duration);
}

/// Cooperative cancellation flag. The CLI installs a Ctrl-C / SIGTERM handler
/// that flips this; the watch loop polls it on every tick.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait StopSignal: Send + Sync {
    fn should_stop(&self) -> bool;
}

/// Persistence port for user [`Settings`]. The composition root chooses a
/// concrete implementation (currently YAML-on-disk) and passes it where load /
/// save is needed.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait SettingsRepository: Send + Sync {
    fn load(&self) -> Result<Option<Settings>, SettingsRepositoryError>;
    fn save(&self, settings: &Settings) -> Result<(), SettingsRepositoryError>;
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsRepositoryError {
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("parse error in {path}: {reason}")]
    Parse { path: PathBuf, reason: String },
    #[error("serialize error: {0}")]
    Serialize(String),
}

/// Snapshot passed to [`Presenter::idle_tick`] each second while idle.
#[derive(Debug, Clone)]
pub struct IdleInfo {
    pub now_epoch: i64,
    pub last_poll_at: i64,
    pub started_at: i64,
    pub resume_count: u32,
    /// Current 5h session window. `None` until the first usage refresh, or if
    /// no active session exists right now (last activity older than 5h).
    pub session_window: Option<SessionWindow>,
    /// Per-model totals **inside** `session_window`. Empty until first refresh.
    pub session_stats: Vec<ModelStats>,
}

/// Configuration snapshot passed to [`Presenter::banner`] at start-up.
#[derive(Debug, Clone)]
pub struct BannerInfo {
    pub session: String,
    pub version: String,
    pub poll_interval_seconds: u64,
    pub buffer_seconds: u64,
    pub limit_phrase: String,
    pub resume_text: String,
}

/// Presentation port — separated so domain/application don't depend on terminal IO.
#[cfg_attr(any(test, feature = "test-support"), mockall::automock)]
pub trait Presenter: Send + Sync {
    fn banner(&self, info: &BannerInfo);
    fn started(&self);
    fn idle_tick(&self, info: &IdleInfo);
    fn limit_detected(&self, target_human: &str, wait_seconds: i64, buffer_seconds: i64);
    fn limit_already_passed(&self, target_human: &str);
    fn countdown_step(&self, remaining_seconds: i64, target_human: &str);
    fn resumed(&self, count: u32, resume_text: &str, session: &str);
    fn shutdown(&self, uptime_seconds: i64, total_resumes: u32);
    fn warn(&self, message: &str);
    fn error(&self, message: &str);
}
