//! `--fuzz N`: play the game N times with random inputs, looking for
//! crashes, broken rules and freezes, without anyone watching.
//!
//! Each run uses seed 1, 2, ... N, and random but realistic inputs: keys
//! tapped for a few frames or held for a while, never one key per frame.
//! Each run is a separate headless run of the game's own program (so a
//! crash in one can't stop the others, and every run starts from a fresh
//! game), several at a time on all the computer's cores.
//!
//! A run has a problem if the game:
//!
//! - **panics** (crashes): the message, where, and the frame;
//! - **breaks a rule** from `ctx.check`: the rule and the first frame;
//! - **freezes**: no reported value changes for `--freeze-seconds`
//!   (10 by default) while keys are being pressed, except on frames where
//!   the game called `ctx.idle_ok()` (a title or pause screen waiting).
//!
//! The inputs of every run with a problem are saved as an input script
//! (`fuzz/seed-17.json`), so `--inputs fuzz/seed-17.json` replays it
//! exactly. For each different problem, one script is also *shrunk*: key
//! presses are removed while the problem still happens, leaving the
//! shortest script that shows it.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use serde_json::Value;

use crate::cli::Options;
use crate::engine::Config;
use crate::input::Key;
use crate::rng::Rng;
use crate::script::{InputAction, InputEvent, InputScript};

/// The keys pressed when `--fuzz-keys` isn't given.
pub(crate) const DEFAULT_KEYS: [Key; 7] = [
    Key::Left,
    Key::Right,
    Key::Up,
    Key::Down,
    Key::Space,
    Key::Z,
    Key::X,
];
/// How long each run lasts when `--frames` isn't given: 30 seconds at 60
/// updates per second.
const DEFAULT_FRAMES: u64 = 1800;
/// At most this many runs while shrinking one script.
const SHRINK_RUNS: usize = 150;

/// What went wrong in one run.
#[derive(Debug, Clone, PartialEq)]
enum Problem {
    Panic { message: String, frame: Option<u64> },
    Check { name: String, frame: u64 },
    Freeze { frame: u64, seconds: f32 },
}

impl Problem {
    /// Problems that are "the same bug" share a key: same kind, and the
    /// same rule or panic message.
    fn key(&self) -> String {
        match self {
            Problem::Panic { message, .. } => format!("panic: {message}"),
            Problem::Check { name, .. } => format!("check: {name}"),
            Problem::Freeze { .. } => String::from("freeze"),
        }
    }

    fn frame(&self) -> Option<u64> {
        match self {
            Problem::Panic { frame, .. } => *frame,
            Problem::Check { frame, .. } | Problem::Freeze { frame, .. } => Some(*frame),
        }
    }

    fn describe(&self) -> String {
        match self {
            Problem::Panic { message, frame } => match frame {
                Some(frame) => format!("PANIC on frame {frame}: {message}"),
                None => format!("PANIC: {message}"),
            },
            Problem::Check { name, frame } => format!("CHECK {name:?} failed on frame {frame}"),
            Problem::Freeze { frame, seconds } => format!(
                "FREEZE: by frame {frame}, no reported value had changed for {seconds} s while keys were pressed"
            ),
        }
    }
}

/// The settings for one fuzzing session.
struct Fuzz {
    exe: PathBuf,
    /// Words passed on to every run: the game's own options, and tuning.
    pass_on: Vec<String>,
    keys: Vec<Key>,
    frames: u64,
    updates_per_second: f64,
    freeze_frames: u64,
    freeze_seconds: f32,
    out: PathBuf,
    work: PathBuf,
}

