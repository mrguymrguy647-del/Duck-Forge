//! The Studio itself, without its window: what's open, what's running,
//! and every action as one named command.
//!
//! A button, a key, a test, a script file and (later) an agent's tool all
//! do the same thing: `studio.run(Command::...)`. The app only draws what
//! this holds and turns clicks into commands.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use duckforge::prelude::{Key, MouseButton};
use duckforge::{InputAction, InputScript};
use serde::{Deserialize, Serialize};

use crate::build::{self, Build, BuildState, Rust, Watcher};
use crate::check::{self, Check, CheckResult};
use crate::clip::{Clip, ClipResult};
use crate::console::{Console, JobEvent, JobId, JobState};
use crate::folder::{Card, Game, GamesFolder, STARTERS, name_from_title};
use crate::play::{AfterHello, Ending, Play, PlayState, Timing};
use crate::problem::Problem;
use crate::settings::Settings;

/// Everything the Studio can be asked to do. In a script file each is a
/// JSON object whose `"do"` is the command's name:
/// `{"do": "game.open", "name": "duck_peaks"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do")]
pub enum Command {
    /// Go to Home (stops the game that's open).
    #[serde(rename = "home.open")]
    HomeOpen,
    /// Use this folder as the games folder.
    #[serde(rename = "home.choose_folder")]
    ChooseFolder { folder: PathBuf },
    /// Look for Rust again (after installing it).
    #[serde(rename = "home.check_again")]
    CheckAgain,
    /// Make a new game from a starter and open it.
    #[serde(rename = "game.new")]
    GameNew { starter: String, name: String },
    /// Open a game: build it if needed, then play it.
    #[serde(rename = "game.open")]
    GameOpen { name: String },
    /// Build the open game now.
    #[serde(rename = "game.build")]
    GameBuild,
    /// Play: carry on from the frozen frame.
    #[serde(rename = "game.play")]
    GamePlay,
    /// Freeze on the frame the game is on (Build).
    #[serde(rename = "game.freeze")]
    GameFreeze,
    /// Tab: freeze if playing, play if frozen.
    #[serde(rename = "game.toggle")]
    GameToggle,
    /// While frozen: play this many more frames, then stay frozen.
    #[serde(rename = "game.step")]
    GameStep { frames: u32 },
    /// Start the game again from the beginning (with the newest build).
    #[serde(rename = "game.restart")]
    GameRestart,
    /// Start again and replay this session's moves, to get back to where
    /// it was.
    #[serde(rename = "game.replay_my_moves")]
    GameReplayMyMoves,
    /// Open the game in its own window, on the frame it's on.
    #[serde(rename = "game.pop_out")]
    GamePopOut,
    /// Close the game's own window and bring it back inside.
    #[serde(rename = "game.bring_back")]
    GameBringBack,
    /// Stop the game.
    #[serde(rename = "game.stop")]
    GameStop,
    /// Press or let go of a key or button, or move the mouse, written as
    /// in an input script: `{"do": "game.input", "press": "Space"}`.
    #[serde(rename = "game.input")]
    GameInput {
        #[serde(flatten)]
        event: serde_json::Map<String, serde_json::Value>,
    },
    /// Run the whole check for the open game.
    #[serde(rename = "check.run")]
    CheckRun,
    /// Show (or hide, with no line) one line of the check, with its picture.
    #[serde(rename = "check.show_problem")]
    CheckShowProblem { line: Option<usize> },
    /// Make a ten-second clip of the last run.
    #[serde(rename = "clip.make")]
    ClipMake,
    /// Run a line typed into the Console panel, in the games folder.
    #[serde(rename = "console.run")]
    ConsoleRun { line: String },
}

/// One command, for lists of what the Studio can do (the Console's help
/// now, an agent's tools later).
pub struct CommandInfo {
    pub name: &'static str,
    pub title: &'static str,
    /// What it needs, in words. Empty if nothing.
    pub needs: &'static str,
}

