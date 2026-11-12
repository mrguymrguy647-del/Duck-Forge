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
        let more = panel.show(ui, Some(&folder), None, &mut pictures);
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
