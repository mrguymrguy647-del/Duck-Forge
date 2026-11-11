//! Check: the whole check for one game, as a friendly list.
//!
//! Check runs `tools/verify`, the same command a person or an agent runs
//! in a terminal, and turns its short report into sentences. Each line
//! keeps verify's own words and details, and the picture verify saved for
//! it, so clicking a problem shows what happened.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::console::{Console, JobEvent, JobId, Keep};
use duckforge::prelude::LetterKind;

use crate::folder::{Card, Game, GamesFolder};
use crate::level::{Level, LevelArt};
use crate::problem::Problem;

/// How many random runs a Check plays (verify's own default).
pub const FUZZ_RUNS: u32 = 50;

/// How one line of the check went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowStatus {
    Ok,
    Skipped,
    /// Nothing broke, but something changed that's worth a look.
    Warning,
    Failed,
}

/// One line of the check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckRow {
    /// Verify's name for the step: `build`, `play tests`, `dodge.json`,
    /// `fuzz 50 runs`.
    pub step: String,
    pub status: RowStatus,
    /// The line as the Studio says it.
    pub say: String,
    /// Verify's own words for it.
    pub text: String,
    /// The lines verify printed under it.
    pub details: Vec<String>,
    /// A picture that shows it, if verify saved one.
    pub picture: Option<PathBuf>,
    /// Something the Studio can do about it, as a button.
    #[serde(default)]
    pub action: Option<RowAction>,
}

/// What a check line's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowAction {
    /// Have the game's bot record its playthrough again, on the level as
    /// it is now (or for the first time).
    RecordAgain,
    /// No button: the line's own text says what to do.
    Explained,
}

/// A finished check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub game: String,
    pub rows: Vec<CheckRow>,
    /// No line failed (warnings don't count).
    pub passed: bool,
    pub problems: u32,
    pub warnings: u32,
    /// When it finished, in seconds since 1970.
    pub at: u64,
}

/// A check that's running, or done.
#[derive(Debug)]
pub struct Check {
    pub game: String,
    pub job: JobId,
    pub result: Option<Result<CheckResult, Problem>>,
    lines: Vec<String>,
}

impl Check {
    pub fn start(console: &mut Console, folder: &GamesFolder, game: &str) -> Check {
        let command = folder.check_command(game, FUZZ_RUNS);
        Check {
            game: game.to_string(),
            job: console.start("Check", &command, Keep::Everything),
            result: None,
            lines: Vec::new(),
        }
    }

    /// The step the check is on now, in words, while it runs.
    pub fn doing(&self) -> &'static str {
        let seen = |step: &str| {
            self.lines
                .iter()
                .any(|line| line.trim_start().starts_with(step))
        };
        if seen("fuzz") {
            "Finishing"
        } else if seen("playthroughs") || self.lines.iter().any(|l| l.contains(".json ")) {
            "Playing random runs"
        } else if seen("play tests") {
            "Replaying the recorded playthroughs"
        } else if seen("build") {
            "Running the play tests"
        } else {
            "Building"
        }
    }

    /// Takes in one thing the check's job did. Returns true when the check
    /// has just finished.
    pub fn take(&mut self, event: &JobEvent, folder: &GamesFolder) -> bool {
        match event {
            JobEvent::Line(_, line) => {
                self.lines.push(line.clone());
                false
            }
            JobEvent::Ended(_) => {
                let result = read_report(&self.lines, folder, &self.game);
                self.result = Some(if result.rows.is_empty() {
                    Err(Problem::new(
                        "The check couldn't run.",
                        "Look at the details, or copy them for an agent.",
                    )
                    .with_details(self.lines.join("\n")))
                } else {
                    let _ = save(folder, &result);
                    Ok(result)
                });
                true
            }
            JobEvent::CouldntStart(why) => {
                self.result = Some(Err(Problem::new(
                    "The check couldn't start.",
                    "Check that the Duck Forge folder still has its tools folder.",
                )
                .with_details(why.clone())));
                true
            }
        }
    }
}