/// Runs `--fuzz`, prints one line per problem and a total, and saves the
/// scripts of the runs with problems.
pub(crate) fn run(config: &Config, options: &Options) -> Result<(), String> {
    let runs = options.fuzz.unwrap_or(0).max(1);
    let exe = std::env::current_exe()
        .map_err(|err| format!("--fuzz can't find its own program: {err}"))?;
    let mut pass_on = crate::cli::game_args();
    if let Some(tune) = &options.tune {
        pass_on.extend([String::from("--tune"), tune.display().to_string()]);
    }
    for (name, value) in &options.set {
        pass_on.extend([String::from("--set"), format!("{name}={value}")]);
    }
    let updates_per_second = 1.0 / f64::from(config.step_time());
    let freeze_seconds = options.freeze_seconds.unwrap_or(10.0);
    let out = options.out.clone().unwrap_or_else(|| PathBuf::from("fuzz"));
    let fuzz = Fuzz {
        exe,
        pass_on,
        keys: options
            .fuzz_keys
            .clone()
            .unwrap_or_else(|| DEFAULT_KEYS.to_vec()),
        frames: options.frames.unwrap_or(DEFAULT_FRAMES),
        updates_per_second,
        freeze_frames: (f64::from(freeze_seconds) * updates_per_second)
            .round()
            .max(1.0) as u64,
        freeze_seconds,
        work: out.join("work"),
        out,
    };
    fs::create_dir_all(&fuzz.work)
        .map_err(|err| format!("could not create {}: {err}", fuzz.work.display()))?;
    remove_old_scripts(&fuzz.out);

    let started = Instant::now();
    let cores = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(runs as usize);
    let next = AtomicU32::new(1);
    let results: Mutex<Vec<(u32, InputScript, Vec<Problem>)>> = Mutex::new(Vec::new());
    let fatal: Mutex<Option<String>> = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..cores {
            scope.spawn(|| {
                loop {
                    let seed = next.fetch_add(1, Ordering::Relaxed);
                    if seed > runs || fatal.lock().unwrap().is_some() {
                        break;
                    }
                    let script = random_script(seed, &fuzz);
                    match fuzz.play(&script, &format!("seed-{seed}")) {
                        Ok(problems) => results.lock().unwrap().push((seed, script, problems)),
                        Err(error) => *fatal.lock().unwrap() = Some(error),
                    }
                }
            });
        }
    });
    if let Some(error) = fatal.into_inner().unwrap() {
        let _ = fs::remove_dir_all(&fuzz.work);
        return Err(format!("--fuzz stopped: {error}"));
    }
    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(seed, ..)| *seed);

    // Group the problems: one line per different problem.
    let mut groups: BTreeMap<String, Vec<(u32, InputScript, Problem)>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut failing_runs = 0;
    for (seed, script, problems) in results {
        failing_runs += usize::from(!problems.is_empty());
        for problem in problems {
            let key = problem.key();
            if !groups.contains_key(&key) {
                order.push(key.clone());
            }
            groups
                .entry(key)
                .or_default()
                .push((seed, script.clone(), problem));
        }
    }
    for key in &order {
        let runs_with_it = &groups[key];
        for (seed, script, _) in runs_with_it {
            save_script(script, &fuzz.out.join(format!("seed-{seed}.json")))?;
        }
        let (seed, script, problem) = &runs_with_it[0];
        let shrunk = fuzz.shrink(script, problem);
        let path = fuzz.out.join(format!("seed-{seed}.json"));
        save_script(&shrunk, &path)?;
        let presses = shrunk
            .events
            .iter()
            .filter(|e| matches!(e.action, InputAction::Press(_)))
            .count();
        let mut line = format!(
            "duckforge: seed {seed}: {} -- replay: --inputs {} ({presses} key press{}, {} frames)",
            problem.describe(),
            path.display(),
            if presses == 1 { "" } else { "es" },
            shrunk.frames.unwrap_or(0)
        );
        if runs_with_it.len() > 1 {
            let others: Vec<String> = runs_with_it[1..]
                .iter()
                .take(5)
                .map(|(s, ..)| s.to_string())
                .collect();
            let more = runs_with_it.len().saturating_sub(6);
            line += &format!(
                "; also seeds {}{}",
                others.join(", "),
                if more > 0 {
                    format!(" and {more} more")
                } else {
                    String::new()
                }
            );
        }
        println!("{line}");
    }
    let _ = fs::remove_dir_all(&fuzz.work);
    let summary = format!(
        "duckforge: fuzzed {runs} runs of {:.0} s (seeds 1 to {runs}, keys {}) on {cores} cores in {:.1} s: ",
        fuzz.frames as f64 / fuzz.updates_per_second,
        fuzz.keys
            .iter()
            .map(|k| k.name())
            .collect::<Vec<_>>()
            .join(","),
        started.elapsed().as_secs_f32()
    );
    if order.is_empty() {
        println!("{summary}no problems found");
    } else {
        println!(
            "{summary}{} different problem{} in {failing_runs} of the runs; scripts saved in {}",
            order.len(),
            if order.len() == 1 { "" } else { "s" },
            fuzz.out.display()
        );
    }
    Ok(())
}

