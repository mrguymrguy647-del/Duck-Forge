//! Playing a game inside the Studio.
//!
//! The game is its own program. The Studio starts it, the game connects
//! back over a local link, and from then on the Studio keeps the time:
//! each time the Studio draws itself it sends one "tick" with the inputs
//! since last time, and the game answers with its picture. Freezing is
//! simply not sending ticks, so the game carries on later from exactly the
//! frame it was on.
//!
//! Every input goes into a recording as it's sent, so a session can always
//! be replayed, even if the game crashes.

use std::collections::{BTreeMap, VecDeque};
use std::io::BufReader;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::time::{Duration, Instant};

use duckforge::prelude::{Key, MouseButton};
use duckforge::script::{Change, ChangeEvent};
use duckforge::studio_link::protocol::{DrawnMap, GIVE_FILES_VARIABLE};
use duckforge::studio_link::protocol::{
    Frame, Hello, KEY_VARIABLE, LINK_VERSION, ToGame, ToStudio,
};
use duckforge::{InputAction, InputEvent, InputScript};

use crate::console::{CommandLine, Console, JobId, JobState, Keep};
use crate::folder::GamesFolder;
use crate::problem::Problem;

/// A gap between two of the Studio's own frames longer than this means
/// the Studio couldn't draw (its window was being dragged or resized, or
/// the computer was busy). That time is simply skipped: the game carries
/// on with one update, never a burst to catch up.
pub const STALL: Duration = Duration::from_millis(100);

/// The most updates one tick asks for. A slow Studio frame (up to a 20th
/// of a second) is caught up on; anything longer slows the game instead.
pub const MOST_UPDATES: u32 = 3;

/// How long the Studio waits, within one of its own frames, for the
/// picture of the tick it just sent. If the game answers in time, the
/// picture is shown in that very frame: no added delay. If not, the last
/// picture stays for one more frame.
pub const PATIENCE: Duration = Duration::from_millis(10);

/// A tick unanswered for this long means the game isn't answering.
pub const NOT_ANSWERING: Duration = Duration::from_secs(2);

/// How long a game gets to start and say hello.
const STARTING_PATIENCE: Duration = Duration::from_secs(15);

/// Decides how many updates each of the Studio's frames asks the game
/// for, so the game runs at its own speed whatever the screen's.
#[derive(Debug, Clone)]
pub struct StudioClock {
    /// Seconds per update.
    step: f64,
    /// Time that has passed but hasn't been played yet, in seconds.
    owed: f64,
    last: Option<Instant>,
}

impl StudioClock {
    pub fn new(updates_per_second: f64) -> StudioClock {
        StudioClock {
            step: 1.0 / updates_per_second.max(1.0),
            owed: 0.0,
            last: None,
        }
    }

    /// Forgets the time so far: the next frame starts afresh with one
    /// update (after a freeze, or a frame the game was too slow for).
    pub fn restart(&mut self) {
        self.last = None;
    }

    /// How many updates are due on a Studio frame drawn at `now`.
    pub fn due(&mut self, now: Instant) -> u32 {
        let Some(last) = self.last.replace(now) else {
            // The first frame: one update, then half a step in hand, so a
            // frame a little early or late still gets exactly one.
            self.owed = self.step * 0.5;
            return 1;
        };
        let gap = now.saturating_duration_since(last);
        if gap > STALL {
            // The Studio couldn't draw. Nothing to catch up on.
            self.owed = self.step * 0.5;
            return 1;
        }
        self.owed += gap.as_secs_f64();
        let updates = ((self.owed / self.step).floor() as u32).min(MOST_UPDATES);
        self.owed -= f64::from(updates) * self.step;
        if self.owed > self.step {
            self.owed = self.step * 0.5; // the rest is dropped, not saved up
        }
        updates
    }
}

/// The game's latest picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Row by row, each `0x00RRGGBB`.
    pub pixels: Vec<u32>,
    /// The frame it shows.
    pub frame: u64,
    /// Goes up by one with every new picture, so the app knows when to
    /// redraw its copy.
    pub serial: u64,
}

/// Why a game isn't running any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ending {
    /// The Studio stopped it.
    Asked,
    /// The game ended itself (its own Quit).
    Quit,
    /// It crashed, or ended without a word.
    Crashed,
}

/// Where a play session stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayState {
    /// The game's program is starting and hasn't said hello yet.
    Starting,
    Playing,
    Frozen,
    /// Catching up: following a recording fast, without drawing.
    Replaying,
    /// Playing in its own window.
    PoppedOut,
    Stopped {
        ending: Ending,
        problem: Option<Problem>,
    },
}

