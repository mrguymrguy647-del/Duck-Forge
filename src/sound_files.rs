//! The sound files a game plays (`.wav`), noted as it loads them, so they
//! can change while the game runs. The sound side of [`files`](crate::files).
//!
//! [`Wav::load`](crate::sound::Wav::load) loads through here. Each file
//! gets one entry in a table: its name (as `files.rs` names files, with
//! forward slashes), the samples the game hears now, and the SHA-256 of
//! the file those samples came from. A `Wav` holds a small shared *slot*
//! from the table, and reads its samples from it each time it's played.
//! So when the table swaps a file's samples, every copy of that `Wav`
//! plays the new sound from then on, and no game has to change.
//!
//! A file's sound changes only in these ways, each kept in a recording on
//! the frame it happens, so a replay hears the same:
//!
//! - in a window, the engine looks at the files twice a second
//!   ([`changed_on_disk`]);
//! - Duck Forge Studio says a file changed (its `file` message);
//! - a replay gives the versions the recording had.
//!
//! Loading a file again while a game runs gives the version the run has
//! now, not what's on disk, so nothing slips in unrecorded.
//!
//! Every version taken is kept, with its file's bytes, until the run
//! ends. When a recording is saved, the versions it names are written to
//! the *sound store*, `out/sound-store/<sha256>.wav`, so a replay can hear
//! exactly those bytes later, even after the file has changed again.
//!
//! Like `files.rs`, what's noted belongs to the thread that loaded it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::sound::Samples;

/// Where saved recordings keep their sounds, from the folder the game
/// runs in. `out/` is left out of Git and of Duck Forge Studio's History.
pub(crate) const STORE_FOLDER: &str = "out/sound-store";

/// The shared place a [`Wav`](crate::sound::Wav) reads its samples from.
/// The table swaps them when the file changes.
#[derive(Debug)]
pub(crate) struct SoundSlot {
    samples: Mutex<Samples>,
}

impl SoundSlot {
    fn new(samples: Samples) -> Arc<SoundSlot> {
        Arc::new(SoundSlot {
            samples: Mutex::new(samples),
        })
    }

