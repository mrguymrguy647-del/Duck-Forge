//! Review mode: check a render the way a viewer sees it, with still
//! pictures and numbers in place of motion and sound.
//!
//! `--review` next to `--video` (or with any headless run, `--inputs` and
//! `--fuzz` too) writes a `review` folder next to the other files:
//! `review.txt` (short, worst first, with times), `review.json`, and
//! pictures: `overview.png`, a filmstrip of every flagged moment and
//! event, `viewer.png` and `spectrogram.png`. `duckforge review any.mp4`
//! reviews a video made anywhere else, from its pixels and sound alone.
//!
//! A game helps by saying what it means, and the review then uses what
//! the engine knows instead of guessing from pixels:
//!
//! ```
//! # use duckforge::prelude::*;
//! # fn update(ctx: &mut Context, thump: Sound) {
//! ctx.place("cellar");                       // a new place: a camera jump here is a cut
//! ctx.event("impact");                       // shown in a filmstrip, sounds checked against it
//! ctx.play_for("impact", thump, 1.0, 0.0);   // must be heard within 2 frames of it
//! ctx.intend("impact shake", 0.5);           // this shaking is meant
//! # }
//! ```
//!
//! name the models that matter with `canvas.label_model`, and write down
//! what each moment must show with [`Expect`]. See docs/AI_GUIDE.md,
//! "Review a render".

pub(crate) mod audio;
pub(crate) mod checks;
pub(crate) mod draw;
pub mod expect;
pub(crate) mod external;
pub(crate) mod frames;
pub mod profiles;
pub(crate) mod truth;

pub use expect::{Expect, ExpectWhen, Mark, What, When};
pub use profiles::{Profile, Screen};
pub use truth::{FrameTruth, TextLook, Thing};

use crate::script::InputScript;
use std::path::{Path, PathBuf};

/// How bad a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// A viewer will notice: fix it.
    Flag,
    /// Probably worth fixing.
    Warn,
    /// For information.
    Note,
}

impl Severity {
    pub fn word(self) -> &'static str {
        match self {
            Severity::Flag => "FLAG",
            Severity::Warn => "warn",
            Severity::Note => "note",
        }
    }
}

/// One thing the review noticed.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub severity: Severity,
    /// Which check: "text contrast", "camera jitter", "too loud"...
    pub check: &'static str,
    /// The first and last frame it's true on.
    pub frame: u64,
    pub end: u64,
    pub text: String,
    /// The pictures that show it (in the review folder).
    pub pictures: Vec<String>,
    /// For a game: an input script that replays the run to just past it.
    pub replay: Option<String>,
}

/// Whether an expectation held.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub expectation: String,
    pub passed: bool,
    /// The frames it was checked on.
    pub frames: Vec<u64>,
    /// Why it failed (empty if it passed).
    pub why: String,
}

/// What a review found, and where it put its files.
#[derive(Debug, Clone)]
pub struct Review {
    pub title: String,
    pub width: usize,
    pub height: usize,
    pub updates_per_second: f64,
    pub frames: usize,
    /// The profiles the picture was judged by.
    pub profiles: Vec<String>,
    pub findings: Vec<Finding>,
    pub outcomes: Vec<Outcome>,
    pub folder: PathBuf,
    pub files: Vec<String>,
    /// How long the review took, in seconds (not counting the render).
    pub took: f32,
    /// Problems reading profile files.
    pub profile_problems: Vec<String>,
}

impl Review {
    /// How many findings of each kind: flags, warnings, notes.
    pub fn counts(&self) -> (usize, usize, usize) {
        let count = |s| self.findings.iter().filter(|f| f.severity == s).count();
        (
            count(Severity::Flag),
            count(Severity::Warn),
            count(Severity::Note),
        )
    }

    /// The expectations that failed.
    pub fn failed(&self) -> Vec<&Outcome> {
        self.outcomes.iter().filter(|o| !o.passed).collect()
    }

    /// The findings from one check.
    pub fn of(&self, check: &str) -> Vec<&Finding> {
        self.findings.iter().filter(|f| f.check == check).collect()
    }

