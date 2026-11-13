//! Every screen of the Studio, drawn with no window.
//!
//! `egui_kittest` runs the app's own drawing code against a pretend
//! screen and renders it to a picture, so this needs no display. The
//! Studio underneath is the real one: real games are built, played
//! through the link, checked and clipped.
//!
//! The quick test below always runs. The screenshots take a few minutes
//! (they build and play real games), so they only run when asked:
//!
//! ```sh
//! STUDIO_SCREENSHOTS=../docs/studio-screens cargo test -p studio_app --test screens -- --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use studio_app::{StudioApp, theme};
use studio_core::build::{Build, BuildState, rust_is_missing};
use studio_core::play::PlayState;
use studio_core::settings::Settings;
use studio_core::{Command, Problem, Studio};

fn duck_forge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

fn studio() -> Studio {
    // Default settings are never saved: the person's own are never touched.
    Studio::new(Settings {
        games_folder: Some(duck_forge()),
        ..Settings::default()
    })
}

/// The app on a pretend 1280 x 800 screen, run the way eframe runs it.
fn harness(app: StudioApp, render: bool) -> Harness<'static, StudioApp> {
    let builder = Harness::builder()
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_render_cursor(false)
        .with_context_setup(|ctx| {
            ctx.set_theme(egui::ThemePreference::Dark);
            theme::apply_brand(ctx);
        });
    let builder = if render { builder.wgpu() } else { builder };
    builder.build_eframe(|_| app)
}

#[test]
fn home_shows_new_game_the_games_and_the_links() {
    let mut harness = harness(StudioApp::new(studio()), false);
    harness.step();
    harness.get_by_label("+ New game");
    harness.get_by_label("Duck Quill");
    harness.get_by_label("Duck Echo");
    harness.get_by_label("My games");
    harness.get_by_label("Duck Peaks");
    // New game opens its dialog: pick a starter, then name it.
    harness.get_by_label("+ New game").click();
    harness.run_steps(3);
    harness.get_by_label("New game: pick a starter");
    harness.get_by_label("Platformer");
    harness.get_by_label("Next").click();
    harness.run_steps(3);
    harness.get_by_label("New game: name it");
}

#[test]
fn a_new_version_shows_a_banner_and_waits_for_a_click() {
    use studio_app::update_banner::UpdateState;
    use studio_core::update::NewVersion;
    // A pretend answer from the check: no internet is used here.
    let mut app = StudioApp::new(studio());
    assert!(matches!(app.update, UpdateState::Off), "tests never check");
    app.update = UpdateState::Available(NewVersion {
        version: String::from("9.9.9"),
        whats_new: String::from("https://example.test/releases/tag/studio-v9.9.9"),
        installer: Some((
            String::from("https://example.test/DuckForgeStudioSetup.exe"),
            5,
        )),
    });
    let mut screen = harness(app, false);
    screen.step();
    screen.get_by_label("Duck Forge Studio 9.9.9 is out.");
    screen.get_by_label(&format!("You have {}.", env!("CARGO_PKG_VERSION")));
    screen.get_by_label("What's new");
    screen.get_by_label("Update");
    // Not now: it goes, and nothing was downloaded.
    screen.get_by_label("Not now").click();
    screen.run_steps(2);
    assert!(screen.query_by_label("Update").is_none());
    assert!(matches!(screen.state().update, UpdateState::Dismissed));

    // Up to date, or no internet: no banner at all.
    let mut app = StudioApp::new(studio());
    app.update = UpdateState::Nothing;
    let mut screen = harness(app, false);
    screen.step();
    assert!(screen.query_by_label_contains("is out.").is_none());

    // Settings has the switch, on unless it's turned off.
    screen.get_by_label("Settings").click();
    screen.run_steps(2);
    screen
        .get_by_label("Tell me when a new version is out")
        .click();
    screen.run_steps(2);
    assert!(!screen.state().studio.settings.check_for_updates);
    assert!(matches!(screen.state().update, UpdateState::Off));
}

