//! Playing sounds out loud, in a window. Only built with the `audio`
//! feature (on by default).
//!
//! A [`Speaker`] keeps a list of sounds that are playing. The sound
//! system asks for more samples a few dozen times a second, on its own
//! thread, and the speaker mixes the playing sounds into them.
//!
//! - On Windows and macOS the `tinyaudio` crate talks to the system.
//! - On Linux, the engine loads the system's sound library (ALSA,
//!   `libasound.so.2`) when the game starts, instead of linking to it when
//!   it's built. So building needs no extra packages, and a computer
//!   without sound still runs every game, just silently.
//!
//! Headless runs never use this: they mix sounds into `audio.wav` instead.
//!
//! When the game closes, the speaker prints one line saying how often the
//! sound thread was late with a refill (which is heard as a crackle or a
//! skip), like `duckforge: sound: 0 late refills in 5 minutes`. If it was
//! late often, a bigger buffer helps: set the `DUCKFORGE_SOUND_BLOCK`
//! environment variable to the moments per refill, like `4096`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::sound::{LoopCommand, LoopId, SAMPLE_RATE, Samples, pan_gains};

/// At most this many sounds play at once; a new one replaces the oldest.
const MAX_VOICES: usize = 32;

/// Moments per refill (each a left and a right sample), unless
/// `DUCKFORGE_SOUND_BLOCK` says otherwise.
///
/// On Windows, `tinyaudio` plays through DirectSound with two buffers of
/// this size: one plays while the other is refilled. A refill that comes
/// more than one buffer late writes over sound that's already playing.
/// DirectSound's "refill now" signal can itself come 10 milliseconds or so
/// late, and a busy computer wakes the sound thread later still, so 1024
/// (about 23 milliseconds) is too small. Windows gets 2048 (about 46 milliseconds):
/// a sound you start is heard 2 to 3 of these later, about 0.1 seconds.
#[cfg(target_os = "windows")]
const BLOCK: usize = 2048;
/// Everywhere else, 1024 (about 23 milliseconds).
#[cfg(not(target_os = "windows"))]
const BLOCK: usize = 1024;

/// The smallest and biggest `DUCKFORGE_SOUND_BLOCK` allowed.
const BLOCK_RANGE: std::ops::RangeInclusive<usize> = 256..=16384;

/// The moments per refill: [`BLOCK`], or the `DUCKFORGE_SOUND_BLOCK`
/// environment variable if it's set to a sensible number.
fn block_size() -> usize {
    let value = std::env::var("DUCKFORGE_SOUND_BLOCK").ok();
    block_size_from(value.as_deref()).unwrap_or_else(|warning| {
        eprintln!("duckforge: {warning}");
        BLOCK
    })
}

/// [`block_size`] given the variable's value, or a warning saying why the
/// value can't be used.
fn block_size_from(value: Option<&str>) -> Result<usize, String> {
    let Some(text) = value else {
        return Ok(BLOCK);
    };
    match text.trim().parse::<usize>() {
        Ok(moments) if BLOCK_RANGE.contains(&moments) => Ok(moments),
        _ => Err(format!(
            "ignoring DUCKFORGE_SOUND_BLOCK={text:?} (use a number from {} to {}, like 4096)",
            BLOCK_RANGE.start(),
            BLOCK_RANGE.end()
        )),
    }
}

/// How long `moments` take to play.
fn play_time(moments: usize) -> Duration {
    Duration::from_secs_f64(moments as f64 / SAMPLE_RATE as f64)
}

/// After this many late refills, the speaker warns (once) while the game
/// is still running.
const LATE_WARNING: u64 = 10;

/// When each refill came, to count the late ones.
///
/// A refill is due every block's play time. One that comes more than a
/// whole block after it was due (so more than two blocks after the one
/// before it) is late: on Windows that block has already played the old
/// sound again, or been played over.
#[derive(Default)]
struct Refills {
    first: Option<Instant>,
    last: Option<Instant>,
    /// How long one block plays: a refill is due this often.
    every: Duration,
    count: u64,
    late: u64,
    /// The longest time between two refills.
    worst: Duration,
    warned: bool,
}

