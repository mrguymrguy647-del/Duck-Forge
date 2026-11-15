//! Sound: a tiny built-in synthesizer, `.wav` files, and the record of
//! every sound a game played.
//!
//! No sound files are needed: describe a sound with a [`Sound`] (a wave, a
//! pitch, a length) and play it with [`Context::play`], or use a ready-made
//! one like [`Sound::coin`]:
//!
//! ```
//! use duckforge::prelude::*;
//!
//! fn update(ctx: &mut Context) {
//!     if ctx.input.was_pressed(Key::Space) {
//!         ctx.play(Sound::jump());
//!     }
//!     ctx.beep(440.0, 100.0); // 440 Hz for 100 milliseconds
//!     ctx.play(Sound {
//!         name: "laser",
//!         wave: Wave::Square,
//!         freq: 1200.0,
//!         slide_to: 200.0,
//!         ms: 150.0,
//!         ..Default::default()
//!     });
//! }
//! ```
//!
//! In a window, sounds come out of the speakers (with the `audio` feature,
//! on by default). Without a window, every sound is mixed into
//! `audio.wav` at the exact frame it was played, and listed in `log.json`,
//! so a test or an AI can check that the right sound happened at the
//! right time.
//!
//! [`Context::play`]: crate::engine::Context::play

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::Value;

use crate::error::LoadError;
use crate::report::ReportValue;

use crate::sprite::missing_file_error;

/// Samples per second of every sound in the engine (CD quality).
pub const SAMPLE_RATE: u32 = 44_100;

/// Sound samples at [`SAMPLE_RATE`], each from -1.0 to 1.0: one channel
/// (mono), or two side by side (stereo: left, right, left, right...).
/// Cloning is cheap: the samples are shared, not copied.
#[derive(Debug, Clone)]
pub(crate) struct Samples {
    data: Arc<[f32]>,
    stereo: bool,
}

impl Samples {
    pub(crate) fn mono(samples: Vec<f32>) -> Samples {
        Samples {
            data: samples.into(),
            stereo: false,
        }
    }

    /// Left and right samples, side by side.
    pub(crate) fn stereo(interleaved: Vec<f32>) -> Samples {
        debug_assert!(
            interleaved.len().is_multiple_of(2),
            "a left and a right each time"
        );
        Samples {
            data: interleaved.into(),
            stereo: true,
        }
    }

    pub(crate) fn is_stereo(&self) -> bool {
        self.stereo
    }

    /// How many moments it holds (a stereo frame is a left and a right).
    pub(crate) fn frames(&self) -> usize {
        if self.stereo {
            self.data.len() / 2
        } else {
            self.data.len()
        }
    }

    /// The same sound, `volume` times as loud.
    pub(crate) fn scaled(&self, volume: f32) -> Samples {
        Samples {
            data: self.data.iter().map(|s| s * volume).collect(),
            stereo: self.stereo,
        }
    }

    /// Moment `i` as (left, right). A mono sample is the same on both sides.
    #[inline]
    pub(crate) fn frame(&self, i: usize) -> (f32, f32) {
        if self.stereo {
            (self.data[2 * i], self.data[2 * i + 1])
        } else {
            (self.data[i], self.data[i])
        }
    }
}

/// How loud a sound is on the left and on the right for a `pan` from -1
/// (left only) through 0 (both, as loud as without a pan) to 1 (right only).
pub(crate) fn pan_gains(pan: f32) -> (f32, f32) {
    let pan = if pan.is_finite() {
        pan.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

/// The shape of a sound wave, which gives a sound its character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    /// Buzzy and bright: the classic 8-bit game sound.
    #[default]
    Square,
    /// Pure and soft, like a whistle.
    Sine,
    /// Between the two: soft but clear, like a flute.
    Triangle,
    /// Hiss and crackle, for hits, explosions and wind. `freq` sets how
    /// grainy it is: low is rumbly, high is hissy.
    Noise,
}

impl Wave {
    fn name(self) -> &'static str {
        match self {
            Wave::Square => "square",
            Wave::Sine => "sine",
            Wave::Triangle => "triangle",
            Wave::Noise => "noise",
        }
    }
}

/// A sound made by the built-in synthesizer. Set what you need and let
/// `..Default::default()` fill in the rest.
///
/// Every sound starts at `freq` and slides to `slide_to` (if set), lasts
/// `ms` milliseconds, fades in over `attack` milliseconds and fades out
/// over the last `decay` milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sound {
    /// A name for the sound, shown in `log.json` and used by play tests
    /// to find it. The ready-made sounds are called `"coin"`, `"jump"`,
    /// `"hit"` and `"explosion"`; `ctx.beep` is `"beep"`. Empty (the
    /// default) is shown as `"sound"`.
    pub name: &'static str,
    /// The wave shape. `Wave::Square` by default.
    pub wave: Wave,
    /// The pitch at the start, in Hz (vibrations per second): 440 is the
    /// A above middle C, 220 is an octave lower, 880 an octave higher.
    pub freq: f32,
    /// The pitch at the end, in Hz: the sound slides there smoothly. 0.0
    /// (the default) keeps the same pitch.
    pub slide_to: f32,
    /// How long it lasts, in milliseconds (1000 is one second).
    pub ms: f32,
    /// How loud: 0.0 silent, 1.0 as loud as possible. 0.5 by default.
    /// Sounds playing at once add up, so keep each well below 1.0.
    pub volume: f32,
    /// Milliseconds to fade in at the start. A few avoids a click.
    pub attack: f32,
    /// Milliseconds to fade out at the end.
    pub decay: f32,
}

impl Default for Sound {
    /// A short 440 Hz square-wave blip.
    fn default() -> Self {
        Sound {
            name: "",
            wave: Wave::Square,
            freq: 440.0,
            slide_to: 0.0,
            ms: 100.0,
            volume: 0.5,
            attack: 2.0,
            decay: 40.0,
        }
    }
}

impl Sound {
    /// A bright rising "bling", for picking something up.
    pub fn coin() -> Sound {
        Sound {
            name: "coin",
            wave: Wave::Square,
            freq: 1050.0,
            slide_to: 1600.0,
            ms: 120.0,
            volume: 0.3,
            attack: 1.0,
            decay: 80.0,
        }
    }

    /// A springy rising "boing".
    pub fn jump() -> Sound {
        Sound {
            name: "jump",
            wave: Wave::Square,
            freq: 220.0,
            slide_to: 600.0,
            ms: 180.0,
            volume: 0.3,
            attack: 2.0,
            decay: 100.0,
        }
    }

    /// A short falling "thud", for getting hurt or bumping into things.
    pub fn hit() -> Sound {
        Sound {
            name: "hit",
            wave: Wave::Square,
            freq: 330.0,
            slide_to: 70.0,
            ms: 150.0,
            volume: 0.4,
            attack: 1.0,
            decay: 120.0,
        }
    }

    /// A long rumbling "boom".
    pub fn explosion() -> Sound {
        Sound {
            name: "explosion",
            wave: Wave::Noise,
            freq: 1500.0,
            slide_to: 80.0,
            ms: 700.0,
            volume: 0.6,
            attack: 2.0,
            decay: 650.0,
        }
    }

    /// The name shown in logs: `name`, or `"sound"` if it's empty.
    pub fn label(&self) -> &'static str {
        if self.name.is_empty() {
            "sound"
        } else {
            self.name
        }
    }

    /// Makes the sound's samples, at [`SAMPLE_RATE`], each from -1.0 to 1.0.
    ///
    /// Only plain arithmetic is used (no `sin` from the system's math
    /// library, whose last digits can differ between computers), so the
    /// same sound gives the same samples everywhere.
    pub fn render(&self) -> Vec<f32> {
        let finite = |v: f32, fallback: f32| if v.is_finite() { v } else { fallback };
        let ms = finite(self.ms, 0.0).clamp(0.0, 60_000.0); // at most a minute
        let length = (ms * SAMPLE_RATE as f32 / 1000.0).round() as usize;
        let volume = finite(self.volume, 0.0).clamp(0.0, 1.0);
        let start = finite(self.freq, 0.0).clamp(0.0, 20_000.0);
        let end = if self.slide_to > 0.0 && self.slide_to.is_finite() {
            self.slide_to.min(20_000.0)
        } else {
            start
        };
        let to_samples = |ms: f32| finite(ms, 0.0).max(0.0) * SAMPLE_RATE as f32 / 1000.0;
        // If the fades are longer than the sound, shrink them to fit.
        let (mut attack, mut decay) = (to_samples(self.attack), to_samples(self.decay));
        if attack + decay > length as f32 && attack + decay > 0.0 {
            let fit = length as f32 / (attack + decay);
            attack *= fit;
            decay *= fit;
        }

        let mut samples = Vec::with_capacity(length);
        let mut phase = 0.0f32; // how far through one wave, from 0 to 1
        let mut noise = Noise::new(0x9E37_79B9);
        let mut held = noise.next(); // noise holds one value per wave
        for i in 0..length {
            let along = if length > 1 {
                i as f32 / (length - 1) as f32
            } else {
                0.0
            };
            let freq = start + (end - start) * along;
            let value = match self.wave {
                Wave::Square => {
                    if phase < 0.5 {
                        1.0
                    } else {
                        -1.0
                    }
                }
                Wave::Sine => sine(phase),
                Wave::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
                Wave::Noise => held,
            };
            let fade_in = if attack > 0.0 {
                (i as f32 / attack).min(1.0)
            } else {
                1.0
            };
            let left = (length - i) as f32;
            let fade_out = if decay > 0.0 {
                (left / decay).min(1.0)
            } else {
                1.0
            };
            samples.push(value * volume * fade_in * fade_out);

            phase += freq / SAMPLE_RATE as f32;
            if phase >= 1.0 {
                phase -= phase.floor();
                held = noise.next();
            }
        }
        samples
    }
}