/// A games folder of the test's own, with one game of the person's.
fn small_games_folder(test: &studio_core::test_folder::TestFolder) -> PathBuf {
    let root = test.join("Duck Forge");
    for (name, text) in [
        (
            "Cargo.toml",
            "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"bee_maze\"\ntest = true\n",
        ),
        ("examples/dodge.rs", "fn main() {}\n"),
        ("examples/bee_maze/main.rs", "fn main() {}\n"),
        ("examples/bee_maze/assets/duck.png", "not really a picture"),
        ("examples/bee_maze/assets/jump.wav", "RIFF"),
        ("examples/bee_maze/level.txt", "p..#\n"),
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    root
}

#[test]
fn a_card_can_be_renamed_duplicated_or_deleted_after_a_question() {
    let test = studio_core::test_folder::TestFolder::new("screens-cards");
    let root = small_games_folder(&test);
    let mut studio = Studio::new(Settings {
        games_folder: Some(root.clone()),
        ..Settings::default()
    });
    studio.recycle_bin = studio_core::recycle::RecycleBin::Folder(test.join("Recycle Bin"));
    let mut screen = harness(StudioApp::new(studio), false);
    screen.step();
    // The person's game has all three; Dodge (it came with Duck Forge) can only be copied.
    assert_eq!(screen.query_all_by_label("Rename").count(), 1);
    assert_eq!(screen.query_all_by_label("Duplicate").count(), 2);
    assert_eq!(screen.query_all_by_label("Delete").count(), 1);

    // Duplicate asks for the copy's name.
    screen
        .query_all_by_label("Duplicate")
        .next()
        .unwrap()
        .click();
    screen.run_steps(3);
    screen.get_by_label("Duplicate Bee Maze");
    screen.get_by_label("Its folder will be examples/bee_maze_2");
    screen.get_by_label("Cancel").click();
    screen.run_steps(3);
    assert!(screen.state().game_dialog.is_none());

    // Delete names the game and asks first; then it's in the Recycle Bin.
    screen.get_by_label("Delete").click();
    screen.run_steps(3);
    screen.get_by_label("Delete Bee Maze?");
    assert!(root.join("examples/bee_maze").exists(), "nothing yet");
    screen.get_by_label("Move it to the Recycle Bin").click();
    screen.run_steps(3);
    assert!(!root.join("examples/bee_maze").exists());
    assert!(test.join("Recycle Bin/bee_maze/main.rs").is_file());
    assert!(
        !std::fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .contains("bee_maze")
    );
    assert!(screen.query_by_label("Bee Maze").is_none());
}

#[test]
fn new_game_lists_empty_first_and_says_what_each_starter_has() {
    let mut screen = harness(StudioApp::new(studio()), false);
    screen.step();
    screen.get_by_label("+ New game").click();
    screen.run_steps(3);
    let starters: Vec<String> = studio_core::folder::STARTERS
        .iter()
        .map(|s| s.title.to_string())
        .collect();
    assert_eq!(starters[0], "Empty");
    for starter in studio_core::folder::STARTERS {
        // (Home's cards are behind the dialog, with some of the same names.)
        assert!(
            screen.query_all_by_label(starter.title).count() >= 1,
            "{}",
            starter.title
        );
        screen.get_by_label(starter.has);
    }
    assert_eq!(
        screen.query_all_by_label("Has a level to paint.").count(),
        2
    );
    assert_eq!(screen.query_all_by_label("No level to paint.").count(), 4);
}

#[test]
fn the_files_panel_shows_the_games_folder_and_clicks_are_commands() {
    use studio_app::files_panel::FilesPanel;
    use studio_app::pictures::Pictures;
    let test = studio_core::test_folder::TestFolder::new("screens-files");
    let root = small_games_folder(&test);
    let folder = root.join("examples/bee_maze");
    let commands = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let seen = commands.clone();
    let mut panel = FilesPanel::default();
    let mut pictures = Pictures::default();
    let mut screen = egui_kittest::Harness::new_ui(move |ui| {
        let more = panel.show(ui, Some(&folder), None, &mut pictures, false);
        seen.borrow_mut().extend(more);
    });
    screen.run();
    for line in ["v assets", "duck.png", "jump.wav", "level.txt", "main.rs"] {
        screen.get_by_label(line);
    }
    screen.get_by_label("Drop pictures and sounds here from File Explorer to copy them in.");
    screen.get_by_label("jump.wav").click();
    screen.run();
    assert!(commands.borrow().contains(&Command::FilesPlay {
        path: String::from("assets/jump.wav")
    }));
    screen.get_by_label("Show in File Explorer").click();
    screen.run();
    assert!(commands.borrow().contains(&Command::FilesOpen {
        path: String::new()
    }));
}

#[test]
fn build_mode_without_a_level_says_what_there_is_to_do() {
    let mut screen = egui_kittest::Harness::new_ui(|ui| {
        studio_app::build_mode::no_level_to_paint(ui, true);
    });
    screen.run();
    for word in ["Feel:", "Sounds:", "Pictures:", "Files:", "Frame by frame:"] {
        screen.get_by_label(word);
    }
}

/// Puts the repository back as it was after the screenshots made a game.
/// One at a time: each of the two long tests makes a game in the Duck
/// Forge folder (and takes it out again).
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct PutBack {
    root: PathBuf,
    manifest: String,
    game: &'static str,
}

impl Drop for PutBack {
    fn drop(&mut self) {
        let _ = std::fs::write(self.root.join("Cargo.toml"), &self.manifest);
        let _ = std::fs::remove_dir_all(self.root.join("examples").join(self.game));
        let _ = std::fs::remove_file(
            self.root
                .join("playthroughs")
                .join(format!("{}.json", self.game)),
        );
        for folder in ["out/verify", "out/sessions", "history"] {
            let _ = std::fs::remove_dir_all(self.root.join(folder).join(self.game));
        }
        for end in ["feel.json", "tune.json"] {
            let _ = std::fs::remove_file(
                self.root
                    .join("examples")
                    .join(format!("{}.{end}", self.game)),
            );
        }
    }
}

struct Shots {
    folder: PathBuf,
    harness: Harness<'static, StudioApp>,
}

impl Shots {
    /// Runs the app's frames at sixty a second until `done`, or gives up
    /// after `patience`.
    fn until(&mut self, what: &str, patience: Duration, mut done: impl FnMut(&StudioApp) -> bool) {
        let started = Instant::now();
        loop {
            let frame = Instant::now();
            self.harness.step();
            if done(self.harness.state()) {
                return;
            }
            assert!(
                started.elapsed() < patience,
                "waited {patience:?} for {what}; the Studio's problem: {:?}",
                self.harness.state().studio.problem
            );
            if let Some(rest) = Duration::from_micros(16_667).checked_sub(frame.elapsed()) {
                std::thread::sleep(rest);
            }
        }
    }

    /// Runs for this long.
    fn play_for(&mut self, time: Duration) {
        let until = Instant::now() + time;
        self.until("time to pass", time * 2 + Duration::from_secs(5), |_| {
            Instant::now() >= until
        });
    }

    fn run(&mut self, command: Command) {
        let result = self.harness.state_mut().studio.run(command);
        if let Err(problem) = result {
            panic!("{problem}\n{}", problem.details);
        }
    }

    fn input(&mut self, event: serde_json::Value) {
        let mut command = event;
        command["do"] = "game.input".into();
        self.run(serde_json::from_value(command).unwrap());
    }

    /// Saves a picture of the screen as it is now.
    fn shoot(&mut self, name: &str) {
        // A few frames first: a panel that just appeared takes two to
        // settle into its size.
        self.harness.run_steps(3);
        let picture = self.harness.render().expect("the screen can be drawn");
        let path = self.folder.join(format!("{name}.png"));
        picture.save(&path).unwrap();
        println!("saved {}", path.display());
    }

    fn playing(&mut self) {
        self.until("the game to play", Duration::from_secs(900), |app| {
            app.studio.play().is_some_and(|play| {
                play.state == PlayState::Playing && play.picture.is_some() && play.frame > 5
            })
        });
    }

    fn idle(&mut self) {
        self.harness.step();
        self.until("the Studio to finish", Duration::from_secs(900), |app| {
            !app.studio.is_busy() && app.studio.making.is_none()
        });
    }
}

#[test]
fn every_screen() {
    let Some(folder) = std::env::var_os("STUDIO_SCREENSHOTS") else {
        println!("set STUDIO_SCREENSHOTS=<folder> to save a picture of every screen");
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let folder = PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let root = duck_forge();
    let _put_back = PutBack {
        manifest: std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
        root: root.clone(),
        game: "bee_maze",
    };
    let mut shots = Shots {
        folder,
        harness: harness(StudioApp::new(studio()), true),
    };

    // New game: the two pages of the dialog, then the real thing.
    shots.harness.step();
    shots.harness.get_by_label("+ New game").click();
    shots.harness.run_steps(3);
    shots.shoot("02-new-game-starter");
    shots.harness.get_by_label("Next").click();
    shots.harness.run_steps(3);
    shots.harness.state_mut().new_game.as_mut().unwrap().name = String::from("Bee Maze");
    shots.harness.run_steps(3);
    shots.shoot("03-new-game-name");
    shots.harness.get_by_label("Make it").click();
    shots.harness.run_steps(2);
    // The new game is made, opened, built and starts playing by itself.
    shots.playing();
    assert_eq!(
        shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .game
            .name,
        "bee_maze"
    );
    shots.input(serde_json::json!({"press": "Right"}));
    shots.play_for(Duration::from_secs(2));
    shots.input(serde_json::json!({"press": "Space"}));
    shots.play_for(Duration::from_millis(250));
    shots.harness.state_mut().studio.settings.show_timing = true;
    shots.shoot("04-game-playing");

    // Tab, mid-jump: frozen on that frame, with Build beside it.
    shots.harness.key_press(egui::Key::Tab);
    shots.harness.run_steps(2);
    assert_eq!(
        shots.harness.state().studio.play().unwrap().state,
        PlayState::Frozen
    );
    shots.shoot("05-game-frozen-build");
    shots.harness.state_mut().studio.settings.show_timing = false;

    // A new game gets its first playthrough (from its bot) and its first
    // Check by themselves. Check: the friendly list, then one line's picture.
    shots.harness.get_by_label("Check").click();
    shots.harness.run_steps(2);
    shots.shoot("06-check-running");
    shots.until("the first check", Duration::from_secs(600), |app| {
        app.studio
            .open
            .as_ref()
            .is_some_and(|open| open.check.is_some() || open.check_result.is_some())
    });
    shots.idle();
    shots.shoot("07-check-list");
    let result = shots
        .harness
        .state()
        .studio
        .open
        .as_ref()
        .unwrap()
        .check_result
        .clone()
        .unwrap();
    println!(
        "the new game's check: {:?}",
        result.rows.iter().map(|row| &row.say).collect::<Vec<_>>()
    );
    assert!(
        result.passed,
        "a new game passes its first check: {result:?}"
    );
    let with_picture = result.rows.iter().position(|row| row.picture.is_some());
    assert!(
        with_picture.is_some(),
        "the playthrough's line has a picture"
    );
    shots.run(Command::CheckShowProblem { line: with_picture });
    shots.shoot("08-check-line-picture");
    shots.harness.state_mut().check_open = false;

    // Build mode: a new plank with a coin on it, and a brick wall, painted
    // on the frozen level. The game takes them and draws the same frame
    // again with them.
    shots.run(Command::LevelPaint {
        cells: vec![(3, 10), (4, 10), (5, 10)],
        tile: String::from("plank"),
        stroke: Some(1),
    });
    shots.run(Command::LevelPlaceThing {
        at: (4, 9),
        thing: String::from("coin"),
    });
    shots.run(Command::LevelPaint {
        cells: vec![(12, 12), (12, 11)],
        tile: String::from("brick"),
        stroke: Some(2),
    });
    shots.harness.state_mut().build.tile = Some('B');
    shots.play_for(Duration::from_millis(600));
    shots.shoot("16-build-painted");
    shots.harness.state_mut().build.whole_level = true;
    shots.harness.run_steps(3);
    shots.shoot("17-build-whole-level");
    shots.harness.state_mut().build.whole_level = false;

    // Check again: the recorded playthrough is from the level as it was.
    shots.harness.state_mut().check_open = true;
    shots.run(Command::CheckRun);
    shots.harness.run_steps(2);
    shots.idle();
    let result = shots
        .harness
        .state()
        .studio
        .open
        .as_ref()
        .unwrap()
        .check_result
        .clone()
        .unwrap();
    println!(
        "the check after painting: {:?}",
        result.rows.iter().map(|row| &row.say).collect::<Vec<_>>()
    );
    let older = result
        .rows
        .iter()
        .position(|row| row.action.is_some())
        .expect("a line saying the playthrough is from an older level");
    shots.run(Command::CheckShowProblem { line: Some(older) });
    shots.shoot("18-check-older-level");
    shots.harness.state_mut().check_open = false;

    // Feel: the game's values as sliders (labels and presets from its feel
    // file), its sounds, its pictures. A higher jump goes to the game at once.
    shots.until("the game's tune values", Duration::from_secs(30), |app| {
        app.studio.play().is_some_and(|play| !play.tunes.is_empty())
    });
    shots.run(Command::FeelSet {
        name: String::from("jump_speed"),
        value: 400.0,
        stroke: Some(9),
    });
    shots.shoot("19-feel");
    shots.run(Command::FeelShowCode {
        name: String::from("jump_speed"),
    });
    shots.harness.state_mut().code_open = true;
    shots.shoot("20-show-me-the-code");
    shots.harness.state_mut().code_open = false;
    let sound = shots
        .harness
        .state()
        .studio
        .open
        .as_ref()
        .unwrap()
        .sounds()
        .first()
        .map(|s| s.name.clone());
    if let Some(sound) = sound {
        shots.run(Command::AssetsShowRecipe { name: sound });
        shots.harness.state_mut().recipe_open = true;
        shots.shoot("21-sound-recipe");
        shots.harness.state_mut().recipe_open = false;
    }
    // History: snapshots, taken by themselves (the first when the game was
    // first opened, one before the Check).
    shots.run(Command::EditSnapshot {
        title: String::from("Feel: Jump height"),
    });
    shots.harness.state_mut().history_open = true;
    shots.shoot("22-history");
    shots.harness.state_mut().history_open = false;
    // Undo puts the jump back as it was.
    shots.run(Command::EditUndo);
    let jump = shots
        .harness
        .state()
        .studio
        .open
        .as_ref()
        .unwrap()
        .tunes
        .get("jump_speed")
        .copied();
    assert_eq!(jump, Some(330.0), "undone");

    // Tab again, play on, then a clip of the run. The Console shows the
    // commands behind the buttons.
    shots.harness.key_press(egui::Key::Tab);
    shots.harness.run_steps(2);
    shots.play_for(Duration::from_secs(2));
    shots.harness.state_mut().studio.settings.show_console = true;
    shots.harness.get_by_label("Clip").click();
    shots.harness.run_steps(2);
    shots.idle();
    shots.harness.run_steps(3);
    shots.shoot("09-clip-and-console");
    shots.run(Command::ConsoleRun {
        line: String::from("cargo test --no-default-features --example bee_maze"),
    });
    shots.idle();
    shots.harness.run_steps(3);
    shots.shoot("10-console-typed-command");
    shots.harness.state_mut().studio.settings.show_console = false;
    shots.harness.run_steps(2);
    shots.harness.get_by_label("Close").click(); // the Clip window
    shots.harness.run_steps(2);

    // The game is ended from outside: "The game stopped".
    let job = shots.harness.state().studio.play().unwrap().job;
    shots.harness.state_mut().studio.console.stop(job);
    shots.until(
        "the game to be seen as stopped",
        Duration::from_secs(20),
        |app| app.studio.crashed().is_some(),
    );
    shots.shoot("11-the-game-stopped");

    // A mistake in the code, as the Studio says it.
    let mistake = Problem::new(
        "There's a mistake in the code. examples/bee_maze/main.rs, line 186: cannot find value \
         `jump_sped` in this scope. The compiler suggests: a local variable with a similar name \
         exists: `jump_speed`",
        "Open the file at that line and fix it; the game builds again by itself when you save. \
         Or copy this for an agent.",
    )
    .with_details(
        "error[E0425]: cannot find value `jump_sped` in this scope\n   --> \
         examples/bee_maze/main.rs:186:42\n    |\n186 |             self.vel.y = -jump_sped;\n    \
         |                           ^^^^^^^^^ help: a local variable with a similar name \
         exists: `jump_speed`",
    );
    shots
        .harness
        .state_mut()
        .studio
        .open
        .as_mut()
        .unwrap()
        .build = Some(Build::for_show(
        "bee_maze",
        BuildState::Failed(mistake),
        0,
        0,
    ));
    shots.harness.state_mut().studio.open.as_mut().unwrap().play = None;
    shots.harness.run_steps(3);
    shots.shoot("12-build-failed");
    shots
        .harness
        .state_mut()
        .studio
        .open
        .as_mut()
        .unwrap()
        .build = Some(Build::for_show("bee_maze", BuildState::Running, 9, 14));
    shots.harness.run_steps(3);
    shots.shoot("13-building");

    // No Rust on the computer.
    shots
        .harness
        .state_mut()
        .studio
        .open
        .as_mut()
        .unwrap()
        .build = None;
    shots.harness.state_mut().studio.rust = Err(rust_is_missing("cargo: program not found"));
    shots.harness.run_steps(3);
    shots.shoot("14-rust-missing");
    shots.harness.state_mut().studio.rust = studio_core::build::find_rust();

    // Two of the examples, checked, so Home shows both kinds of card.
    for game in ["duck_peaks", "template"] {
        shots.run(Command::GameOpen {
            name: game.to_string(),
        });
        shots.run(Command::CheckRun);
        shots.idle();
    }

    // Home, now with a game of one's own on it.
    shots.run(Command::HomeOpen);
    shots.harness.run_steps(3);
    shots.shoot("01-home");

    // Duck Craft inside the Studio: 3D, and its note about saving.
    shots.run(Command::GameOpen {
        name: String::from("duck_craft"),
    });
    shots.playing();
    // Enter starts a new island from the title screen.
    shots.input(serde_json::json!({"press": "Enter"}));
    shots.play_for(Duration::from_millis(200));
    shots.input(serde_json::json!({"release": "Enter"}));
    shots.play_for(Duration::from_secs(3));
    shots.input(serde_json::json!({"mouse_motion": [40, 6]}));
    shots.input(serde_json::json!({"press": "W"}));
    shots.play_for(Duration::from_secs(2));
    shots.harness.state_mut().studio.settings.show_timing = true;
    shots.shoot("15-duck-craft-playing");
    let play = shots.harness.state().studio.play().unwrap();
    println!(
        "Duck Craft: wants the pointer: {}, notes: {:?}, drawn by: {}",
        play.wants_pointer, play.notes, play.renderer
    );
    shots.run(Command::HomeOpen);
}

/// The beginner's proof (`studio/proof/beginners-proof.json`, the script
/// the core's own test and the release workflow run), followed in the
/// app: a picture of the Studio at each of its `"shot"`s.
#[test]
fn the_beginners_proof() {
    use studio_core::script::{Step, run_step};
    let Some(folder) = std::env::var_os("STUDIO_SCREENSHOTS") else {
        println!("set STUDIO_SCREENSHOTS=<folder> to save pictures of the beginner's proof");
        return;
    };
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let folder = PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let root = duck_forge();
    let _put_back = PutBack {
        manifest: std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
        root: root.clone(),
        game: "gap_dash",
    };
    let mut shots = Shots {
        folder,
        harness: harness(StudioApp::new(studio()), true),
    };
    let script = std::fs::read_to_string(root.join("studio/proof/beginners-proof.json")).unwrap();
    let steps: Vec<Step> = serde_json::from_str(&script).unwrap();
    let mut pictures = 0;
    for (number, step) in steps.iter().enumerate() {
        match step {
            Step::Shot(shot) => {
                pictures += 1;
                // "3 frozen in mid-jump over the gap" -> proof-03-frozen-in-mid-jump-over-the-gap
                let words: String = shot
                    .shot
                    .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ')
                    .chars()
                    .map(|c| {
                        if c.is_ascii_alphanumeric() {
                            c.to_ascii_lowercase()
                        } else {
                            '-'
                        }
                    })
                    .collect();
                let words: Vec<&str> = words.split('-').filter(|word| !word.is_empty()).collect();
                shots.shoot(&format!("proof-{pictures:02}-{}", words.join("-")));
            }
            _ => {
                let app = shots.harness.state_mut();
                // The Check panel is open while Check is the subject.
                match step {
                    Step::Do(Command::CheckRun | Command::CheckRecordAgain) => {
                        app.check_open = true;
                    }
                    Step::Do(
                        Command::GameRestart | Command::GamePlay | Command::GameOpen { .. },
                    ) => {
                        app.check_open = false;
                    }
                    _ => {}
                }
                if let Err(problem) = run_step(&mut app.studio, step) {
                    panic!(
                        "step {} ({step:?}): {problem}\n{}",
                        number + 1,
                        problem.details
                    );
                }
            }
        }
    }
    assert_eq!(pictures, 14);
    for entry in std::fs::read_dir(root.join("out/clips")).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("gap_dash-") {
            let path = entry.path();
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir_all(&path);
        }
    }
}

/// Puts one file back as it was.
struct FileBack {
    path: PathBuf,
    text: String,
}

impl Drop for FileBack {
    fn drop(&mut self) {
        let _ = std::fs::write(&self.path, &self.text);
    }
}

/// The scene editor, with the mouse and the keyboard, on the real scene
/// demo: the Scene list, a click that selects, a drag that moves where an
/// object starts, Undo, a drag that moves the duck for this run only, "As
/// it starts", the Move tool, and the tile map's paint tools. With
/// STUDIO_SCREENSHOTS set it saves a picture of each.
#[test]
fn the_scene_editor_selects_and_moves_objects_with_the_mouse() {
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let pictures = std::env::var_os("STUDIO_SCREENSHOTS").map(PathBuf::from);
    if let Some(folder) = &pictures {
        std::fs::create_dir_all(folder).unwrap();
    }
    let scene_file = duck_forge().join("examples/scene_demo/scenes/pond.json");
    let _back = FileBack {
        text: std::fs::read_to_string(&scene_file).unwrap(),
        path: scene_file.clone(),
    };
    let plain = |text: &str| text.replace("\r\n", "\n");
    let original = plain(&_back.text);
    let on_disk = || plain(&std::fs::read_to_string(&scene_file).unwrap());
    let mut shots = Shots {
        folder: pictures.clone().unwrap_or_default(),
        harness: harness(StudioApp::new(studio()), pictures.is_some()),
    };
    let shoot = |shots: &mut Shots, name: &str| {
        if pictures.is_some() {
            shots.shoot(name);
        }
    };
    // Where an object (or where it starts) is on the screen.
    let middle_of = |shots: &Shots, id: u64, start: bool| -> egui::Pos2 {
        let app = shots.harness.state();
        let placement = app.game_placement().expect("the game is on screen");
        let drawn = app.studio.open.as_ref().unwrap().drawn_scene().unwrap();
        let object = drawn.object(id).unwrap();
        let rect = if start {
            object.start_rect
        } else {
            object.rect
        }
        .unwrap();
        let each = placement.scale / placement.pixels_per_point;
        placement.rect.min + egui::vec2(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0) * each
    };
    // Points on screen for each pixel of the scene.
    let points_per_pixel = |shots: &Shots| -> f32 {
        let app = shots.harness.state();
        let placement = app.game_placement().unwrap();
        let drawn = app.studio.open.as_ref().unwrap().drawn_scene().unwrap();
        placement.scale / placement.pixels_per_point * drawn.scale
    };
    let click = |shots: &mut Shots, at: egui::Pos2| {
        shots.harness.hover_at(at);
        shots.harness.step();
        shots.harness.drag_at(at);
        shots.harness.step();
        shots.harness.drop_at(at);
        shots.harness.run_steps(4);
    };
    // Press at `from`, move to `to` in a few goes, and let go.
    let drag = |shots: &mut Shots, from: egui::Pos2, to: egui::Pos2| {
        shots.harness.hover_at(from);
        shots.harness.step();
        shots.harness.drag_at(from);
        shots.harness.step();
        for part in 1..=6 {
            shots
                .harness
                .hover_at(from + (to - from) * (part as f32 / 6.0));
            shots.play_for(Duration::from_millis(40));
        }
        shots.play_for(Duration::from_millis(150));
    };
    let let_go = |shots: &mut Shots, at: egui::Pos2| {
        shots.harness.drop_at(at);
        shots.play_for(Duration::from_millis(300));
    };
    let selected = |shots: &Shots| -> Vec<u64> {
        shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .selected
            .clone()
    };

    shots.harness.step();
    shots.run(Command::GameOpen {
        name: String::from("scene_demo"),
    });
    shots.playing();
    shots.input(serde_json::json!({"press": "Right"}));
    // A short walk: the duck stops before the sign, not in front of it.
    shots.play_for(Duration::from_millis(300));
    shots.input(serde_json::json!({"release": "Right"}));
    shots.play_for(Duration::from_millis(300));
    shots.run(Command::GameFreeze);
    shots.until(
        "the frozen picture's objects",
        Duration::from_secs(20),
        |app| {
            let open = app.studio.open.as_ref().unwrap();
            open.drawn_scene().is_some() && app.game_placement().is_some()
        },
    );
    shots.harness.run_steps(4);

    // The Scene list, and nothing selected.
    shots.harness.get_by_label("Scene");
    // Beside "Scene", and in Files below it.
    assert!(shots.harness.get_all_by_label("pond.json").count() >= 2);
    shots.harness.get_by_label("big crate");
    shots.harness.get_by_label_contains("1 flag painted on it");
    shots.harness.get_by_label("Frozen moment");
    shots.harness.get_by_label("As it starts");
    shoot(&mut shots, "23-scene-list");

    // A click on the sign, in the game, selects it. The Inspector shows it.
    let sign = middle_of(&shots, 4, false);
    click(&mut shots, sign);
    assert_eq!(selected(&shots), [4]);
    assert_eq!(
        shots.harness.state().right_tab,
        studio_app::app::RightTab::Inspector
    );
    shots.harness.get_by_label_contains("line 9 of pond.json");
    shots.harness.get_by_label("Picture");
    shots.harness.get_by_label("Layer");
    shoot(&mut shots, "24-an-object-selected");

    // Dragging it moves where it starts: the file changes, on the grid.
    let each = points_per_pixel(&shots);
    let to = sign + egui::vec2(47.0 * each, 0.0);
    drag(&mut shots, sign, to);
    shoot(&mut shots, "25-dragging-an-object");
    let_go(&mut shots, to);
    assert!(
        on_disk().contains(r#"{"id": 4, "name": "sign", "at": [168, 208]"#),
        "47 pixels right lands on the grid at 168: {}",
        on_disk()
    );
    assert_eq!(selected(&shots), [4]);
    let app = shots.harness.state();
    assert_eq!(
        app.studio.open.as_ref().unwrap().undo.next_undo(),
        Some("Move sign"),
        "one drag is one step"
    );

    // Ctrl+Z puts it back.
    shots
        .harness
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    shots.play_for(Duration::from_millis(300));
    assert_eq!(on_disk(), original, "undone");

    // The duck has walked: it shows where it starts. Dragging the duck
    // itself moves it for this run only, and the file stays.
    let duck = middle_of(&shots, 9, false);
    click(&mut shots, duck);
    assert_eq!(selected(&shots), [9]);
    shots.harness.get_by_label("Starts at");
    shots.harness.get_by_label("Now");
    shots.harness.get_by_label("Start here");
    shoot(&mut shots, "26-moved-since-the-start");
    let was = middle_of(&shots, 9, false);
    let to = was + egui::vec2(40.0 * each, 0.0);
    drag(&mut shots, was, to);
    let_go(&mut shots, to);
    assert_eq!(on_disk(), original, "the file is left alone");
    let now = middle_of(&shots, 9, false);
    assert!(
        now.x > was.x + 30.0 * each,
        "the duck moved: {was:?} to {now:?}"
    );
    assert_eq!(
        shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .undo
            .next_undo(),
        Some("Move duck for this run")
    );

    // Dragging the dashed outline moves where it starts.
    let outline = middle_of(&shots, 9, true);
    let to = outline + egui::vec2(16.0 * each, 0.0);
    drag(&mut shots, outline, to);
    let_go(&mut shots, to);
    assert!(
        on_disk().contains(r#"{"id": 9, "name": "duck", "at": [56, 208]"#),
        "{}",
        on_disk()
    );
    shoot(
        &mut shots,
        "27-the-duck-moved-for-this-run-and-its-start-moved",
    );

    // As it starts: the scene as the file has it.
    shots.harness.get_by_label("As it starts").click();
    shots.until("the scene as it starts", Duration::from_secs(10), |app| {
        let open = app.studio.open.as_ref().unwrap();
        open.drawn_scene().is_some_and(|drawn| drawn.as_it_starts)
    });
    shoot(&mut shots, "28-as-it-starts");
    // What's dragged there is how the scene starts: the file.
    let crate_at = middle_of(&shots, 5, false);
    let to = crate_at + egui::vec2(16.0 * each, 0.0);
    drag(&mut shots, crate_at, to);
    let_go(&mut shots, to);
    assert!(
        on_disk().contains(r#"{"id": 5, "name": "crate", "at": [264, 208]"#),
        "{}",
        on_disk()
    );
    assert!(
        middle_of(&shots, 5, false).x > crate_at.x + 10.0 * each,
        "and the picture of the start shows it"
    );
    shots
        .harness
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    shots.play_for(Duration::from_millis(300));
    shots.harness.get_by_label("Frozen moment").click();
    shots.until("the frozen moment again", Duration::from_secs(10), |app| {
        let open = app.studio.open.as_ref().unwrap();
        open.drawn_scene().is_some_and(|drawn| !drawn.as_it_starts)
    });

    // G is the Move tool: an arrow keeps the drag to one direction.
    shots.harness.hover_at(sign + egui::vec2(0.0, -80.0));
    shots.harness.key_press(egui::Key::G);
    shots.harness.run_steps(3);
    assert_eq!(
        shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .view
            .tool,
        studio_core::scene_edit::Tool::Move
    );
    click(&mut shots, sign);
    assert_eq!(selected(&shots), [4]);
    shoot(&mut shots, "29-the-move-tool");
    let arrow = sign + egui::vec2(24.0, 0.0);
    let to = arrow + egui::vec2(16.0 * each, 30.0);
    drag(&mut shots, arrow, to);
    let_go(&mut shots, to);
    assert!(
        on_disk().contains(r#"{"id": 4, "name": "sign", "at": [136, 208]"#),
        "across only, whatever the pointer did up and down: {}",
        on_disk()
    );

    // Two selected: dragging one takes the other along, as one step.
    shots.harness.key_press(egui::Key::V);
    shots.harness.run_steps(2);
    shots.run(Command::SelectSet { ids: vec![4, 5] });
    shots.harness.run_steps(3);
    shots.harness.get_by_label("2 objects");
    let crate_at = middle_of(&shots, 5, false);
    let to = crate_at + egui::vec2(16.0 * each, 0.0);
    drag(&mut shots, crate_at, to);
    let_go(&mut shots, to);
    let text = on_disk();
    assert!(
        text.contains(r#"{"id": 4, "name": "sign", "at": [152, 208]"#)
            && text.contains(r#"{"id": 5, "name": "crate", "at": [264, 208]"#),
        "{text}"
    );
    shots
        .harness
        .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    shots.play_for(Duration::from_millis(300));
    assert!(
        on_disk().contains(r#"{"id": 5, "name": "crate", "at": [248, 208]"#)
            && on_disk().contains(r#"{"id": 4, "name": "sign", "at": [136, 208]"#),
        "one Undo puts both back: {}",
        on_disk()
    );

    // The tile map, picked in the list: the paint tools are back.
    shots.harness.get_by_label("ground").click();
    shots.harness.run_steps(4);
    shots.harness.get_by_label("Tiles");
    shoot(&mut shots, "30-the-tile-map-selected");

    // Escape lets go of everything; Tab plays on from the same frame.
    shots.harness.key_press(egui::Key::Escape);
    shots.harness.run_steps(3);
    assert!(selected(&shots).is_empty());
    let frame = shots.harness.state().studio.play().unwrap().frame;
    shots.harness.key_press(egui::Key::Tab);
    shots.until("the game to play on", Duration::from_secs(20), |app| {
        app.studio.play().is_some_and(|play| play.frame > frame + 5)
    });
    shots.run(Command::GameStop);
}

/// Step 3's tools, with the mouse and the keyboard, on the real scene
/// demo: turning on the ring, sizing by a corner, a box round several,
/// the arrow keys, Ctrl+C, Ctrl+V, Ctrl+D and Delete, the Add window,
/// dragging a row of the Scene list, its right-click menu, and the
/// Inspector for several. Each change is undone with Ctrl+Z, back to the
/// file as it was. With STUDIO_SCREENSHOTS set it saves a picture of each.
#[test]
fn the_scene_editors_other_tools_work_with_the_mouse_and_the_keys() {
    use studio_core::scene_edit::{self, Tool};

    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let pictures = std::env::var_os("STUDIO_SCREENSHOTS").map(PathBuf::from);
    if let Some(folder) = &pictures {
        std::fs::create_dir_all(folder).unwrap();
    }
    let scene_file = duck_forge().join("examples/scene_demo/scenes/pond.json");
    let _back = FileBack {
        text: std::fs::read_to_string(&scene_file).unwrap(),
        path: scene_file.clone(),
    };
    let plain = |text: &str| text.replace("\r\n", "\n");
    let original = plain(&_back.text);
    let on_disk = || plain(&std::fs::read_to_string(&scene_file).unwrap());
    let mut shots = Shots {
        folder: pictures.clone().unwrap_or_default(),
        harness: harness(StudioApp::new(studio()), pictures.is_some()),
    };
    let shoot = |shots: &mut Shots, name: &str| {
        if pictures.is_some() {
            shots.shoot(name);
        }
    };
    // A point of the game's picture, on the screen.
    let on_screen = |shots: &Shots, point: duckforge::prelude::Vec2| -> egui::Pos2 {
        let placement = shots.harness.state().game_placement().unwrap();
        let each = placement.scale / placement.pixels_per_point;
        placement.rect.min + egui::vec2(point.x, point.y) * each
    };
    let shape_of = |shots: &Shots, id: u64| {
        let open = shots.harness.state().studio.open.as_ref().unwrap();
        let drawn = open.drawn_scene().unwrap();
        scene_edit::shape(drawn.object(id).unwrap()).unwrap()
    };
    let middle_of = |shots: &Shots, id: u64| {
        let (rect, _, _) = shape_of(shots, id);
        on_screen(
            shots,
            duckforge::prelude::Vec2::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
        )
    };
    let click = |shots: &mut Shots, at: egui::Pos2| {
        shots.harness.hover_at(at);
        shots.harness.step();
        shots.harness.drag_at(at);
        shots.harness.step();
        shots.harness.drop_at(at);
        shots.harness.run_steps(4);
    };
    let drag = |shots: &mut Shots, from: egui::Pos2, to: egui::Pos2| {
        shots.harness.hover_at(from);
        shots.harness.step();
        shots.harness.drag_at(from);
        shots.harness.step();
        for part in 1..=6 {
            shots
                .harness
                .hover_at(from + (to - from) * (part as f32 / 6.0));
            shots.play_for(Duration::from_millis(40));
        }
        shots.play_for(Duration::from_millis(150));
    };
    let let_go = |shots: &mut Shots, at: egui::Pos2| {
        shots.harness.drop_at(at);
        shots.play_for(Duration::from_millis(300));
    };
    let key = |shots: &mut Shots, modifiers: egui::Modifiers, key: egui::Key| {
        shots.harness.key_press_modifiers(modifiers, key);
        shots.play_for(Duration::from_millis(250));
    };
    let undo = |shots: &mut Shots| {
        shots
            .harness
            .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        shots.play_for(Duration::from_millis(300));
    };
    let selected = |shots: &Shots| -> Vec<u64> {
        shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .selected
            .clone()
    };
    let ids_in_file = |shots: &Shots| -> Vec<u64> {
        let doc = shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .scene
            .clone()
            .unwrap();
        doc.file
            .unwrap()
            .objects
            .iter()
            .map(|o| o.id().number())
            .collect()
    };
    let tool = |shots: &Shots| {
        shots
            .harness
            .state()
            .studio
            .open
            .as_ref()
            .unwrap()
            .view
            .tool
    };

    shots.harness.step();
    shots.run(Command::GameOpen {
        name: String::from("scene_demo"),
    });
    shots.playing();
    shots.play_for(Duration::from_millis(300));
    shots.run(Command::GameFreeze);
    shots.until(
        "the frozen picture's objects",
        Duration::from_secs(20),
        |app| {
            let open = app.studio.open.as_ref().unwrap();
            open.drawn_scene().is_some() && app.game_placement().is_some()
        },
    );
    shots.harness.run_steps(4);
    let picture_scale = {
        let open = shots.harness.state().studio.open.as_ref().unwrap();
        open.drawn_scene().unwrap().scale
    };

    // R, and the sign: a ring round it. Dragged a quarter round
    // clockwise, it lands on 75 degrees (-8 + 90, to the nearest 15).
    shots.harness.hover_at(egui::pos2(640.0, 120.0));
    key(&mut shots, egui::Modifiers::NONE, egui::Key::R);
    assert_eq!(tool(&shots), Tool::Turn);
    let sign = middle_of(&shots, 4);
    click(&mut shots, sign);
    assert_eq!(selected(&shots), [4]);
    let (rect, pivot, turn) = shape_of(&shots, 4);
    let middle = on_screen(&shots, pivot);
    let radius = scene_edit::corners(rect, pivot, turn)
        .iter()
        .map(|corner| on_screen(&shots, *corner).distance(middle))
        .fold(0.0, f32::max)
        + 12.0;
    let radius = radius.max(24.0);
    shoot(&mut shots, "31-the-turn-tool");
    drag(
        &mut shots,
        middle + egui::vec2(radius, 0.0),
        middle + egui::vec2(0.0, radius),
    );
    let_go(&mut shots, middle + egui::vec2(0.0, radius));
    assert!(
        on_disk().contains(r#"{"id": 4, "name": "sign", "at": [120, 208], "turn": 75"#),
        "{}",
        on_disk()
    );
    assert_eq!(shape_of(&shots, 4).2, 75.0, "the game draws it turned");
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // S: four corners. The top-right one dragged twice as far from the
    // sign's feet makes it twice as big, standing where it stood.
    key(&mut shots, egui::Modifiers::NONE, egui::Key::S);
    assert_eq!(tool(&shots), Tool::Size);
    let (rect, pivot, turn) = shape_of(&shots, 4);
    let corner = scene_edit::corners(rect, pivot, turn)[1];
    let feet = on_screen(&shots, pivot);
    let grab = on_screen(&shots, corner);
    shoot(&mut shots, "32-the-size-tool");
    drag(&mut shots, grab, feet + (grab - feet) * 2.0);
    let_go(&mut shots, feet + (grab - feet) * 2.0);
    assert!(
        on_disk().contains(r#""turn": -8, "scale": 2, "picture": "assets/sign.png""#),
        "{}",
        on_disk()
    );
    assert_eq!(shape_of(&shots, 4).0.w, rect.w * 2.0);
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // V, and a box from empty sky round the small crate and the coin
    // above it.
    key(&mut shots, egui::Modifiers::NONE, egui::Key::V);
    let box_from = on_screen(
        &shots,
        duckforge::prelude::Vec2::new(238.0, 165.0) * picture_scale,
    );
    let box_to = on_screen(
        &shots,
        duckforge::prelude::Vec2::new(262.0, 212.0) * picture_scale,
    );
    let scroll = {
        let open = shots.harness.state().studio.open.as_ref().unwrap();
        open.drawn_scene().unwrap().offset
    };
    let shift = on_screen(&shots, scroll) - on_screen(&shots, duckforge::prelude::Vec2::ZERO);
    drag(&mut shots, box_from + shift, box_to + shift);
    shoot(&mut shots, "33-a-box-round-two");
    let_go(&mut shots, box_to + shift);
    let mut boxed = selected(&shots);
    boxed.sort();
    assert_eq!(boxed, [5, 7], "the crate and the coin");
    shots.harness.get_by_label("2 objects");
    shoot(&mut shots, "34-two-in-the-inspector");

    // Ctrl+D: copies of both, selected; Delete takes them away again.
    key(&mut shots, egui::Modifiers::COMMAND, egui::Key::D);
    assert_eq!(selected(&shots), [12, 13]);
    assert_eq!(ids_in_file(&shots).len(), 13);
    key(&mut shots, egui::Modifiers::NONE, egui::Key::Delete);
    assert_eq!(ids_in_file(&shots).len(), 11);
    undo(&mut shots);
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // The arrow keys: three pixels and a grid square to the right, one
    // step for Undo.
    click(&mut shots, sign);
    assert_eq!(selected(&shots), [4]);
    for _ in 0..3 {
        key(&mut shots, egui::Modifiers::NONE, egui::Key::ArrowRight);
    }
    key(&mut shots, egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    assert!(
        on_disk().contains(r#"{"id": 4, "name": "sign", "at": [131, 208]"#),
        "{}",
        on_disk()
    );
    undo(&mut shots);
    assert_eq!(on_disk(), original, "all four nudges were one step");

    // Ctrl+C, then Ctrl+V with the pointer over the sky: a sign there.
    key(&mut shots, egui::Modifiers::COMMAND, egui::Key::C);
    assert_eq!(shots.harness.state().studio.clipboard.len(), 1);
    let sky = on_screen(
        &shots,
        duckforge::prelude::Vec2::new(300.0, 100.0) * picture_scale,
    ) + shift;
    shots.harness.hover_at(sky);
    shots.harness.step();
    key(&mut shots, egui::Modifiers::COMMAND, egui::Key::V);
    assert_eq!(selected(&shots), [12]);
    assert!(
        on_disk().contains(r#"{"id": 12, "name": "sign", "at": [304, 104], "turn": -8"#),
        "{}",
        on_disk()
    );
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // + Add: the plain kinds. An area, in the middle of the view.
    shots.harness.get_by_label("+ Add").click();
    shots.harness.run_steps(3);
    shots.harness.get_by_label("Add an object");
    shots.harness.get_by_label("Area").click();
    shots.harness.run_steps(2);
    shoot(&mut shots, "35-add-an-object");
    shots.harness.get_by_label("Add it").click();
    shots.play_for(Duration::from_millis(300));
    assert_eq!(selected(&shots), [12]);
    assert!(on_disk().contains(r#""kind": "area""#), "{}", on_disk());
    shots.harness.get_by_label("Box");
    shoot(&mut shots, "36-an-area-added");
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // The bush dragged up the list, above the sky: first in the file.
    let bush = shots.harness.get_by_label("bush").rect().center();
    let sky_row = shots.harness.get_by_label("sky").rect();
    drag(&mut shots, bush, egui::pos2(bush.x, sky_row.top() + 2.0));
    shoot(&mut shots, "37-reordering-the-list");
    let_go(&mut shots, egui::pos2(bush.x, sky_row.top() + 2.0));
    assert_eq!(ids_in_file(&shots)[0], 10, "{:?}", ids_in_file(&shots));
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // Right-click on a row: its menu.
    shots.harness.get_by_label("crate").click_secondary();
    shots.harness.run_steps(3);
    shoot(&mut shots, "38-a-rows-menu");
    shots
        .harness
        .get_by_label("Send to the back (drawn first)")
        .click();
    shots.play_for(Duration::from_millis(300));
    assert_eq!(ids_in_file(&shots)[0], 5);
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // Tab plays on from the same frame.
    shots.harness.key_press(egui::Key::Escape);
    shots.harness.run_steps(2);
    let frame = shots.harness.state().studio.play().unwrap().frame;
    shots.harness.key_press(egui::Key::Tab);
    shots.until("the game to play on", Duration::from_secs(20), |app| {
        app.studio.play().is_some_and(|play| play.frame > frame + 5)
    });
    shots.run(Command::GameStop);
}

/// A file dropped on the window from File Explorer, the way Windows gives
/// it.
#[derive(Debug)]
struct FromExplorer(PathBuf);

impl egui::DroppedFile for FromExplorer {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// Takes a file away at the end, even if the test fails.
struct TakeAway(PathBuf);

impl Drop for TakeAway {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Step 4 with the mouse: pictures and sounds dragged from Files into the
/// scene and onto the Inspector's boxes, a picture dropped from File
/// Explorer, and Sound in Add.
#[test]
fn files_drag_into_the_scene_and_onto_the_inspectors_boxes() {
    let _one = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let pictures = std::env::var_os("STUDIO_SCREENSHOTS").map(PathBuf::from);
    if let Some(folder) = &pictures {
        std::fs::create_dir_all(folder).unwrap();
    }
    let game = duck_forge().join("examples/scene_demo");
    let scene_file = game.join("scenes/pond.json");
    let _back = FileBack {
        text: std::fs::read_to_string(&scene_file).unwrap(),
        path: scene_file.clone(),
    };
    let _flower = TakeAway(game.join("assets/flower.png"));
    let plain = |text: &str| text.replace("\r\n", "\n");
    let original = plain(&_back.text);
    let on_disk = || plain(&std::fs::read_to_string(&scene_file).unwrap());
    let mut shots = Shots {
        folder: pictures.clone().unwrap_or_default(),
        harness: harness(StudioApp::new(studio()), pictures.is_some()),
    };
    let shoot = |shots: &mut Shots, name: &str| {
        if pictures.is_some() {
            shots.shoot(name);
        }
    };
    let on_screen = |shots: &Shots, point: duckforge::prelude::Vec2| -> egui::Pos2 {
        let placement = shots.harness.state().game_placement().unwrap();
        let each = placement.scale / placement.pixels_per_point;
        placement.rect.min + egui::vec2(point.x, point.y) * each
    };
    let middle_of = |shots: &Shots, id: u64| {
        let open = shots.harness.state().studio.open.as_ref().unwrap();
        let drawn = open.drawn_scene().unwrap();
        let (rect, _, _) = studio_core::scene_edit::shape(drawn.object(id).unwrap()).unwrap();
        on_screen(
            shots,
            duckforge::prelude::Vec2::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
        )
    };
    let click = |shots: &mut Shots, at: egui::Pos2| {
        shots.harness.hover_at(at);
        shots.harness.step();
        shots.harness.drag_at(at);
        shots.harness.step();
        shots.harness.drop_at(at);
        shots.harness.run_steps(4);
    };
    let drag = |shots: &mut Shots, from: egui::Pos2, to: egui::Pos2| {
        shots.harness.hover_at(from);
        shots.harness.step();
        shots.harness.drag_at(from);
        shots.harness.step();
        for part in 1..=6 {
            shots
                .harness
                .hover_at(from + (to - from) * (part as f32 / 6.0));
            shots.play_for(Duration::from_millis(40));
        }
        shots.play_for(Duration::from_millis(150));
    };
    let let_go = |shots: &mut Shots, at: egui::Pos2| {
        shots.harness.drop_at(at);
        shots.play_for(Duration::from_millis(300));
    };
    let undo = |shots: &mut Shots| {
        shots
            .harness
            .key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        shots.play_for(Duration::from_millis(300));
    };
    let selected = |shots: &Shots| -> Vec<u64> {
        let open = shots.harness.state().studio.open.as_ref().unwrap();
        open.selected.clone()
    };
    // A line of Files (the Feel tab lists the pictures too, as links).
    let row = |shots: &Shots, name: &str| {
        shots
            .harness
            .get_by_role_and_label(egui::accesskit::Role::Button, name)
            .rect()
            .center()
    };
    // A line of Files scrolled into view first.
    let file_row = |shots: &mut Shots, name: &str| {
        shots
            .harness
            .get_by_role_and_label(egui::accesskit::Role::Button, name)
            .scroll_to_me();
        shots.harness.run_steps(3);
        row(shots, name)
    };
    // A box of the Inspector, by what it shows.
    let a_box = |shots: &Shots, showing: &str| shots.harness.get_by_value(showing).rect().center();

    shots.harness.step();
    shots.run(Command::GameOpen {
        name: String::from("scene_demo"),
    });
    shots.playing();
    shots.play_for(Duration::from_millis(300));
    shots.run(Command::GameFreeze);
    shots.until(
        "the frozen picture's objects",
        Duration::from_secs(20),
        |app| {
            let open = app.studio.open.as_ref().unwrap();
            open.drawn_scene().is_some() && app.game_placement().is_some()
        },
    );
    shots.harness.run_steps(4);
    let (scale, scroll) = {
        let open = shots.harness.state().studio.open.as_ref().unwrap();
        let drawn = open.drawn_scene().unwrap();
        (drawn.scale, drawn.offset)
    };
    // A point of the scene, on the screen.
    let in_scene = |shots: &Shots, x: f32, y: f32| {
        on_screen(shots, duckforge::prelude::Vec2::new(x, y) * scale + scroll)
    };

    // Files sits under the Scene list. crate.png dragged into the sky:
    // a new crate where it's let go, on the grid, selected.
    shots.harness.get_by_label("Files");
    let crate_row = file_row(&mut shots, "crate.png");
    let sky = in_scene(&shots, 300.0, 100.0);
    drag(&mut shots, crate_row, sky);
    let dragging = studio_app::files_panel::dragging(&shots.harness.ctx);
    assert_eq!(dragging.unwrap().path, "assets/crate.png");
    shoot(&mut shots, "39-dragging-a-picture-from-files");
    let_go(&mut shots, sky);
    assert_eq!(selected(&shots), [12]);
    assert!(
        on_disk().contains(r#"{"id": 12, "name": "crate 2", "at": [304, 104]"#)
            || on_disk().contains(r#""at": [304, 104], "picture": "assets/crate.png""#),
        "{}",
        on_disk()
    );
    shoot(&mut shots, "40-a-crate-dropped-in-the-sky");
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // The sign selected, and bush.png dragged onto its Picture box: the
    // sign is a bush now.
    let sign = middle_of(&shots, 4);
    click(&mut shots, sign);
    assert_eq!(selected(&shots), [4]);
    shots.harness.run_steps(3);
    let picture_box = a_box(&shots, "assets/sign.png");
    let bush = file_row(&mut shots, "bush.png");
    drag(&mut shots, bush, picture_box);
    shots.harness.get_by_label("Let go to use bush.png");
    shoot(&mut shots, "41-a-picture-onto-the-picture-box");
    let_go(&mut shots, picture_box);
    let sign_line = |text: &str| {
        text.lines()
            .find(|line| line.contains(r#""id": 4,"#))
            .unwrap_or_default()
            .to_string()
    };
    assert!(
        sign_line(&on_disk()).contains(r#""picture": "assets/bush.png""#),
        "{}",
        on_disk()
    );
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // music.wav dragged into the scene: a Sound object that loops, with
    // its own boxes in the Inspector.
    let music_row = file_row(&mut shots, "music.wav");
    drag(&mut shots, music_row, sky);
    let_go(&mut shots, sky);
    assert_eq!(selected(&shots), [12]);
    assert!(
        on_disk().contains(r#""kind": "sound", "sound": "assets/music.wav", "loops": true"#)
            || on_disk().contains(r#""sound": "assets/music.wav""#),
        "{}",
        on_disk()
    );
    shots.harness.run_steps(3);
    shots.harness.get_by_label("Plays over and over (music)");
    shots.harness.get_by_label("Volume");
    shoot(&mut shots, "42-a-sound-in-the-scene");

    // coin.wav onto its Sound box: that setting gets the coin.
    let sound_box = a_box(&shots, "assets/music.wav");
    let coin = file_row(&mut shots, "coin.wav");
    drag(&mut shots, coin, sound_box);
    shoot(&mut shots, "43-a-sound-onto-the-sound-box");
    let_go(&mut shots, sound_box);
    assert!(
        on_disk().contains(r#""sound": "assets/coin.wav""#),
        "{}",
        on_disk()
    );
    // A picture isn't a sound: the box says so, and nothing changes.
    let sound_box = a_box(&shots, "assets/coin.wav");
    let before = on_disk();
    let sign_png = file_row(&mut shots, "sign.png");
    drag(&mut shots, sign_png, sound_box);
    shots.harness.get_by_label("sign.png isn't a sound");
    let_go(&mut shots, sound_box);
    assert_eq!(on_disk(), before);
    undo(&mut shots);
    undo(&mut shots);
    assert_eq!(on_disk(), original);

    // A picture from File Explorer, dropped on the sky: copied into
    // assets/ first, then added where it was let go.
    let elsewhere = studio_core::test_folder::TestFolder::new("screens-explorer");
    let flower = elsewhere.join("flower.png");
    std::fs::copy(game.join("assets/sign.png"), &flower).unwrap();
    shots.harness.hover_at(sky);
    shots.harness.step();
    shots
        .harness
        .input_mut()
        .hovered_files
        .push(egui::HoveredFile {
            path: Some(flower.clone()),
            mime: String::new(),
        });
    shots.harness.run_steps(2);
    shoot(&mut shots, "44-a-picture-from-file-explorer-over-the-scene");
    shots
        .harness
        .input_mut()
        .dropped_files
        .push(std::sync::Arc::new(FromExplorer(flower.clone())));
    shots.harness.input_mut().hovered_files.clear();
    shots.play_for(Duration::from_millis(300));
    assert!(game.join("assets/flower.png").exists());
    assert!(
        on_disk().contains(r#""at": [304, 104], "picture": "assets/flower.png""#),
        "{}",
        on_disk()
    );
    shoot(&mut shots, "45-the-flower-copied-in-and-added");
    undo(&mut shots);
    assert_eq!(on_disk(), original);
    assert!(
        game.join("assets/flower.png").exists(),
        "a file copied in stays"
    );

    // + Add has Sound now, with the game's sounds to pick from.
    shots.harness.get_by_label("+ Add").click();
    shots.harness.run_steps(3);
    shots.harness.get_by_label("Sound").click();
    shots.harness.run_steps(2);
    shots.harness.get_by_label("Which sound");
    shoot(&mut shots, "46-add-a-sound");
    shots.harness.get_by_label("Cancel").click();
    shots.harness.run_steps(2);
    assert_eq!(on_disk(), original);
    shots.run(Command::GameStop);
}
