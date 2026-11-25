//! Price math for the tide. Pure functions, so they are unit-tested off-chain.

use crate::{RiptideError, PRICE_SCALE};
use anchor_lang::prelude::*;

/// Highest per-sub-step volatility a market may be created with (5%).
pub const MAX_VOLATILITY_BPS: u16 = 500;
/// Each VRF callback applies this many small random steps.
pub const SUB_STEPS: usize = 6;
/// The price never falls below 1.0 chip.
pub const PRICE_FLOOR: u64 = PRICE_SCALE;
/// Out of 256: chance that a tick carries a "riptide" wave (about 2.3%).
pub const WAVE_ODDS: u8 = 6;
/// A wave moves the price between 3% and 8% in one tick.
pub const WAVE_MIN_BPS: u64 = 300;
pub const WAVE_SPREAD_BPS: u64 = 500;

pub struct TideStep {
    pub price: u64,
    /// 1 for an up-wave, -1 for a down-wave, 0 for a calm tick.
    pub wave: i8,
}

/// Move `price` by one tick using 32 bytes of verifiable randomness.
///
/// Bytes 0..24 drive six small steps of up to `volatility_bps` each way.
/// Bytes 24..27 decide whether a wave hits, which way and how hard.
pub fn next_price(price: u64, randomness: &[u8; 32], volatility_bps: u16) -> TideStep {
    let vol = volatility_bps as i128;
    let mut p = price as i128;
    for i in 0..SUB_STEPS {
        let chunk: [u8; 4] = randomness[i * 4..i * 4 + 4].try_into().unwrap();
        let r = u32::from_le_bytes(chunk) as i128;
        let step_bps = r % (2 * vol + 1) - vol;
        p = p * (10_000 + step_bps) / 10_000;
    }

    let mut wave = 0i8;
    if randomness[24] < WAVE_ODDS {
        let magnitude = (WAVE_MIN_BPS + (randomness[26] as u64 % WAVE_SPREAD_BPS)) as i128;
        if randomness[25] & 1 == 0 {
            wave = 1;
            p = p * (10_000 + magnitude) / 10_000;
        } else {
            wave = -1;
            p = p * (10_000 - magnitude) / 10_000;
        }
    }

    let price = p.clamp(PRICE_FLOOR as i128, u64::MAX as i128) as u64;
    TideStep { price, wave }
}

/// Chip PnL of a signed position `size` opened at `entry` and marked at `price`.
pub fn pnl(size: i64, entry: u64, price: u64) -> Result<i64> {
    if size == 0 {
        return Ok(0);
    }
    let diff = price as i128 - entry as i128;
    let value = size as i128 * diff / PRICE_SCALE as i128;
    i64::try_from(value).map_err(|_| error!(RiptideError::MathOverflow))
}

/// Position quantity (6 decimals) worth `notional` chips at `price`.
pub fn qty_for_notional(notional: i64, price: u64) -> Result<i64> {
    let qty = notional as i128 * PRICE_SCALE as i128 / price as i128;
    i64::try_from(qty).map_err(|_| error!(RiptideError::MathOverflow))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CHIP, OPEN_PRICE};

    fn rnd(seed: u8) -> [u8; 32] {
        let mut out = [0u8; 32];
        let mut x: u32 = 0x9E37_79B9 ^ seed as u32;
        for b in out.iter_mut() {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            *b = x as u8;
        }
        out
    }

    #[test]
    fn calm_tick_stays_within_volatility_band() {
        let mut r = rnd(1);
        r[24] = 255; // no wave
        let step = next_price(OPEN_PRICE, &r, 40);
        assert_eq!(step.wave, 0);
        // six steps of at most 0.4% each way
        let lo = OPEN_PRICE as f64 * 0.996f64.powi(6);
        let hi = OPEN_PRICE as f64 * 1.004f64.powi(6);
        assert!((step.price as f64) >= lo - 1.0 && (step.price as f64) <= hi + 1.0);
    }

    #[test]
    fn waves_move_hard_in_their_direction() {
        let mut r = [0u8; 32];
        // zero sub-step bytes give the most negative step; neutralize with vol=0-ish band
        for i in 0..24 {
            r[i] = 0;
        }
        r[24] = 0; // wave hits
        r[25] = 0; // up
        r[26] = 0; // 3%
        let calm = {
            let mut c = r;
            c[24] = 255;
            next_price(OPEN_PRICE, &c, 1).price
        };
        let up = next_price(OPEN_PRICE, &r, 1);
        assert_eq!(up.wave, 1);
        assert_eq!(up.price, calm * 10_300 / 10_000);
        r[25] = 1;
        let down = next_price(OPEN_PRICE, &r, 1);
        assert_eq!(down.wave, -1);
        assert_eq!(down.price, calm * 9_700 / 10_000);
    }

    #[test]
    fn price_never_breaks_the_floor() {
        let mut p = 2 * PRICE_FLOOR;
        let mut r = [0u8; 32];
        r[24] = 0;
        r[25] = 1; // keep hitting down-waves
        for _ in 0..500 {
            p = next_price(p, &r, MAX_VOLATILITY_BPS).price;
        }
        assert_eq!(p, PRICE_FLOOR);
    }

    #[test]
    fn long_and_short_pnl_are_symmetric() {
        let qty = qty_for_notional(1_000 * CHIP, OPEN_PRICE).unwrap();
        assert_eq!(qty, 10 * 1_000_000); // 10 units at 100 chips
        let up = OPEN_PRICE + 5 * PRICE_SCALE; // +5%
        assert_eq!(pnl(qty, OPEN_PRICE, up).unwrap(), 50 * CHIP);
        assert_eq!(pnl(-qty, OPEN_PRICE, up).unwrap(), -50 * CHIP);
    }

    #[test]
    fn random_walk_is_roughly_balanced() {
        let mut ups = 0;
        for s in 0..=255u8 {
            let mut r = rnd(s);
            r[24] = 255;
            if next_price(OPEN_PRICE, &r, 40).price > OPEN_PRICE {
                ups += 1;
            }
        }
        assert!((90..=166).contains(&ups), "ups={ups}");
    }
}
