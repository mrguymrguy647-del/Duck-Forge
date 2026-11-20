//! A set of variants of one sound (eight footsteps, say), and playing one
//! of them at random, so a sound heard over and over doesn't sound the
//! same every time.
//!
//! Duck Echo makes them: `step_1.wav` to `step_8.wav`, with a list,
//! `step.variants.json`, that names them. Load the list, and play the set
//! like a `Wav`:
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # fn example(ctx: &mut Context) -> Result<(), duckforge::LoadError> {
//! let steps = SoundSet::load("examples/mygame/assets/step.variants.json")?;
//! ctx.play_with(&steps, 0.8, 0.0); // one of the eight
//! # Ok(())
//! # }
//! ```
//!
//! Which one plays comes from [`Context::sound_rng`], made from the game's
//! seed, so a replay plays the same ones; and it's never the same one
//! twice in a row. Each variant is an ordinary `Wav`, watched and swapped
//! like any other when its file changes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

use crate::error::LoadError;
use crate::rng::Rng;
use crate::sound::Wav;

/// No variant has played yet.
const NONE_YET: u32 = u32::MAX;

/// Variants of one sound, from a Duck Echo `.variants.json` list, ready to
/// play with [`Context::play_with`](crate::engine::Context::play_with).
/// Cloning one is cheap, and the clones share what played last.
#[derive(Clone)]
pub struct SoundSet {
    name: String,
    variants: Vec<Wav>,
    /// The index of the variant that played last (`NONE_YET` before the
    /// first), shared by the clones, so none plays twice in a row.
    last: Arc<AtomicU32>,
}

impl SoundSet {
    /// Loads a set from the list Duck Echo writes, `step.variants.json`,
    /// and each `.wav` it names (from the list's folder). The set's name
    /// in logs is the list's name without `.variants.json` (`step`).
    ///
    /// A list that's wrong says where: the line, or the field, like
    /// `files[2] has no "file"`.
    pub fn load(path: impl AsRef<Path>) -> Result<SoundSet, LoadError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "sound set", &error))?;
        let wrong = |problem: String| {
            LoadError::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{} isn't a list of variants Duck Forge can read: {problem}; make it again \
                     with Duck Echo (sound_variants, or Send to game with variants)",
                    path.display()
                ),
            )
        };
        let files = files_in(&text).map_err(wrong)?;
        let folder = path.parent().unwrap_or(Path::new(""));
        let mut variants = Vec::with_capacity(files.len());
        for file in files {
            variants.push(Wav::load(folder.join(file))?);
        }
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = file_name
            .strip_suffix(".variants.json")
            .or_else(|| file_name.strip_suffix(".json"))
            .unwrap_or(&file_name)
            .to_string();
        Ok(SoundSet {
            name,
            variants,
            last: Arc::new(AtomicU32::new(NONE_YET)),
        })
    }

    /// The name shown in logs and used by play tests (`step`): every
    /// variant is logged with this name, and its number.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The variants, in the list's order (the first is variant 1).
    pub fn variants(&self) -> &[Wav] {
        &self.variants
    }

    /// How many variants it has.
    pub fn len(&self) -> usize {
        self.variants.len()
    }

    /// Whether it has none (a list Duck Forge loads always has at least one).
    pub fn is_empty(&self) -> bool {
        self.variants.is_empty()
    }

    /// Chooses the variant to play, with `rng` (the game's
    /// [`sound_rng`](crate::engine::Context::sound_rng)): any but the one
    /// played last, when there are two or more. Returns its index.
    pub(crate) fn choose(&self, rng: &mut Rng) -> usize {
        choose_from(self.variants.len(), &self.last, rng)
    }
}

/// Chooses one of `count` variants with `rng`, never `last` (the one
/// before) when there are two or more, and notes it as the last. Only
/// `rng` decides, so the same seed always chooses the same ones.
fn choose_from(count: usize, last: &AtomicU32, rng: &mut Rng) -> usize {
    if count < 2 {
        return 0;
    }
    let before = last.load(Ordering::Relaxed) as usize;
    let index = if before < count {
        // One of the others: count - 1 to choose from, skipping it.
        let pick = rng.range_i32(0, count as i32 - 1) as usize;
        if pick >= before { pick + 1 } else { pick }
    } else {
        rng.range_i32(0, count as i32) as usize
    };
    last.store(index as u32, Ordering::Relaxed);
    index
}

impl std::fmt::Debug for SoundSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SoundSet({:?}, {} variants)",
            self.name,
            self.variants.len()
        )
    }
}