/// The words verify uses for how a step went.
const STATUSES: [(&str, RowStatus); 5] = [
    (" ok: ", RowStatus::Ok),
    (" skipped: ", RowStatus::Skipped),
    (" FAILED: ", RowStatus::Failed),
    (" changed: ", RowStatus::Warning),
    (" tuned: ", RowStatus::Warning),
];

/// Reads verify's report into the check's lines.
pub fn read_report(lines: &[String], folder: &GamesFolder, game: &str) -> CheckResult {
    let mut rows: Vec<CheckRow> = Vec::new();
    for line in lines {
        let is_step = line.starts_with("  ") && !line.starts_with("   ");
        let found = STATUSES
            .iter()
            .filter_map(|(word, status)| line.find(word).map(|at| (at, *word, *status)))
            .min_by_key(|(at, _, _)| *at);
        match (is_step, found) {
            (true, Some((at, word, status))) => {
                let step = line[..at].trim().to_string();
                let text = line[at + word.len()..].trim().to_string();
                rows.push(row(folder, game, step, status, text));
            }
            _ if line.starts_with("      ") => {
                if let Some(last) = rows.last_mut() {
                    last.details.push(line.trim().to_string());
                }
            }
            _ => {}
        }
    }
    for row in &mut rows {
        say_it_plainly(row, folder, game);
    }
    let problems = rows
        .iter()
        .filter(|r| r.status == RowStatus::Failed)
        .count() as u32;
    let warnings = rows
        .iter()
        .filter(|r| r.status == RowStatus::Warning)
        .count() as u32;
    CheckResult {
        game: game.to_string(),
        passed: problems == 0 && !rows.is_empty(),
        problems,
        warnings,
        rows,
        at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    }
}

/// Once a line has its details: the cases worth their own plain words.
///
/// - The starters' "the bot can still win" tests say why the bot can't:
///   the Feel values as they are now, one of the Feel presets, or the
///   level. That sentence becomes the line, not "some play tests fail".
/// - No playthrough recorded yet (a new game whose bot couldn't win when
///   it was made, say): a line that offers to record one.
fn say_it_plainly(row: &mut CheckRow, folder: &GamesFolder, game: &str) {
    if row.step == "play tests" && row.status == RowStatus::Failed {
        let cases = [
            (
                "With the Feel values as they are now, ",
                "The bot can't win with the Feel values as they are now",
            ),
            (
                "With the Feel preset ",
                "The bot can't win with one of the Feel presets",
            ),
            (
                "On the level as it is now, ",
                "The bot can't win on the level as it is now",
            ),
        ];
        for (start, say) in cases {
            if let Some(why) = row.details.iter().find(|line| line.starts_with(start)) {
                row.say = say.to_string();
                row.text = why.clone();
                row.action = Some(RowAction::Explained);
                return;
            }
        }
    }
    if row.step == "playthroughs" {
        row.say = String::from("No playthrough recorded yet");
        row.status = RowStatus::Warning;
        let bot = folder
            .root()
            .join("examples")
            .join(game)
            .join("bot.rs")
            .is_file();
        if bot {
            row.text = String::from(
                "Record one: the game's bot plays the level as it is now, and that's kept so \
                 Check can replay it every time.",
            );
            row.action = Some(RowAction::RecordAgain);
        } else {
            row.text = String::from(
                "Play the game and press F10 to record one, so Check can replay it every time.",
            );
            row.action = Some(RowAction::Explained);
        }
    }
}

