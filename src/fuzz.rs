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
//!
//! The keys pressed are the ones the game reads. Before the runs, a few
//! short practice runs press the keys found so far and note every key the
//! game asks about (`is_down`, `was_pressed`...), until no new one turns
//! up. Escape is left out (it usually quits, which ends a run early), and
//! so are the engine's own F1, F3, F10 and F12. `--fuzz-keys` picks the
//! keys instead.
//!
//! "No problems found" only means something if the random player got
//! somewhere, so the end also prints the **coverage**: the lowest and
//! highest of every reported number across all runs, the text values
//! reached (and any a playthrough ends on that no run reached), and how far
//! the runs got.

use std::collections::{BTreeMap, BTreeSet};
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
use crate::report::ReportValue;
use crate::rng::Rng;
use crate::script::{InputAction, InputEvent, InputScript};

/// The keys the first practice run presses, to find the keys a game
/// reads (and the keys used if a game reads none).
pub(crate) const DEFAULT_KEYS: [Key; 8] = [
    Key::Left,
    Key::Right,
    Key::Up,
    Key::Down,
    Key::Space,
    Key::Z,
    Key::X,
    Key::Enter,
];
/// Keys the fuzzer never learns: Escape usually quits, and the F keys are
/// the engine's.
const NEVER_LEARNED: [Key; 5] = [Key::Escape, Key::F1, Key::F3, Key::F10, Key::F12];
/// At most this many practice runs to learn the keys.
const PRACTICE_RUNS: u32 = 4;
/// And each lasts at most this many frames (15 seconds at 60 per second).
const PRACTICE_FRAMES: u64 = 900;
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
#[derive(Debug, Clone)]
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
    let mut fuzz = Fuzz {
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
    let learned = options.fuzz_keys.is_none();
    if learned {
        fuzz.keys = fuzz.learn_keys().inspect_err(|_| {
            let _ = fs::remove_dir_all(&fuzz.work);
        })?;
    }
    let cores = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(runs as usize);
    let next = AtomicU32::new(1);
    let results: Mutex<Vec<(u32, InputScript, Vec<Problem>)>> = Mutex::new(Vec::new());
    let coverage = Mutex::new(Coverage::default());
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
                        Ok((problems, run)) => {
                            coverage.lock().unwrap().merge(run);
                            results.lock().unwrap().push((seed, script, problems));
                        }
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
        "duckforge: fuzzed {runs} runs of {:.0} s (seeds 1 to {runs}, keys {}{}) on {cores} cores in {:.1} s: ",
        fuzz.frames as f64 / fuzz.updates_per_second,
        fuzz.keys
            .iter()
            .map(|k| k.name())
            .collect::<Vec<_>>()
            .join(","),
        if learned {
            ", the ones the game reads"
        } else {
            ""
        },
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
    let coverage = coverage.into_inner().unwrap();
    for line in coverage.lines(fuzz.frames, &playthrough_endings()) {
        println!("duckforge: coverage: {line}");
    }
    Ok(())
}

/// What the random runs reached, added up over all of them.
#[derive(Debug, Default, Clone, PartialEq)]
struct Coverage {
    /// Each reported number: the lowest and highest it ever was.
    numbers: BTreeMap<String, (f64, f64)>,
    /// Each reported text or yes/no: every value it ever had.
    texts: BTreeMap<String, BTreeSet<String>>,
    /// How many frames each run lasted.
    lengths: Vec<u64>,
    /// Every key the game asked about.
    keys_read: BTreeSet<String>,
}

impl Coverage {
    /// One run's coverage, from its `log.json`.
    fn from_log(log: &Value) -> Coverage {
        let mut coverage = Coverage::default();
        for change in log["changes"].as_array().into_iter().flatten() {
            for (name, value) in change.as_object().into_iter().flatten() {
                if name == "frame" {
                    continue;
                }
                match value {
                    Value::Number(n) => {
                        let n = n.as_f64().unwrap_or(0.0);
                        let range = coverage.numbers.entry(name.clone()).or_insert((n, n));
                        *range = (range.0.min(n), range.1.max(n));
                    }
                    Value::String(text) => coverage.add_text(name, text.clone()),
                    other => coverage.add_text(name, other.to_string()),
                }
            }
        }
        coverage.lengths.push(log["frames"].as_u64().unwrap_or(0));
        let keys = log["keys_read"].as_array().into_iter().flatten();
        coverage.keys_read = keys.filter_map(Value::as_str).map(String::from).collect();
        coverage
    }

