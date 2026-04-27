//! Pure domain types and parsing logic for claude-limit-watchdog.
//!
//! This crate has **no** runtime IO and no async runtime. It must remain trivially
//! testable in isolation. New IO/presentation deps belong in `clw-infrastructure`.

pub mod parser;
pub mod reset_time;

pub use parser::{parse_reset_line, ParseError};
pub use reset_time::{ResetTime, ScheduleError};
