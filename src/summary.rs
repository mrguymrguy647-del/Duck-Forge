//! The short summary of a headless run: what a person or an AI reads
//! first, before opening any picture or `log.json`.
//!
//! It's printed after the run and saved as `summary.txt`, at most about 15
//! lines: how the run went, the game's final reported values, the lowest
//! and highest of each number, when text values changed, how many times
//! each sound played, failed checks, warnings, and where the files are.
//! Long lists are cut short; `log.json` always has everything.
//!
//! `--quiet` prints only the problems (nothing at all if there are none),
//! and `--json` prints the same summary as one JSON object, for programs.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::engine::{FailedCheck, Game};
use crate::report::ReportValue;
use crate::simulation::Simulation;

/// At most this many final values are listed (the rest: "and N more").
const MAX_FINAL: usize = 10;
/// At most this many numbers get a lowest-highest range.
const MAX_RANGES: usize = 8;
/// At most this many lines show text (or yes/no) values' changes.
const MAX_TEXT_LINES: usize = 2;
/// Each of those lines shows at most this many changes.
const MAX_CHANGES: usize = 6;
/// At most this many failed checks and warnings are listed, together.
const MAX_PROBLEMS: usize = 3;

/// Everything the summary says, gathered once so the text, the JSON and
/// `--quiet` all say the same thing.
#[derive(Debug, Clone, Default)]
pub(crate) struct Summary {
    pub game: String,
    pub frames: u64,
    pub game_seconds: f32,
    pub run_seconds: f32,
    pub seed: u32,
    pub quit: bool,
    /// Input events replayed, and how many the script had.
    pub inputs: Option<(usize, usize)>,
    pub final_values: Vec<(String, ReportValue)>,
    /// Each number that changed: its lowest and highest value.
    pub ranges: Vec<(String, f64, f64)>,
    /// Each text or yes/no value that changed: (frame, new value) for every change.
    pub text_changes: Vec<(String, Vec<(u64, ReportValue)>)>,
    /// How many times each sound played, by name.
    pub sounds: BTreeMap<String, usize>,
    /// Tune values that didn't come from the game's default: name, value, where from.
    pub tuned: Vec<(String, f32, &'static str)>,
    /// How many different `ctx.check` rules the game has.
    pub checks: usize,
    pub failed: Vec<FailedCheck>,
    pub warnings: Vec<String>,
    /// Other things worth a line, like "wrote 3 tune values to FILE".
    pub notes: Vec<String>,
    pub folder: Option<PathBuf>,
    pub files: Vec<String>,
}

impl Summary {
    /// Gathers the summary of a finished run.
    pub(crate) fn of<G: Game>(sim: &Simulation<G>, game: &str, run_seconds: f32) -> Summary {
        let ctx = sim.context();
        let mut summary = Summary {
            game: game.to_string(),
            frames: ctx.frame(),
            game_seconds: ctx.time(),
            run_seconds,
            seed: sim.seed(),
            quit: sim.quit_requested(),
            final_values: sim
                .reports()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
            checks: ctx.checks_seen(),
            failed: sim.failed_checks().to_vec(),
            ..Summary::default()
        };
        // Every value each name had, in order, to find ranges and changes.
        let mut history: BTreeMap<&str, Vec<(u64, &ReportValue)>> = BTreeMap::new();
        for report in sim.report_log() {
            for (name, value) in &report.changes {
                history
                    .entry(name.as_str())
                    .or_default()
                    .push((report.frame, value));
            }
        }
        for (name, values) in history {
            if values.len() < 2 {
                continue; // never changed after it was first reported
            }
            let numbers: Vec<f64> = values.iter().filter_map(|(_, v)| v.as_number()).collect();
            if numbers.len() == values.len() {
                let low = numbers.iter().copied().fold(f64::INFINITY, f64::min);
                let high = numbers.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                summary.ranges.push((name.to_string(), low, high));
            } else {
                let changes = values.iter().map(|(f, v)| (*f, (*v).clone())).collect();
                summary.text_changes.push((name.to_string(), changes));
            }
        }
        for played in sim.sounds() {
            *summary.sounds.entry(played.name.clone()).or_default() += 1;
        }
        summary.tuned = sim
            .tunes()
            .used()
            .into_iter()
            .filter(|(_, _, source)| *source != crate::tune::Source::Default)
            .map(|(name, value, source)| (name, value, source.name()))
            .collect();
        summary
    }

