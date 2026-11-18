//! Music: long sounds in OGG Vorbis files, decoded a little at a time
//! while they play, instead of whole into memory.
//!
//! A minute of stereo music as a `Wav` is 21 MB of samples in memory; as
//! an `.ogg` it's about 0.8 MB of file, and playing it needs a few
//! hundred KB more. Load it with [`Music::load`] and play it like a
//! looping `Wav`:
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # fn example(ctx: &mut Context) -> Result<(), duckforge::LoadError> {
//! let day = Music::load("examples/mygame/assets/day.ogg")?;
//! let theme = ctx.play_loop(&day, 0.6);
//! ctx.set_loop_volume(theme, 0.3);
//! ctx.stop_loop(theme);
//! # Ok(())
//! # }
//! ```
//!
//! How it plays:
//!
//! - **In a window**, each playing piece of music has a decoder thread
//!   that keeps about half a second of sound ready in a ring buffer
//!   ([`MusicStream`]). The speakers' sound thread only copies from that
//!   buffer: it never decodes, never reads a file and never waits. If the
//!   decoder ever falls behind, the sound thread plays silence for the
//!   missing part and counts it (it's in the line the game prints when it
//!   closes).
//! - **Headless** (`audio.wav`, tests, replays), the mixer decodes the
//!   music itself, in step with the frames, with no thread, so `audio.wav`
//!   is exact and the same on every run.
//!
//! Both use the same [`LoopReader`], which decodes, trims the music to
//! its exact length and starts again at its first frame, so the two give
//! the same samples, and a loop has no gap.
//!
//! **The exact length.** A file from Duck Echo says how many frames it
//! has in a Vorbis comment, `DUCKECHO_FRAMES=1008000`. The decoder
//! (`lewton`) can give a few hundred frames too many at the end of a short
//! file, so the music is cut to that. A file without the comment is cut
//! to the position its last Ogg page gives, which is what the Ogg format
//! says the length is.
//!
//! Music is at 44,100 frames a second (Duck Echo's rate), mono or stereo.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::error::LoadError;
use crate::sound::{Audio, SAMPLE_RATE};
use crate::sound_files::{LoadProblem, SoundSlot};

/// The Vorbis comment Duck Echo writes with the music's length in frames.
pub(crate) const FRAMES_TAG: &str = "DUCKECHO_FRAMES";

/// A piece of music from an `.ogg` file, ready to play with
/// [`Context::play_loop`](crate::engine::Context::play_loop).
///
/// Loading reads the file and checks it, but doesn't decode the music:
/// that happens while it plays. Cloning a `Music` is cheap.
///
/// When the file changes while the game runs (sent again from Duck Echo),
/// a loop playing it switches to the new music when it next starts again
/// from the beginning, as a `Wav` loop does.
#[derive(Clone)]
pub struct Music {
    name: String,
    path: PathBuf,
    slot: Arc<SoundSlot>,
}

impl Music {
    /// Loads an `.ogg` file of OGG Vorbis music, at 44,100 frames a
    /// second, mono or stereo. Its name in logs is the file name without
    /// `.ogg`.
    ///
    /// If the file is missing, isn't OGG Vorbis, isn't whole, or is at
    /// another rate, the error says so in one sentence.
    pub fn load(path: impl AsRef<Path>) -> Result<Music, LoadError> {
        let path = path.as_ref();
        let is_ogg = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ogg"));
        if !is_ogg {
            return Err(LoadError::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "{} isn't an .ogg file; Music::load plays OGG Vorbis music (a .wav loads \
                     with Wav::load, and loops with ctx.play_loop too)",
                    path.display()
                ),
            ));
        }
        let slot = crate::sound_files::load(path).map_err(|problem| match problem {
            LoadProblem::Unreadable(error) => {
                crate::sprite::missing_file_error(path, "music", &error)
            }
            LoadProblem::NotWav(problem) => LoadError::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "couldn't play {} as music ({problem}); save it again as OGG Vorbis at \
                     44100 Hz (Duck Echo's `render -o music.ogg` does)",
                    path.display()
                ),
            ),
        })?;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| String::from("music"));
        Ok(Music {
            name,
            path: path.to_path_buf(),
            slot,
        })
    }

    /// The name shown in logs and used by play tests: the file name
    /// without `.ogg`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// How long one time through the music lasts, in seconds.
    pub fn seconds(&self) -> f32 {
        self.ogg()
            .map_or(0.0, |ogg| ogg.frames as f32 / SAMPLE_RATE as f32)
    }

    /// Whether the music is stereo (a left and a right channel).
    pub fn is_stereo(&self) -> bool {
        self.ogg().is_some_and(|ogg| ogg.channels == 2)
    }

    /// The file's path, as it was loaded.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The music as the engine has it now (the newest version, if the
    /// file changed).
    pub(crate) fn ogg(&self) -> Option<Arc<OggData>> {
        match self.slot.audio() {
            Audio::Ogg(ogg) => Some(ogg),
            Audio::Samples(_) => None,
        }
    }
}