    /// One line: what it found, and where to read it.
    pub fn line(&self) -> String {
        let (flags, warnings, notes) = self.counts();
        let failed = self.failed().len();
        let expected = if self.outcomes.is_empty() {
            String::new()
        } else {
            format!(
                ", {} of {} expectations passed",
                self.outcomes.len() - failed,
                self.outcomes.len()
            )
        };
        format!(
            "review: {flags} flagged, {warnings} warnings, {notes} notes{expected} (see {})",
            self.folder.join("review.txt").display()
        )
    }

    fn t(&self, frame: u64) -> String {
        checks::time(frame, self.updates_per_second)
    }

    /// `review.txt`: short, worst first, with times.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        let seconds = self.frames as f64 / self.updates_per_second;
        out += &format!(
            "Review of {}: {seconds:.1} s, {}x{}, {:.0} frames a second\n",
            self.title, self.width, self.height, self.updates_per_second
        );
        let profiles = if self.profiles.is_empty() {
            String::from("none match this shape")
        } else {
            self.profiles.join(", ")
        };
        out += &format!("Judged as seen on: {profiles}\n");
        let (flags, warnings, notes) = self.counts();
        out += &format!(
            "{flags} flagged, {warnings} warnings, {notes} notes. Reviewed in {:.1} s.\n",
            self.took
        );
        if !self.outcomes.is_empty() {
            let failed = self.failed().len();
            out += &format!(
                "\nExpectations: {} of {} passed\n",
                self.outcomes.len() - failed,
                self.outcomes.len()
            );
            for o in self
                .outcomes
                .iter()
                .filter(|o| !o.passed)
                .chain(self.outcomes.iter().filter(|o| o.passed))
            {
                if o.passed {
                    out += &format!("  pass  {}\n", o.expectation);
                } else {
                    out += &format!("  FAIL  {}: {}\n", o.expectation, o.why);
                }
            }
        }
        let shown: Vec<&Finding> = self.findings.iter().take(40).collect();
        if !shown.is_empty() {
            out += "\nWorst first:\n";
        }
        for f in &shown {
            let when = if f.end > f.frame {
                format!(
                    "{}-{}",
                    self.t(f.frame).trim_end_matches(" s"),
                    self.t(f.end)
                )
            } else {
                self.t(f.frame)
            };
            let mut line = format!(
                "  {} {when:>15}  {}: {}",
                f.severity.word(),
                f.check,
                f.text
            );
            if !f.pictures.is_empty() {
                line += &format!("  [{}]", f.pictures.join(", "));
            }
            if let Some(replay) = &f.replay {
                line += &format!("  (replay: {replay})");
            }
            out += &line;
            out += "\n";
        }
        if self.findings.len() > shown.len() {
            out += &format!(
                "  ...and {} more in review.json\n",
                self.findings.len() - shown.len()
            );
        }
        for p in &self.profile_problems {
            out += &format!("\nProfile problem: {p}\n");
        }
        let pictures: Vec<&str> = self
            .files
            .iter()
            .filter(|f| f.ends_with(".png"))
            .map(String::as_str)
            .collect();
        if !pictures.is_empty() {
            out += &format!("\nPictures: {}\n", pictures.join(", "));
        }
        out
    }

    /// `review.json`: everything, for programs.
    pub(crate) fn to_json(&self, run: &Run, rec: &frames::Recorder) -> String {
        let finding = |f: &Finding| {
            serde_json::json!({
                "severity": f.severity.word().to_lowercase(),
                "check": f.check,
                "frame": f.frame,
                "end": f.end,
                "seconds": f.frame as f64 / self.updates_per_second,
                "text": f.text,
                "pictures": f.pictures,
                "replay": f.replay,
            })
        };
        let outcomes: Vec<_> = self
            .outcomes
            .iter()
            .map(|o| serde_json::json!({"expectation": o.expectation, "passed": o.passed, "frames": o.frames, "why": o.why}))
            .collect();
        // The curves, a value every tenth of a second.
        let step = ((self.updates_per_second / 10.0).round() as usize).max(1);
        let brightness: Vec<f32> = rec
            .stats
            .iter()
            .step_by(step)
            .map(|s| (s.mean * 1000.0).round() / 1000.0)
            .collect();
        let change: Vec<f32> = rec
            .stats
            .iter()
            .step_by(step)
            .map(|s| (s.changed * 1000.0).round() / 1000.0)
            .collect();
        let loudness: Vec<f32> = run
            .audio
            .as_ref()
            .map(|a| {
                audio::loudness(&a.mono, a.rate)
                    .into_iter()
                    .map(|d| d.round())
                    .collect()
            })
            .unwrap_or_default();
        let sounds: Vec<_> = run
            .sounds
            .iter()
            .map(|s| serde_json::json!({"frame": s.frame, "name": s.name, "event": s.event, "volume": s.volume, "silent_start": s.lead}))
            .collect();
        let marks: Vec<_> = run
            .marks
            .iter()
            .map(|(f, m)| {
                let (kind, name) = match m {
                    Mark::Event(e) => ("event", e.clone()),
                    Mark::Place(p) => ("place", p.clone()),
                    Mark::Cut => ("cut", String::new()),
                    Mark::Intend(w, s) => ("intended", format!("{w} ({s} s)")),
                };
                serde_json::json!({"frame": f, "kind": kind, "name": name})
            })
            .collect();
        let value = serde_json::json!({
            "title": self.title,
            "width": self.width,
            "height": self.height,
            "updates_per_second": self.updates_per_second,
            "frames": self.frames,
            "seconds": self.frames as f64 / self.updates_per_second,
            "source": run.source,
            "seed": run.seed,
            "profiles": self.profiles,
            "profile_problems": self.profile_problems,
            "counts": {"flag": self.counts().0, "warn": self.counts().1, "note": self.counts().2},
            "expectations": outcomes,
            "findings": self.findings.iter().map(finding).collect::<Vec<_>>(),
            "marks": marks,
            "sounds": sounds,
            "peak_before_limiter": run.audio.as_ref().map(|a| a.peak),
            "curves_every_tenth_of_a_second": {"brightness": brightness, "change": change, "loudness_db": loudness},
            "files": self.files,
            "took_seconds": self.took,
        });
        serde_json::to_string_pretty(&value).unwrap_or_default() + "\n"
    }
}