    /// The samples to play now. Cheap: they're shared, not copied.
    pub(crate) fn samples(&self) -> Samples {
        self.samples
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set(&self, samples: Samples) {
        *self.samples.lock().unwrap_or_else(|e| e.into_inner()) = samples;
    }
}

/// One version of a sound file: the file's bytes, their hash, and the
/// samples decoded from them.
#[derive(Clone)]
pub(crate) struct Version {
    /// `sha256:` and 64 hex digits, as a recording writes it.
    pub(crate) hash: String,
    bytes: Arc<[u8]>,
    pub(crate) samples: Samples,
}

impl Version {
    /// Decodes `bytes`. The error says what's wrong with the file.
    fn from_bytes(bytes: Vec<u8>) -> Result<Version, String> {
        let samples = crate::sound::decode_wav(&bytes)?;
        Ok(Version {
            hash: hash_of(&bytes),
            bytes: bytes.into(),
            samples,
        })
    }
}

/// What the table knows about one file.
struct Entry {
    /// Where to read it from.
    path: PathBuf,
    /// The slot its `Wav`s read from.
    slot: Arc<SoundSlot>,
    /// The hash of the version the game hears now.
    now: String,
    /// The hash of the first version it heard (or the one a replay
    /// started with): a recording's `"sounds"`.
    first: String,
    /// The file's size and modified time when last read, to notice a
    /// change cheaply.
    seen: Option<Seen>,
    /// The size and time of a version that didn't decode, so the problem
    /// is said once, not twice a second.
    said_problem: Option<Seen>,
}

/// A file's size and modified time.
type Seen = (u64, Option<SystemTime>);

#[derive(Default)]
struct SoundFiles {
    entries: BTreeMap<String, Entry>,
    /// Every version taken, by hash, for the sound store.
    versions: BTreeMap<String, Version>,
    /// A replay's versions, by name: the latest the recording had so far.
    given: BTreeMap<String, String>,
    /// Games running on this thread (a `Simulation` counts one).
    runs: u32,
    /// Problems for the replay's summary, waiting to be taken.
    warnings: Vec<String>,
    /// Files already warned about, so each is said once.
    warned: BTreeSet<String>,
    /// Where the sound store is, if not [`STORE_FOLDER`] (tests).
    store: Option<PathBuf>,
}

thread_local! {
    static TABLE: RefCell<SoundFiles> = RefCell::new(SoundFiles::default());
}

/// Why a file couldn't be loaded.
pub(crate) enum LoadProblem {
    /// It couldn't be read (missing, say).
    Unreadable(io::Error),
    /// It was read, but isn't a WAV duckforge can play: what's wrong.
    NotWav(String),
}

/// Is `name` a sound file the table looks after (a `.wav`)?
pub(crate) fn is_sound_file(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

/// Loads a sound file for a `Wav`, and notes it.
///
/// While a game runs, a file it has loaded before gives the version the
/// run has now. A file a replay gives a version for gets that version.
/// Otherwise it's read from disk.
pub(crate) fn load(path: &Path) -> Result<Arc<SoundSlot>, LoadProblem> {
    let name = crate::files::name_of(path);
    let (known, running, given) = TABLE.with(|table| {
        let table = table.borrow();
        (
            table.entries.get(&name).map(|e| e.slot.clone()),
            table.runs > 0,
            table.given.get(&name).cloned(),
        )
    });
    if let (Some(slot), true) = (&known, running) {
        return Ok(slot.clone());
    }
    let (version, seen) = match given {
        Some(hash) => (version_for(&name, path, &hash)?, seen_now(path)),
        None => read(path)?,
    };
    Ok(TABLE.with(|table| {
        let mut table = table.borrow_mut();
        let slot = match table.entries.get_mut(&name) {
            // Loaded again with no game running: the file as it is now,
            // for every copy of it.
            Some(entry) => {
                entry.slot.set(version.samples.clone());
                entry.now = version.hash.clone();
                entry.seen = seen;
                entry.slot.clone()
            }
            None => {
                let slot = SoundSlot::new(version.samples.clone());
                table.entries.insert(
                    name,
                    Entry {
                        path: path.to_path_buf(),
                        slot: slot.clone(),
                        now: version.hash.clone(),
                        first: version.hash.clone(),
                        seen,
                        said_problem: None,
                    },
                );
                slot
            }
        };
        table.versions.insert(version.hash.clone(), version);
        slot
    }))
}

/// Reads and decodes a file from disk, with its size and time.
fn read(path: &Path) -> Result<(Version, Option<Seen>), LoadProblem> {
    // The size and time before reading: if the file changes while it's
    // read, the next look sees it changed and reads it again.
    let seen = seen_now(path);
    let bytes = std::fs::read(path).map_err(LoadProblem::Unreadable)?;
    let version = Version::from_bytes(bytes).map_err(LoadProblem::NotWav)?;
    Ok((version, seen))
}

/// Like [`read`], for a file that changed while the game runs: it must
/// also be whole. The WAV reader plays what's there of a file that's
/// shorter than its header says, which is right for a file that was
/// saved oddly, but a file still being written would be swapped in cut
/// short.
fn read_whole(path: &Path) -> Result<(Version, Option<Seen>), LoadProblem> {
    let seen = seen_now(path);
    let bytes = std::fs::read(path).map_err(LoadProblem::Unreadable)?;
    is_whole(&bytes).map_err(LoadProblem::NotWav)?;
    let version = Version::from_bytes(bytes).map_err(LoadProblem::NotWav)?;
    Ok((version, seen))
}

/// Whether a WAV file's sound is all there: its `data` section is as long
/// as its header says. (A size of 0 or 0xFFFFFFFF means "not known", as
/// some programs write while recording: that's taken as whole.)
fn is_whole(bytes: &[u8]) -> Result<(), String> {
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]]);
        let body = at + 8;
        if id == b"data" {
            let unknown = size == 0 || size == u32::MAX;
            if !unknown && body + size as usize > bytes.len() {
                return Err(format!(
                    "it has {} of the {size} bytes of sound its header says; still being written?",
                    bytes.len() - body
                ));
            }
            return Ok(());
        }
        at = body
            .saturating_add(size as usize)
            .saturating_add(size as usize % 2);
    }
    Err(String::from(
        "it has no \"data\" section yet; still being written?",
    ))
}

