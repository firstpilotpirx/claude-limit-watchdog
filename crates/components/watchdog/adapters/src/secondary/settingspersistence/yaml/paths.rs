//! Host-derived default paths used by the YAML settings adapter and offered
//! by the wizard.
//!
//! Kept in this adapter (not in `apps/cli/`) because they are part of the
//! YAML/`$HOME`-on-disk *technology* contract — same crate that knows the
//! file format also knows where the file lives. The composition root and
//! the wizard both consume these helpers; neither derives them from scratch.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("could not locate $HOME directory")]
    NoHomeDir,
}

/// Default config path: `$HOME/.claude-limit-watchdog/config.yaml`.
pub fn default_config_path() -> Result<PathBuf, PathError> {
    let home = dirs::home_dir().ok_or(PathError::NoHomeDir)?;
    Ok(home.join(".claude-limit-watchdog").join("config.yaml"))
}

/// Default Claude Code working directory: `$HOME/.claude`.
pub fn default_claude_dir() -> Result<PathBuf, PathError> {
    let home = dirs::home_dir().ok_or(PathError::NoHomeDir)?;
    Ok(home.join(".claude"))
}

/// `$HOME/.claude-personal` — the alternative most users have if they run
/// `CLAUDE_CONFIG_DIR` to keep two profiles.
pub fn personal_claude_dir() -> Result<PathBuf, PathError> {
    let home = dirs::home_dir().ok_or(PathError::NoHomeDir)?;
    Ok(home.join(".claude-personal"))
}