    fn add_text(&mut self, name: &str, value: String) {
        self.texts
            .entry(name.to_string())
            .or_default()
            .insert(value);
    }

    fn merge(&mut self, other: Coverage) {
        for (name, (low, high)) in other.numbers {
            let range = self.numbers.entry(name).or_insert((low, high));
            *range = (range.0.min(low), range.1.max(high));
        }
        for (name, values) in other.texts {
            self.texts.entry(name).or_default().extend(values);
        }
        self.lengths.extend(other.lengths);
        self.keys_read.extend(other.keys_read);
    }

    /// The coverage lines: numbers, then text values and how long runs
    /// lasted. `endings` are text values playthroughs end on, by name
    /// (with the file), to point out the ones no run reached.
    fn lines(
        &self,
        planned: u64,
        endings: &BTreeMap<String, Vec<(String, String)>>,
    ) -> Vec<String> {
        let mut lines = Vec::new();
        if self.numbers.is_empty() && self.texts.is_empty() {
            lines.push(String::from(
                "the game reported nothing, so there's no telling what the runs reached",
            ));
        }
        let number = |n: f64| ReportValue::from(n).to_string();
        let numbers: Vec<String> = self
            .numbers
            .iter()
            .map(|(name, &(low, high))| {
                if low == high {
                    format!("{name} always {}", number(low))
                } else {
                    format!("{name} {} to {}", number(low), number(high))
                }
            })
            .collect();
        if !numbers.is_empty() {
            lines.push(numbers.join(", "));
        }
        let mut parts: Vec<String> = Vec::new();
        for (name, values) in &self.texts {
            let reached: Vec<&str> = values.iter().map(String::as_str).collect();
            let mut part = format!("{name} reached {}", reached.join(", "));
            let missed: Vec<String> = endings
                .get(name)
                .into_iter()
                .flatten()
                .filter(|(value, _)| !values.contains(value))
                .map(|(value, file)| format!("{value} (where {file} ends)"))
                .collect();
            if !missed.is_empty() {
                part += &format!("; never {}", missed.join(", "));
            }
            parts.push(part);
        }
        let longest = self.lengths.iter().copied().max().unwrap_or(0);
        let early: Vec<u64> = self
            .lengths
            .iter()
            .copied()
            .filter(|&n| n < planned)
            .collect();
        parts.push(match early.iter().min() {
            None => format!("every run lasted all {planned} frames"),
            Some(first) => format!(
                "furthest frame {longest}; {} of {} runs ended early (quit or crash), the first at frame {first}",
                early.len(),
                self.lengths.len()
            ),
        });
        lines.push(parts.join("; "));
        lines
    }
}

/// The text (and yes/no) values the game's playthroughs end on, from
/// their `"expect"` blocks: `state` -> `[("Won", "playthroughs/x.json")]`.
fn playthrough_endings() -> BTreeMap<String, Vec<(String, String)>> {
    let mut endings: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let Some(game) = crate::tune::game_name_of_this_program() else {
        return endings;
    };
    let Ok(entries) = fs::read_dir("playthroughs") else {
        return endings;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
            stem.is_some_and(|stem| stem == game || stem.starts_with(&format!("{game}-")))
                && path.extension().is_some_and(|e| e == "json")
        })
        .collect();
    files.sort();
    for file in files {
        let Ok(script) = InputScript::load(&file) else {
            continue;
        };
        for (name, value) in script.expect.into_iter().flatten() {
            let value = match value {
                ReportValue::Text(text) => text,
                ReportValue::Bool(b) => b.to_string(),
                ReportValue::Number(_) => continue,
            };
            let place = file.display().to_string().replace('\\', "/");
            let list = endings.entry(name).or_default();
            if !list.iter().any(|(v, _)| *v == value) {
                list.push((value, place));
            }
        }
    }
    endings
}

impl Fuzz {
    /// Plays `script` in a fresh headless run of the game, and says what
    /// went wrong, if anything: every failed check and a freeze, or the
    /// panic that ended it. An error means the run couldn't happen at all
    /// (a bad option, say), which would be the same for every run.
    /// The keys this game reads, found by practice runs: the first presses
    /// [`DEFAULT_KEYS`], each next one the keys found so far, until no new
    /// key turns up. [`DEFAULT_KEYS`] if the game reads none.
    fn learn_keys(&self) -> Result<Vec<Key>, String> {
        self.learn_keys_by(|practice, script, name| Ok(practice.play(script, name)?.1))
    }

