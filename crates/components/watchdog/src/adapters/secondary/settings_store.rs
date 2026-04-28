//! Read & write the YAML settings file at `~/.claude-limit-watchdog/config.yaml`.
//!
//! Atomic save: write to `*.tmp` then rename, so a crash mid-write doesn't
//! truncate the user's config.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::core::application::Settings;

#[derive(Debug, thiserror::Error)]
pub enum SettingsStoreError {
    #[error("could not locate $HOME directory")]
    NoHomeDir,
    #[error("io error at {path}: {source}")]
    Io {
        path:   PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("yaml parse error in {path}: {source}")]
    Parse {
        path:   PathBuf,
        #[source]
        source: serde_yml::Error,
    },
    #[error("yaml serialize error: {0}")]
    Serialize(#[source] serde_yml::Error),
}

/// Default config path: `$HOME/.claude-limit-watchdog/config.yaml`.
pub fn default_config_path() -> Result<PathBuf, SettingsStoreError> {
    let home = dirs::home_dir().ok_or(SettingsStoreError::NoHomeDir)?;
    Ok(home.join(".claude-limit-watchdog").join("config.yaml"))
}

/// Default Claude Code working directory: `$HOME/.claude`.
pub fn default_claude_dir() -> Result<PathBuf, SettingsStoreError> {
    let home = dirs::home_dir().ok_or(SettingsStoreError::NoHomeDir)?;
    Ok(home.join(".claude"))
}

/// `$HOME/.claude-personal` — the alternative most users have if they run
/// `CLAUDE_CONFIG_DIR` to keep two profiles.
pub fn personal_claude_dir() -> Result<PathBuf, SettingsStoreError> {
    let home = dirs::home_dir().ok_or(SettingsStoreError::NoHomeDir)?;
    Ok(home.join(".claude-personal"))
}

/// Load settings from `path`. Returns `Ok(None)` if the file doesn't exist.
pub fn load(path: &Path) -> Result<Option<Settings>, SettingsStoreError> {
    let text = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(SettingsStoreError::Io {
                path:   path.to_path_buf(),
                source: e,
            })
        }
    };
    let settings: Settings = serde_yml::from_str(&text).map_err(|e| SettingsStoreError::Parse {
        path:   path.to_path_buf(),
        source: e,
    })?;
    Ok(Some(settings))
}

/// Render settings as a YAML string (for `config show`). Kept here so the
/// CLI doesn't need a direct `serde_yml` dependency.
pub fn to_yaml_string(settings: &Settings) -> Result<String, SettingsStoreError> {
    serde_yml::to_string(settings).map_err(SettingsStoreError::Serialize)
}

/// Atomically write settings to `path`. Creates parent dirs as needed.
pub fn save(path: &Path, settings: &Settings) -> Result<(), SettingsStoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| SettingsStoreError::Io {
            path:   parent.to_path_buf(),
            source: e,
        })?;
    }
    let yaml = serde_yml::to_string(settings).map_err(SettingsStoreError::Serialize)?;

    let tmp = path.with_extension("yaml.tmp");
    {
        let mut f = fs::File::create(&tmp).map_err(|e| SettingsStoreError::Io {
            path:   tmp.clone(),
            source: e,
        })?;
        f.write_all(yaml.as_bytes())
            .map_err(|e| SettingsStoreError::Io { path: tmp.clone(), source: e })?;
        f.sync_all()
            .map_err(|e| SettingsStoreError::Io { path: tmp.clone(), source: e })?;
    }
    fs::rename(&tmp, path).map_err(|e| SettingsStoreError::Io {
        path:   path.to_path_buf(),
        source: e,
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sample_settings() -> Settings {
        Settings {
            claude_dir:         PathBuf::from("/tmp/claude"),
            poll_interval_secs: 30,
            buffer_secs:        45,
            pane_lines:         150,
            limit_phrase:       "limit".to_string(),
            resume_text:        "go".to_string(),
        }
    }

    #[test]
    fn round_trip() {
        let dir = tempdir();
        let path = dir.join("config.yaml");
        let s = sample_settings();
        save(&path, &s).unwrap();
        let loaded = load(&path).unwrap().expect("settings present");
        assert_eq!(loaded.claude_dir, s.claude_dir);
        assert_eq!(loaded.poll_interval_secs, s.poll_interval_secs);
        assert_eq!(loaded.buffer_secs, s.buffer_secs);
        assert_eq!(loaded.pane_lines, s.pane_lines);
        assert_eq!(loaded.limit_phrase, s.limit_phrase);
        assert_eq!(loaded.resume_text, s.resume_text);
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempdir();
        let path = dir.join("does-not-exist.yaml");
        assert!(load(&path).unwrap().is_none());
    }

    /// Tiny tempdir helper — we don't want a `tempfile` dep just for tests.
    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("clw-store-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