impl Refills {
    /// Notes a refill of `moments` at `now`. Returns a warning, once, when
    /// the refills have been late many times.
    fn note(&mut self, now: Instant, moments: usize) -> Option<String> {
        self.every = play_time(moments);
        self.first.get_or_insert(now);
        if let Some(last) = self.last {
            let gap = now.saturating_duration_since(last);
            self.worst = self.worst.max(gap);
            if gap > 2 * self.every {
                self.late += 1;
            }
        }
        self.last = Some(now);
        self.count += 1;
        if self.late >= LATE_WARNING && !self.warned {
            self.warned = true;
            return Some(format!(
                "duckforge: sound: {} refills were late (the longest wait was {} ms, when one \
                 is due every {} ms), so the sound may crackle. A bigger buffer helps: \
                 set DUCKFORGE_SOUND_BLOCK={}",
                self.late,
                self.worst.as_millis(),
                self.every.as_millis(),
                (2 * moments).min(*BLOCK_RANGE.end())
            ));
        }
        None
    }

    /// The line printed when the game closes, like `duckforge: sound: 0
    /// late refills in 5 minutes (...)`, or `None` if the sound never
    /// started.
    fn report(&self) -> Option<String> {
        let (Some(first), Some(last)) = (self.first, self.last) else {
            return None;
        };
        if self.count < 2 {
            return None;
        }
        let plural = if self.late == 1 { "" } else { "s" };
        Some(format!(
            "duckforge: sound: {} late refill{plural} in {} (one is due every {} ms; the longest \
             wait was {} ms)",
            self.late,
            how_long(last.saturating_duration_since(first)),
            self.every.as_millis(),
            self.worst.as_millis()
        ))
    }
}

/// A length of time in words: "40 seconds", "5 minutes".
fn how_long(time: Duration) -> String {
    let seconds = time.as_secs_f64().round() as u64;
    if seconds < 90 {
        let plural = if seconds == 1 { "" } else { "s" };
        format!("{seconds} second{plural}")
    } else {
        format!("{} minutes", (seconds as f64 / 60.0).round() as u64)
    }
}

/// Mixes are left alone up to this loudness; louder peaks are rounded off
/// so they never pass 1.0.
const LIMIT_KNEE: f32 = 0.8;

/// A gentle limiter for one sample: anything between -0.8 and 0.8 is left
/// as it is, and louder samples bend smoothly towards 1.0 (or -1.0)
/// without reaching it. Cutting peaks off flat at 1.0 instead makes a
/// crackle whenever a few loud sounds add up.
fn soft_limit(sample: f32) -> f32 {
    let size = sample.abs();
    if size <= LIMIT_KNEE {
        return sample;
    }
    // tanh starts out rising at the same rate as the sample, so the bend
    // has no corner, and it never quite reaches 1.
    let room = 1.0 - LIMIT_KNEE;
    (LIMIT_KNEE + room * ((size - LIMIT_KNEE) / room).tanh()).copysign(sample)
}

/// One sound playing: its samples, how loud on the left and the right
/// (from its pan), and how far through it we are.
struct Voice {
    samples: Samples,
    gains: (f32, f32),
    at: usize,
}

/// One looping sound playing.
struct Loop {
    /// The loop's number, from its [`LoopId`](crate::sound::LoopId).
    id: u32,
    samples: Samples,
    /// How far through the samples we are.
    at: usize,
    /// Its volume now...
    volume: f32,
    /// ...and the volume it's going to. A change glides from one to the
    /// other across the next block, because a sudden jump clicks.
    target: f32,
    /// Stopped: it fades to silence over the next block, then goes.
    stopping: bool,
}

impl Loop {
    /// Is this the loop `id`, and still playing? (A stopping loop is
    /// fading out, so it takes no more changes.)
    fn is(&self, id: LoopId) -> bool {
        self.id == id.0 && !self.stopping
    }
}

/// The sounds playing right now, and how far through each one we are.
#[derive(Default)]
struct Voices {
    playing: Vec<Voice>,
    loops: Vec<Loop>,
    refills: Refills,
}

