//! `std::time`-backed [`Clock`] implementation.

use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clw_watchdog_core::application::ports::Clock;

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_epoch(&self) -> i64 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        i64::try_from(now.as_secs()).unwrap_or(i64::MAX)
    }

    fn sleep(&self, duration: Duration) {
        thread::sleep(duration);
    }
}