    /// [`learn_keys`](Fuzz::learn_keys), with `play` running a practice
    /// run and giving its coverage (a test gives a pretend game).
    fn learn_keys_by(
        &self,
        mut play: impl FnMut(&Fuzz, &InputScript, &str) -> Result<Coverage, String>,
    ) -> Result<Vec<Key>, String> {
        let mut found: Vec<Key> = Vec::new();
        for round in 1..=PRACTICE_RUNS {
            let mut practice = self.clone();
            practice.frames = self.frames.min(PRACTICE_FRAMES);
            practice.keys = if found.is_empty() {
                DEFAULT_KEYS.to_vec()
            } else {
                found.clone()
            };
            let script = random_script(round, &practice);
            let coverage = play(&practice, &script, &format!("practice-{round}"))?;
            let new = learnable(&coverage.keys_read, &found);
            if new.is_empty() {
                break;
            }
            found.extend(new);
            // Always in the same order, so the runs are the same every time.
            found.sort_by_key(|key| Key::ALL.iter().position(|k| k == key));
        }
        Ok(if found.is_empty() {
            DEFAULT_KEYS.to_vec()
        } else {
            found
        })
    }

    fn play(&self, script: &InputScript, name: &str) -> Result<(Vec<Problem>, Coverage), String> {
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
        let (problems, coverage) = if output.status.success() {
            let log = read_json(&out.join("log.json"))?;
            (self.problems_in(&log, script), Coverage::from_log(&log))
        } else if let Some(panic) = panic_from(&stderr) {
            // A crash leaves no log: all we know is how far it got.
            let coverage = Coverage {
                lengths: vec![panic.frame().unwrap_or(0)],
                ..Coverage::default()
            };
            (vec![panic], coverage)
        } else {
            let first = stderr
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("it failed without a message");
            return Err(first.trim_start_matches("Error: ").to_string());
        };
        let _ = fs::remove_dir_all(&out);
        let _ = fs::remove_file(&script_path);
        Ok((problems, coverage))
    }

    /// Failed checks and freezes, from a finished run's `log.json`.
    #[cfg(test)]
    fn read_log(&self, path: &Path, script: &InputScript) -> Result<Vec<Problem>, String> {
        Ok(self.problems_in(&read_json(path)?, script))
    }

