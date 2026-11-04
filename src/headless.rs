//! Running a game with no window: for tests, for scripts, and for
//! computers with no screen (like a cloud server where an AI writes games).
//!
//! The game runs as fast as the computer allows, not in real time: ten
//! seconds of game time usually takes a fraction of a second. Pictures are
//! drawn on the CPU, so no graphics card is needed, and saved as PNG files.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde_json::Value;

use crate::canvas::Canvas;
use crate::cli::{self, Options};
use crate::engine::{Config, Game, Timestep};
use crate::pictures::{self, Crop, GifWriter};
use crate::report;
use crate::script::{self, InputScript};
use crate::simulation::Simulation;
use crate::sound;

/// What a headless run should do. Make one with `HeadlessOptions::default()`
/// and change what you need:
///
/// ```
/// use duckforge::HeadlessOptions;
///
/// let options = HeadlessOptions {
///     frames: 120,
///     out: Some("shots".into()),
///     sheet: true,
///     ..HeadlessOptions::default()
/// };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct HeadlessOptions {
    /// How many updates to run (60 is one second of game time with the
    /// default timestep). The run stops sooner if the game calls
    /// `ctx.quit()`.
    pub frames: u64,
    /// The folder to save files in. It's created if it doesn't exist.
    /// `None` saves nothing. Otherwise it always gets `last.png` (the last
    /// frame) and `log.json` (what happened, and what the game reported),
    /// plus whatever the options below ask for.
    pub out: Option<PathBuf>,
    /// The random seed. `None` uses the script's seed, or `Config::seed`,
    /// or if those are `None` too,
    /// [`DEFAULT_SEED`](crate::simulation::DEFAULT_SEED), so runs are
    /// repeatable unless you ask otherwise.
    pub seed: Option<u32>,
    /// Inputs to replay: a recorded playthrough, or one written by hand or
    /// by a program. `None` runs the game with nobody touching anything.
    pub inputs: Option<InputScript>,
    /// Save frame N, 2N, 3N... as `frame-000060.png` and so on.
    pub snapshot_every: Option<u64>,
    /// Save `sheet.png`: many frames in a grid, each labelled with its frame
    /// number. It shows the `snapshot_every` frames if that's set, or else
    /// 16 frames spread evenly over the run.
    pub sheet: bool,
    /// Save `run.gif`: the whole run as an animated GIF, smaller and at
    /// about 20 pictures per second, to watch anywhere, even on a phone.
    pub gif: bool,
    /// Also save an enlarged close-up of this part of the picture, for the
    /// last frame (`closeup.png`) and each snapshot (`closeup-000060.png`).
    pub crop: Option<Crop>,
    /// How many times bigger a close-up is (each pixel becomes a
    /// `zoom` x `zoom` square). With `zoom` above 1 and no `crop`, the
    /// close-up is of the whole picture.
    pub zoom: usize,
    /// Draw the debug overlay (F1 in a window) on every picture: FPS,
    /// frame, camera, mouse, and the shapes the game registered with
    /// `ctx.debug_rect` and `ctx.debug_circle`.
    pub debug: bool,
}

impl Default for HeadlessOptions {
    fn default() -> Self {
        Self {
            frames: 600,
            out: None,
            seed: None,
            inputs: None,
            snapshot_every: None,
            sheet: false,
            gif: false,
            crop: None,
            zoom: 1,
            debug: false,
        }
    }
}

/// How many frames a sheet shows when there are no snapshots to show.
const SHEET_FRAMES: usize = 16;
/// The GIF aims for this many pictures per second of game time.
const GIF_PICTURES_PER_SECOND: f64 = 20.0;
/// ...but never more pictures than this, so long runs stay a sensible size.
const GIF_MAX_PICTURES: u64 = 600;
/// GIF pictures are shrunk to at most this wide.
const GIF_MAX_WIDTH: usize = 200;

/// Runs `game` for `options.frames` updates without a window, saves the
/// files `options` asks for, and hands back the finished [`Simulation`], so
/// you can look at the game, its last picture and what it reported.
///
/// Every game can do this from the command line too, without any code:
/// `cargo run --example breakout -- --headless --frames 600 --out shots`.
///
/// Returns an error if the config or options are impossible, or a file
/// can't be saved.
pub fn run_headless<G: Game>(
    config: Config,
    game: G,
    options: &HeadlessOptions,
) -> Result<Simulation<G>, String> {
    run_and_save(config, game, options).map(|(sim, _)| sim)
}

