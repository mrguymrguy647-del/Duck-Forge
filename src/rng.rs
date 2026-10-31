//! A tiny random number generator.

use std::time::{SystemTime, UNIX_EPOCH};

/// A fast pseudo-random number generator (the "xorshift32" algorithm).
///
/// Good enough for games (particle directions, enemy spawns...). It is *not*
/// suitable for anything security-related like passwords or keys.
///
/// The same seed always produces the same sequence, which is handy for tests
/// and for replaying a game exactly.
#[derive(Debug, Clone)]
pub struct Rng {
    state: u32,
}

impl Rng {
    /// A generator that starts from `seed`. Any number works, and seeds
    /// that are close together (1, 2, 3...) give unrelated sequences.
    pub fn new(seed: u32) -> Self {
        let state = scramble(seed);
        // xorshift gets stuck forever at 0, so replace a zero state with any other number.
        let state = if state == 0 { 0x9E37_79B9 } else { state };
        Self { state }
    }

    /// A generator seeded from the clock, so every run of the game is different.
    pub fn from_time() -> Self {
        Self::new(seed_from_clock())
    }

    /// The next random 32-bit number. The three shift-and-XOR steps scramble the bits.
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    /// A random float in `0.0..1.0` (1.0 itself is never returned).
    pub fn next_f32(&mut self) -> f32 {
        // Keep the top 24 bits: an f32 can represent every 24-bit integer exactly.
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// A random float in `min..max` (`max` itself is never returned).
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        between(min, max, self.next_f32())
    }

    /// A random integer in `min..max` (`max` excluded). Returns `min` if the range is empty.
    pub fn range_i32(&mut self, min: i32, max: i32) -> i32 {
        if max <= min {
            return min;
        }
        let span = (max as i64 - min as i64) as u64;
        (min as i64 + (self.next_u32() as u64 % span) as i64) as i32
    }

    /// Returns `true` with the given probability (`0.25` = 25% of the time).
    pub fn chance(&mut self, probability: f32) -> bool {
        self.next_f32() < probability
    }
}

/// Mixes up the bits of a seed, so that every bit of the input affects
/// every bit of the output.
///
/// xorshift on its own starts badly from small seeds: from 7 or 8 its first
/// numbers are tiny (about 0.0004 out of 1.0) and almost equal, so seeds 7
/// and 8 would give nearly the same game. After scrambling, 7 and 8 are as
/// different as any two numbers. (These steps are the "lowbias32" hash by
/// Chris Wellons. Each one can be undone, so no two seeds scramble to the
/// same state.)
fn scramble(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    x
}

/// A seed that is different on every run: the nanoseconds part of the clock.
pub(crate) fn seed_from_clock() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(1)
}

/// The number `unit` of the way from `min` to `max`, where `unit` is in
/// `0.0..1.0`, and never `max` itself.
///
/// Rounding can land exactly on `max` even though `unit` is below 1.0:
/// `1.0 + 0.99999994` rounds to `2.0`. That matters, because code like
/// `list[rng.range(0.0, len as f32) as usize]` would then index one past the
/// end of the list. So in that case we step down to the next number below `max`.
fn between(min: f32, max: f32, unit: f32) -> f32 {
    let value = min + (max - min) * unit;
    if value >= max && max > min {
        max.next_down()
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn nearby_seeds_give_unrelated_numbers() {
        // The first number from seeds 1 to 1000 should be spread over the
        // whole range 0..1, not bunched up near 0.
        let firsts: Vec<f32> = (1..=1000).map(|seed| Rng::new(seed).next_f32()).collect();
        let below_tenth = firsts.iter().filter(|&&x| x < 0.1).count();
        let average = firsts.iter().sum::<f32>() / firsts.len() as f32;
        assert!(
            (50..150).contains(&below_tenth),
            "{below_tenth} of 1000 were below 0.1"
        );
        assert!((0.45..0.55).contains(&average), "average {average}");
        assert!((Rng::new(7).next_f32() - Rng::new(8).next_f32()).abs() > 0.01);
    }

    #[test]
    fn zero_seed_still_works() {
        let mut rng = Rng::new(0);
        assert_ne!(rng.next_u32(), 0);
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let f = rng.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&f));
            let i = rng.range_i32(-5, 5);
            assert!((-5..5).contains(&i));
        }
        assert_eq!(rng.range_i32(3, 3), 3);
    }

    #[test]
    fn range_never_returns_max() {
        // The biggest value next_f32 can return.
        let highest = 1.0 - 1.0 / (1u32 << 24) as f32;
        assert_eq!(1.0 + highest, 2.0); // why this matters: rounding reaches `max`
        for (min, max) in [(1.0, 2.0), (0.0, 1.0), (-3.0, 5.0), (0.0, 1e-30)] {
            let value = between(min, max, highest);
            assert!(value >= min && value < max, "{min}..{max} gave {value}");
        }
        assert_eq!(between(4.0, 4.0, highest), 4.0); // an empty range gives `min`
    }
}