impl std::fmt::Debug for Music {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Music({:?}, {:.2} s)", self.path, self.seconds())
    }
}

/// An OGG Vorbis file in memory: its bytes, and what its headers say.
pub(crate) struct OggData {
    bytes: Arc<[u8]>,
    /// How many frames one time through has: where a loop wraps.
    pub(crate) frames: u64,
    /// 1 (mono) or 2 (stereo).
    pub(crate) channels: u8,
}

impl std::fmt::Debug for OggData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OggData({} frames, {} channels)",
            self.frames, self.channels
        )
    }
}

impl OggData {
    /// Checks an OGG Vorbis file is whole and one the engine plays, and
    /// reads its headers (not its music). The error says what's wrong.
    pub(crate) fn from_bytes(bytes: Arc<[u8]>) -> Result<OggData, String> {
        let last_position = whole_pages(&bytes)?;
        let reader = lewton::inside_ogg::OggStreamReader::new(Cursor::new(bytes.clone()))
            .map_err(|error| format!("it isn't OGG Vorbis ({error})"))?;
        let rate = reader.ident_hdr.audio_sample_rate;
        if rate != SAMPLE_RATE {
            return Err(format!(
                "it's at {rate} frames a second, and music plays at {SAMPLE_RATE}"
            ));
        }
        let channels = reader.ident_hdr.audio_channels;
        if !(1..=2).contains(&channels) {
            return Err(format!(
                "it has {channels} channels; music is mono or stereo"
            ));
        }
        let tagged = reader
            .comment_hdr
            .comment_list
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(FRAMES_TAG))
            .and_then(|(_, value)| value.trim().parse::<u64>().ok());
        Ok(OggData {
            bytes,
            frames: tagged.unwrap_or(last_position),
            channels,
        })
    }

    /// The file's bytes.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Walks an Ogg file's pages, checking each one's checksum, and that the
/// last one says the stream ends there. Returns the last page's position
/// (the length of the music, in frames). A file still being written fails
/// here, without decoding any music.
fn whole_pages(bytes: &[u8]) -> Result<u64, String> {
    let mut at = 0;
    let mut last = None;
    while at < bytes.len() {
        let page = &bytes[at..];
        if page.len() < 27 || &page[..4] != b"OggS" {
            return Err(String::from(
                "a part of it isn't an Ogg page; still being written?",
            ));
        }
        let segments = page[26] as usize;
        let header = 27 + segments;
        if page.len() < header {
            return Err(String::from(
                "it ends in the middle of a page; still being written?",
            ));
        }
        let body: usize = page[27..header].iter().map(|&s| s as usize).sum();
        let size = header + body;
        if page.len() < size {
            return Err(String::from(
                "it ends in the middle of a page; still being written?",
            ));
        }
        let stored = u32::from_le_bytes([page[22], page[23], page[24], page[25]]);
        if ogg_crc(&page[..size]) != stored {
            return Err(String::from(
                "a page's checksum is wrong; the file is damaged or still being written",
            ));
        }
        let position = u64::from_le_bytes(page[6..14].try_into().expect("8 bytes"));
        let ends = page[5] & 4 != 0;
        last = Some((position, ends));
        at += size;
    }
    match last {
        Some((position, true)) => Ok(position),
        Some((_, false)) => Err(String::from(
            "its last page doesn't end the stream; still being written?",
        )),
        None => Err(String::from("it's empty")),
    }
}

