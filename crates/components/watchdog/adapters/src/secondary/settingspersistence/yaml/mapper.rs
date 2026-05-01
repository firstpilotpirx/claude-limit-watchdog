//! Map between the on-disk DTO ([`SettingsYamlDto`]) and the domain VO
//! ([`Settings`]). Pure conversion — no IO.
//!
//! [`Settings`]: clw_watchdog_core::application::Settings

use clw_watchdog_core::application::Settings;

use super::dto::SettingsYamlDto;

#[must_use]
pub fn dto_to_domain(dto: SettingsYamlDto) -> Settings {
    Settings {
        claude_dir: dto.claude_dir,
        poll_interval_secs: dto.poll_interval_secs,
        buffer_secs: dto.buffer_secs,
        pane_lines: dto.pane_lines,
        limit_phrase: dto.limit_phrase,
        resume_text: dto.resume_text,
    }
}

#[must_use]
pub fn domain_to_dto(settings: &Settings) -> SettingsYamlDto {
    SettingsYamlDto {
        claude_dir: settings.claude_dir.clone(),
        poll_interval_secs: settings.poll_interval_secs,
        buffer_secs: settings.buffer_secs,
        pane_lines: settings.pane_lines,
        limit_phrase: settings.limit_phrase.clone(),
        resume_text: settings.resume_text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn sample() -> Settings {
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
    fn round_trip_through_dto() {
        let original = sample();
        let dto = domain_to_dto(&original);
        let back = dto_to_domain(dto);
        assert_eq!(back, original);
    }
}