/// `sin(phase * 2π)` for a phase from 0 to 1, using Bhaskara's formula
/// from the 7th century: close enough to hear no difference, and made of
/// plain arithmetic, so it gives the same answer on every computer.
fn sine(phase: f32) -> f32 {
    let (half, sign) = if phase < 0.5 {
        (phase, 1.0)
    } else {
        (phase - 0.5, -1.0)
    };
    let x = half * 2.0; // 0 to 1 across the half wave
    let curve = x * (1.0 - x);
    sign * 16.0 * curve / (5.0 - 4.0 * curve)
}

/// A tiny random number generator for noise (xorshift).
struct Noise(u32);

impl Noise {
    fn new(seed: u32) -> Noise {
        Noise(seed.max(1))
    }

    /// A number from -1.0 to 1.0.
    fn next(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x >> 8) as f32 / (1u32 << 23) as f32 - 1.0
    }
}

/// A sound loaded from a `.wav` file, ready to play with
/// [`Context::play_wav`](crate::engine::Context::play_wav).
///
/// ```no_run
/// # use duckforge::prelude::*;
/// # fn example() -> Result<(), duckforge::LoadError> {
/// let quack = Wav::load("assets/quack.wav")?;
/// # Ok(())
/// # }
/// // later, in update: ctx.play_wav(&quack);
/// ```
///
/// Cloning a `Wav` is cheap: the samples are shared, not copied.
#[derive(Clone)]
pub struct Wav {
    name: String,
    path: PathBuf,
    samples: Samples,
}

impl Wav {
    /// Loads a `.wav` file: 8, 16, 24 or 32-bit, or floating point, at any
    /// sample rate (converted to [`SAMPLE_RATE`]), mono or stereo. Stereo
    /// stays stereo, left and right; a mono sound plays the same on both
    /// sides. (Files with more than two channels keep the first two.) The
    /// sound's name in logs is the file name without `.wav`.
    ///
    /// If the file is missing or isn't a WAV duckforge can read, the error
    /// says so in one sentence.
    pub fn load(path: impl AsRef<Path>) -> Result<Wav, LoadError> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|error| missing_file_error(path, "sound", &error))?;
        let samples = decode_wav(&bytes).map_err(|problem| {
            LoadError::new(
                io::ErrorKind::InvalidData,
                format!(
                    "couldn't read {} as a WAV sound ({problem}); save it again as a plain \
                     16-bit PCM .wav file (most sound editors call this \"WAV, 16-bit PCM\")",
                    path.display()
                ),
            )
        })?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("wav"));
        Ok(Wav {
            name,
            path: path.to_path_buf(),
            samples,
        })
    }

    /// The name shown in logs and used by play tests: the file name
    /// without `.wav`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How long the sound lasts, in seconds.
    pub fn seconds(&self) -> f32 {
        self.samples.frames() as f32 / SAMPLE_RATE as f32
    }

    /// Whether the file is stereo (a left and a right channel).
    pub fn is_stereo(&self) -> bool {
        self.samples.is_stereo()
    }
}

impl std::fmt::Debug for Wav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wav({:?}, {:.2} s)", self.path, self.seconds())
    }
}

/// Reads the samples out of a WAV file, converted to [`SAMPLE_RATE`]:
/// mono stays mono, stereo stays stereo (more channels keep the first
/// two). The error says what's wrong with the file.
fn decode_wav(bytes: &[u8]) -> Result<Samples, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(String::from("it doesn't start like a WAV file"));
    }
    let u16_at = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let u32_at =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);

    // A WAV file is a list of "chunks": "fmt " describes the sound, "data"
    // holds it. Other chunks (like comments) are skipped.
    let mut format = None;
    let mut data = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32_at(at + 4) as usize;
        let body = at + 8;
        let end = body.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end - body >= 16 {
            let mut kind = u16_at(body);
            if kind == 0xFFFE && end - body >= 26 {
                kind = u16_at(body + 24); // "extensible": the real format is further in
            }
            format = Some((kind, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            data = Some(&bytes[body..end]);
        }
        at = body.saturating_add(size).saturating_add(size % 2); // chunks are padded to even sizes
    }
    let Some((kind, channels, rate, bits)) = format else {
        return Err(String::from(
            "it has no \"fmt \" section describing the sound",
        ));
    };
    let Some(data) = data else {
        return Err(String::from(
            "it has no \"data\" section with the sound in it",
        ));
    };
    if channels == 0 || rate == 0 {
        return Err(String::from(
            "it says it has 0 channels or 0 samples per second",
        ));
    }
    let read: fn(&[u8]) -> f32 = match (kind, bits) {
        (1, 8) => |b| (b[0] as f32 - 128.0) / 128.0,
        (1, 16) => |b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
        (1, 24) => |b| i32::from_le_bytes([0, b[0], b[1], b[2]]) as f32 / 2_147_483_648.0,
        (1, 32) => |b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2_147_483_648.0,
        (3, 32) => |b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]).clamp(-1.0, 1.0),
        (1 | 3, _) => return Err(format!("{bits}-bit samples aren't supported")),
        _ => return Err(format!("it's compressed (format {kind}), not plain PCM")),
    };
    let width = bits as usize / 8;
    let frame = width * channels as usize;
    let kept = (channels as usize).min(2);
    let samples: Vec<f32> = data
        .chunks_exact(frame)
        .flat_map(|f| f.chunks_exact(width).take(kept).map(read))
        .collect();
    let samples = resample(&samples, kept, rate);
    Ok(if kept == 2 {
        Samples::stereo(samples)
    } else {
        Samples::mono(samples)
    })
}

/// How many original samples the resampling filter reaches on each side
/// (when making a sound longer; further when making it shorter).
const SINC_REACH: usize = 12;
/// How finely the filter is worked out between two original samples.
const SINC_STEPS: usize = 1024;

/// Converts samples recorded at `rate` per second to [`SAMPLE_RATE`]
/// (`channels` of them side by side, 1 or 2), or copies them if they're
/// already at that rate.
///
/// Each new sample is a weighted sum of the original samples round it:
/// the weights are a *windowed sinc*, the curve that rebuilds a sound
/// exactly from its samples (sinc), faded to zero at the edges (a Blackman
/// window) so it only reaches 12 samples each way. Joining samples with
/// straight lines instead adds a hiss and a metallic ring. When the sound
/// is made shorter (from 48000 or 96000 per second), the filter is widened
/// so it also removes the notes too high for 44100 per second, which would
/// otherwise fold back down as false ones (aliasing). The weights are
/// worked out once into a table, so a minute of music takes well under a
/// second.
fn resample(samples: &[f32], channels: usize, rate: u32) -> Vec<f32> {
    let frames = samples.len() / channels;
    if rate == SAMPLE_RATE || frames == 0 {
        return samples.to_vec();
    }
    let length = (frames as u64 * SAMPLE_RATE as u64 / rate as u64) as usize;
    let step = rate as f64 / SAMPLE_RATE as f64; // original samples per new one
    // The highest note kept, as a share of the original's highest: all of
    // it when making the sound longer, less when making it shorter.
    let cutoff = (1.0 / step).min(1.0);
    let reach = (SINC_REACH as f64 / cutoff).ceil() as usize;
    let taps = 2 * reach;
    let table = sinc_table(cutoff, reach);

    let mut out = vec![0.0f32; length * channels];
    for i in 0..length {
        let at = i as f64 * step;
        let whole = at.floor();
        let row = ((at - whole) * SINC_STEPS as f64).round() as usize;
        let weights = &table[row * taps..(row + 1) * taps];
        // The first original sample in reach, and the part of the filter
        // that lands inside the sound (outside it is silence).
        let first = whole as i64 - (reach as i64 - 1);
        let from = first.max(0);
        let to = (first + taps as i64).min(frames as i64);
        if from >= to {
            continue;
        }
        let weights = &weights[(from - first) as usize..(to - first) as usize];
        for c in 0..channels {
            let mut sum = 0.0f32;
            for (k, w) in (from as usize..to as usize).zip(weights) {
                sum += samples[k * channels + c] * w;
            }
            out[i * channels + c] = sum;
        }
    }
    out
}

/// The resampling filter's weights: `SINC_STEPS + 1` rows (how far the new
/// sample is past an original one, from 0 to 1), each `2 * reach` weights
/// for the original samples from `reach - 1` before to `reach` after.
/// Each row adds up to 1, so a steady level stays exactly the same.
fn sinc_table(cutoff: f64, reach: usize) -> Vec<f32> {
    use std::f64::consts::PI;
    let taps = 2 * reach;
    let mut table = Vec::with_capacity((SINC_STEPS + 1) * taps);
    let mut row = vec![0.0f64; taps];
    for step in 0..=SINC_STEPS {
        let past = step as f64 / SINC_STEPS as f64;
        for (t, weight) in row.iter_mut().enumerate() {
            // How far this original sample is from the new one.
            let d = t as f64 - (reach as f64 - 1.0) - past;
            let x = cutoff * d;
            let sinc = if x == 0.0 {
                1.0
            } else {
                (PI * x).sin() / (PI * x)
            };
            let u = d / reach as f64; // -1 to 1 across the filter
            let window = if u.abs() >= 1.0 {
                0.0
            } else {
                0.42 + 0.5 * (PI * u).cos() + 0.08 * (2.0 * PI * u).cos()
            };
            *weight = cutoff * sinc * window;
        }
        let total: f64 = row.iter().sum();
        table.extend(row.iter().map(|w| (w / total) as f32));
    }
    table
}