    /// Failed checks and freezes, from a finished run's log.
    fn problems_in(&self, log: &Value, script: &InputScript) -> Vec<Problem> {
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
        // Only presses of keys the game reads could change anything.
        let keys_read: Option<Vec<&str>> = log["keys_read"]
            .as_array()
            .map(|keys| keys.iter().filter_map(Value::as_str).collect());
        let press_frames: Vec<u64> = script
            .events
            .iter()
            .filter(|e| match &e.action {
                InputAction::Press(key) => keys_read
                    .as_ref()
                    .is_none_or(|read| read.contains(&key.name().as_str())),
                _ => false,
            })
            .map(|e| e.frame)
            .collect();
        let frozen =
            reports_something.then(|| freeze(&changes, frames, &press_frames, self.freeze_frames));
        if let Some(frame) = frozen.flatten() {
            problems.push(Problem::Freeze {
                frame,
                seconds: self.freeze_seconds,
            });
        }
        problems
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
                    .is_ok_and(|(problems, _)| problems.iter().any(|p| p.key() == problem.key()));
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
fn freeze(changes: &[u64], frames: u64, press_frames: &[u64], freeze_frames: u64) -> Option<u64> {
    if changes.is_empty() {
        return None;
    }
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

fn read_json(path: &Path) -> Result<Value, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    serde_json::from_str(&text).map_err(|err| format!("{} isn't JSON: {err}", path.display()))
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
        ..InputScript::default()
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

/// The keys in `read` (names from `log.json`) worth pressing that aren't
/// in `known` yet.
fn learnable(read: &BTreeSet<String>, known: &[Key]) -> Vec<Key> {
    read.iter()
        .filter_map(|name| Key::from_name(name))
        .filter(|key| !NEVER_LEARNED.contains(key) && !known.contains(key))
        .collect()
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

    /// A pretend game: on its title screen it reads Enter (and Escape, to
    /// quit); once Enter is pressed it reads W A S D; once W is pressed,
    /// Left Shift too (to run).
    #[test]
    fn the_keys_pressed_are_learned_from_the_game() {
        let fuzz = settings(1800);
        let mut practice_runs = 0;
        let keys = fuzz
            .learn_keys_by(|practice, script, _| {
                practice_runs += 1;
                assert!(script.frames.unwrap() <= PRACTICE_FRAMES);
                let pressed = |key| {
                    practice.keys.contains(&key)
                        && script
                            .events
                            .iter()
                            .any(|e| e.action == InputAction::Press(key))
                };
                let mut read = vec!["Enter", "Escape"];
                if pressed(Key::Enter) {
                    read.extend(["W", "A", "S", "D"]);
                }
                if pressed(Key::W) {
                    read.push("LeftShift");
                }
                Ok(Coverage {
                    keys_read: read.into_iter().map(String::from).collect(),
                    ..Coverage::default()
                })
            })
            .unwrap();
        // Not Escape (it quits), in the order of `Key::ALL`.
        assert_eq!(
            keys,
            [Key::A, Key::D, Key::S, Key::W, Key::Enter, Key::LeftShift]
        );
        assert!(practice_runs <= PRACTICE_RUNS);

        // A game that reads no keys at all gets the usual ones.
        let none = fuzz
            .learn_keys_by(|_, _, _| Ok(Coverage::default()))
            .unwrap();
        assert_eq!(none, DEFAULT_KEYS);
    }

    #[test]
    fn a_freeze_needs_silence_while_keys_are_pressed() {
        let script = |presses: &[u64]| presses.to_vec();
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
        let folder = crate::test_folder::TestFolder::new("fuzz-idle");
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
    }

    #[test]
    fn pressing_keys_the_game_never_reads_is_not_a_freeze() {
        let folder = crate::test_folder::TestFolder::new("fuzz-keys");
        let path = folder.join("log.json");
        let fuzz = settings(2000);
        let pressing = InputScript {
            events: vec![InputEvent {
                frame: 100,
                action: InputAction::Press(Key::Left),
            }],
            ..InputScript::default()
        };
        // The game only ever reads Space, so pressing Left can't do anything.
        fs::write(
            &path,
            r#"{"frames": 2000, "changes": [{"frame": 1}], "keys_read": ["Space"]}"#,
        )
        .unwrap();
        assert!(fuzz.read_log(&path, &pressing).unwrap().is_empty());
        // If it reads Left, it's a freeze again.
        fs::write(
            &path,
            r#"{"frames": 2000, "changes": [{"frame": 1}], "keys_read": ["Left", "Space"]}"#,
        )
        .unwrap();
        assert!(matches!(
            fuzz.read_log(&path, &pressing).unwrap()[..],
            [Problem::Freeze { frame: 601, .. }]
        ));
    }

    #[test]
    fn coverage_adds_up_what_the_runs_reached() {
        let log = |changes: &str, frames: u64| -> Value {
            serde_json::from_str(&format!(
                r#"{{"frames": {frames}, "changes": [{changes}]}}"#
            ))
            .unwrap()
        };
        let mut all = Coverage::default();
        all.merge(Coverage::from_log(&log(
            r#"{"frame": 1, "delivered": 0, "lives": 3, "state": "Playing"},
               {"frame": 90, "delivered": 2, "lives": 2}"#,
            1800,
        )));
        all.merge(Coverage::from_log(&log(
            r#"{"frame": 1, "delivered": 0, "lives": 3, "state": "Playing", "won": false},
               {"frame": 700, "lives": 0, "state": "Lost"}"#,
            760,
        )));
        // A crash: only how far it got.
        all.merge(Coverage {
            lengths: vec![300],
            ..Coverage::default()
        });
        let endings = BTreeMap::from([(
            String::from("state"),
            vec![
                (String::from("Lost"), String::from("playthroughs/a.json")),
                (String::from("Won"), String::from("playthroughs/b.json")),
            ],
        )]);
        assert_eq!(
            all.lines(1800, &endings),
            [
                "delivered 0 to 2, lives 0 to 3",
                "state reached Lost, Playing; never Won (where playthroughs/b.json ends); \
                 won reached false; furthest frame 1800; 2 of 3 runs ended early \
                 (quit or crash), the first at frame 300"
            ]
        );
        let quiet = Coverage::from_log(&log("", 1800));
        assert_eq!(
            quiet.lines(1800, &BTreeMap::new()),
            [
                "the game reported nothing, so there's no telling what the runs reached",
                "every run lasted all 1800 frames"
            ]
        );
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
