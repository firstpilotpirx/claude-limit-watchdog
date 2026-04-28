//! Claude Code rate-limit watchdog component.
//!
//! Hexagonal layout:
//!
//! * [`core::domain`]      — pure types & functions, zero IO.
//! * [`core::application`] — ports (traits) + use cases that orchestrate the domain.
//! * [`adapters::primary`]   — input adapters: Ctrl-C handler, interactive wizard.
//! * [`adapters::secondary`] — output adapters: tmux, terminal presenter, filesystem.
//!
//! The composition root (binary) lives in `crates/apps/cli` and wires concrete
//! adapters into [`core::application::WatchService`].

pub mod adapters;
pub mod core;

// ---- Re-exports for the composition root ----

pub use crate::adapters::primary::ctrlc::CtrlCStop;
pub use crate::adapters::primary::wizard;
pub use crate::adapters::secondary::clock::SystemClock;
pub use crate::adapters::secondary::presenter::TerminalPresenter;
pub use crate::adapters::secondary::settings_store;
pub use crate::adapters::secondary::settings_store::{
    default_claude_dir, default_config_path, personal_claude_dir, SettingsStoreError,
};
pub use crate::adapters::secondary::tmux::TmuxCli;
pub use crate::adapters::secondary::usage_log::ClaudeCodeLogReader;
pub use crate::core::application::ports::Presenter;
pub use crate::core::application::settings::Settings;
pub use crate::core::application::usage_report::{
    UsageLogReader, UsageReadError, UsageReport, UsageReportService,
};
pub use crate::core::application::watch_service::{
    RunStats, WatchConfig, WatchError, WatchService,
};
