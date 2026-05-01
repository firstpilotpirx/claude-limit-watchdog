//! Watchdog component — primary & secondary adapters.
//!
//! All ports come from the sibling crate `clw-watchdog-core`. The dependency
//! direction (`adapters → core`) is enforced by Cargo: this crate has
//! `clw-watchdog-core` in its `[dependencies]`, the reverse does not exist.
//!
//! Layout (`<feature>/<tech>/`):
//!
//! ```text
//! primary/
//! ├── lifecycle/signal/         — Ctrl-C / SIGTERM stop signal
//! └── configsetup/stdio/        — interactive wizard
//!
//! secondary/
//! ├── clock/system/             — std::time-backed Clock
//! ├── panecontrol/tmux/         — tmux CLI Pane adapter
//! ├── userpresentation/stdio/   — terminal Presenter
//! ├── usagelog/filesystem/      — Claude Code *.jsonl log reader
//! └── settingspersistence/yaml/ — YAML SettingsRepository (DTO + mapper + adapter)
//! ```

pub mod primary;
pub mod secondary;
