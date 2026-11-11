//! Duck Forge Studio: make games without a terminal, with the game
//! playing inside the Studio.
//!
//!   DuckForgeStudio                      opens the Studio
//!   DuckForgeStudio --run script.json    runs a list of commands, no window
//!   DuckForgeStudio --timing duck_peaks  plays a game for ten seconds with
//!                                        no window and prints its Timing line
//!   DuckForgeStudio --commands           lists every command

// A window app: no black console window behind it on Windows. (Debug
// builds keep it, so what the Studio prints can be read.)
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::time::{Duration, Instant};

use eframe::egui;
use studio_app::{StudioApp, theme};
use studio_core::script::{Step, Wait, frame, run_script, run_step};
use studio_core::settings::Settings;
use studio_core::studio::COMMANDS;
use studio_core::{Command, Studio};

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--run") => std::process::exit(run_file(args.get(1))),
        Some("--timing") => std::process::exit(timing(args.get(1), args.get(2))),
        Some("--commands") => {
            for command in COMMANDS {
                let needs = if command.needs.is_empty() {
                    String::new()
                } else {
                    format!("  ({})", command.needs)
                };
                println!("{:<24} {}{needs}", command.name, command.title);
            }
            return Ok(());
        }
        Some("--version") => {
            println!("Duck Forge Studio {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }

    // The window and taskbar icon. (DuckForgeStudio.exe's own icon, the
    // one File Explorer shows, is put in by build.rs.)
    let icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../../../brand/duck-forge-icon-256.png"))
            .expect("brand/duck-forge-icon-256.png should be a valid PNG file");
    let settings = Settings::load();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Duck Forge Studio")
            // The size used when the window isn't maximized. The game needs
            // room, so it starts maximized.
            .with_inner_size([1280.0, 800.0])
            .with_maximized(true)
            .with_min_inner_size([720.0, 480.0])
            .with_icon(icon),
        ..Default::default()
    };
    eframe::run_native(
        "Duck Forge Studio",
        options,
        Box::new(|creation_context| {
            let ctx = &creation_context.egui_ctx;
            ctx.set_theme(theme::from_name(settings.theme.as_deref()));
            theme::apply_brand(ctx);
            Ok(Box::new(StudioApp::new(Studio::new(settings))))
        }),
    )
}

/// Runs a script file with no window. The exit code is 0 if every step
/// worked.
fn run_file(file: Option<&String>) -> i32 {
    let Some(file) = file else {
        eprintln!("usage: DuckForgeStudio --run script.json");
        return 2;
    };
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(problem) => {
            eprintln!("Couldn't read {file}: {problem}");
            return 2;
        }
    };
    let mut studio = Studio::new(Settings::load());
    match run_script(&mut studio, &text) {
        Ok(said) => {
            for line in said {
                println!("{line}");
            }
            0
        }
        Err(problem) => {
            eprintln!("{}\n{}", problem.what, problem.next);
            if !problem.details.is_empty() {
                eprintln!("\n{}", problem.details);
            }
            1
        }
    }
}

/// Plays a game through the link for a while with no window, exactly as
/// the app does it (one tick per sixtieth of a second, waiting for each
/// picture), and prints the Timing line.
fn timing(game: Option<&String>, seconds: Option<&String>) -> i32 {
    let Some(game) = game else {
        eprintln!("usage: DuckForgeStudio --timing <game> [seconds]");
        return 2;
    };
    let seconds: f32 = seconds.and_then(|s| s.parse().ok()).unwrap_or(10.0);
    let mut studio = Studio::new(Settings::load());
    let steps = [
        Step::Do(Command::GameOpen { name: game.clone() }),
        Step::Wait(Wait::Playing),
    ];
    for step in &steps {
        if let Err(problem) = run_step(&mut studio, step) {
            eprintln!("{}\n{}\n{}", problem.what, problem.next, problem.details);
            return 1;
        }
    }
    // A second to settle (the first frames load pictures and sounds).
    let settle = Instant::now() + Duration::from_secs(1);
    while Instant::now() < settle {
        frame(&mut studio, Instant::now());
    }
    let mut lines = Vec::new();
    let until = Instant::now() + Duration::from_secs_f32(seconds);
    let mut last_line = Instant::now();
    while Instant::now() < until {
        frame(&mut studio, Instant::now());
        if last_line.elapsed() >= Duration::from_secs(5) {
            lines.push(studio.timing(Instant::now()));
            last_line = Instant::now();
        }
    }
    lines.push(studio.timing(Instant::now()));
    let renderer = studio.play().map_or(String::new(), |play| play.drawn_by());
    println!("{game}, {seconds} s through the link, drawn by {renderer}:");
    for timing in lines {
        println!(
            "  {:.1} updates a second, {:.1} pictures a second, longest gap {:.1} ms, game \
             {:.2} ms, link {:.2} ms, slowest round trip {:.1} ms, late {} of {}",
            timing.updates_per_second,
            timing.pictures_per_second,
            timing.longest_gap,
            timing.game,
            timing.link,
            timing.slowest,
            timing.late,
            timing.ticks
        );
    }
    let _ = studio.run(Command::GameStop);
    0
}
