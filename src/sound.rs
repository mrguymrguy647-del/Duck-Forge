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

use crate::report::ReportValue;

use crate::sprite::missing_file_error;

/// Samples per second of every sound in the engine (CD quality).
pub const SAMPLE_RATE: u32 = 44_100;

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
/// # fn example() -> std::io::Result<()> {
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
    samples: Arc<[f32]>,
}

impl Wav {
    /// Loads a `.wav` file: 8, 16, 24 or 32-bit, or floating point, at any
    /// sample rate, mono or stereo (stereo is mixed down to mono). The
    /// sound's name in logs is the file name without `.wav`.
    ///
    /// If the file is missing or isn't a WAV duckforge can read, the error
    /// says so in one sentence.
    pub fn load(path: impl AsRef<Path>) -> io::Result<Wav> {
        let path = path.as_ref();
        let bytes = fs::read(path).map_err(|error| missing_file_error(path, "sound", &error))?;
        let samples = decode_wav(&bytes).map_err(|problem| {
            io::Error::new(
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
            samples: samples.into(),
        })
    }

    /// The name shown in logs and used by play tests: the file name
    /// without `.wav`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How long the sound lasts, in seconds.
    pub fn seconds(&self) -> f32 {
        self.samples.len() as f32 / SAMPLE_RATE as f32
    }
}

impl std::fmt::Debug for Wav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Wav({:?}, {:.2} s)", self.path, self.seconds())
    }
}

/// Reads the samples out of a WAV file, mixed to mono and converted to
/// [`SAMPLE_RATE`]. The error says what's wrong with the file.
fn decode_wav(bytes: &[u8]) -> Result<Vec<f32>, String> {
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
    let mono: Vec<f32> = data
        .chunks_exact(frame)
        .map(|f| f.chunks_exact(width).map(read).sum::<f32>() / channels as f32)
        .collect();
    Ok(resample(&mono, rate))
}

/// Converts samples recorded at `rate` per second to [`SAMPLE_RATE`],
/// drawing straight lines between the original samples.
fn resample(samples: &[f32], rate: u32) -> Vec<f32> {
    if rate == SAMPLE_RATE || samples.is_empty() {
        return samples.to_vec();
    }
    let length = (samples.len() as u64 * SAMPLE_RATE as u64 / rate as u64) as usize;
    let step = rate as f64 / SAMPLE_RATE as f64;
    (0..length)
        .map(|i| {
            let at = i as f64 * step;
            let before = (at as usize).min(samples.len() - 1);
            let after = (before + 1).min(samples.len() - 1);
            let t = (at - before as f64) as f32;
            samples[before] + (samples[after] - samples[before]) * t
        })
        .collect()
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
    pub(crate) samples: Arc<[f32]>,
    /// The sample it starts at in `audio.wav`.
    pub(crate) start: u64,
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
    pub(crate) samples: Arc<[f32]>,
}

impl SoundRequest {
    pub(crate) fn synth(sound: Sound) -> SoundRequest {
        SoundRequest {
            name: sound.label().to_string(),
            samples: sound.render().into(),
            source: SoundSource::Synth(sound),
        }
    }

    pub(crate) fn wav(wav: &Wav) -> SoundRequest {
        SoundRequest {
            name: wav.name.clone(),
            source: SoundSource::File(wav.path.clone()),
            samples: Arc::clone(&wav.samples),
        }
    }
}

impl PlayedSound {
    /// The sound's line in `log.json`: frame and name first, then its
    /// settings or its file, like
    /// `{"frame": 10, "sound": "coin", "wave": "square", "freq": 1050, ...}`.
    pub(crate) fn to_json(&self) -> String {
        let text = |t: &str| Value::from(t).to_string();
        // Numbers to 2 decimal places, and whole numbers without ".0".
        let number = |v: f32| ReportValue::from((v as f64 * 100.0).round() / 100.0).to_json();
        let mut line = format!(
            "{{\"frame\": {}, \"sound\": {}",
            self.frame,
            text(&self.name)
        );
        match &self.source {
            SoundSource::Synth(s) => {
                line += &format!(", \"wave\": {}", text(s.wave.name()));
                for (key, value) in [
                    ("freq", s.freq),
                    ("slide_to", s.slide_to),
                    ("ms", s.ms),
                    ("volume", s.volume),
                    ("attack", s.attack),
                    ("decay", s.decay),
                ] {
                    line += &format!(", \"{key}\": {}", number(value));
                }
            }
            SoundSource::File(path) => {
                line += &format!(", \"file\": {}", text(&path.to_string_lossy()));
            }
        }
        line + "}"
    }
}

/// Mixes every sound into one track, each starting at its own sample, and
/// turns it into the bytes of a 16-bit mono `.wav` file. The track is at
/// least `length` samples long, and longer if a sound rings on past it.
pub(crate) fn mix_to_wav(sounds: &[PlayedSound], length: u64) -> Vec<u8> {
    let end = sounds
        .iter()
        .map(|s| s.start + s.samples.len() as u64)
        .fold(length, u64::max);
    let mut track = vec![0.0f32; end as usize];
    for sound in sounds {
        let start = sound.start as usize;
        for (out, &sample) in track[start..].iter_mut().zip(sound.samples.iter()) {
            *out += sample;
        }
    }
    // Too loud is cut off at the top (clipping), rather than wrapping round.
    let pcm = track
        .iter()
        .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16);
    wav_bytes(pcm, track.len())
}

/// A 16-bit mono WAV file at [`SAMPLE_RATE`] holding `samples`.
pub(crate) fn wav_bytes(samples: impl Iterator<Item = i16>, count: usize) -> Vec<u8> {
    let data_size = (count * 2) as u32;
    let mut out = Vec::with_capacity(44 + count * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // size of the fmt section
    out.extend_from_slice(&1u16.to_le_bytes()); // plain PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per sample
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
        assert_eq!(decoded.len(), 2205);
        assert_eq!(decoded[100], 1000.0 / 32768.0);

        // An 8-bit stereo file at 22050 Hz: mixed to mono and stretched to 44100.
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
        assert_eq!(decoded.len(), 4);
        assert_eq!(decoded[0], 0.0);
        assert!(
            decoded.iter().all(|v| v.abs() < 0.01),
            "left and right cancel out"
        );
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

        let dir = std::env::temp_dir().join(format!("duckforge-wav-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
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
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mixing_adds_sounds_at_their_start_and_clips() {
        let sound = |start: u64, samples: Vec<f32>| PlayedSound {
            frame: 1,
            name: String::from("t"),
            source: SoundSource::Synth(Sound::default()),
            samples: samples.into(),
            start,
        };
        let wav = mix_to_wav(&[sound(2, vec![0.5, 0.5]), sound(3, vec![0.75, 0.25])], 3);
        let decoded = decode_wav(&wav).unwrap();
        let expected = [0.0, 0.0, 0.5, 1.0, 0.25]; // 0.5 + 0.75 is cut off at 1.0
        assert_eq!(decoded.len(), 5, "the last sound rings on past the length");
        for (got, want) in decoded.iter().zip(expected) {
            assert!((got - want).abs() < 0.001, "{decoded:?}");
        }
    }
}