impl Fuzz {
    /// Plays `script` in a fresh headless run of the game, and says what
    /// went wrong, if anything: every failed check and a freeze, or the
    /// panic that ended it. An error means the run couldn't happen at all
    /// (a bad option, say), which would be the same for every run.
    fn play(&self, script: &InputScript, name: &str) -> Result<Vec<Problem>, String> {
        let script_path = self.work.join(format!("{name}.json"));
        let out = self.work.join(name);
        save_script(script, &script_path)?;
        let frames = script.frames.unwrap_or(self.frames);
        let output = Command::new(&self.exe)
            .args(["--headless", "--inputs"])
            .arg(&script_path)
            .args(["--frames", &frames.to_string(), "--out"])
            .arg(&out)
            .args(&self.pass_on)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|err| format!("could not start {}: {err}", self.exe.display()))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let problems = if output.status.success() {
            self.read_log(&out.join("log.json"), script)?
        } else if let Some(panic) = panic_from(&stderr) {
            vec![panic]
        } else {
            let first = stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("it failed without a message");
            return Err(first.trim_start_matches("Error: ").to_string());
        };
        let _ = fs::remove_dir_all(&out);
        let _ = fs::remove_file(&script_path);
        Ok(problems)
    }

    /// Failed checks and freezes, from a finished run's `log.json`.
    fn read_log(&self, path: &Path, script: &InputScript) -> Result<Vec<Problem>, String> {
        let text = fs::read_to_string(path)
            .map_err(|err| format!("could not read {}: {err}", path.display()))?;
        let log: Value = serde_json::from_str(&text)
            .map_err(|err| format!("{} isn't JSON: {err}", path.display()))?;
        let mut problems: Vec<Problem> = log["checks"]["failed"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|failed| Problem::Check {
                name: failed["name"].as_str().unwrap_or("?").to_string(),
                frame: failed["frame"].as_u64().unwrap_or(0),
            })
            .collect();
        // Frames where something changed, or the game said waiting is fine
        // (`ctx.idle_ok()`): a freeze is a long stretch with neither.
        let mut changes: Vec<u64> = log["changes"]
            .as_array()
            .map(|changes| changes.iter().filter_map(|c| c["frame"].as_u64()).collect())
            .unwrap_or_default();
        let reports_something = !changes.is_empty();
        for stretch in log["idle"].as_array().into_iter().flatten() {
            let (from, to) = (
                stretch[0].as_u64().unwrap_or(0),
                stretch[1].as_u64().unwrap_or(0),
            );
            changes.extend(from..=to);
        }
        changes.sort_unstable();
        changes.dedup();
        let frames = log["frames"].as_u64().unwrap_or(0);
        let frozen =
            reports_something.then(|| freeze(&changes, frames, script, self.freeze_frames));
        if let Some(frame) = frozen.flatten() {
            problems.push(Problem::Freeze {
                frame,
                seconds: self.freeze_seconds,
            });
        }
        Ok(problems)
    }

    /// The shortest script (by key presses) that still has the same
    /// problem: first cut off everything after it, then remove presses in
    /// chunks, then one at a time, keeping each removal that still fails.
    fn shrink(&self, script: &InputScript, problem: &Problem) -> InputScript {
        let end = problem
            .frame()
            .map_or(self.frames, |f| (f + 1).min(self.frames));
        let mut best = cut_after(script, end);
        let mut presses = presses(&best);
        let mut runs = 0;
        let mut chunk = presses.len().div_ceil(2).max(1);
        while !presses.is_empty() && runs < SHRINK_RUNS {
            let mut removed_any = false;
            let mut start = 0;
            while start < presses.len() && runs < SHRINK_RUNS {
                let mut fewer = presses.clone();
                fewer.drain(start..(start + chunk).min(presses.len()));
                let candidate = from_presses(&fewer, &best);
                runs += 1;
                let same = self
                    .play(&candidate, &format!("shrink-{}", std::process::id()))
                    .is_ok_and(|problems| problems.iter().any(|p| p.key() == problem.key()));
                if same {
                    presses = fewer;
                    best = candidate;
                    removed_any = true;
                } else {
                    start += chunk;
                }
            }
            if chunk == 1 && !removed_any {
                break;
            }
            chunk = (chunk / 2).max(1);
        }
        best
    }
}

