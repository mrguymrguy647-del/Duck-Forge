//! The game's side of the link: plays the game for the Studio.

use std::error::Error;
use std::io::{self, BufReader, BufWriter};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use super::protocol::{Frame, Hello, KEY_VARIABLE, LINK_VERSION, ToGame, ToStudio};
use crate::cli::Options;
use crate::engine::{Config, EngineError, Game, RunMode, Timestep};
use crate::rng;
use crate::script::InputScript;
use crate::simulation::Simulation;

/// If the Studio is quiet for this long while the game is inside it, the
/// game is frozen (or the Studio can't draw: its window is being dragged),
/// so the sound is held until the next tick.
const QUIET: Duration = Duration::from_millis(100);

/// The sound, where the game was built with it.
#[cfg(feature = "audio")]
type Speaker = Option<crate::speaker::Speaker>;
/// Built without sound: nothing to play through.
#[cfg(not(feature = "audio"))]
struct Speaker;

#[cfg(feature = "audio")]
fn open_speaker() -> Speaker {
    match crate::speaker::Speaker::open() {
        Ok(speaker) => Some(speaker),
        Err(problem) => {
            println!("duckforge: no sound: {problem}; the game carries on silently");
            None
        }
    }
}
#[cfg(not(feature = "audio"))]
fn open_speaker() -> Speaker {
    Speaker
}

/// Holds the sound (the game is frozen) or lets it carry on.
fn pause_sound(speaker: &Speaker, paused: bool) {
    #[cfg(feature = "audio")]
    if let Some(speaker) = speaker {
        speaker.pause(paused);
    }
    #[cfg(not(feature = "audio"))]
    let _ = (speaker, paused);
}

/// Sends the sounds the last updates played to the speakers. After a fast
/// run only the loops are passed on: hundreds of old coin sounds at once
/// would be a bang.
fn play_new_sounds<G: Game>(sim: &mut Simulation<G>, speaker: &Speaker, one_shots: bool) {
    let sounds = sim.take_new_sounds();
    let loops = sim.take_new_loop_commands();
    #[cfg(feature = "audio")]
    if let Some(speaker) = speaker {
        if one_shots {
            for sound in sounds {
                speaker.play(sound.samples, sound.pan);
            }
        }
        for command in loops {
            speaker.change_loop(command);
        }
    }
    #[cfg(not(feature = "audio"))]
    let _ = (speaker, one_shots, sounds, loops);
}

/// The connection, as the game uses it.
struct Link {
    stream: TcpStream,
    reader: BufReader<TcpStream>,
    writer: BufWriter<TcpStream>,
}

/// What listening for the Studio's next message gave.
enum Heard {
    Message(ToGame),
    /// Nothing yet.
    Quiet,
    /// The Studio closed the connection (or it broke).
    Gone,
}

impl Link {
    fn connect(address: &str) -> Result<Link, String> {
        let stream = TcpStream::connect(address).map_err(|err| {
            format!(
                "couldn't reach Duck Forge Studio at {address} ({err}). --studio is the Studio's \
                 own option: it starts games with it. To play in a window, leave it out."
            )
        })?;
        let _ = stream.set_nodelay(true); // small messages go at once
        let copy = |stream: &TcpStream| {
            stream
                .try_clone()
                .map_err(|err| format!("couldn't set up the link to the Studio ({err})"))
        };
        Ok(Link {
            reader: BufReader::new(copy(&stream)?),
            // Big enough for a whole picture, so it goes in one piece.
            writer: BufWriter::with_capacity(1 << 20, copy(&stream)?),
            stream,
        })
    }

    fn send(&mut self, message: &ToStudio) -> io::Result<()> {
        message.write(&mut self.writer)
    }

    /// Waits up to `patience` for the Studio's next message (not at all
    /// with `Duration::ZERO`).
    fn listen(&mut self, patience: Duration) -> Heard {
        if self.reader.buffer().is_empty() {
            // Look before reading, so a wait that runs out can never cut a
            // message in half.
            let waited = if patience.is_zero() {
                let _ = self.stream.set_nonblocking(true);
                let waited = self.stream.peek(&mut [0u8; 1]);
                let _ = self.stream.set_nonblocking(false);
                waited
            } else {
                let _ = self.stream.set_read_timeout(Some(patience));
                let waited = self.stream.peek(&mut [0u8; 1]);
                let _ = self.stream.set_read_timeout(None);
                waited
            };
            match waited {
                Ok(0) => return Heard::Gone,
                Ok(_) => {}
                Err(err)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Heard::Quiet;
                }
                Err(_) => return Heard::Gone,
            }
        }
        match ToGame::read(&mut self.reader) {
            Ok(Some(message)) => Heard::Message(message),
            Ok(None) => Heard::Gone,
            Err(problem) => {
                eprintln!("duckforge: the link to the Studio broke: {problem}");
                Heard::Gone
            }
        }
    }
}

