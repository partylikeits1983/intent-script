//! Price ⇄ tick conversions for Uniswap V3 concentrated LP positions.
//!
//! End users reason in prices ("1 WETH = 3000 USDC"); the protocol reasons in
//! ticks, where `price_raw = 1.0001^tick` and `price_raw` is the *raw* (pre-
//! decimals) ratio of `token1 / token0`. This module is the bridge.

use alloc::format;
use alloc::string::ToString;

use crate::error::{CompileError, Result};

/// Uniswap V3 tick bounds from TickMath.sol.
pub const MIN_TICK: i32 = -887272;
pub const MAX_TICK: i32 = 887272;

/// A parsed `price_lower` / `price_upper` value. `Min`/`Max` are the sentinel
/// strings users may supply for a full-range bound.
pub enum PriceBound<'a> {
    Min,
    Max,
    Explicit(&'a str),
}

impl<'a> PriceBound<'a> {
    pub fn parse(raw: &'a str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "min" => PriceBound::Min,
            "max" => PriceBound::Max,
            _ => PriceBound::Explicit(raw),
        }
    }
}

/// Convert a human-readable price into a Uniswap V3 tick.
///
/// `price_human` is in **`quote_token` per 1 unit of the other token** (human
/// units — so "0.999" for USDC/USDT means 1 USDC = 0.999 USDT).
/// `quote_is_token1` selects direction: when `true`, price is token1-per-token0
/// (Uniswap's canonical direction); when `false`, it's token0-per-token1 and
/// we invert before applying the formula.
///
/// The raw (on-chain) price accounts for the decimals gap between the two
/// tokens: `price_raw = price_human * 10^(decimals1 - decimals0)` in the
/// canonical direction.
///
/// Returns the floor-rounded tick; callers snap to fee-tier spacing via
/// [`snap_range`].
pub fn price_to_tick(
    price_human: &str,
    quote_is_token1: bool,
    decimals0: u8,
    decimals1: u8,
) -> Result<i32> {
    let mut p: f64 = price_human.parse().map_err(|_| {
        CompileError::InvalidAmount(format!(
            "LP price '{}' is not a valid decimal number",
            price_human
        ))
    })?;
    if !p.is_finite() || p <= 0.0 {
        return Err(CompileError::InvalidAmount(format!(
            "LP price '{}' must be a positive finite number",
            price_human
        )));
    }

    // If the user is quoting in token0, invert so we're always working in the
    // canonical token1-per-token0 direction.
    if !quote_is_token1 {
        p = 1.0 / p;
    }

    // Adjust for decimals: raw ratio = human ratio * 10^(dec1 - dec0).
    let dec_diff = decimals1 as i32 - decimals0 as i32;
    let price_raw = p * f64_pow10(dec_diff);
    if !price_raw.is_finite() || price_raw <= 0.0 {
        return Err(CompileError::InvalidAmount(format!(
            "LP price '{}' produced a non-finite raw ratio after decimals adjustment",
            price_human
        )));
    }

    // tick = log_{1.0001}(price_raw) = ln(price_raw) / ln(1.0001)
    let tick_f = price_raw.ln() / 1.0001_f64.ln();
    if !tick_f.is_finite() {
        return Err(CompileError::InvalidAmount(format!(
            "LP price '{}' produced a non-finite tick value",
            price_human
        )));
    }
    let tick = tick_f.floor();
    if tick < MIN_TICK as f64 || tick > MAX_TICK as f64 {
        return Err(CompileError::InvalidChain(format!(
            "LP price '{}' maps to tick {} which is outside bounds [{}, {}]",
            price_human, tick, MIN_TICK, MAX_TICK
        )));
    }
    Ok(tick as i32)
}