/// The frame a freeze is noticed on: the first stretch of `freeze_frames`
/// frames with no change to any reported value while at least one key was
/// pressed in it. `None` if the game never reports anything (then there's
/// nothing to watch).
fn freeze(changes: &[u64], frames: u64, script: &InputScript, freeze_frames: u64) -> Option<u64> {
    if changes.is_empty() {
        return None;
    }
    let press_frames: Vec<u64> = script
        .events
        .iter()
        .filter(|e| matches!(e.action, InputAction::Press(_)))
        .map(|e| e.frame)
        .collect();
    let mut stretches: Vec<(u64, u64)> = changes.windows(2).map(|w| (w[0], w[1])).collect();
    stretches.push((*changes.last().unwrap(), frames + 1));
    for (from, to) in stretches {
        let noticed = from + freeze_frames;
        if to > noticed && press_frames.iter().any(|&p| p > from && p <= noticed) {
            return Some(noticed);
        }
    }
    None
}

/// The panic in a run's error output: its message, place and frame, from
/// the engine's one line,
/// `duckforge: the game panicked on frame 88 at examples/x.rs:10:5: message`.
fn panic_from(stderr: &str) -> Option<Problem> {
    let line = stderr.lines().find_map(|l| {
        l.strip_prefix("duckforge: the game panicked on frame ")
            .or_else(|| l.strip_prefix("duckforge: panicked"))
    })?;
    let (frame, rest) = match line.split_once(' ') {
        Some((n, rest)) if n.parse::<u64>().is_ok() => (n.parse().ok(), rest),
        _ => (None, line),
    };
    let rest = rest.trim_start().strip_prefix("at ").unwrap_or(rest);
    let (place, message) = rest.split_once(": ").unwrap_or(("", rest));
    Some(Problem::Panic {
        message: format!("{} ({place})", message.trim()),
        frame,
    })
}

/// Random but realistic inputs for one run: keys tapped (2 to 8 frames)
/// or held (a quarter of a second to two seconds), a few at a time, with
/// pauses between. The same seed always makes the same script.
fn random_script(seed: u32, fuzz: &Fuzz) -> InputScript {
    let mut rng = Rng::new(seed.wrapping_mul(2_654_435_761) ^ 0x00F0_022D);
    let mut events = Vec::new();
    let mut held_until: Vec<u64> = vec![0; fuzz.keys.len()];
    let mut frame = 1 + rng.range_i32(0, 30) as u64;
    while frame < fuzz.frames {
        let k = rng.range_i32(0, fuzz.keys.len() as i32) as usize;
        if held_until[k] <= frame {
            let length = if rng.chance(0.55) {
                rng.range_i32(2, 9) as u64
            } else {
                rng.range_i32(15, 121) as u64
            };
            let release = frame + length;
            events.push(InputEvent {
                frame,
                action: InputAction::Press(fuzz.keys[k]),
            });
            if release <= fuzz.frames {
                events.push(InputEvent {
                    frame: release,
                    action: InputAction::Release(fuzz.keys[k]),
                });
            }
            held_until[k] = release + 1;
        }
        frame += rng.range_i32(3, 41) as u64;
    }
    events.sort_by_key(|e| e.frame);
    InputScript {
        seed: Some(seed),
        updates_per_second: Some(fuzz.updates_per_second.round() as u32),
        frames: Some(fuzz.frames),
        events,
    }
}