/// A file's size and modified time now, if it can be looked at.
fn seen_now(path: &Path) -> Option<Seen> {
    let data = std::fs::metadata(path).ok()?;
    Some((data.len(), data.modified().ok()))
}

/// A file that changed on disk, decoded and ready to swap in.
pub(crate) struct Changed {
    pub(crate) name: String,
    pub(crate) version: Version,
}

/// Looks at every sound file loaded from disk: first only its size and
/// modified time (cheap), and when those changed, reads it, checks that
/// it decodes, and works out its hash. Returns the files whose sound is
/// now different, and what went wrong with any that didn't decode (each
/// said once). A file that doesn't decode yet (still being written) is
/// looked at again next time; the game keeps the sound it has.
#[cfg_attr(not(feature = "window"), allow(dead_code))] // the window and the Studio's link look
pub(crate) fn changed_on_disk() -> (Vec<Changed>, Vec<String>) {
    let looked_at: Vec<(String, PathBuf, Option<Seen>)> = TABLE.with(|table| {
        table
            .borrow()
            .entries
            .iter()
            .map(|(name, entry)| (name.clone(), entry.path.clone(), entry.seen))
            .collect()
    });
    let mut changed = Vec::new();
    let mut problems = Vec::new();
    for (name, path, seen) in looked_at {
        let now = seen_now(&path);
        if now.is_none() || now == seen {
            continue; // missing for a moment (being replaced), or the same
        }
        match look_at(&name, &path) {
            Ok(Some(version)) => changed.push(Changed { name, version }),
            Ok(None) => {}
            Err(problem) => problems.extend(problem),
        }
    }
    (changed, problems)
}

/// Reads `name` from disk now (Duck Forge Studio said it changed).
/// `Ok(Some)` if its sound is different, `Ok(None)` if it's the same or
/// not a file the game has loaded, `Err` if it doesn't decode yet (the
/// game keeps the sound it has; it's looked at again later).
pub(crate) fn read_now(name: &str) -> Result<Option<Changed>, String> {
    let path = TABLE.with(|table| table.borrow().entries.get(name).map(|e| e.path.clone()));
    let Some(path) = path else {
        return Ok(None); // not one the game plays
    };
    match look_at(name, &path) {
        Ok(version) => Ok(version.map(|version| Changed {
            name: name.to_string(),
            version,
        })),
        Err(problem) => Err(problem.unwrap_or_else(|| format!("{name} can't be read yet"))),
    }
}

/// Reads one file and compares it with what the game hears. The error is
/// the problem to say, or `None` if it was said before.
fn look_at(name: &str, path: &Path) -> Result<Option<Version>, Option<String>> {
    match read_whole(path) {
        Ok((version, seen)) => Ok(TABLE.with(|table| {
            let mut table = table.borrow_mut();
            let entry = table.entries.get_mut(name)?;
            entry.seen = seen;
            entry.said_problem = None;
            (entry.now != version.hash).then_some(version)
        })),
        Err(problem) => {
            let why = match problem {
                LoadProblem::Unreadable(error) => error.to_string(),
                LoadProblem::NotWav(why) => why,
            };
            // Not noted as seen, so it's read again next time. Said once
            // for each size and time it has.
            let seen = seen_now(path);
            let first_time = TABLE.with(|table| {
                let mut table = table.borrow_mut();
                let entry = table.entries.get_mut(name)?;
                (entry.said_problem != seen).then(|| entry.said_problem = seen)
            });
            Err(first_time.map(|()| {
                format!("{name} changed, but can't be read yet ({why}); keeping the old sound")
            }))
        }
    }
}

