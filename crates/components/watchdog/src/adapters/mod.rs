//! Adapters that connect the application's ports to the real world.
//!
//! * [`primary`]   — drive the application: signals, interactive prompts.
//! * [`secondary`] — driven by the application: tmux, terminal, filesystem.

pub mod primary;
pub mod secondary;