/// One answered tick, for the Timing line.
#[derive(Debug, Clone, Copy)]
struct Sample {
    at: Instant,
    /// From sending the tick to having its picture, in milliseconds.
    round_trip: f32,
    /// The game's own time for the updates and the drawing.
    game: f32,
    /// Whether the picture came in the Studio frame that asked for it.
    on_time: bool,
    updates: u32,
}

/// The Timing line: how the last five seconds went.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Timing {
    /// Game updates per second (60 for most games).
    pub updates_per_second: f32,
    /// New pictures per second.
    pub pictures_per_second: f32,
    /// The longest wait between two new pictures, in milliseconds.
    pub longest_gap: f32,
    /// The game's own update and draw time for a tick, on average.
    pub game: f32,
    /// What the link adds: the round trip minus the game's own time.
    pub link: f32,
    /// The slowest round trip.
    pub slowest: f32,
    /// Ticks whose picture wasn't ready for the Studio frame that asked.
    pub late: u32,
    pub ticks: u32,
}

/// A message from the game (and when it arrived), or the end of the
/// connection.
enum Heard {
    Message(Box<ToStudio>, Instant),
    Closed,
}

/// A tick (or a run) the game hasn't answered yet.
#[derive(Debug, Clone, Copy)]
struct Waiting {
    id: u64,
    sent: Instant,
    updates: u32,
    /// The Studio drew a frame without its picture.
    late: bool,
}

/// One play session: a running game and the Studio's side of its link.
pub struct Play {
    pub game: String,
    pub job: JobId,
    pub state: PlayState,
    pub hello: Option<Hello>,
    /// The last frame the game has played.
    pub frame: u64,
    pub picture: Option<Picture>,
    /// The game wants the pointer hidden and held (`ctx.lock_pointer`).
    pub wants_pointer: bool,
    /// What draws the game's 3D: the CPU, or the graphics card's name.
    pub renderer: String,
    /// Lines the game gave with `ctx.studio_note`.
    pub notes: Vec<String>,
    /// The game hasn't answered a tick for [`NOT_ANSWERING`].
    pub not_answering: bool,
    key: String,
    listener: Option<TcpListener>,
    writer: Option<TcpStream>,
    heard: Option<Receiver<Heard>>,
    started: Instant,
    clock: StudioClock,
    next_id: u64,
    waiting: Option<Waiting>,
    /// Inputs not sent yet (they go with the next tick that has an update).
    pending: Vec<InputAction>,
    /// Everything sent so far, by frame: the session's recording.
    events: Vec<InputEvent>,
    held_keys: Vec<Key>,
    held_buttons: Vec<MouseButton>,
    samples: VecDeque<Sample>,
    pictures: VecDeque<Instant>,
    serial: u64,
    answers: Vec<(u64, String)>,
    bye: Option<String>,
    /// What to do once the game has said hello.
    then: AfterHello,
    /// The game draws its debug overlay (F1) on its pictures.
    debug_overlay: bool,
    /// Where tile maps were drawn on the latest picture.
    pub maps: Vec<DrawnMap>,
    /// Files and tune values changed so far, by frame: the session's
    /// recording has them too.
    changes: Vec<ChangeEvent>,
    /// The texts of the game's level files when it started, for the
    /// Studio's own recording.
    files_at_start: BTreeMap<String, String>,
    /// The game's answers to changed files, not yet taken: the file's
    /// name, and whether the game took it.
    file_answers: Vec<(String, bool)>,
    /// The game's `ctx.tune` values, as it last said them.
    pub tunes: Vec<crate::feel::TuneValue>,
    /// The sounds the game has played, as it last said them.
    pub sounds: Vec<crate::assets::SoundInfo>,
}

/// What a new session does first.
#[derive(Debug, Clone, PartialEq)]
pub enum AfterHello {
    /// Start playing at once.
    Play,
    /// Show the first frame and wait, frozen.
    Freeze,
    /// Follow this recording for this many frames, fast, then freeze there.
    Replay(Box<InputScript>, u64),
}

