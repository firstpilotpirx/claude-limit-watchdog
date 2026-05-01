//! Wire DTO for the on-disk YAML config — owns `serde` derives so the domain
//! [`Settings`] stays framework-free.
//!
//! Field names appear verbatim in `config.yaml`; keep them stable.
//!
//! [`Settings`]: clw_watchdog_core::application::Settings

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsYamlDto {
    pub claude_dir: PathBuf,
    pub poll_interval_secs: u64,
    pub buffer_secs: u64,
    pub pane_lines: u32,
    pub limit_phrase: String,
    pub resume_text: String,
}