/// Each key press, with the frame it's released on (if it is).
fn presses(script: &InputScript) -> Vec<(u64, Key, Option<u64>)> {
    let mut pairs = Vec::new();
    for (i, event) in script.events.iter().enumerate() {
        if let InputAction::Press(key) = event.action {
            let release = script.events[i + 1..]
                .iter()
                .find(|e| e.action == InputAction::Release(key))
                .map(|e| e.frame);
            pairs.push((event.frame, key, release));
        }
    }
    pairs
}

/// A script with only these presses (and their releases).
fn from_presses(presses: &[(u64, Key, Option<u64>)], like: &InputScript) -> InputScript {
    let mut events = Vec::new();
    for &(frame, key, release) in presses {
        events.push(InputEvent {
            frame,
            action: InputAction::Press(key),
        });
        if let Some(release) = release {
            events.push(InputEvent {
                frame: release,
                action: InputAction::Release(key),
            });
        }
    }
    events.sort_by_key(|e| e.frame);
    InputScript {
        events,
        ..like.clone()
    }
}

/// The script up to and including frame `end`.
fn cut_after(script: &InputScript, end: u64) -> InputScript {
    InputScript {
        events: script
            .events
            .iter()
            .filter(|e| e.frame <= end)
            .cloned()
            .collect(),
        frames: Some(end),
        ..script.clone()
    }
}

fn save_script(script: &InputScript, path: &Path) -> Result<(), String> {
    script.save(path)
}