impl Play {
    /// Starts `program` (a game built with the Studio link), run from the
    /// games folder, and waits for it to connect.
    pub fn start(
        console: &mut Console,
        folder: &GamesFolder,
        game: &str,
        program: &Path,
        seed: Option<u32>,
        then: AfterHello,
        given: Option<BTreeMap<String, String>>,
    ) -> Result<Play, Problem> {
        let cant = |what: &str, problem: std::io::Error| {
            Problem::new(
                "The Studio couldn't set up the link to the game.",
                "Try again. If it keeps happening, restart the Studio.",
            )
            .with_details(format!("{what}: {problem}"))
        };
        // Only this computer can connect: 127.0.0.1 is the computer itself.
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| cant("listening", e))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| cant("listening", e))?;
        let address = listener
            .local_addr()
            .map_err(|e| cant("listening", e))?
            .to_string();
        let key = one_time_key();
        let mut words = vec![String::from("--studio"), address.clone()];
        if let Some(seed) = seed {
            words.extend([String::from("--seed"), seed.to_string()]);
        }
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        let shown = match program.strip_prefix(folder.root()) {
            Ok(short) => short.display().to_string(),
            Err(_) => program.display().to_string(),
        };
        let mut command = CommandLine::new(program, &words, folder.root())
            .shown_as(format!("{shown} {}", words.join(" ")))
            .with_env(KEY_VARIABLE, &key);
        // "Play from here": the game reads these texts for its files
        // instead of what's on disk, this time only.
        if let Some(texts) = &given {
            let file = folder.sessions_folder(game).join("given-files.json");
            let written = std::fs::create_dir_all(folder.sessions_folder(game)).and_then(|()| {
                std::fs::write(&file, serde_json::to_string(texts).unwrap_or_default())
            });
            written.map_err(|e| cant("writing the level to play from", e))?;
            command = command.with_env(GIVE_FILES_VARIABLE, &file.to_string_lossy());
        }
        let job = console.start("Play", &command, Keep::Everything);
        Ok(Play {
            game: game.to_string(),
            job,
            state: PlayState::Starting,
            hello: None,
            frame: 0,
            picture: None,
            wants_pointer: false,
            renderer: String::new(),
            notes: Vec::new(),
            not_answering: false,
            key,
            listener: Some(listener),
            writer: None,
            heard: None,
            started: Instant::now(),
            clock: StudioClock::new(60.0),
            next_id: 1,
            waiting: None,
            pending: Vec::new(),
            events: Vec::new(),
            held_keys: Vec::new(),
            held_buttons: Vec::new(),
            samples: VecDeque::new(),
            pictures: VecDeque::new(),
            serial: 0,
            answers: Vec::new(),
            bye: None,
            then,
            debug_overlay: false,
            maps: Vec::new(),
            changes: Vec::new(),
            files_at_start: given.unwrap_or_default(),
            file_answers: Vec::new(),
            tunes: Vec::new(),
            sounds: Vec::new(),
        })
    }

    /// Asks the game for its tune values and the sounds it has played;
    /// the answers land in `tunes` and `sounds`.
    pub fn ask_about_feel(&mut self) {
        if self.hello.is_none() || !self.is_running() {
            return;
        }
        for what in ["tunes", "sounds"] {
            let id = self.next_id;
            self.next_id += 1;
            self.send(&ToGame::Ask {
                id,
                what: what.to_string(),
            });
        }
    }

    /// Has the game play one of its sounds again (it isn't changed by it).
    pub fn play_sound(&mut self, name: &str) {
        if self.hello.is_some() && self.is_running() {
            self.send(&ToGame::PlaySound {
                name: name.to_string(),
            });
        }
    }

    /// The Studio's record of the game's level files as they were when it
    /// started (for the session's recording if the game can't give its own).
    pub fn note_files_at_start(&mut self, files: BTreeMap<String, String>) {
        for (name, text) in files {
            self.files_at_start.entry(name).or_insert(text);
        }
    }

    /// A file the game read has new text (a level painted in Build mode):
    /// the game reads it again now. A frozen game draws its picture again,
    /// so the change shows at once. Kept in the session's recording.
    pub fn send_file(&mut self, name: &str, text: &str) {
        if self.hello.is_none() || !self.is_running() {
            return;
        }
        self.send(&ToGame::File {
            path: name.to_string(),
            text: text.to_string(),
        });
        self.changes.push(ChangeEvent {
            frame: self.frame + 1,
            change: Change::File {
                path: name.to_string(),
                text: text.to_string(),
            },
        });
        if self.state == PlayState::Frozen {
            self.step(0, &[]);
        }
    }

    /// New `ctx.tune` values (Feel), from now on. Kept in the recording.
    pub fn send_tunes(&mut self, values: BTreeMap<String, f32>) {
        if self.hello.is_none() || !self.is_running() || values.is_empty() {
            return;
        }
        self.send(&ToGame::Tune {
            values: values.clone(),
        });
        self.changes.push(ChangeEvent {
            frame: self.frame + 1,
            change: Change::Tune(values),
        });
        if self.state == PlayState::Frozen {
            self.step(0, &[]);
        }
    }

    /// The game's answers about changed files since this was last asked:
    /// each file's name, and whether the game took the change.
    pub fn take_file_answers(&mut self) -> Vec<(String, bool)> {
        std::mem::take(&mut self.file_answers)
    }

    /// Has the game draw its debug overlay (F1) on its pictures, or stop.
    /// A frozen game draws its picture again, so it shows at once.
    pub fn set_debug_overlay(&mut self, on: bool) {
        self.debug_overlay = on;
        if self.hello.is_some() {
            self.send(&ToGame::Overlay { debug: on });
            if self.state == PlayState::Frozen {
                self.step(0, &[]);
            }
        }
    }

    pub fn is_running(&self) -> bool {
        !matches!(self.state, PlayState::Stopped { .. })
    }

    /// Lets the session take in whatever has happened: the game
    /// connecting, messages from it, its end. Never waits. Returns true if
    /// there's a new picture.
    pub fn pump(&mut self, console: &Console) -> bool {
        if self.state == PlayState::Starting {
            self.try_to_connect(console);
        }
        let mut new_picture = false;
        loop {
            let heard = match &self.heard {
                Some(heard) => heard.try_recv().ok(),
                None => None,
            };
            let Some(heard) = heard else {
                break;
            };
            new_picture |= self.take(heard, console);
        }
        new_picture
    }

    /// Waits up to `patience` for the picture of the tick just sent. If it
    /// comes, the Studio shows it in this same frame of its own: no added
    /// delay. If not, the last picture stays for one more frame. Returns
    /// true if there's a new picture.
    pub fn wait_for_picture(&mut self, console: &Console, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        let mut new_picture = self.pump(console);
        while self.waiting.is_some() && self.is_running() {
            let left = deadline.saturating_duration_since(Instant::now());
            let heard = match &self.heard {
                Some(heard) => heard.recv_timeout(left),
                None => break,
            };
            match heard {
                Ok(heard) => new_picture |= self.take(heard, console),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    new_picture |= self.take(Heard::Closed, console);
                    break;
                }
            }
        }
        if let Some(waiting) = &mut self.waiting {
            waiting.late = true;
        }
        new_picture
    }

    /// The start of one of the Studio's own frames, at `now`, with what
    /// the player did since the last one. While playing, this sends the
    /// game its tick.
    pub fn begin_frame(&mut self, now: Instant, actions: &[InputAction]) {
        if self.state != PlayState::Playing {
            return; // frozen (or not there): inputs go nowhere
        }
        for &action in actions {
            self.note_held(action);
        }
        self.pending.extend_from_slice(actions);
        if let Some(waiting) = self.waiting {
            // The game is still working on the last tick: don't pile up
            // another. The time isn't lost: the next tick asks for the
            // updates owed (never more than three, and nothing at all
            // after a gap longer than a stall), so a game that draws
            // slowly still runs at its own speed, as in its own window.
            self.not_answering = now.saturating_duration_since(waiting.sent) > NOT_ANSWERING;
            return;
        }
        let updates = self.clock.due(now);
        if updates == 0 {
            return; // a fast screen: nothing due on this frame
        }
        self.send_tick(updates, now);
    }

    /// Sends a tick with the pending inputs, and records them.
    fn send_tick(&mut self, updates: u32, now: Instant) {
        // A key pressed and let go since the last update would never be
        // seen by the game (the two cancel out), so its release waits for
        // the next tick: the game sees a one-frame tap.
        let mut now_events = Vec::new();
        let mut later = Vec::new();
        for action in std::mem::take(&mut self.pending) {
            let tapped = match action {
                InputAction::Release(key) => now_events.contains(&InputAction::Press(key)),
                InputAction::MouseRelease(button) => {
                    now_events.contains(&InputAction::MousePress(button))
                }
                _ => false,
            };
            if tapped {
                later.push(action);
            } else {
                now_events.push(action);
            }
        }
        self.pending = later;
        let events: Vec<InputEvent> = now_events
            .into_iter()
            .map(|action| InputEvent {
                frame: self.frame + 1,
                action,
            })
            .collect();
        let id = self.next_id;
        self.next_id += 1;
        self.events.extend(events.iter().copied());
        self.send(&ToGame::Tick {
            id,
            updates,
            events,
        });
        self.waiting = Some(Waiting {
            id,
            sent: now,
            updates,
            late: false,
        });
    }

    fn note_held(&mut self, action: InputAction) {
        match action {
            InputAction::Press(key) if !self.held_keys.contains(&key) => self.held_keys.push(key),
            InputAction::Release(key) => self.held_keys.retain(|held| *held != key),
            InputAction::MousePress(button) if !self.held_buttons.contains(&button) => {
                self.held_buttons.push(button);
            }
            InputAction::MouseRelease(button) => self.held_buttons.retain(|held| *held != button),
            _ => {}
        }
    }

    /// Freezes the game on the frame it's on.
    pub fn freeze(&mut self) {
        if self.state == PlayState::Playing {
            self.state = PlayState::Frozen;
            self.not_answering = false;
        }
    }

    /// Carries on from the frozen frame. `keys_down` and `buttons_down`
    /// are what's really held right now: anything the game still thinks
    /// is held but isn't, is let go on the first new update, so nothing
    /// sticks.
    pub fn resume(&mut self, keys_down: &[Key], buttons_down: &[MouseButton]) {
        if self.state != PlayState::Frozen {
            return;
        }
        let let_go: Vec<InputAction> = self
            .held_keys
            .iter()
            .filter(|key| !keys_down.contains(key))
            .map(|&key| InputAction::Release(key))
            .chain(
                self.held_buttons
                    .iter()
                    .filter(|button| !buttons_down.contains(button))
                    .map(|&button| InputAction::MouseRelease(button)),
            )
            .collect();
        for &action in &let_go {
            self.note_held(action);
        }
        self.pending.extend(let_go);
        self.clock.restart();
        self.state = PlayState::Playing;
    }

    /// While frozen: plays exactly `updates` more frames, with `actions`
    /// applied first, and stays frozen.
    pub fn step(&mut self, updates: u32, actions: &[InputAction]) {
        if self.state != PlayState::Frozen || self.waiting.is_some() {
            return;
        }
        for &action in actions {
            self.note_held(action);
        }
        self.pending.extend_from_slice(actions);
        self.send_tick(updates, Instant::now());
    }

    /// Queues one input for the game: it goes with the next tick (or the
    /// next step, if the game is frozen). Scripts and tests use this.
    pub fn queue(&mut self, action: InputAction) {
        self.note_held(action);
        self.pending.push(action);
    }

    /// Asks the game to open its own window, on the frame it's on.
    pub fn pop_out(&mut self) {
        if matches!(self.state, PlayState::Playing | PlayState::Frozen) {
            self.send(&ToGame::Window { open: true });
        }
    }

    /// Asks the game to close its own window and come back.
    pub fn bring_back(&mut self) {
        if self.state == PlayState::PoppedOut {
            self.send(&ToGame::Window { open: false });
        }
    }

    /// Ends the game politely: its `quit` runs. If it doesn't go within a
    /// second, it's stopped.
    pub fn stop(&mut self, console: &mut Console) {
        if !self.is_running() {
            return;
        }
        self.send(&ToGame::Quit);
        let deadline = Instant::now() + Duration::from_secs(1);
        while self.is_running() && Instant::now() < deadline {
            console.poll();
            self.pump(console);
            std::thread::sleep(Duration::from_millis(2));
        }
        console.stop(self.job);
        if self.is_running() {
            self.state = PlayState::Stopped {
                ending: Ending::Asked,
                problem: None,
            };
        }
        self.writer = None;
    }

    /// The session so far as an input script: what the game itself
    /// recorded if it answers within `patience` (that has the values it
    /// reported at the end, too), or else what the Studio sent.
    pub fn recording(&mut self, console: &Console, patience: Duration) -> InputScript {
        if !self.is_running() || self.state == PlayState::Starting {
            return self.own_recording();
        }
        let id = self.next_id;
        self.next_id += 1;
        self.send(&ToGame::Ask {
            id,
            what: String::from("recording"),
        });
        let deadline = Instant::now() + patience;
        while Instant::now() < deadline && self.is_running() {
            self.pump(console);
            if let Some(at) = self.answers.iter().position(|(answer, _)| *answer == id) {
                let (_, text) = self.answers.remove(at);
                if let Ok(script) = InputScript::parse(&text) {
                    return script;
                }
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.own_recording()
    }

    /// The session as the Studio itself recorded it: every input it sent,
    /// by frame. It's there even after the game has crashed.
    fn own_recording(&self) -> InputScript {
        InputScript {
            seed: self.hello.as_ref().map(|hello| hello.seed),
            updates_per_second: self
                .hello
                .as_ref()
                .map(|hello| hello.updates_per_second.round() as u32),
            frames: Some(self.frame),
            events: self.events.clone(),
            expect: None,
            tune: None,
            files: self.files_at_start.clone(),
            changes: self.changes.clone(),
        }
    }

    /// Saves the session as an input script in the game's sessions
    /// folder, and as `latest.json` there, and says where.
    pub fn save_session(
        &mut self,
        console: &Console,
        folder: &GamesFolder,
    ) -> Result<PathBuf, Problem> {
        let script = self.recording(console, Duration::from_millis(500));
        let sessions = folder.sessions_folder(&self.game);
        let cant = |problem: String| {
            Problem::new(
                "The Studio couldn't save this session's recording.",
                "Check that the disk isn't full, then try again.",
            )
            .with_details(problem)
        };
        std::fs::create_dir_all(&sessions).map_err(|e| cant(e.to_string()))?;
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let path = sessions.join(format!("session-{stamp}.json"));
        script.save(&path).map_err(cant)?;
        script.save(sessions.join("latest.json")).map_err(cant)?;
        Ok(path)
    }

    /// The Timing line for the five seconds up to `now`.
    pub fn timing(&self, now: Instant) -> Timing {
        let window = Duration::from_secs(5);
        let recent: Vec<&Sample> = self
            .samples
            .iter()
            .filter(|sample| now.saturating_duration_since(sample.at) <= window)
            .collect();
        let pictures: Vec<Instant> = self
            .pictures
            .iter()
            .copied()
            .filter(|at| now.saturating_duration_since(*at) <= window)
            .collect();
        if recent.is_empty() || pictures.len() < 2 {
            return Timing::default();
        }
        let span = pictures[pictures.len() - 1]
            .duration_since(pictures[0])
            .as_secs_f32()
            .max(0.001);
        let count = recent.len() as f32;
        let round_trip = recent.iter().map(|s| s.round_trip).sum::<f32>() / count;
        let game = recent.iter().map(|s| s.game).sum::<f32>() / count;
        let updates: u32 = recent.iter().skip(1).map(|s| s.updates).sum();
        let ticks_span = recent[recent.len() - 1]
            .at
            .duration_since(recent[0].at)
            .as_secs_f32()
            .max(0.001);
        Timing {
            updates_per_second: updates as f32 / ticks_span,
            pictures_per_second: (pictures.len() - 1) as f32 / span,
            longest_gap: pictures
                .windows(2)
                .map(|pair| pair[1].duration_since(pair[0]).as_secs_f32() * 1000.0)
                .fold(0.0, f32::max),
            game,
            link: (round_trip - game).max(0.0),
            slowest: recent.iter().map(|s| s.round_trip).fold(0.0, f32::max),
            late: recent.iter().filter(|s| !s.on_time).count() as u32,
            ticks: recent.len() as u32,
        }
    }

    /// What draws the game, in words for a person.
    pub fn drawn_by(&self) -> String {
        match self.renderer.strip_prefix("GPU: ") {
            Some(card) => format!("the graphics card ({card})"),
            None if self.renderer.starts_with("GPU") => {
                // Asked for, never needed: the game has drawn no 3D.
                String::from("the CPU (this game draws no 3D)")
            }
            None if self.renderer.is_empty() => String::from("nothing yet"),
            None => String::from("the CPU"),
        }
    }

    /// The inputs sent so far, by frame.
    pub fn events(&self) -> &[InputEvent] {
        &self.events
    }

    fn send(&mut self, message: &ToGame) {
        if let Some(writer) = &mut self.writer
            && message.write(writer).is_err()
        {
            self.writer = None; // the reader notices the game has gone
        }
    }

    /// Takes the game's call, if it has connected by now.
    fn try_to_connect(&mut self, console: &Console) {
        let Some(listener) = &self.listener else {
            return;
        };
        let stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(_) => {
                // Not yet. Has the game ended before it got this far?
                let ended = matches!(
                    console.entry(self.job).map(|entry| &entry.state),
                    Some(JobState::Ended(_) | JobState::CouldntStart(_)) | None
                );
                if ended || self.started.elapsed() > STARTING_PATIENCE {
                    self.listener = None;
                    self.end(console);
                }
                return;
            }
        };
        self.listener = None;
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_nodelay(true);
        let Ok(reading) = stream.try_clone() else {
            self.end(console);
            return;
        };
        let (sender, heard) = channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::with_capacity(1 << 20, reading);
            loop {
                match ToStudio::read(&mut reader) {
                    Ok(Some(message)) => {
                        let heard = Heard::Message(Box::new(message), Instant::now());
                        if sender.send(heard).is_err() {
                            break;
                        }
                    }
                    Ok(None) | Err(_) => {
                        let _ = sender.send(Heard::Closed);
                        break;
                    }
                }
            }
        });
        self.writer = Some(stream);
        self.heard = Some(heard);
    }

    /// Takes in one thing heard from the game. Returns true for a new picture.
    fn take(&mut self, heard: Heard, console: &Console) -> bool {
        let (message, arrived) = match heard {
            Heard::Message(message, arrived) => (*message, arrived),
            Heard::Closed => {
                self.end(console);
                return false;
            }
        };
        match message {
            ToStudio::Hello(hello) => {
                if hello.link != LINK_VERSION || hello.key != self.key {
                    self.bye = Some(String::from("the wrong game answered"));
                    self.writer = None;
                    return false;
                }
                self.clock = StudioClock::new(hello.updates_per_second);
                self.hello = Some(hello);
                if self.debug_overlay {
                    self.send(&ToGame::Overlay { debug: true });
                }
                match std::mem::replace(&mut self.then, AfterHello::Play) {
                    AfterHello::Play => {
                        self.state = PlayState::Playing;
                    }
                    AfterHello::Freeze => {
                        // Draw the game as it starts, before any update.
                        self.state = PlayState::Frozen;
                        self.send_tick(0, Instant::now());
                    }
                    AfterHello::Replay(script, frames) => {
                        self.state = PlayState::Replaying;
                        self.events = script
                            .events
                            .iter()
                            .copied()
                            .filter(|event| event.frame <= frames)
                            .collect();
                        self.changes = script
                            .changes
                            .iter()
                            .filter(|change| change.frame <= frames)
                            .cloned()
                            .collect();
                        let id = self.next_id;
                        self.next_id += 1;
                        self.send(&ToGame::Run {
                            id,
                            updates: frames,
                            script: Some(script.to_json()),
                        });
                        self.waiting = Some(Waiting {
                            id,
                            sent: Instant::now(),
                            updates: 0,
                            late: false,
                        });
                    }
                }
                false
            }
            ToStudio::Frame(frame) => {
                self.take_frame(frame, arrived);
                true
            }
            ToStudio::Answer { id, what, text } => {
                match what.as_str() {
                    "tunes" => {
                        if let Ok(tunes) = serde_json::from_str(&text) {
                            self.tunes = tunes;
                        }
                    }
                    "sounds" => {
                        if let Ok(sounds) = serde_json::from_str(&text) {
                            self.sounds = sounds;
                        }
                    }
                    _ => self.answers.push((id, text)),
                }
                false
            }
            ToStudio::Note(note) => {
                if !self.notes.contains(&note) {
                    self.notes.push(note);
                }
                false
            }
            ToStudio::Window { open, frame } => {
                self.frame = frame;
                self.waiting = None;
                self.not_answering = false;
                if open {
                    self.state = PlayState::PoppedOut;
                } else if self.state == PlayState::PoppedOut {
                    // Back inside: carry on playing, starting the clock afresh.
                    self.held_keys.clear();
                    self.held_buttons.clear();
                    self.clock.restart();
                    self.state = PlayState::Playing;
                }
                false
            }
            ToStudio::Changed { path, reloaded } => {
                self.file_answers.push((path, reloaded));
                false
            }
            ToStudio::Bye { why } => {
                self.bye = Some(why);
                false
            }
        }
    }

    /// Takes in a picture that arrived at `now`.
    fn take_frame(&mut self, frame: Frame, now: Instant) {
        if let Some(waiting) = self.waiting.filter(|waiting| waiting.id == frame.tick) {
            self.waiting = None;
            self.not_answering = false;
            if self.state == PlayState::Replaying {
                self.state = PlayState::Frozen; // caught up: wait here
            } else {
                self.samples.push_back(Sample {
                    at: now,
                    round_trip: now.duration_since(waiting.sent).as_secs_f32() * 1000.0,
                    game: (frame.update_micros + frame.draw_micros) as f32 / 1000.0,
                    on_time: !waiting.late,
                    updates: waiting.updates,
                });
                while self.samples.len() > 2000 {
                    self.samples.pop_front();
                }
            }
        }
        if frame.frame != self.frame || self.picture.is_none() {
            self.pictures.push_back(now);
            while self.pictures.len() > 2000 {
                self.pictures.pop_front();
            }
        }
        self.frame = frame.frame;
        self.wants_pointer = frame.wants_pointer;
        self.renderer = frame.renderer;
        self.maps = frame.maps;
        self.serial += 1;
        self.picture = Some(Picture {
            width: frame.width,
            height: frame.height,
            pixels: frame.pixels,
            frame: frame.frame,
            serial: self.serial,
        });
    }

    /// The game is gone: works out why, and says it plainly.
    fn end(&mut self, console: &Console) {
        if !self.is_running() {
            return;
        }
        self.writer = None;
        self.waiting = None;
        self.not_answering = false;
        let (ending, problem) = match self.bye.as_deref() {
            Some("asked") => (Ending::Asked, None),
            Some("quit") => (Ending::Quit, None),
            said => {
                let lines: Vec<String> = console
                    .entry(self.job)
                    .map(|entry| entry.lines.clone())
                    .unwrap_or_default();
                let what = match (said, self.hello.is_some()) {
                    (Some(why), _) => format!("The game stopped: {why}."),
                    (None, false) => String::from("The game stopped before it had started."),
                    (None, true) => String::from("The game stopped."),
                };
                let reason = crash_reason(&lines, self.frame);
                let problem = Problem::new(
                    format!("{what} {reason}"),
                    "Press Restart to start it again, or Restart and replay my moves to get \
                     back to where you were.",
                )
                .with_details(lines.join("\n"));
                (Ending::Crashed, Some(problem))
            }
        };
        self.state = PlayState::Stopped { ending, problem };
    }
}

