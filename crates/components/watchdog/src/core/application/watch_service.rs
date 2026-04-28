//! Watchdog use case.
//!
//! Pure orchestration: every IO operation goes through a port (`Tmux`, `Clock`,
//! `StopSignal`, `Presenter`). All time math lives in [`crate::core::domain`].

use std::time::Duration;

use super::ports::{BannerInfo, Clock, IdleInfo, Presenter, StopSignal, Tmux, TmuxError};
use super::usage_report::{UsageLogReader, UsageReportService};
use crate::core::domain::{parse_reset_line, ModelStats, ResetTime, SessionWindow};

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("tmux session '{0}' not found at start")]
    SessionMissingAtStart(String),
    #[error("tmux session '{0}' disappeared while running")]
    SessionLost(String),
    #[error(transparent)]
    Tmux(#[from] TmuxError),
}

#[derive(Debug, Clone)]
pub struct WatchConfig {
    pub session: String,
    pub poll_interval: Duration,
    pub buffer: Duration,
    pub limit_phrase: String,
    pub resume_text: String,
    pub pane_lines: u32,
}

impl WatchConfig {
    #[must_use]
    pub fn defaults_for(session: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            poll_interval: Duration::from_secs(60),
            buffer: Duration::from_secs(60),
            limit_phrase: "You've hit your limit".to_string(),
            resume_text: "continue the work where you left off".to_string(),
            pane_lines: 200,
        }
    }

    fn poll_interval_secs(&self) -> i64 {
        i64::try_from(self.poll_interval.as_secs()).unwrap_or(i64::MAX)
    }

    fn buffer_secs(&self) -> i64 {
        i64::try_from(self.buffer.as_secs()).unwrap_or(i64::MAX)
    }
}

/// Returned from [`WatchService::run`] on clean shutdown.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunStats {
    pub uptime_seconds: i64,
    pub resume_count: u32,
}

#[derive(Debug)]
pub struct WatchService<T, C, S, P, U> {
    tmux:          T,
    clock:         C,
    stop:          S,
    presenter:     P,
    usage_service: UsageReportService<U>,
    cfg:           WatchConfig,
}

#[derive(Debug)]
struct State {
    started_at:         i64,
    last_handled_epoch: i64,
    last_poll_at:       i64,
    resume_count:       u32,
    session_window:     Option<SessionWindow>,
    session_stats:      Vec<ModelStats>,
}