/// From now on the game hears `version` for `name`: every `Wav` of it
/// plays the new samples. A one-shot already playing keeps the old ones.
pub(crate) fn swap(name: &str, version: Version) {
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if let Some(entry) = table.entries.get_mut(name) {
            entry.slot.set(version.samples.clone());
            entry.now = version.hash.clone();
        }
        if table.given.contains_key(name) {
            table.given.insert(name.to_string(), version.hash.clone());
        }
        table.versions.insert(version.hash.clone(), version);
    });
}

/// A recording's version of `name`: from what this run has already
/// taken, else the sound store, else the file on disk if it's the same
/// version. Failing those, the file on disk as it is now, with a warning:
/// the game plays out the same, but `audio.wav` won't.
pub(crate) fn version_for(name: &str, path: &Path, hash: &str) -> Result<Version, LoadProblem> {
    if let Some(version) = TABLE.with(|table| table.borrow().versions.get(hash).cloned()) {
        return Ok(version);
    }
    let stored = store_path(hash).and_then(|file| std::fs::read(file).ok());
    if let Some(bytes) = stored
        && hash_of(&bytes) == hash
        && let Ok(version) = Version::from_bytes(bytes)
    {
        return Ok(version);
    }
    let (version, _) = read(path)?;
    if version.hash != hash {
        warn_once(
            name,
            format!(
                "{name} differs from the recording and the sound store doesn't have the \
                 recorded one; the game plays out the same, audio.wav will not"
            ),
        );
    }
    Ok(version)
}

/// A replay starts: from now on `name` gives the recorded version
/// `hash`, and counts as the game's first version of it. Returns the
/// version for a file already loaded, to swap in now (if it differs).
pub(crate) fn start_with(sounds: &BTreeMap<String, String>) -> Vec<Changed> {
    let mut swaps = Vec::new();
    for (name, hash) in sounds {
        let loaded = TABLE.with(|table| {
            let mut table = table.borrow_mut();
            table.given.insert(name.clone(), hash.clone());
            let entry = table.entries.get_mut(name)?;
            entry.first = hash.clone();
            (entry.now != *hash).then(|| entry.path.clone())
        });
        if let Some(path) = loaded {
            match version_for(name, &path, hash) {
                Ok(version) => swaps.push(Changed {
                    name: name.clone(),
                    version,
                }),
                Err(_) => warn_once(
                    name,
                    format!("{name} can't be read, so it keeps the sound it has"),
                ),
            }
        }
    }
    swaps
}

/// A recorded change during a replay: the version to swap in for `name`,
/// or `None` (with a warning) if neither the store nor the disk has it.
pub(crate) fn recorded_change(name: &str, hash: &str) -> Option<Version> {
    let path = TABLE.with(|table| {
        let mut table = table.borrow_mut();
        table.given.insert(name.to_string(), hash.to_string());
        table.entries.get(name).map(|e| e.path.clone())
    })?;
    match version_for(name, &path, hash) {
        Ok(version) => Some(version),
        Err(_) => {
            warn_once(
                name,
                format!("{name} can't be read, so it keeps the sound it has"),
            );
            None
        }
    }
}

fn warn_once(name: &str, warning: String) {
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        if table.warned.insert(name.to_string()) {
            table.warnings.push(warning);
        }
    });
}

/// Problems the replay ran into, since last time.
pub(crate) fn take_warnings() -> Vec<String> {
    TABLE.with(|table| std::mem::take(&mut table.borrow_mut().warnings))
}