/// One line of the check, said the Studio's way.
fn row(
    folder: &GamesFolder,
    game: &str,
    step: String,
    status: RowStatus,
    text: String,
) -> CheckRow {
    let check_folder = folder.check_folder(game);
    let first_number = |text: &str| -> Option<u32> {
        text.split(|c: char| !c.is_ascii_digit())
            .find(|word| !word.is_empty())?
            .parse()
            .ok()
    };
    let mut picture = None;
    let say = if step == "build" {
        match status {
            RowStatus::Failed => String::from("It doesn't build"),
            _ => String::from("It builds"),
        }
    } else if step == "play tests" {
        match (status, first_number(&text)) {
            (RowStatus::Failed, _) => String::from("Some of its play tests fail"),
            (_, Some(1)) => String::from("Its 1 play test passes"),
            (_, Some(count)) if count > 0 => format!("Its {count} play tests pass"),
            _ => String::from("It has no play tests yet"),
        }
    } else if step == "playthroughs" {
        String::from("It has no recorded playthrough yet")
    } else if let Some(name) = step.strip_suffix(".json") {
        let run = check_folder.join(name);
        picture = [run.join("sheet.png"), run.join("last.png")]
            .into_iter()
            .find(|picture| picture.is_file());
        match status {
            RowStatus::Failed => format!("The recorded playthrough \"{name}\" hits a problem"),
            RowStatus::Warning => {
                format!("The recorded playthrough \"{name}\" has changed: {text}")
            }
            _ => format!("The recorded playthrough \"{name}\" plays out as before"),
        }
    } else if step.starts_with("fuzz") {
        let runs = first_number(&step).unwrap_or(0);
        match status {
            RowStatus::Failed => match first_number(&text) {
                Some(1) => format!("{runs} random runs found 1 problem"),
                Some(count) => format!("{runs} random runs found {count} different problems"),
                None => format!("{runs} random runs found problems"),
            },
            RowStatus::Skipped => String::from("Random runs were skipped"),
            _ => format!("{runs} random runs: no crashes, no failed checks, no freezes"),
        }
    } else {
        format!("{step}: {text}")
    };
    CheckRow {
        step,
        status,
        say,
        text,
        details: Vec::new(),
        picture,
        action: None,
    }
}

/// The Studio's own lines about a game's level, after verify's: the
/// legend's rules it breaks (problems), things that look out of reach
/// (warnings, with a picture of the spot: the reckoning is cautious), and
/// recorded playthroughs made on an older version of it (warnings: they
/// still replay on the level they were made on, so they still pass).
pub fn level_rows(
    folder: &GamesFolder,
    game: &str,
    level: &Level,
    jump: Option<duckforge::prelude::Jump>,
) -> Vec<CheckRow> {
    let mut rows = Vec::new();
    let row = |step: &str, status, say: String| CheckRow {
        step: step.to_string(),
        status,
        say,
        text: String::new(),
        details: Vec::new(),
        picture: None,
        action: None,
    };
    for rule in level.broken_rules() {
        rows.push(row("level rules", RowStatus::Failed, rule));
    }
    if let (Some(jump), Ok(map)) = (jump, level.map()) {
        let start = map.spawns("player").first().copied();
        if let Some(start) = start {
            let reach = map.reach(start, jump);
            let art = LevelArt::load(level);
            for letter in level
                .letters
                .iter()
                .filter(|l| l.kind == LetterKind::Thing && !l.start)
            {
                for (index, at) in reach
                    .missing(&map.spawns(&letter.name))
                    .into_iter()
                    .enumerate()
                {
                    let cell = map.cell_at(duckforge::prelude::vec2(at.x, at.y - 1.0));
                    let mut line = row(
                        "level reach",
                        RowStatus::Warning,
                        format!(
                            "A {} looks out of reach (column {}, row {})",
                            letter.title().to_lowercase(),
                            cell.0,
                            cell.1
                        ),
                    );
                    line.text = String::from(
                        "Worked out from how high and far the duck jumps, cautiously: the bot may \
                         still get there. What to try: move a plank closer, or lower it.",
                    );
                    let picture = folder
                        .check_folder(game)
                        .join(format!("reach-{}-{index}.png", letter.name));
                    if art.save_spot(level, cell, &picture).is_ok() {
                        line.picture = Some(picture);
                    }
                    rows.push(line);
                }
            }
        }
    }
    // Line endings don't count: a Windows checkout has `\r\n`.
    let now = crate::play::lines_end_with_newline(&level.text());
    for playthrough in folder.playthroughs(game) {
        let Ok(script) = duckforge::InputScript::load(&playthrough) else {
            continue;
        };
        let recorded_on = script
            .files
            .iter()
            .find(|(name, _)| name.ends_with(&level.name) || level.name.ends_with(name.as_str()));
        if let Some((_, text)) = recorded_on
            && crate::play::lines_end_with_newline(text).trim_end() != now.trim_end()
        {
            let name = playthrough
                .file_stem()
                .map_or(String::new(), |stem| stem.to_string_lossy().into_owned());
            let mut line = row(
                &format!("{name}.json older level"),
                RowStatus::Warning,
                format!(
                    "The recorded playthrough \"{name}\" is from an older level. It still replays \
                     on that one."
                ),
            );
            line.text = String::from("Record it again to play the level as it is now.");
            line.action = Some(RowAction::RecordAgain);
            rows.push(line);
        }
    }
    rows
}