/// Every command, in one table.
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "home.open",
        title: "Go to Home",
        needs: "",
    },
    CommandInfo {
        name: "home.choose_folder",
        title: "Choose the games folder",
        needs: "folder: the Duck Forge folder",
    },
    CommandInfo {
        name: "home.check_again",
        title: "Look for Rust again",
        needs: "",
    },
    CommandInfo {
        name: "game.new",
        title: "Make a new game",
        needs: "starter: platformer, topdown, shooter, runner3d or tiny; name: like Bee Maze",
    },
    CommandInfo {
        name: "game.open",
        title: "Open a game",
        needs: "name: like duck_peaks",
    },
    CommandInfo {
        name: "game.build",
        title: "Build the game now",
        needs: "",
    },
    CommandInfo {
        name: "game.play",
        title: "Play",
        needs: "",
    },
    CommandInfo {
        name: "game.freeze",
        title: "Freeze (Build)",
        needs: "",
    },
    CommandInfo {
        name: "game.toggle",
        title: "Switch between Build and Play (Tab)",
        needs: "",
    },
    CommandInfo {
        name: "game.step",
        title: "Play a few more frames, frozen",
        needs: "frames: how many",
    },
    CommandInfo {
        name: "game.restart",
        title: "Restart the game",
        needs: "",
    },
    CommandInfo {
        name: "game.replay_my_moves",
        title: "Restart and replay my moves",
        needs: "",
    },
    CommandInfo {
        name: "game.pop_out",
        title: "Pop out into its own window",
        needs: "",
    },
    CommandInfo {
        name: "game.bring_back",
        title: "Bring the game back inside",
        needs: "",
    },
    CommandInfo {
        name: "game.stop",
        title: "Stop the game",
        needs: "",
    },
    CommandInfo {
        name: "game.input",
        title: "Press a key, as in an input script",
        needs: "one of press, release, mouse, mouse_motion, mouse_press, mouse_release, scroll",
    },
    CommandInfo {
        name: "check.run",
        title: "Check the game",
        needs: "",
    },
    CommandInfo {
        name: "check.show_problem",
        title: "Show one line of the check",
        needs: "line: which one, from 0 (none hides it)",
    },
    CommandInfo {
        name: "clip.make",
        title: "Make a clip of the last run",
        needs: "",
    },
    CommandInfo {
        name: "console.run",
        title: "Run a typed command",
        needs: "line: the command",
    },
];

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Game,
}

/// A game's card on Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeCard {
    pub game: Game,
    pub card: Card,
    pub picture: Option<PathBuf>,
}

/// A new game being made.
#[derive(Debug)]
pub struct Making {
    pub name: String,
    pub title: String,
    pub job: JobId,
}

/// The game that's open, and everything going on with it.
pub struct OpenGame {
    pub game: Game,
    /// The build in progress, or the last one.
    pub build: Option<Build>,
    /// The newest built copy, ready to play.
    pub program: Option<PathBuf>,
    /// A newer build is ready than the one being played.
    pub new_build_ready: bool,
    pub play: Option<Play>,
    pub check: Option<Check>,
    pub check_result: Option<CheckResult>,
    /// The check's line whose picture is showing.
    pub shown_line: Option<usize>,
    pub clip: Option<Clip>,
    pub clip_result: Option<Result<ClipResult, Problem>>,
    /// The last session saved, to make clips from and to replay.
    pub last_session: Option<PathBuf>,
    /// The last finished session's recording, for "replay my moves".
    last_recording: Option<InputScript>,
    watcher: Watcher,
    watched: Vec<PathBuf>,
    looked: Instant,
    /// Start playing as soon as there's a build.
    play_when_built: bool,
}

impl OpenGame {
    /// What's wrong with the build, if it failed.
    pub fn build_problem(&self) -> Option<&Problem> {
        match self.build.as_ref().map(|build| &build.state) {
            Some(BuildState::Failed(problem)) => Some(problem),
            _ => None,
        }
    }

