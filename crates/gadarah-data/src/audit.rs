use chrono::{Datelike, TimeZone, Utc, Weekday};
use serde::{Deserialize, Serialize};

use gadarah_core::{Bar, Timeframe};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataAuditResult {
    pub timeframe: Timeframe,
    pub total_bars: usize,
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub duplicate_timestamps: usize,
    pub out_of_order_bars: usize,
    pub misaligned_timestamps: usize,
    pub unexpected_gap_count: usize,
    pub missing_bar_estimate: usize,
    pub largest_unexpected_gap_secs: i64,
    pub invalid_price_bars: usize,
    pub zero_volume_bars: usize,
}

impl DataAuditResult {
    pub fn passed(&self) -> bool {
        self.duplicate_timestamps == 0
            && self.out_of_order_bars == 0
            && self.misaligned_timestamps == 0
            && self.unexpected_gap_count == 0
            && self.invalid_price_bars == 0
    }
}

impl Default for DataAuditResult {
    fn default() -> Self {
        Self {
            timeframe: Timeframe::M15,
            total_bars: 0,
            start_ts: None,
            end_ts: None,
            duplicate_timestamps: 0,
            out_of_order_bars: 0,
            misaligned_timestamps: 0,
            unexpected_gap_count: 0,
            missing_bar_estimate: 0,
            largest_unexpected_gap_secs: 0,
            invalid_price_bars: 0,
            zero_volume_bars: 0,
        }
    }
}

pub fn audit_bars(bars: &[Bar], timeframe: Timeframe) -> DataAuditResult {
    let expected_step = timeframe.seconds();
    let mut result = DataAuditResult {
        timeframe,
        total_bars: bars.len(),
        start_ts: bars.first().map(|bar| bar.timestamp),
        end_ts: bars.last().map(|bar| bar.timestamp),
        ..DataAuditResult::default()
    };

    for bar in bars {
        if bar.timestamp.rem_euclid(expected_step) != 0 {
            result.misaligned_timestamps += 1;
        }
        if bar.high < bar.low
            || bar.open > bar.high
            || bar.open < bar.low
            || bar.close > bar.high
            || bar.close < bar.low
        {
            result.invalid_price_bars += 1;
        }
        if bar.volume == 0 {
            result.zero_volume_bars += 1;
        }
    }

    for window in bars.windows(2) {
        let prev = &window[0];
        let curr = &window[1];
        let delta = curr.timestamp - prev.timestamp;

        if delta == 0 {
            result.duplicate_timestamps += 1;
            continue;
        }
        if delta < 0 {
            result.out_of_order_bars += 1;
            continue;
        }
        if delta <= expected_step {
            continue;
        }
        if is_expected_weekend_gap(prev.timestamp, curr.timestamp) {
            continue;
        }

        result.unexpected_gap_count += 1;
        result.largest_unexpected_gap_secs = result.largest_unexpected_gap_secs.max(delta);
        result.missing_bar_estimate += delta
            .checked_div(expected_step)
            .unwrap_or(0)
            .saturating_sub(1) as usize;
    }

    result
}

fn is_expected_weekend_gap(prev_ts: i64, curr_ts: i64) -> bool {
    let Some(prev_dt) = Utc.timestamp_opt(prev_ts, 0).single() else {
        return false;
    };
    let Some(curr_dt) = Utc.timestamp_opt(curr_ts, 0).single() else {
        return false;
    };

    matches!(prev_dt.weekday(), Weekday::Fri)
        && matches!(curr_dt.weekday(), Weekday::Sun | Weekday::Mon)
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    fn bar(ts: i64, timeframe: Timeframe) -> Bar {
        Bar {
            open: dec!(1.1000),
            high: dec!(1.1010),
            low: dec!(1.0990),
            close: dec!(1.1005),
            volume: 10,
            timestamp: ts,
            timeframe,
        }
    }

    #[test]
    fn detects_duplicate_and_gap() {
        let timeframe = Timeframe::M15;
        let bars = vec![
            bar(0, timeframe),
            bar(0, timeframe),
            bar(900, timeframe),
            bar(3600, timeframe),
        ];

        let audit = audit_bars(&bars, timeframe);
        assert_eq!(audit.duplicate_timestamps, 1);
        assert_eq!(audit.unexpected_gap_count, 1);
        assert_eq!(audit.missing_bar_estimate, 2);
    }

    #[test]
    fn ignores_weekend_gap() {
        let timeframe = Timeframe::M15;
        let friday = Utc
            .with_ymd_and_hms(2026, 4, 3, 21, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        let sunday = Utc
            .with_ymd_and_hms(2026, 4, 5, 21, 15, 0)
            .single()
            .unwrap()
            .timestamp();
        let bars = vec![bar(friday, timeframe), bar(sunday, timeframe)];

        let audit = audit_bars(&bars, timeframe);
        assert_eq!(audit.unexpected_gap_count, 0);
    }
}