/// Convert a Uniswap V3 tick back into a human-readable price. Inverse of
/// [`price_to_tick`]. Returned price is in `quote_token` per 1 unit of the
/// other token, honoring `quote_is_token1` in the same sense as the forward
/// direction.
pub fn tick_to_price(tick: i32, quote_is_token1: bool, dec0: u8, dec1: u8) -> f64 {
    // price_raw = 1.0001^tick
    let price_raw = 1.0001_f64.powi(tick);
    // price_human (quote = token1) = price_raw / 10^(decimals1 - decimals0)
    let dec_diff = decimals1_minus_decimals0(dec0, dec1);
    let mut p = price_raw / f64_pow10(dec_diff);
    if !quote_is_token1 {
        p = 1.0 / p;
    }
    p
}

fn decimals1_minus_decimals0(dec0: u8, dec1: u8) -> i32 {
    dec1 as i32 - dec0 as i32
}

/// Snap a tick range to the pool's fee-tier spacing.
///
/// The lower tick is rounded **down** and the upper tick is rounded **up** so
/// that the realized range always contains the requested range. Both ends are
/// then clamped to the largest/smallest multiples of `spacing` that still lie
/// inside `[MIN_TICK, MAX_TICK]` — matching the `±887220` full-range
/// convention for the 0.3% fee tier.
pub fn snap_range(lower: i32, upper: i32, spacing: i32) -> (i32, i32) {
    // Smallest multiple of `spacing` that is ≥ MIN_TICK; largest ≤ MAX_TICK.
    let min_aligned = MIN_TICK + (-MIN_TICK).rem_euclid(spacing);
    let max_aligned = MAX_TICK - MAX_TICK.rem_euclid(spacing);

    let snap_down = |t: i32| -> i32 {
        let rem = t.rem_euclid(spacing);
        (t - rem).max(min_aligned)
    };
    let snap_up = |t: i32| -> i32 {
        let rem = t.rem_euclid(spacing);
        let up = if rem == 0 { t } else { t + (spacing - rem) };
        up.min(max_aligned)
    };
    (snap_down(lower), snap_up(upper))
}

/// Resolve a `price_lower` string into a concrete tick, honoring the `"min"`
/// sentinel.
pub fn resolve_lower_bound(raw: &str, quote_is_token1: bool, dec0: u8, dec1: u8) -> Result<i32> {
    match PriceBound::parse(raw) {
        PriceBound::Min => Ok(MIN_TICK),
        PriceBound::Max => Err(CompileError::InvalidAmount(
            "LP price_lower cannot be 'max' — use 'min' or an explicit value".to_string(),
        )),
        PriceBound::Explicit(s) => price_to_tick(s, quote_is_token1, dec0, dec1),
    }
}

/// Resolve a `price_upper` string into a concrete tick, honoring the `"max"`
/// sentinel.
pub fn resolve_upper_bound(raw: &str, quote_is_token1: bool, dec0: u8, dec1: u8) -> Result<i32> {
    match PriceBound::parse(raw) {
        PriceBound::Max => Ok(MAX_TICK),
        PriceBound::Min => Err(CompileError::InvalidAmount(
            "LP price_upper cannot be 'min' — use 'max' or an explicit value".to_string(),
        )),
        PriceBound::Explicit(s) => price_to_tick(s, quote_is_token1, dec0, dec1),
    }
}

/// When the quote token is token0 (inverted direction), the raw price is
/// `1/price_human`, so larger human prices become *smaller* ticks. In that
/// case callers must swap the resulting `lower`/`upper` pair so the realized
/// tick range is still `lower < upper`.
pub fn maybe_swap_inverted(lower: i32, upper: i32, quote_is_token1: bool) -> (i32, i32) {
    if quote_is_token1 {
        (lower, upper)
    } else {
        (upper, lower)
    }
}