/// One sound the game played: what it was, and on which frame.
#[derive(Debug, Clone)]
pub struct PlayedSound {
    /// The update it was played in (the first update is frame 1).
    pub frame: u64,
    /// Its name: [`Sound::name`] (or `"sound"`), `"beep"`, or a `.wav`
    /// file's name without `.wav`.
    pub name: String,
    pub(crate) source: SoundSource,
    pub(crate) samples: Samples,
    /// Where it was played, from -1 (left) to 1 (right); 0 is both sides.
    pub(crate) pan: f32,
    /// How loud it was played: 1.0 as it is (its samples are already
    /// this loud).
    pub(crate) volume: f32,
    /// The sample it starts at in `audio.wav`.
    pub(crate) start: u64,
    /// The part of the mix it's in (effects, unless the game said).
    pub group: SoundGroup,
    /// The event it was played for (`ctx.play_for`), if any.
    pub event: Option<String>,
}

/// Where a sound came from.
#[derive(Debug, Clone)]
pub(crate) enum SoundSource {
    Synth(Sound),
    File(PathBuf),
}

/// A sound asked for during an update, before the engine knows its frame.
#[derive(Debug, Clone)]
pub(crate) struct SoundRequest {
    pub(crate) name: String,
    pub(crate) source: SoundSource,
    pub(crate) samples: Samples,
    /// -1 (left) to 1 (right); 0 is both sides, as without a pan.
    pub(crate) pan: f32,
    /// 1.0 as it is (the samples are already this loud).
    pub(crate) volume: f32,
    /// The part of the mix it's in.
    pub(crate) group: SoundGroup,
    /// The event it was played for (`ctx.play_for`), if any.
    pub(crate) event: Option<String>,
}

impl SoundRequest {
    pub(crate) fn synth(sound: Sound) -> SoundRequest {
        SoundRequest {
            name: sound.label().to_string(),
            samples: Samples::mono(sound.render()),
            source: SoundSource::Synth(sound),
            pan: 0.0,
            volume: 1.0,
            group: SoundGroup::Effects,
            event: None,
        }
    }

    pub(crate) fn wav(wav: &Wav) -> SoundRequest {
        SoundRequest {
            name: wav.name.clone(),
            source: SoundSource::File(wav.path.clone()),
            samples: wav.samples.clone(),
            pan: 0.0,
            volume: 1.0,
            group: SoundGroup::Effects,
            event: None,
        }
    }

    /// The same sound, in `group` of the mix.
    pub(crate) fn in_group(self, group: SoundGroup) -> SoundRequest {
        SoundRequest { group, ..self }
    }

    /// The same sound, played at `pan`.
    pub(crate) fn panned(self, pan: f32) -> SoundRequest {
        SoundRequest { pan, ..self }
    }

    /// The same sound, `volume` times as loud (1.0 changes nothing).
    pub(crate) fn louder(self, volume: f32) -> SoundRequest {
        if volume == 1.0 {
            return self;
        }
        SoundRequest {
            samples: self.samples.scaled(volume),
            volume,
            ..self
        }
    }
}

impl PlayedSound {
    /// The sound's line in `log.json`: frame and name first, then its
    /// settings or its file, like
    /// `{"frame": 10, "sound": "coin", "wave": "square", "freq": 1050, ...}`.
    pub(crate) fn to_json(&self) -> String {
        let mut pan = if self.pan == 0.0 {
            String::new() // not panned: the line stays as it always was
        } else {
            format!(", \"pan\": {}", number_json(self.pan))
        };
        if self.volume != 1.0 {
            pan += &format!(", \"volume\": {}", number_json(self.volume));
        }
        if self.group != SoundGroup::Effects {
            pan += &format!(", \"group\": \"{}\"", self.group.name());
        }
        format!(
            "{{\"frame\": {}, \"sound\": {}{}{pan}}}",
            self.frame,
            Value::from(self.name.as_str()),
            source_json(&self.source)
        )
    }
}

/// A sound's settings (`, "wave": "square", "freq": 1050, ...`) or its
/// file (`, "file": "assets/quack.wav"`), for `log.json`.
fn source_json(source: &SoundSource) -> String {
    let text = |t: &str| Value::from(t).to_string();
    match source {
        SoundSource::Synth(s) => {
            let mut line = format!(", \"wave\": {}", text(s.wave.name()));
            for (key, value) in [
                ("freq", s.freq),
                ("slide_to", s.slide_to),
                ("ms", s.ms),
                ("volume", s.volume),
                ("attack", s.attack),
                ("decay", s.decay),
            ] {
                line += &format!(", \"{key}\": {}", number_json(value));
            }
            line
        }
        SoundSource::File(path) => format!(", \"file\": {}", text(&path.to_string_lossy())),
    }
}

/// A number for `log.json`: to 2 decimal places, whole numbers without ".0".
fn number_json(v: f32) -> String {
    ReportValue::from((v as f64 * 100.0).round() / 100.0)
        .to_json()
        .to_string()
}

/// A looping sound that's playing, from
/// [`Context::play_loop`](crate::engine::Context::play_loop). Keep it to
/// change the loop's volume or stop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LoopId(pub(crate) u32);

/// What [`Context::play_loop`](crate::engine::Context::play_loop) can
/// repeat: a `.wav` file (`&music`) or a synthesizer [`Sound`]. You don't
/// make one yourself; pass either straight to `play_loop`.
pub struct LoopSound(pub(crate) SoundRequest);

impl From<&Wav> for LoopSound {
    fn from(wav: &Wav) -> LoopSound {
        LoopSound(SoundRequest::wav(wav))
    }
}

impl From<&Sound> for LoopSound {
    fn from(sound: &Sound) -> LoopSound {
        LoopSound(SoundRequest::synth(*sound))
    }
}

impl From<Sound> for LoopSound {
    fn from(sound: Sound) -> LoopSound {
        LoopSound(SoundRequest::synth(sound))
    }
}

/// What the game asked a loop to do during an update.
#[derive(Debug, Clone)]
pub(crate) enum LoopCommand {
    Start(LoopId, SoundRequest, f32),
    Volume(LoopId, f32),
    Stop(LoopId),
    /// A change to the mix: a group's volume, the master volume, ducking.
    Mix(MixCommand),
}

/// One looping sound the game played: when it started and stopped, and
/// its volume over time. Play tests read them with
/// [`Simulation::loops`](crate::Simulation::loops).
#[derive(Debug, Clone)]
pub struct PlayedLoop {
    pub id: LoopId,
    /// Its name: the `.wav` file's name without `.wav`, or the
    /// [`Sound::name`].
    pub name: String,
    /// The update it started in.
    pub start: u64,
    /// The update it was stopped in, or `None` if it was still playing at
    /// the end.
    pub stop: Option<u64>,
    /// Its volume from each frame on, the first being the starting volume:
    /// `[(1, 0.6), (400, 0.3)]`.
    pub volumes: Vec<(u64, f32)>,
    pub(crate) source: SoundSource,
    pub(crate) samples: Samples,
    /// The samples in `audio.wav` where it starts and stops, and where
    /// each volume begins (the same order as `volumes`).
    pub(crate) start_sample: u64,
    pub(crate) stop_sample: Option<u64>,
    pub(crate) volume_samples: Vec<u64>,
    /// The part of the mix it's in (music, unless the game said).
    pub group: SoundGroup,
}

impl PlayedLoop {
    /// Was it still playing at the end of the run?
    pub fn is_playing(&self) -> bool {
        self.stop.is_none()
    }

    /// Its volume during update `frame` (0 before it started or after it
    /// stopped).
    pub fn volume_at(&self, frame: u64) -> f32 {
        if frame < self.start || self.stop.is_some_and(|stop| frame >= stop) {
            return 0.0;
        }
        self.volumes
            .iter()
            .take_while(|(from, _)| *from <= frame)
            .last()
            .map_or(0.0, |&(_, volume)| volume)
    }

    /// The loop's line in `log.json`, like
    /// `{"loop": "theme", "file": "...", "start": 1, "stop": 900, "volume": [[1, 0.6], [400, 0.3]]}`.
    pub(crate) fn to_json(&self) -> String {
        let stop = self.stop.map_or(String::from("null"), |f| f.to_string());
        let volumes: Vec<String> = self
            .volumes
            .iter()
            .map(|&(frame, volume)| format!("[{frame}, {}]", number_json(volume)))
            .collect();
        let group = if self.group == SoundGroup::Music {
            String::new()
        } else {
            format!(", \"group\": \"{}\"", self.group.name())
        };
        format!(
            "{{\"loop\": {}{}, \"start\": {}, \"stop\": {stop}, \"volume\": [{}]{group}}}",
            Value::from(self.name.as_str()),
            source_json(&self.source),
            self.start,
            volumes.join(", ")
        )
    }