/// Adds the level's lines to a finished check, and counts them in.
pub fn add_level_rows(result: &mut CheckResult, rows: Vec<CheckRow>) {
    result
        .rows
        .retain(|row| !row.step.starts_with("level ") && !row.step.ends_with(" older level"));
    for row in rows {
        match row.status {
            RowStatus::Failed => {
                result.problems += 1;
                result.passed = false;
            }
            RowStatus::Warning => result.warnings += 1,
            _ => {}
        }
        result.rows.push(row);
    }
}

fn check_file(folder: &GamesFolder, game: &str) -> PathBuf {
    folder.check_folder(game).join("check.json")
}

/// Saves a check's result next to what verify saved, as `check.json`, so
/// Home's cards (and anyone else) can read how the last check went.
pub fn save(folder: &GamesFolder, result: &CheckResult) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(result).map_err(std::io::Error::other)?;
    std::fs::create_dir_all(folder.check_folder(&result.game))?;
    std::fs::write(check_file(folder, &result.game), text)
}

/// The last check of `game`, if there was one.
pub fn load(folder: &GamesFolder, game: &str) -> Option<CheckResult> {
    let text = std::fs::read_to_string(check_file(folder, game)).ok()?;
    serde_json::from_str(&text).ok()
}

/// What a game's card says: "Checks pass" only if the last check passed
/// and nothing has changed since.
pub fn card(folder: &GamesFolder, game: &Game) -> Card {
    let Some(last) = load(folder, &game.name) else {
        return Card::NeedsALook(String::from("never checked"));
    };
    if !last.passed {
        return Card::NeedsALook(match last.problems {
            1 => String::from("1 problem"),
            count => format!("{count} problems"),
        });
    }
    let changed = folder.last_changed(game).is_some_and(|changed| {
        changed
            .duration_since(UNIX_EPOCH)
            .is_ok_and(|since| since.as_secs() > last.at)
    });
    if changed {
        Card::NeedsALook(String::from("changed since the last check"))
    } else {
        Card::ChecksPass
    }
}