impl Voices {
    /// Adds up every playing sound into `out` (left and right side by
    /// side: left, right, left, right...) and moves them all on. A mono
    /// sound plays the same on both sides; a stereo one keeps its own.
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        for voice in &mut self.playing {
            let (to_left, to_right) = voice.gains;
            let end = (voice.at + frames).min(voice.samples.frames());
            for (pair, i) in out.chunks_exact_mut(2).zip(voice.at..end) {
                let (l, r) = voice.samples.frame(i);
                pair[0] += l * to_left;
                pair[1] += r * to_right;
            }
            voice.at += frames;
        }
        self.playing
            .retain(|voice| voice.at < voice.samples.frames());
        for looped in &mut self.loops {
            let length = looped.samples.frames();
            let (from, to) = (looped.volume, looped.target);
            looped.volume = to;
            if length == 0 || frames == 0 {
                continue;
            }
            for (n, pair) in out.chunks_exact_mut(2).enumerate() {
                // A straight line from the old volume to the new one,
                // reaching it at the block's last moment.
                let volume = from + (to - from) * (n + 1) as f32 / frames as f32;
                let (l, r) = looped.samples.frame(looped.at);
                pair[0] += l * volume;
                pair[1] += r * volume;
                looped.at = (looped.at + 1) % length; // round again, with no gap
            }
        }
        // Stopped loops have now faded out.
        self.loops.retain(|looped| !looped.stopping);
        for o in out {
            *o = soft_limit(*o);
        }
    }

    /// Starts, changes or stops a looping sound. A new volume, or a stop,
    /// takes effect over the next block.
    fn change_loop(&mut self, command: LoopCommand) {
        match command {
            LoopCommand::Start(id, request, volume) => {
                // A loop starts at its volume straight away.
                self.loops.push(Loop {
                    id: id.0,
                    samples: request.samples,
                    at: 0,
                    volume,
                    target: volume,
                    stopping: false,
                });
            }
            LoopCommand::Volume(id, volume) => {
                for looped in self.loops.iter_mut().filter(|l| l.is(id)) {
                    looped.target = volume;
                }
            }
            LoopCommand::Stop(id) => {
                for looped in self.loops.iter_mut().filter(|l| l.is(id)) {
                    looped.target = 0.0;
                    looped.stopping = true;
                }
            }
        }
    }
}

/// What both kinds of sound device do each time they want more sound:
/// note the time, then mix `out`. A warning about late refills is printed
/// after letting go of the lock, so printing can't hold up the game.
fn refill(voices: &Mutex<Voices>, out: &mut [f32]) {
    let now = Instant::now();
    let warning = {
        let mut voices = voices.lock().unwrap_or_else(|e| e.into_inner());
        let warning = voices.refills.note(now, out.len() / 2);
        voices.fill(out);
        warning
    };
    if let Some(warning) = warning {
        eprintln!("{warning}");
    }
}

/// The game's connection to the speakers.
pub(crate) struct Speaker {
    voices: Arc<Mutex<Voices>>,
    // Kept only so the device stays open: dropping it stops the sound.
    _device: backend::Device,
}

impl Speaker {
    /// Opens the default sound device, or says in one sentence why it
    /// couldn't (no sound card, no sound library...).
    pub(crate) fn open() -> Result<Speaker, String> {
        let voices = Arc::new(Mutex::new(Voices::default()));
        let device = backend::Device::open(Arc::clone(&voices))?;
        Ok(Speaker {
            voices,
            _device: device,
        })
    }

    /// Starts playing `samples` (at [`SAMPLE_RATE`], mono or stereo) at
    /// `pan` (-1 left, 0 both sides, 1 right).
    pub(crate) fn play(&self, samples: Samples, pan: f32) {
        // A panic on the sound thread can't happen while it holds the
        // lock (filling can't panic), but recover anyway rather than crash.
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        if voices.playing.len() >= MAX_VOICES {
            voices.playing.remove(0);
        }
        voices.playing.push(Voice {
            samples,
            gains: pan_gains(pan),
            at: 0,
        });
    }

    /// Starts, changes or stops a looping sound.
    pub(crate) fn change_loop(&self, command: LoopCommand) {
        self.voices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .change_loop(command);
    }
}

impl Drop for Speaker {
    /// Says how the sound went, in one line, as the game closes.
    fn drop(&mut self) {
        let voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(report) = voices.refills.report() {
            eprintln!("{report}");
        }
    }
}

/// Windows and macOS: the `tinyaudio` crate.
#[cfg(not(target_os = "linux"))]
mod backend {
    use super::*;
    use tinyaudio::{OutputDevice, OutputDeviceParameters, run_output_device};