/// The checksum of an Ogg page (CRC-32 with the polynomial 0x04C11DB7,
/// as the Ogg format says), worked out with the checksum's own four bytes
/// as zeros.
fn ogg_crc(page: &[u8]) -> u32 {
    let mut crc = 0u32;
    for (i, &byte) in page.iter().enumerate() {
        let byte = if (22..26).contains(&i) { 0 } else { byte };
        crc ^= (byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04C1_1DB7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// Plays a piece of music over and over: decodes it, cuts it to its
/// exact length, and starts again at its first frame with no gap. When
/// told of a newer version of the file, it plays that from its next wrap.
///
/// The window's decoder thread and the headless mixer both use this, so
/// both give exactly the same samples.
pub(crate) struct LoopReader {
    ogg: Arc<OggData>,
    /// A newer version, waiting for the wrap.
    next: Option<Arc<OggData>>,
    decoder: Option<lewton::inside_ogg::OggStreamReader<Cursor<Arc<[u8]>>>>,
    /// Frames decoded and not yet given: left and right side by side.
    waiting: Vec<f32>,
    /// How many of `waiting`'s samples have been given.
    taken: usize,
    /// Frames given since the music last started.
    given: u64,
    /// The music broke off before its length (a damaged file): silence
    /// to the end of this time through.
    silent: bool,
}

impl LoopReader {
    pub(crate) fn new(ogg: Arc<OggData>) -> LoopReader {
        LoopReader {
            ogg,
            next: None,
            decoder: None,
            waiting: Vec::new(),
            taken: 0,
            given: 0,
            silent: false,
        }
    }

    /// Play `ogg` from the next wrap on (a newer one replaces one
    /// already waiting). (A window's decoder thread does this; headless,
    /// each version gets its own reader, from the wrap it starts at.)
    #[cfg_attr(
        not(all(feature = "audio", any(feature = "window", feature = "studio"))),
        allow(dead_code)
    )]
    pub(crate) fn set_next(&mut self, ogg: Arc<OggData>) {
        self.next = Some(ogg);
    }

    /// Fills `out` with the next frames, left and right side by side (a
    /// mono file gives the same on both sides).
    pub(crate) fn fill(&mut self, out: &mut [f32]) {
        let end = out.len() / 2 * 2;
        let mut at = 0;
        while at < end {
            if self.given >= self.ogg.frames {
                self.wrap();
                if self.ogg.frames == 0 {
                    out[at..].fill(0.0); // no music at all
                    return;
                }
            }
            let room = (end - at) / 2;
            let rest_of_round = (self.ogg.frames - self.given) as usize;
            if self.silent {
                let n = room.min(rest_of_round);
                out[at..at + 2 * n].fill(0.0);
                at += 2 * n;
                self.given += n as u64;
                continue;
            }
            if self.taken >= self.waiting.len() {
                self.decode_more();
                continue;
            }
            let ready = (self.waiting.len() - self.taken) / 2;
            let n = room.min(rest_of_round).min(ready);
            out[at..at + 2 * n].copy_from_slice(&self.waiting[self.taken..self.taken + 2 * n]);
            at += 2 * n;
            self.taken += 2 * n;
            self.given += n as u64;
        }
    }

    /// Starts again from the music's first frame: with the newer version,
    /// if there is one.
    fn wrap(&mut self) {
        if let Some(next) = self.next.take() {
            self.ogg = next;
        }
        self.decoder = None;
        self.waiting.clear();
        self.taken = 0;
        self.given = 0;
        self.silent = false;
    }

    /// Decodes the next packet of music into `waiting`.
    fn decode_more(&mut self) {
        self.waiting.clear();
        self.taken = 0;
        if self.decoder.is_none() {
            let bytes = Cursor::new(self.ogg.bytes.clone());
            match lewton::inside_ogg::OggStreamReader::new(bytes) {
                Ok(decoder) => self.decoder = Some(decoder),
                Err(_) => {
                    self.silent = true;
                    return;
                }
            }
        }
        let decoder = self.decoder.as_mut().expect("made above");
        match decoder.read_dec_packet_generic::<lewton::samples::InterleavedSamples<f32>>() {
            Ok(Some(packet)) => {
                let channels = packet.channel_count.max(1);
                for frame in packet.samples.chunks_exact(channels) {
                    let left = frame[0];
                    let right = if channels >= 2 { frame[1] } else { left };
                    self.waiting.push(left);
                    self.waiting.push(right);
                }
            }
            // It ended (or broke) before its length: silence to the end.
            Ok(None) | Err(_) => self.silent = true,
        }
    }
}

/// How much sound a window's decoder thread keeps ready: half a second.
#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
pub(crate) const RING_FRAMES: usize = SAMPLE_RATE as usize / 2;

/// How many frames the decoder thread adds at a time.
#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
const CHUNK_FRAMES: usize = 1024;

/// How long the decoder thread rests when the ring is full.
#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
const REST: Duration = Duration::from_millis(5);

/// A ring buffer of samples for one writer thread and one reader thread,
/// where neither ever waits for the other: the writer adds only where
/// there's room, the reader takes only what's there.
///
/// Each sample is kept as the bits of an `f32` in an atomic number, and
/// the two counters say how many samples have been written and read in
/// all. Writing the samples, then the count with `Release`, and reading
/// the count with `Acquire`, then the samples, means the reader never sees
/// a sample before it's written. No locks, and no `unsafe` code.
#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
struct Ring {
    samples: Box<[AtomicU32]>,
    written: AtomicUsize,
    read: AtomicUsize,
}

#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
impl Ring {
    fn new(samples: usize) -> Ring {
        Ring {
            samples: (0..samples).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicUsize::new(0),
            read: AtomicUsize::new(0),
        }
    }

    /// Room for this many more samples (the writer's side).
    fn room(&self) -> usize {
        let written = self.written.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        self.samples.len() - (written - read)
    }

    /// Adds `data` (the writer's side; there must be room).
    fn push(&self, data: &[f32]) {
        let written = self.written.load(Ordering::Relaxed);
        debug_assert!(data.len() <= self.room());
        let size = self.samples.len();
        for (i, &sample) in data.iter().enumerate() {
            self.samples[(written + i) % size].store(sample.to_bits(), Ordering::Relaxed);
        }
        self.written.store(written + data.len(), Ordering::Release);
    }

    /// Takes up to `out.len()` samples (the reader's side). Returns how
    /// many it took.
    fn pop(&self, out: &mut [f32]) -> usize {
        let read = self.read.load(Ordering::Relaxed);
        let written = self.written.load(Ordering::Acquire);
        let n = (written - read).min(out.len());
        let size = self.samples.len();
        for (i, sample) in out[..n].iter_mut().enumerate() {
            *sample = f32::from_bits(self.samples[(read + i) % size].load(Ordering::Relaxed));
        }
        self.read.store(read + n, Ordering::Release);
        n
    }
}

/// What a stream's two threads share.
#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
struct Shared {
    ring: Ring,
    /// Set when the stream is dropped: the decoder thread ends.
    stop: AtomicBool,
    /// A newer version of the music, for the decoder's next wrap. Only the
    /// game's thread and the decoder thread touch this lock, never the
    /// sound thread.
    next: Mutex<Option<Arc<OggData>>>,
}

/// One piece of music playing in a window: a decoder thread keeps about
/// half a second of it ready in a ring buffer, and the sound thread reads
/// from that ([`read`](MusicStream::read)) without ever waiting.
pub(crate) struct MusicStream {
    shared: Arc<Shared>,
}

#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
impl MusicStream {
    /// Starts decoding `ogg`. The first part is decoded here, before the
    /// thread starts, so the music begins without a gap.
    pub(crate) fn start(ogg: Arc<OggData>) -> MusicStream {
        MusicStream::start_with_rest(ogg, Duration::ZERO)
    }

    /// [`start`](MusicStream::start), with the decoder thread resting
    /// `slowness` before each part it decodes (tests: a slow computer).
    pub(crate) fn start_with_rest(ogg: Arc<OggData>, slowness: Duration) -> MusicStream {
        let shared = Arc::new(Shared {
            ring: Ring::new(RING_FRAMES * 2),
            stop: AtomicBool::new(false),
            next: Mutex::new(None),
        });
        let mut reader = LoopReader::new(ogg);
        let mut chunk = vec![0.0f32; CHUNK_FRAMES * 2];
        // Half the ring before the music starts.
        while shared.ring.room() > RING_FRAMES {
            reader.fill(&mut chunk);
            shared.ring.push(&chunk);
        }
        let theirs = Arc::clone(&shared);
        let started = std::thread::Builder::new()
            .name(String::from("duckforge music"))
            .spawn(move || decode(&theirs, reader, chunk, slowness));
        if let Err(error) = started {
            // The music plays what's ready, then silence: say why once.
            eprintln!("duckforge: couldn't start a thread to play music ({error})");
        }
        MusicStream { shared }
    }

    /// Takes the next frames into `out` (left and right side by side),
    /// as many as are ready, and returns how many frames it took. Never
    /// waits.
    pub(crate) fn read(&self, out: &mut [f32]) -> usize {
        let whole_frames = out.len() / 2 * 2;
        self.shared.ring.pop(&mut out[..whole_frames]) / 2
    }

    /// Plays `ogg` (the file changed) from the music's next wrap.
    pub(crate) fn next_round(&self, ogg: Arc<OggData>) {
        *self.shared.next.lock().unwrap_or_else(|e| e.into_inner()) = Some(ogg);
    }
}

impl Drop for MusicStream {
    fn drop(&mut self) {
        // The decoder thread sees this within a few milliseconds and ends.
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// The decoder thread: keeps the ring as full as it can, a part at a
/// time, resting while it's full, until the stream is dropped.
#[cfg_attr(
    not(all(feature = "audio", any(feature = "window", feature = "studio"))),
    allow(dead_code)
)]
fn decode(shared: &Shared, mut reader: LoopReader, mut chunk: Vec<f32>, slowness: Duration) {
    while !shared.stop.load(Ordering::Relaxed) {
        if let Some(next) = shared.next.lock().unwrap_or_else(|e| e.into_inner()).take() {
            reader.set_next(next);
        }
        if shared.ring.room() < chunk.len() {
            std::thread::sleep(REST);
            continue;
        }
        if !slowness.is_zero() {
            std::thread::sleep(slowness);
        }
        reader.fill(&mut chunk);
        shared.ring.push(&chunk);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Config, Context, Game};
    use crate::headless::{HeadlessOptions, run_headless};
    use crate::simulation::Simulation;
    use crate::sound::LoopId;
    use crate::test_folder::TestFolder;
    use std::time::Instant;

    /// Duck Craft's day theme, from Duck Echo (`duck_craft_day.toml`,
    /// quality 4): 1,008,000 frames, stereo.
    const DAY: &str = "examples/duck_craft/assets/music_day.ogg";
    const NIGHT: &str = "examples/duck_craft/assets/music_night.ogg";

    /// A test's own copy of a file of the engine's.
    fn copy(folder: &TestFolder, from: &str, to: &str) -> PathBuf {
        let path = folder.join(to);
        let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(from)).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn ogg_of(path: &Path) -> Arc<OggData> {
        Arc::new(OggData::from_bytes(std::fs::read(path).unwrap().into()).unwrap())
    }

    /// The whole file decoded at once with lewton, as Duck Echo checks it:
    /// left and right side by side, cut to its length. Also how many
    /// frames lewton gave before the cut.
    fn decoded_whole(ogg: &OggData) -> (Vec<f32>, u64) {
        let mut reader =
            lewton::inside_ogg::OggStreamReader::new(Cursor::new(ogg.bytes().to_vec())).unwrap();
        let mut both = Vec::new();
        while let Some(packet) = reader
            .read_dec_packet_generic::<lewton::samples::InterleavedSamples<f32>>()
            .unwrap()
        {
            let channels = packet.channel_count;
            for frame in packet.samples.chunks_exact(channels) {
                both.push(frame[0]);
                both.push(if channels >= 2 { frame[1] } else { frame[0] });
            }
        }
        let given = both.len() as u64 / 2;
        both.truncate(2 * ogg.frames as usize);
        (both, given)
    }

    /// `frames` frames from a `LoopReader`, in parts of `part` frames.
    fn read_loop(ogg: Arc<OggData>, frames: usize, part: usize) -> Vec<f32> {
        let mut reader = LoopReader::new(ogg);
        let mut out = vec![0.0f32; 2 * frames];
        for piece in out.chunks_mut(2 * part) {
            reader.fill(piece);
        }
        out
    }

    /// A sample as `audio.wav` has it (16-bit).
    fn pcm(sample: f32) -> i16 {
        (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16
    }

    /// The left and right samples of a 16-bit stereo `audio.wav`.
    fn wav_samples(bytes: &[u8]) -> Vec<i16> {
        assert_eq!(&bytes[22..24], &2u16.to_le_bytes(), "stereo");
        bytes[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect()
    }

    fn config() -> Config {
        Config {
            width: 32,
            height: 8,
            ..Config::default()
        }
    }

    /// Plays a piece of music from frame 1 at full volume, turns it to
    /// half on `half_on` and stops it on `stop_on`.
    struct Theme {
        music: Music,
        id: Option<LoopId>,
        half_on: u64,
        stop_on: u64,
    }

    impl Theme {
        fn new(music: Music) -> Theme {
            Theme {
                music,
                id: None,
                half_on: u64::MAX,
                stop_on: u64::MAX,
            }
        }
    }

    impl Game for Theme {
        fn update(&mut self, ctx: &mut Context) {
            let frame = ctx.frame();
            if frame == 1 {
                self.id = Some(ctx.play_loop(&self.music, 1.0));
            }
            if frame == self.half_on {
                ctx.set_loop_volume(self.id.unwrap(), 0.5);
            }
            if frame == self.stop_on {
                ctx.stop_loop(self.id.unwrap());
            }
        }
        fn draw(&self, _: &mut crate::canvas::Canvas) {}
    }

    /// The SHA-256 of the 16-bit `audio.wav` of three times through the
    /// day theme, as Linux makes it. Windows must make the same: lewton
    /// works out some tables with the system's `sin` and `cos`, which
    /// could differ in their last digit between systems.
    const THREE_LOOPS_SHA256: &str =
        "sha256:be959ef6ba5f09132b50f85901a1704f4da78dc97d44c7ffe6333788787ac8af";

    #[test]
    fn the_day_theme_loops_three_times_in_audio_wav_every_sample_the_file_repeated() {
        let folder = TestFolder::new("music-three-loops");
        let day = Music::load(copy(&folder, DAY, "day.ogg")).unwrap();
        let ogg = day.ogg().unwrap();
        assert_eq!((ogg.frames, ogg.channels), (1_008_000, 2));
        assert!((day.seconds() - 22.857_143).abs() < 1e-4);
        let (once, _) = decoded_whole(&ogg);

        // Three times through is 3,024,000 frames: 4,115 updates of 735.
        let out = folder.join("out");
        let options = HeadlessOptions {
            frames: 4115,
            out: Some(out.clone()),
            ..HeadlessOptions::default()
        };
        run_headless(config(), Theme::new(day), &options).unwrap();
        let bytes = std::fs::read(out.join("audio.wav")).unwrap();
        let samples = wav_samples(&bytes);
        assert!(samples.len() >= 2 * 3_024_000);
        for round in 0..3 {
            let start = round * once.len();
            let wrong = (0..once.len()).find(|&i| samples[start + i] != pcm(once[i]));
            assert_eq!(
                wrong,
                None,
                "time {} through: sample {wrong:?} differs",
                round + 1
            );
        }
        let log = std::fs::read_to_string(out.join("log.json")).unwrap();
        assert!(log.contains(r#"{"loop": "day", "file": "#), "{log}");

        let hash = crate::sound_files::hash_of(&bytes);
        eprintln!("three times through the day theme: audio.wav is {hash}");
        assert_eq!(
            hash, THREE_LOOPS_SHA256,
            "audio.wav differs from the one Linux makes: lewton's tables need Duck Forge's \
             own fixed math (docs/decisions.md)"
        );
    }

    #[test]
    fn a_windows_decoder_thread_gives_the_same_samples_as_the_headless_mixer() {
        let folder = TestFolder::new("music-two-paths");
        let ogg = ogg_of(&copy(&folder, DAY, "day.ogg"));
        let frames = 3 * ogg.frames as usize;
        let (once, _) = decoded_whole(&ogg);

        // Headless: the mixer's reader, in the mixer's parts.
        let headless = read_loop(ogg.clone(), frames, 4096);
        // The file decoded at once, three times over, is the same...
        let repeated: Vec<f32> = once.iter().chain(&once).chain(&once).copied().collect();
        assert!(
            bits(&headless) == bits(&repeated),
            "the reader is the file, repeated"
        );

        // ...and so is what the sound thread takes from a decoder thread,
        // in its own block sizes, as it's ready.
        let stream = MusicStream::start(ogg);
        let mut window = Vec::with_capacity(2 * frames);
        let mut block = vec![0.0f32; 2 * 1024];
        while window.len() < 2 * frames {
            let want = (2 * frames - window.len()).min(block.len());
            let got = stream.read(&mut block[..want]);
            window.extend_from_slice(&block[..2 * got]);
            if got == 0 {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        assert!(
            bits(&window) == bits(&headless),
            "every sample the same, both wraps too"
        );
    }

    /// Samples as their exact bits, to compare with no rounding.
    fn bits(samples: &[f32]) -> Vec<u32> {
        samples.iter().map(|s| s.to_bits()).collect()
    }

    #[test]
    fn volume_changes_and_stops_land_on_their_frames() {
        let folder = TestFolder::new("music-volumes");
        let day = Music::load(copy(&folder, DAY, "day.ogg")).unwrap();
        let (once, _) = decoded_whole(&day.ogg().unwrap());
        let out = folder.join("out");
        let game = Theme {
            half_on: 30,
            stop_on: 60,
            ..Theme::new(day)
        };
        let options = HeadlessOptions {
            frames: 90,
            out: Some(out.clone()),
            ..HeadlessOptions::default()
        };
        run_headless(config(), game, &options).unwrap();
        let samples = wav_samples(&std::fs::read(out.join("audio.wav")).unwrap());
        // Update 30 begins at sample 29 x 735 = 21,315; update 60 at 43,365.
        let (half, stop) = (2 * 21_315, 2 * 43_365);
        for (i, &got) in samples.iter().enumerate() {
            let want = if i < half {
                pcm(once[i])
            } else if i < stop {
                pcm(once[i] * 0.5)
            } else {
                0
            };
            assert_eq!(got, want, "sample {} ({} of a frame)", i / 2, i % 2);
        }
        assert_eq!(samples.len(), 2 * 89 * 735 + 2 * 735);
    }

    #[test]
    fn music_is_cut_to_its_exact_length_and_joins_with_no_gap() {
        let folder = TestFolder::new("music-length");
        // Duck Echo's chime, 0.84 s: in a file this short lewton gives
        // 557 frames too many at the end.
        let chime = ogg_of(&folder.fixture("music").join("chime.ogg"));
        assert_eq!(chime.frames, 37_139, "from its DUCKECHO_FRAMES comment");
        let (once, given) = decoded_whole(&chime);
        assert_eq!(given, 37_696, "what lewton gives");
        let twice = read_loop(chime.clone(), 2 * 37_139 + 10, 1000);
        assert!(bits(&twice[..once.len()]) == bits(&once));
        assert!(
            bits(&twice[once.len()..2 * once.len()]) == bits(&once),
            "the second time starts straight after the last frame"
        );
        // The day theme's last Ogg page says the same length as its
        // comment: what a file without the comment is cut to.
        let day = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(DAY)).unwrap();
        assert_eq!(whole_pages(&day), Ok(1_008_000));
    }

    #[test]
    fn the_sound_thread_never_waits_for_the_decoder() {
        let folder = TestFolder::new("music-never-waits");
        let ogg = ogg_of(&copy(&folder, DAY, "day.ogg"));

        // A decoder thread that's stuck (resting a minute before each
        // part): the first quarter second was decoded before it started,
        // and after that every read comes back at once, with nothing.
        let stuck = MusicStream::start_with_rest(ogg.clone(), Duration::from_secs(60));
        let mut block = vec![0.0f32; 2 * 1024];
        let mut ready = 0;
        loop {
            let got = stuck.read(&mut block);
            if got == 0 {
                break;
            }
            ready += got;
        }
        assert!(ready >= RING_FRAMES / 2, "{ready} frames were ready");
        let started = Instant::now();
        for _ in 0..10_000 {
            assert_eq!(stuck.read(&mut block), 0);
        }
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "10,000 reads took {:?}",
            started.elapsed()
        );

        // A slow decoder (resting before each part): the reader takes
        // what's there, often nothing, and the music still comes out
        // right, every sample, just late.
        let slow = MusicStream::start_with_rest(ogg.clone(), Duration::from_millis(2));
        let frames = 200_000;
        let (mut taken, mut empty) = (Vec::with_capacity(2 * frames), 0);
        while taken.len() < 2 * frames {
            let want = (2 * frames - taken.len()).min(block.len());
            let got = slow.read(&mut block[..want]);
            taken.extend_from_slice(&block[..2 * got]);
            if got == 0 {
                empty += 1;
                std::thread::sleep(Duration::from_micros(200));
            }
        }
        assert!(empty > 0, "the reader was faster than the decoder");
        assert!(bits(&taken) == bits(&read_loop(ogg, frames, 4096)));
    }

    #[test]
    fn ten_minutes_of_music_through_a_decoder_thread() {
        // A long run of the window's path, as fast as the decoder goes:
        // the reader takes a block whenever one is ready, and every one of
        // 26,460,000 frames (26 times through) is the right sample.
        let folder = TestFolder::new("music-soak");
        let ogg = ogg_of(&copy(&folder, NIGHT, "night.ogg"));
        let frames: usize = 10 * 60 * SAMPLE_RATE as usize;
        let stream = MusicStream::start(ogg.clone());
        let mut reference = LoopReader::new(ogg);
        let (mut block, mut expected) = (vec![0.0f32; 2 * 1024], vec![0.0f32; 2 * 1024]);
        let (mut done, mut longest) = (0, Duration::ZERO);
        while done < frames {
            let want = (frames - done).min(1024);
            let asked = Instant::now();
            let got = stream.read(&mut block[..2 * want]);
            longest = longest.max(asked.elapsed());
            if got == 0 {
                std::thread::yield_now();
                continue;
            }
            reference.fill(&mut expected[..2 * got]);
            assert!(
                bits(&block[..2 * got]) == bits(&expected[..2 * got]),
                "frames from {done} differ"
            );
            done += got;
        }
        // A read only copies what's there: it never waits on a lock or a
        // file (this is generous, for a busy test machine).
        assert!(
            longest < Duration::from_millis(100),
            "the longest read took {longest:?}"
        );
    }

    #[test]
    fn a_changed_ogg_plays_from_its_next_wrap_and_replays_the_same() {
        let folder = TestFolder::new("music-change");
        crate::sound_files::use_store(&folder.join("store"));
        let path = copy(&folder, DAY, "theme.ogg");
        let name = crate::files::name_of(&path);
        let (day, night) = (
            ogg_of(&path),
            ogg_of(&Path::new(env!("CARGO_MANIFEST_DIR")).join(NIGHT)),
        );
        let (day_once, _) = decoded_whole(&day);
        let (night_once, _) = decoded_whole(&night);

        let mut live = Simulation::new(config(), Theme::new(Music::load(&path).unwrap()));
        live.run(39);
        // Sent again from Duck Echo (saved whole, then renamed into place).
        let partial = folder.join("theme.partial");
        std::fs::write(&partial, night.bytes()).unwrap();
        std::fs::rename(&partial, &path).unwrap();
        assert_eq!(live.change_sound_file(&name), Ok(Some(40)));
        live.run(1400 - 39);

        // Update 40 begins at sample 28,665, in the first time through:
        // the night theme starts at the wrap, 1,008,000.
        let starts: Vec<(u64, u64)> = live.loops()[0]
            .versions
            .iter()
            .map(|(at, a)| (*at, a.frames()))
            .collect();
        assert_eq!(starts, [(0, 1_008_000), (1_008_000, 1_008_000)]);
        let length = live.sample_at(1401);
        let audio = crate::sound::mix_to_wav(live.sounds(), live.loops(), length).bytes;
        let samples = wav_samples(&audio);
        let wrap = 2 * 1_008_000;
        assert!(
            (0..wrap).all(|i| samples[i] == pcm(day_once[i])),
            "the day to its end"
        );
        assert!(
            (wrap..samples.len()).all(|i| samples[i] == pcm(night_once[i - wrap])),
            "then the night from its start"
        );

        // Recorded, the file changed again, replayed from the store.
        let recording = live.recording();
        assert_eq!(live.save_sounds(&recording), Ok(2));
        drop(live);
        std::fs::write(&path, b"not music any more").unwrap();
        std::fs::write(&path, day.bytes()).unwrap();
        let out = folder.join("out");
        let replay = run_headless(
            config(),
            Theme::new(Music::load(&path).unwrap()),
            &HeadlessOptions {
                frames: 1400,
                out: Some(out.clone()),
                inputs: Some(recording),
                ..HeadlessOptions::default()
            },
        )
        .unwrap();
        assert!(
            replay.replay_warnings().is_empty(),
            "{:?}",
            replay.replay_warnings()
        );
        assert!(
            std::fs::read(out.join("audio.wav")).unwrap() == audio,
            "the same audio.wav"
        );
    }

    #[test]
    fn music_that_cant_be_played_says_why() {
        let folder = TestFolder::new("music-problems");
        let say = |path: &Path| Music::load(path).unwrap_err().to_string();
        let wav = folder.file("step.wav", "RIFF");
        assert!(say(&wav).contains("Wav::load"), "{}", say(&wav));
        let missing = folder.join("gone.ogg");
        assert!(say(&missing).contains("gone.ogg"), "{}", say(&missing));
        // Half written: it stops in the middle of a page.
        let day = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join(DAY)).unwrap();
        let half = folder.join("half.ogg");
        std::fs::write(&half, &day[..day.len() / 2]).unwrap();
        assert!(say(&half).contains("still being written"), "{}", say(&half));
        // Damaged: a byte changed in the middle.
        let mut damaged = day.clone();
        damaged[day.len() / 2] ^= 0x55;
        let bad = folder.join("bad.ogg");
        std::fs::write(&bad, &damaged).unwrap();
        assert!(say(&bad).contains("checksum"), "{}", say(&bad));
        let text = folder.file("notes.ogg", "hello");
        assert!(say(&text).contains("isn't an Ogg page"), "{}", say(&text));
    }
}
