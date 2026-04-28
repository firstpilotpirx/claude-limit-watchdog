//! Parse a Claude rate-limit message ("...resets HH:MM (TZ)") into a [`ResetTime`].
//!
//! Recognises:
//!   - `...resets 3:50am (Europe/Belgrade)`
//!   - `...resets 15:50 (UTC)`
//!   - `...resets 3am (UTC)`
//!   - `...resets 12:00 pm (America/New_York)`

use std::sync::OnceLock;

use jiff::tz::TimeZone;
use regex::Regex;

use super::reset_time::ResetTime;

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("input did not match the expected limit message format")]
    NoMatch,
    #[error("invalid hour {hour} for am/pm clock")]
    InvalidHourAmPm { hour: u8 },
    #[error("invalid hour {hour} for 24-hour clock")]
    InvalidHour24 { hour: u8 },
    #[error("invalid minute {minute}")]
    InvalidMinute { minute: u8 },
    #[error("unknown timezone {0:?}")]
    UnknownTimezone(String),
}

fn pattern() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"resets\s+(\d{1,2})(?::(\d{2}))?\s*([aApP][mM])?\s*\(([^)]+)\)")
            .expect("static regex must compile")
    })
}

pub fn parse_reset_line(input: &str) -> Result<ResetTime, ParseError> {
    let caps = pattern().captures(input).ok_or(ParseError::NoMatch)?;

    let mut hour: u8 = caps[1].parse().map_err(|_| ParseError::NoMatch)?;
    let minute: u8 = caps
        .get(2)
        .map(|m| m.as_str().parse::<u8>().map_err(|_| ParseError::NoMatch))
        .transpose()?
        .unwrap_or(0);
    let suffix = caps.get(3).map(|m| m.as_str().to_ascii_lowercase());
    let tz_name = caps[4].to_string();

    match suffix.as_deref() {
        Some("am") => {
            if !(1..=12).contains(&hour) {
                return Err(ParseError::InvalidHourAmPm { hour });
            }
            if hour == 12 {
                hour = 0;
            }
        }
        Some("pm") => {
            if !(1..=12).contains(&hour) {
                return Err(ParseError::InvalidHourAmPm { hour });
            }
            if hour < 12 {
                hour += 12;
            }
        }
        Some(_) | None => {
            if hour > 23 {
                return Err(ParseError::InvalidHour24 { hour });
            }
        }
    }
    if minute > 59 {
        return Err(ParseError::InvalidMinute { minute });
    }

    let timezone =
        TimeZone::get(&tz_name).map_err(|_| ParseError::UnknownTimezone(tz_name.clone()))?;

    let time = jiff::civil::time(
        i8::try_from(hour).expect("0..=23 fits in i8"),
        i8::try_from(minute).expect("0..=59 fits in i8"),
        0,
        0,
    );

    Ok(ResetTime::new(time, timezone))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "You've hit your limit · resets 3:50am (Europe/Belgrade)",
        3,
        50,
        "Europe/Belgrade"
    )]
    #[case("...resets 15:50 (UTC)", 15, 50, "UTC")]
    #[case("...resets 3am (UTC)", 3, 0, "UTC")]
    #[case("...resets 12:00 pm (America/New_York)", 12, 0, "America/New_York")]
    #[case("...resets 12am (UTC)", 0, 0, "UTC")]
    #[case("...resets 12pm (UTC)", 12, 0, "UTC")]
    #[case("...resets 1AM (Europe/Belgrade)", 1, 0, "Europe/Belgrade")]
    fn parses_known_formats(
        #[case] input: &str,
        #[case] hour: i8,
        #[case] minute: i8,
        #[case] tz: &str,
    ) {
        let r = parse_reset_line(input).expect("should parse");
        assert_eq!(r.time().hour(), hour);
        assert_eq!(r.time().minute(), minute);
        assert_eq!(r.timezone().iana_name(), Some(tz));
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(
            parse_reset_line("totally unrelated"),
            Err(ParseError::NoMatch),
        ));
    }

    #[test]
    fn rejects_unknown_timezone() {
        assert!(matches!(
            parse_reset_line("...resets 3:00 (Mars/Olympus)"),
            Err(ParseError::UnknownTimezone(_)),
        ));
    }

    #[test]
    fn rejects_invalid_hour_24h() {
        assert!(matches!(
            parse_reset_line("...resets 25:00 (UTC)"),
            Err(ParseError::InvalidHour24 { hour: 25 }),
        ));
    }
}
