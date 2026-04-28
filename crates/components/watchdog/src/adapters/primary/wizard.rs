//! Primary adapter: interactive first-run / reconfigure wizard.
//!
//! Prompts the user with arrow-key menus and text inputs (via `dialoguer`),
//! returns a fully-populated [`Settings`]. No IO beyond the terminal — the
//! caller is responsible for persisting the result.

use std::path::PathBuf;

use anyhow::{Context, Result};
use dialoguer::theme::ColorfulTheme;
use dialoguer::{Input, Select};

use crate::adapters::secondary::settings_store::{default_claude_dir, personal_claude_dir};
use crate::core::application::Settings;

/// Run the wizard. `existing` pre-fills field defaults when reconfiguring.
pub fn run(existing: Option<&Settings>) -> Result<Settings> {
    let theme = ColorfulTheme::default();

    println!();
    println!("Claude Limit Watchdog — configuration wizard");
    println!("Use ↑/↓ to navigate, Enter to confirm.");
    println!();

    let claude_dir = ask_claude_dir(&theme, existing)?;
    let defaults = existing
        .cloned()
        .unwrap_or_else(|| Settings::defaults_with_claude_dir(claude_dir.clone()));

    let poll_interval_secs: u64 = Input::with_theme(&theme)
        .with_prompt("Poll interval (seconds)")
        .default(defaults.poll_interval_secs)
        .interact_text()
        .context("read poll interval")?;

    let buffer_secs: u64 = Input::with_theme(&theme)
        .with_prompt("Buffer after reset before resuming (seconds)")
        .default(defaults.buffer_secs)
        .interact_text()
        .context("read buffer")?;

    let pane_lines: u32 = Input::with_theme(&theme)
        .with_prompt("Tmux pane lines to capture per poll")
        .default(defaults.pane_lines)
        .interact_text()
        .context("read pane lines")?;

    let limit_phrase: String = Input::with_theme(&theme)
        .with_prompt("Phrase that marks a limit-hit message")
        .default(defaults.limit_phrase.clone())
        .interact_text()
        .context("read limit phrase")?;

    let resume_text: String = Input::with_theme(&theme)
        .with_prompt("Text to send into the pane to resume")
        .default(defaults.resume_text.clone())
        .interact_text()
        .context("read resume text")?;

    Ok(Settings {
        claude_dir,
        poll_interval_secs,
        buffer_secs,
        pane_lines,
        limit_phrase,
        resume_text,
    })
}

fn ask_claude_dir(theme: &ColorfulTheme, existing: Option<&Settings>) -> Result<PathBuf> {
    let default = default_claude_dir().context("locate default Claude dir")?;
    let personal = personal_claude_dir().context("locate personal Claude dir")?;

    let label_default = format!("{} (default)", default.display());
    let label_personal = format!("{}", personal.display());
    let items = vec![
        label_default.as_str(),
        label_personal.as_str(),
        "Custom path…",
    ];

    // Pre-select the matching entry when reconfiguring.
    let preselect = match existing.map(|s| &s.claude_dir) {
        Some(p) if *p == default => 0,
        Some(p) if *p == personal => 1,
        Some(_) => 2,
        None => 0,
    };

    let choice = Select::with_theme(theme)
        .with_prompt("Claude Code working directory")
        .items(&items)
        .default(preselect)
        .interact()
        .context("read claude_dir choice")?;

    match choice {
        0 => Ok(default),
        1 => Ok(personal),
        _ => {
            let preset = existing
                .map(|s| s.claude_dir.display().to_string())
                .unwrap_or_default();
            let raw: String = Input::with_theme(theme)
                .with_prompt("Path to Claude Code working directory")
                .with_initial_text(preset)
                .interact_text()
                .context("read custom claude_dir")?;
            Ok(PathBuf::from(expand_tilde(&raw)))
        }
    }
}

/// Expand a leading `~` to `$HOME`. We accept a `String` and return a `String`
/// because the rest of the wizard converts to `PathBuf` immediately.
fn expand_tilde(input: &str) -> String {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            let mut s = home.to_string_lossy().into_owned();
            s.push('/');
            s.push_str(rest);
            return s;
        }
    }
    input.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_replaces_leading_tilde() {
        std::env::set_var("HOME", "/Users/x");
        assert_eq!(expand_tilde("~/foo/bar"), "/Users/x/foo/bar");
        assert_eq!(expand_tilde("/abs/path"), "/abs/path");
        assert_eq!(expand_tilde("relative"), "relative");
    }
}
