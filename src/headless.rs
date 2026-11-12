//! Running a game with no window: for tests, for scripts, and for
//! computers with no screen (like a cloud server where an AI writes games).
//!
//! The game runs as fast as the computer allows, not in real time: ten
//! seconds of game time usually takes a fraction of a second. Pictures are
//! drawn on the CPU, so no graphics card is needed, and saved as PNG files.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use serde_json::Value;

use crate::canvas::Canvas;
use crate::cli::{self, Options};
use crate::engine::{Config, Game, Timestep};
use crate::on_screen;
use crate::pictures::{self, Crop, GifWriter};
use crate::render3d::Renderer;
use crate::report;
use crate::script::{self, InputScript};
use crate::sheet;
use crate::simulation::Simulation;
use crate::sound;
use crate::summary::Summary;
use crate::video::{ClipWriter, VideoWriter};

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
    /// plus whatever the options below ask for. When `debug` or `grid`
    /// draws on `last.png`, `last.plain.png` is the same frame without.
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
    /// Draw a light grid every this many pixels, with coordinates, on
    /// `last.png`, the snapshots and the close-ups (`--grid N`).
    pub grid: Option<usize>,
    /// Draw that grid in world coordinates, through the game's camera,
    /// instead of every N screen pixels (`--grid-world N`).
    pub grid_world: bool,
    /// The tune file for `ctx.tune` (`--tune FILE`). `None` uses
    /// `examples/<game>.tune.json` if it exists.
    pub tune_file: Option<PathBuf>,
    /// Tune values that win over the file (`--set name=value`).
    pub tune: Vec<(String, f32)>,
    /// After the run, write every name the game passed to `ctx.tune`, with
    /// its value, to the tune file (`--write-tune`).
    pub write_tune: bool,
    /// The file `inputs` came from, if any, to name it in warnings.
    pub inputs_file: Option<PathBuf>,
    /// Make `sheet.png` from the frames where reported values changed
    /// (`--sheet-events`), each with what changed under it, instead of
    /// spreading them evenly.
    pub sheet_events: bool,
    /// Also save `sheet-follow.png`: the same frames, each cut down to the
    /// area around this label and enlarged three times (`--sheet-follow NAME`).
    pub sheet_follow: Option<String>,
    /// Save the whole run as a video (`--video FILE.mp4`): every frame, at
    /// the game's frame rate, enlarged with sharp pixels into 1920 x 1080,
    /// with the sound. Needs `ffmpeg`; without it, the frames are saved as
    /// PNG files and the command to join them is printed.
    pub video: Option<PathBuf>,
    /// Save the end of the run as a clip (`--clip FILE.mp4`): a video with
    /// sound if `ffmpeg` is there, or else a full-size GIF next to it.
    pub clip: Option<PathBuf>,
    /// How many seconds of the run's end the clip keeps (`--clip-seconds`).
    pub clip_seconds: f32,
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
            grid: None,
            grid_world: false,
            tune_file: None,
            tune: Vec::new(),
            write_tune: false,
            inputs_file: None,
            sheet_events: false,
            sheet_follow: None,
            video: None,
            clip: None,
            clip_seconds: 10.0,
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

/// The frame a headless run is on, for the message when a game panics.
static RUNNING_FRAME: AtomicU64 = AtomicU64::new(0);

/// Makes a panic one line that says where and on which frame it happened,
/// so a crash can be replayed to just before it (`--frames N`):
/// `duckforge: the game panicked on frame 12 at examples/x.rs:40:9: message`.
/// With `RUST_BACKTRACE=full`, Rust's usual message and backtrace follow.
/// (Many servers set `RUST_BACKTRACE=1` for everything, so that alone
/// doesn't turn the long version on.)
fn report_panic_frames() {
    let usual = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!(
            "{}",
            panic_line(RUNNING_FRAME.load(Ordering::Relaxed), info)
        );
        if std::env::var_os("RUST_BACKTRACE").is_some_and(|v| v == "full") {
            usual(info);
        }
    }));
}