/// The seeds of the random runs that found problems, from a fuzz line's
/// details (`seed 17: ...`). Each has a replay script in the check
/// folder's `fuzz/seed-17.json`.
pub fn problem_seeds(row: &CheckRow) -> Vec<u32> {
    row.details
        .iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("seed ")?;
            rest.split(|c: char| !c.is_ascii_digit())
                .next()?
                .parse()
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::tests::duck_forge;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    #[test]
    fn a_clean_report_becomes_friendly_lines() {
        let report = lines(
            "verify duck_peaks\n\
             \x20 build                        ok: built without a window\n\
             \x20 play tests                   ok: 9 passed; 0 failed; 1 ignored\n\
             \x20 duck_peaks.json              ok: 938 frames; coins=12, state=Won\n\
             \x20 fuzz 50 runs                 ok: no problems found\n\
             \x20     coverage: coins 0 to 2\n\
             VERIFY OK: duck_peaks in 3 s (pictures and summaries in out/verify/duck_peaks)",
        );
        let result = read_report(&report, &duck_forge(), "duck_peaks");
        let said: Vec<&str> = result.rows.iter().map(|row| row.say.as_str()).collect();
        assert_eq!(
            said,
            [
                "It builds",
                "Its 9 play tests pass",
                "The recorded playthrough \"duck_peaks\" plays out as before",
                "50 random runs: no crashes, no failed checks, no freezes",
            ]
        );
        assert!(result.passed);
        assert_eq!((result.problems, result.warnings), (0, 0));
        assert_eq!(result.rows[3].details, ["coverage: coins 0 to 2"]);
        assert_eq!(result.rows[2].text, "938 frames; coins=12, state=Won");
    }

    #[test]
    fn problems_and_warnings_keep_verifys_details() {
        let report = lines(
            "verify bee_maze\n\
             \x20 build                        ok: built without a window\n\
             \x20 play tests                   FAILED: 7 passed; 2 failed; 0 ignored; all in out/verify/bee_maze/tests.txt\n\
             \x20     thread 'tests::the_wall_stops_the_duck' panicked at examples/bee_maze/tests.rs:84:5:\n\
             \x20     assertion `left == right` failed\n\
             \x20 bee_maze.json                changed: coins 8 -> 7\n\
             \x20     if that's right: cargo run ... --accept\n\
             \x20 fuzz 50 runs                 FAILED: 2 different problem(s); replay scripts in out/verify/bee_maze/fuzz\n\
             \x20     seed 17: crash on frame 233: index out of bounds\n\
             \x20     seed 31 (and 4 more): freeze from frame 900\n\
             VERIFY FAILED: 2 of the steps above for bee_maze (everything in out/verify/bee_maze)",
        );
        let result = read_report(&report, &duck_forge(), "bee_maze");
        assert!(!result.passed);
        assert_eq!((result.problems, result.warnings), (2, 1));
        let tests = &result.rows[1];
        assert_eq!(tests.say, "Some of its play tests fail");

        // The bot's own sentence becomes the line; and a game with no
        // playthrough yet says so, rather than a test failing to read it.
        let feel = read_report(
            &lines(
                "  build                        ok: built without a window\n\
                 \x20 play tests                   FAILED: 9 passed; 1 failed; 1 ignored; all in out/verify/template/tests.txt\n\
                 \x20     thread 'tests::the_bot_can_still_win_on_the_level' panicked at examples/template/tests.rs:171:13:\n\
                 \x20     With the Feel values as they are now, the bot can't get from column 31, row 9 to the coin at column 34, row 6 (its jump is 3.0 tiles high and 5.2 tiles long). With the values in the code the bot wins: change Feel back (the preset \"As it was made\"), or change the level to suit the new Feel.\n\
                 \x20 playthroughs                 ok: none (save one as playthroughs/template.json)\n",
            ),
            &duck_forge(),
            "template",
        );
        let bot = &feel.rows[1];
        assert_eq!(
            bot.say,
            "The bot can't win with the Feel values as they are now"
        );
        assert!(
            bot.text
                .starts_with("With the Feel values as they are now, the bot can't get")
        );
        let none = &feel.rows[2];
        assert_eq!(none.say, "No playthrough recorded yet");
        assert_eq!(none.status, RowStatus::Warning);
        assert_eq!(none.action, Some(RowAction::RecordAgain), "it has a bot");
        assert_eq!(tests.details.len(), 2);
        assert!(tests.details[0].contains("tests.rs:84"));
        let playthrough = &result.rows[2];
        assert_eq!(playthrough.status, RowStatus::Warning);
        assert_eq!(
            playthrough.say,
            "The recorded playthrough \"bee_maze\" has changed: coins 8 -> 7"
        );
        let fuzz = &result.rows[3];
        assert_eq!(fuzz.say, "50 random runs found 2 different problems");
        assert_eq!(problem_seeds(fuzz), [17, 31]);
    }

    #[test]
    fn a_build_that_fails_stops_there() {
        let report = lines(
            "verify bee_maze\n\
             \x20 build                        FAILED: see out/verify/bee_maze/build.txt\n\
             \x20     error[E0425]: cannot find value `x` in this scope\n\
             VERIFY FAILED: bee_maze does not build",
        );
        let result = read_report(&report, &duck_forge(), "bee_maze");
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].say, "It doesn't build");
        assert!(!result.passed);
        // Nothing readable at all isn't "passed" either.
        assert!(
            !read_report(
                &lines("usage: tools/verify.sh <example>"),
                &duck_forge(),
                "x"
            )
            .passed
        );
    }
}