/// Why a game crashed, in a sentence, from what it printed: Rust's own
/// "panicked at" lines say the file, the line and the reason.
pub fn crash_reason(lines: &[String], frame: u64) -> String {
    let panicked = lines.iter().rposition(|line| line.contains("panicked at "));
    if let Some(at) = panicked {
        let place = lines[at]
            .rsplit("panicked at ")
            .next()
            .unwrap_or_default()
            .trim_end_matches(':');
        let mut parts = place.split(':');
        let file = parts.next().unwrap_or_default();
        let file = file.rsplit(['/', '\\']).next().unwrap_or(file);
        let line = parts.next().unwrap_or_default();
        let message = lines
            .get(at + 1)
            .map(|line| line.trim())
            .unwrap_or_default();
        let on_frame = if frame > 0 {
            format!(" on frame {frame}")
        } else {
            String::new()
        };
        return format!("Its code hit a problem{on_frame}: \"{message}\" in {file}, line {line}.");
    }
    // A game that couldn't start says why in a line starting "Error:".
    match lines
        .iter()
        .rev()
        .find(|line| line.trim_start().starts_with("Error:"))
    {
        Some(error) => format!(
            "It said: {}",
            error.trim().trim_start_matches("Error:").trim()
        ),
        None => String::from(
            "It didn't say why. (That's what it looks like when a game is closed from outside \
             the Studio.)",
        ),
    }
}