/// Everything about a run (or a video) the review needs besides its
/// frames.
#[derive(Debug, Clone, Default)]
pub(crate) struct Run {
    pub(crate) title: String,
    pub(crate) ups: f64,
    /// Made by the engine as a game (truth, presses and timings known).
    pub(crate) game: bool,
    pub(crate) marks: Vec<(u64, Mark)>,
    pub(crate) expectations: Vec<Expect>,
    pub(crate) sounds: Vec<audio::SoundStart>,
    pub(crate) audio: Option<audio::Audio>,
    /// Key presses: frame and key.
    pub(crate) presses: Vec<(u64, String)>,
    /// Each update's time (ms) and keys held, from frame 1.
    pub(crate) update_ms: Vec<f32>,
    pub(crate) held: Vec<u8>,
    pub(crate) seed: Option<u32>,
    /// What was replayed, to replay each problem.
    pub(crate) recording: Option<InputScript>,
    /// What was reviewed, in words.
    pub(crate) source: String,
    pub(crate) profiles_folder: Option<PathBuf>,
}

/// Our files in a review folder (so a new review replaces an old one).
fn is_review_file(name: &str) -> bool {
    name == "review.txt"
        || name == "review.json"
        || (name.ends_with(".png")
            && [
                "overview",
                "viewer",
                "spectrogram",
                "slowest",
                "film-",
                "event-",
            ]
            .iter()
            .any(|p| name.starts_with(p)))
}