    pub fn is_building(&self) -> bool {
        matches!(
            self.build.as_ref().map(|build| &build.state),
            Some(BuildState::Running)
        )
    }

    pub fn play_state(&self) -> Option<&PlayState> {
        self.play.as_ref().map(|play| &play.state)
    }
}

/// The Studio.
pub struct Studio {
    pub settings: Settings,
    pub folder: Option<GamesFolder>,
    /// Why there's no games folder, if there isn't.
    pub folder_problem: Option<Problem>,
    pub rust: Result<Rust, Problem>,
    pub console: Console,
    pub screen: Screen,
    pub cards: Vec<HomeCard>,
    pub making: Option<Making>,
    pub open: Option<OpenGame>,
    /// The last thing that went wrong with a command, for the app to show.
    pub problem: Option<Problem>,
}

impl Studio {
    /// Starts the Studio with these settings: finds the games folder and
    /// Rust, and lists the games.
    pub fn new(settings: Settings) -> Studio {
        let mut studio = Studio {
            settings,
            folder: None,
            folder_problem: None,
            rust: build::find_rust(),
            console: Console::default(),
            screen: Screen::Home,
            cards: Vec::new(),
            making: None,
            open: None,
            problem: None,
        };
        studio.console.path = studio.rust.as_ref().ok().and_then(|rust| rust.path.clone());
        let wanted = studio
            .settings
            .games_folder
            .clone()
            .or_else(GamesFolder::find);
        match wanted {
            Some(root) => match GamesFolder::open(root) {
                Ok(folder) => studio.folder = Some(folder),
                Err(problem) => studio.folder_problem = Some(problem),
            },
            None => {
                studio.folder_problem = Some(Problem::new(
                    "The Studio doesn't know where Duck Forge is yet.",
                    "Choose the Duck Forge folder: the one with Cargo.toml and an examples \
                     folder in it.",
                ));
            }
        }
        studio.refresh_cards();
        studio
    }

    /// Reads the games and how their last checks went, for Home.
    pub fn refresh_cards(&mut self) {
        self.cards = match &self.folder {
            Some(folder) => folder
                .games()
                .into_iter()
                .map(|game| HomeCard {
                    card: check::card(folder, &game),
                    picture: folder.card_picture(&game.name),
                    game,
                })
                .collect(),
            None => Vec::new(),
        };
    }

    fn folder(&self) -> Result<GamesFolder, Problem> {
        self.folder.clone().ok_or_else(|| {
            self.folder_problem.clone().unwrap_or_else(|| {
                Problem::new(
                    "There's no games folder yet.",
                    "Choose the Duck Forge folder.",
                )
            })
        })
    }

    fn rust(&self) -> Result<Rust, Problem> {
        self.rust.clone()
    }

    fn open_game(&mut self) -> Result<&mut OpenGame, Problem> {
        self.open
            .as_mut()
            .ok_or_else(|| Problem::new("No game is open.", "Open a game from Home first."))
    }

    /// Does one command. Says what happened in a few words, or what went
    /// wrong. A problem is also kept in `problem`, for the app to show.
    pub fn run(&mut self, command: Command) -> Result<String, Problem> {
        let result = self.do_command(command);
        if let Err(problem) = &result {
            self.problem = Some(problem.clone());
        }
        result
    }

