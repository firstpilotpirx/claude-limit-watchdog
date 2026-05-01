//! tmux-CLI implementation of the [`Pane`] port.

use std::process::Command;

use clw_watchdog_core::application::ports::{Pane, PaneError};

#[derive(Debug, Default, Clone, Copy)]
pub struct TmuxPane;

impl Pane for TmuxPane {
    fn exists(&self, name: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", name])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn capture(&self, name: &str, lines: u32) -> Result<String, PaneError> {
        let start = format!("-{lines}");
        let out = Command::new("tmux")
            .args(["capture-pane", "-t", name, "-p", "-S", &start])
            .output()
            .map_err(|e| PaneError::OperationFailed(format!("tmux capture-pane: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(PaneError::OperationFailed(format!(
                "tmux capture-pane exit {:?}: {}",
                out.status.code(),
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn send(&self, name: &str, text: &str) -> Result<(), PaneError> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", name, text, "Enter"])
            .status()
            .map_err(|e| PaneError::OperationFailed(format!("tmux send-keys: {e}")))?;
        if !status.success() {
            return Err(PaneError::OperationFailed(format!(
                "tmux send-keys exit {:?}",
                status.code()
            )));
        }
        Ok(())
    }
}