/// The same finding again later (the same check, saying the same thing:
/// a caption shown every round): one line, with every time it happens.
fn merge_repeats(findings: Vec<Finding>, ups: f64) -> Vec<Finding> {
    let mut out: Vec<(Finding, Vec<u64>)> = Vec::new();
    for f in findings {
        match out
            .iter_mut()
            .find(|(g, _)| g.severity == f.severity && g.check == f.check && g.text == f.text)
        {
            Some((_, times)) => times.push(f.frame),
            None => out.push((f, Vec::new())),
        }
    }
    out.into_iter()
        .map(|(mut f, times)| {
            if !times.is_empty() {
                let shown: Vec<String> = times
                    .iter()
                    .take(8)
                    .map(|&t| format!("{:.1}", t as f64 / ups))
                    .collect();
                let more = times.len().saturating_sub(8);
                f.text += &format!(
                    " (again at {} s{})",
                    shown.join(", "),
                    if more > 0 {
                        format!(" and {more} more times")
                    } else {
                        String::new()
                    }
                );
            }
            f
        })
        .collect()
}

/// Checks the frames and the run, draws the pictures and writes
/// `review.txt` and `review.json` in `folder`.
pub(crate) fn finish(
    rec: &frames::Recorder,
    run: &Run,
    folder: &Path,
    size: (usize, usize),
) -> Result<Review, String> {
    let started = std::time::Instant::now();
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("could not create {}: {e}", folder.display()))?;
    if let Ok(entries) = std::fs::read_dir(folder) {
        for entry in entries.flatten() {
            if is_review_file(&entry.file_name().to_string_lossy()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_dir_all(folder.join("replays"));
    let (all, profile_problems) = profiles::load(run.profiles_folder.as_deref());
    let matching = profiles::matching(&all, rec.width, rec.height, run.game);
    let (mut findings, outcomes) = checks::Checking::new(rec, run, &matching).all();
    findings.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.frame.cmp(&b.frame)));
    let mut findings = merge_repeats(findings, run.ups);
    let mut files = Vec::new();
    draw::overview(rec, run, &findings, folder, &mut files)?;
    draw::filmstrips(rec, run, &mut findings, folder, &mut files)?;
    draw::viewer(rec, run, &matching, folder, &mut files)?;
    draw::spectrogram(run, folder, &mut files)?;
    // A replay for each problem in a game.
    if let Some(recording) = &run.recording {
        let replays = folder.join("replays");
        let mut made = 0;
        for f in findings.iter_mut().filter(|f| f.severity != Severity::Note) {
            if made >= 20 {
                break;
            }
            made += 1;
            let mut script = recording.clone();
            let until = f.end + 30;
            script.events.retain(|e| e.frame <= until);
            script.frames = Some(until);
            script.expect = None;
            if made == 1 {
                std::fs::create_dir_all(&replays)
                    .map_err(|e| format!("could not create {}: {e}", replays.display()))?;
            }
            let name = format!(
                "replays/problem-{made:02}-{}.json",
                f.check.replace(' ', "-")
            );
            script
                .save(folder.join(&name))
                .map_err(|e| format!("could not save {name}: {e}"))?;
            f.replay = Some(name);
        }
    }
    files.push(String::from("review.txt"));
    files.push(String::from("review.json"));
    let mut review = Review {
        title: run.title.clone(),
        width: size.0,
        height: size.1,
        updates_per_second: run.ups,
        frames: rec.stats.len(),
        profiles: matching.iter().map(|p| p.name.clone()).collect(),
        findings,
        outcomes,
        folder: folder.to_path_buf(),
        files,
        took: 0.0,
        profile_problems: profile_problems
            .into_iter()
            .filter(|p| !p.contains("could not read the folder review_profiles"))
            .collect(),
    };
    review.took = started.elapsed().as_secs_f32();
    let write = |name: &str, text: String| {
        std::fs::write(folder.join(name), text).map_err(|e| format!("could not save {name}: {e}"))
    };
    write("review.json", review.to_json(run, rec))?;
    write("review.txt", review.to_text())?;
    Ok(review)
}

/// Reviews a video made anywhere (`duckforge review any.mp4`): from its
/// pixels and sound alone, since the engine didn't make it. Needs
/// `ffmpeg`. The review goes in `out` (by default, a folder named after
/// the video with `-review` on the end, next to it).
pub fn review_video(
    path: &Path,
    out: Option<&Path>,
    profiles: Option<&Path>,
) -> Result<Review, String> {
    external::review(path, out, profiles)
}