impl<T, C, S, P, U> WatchService<T, C, S, P, U>
where
    T: Tmux,
    C: Clock,
    S: StopSignal,
    P: Presenter,
    U: UsageLogReader,
{
    pub fn new(
        tmux: T,
        clock: C,
        stop: S,
        presenter: P,
        usage_reader: U,
        cfg: WatchConfig,
    ) -> Self {
        // Session length matches Claude's published 5h plan window.
        let usage_service = UsageReportService::new(usage_reader, 5 * 3600);
        Self { tmux, clock, stop, presenter, usage_service, cfg }
    }

    pub fn presenter(&self) -> &P {
        &self.presenter
    }

    /// Run the watch loop. Returns when [`StopSignal::should_stop`] becomes true,
    /// or with [`WatchError::SessionLost`] if the tmux session disappears.
    pub fn run(&self) -> Result<RunStats, WatchError> {
        let started_at = self.clock.now_epoch();
        let mut state = State {
            started_at,
            last_handled_epoch: 0,
            last_poll_at: started_at,
            resume_count: 0,
            session_window: None,
            session_stats: Vec::new(),
        };

        if !self.tmux.has_session(&self.cfg.session) {
            return Err(WatchError::SessionMissingAtStart(self.cfg.session.clone()));
        }

        self.presenter.banner(&BannerInfo {
            session: self.cfg.session.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            poll_interval_seconds: self.cfg.poll_interval.as_secs(),
            buffer_seconds: self.cfg.buffer.as_secs(),
            limit_phrase: self.cfg.limit_phrase.clone(),
            resume_text: self.cfg.resume_text.clone(),
        });
        self.presenter.started();

        // First check fires immediately so a stale limit message in the pane is
        // handled without waiting a full poll interval.
        self.check_once(&mut state)?;
        self.refresh_usage(&mut state);

        let poll_interval = self.cfg.poll_interval_secs();
        let mut last_check = self.clock.now_epoch();

        while !self.stop.should_stop() {
            let now = self.clock.now_epoch();
            if now - last_check >= poll_interval {
                last_check = now;
                self.check_once(&mut state)?;
                self.refresh_usage(&mut state);
            }
            self.presenter.idle_tick(&IdleInfo {
                now_epoch:      now,
                last_poll_at:   state.last_poll_at,
                started_at:     state.started_at,
                resume_count:   state.resume_count,
                session_window: state.session_window,
                session_stats:  state.session_stats.clone(),
            });
            self.clock.sleep(Duration::from_secs(1));
        }

        Ok(RunStats {
            uptime_seconds: self.clock.now_epoch() - started_at,
            resume_count: state.resume_count,
        })
    }

    fn check_once(&self, state: &mut State) -> Result<(), WatchError> {
        if !self.tmux.has_session(&self.cfg.session) {
            return Err(WatchError::SessionLost(self.cfg.session.clone()));
        }
        state.last_poll_at = self.clock.now_epoch();

        let pane = self
            .tmux
            .capture_pane(&self.cfg.session, self.cfg.pane_lines)?;
        let Some(reset_time) = self.detect_limit(&pane) else {
            return Ok(());
        };

        let now = self.clock.now_epoch();
        let target_epoch = match reset_time.target_epoch_today(now) {
            Ok(e) => e,
            Err(err) => {
                self.presenter
                    .warn(&format!("could not compute reset epoch: {err}"));
                return Ok(());
            }
        };

        // Dedup: same target as last successful resume → ignore.
        if target_epoch == state.last_handled_epoch {
            return Ok(());
        }

        let target_human = reset_time.human_label();

        if target_epoch <= now {
            // Reset already happened — message is stale, send right away.
            self.presenter.limit_already_passed(&target_human);
            self.send_resume(state)?;
            state.last_handled_epoch = target_epoch;
            return Ok(());
        }

        let wait_secs = (target_epoch - now) + self.cfg.buffer_secs();
        let end_epoch = now + wait_secs;
        self.presenter
            .limit_detected(&target_human, wait_secs, self.cfg.buffer_secs());

        self.countdown(end_epoch, &target_human);
        if self.stop.should_stop() {
            return Ok(());
        }
        self.send_resume(state)?;
        state.last_handled_epoch = target_epoch;
        Ok(())
    }

    /// Re-read Claude Code logs and recompute the per-model totals for the
    /// current 5h window. Errors are surfaced via the presenter (warning) and
    /// then swallowed — the watchdog stays up even if log parsing fails.
    fn refresh_usage(&self, state: &mut State) {
        let now = self.clock.now_epoch();
        match self.usage_service.current_session(now) {
            Ok(report) => {
                state.session_window = report.window;
                state.session_stats = report.stats;
            }
            Err(e) => {
                self.presenter.warn(&format!("usage refresh failed: {e}"));
            }
        }
    }

    fn countdown(&self, end_epoch: i64, target_human: &str) {
        let total = end_epoch - self.clock.now_epoch();
        if total <= 0 {
            return;
        }
        while !self.stop.should_stop() {
            let now = self.clock.now_epoch();
            let remaining = end_epoch - now;
            if remaining <= 0 {
                break;
            }
            self.presenter
                .countdown_step(remaining, total, target_human);
            self.clock.sleep(Duration::from_secs(1));
        }
    }

    fn send_resume(&self, state: &mut State) -> Result<(), WatchError> {
        self.tmux
            .send_keys(&self.cfg.session, &self.cfg.resume_text)?;
        state.resume_count += 1;
        self.presenter
            .resumed(state.resume_count, &self.cfg.resume_text, &self.cfg.session);
        Ok(())
    }

    /// Find the most recent line containing the limit phrase, parse it, return the
    /// reset time. Returns `None` when there's no limit message in the pane.
    #[must_use]
    pub fn detect_limit(&self, pane_text: &str) -> Option<ResetTime> {
        let last_match = pane_text
            .lines()
            .filter(|l| l.contains(&self.cfg.limit_phrase))
            .next_back()?;
        parse_reset_line(last_match).ok()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use mockall::predicate::eq;

    use super::*;
    use crate::core::application::ports::{
        IdleInfo, MockClock, MockPresenter, MockStopSignal, MockTmux,
    };
    use crate::core::application::usage_report::MockUsageLogReader;

    fn empty_usage_reader() -> MockUsageLogReader {
        let mut r = MockUsageLogReader::new();
        r.expect_read_all().returning(|| Ok(Vec::new()));
        r
    }

    fn defaults() -> WatchConfig {
        WatchConfig::defaults_for("work")
    }

    fn presenter_allowing_anything() -> MockPresenter {
        let mut p = MockPresenter::new();
        p.expect_banner().returning(|_| ());
        p.expect_started().returning(|| ());
        p.expect_idle_tick().returning(|_info: &IdleInfo| ());
        p.expect_limit_detected().returning(|_, _, _| ());
        p.expect_limit_already_passed().returning(|_| ());
        p.expect_countdown_step().returning(|_, _, _| ());
        p.expect_resumed().returning(|_, _, _| ());
        p.expect_warn().returning(|_| ());
        p.expect_error().returning(|_| ());
        p.expect_shutdown().returning(|_, _| ());
        p
    }

    #[test]
    fn detect_limit_finds_latest_match_in_pane() {
        let svc = WatchService::new(
            MockTmux::new(),
            MockClock::new(),
            MockStopSignal::new(),
            MockPresenter::new(),
            empty_usage_reader(),
            defaults(),
        );
        let pane = "\
some earlier output
You've hit your limit · resets 3:00am (UTC)
later unrelated output
You've hit your limit · resets 4:30am (UTC)
trailing line
";
        let r = svc.detect_limit(pane).expect("limit should be detected");
        assert_eq!(r.time().hour(), 4);
        assert_eq!(r.time().minute(), 30);
    }

    #[test]
    fn detect_limit_returns_none_when_absent() {
        let svc = WatchService::new(
            MockTmux::new(),
            MockClock::new(),
            MockStopSignal::new(),
            MockPresenter::new(),
            empty_usage_reader(),
            defaults(),
        );
        assert!(svc
            .detect_limit("nothing interesting here\nat all\n")
            .is_none());
    }

    #[test]
    fn missing_session_at_start_is_an_error() {
        let mut tmux = MockTmux::new();
        tmux.expect_has_session()
            .with(eq("work"))
            .return_const(false);

        let mut clock = MockClock::new();
        clock.expect_now_epoch().return_const(0_i64);

        let svc = WatchService::new(
            tmux,
            clock,
            MockStopSignal::new(),
            MockPresenter::new(),
            empty_usage_reader(),
            defaults(),
        );

        assert!(matches!(
            svc.run(),
            Err(WatchError::SessionMissingAtStart(_))
        ));
    }

    /// End-to-end: pane already contains a limit message whose reset is in the past
    /// → service sends resume immediately and exits cleanly when stop is signalled.
    #[test]
    fn run_sends_resume_when_reset_is_in_past() {
        let mut tmux = MockTmux::new();
        tmux.expect_has_session().return_const(true);
        tmux.expect_capture_pane()
            .returning(|_, _| Ok("You've hit your limit · resets 1:00am (UTC)\n".to_string()));
        let send_count = std::sync::Arc::new(Mutex::new(0u32));
        let send_count_clone = send_count.clone();
        tmux.expect_send_keys().returning(move |_, _| {
            *send_count_clone.lock().unwrap() += 1;
            Ok(())
        });

        // Clock: "now" is 2024-01-15 12:00 UTC = 1_705_320_000 — well past 01:00.
        let mut clock = MockClock::new();
        clock.expect_now_epoch().return_const(1_705_320_000_i64);
        clock.expect_sleep().returning(|_: Duration| ());

        // Stop: idle once, then stop.
        let calls = std::sync::Arc::new(Mutex::new(0u32));
        let calls2 = calls.clone();
        let mut stop = MockStopSignal::new();
        stop.expect_should_stop().returning(move || {
            let mut c = calls2.lock().unwrap();
            *c += 1;
            *c > 1
        });

        let svc = WatchService::new(
            tmux,
            clock,
            stop,
            presenter_allowing_anything(),
            empty_usage_reader(),
            defaults(),
        );

        let stats = svc.run().expect("clean shutdown");
        assert_eq!(stats.resume_count, 1);
        assert_eq!(*send_count.lock().unwrap(), 1);
    }
}