    fn do_command(&mut self, command: Command) -> Result<String, Problem> {
        match command {
            Command::HomeOpen => {
                self.close_game();
                self.screen = Screen::Home;
                self.refresh_cards();
                Ok(String::from("Home"))
            }
            Command::ChooseFolder { folder } => {
                let opened = GamesFolder::open(folder.clone())?;
                self.close_game();
                self.settings.games_folder = Some(opened.root().to_path_buf());
                self.settings.save();
                self.folder = Some(opened);
                self.folder_problem = None;
                self.screen = Screen::Home;
                self.refresh_cards();
                Ok(format!("The games folder is {}", folder.display()))
            }
            Command::CheckAgain => {
                self.rust = build::find_rust();
                self.console.path = self.rust.as_ref().ok().and_then(|rust| rust.path.clone());
                let rust = self.rust()?;
                if let Some(open) = &mut self.open
                    && open.program.is_none()
                    && !open.is_building()
                {
                    open.play_when_built = true;
                    self.start_build()?;
                }
                Ok(format!("Found {}", rust.version))
            }
            Command::GameNew { starter, name } => self.new_game(&starter, &name),
            Command::GameOpen { name } => {
                let folder = self.folder()?;
                let game = folder.game(&name).ok_or_else(|| {
                    Problem::new(
                        format!("There's no game called {name}."),
                        "Pick one from Home.",
                    )
                })?;
                self.close_game();
                let watched = folder.code_files(&game);
                self.open = Some(OpenGame {
                    check_result: check::load(&folder, &game.name),
                    game,
                    build: None,
                    program: None,
                    new_build_ready: false,
                    play: None,
                    check: None,
                    shown_line: None,
                    clip: None,
                    clip_result: None,
                    last_session: None,
                    last_recording: None,
                    watcher: Watcher::new(&watched),
                    watched,
                    looked: Instant::now(),
                    play_when_built: true,
                });
                self.screen = Screen::Game;
                self.problem = None;
                // Without Rust the game screen says so; that's not this
                // command failing.
                if self.rust.is_ok() {
                    self.start_build()?;
                }
                Ok(format!("Opened {name}"))
            }
            Command::GameBuild => {
                self.start_build()?;
                Ok(String::from("Building"))
            }
            Command::GamePlay => {
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.state == PlayState::Frozen => {
                        play.resume(&[], &[]);
                        Ok(String::from("Playing"))
                    }
                    Some(play) if play.is_running() => Ok(String::from("Already playing")),
                    _ => {
                        self.start_play(AfterHello::Play, None)?;
                        Ok(String::from("Starting"))
                    }
                }
            }
            Command::GameFreeze => {
                self.freeze();
                Ok(String::from("Frozen"))
            }
            Command::GameToggle => {
                let frozen = matches!(
                    self.open_game()?.play_state(),
                    Some(PlayState::Frozen) | Some(PlayState::Stopped { .. }) | None
                );
                self.do_command(if frozen {
                    Command::GamePlay
                } else {
                    Command::GameFreeze
                })
            }
            Command::GameStep { frames } => {
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.state == PlayState::Frozen => {
                        play.step(frames, &[]);
                        Ok(format!("Played {frames} more frames"))
                    }
                    _ => Err(Problem::new(
                        "The game isn't frozen.",
                        "Freeze it first (Tab), then step.",
                    )),
                }
            }
            Command::GameRestart => {
                self.stop_play();
                self.start_play(AfterHello::Play, None)?;
                Ok(String::from("Restarting"))
            }
            Command::GameReplayMyMoves => {
                self.stop_play();
                let script = self
                    .open_game()?
                    .last_recording
                    .clone()
                    .filter(|script| script.frames.unwrap_or(0) > 0)
                    .ok_or_else(|| {
                        Problem::new(
                            "There are no moves to replay yet.",
                            "Press Restart to start the game from the beginning.",
                        )
                    })?;
                let frames = script.frames.unwrap_or(0);
                let seed = script.seed;
                self.start_play(AfterHello::Replay(Box::new(script), frames), seed)?;
                Ok(format!("Replaying {frames} frames"))
            }
            Command::GamePopOut => {
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.is_running() => {
                        play.pop_out();
                        Ok(String::from("Popping out"))
                    }
                    _ => Err(Problem::new("The game isn't running.", "Press Play first.")),
                }
            }
            Command::GameBringBack => {
                if let Some(play) = &mut self.open_game()?.play {
                    play.bring_back();
                }
                Ok(String::from("Bringing it back"))
            }
            Command::GameStop => {
                self.stop_play();
                Ok(String::from("Stopped"))
            }
            Command::GameInput { event } => {
                let action = action_from(&event)?;
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.is_running() => {
                        play.queue(action);
                        Ok(String::from("Sent"))
                    }
                    _ => Err(Problem::new("The game isn't running.", "Press Play first.")),
                }
            }
            Command::CheckRun => {
                let folder = self.folder()?;
                self.rust()?;
                let name = self.open_game()?.game.name.clone();
                let check = Check::start(&mut self.console, &folder, &name);
                let open = self.open_game()?;
                open.check = Some(check);
                open.shown_line = None;
                Ok(String::from("Checking"))
            }
            Command::CheckShowProblem { line } => {
                let open = self.open_game()?;
                let lines = open
                    .check_result
                    .as_ref()
                    .map_or(0, |result| result.rows.len());
                open.shown_line = line.filter(|line| *line < lines);
                Ok(String::from("Shown"))
            }
            Command::ClipMake => self.make_clip(),
            Command::ConsoleRun { line } => {
                let folder = self.folder()?;
                self.console.run_typed(&line, folder.root());
                Ok(format!("Running {line}"))
            }
        }
    }

    fn new_game(&mut self, starter: &str, typed: &str) -> Result<String, Problem> {
        let folder = self.folder()?;
        self.rust()?;
        let starter = STARTERS
            .iter()
            .find(|known| known.kind == starter)
            .ok_or_else(|| {
                Problem::new(
                    format!("There's no starter called {starter}."),
                    "Pick one of: platformer, topdown, shooter, runner3d, tiny.",
                )
            })?;
        let name =
            name_from_title(typed).map_err(|why| Problem::new(why, "Then press Make it."))?;
        if folder.game(&name).is_some() {
            return Err(Problem::new(
                format!(
                    "There's already a game called {}.",
                    crate::folder::title_of(&name)
                ),
                "Pick another name.",
            ));
        }
        let command = folder.new_game_command(&name, starter.kind);
        let job = self
            .console
            .start("New game", &command, crate::console::Keep::Everything);
        self.making = Some(Making {
            title: crate::folder::title_of(&name),
            name,
            job,
        });
        Ok(String::from("Making the game"))
    }

    fn start_build(&mut self) -> Result<(), Problem> {
        let folder = self.folder()?;
        let rust = self.rust()?;
        let name = self.open_game()?.game.name.clone();
        if self.open_game()?.is_building() {
            return Ok(());
        }
        let expected = self.settings.build_parts.get(&name).copied().unwrap_or(0);
        let build = Build::start(&mut self.console, &folder, &rust, &name, expected);
        self.open_game()?.build = Some(build);
        Ok(())
    }

    /// Starts the built game. With no build yet, it starts when there is one.
    fn start_play(&mut self, then: AfterHello, seed: Option<u32>) -> Result<(), Problem> {
        let folder = self.folder()?;
        let open = self.open_game()?;
        let Some(program) = open.program.clone() else {
            open.play_when_built = true;
            if !open.is_building() {
                self.start_build()?;
            }
            return Ok(());
        };
        let name = open.game.name.clone();
        let play = Play::start(&mut self.console, &folder, &name, &program, seed, then)?;
        let open = self.open_game()?;
        open.play = Some(play);
        open.new_build_ready = false;
        Ok(())
    }

    /// Freezes the game and saves the session so far.
    fn freeze(&mut self) {
        let (Some(open), Some(folder)) = (&mut self.open, &self.folder) else {
            return;
        };
        let Some(play) = &mut open.play else {
            return;
        };
        if play.state == PlayState::Playing {
            play.freeze();
            // Let the tick in flight land, so the recording is whole.
            play.wait_for_picture(&self.console, Duration::from_millis(250));
            if let Ok(path) = play.save_session(&self.console, folder) {
                open.last_session = Some(path);
            }
        }
    }

    /// Stops the running game, keeping its session.
    fn stop_play(&mut self) {
        let (Some(open), Some(folder)) = (&mut self.open, &self.folder) else {
            return;
        };
        let Some(play) = &mut open.play else {
            return;
        };
        if play.is_running() {
            if play.hello.is_some() {
                let recording = play.recording(&self.console, Duration::from_millis(500));
                open.last_recording = Some(recording);
                if let Ok(path) = play.save_session(&self.console, folder) {
                    open.last_session = Some(path);
                }
            }
            play.stop(&mut self.console);
        }
    }

    fn close_game(&mut self) {
        self.stop_play();
        if let Some(open) = self.open.take() {
            for job in [
                open.build
                    .as_ref()
                    .filter(|b| b.state == BuildState::Running)
                    .map(|b| b.job),
                open.check
                    .as_ref()
                    .filter(|c| c.result.is_none())
                    .map(|c| c.job),
                open.clip
                    .as_ref()
                    .filter(|c| c.result.is_none())
                    .map(|c| c.job),
            ]
            .into_iter()
            .flatten()
            {
                self.console.stop(job);
            }
        }
    }

    fn make_clip(&mut self) -> Result<String, Problem> {
        let folder = self.folder()?;
        let rust = self.rust()?;
        // The run so far, if a game is running; else the last one saved.
        if let Some(open) = &mut self.open {
            let playing = open
                .play
                .as_mut()
                .filter(|play| play.is_running() && play.frame > 0);
            if let Some(play) = playing
                && let Ok(path) = play.save_session(&self.console, &folder)
            {
                open.last_session = Some(path);
            }
        }
        let open = self.open_game()?;
        let name = open.game.name.clone();
        let session = open
            .last_session
            .clone()
            .or_else(|| {
                let latest = folder.sessions_folder(&name).join("latest.json");
                latest.is_file().then_some(latest)
            })
            .ok_or_else(|| {
                Problem::new(
                    "There's no run to make a clip of yet.",
                    "Play the game for a moment, then press Clip: a clip is the last ten \
                     seconds of what you played.",
                )
            })?;
        let clip = Clip::start(&mut self.console, &folder, &rust.cargo, &name, &session);
        let open = self.open_game()?;
        open.clip = Some(clip);
        open.clip_result = None;
        Ok(String::from("Making a clip"))
    }

    /// Takes in whatever has happened since last time: output from the
    /// commands running, a finished build, changed code, the game's
    /// messages. Call it often; it never waits.
    pub fn update(&mut self, now: Instant) {
        let events = self.console.poll();
        for (job, event) in &events {
            self.take_job_event(*job, event);
        }
        self.watch_code(now);
        if let Some(open) = &mut self.open
            && let Some(play) = &mut open.play
        {
            let was_running = play.is_running();
            play.pump(&self.console);
            if was_running && !play.is_running() {
                // It ended by itself: keep what the Studio recorded, so
                // "replay my moves" can get back to just before.
                let recording = play.recording(&self.console, Duration::ZERO);
                open.last_recording = Some(recording);
                if let Some(folder) = &self.folder
                    && let Ok(path) = play.save_session(&self.console, folder)
                {
                    open.last_session = Some(path);
                }
            }
        }
    }

    fn take_job_event(&mut self, job: JobId, event: &JobEvent) {
        // A new game being made.
        if self.making.as_ref().is_some_and(|making| making.job == job) {
            match event {
                JobEvent::Ended(Some(0)) => {
                    let name = self
                        .making
                        .take()
                        .map(|making| making.name)
                        .unwrap_or_default();
                    let _ = self.run(Command::GameOpen { name });
                }
                JobEvent::Ended(_) | JobEvent::CouldntStart(_) => {
                    self.making = None;
                    self.problem = Some(
                        Problem::new(
                            "The new game couldn't be made.",
                            "Look at the details, or copy them for an agent.",
                        )
                        .with_details(self.console.tail(job, 40)),
                    );
                }
                JobEvent::Line(..) => {}
            }
            return;
        }
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        if let Some(build) = open.build.as_mut().filter(|build| build.job == job) {
            if build.take(event) {
                let mut start = false;
                if let BuildState::Done { program } = &build.state {
                    self.settings
                        .build_parts
                        .insert(build.game.clone(), build.parts.max(1));
                    self.settings.save();
                    match build::copy_to_play(&folder, program) {
                        Ok(copy) => {
                            open.program = Some(copy);
                            let running = open.play.as_ref().is_some_and(Play::is_running);
                            open.new_build_ready = running;
                            start = !running
                                && (open.play_when_built
                                    || matches!(
                                        open.play_state(),
                                        Some(PlayState::Stopped { .. })
                                    ));
                        }
                        Err(problem) => build.state = BuildState::Failed(problem),
                    }
                }
                if start {
                    open.play_when_built = false;
                    if let Err(problem) = self.start_play(AfterHello::Play, None) {
                        self.problem = Some(problem);
                    }
                }
            }
            return;
        }
        if let Some(check) = open.check.as_mut().filter(|check| check.job == job) {
            if check.take(event, &folder) {
                match check.result.take() {
                    Some(Ok(result)) => open.check_result = Some(result),
                    Some(Err(problem)) => self.problem = Some(problem),
                    None => {}
                }
                open.check = None;
            }
            return;
        }
        if let Some(clip) = open.clip.as_mut().filter(|clip| clip.job == job)
            && clip.take(event)
        {
            open.clip_result = clip.result.take();
            open.clip = None;
        }
    }

    /// Twice a second, looks at the game's code. Once a change has stayed
    /// unchanged for a second, the game is built again.
    fn watch_code(&mut self, now: Instant) {
        let Some(open) = &mut self.open else {
            return;
        };
        if now.saturating_duration_since(open.looked) < Duration::from_millis(500) {
            return;
        }
        open.looked = now;
        if open.watcher.look(&open.watched, now) && self.rust.is_ok() {
            if open.is_building() {
                // Changed again mid-build: build once more when this one ends.
                if let Some(build) = &open.build {
                    self.console.stop(build.job);
                }
                open.build = None;
            }
            let _ = self.start_build();
        }
    }

    /// The start of one of the app's own frames: takes in what's new, and
    /// (while playing) sends the game its tick with what the player did.
    pub fn begin_frame(&mut self, now: Instant, actions: &[InputAction]) {
        self.update(now);
        if let Some(play) = self.open.as_mut().and_then(|open| open.play.as_mut()) {
            play.begin_frame(now, actions);
        }
    }

    /// Waits up to `patience` for the picture of the tick this frame sent.
    /// Returns true if there's a new picture to show.
    pub fn wait_for_picture(&mut self, patience: Duration) -> bool {
        match self.open.as_mut().and_then(|open| open.play.as_mut()) {
            Some(play) => play.wait_for_picture(&self.console, patience),
            None => false,
        }
    }

    /// Carries on from a freeze, letting go of whatever isn't really held
    /// any more.
    pub fn resume(&mut self, keys_down: &[Key], buttons_down: &[MouseButton]) {
        if let Some(play) = self.open.as_mut().and_then(|open| open.play.as_mut()) {
            play.resume(keys_down, buttons_down);
        }
    }

    pub fn play(&self) -> Option<&Play> {
        self.open.as_ref().and_then(|open| open.play.as_ref())
    }

    /// The Timing line of the game that's playing.
    pub fn timing(&self, now: Instant) -> Timing {
        self.play()
            .map_or(Timing::default(), |play| play.timing(now))
    }

    /// Whether the game that's open stopped by crashing.
    pub fn crashed(&self) -> Option<&Problem> {
        match self.play().map(|play| &play.state) {
            Some(PlayState::Stopped {
                ending: Ending::Crashed,
                problem: Some(problem),
            }) => Some(problem),
            _ => None,
        }
    }

    /// Whether any command the Studio started is still running.
    pub fn is_busy(&self) -> bool {
        self.console
            .entries()
            .iter()
            .any(|entry| entry.state == JobState::Running && entry.title != "Play")
    }
}

