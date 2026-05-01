//! Secondary (driven) adapters: implementations of the application ports
//! against real systems (tmux, terminal, filesystem).

pub mod clock;
pub mod panecontrol;
pub mod settingspersistence;
pub mod usagelog;
pub mod userpresentation;
