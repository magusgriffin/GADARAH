use std::collections::HashMap;

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
}

impl Default for CorrelationGuardConfig {
    fn default() -> Self {
        Self {
            threshold: 0.70,
            max_cluster_exposure: 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CorrelationGuard {
    config: CorrelationGuardConfig,
    correlations: HashMap<(String, String), f64>,
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
        }
    }

    /// Get the correlation between two symbols.  Returns 0.0 for unknown pairs,
    /// 1.0 for the same symbol.
    pub fn correlation(&self, a: &str, b: &str) -> f64 {
        if a == b {
            return 1.0;
        }
        self.correlations
            .get(&(a.to_string(), b.to_string()))
            .copied()
            .unwrap_or(0.0)
    }

    /// Check whether a new position on `symbol` / `direction` would create
    /// excessive correlated exposure given the current `open_positions`.
    ///
    /// Returns `None` if the trade is allowed, or `Some(reason)` if blocked.
    pub fn check(
        &self,
        symbol: &str,
        direction: Direction,
        open_positions: &[(String, Direction)],
    ) -> Option<String> {
        let mut aligned_count = 0usize;

        for (pos_symbol, pos_dir) in open_positions {
            let corr = self.correlation(symbol, pos_symbol);
            let abs_corr = corr.abs();

            if abs_corr < self.config.threshold {
                continue;
            }

            // Determine if the new trade is directionally aligned with the
            // existing position after accounting for correlation sign.
            let effectively_same_direction = if corr > 0.0 {
                // Positive correlation: same direction = aligned
                direction == *pos_dir
            } else {
                // Negative correlation: opposite direction = aligned
                // (e.g. long EURUSD + short USDCHF = same bet)
                direction != *pos_dir
            };

            if effectively_same_direction {
                aligned_count += 1;
            }
        }

        if aligned_count >= self.config.max_cluster_exposure {
            Some(format!(
                "Correlation guard: {} {} would create {} aligned correlated positions (max {})",
                symbol,
                match direction {
                    Direction::Buy => "Buy",
                    Direction::Sell => "Sell",
                },
                aligned_count + 1,
                self.config.max_cluster_exposure,
            ))
        } else {
            None
        }
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
        });
        let open = vec![("EURUSD".to_string(), Direction::Buy)];
        // USDCHF is -0.92 correlated with EURUSD
        // Selling USDCHF while long EURUSD = same directional bet
        let result = guard.check("USDCHF", Direction::Sell, &open);
        assert!(result.is_some());
    }

    #[test]
    fn allows_hedge_on_negatively_correlated_pair() {
        let guard = CorrelationGuard::new(CorrelationGuardConfig {
            threshold: 0.70,
            max_cluster_exposure: 1,
        });
        let open = vec![("EURUSD".to_string(), Direction::Buy)];
        // Buying USDCHF while long EURUSD = hedge (opposite bet)
        let result = guard.check("USDCHF", Direction::Buy, &open);
        assert!(result.is_none());
    }
}
