//! YAML implementation of the [`SettingsRepository`] port.
//!
//! Layout:
//!
//! * [`dto`]        — `SettingsYamlDto` with `serde` derives (the wire format).
//! * [`mapper`]     — converts between `SettingsYamlDto` and `Settings` (domain VO).
//! * [`repository`] — `YamlSettingsRepository`: filesystem load/save with atomic
//!                    write (write-then-rename).
//! * [`paths`]      — host-derived defaults (`~/.claude-limit-watchdog/config.yaml`,
//!                    `~/.claude`, `~/.claude-personal`).
//!
//! [`SettingsRepository`]: clw_watchdog_core::application::ports::SettingsRepository

pub mod dto;
pub mod mapper;
pub mod paths;
pub mod repository;

pub use paths::{default_claude_dir, default_config_path, personal_claude_dir, PathError};
pub use repository::{to_yaml_string, YamlSettingsRepository};