/// The one line [`report_panic_frames`] prints.
fn panic_line(frame: u64, info: &std::panic::PanicHookInfo) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| String::from("(no message)"));
    let message = message.replace('\n', " / ");
    let place = info
        .location()
        .map(|l| format!(" at {}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_default();
    if frame > 0 {
        format!("duckforge: the game panicked on frame {frame}{place}: {message}")
    } else {
        format!("duckforge: panicked{place}: {message}")
    }
}

/// How many frames, spread over the run, are checked for scene warnings.
const CHECK_FRAMES: usize = 16;

/// [`run_headless`], also returning what it saved and found.
fn run_and_save<G: Game>(
    mut config: Config,
    game: G,
    options: &HeadlessOptions,
) -> Result<(Simulation<G>, Summary), String> {
    config.check()?;
    let started = Instant::now();
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
    let timestep_warning = options
        .inputs
        .as_ref()
        .and_then(|script| script::timestep_warning(script, &config));

    // --tune FILE, or the file the game names in its Config, or the default.
    let tunes = crate::tune::Tunes::for_run(
        options.tune_file.as_deref(),
        config.tune_file.as_deref(),
        &options.tune,
    )?;
    // Always the CPU, so a run gives the same pictures on every computer,
    // unless DUCKFORGE_RENDERER asks otherwise (to compare with the GPU).
    let renderer = crate::render3d::renderer_from_env().unwrap_or(Renderer::Cpu);
    let mut sim = Simulation::start(&config, game, renderer);
    sim.set_tunes(tunes);
    sim.set_debug_overlay(options.debug);
    if let Some(script) = &options.inputs {
        sim.follow(script.clone());
    }

    // Without a folder, there's nothing to save: just run.
    let Some(folder) = &options.out else {
        sim.run(options.frames);
        if let Some(note) = write_tune_if_asked(&mut sim, options)? {
            println!("duckforge: {note}");
        }
        let summary = Summary::of(&sim, &title, started.elapsed().as_secs_f32());
        return Ok((sim, summary));
    };
    fs::create_dir_all(folder)
        .map_err(|err| format!("could not create the folder {}: {err}", folder.display()))?;
    remove_old_files(folder);
    let mut saved = Vec::new();

    // Which frames go on the sheet. (With --sheet-events, these are only
    // the evenly spread ones that fill the gaps between events.)
    let any_sheet = options.sheet || options.sheet_events || options.sheet_follow.is_some();
    let mut picker = sheet::SheetPicker::default();
    let sheet_frames: BTreeSet<u64> = if !any_sheet {
        BTreeSet::new()
    } else if let (Some(every), false) = (options.snapshot_every, options.sheet_events) {
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

    // Frames whose scenes are checked for warnings: the first, some spread
    // over the run, every snapshot, and the last.
    let mut check_frames: BTreeSet<u64> = pictures::spread_frames(options.frames, CHECK_FRAMES)
        .into_iter()
        .collect();
    check_frames.insert(1);
    let mut scenes = Vec::new();

    // The clip: the last seconds of the run.
    let clip_frames = ((f64::from(options.clip_seconds) * updates_per_second).round() as u64)
        .clamp(1, options.frames.max(1));
    let clip_from = options.frames.saturating_sub(clip_frames) + 1;
    let mut clip = match &options.clip {
        Some(path) => Some(ClipWriter::start(
            path,
            width,
            height,
            updates_per_second,
            folder,
            (clip_from - 1) as f64 / updates_per_second,
        )?),
        None => None,
    };

    let mut video = match &options.video {
        Some(path) => Some(VideoWriter::start(
            path,
            width,
            height,
            updates_per_second,
            folder,
        )?),
        None => None,
    };

    for _ in 0..options.frames {
        if sim.quit_requested() {
            break;
        }
        RUNNING_FRAME.store(sim.context().frame() + 1, Ordering::Relaxed);
        sim.step();
        let frame = sim.context().frame();
        let snapshot = options
            .snapshot_every
            .is_some_and(|every| frame.is_multiple_of(every));
        if snapshot || check_frames.contains(&frame) {
            scenes.push(sim.scene().clone());
        }
        if snapshot {
            let picture = for_saving(&mut sim, options);
            let name = format!("frame-{frame:06}.png");
            save_png(&picture, &folder.join(&name))?;
            saved.push(name);
            let name = format!("frame-{frame:06}.scene.json");
            save_text(&sim.scene().to_json(), &folder.join(&name))?;
            saved.push(name);
            if let Some((crop, zoom)) = close_up {
                let name = format!("closeup-{frame:06}.png");
                save_png(
                    &pictures::close_up(&picture, crop, zoom),
                    &folder.join(&name),
                )?;
                saved.push(name);
            }
        }
        let spread = sheet_frames.contains(&frame);
        if options.sheet_events {
            let changed = sim
                .report_log()
                .last()
                .filter(|report| report.frame == frame)
                .map(|report| report.changes.clone())
                .unwrap_or_default();
            if picker.reasons(frame, &changed) || spread {
                let picture = sim.picture().copy_picture();
                picker.keep(frame, picture, sim.scene().clone(), spread);
            }
        } else if spread {
            let picture = sim.picture().copy_picture();
            sheet.push((frame, picture, sim.scene().clone()));
        }
        if let Some(video) = &mut video {
            video.add(sim.picture())?;
        }
        if let Some(clip) = &mut clip {
            if frame >= clip_from {
                clip.add(sim.picture())?;
            }
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
    let last_picture = for_saving(&mut sim, options);
    save_png(&last_picture, &folder.join("last.png"))?;
    saved.push(String::from("last.png"));
    if options.debug || options.grid.is_some() {
        // The same moment with nothing drawn on top: what the game
        // itself drew (the Studio's cards use it). Drawing doesn't change
        // the game, so this is safe to do at the end.
        sim.set_debug_overlay(false);
        let plain = sim.picture().copy_picture();
        sim.set_debug_overlay(options.debug);
        save_png(&plain, &folder.join("last.plain.png"))?;
        saved.push(String::from("last.plain.png"));
    }
    save_text(&sim.scene().to_json(), &folder.join("last.scene.json"))?;
    saved.push(String::from("last.scene.json"));
    if scenes.last().is_none_or(|scene| scene.frame != last) && last > 0 {
        scenes.push(sim.scene().clone());
    }
    let mut warnings: Vec<String> = timestep_warning.into_iter().collect();
    warnings.extend(skipped_events_warning(options));
    warnings.extend(sim.replay_warnings().iter().cloned());
    warnings.extend(on_screen::warnings(&scenes, width as f32, height as f32));
    if let Some((crop, zoom)) = close_up {
        save_png(
            &pictures::close_up(&last_picture, crop, zoom),
            &folder.join("closeup.png"),
        )?;
        saved.push(String::from("closeup.png"));
    }
    if any_sheet {
        // Always end the sheet on the last frame, even if the game quit early.
        let frames: Vec<sheet::SheetFrame> = if options.sheet_events {
            if last > 0 {
                let picture = sim.picture().copy_picture();
                picker.keep(last, picture, sim.scene().clone(), true);
            }
            picker.choose(last, SHEET_FRAMES)
        } else {
            if sheet.last().is_none_or(|(frame, ..)| *frame != last) && last > 0 {
                let picture = sim.picture().copy_picture();
                sheet.push((last, picture, sim.scene().clone()));
            }
            sheet
                .into_iter()
                .map(|(frame, picture, scene)| sheet::SheetFrame {
                    frame,
                    picture,
                    scene,
                    caption: String::new(),
                })
                .collect()
        };
        let captioned: Vec<(u64, Canvas, String)> = frames
            .iter()
            .map(|f| (f.frame, f.picture.copy_picture(), f.caption.clone()))
            .collect();
        let picture = pictures::contact_sheet_with_captions(&captioned, updates_per_second as f32);
        save_png(&picture, &folder.join("sheet.png"))?;
        saved.push(String::from("sheet.png"));
        if let Some(name) = &options.sheet_follow {
            let followed = sheet::follow(&frames, name);
            let picture =
                pictures::contact_sheet_with_captions(&followed, updates_per_second as f32);
            save_png(&picture, &folder.join("sheet-follow.png"))?;
            saved.push(String::from("sheet-follow.png"));
        }
    }
    let has_sound = !sim.sounds().is_empty() || !sim.loops().is_empty();
    if has_sound {
        // Every sound, mixed at the sample where its frame starts, and every
        // loop. The track lasts as long as the run (or longer, if a sound
        // rings on).
        let length = sim.sample_at(last + 1);
        let path = folder.join("audio.wav");
        fs::write(&path, sound::mix_to_wav(sim.sounds(), sim.loops(), length))
            .map_err(|err| format!("could not save {}: {err}", path.display()))?;
        saved.push(String::from("audio.wav"));
    }
    let video_note = match video {
        Some(video) => {
            let wav = folder.join("audio.wav");
            let audio = has_sound.then_some(wav.as_path());
            Some(video.finish(audio)?)
        }
        None => None,
    };
    let clip_note = match clip {
        Some(clip) => {
            let wav = folder.join("audio.wav");
            Some(clip.finish(has_sound.then_some(wav.as_path()))?)
        }
        None => None,
    };
    warnings.extend(unused_set_warnings(&sim));
    // Did the playthrough end the way it's expected to? (Only when it ran
    // its own length: a different --frames ends somewhere else.)
    let (mut changed, mut tune_changed) = match &options.inputs {
        Some(script) if script.frames.is_none_or(|f| f == options.frames) => {
            let now = sim.tunes().used().into_iter().map(|(n, v, _)| (n, v));
            (
                script.changes_from_expected(sim.reports()),
                script.tune_changes(now),
            )
        }
        _ => (Vec::new(), Vec::new()),
    };
    // Tune values changed since the recording. Still the expected ending:
    // a warning (you can tune without re-recording). A different ending: a
    // failure, naming the changed values as the likely cause.
    if !changed.is_empty() && !tune_changed.is_empty() {
        let file = options
            .inputs_file
            .as_ref()
            .map_or(String::from("the playthrough"), |f| f.display().to_string());
        warnings.push(format!(
            "{file} ends differently ({}), and tune values changed since it was recorded ({}): \
             that's the likely cause. If the new values are right, record it again (F10 in the \
             game, or the test that recorded it) or keep this ending with --accept",
            changed.join(", "),
            tune_changed.join(", ")
        ));
        changed.clear();
        tune_changed.clear();
    }
    saved.push(String::from("log.json"));
    saved.push(String::from("summary.txt"));
    let log = log_json(
        &title,
        &sim,
        updates_per_second,
        options,
        &saved,
        &warnings,
        &changed,
    );
    let path = folder.join("log.json");
    fs::write(&path, log).map_err(|err| format!("could not save {}: {err}", path.display()))?;
    let note = write_tune_if_asked(&mut sim, options)?;

    let mut summary = Summary::of(&sim, &title, started.elapsed().as_secs_f32());
    summary.changed = changed;
    summary.tune_changed = tune_changed;
    if let Some(script) = &options.inputs {
        let replayed = script.events.iter().filter(|e| e.frame <= last).count();
        summary.inputs = Some((replayed, script.events.len()));
    }
    summary.warnings = warnings;
    summary.notes.extend(note);
    summary.notes.extend(video_note);
    summary.notes.extend(clip_note);
    summary.folder = Some(folder.clone());
    summary.files = saved;
    save_text(&summary.to_text(), &folder.join("summary.txt"))?;
    sim.quit(); // the run is over: the game's own goodbye, last of all
    Ok((sim, summary))
}

/// A warning if the run stops before some of the script's events: they're
/// silently never replayed, so the run isn't the playthrough it claims to be.
fn skipped_events_warning(options: &HeadlessOptions) -> Option<String> {
    let script = options.inputs.as_ref()?;
    let skipped = script.events_after(options.frames);
    if skipped == 0 {
        return None;
    }
    let name = options
        .inputs_file
        .as_ref()
        .map_or(String::from("the input script"), |f| {
            f.display().to_string()
        });
    Some(format!(
        "{name}: {skipped} of its {} input events {} after frame {}, where the run stopped, \
         so they were never replayed; leave out --frames, or make the script's \"frames\" \
         at least {}",
        script.events.len(),
        if skipped == 1 { "comes" } else { "come" },
        options.frames,
        script.last_event_frame()
    ))
}

/// The picture as it's saved: with `--grid`'s grid on it, if asked for
/// (in world coordinates with `--grid-world`, and never over the debug
/// overlay's box).
fn for_saving<G: Game>(sim: &mut Simulation<G>, options: &HeadlessOptions) -> Canvas {
    sim.picture();
    let picture = sim.picture().copy_picture();
    match options.grid {
        Some(every) => {
            let (camera, debug_box) = sim.drawn_extras();
            let camera = if options.grid_world { camera } else { None };
            pictures::with_grid(&picture, every, camera, debug_box)
        }
        None => picture,
    }
}

fn save_text(text: &str, path: &Path) -> Result<(), String> {
    fs::write(path, text).map_err(|err| format!("could not save {}: {err}", path.display()))
}

/// `--write-tune`: saves every tune name the game used to the tune file,
/// and says so.
fn write_tune_if_asked<G: Game>(
    sim: &mut Simulation<G>,
    options: &HeadlessOptions,
) -> Result<Option<String>, String> {
    if !options.write_tune {
        return Ok(None);
    }
    let path = sim.tunes().write_file()?;
    let count = sim.tunes().used().len();
    let kept = sim.tunes().unused_file_names();
    let mut note = format!("wrote {count} tune values to {}", path.display());
    if !kept.is_empty() {
        note += &format!(
            " (and kept {}, which the game didn't ask for)",
            kept.join(", ")
        );
    }
    Ok(Some(note))
}

/// A warning for each `--set NAME=N` the game never asked for, and one
/// for names in the tune file it never asked for.
fn unused_set_warnings<G: Game>(sim: &Simulation<G>) -> Vec<String> {
    let tunes = sim.tunes();
    let asked: Vec<String> = tunes.used().into_iter().map(|(name, ..)| name).collect();
    let asked = if asked.is_empty() {
        String::from("nothing")
    } else {
        asked.join(", ")
    };
    let mut warnings: Vec<String> = tunes
        .unused_sets()
        .into_iter()
        .map(|name| {
            format!(
                "--set {name} was never asked for (the game asked for: {asked}); check the spelling, and read tune values at the top of update"
            )
        })
        .collect();
    let stale = tunes.unused_file_names();
    if let (false, Some(path)) = (stale.is_empty(), tunes.path()) {
        warnings.push(format!(
            "{} has {}, which the game never asked for (it asked for: {asked}); remove them, or check the spelling",
            path.display(),
            stale.join(", ")
        ));
    }
    warnings
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
    let numbered = |prefix: &str, suffix: &str| {
        name.strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
            .is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|b| b.is_ascii_digit()))
    };
    matches!(
        name,
        "last.png"
            | "last.scene.json"
            | "closeup.png"
            | "sheet.png"
            | "sheet-follow.png"
            | "run.gif"
            | "audio.wav"
            | "log.json"
            | "summary.txt"
    ) || numbered("frame-", ".png")
        || numbered("frame-", ".scene.json")
        || numbered("closeup-", ".png")
}

/// `log.json`: what was run, which files were saved, the game's reported
/// values at the end, and every change to them, one line per frame.
fn log_json<G: Game>(
    title: &str,
    sim: &Simulation<G>,
    updates_per_second: f64,
    options: &HeadlessOptions,
    files: &[String],
    warnings: &[String],
    changed: &[String],
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
    // Extras drawn on the saved pictures, so --diff can tell they differ.
    text += &format!(
        "  \"drawn_on_pictures\": {{\"grid\": {}, \"grid_world\": {}, \"debug\": {}}},\n",
        options.grid.map_or(String::from("null"), |n| n.to_string()),
        options.grid_world,
        options.debug
    );
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
    text += &format!("  \"tune\": {},\n", sim.tunes().to_json());
    let failed: Vec<String> = sim
        .failed_checks()
        .iter()
        .map(|f| {
            format!(
                "{{\"name\": {}, \"frame\": {}}}",
                json(Value::from(f.name.as_str())),
                f.frame
            )
        })
        .collect();
    let idle: Vec<String> = sim
        .idle_frames()
        .iter()
        .map(|(a, b)| format!("[{a}, {b}]"))
        .collect();
    text += &format!("  \"idle\": [{}],\n", idle.join(", "));
    // The keys the game ever asked about: --fuzz ignores presses of others.
    let keys: Vec<Value> = ctx.input.keys_read().into_iter().map(Value::from).collect();
    text += &format!(
        "  \"keys_read\": {},\n",
        json(Value::from(keys)).replace(',', ", ")
    );
    text += &format!(
        "  \"checks\": {{\"rules\": {}, \"failed\": [{}]}},\n",
        sim.context().checks_seen(),
        failed.join(", ")
    );
    let warnings: Vec<Value> = warnings.iter().map(|w| Value::from(w.as_str())).collect();
    text += &format!("  \"warnings\": {},\n", json(Value::from(warnings)));
    let changed: Vec<Value> = changed.iter().map(|c| Value::from(c.as_str())).collect();
    text += &format!(
        "  \"playthrough_changed\": {},\n",
        json(Value::from(changed))
    );
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
    text += "  \"loops\": [";
    let loops = sim.loops();
    for (index, looped) in loops.iter().enumerate() {
        let comma = if index + 1 < loops.len() { "," } else { "" };
        text += &format!("\n    {}{comma}", looped.to_json());
    }
    if !loops.is_empty() {
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
/// `--headless --time`: plays the run drawing every frame and saving
/// nothing, and says how long the updates and the drawing took. For
/// comparing one build with another: the game's own work, with no window
/// and no files in the way.
pub(crate) fn time_from_command_line<G: Game>(
    mut config: Config,
    game: G,
    options: &Options,
) -> Result<String, String> {
    let inputs = match &options.inputs {
        Some(path) => Some(InputScript::load(path)?),
        None => None,
    };
    let updates_per_second = 1.0 / f64::from(config.step_time());
    let frames = options
        .frames
        .or(inputs
            .as_ref()
            .and_then(|script| script.run_length(updates_per_second)))
        .unwrap_or(HeadlessOptions::default().frames);
    let script_seed = inputs.as_ref().and_then(|script| script.seed);
    config.seed = options.seed.or(script_seed).or(config.seed);
    let tunes = crate::tune::Tunes::for_run(
        options.tune.as_deref(),
        config.tune_file.as_deref(),
        &options.set,
    )?;
    let renderer = crate::render3d::renderer_from_env().unwrap_or(Renderer::Cpu);
    let title = config.title.clone();
    let mut sim = Simulation::start(&config, game, renderer);
    sim.set_tunes(tunes);
    if let Some(script) = inputs {
        sim.follow(script);
    }
    let (mut updating, mut drawing) = (std::time::Duration::ZERO, std::time::Duration::ZERO);
    let mut played = 0u64;
    for _ in 0..frames {
        if sim.quit_requested() {
            break;
        }
        let before = Instant::now();
        sim.step();
        updating += before.elapsed();
        let before = Instant::now();
        sim.picture();
        drawing += before.elapsed();
        played += 1;
    }
    sim.quit();
    let each = |all: std::time::Duration| all.as_secs_f64() * 1e6 / played.max(1) as f64;
    Ok(format!(
        "duckforge: timed {title:?} for {played} frames: updates {:.1} ms in all \
         ({:.2} microseconds each), drawing {:.1} ms in all ({:.1} microseconds each)",
        updating.as_secs_f64() * 1e3,
        each(updating),
        drawing.as_secs_f64() * 1e3,
        each(drawing),
    ))
}

pub(crate) fn run_from_command_line<G: Game>(
    mut config: Config,
    game: G,
    options: &Options,
) -> Result<(), String> {
    let inputs = match &options.inputs {
        Some(path) => Some(InputScript::load(path)?),
        None => None,
    };
    // How long to run: as asked, or as long as the recording (its
    // "frames", or its last event plus two seconds), or 10 seconds.
    let updates_per_second = 1.0 / f64::from(config.step_time());
    let frames = options
        .frames
        .or(inputs
            .as_ref()
            .and_then(|script| script.run_length(updates_per_second)))
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
        grid: options.grid,
        grid_world: options.grid_world,
        tune_file: options.tune.clone(),
        tune: options.set.clone(),
        write_tune: options.write_tune,
        inputs_file: options.inputs.clone(),
        sheet_events: options.sheet_events,
        sheet_follow: options.sheet_follow.clone(),
        video: options.video.clone(),
        clip: options.clip.clone(),
        clip_seconds: options.clip_seconds.unwrap_or(10.0),
    };
    if let Some(corner) = options.debug_corner {
        config.debug_corner = corner;
    }
    report_panic_frames();
    let (mut sim, mut summary) = run_and_save(config, game, &headless)?;
    if let (true, Some(path), Some(mut script)) = (options.accept, &options.inputs, headless.inputs)
    {
        // --accept: this ending becomes the expected one.
        script.expect = Some(
            sim.reports()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
        );
        // And the tune values it ends that way with.
        let tuned: std::collections::BTreeMap<String, f32> = sim
            .tunes()
            .used()
            .into_iter()
            .map(|(name, value, _)| (name, value))
            .collect();
        script.tune = (!tuned.is_empty()).then_some(tuned);
        script.save(path)?;
        let was = if summary.changed.is_empty() {
            String::new()
        } else {
            format!(" (it had changed: {})", summary.changed.join(", "))
        };
        summary.changed.clear();
        summary.tune_changed.clear();
        summary.notes.push(format!(
            "accepted: wrote this ending to {}'s \"expect\"{was}",
            path.display()
        ));
        if let Some(folder) = &headless.out {
            save_text(&summary.to_text(), &folder.join("summary.txt"))?;
        }
    }
    if options.json {
        let mut value = summary.to_json();
        if options.describe {
            value["describe"] = Value::from(sim.scene().describe());
        }
        println!("{value}");
    } else if options.quiet {
        for line in summary.problems() {
            println!("duckforge: {line}");
        }
    } else {
        print!("{}", summary.to_text());
        if options.describe {
            println!("{}", sim.scene().describe());
        }
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
    use crate::math::{Rect, vec2};
    use std::process::Command;

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

    /// A new, empty folder of the test's own, in the system's temporary folder.
    fn test_folder(name: &str) -> crate::test_folder::TestFolder {
        crate::test_folder::TestFolder::new(name)
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
            out: Some(folder.to_path_buf()),
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
                out: Some(folder.to_path_buf()),
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
    fn loops_repeat_change_volume_stop_and_are_logged() {
        let folder = test_folder("audio-loops");
        fs::create_dir_all(&folder).unwrap();
        // A 1,000-sample loop (like a Duck Echo export): a ramp, so every
        // place in it is different and a gap or a jump would show.
        let path = folder.join("theme.wav");
        let ramp = (0..1000).map(|i| (i * 16) as i16);
        fs::write(&path, sound::wav_bytes(ramp, 1000)).unwrap();
        let theme = crate::sound::Wav::load(&path).unwrap();

        struct Musical {
            theme: crate::sound::Wav,
            music: Option<crate::sound::LoopId>,
        }
        impl Game for Musical {
            fn update(&mut self, ctx: &mut Context) {
                match ctx.frame() {
                    1 => self.music = Some(ctx.play_loop(&self.theme, 1.0)),
                    20 => ctx.set_loop_volume(self.music.unwrap(), 0.5),
                    40 => ctx.stop_loop(self.music.unwrap()),
                    41 => ctx.stop_loop(self.music.unwrap()), // again: nothing happens
                    50 => {
                        let hum = crate::sound::Sound {
                            name: "hum",
                            ..Default::default()
                        };
                        ctx.play_loop(hum, 0.25);
                    }
                    _ => {}
                }
            }
            fn draw(&self, _: &mut Canvas) {}
        }
        let out = folder.join("run");
        let options = HeadlessOptions {
            frames: 60,
            out: Some(out.clone()),
            ..HeadlessOptions::default()
        };
        let game = Musical { theme, music: None };
        let (sim, summary) = run_and_save(config(), game, &options).unwrap();

        let loops = sim.loops();
        assert_eq!(loops.len(), 2);
        let (music, hum) = (&loops[0], &loops[1]);
        assert_eq!(
            (music.name.as_str(), music.start, music.stop),
            ("theme", 1, Some(40))
        );
        assert_eq!(music.volumes, [(1, 1.0), (20, 0.5)]);
        assert_eq!(
            (
                music.volume_at(19),
                music.volume_at(20),
                music.volume_at(40)
            ),
            (1.0, 0.5, 0.0)
        );
        assert_eq!((hum.name.as_str(), hum.start), ("hum", 50));
        assert!(hum.is_playing() && !music.is_playing());

        let log = fs::read_to_string(out.join("log.json")).unwrap();
        assert!(
            log.contains(r#"{"loop": "theme", "file": "#)
                && log.contains(r#""start": 1, "stop": 40, "volume": [[1, 1], [20, 0.5]]}"#),
            "{log}"
        );
        assert!(
            log.contains(r#""start": 50, "stop": null, "volume": [[50, 0.25]]}"#),
            "{log}"
        );
        assert_eq!(
            summary.loops,
            [
                "theme from frame 1 to frame 40",
                "hum from frame 50 to the end"
            ]
        );

        // audio.wav: 735 samples per frame at 60 updates a second. The
        // ramp goes round every 1,000 samples with no gap, halves at frame
        // 20 (sample 13,965), and stops at frame 40 (sample 28,665).
        let wav = fs::read(out.join("audio.wav")).unwrap();
        let samples: Vec<i16> = wav[44..]
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert_eq!(samples.len(), 60 * 735, "as long as the run");
        let ramp_at = |i: usize| (i % 1000) as i32 * 16;
        for i in [0, 1, 999, 1000, 1001, 13_964] {
            assert!(
                (samples[i] as i32 - ramp_at(i)).abs() <= 1,
                "sample {i}: {}",
                samples[i]
            );
        }
        for i in [13_965, 20_000, 28_664] {
            assert!(
                (samples[i] as i32 - ramp_at(i) / 2).abs() <= 1,
                "sample {i} at half volume: {}",
                samples[i]
            );
        }
        assert!(samples[28_665..49 * 735].iter().all(|&s| s == 0), "stopped");
        assert!(
            samples[49 * 735..].iter().any(|&s| s != 0),
            "the hum, to the end"
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

    /// A ball whose speed, size and randomness come from `ctx.tune`.
    struct Tuned {
        x: f32,
    }

    impl Game for Tuned {
        fn update(&mut self, ctx: &mut Context) {
            let speed = ctx.tune("speed", 10.0);
            let wobble = ctx.tune("wobble", 0.0);
            self.x += speed * ctx.dt() + ctx.rng.range(0.0, wobble);
            ctx.report("speed", speed);
            ctx.report("size", ctx.tune("size", 4.0));
            ctx.report("x", self.x);
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            canvas.fill_rect(Rect::new(self.x, 2.0, 2.0, 2.0), Color::WHITE);
        }
    }

    fn tuned_run(
        name: &str,
        file: Option<&str>,
        set: &[(&str, f32)],
    ) -> (Simulation<Tuned>, String) {
        let folder = test_folder(name);
        fs::create_dir_all(&folder).unwrap();
        let tune_file = file.map(|text| {
            let path = folder.join("game.tune.json");
            fs::write(&path, text).unwrap();
            path
        });
        let options = HeadlessOptions {
            frames: 60,
            out: Some(folder.join("out")),
            tune_file,
            tune: set
                .iter()
                .map(|&(name, value)| (name.to_string(), value))
                .collect(),
            ..HeadlessOptions::default()
        };
        let sim = run_headless(config(), Tuned { x: 0.0 }, &options).unwrap();
        let log = fs::read_to_string(folder.join("out/log.json")).unwrap();
        (sim, log)
    }

    #[test]
    fn set_beats_the_tune_file_which_beats_the_default() {
        let file = r#"{"speed": 20, "size": 6}"#;
        let (defaults, _) = tuned_run("tune-default", None, &[]);
        assert_eq!(defaults.number("speed"), 10.0);
        let (from_file, _) = tuned_run("tune-file", Some(file), &[]);
        assert_eq!(
            (from_file.number("speed"), from_file.number("size")),
            (20.0, 6.0)
        );
        let (from_set, log) = tuned_run("tune-set", Some(file), &[("speed", 30.0)]);
        assert_eq!(
            (from_set.number("speed"), from_set.number("size")),
            (30.0, 6.0)
        );
        assert!(
            log.contains(
                r#""tune": {"size": {"value": 6, "from": "file"}, "speed": {"value": 30, "from": "set"}, "wobble": {"value": 0, "from": "default"}}"#
            ),
            "{log}"
        );
    }

    #[test]
    fn the_same_seed_and_tune_values_give_the_same_run() {
        let file = r#"{"wobble": 2.5}"#;
        let (mut a, log_a) = tuned_run("tune-same-a", Some(file), &[("speed", 7.0)]);
        let (mut b, log_b) = tuned_run("tune-same-b", Some(file), &[("speed", 7.0)]);
        assert_eq!(log_a, log_b);
        assert_eq!(a.picture().pixels(), b.picture().pixels());
        let (other, _) = tuned_run("tune-same-c", Some(file), &[("speed", 8.0)]);
        assert_ne!(
            other.number("x"),
            a.number("x"),
            "a different value plays differently"
        );
    }

    #[test]
    fn write_tune_lists_every_name_the_game_used() {
        let folder = test_folder("tune-write");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("written.tune.json");
        let options = HeadlessOptions {
            frames: 2,
            tune_file: None,
            tune: vec![(String::from("speed"), 12.0)],
            write_tune: true,
            ..HeadlessOptions::default()
        };
        // With no file at all, --write-tune needs to know where to write.
        let mut with_path = options.clone();
        with_path.tune_file = Some(path.clone());
        fs::write(&path, "{}").unwrap();
        run_headless(config(), Tuned { x: 0.0 }, &with_path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "{\n  \"size\": 4,\n  \"speed\": 12,\n  \"wobble\": 0\n}\n"
        );
    }

    #[test]
    fn tune_file_names_the_game_never_asks_for_are_a_warning() {
        let folder = test_folder("tune-stale");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("copied.tune.json");
        // Copied from another game: "speed" is used, the other two aren't.
        fs::write(
            &path,
            r#"{"speed": 3, "player_speed": 90, "enemy_speed": 50}"#,
        )
        .unwrap();
        let options = HeadlessOptions {
            frames: 2,
            out: Some(folder.join("out")),
            tune_file: Some(path.clone()),
            ..HeadlessOptions::default()
        };
        let (_, summary) = run_and_save(config(), Tuned { x: 0.0 }, &options).unwrap();
        assert_eq!(summary.warnings.len(), 1, "{:?}", summary.warnings);
        let warning = &summary.warnings[0];
        assert!(
            warning.contains("has enemy_speed, player_speed, which the game never asked for"),
            "{warning}"
        );
        assert!(
            warning.contains("it asked for: size, speed, wobble"),
            "{warning}"
        );
    }

    #[test]
    fn a_missing_tune_file_is_one_clear_error() {
        let options = HeadlessOptions {
            tune_file: Some(PathBuf::from("nowhere/game.tune.json")),
            ..HeadlessOptions::default()
        };
        let error = run_headless(config(), Tuned { x: 0.0 }, &options)
            .err()
            .unwrap();
        assert!(
            error.contains("couldn't find the tune file nowhere/game.tune.json"),
            "{error}"
        );
    }

    /// Labels a player that walks right, through a camera that follows
    /// it, a "ghost" far outside the level, and a score that's too long.
    struct Labelled {
        x: f32,
    }

    impl Game for Labelled {
        fn update(&mut self, ctx: &mut Context) {
            self.x += 1.0;
            let _ = ctx;
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            let mut camera = crate::camera::Camera2D::new(64.0, 8.0);
            camera.position = vec2(self.x + 32.0, 4.0);
            canvas.push_camera(&camera);
            canvas.label("player", Rect::new(self.x, 2.0, 4.0, 4.0));
            canvas.label("enemy", Rect::new(self.x + 2.0, 2.0, 4.0, 4.0)); // on top of the player!
            canvas.label("ghost", Rect::new(-500.0, 0.0, 4.0, 4.0));
            canvas.pop_camera();
            canvas.draw_text("SCORE 123456789", vec2(10.0, 1.0), 1, Color::WHITE);
        }
    }

    #[test]
    fn scenes_are_saved_described_and_checked() {
        let folder = test_folder("scenes");
        let options = HeadlessOptions {
            frames: 30,
            out: Some(folder.to_path_buf()),
            snapshot_every: Some(15),
            ..HeadlessOptions::default()
        };
        let (mut sim, outcome) = run_and_save(config(), Labelled { x: 0.0 }, &options).unwrap();
        let scene = sim.scene();
        assert_eq!(scene.frame, 30);
        let player = scene.labels_named("player").next().unwrap();
        assert_eq!(player.world, Rect::new(30.0, 2.0, 4.0, 4.0));
        assert_eq!(
            player.screen,
            Rect::new(30.0 - 30.0, 2.0, 4.0, 4.0),
            "the camera keeps it on the left"
        );
        assert!(player.on_screen);
        assert_eq!(
            scene.describe(),
            "frame 30, camera 62,4 zoom 1\n\
             player at 30,2 4x4\n\
             enemy at 32,2 4x4\n\
             ghost at -500,0 4x4 (off screen)\n\
             text: \"SCORE 123456789\" at 10,1"
        );
        let saved = fs::read_to_string(folder.join("frame-000015.scene.json")).unwrap();
        assert!(
            saved.contains(r#""frame": 15"#)
                && saved.contains(r#"{"name": "player", "world": [15, 2, 4, 4]"#),
            "{saved}"
        );
        assert!(folder.join("last.scene.json").exists());

        // The ghost is never on screen, and far outside everything the
        // camera showed: drawn in the wrong place (see on_screen::warnings).
        let warnings = &outcome.warnings;
        assert_eq!(warnings.len(), 3, "{warnings:#?}");
        assert!(
            warnings[2].starts_with(r#"label "ghost" at -500,0 is off screen"#),
            "{warnings:#?}"
        );
        assert!(
            warnings[0]
                .starts_with(r#"frame 1: labels "player" at 1,2 and "enemy" at 3,2 overlap"#),
            "{warnings:#?}"
        );
        assert!(
            warnings[1].starts_with(r#"frame 1: the text "SCORE 123456789""#),
            "{warnings:#?}"
        );
        let log = fs::read_to_string(folder.join("log.json")).unwrap();
        assert!(log.contains(r#""warnings": ["frame 1: labels"#), "{log}");
    }

    #[test]
    fn a_clean_game_has_no_warnings() {
        let folder = test_folder("scenes-clean");
        let options = HeadlessOptions {
            frames: 30,
            out: Some(folder.to_path_buf()),
            ..HeadlessOptions::default()
        };
        let (_, outcome) = run_and_save(config(), Mover { x: 0.0, quit_at: 0 }, &options).unwrap();
        assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    }

    #[test]
    fn grid_lines_and_numbers_go_on_the_saved_pictures_only() {
        let folder = test_folder("grid");
        let tall = Config {
            width: 64,
            height: 64,
            ..config()
        };
        let options = HeadlessOptions {
            frames: 1,
            out: Some(folder.to_path_buf()),
            grid: Some(16),
            ..HeadlessOptions::default()
        };
        let mut sim = run_headless(tall, Mover { x: 0.0, quit_at: 0 }, &options).unwrap();
        let saved = Canvas::load_png_for_tests(&folder.join("last.png"));
        let on_line = saved.get_pixel(16, 40).unwrap();
        assert!(
            on_line.r > 0 && on_line.r < 255,
            "a faint line at x = 16: {on_line:?}"
        );
        assert_eq!(
            saved.get_pixel(20, 40),
            Some(Color::BLACK),
            "nothing between lines"
        );
        // "32" is written along the bottom edge, next to the line at x = 32.
        let numbers =
            (33..45).any(|x| (58..63).any(|y| saved.get_pixel(x, y) == Some(Color::WHITE)));
        assert!(numbers, "coordinates along the edge");
        assert_eq!(
            sim.picture().get_pixel(16, 40),
            Some(Color::BLACK),
            "the game's own picture has no grid"
        );
        let plain = Canvas::load_png_for_tests(&folder.join("last.plain.png"));
        assert_eq!(
            plain.get_pixel(16, 40),
            Some(Color::BLACK),
            "last.plain.png is the frame with nothing on top"
        );
        let log = fs::read_to_string(folder.join("log.json")).unwrap();
        assert!(
            log.contains(
                r#""drawn_on_pictures": {"grid": 16, "grid_world": false, "debug": false}"#
            ),
            "{log}"
        );
    }

    /// Breaks the rule "x below 20" from frame 21 on, and keeps a second
    /// rule that always holds.
    struct Breaker {
        x: u32,
    }

    impl Game for Breaker {
        fn update(&mut self, ctx: &mut Context) {
            self.x += 1;
            ctx.check("x below 20", self.x < 21);
            ctx.check("always true", true);
            ctx.report("x", self.x);
        }

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
        }
    }

    #[test]
    fn a_failed_check_is_recorded_once_with_its_first_frame() {
        let folder = test_folder("checks");
        let options = HeadlessOptions {
            frames: 40,
            out: Some(folder.to_path_buf()),
            ..HeadlessOptions::default()
        };
        let sim = run_headless(config(), Breaker { x: 0 }, &options).unwrap();
        assert_eq!(
            sim.failed_checks(),
            [crate::engine::FailedCheck {
                name: String::from("x below 20"),
                frame: 21
            }]
        );
        let log = fs::read_to_string(folder.join("log.json")).unwrap();
        assert!(
            log.contains(
                r#""checks": {"rules": 2, "failed": [{"name": "x below 20", "frame": 21}]}"#
            ),
            "{log}"
        );
    }

    #[test]
    fn play_tests_fail_on_a_failed_check_unless_told_not_to() {
        let failing = std::panic::catch_unwind(|| {
            crate::PlayTest::new(config(), Breaker { x: 0 })
                .frames(40)
                .run();
        });
        let message = *failing.unwrap_err().downcast::<String>().unwrap();
        assert!(
            message.contains(r#"the check "x below 20" failed on frame 21"#)
                && message.contains("--frames 20"),
            "{message}"
        );
        let allowed = crate::PlayTest::new(config(), Breaker { x: 0 })
            .frames(40)
            .ignore_checks()
            .run();
        assert_eq!(allowed.failed_checks().len(), 1);
        let fine = crate::PlayTest::new(config(), Breaker { x: 0 })
            .frames(20)
            .run();
        assert!(
            fine.failed_checks().is_empty(),
            "holds for the first 20 frames"
        );
    }

    /// Looks at world point (1000, 500) at zoom 2 on a 160 x 120 screen.
    struct Zoomed;

    impl Game for Zoomed {
        fn update(&mut self, _: &mut Context) {}

        fn draw(&self, canvas: &mut Canvas) {
            canvas.clear(Color::BLACK);
            let mut camera = crate::camera::Camera2D::new(160.0, 120.0);
            camera.position = vec2(1000.0, 500.0);
            camera.zoom = 2.0;
            canvas.push_camera(&camera);
            canvas.fill_rect(Rect::new(1000.0, 500.0, 1.0, 1.0), Color::RED);
            canvas.pop_camera();
        }
    }

    fn saved_last(name: &str, game_config: Config, options: HeadlessOptions) -> Canvas {
        let folder = test_folder(name);
        let options = HeadlessOptions {
            frames: 2,
            out: Some(folder.to_path_buf()),
            ..options
        };
        run_headless(game_config, Zoomed, &options).unwrap();
        Canvas::load_png_for_tests(&folder.join("last.png"))
    }

    #[test]
    fn a_world_grid_follows_the_camera() {
        let small = Config {
            width: 160,
            height: 120,
            ..config()
        };
        let options = HeadlessOptions {
            grid: Some(50),
            grid_world: true,
            ..HeadlessOptions::default()
        };
        let picture = saved_last("grid-world", small, options);
        // World x = 1000 is the middle of the screen (x = 80); x = 1050 is
        // 50 world units = 100 pixels further, off the screen; 950 is at -20.
        // So the one vertical line is at x = 80, and it's labelled "1000".
        let column = |x: i32| {
            (20..100)
                .filter(|&y| picture.get_pixel(x, y) != Some(Color::BLACK))
                .count()
        };
        assert!(column(80) > 50, "a line at world x = 1000");
        assert_eq!(
            column(70),
            1,
            "no screen-pixel lines, only the crossing world y = 500"
        );
        let row = |y: i32| {
            (0..160)
                .filter(|&x| picture.get_pixel(x, y) != Some(Color::BLACK))
                .count()
        };
        assert!(
            row(60) > 100,
            "a line at world y = 500, the middle of the screen"
        );
        let numbers: String = (81..100)
            .map(|x| {
                if (113..118).any(|y| picture.get_pixel(x, y) == Some(Color::WHITE)) {
                    '#'
                } else {
                    '.'
                }
            })
            .collect();
        assert!(
            numbers.contains('#'),
            "a number next to the line: {numbers}"
        );
    }

    #[test]
    fn the_grid_leaves_the_debug_box_alone() {
        let small = Config {
            width: 160,
            height: 120,
            ..config()
        };
        let plain = saved_last(
            "grid-debug-plain",
            small.clone(),
            HeadlessOptions {
                debug: true,
                ..HeadlessOptions::default()
            },
        );
        let gridded = saved_last(
            "grid-debug-grid",
            small,
            HeadlessOptions {
                debug: true,
                grid: Some(8),
                ..HeadlessOptions::default()
            },
        );
        // The box is in the bottom-right corner: identical with or without the grid.
        for (x, y) in [(159, 119), (120, 110), (150, 105)] {
            assert_eq!(plain.get_pixel(x, y), gridded.get_pixel(x, y), "({x}, {y})");
        }
        assert_ne!(
            plain.get_pixel(8, 40),
            gridded.get_pixel(8, 40),
            "the grid is there elsewhere"
        );
    }

    #[test]
    fn a_silent_game_saves_no_audio() {
        let folder = test_folder("audio-none");
        let options = HeadlessOptions {
            frames: 5,
            out: Some(folder.to_path_buf()),
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
            out: Some(folder.to_path_buf()),
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
        let folder = test_folder("headless-not-a-folder");
        let file = folder.file("in-the-way", "in the way");
        let options = HeadlessOptions {
            frames: 1,
            out: Some(file.join("pictures")),
            ..HeadlessOptions::default()
        };
        let err = run_headless(config(), game(), &options).err().unwrap();
        assert!(err.contains("could not create the folder"), "{err}");
    }

    /// [`Tuned`], also asking for a "shade" that changes nothing it reports.
    struct Shaded(Tuned);

    impl Game for Shaded {
        fn update(&mut self, ctx: &mut Context) {
            self.0.update(ctx);
            let _ = ctx.tune("shade", 1.0);
        }

        fn draw(&self, canvas: &mut Canvas) {
            self.0.draw(canvas);
        }
    }

    #[test]
    fn a_replay_uses_the_tune_values_it_was_recorded_with() {
        let folder = test_folder("tune-replay");
        fs::create_dir_all(&folder).unwrap();
        let set = |pairs: &[(&str, f32)]| -> Vec<(String, f32)> {
            pairs.iter().map(|&(n, v)| (n.to_string(), v)).collect()
        };
        // Recorded with --set wobble=0.5 (and shade 2, which changes nothing
        // the game reports): its ending, and its tune values.
        let recorded = run_headless(
            config(),
            Shaded(Tuned { x: 0.0 }),
            &HeadlessOptions {
                frames: 30,
                tune: set(&[("wobble", 0.5), ("shade", 2.0)]),
                ..HeadlessOptions::default()
            },
        )
        .unwrap();
        let mut script = recorded.recording();
        assert_eq!(
            script.tune.as_ref().unwrap()["wobble"],
            0.5,
            "the recording keeps them"
        );
        script.events.clear();
        let replay = |config: Config, tune: Vec<(String, f32)>, tune_file: Option<PathBuf>| {
            let options = HeadlessOptions {
                frames: 30,
                out: Some(folder.join("out")),
                inputs: Some(script.clone()),
                inputs_file: Some(PathBuf::from("playthroughs/t.json")),
                tune,
                tune_file,
                ..HeadlessOptions::default()
            };
            run_and_save(config, Shaded(Tuned { x: 0.0 }), &options)
                .unwrap()
                .1
        };

        // No --set: the recorded values are used, so it ends as recorded,
        // and the summary says which values came from the recording.
        let plain = replay(config(), Vec::new(), None);
        assert!(plain.changed.is_empty() && plain.tune_changed.is_empty());
        assert!(plain.warnings.is_empty(), "{:?}", plain.warnings);
        assert!(
            plain
                .tuned
                .contains(&(String::from("wobble"), 0.5, "recording", 0.0))
        );
        let text = plain.to_text();
        assert!(
            text.contains("wobble = 0.5 (from the recording; code says 0)"),
            "{text}"
        );
        // The game's own tune file doesn't beat the recording either.
        let file = folder.join("game.tune.json");
        fs::write(&file, r#"{"wobble": 0}"#).unwrap();
        let named = Config {
            tune_file: Some(file.clone()),
            ..config()
        };
        assert!(replay(named, Vec::new(), None).changed.is_empty());

        // --set wins: wobble 0 ends differently, and the summary names
        // the changed value as the likely cause.
        let failed = replay(config(), set(&[("wobble", 0.0)]), None);
        assert_eq!(failed.warnings.len(), 1, "{:?}", failed.warnings);
        let warning = &failed.warnings[0];
        assert!(
            warning.starts_with("playthroughs/t.json ends differently (x "),
            "{warning}"
        );
        assert!(
            warning.contains("tune values changed since it was recorded (wobble 0.5 to 0)"),
            "{warning}"
        );
        // --tune FILE wins too.
        let from_file = replay(config(), Vec::new(), Some(file));
        assert_eq!(from_file.warnings.len(), 1, "{:?}", from_file.warnings);

        // A value changed with --set that doesn't change the ending: only
        // the "playthrough tuned" warning.
        let tuned = replay(config(), set(&[("shade", 3.0)]), None);
        assert_eq!(tuned.tune_changed, ["shade 2 to 3"]);
        assert!(tuned.changed.is_empty() && tuned.warnings.is_empty());
        assert!(tuned.problems()[0].starts_with(
            "playthrough tuned: recorded with other tune values (shade 2 to 3), and it still ends as expected"
        ));
    }

    #[test]
    fn a_game_can_name_its_tune_file() {
        let folder = test_folder("tune-named");
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("named.tune.json");
        fs::write(&path, r#"{"speed": 20}"#).unwrap();
        let named = Config {
            tune_file: Some(path.clone()),
            ..config()
        };
        // A Simulation (what bots and play tests use) and a headless run
        // both read it, whatever the program is called.
        let mut sim = Simulation::new(named.clone(), Tuned { x: 0.0 });
        sim.step();
        assert_eq!(sim.number("speed"), 20.0);
        let run = run_headless(
            named,
            Tuned { x: 0.0 },
            &HeadlessOptions {
                frames: 2,
                ..HeadlessOptions::default()
            },
        )
        .unwrap();
        assert_eq!(run.number("speed"), 20.0);
        // And it's fine for it not to exist yet: the code's values.
        let missing = Config {
            tune_file: Some(folder.join("not-yet.tune.json")),
            ..config()
        };
        let mut sim = Simulation::new(missing, Tuned { x: 0.0 });
        sim.step();
        assert_eq!(sim.number("speed"), 10.0);
        // A recording keeps the values it was made with.
        assert_eq!(sim.recording().tune.unwrap()["speed"], 10.0);
    }

    #[test]
    fn a_replay_that_ends_differently_is_a_changed_playthrough() {
        let folder = test_folder("expect");
        // ReportingMover reports "x", one more each frame: 30 after 30 frames.
        let script = |x: u32| {
            InputScript::parse(&format!(
                r#"{{"frames": 30, "expect": {{"x": {x}, "far": true}}, "events": []}}"#
            ))
            .unwrap()
        };
        let options = |script| HeadlessOptions {
            frames: 30,
            out: Some(folder.to_path_buf()),
            inputs: Some(script),
            ..HeadlessOptions::default()
        };
        let run = |x| {
            run_and_save(config(), ReportingMover { x: 0.0 }, &options(script(x)))
                .unwrap()
                .1
        };
        assert!(run(30).changed.is_empty(), "it ends as expected");
        let summary = run(25);
        assert_eq!(summary.changed, ["x 25 to 30"]);
        assert!(!summary.ok());
        assert!(summary.problems().last().unwrap().starts_with(
            "playthrough changed: x 25 to 30; if that's right, accept it with --accept"
        ));
        let log = fs::read_to_string(folder.join("log.json")).unwrap();
        assert!(
            log.contains(r#""playthrough_changed": ["x 25 to 30"]"#),
            "{log}"
        );
        // A different length ends somewhere else: no comparison.
        let mut shorter = options(script(25));
        shorter.frames = 20;
        let (_, summary) = run_and_save(config(), ReportingMover { x: 0.0 }, &shorter).unwrap();
        assert!(summary.changed.is_empty());
    }

    #[test]
    fn an_event_sheet_shows_the_frames_where_values_changed() {
        /// Loses a life on frames 500 and 550; "x" changes every frame.
        struct Lives;
        impl Game for Lives {
            fn update(&mut self, ctx: &mut Context) {
                let frame = ctx.frame();
                ctx.report(
                    "lives",
                    3 - u32::from(frame >= 500) - u32::from(frame >= 550),
                );
                ctx.report("x", frame);
            }
            fn draw(&self, canvas: &mut Canvas) {
                canvas.clear(Color::BLACK);
            }
        }
        let folder = test_folder("sheet-events");
        let options = HeadlessOptions {
            frames: 600,
            out: Some(folder.to_path_buf()),
            sheet_events: true,
            ..HeadlessOptions::default()
        };
        let (_, summary) = run_and_save(config(), Lives, &options).unwrap();
        assert!(summary.files.contains(&String::from("sheet.png")));
        let sheet = Canvas::load_png_for_tests(&folder.join("sheet.png"));
        // 16 frames of 64 x 8 in 4 rows, with a caption line each.
        assert_eq!(sheet.height(), 4 + 4 * (8 + 14 + 13 + 4));
    }

    /// Both ways of making a video, in one test (the first changes an
    /// environment variable, which every test in this program shares).
    #[test]
    fn a_video_has_every_frame_or_the_frames_and_the_command() {
        let folder = test_folder("video");
        let options = |name: &str| HeadlessOptions {
            frames: 30,
            out: Some(folder.join(name)),
            video: Some(folder.join(format!("{name}.mp4"))),
            ..HeadlessOptions::default()
        };
        // Without ffmpeg: 30 PNG frames, and the command to join them.
        // SAFETY: no other test in this program reads this variable.
        unsafe { std::env::set_var("DUCKFORGE_FFMPEG", "no-such-ffmpeg-here") };
        let result = run_and_save(config(), Mover { x: 0.0, quit_at: 0 }, &options("pngs"));
        unsafe { std::env::remove_var("DUCKFORGE_FFMPEG") };
        let summary = result.unwrap().1;
        let frames = fs::read_dir(folder.join("pngs/video-frames"))
            .unwrap()
            .count();
        assert_eq!(frames, 30);
        let note = summary
            .notes
            .iter()
            .find(|n| n.starts_with("video:"))
            .unwrap();
        assert!(
            note.contains("ffmpeg wasn't found, so the 30 frames are in"),
            "{note}"
        );
        assert!(note.contains("-framerate 60 -i "), "{note}");
        assert!(
            note.contains("scale=1920:240:flags=neighbor"),
            "64 x 8, 30 times: {note}"
        );

        // A clip without ffmpeg: a GIF of the run's last half second, full
        // size, 50 pictures a second (five of every six frames).
        let clip = |name: &str| HeadlessOptions {
            frames: 90,
            out: Some(folder.join(name)),
            clip: Some(folder.join(format!("{name}.mp4"))),
            clip_seconds: 0.5,
            ..HeadlessOptions::default()
        };
        unsafe { std::env::set_var("DUCKFORGE_FFMPEG", "no-such-ffmpeg-here") };
        let result = run_and_save(config(), Mover { x: 0.0, quit_at: 0 }, &clip("gif-clip"));
        unsafe { std::env::remove_var("DUCKFORGE_FFMPEG") };
        let summary = result.unwrap().1;
        let note = summary
            .notes
            .iter()
            .find(|n| n.starts_with("clip:"))
            .unwrap();
        assert!(note.contains("gif-clip.gif (25 pictures, 0.5 s at 50 a second, full size"));
        assert!(note.contains("install ffmpeg for a video with sound: https://ffmpeg.org"));
        let gif = fs::File::open(folder.join("gif-clip.gif")).unwrap();
        let mut decoder = gif::DecodeOptions::new().read_info(gif).unwrap();
        assert_eq!((decoder.width(), decoder.height()), (64, 8), "full size");
        let mut pictures = 0;
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            assert_eq!(frame.delay, 2, "two hundredths of a second each");
            pictures += 1;
        }
        assert_eq!(pictures, 25);

        // With ffmpeg (if this machine has it): a 1080p video, 30 frames.
        let has_ffmpeg = Command::new("ffmpeg").arg("-version").output().is_ok();
        if has_ffmpeg {
            let (_, summary) =
                run_and_save(config(), Mover { x: 0.0, quit_at: 0 }, &options("real")).unwrap();
            let note = summary
                .notes
                .iter()
                .find(|n| n.starts_with("video:"))
                .unwrap();
            assert!(note.contains("30 frames, 0.5 s at 60 fps"), "{note}");
            let probe = Command::new("ffprobe")
                .args(["-v", "error", "-count_frames", "-show_entries"])
                .args(["stream=width,height,nb_read_frames", "-of", "csv=p=0"])
                .arg(folder.join("real.mp4"))
                .output();
            if let Ok(probe) = probe {
                assert_eq!(
                    String::from_utf8_lossy(&probe.stdout).trim(),
                    "1920,1080,30"
                );
            }
            // And a clip is a video too: the last 30 of 90 frames.
            let (_, summary) =
                run_and_save(config(), Mover { x: 0.0, quit_at: 0 }, &clip("real-clip")).unwrap();
            let note = summary
                .notes
                .iter()
                .find(|n| n.starts_with("clip:"))
                .unwrap();
            assert!(note.contains("real-clip.mp4"), "{note}");
            assert!(note.contains("30 frames, 0.5 s at 60 fps"), "{note}");
            assert!(folder.join("real-clip.mp4").exists());
        }
    }

    #[test]
    fn events_after_the_last_frame_run_are_a_warning() {
        let folder = test_folder("skipped-events");
        let script = InputScript::parse(
            r#"{"events": [{"frame": 5, "press": "Space"}, {"frame": 898, "release": "Space"},
                           {"frame": 899, "press": "Left"}]}"#,
        )
        .unwrap();
        let options = HeadlessOptions {
            frames: 600,
            out: Some(folder.to_path_buf()),
            inputs: Some(script.clone()),
            inputs_file: Some(PathBuf::from("playthroughs/hand.json")),
            ..HeadlessOptions::default()
        };
        let mover = || Mover { x: 0.0, quit_at: 0 };
        let (_, summary) = run_and_save(config(), mover(), &options).unwrap();
        assert_eq!(summary.warnings.len(), 1, "{:?}", summary.warnings);
        assert!(
            summary.warnings[0].starts_with(
                "playthroughs/hand.json: 2 of its 3 input events come after frame 600"
            ),
            "{}",
            summary.warnings[0]
        );
        assert!(summary.warnings[0].contains("at least 899"));
        // Run long enough, and all is well.
        let options = HeadlessOptions {
            frames: script.run_length(60.0).unwrap(),
            ..options
        };
        let (_, summary) = run_and_save(config(), mover(), &options).unwrap();
        assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
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
            out: Some(folder.to_path_buf()),
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
                "frame-000010.scene.json",
                "frame-000020.png",
                "frame-000020.scene.json",
                "frame-000030.png",
                "frame-000030.scene.json",
                "last.png",
                "last.scene.json",
                "log.json",
                "notes.txt",
                "run.gif",
                "sheet.png",
                "summary.txt",
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
        assert_eq!(log["files"].as_array().unwrap().len(), 16); // all but notes.txt

        // The summary: final values, the number's range, when "far" changed.
        let summary = fs::read_to_string(folder.join("summary.txt")).unwrap();
        assert!(summary.contains("for 30 frames"), "{summary}");
        assert!(
            summary.contains("\nfinal: far = true, x = 30\n"),
            "{summary}"
        );
        assert!(
            summary.contains("\nlowest to highest: x 1 to 30\n"),
            "{summary}"
        );
        assert!(
            summary.contains("\nfar: false on frame 1, true on 10\n"),
            "{summary}"
        );
        assert!(summary.contains("3 snapshots"), "{summary}");
    }

    #[test]
    fn our_files_are_recognised_by_name() {
        assert!(is_our_file("frame-000060.png"));
        assert!(is_our_file("closeup-123456.png"));
        assert!(is_our_file("log.json"));
        assert!(is_our_file("summary.txt"));
        assert!(is_our_file("frame-000060.scene.json"));
        assert!(is_our_file("last.scene.json"));
        assert!(!is_our_file("frame-000060.json"));
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
