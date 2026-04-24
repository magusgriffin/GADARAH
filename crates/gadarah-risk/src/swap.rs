//! Per-symbol swap (overnight rollover) rate table.
//!
//! Swap is the interest differential that brokers credit or debit on
//! positions held past the 5 pm America/New_York daily rollover. The rate
//! varies by pair and direction; Wednesday triple-swap is the standard
//! weekend-carry convention that we also honor here.
//!
//! Values are broker- and symbol-dependent. The table below is a
//! conservative snapshot drawn from major prop-firm cTrader feeds as of
//! 2026-04: rates in **USD per standard lot per night**, negative meaning
//! the position holder is debited. When the symbol is missing from the
//! table we return zero — correct for instruments we don't trade, and a
//! safe default (no free credit, no surprise debit) for anything else.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use chrono::{DateTime, Datelike, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::America::New_York;
use gadarah_core::Direction;

/// Seed rates: `(symbol, long_per_lot_usd, short_per_lot_usd)`. Real
/// numbers — e.g. USDJPY long pays (+$0.31) and USDJPY short is debited
/// (−$1.26) at the time of writing. Values update periodically; the
/// backtest engine pins a snapshot so historical runs stay deterministic.
const SEED_RATES: &[(&str, Decimal, Decimal)] = &[
    ("EURUSD", dec!(-7.50), dec!(2.80)),
    ("GBPUSD", dec!(-6.10), dec!(1.50)),
    ("USDJPY", dec!(0.31), dec!(-1.26)),
    ("AUDUSD", dec!(-4.60), dec!(0.70)),
    ("NZDUSD", dec!(-5.80), dec!(1.90)),
    ("USDCAD", dec!(-0.42), dec!(-3.10)),
    ("USDCHF", dec!(1.10), dec!(-5.40)),
    ("EURJPY", dec!(-6.20), dec!(1.90)),
    ("GBPJPY", dec!(-4.90), dec!(0.50)),
    ("EURGBP", dec!(-1.20), dec!(-1.10)),
    ("XAUUSD", dec!(-8.20), dec!(2.10)),
];

/// Lookup table keyed by symbol.
#[derive(Debug, Clone)]
pub struct SwapTable {
    rates: HashMap<String, (Decimal, Decimal)>,
}

impl SwapTable {
    /// Seed table populated from the built-in snapshot.
    pub fn seeded() -> Self {
        let mut rates = HashMap::new();
        for (sym, long, short) in SEED_RATES {
            rates.insert((*sym).to_string(), (*long, *short));
        }
        Self { rates }
    }

    /// Replace or insert a swap rate.
    pub fn set(&mut self, symbol: &str, long_per_lot: Decimal, short_per_lot: Decimal) {
        self.rates
            .insert(symbol.to_string(), (long_per_lot, short_per_lot));
    }

    /// Per-lot swap charge in USD for this symbol + direction. Returns zero
    /// when the symbol is not in the table. Positive values are credits,
    /// negative are debits.
    pub fn per_lot(&self, symbol: &str, direction: Direction) -> Decimal {
        match self.rates.get(symbol) {
            Some((long, short)) => match direction {
                Direction::Buy => *long,
                Direction::Sell => *short,
            },
            None => Decimal::ZERO,
        }
    }

    /// Total swap charge for a position held across the 5 pm NY rollover.
    /// Wednesday rollovers bill 3× as compensation for the settlement
    /// weekend (T+2 convention on FX). Returns the USD P&L delta — add it
    /// to account equity at the rollover tick.
    pub fn charge_for_rollover(
        &self,
        symbol: &str,
        direction: Direction,
        lots: Decimal,
        rollover_utc_ts: i64,
    ) -> Decimal {
        let per_lot = self.per_lot(symbol, direction);
        if per_lot.is_zero() {
            return Decimal::ZERO;
        }
        let multiplier = if is_triple_swap_day(rollover_utc_ts) {
            dec!(3)
        } else {
            dec!(1)
        };
        per_lot * lots * multiplier
    }
}

impl Default for SwapTable {
    fn default() -> Self {
        Self::seeded()
    }
}

/// Return true when the 5 pm NY rollover of `ts` lands on a Wednesday —
/// the standard triple-swap day that books the three nights of interest
/// covering the Saturday/Sunday carry (FX settles T+2).
pub fn is_triple_swap_day(ts: i64) -> bool {
    let utc_dt: DateTime<Utc> = match Utc.timestamp_opt(ts, 0).single() {
        Some(dt) => dt,
        None => return false,
    };
    let ny = utc_dt.with_timezone(&New_York);
    // 17:00 NY boundary — if we're past the cutoff, the rollover we just
    // crossed carries the current day's label.
    let effective = if ny.hour() < 17 {
        ny - chrono::Duration::days(1)
    } else {
        ny
    };
    effective.weekday() == Weekday::Wed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_table_has_known_pairs() {
        let t = SwapTable::seeded();
        assert_ne!(t.per_lot("EURUSD", Direction::Buy), Decimal::ZERO);
        assert_ne!(t.per_lot("XAUUSD", Direction::Sell), Decimal::ZERO);
    }

    #[test]
    fn unknown_symbol_returns_zero() {
        let t = SwapTable::seeded();
        assert_eq!(t.per_lot("UNKNOWN", Direction::Buy), Decimal::ZERO);
    }

    #[test]
    fn triple_swap_fires_on_wednesday_after_nyclose() {
        // 2025-05-07 22:00 UTC = Wed 18:00 NY (EDT, so UTC-4). Past the
        // 17:00 cutoff → rollover is logged under Wed → triple.
        let wed_after_close = 1_746_655_200; // 2025-05-07 22:00 UTC
        assert!(is_triple_swap_day(wed_after_close));
    }

    #[test]
    fn triple_swap_does_not_fire_on_tuesday() {
        // 2025-05-06 22:00 UTC = Tue 18:00 NY. Rollover is Tue, not Wed.
        let tue_after_close = 1_746_568_800; // 2025-05-06 22:00 UTC
        assert!(!is_triple_swap_day(tue_after_close));
    }
}
