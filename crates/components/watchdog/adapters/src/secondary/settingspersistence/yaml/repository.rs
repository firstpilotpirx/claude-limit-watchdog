//! YAML implementation of the [`SettingsRepository`] port.
//!
//! Atomic save: write to `*.tmp` then rename, so a crash mid-write doesn't
//! truncate the user's config.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use clw_watchdog_core::application::ports::{SettingsRepository, SettingsRepositoryError};
use clw_watchdog_core::application::Settings;

use super::dto::SettingsYamlDto;
use super::mapper::{domain_to_dto, dto_to_domain};

/// File-backed YAML implementation of [`SettingsRepository`].
#[derive(Debug, Clone)]
pub struct YamlSettingsRepository {
    path: PathBuf,
}

impl YamlSettingsRepository {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl SettingsRepository for YamlSettingsRepository {
    fn load(&self) -> Result<Option<Settings>, SettingsRepositoryError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => {
                return Err(SettingsRepositoryError::Io {
                    path: self.path.clone(),
                    source: e,
                })
            }
        };
        let dto: SettingsYamlDto =
            serde_yml::from_str(&text).map_err(|e| SettingsRepositoryError::Parse {
                path: self.path.clone(),
                reason: e.to_string(),
            })?;
        Ok(Some(dto_to_domain(dto)))
    }

    fn save(&self, settings: &Settings) -> Result<(), SettingsRepositoryError> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| SettingsRepositoryError::Io {
                path: parent.to_path_buf(),
                source: e,
            })?;
        }
        let dto = domain_to_dto(settings);
        let yaml = serde_yml::to_string(&dto)
            .map_err(|e| SettingsRepositoryError::Serialize(e.to_string()))?;

        let tmp = self.path.with_extension("yaml.tmp");
        {
            let mut f = fs::File::create(&tmp).map_err(|e| SettingsRepositoryError::Io {
                path: tmp.clone(),
                source: e,
            })?;
            f.write_all(yaml.as_bytes())
                .map_err(|e| SettingsRepositoryError::Io {
                    path: tmp.clone(),
                    source: e,
                })?;
            f.sync_all().map_err(|e| SettingsRepositoryError::Io {
                path: tmp.clone(),
                source: e,
            })?;
        }
        fs::rename(&tmp, &self.path).map_err(|e| SettingsRepositoryError::Io {
            path: self.path.clone(),
            source: e,
        })?;
        Ok(())
    }
}

/// Render settings as a YAML string (for `config show`). Kept here so the
/// CLI doesn't need a direct `serde_yml` dependency.
pub fn to_yaml_string(settings: &Settings) -> Result<String, SettingsRepositoryError> {
    let dto = domain_to_dto(settings);
    serde_yml::to_string(&dto).map_err(|e| SettingsRepositoryError::Serialize(e.to_string()))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sample_settings() -> Settings {
        Settings {
            claude_dir: PathBuf::from("/tmp/claude"),
            poll_interval_secs: 30,
            buffer_secs: 45,
            pane_lines: 150,
            limit_phrase: "limit".to_string(),
            resume_text: "go".to_string(),
        }
    }

    #[test]
    fn round_trip() {
        let dir = tempdir();
        let path = dir.join("config.yaml");
        let repo = YamlSettingsRepository::new(path);
        let s = sample_settings();
        repo.save(&s).unwrap();
        let loaded = repo.load().unwrap().expect("settings present");
        assert_eq!(loaded, s);
    }

    #[test]
    fn missing_file_returns_none() {
        let dir = tempdir();
        let path = dir.join("does-not-exist.yaml");
        let repo = YamlSettingsRepository::new(path);
        assert!(repo.load().unwrap().is_none());
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