impl Drop for Studio {
    /// Closing the Studio ends the game politely (so it can save), then
    /// stops everything else it started.
    fn drop(&mut self) {
        self.close_game();
        self.settings.save();
    }
}

/// An input, from the fields of a `game.input` command: the same words as
/// an event in an input script.
fn action_from(event: &serde_json::Map<String, serde_json::Value>) -> Result<InputAction, Problem> {
    let mut event = event.clone();
    event.insert(String::from("frame"), serde_json::Value::from(1));
    let script = serde_json::json!({ "events": [event] }).to_string();
    InputScript::parse(&script)
        .ok()
        .and_then(|script| script.events.first().map(|event| event.action))
        .ok_or_else(|| {
            Problem::new(
                "That input can't be read.",
                "Write it as in an input script, like {\"do\": \"game.input\", \"press\": \
                 \"Space\"}. docs/input-format.md lists them all.",
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_reads_from_json_and_is_in_the_table() {
        let written = [
            r#"{"do": "home.open"}"#,
            r#"{"do": "home.choose_folder", "folder": "C:/Duck Forge"}"#,
            r#"{"do": "home.check_again"}"#,
            r#"{"do": "game.new", "starter": "platformer", "name": "Bee Maze"}"#,
            r#"{"do": "game.open", "name": "duck_peaks"}"#,
            r#"{"do": "game.build"}"#,
            r#"{"do": "game.play"}"#,
            r#"{"do": "game.freeze"}"#,
            r#"{"do": "game.toggle"}"#,
            r#"{"do": "game.step", "frames": 5}"#,
            r#"{"do": "game.restart"}"#,
            r#"{"do": "game.replay_my_moves"}"#,
            r#"{"do": "game.pop_out"}"#,
            r#"{"do": "game.bring_back"}"#,
            r#"{"do": "game.stop"}"#,
            r#"{"do": "game.input", "press": "Space"}"#,
            r#"{"do": "check.run"}"#,
            r#"{"do": "check.show_problem", "line": 2}"#,
            r#"{"do": "clip.make"}"#,
            r#"{"do": "console.run", "line": "cargo test"}"#,
        ];
        let mut names = Vec::new();
        for text in written {
            let command: Command =
                serde_json::from_str(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            let back = serde_json::to_value(&command).unwrap();
            names.push(back["do"].as_str().unwrap().to_string());
        }
        let table: Vec<&str> = COMMANDS.iter().map(|info| info.name).collect();
        assert_eq!(
            names, table,
            "the table lists every command, in the same order"
        );
        assert!(serde_json::from_str::<Command>(r#"{"do": "game.fly"}"#).is_err());
    }

    #[test]
    fn inputs_are_written_as_in_an_input_script() {
        let read = |text: &str| {
            let Command::GameInput { event } = serde_json::from_str(text).unwrap() else {
                panic!("an input");
            };
            action_from(&event)
        };
        assert_eq!(
            read(r#"{"do": "game.input", "press": "Space"}"#),
            Ok(InputAction::Press(Key::Space))
        );
        assert_eq!(
            read(r#"{"do": "game.input", "mouse_press": "Left"}"#),
            Ok(InputAction::MousePress(MouseButton::Left))
        );
        assert!(read(r#"{"do": "game.input", "press": "Spcae"}"#).is_err());
    }

    #[test]
    fn commands_that_need_a_game_say_so_plainly() {
        // SAFETY: tests in this crate only ever set this variable to a
        // scratch file, so the person's own settings are never touched.
        unsafe {
            std::env::set_var(
                "DUCK_FORGE_STUDIO_SETTINGS",
                std::env::temp_dir().join("duck-forge-studio-test-settings.json"),
            );
        }
        let mut studio = Studio::new(Settings::default());
        let problem = studio.run(Command::GameToggle).unwrap_err();
        assert_eq!(problem.what, "No game is open.");
        assert_eq!(studio.problem, Some(problem));
        let problem = studio.run(Command::GameOpen {
            name: String::from("no_such_game"),
        });
        assert!(
            problem
                .unwrap_err()
                .what
                .contains("no game called no_such_game")
        );
    }
}
