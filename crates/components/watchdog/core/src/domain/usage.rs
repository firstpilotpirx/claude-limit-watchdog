//! Token-usage records, session-window detection, and per-model aggregation.
//!
//! Pure data + pure functions. No filesystem access, no JSON parsing — those
//! live in [`crate::adapters::secondary::usage_log`]. The adapter produces a
//! `Vec<UsageRecord>`, this module computes everything that follows.

/// One assistant turn extracted from a Claude Code log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub model: String,
    pub timestamp_epoch: i64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

/// Aggregated totals for one model over some set of records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelStats {
    pub model: String,
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
}

impl ModelStats {
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            + self.output_tokens
            + self.cache_creation_input_tokens
            + self.cache_read_input_tokens
    }
}

/// A concrete usage interval with absolute boundaries (not "last N hours").
///
/// Claude Code's plan tracks usage in 5-hour rolling windows that start with
/// the **first request after the previous window expired**, not as a sliding
/// "now − 5h" interval. [`find_current_session_window`] reconstructs that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionWindow {
    pub start_epoch: i64,
    pub end_epoch: i64,
}

impl SessionWindow {
    #[must_use]
    pub fn contains(&self, epoch: i64) -> bool {
        epoch >= self.start_epoch && epoch < self.end_epoch
    }

    #[must_use]
    pub fn duration_seconds(&self) -> i64 {
        self.end_epoch - self.start_epoch
    }
}

/// Find the **current** session window: the last contiguous run of activity
/// where consecutive records are <= `session_length_secs` apart.
///
/// Returns `None` if there are no records, or if the most recent record is
/// older than `now_epoch - session_length_secs` (meaning the previous session
/// already expired and a new one hasn't started yet).
#[must_use]
pub fn find_current_session_window(
    records: &[UsageRecord],
    session_length_secs: i64,
    now_epoch: i64,
) -> Option<SessionWindow> {
    if records.is_empty() {
        return None;
    }

    let mut sorted: Vec<i64> = records.iter().map(|r| r.timestamp_epoch).collect();
    sorted.sort_unstable();

    let mut session_start = sorted[0];
    let mut last = session_start;
    for &ts in &sorted[1..] {
        if ts - last > session_length_secs {
            session_start = ts;
        }
        last = ts;
    }

    let session_end = session_start + session_length_secs;

    // The previous session expired and "now" is past it — no active window.
    if now_epoch >= session_end {
        return None;
    }
    Some(SessionWindow {
        start_epoch: session_start,
        end_epoch: session_end,
    })
}

/// Group records by `model` and sum tokens. Records with model `<synthetic>`
/// are excluded — those are local Claude Code messages that don't bill against
/// the plan.
#[must_use]
pub fn aggregate_by_model(records: &[UsageRecord]) -> Vec<ModelStats> {
    use std::collections::BTreeMap;

    let mut by_model: BTreeMap<String, ModelStats> = BTreeMap::new();
    for r in records {
        if r.model == "<synthetic>" {
            continue;
        }
        let entry = by_model
            .entry(r.model.clone())
            .or_insert_with(|| ModelStats {
                model: r.model.clone(),
                turns: 0,
                input_tokens: 0,
                output_tokens: 0,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
            });
        entry.turns += 1;
        entry.input_tokens += r.input_tokens;
        entry.output_tokens += r.output_tokens;
        entry.cache_creation_input_tokens += r.cache_creation_input_tokens;
        entry.cache_read_input_tokens += r.cache_read_input_tokens;
    }

    // Sort: highest total tokens first.
    let mut out: Vec<ModelStats> = by_model.into_values().collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.total_tokens()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(model: &str, ts: i64, output: u64) -> UsageRecord {
        UsageRecord {
            model: model.to_string(),
            timestamp_epoch: ts,
            input_tokens: 0,
            output_tokens: output,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        }
    }

    const FIVE_HOURS: i64 = 5 * 3600;

    #[test]
    fn no_records_means_no_window() {
        assert!(find_current_session_window(&[], FIVE_HOURS, 100).is_none());
    }

    #[test]
    fn single_record_starts_a_window() {
        let r = vec![rec("opus", 1000, 1)];
        let w = find_current_session_window(&r, FIVE_HOURS, 1000 + 60).unwrap();
        assert_eq!(w.start_epoch, 1000);
        assert_eq!(w.end_epoch, 1000 + FIVE_HOURS);
    }

    #[test]
    fn long_continuous_activity_is_one_session() {
        // 5 records each 30 minutes apart — always within session length.
        let r: Vec<UsageRecord> = (0..5).map(|i| rec("opus", 1000 + i * 1800, 1)).collect();
        let w = find_current_session_window(&r, FIVE_HOURS, 1000 + 5 * 1800).unwrap();
        assert_eq!(w.start_epoch, 1000);
    }

    #[test]
    fn gap_longer_than_session_starts_new_window() {
        // Old activity around t=1000, then a 6-hour gap, then activity around t=1000 + 6h
        let new_start = 1000 + 6 * 3600;
        let r = vec![
            rec("opus", 1000, 1),
            rec("opus", 1500, 1),
            rec("opus", new_start, 1),
            rec("opus", new_start + 600, 1),
        ];
        let w = find_current_session_window(&r, FIVE_HOURS, new_start + 600).unwrap();
        assert_eq!(w.start_epoch, new_start);
    }

    #[test]
    fn expired_session_returns_none() {
        // Last activity was 6h ago, the 5h window from then is gone.
        let now = 1_000_000;
        let r = vec![rec("opus", now - 6 * 3600, 1)];
        assert!(find_current_session_window(&r, FIVE_HOURS, now).is_none());
    }

    #[test]
    fn aggregate_groups_by_model_and_sorts_by_total() {
        let r = vec![
            rec("opus", 1, 100),
            rec("opus", 2, 200),
            rec("sonnet", 3, 50),
            rec("haiku", 4, 1000),
        ];
        let stats = aggregate_by_model(&r);
        assert_eq!(stats.len(), 3);
        assert_eq!(stats[0].model, "haiku"); // largest first
        assert_eq!(stats[0].turns, 1);
        assert_eq!(stats[1].model, "opus");
        assert_eq!(stats[1].turns, 2);
        assert_eq!(stats[1].output_tokens, 300);
    }

    #[test]
    fn aggregate_excludes_synthetic() {
        let r = vec![rec("opus", 1, 100), rec("<synthetic>", 2, 999_999)];
        let stats = aggregate_by_model(&r);
        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].model, "opus");
    }
}
