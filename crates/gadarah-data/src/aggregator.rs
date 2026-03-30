use gadarah_core::{Bar, Timeframe};

use crate::error::DataError;

// ---------------------------------------------------------------------------
// Bar aggregation: lower timeframe → higher timeframe
// ---------------------------------------------------------------------------

/// Aggregate a sorted slice of bars from a lower timeframe into a higher timeframe.
///
/// For example, three M5 bars → one M15 bar. The input bars MUST be sorted by
/// timestamp ascending and must all share the same source timeframe.
///
/// The output bars are aligned to the target timeframe boundary:
/// - M5 → M15: timestamp rounded down to nearest 900s boundary
/// - M1 → M5:  timestamp rounded down to nearest 300s boundary
/// - etc.
pub fn aggregate_bars(source: &[Bar], target_tf: Timeframe) -> Result<Vec<Bar>, DataError> {
    if source.is_empty() {
        return Err(DataError::EmptyAggregation);
    }

    let target_secs = target_tf.seconds();
    let mut result: Vec<Bar> = Vec::new();

    for bar in source {
        let bucket_ts = align_timestamp(bar.timestamp, target_secs);

        match result.last_mut() {
            Some(agg) if agg.timestamp == bucket_ts => {
                // Extend existing aggregated bar
                if bar.high > agg.high {
                    agg.high = bar.high;
                }
                if bar.low < agg.low {
                    agg.low = bar.low;
                }
                agg.close = bar.close;
                agg.volume += bar.volume;
            }
            _ => {
                // Start new aggregated bar
                result.push(Bar {
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    volume: bar.volume,
                    timestamp: bucket_ts,
                    timeframe: target_tf,
                });
            }
        }
    }

    Ok(result)
}

/// Align a timestamp down to the nearest timeframe boundary.
fn align_timestamp(ts: i64, period_secs: i64) -> i64 {
    ts - (ts % period_secs)
}

// ---------------------------------------------------------------------------
// Streaming aggregator: feed bars one at a time
// ---------------------------------------------------------------------------

/// A streaming aggregator that accumulates bars from a source timeframe and
/// emits completed bars of the target timeframe.
pub struct StreamAggregator {
    target_tf: Timeframe,
    target_secs: i64,
    current: Option<Bar>,
}

impl StreamAggregator {
    pub fn new(target_tf: Timeframe) -> Self {
        Self {
            target_tf,
            target_secs: target_tf.seconds(),
            current: None,
        }
    }

    /// Feed a single source bar. Returns `Some(completed_bar)` when a target-TF
    /// bar is complete (i.e., the new bar belongs to a different bucket).
    pub fn feed(&mut self, bar: &Bar) -> Option<Bar> {
        let bucket_ts = align_timestamp(bar.timestamp, self.target_secs);

        match &mut self.current {
            Some(agg) if agg.timestamp == bucket_ts => {
                // Same bucket — extend
                merge_into(agg, bar);
                None
            }
            Some(_) => {
                // New bucket — emit the completed bar, start fresh
                let completed = self.current.take().unwrap();
                self.current = Some(Bar {
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    volume: bar.volume,
                    timestamp: bucket_ts,
                    timeframe: self.target_tf,
                });
                Some(completed)
            }
            None => {
                // First bar
                self.current = Some(Bar {
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    volume: bar.volume,
                    timestamp: bucket_ts,
                    timeframe: self.target_tf,
                });
                None
            }
        }
    }

    /// Flush any in-progress bar (e.g., at end of data or session).
    pub fn flush(&mut self) -> Option<Bar> {
        self.current.take()
    }

    /// Peek at the current in-progress bar without consuming it.
    pub fn peek(&self) -> Option<&Bar> {
        self.current.as_ref()
    }
}

fn merge_into(agg: &mut Bar, bar: &Bar) {
    if bar.high > agg.high {
        agg.high = bar.high;
    }
    if bar.low < agg.low {
        agg.low = bar.low;
    }
    agg.close = bar.close;
    agg.volume += bar.volume;
}

// ---------------------------------------------------------------------------
// Multi-timeframe aggregator: M1 → M5, M15, H1, H4, D1 simultaneously
// ---------------------------------------------------------------------------

/// Feeds M1 bars and produces aligned bars for all higher timeframes.
pub struct MultiTfAggregator {
    pub m5: StreamAggregator,
    pub m15: StreamAggregator,
    pub h1: StreamAggregator,
    pub h4: StreamAggregator,
    pub d1: StreamAggregator,
}

/// Bars emitted by the multi-TF aggregator for a single M1 feed.
#[derive(Debug, Default)]
pub struct MultiTfOutput {
    pub m5: Option<Bar>,
    pub m15: Option<Bar>,
    pub h1: Option<Bar>,
    pub h4: Option<Bar>,
    pub d1: Option<Bar>,
}

