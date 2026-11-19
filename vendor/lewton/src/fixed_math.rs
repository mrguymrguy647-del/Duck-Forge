// Duck Forge's change to lewton (see DUCKFORGE.md).
//
//! Math that gives the same bits on every computer.
//!
//! lewton worked out its tables (the window, the transform's factors) with
//! `f32::sin`, `f32::cos` and friends. Those call the system's maths
//! library, and the last digit of their answers differs between Windows
//! and Linux, so the same music decoded to slightly different samples.
//! These do the same sums with nothing but adding, multiplying, dividing
//! and `sqrt` (which every computer does exactly the same way, as the
//! IEEE 754 standard says), in `f64`, then round to `f32`. They're as
//! close to the true values as the system's, and identical everywhere.

/// π/2, split into two parts so that `x - n * π/2` loses nothing for the
/// sizes of `x` lewton uses (from fdlibm, by Sun Microsystems).
const PIO2_HI: f64 = 1.570_796_326_734_125_614_17e+00;
const PIO2_LO: f64 = 6.077_100_506_506_192_249_32e-11;
const TWO_OVER_PI: f64 = 6.366_197_723_675_813_824_33e-01;

/// sin(x) for |x| ≤ π/4 (fdlibm's polynomial).
fn sin_small(x: f64) -> f64 {
    const S1: f64 = -1.666_666_666_666_663_243_48e-01;
    const S2: f64 = 8.333_333_333_322_489_461_24e-03;
    const S3: f64 = -1.984_126_982_985_794_931_34e-04;
    const S4: f64 = 2.755_731_370_707_006_767_89e-06;
    const S5: f64 = -2.505_076_025_340_686_341_95e-08;
    const S6: f64 = 1.589_690_995_211_550_102_21e-10;
    let z = x * x;
    let r = S2 + z * (S3 + z * (S4 + z * (S5 + z * S6)));
    x + x * z * (S1 + z * r)
}

/// cos(x) for |x| ≤ π/4 (fdlibm's polynomial).
fn cos_small(x: f64) -> f64 {
    const C1: f64 = 4.166_666_666_666_660_190_37e-02;
    const C2: f64 = -1.388_888_888_887_410_957_49e-03;
    const C3: f64 = 2.480_158_728_947_672_941_78e-05;
    const C4: f64 = -2.755_731_435_139_066_330_35e-07;
    const C5: f64 = 2.087_572_321_298_174_827_90e-09;
    const C6: f64 = -1.135_964_755_778_819_482_65e-11;
    let z = x * x;
    let r = z * (C1 + z * (C2 + z * (C3 + z * (C4 + z * (C5 + z * C6)))));
    1.0 - 0.5 * z + z * r
}

/// `x` as a whole number of quarter turns and what's left, |left| ≤ π/4.
fn quarter_turns(x: f64) -> (i64, f64) {
    let n = (x * TWO_OVER_PI + 0.5).floor();
    let left = (x - n * PIO2_HI) - n * PIO2_LO;
    (n as i64, left)
}

fn sin64(x: f64) -> f64 {
    let (n, r) = quarter_turns(x);
    match n.rem_euclid(4) {
        0 => sin_small(r),
        1 => cos_small(r),
        2 => -sin_small(r),
        _ => -cos_small(r),
    }
}

fn cos64(x: f64) -> f64 {
    let (n, r) = quarter_turns(x);
    match n.rem_euclid(4) {
        0 => cos_small(r),
        1 => -sin_small(r),
        2 => -cos_small(r),
        _ => sin_small(r),
    }
}

/// e^x.
fn exp64(x: f64) -> f64 {
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-01;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const LOG2_E: f64 = 1.442_695_040_888_963_387_00e+00;
    if x.is_nan() {
        return x;
    }
    if x > 709.0 {
        return f64::INFINITY;
    }
    if x < -745.0 {
        return 0.0;
    }
    // x = k ln 2 + r, |r| ≤ ln 2 / 2, then e^r by its series.
    let k = (x * LOG2_E + 0.5).floor();
    let r = (x - k * LN2_HI) - k * LN2_LO;
    let mut term = 1.0;
    let mut sum = 1.0;
    for i in 1..=17 {
        term *= r / i as f64;
        sum += term;
    }
    scale_by_power_of_two(sum, k as i32)
}

/// `value` times 2^k, exactly (in two steps, so very large or very small
/// k still works).
fn scale_by_power_of_two(value: f64, k: i32) -> f64 {
    let half = k / 2;
    value * power_of_two_64(half) * power_of_two_64(k - half)
}

/// 2^k as an `f64`, for -1022 ≤ k ≤ 1023 (0 or infinity beyond).
fn power_of_two_64(k: i32) -> f64 {
    if k > 1023 {
        f64::INFINITY
    } else if k < -1022 {
        0.0
    } else {
        f64::from_bits(((k + 1023) as u64) << 52)
    }
}

/// arctan(x).
fn atan64(x: f64) -> f64 {
    if x.is_nan() {
        return x;
    }
    if x < 0.0 {
        return -atan64(-x);
    }
    if x > 1.0 {
        return (PIO2_HI - atan64(1.0 / x)) + PIO2_LO; // π/2 - atan(1/x)
    }
    // Halve the angle twice (atan x = 2 atan(x / (1 + √(1 + x²)))), down
    // to |x| ≤ 0.2, then the series x - x³/3 + x⁵/5 - ...
    let mut y = x;
    for _ in 0..2 {
        y /= 1.0 + (1.0 + y * y).sqrt();
    }
    let (y2, mut power, mut sum) = (y * y, y, 0.0);
    for i in 0..24 {
        let term = power / (2 * i + 1) as f64;
        sum += if i % 2 == 0 { term } else { -term };
        power *= y2;
    }
    4.0 * sum
}

/// sin(x), the same on every computer.
pub fn sin(x: f32) -> f32 {
    sin64(x as f64) as f32
}

/// cos(x), the same on every computer.
pub fn cos(x: f32) -> f32 {
    cos64(x as f64) as f32
}

/// e^x, the same on every computer.
pub fn exp(x: f32) -> f32 {
    exp64(x as f64) as f32
}

/// arctan(x), the same on every computer.
pub fn atan(x: f32) -> f32 {
    atan64(x as f64) as f32
}

/// 2^k as an `f32`, exactly (0 below the smallest, infinity above the
/// largest).
pub fn power_of_two(k: i32) -> f32 {
    if k > 127 {
        f32::INFINITY
    } else if k >= -126 {
        f32::from_bits(((k + 127) as u32) << 23)
    } else if k >= -149 {
        f32::from_bits(1 << (k + 149)) // too small for the usual form
    } else {
        0.0
    }
}
