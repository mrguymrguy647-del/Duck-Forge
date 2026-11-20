# Duck Forge's copy of lewton

This is [lewton](https://github.com/RustAudio/lewton) 0.10.2, the pure
Rust OGG Vorbis decoder, by est31 and its contributors (MIT or Apache-2.0,
see `LICENSE`). Duck Forge decodes its OGG music with it
(`src/music.rs` in Duck Forge).

## Why a copy

lewton works out some tables with `f32::sin`, `f32::cos`, `f32::exp`,
`f32::atan` and `f32::exp2`: the window that blends one block of music
into the next, and the factors of its transform. Those call the system's
maths library, whose last digit differs between Windows and Linux, so the
same music decoded to slightly different samples on each, and a headless
run's `audio.wav` wasn't the same file on both. Duck Forge's rule is that
the same inputs give the same game, and the same sound, everywhere.

## What changed (and nothing else)

- `src/fixed_math.rs` (new): `sin`, `cos`, `exp`, `atan` and
  `power_of_two`, done with nothing but adding, multiplying, dividing
  and `sqrt` in `f64` (fdlibm's polynomials for `sin` and `cos`, by Sun
  Microsystems), then rounded to `f32`. As accurate as the system's, and
  the same bits on every computer.
- `src/header_cached.rs`: the window (`win_slope`), the transform's
  factors (`compute_twiddle_factors`) and floor type 0's bark map use it.
- `src/audio.rs`: floor type 0's curve and the slow test transform use it.
- `src/bitpacking.rs`: `float32_unpack` makes its power of two exactly.
- `src/lib.rs`: `pub mod fixed_math` (hidden, so Duck Forge's tests can
  check it against the system's), and its 2016 code's warnings are off.
- `Cargo.toml`: only the `ogg` feature; the async feature, the C API, the
  examples and the development dependencies are left out.

Duck Forge's tests check the fixed math against the system's, and pin the
SHA-256 of a decoded `audio.wav` that GitHub's Windows machine must match.
