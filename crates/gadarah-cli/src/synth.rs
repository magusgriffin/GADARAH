use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use gadarah_core::{Bar, Timeframe};

/// Generate realistic synthetic M15 bars for backtesting.
///
/// Simulates forex-like price action with:
/// - Random walk with mean reversion
/// - Session-dependent volatility (Asian=low, London/NY=high)
/// - Occasional trending periods
/// - Volume patterns correlated with session activity
pub fn generate_bars(
    _symbol: &str,
    num_bars: usize,
    start_price: Decimal,
    start_ts: i64,
    seed: u64,
) -> Vec<Bar> {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut bars = Vec::with_capacity(num_bars);
    let mut price = start_price;
    let mean_price = start_price;

    // State for trending periods
    let mut trend_bias: f64 = 0.0;
    let mut trend_bars_left: u32 = 0;

    for i in 0..num_bars {
        let ts = start_ts + (i as i64) * 900; // M15 = 900 seconds
        let utc_hour = ((ts % 86400) / 3600) as u8;

        // Session-dependent volatility (in pips)
        let base_vol_pips: f64 = match utc_hour {
            0..=6 => 3.0,   // Asian: low vol
            7..=8 => 8.0,   // London open: high vol
            9..=11 => 6.0,  // London: moderate-high
            12..=15 => 9.0, // Overlap: highest vol
            16..=20 => 5.0, // NY PM: moderate
            _ => 2.0,       // Dead: very low
        };

        // Session-dependent volume
        let base_volume: u64 = match utc_hour {
            0..=6 => 500 + rng.gen_range(0..300),
            7..=8 => 2000 + rng.gen_range(0..1500),
            9..=11 => 1500 + rng.gen_range(0..1000),
            12..=15 => 2500 + rng.gen_range(0..2000),
            16..=20 => 1000 + rng.gen_range(0..800),
            _ => 200 + rng.gen_range(0..200),
        };

        // Occasionally start a trending period
        if trend_bars_left == 0 && rng.gen::<f64>() < 0.02 {
            trend_bias = if rng.gen::<bool>() { 1.5 } else { -1.5 };
            trend_bars_left = rng.gen_range(10..40);
        }
        if trend_bars_left > 0 {
            trend_bars_left -= 1;
        } else {
            trend_bias = 0.0;
        }

        // Mean reversion component
        let price_f64 = decimal_to_f64(price);
        let mean_f64 = decimal_to_f64(mean_price);
        let reversion = (mean_f64 - price_f64) * 0.002;

        // Random walk + mean reversion + trend
        let move_pips =
            rng.gen::<f64>() * base_vol_pips * 2.0 - base_vol_pips + trend_bias + reversion;

        let pip_size = 0.0001; // forex major
        let price_change = move_pips * pip_size;

        // Generate OHLC from the move
        let open = price;
        let close_f64 = price_f64 + price_change;
        let close = f64_to_decimal(close_f64);

        // High and low: extend beyond open/close by random amount
        let wick_up = rng.gen::<f64>() * base_vol_pips * 0.5 * pip_size;
        let wick_down = rng.gen::<f64>() * base_vol_pips * 0.5 * pip_size;

        let high = open.max(close) + f64_to_decimal(wick_up);
        let low = open.min(close) - f64_to_decimal(wick_down);

        bars.push(Bar {
            open,
            high,
            low,
            close,
            volume: base_volume,
            timestamp: ts,
            timeframe: Timeframe::M15,
        });

        price = close;
    }

    bars
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_string().parse::<f64>().unwrap_or(0.0)
}

fn f64_to_decimal(f: f64) -> Decimal {
    Decimal::from_str_exact(&format!("{:.5}", f)).unwrap_or(dec!(1.10000))
}