    /// Adds the loop into the `left` and `right` tracks, repeating it with
    /// no gap, from its start to its stop (or the end of the tracks), at
    /// each volume in turn.
    fn mix_into(&self, left: &mut [f32], right: &mut [f32], gain: Option<&[f32]>) {
        let length = self.samples.frames() as u64;
        if length == 0 {
            return;
        }
        let end = self
            .stop_sample
            .unwrap_or(left.len() as u64)
            .min(left.len() as u64);
        for (index, &volume) in self.volumes.iter().map(|(_, v)| v).enumerate() {
            let from = self.volume_samples[index].max(self.start_sample);
            let to = self
                .volume_samples
                .get(index + 1)
                .copied()
                .unwrap_or(end)
                .min(end);
            for at in from..to {
                let i = ((at - self.start_sample) % length) as usize;
                let (l, r) = self.samples.frame(i);
                let v = match gain {
                    Some(gain) => volume * gain[at as usize],
                    None => volume,
                };
                left[at as usize] += l * v;
                right[at as usize] += r * v;
            }
        }
    }
}

/// The parts of the mix, each with its own volume: music, ambience (rain,
/// wind, a hum) and effects (footsteps, doors, screams). A sound played
/// once is an effect and a loop is music, unless the game says
/// otherwise ([`Context::play_in`](crate::engine::Context::play_in),
/// [`Context::play_loop_in`](crate::engine::Context::play_loop_in)).
///
/// ```
/// # use duckforge::prelude::*;
/// # fn example(ctx: &mut Context, music: &Wav, rain: &Wav, scream: &Wav) {
/// ctx.play_loop(music, 0.8);                           // music
/// ctx.play_loop_in(SoundGroup::Ambience, rain, 0.5);   // ambience
/// ctx.set_group_volume(SoundGroup::Ambience, 0.6);     // all of it quieter
/// ctx.set_master_volume(0.9);                          // everything
/// ctx.duck(SoundGroup::Music, 0.25, "scream");         // music dips while a scream plays
/// ctx.play_with(scream, 1.0, 0.0);                     // the scream
/// # }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SoundGroup {
    Music,
    Ambience,
    #[default]
    Effects,
}

impl SoundGroup {
    /// Every group, in order.
    pub const ALL: [SoundGroup; 3] = [SoundGroup::Music, SoundGroup::Ambience, SoundGroup::Effects];

    /// Its name in `log.json`: `"music"`, `"ambience"`, `"effects"`.
    pub fn name(self) -> &'static str {
        match self {
            SoundGroup::Music => "music",
            SoundGroup::Ambience => "ambience",
            SoundGroup::Effects => "effects",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

/// What makes a group duck ([`Context::duck`](crate::engine::Context::duck)):
/// sounds with this name (`"scream"`, a `.wav` file's name without `.wav`)
/// or any sound in this group.
#[derive(Debug, Clone, PartialEq)]
pub enum DuckTrigger {
    Named(String),
    Group(SoundGroup),
}

impl From<&str> for DuckTrigger {
    fn from(name: &str) -> DuckTrigger {
        DuckTrigger::Named(name.to_string())
    }
}

impl From<SoundGroup> for DuckTrigger {
    fn from(group: SoundGroup) -> DuckTrigger {
        DuckTrigger::Group(group)
    }
}

impl DuckTrigger {
    /// Is a sound called `name` in `group` what this ducks for?
    pub(crate) fn matches(&self, name: &str, group: SoundGroup) -> bool {
        match self {
            DuckTrigger::Named(n) => n == name,
            DuckTrigger::Group(g) => *g == group,
        }
    }
}

/// A change to the mix, asked for during an update.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MixCommand {
    GroupVolume(SoundGroup, f32),
    Master(f32),
    /// While `trigger` plays, `group` goes down to `level` of its volume.
    Duck(SoundGroup, f32, DuckTrigger),
    StopDucking(SoundGroup),
}

/// A volume change glides over this many seconds (a jump would click).
pub(crate) const VOLUME_GLIDE: f32 = 0.01;
/// A ducked group goes down over this long when its trigger starts...
pub(crate) const DUCK_DOWN: f32 = 0.1;
/// ...and comes back up over this long after it ends.
pub(crate) const DUCK_UP: f32 = 0.5;

/// Every change to the mix in a run, each at the sample it starts at.
#[derive(Debug, Clone, Default)]
pub struct MixLog {
    pub(crate) changes: Vec<(u64, MixCommand)>,
}

impl MixLog {
    /// Nothing changed: every group and the master at 1, no ducking.
    pub(crate) fn is_plain(&self) -> bool {
        self.changes.is_empty()
    }

    /// Each group's volume (times the master and its ducking) at every
    /// sample up to `length`, or `None` for a run that never changed the
    /// mix (then nothing is multiplied, and the mix is what it always was).
    pub(crate) fn gains(
        &self,
        sounds: &[PlayedSound],
        loops: &[PlayedLoop],
        length: usize,
    ) -> Option<[Vec<f32>; 3]> {
        if self.is_plain() {
            return None;
        }
        let glide = (VOLUME_GLIDE * SAMPLE_RATE as f32).max(1.0);
        let (down, up) = (DUCK_DOWN * SAMPLE_RATE as f32, DUCK_UP * SAMPLE_RATE as f32);
        // Where each sound or loop plays, with its name and group.
        let mut spans: Vec<(usize, usize, &str, SoundGroup)> = sounds
            .iter()
            .map(|s| {
                (
                    s.start as usize,
                    s.start as usize + s.samples.frames(),
                    s.name.as_str(),
                    s.group,
                )
            })
            .collect();
        spans.extend(loops.iter().map(|l| {
            let stop = l.stop_sample.map_or(length, |s| s as usize);
            (l.start_sample as usize, stop, l.name.as_str(), l.group)
        }));
        Some(SoundGroup::ALL.map(|group| {
            let mut gain = vec![1.0f32; length];
            let (mut volume, mut master) = (1.0f32, 1.0f32);
            // The volume as set, gliding to each new setting.
            let (mut base, mut step) = (1.0f32, 0.0f32);
            let mut rules: Vec<(f32, &DuckTrigger)> = Vec::new();
            // The level the rules duck each sample to (1: not ducked),
            // worked out again whenever the rules change...
            let mut ducked_to = vec![1.0f32; length];
            // ...and the ducking now: down quickly, back up slowly.
            let mut duck = 1.0f32;
            let mut changes = self.changes.iter().peekable();
            for i in 0..length {
                let (mut volume_changed, mut rules_changed) = (false, false);
                while let Some((at, command)) = changes.peek() {
                    if *at as usize > i {
                        break;
                    }
                    match command {
                        MixCommand::GroupVolume(g, v) if *g == group => {
                            volume = *v;
                            volume_changed = true;
                        }
                        MixCommand::Master(v) => {
                            master = *v;
                            volume_changed = true;
                        }
                        MixCommand::Duck(g, level, trigger) if *g == group => {
                            rules.push((level.clamp(0.0, 1.0), trigger));
                            rules_changed = true;
                        }
                        MixCommand::StopDucking(g) if *g == group => {
                            rules.clear();
                            rules_changed = true;
                        }
                        _ => {}
                    }
                    changes.next();
                }
                if volume_changed {
                    step = (volume * master - base) / glide;
                }
                if rules_changed {
                    ducked_to[i..].fill(1.0);
                    for &(level, trigger) in &rules {
                        for &(from, to, name, g) in &spans {
                            if trigger.matches(name, g) {
                                for d in &mut ducked_to[from.clamp(i, length)..to.clamp(i, length)]
                                {
                                    *d = d.min(level);
                                }
                            }
                        }
                    }
                }
                if step != 0.0 {
                    base += step;
                    let target = volume * master;
                    if (step > 0.0 && base >= target) || (step < 0.0 && base <= target) {
                        base = target;
                        step = 0.0;
                    }
                }
                let want = ducked_to[i];
                duck = if want < duck {
                    (duck - 1.0 / down).max(want)
                } else {
                    (duck + 1.0 / up).min(want)
                };
                gain[i] = base * duck;
            }
            gain
        }))
    }
}

/// A moment the mix was loudest before the limiter, and what was playing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoudMoment {
    pub(crate) sample: u64,
    /// How loud the mix was there (1.0 is full volume).
    pub(crate) level: f32,
    /// What was sounding there, loudest first: its name, whether it's a
    /// loop, the volume it was played at, and how loud it was there.
    pub(crate) sounds: Vec<(String, bool, f32, f32)>,
}

