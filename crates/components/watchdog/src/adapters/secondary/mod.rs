//! Secondary (driven) adapters: implementations of application ports against
//! real systems (tmux, the terminal, the filesystem).

pub mod clock;
pub mod presenter;
pub mod settings_store;
pub mod tmux;
pub mod usage_log;
