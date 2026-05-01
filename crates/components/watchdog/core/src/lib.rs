//! Watchdog component — inner hexagon (`core`).
//!
//! * [`domain`]      — pure types & functions, zero IO.
//! * [`application`] — ports (traits) + use cases that orchestrate the domain.
//!
//! Adapters live in the sibling crate `clw-watchdog-adapters` and depend on
//! this crate, never the reverse — the dependency rule is enforced by the
//! Cargo crate graph (no archtest needed).

pub mod application;
pub mod domain;