/// The loudest moments over full volume (at most `count`, at least a
/// second apart), and what was sounding at each.
fn loud_moments(
    left: &[f32],
    right: &[f32],
    sounds: &[PlayedSound],
    loops: &[PlayedLoop],
    gains: Option<&[Vec<f32>; 3]>,
    count: usize,
) -> Vec<LoudMoment> {
    let level = |i: usize| left[i].abs().max(right[i].abs());
    // The loudest sample of each second that went over.
    let second = SAMPLE_RATE as usize;
    let mut peaks: Vec<(usize, f32)> = (0..left.len().div_ceil(second))
        .filter_map(|s| {
            let range = s * second..((s + 1) * second).min(left.len());
            range
                .map(|i| (i, level(i)))
                .filter(|&(_, l)| l > 1.0 && l.is_finite())
                .max_by(|a, b| a.1.total_cmp(&b.1))
        })
        .collect();
    peaks.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut chosen: Vec<(usize, f32)> = Vec::new();
    for peak in peaks {
        if chosen.iter().all(|c| c.0.abs_diff(peak.0) >= second) {
            chosen.push(peak);
        }
        if chosen.len() == count {
            break;
        }
    }
    let gain = |group: SoundGroup, i: usize| {
        gains.map_or(1.0, |g| g[group.index()].get(i).copied().unwrap_or(1.0))
    };
    chosen
        .into_iter()
        .map(|(i, level)| {
            let mut playing: Vec<(String, bool, f32, f32)> = Vec::new();
            for s in sounds {
                let at = i as u64;
                if at >= s.start && ((at - s.start) as usize) < s.samples.frames() {
                    let (l, r) = s.samples.frame((at - s.start) as usize);
                    let (gl, gr) = pan_gains(s.pan);
                    let loud = (l * gl).abs().max((r * gr).abs()) * gain(s.group, i);
                    playing.push((s.name.clone(), false, s.volume, loud));
                }
            }
            for l in loops {
                let at = i as u64;
                let stopped = l.stop_sample.is_some_and(|stop| at >= stop);
                if at < l.start_sample || stopped || l.samples.frames() == 0 {
                    continue;
                }
                let k = l
                    .volume_samples
                    .iter()
                    .rposition(|&from| from <= at)
                    .unwrap_or(0);
                let volume = l.volumes[k].1;
                let (a, b) = l
                    .samples
                    .frame(((at - l.start_sample) % l.samples.frames() as u64) as usize);
                let loud = a.abs().max(b.abs()) * volume * gain(l.group, i);
                playing.push((l.name.clone(), true, volume, loud));
            }
            playing.sort_by(|a, b| b.3.total_cmp(&a.3));
            LoudMoment {
                sample: i as u64,
                level,
                sounds: playing,
            }
        })
        .collect()
}

impl LoudMoment {
    /// What to play quieter so this moment fits under full volume: the
    /// loudest sound alone if turning it down is enough, or else
    /// everything playing then, by the same share. Each with the volume to
    /// use instead.
    /// Each: its name, whether it's a loop, its volume now and the
    /// volume to play it at.
    pub(crate) fn suggestion(&self) -> Vec<(String, bool, f32, f32)> {
        let excess = self.level - CEILING;
        let round_down = |v: f32| (v * 100.0).floor() / 100.0;
        match self.sounds.first() {
            Some((name, looped, volume, loud)) if *loud > excess => {
                vec![(
                    name.clone(),
                    *looped,
                    *volume,
                    round_down(volume * (loud - excess) / loud),
                )]
            }
            _ => {
                let share = CEILING / self.level;
                self.sounds
                    .iter()
                    .map(|(name, looped, volume, _)| {
                        (name.clone(), *looped, *volume, round_down(volume * share))
                    })
                    .collect()
            }
        }
    }
}

/// A track mixed by [`mix_to_wav`], and whether it was too loud.
pub(crate) struct Mixed {
    /// The `.wav` file's bytes.
    pub(crate) bytes: Vec<u8>,
    /// How loud the mix was at its loudest, before the limiter (1.0 is full
    /// volume; more would have clipped).
    pub(crate) peak: f32,
    /// Samples that were over full volume, and the first and last of them.
    pub(crate) over: usize,
    pub(crate) first_over: u64,
    pub(crate) last_over: u64,
    /// The loudest moments over full volume, and what was sounding.
    pub(crate) moments: Vec<LoudMoment>,
}

/// The loudest the limiter lets a mix be: a little under full volume.
pub(crate) const CEILING: f32 = 0.98;

/// Mixes every sound and loop into a track, each starting at its own
/// sample, and turns it into the bytes of a 16-bit `.wav` file: mono, or
/// stereo if any sound is stereo or panned. The track is at least `length`
/// samples long, and longer if a sound rings on past it.
///
/// A mix that would go over full volume is turned down where it's too
/// loud by a *limiter* (see [`limit`]), instead of being cut off flat
/// (clipping, which crackles); [`Mixed`] says by how much.
pub(crate) fn mix_to_wav(
    sounds: &[PlayedSound],
    loops: &[PlayedLoop],
    length: u64,
    mix: &MixLog,
) -> Mixed {
    let (mut left, mut right, moments) = mix_floats(sounds, loops, length, mix, 2);
    let (peak, over, first_over, last_over) = limit(&mut left, &mut right);
    // Anything still over (never, after the limiter) is cut off at the top,
    // rather than wrapping round.
    let pcm = |s: f32| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let stereo = sounds.iter().any(|s| s.samples.is_stereo() || s.pan != 0.0)
        || loops.iter().any(|l| l.samples.is_stereo());
    let bytes = if stereo {
        let both = left
            .iter()
            .zip(&right)
            .flat_map(|(&l, &r)| [pcm(l), pcm(r)]);
        stereo_wav_bytes(both, left.len())
    } else {
        // Every sound is the same on both sides: a mono file, as always.
        wav_bytes(left.iter().map(|&s| pcm(s)), left.len())
    };
    Mixed {
        bytes,
        peak,
        over,
        first_over,
        last_over,
        moments,
    }
}

/// The mix as `--review` hears it: mono, after the limiter, with up to
/// `moments` over-full moments (a second apart) named, and the peak
/// before the limiter.
pub(crate) fn mix_for_review(
    sounds: &[PlayedSound],
    loops: &[PlayedLoop],
    length: u64,
    mix: &MixLog,
    moments: usize,
) -> (Vec<f32>, f32, Vec<LoudMoment>) {
    let (mut left, mut right, loud) = mix_floats(sounds, loops, length, mix, moments);
    let (peak, ..) = limit(&mut left, &mut right);
    let mono = left
        .iter()
        .zip(&right)
        .map(|(l, r)| (l + r) * 0.5)
        .collect();
    (mono, peak, loud)
}

/// Every sound and loop mixed, before the limiter, and the loudest
/// `moments` moments over full volume.
fn mix_floats(
    sounds: &[PlayedSound],
    loops: &[PlayedLoop],
    length: u64,
    mix: &MixLog,
    moments: usize,
) -> (Vec<f32>, Vec<f32>, Vec<LoudMoment>) {
    let end = sounds
        .iter()
        .map(|s| s.start + s.samples.frames() as u64)
        .fold(length, u64::max);
    let mut left = vec![0.0f32; end as usize];
    let mut right = vec![0.0f32; end as usize];
    // Each group's volume over time, if the game ever changed the mix.
    let gains = mix.gains(sounds, loops, end as usize);
    for sound in sounds {
        let start = sound.start as usize;
        let (to_left, to_right) = pan_gains(sound.pan);
        match &gains {
            None => {
                for i in 0..sound.samples.frames() {
                    let (l, r) = sound.samples.frame(i);
                    left[start + i] += l * to_left;
                    right[start + i] += r * to_right;
                }
            }
            Some(gains) => {
                let gain = &gains[sound.group.index()];
                for i in 0..sound.samples.frames() {
                    let (l, r) = sound.samples.frame(i);
                    left[start + i] += l * to_left * gain[start + i];
                    right[start + i] += r * to_right * gain[start + i];
                }
            }
        }
    }
    // Loops still playing at the end stop with the run, not with the last
    // one-off sound ringing on.
    let run = (length as usize).min(left.len());
    for looped in loops {
        let gain = gains.as_ref().map(|g| &g[looped.group.index()][..run]);
        looped.mix_into(&mut left[..run], &mut right[..run], gain);
    }
    let moments = loud_moments(&left, &right, sounds, loops, gains.as_ref(), moments);
    (left, right, moments)
}

/// The master limiter: if the mix goes over full volume anywhere, it's
/// turned down just there, smoothly, to [`CEILING`]. It looks 5 ms ahead,
/// so the volume is already down when a loud moment arrives (nothing gets
/// through), and comes back up over about a tenth of a second after (no
/// pumping). A mix that's never too loud isn't touched at all.
///
/// Returns the mix's peak before limiting, and how many samples were over
/// full volume, the first and the last.
/// The loudest sample, and whether any wasn't a number (or was infinite).
#[derive(Clone, Copy)]
struct Loudest(f32, bool);

impl Loudest {
    fn max_and(self, other: Loudest) -> (f32, bool) {
        (self.0.max(other.0), self.1 || other.1)
    }
}

/// [`Loudest`] of `samples`, eight at a time, so the computer can do the
/// eight at once: a long mix is read in a few milliseconds.
fn loudest_of(samples: &[f32]) -> Loudest {
    let mut top = [0.0f32; 8];
    // Anything times 0 is 0, but for a sample that isn't a number or is
    // infinite: then the sum isn't a number either.
    let mut poison = [0.0f32; 8];
    let mut eights = samples.chunks_exact(8);
    for eight in &mut eights {
        for k in 0..8 {
            let a = eight[k].abs();
            top[k] = if a > top[k] { a } else { top[k] };
            poison[k] += eight[k] * 0.0;
        }
    }
    let mut loudest = top.iter().fold(0.0f32, |m, &t| m.max(t));
    let mut bad = poison.iter().any(|p| p.is_nan());
    for &s in eights.remainder() {
        loudest = loudest.max(s.abs());
        bad |= !s.is_finite();
    }
    Loudest(loudest, bad)
}