impl MultiTfAggregator {
    pub fn new() -> Self {
        Self {
            m5: StreamAggregator::new(Timeframe::M5),
            m15: StreamAggregator::new(Timeframe::M15),
            h1: StreamAggregator::new(Timeframe::H1),
            h4: StreamAggregator::new(Timeframe::H4),
            d1: StreamAggregator::new(Timeframe::D1),
        }
    }

    /// Feed a single M1 bar and collect any completed higher-TF bars.
    pub fn feed_m1(&mut self, bar: &Bar) -> MultiTfOutput {
        MultiTfOutput {
            m5: self.m5.feed(bar),
            m15: self.m15.feed(bar),
            h1: self.h1.feed(bar),
            h4: self.h4.feed(bar),
            d1: self.d1.feed(bar),
        }
    }

    /// Flush all in-progress bars.
    pub fn flush_all(&mut self) -> MultiTfOutput {
        MultiTfOutput {
            m5: self.m5.flush(),
            m15: self.m15.flush(),
            h1: self.h1.flush(),
            h4: self.h4.flush(),
            d1: self.d1.flush(),
        }
    }
}

impl Default for MultiTfAggregator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn m1_bar(ts: i64, close: Decimal) -> Bar {
        Bar {
            open: dec!(1.10000),
            high: close + dec!(0.00050),
            low: close - dec!(0.00050),
            close,
            volume: 100,
            timestamp: ts,
            timeframe: Timeframe::M1,
        }
    }

    // Use aligned base: 1700006400 % 300 = 0, % 3600 = 0
    const ALIGNED_BASE: i64 = 1700006400;

    #[test]
    fn aggregate_m1_to_m5() {
        // 5 M1 bars starting at a 5-min boundary
        let bars: Vec<Bar> = (0..5)
            .map(|i| m1_bar(ALIGNED_BASE + i * 60, dec!(1.10000) + Decimal::new(i, 4)))
            .collect();

        let agg = aggregate_bars(&bars, Timeframe::M5).unwrap();
        assert_eq!(agg.len(), 1);
        assert_eq!(agg[0].timeframe, Timeframe::M5);
        assert_eq!(agg[0].open, bars[0].open);
        assert_eq!(agg[0].close, bars[4].close);
    }

    #[test]
    fn aggregate_spans_two_buckets() {
        // 10 M1 bars → should produce 2 M5 bars
        let bars: Vec<Bar> = (0..10)
            .map(|i| m1_bar(ALIGNED_BASE + i * 60, dec!(1.10000)))
            .collect();

        let agg = aggregate_bars(&bars, Timeframe::M5).unwrap();
        assert_eq!(agg.len(), 2);
        assert_eq!(agg[0].timestamp, ALIGNED_BASE);
        assert_eq!(agg[1].timestamp, ALIGNED_BASE + 300);
    }

    #[test]
    fn streaming_aggregator_emits_on_boundary() {
        let mut sa = StreamAggregator::new(Timeframe::M5);

        // Feed 5 bars in first bucket — no emission yet
        for i in 0..5 {
            let result = sa.feed(&m1_bar(ALIGNED_BASE + i * 60, dec!(1.10000)));
            assert!(result.is_none());
        }

        // 6th bar crosses into next bucket — emits the first bucket
        let result = sa.feed(&m1_bar(ALIGNED_BASE + 300, dec!(1.10000)));
        assert!(result.is_some());
        assert_eq!(result.unwrap().timestamp, ALIGNED_BASE);

        // Flush the in-progress bar
        let flushed = sa.flush();
        assert!(flushed.is_some());
        assert_eq!(flushed.unwrap().timestamp, ALIGNED_BASE + 300);
    }

    #[test]
    fn multi_tf_aggregator() {
        let mut mtf = MultiTfAggregator::new();

        // Feed 60 M1 bars (= 1 hour) from H1-aligned base
        let mut m5_count = 0;
        let mut h1_count = 0;
        for i in 0..60 {
            let out = mtf.feed_m1(&m1_bar(ALIGNED_BASE + i * 60, dec!(1.10000)));
            if out.m5.is_some() {
                m5_count += 1;
            }
            if out.h1.is_some() {
                h1_count += 1;
            }
        }

        // In streaming mode, emissions happen when the *next* bucket starts.
        // 60 M1 bars at an aligned base: bars 0-4 in bucket 0, bar 5 emits bucket 0, etc.
        // Last bucket (bars 55-59) is still in-progress → 11 emitted.
        assert_eq!(m5_count, 11);
        // All 60 bars are in the same H1 bucket, so no H1 emission.
        assert_eq!(h1_count, 0);
    }

    #[test]
    fn empty_aggregation_errors() {
        let result = aggregate_bars(&[], Timeframe::M5);
        assert!(result.is_err());
    }
}