    pub(crate) struct Device(#[allow(dead_code)] OutputDevice);

    impl Device {
        pub(crate) fn open(voices: Arc<Mutex<Voices>>) -> Result<Device, String> {
            let params = OutputDeviceParameters {
                sample_rate: SAMPLE_RATE as usize,
                channels_count: 2,
                channel_sample_count: block_size(),
            };
            // tinyaudio asks for left and right side by side, just as
            // `fill` makes them.
            let device = run_output_device(params, move |stereo: &mut [f32]| {
                refill(&voices, stereo);
            })
            .map_err(|err| format!("couldn't open the sound device ({err})"))?;
            Ok(Device(device))
        }
    }
}

/// Linux: ALSA, loaded when the game starts.
#[cfg(target_os = "linux")]
mod backend {
    use super::*;
    use std::ffi::{CStr, c_char, c_int, c_long, c_uint, c_ulong, c_void};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread::JoinHandle;

    /// ALSA's handle for an open sound device.
    type Pcm = *mut c_void;

    // The numbers ALSA uses for "play", "16-bit samples" and "one buffer
    // with the channels side by side" (from ALSA's header files).
    const STREAM_PLAYBACK: c_int = 0;
    const FORMAT_S16_LE: c_int = 2;
    const ACCESS_RW_INTERLEAVED: c_int = 3;

    /// The ALSA functions the engine uses, found in `libasound.so.2`.
    struct Alsa {
        open: unsafe extern "C" fn(*mut Pcm, *const c_char, c_int, c_int) -> c_int,
        set_params: unsafe extern "C" fn(Pcm, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int,
        writei: unsafe extern "C" fn(Pcm, *const c_void, c_ulong) -> c_long,
        recover: unsafe extern "C" fn(Pcm, c_int, c_int) -> c_int,
        close: unsafe extern "C" fn(Pcm) -> c_int,
        strerror: unsafe extern "C" fn(c_int) -> *const c_char,
    }

    /// Looks up the function `name` in `library`, as a function pointer of
    /// type `F`.
    ///
    /// # Safety
    ///
    /// `F` must be an `unsafe extern "C" fn` type matching the function's
    /// real signature.
    unsafe fn find<F: Copy>(library: *mut c_void, name: &CStr) -> Result<F, String> {
        assert_eq!(std::mem::size_of::<F>(), std::mem::size_of::<*mut c_void>());
        // SAFETY: `library` came from dlopen and `name` is a C string.
        let function = unsafe { libc::dlsym(library, name.as_ptr()) };
        if function.is_null() {
            return Err(format!("the ALSA library has no {name:?}"));
        }
        // SAFETY: a function's address and a function pointer to it are the
        // same bits; the caller promises `F` has the right signature.
        Ok(unsafe { std::mem::transmute_copy::<*mut c_void, F>(&function) })
    }

    impl Alsa {
        /// Loads the library and looks up each function by name.
        fn load() -> Result<Alsa, String> {
            // SAFETY: dlopen and dlsym are given valid C strings. Each
            // function pointer is used with the exact signature ALSA's
            // documentation gives it. The library is never unloaded, so
            // the pointers stay valid for the rest of the program.
            unsafe {
                let library = libc::dlopen(c"libasound.so.2".as_ptr(), libc::RTLD_NOW);
                if library.is_null() {
                    return Err(String::from(
                        "the ALSA sound library (libasound.so.2) isn't installed",
                    ));
                }
                Ok(Alsa {
                    open: find(library, c"snd_pcm_open")?,
                    set_params: find(library, c"snd_pcm_set_params")?,
                    writei: find(library, c"snd_pcm_writei")?,
                    recover: find(library, c"snd_pcm_recover")?,
                    close: find(library, c"snd_pcm_close")?,
                    strerror: find(library, c"snd_strerror")?,
                })
            }
        }

        fn error(&self, code: c_int) -> String {
            // SAFETY: snd_strerror returns a pointer to a constant C string.
            unsafe { CStr::from_ptr((self.strerror)(code)) }
                .to_string_lossy()
                .into_owned()
        }
    }

    /// The open device and the thread that keeps feeding it.
    pub(crate) struct Device {
        running: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    /// The device handle, moved to the feeding thread. ALSA allows a device
    /// to be used from another thread, as long as only one uses it at a time.
    struct Handle(Pcm);
    // SAFETY: only the feeding thread uses the handle after it's moved there.
    unsafe impl Send for Handle {}