/// `10.0_f64.powi(n)` but safe for no_std builds without relying on the std
/// math intrinsics. For the small range we care about (`-18..=18`) an integer
/// power is both exact for non-negative exponents and fast.
fn f64_pow10(n: i32) -> f64 {
    // f64::powi is available in std and (from core 1.85) core; here we stay on
    // the safe side by looping so we don't depend on it from no_std.
    let mut out = 1.0_f64;
    if n >= 0 {
        for _ in 0..n {
            out *= 10.0;
        }
    } else {
        for _ in 0..(-n) {
            out /= 10.0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usdc_usdt_price_1_is_tick_0() {
        // Both tokens 6-decimal, so decimals adjustment is a no-op. ln(1) = 0.
        let tick = price_to_tick("1.0", true, 6, 6).unwrap();
        assert_eq!(tick, 0);
    }

    #[test]
    fn tight_stables_round_trip() {
        // USDC/USDT 0.999 → tiny negative tick, 1.001 → tiny positive tick.
        let lo = price_to_tick("0.999", true, 6, 6).unwrap();
        let hi = price_to_tick("1.001", true, 6, 6).unwrap();
        assert!(lo < 0 && hi > 0);
        assert!(hi - lo <= 22); // ~1.0001^20 ≈ 1.002
        let (snapped_lo, snapped_hi) = snap_range(lo, hi, 10);
        assert_eq!(snapped_lo % 10, 0);
        assert_eq!(snapped_hi % 10, 0);
        assert!(snapped_lo <= lo);
        assert!(snapped_hi >= hi);
    }

    #[test]
    fn min_max_sentinels() {
        assert!(matches!(PriceBound::parse("min"), PriceBound::Min));
        assert!(matches!(PriceBound::parse("MAX"), PriceBound::Max));
        assert!(matches!(PriceBound::parse(" Min "), PriceBound::Min));
        assert!(matches!(
            PriceBound::parse("1.5"),
            PriceBound::Explicit("1.5")
        ));
    }

    #[test]
    fn snap_rounds_outward() {
        // spacing 60, lower=-5 should snap down to -60, upper=5 should snap up to 60.
        let (lo, hi) = snap_range(-5, 5, 60);
        assert_eq!(lo, -60);
        assert_eq!(hi, 60);
        // Already-aligned ticks stay put.
        let (lo, hi) = snap_range(-120, 120, 60);
        assert_eq!(lo, -120);
        assert_eq!(hi, 120);
    }

    #[test]
    fn snap_clamps_full_range_to_max_multiples() {
        // MIN_TICK=-887272, spacing=60 → aligned floor is -887220.
        // (We snap MIN_TICK down, but we can't go below the largest multiple ≤ MIN_TICK
        // that still fits — i.e. -887220 is the conventional "full range" lower bound.)
        let (_, hi) = snap_range(0, MAX_TICK, 60);
        assert_eq!(hi, 887220); // 887272 snapped *up* would overflow, so clamped down to 887220.
    }

    #[test]
    fn inverted_direction_matches_reciprocal_human_price() {
        // For the same pool (USDC as token0 / WETH as token1), quoting as
        // "1 WETH = 3000 USDC" (quote=USDC, token0-denominated) must produce
        // the same tick as quoting as "1 USDC = 0.000333... WETH" (quote=WETH,
        // token1-denominated). That is: inverting `quote_is_token1` and the
        // human price together is a no-op at the tick level.
        let a = price_to_tick("3000", false, 6, 18).unwrap();
        let b = price_to_tick("0.0003333333333333333", true, 6, 18).unwrap();
        assert!((a - b).abs() <= 1, "a={a}, b={b}");
    }

    #[test]
    fn symmetric_pool_inverted_is_negated() {
        // For a symmetric-decimals pool, quote=token0 and quote=token1 at
        // price=P produce ticks that are ~negatives of each other (off-by-one
        // possible from floor-rounding on both halves).
        let a = price_to_tick("3000", true, 6, 6).unwrap();
        let b = price_to_tick("3000", false, 6, 6).unwrap();
        assert!((a + b).abs() <= 1, "a={a}, b={b}");
    }

    #[test]
    fn rejects_non_positive_or_malformed() {
        assert!(price_to_tick("0", true, 6, 6).is_err());
        assert!(price_to_tick("-1", true, 6, 6).is_err());
        assert!(price_to_tick("abc", true, 6, 6).is_err());
    }
}
