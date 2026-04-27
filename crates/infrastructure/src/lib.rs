//! Adapters that implement the application ports against the real world.
//!
//! Anything that touches the OS, the terminal, or external processes lives here.
//! Domain & application crates must not depend on this crate.

pub mod clock;
pub mod presenter;
pub mod stop;
pub mod tmux;

pub use clock::SystemClock;
pub use presenter::TerminalPresenter;
pub use stop::CtrlCStop;
pub use tmux::TmuxCli;