/// Draws the game as it is now and sends the picture to the Studio,
/// straight from the canvas.
fn send_picture<G: Game>(
    link: &mut Link,
    sim: &mut Simulation<G>,
    tick: u64,
    update_micros: u32,
) -> io::Result<()> {
    let started = Instant::now();
    let canvas = sim.picture(); // draws, if this frame hasn't been drawn yet
    let (width, height) = (canvas.width() as u32, canvas.height() as u32);
    let draw_micros = started.elapsed().as_micros().min(u32::MAX as u128) as u32;
    let header = Frame {
        frame: sim.context().frame(),
        tick,
        width,
        height,
        update_micros,
        draw_micros,
        wants_pointer: sim.context().pointer_locked(),
        renderer: sim.context().renderer_name().to_string(),
        pixels: Vec::new(),
    };
    header.write_with(&mut link.writer, sim.picture().pixels())
}

/// Plays `game` inside Duck Forge Studio, which is listening at `address`,
/// until the Studio says to stop, the game calls
/// [`Context::quit`](crate::engine::Context::quit), or the Studio goes away.
pub(crate) fn run<G: Game>(
    mut config: Config,
    game: G,
    options: &Options,
    address: &str,
) -> Result<(), Box<dyn Error>> {
    config.check().map_err(EngineError)?;
    // Pick the seed now, so the Studio can record which one this run used.
    let seed = options
        .seed
        .or(config.seed)
        .unwrap_or_else(rng::seed_from_clock);
    config.seed = Some(seed);
    let renderer = crate::render3d::renderer_from_env().unwrap_or(config.renderer);
    let tunes = crate::tune::Tunes::for_run(
        options.tune.as_deref(),
        config.tune_file.as_deref(),
        &options.set,
    )
    .map_err(EngineError)?;
    let mut sim = Simulation::start(&config, game, renderer);
    sim.set_run_mode(RunMode::Studio);
    sim.set_tunes(tunes);
    sim.forget_report_log();
    sim.set_debug_overlay(options.debug);

    let mut link = Link::connect(address).map_err(EngineError)?;
    link.send(&ToStudio::Hello(Hello {
        link: LINK_VERSION,
        key: std::env::var(KEY_VARIABLE).unwrap_or_default(),
        title: config.title.clone(),
        width: config.width as u32,
        height: config.height as u32,
        updates_per_second: match config.timestep {
            Timestep::Fixed { updates_per_second } => f64::from(updates_per_second.max(1)),
            Timestep::RealTime => 1.0 / f64::from(config.step_time()),
        },
        seed,
    }))?;
    let speaker = open_speaker();

    // The game's own window, once it has been popped out.
    #[cfg(feature = "window")]
    let mut window: Option<crate::window::PoppedOut> = None;
    let mut popped_out = false;
    let why = loop {
        // In its own window the game keeps its own time, one turn of the
        // window's loop after another, listening in between.
        #[cfg(feature = "window")]
        if popped_out {
            let Some(own) = &mut window else {
                unreachable!("popped out means the window is open");
            };
            match own.turn(&mut sim)? {
                Some(_) => play_new_sounds(&mut sim, &speaker, true),
                None => {
                    // The window's close button: come back inside the Studio.
                    own.set_visible(false);
                    popped_out = false;
                    link.send(&ToStudio::Window {
                        open: false,
                        frame: sim.context().frame(),
                    })?;
                    send_picture(&mut link, &mut sim, 0, 0)?;
                    continue;
                }
            }
        }
        let heard = link.listen(if popped_out { Duration::ZERO } else { QUIET });
        let message = match heard {
            Heard::Message(message) => message,
            Heard::Quiet => {
                if !popped_out {
                    pause_sound(&speaker, true);
                }
                continue;
            }
            Heard::Gone => break "the Studio went away",
        };
        match message {
            ToGame::Tick {
                id,
                updates,
                events,
            } => {
                if popped_out {
                    continue; // the game's own window is in charge
                }
                if updates > 0 {
                    pause_sound(&speaker, false);
                }
                for event in events {
                    sim.apply(event.action);
                }
                let started = Instant::now();
                for _ in 0..updates {
                    sim.step();
                    if sim.quit_requested() {
                        break;
                    }
                }
                let update_micros = started.elapsed().as_micros().min(u32::MAX as u128) as u32;
                play_new_sounds(&mut sim, &speaker, true);
                send_picture(&mut link, &mut sim, id, update_micros)?;
            }
            ToGame::Run {
                id,
                updates,
                script,
            } => {
                if let Some(text) = script {
                    if sim.context().frame() > 0 {
                        break "a script can only be followed from the start";
                    }
                    match InputScript::parse(&text) {
                        Ok(script) => sim.follow(script),
                        Err(problem) => {
                            eprintln!("duckforge: the Studio's script can't be read: {problem}");
                            break "the Studio sent a script the game can't read";
                        }
                    }
                }
                let started = Instant::now();
                for _ in 0..updates {
                    sim.step();
                    if sim.quit_requested() {
                        break;
                    }
                }
                let update_micros = started.elapsed().as_micros().min(u32::MAX as u128) as u32;
                play_new_sounds(&mut sim, &speaker, false);
                send_picture(&mut link, &mut sim, id, update_micros)?;
            }
            ToGame::Window { open } => {
                #[cfg(feature = "window")]
                {
                    if open && !popped_out {
                        if window.is_none() {
                            match crate::window::PoppedOut::open(&config) {
                                Ok(opened) => window = Some(opened),
                                Err(problem) => {
                                    link.send(&ToStudio::Note(format!(
                                        "The game couldn't open its own window ({problem})"
                                    )))?;
                                    continue;
                                }
                            }
                        }
                        if let Some(own) = &mut window {
                            own.set_visible(true);
                        }
                        pause_sound(&speaker, false);
                        popped_out = true;
                    } else if !open && popped_out {
                        if let Some(own) = &mut window {
                            own.set_visible(false);
                        }
                        popped_out = false;
                    }
                    link.send(&ToStudio::Window {
                        open: popped_out,
                        frame: sim.context().frame(),
                    })?;
                    if !popped_out {
                        send_picture(&mut link, &mut sim, 0, 0)?;
                    }
                }
                #[cfg(not(feature = "window"))]
                {
                    let _ = open;
                    link.send(&ToStudio::Note(String::from(
                        "This game was built without a window, so it can't pop out",
                    )))?;
                }
            }
            ToGame::Ask { id, what } => {
                let text = match what.as_str() {
                    "recording" => sim.recording().to_json(),
                    _ => String::new(),
                };
                link.send(&ToStudio::Answer { id, what, text })?;
            }
            ToGame::Quit => break "asked",
        }
        for note in sim.with_notes() {
            link.send(&ToStudio::Note(note))?;
        }
        if sim.quit_requested() {
            break "quit";
        }
    };
    #[cfg(feature = "window")]
    drop(window);
    #[cfg(not(feature = "window"))]
    let _ = &mut popped_out;
    // Every way out ends here: the game gets to say goodbye.
    sim.quit();
    let _ = link.send(&ToStudio::Bye {
        why: why.to_string(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;
    use crate::color::Color;
    use crate::engine::Context;
    use crate::input::Key;
    use crate::math::{Rect, vec2};
    use crate::script::{InputAction, InputEvent};
    use std::net::TcpListener;

    /// A box that moves right while Right is held and jumps on Space, with
    /// a little randomness, so a wrong frame or seed shows.
    #[derive(Default)]
    struct Hopper {
        x: f32,
        y: f32,
        speed_y: f32,
        said_goodbye: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Game for Hopper {
        fn init(&mut self, ctx: &mut Context) {
            ctx.studio_note("hello from the hopper");
            assert!(ctx.in_studio() && !ctx.is_headless());
        }

        fn update(&mut self, ctx: &mut Context) {
            if ctx.input.is_down(Key::Right) {
                self.x += 1.0 + ctx.rng.range(0.0, 0.5);
            }
            if ctx.input.was_pressed(Key::Space) && self.y == 0.0 {
                self.speed_y = 4.0;
            }
            self.y = (self.y + self.speed_y).max(0.0);
            self.speed_y = if self.y > 0.0 {
                self.speed_y - 0.25
            } else {
                0.0
            };
            if ctx.input.was_pressed(Key::Escape) {
                ctx.quit();
            }
            ctx.report("x", self.x);
            ctx.report("y", self.y);
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            canvas.fill_rect(Rect::new(self.x, 40.0 - self.y, 4.0, 4.0), Color::WHITE);
        }

        fn quit(&mut self, _: &mut Context) {
            self.said_goodbye
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn config() -> Config {
        Config {
            width: 64,
            height: 48,
            seed: Some(5),
            ..Config::default()
        }
    }

    /// The Studio's end of the link, for a game running on another thread.
    struct Studio {
        reader: BufReader<TcpStream>,
        writer: TcpStream,
        game: std::thread::JoinHandle<Result<(), String>>,
        goodbye: std::sync::Arc<std::sync::atomic::AtomicBool>,
        next: u64,
    }

    impl Studio {
        fn start() -> (Studio, Hello) {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap().to_string();
            let hopper = Hopper::default();
            let goodbye = hopper.said_goodbye.clone();
            let game = std::thread::spawn(move || {
                run(config(), hopper, &Options::default(), &address).map_err(|e| e.to_string())
            });
            let (stream, _) = listener.accept().unwrap();
            let mut studio = Studio {
                reader: BufReader::new(stream.try_clone().unwrap()),
                writer: stream,
                game,
                goodbye,
                next: 1,
            };
            let ToStudio::Hello(hello) = studio.hear() else {
                panic!("a game says hello first");
            };
            (studio, hello)
        }

        fn hear(&mut self) -> ToStudio {
            ToStudio::read(&mut self.reader)
                .unwrap()
                .expect("the game is still there")
        }

        /// The next picture, skipping notes.
        fn hear_frame(&mut self) -> Frame {
            loop {
                if let ToStudio::Frame(frame) = self.hear() {
                    return frame;
                }
            }
        }

        fn tick(&mut self, updates: u32, actions: &[InputAction]) -> Frame {
            let events = actions
                .iter()
                // The frame is only for the Studio's records: the game applies
                // a tick's inputs before that tick's first update.
                .map(|&action| InputEvent { frame: 1, action })
                .collect();
            let id = self.next;
            self.next += 1;
            ToGame::Tick {
                id,
                updates,
                events,
            }
            .write(&mut self.writer)
            .unwrap();
            let frame = self.hear_frame();
            assert_eq!(frame.tick, id, "the picture answers this tick");
            frame
        }

        fn recording(&mut self) -> InputScript {
            ToGame::Ask {
                id: 77,
                what: String::from("recording"),
            }
            .write(&mut self.writer)
            .unwrap();
            loop {
                if let ToStudio::Answer { id: 77, text, .. } = self.hear() {
                    return InputScript::parse(&text).unwrap();
                }
            }
        }

        /// Asks the game to end, and waits until it has.
        fn stop(mut self) -> String {
            ToGame::Quit.write(&mut self.writer).unwrap();
            let why = loop {
                if let ToStudio::Bye { why } = self.hear() {
                    break why;
                }
            };
            self.game.join().unwrap().unwrap();
            why
        }
    }

    /// The same moves on a plain simulation, to compare with.
    fn played_directly(moves: &[(u32, &[InputAction])]) -> Simulation<Hopper> {
        let mut sim = Simulation::new(config(), Hopper::default());
        sim.set_run_mode(RunMode::Studio);
        for (updates, actions) in moves {
            for &action in *actions {
                sim.apply(action);
            }
            sim.run(u64::from(*updates));
        }
        sim
    }

    #[test]
    fn a_game_played_through_the_link_plays_exactly_as_it_does_directly() {
        let moves: [(u32, &[InputAction]); 5] = [
            (1, &[InputAction::Press(Key::Right)]),
            (20, &[]),
            (1, &[InputAction::Press(Key::Space)]),
            (7, &[InputAction::Release(Key::Space)]),
            (30, &[InputAction::Release(Key::Right)]),
        ];
        let (mut studio, hello) = Studio::start();
        assert_eq!((hello.width, hello.height, hello.seed), (64, 48, 5));
        assert_eq!(hello.updates_per_second, 60.0);
        let mut last = Frame::default();
        for (updates, actions) in moves {
            last = studio.tick(updates, actions);
        }
        let mut direct = played_directly(&moves);
        assert_eq!(last.frame, 59);
        assert_eq!(last.pixels, direct.picture().pixels(), "the same picture");
        assert_eq!((last.width, last.height), (64, 48));
        // What the game recorded is what a recording of the direct run holds.
        let recording = studio.recording();
        assert_eq!(recording.events, direct.recording().events);
        assert_eq!(recording.expect, direct.recording().expect);
        assert_eq!(recording.seed, Some(5));
        assert_eq!(studio.stop(), "asked");
    }

    #[test]
    fn freezing_is_just_not_ticking_and_carries_on_from_the_same_frame() {
        let (mut studio, _) = Studio::start();
        studio.tick(1, &[InputAction::Press(Key::Right)]);
        studio.tick(
            1,
            &[
                InputAction::Press(Key::Space),
                InputAction::Release(Key::Right),
            ],
        );
        let mid_jump = studio.tick(6, &[]);
        // Frozen: no ticks for longer than the game waits before it holds
        // the sound. Asking for the picture again changes nothing.
        std::thread::sleep(QUIET * 3);
        let still = studio.tick(0, &[]);
        assert_eq!(still.frame, mid_jump.frame, "no update ran");
        assert_eq!(still.pixels, mid_jump.pixels, "and the picture is the same");
        let after = studio.tick(10, &[]);
        let mut direct = played_directly(&[
            (1, &[InputAction::Press(Key::Right)]),
            (
                1,
                &[
                    InputAction::Press(Key::Space),
                    InputAction::Release(Key::Right),
                ],
            ),
            (16, &[]),
        ]);
        assert_eq!(after.frame, 18);
        assert_eq!(after.pixels, direct.picture().pixels());
        studio.stop();
    }

    #[test]
    fn a_fast_run_follows_a_script_and_draws_once() {
        // Record a short game, then start again and run the recording fast.
        let (mut first, _) = Studio::start();
        first.tick(1, &[InputAction::Press(Key::Right)]);
        first.tick(1, &[InputAction::Press(Key::Space)]);
        let wanted = first.tick(40, &[InputAction::Release(Key::Space)]);
        let script = first.recording();
        first.stop();

        let (mut second, _) = Studio::start();
        ToGame::Run {
            id: 9,
            updates: 42,
            script: Some(script.to_json()),
        }
        .write(&mut second.writer)
        .unwrap();
        let caught_up = second.hear_frame();
        assert_eq!((caught_up.tick, caught_up.frame), (9, 42));
        assert_eq!(caught_up.pixels, wanted.pixels, "back where it was");
        second.stop();
    }

    #[test]
    fn the_game_gives_its_note_and_says_goodbye_however_it_ends() {
        // Asked to stop.
        let (mut studio, _) = Studio::start();
        studio.tick(1, &[]);
        assert!(matches!(studio.hear(), ToStudio::Note(note) if note == "hello from the hopper"));
        let goodbye = studio.goodbye.clone();
        assert_eq!(studio.stop(), "asked");
        assert!(goodbye.load(std::sync::atomic::Ordering::SeqCst));

        // The game quits by itself (Escape).
        let (mut studio, _) = Studio::start();
        studio.tick(1, &[InputAction::Press(Key::Escape)]);
        let why = loop {
            if let ToStudio::Bye { why } = studio.hear() {
                break why;
            }
        };
        assert_eq!(why, "quit");
        studio.game.join().unwrap().unwrap();
        assert!(studio.goodbye.load(std::sync::atomic::Ordering::SeqCst));

        // The Studio goes away without a word.
        let (studio, _) = Studio::start();
        let Studio {
            reader,
            writer,
            game,
            goodbye,
            ..
        } = studio;
        drop((reader, writer));
        game.join().unwrap().unwrap();
        assert!(goodbye.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn a_game_with_nowhere_to_connect_says_so_plainly() {
        let problem = run(
            config(),
            Hopper::default(),
            &Options::default(),
            "127.0.0.1:1",
        )
        .unwrap_err()
        .to_string();
        assert!(
            problem.contains("couldn't reach Duck Forge Studio"),
            "{problem}"
        );
        let _ = vec2(0.0, 0.0);
    }
}
