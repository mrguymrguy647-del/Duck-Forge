//! Running the Studio with no window: a list of commands from a file.
//!
//! A script is a JSON list. Each item is a command (the same ones the
//! buttons do, see [`Command`]) or a wait:
//!
//! ```json
//! [
//!   {"do": "game.open", "name": "duck_peaks"},
//!   {"wait": "playing"},
//!   {"wait": "frames", "frames": 120},
//!   {"do": "game.toggle"},
//!   {"do": "check.run"},
//!   {"wait": "idle"}
//! ]
//! ```
//!
//! While it waits, the Studio runs just as it does with a window: sixty
//! times a second it takes in what's new and sends the game its tick.

use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::play::PlayState;
use crate::problem::Problem;
use crate::studio::{Command, Studio};

/// One line of a script.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Step {
    Wait(Wait),
    Do(Command),
}

/// Something to wait for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "wait", rename_all = "snake_case")]
pub enum Wait {
    /// Until the open game is playing (built and started).
    Playing,
    /// Until the game is frozen (a replay has caught up, say).
    Frozen,
    /// Until the game has played this many more frames.
    Frames { frames: u64 },
    /// For this many seconds.
    Seconds { seconds: f32 },
    /// Until nothing the Studio started is still running (a build, a
    /// check, a clip, a new game being made).
    Idle,
}

/// How long a wait may take before the script gives up.
const PATIENCE: Duration = Duration::from_secs(900);

/// One of the Studio's frames, as the app does it: take in what's new,
/// send the tick, wait for the picture, then rest until the next frame.
pub fn frame(studio: &mut Studio, started: Instant) {
    studio.begin_frame(started, &[]);
    studio.wait_for_picture(crate::play::PATIENCE);
    let spent = started.elapsed();
    if let Some(rest) = Duration::from_micros(16_667).checked_sub(spent) {
        std::thread::sleep(rest);
    }
}

/// Runs frames until `done` says so.
fn wait_until(
    studio: &mut Studio,
    what: &str,
    mut done: impl FnMut(&Studio) -> bool,
) -> Result<(), Problem> {
    let started = Instant::now();
    loop {
        frame(studio, Instant::now());
        if let Some(problem) = stopped_by_a_problem(studio) {
            return Err(problem);
        }
        if done(studio) {
            return Ok(());
        }
        if started.elapsed() > PATIENCE {
            return Err(Problem::new(
                format!("The script waited fifteen minutes for {what}, and it never came."),
                "Look at the Console's last lines to see what the Studio was doing.",
            ));
        }
    }
}

/// A problem that means what's being waited for will never come: the
/// game stopped, its build failed, or the Studio has something to say
/// (a new game couldn't be made, say). With nobody there to read a
/// dialog, a script stops and says it.
fn stopped_by_a_problem(studio: &Studio) -> Option<Problem> {
    if let Some(problem) = studio.crashed() {
        return Some(problem.clone());
    }
    if let Some(problem) = &studio.problem {
        return Some(problem.clone());
    }
    studio.open.as_ref()?.build_problem().cloned()
}

/// Does one step of a script. Says what happened, in a few words.
pub fn run_step(studio: &mut Studio, step: &Step) -> Result<String, Problem> {
    match step {
        Step::Do(command) => studio.run(command.clone()),
        Step::Wait(Wait::Playing) => {
            wait_until(studio, "the game to start playing", |studio| {
                studio
                    .play()
                    .is_some_and(|play| play.state == PlayState::Playing && play.picture.is_some())
            })?;
            Ok(String::from("The game is playing"))
        }
        Step::Wait(Wait::Frozen) => {
            wait_until(studio, "the game to be frozen", |studio| {
                studio
                    .play()
                    .is_some_and(|play| play.state == PlayState::Frozen && play.picture.is_some())
            })?;
            Ok(String::from("The game is frozen"))
        }
        Step::Wait(Wait::Frames { frames }) => {
            let from = studio.play().map_or(0, |play| play.frame);
            wait_until(studio, "the game to play on", |studio| {
                studio
                    .play()
                    .is_some_and(|play| play.frame >= from + frames)
            })?;
            Ok(format!("Played {frames} frames"))
        }
        Step::Wait(Wait::Seconds { seconds }) => {
            let until = Instant::now() + Duration::from_secs_f32(seconds.max(0.0));
            while Instant::now() < until {
                frame(studio, Instant::now());
            }
            Ok(format!("Waited {seconds} seconds"))
        }
        Step::Wait(Wait::Idle) => {
            // A frame first, so a command just started counts as running.
            frame(studio, Instant::now());
            wait_until(studio, "the Studio to finish what it started", |studio| {
                !studio.is_busy() && studio.making.is_none()
            })?;
            Ok(String::from("Nothing is running"))
        }
    }
}

/// Runs a whole script (JSON text). Returns what each step said, or the
/// first problem, with the number of the step it happened on.
pub fn run_script(studio: &mut Studio, text: &str) -> Result<Vec<String>, Problem> {
    let steps: Vec<Step> = serde_json::from_str(text).map_err(|problem| {
        Problem::new(
            "The script can't be read.",
            "A script is a JSON list of commands like {\"do\": \"game.open\", \"name\": \
             \"dodge\"} and waits like {\"wait\": \"idle\"}.",
        )
        .with_details(problem.to_string())
    })?;
    let mut said = Vec::new();
    for (number, step) in steps.iter().enumerate() {
        match run_step(studio, step) {
            Ok(line) => said.push(line),
            Err(mut problem) => {
                problem.what = format!("Step {} of the script: {}", number + 1, problem.what);
                return Err(problem);
            }
        }
    }
    Ok(said)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_is_commands_and_waits() {
        let steps: Vec<Step> = serde_json::from_str(
            r#"[
                {"do": "game.open", "name": "duck_peaks"},
                {"wait": "playing"},
                {"wait": "frames", "frames": 120},
                {"do": "game.toggle"},
                {"wait": "seconds", "seconds": 0.5},
                {"wait": "idle"}
            ]"#,
        )
        .unwrap();
        assert_eq!(steps.len(), 6);
        assert_eq!(steps[1], Step::Wait(Wait::Playing));
        assert_eq!(steps[2], Step::Wait(Wait::Frames { frames: 120 }));
        assert_eq!(steps[3], Step::Do(Command::GameToggle));
        assert!(serde_json::from_str::<Vec<Step>>(r#"[{"do": "fly"}]"#).is_err());
    }
}