/// [`run_headless`], also returning the names of the files it saved.
fn run_and_save<G: Game>(
    mut config: Config,
    game: G,
    options: &HeadlessOptions,
) -> Result<(Simulation<G>, Vec<String>), String> {
    config.check()?;
    let close_up = match (options.crop, options.zoom) {
        (Some(crop), zoom) => Some((crop, zoom.max(1))),
        (None, zoom) if zoom > 1 => Some((
            Crop {
                x: 0,
                y: 0,
                width: config.width,
                height: config.height,
            },
            zoom,
        )),
        (None, _) => None,
    };
    if let Some((crop, _)) = close_up {
        crop.check(config.width, config.height)?;
    }
    if options.snapshot_every == Some(0) {
        return Err(String::from("snapshot_every must be at least 1"));
    }
    let script_seed = options.inputs.as_ref().and_then(|script| script.seed);
    config.seed = options.seed.or(script_seed).or(config.seed);
    let updates_per_second = match config.timestep {
        Timestep::Fixed { updates_per_second } => f64::from(updates_per_second.max(1)),
        Timestep::RealTime => 1.0 / f64::from(config.step_time()),
    };
    let title = config.title.clone();
    let (width, height) = (config.width, config.height);

    let mut sim = Simulation::new(config, game);
    sim.set_debug_overlay(options.debug);
    if let Some(script) = &options.inputs {
        sim.follow(script.clone());
    }

    // Without a folder, there's nothing to save: just run.
    let Some(folder) = &options.out else {
        sim.run(options.frames);
        return Ok((sim, Vec::new()));
    };
    fs::create_dir_all(folder)
        .map_err(|err| format!("could not create the folder {}: {err}", folder.display()))?;
    remove_old_files(folder);
    let mut saved = Vec::new();

    // Which frames go on the sheet.
    let sheet_frames: BTreeSet<u64> = if !options.sheet {
        BTreeSet::new()
    } else if let Some(every) = options.snapshot_every {
        let snapshots = options.frames / every;
        let keep_every = snapshots.div_ceil(pictures::SHEET_MAX_FRAMES as u64).max(1);
        (1..=snapshots)
            .filter(|n| n % keep_every == 0)
            .map(|n| n * every)
            .collect()
    } else {
        pictures::spread_frames(options.frames, SHEET_FRAMES)
            .into_iter()
            .collect()
    };
    let mut sheet = Vec::new();

    // The GIF: one picture every `gif_every` frames.
    let gif_every = exact_gif_step(
        ((updates_per_second / GIF_PICTURES_PER_SECOND).round() as u64)
            .max(options.frames.div_ceil(GIF_MAX_PICTURES))
            .max(1),
        updates_per_second,
    );
    let mut gif = if options.gif {
        let factor = pictures::shrink_factor(width, GIF_MAX_WIDTH);
        // GIF timing is in hundredths of a second, and most viewers treat
        // anything under 2 as "as fast as possible".
        let delay = ((gif_every as f64 * 100.0 / updates_per_second).round() as u16).max(2);
        let path = folder.join("run.gif");
        let writer = GifWriter::create(&path, width, height, factor, delay)
            .map_err(|err| format!("could not save {}: {err}", path.display()))?;
        saved.push(String::from("run.gif"));
        Some(writer)
    } else {
        None
    };

    for _ in 0..options.frames {
        if sim.quit_requested() {
            break;
        }
        sim.step();
        let frame = sim.context().frame();
        if options
            .snapshot_every
            .is_some_and(|every| frame.is_multiple_of(every))
        {
            let name = format!("frame-{frame:06}.png");
            save_png(sim.picture(), &folder.join(&name))?;
            saved.push(name);
            if let Some((crop, zoom)) = close_up {
                let name = format!("closeup-{frame:06}.png");
                save_png(
                    &pictures::close_up(sim.picture(), crop, zoom),
                    &folder.join(&name),
                )?;
                saved.push(name);
            }
        }
        if sheet_frames.contains(&frame) {
            sheet.push((frame, sim.picture().copy_picture()));
        }
        if let Some(gif) = &mut gif {
            if (frame - 1).is_multiple_of(gif_every) {
                gif.add(sim.picture())
                    .map_err(|err| format!("could not add to run.gif: {err}"))?;
            }
        }
    }
    drop(gif); // finishes the GIF file

    let last = sim.context().frame();
    save_png(sim.picture(), &folder.join("last.png"))?;
    saved.push(String::from("last.png"));
    if let Some((crop, zoom)) = close_up {
        save_png(
            &pictures::close_up(sim.picture(), crop, zoom),
            &folder.join("closeup.png"),
        )?;
        saved.push(String::from("closeup.png"));
    }
    if options.sheet {
        // Always end the sheet on the last frame, even if the game quit early.
        if sheet.last().is_none_or(|(frame, _)| *frame != last) && last > 0 {
            sheet.push((last, sim.picture().copy_picture()));
        }
        let picture = pictures::contact_sheet(&sheet, updates_per_second as f32);
        save_png(&picture, &folder.join("sheet.png"))?;
        saved.push(String::from("sheet.png"));
    }
    if !sim.sounds().is_empty() {
        // Every sound, mixed at the sample where its frame starts. The
        // track lasts as long as the run (or longer, if a sound rings on).
        let length = sim.sample_at(last + 1);
        let path = folder.join("audio.wav");
        fs::write(&path, sound::mix_to_wav(sim.sounds(), length))
            .map_err(|err| format!("could not save {}: {err}", path.display()))?;
        saved.push(String::from("audio.wav"));
    }
    saved.push(String::from("log.json"));
    let log = log_json(&title, &sim, updates_per_second, options, &saved);
    let path = folder.join("log.json");
    fs::write(&path, log).map_err(|err| format!("could not save {}: {err}", path.display()))?;
    Ok((sim, saved))
}