/// A key only this Studio and the game it starts know.
fn one_time_key() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let here = &nanos as *const u128 as usize as u128;
    format!(
        "{:x}",
        nanos ^ (here << 64) ^ u128::from(std::process::id())
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_gives_one_update_a_frame_on_a_60_hz_screen() {
        let mut clock = StudioClock::new(60.0);
        let start = Instant::now();
        let mut total = 0;
        for frame in 0..600u64 {
            // Frames a little early and a little late, as real ones are.
            let wobble = [0u64, 1500, 0, 0, 2500, 500][(frame % 6) as usize];
            let now = start + Duration::from_micros(frame * 16_667 + wobble);
            let updates = clock.due(now);
            assert_eq!(updates, 1, "frame {frame}");
            total += updates;
        }
        assert_eq!(total, 600);
    }

    #[test]
    fn the_clock_keeps_the_games_speed_on_faster_and_slower_screens() {
        for (hz, frames) in [(144.0, 1440u64), (120.0, 1200), (30.0, 300), (75.0, 750)] {
            let mut clock = StudioClock::new(60.0);
            let start = Instant::now();
            let total: u32 = (0..frames)
                .map(|frame| clock.due(start + Duration::from_secs_f64(frame as f64 / hz)))
                .sum();
            assert!(
                (598..=602).contains(&total),
                "ten seconds at {hz} Hz ran {total} updates"
            );
        }
    }

    #[test]
    fn a_blocked_studio_freezes_the_game_and_never_bursts_afterwards() {
        let mut clock = StudioClock::new(60.0);
        let start = Instant::now();
        let ms = |n: u64| start + Duration::from_millis(n);
        assert_eq!(clock.due(ms(0)), 1);
        assert_eq!(clock.due(ms(17)), 1);
        assert_eq!(clock.due(ms(33)), 1);
        // The window is dragged for three seconds: the Studio draws nothing.
        assert_eq!(clock.due(ms(3033)), 1, "one update, not 180");
        assert_eq!(clock.due(ms(3050)), 1, "and straight back to one a frame");
        assert_eq!(clock.due(ms(3067)), 1);
        // A short hiccup (a 20th of a second) is caught up on, but capped.
        assert_eq!(clock.due(ms(3117)), 3);
        assert_eq!(clock.due(ms(3134)), 1);
        // Just under the stall limit: still capped at three, and the rest
        // is dropped rather than saved for later.
        assert_eq!(clock.due(ms(3230)), 3);
        assert_eq!(clock.due(ms(3247)), 1);
        assert_eq!(clock.due(ms(3263)), 1);
    }

    #[test]
    fn a_crash_is_explained_from_what_the_game_printed() {
        let lines: Vec<String> = [
            "duckforge: no sound",
            "thread 'main' panicked at examples/dodge.rs:210:17:",
            "index out of bounds: the len is 3 but the index is 7",
            "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
        ]
        .iter()
        .map(|line| line.to_string())
        .collect();
        assert_eq!(
            crash_reason(&lines, 412),
            "Its code hit a problem on frame 412: \"index out of bounds: the len is 3 but the \
             index is 7\" in dodge.rs, line 210."
        );
        let quiet = vec![
            String::from("Error: couldn't load examples/x/hero.png"),
            String::from("duckforge: sound: 0 late refills in 2 seconds"),
        ];
        assert_eq!(
            crash_reason(&quiet, 0),
            "It said: couldn't load examples/x/hero.png"
        );
        // Lines that aren't about the ending aren't quoted as if they were.
        let unrelated = vec![String::from("duckforge: no sound: no sound card")];
        assert!(crash_reason(&unrelated, 0).starts_with("It didn't say why."));
        assert!(crash_reason(&[], 0).starts_with("It didn't say why."));
    }
}