    impl Device {
        pub(crate) fn open(voices: Arc<Mutex<Voices>>) -> Result<Device, String> {
            Device::open_named(c"default", voices)
        }

        /// Opens the ALSA device called `name` (`"default"` is the
        /// speakers; `"null"` swallows everything, which tests use).
        pub(super) fn open_named(
            name: &CStr,
            voices: Arc<Mutex<Voices>>,
        ) -> Result<Device, String> {
            let alsa = Alsa::load()?;
            let mut pcm: Pcm = std::ptr::null_mut();
            // SAFETY: `pcm` is a valid place for ALSA to write the handle.
            let code = unsafe { (alsa.open)(&mut pcm, name.as_ptr(), STREAM_PLAYBACK, 0) };
            if code < 0 {
                return Err(format!(
                    "couldn't open the sound device ({})",
                    alsa.error(code)
                ));
            }
            // Stereo 16-bit at 44100 Hz, letting ALSA convert if the card
            // wants something else, with room for two blocks (and at least
            // 50 ms) in ALSA's own buffer.
            let block = block_size();
            let buffer_us = (2 * play_time(block).as_micros()).max(50_000) as c_uint;
            // SAFETY: `pcm` was just opened.
            let code = unsafe {
                (alsa.set_params)(
                    pcm,
                    FORMAT_S16_LE,
                    ACCESS_RW_INTERLEAVED,
                    2,
                    SAMPLE_RATE,
                    1,
                    buffer_us,
                )
            };
            if code < 0 {
                // SAFETY: closing the handle we opened, once.
                unsafe { (alsa.close)(pcm) };
                return Err(format!(
                    "couldn't set up the sound device ({})",
                    alsa.error(code)
                ));
            }

            let running = Arc::new(AtomicBool::new(true));
            let still_running = Arc::clone(&running);
            let handle = Handle(pcm);
            let thread = std::thread::Builder::new()
                .name(String::from("duckforge sound"))
                .spawn(move || {
                    let handle = handle;
                    // Left and right side by side: two samples a moment.
                    let mut mixed = vec![0.0f32; 2 * block];
                    let mut pcm16 = vec![0i16; 2 * block];
                    while still_running.load(Ordering::Relaxed) {
                        refill(&voices, &mut mixed);
                        for (out, &s) in pcm16.iter_mut().zip(&mixed) {
                            *out = (s * 32767.0) as i16;
                        }
                        // Waits until the device has room, which is what
                        // keeps this loop in step with the speakers.
                        // SAFETY: the buffer holds `block` moments of two channels.
                        let written = unsafe {
                            (alsa.writei)(handle.0, pcm16.as_ptr().cast(), block as c_ulong)
                        };
                        if written < 0 {
                            // An "underrun" (we were late) or similar: reset and carry on.
                            // SAFETY: the handle is open.
                            let fixed = unsafe { (alsa.recover)(handle.0, written as c_int, 1) };
                            if fixed < 0 {
                                break; // the device is gone; stop quietly
                            }
                        }
                    }
                    // SAFETY: this thread is the last user of the handle.
                    unsafe { (alsa.close)(handle.0) };
                })
                .map_err(|err| format!("couldn't start the sound thread ({err})"))?;
            Ok(Device {
                running,
                thread: Some(thread),
            })
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn alsa_plays_into_the_null_device() {
            // Checks the function signatures for real, on computers that have
            // ALSA. The "null" device needs no sound card: it swallows samples.
            if Alsa::load().is_err() {
                return; // no ALSA here: nothing to check
            }
            let voices = Arc::new(Mutex::new(Voices::default()));
            let device = Device::open_named(c"null", Arc::clone(&voices))
                .expect("the null device always opens");
            let beep = Samples::mono(crate::sound::Sound::default().render());
            voices.lock().unwrap().playing.push(Voice {
                samples: beep,
                gains: (1.0, 1.0),
                at: 0,
            });
            // Wait until the sound thread has played the whole beep.
            for _ in 0..200 {
                if voices.lock().unwrap().playing.is_empty() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert!(
                voices.lock().unwrap().playing.is_empty(),
                "the beep was played"
            );
            assert!(
                voices.lock().unwrap().refills.report().is_some(),
                "each refill was timed"
            );
            drop(device); // stops the thread and closes the device
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(samples: Samples, pan: f32) -> Voice {
        Voice {
            samples,
            gains: pan_gains(pan),
            at: 0,
        }
    }

    fn looping(id: u32, samples: Samples, volume: f32) -> Loop {
        Loop {
            id,
            samples,
            at: 0,
            volume,
            target: volume,
            stopping: false,
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-6
    }

    /// Only the left samples of a filled buffer (left, right, left, right...).
    fn lefts(out: &[f32]) -> Vec<f32> {
        out.iter().step_by(2).copied().collect()
    }

    #[test]
    fn voices_add_up_move_on_and_finish() {
        let mut voices = Voices::default();
        voices
            .playing
            .push(voice(Samples::mono(vec![0.25, 0.25, 0.25]), 0.0));
        voices
            .playing
            .push(voice(Samples::mono(vec![0.5, 1.0]), 0.0));
        let mut out = [9.0; 4]; // two moments, left and right
        voices.fill(&mut out);
        assert_eq!(&out[..2], [0.75, 0.75], "added together");
        // 0.25 + 1.0 is too loud: the limiter brings it just under 1.0.
        assert!(out[2] > 0.99 && out[2] < 1.0, "{out:?}");
        assert_eq!(out[2], out[3]);
        voices.fill(&mut out);
        assert_eq!(out, [0.25, 0.25, 0.0, 0.0]);
        assert!(voices.playing.is_empty(), "both finished");
    }

    #[test]
    fn stereo_sounds_keep_their_sides_and_pans_move_sounds() {
        let mut voices = Voices::default();
        // Left 0.5 and right -0.25, then left 0.1 and right 0.2.
        voices
            .playing
            .push(voice(Samples::stereo(vec![0.5, -0.25, 0.1, 0.2]), 0.0));
        let mut out = [0.0; 4];
        voices.fill(&mut out);
        assert_eq!(out, [0.5, -0.25, 0.1, 0.2], "each side as it was");
        // A mono sound panned half left: the right side at half.
        voices
            .playing
            .push(voice(Samples::mono(vec![0.8, 0.8]), -0.5));
        voices.fill(&mut out);
        assert_eq!(out, [0.8, 0.4, 0.8, 0.4]);
    }

    #[test]
    fn loops_go_round_with_no_gap_at_their_volume() {
        let mut voices = Voices::default();
        voices
            .loops
            .push(looping(1, Samples::mono(vec![0.1, 0.2, 0.3]), 2.0));
        let mut out = [0.0; 8];
        voices.fill(&mut out);
        let want = [0.2, 0.4, 0.6, 0.2];
        for (got, want) in lefts(&out).iter().zip(want) {
            assert!((got - want).abs() < 1e-6, "{out:?}");
        }
        assert_eq!(out[0], out[1], "a mono loop is the same on both sides");
        voices.fill(&mut out);
        assert!(
            (out[0] - 0.4).abs() < 1e-6,
            "carries on where it was: {out:?}"
        );
    }

    #[test]
    fn the_limiter_leaves_normal_levels_alone_and_rounds_off_peaks() {
        for x in [0.0, 0.3, -0.5, 0.8, -0.8] {
            assert_eq!(soft_limit(x), x, "{x} is left as it is");
        }
        let mut last = 0.0;
        for step in 1..=400 {
            let x = step as f32 * 0.01; // 0.01 up to 4.0
            let y = soft_limit(x);
            assert!(y <= 1.0, "{x} never comes out past 1.0, giving {y}");
            if x <= 1.5 {
                assert!(y < 1.0, "{x} stays under 1.0, giving {y}");
            }
            assert!(y >= last, "louder in never gives quieter out");
            assert_eq!(soft_limit(-x), -y, "the same for the low side");
            last = y;
        }
        // No corner at the knee: just past it, the output still rises at
        // the same rate as the input.
        let rise = (soft_limit(0.801) - soft_limit(0.8)) / 0.001;
        assert!((rise - 1.0).abs() < 0.01, "slope {rise}");
        // A very loud mix comes out just under full, not cut off flat.
        assert!(soft_limit(1.2) > 0.95 && soft_limit(1.2) < 1.0);
    }

    #[test]
    fn a_loud_mix_is_rounded_off_not_cut_flat() {
        let mut voices = Voices::default();
        // Three loud loops add up to 0.9, 1.2 and 1.5.
        for id in 1..=3 {
            voices
                .loops
                .push(looping(id, Samples::mono(vec![0.3, 0.4, 0.5]), 1.0));
        }
        let mut out = [0.0; 6];
        voices.fill(&mut out);
        let got = lefts(&out);
        assert!(got[0] < 0.9 && got[0] > 0.85, "{got:?}");
        assert!(
            got[0] < got[1] && got[1] < got[2],
            "still in order: {got:?}"
        );
        assert!(got[2] < 1.0, "{got:?}");
    }

    #[test]
    fn a_new_loop_volume_glides_across_one_block() {
        let mut voices = Voices::default();
        voices.loops.push(looping(7, Samples::mono(vec![1.0]), 0.2));
        voices.change_loop(LoopCommand::Volume(LoopId(7), 0.6));
        let mut out = [0.0; 8]; // four moments
        voices.fill(&mut out);
        let want = [0.3, 0.4, 0.5, 0.6];
        for (got, want) in lefts(&out).iter().zip(want) {
            assert!(close(*got, want), "a step at a time to 0.6: {out:?}");
        }
        voices.fill(&mut out);
        assert!(
            lefts(&out).iter().all(|&v| close(v, 0.6)),
            "then it stays there: {out:?}"
        );
    }

    #[test]
    fn a_stopped_loop_fades_out_then_goes() {
        let mut voices = Voices::default();
        voices.loops.push(looping(3, Samples::mono(vec![1.0]), 0.8));
        voices.change_loop(LoopCommand::Stop(LoopId(3)));
        // Too late: it's stopping.
        voices.change_loop(LoopCommand::Volume(LoopId(3), 1.0));
        let mut out = [0.0; 8];
        voices.fill(&mut out);
        let want = [0.6, 0.4, 0.2, 0.0];
        for (got, want) in lefts(&out).iter().zip(want) {
            assert!(close(*got, want), "fades to silence: {out:?}");
        }
        assert!(voices.loops.is_empty(), "and then it's gone");
    }

    #[test]
    fn late_refills_are_counted_and_reported() {
        let mut refills = Refills::default();
        assert_eq!(refills.report(), None, "nothing to say before any sound");
        let start = Instant::now();
        let every = play_time(1024); // about 23 ms
        let mut now = start;
        // Ten minutes of refills on time, with a little wobble...
        for n in 0..26_000u32 {
            let wobble = Duration::from_millis(u64::from(n % 3) * 4);
            assert_eq!(refills.note(now + wobble, 1024), None);
            now += every;
        }
        assert_eq!(refills.late, 0);
        // ...then one that waits more than two blocks.
        now += 2 * every;
        assert_eq!(refills.note(now, 1024), None);
        assert_eq!(refills.late, 1);
        let report = refills.report().unwrap();
        assert!(
            report.starts_with("duckforge: sound: 1 late refill in 10 minutes"),
            "{report}"
        );
        assert!(report.contains("due every 23 ms"), "{report}");
        // Many late ones: a warning, only once.
        let mut warnings = 0;
        for _ in 0..20 {
            now += 3 * every;
            warnings += usize::from(refills.note(now, 1024).is_some());
        }
        assert_eq!(warnings, 1);
        assert_eq!(refills.late, 21);
    }

    #[test]
    fn times_read_as_words() {
        assert_eq!(how_long(Duration::from_secs(1)), "1 second");
        assert_eq!(how_long(Duration::from_millis(40_400)), "40 seconds");
        assert_eq!(how_long(Duration::from_secs(300)), "5 minutes");
    }

    #[test]
    fn the_block_size_can_be_changed_with_a_variable() {
        assert_eq!(block_size_from(None), Ok(BLOCK));
        assert_eq!(block_size_from(Some("4096")), Ok(4096));
        assert_eq!(block_size_from(Some(" 1536 ")), Ok(1536));
        for bad in ["", "big", "-5", "10", "100000"] {
            let warning = block_size_from(Some(bad)).unwrap_err();
            assert!(warning.contains("DUCKFORGE_SOUND_BLOCK"), "{warning}");
        }
    }
}
