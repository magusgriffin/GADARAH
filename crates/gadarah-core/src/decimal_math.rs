use rust_decimal::Decimal;
use rust_decimal_macros::dec;

/// Square root of a `Decimal` via Newton's method (10 iterations).
///
/// Returns `Decimal::ZERO` for zero input.
/// Panics on negative input (caller is responsible for ensuring non-negative).
pub fn decimal_sqrt(value: Decimal) -> Decimal {
    if value.is_zero() {
        return Decimal::ZERO;
    }
    if value < Decimal::ZERO {
        // Defensive: return zero rather than panic in production-critical code.
        return Decimal::ZERO;
    }

    // Initial guess: start at value/2 clamped to a reasonable range.
    // For very small values (< 1) use value itself as the initial guess
    // so we don't overshoot.
    let mut guess = if value < dec!(1) {
        value
    } else {
        value / dec!(2)
    };

    // Refine: if guess is zero (possible for very tiny decimals), use a small seed
    if guess.is_zero() {
        guess = dec!(0.0000000001);
    }

    for _ in 0..20 {
        // Newton step: guess = (guess + value / guess) / 2
        let next = (guess + value / guess) / dec!(2);
        // Early termination if converged
        if (next - guess).abs() < dec!(0.00000000000001) {
            return next;
        }
        guess = next;
    }

    guess
}

/// Natural logarithm of a positive `Decimal`.
///
/// Uses the identity: ln(x) = ln(m * 2^e) = ln(m) + e*ln(2),
/// where m is scaled to [0.5, 2.0) so the Taylor series converges quickly.
///
/// For the series part, we use the expansion around 1:
///   ln(1+u) = u - u^2/2 + u^3/3 - u^4/4 + ...
/// where u = (m - 1).
///
/// Returns `Decimal::ZERO` for values <= 0 (defensive, should not be called with non-positive).
pub fn decimal_ln(value: Decimal) -> Decimal {
    if value <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    if value == dec!(1) {
        return Decimal::ZERO;
    }

    // ln(2) pre-computed to high precision
    let ln2 = dec!(0.6931471805599453);

    // Scale x into the range [0.5, 2.0) by repeatedly dividing/multiplying by 2.
    let mut x = value;
    let mut exponent: i64 = 0;

    // Scale down if x >= 2
    while x >= dec!(2) {
        x /= dec!(2);
        exponent += 1;
    }
    // Scale up if x < 0.5
    while x < dec!(0.5) {
        x *= dec!(2);
        exponent -= 1;
    }

    // Now x is in [0.5, 2.0). Compute ln(x) using the series ln(1+u) where u = x - 1.
    // u is in [-0.5, 1.0).
    // For better convergence when |u| is large (close to 1.0), we use the
    // identity: ln(x) = 2 * atanh((x-1)/(x+1))
    // where atanh(t) = t + t^3/3 + t^5/5 + t^7/7 + ...
    // This converges for |t| < 1, and t = (x-1)/(x+1) is always in (-1/3, 1/3)
    // for x in [0.5, 2.0), giving fast convergence.

    let t = (x - dec!(1)) / (x + dec!(1));
    let t2 = t * t;
    let mut term = t;
    let mut sum = t;

    // 30 terms of the series: enough for ~28-digit precision within our t range
    for i in 1..30 {
        term *= t2;
        let denom = Decimal::from(2 * i + 1);
        sum += term / denom;
    }

    let ln_x = dec!(2) * sum;

    // ln(value) = ln(x) + exponent * ln(2)
    ln_x + Decimal::from(exponent) * ln2
}