/// Every sound file loaded, with the hash of its first version (or the
/// version a replay started with): a recording's `"sounds"`.
pub(crate) fn first_hashes() -> BTreeMap<String, String> {
    TABLE.with(|table| {
        table
            .borrow()
            .entries
            .iter()
            .map(|(name, entry)| (name.clone(), entry.first.clone()))
            .collect()
    })
}

/// Writes the versions named in `hashes` to the sound store, if they
/// aren't there yet. Returns how many were written. Each is written to a
/// temporary file and renamed into place, so a store file is always
/// whole.
pub(crate) fn save_to_store<'a>(
    hashes: impl IntoIterator<Item = &'a String>,
) -> Result<usize, String> {
    let mut written = 0;
    for hash in hashes {
        let Some(path) = store_path(hash) else {
            continue; // not a hash this engine writes
        };
        if path.exists() {
            continue;
        }
        let Some(version) = TABLE.with(|table| table.borrow().versions.get(hash).cloned()) else {
            continue; // not heard in this run: nothing to keep
        };
        let folder = path.parent().unwrap_or(Path::new("."));
        let cant = |error: io::Error| format!("couldn't save {}: {error}", path.display());
        std::fs::create_dir_all(folder).map_err(cant)?;
        let partial = path.with_extension(format!(
            "wav.{}-{:?}.partial",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&partial, &version.bytes).map_err(cant)?;
        if let Err(error) = std::fs::rename(&partial, &path) {
            let _ = std::fs::remove_file(&partial);
            // Another program put the same version there at the same moment.
            if !path.exists() {
                return Err(cant(error));
            }
        }
        written += 1;
    }
    Ok(written)
}

/// Where the store keeps the version `hash` (`sha256:` and 64 hex digits).
fn store_path(hash: &str) -> Option<PathBuf> {
    let hex = hash.strip_prefix("sha256:")?;
    let plain = hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit());
    if !plain {
        return None; // never a path made from odd text in a recording
    }
    let folder = TABLE
        .with(|table| table.borrow().store.clone())
        .unwrap_or_else(|| PathBuf::from(STORE_FOLDER));
    Some(folder.join(format!("{}.wav", hex.to_ascii_lowercase())))
}

/// Uses `folder` as the sound store on this thread (tests).
#[cfg(test)]
pub(crate) fn use_store(folder: &Path) {
    TABLE.with(|table| table.borrow_mut().store = Some(folder.to_path_buf()));
}

/// A game starts running on this thread.
pub(crate) fn run_started() {
    TABLE.with(|table| table.borrow_mut().runs += 1);
}

/// A game on this thread has ended: its replay's versions are forgotten,
/// and so is every file no `Wav` holds any more, so the next game reads
/// the disk again.
pub(crate) fn run_ended() {
    TABLE.with(|table| {
        let mut table = table.borrow_mut();
        table.runs = table.runs.saturating_sub(1);
        if table.runs == 0 {
            table.given.clear();
            table.warned.clear();
            table
                .entries
                .retain(|_, entry| Arc::strong_count(&entry.slot) > 1);
            let kept: BTreeSet<String> = table.entries.values().map(|e| e.now.clone()).collect();
            table.versions.retain(|hash, _| kept.contains(hash));
        }
    });
}