    /// True if nothing went wrong: no failed checks and no warnings.
    pub(crate) fn ok(&self) -> bool {
        self.failed.is_empty() && self.warnings.is_empty()
    }

    /// Just the problems, one line each: what `--quiet` prints.
    pub(crate) fn problems(&self) -> Vec<String> {
        let mut lines: Vec<String> = self
            .failed
            .iter()
            .map(|check| {
                format!(
                    "CHECK FAILED: {:?} first failed on frame {}",
                    check.name, check.frame
                )
            })
            .chain(self.warnings.iter().map(|w| format!("warning: {w}")))
            .collect();
        if lines.len() > MAX_PROBLEMS {
            let more = lines.len() - MAX_PROBLEMS;
            lines.truncate(MAX_PROBLEMS);
            lines.push(format!("and {more} more problems (all in log.json)"));
        }
        lines
    }

    /// The summary as text: at most about 15 lines.
    pub(crate) fn to_text(&self) -> String {
        let mut lines = Vec::new();
        let mut first = format!(
            "duckforge: ran {:?} for {} frames ({:.1} s of game time) in {:.2} s, seed {}",
            self.game, self.frames, self.game_seconds, self.run_seconds, self.seed
        );
        if let Some((replayed, total)) = self.inputs {
            first += &format!(", replaying {replayed} of {total} input events");
        }
        if self.quit {
            first += "; the game quit by itself";
        }
        lines.push(first);
        if !self.final_values.is_empty() {
            let values: Vec<String> = self
                .final_values
                .iter()
                .map(|(name, value)| format!("{name} = {value}"))
                .collect();
            lines.push(format!("final: {}", cut(&values, MAX_FINAL)));
        }
        if !self.ranges.is_empty() {
            let ranges: Vec<String> = self
                .ranges
                .iter()
                .map(|(name, low, high)| {
                    let (low, high) = (ReportValue::from(*low), ReportValue::from(*high));
                    format!("{name} {low} to {high}")
                })
                .collect();
            lines.push(format!("lowest to highest: {}", cut(&ranges, MAX_RANGES)));
        }
        // If they don't all fit, the last line lists the rest by name.
        let shown = if self.text_changes.len() > MAX_TEXT_LINES {
            MAX_TEXT_LINES - 1
        } else {
            MAX_TEXT_LINES
        };
        for (name, changes) in self.text_changes.iter().take(shown) {
            lines.push(format!("{name}: {}", changes_text(changes)));
        }
        if self.text_changes.len() > shown {
            let more: Vec<&str> = self.text_changes[shown..]
                .iter()
                .map(|(name, _)| name.as_str())
                .collect();
            lines.push(format!("also changed: {} (see log.json)", more.join(", ")));
        }
        if !self.sounds.is_empty() {
            let sounds: Vec<String> = self
                .sounds
                .iter()
                .map(|(name, count)| format!("{name} x{count}"))
                .collect();
            lines.push(format!("sounds: {}", cut(&sounds, MAX_FINAL)));
        }
        if !self.tuned.is_empty() {
            let tuned: Vec<String> = self
                .tuned
                .iter()
                .map(|(name, value, source)| format!("{name} = {value} ({source})"))
                .collect();
            lines.push(format!("tuned: {}", cut(&tuned, MAX_FINAL)));
        }
        if self.checks > 0 {
            let failed = match self.failed.len() {
                0 => String::from("none failed"),
                1 => String::from("1 failed"),
                n => format!("{n} failed"),
            };
            let rules = if self.checks == 1 { "rule" } else { "rules" };
            lines.push(format!("checks: {} {rules}, {failed}", self.checks));
        }
        lines.extend(self.problems());
        lines.extend(self.notes.iter().cloned());
        if let Some(folder) = &self.folder {
            lines.push(format!(
                "saved in {}: {}",
                folder.display(),
                files_text(&self.files)
            ));
            if self.files.iter().any(|f| f == "run.gif")
                && self.files.iter().any(|f| f == "audio.wav")
            {
                lines.push(format!(
                    "watch with sound: ffmpeg -y -i {gif} -i {wav} -c:v libx264 -pix_fmt yuv420p -vf \"scale=trunc(iw/2)*2:trunc(ih/2)*2\" -c:a aac -shortest {mp4}",
                    gif = folder.join("run.gif").display(),
                    wav = folder.join("audio.wav").display(),
                    mp4 = folder.join("run.mp4").display(),
                ));
            }
        }
        lines.join("\n") + "\n"
    }