/// GIF timing is in hundredths of a second. Picks the smallest step of at
/// least `every` frames that lasts a whole number of hundredths, so the
/// GIF plays at exactly the game's speed and stays in step with
/// `audio.wav` (at 60 updates per second: a multiple of 3 frames, 5/100 s).
/// If that would make the GIF too jerky, `every` is kept as it is.
fn exact_gif_step(every: u64, updates_per_second: f64) -> u64 {
    let ups = updates_per_second.round() as u64;
    if ups == 0 || (ups as f64 - updates_per_second).abs() > 1e-9 {
        return every;
    }
    let (mut a, mut b) = (ups, 100);
    while b != 0 {
        (a, b) = (b, a % b); // a ends as the greatest common divisor
    }
    let unit = ups / a; // frames that last a whole number of hundredths
    let exact = every.div_ceil(unit) * unit;
    if exact <= every * 2 { exact } else { every }
}

fn save_png(picture: &Canvas, path: &Path) -> Result<(), String> {
    picture
        .save_png(path)
        .map_err(|err| format!("could not save {}: {err}", path.display()))
}

/// Deletes the files an earlier run left in `folder`, so old snapshots
/// can't be mistaken for new ones. Only names this module writes are
/// touched; anything else in the folder is left alone.
fn remove_old_files(folder: &Path) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_our_file(&name) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn is_our_file(name: &str) -> bool {
    let numbered = |prefix: &str| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(".png"))
            .is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()))
    };
    matches!(
        name,
        "last.png" | "closeup.png" | "sheet.png" | "run.gif" | "audio.wav" | "log.json"
    ) || numbered("frame-")
        || numbered("closeup-")
}