/// `sha256:` and the SHA-256 of `bytes` in hex, as recordings write it.
pub(crate) fn hash_of(bytes: &[u8]) -> String {
    let digest = sha256(bytes);
    let mut text = String::with_capacity(7 + 64);
    text.push_str("sha256:");
    for byte in digest {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// The SHA-256 of `bytes` (FIPS 180-4), written out here rather than
/// adding a crate to the headless build (see docs/decisions.md). It only
/// names versions of a file; nothing secret depends on it.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    // The message, then a 1 bit, then zeros, then its length in bits, to
    // a whole number of 64-byte blocks.
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&((bytes.len() as u64).wrapping_mul(8)).to_be_bytes());

    let mut w = [0u32; 64];
    for block in message.chunks_exact(64) {
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (total, add) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *total = total.wrapping_add(add);
        }
    }
    let mut digest = [0u8; 32];
    for (bytes, word) in digest.chunks_exact_mut(4).zip(h) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_standard_examples() {
        assert_eq!(
            hash_of(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hash_of(b"abc"),
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // Two blocks.
        assert_eq!(
            hash_of(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "sha256:248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
        // A million 'a's: many blocks.
        assert_eq!(
            hash_of(&vec![b'a'; 1_000_000]),
            "sha256:cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    use crate::engine::{Config, Context, Game};
    use crate::headless::{HeadlessOptions, run_headless};
    use crate::script::Change;
    use crate::simulation::Simulation;
    use crate::sound::{LoopId, Wav, mix_to_wav, wav_bytes};
    use crate::test_folder::TestFolder;

    /// A 16-bit mono WAV of `count` samples, each `value(i)`: a ramp makes
    /// every place in the sound different, so a jump would show.
    fn wav(count: usize, value: impl Fn(usize) -> i16) -> Vec<u8> {
        wav_bytes((0..count).map(value), count)
    }

    /// Saves a file whole, the way Duck Echo does: written beside it, then
    /// renamed into place.
    fn save(path: &Path, bytes: &[u8]) {
        let partial = path.with_extension("partial");
        std::fs::write(&partial, bytes).unwrap();
        std::fs::rename(&partial, path).unwrap();
    }

    /// A step sound every 10 frames, and a theme looping from frame 1.
    struct Steps {
        step: Wav,
        theme: Wav,
        music: Option<LoopId>,
    }

    impl Steps {
        fn load(folder: &Path) -> Steps {
            Steps {
                step: Wav::load(folder.join("step.wav")).unwrap(),
                theme: Wav::load(folder.join("theme.wav")).unwrap(),
                music: None,
            }
        }
    }

    impl Game for Steps {
        fn update(&mut self, ctx: &mut Context) {
            if ctx.frame() == 1 {
                self.music = Some(ctx.play_loop(&self.theme, 0.5));
            }
            if ctx.frame().is_multiple_of(10) {
                ctx.play_wav(&self.step);
                // What a game sees: the new length from the change on.
                ctx.report("step_seconds", self.step.seconds());
            }
        }
        fn draw(&self, _: &mut crate::canvas::Canvas) {}
    }

    fn config() -> Config {
        Config {
            width: 32,
            height: 8,
            ..Config::default()
        }
    }

    #[test]
    fn a_sound_changed_on_frame_40_replays_to_the_same_audio_wav() {
        let folder = TestFolder::new("sound-change-replay");
        use_store(&folder.join("store"));
        let (step, theme) = (folder.join("step.wav"), folder.join("theme.wav"));
        let (step_name, theme_name) = (crate::files::name_of(&step), crate::files::name_of(&theme));
        save(&step, &wav(300, |i| (i * 50) as i16));
        save(&theme, &wav(1000, |i| (i * 16) as i16));

        // Played live: both files change between updates 39 and 40.
        let mut live = Simulation::new(config(), Steps::load(&folder));
        live.run(39);
        save(&step, &wav(500, |i| -((i * 30) as i16)));
        save(&theme, &wav(700, |i| 9000 - (i * 10) as i16));
        assert_eq!(live.change_sound_file(&step_name), Ok(Some(40)));
        // Duck Forge Studio's `file` message comes the same way.
        assert!(live.change_file(&theme_name, ""));
        assert_eq!(
            live.change_sound_file(&theme_name),
            Ok(None),
            "already taken"
        );
        live.run(41);

        // New plays use the new sound; the one played on frame 30 kept
        // the old one.
        let steps: Vec<(u64, usize)> = live
            .sounds()
            .iter()
            .map(|s| (s.frame, s.samples.frames()))
            .collect();
        assert_eq!(
            &steps[..5],
            [(10, 300), (20, 300), (30, 300), (40, 500), (50, 500)]
        );
        let live_audio = mix_to_wav(live.sounds(), live.loops(), live.sample_at(81)).bytes;

        // The recording: the first versions, and both changes on frame 40.
        let recording = live.recording();
        let first = |path: &Path| hash_of(&std::fs::read(path).unwrap());
        assert_eq!(recording.sounds.len(), 2);
        let changes: Vec<(u64, String)> = recording
            .changes
            .iter()
            .filter_map(|c| match &c.change {
                Change::SoundFile { path, sha256 } => {
                    assert_eq!(*sha256, first(Path::new(path)), "the new file's hash");
                    Some((c.frame, path.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(changes, [(40, step_name.clone()), (40, theme_name.clone())]);
        let script = crate::script::InputScript::parse(&recording.to_json()).unwrap();
        assert_eq!(script, recording, "it reads back the same");
        assert_eq!(live.save_sounds(&recording), Ok(4), "two versions of each");
        assert_eq!(live.save_sounds(&recording), Ok(0), "already in the store");
        drop(live);

        // The files change again: the replay must use the store.
        save(&step, &wav(100, |_| 1234));
        save(&theme, &wav(100, |_| -1234));
        let out = folder.join("out");
        let replay = run_headless(
            config(),
            Steps::load(&folder),
            &HeadlessOptions {
                frames: 80,
                out: Some(out.clone()),
                inputs: Some(script.clone()),
                ..HeadlessOptions::default()
            },
        )
        .unwrap();
        assert!(
            replay.replay_warnings().is_empty(),
            "{:?}",
            replay.replay_warnings()
        );
        let replayed = std::fs::read(out.join("audio.wav")).unwrap();
        assert!(
            replayed == live_audio,
            "audio.wav is the same, byte for byte"
        );
        assert_eq!(
            replay.recording().changes,
            recording.changes,
            "and its own recording keeps the changes"
        );
        drop(replay);

        // Without the store, and with today's files: a plain warning, and
        // the game still plays out the same (it reports the same).
        std::fs::remove_dir_all(folder.join("store")).unwrap();
        let without = run_headless(
            config(),
            Steps::load(&folder),
            &HeadlessOptions {
                frames: 80,
                inputs: Some(script),
                ..HeadlessOptions::default()
            },
        )
        .unwrap();
        let warnings = without.replay_warnings().join("\n");
        assert!(
            warnings.contains(&format!("{step_name} differs from the recording"))
                && warnings.contains("the game plays out the same, audio.wav will not"),
            "{warnings}"
        );
        assert_eq!(without.reports().count(), 1);
    }

    #[test]
    fn a_loop_changes_only_at_its_wrap() {
        let folder = TestFolder::new("sound-change-wrap");
        use_store(&folder.join("store"));
        let theme = folder.join("theme.wav");
        let name = crate::files::name_of(&theme);
        save(&theme, &wav(1000, |i| (i * 16) as i16)); // a ramp up
        struct Music(Wav);
        impl Game for Music {
            fn update(&mut self, ctx: &mut Context) {
                if ctx.frame() == 1 {
                    ctx.play_loop(&self.0, 1.0);
                }
            }
            fn draw(&self, _: &mut crate::canvas::Canvas) {}
        }
        let mut sim = Simulation::new(config(), Music(Wav::load(&theme).unwrap()));
        sim.run(39);
        save(&theme, &wav(600, |i| -((i * 20) as i16))); // a ramp down
        assert_eq!(sim.change_sound_file(&name), Ok(Some(40)));
        sim.run(11);

        // Update 40 begins at sample 39 x 735 = 28,665. The loop started at
        // sample 0 and goes round every 1,000, so its next wrap is 29,000.
        let looped = &sim.loops()[0];
        let starts: Vec<(u64, usize)> = looped
            .versions
            .iter()
            .map(|(at, s)| (*at, s.frames()))
            .collect();
        assert_eq!(starts, [(0, 1000), (29_000, 600)]);
        let audio = mix_to_wav(sim.sounds(), sim.loops(), sim.sample_at(51)).bytes;
        let samples = crate::sound::decode_wav(&audio).unwrap();
        let at = |i: usize| (samples.frame(i).0 * 32768.0).round() as i32;
        // Still the old ramp up to the wrap, mid-way through it when the
        // file changed...
        assert_eq!(at(28_665), 665 * 16);
        assert_eq!(at(28_999), 999 * 16);
        // ...then the new sound from its start, round every 600.
        assert_eq!(at(29_000), 0);
        assert_eq!(at(29_001), -20);
        assert_eq!(at(29_599), -599 * 20);
        assert_eq!(at(29_600), 0);

        // Two changes before one wrap: the newer waits for it instead.
        save(&theme, &wav(400, |_| 777));
        let frame = sim.change_sound_file(&name).unwrap().unwrap();
        save(&theme, &wav(800, |_| 888));
        assert_eq!(sim.change_sound_file(&name), Ok(Some(frame)));
        let looped = &sim.loops()[0];
        let last = looped.versions.last().unwrap();
        assert_eq!((last.1.frames(), looped.versions.len()), (800, 3));
    }

    #[test]
    fn a_half_written_file_is_not_taken_until_it_is_whole() {
        let folder = TestFolder::new("sound-change-half");
        use_store(&folder.join("store"));
        let step = folder.join("step.wav");
        let name = crate::files::name_of(&step);
        save(&step, &wav(300, |i| i as i16));
        struct Plays(Wav);
        impl Game for Plays {
            fn update(&mut self, ctx: &mut Context) {
                ctx.play_wav(&self.0);
            }
            fn draw(&self, _: &mut crate::canvas::Canvas) {}
        }
        let mut sim = Simulation::new(config(), Plays(Wav::load(&step).unwrap()));
        sim.run(10);

        // Written in place, and caught half-way: its header says 2,000
        // bytes of sound, and 700 are there.
        let new = wav(1000, |i| -(i as i16));
        std::fs::write(&step, &new[..744]).unwrap();
        let problem = sim.change_sound_file(&name).unwrap_err();
        assert!(problem.contains("still being written"), "{problem}");
        // The window's look says so once, and keeps the old sound.
        let said = sim.pick_up_sound_files();
        assert!(said.is_empty(), "said already: {said:?}");
        std::fs::write(&step, &new[..1500]).unwrap();
        let said = sim.pick_up_sound_files();
        assert!(
            said.len() == 1 && said[0].contains("can't be read yet"),
            "{said:?}"
        );
        assert!(sim.pick_up_sound_files().is_empty(), "said once");
        // Nothing written yet: the game still hears the old sound.
        std::fs::write(&step, &new[..20]).unwrap();
        let _ = sim.pick_up_sound_files();
        sim.run(1);
        assert_eq!(sim.sounds().last().unwrap().samples.frames(), 300);

        // Whole: taken, from the coming update.
        std::fs::write(&step, &new).unwrap();
        assert_eq!(
            sim.pick_up_sound_files(),
            [format!("{name} changed; plays from frame 12")]
        );
        sim.run(1);
        assert_eq!(sim.sounds().last().unwrap().samples.frames(), 1000);
        assert_eq!(
            sim.recording().changes.len(),
            1,
            "only the whole one is kept"
        );
    }

    #[test]
    fn only_plain_hashes_name_a_store_file() {
        let good = format!("sha256:{}", "ab".repeat(32));
        assert!(
            store_path(&good)
                .unwrap()
                .ends_with(format!("{}.wav", "ab".repeat(32)))
        );
        assert!(store_path("sha256:../../evil").is_none());
        assert!(
            store_path(&"ab".repeat(32)).is_none(),
            "the sha256: is part of it"
        );
    }
}
