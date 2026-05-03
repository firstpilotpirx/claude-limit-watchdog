//! Pure domain types and functions. No IO, no async, no system clock.
//!
//! New IO/presentation deps belong in the adapters crate.

pub mod menu;
pub mod parser;
pub mod reset_time;
pub mod usage;

pub use menu::{detect_rate_limit_menu, RateLimitMenu};
pub use parser::{parse_reset_line, ParseError};
pub use reset_time::{ResetTime, ScheduleError};
pub use usage::{
    aggregate_by_model, find_current_session_window, ModelStats, SessionWindow, UsageRecord,
};