/// Generate ~2 years of M15 bars with realistic forex session structure.
///
/// Key patterns injected:
/// - Asian session (00-07): tight range, low volatility
/// - London open (07-08): directional breakout from Asian range (~40% of days)
/// - London (09-11): continuation or reversion
/// - NY overlap (12-15): highest volatility, strong trends
/// - NY PM (16-20): fading volatility
/// - Periodic squeeze → expansion cycles (for BreakoutHead)
/// - Volume spikes on breakout bars
pub fn generate_2y_bars(_symbol: &str, start_price: Decimal, seed: u64) -> Vec<Bar> {
    let start_ts: i64 = 1672531200; // 2023-01-01 00:00:00 UTC
    let total_potential = 105 * 7 * 96;

    let mut rng = StdRng::seed_from_u64(seed);
    let mut bars = Vec::with_capacity(50_000);
    let mut price = start_price;
    let mean_price = decimal_to_f64(start_price);

    // Daily state
    let mut day_direction: f64 = 0.0; // +1 or -1, set at London open
    let mut day_strength: f64 = 0.0; // how strong the day's trend is
    let mut last_day: i64 = -1;
    let mut asian_high: f64 = 0.0;
    let mut asian_low: f64 = f64::MAX;

    // Squeeze/expansion cycle
    let mut squeeze_phase: bool = false;
    let mut squeeze_bars_left: u32 = 0;
    let mut expansion_bars_left: u32 = 0;

    // Multi-day trend
    let mut trend_bias: f64 = 0.0;
    let mut trend_bars_left: u32 = 0;

    for i in 0..total_potential {
        let ts = start_ts + (i as i64) * 900;
        let days_since_epoch = ts / 86400;
        let dow = ((days_since_epoch + 4) % 7) as u8;
        if dow >= 5 {
            continue;
        }

        let utc_hour = ((ts % 86400) / 3600) as u8;
        let _utc_minute = ((ts % 3600) / 60) as u8;
        let day = ts / 86400;
        let price_f64 = decimal_to_f64(price);

        // --- Daily reset ---
        if day != last_day {
            last_day = day;
            asian_high = 0.0;
            asian_low = f64::MAX;
            // Random day direction (set properly at London open)
            day_direction = 0.0;
            day_strength = rng.gen_range(0.3..1.5);
        }

        // --- Multi-day trend (starts every ~2 weeks, lasts 1-3 weeks) ---
        if trend_bars_left == 0 && rng.gen::<f64>() < 0.005 {
            trend_bias = if rng.gen::<bool>() { 0.8 } else { -0.8 };
            trend_bars_left = rng.gen_range(200..700);
        }
        if trend_bars_left > 0 {
            trend_bars_left -= 1;
        } else {
            trend_bias *= 0.95; // decay
        }

        // --- Squeeze/expansion cycle ---
        if squeeze_bars_left == 0 && expansion_bars_left == 0 && rng.gen::<f64>() < 0.01 {
            squeeze_phase = true;
            squeeze_bars_left = rng.gen_range(12..30); // 3-7 hours of squeeze
        }
        if squeeze_bars_left > 0 {
            squeeze_bars_left -= 1;
            if squeeze_bars_left == 0 {
                squeeze_phase = false;
                expansion_bars_left = rng.gen_range(4..12);
            }
        }
        expansion_bars_left = expansion_bars_left.saturating_sub(1);

        // --- Session-based volatility ---
        let (base_vol, base_volume, session_bias) = match utc_hour {
            0..=6 => {
                // Asian: tight range, low vol, track high/low
                let vol = if squeeze_phase { 1.5 } else { 2.5 };
                let volume = 400 + rng.gen_range(0..300);
                (vol, volume, 0.0)
            }
            7 => {
                // London open: directional breakout from Asian range
                if day_direction == 0.0 {
                    // Decide direction based on trend + randomness
                    let r: f64 = rng.gen();
                    day_direction = if r + trend_bias * 0.3 > 0.5 {
                        1.0
                    } else {
                        -1.0
                    };
                }
                let vol = if expansion_bars_left > 0 { 14.0 } else { 10.0 };
                let volume = 2500 + rng.gen_range(0..2000);
                let bias = day_direction * day_strength * 3.0;
                (vol, volume, bias)
            }
            8 => {
                // London open continuation
                let vol = if expansion_bars_left > 0 { 12.0 } else { 8.0 };
                let volume = 2000 + rng.gen_range(0..1500);
                let bias = day_direction * day_strength * 2.0;
                (vol, volume, bias)
            }
            9..=11 => {
                // London session: moderate, some continuation
                let vol = if squeeze_phase { 3.0 } else { 6.0 };
                let volume = 1500 + rng.gen_range(0..1000);
                let bias = day_direction * day_strength * 0.5;
                (vol, volume, bias)
            }
            12..=15 => {
                // NY overlap: highest vol, fresh momentum
                let vol = if expansion_bars_left > 0 { 15.0 } else { 9.0 };
                let volume = 3000 + rng.gen_range(0..2500);
                // NY can continue or reverse the day's direction
                let ny_bias = if rng.gen::<f64>() < 0.6 {
                    day_direction * day_strength * 2.0
                } else {
                    -day_direction * day_strength * 1.5
                };
                (vol, volume, ny_bias)
            }
            16..=20 => {
                // NY PM: fading
                let vol = if squeeze_phase { 2.5 } else { 4.0 };
                let volume = 800 + rng.gen_range(0..600);
                (vol, volume, day_direction * 0.3)
            }
            _ => (1.5, 200 + rng.gen_range(0..200), 0.0),
        };

        // --- Price move ---
        let mean_reversion = (mean_price - price_f64) * 0.001;
        let random_component = (rng.gen::<f64>() - 0.5) * 2.0 * base_vol;
        let move_pips = random_component + session_bias + trend_bias + mean_reversion;

        let pip_size = 0.0001;
        let new_price = price_f64 + move_pips * pip_size;

        let open = price;
        let close = f64_to_decimal(new_price);

        // Wicks: proportional to volatility
        let wick_up = rng.gen::<f64>() * base_vol * 0.4 * pip_size;
        let wick_down = rng.gen::<f64>() * base_vol * 0.4 * pip_size;
        let high = open.max(close) + f64_to_decimal(wick_up);
        let low = open.min(close) - f64_to_decimal(wick_down);

        // Track Asian range
        if utc_hour < 7 {
            asian_high = asian_high.max(decimal_to_f64(high));
            asian_low = asian_low.min(decimal_to_f64(low));
        }

        // Volume spike on expansion bars
        let final_volume = if expansion_bars_left > 0 {
            (base_volume as f64 * 1.8) as u64
        } else if squeeze_phase {
            (base_volume as f64 * 0.6) as u64
        } else {
            base_volume
        };

        bars.push(Bar {
            open,
            high,
            low,
            close,
            volume: final_volume,
            timestamp: ts,
            timeframe: Timeframe::M15,
        });

        price = close;
    }

    bars
}
