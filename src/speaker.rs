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

use std::sync::{Arc, Mutex};

use crate::sound::SAMPLE_RATE;

/// At most this many sounds play at once; a new one replaces the oldest.
const MAX_VOICES: usize = 32;
/// Samples per refill: 1024 is about 23 milliseconds, short enough that a
/// sound feels instant, long enough not to stutter.
const BLOCK: usize = 1024;

/// The sounds playing right now, and how far through each one we are.
#[derive(Default)]
struct Voices {
    playing: Vec<(Arc<[f32]>, usize)>,
}

impl Voices {
    /// Adds up every playing sound into `out` and moves them all on.
    fn fill(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        for (samples, at) in &mut self.playing {
            let rest = &samples[(*at).min(samples.len())..];
            for (o, s) in out.iter_mut().zip(rest) {
                *o += s;
            }
            *at += out.len();
        }
        self.playing.retain(|(samples, at)| *at < samples.len());
        for o in out {
            *o = o.clamp(-1.0, 1.0);
        }
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

    /// Starts playing `samples` (at [`SAMPLE_RATE`], mono).
    pub(crate) fn play(&self, samples: Arc<[f32]>) {
        // A panic on the sound thread can't happen while it holds the
        // lock (filling can't panic), but recover anyway rather than crash.
        let mut voices = self.voices.lock().unwrap_or_else(|e| e.into_inner());
        if voices.playing.len() >= MAX_VOICES {
            voices.playing.remove(0);
        }
        voices.playing.push((samples, 0));
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
                channel_sample_count: BLOCK,
            };
            let mut mono = vec![0.0; BLOCK];
            let device = run_output_device(params, move |stereo: &mut [f32]| {
                let frames = stereo.len() / 2;
                mono.resize(frames, 0.0);
                voices
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .fill(&mut mono);
                for (pair, &sample) in stereo.chunks_exact_mut(2).zip(&mono) {
                    pair[0] = sample; // left
                    pair[1] = sample; // right
                }
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
            // Mono 16-bit at 44100 Hz, letting ALSA convert if the card
            // wants something else, with about 50 ms of buffering.
            // SAFETY: `pcm` was just opened.
            let code = unsafe {
                (alsa.set_params)(
                    pcm,
                    FORMAT_S16_LE,
                    ACCESS_RW_INTERLEAVED,
                    1,
                    SAMPLE_RATE,
                    1,
                    50_000,
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
                    let mut mixed = vec![0.0f32; BLOCK];
                    let mut pcm16 = vec![0i16; BLOCK];
                    while still_running.load(Ordering::Relaxed) {
                        voices
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .fill(&mut mixed);
                        for (out, &s) in pcm16.iter_mut().zip(&mixed) {
                            *out = (s * 32767.0) as i16;
                        }
                        // Waits until the device has room, which is what
                        // keeps this loop in step with the speakers.
                        // SAFETY: the buffer holds BLOCK samples of one channel.
                        let written = unsafe {
                            (alsa.writei)(handle.0, pcm16.as_ptr().cast(), BLOCK as c_ulong)
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
            let beep: Arc<[f32]> = crate::sound::Sound::default().render().into();
            voices.lock().unwrap().playing.push((beep, 0));
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
            drop(device); // stops the thread and closes the device
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voices_add_up_move_on_and_finish() {
        let mut voices = Voices::default();
        voices.playing.push((vec![0.25, 0.25, 0.25].into(), 0));
        voices.playing.push((vec![0.5, 1.0].into(), 0));
        let mut out = [9.0; 2];
        voices.fill(&mut out);
        assert_eq!(out, [0.75, 1.0], "added together, and cut off at 1.0");
        voices.fill(&mut out);
        assert_eq!(out, [0.25, 0.0]);
        assert!(voices.playing.is_empty(), "both finished");
    }
}
