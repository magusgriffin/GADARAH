use std::collections::{HashMap, VecDeque};

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use gadarah_core::Direction;

// ---------------------------------------------------------------------------
// Correlation Guard — prevents double exposure on correlated pairs
// ---------------------------------------------------------------------------

/// Static correlation coefficients between major FX pairs.
/// Derived from historical daily returns.  Positive = move together,
/// negative = move opposite.  Values above the threshold (default 0.70)
/// are treated as "same trade idea" and the guard blocks the second entry.
const PAIR_CORRELATIONS: &[(&str, &str, f64)] = &[
    ("EURUSD", "GBPUSD", 0.87),
    ("EURUSD", "AUDUSD", 0.75),
    ("EURUSD", "NZDUSD", 0.72),
    ("EURUSD", "USDCHF", -0.92),
    ("EURUSD", "USDJPY", -0.30),
    ("GBPUSD", "AUDUSD", 0.68),
    ("GBPUSD", "NZDUSD", 0.62),
    ("GBPUSD", "USDCHF", -0.85),
    ("AUDUSD", "NZDUSD", 0.94),
    ("AUDUSD", "USDCAD", -0.60),
    ("USDJPY", "USDCHF", 0.42),
    ("XAUUSD", "EURUSD", 0.40),
    ("XAUUSD", "USDCHF", -0.45),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrelationGuardConfig {
    /// Minimum |correlation| to consider two pairs "correlated".
    pub threshold: f64,
    /// Maximum net directional exposure across correlated pairs.
    /// e.g. 2 means at most 2 positions on the same side of a correlated cluster.
    pub max_cluster_exposure: usize,
    /// Minimum samples required before the rolling estimator is trusted over
    /// the static seed table.
    pub rolling_min_samples: usize,
    /// Maximum rolling-window length (samples). Older samples fall off.
    pub rolling_window: usize,
    /// Aggregate unrealized loss (expressed as % of starting balance) across
    /// an aligned correlated cluster that triggers `ReduceCluster` instead of
    /// a plain block. Negative values here are losses; the comparison is
    /// `sum(pnl_pct) < -cluster_loss_threshold_pct`.
    pub cluster_loss_threshold_pct: Decimal,
}

impl Default for CorrelationGuardConfig {
    fn default() -> Self {
        Self {
            threshold: 0.70,
            max_cluster_exposure: 2,
            rolling_min_samples: 100,
            rolling_window: 500,
            cluster_loss_threshold_pct: dec!(1.0),
        }
    }
}

/// Per-symbol log-return buffer used for rolling Pearson correlation.
#[derive(Debug, Clone)]
pub struct RollingReturns {
    prev_price: Option<f64>,
    returns: VecDeque<f64>,
    capacity: usize,
}

impl RollingReturns {
    pub fn new(capacity: usize) -> Self {
        Self {
            prev_price: None,
            returns: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, price: f64) {
        if price <= 0.0 || !price.is_finite() {
            return;
        }
        if let Some(prev) = self.prev_price {
            if prev > 0.0 {
                let r = (price / prev).ln();
                if r.is_finite() {
                    if self.returns.len() == self.capacity {
                        self.returns.pop_front();
                    }
                    self.returns.push_back(r);
                }
            }
        }
        self.prev_price = Some(price);
    }

    pub fn len(&self) -> usize {
        self.returns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.returns.is_empty()
    }
}

/// Snapshot of an open position used by `CorrelationGuard::evaluate` to
/// decide whether a new entry should be blocked or a rebalance requested.
#[derive(Debug, Clone)]
pub struct PositionRef {
    pub id: u64,
    pub symbol: String,
    pub direction: Direction,
    /// Unix seconds the position was opened. Used to pick the oldest loser
    /// for `ReduceCluster`.
    pub opened_at: i64,
    /// Unrealized PnL as a percent of starting balance (negative = loss).
    pub unrealized_pnl_pct: Decimal,
}

/// What the guard recommends for a prospective new entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PortfolioAction {
    /// Accept the new position.
    Allow,
    /// Reject the new position; correlated cluster is at exposure cap but
    /// not yet in pain.
    Block { reason: String },
    /// Reject the new position AND close an existing correlated one because
    /// the cluster is already bleeding beyond `cluster_loss_threshold_pct`.
    ReduceCluster {
        reason: String,
        close_position_id: u64,
    },
}

#[derive(Debug, Clone)]
pub struct CorrelationGuard {
    config: CorrelationGuardConfig,
    correlations: HashMap<(String, String), f64>,
    rolling: HashMap<String, RollingReturns>,
}

impl CorrelationGuard {
    pub fn new(config: CorrelationGuardConfig) -> Self {
        let mut correlations = HashMap::new();
        for &(a, b, corr) in PAIR_CORRELATIONS {
            correlations.insert((a.to_string(), b.to_string()), corr);
            correlations.insert((b.to_string(), a.to_string()), corr);
        }
        Self {
            config,
            correlations,
            rolling: HashMap::new(),
        }
    }

    /// Ingest a fresh price observation for `symbol`. The engine derives a
    /// log-return from the previous observation and stores up to
    /// `rolling_window` samples.
    pub fn ingest_price(&mut self, symbol: &str, price: Decimal) {
        let price_f64 = match price.to_f64() {
            Some(p) if p > 0.0 && p.is_finite() => p,
            _ => return,
        };
        let capacity = self.config.rolling_window;
        self.rolling
            .entry(symbol.to_string())
            .or_insert_with(|| RollingReturns::new(capacity))
            .push(price_f64);
    }

    /// Rolling Pearson correlation over the overlap of two symbols' return
    /// series. Returns `None` when either side has fewer than
    /// `rolling_min_samples` observations.
    fn rolling_correlation(&self, a: &str, b: &str) -> Option<f64> {
        let ra = self.rolling.get(a)?;
        let rb = self.rolling.get(b)?;
        let min_n = self.config.rolling_min_samples;
        if ra.len() < min_n || rb.len() < min_n {
            return None;
        }
        let overlap = ra.len().min(rb.len());
        if overlap < min_n {
            return None;
        }
        let ra_slice: Vec<f64> = ra.returns.iter().rev().take(overlap).copied().collect();
        let rb_slice: Vec<f64> = rb.returns.iter().rev().take(overlap).copied().collect();
        pearson(&ra_slice, &rb_slice)
    }

    /// Get the correlation between two symbols. Prefers the rolling estimator
    /// once ≥`rolling_min_samples` observations are available on both sides;
    /// falls back to the static seed table otherwise. Returns 0.0 for
    /// unknown pairs, 1.0 for the same symbol.
    pub fn correlation(&self, a: &str, b: &str) -> f64 {
        if a == b {
            return 1.0;
        }
        if let Some(c) = self.rolling_correlation(a, b) {
            return c;
        }
        self.correlations
            .get(&(a.to_string(), b.to_string()))
            .copied()
            .unwrap_or(0.0)
    }

    /// Legacy API — kept so existing callers continue to compile. Returns
    /// `Some(reason)` iff the guard would block the new entry.
    pub fn check(
        &self,
        symbol: &str,
        direction: Direction,
        open_positions: &[(String, Direction)],
    ) -> Option<String> {
        // Translate the caller's minimal tuple into PositionRef with neutral
        // PnL so the cluster-loss test cannot fire — preserves the original
        // "block-only" semantics of this function.
        let positions: Vec<PositionRef> = open_positions
            .iter()
            .enumerate()
            .map(|(i, (sym, dir))| PositionRef {
                id: i as u64,
                symbol: sym.clone(),
                direction: *dir,
                opened_at: 0,
                unrealized_pnl_pct: Decimal::ZERO,
            })
            .collect();

        match self.evaluate(symbol, direction, &positions) {
            PortfolioAction::Block { reason } | PortfolioAction::ReduceCluster { reason, .. } => {
                Some(reason)
            }
            PortfolioAction::Allow => None,
        }
    }

    /// Full rebalance-aware check. Returns `Allow` when the entry is fine,
    /// `Block` when the cluster is at the exposure cap without significant
    /// unrealized loss, or `ReduceCluster` when the aligned cluster is
    /// already bleeding below `cluster_loss_threshold_pct` — callers should
    /// honor the recommendation and close `close_position_id`.
    pub fn evaluate(
        &self,
        symbol: &str,
        direction: Direction,
        open_positions: &[PositionRef],
    ) -> PortfolioAction {
        let mut aligned: Vec<&PositionRef> = Vec::new();

        for pos in open_positions {
            let corr = self.correlation(symbol, &pos.symbol);
            if corr.abs() < self.config.threshold {
                continue;
            }
            let effectively_same_direction = if corr > 0.0 {
                direction == pos.direction
            } else {
                direction != pos.direction
            };
            if effectively_same_direction {
                aligned.push(pos);
            }
        }

        if aligned.len() < self.config.max_cluster_exposure {
            return PortfolioAction::Allow;
        }

        let side_label = match direction {
            Direction::Buy => "Buy",
            Direction::Sell => "Sell",
        };
        let reason = format!(
            "Correlation guard: {} {} would create {} aligned correlated positions (max {})",
            symbol,
            side_label,
            aligned.len() + 1,
            self.config.max_cluster_exposure,
        );

        let cluster_pnl: Decimal = aligned.iter().map(|p| p.unrealized_pnl_pct).sum();
        if cluster_pnl < -self.config.cluster_loss_threshold_pct {
            // Pick the oldest entry in the cluster — that one has had the
            // most time to work and has the least recency bias on the edge
            // we originally entered for.
            let oldest = aligned.iter().min_by_key(|p| p.opened_at).copied();
            if let Some(victim) = oldest {
                return PortfolioAction::ReduceCluster {
                    reason: format!(
                        "{reason}; cluster down {:.2}% — reduce oldest position",
                        cluster_pnl
                    ),
                    close_position_id: victim.id,
                };
            }
        }

        PortfolioAction::Block { reason }
    }
}

fn pearson(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.len() < 2 {
        return None;
    }
    let n = a.len() as f64;
    let mean_a = a.iter().sum::<f64>() / n;
    let mean_b = b.iter().sum::<f64>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..a.len() {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    let denom = (var_a * var_b).sqrt();
    if denom == 0.0 || !denom.is_finite() {
        return None;
    }
    let c = cov / denom;
    if c.is_finite() {
        Some(c.clamp(-1.0, 1.0))
    } else {
        None
    }
}

impl Default for CorrelationGuard {
    fn default() -> Self {
        Self::new(CorrelationGuardConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_symbol_correlation_is_one() {
        let guard = CorrelationGuard::default();
        assert_eq!(guard.correlation("EURUSD", "EURUSD"), 1.0);
    }

    #[test]
    fn known_pair_returns_correlation() {
        let guard = CorrelationGuard::default();
        let c = guard.correlation("EURUSD", "GBPUSD");
        assert!(c > 0.80);
    }

    #[test]
    fn unknown_pair_returns_zero() {
        let guard = CorrelationGuard::default();
        assert_eq!(guard.correlation("EURUSD", "BTCUSD"), 0.0);
    }

    #[test]
    fn blocks_when_cluster_exposure_exceeded() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 1,
            ..CorrelationGuardConfig::default()
        });
        let open = vec![("EURUSD".to_string(), Direction::Buy)];
        // GBPUSD is correlated with EURUSD at 0.87 — max_cluster_exposure=1 means
        // even one aligned correlated position blocks the next
        let result = guard.check("GBPUSD", Direction::Buy, &open);
        assert!(result.is_some());
    }

    #[test]
    fn allows_within_cluster_limit() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 2,
            ..CorrelationGuardConfig::default()
        });
        let open = vec![("EURUSD".to_string(), Direction::Buy)];
        // One correlated position is fine when limit is 2
        let result = guard.check("GBPUSD", Direction::Buy, &open);
        assert!(result.is_none());
    }

    #[test]
    fn allows_uncorrelated_position() {
        let guard = CorrelationGuard::default();
        let open = vec![
            ("EURUSD".to_string(), Direction::Buy),
            ("GBPUSD".to_string(), Direction::Buy),
        ];
        // USDJPY has low correlation with EUR/GBP — should be allowed
        let result = guard.check("USDJPY", Direction::Buy, &open);
        assert!(result.is_none());
    }

    #[test]
    fn negative_correlation_opposite_direction_counts_as_aligned() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 1,
            ..CorrelationGuardConfig::default()
        });
        let open = vec![("EURUSD".to_string(), Direction::Buy)];
        // USDCHF is -0.92 correlated with EURUSD
        // Selling USDCHF while long EURUSD = same directional bet
        let result = guard.check("USDCHF", Direction::Sell, &open);
        assert!(result.is_some());
    }

    #[test]
    fn ingest_price_builds_rolling_returns() {
        let mut guard = CorrelationGuard::default();
        guard.ingest_price("EURUSD", dec!(1.1000));
        guard.ingest_price("EURUSD", dec!(1.1010));
        guard.ingest_price("EURUSD", dec!(1.1020));
        let r = guard.rolling.get("EURUSD").unwrap();
        assert_eq!(r.len(), 2); // first price sets baseline, next 2 produce returns
    }

    #[test]
    fn rolling_correlation_detects_perfect_positive() {
        let mut guard = CorrelationGuard::default();
        // Feed the same price series to two symbols — returns are identical,
        // pearson ≈ 1.0.
        for i in 0..200 {
            let px = 1.0 + (i as f64) * 0.0001;
            guard.ingest_price("SYM_A", Decimal::try_from(px).unwrap());
            guard.ingest_price("SYM_B", Decimal::try_from(px).unwrap());
        }
        let c = guard.correlation("SYM_A", "SYM_B");
        assert!(c > 0.99, "expected ~1.0 correlation, got {c}");
    }

    #[test]
    fn rolling_wins_over_static_seed_when_enough_samples() {
        let mut guard = CorrelationGuard::default();
        // EURUSD/GBPUSD static seed is +0.87. Feed genuinely anti-correlated
        // returns so rolling should dominate with a strongly negative value.
        let mut px_a = 1.0f64;
        let mut px_b = 1.0f64;
        guard.ingest_price("EURUSD", Decimal::try_from(px_a).unwrap());
        guard.ingest_price("GBPUSD", Decimal::try_from(px_b).unwrap());
        for i in 0..200 {
            let delta = if i % 2 == 0 { 0.001 } else { -0.001 };
            px_a *= 1.0 + delta;
            px_b *= 1.0 - delta;
            guard.ingest_price("EURUSD", Decimal::try_from(px_a).unwrap());
            guard.ingest_price("GBPUSD", Decimal::try_from(px_b).unwrap());
        }
        let c = guard.correlation("EURUSD", "GBPUSD");
        assert!(
            c < -0.9,
            "rolling anti-correlation should override +0.87 static seed; got {c}"
        );
    }

    #[test]
    fn static_seed_used_when_rolling_samples_sparse() {
        let mut guard = CorrelationGuard::default();
        // Feed just 5 samples — well below rolling_min_samples = 100.
        for i in 0..6 {
            let px = 1.0 + (i as f64) * 0.001;
            guard.ingest_price("EURUSD", Decimal::try_from(px).unwrap());
            guard.ingest_price("GBPUSD", Decimal::try_from(px).unwrap());
        }
        // Falls back to the +0.87 static seed
        let c = guard.correlation("EURUSD", "GBPUSD");
        assert!((c - 0.87).abs() < 1e-9);
    }

    #[test]
    fn evaluate_returns_reduce_cluster_when_losing() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 1,
            rolling_min_samples: 100,
            rolling_window: 500,
            cluster_loss_threshold_pct: dec!(0.5),
        });
        let open = vec![PositionRef {
            id: 42,
            symbol: "EURUSD".to_string(),
            direction: Direction::Buy,
            opened_at: 1_000,
            unrealized_pnl_pct: dec!(-0.80), // -0.80% loss
        }];
        let action = guard.evaluate("GBPUSD", Direction::Buy, &open);
        match action {
            PortfolioAction::ReduceCluster {
                close_position_id, ..
            } => assert_eq!(close_position_id, 42),
            other => panic!("expected ReduceCluster, got {other:?}"),
        }
    }

    #[test]
    fn evaluate_returns_block_when_cluster_not_losing_enough() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 1,
            rolling_min_samples: 100,
            rolling_window: 500,
            cluster_loss_threshold_pct: dec!(1.0),
        });
        let open = vec![PositionRef {
            id: 1,
            symbol: "EURUSD".to_string(),
            direction: Direction::Buy,
            opened_at: 1_000,
            unrealized_pnl_pct: dec!(-0.20),
        }];
        let action = guard.evaluate("GBPUSD", Direction::Buy, &open);
        assert!(matches!(action, PortfolioAction::Block { .. }));
    }

    #[test]
    fn allows_hedge_on_negatively_correlated_pair() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 1,
            ..CorrelationGuardConfig::default()
        });
        let open = vec![("EURUSD".to_string(), Direction::Buy)];
        // Buying USDCHF while long EURUSD = hedge (opposite bet)
        let result = guard.check("USDCHF", Direction::Buy, &open);
        assert!(result.is_none());
    }
}