/// The files a `.variants.json` list names, in order. The error says where
/// it's wrong.
fn files_in(text: &str) -> Result<Vec<PathBuf>, String> {
    let list: Value = serde_json::from_str(text)
        .map_err(|error| format!("it isn't JSON (line {}: {error})", error.line()))?;
    let Value::Object(fields) = &list else {
        return Err(String::from(
            "it should be an object like {\"kind\": \"duck-echo-variants\", \"files\": [...]}",
        ));
    };
    match fields.get("kind").and_then(Value::as_str) {
        Some("duck-echo-variants") => {}
        Some(other) => {
            return Err(format!(
                "\"kind\" is {other:?}, and a list of variants is \"duck-echo-variants\""
            ));
        }
        None => return Err(String::from("it has no \"kind\": \"duck-echo-variants\"")),
    }
    let Some(Value::Array(files)) = fields.get("files") else {
        return Err(String::from("it has no \"files\" list"));
    };
    if files.is_empty() {
        return Err(String::from("its \"files\" list is empty"));
    }
    files
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let file = entry
                .get("file")
                .ok_or_else(|| format!("files[{i}] has no \"file\""))?
                .as_str()
                .ok_or_else(|| {
                    format!("files[{i}].file should be a file name, like \"step_1.wav\"")
                })?;
            // Only files beside the list: never a path that climbs out.
            let plain = Path::new(file)
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)));
            if !plain || file.is_empty() {
                return Err(format!(
                    "files[{i}].file is {file:?}; it should name a file beside the list"
                ));
            }
            Ok(PathBuf::from(file))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wrong_list_says_where() {
        let say = |text: &str| files_in(text).unwrap_err();
        assert!(
            say("{\"kind\": ").contains("line 1"),
            "{}",
            say("{\"kind\": ")
        );
        assert!(say("[]").contains("should be an object"));
        assert!(say("{\"files\": []}").contains("no \"kind\""));
        let list =
            |files: &str| format!("{{\"kind\": \"duck-echo-variants\", \"files\": {files}}}");
        assert!(say(&list("[]")).contains("empty"));
        assert_eq!(
            say(&list("[{\"file\": \"a.wav\"}, {}]")),
            "files[1] has no \"file\""
        );
        assert!(say(&list("[{\"file\": 3}]")).starts_with("files[0].file should be"));
        assert!(say(&list("[{\"file\": \"../../evil.wav\"}]")).contains("beside the list"));
        assert_eq!(
            files_in(&list(
                "[{\"file\": \"step_1.wav\"}, {\"file\": \"step_2.wav\"}]"
            ))
            .unwrap(),
            [PathBuf::from("step_1.wav"), PathBuf::from("step_2.wav")]
        );
    }

    #[test]
    fn never_the_same_variant_twice_in_a_row_and_every_one_plays() {
        let last = AtomicU32::new(NONE_YET);
        let mut rng = Rng::new(3);
        let picks: Vec<usize> = (0..400).map(|_| choose_from(4, &last, &mut rng)).collect();
        assert!(picks.windows(2).all(|two| two[0] != two[1]));
        for variant in 0..4 {
            assert!(picks.contains(&variant), "variant {variant} never played");
        }
        // The same seed chooses the same ones.
        let again = AtomicU32::new(NONE_YET);
        let mut rng = Rng::new(3);
        let replayed: Vec<usize> = (0..400).map(|_| choose_from(4, &again, &mut rng)).collect();
        assert_eq!(picks, replayed);
        // One variant, or none: always the first.
        assert_eq!(choose_from(1, &last, &mut rng), 0);
        assert_eq!(choose_from(0, &last, &mut rng), 0);
    }

    use crate::engine::{Config, Context, Game};
    use crate::headless::{HeadlessOptions, run_headless};
    use crate::simulation::Simulation;
    use crate::test_folder::TestFolder;

    /// A set of four steps, each a different length (100, 200, 300 and 400
    /// samples), so which one played can be seen in `audio.wav` too.
    fn four_steps(folder: &Path) -> PathBuf {
        let mut files = Vec::new();
        for n in 1..=4 {
            let count = 100 * n;
            let bytes = crate::sound::wav_bytes((0..count).map(|i| (i * 7) as i16), count);
            std::fs::write(folder.join(format!("step_{n}.wav")), bytes).unwrap();
            files.push(format!("{{\"file\": \"step_{n}.wav\", \"seed\": {n}}}"));
        }
        let list = folder.join("step.variants.json");
        std::fs::write(
            &list,
            format!(
                "{{\"kind\": \"duck-echo-variants\", \"version\": 1, \"pick\": \"random\", \
                 \"files\": [{}]}}",
                files.join(", ")
            ),
        )
        .unwrap();
        list
    }

    /// Plays a step every 4 frames (or not), and reports the game's own
    /// random numbers.
    struct Walker {
        steps: SoundSet,
        play: bool,
        rolled: Vec<u32>,
    }

    impl Game for Walker {
        fn update(&mut self, ctx: &mut Context) {
            if self.play && ctx.frame() % 4 == 1 {
                ctx.play_with(&self.steps, 0.8, 0.0);
            }
            self.rolled.push(ctx.rng.next_u32());
            ctx.report("roll", self.rolled.last().copied().unwrap_or(0) as f64);
        }
        fn draw(&self, _: &mut crate::canvas::Canvas) {}
    }

    fn config() -> Config {
        Config {
            width: 32,
            height: 8,
            seed: Some(77),
            ..Config::default()
        }
    }

    /// (frame, name, variant) of every sound played.
    fn heard(sim: &Simulation<Walker>) -> Vec<(u64, String, Option<u32>)> {
        sim.sounds()
            .iter()
            .map(|s| (s.frame, s.name.clone(), s.variant))
            .collect()
    }

    #[test]
    fn a_replay_picks_the_same_variants() {
        let folder = TestFolder::new("set-replay");
        let list = four_steps(&folder);
        let walker = |play| Walker {
            steps: SoundSet::load(&list).unwrap(),
            play,
            rolled: Vec::new(),
        };
        let mut live = Simulation::new(config(), walker(true));
        live.run(120);
        let played = heard(&live);
        assert_eq!(played.len(), 30);
        assert!(
            played.iter().all(|(_, name, _)| name == "step"),
            "the set's name"
        );
        let variants: Vec<u32> = played.iter().map(|(_, _, v)| v.unwrap()).collect();
        assert!(
            variants.windows(2).all(|two| two[0] != two[1]),
            "{variants:?}"
        );
        assert!((1..=4).all(|v| variants.contains(&v)), "{variants:?}");
        let recording = live.recording();
        drop(live);

        // Replayed headless from the recording: the same steps, the same
        // variants, and log.json names each one.
        let out = folder.join("out");
        let replay = run_headless(
            config(),
            walker(true),
            &HeadlessOptions {
                frames: 120,
                out: Some(out.clone()),
                inputs: Some(recording),
                ..HeadlessOptions::default()
            },
        )
        .unwrap();
        assert_eq!(heard(&replay), played);
        let log = std::fs::read_to_string(out.join("log.json")).unwrap();
        let first = format!("\"variant\": {}}}", variants[0]);
        assert!(
            log.contains(r#"{"frame": 1, "sound": "step", "file": "#) && log.contains(&first),
            "{log}"
        );
        // Variant 3 is 300 samples: what's in audio.wav is that variant.
        let lengths: Vec<usize> = replay.sounds().iter().map(|s| s.samples.frames()).collect();
        assert!(
            lengths
                .iter()
                .zip(&variants)
                .all(|(&len, &v)| len == 100 * v as usize)
        );
    }

    #[test]
    fn playing_variants_never_changes_the_games_own_random_numbers() {
        let folder = TestFolder::new("set-rng");
        let list = four_steps(&folder);
        let run = |play| {
            let mut sim = Simulation::new(
                config(),
                Walker {
                    steps: SoundSet::load(&list).unwrap(),
                    play,
                    rolled: Vec::new(),
                },
            );
            sim.run(100);
            sim.game().rolled.clone()
        };
        assert_eq!(run(true), run(false), "the same ctx.rng numbers either way");
    }

    #[test]
    fn a_scene_plays_one_of_a_set() {
        use crate::scene::Scene;
        let folder = TestFolder::new("set-scene");
        std::fs::create_dir_all(folder.join("assets")).unwrap();
        four_steps(&folder.join("assets"));
        let path = folder.join("pond.json");
        std::fs::write(
            &path,
            r#"{"duckforge_scene": 1, "objects": [
                {"id": 1, "name": "steps", "kind": "sound", "sound": "assets/step.variants.json"}]}"#,
        )
        .unwrap();
        let scene = Scene::load(&path).unwrap();
        assert!(scene.problems().is_empty(), "{:?}", scene.problems());
        struct Pond(Scene);
        impl Game for Pond {
            fn update(&mut self, ctx: &mut Context) {
                self.0.update(ctx);
            }
            fn draw(&self, _: &mut crate::canvas::Canvas) {}
        }
        let mut sim = Simulation::new(config(), Pond(scene));
        sim.run(2);
        let sound = &sim.sounds()[0];
        assert_eq!((sound.name.as_str(), sound.frame), ("step", 1));
        assert!(sound.variant.is_some_and(|v| (1..=4).contains(&v)));
    }
}
