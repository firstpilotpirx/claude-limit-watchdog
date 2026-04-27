//! Application layer.
//!
//! Defines the **ports** (traits) the binary's IO has to satisfy, and the
//! **use cases** (services) that orchestrate domain logic against those ports.
//!
//! No regex, no system clock, no terminal IO here — those live in
//! `clw-infrastructure` and are injected via the traits in [`ports`].

pub mod ports;
pub mod watch_service;

pub use watch_service::{RunStats, WatchConfig, WatchError, WatchService};