/// `log.json`: what was run, which files were saved, the game's reported
/// values at the end, and every change to them, one line per frame.
fn log_json<G: Game>(
    title: &str,
    sim: &Simulation<G>,
    updates_per_second: f64,
    options: &HeadlessOptions,
    files: &[String],
) -> String {
    let ctx = sim.context();
    let json = |value: Value| value.to_string();
    let mut text = String::from("{\n");
    text += &format!("  \"game\": {},\n", json(Value::from(title)));
    text += &format!("  \"seed\": {},\n", sim.seed());
    text += &format!(
        "  \"updates_per_second\": {},\n",
        report::ReportValue::from(updates_per_second)
    );
    text += &format!("  \"frames\": {},\n", ctx.frame());
    text += &format!(
        "  \"seconds\": {},\n",
        json(Value::from(
            (ctx.frame() as f64 / updates_per_second * 100.0).round() / 100.0
        ))
    );
    text += &format!("  \"game_quit\": {},\n", sim.quit_requested());
    if let Some(script) = &options.inputs {
        let replayed = script
            .events
            .iter()
            .filter(|event| event.frame <= ctx.frame())
            .count();
        text += &format!(
            "  \"inputs\": {{\"events\": {}, \"replayed\": {replayed}}},\n",
            script.events.len()
        );
    }
    let files: Vec<Value> = files
        .iter()
        .map(|name| Value::from(name.as_str()))
        .collect();
    text += &format!(
        "  \"files\": {},\n",
        json(Value::from(files)).replace(',', ", ")
    );
    let last: Vec<String> = sim
        .reports()
        .map(|(name, value)| format!("{}: {}", json(Value::from(name)), value.to_json()))
        .collect();
    text += &format!("  \"final\": {{{}}},\n", last.join(", "));
    text += "  \"sounds\": [";
    let sounds = sim.sounds();
    for (index, played) in sounds.iter().enumerate() {
        let comma = if index + 1 < sounds.len() { "," } else { "" };
        text += &format!("\n    {}{comma}", played.to_json());
    }
    if !sounds.is_empty() {
        text += "\n  ";
    }
    text += "],\n";
    text += "  \"changes\": [";
    let changes = sim.report_log();
    for (index, change) in changes.iter().enumerate() {
        let comma = if index + 1 < changes.len() { "," } else { "" };
        text += &format!("\n    {}{comma}", report::frame_json(change));
    }
    if !changes.is_empty() {
        text += "\n  ";
    }
    text += "]\n}\n";
    text
}