/// Deletes `seed-*.json` scripts left by an earlier fuzz in `folder`.
fn remove_old_scripts(folder: &Path) {
    for entry in fs::read_dir(folder).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with("seed-") && name.ends_with(".json") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Reads `--fuzz-keys Left,Right,Space`.
pub(crate) fn parse_keys(text: &str) -> Result<Vec<Key>, String> {
    let keys: Result<Vec<Key>, String> = text
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| {
            Key::from_name(name).ok_or_else(|| {
                format!("--fuzz-keys: {name:?} isn't a key name; use names like Left, Right, Up, Space, Z, Enter")
            })
        })
        .collect();
    let keys = keys?;
    if keys.is_empty() {
        return Err(String::from(
            "--fuzz-keys needs at least one key, like --fuzz-keys Left,Right,Space",
        ));
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(frames: u64) -> Fuzz {
        Fuzz {
            exe: PathBuf::new(),
            pass_on: Vec::new(),
            keys: vec![Key::Left, Key::Right, Key::Space],
            frames,
            updates_per_second: 60.0,
            freeze_frames: 600,
            freeze_seconds: 10.0,
            out: PathBuf::new(),
            work: PathBuf::new(),
        }
    }

    #[test]
    fn random_inputs_are_repeatable_realistic_and_balanced() {
        let fuzz = settings(1800);
        let script = random_script(17, &fuzz);
        assert_eq!(
            script,
            random_script(17, &fuzz),
            "the same seed, the same script"
        );
        assert_ne!(script.events, random_script(18, &fuzz).events);
        assert_eq!((script.seed, script.frames), (Some(17), Some(1800)));
        let pairs = presses(&script);
        assert!(
            pairs.len() > 50,
            "lots of presses in 30 seconds: {}",
            pairs.len()
        );
        for (frame, key, release) in &pairs {
            if let Some(release) = release {
                assert!(
                    release - frame >= 2,
                    "{key:?} at {frame} is held at least 2 frames"
                );
            }
        }
        // Some taps and some holds.
        let lengths: Vec<u64> = pairs
            .iter()
            .filter_map(|(f, _, r)| r.map(|r| r - f))
            .collect();
        assert!(lengths.iter().any(|&l| l <= 8) && lengths.iter().any(|&l| l >= 15));
        // A key is never pressed again while it's still held.
        for key in [Key::Left, Key::Right, Key::Space] {
            let mut down = false;
            for event in &script.events {
                match event.action {
                    InputAction::Press(k) if k == key => {
                        assert!(!down, "{key:?} pressed twice at {}", event.frame);
                        down = true;
                    }
                    InputAction::Release(k) if k == key => down = false,
                    _ => {}
                }
            }
        }
    }

    #[test]
    fn a_freeze_needs_silence_while_keys_are_pressed() {
        let script = |presses: &[u64]| InputScript {
            events: presses
                .iter()
                .map(|&frame| InputEvent {
                    frame,
                    action: InputAction::Press(Key::Space),
                })
                .collect(),
            ..InputScript::default()
        };
        // Changes on frames 1 and 50, then nothing until the end (frame 2000).
        let changes = [1, 50];
        assert_eq!(freeze(&changes, 2000, &script(&[100]), 600), Some(650));
        assert_eq!(
            freeze(&changes, 2000, &script(&[]), 600),
            None,
            "nobody pressed anything"
        );
        assert_eq!(
            freeze(&changes, 400, &script(&[100]), 600),
            None,
            "the run ended first"
        );
        assert_eq!(
            freeze(&[], 2000, &script(&[100]), 600),
            None,
            "the game reports nothing"
        );
        assert_eq!(
            freeze(&[1, 50, 640], 2000, &script(&[100, 700]), 600),
            Some(1240),
            "a later stretch"
        );
    }

    #[test]
    fn idle_frames_are_not_a_freeze() {
        let folder =
            std::env::temp_dir().join(format!("duckforge-fuzz-idle-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("log.json");
        let fuzz = settings(2000);
        let pressing = InputScript {
            events: vec![InputEvent {
                frame: 100,
                action: InputAction::Press(Key::Space),
            }],
            ..InputScript::default()
        };
        // Something changed on frame 1, then nothing: a freeze at frame 601...
        fs::write(
            &path,
            r#"{"frames": 2000, "changes": [{"frame": 1}], "idle": []}"#,
        )
        .unwrap();
        assert!(matches!(
            fuzz.read_log(&path, &pressing).unwrap()[..],
            [Problem::Freeze { frame: 601, .. }]
        ));
        // ...unless the game said it was waiting on purpose the whole time.
        fs::write(
            &path,
            r#"{"frames": 2000, "changes": [{"frame": 1}], "idle": [[2, 2000]]}"#,
        )
        .unwrap();
        assert!(fuzz.read_log(&path, &pressing).unwrap().is_empty());
        // Waiting only until frame 300 moves the freeze later.
        fs::write(
            &path,
            r#"{"frames": 2000, "changes": [{"frame": 1}], "idle": [[2, 300]]}"#,
        )
        .unwrap();
        let pressing_later = InputScript {
            events: vec![InputEvent {
                frame: 400,
                action: InputAction::Press(Key::Space),
            }],
            ..InputScript::default()
        };
        assert!(matches!(
            fuzz.read_log(&path, &pressing_later).unwrap()[..],
            [Problem::Freeze { frame: 900, .. }]
        ));
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn panics_are_read_from_the_error_output() {
        let stderr = "duckforge: the game panicked on frame 88 at examples/x.rs:10:5: \
                      attempt to subtract with overflow\n";
        assert_eq!(
            panic_from(stderr),
            Some(Problem::Panic {
                message: String::from("attempt to subtract with overflow (examples/x.rs:10:5)"),
                frame: Some(88),
            })
        );
        assert_eq!(panic_from("Error: something else"), None);
        assert_eq!(
            panic_from("duckforge: panicked at src/a.rs:1:2: no game yet"),
            Some(Problem::Panic {
                message: String::from("no game yet (src/a.rs:1:2)"),
                frame: None,
            })
        );
    }

    #[test]
    fn shrinking_helpers_keep_presses_and_releases_together() {
        let fuzz = settings(300);
        let script = random_script(3, &fuzz);
        let pairs = presses(&script);
        let rebuilt = from_presses(&pairs, &script);
        assert_eq!(presses(&rebuilt), pairs);
        let cut = cut_after(&script, 100);
        assert!(cut.events.iter().all(|e| e.frame <= 100));
        assert_eq!(cut.frames, Some(100));
    }

    #[test]
    fn key_lists_are_read() {
        assert_eq!(
            parse_keys("Left, Right,Space"),
            Ok(vec![Key::Left, Key::Right, Key::Space])
        );
        assert!(
            parse_keys("Left,Jump")
                .unwrap_err()
                .contains("\"Jump\" isn't a key name")
        );
        assert!(parse_keys("").is_err());
    }
}
