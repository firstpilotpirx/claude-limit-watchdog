//! Application layer: ports (traits) + use cases.
//!
//! No regex, no system clock, no terminal IO here — those live in the
//! `clw-watchdog-adapters` crate and are injected via the traits in [`ports`].

pub mod ports;
pub mod settings;
pub mod usage_report;
pub mod watch_service;

pub use settings::Settings;
pub use usage_report::{UsageLogReader, UsageReadError, UsageReport, UsageReportService};
pub use watch_service::{RunStats, WatchConfig, WatchError, WatchService};
