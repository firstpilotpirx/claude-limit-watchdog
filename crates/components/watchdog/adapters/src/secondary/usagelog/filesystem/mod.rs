//! Read Claude Code session logs (`*.jsonl` under `~/.claude/projects/...`)
//! and convert to domain `UsageRecord`s.
//!
//! Tolerant of unknown fields and partial records; bad lines are skipped, not
//! fatal — Claude Code's schema evolves and we don't want every release to break us.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use clw_watchdog_core::application::{UsageLogReader, UsageReadError};
use clw_watchdog_core::domain::UsageRecord;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct LogLine {
    #[serde(rename = "type")]
    entry_type: Option<String>,
    timestamp: Option<String>,
    message: Option<LogMessage>,
}

#[derive(Debug, Deserialize)]
struct LogMessage {
    model: Option<String>,
    usage: Option<LogUsage>,
}

#[derive(Debug, Deserialize, Default)]
#[allow(
    clippy::struct_field_names,
    reason = "field names mirror the JSON schema verbatim"
)]
struct LogUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

/// Reads jsonl files from one or more Claude Code config directories.
///
/// Sensible defaults: `~/.claude/projects/` and (for installs that override
/// `CLAUDE_CONFIG_DIR`) `~/.claude-personal/projects/`. Either, neither, or
/// both may exist — the reader silently skips missing dirs.
#[derive(Debug, Clone)]
pub struct ClaudeCodeLogReader {
    roots: Vec<PathBuf>,
}

impl ClaudeCodeLogReader {
    #[must_use]
    pub fn with_default_roots() -> Self {
        let mut roots = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            roots.push(PathBuf::from(&home).join(".claude").join("projects"));
            roots.push(
                PathBuf::from(&home)
                    .join(".claude-personal")
                    .join("projects"),
            );
        }
        if let Ok(custom) = std::env::var("CLAUDE_CONFIG_DIR") {
            roots.push(PathBuf::from(custom).join("projects"));
        }
        Self { roots }
    }

    #[must_use]
    pub fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    /// Read from a single Claude Code root (the path that contains `projects/`).
    /// This is what the configured `claude_dir` resolves to.
    #[must_use]
    pub fn for_claude_dir(claude_dir: &Path) -> Self {
        Self {
            roots: vec![claude_dir.join("projects")],
        }
    }
}

impl UsageLogReader for ClaudeCodeLogReader {
    fn read_all(&self) -> Result<Vec<UsageRecord>, UsageReadError> {
        let mut records = Vec::new();
        let mut any_root_exists = false;
        for root in &self.roots {
            if !root.is_dir() {
                continue;
            }
            any_root_exists = true;
            collect_jsonl(root, &mut records).map_err(|e| UsageReadError::Io(e.to_string()))?;
        }
        if !any_root_exists {
            return Err(UsageReadError::DirNotFound);
        }
        Ok(records)
    }
}

fn collect_jsonl(dir: &Path, out: &mut Vec<UsageRecord>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            collect_jsonl(&path, out)?;
        } else if ft.is_file()
            && path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("jsonl"))
        {
            parse_jsonl_file(&path, out)?;
        }
    }
    Ok(())
}

fn parse_jsonl_file(path: &Path, out: &mut Vec<UsageRecord>) -> std::io::Result<()> {
    let f = fs::File::open(path)?;
    let reader = BufReader::new(f);
    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        let parsed: LogLine = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue, // tolerate evolving schema
        };
        if parsed.entry_type.as_deref() != Some("assistant") {
            continue;
        }
        let Some(msg) = parsed.message else { continue };
        let Some(model) = msg.model else { continue };
        let Some(usage) = msg.usage else { continue };
        let Some(timestamp) = parsed.timestamp else {
            continue;
        };
        let Ok(ts) = timestamp.parse::<jiff::Timestamp>() else {
            continue;
        };

        out.push(UsageRecord {
            model,
            timestamp_epoch: ts.as_second(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cache_creation_input_tokens: usage.cache_creation_input_tokens,
            cache_read_input_tokens: usage.cache_read_input_tokens,
        });
    }
    Ok(())
}