fn limit(left: &mut [f32], right: &mut [f32]) -> (f32, usize, u64, u64) {
    let n = left.len();
    // The usual mix is never too loud: one quick look for the loudest
    // sample (a sample that isn't a number makes the peak infinite) says
    // so, and it stops there.
    let (loudest, not_a_number) = loudest_of(left).max_and(loudest_of(right));
    let peak = if not_a_number { f32::INFINITY } else { loudest };
    if loudest <= 1.0 {
        return (peak, 0, 0, 0);
    }
    let level = |i: usize| left[i].abs().max(right[i].abs());
    let (mut over, mut first, mut last) = (0, 0, 0);
    for i in 0..n {
        if level(i) > 1.0 {
            if over == 0 {
                first = i as u64;
            }
            last = i as u64;
            over += 1;
        }
    }
    // The volume each sample needs to stay under the ceiling.
    let need: Vec<f32> = (0..n)
        .map(|i| {
            let l = level(i);
            if l > CEILING && l.is_finite() {
                CEILING / l
            } else if l.is_finite() {
                1.0
            } else {
                0.0
            }
        })
        .collect();
    let ahead = SAMPLE_RATE as usize / 200; // 5 ms
    // The quietest need from here to 5 ms ahead (a sliding minimum)...
    let mut lowest = vec![1.0f32; n];
    let mut window: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    for i in (0..n).rev() {
        while window.back().is_some_and(|&j| need[j] >= need[i]) {
            window.pop_back();
        }
        window.push_back(i);
        while window.front().is_some_and(|&j| j > i + ahead) {
            window.pop_front();
        }
        lowest[i] = need[window[0]];
    }
    // ...averaged over the 5 ms before, so the volume glides down: each
    // of those minimums covers this sample, so their average is never
    // louder than it may be.
    // (Before the start, the first minimum stands in: it covers them too.)
    let mut gain = vec![1.0f32; n];
    let mut sum = 0.0f64;
    for i in 0..n {
        sum += lowest[i] as f64;
        if i > ahead {
            sum -= lowest[i - ahead - 1] as f64;
        }
        let before_start = ahead.saturating_sub(i) as f64 * lowest[0] as f64;
        gain[i] = ((sum + before_start) / (ahead + 1) as f64) as f32;
    }
    // Coming back up slowly, never faster than the average allows.
    let release = 1.0 - (-1.0 / (0.1 * SAMPLE_RATE as f32)).exp();
    let mut now = 1.0f32;
    for i in 0..n {
        now = (now + (1.0 - now) * release).min(gain[i]).min(need[i]);
        left[i] *= now;
        right[i] *= now;
    }
    (peak, over, first, last)
}

/// A 16-bit mono WAV file at [`SAMPLE_RATE`] holding `samples`.
pub(crate) fn wav_bytes(samples: impl Iterator<Item = i16>, count: usize) -> Vec<u8> {
    wav_file(samples, count, 1)
}

/// A 16-bit stereo WAV file at [`SAMPLE_RATE`]: `frames` pairs of left
/// and right samples.
pub(crate) fn stereo_wav_bytes(samples: impl Iterator<Item = i16>, frames: usize) -> Vec<u8> {
    wav_file(samples, frames * 2, 2)
}

