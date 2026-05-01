//! Primary adapter: Ctrl-C / SIGTERM handler (and optional 'q'-to-quit
//! keyboard listener) that flips a shared [`StopSignal`] flag.

use std::io::{IsTerminal, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clw_watchdog_core::application::ports::StopSignal;

/// `StopSignal` backed by the `ctrlc` crate's signal handler (SIGINT + SIGTERM).
/// Optionally also flipped by [`CtrlCStop::enable_q_to_quit`].
#[derive(Debug, Clone)]
pub struct CtrlCStop {
    flag: Arc<AtomicBool>,
}

impl CtrlCStop {
    /// Install the global signal handler. Must only be called once per process.
    pub fn install() -> Result<Self, ctrlc::Error> {
        let flag = Arc::new(AtomicBool::new(false));
        let f = flag.clone();
        ctrlc::set_handler(move || f.store(true, Ordering::SeqCst))?;
        Ok(Self { flag })
    }

    /// Spawn a background thread that flips the stop flag when 'q' or 'Q'
    /// is read on stdin. No-op when stdin isn't a TTY (so piped input
    /// doesn't accidentally trigger an exit on the byte 'q' inside data).
    ///
    /// Assumes the terminal is in non-canonical mode (`stty -icanon`) — the
    /// `TerminalPresenter` configures that on construction, so call this
    /// **after** the presenter is built. In canonical mode the user would
    /// have to also press Enter, which still works but is unintuitive.
    ///
    /// The thread exits naturally on the first 'q'/'Q' (or on stdin EOF /
    /// read error). If the watchdog stops via signal instead, the thread
    /// is left blocked on `read` and dies with the process — fine, since
    /// it owns no resources beyond stdin.
    pub fn enable_q_to_quit(&self) {
        if !std::io::stdin().is_terminal() {
            return;
        }
        let flag = self.flag.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1];
            let mut stdin = std::io::stdin();
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {
                        if buf[0] == b'q' || buf[0] == b'Q' {
                            flag.store(true, Ordering::SeqCst);
                            return;
                        }
                    }
                }
            }
        });
    }
}

impl StopSignal for CtrlCStop {
    fn should_stop(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}
