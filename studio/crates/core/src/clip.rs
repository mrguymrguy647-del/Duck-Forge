//! Clip: a ten-second video of the last run.
//!
//! Every session is saved as an input script, and a replay is exact. So a
//! clip is made by replaying the session with no window and keeping its
//! last ten seconds (`--clip`, the engine's own option): a video with
//! sound when `ffmpeg` is installed, or else a full-size GIF with a line
//! saying where to get `ffmpeg`.

use std::path::{Path, PathBuf};

use crate::console::{CommandLine, Console, JobEvent, JobId, Keep};
use crate::folder::GamesFolder;
use crate::problem::Problem;

/// The official page for `ffmpeg`, offered when a clip has no sound.
pub const FFMPEG_PAGE: &str = "https://ffmpeg.org/download.html";

/// A finished clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipResult {
    pub file: PathBuf,
    pub with_sound: bool,
    /// The engine's own line about it.
    pub note: String,
    /// It's a GIF because `ffmpeg` wasn't found: offer its page.
    pub needs_ffmpeg: bool,
}

/// A clip being made, or made.
#[derive(Debug)]
pub struct Clip {
    pub game: String,
    pub job: JobId,
    pub result: Option<Result<ClipResult, Problem>>,
    wanted: PathBuf,
    lines: Vec<String>,
}

impl Clip {
    /// Starts making a clip of `session` (an input script).
    pub fn start(
        console: &mut Console,
        folder: &GamesFolder,
        cargo: &Path,
        game: &str,
        session: &Path,
    ) -> Clip {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let root = folder.root();
        let short = |path: &Path| -> String {
            path.strip_prefix(root)
                .unwrap_or(path)
                .display()
                .to_string()
                .replace('\\', "/")
        };
        let wanted = folder.clips_folder().join(format!("{game}-{stamp}.mp4"));
        let work = folder.clips_folder().join(format!("{game}-work"));
        let (session, clip, out) = (short(session), short(&wanted), short(&work));
        let command = CommandLine::new(
            cargo,
            &[
                "run",
                "-q",
                "--no-default-features",
                "--example",
                game,
                "--",
                "--headless",
                "--inputs",
                &session,
                "--clip",
                &clip,
                "--out",
                &out,
            ],
            root,
        )
        .shown_as(format!(
            "cargo run -q --no-default-features --example {game} -- --headless --inputs \
             {session} --clip {clip} --out {out}"
        ));
        Clip {
            game: game.to_string(),
            job: console.start("Clip", &command, Keep::Everything),
            result: None,
            wanted,
            lines: Vec::new(),
        }
    }

    /// Takes in one thing the clip's job did. Returns true when the clip
    /// has just finished, either way.
    pub fn take(&mut self, event: &JobEvent) -> bool {
        match event {
            JobEvent::Line(_, line) => {
                self.lines.push(line.clone());
                false
            }
            JobEvent::Ended(code) => {
                self.result = Some(self.finished(*code));
                true
            }
            JobEvent::CouldntStart(why) => {
                self.result = Some(Err(crate::build::rust_is_missing(why)));
                true
            }
        }
    }

    fn finished(&self, code: Option<i32>) -> Result<ClipResult, Problem> {
        let gif = self.wanted.with_extension("gif");
        let note = self
            .lines
            .iter()
            .find_map(|line| line.find("clip: ").map(|at| line[at..].trim().to_string()))
            .unwrap_or_default();
        let file = if self.wanted.is_file() {
            Some(self.wanted.clone())
        } else if gif.is_file() {
            Some(gif)
        } else {
            None
        };
        match (code, file) {
            (Some(0), Some(file)) => {
                let is_gif = file.extension().is_some_and(|e| e == "gif");
                Ok(ClipResult {
                    with_sound: !is_gif && note.contains("with sound"),
                    needs_ffmpeg: is_gif,
                    file,
                    note,
                })
            }
            _ => Err(Problem::new(
                "The clip couldn't be made.",
                "Look at the details, or copy them for an agent.",
            )
            .with_details(self.lines.join("\n"))),
        }
    }
}
