use std::process::Command;

use clw_application::ports::{Tmux, TmuxError};

#[derive(Debug, Default, Clone, Copy)]
pub struct TmuxCli;

impl Tmux for TmuxCli {
    fn has_session(&self, name: &str) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", name])
            .output()
            .is_ok_and(|o| o.status.success())
    }

    fn capture_pane(&self, name: &str, lines: u32) -> Result<String, TmuxError> {
        let start = format!("-{lines}");
        let out = Command::new("tmux")
            .args(["capture-pane", "-t", name, "-p", "-S", &start])
            .output()
            .map_err(|e| TmuxError::CommandFailed(format!("tmux capture-pane: {e}")))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(TmuxError::CommandFailed(format!(
                "tmux capture-pane exit {:?}: {}",
                out.status.code(),
                stderr.trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn send_keys(&self, name: &str, text: &str) -> Result<(), TmuxError> {
        let status = Command::new("tmux")
            .args(["send-keys", "-t", name, text, "Enter"])
            .status()
            .map_err(|e| TmuxError::CommandFailed(format!("tmux send-keys: {e}")))?;
        if !status.success() {
            return Err(TmuxError::CommandFailed(format!(
                "tmux send-keys exit {:?}",
                status.code()
            )));
        }
        Ok(())
    }
}
