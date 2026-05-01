use jiff::civil::Time;
use jiff::tz::TimeZone;

/// A wall-clock reset moment as advertised by Claude Code (e.g. `3:50am Europe/Belgrade`).
///
/// The conversion from "the next 03:50 in TZ" to an absolute epoch happens via
/// [`ResetTime::target_epoch_today`] — pure, takes `now_epoch` as input.
#[derive(Debug, Clone)]
pub struct ResetTime {
    time: Time,
    timezone: TimeZone,
}

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("invalid epoch {0}")]
    InvalidEpoch(i64),
    #[error("could not resolve {time} in {tz_name}: {reason}")]
    UnresolvableLocalTime {
        time: Time,
        tz_name: String,
        reason: String,
    },
}

impl ResetTime {
    #[must_use]
    pub fn new(time: Time, timezone: TimeZone) -> Self {
        Self { time, timezone }
    }

    #[must_use]
    pub fn time(&self) -> Time {
        self.time
    }

    #[must_use]
    pub fn timezone(&self) -> &TimeZone {
        &self.timezone
    }

    /// Compute the epoch of "today's HH:MM in this TZ", given the current `now_epoch`.
    ///
    /// The calendar date is taken from `now` in the **target** timezone and combined
    /// with `self.time`. The caller decides what to do if the result is in the past
    /// (the watchdog interprets that as "the reset already happened — send now").
    pub fn target_epoch_today(&self, now_epoch: i64) -> Result<i64, ScheduleError> {
        let now_ts = jiff::Timestamp::from_second(now_epoch)
            .map_err(|_| ScheduleError::InvalidEpoch(now_epoch))?;
        let zoned_now = now_ts.to_zoned(self.timezone.clone());
        let today = zoned_now.date();
        let target_dt = today.at(self.time.hour(), self.time.minute(), 0, 0);
        let target_zoned = target_dt.to_zoned(self.timezone.clone()).map_err(|e| {
            ScheduleError::UnresolvableLocalTime {
                time: self.time,
                tz_name: self.timezone.iana_name().unwrap_or("?").to_string(),
                reason: e.to_string(),
            }
        })?;
        Ok(target_zoned.timestamp().as_second())
    }

    /// Human-readable form, e.g. `03:50 Europe/Belgrade`.
    #[must_use]
    pub fn human_label(&self) -> String {
        format!(
            "{:02}:{:02} {}",
            self.time.hour(),
            self.time.minute(),
            self.timezone.iana_name().unwrap_or("?"),
        )
    }
}

impl PartialEq for ResetTime {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time && self.timezone.iana_name() == other.timezone.iana_name()
    }
}

impl Eq for ResetTime {}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt(hour: i8, minute: i8, tz: &str) -> ResetTime {
        ResetTime::new(
            jiff::civil::time(hour, minute, 0, 0),
            TimeZone::get(tz).unwrap(),
        )
    }

    #[test]
    fn target_epoch_for_today_utc_in_future() {
        // 2024-01-15 00:00:00 UTC = 1_705_276_800
        let now = 1_705_276_800;
        let target = rt(3, 50, "UTC").target_epoch_today(now).unwrap();
        // 2024-01-15 03:50:00 UTC = now + 3*3600 + 50*60
        assert_eq!(target, now + 3 * 3600 + 50 * 60);
    }

    #[test]
    fn target_epoch_for_today_utc_in_past() {
        // Same day, "now" already past 03:50 UTC.
        let now = 1_705_276_800 + 5 * 3600; // 05:00 UTC
        let target = rt(3, 50, "UTC").target_epoch_today(now).unwrap();
        assert!(
            target < now,
            "target must be in the past so the watchdog sends the resume immediately"
        );
        assert_eq!(target, 1_705_276_800 + 3 * 3600 + 50 * 60);
    }

    #[test]
    fn target_epoch_respects_timezone_offset() {
        // 03:50 Europe/Belgrade (UTC+1 in January) = 02:50 UTC.
        let now = 1_705_276_800; // 2024-01-15 00:00 UTC
        let target = rt(3, 50, "Europe/Belgrade")
            .target_epoch_today(now)
            .unwrap();
        assert_eq!(target, 1_705_276_800 + 2 * 3600 + 50 * 60);
    }

    #[test]
    fn human_label_formats_padded() {
        assert_eq!(rt(3, 5, "UTC").human_label(), "03:05 UTC");
        assert_eq!(
            rt(15, 50, "Europe/Belgrade").human_label(),
            "15:50 Europe/Belgrade"
        );
    }
}