/// A 16-bit WAV file at [`SAMPLE_RATE`] with `count` samples in all, in
/// `channels` channels side by side.
fn wav_file(samples: impl Iterator<Item = i16>, count: usize, channels: u16) -> Vec<u8> {
    let data_size = (count * 2) as u32;
    let mut out = Vec::with_capacity(44 + count * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // size of the fmt section
    out.extend_from_slice(&1u16.to_le_bytes()); // plain PCM
    out.extend_from_slice(&channels.to_le_bytes()); // 1 mono, 2 stereo
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    let frame = 2 * channels as u32; // bytes per moment: 2 a sample
    out.extend_from_slice(&(SAMPLE_RATE * frame).to_le_bytes()); // bytes per second
    out.extend_from_slice(&(frame as u16).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_lasts_as_long_as_asked() {
        let beep = Sound {
            ms: 100.0,
            ..Default::default()
        };
        assert_eq!(beep.render().len(), 4410);
        let silent = Sound {
            ms: f32::NAN,
            ..Default::default()
        };
        assert!(silent.render().is_empty());
    }

    #[test]
    fn every_wave_stays_in_range_and_fades() {
        for wave in [Wave::Square, Wave::Sine, Wave::Triangle, Wave::Noise] {
            let s = Sound {
                wave,
                volume: 1.0,
                attack: 10.0,
                decay: 10.0,
                ..Default::default()
            };
            let samples = s.render();
            assert!(samples.iter().all(|v| (-1.0..=1.0).contains(v)), "{wave:?}");
            assert_eq!(samples[0], 0.0, "{wave:?} fades in from silence");
            assert!(samples.last().unwrap().abs() < 0.01, "{wave:?} fades out");
            let loudest = samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(loudest > 0.9, "{wave:?} reaches full volume: {loudest}");
        }
    }

    #[test]
    fn sine_is_close_to_the_real_thing() {
        for i in 0..100 {
            let phase = i as f32 / 100.0;
            let real = (phase * std::f32::consts::TAU).sin();
            assert!((sine(phase) - real).abs() < 0.002, "at {phase}");
        }
    }

    #[test]
    fn a_square_wave_has_the_right_pitch() {
        // 441 Hz: exactly 100 samples per wave, 50 up then 50 down.
        let s = Sound {
            freq: 441.0,
            attack: 0.0,
            decay: 0.0,
            volume: 1.0,
            ms: 10.0,
            ..Default::default()
        };
        let samples = s.render();
        let flips = samples.windows(2).filter(|w| w[0] != w[1]).count();
        assert_eq!(flips, 8, "441 samples, 100 per wave: 8 flips");
        assert_eq!(samples[25], 1.0, "up for the first half of each wave");
        assert_eq!(samples[75], -1.0, "down for the second half");
        assert_eq!(samples[125], 1.0);
    }

    #[test]
    fn presets_have_names_and_rendering_is_repeatable() {
        for (sound, name) in [
            (Sound::coin(), "coin"),
            (Sound::jump(), "jump"),
            (Sound::hit(), "hit"),
            (Sound::explosion(), "explosion"),
        ] {
            assert_eq!(sound.label(), name);
            assert_eq!(sound.render(), sound.render());
        }
        assert_eq!(Sound::default().label(), "sound");
    }

    #[test]
    fn wav_files_round_trip_and_resample() {
        let samples: Vec<i16> = (0..2205).map(|i| (i * 10) as i16).collect();
        let bytes = wav_bytes(samples.iter().copied(), samples.len());
        let decoded = decode_wav(&bytes).unwrap();
        assert!(!decoded.is_stereo());
        assert_eq!(decoded.frames(), 2205);
        // Already at 44100 Hz: not resampled, so exactly the file's samples.
        assert_eq!(decoded.frame(100), (1000.0 / 32768.0, 1000.0 / 32768.0));

        // An 8-bit stereo file at 22050 Hz: stays stereo, stretched to 44100.
        let mut odd = Vec::new();
        odd.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
        odd.extend_from_slice(&16u32.to_le_bytes());
        odd.extend_from_slice(&1u16.to_le_bytes());
        odd.extend_from_slice(&2u16.to_le_bytes()); // stereo
        odd.extend_from_slice(&22050u32.to_le_bytes());
        odd.extend_from_slice(&44100u32.to_le_bytes());
        odd.extend_from_slice(&2u16.to_le_bytes());
        odd.extend_from_slice(&8u16.to_le_bytes());
        odd.extend_from_slice(b"data");
        odd.extend_from_slice(&4u32.to_le_bytes());
        odd.extend_from_slice(&[128, 128, 255, 1]); // silence, then left up + right down
        let decoded = decode_wav(&odd).unwrap();
        assert!(decoded.is_stereo());
        assert_eq!(decoded.frames(), 4, "2 moments at 22050 Hz are 4 at 44100");
        let (left, right) = decoded.frame(0);
        assert!(left.abs() < 1e-6 && right.abs() < 1e-6, "silence first");
        // The second moment is still left up and right down: not mixed away.
        let (left, right) = decoded.frame(2);
        assert!((left - 127.0 / 128.0).abs() < 0.01, "{left}");
        assert!((right + 127.0 / 128.0).abs() < 0.01, "{right}");
    }

    /// A WAV file at `rate` with `channels` channels, holding `samples`
    /// (side by side for stereo), as 16-bit numbers.
    fn wav_at(rate: u32, channels: u16, samples: &[f32]) -> Vec<u8> {
        let mut bytes = wav_file(
            samples.iter().map(|&s| (s * 32767.0).round() as i16),
            samples.len(),
            channels,
        );
        bytes[24..28].copy_from_slice(&rate.to_le_bytes());
        let frame = 2 * channels as u32;
        bytes[28..32].copy_from_slice(&(rate * frame).to_le_bytes());
        bytes
    }

    /// `seconds` of a sine wave at `freq` Hz, `rate` samples a second.
    fn sine_wave(freq: f32, rate: u32, seconds: f32) -> Vec<f32> {
        let count = (rate as f32 * seconds) as usize;
        (0..count)
            .map(|i| 0.5 * (std::f32::consts::TAU * freq * i as f32 / rate as f32).sin())
            .collect()
    }

    /// The biggest difference between `samples` (at 44100 Hz) and the
    /// perfect sine wave, away from the first and last 10 ms (where the
    /// sound starts and stops, and a filter has nothing to see past them).
    fn worst_error(samples: impl Iterator<Item = f32>, freq: f32) -> f32 {
        let samples: Vec<f32> = samples.collect();
        let edge = 441;
        (edge..samples.len() - edge)
            .map(|i| {
                let ideal = 0.5 * (std::f32::consts::TAU * freq * i as f32 / 44_100.0).sin();
                (samples[i] - ideal).abs()
            })
            .fold(0.0, f32::max)
    }

    #[test]
    fn stereo_files_keep_left_and_right_apart() {
        // Left: a 440 Hz sine. Right: silence, then a steady -0.25.
        let left = sine_wave(440.0, 44_100, 0.1);
        let interleaved: Vec<f32> = left
            .iter()
            .enumerate()
            .flat_map(|(i, &l)| [l, if i < 2000 { 0.0 } else { -0.25 }])
            .collect();
        let decoded = decode_wav(&wav_at(44_100, 2, &interleaved)).unwrap();
        assert!(decoded.is_stereo());
        assert_eq!(decoded.frames(), left.len());
        for i in [0, 100, 1999, 2000, 4000] {
            let (l, r) = decoded.frame(i);
            assert!((l - left[i]).abs() < 1e-4, "left at {i}");
            let want = if i < 2000 { 0.0 } else { -0.25 };
            assert!((r - want).abs() < 1e-4, "right at {i}: {r}");
        }
        // A mono file is the same on both sides, exactly as before.
        let mono = decode_wav(&wav_at(44_100, 1, &left)).unwrap();
        assert!(!mono.is_stereo());
        let (l, r) = mono.frame(100);
        assert_eq!(l, r);
        assert!((l - left[100]).abs() < 1e-4);
    }

    #[test]
    fn a_resampled_sine_stays_a_clean_sine_at_the_right_length() {
        for rate in [16_000, 22_050, 32_000, 48_000] {
            let decoded = decode_wav(&wav_at(rate, 1, &sine_wave(1000.0, rate, 1.0))).unwrap();
            assert_eq!(decoded.frames(), 44_100, "one second at {rate} Hz");
            let error = worst_error((0..decoded.frames()).map(|i| decoded.frame(i).0), 1000.0);
            // About 0.0004 here; straight lines between the samples (the
            // old way) were off by 0.005 at 22050 Hz.
            assert!(error < 0.001, "{rate} Hz: off by {error}");
        }
        // A stereo file is resampled side by side, each side on its own.
        let (a, b) = (
            sine_wave(500.0, 22_050, 0.5),
            sine_wave(3000.0, 22_050, 0.5),
        );
        let both: Vec<f32> = a.iter().zip(&b).flat_map(|(&l, &r)| [l, r]).collect();
        let decoded = decode_wav(&wav_at(22_050, 2, &both)).unwrap();
        assert_eq!(decoded.frames(), 22_050);
        let frames = 0..decoded.frames();
        assert!(worst_error(frames.clone().map(|i| decoded.frame(i).0), 500.0) < 0.001);
        assert!(worst_error(frames.map(|i| decoded.frame(i).1), 3000.0) < 0.001);
    }

    #[test]
    fn notes_too_high_for_44100_are_removed_not_folded_down() {
        // 30 kHz can't exist at 44100 samples a second. Without a low-pass
        // filter it would come out as a false 14.1 kHz whine.
        let decoded = decode_wav(&wav_at(96_000, 1, &sine_wave(30_000.0, 96_000, 0.5))).unwrap();
        assert_eq!(decoded.frames(), 22_050);
        let inside = 441..decoded.frames() - 441;
        let loudest = inside.map(|i| decoded.frame(i).0.abs()).fold(0.0, f32::max);
        assert!(loudest < 0.01, "a sine of 0.5 came out at {loudest}");
    }

    #[test]
    fn a_minute_of_music_resamples_quickly() {
        let music = sine_wave(440.0, 22_050, 60.0);
        let both: Vec<f32> = music.iter().flat_map(|&s| [s, -s]).collect();
        let started = std::time::Instant::now();
        let out = resample(&both, 2, 22_050);
        let seconds = started.elapsed().as_secs_f32();
        assert_eq!(out.len(), 2 * 60 * 44_100);
        // Well under a second when optimized; this allows for test builds.
        assert!(seconds < 20.0, "{seconds} s");
    }

    #[test]
    fn stereo_and_panned_sounds_make_a_stereo_recording() {
        let played = |samples: Samples, pan: f32| PlayedSound {
            frame: 1,
            name: String::from("t"),
            source: SoundSource::Synth(Sound::default()),
            samples,
            pan,
            volume: 1.0,
            start: 0,
            group: SoundGroup::Effects,
            event: None,
        };
        // Mono and not panned: a mono file, as always.
        let mono = mix_to_wav(
            &[played(Samples::mono(vec![0.5; 4]), 0.0)],
            &[],
            4,
            &MixLog::default(),
        )
        .bytes;
        assert_eq!(u16::from_le_bytes([mono[22], mono[23]]), 1);
        // A stereo sound: a stereo file with each side kept.
        let stereo = Samples::stereo(vec![0.5, -0.5, 0.25, 0.0]);
        let wav = mix_to_wav(&[played(stereo, 0.0)], &[], 2, &MixLog::default()).bytes;
        let decoded = decode_wav(&wav).unwrap();
        assert!(decoded.is_stereo());
        assert!((decoded.frame(0).0 - 0.5).abs() < 0.001);
        assert!((decoded.frame(0).1 + 0.5).abs() < 0.001);
        // A mono sound panned all the way right: nothing on the left.
        let wav = mix_to_wav(
            &[played(Samples::mono(vec![0.5; 2]), 1.0)],
            &[],
            2,
            &MixLog::default(),
        )
        .bytes;
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.frame(1).0, 0.0);
        assert!((decoded.frame(1).1 - 0.5).abs() < 0.001);
        assert_eq!(
            pan_gains(0.0),
            (1.0, 1.0),
            "the middle is as loud as no pan"
        );
        assert_eq!(pan_gains(-0.5), (1.0, 0.5));
        assert_eq!(pan_gains(f32::NAN), (1.0, 1.0));
        let json = played(Samples::mono(vec![]), -0.5).to_json();
        assert!(json.ends_with(r#""pan": -0.5}"#), "{json}");
    }

    #[test]
    fn broken_wav_files_say_what_is_wrong() {
        assert!(
            decode_wav(b"hello")
                .unwrap_err()
                .contains("doesn't start like a WAV")
        );
        let mut no_data = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        no_data.extend_from_slice(&16u32.to_le_bytes());
        no_data.extend_from_slice(&[1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0]);
        assert!(decode_wav(&no_data).unwrap_err().contains("no \"data\""));
        let mut mp3ish = no_data.clone();
        mp3ish[20] = 0x55; // format 0x55 is MP3
        mp3ish.extend_from_slice(b"data\0\0\0\0");
        assert!(decode_wav(&mp3ish).unwrap_err().contains("compressed"));

        let dir = crate::test_folder::TestFolder::new("wav");
        let missing = Wav::load(dir.join("nope.wav")).unwrap_err().to_string();
        assert!(
            missing.contains("couldn't find the sound file"),
            "{missing}"
        );
        fs::write(dir.join("bad.wav"), b"not a wav").unwrap();
        let bad = Wav::load(dir.join("bad.wav")).unwrap_err().to_string();
        assert!(
            bad.contains("as a WAV sound") && bad.contains("16-bit PCM"),
            "{bad}"
        );
    }

    /// A sound called `name` in `group`: `level` from sample `start` for
    /// `length` samples.
    fn named(name: &str, group: SoundGroup, start: u64, length: usize, level: f32) -> PlayedSound {
        PlayedSound {
            name: name.to_string(),
            group,
            ..sound_at(start, vec![level; length])
        }
    }

    /// A loop of a steady `level`, from sample 0 to the end.
    fn steady_loop(name: &str, group: SoundGroup, level: f32) -> PlayedLoop {
        PlayedLoop {
            id: LoopId(1),
            name: name.to_string(),
            start: 1,
            stop: None,
            volumes: vec![(1, 1.0)],
            source: SoundSource::Synth(Sound::default()),
            samples: Samples::mono(vec![level; 100]),
            start_sample: 0,
            stop_sample: None,
            volume_samples: vec![0],
            group,
        }
    }

    fn mixed_at(
        sounds: &[PlayedSound],
        loops: &[PlayedLoop],
        length: u64,
        mix: &MixLog,
    ) -> Samples {
        decode_wav(&mix_to_wav(sounds, loops, length, mix).bytes).unwrap()
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.002
    }

    #[test]
    fn group_volumes_and_the_master_turn_their_sounds_down() {
        let effect = named("door", SoundGroup::Effects, 0, 20_000, 0.5);
        let music = steady_loop("theme", SoundGroup::Music, 0.4);
        let mix = MixLog {
            changes: vec![
                (0, MixCommand::GroupVolume(SoundGroup::Music, 0.5)),
                (10_000, MixCommand::Master(0.5)),
            ],
        };
        let out = mixed_at(
            std::slice::from_ref(&effect),
            std::slice::from_ref(&music),
            20_000,
            &mix,
        );
        assert!(
            near(out.frame(5_000).0, 0.5 + 0.4 * 0.5),
            "{}",
            out.frame(5_000).0
        );
        assert!(
            near(out.frame(15_000).0, (0.5 + 0.2) * 0.5),
            "{}",
            out.frame(15_000).0
        );
        // It glides there (10 ms), not jumps.
        let halfway = out.frame(10_000 + 220).0;
        assert!(halfway < 0.69 && halfway > 0.36, "{halfway}");
        // With no changes, nothing is touched: the old mix to the bit.
        assert!(MixLog::default().gains(&[effect], &[music], 10).is_none());
    }

    #[test]
    fn music_dips_while_a_scream_plays_and_comes_back_after() {
        let music = steady_loop("theme", SoundGroup::Music, 0.4);
        // A silent "scream" from 1 s to 2 s (so only the music is heard).
        let rate = SAMPLE_RATE as u64;
        let scream = named("scream", SoundGroup::Effects, rate, rate as usize, 0.0);
        let steps = named("steps", SoundGroup::Effects, 0, 3 * rate as usize, 0.0);
        let mix = MixLog {
            changes: vec![(
                0,
                MixCommand::Duck(SoundGroup::Music, 0.25, DuckTrigger::from("scream")),
            )],
        };
        let out = mixed_at(&[scream, steps], &[music], 3 * rate, &mix);
        let at = |seconds: f32| out.frame((seconds * rate as f32) as usize).0;
        assert!(near(at(0.5), 0.4), "before: {}", at(0.5));
        assert!(near(at(1.15), 0.1), "ducked within 0.1 s: {}", at(1.15));
        assert!(near(at(1.9), 0.1));
        assert!(
            at(2.2) > 0.1 && at(2.2) < 0.4,
            "coming back up: {}",
            at(2.2)
        );
        assert!(near(at(2.6), 0.4), "back after 0.5 s: {}", at(2.6));
        // Footsteps don't duck it; ducking for a whole group does.
        let music = steady_loop("rain", SoundGroup::Ambience, 0.4);
        let mix = MixLog {
            changes: vec![(
                0,
                MixCommand::Duck(
                    SoundGroup::Ambience,
                    0.5,
                    DuckTrigger::from(SoundGroup::Effects),
                ),
            )],
        };
        let steps = named("steps", SoundGroup::Effects, rate, rate as usize, 0.0);
        let out = mixed_at(&[steps], &[music], 3 * rate, &mix);
        assert!(near(out.frame((rate * 3 / 2) as usize).0, 0.2));
    }

    #[test]
    fn a_mix_too_loud_says_what_was_playing_and_what_to_turn_down() {
        let rate = SAMPLE_RATE as u64;
        // A scream (0.9) over thunder (0.6) at 1 s; later a crash (0.7)
        // over the thunder again, quieter.
        let sounds = [
            named("scream", SoundGroup::Effects, rate, 2000, 0.9),
            named("thunder", SoundGroup::Ambience, rate - 1000, 6000, 0.6),
            named("thunder", SoundGroup::Ambience, 3 * rate, 6000, 0.6),
            named("crash", SoundGroup::Effects, 3 * rate + 100, 2000, 0.7),
        ];
        let mixed = mix_to_wav(&sounds, &[], 4 * rate, &MixLog::default());
        assert_eq!(
            mixed.moments.len(),
            2,
            "two moments, a second or more apart"
        );
        let first = &mixed.moments[0];
        assert!(near(first.level, 1.5));
        assert_eq!(first.sounds[0].0, "scream");
        assert!(near(first.sounds[0].3, 0.9) && near(first.sounds[1].3, 0.6));
        // The scream alone, turned down to 0.42, brings it under 0.98.
        assert_eq!(
            first.suggestion(),
            [(String::from("scream"), false, 1.0, 0.42)]
        );
        let second = &mixed.moments[1];
        assert!(near(second.level, 1.3));
        assert_eq!(second.sounds[0].0, "crash");
    }

    fn sound_at(start: u64, samples: Vec<f32>) -> PlayedSound {
        PlayedSound {
            frame: 1,
            name: String::from("t"),
            source: SoundSource::Synth(Sound::default()),
            samples: Samples::mono(samples),
            pan: 0.0,
            volume: 1.0,
            start,
            group: SoundGroup::Effects,
            event: None,
        }
    }

    #[test]
    fn mixing_adds_sounds_at_their_start_and_limits_what_would_clip() {
        let wav = mix_to_wav(&[sound_at(2, vec![0.5, 0.25])], &[], 3, &MixLog::default());
        let decoded = decode_wav(&wav.bytes).unwrap();
        assert_eq!(decoded.frames(), 4, "the sound rings on past the length");
        for (i, want) in [0.0, 0.0, 0.5, 0.25].into_iter().enumerate() {
            assert!((decoded.frame(i).0 - want).abs() < 0.001, "{decoded:?}");
        }
        assert_eq!((wav.peak, wav.over), (0.5, 0), "quiet enough: untouched");

        // 0.5 + 0.75 would be 1.25: the limiter turns the mix down to the
        // ceiling there, and (looking ahead) just before, smoothly.
        let wav = mix_to_wav(
            &[sound_at(2, vec![0.5, 0.5]), sound_at(3, vec![0.75, 0.25])],
            &[],
            3,
            &MixLog::default(),
        );
        let decoded = decode_wav(&wav.bytes).unwrap();
        let k = CEILING / 1.25;
        for (i, want) in [0.0, 0.0, 0.5 * k, CEILING, 0.25 * k]
            .into_iter()
            .enumerate()
        {
            assert!(
                (decoded.frame(i).0 - want).abs() < 0.001,
                "{i}: {decoded:?}"
            );
        }
        assert_eq!(
            (wav.peak, wav.over, wav.first_over, wav.last_over),
            (1.25, 1, 3, 3)
        );
    }

    #[test]
    fn the_limiter_leaves_quiet_parts_alone_and_never_lets_the_mix_clip() {
        // A steady tone at 0.5 for a second, and a burst three times as
        // loud from 0.4 to 0.5 seconds: 2.0 at its loudest.
        let rate = SAMPLE_RATE as usize;
        let tone: Vec<f32> = (0..rate).map(|i| 0.5 * (i as f32 * 0.05).sin()).collect();
        let burst: Vec<f32> = (0..rate / 10)
            .map(|i| 1.5 * (i as f32 * 0.31).sin())
            .collect();
        let start = (rate * 4 / 10) as u64;
        let wav = mix_to_wav(
            &[sound_at(0, tone.clone()), sound_at(start, burst)],
            &[],
            rate as u64,
            &MixLog::default(),
        );
        assert!(
            wav.peak > 1.9 && wav.over > 1000,
            "{} {}",
            wav.peak,
            wav.over
        );
        let decoded = decode_wav(&wav.bytes).unwrap();
        let (loudest, at) = (0..decoded.frames())
            .map(|i| (decoded.frame(i).0.abs(), i))
            .fold((0.0f32, 0), |a, b| if b.0 > a.0 { b } else { a });
        assert!(loudest <= CEILING + 0.0001, "never over: {loudest} at {at}");
        // Long before the burst, and long after it, the tone is as it was.
        for i in [1000, rate / 4, rate - 100] {
            assert!((decoded.frame(i).0 - tone[i]).abs() < 0.002, "{i}");
        }
        // During it, it's turned down rather than cut flat: no run of
        // samples stuck at the ceiling.
        let stuck = (start as usize..start as usize + rate / 10)
            .filter(|&i| decoded.frame(i).0.abs() >= CEILING - 0.0005)
            .count();
        assert!(stuck < 200, "{stuck} samples at the ceiling");
    }

    #[test]
    fn loops_repeat_with_no_gap_change_volume_and_stop() {
        let looped = |stop: Option<u64>| PlayedLoop {
            id: LoopId(1),
            name: String::from("hum"),
            start: 1,
            stop: stop.map(|_| 3),
            volumes: vec![(1, 1.0), (2, 0.5)],
            source: SoundSource::Synth(Sound::default()),
            samples: Samples::mono(vec![0.1, 0.2, 0.3]),
            start_sample: 1,
            stop_sample: stop,
            volume_samples: vec![1, 5],
            group: SoundGroup::Music,
        };
        // Starts at sample 1, goes round every 3 samples, half as loud
        // from sample 5, stopped at sample 7.
        let wav = mix_to_wav(&[], &[looped(Some(7))], 9, &MixLog::default()).bytes;
        let decoded = decode_wav(&wav).unwrap();
        let expected = [0.0, 0.1, 0.2, 0.3, 0.1, 0.1, 0.15, 0.0, 0.0];
        assert_eq!(decoded.frames(), 9);
        for (i, want) in expected.into_iter().enumerate() {
            assert!((decoded.frame(i).0 - want).abs() < 0.001, "{decoded:?}");
        }
        // Never stopped: it plays to the end of the run, and no further.
        let wav = mix_to_wav(&[], &[looped(None)], 9, &MixLog::default()).bytes;
        let decoded = decode_wav(&wav).unwrap();
        assert!((decoded.frame(8).0 - 0.1).abs() < 0.001, "{decoded:?}");
        let json = looped(Some(7)).to_json();
        assert!(
            json.starts_with(r#"{"loop": "hum", "wave": "square""#)
                && json.ends_with(r#""start": 1, "stop": 3, "volume": [[1, 1], [2, 0.5]]}"#),
            "{json}"
        );
    }
}