/// What `run` does with `--headless`: runs the game, then says what happened.
pub(crate) fn run_from_command_line<G: Game>(
    config: Config,
    game: G,
    options: &Options,
) -> Result<(), String> {
    let inputs = match &options.inputs {
        Some(path) => Some(InputScript::load(path)?),
        None => None,
    };
    if let Some(warning) = inputs
        .as_ref()
        .and_then(|script| script::timestep_warning(script, &config))
    {
        eprintln!("duckforge: {warning}");
    }
    // How long to run: as asked, or as long as the recording, or 10 seconds.
    let frames = options
        .frames
        .or(inputs.as_ref().and_then(|script| script.frames))
        .unwrap_or(HeadlessOptions::default().frames);
    let headless = HeadlessOptions {
        frames,
        out: Some(
            options
                .out
                .clone()
                .unwrap_or_else(|| PathBuf::from(cli::DEFAULT_OUT)),
        ),
        seed: options.seed,
        inputs,
        snapshot_every: options.snapshot_every,
        sheet: options.sheet,
        gif: options.gif,
        crop: options.crop,
        // A crop is for looking closely, so it's enlarged unless asked otherwise.
        zoom: options
            .zoom
            .unwrap_or(if options.crop.is_some() { 4 } else { 1 }),
        debug: options.debug,
    };
    let title = config.title.clone();
    let started = Instant::now();
    let (sim, saved) = run_and_save(config, game, &headless)?;
    let seconds = started.elapsed().as_secs_f32();

    let ctx = sim.context();
    let mut summary = format!(
        "duckforge: ran {title:?} without a window for {} frames ({:.1} s of game time) in {seconds:.2} s, seed {}",
        ctx.frame(),
        ctx.time(),
        sim.seed(),
    );
    if sim.quit_requested() {
        summary += "; the game quit by itself";
    }
    println!("{summary}");
    if let (Some(path), Some(script)) = (&options.inputs, &headless.inputs) {
        let used = script
            .events
            .iter()
            .filter(|event| event.frame <= ctx.frame())
            .count();
        println!(
            "duckforge: replayed {used} of the {} input events in {}",
            script.events.len(),
            path.display()
        );
    }
    if let Some(folder) = &headless.out {
        let snapshots = saved
            .iter()
            .filter(|name| name.starts_with("frame-"))
            .count();
        let mut listed: Vec<String> = saved
            .iter()
            .filter(|name| !name.starts_with("frame-") && !name.starts_with("closeup-0"))
            .cloned()
            .collect();
        if snapshots > 0 {
            listed.push(format!("{snapshots} snapshots (frame-NNNNNN.png)"));
        }
        println!(
            "duckforge: saved in {}: {}",
            folder.display(),
            listed.join(", ")
        );
        let sounds = sim.sounds().len();
        if sounds > 0 {
            println!(
                "duckforge: the game played {sounds} sound{}; audio.wav has them all, and log.json lists each one with its frame",
                if sounds == 1 { "" } else { "s" }
            );
            if saved.iter().any(|name| name == "run.gif") {
                println!(
                    "duckforge: to watch with sound, join them with ffmpeg:\n  \
                     ffmpeg -i {gif} -i {wav} -c:v libx264 -pix_fmt yuv420p -vf \"scale=trunc(iw/2)*2:trunc(ih/2)*2\" -c:a aac -shortest {mp4}",
                    gif = folder.join("run.gif").display(),
                    wav = folder.join("audio.wav").display(),
                    mp4 = folder.join("run.mp4").display(),
                );
            }
        }
    }
    let mut reported: Vec<String> = sim
        .reports()
        .map(|(name, value)| format!("{name} = {value}"))
        .collect();
    if !reported.is_empty() {
        reported.truncate(12);
        println!("duckforge: the game reported {}", reported.join(", "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;
    use crate::color::Color;
    use crate::engine::Context;
    use crate::input::Key;
    use crate::math::Rect;

    /// A square that moves right one pixel per update, and quits at frame `quit_at`.
    struct Mover {
        x: f32,
        quit_at: u64,
    }

    impl Game for Mover {
        fn update(&mut self, ctx: &mut Context) {
            self.x += 1.0;
            if ctx.frame() == self.quit_at {
                ctx.quit();
            }
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            canvas.fill_rect(Rect::new(self.x, 0.0, 1.0, 1.0), Color::WHITE);
        }
    }

    fn config() -> Config {
        Config {
            width: 64,
            height: 8,
            ..Config::default()
        }
    }

    /// A fresh, empty path in the system's temporary folder for a test to write to.
    fn test_folder(name: &str) -> PathBuf {
        let folder = std::env::temp_dir()
            .join(format!("duckforge-tests-{}", std::process::id()))
            .join(name);
        let _ = fs::remove_dir_all(&folder);
        let _ = fs::remove_file(&folder);
        folder
    }

    /// Plays a coin sound whenever Space is pressed, and random beeps.
    struct Noisy;

    impl Game for Noisy {
        fn update(&mut self, ctx: &mut Context) {
            if ctx.input.was_pressed(Key::Space) {
                ctx.play(crate::sound::Sound::coin());
            }
            if ctx.rng.chance(0.05) {
                let pitch = ctx.rng.range(200.0, 900.0);
                ctx.beep(pitch, 30.0);
            }
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
        }
    }

    fn noisy_run(name: &str, seed: u32) -> (Vec<u8>, String, Simulation<Noisy>) {
        let folder = test_folder(name);
        let script = InputScript::parse(
            r#"{"events": [{"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"},
                           {"frame": 90, "press": "Space"}]}"#,
        )
        .unwrap();
        let options = HeadlessOptions {
            frames: 120,
            out: Some(folder.clone()),
            inputs: Some(script),
            seed: Some(seed),
            ..HeadlessOptions::default()
        };
        let sim = run_headless(config(), Noisy, &options).unwrap();
        let wav = fs::read(folder.join("audio.wav")).unwrap();
        let log = fs::read_to_string(folder.join("log.json")).unwrap();
        (wav, log, sim)
    }

    #[test]
    fn the_same_seed_and_inputs_give_the_same_audio() {
        let (first, log, sim) = noisy_run("audio-a", 4);
        let (again, _, _) = noisy_run("audio-b", 4);
        let (other, _, _) = noisy_run("audio-c", 5);
        assert_eq!(first, again, "byte for byte the same");
        assert_ne!(first, other, "another seed beeps differently");
        // Two seconds at 44100 samples a second, 2 bytes each, plus the header.
        assert_eq!(first.len(), 44 + 120 * 735 * 2);
        assert_eq!(sim.sound_frames("coin"), [10, 90]);
        sim.assert_sound("coin", 88, 2);
        assert!(
            log.contains(r#"{"frame": 10, "sound": "coin", "wave": "square", "freq": 1050, "slide_to": 1600, "ms": 120, "volume": 0.3"#),
            "{log}"
        );
        assert!(log.contains("\"audio.wav\""), "listed with the files");
    }

    #[test]
    fn a_sound_starts_at_the_first_sample_of_its_frame() {
        struct OneBeep(u64);
        impl Game for OneBeep {
            fn update(&mut self, ctx: &mut Context) {
                if ctx.frame() == self.0 {
                    ctx.play(crate::sound::Sound {
                        attack: 0.0,
                        ..Default::default()
                    });
                }
            }
            fn draw(&self, _: &mut Canvas) {}
        }
        for (frame, ups) in [(1, 60), (7, 60), (100, 60), (7, 50), (7, 144)] {
            let folder = test_folder(&format!("audio-start-{frame}-{ups}"));
            let options = HeadlessOptions {
                frames: frame + 10,
                out: Some(folder.clone()),
                ..HeadlessOptions::default()
            };
            let config = Config {
                timestep: Timestep::Fixed {
                    updates_per_second: ups,
                },
                ..config()
            };
            run_headless(config, OneBeep(frame), &options).unwrap();
            let wav = fs::read(folder.join("audio.wav")).unwrap();
            let samples: Vec<i16> = wav[44..]
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect();
            let first_sound = samples.iter().position(|&s| s != 0).unwrap() as u64;
            let expected = (frame - 1) * 44100 / ups as u64;
            assert_eq!(
                first_sound, expected,
                "frame {frame} at {ups} updates a second"
            );
        }
    }

    #[test]
    fn a_missing_sound_fails_with_what_was_played() {
        let (_, _, sim) = noisy_run("audio-missing", 4);
        let wrong_frame = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sim.assert_sound("coin", 50, 3)
        }));
        let message = *wrong_frame.unwrap_err().downcast::<String>().unwrap();
        assert!(message.contains("played on frames [10, 90]"), "{message}");
        let never = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sim.assert_sound("explosion", 50, 3)
        }));
        let message = *never.unwrap_err().downcast::<String>().unwrap();
        assert!(
            message.contains("never played") && message.contains("coin at 10"),
            "{message}"
        );
    }

    #[test]
    fn a_loaded_wav_is_played_into_audio_wav_and_logged() {
        let folder = test_folder("audio-wav-file");
        fs::create_dir_all(&folder).unwrap();
        // A 100-sample .wav file, every sample at half volume.
        let path = folder.join("quack.wav");
        fs::write(
            &path,
            sound::wav_bytes(std::iter::repeat_n(16384, 100), 100),
        )
        .unwrap();
        let quack = crate::sound::Wav::load(&path).unwrap();
        assert_eq!(quack.name(), "quack");

        struct Quacker(crate::sound::Wav);
        impl Game for Quacker {
            fn update(&mut self, ctx: &mut Context) {
                if ctx.frame() == 3 {
                    ctx.play_wav(&self.0);
                }
            }
            fn draw(&self, _: &mut Canvas) {}
        }
        let out = folder.join("run");
        let options = HeadlessOptions {
            frames: 5,
            out: Some(out.clone()),
            ..HeadlessOptions::default()
        };
        let sim = run_headless(config(), Quacker(quack), &options).unwrap();
        sim.assert_sound("quack", 3, 0);
        let log = fs::read_to_string(out.join("log.json")).unwrap();
        assert!(
            log.contains(r#"{"frame": 3, "sound": "quack", "file": "#),
            "{log}"
        );
        let wav = fs::read(out.join("audio.wav")).unwrap();
        let samples: Vec<i16> = wav[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        let start = 2 * 735; // frame 3 starts 2 updates in
        assert_eq!(samples[start - 1], 0);
        assert_eq!(samples[start], 16384, "the file's samples, at its frame");
        assert_eq!(samples[start + 99], 16384);
        assert_eq!(samples[start + 100], 0, "and only for as long as the file");
    }

    #[test]
    fn a_silent_game_saves_no_audio() {
        let folder = test_folder("audio-none");
        let options = HeadlessOptions {
            frames: 5,
            out: Some(folder.clone()),
            ..HeadlessOptions::default()
        };
        run_headless(config(), Mover { x: 0.0, quit_at: 0 }, &options).unwrap();
        assert!(!folder.join("audio.wav").exists());
        let log = fs::read_to_string(folder.join("log.json")).unwrap();
        assert!(log.contains("\"sounds\": []"), "{log}");
    }

    #[test]
    fn gif_steps_last_whole_hundredths() {
        assert_eq!(exact_gif_step(3, 60.0), 3);
        assert_eq!(
            exact_gif_step(4, 60.0),
            6,
            "4/60 s isn't whole hundredths; 6/60 is"
        );
        assert_eq!(exact_gif_step(2, 50.0), 2, "every step is exact at 50");
        assert_eq!(
            exact_gif_step(7, 144.0),
            7,
            "rather than jump to 36, keep it"
        );
    }

    #[test]
    fn runs_the_frames_and_saves_the_last_one() {
        let folder = test_folder("headless-last");
        let options = HeadlessOptions {
            frames: 30,
            out: Some(folder.clone()),
            ..HeadlessOptions::default()
        };
        let game = Mover { x: 0.0, quit_at: 0 };
        let mut sim = run_headless(config(), game, &options).unwrap();
        assert_eq!(sim.context().frame(), 30);
        assert_eq!(sim.game().x, 30.0);
        assert_eq!(sim.picture().get_pixel(30, 0), Some(Color::WHITE));

        let saved = Canvas::load_png_for_tests(&folder.join("last.png"));
        assert_eq!(saved.pixels(), sim.picture().pixels());
    }

    #[test]
    fn stops_early_when_the_game_quits() {
        let game = Mover {
            x: 0.0,
            quit_at: 12,
        };
        let sim = run_headless(config(), game, &HeadlessOptions::default()).unwrap();
        assert!(sim.quit_requested());
        assert_eq!(sim.context().frame(), 12);
    }

    #[test]
    fn the_seed_option_wins_over_the_config() {
        let options = HeadlessOptions {
            frames: 1,
            seed: Some(99),
            ..HeadlessOptions::default()
        };
        let seeded = Config {
            seed: Some(5),
            ..config()
        };
        let game = Mover { x: 0.0, quit_at: 0 };
        assert_eq!(run_headless(seeded, game, &options).unwrap().seed(), 99);
    }

    #[test]
    fn problems_come_back_as_errors() {
        let game = || Mover { x: 0.0, quit_at: 0 };
        let no_size = Config {
            width: 0,
            ..config()
        };
        let err = run_headless(no_size, game(), &HeadlessOptions::default()).err();
        assert!(err.unwrap().contains("at least 1 x 1"));

        // A file where the folder should be.
        let file = test_folder("headless-not-a-folder");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "in the way").unwrap();
        let options = HeadlessOptions {
            frames: 1,
            out: Some(file.join("pictures")),
            ..HeadlessOptions::default()
        };
        let err = run_headless(config(), game(), &options).err().unwrap();
        assert!(err.contains("could not create the folder"), "{err}");
    }

    #[test]
    fn inputs_are_replayed_with_their_seed() {
        struct Counter {
            presses: u32,
            first_number: Option<f32>,
        }
        impl Game for Counter {
            fn update(&mut self, ctx: &mut Context) {
                self.first_number.get_or_insert(ctx.rng.next_f32());
                if ctx.input.was_pressed(crate::input::Key::Space) {
                    self.presses += 1;
                }
            }
            fn draw(&self, _: &mut Canvas) {}
        }
        let script = InputScript::parse(
            r#"{"seed": 9, "events": [
                {"frame": 2, "press": "Space"}, {"frame": 3, "release": "Space"},
                {"frame": 9, "press": "Space"}, {"frame": 10, "release": "Space"},
                {"frame": 50, "press": "Space"}
            ]}"#,
        )
        .unwrap();
        let options = HeadlessOptions {
            frames: 20,
            inputs: Some(script),
            ..HeadlessOptions::default()
        };
        let game = Counter {
            presses: 0,
            first_number: None,
        };
        let sim = run_headless(config(), game, &options).unwrap();
        assert_eq!(sim.game().presses, 2); // frame 50 is never reached
        assert_eq!(sim.seed(), 9);
        let mut rng = crate::rng::Rng::new(9);
        assert_eq!(sim.game().first_number, Some(rng.next_f32()));
    }

    /// Like `Mover`, but reports where it is.
    struct ReportingMover {
        x: f32,
    }

    impl Game for ReportingMover {
        fn update(&mut self, ctx: &mut Context) {
            self.x += 1.0;
            ctx.report("x", self.x);
            ctx.report("far", self.x >= 10.0);
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            canvas.fill_rect(Rect::new(self.x, 0.0, 1.0, 1.0), Color::WHITE);
        }
    }

    #[test]
    fn every_kind_of_picture_and_the_log_are_saved() {
        let folder = test_folder("headless-everything");
        fs::create_dir_all(&folder).unwrap();
        // Leftovers from an earlier run, and a file that isn't ours.
        fs::write(folder.join("frame-000999.png"), "old").unwrap();
        fs::write(folder.join("notes.txt"), "mine").unwrap();

        let options = HeadlessOptions {
            frames: 30,
            out: Some(folder.clone()),
            snapshot_every: Some(10),
            sheet: true,
            gif: true,
            crop: Some(Crop {
                x: 5,
                y: 0,
                width: 10,
                height: 4,
            }),
            zoom: 3,
            ..HeadlessOptions::default()
        };
        let sim = run_headless(config(), ReportingMover { x: 0.0 }, &options).unwrap();
        assert_eq!(sim.reported("x").and_then(|x| x.as_number()), Some(30.0));

        let mut names: Vec<String> = fs::read_dir(&folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "closeup-000010.png",
                "closeup-000020.png",
                "closeup-000030.png",
                "closeup.png",
                "frame-000010.png",
                "frame-000020.png",
                "frame-000030.png",
                "last.png",
                "log.json",
                "notes.txt",
                "run.gif",
                "sheet.png",
            ]
        );

        // The close-up of frame 10: the square at x = 10 is pixel 5 of the
        // crop, which becomes pixels 15..18 when enlarged 3 times.
        let close_up = Canvas::load_png_for_tests(&folder.join("closeup-000010.png"));
        assert_eq!((close_up.width(), close_up.height()), (30, 12));
        assert_eq!(close_up.get_pixel(15, 0), Some(Color::WHITE));
        assert_eq!(close_up.get_pixel(14, 0), Some(Color::BLACK));

        let log: Value =
            serde_json::from_str(&fs::read_to_string(folder.join("log.json")).unwrap()).unwrap();
        assert_eq!(log["frames"], 30);
        assert_eq!(log["updates_per_second"], 60);
        assert_eq!(log["final"]["x"], 30);
        assert_eq!(log["final"]["far"], true);
        // "far" changes once, on frame 10; "x" every frame.
        let changes = log["changes"].as_array().unwrap();
        assert_eq!(changes.len(), 30);
        assert_eq!(changes[9]["far"], true);
        assert!(changes[10].get("far").is_none());
        assert_eq!(log["files"].as_array().unwrap().len(), 11); // all but notes.txt
    }

    #[test]
    fn our_files_are_recognised_by_name() {
        assert!(is_our_file("frame-000060.png"));
        assert!(is_our_file("closeup-123456.png"));
        assert!(is_our_file("log.json"));
        assert!(!is_our_file("frame-60.png"));
        assert!(!is_our_file("frame-00006x.png"));
        assert!(!is_our_file("holiday.png"));
    }

    #[test]
    fn a_crop_outside_the_canvas_is_an_error_before_running() {
        let options = HeadlessOptions {
            crop: Some(Crop {
                x: 60,
                y: 0,
                width: 10,
                height: 1,
            }),
            ..HeadlessOptions::default()
        };
        let err = run_headless(config(), ReportingMover { x: 0.0 }, &options).err();
        assert!(err.unwrap().contains("doesn't fit"));
    }
}
