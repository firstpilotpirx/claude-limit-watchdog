use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clw_application::ports::StopSignal;

/// `StopSignal` backed by the `ctrlc` crate's signal handler (SIGINT + SIGTERM).
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
}

impl StopSignal for CtrlCStop {
    fn should_stop(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}