    /// The summary as one JSON object, with nothing cut short.
    pub(crate) fn to_json(&self) -> Value {
        let mut final_values = Map::new();
        for (name, value) in &self.final_values {
            final_values.insert(name.clone(), value.to_json());
        }
        let mut ranges = Map::new();
        for (name, low, high) in &self.ranges {
            ranges.insert(
                name.clone(),
                json!([
                    ReportValue::from(*low).to_json(),
                    ReportValue::from(*high).to_json()
                ]),
            );
        }
        let mut changes = Map::new();
        for (name, list) in &self.text_changes {
            let list: Vec<Value> = list.iter().map(|(f, v)| json!([f, v.to_json()])).collect();
            changes.insert(name.clone(), Value::from(list));
        }
        let mut tuned = Map::new();
        for (name, value, source) in &self.tuned {
            tuned.insert(
                name.clone(),
                json!({"value": ReportValue::from(*value).to_json(), "from": source}),
            );
        }
        let failed: Vec<Value> = self
            .failed
            .iter()
            .map(|f| json!({"name": f.name, "frame": f.frame}))
            .collect();
        json!({
            "ok": self.ok(),
            "game": self.game,
            "frames": self.frames,
            "game_seconds": (f64::from(self.game_seconds) * 100.0).round() / 100.0,
            "run_seconds": (f64::from(self.run_seconds) * 100.0).round() / 100.0,
            "seed": self.seed,
            "game_quit": self.quit,
            "inputs": self.inputs.map(|(replayed, total)| json!({"replayed": replayed, "events": total})),
            "final": final_values,
            "ranges": ranges,
            "text_changes": changes,
            "sounds": self.sounds,
            "tuned": tuned,
            "checks": {"rules": self.checks, "failed": failed},
            "warnings": self.warnings,
            "notes": self.notes,
            "folder": self.folder.as_ref().map(|f| f.display().to_string()),
            "files": self.files,
        })
    }
}

/// `a, b, c and 4 more`.
fn cut(items: &[String], keep: usize) -> String {
    if items.len() <= keep {
        items.join(", ")
    } else {
        format!(
            "{}, and {} more (see log.json)",
            items[..keep].join(", "),
            items.len() - keep
        )
    }
}

/// `"Title" on frame 1, "Playing" on 95, ... "Over" on 900 (14 changes)`.
fn changes_text(changes: &[(u64, ReportValue)]) -> String {
    let one = |(frame, value): &(u64, ReportValue)| format!("{value} on {frame}");
    let mut text = format!("{} on frame {}", changes[0].1, changes[0].0);
    if changes.len() <= MAX_CHANGES {
        for change in &changes[1..] {
            text += &format!(", {}", one(change));
        }
    } else {
        for change in &changes[1..3] {
            text += &format!(", {}", one(change));
        }
        text += ", ...";
        for change in &changes[changes.len() - 2..] {
            text += &format!(", {}", one(change));
        }
        text += &format!(" ({} changes, all in log.json)", changes.len());
    }
    text
}

/// The saved files, with the numbered ones counted instead of listed.
fn files_text(files: &[String]) -> String {
    let snapshots = files
        .iter()
        .filter(|name| name.starts_with("frame-") && name.ends_with(".png"))
        .count();
    let mut listed: Vec<String> = files
        .iter()
        .filter(|name| !name.starts_with("frame-") && !name.starts_with("closeup-0"))
        .cloned()
        .collect();
    if snapshots > 0 {
        let s = if snapshots == 1 { "" } else { "s" };
        listed.push(format!(
            "{snapshots} snapshot{s} (frame-NNNNNN.png and .scene.json)"
        ));
    }
    listed.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn busy() -> Summary {
        let many = |prefix: &str, n: usize| -> Vec<String> {
            (0..n).map(|i| format!("{prefix} {i}")).collect()
        };
        Summary {
            game: String::from("Busy"),
            frames: 600,
            game_seconds: 10.0,
            run_seconds: 0.25,
            seed: 7,
            quit: true,
            inputs: Some((40, 41)),
            final_values: (0..30)
                .map(|i| (format!("value{i}"), ReportValue::from(i)))
                .collect(),
            ranges: (0..30).map(|i| (format!("value{i}"), 0.0, 9.5)).collect(),
            text_changes: (0..5)
                .map(|i| {
                    let changes = (0..20)
                        .map(|f| (f * 10 + 1, ReportValue::from(format!("S{f}"))))
                        .collect();
                    (format!("state{i}"), changes)
                })
                .collect(),
            sounds: (0..30).map(|i| (format!("sound{i}"), 2)).collect(),
            tuned: vec![(String::from("gravity"), 900.0, "file")],
            checks: 9,
            failed: (0..9)
                .map(|i| FailedCheck {
                    name: format!("rule {i}"),
                    frame: i,
                })
                .collect(),
            warnings: many("something is off", 9),
            notes: vec![String::from("wrote 3 tune values to x.tune.json")],
            folder: Some(PathBuf::from("shots")),
            files: vec![
                String::from("run.gif"),
                String::from("frame-000060.png"),
                String::from("frame-000060.scene.json"),
                String::from("last.png"),
                String::from("audio.wav"),
                String::from("log.json"),
                String::from("summary.txt"),
            ],
        }
    }

    #[test]
    fn even_a_busy_run_is_summed_up_in_about_15_lines() {
        let text = busy().to_text();
        let lines = text.lines().count();
        assert!(lines <= 15, "{lines} lines:\n{text}");
        assert!(text.contains("CHECK FAILED: \"rule 0\" first failed on frame 0"));
        assert!(text.contains("and 15 more problems (all in log.json)"));
        assert!(text.contains("and 20 more (see log.json)"));
        assert!(text.contains("(20 changes, all in log.json)"));
        assert!(text.contains("also changed: state1, state2, state3, state4"));
        assert!(text.contains(", 1 snapshot (frame-NNNNNN.png and .scene.json)"));
        assert!(text.contains("seed 7, replaying 40 of 41 input events; the game quit"));
        assert!(text.contains("watch with sound: ffmpeg"));
        assert!(text.contains("wrote 3 tune values"));
        // Every line is short enough to read, except the ffmpeg command.
        for line in text.lines().filter(|l| !l.contains("ffmpeg")) {
            assert!(line.len() < 300, "too long: {line}");
        }
    }

    #[test]
    fn quiet_is_only_the_problems() {
        let mut summary = busy();
        assert_eq!(summary.problems().len(), 4);
        summary.failed.truncate(1);
        summary.warnings.truncate(1);
        assert_eq!(summary.problems().len(), 2);
        assert!(!summary.ok());
        summary.failed.clear();
        summary.warnings.clear();
        assert!(summary.problems().is_empty());
        assert!(summary.ok());
    }

    #[test]
    fn json_has_everything_uncut() {
        let value = busy().to_json();
        assert_eq!(value["ok"], false);
        assert_eq!(value["final"].as_object().unwrap().len(), 30);
        assert_eq!(value["ranges"]["value3"], json!([0, 9.5]));
        assert_eq!(value["text_changes"]["state0"][1], json!([11, "S1"]));
        assert_eq!(value["sounds"]["sound0"], 2);
        assert_eq!(
            value["tuned"]["gravity"],
            json!({"value": 900, "from": "file"})
        );
        assert_eq!(
            value["checks"]["failed"][8],
            json!({"name": "rule 8", "frame": 8})
        );
        assert_eq!(value["inputs"], json!({"replayed": 40, "events": 41}));
        assert_eq!(value["folder"], "shots");
        // One line, so a program can read it with one read.
        assert!(!value.to_string().contains('\n'));
    }

    #[test]
    fn a_quiet_little_run_is_a_few_lines() {
        let summary = Summary {
            game: String::from("Hello"),
            frames: 60,
            game_seconds: 1.0,
            ..Summary::default()
        };
        assert_eq!(
            summary.to_text(),
            "duckforge: ran \"Hello\" for 60 frames (1.0 s of game time) in 0.00 s, seed 0\n"
        );
    }
}
