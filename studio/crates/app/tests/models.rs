//! A game's 3D models in tabs: Duck Mold's Studio inside Duck Forge's.
//!
//! The tab is Duck Mold's own code (its `duck-mold-studio` library),
//! drawn with no window here as everything else is. These tests open a
//! model in a tab, change it there and from a command, undo, and check
//! that what is sent to the game is a file the Studio's own engine
//! reads.

use std::path::{Path, PathBuf};

use eframe::egui;
use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use studio_app::{StudioApp, theme};
use studio_core::test_folder::TestFolder;
use studio_core::{Command, Problem, Studio};

/// A box with a picture on its front: a model with an image, so that
/// what is sent to the game has one.
const CRATE: &str = "version = 0\nname = \"crate\"\n\n[palette]\nwood = \"#B07A3F\"\nred = \"#D0453A\"\n\n[[parts]]\nname = \"box\"\nshape = \"box\"\nsize = [1, 1, 1]\ncolor = \"wood\"\nimages = { front = \"textures/mark.png\" }\n";

/// A games folder of the test's own, with one game that has a model's
/// recipe in `art`.
fn games_folder(test: &TestFolder) -> PathBuf {
    let root = test.join("Duck Forge");
    for (name, text) in [
        (
            "Cargo.toml",
            "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"bee_maze\"\ntest = true\n",
        ),
        ("examples/bee_maze/main.rs", "fn main() {}\n"),
        ("examples/bee_maze/assets/duck.png", "not really a picture"),
        ("examples/bee_maze/art/crate.toml", CRATE),
    ] {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    // A 4 x 4 picture: a red mark on a see-through square.
    let mut pixels = vec![[0u8, 0, 0, 0]; 16];
    for at in [5, 6, 9, 10] {
        pixels[at] = [208, 69, 58, 255];
    }
    let png = duck_mold_core::picture::write_png(4, 4, &pixels, false);
    let textures = root.join("examples/bee_maze/art/textures");
    std::fs::create_dir_all(&textures).unwrap();
    std::fs::write(textures.join("mark.png"), png).unwrap();
    root
}

/// The Studio with that game open. (No Rust here: nothing is built.)
fn studio(test: &TestFolder) -> (Studio, PathBuf) {
    let root = games_folder(test);
    let mut settings = test.settings();
    settings.games_folder = Some(root.clone());
    let mut studio = Studio::new(settings);
    studio.rust = Err(Problem::new("No Rust in this test.", ""));
    studio.duck_mold_undo = Some(test.join("duck-mold/undo"));
    studio
        .run(Command::GameOpen {
            name: String::from("bee_maze"),
        })
        .unwrap();
    (studio, root.join("examples/bee_maze"))
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

fn open(place: &str) -> Command {
    Command::ModelOpen {
        path: place.to_string(),
    }
}

fn recipe_text(game: &Path) -> String {
    std::fs::read_to_string(game.join("art/crate.toml")).unwrap()
}

#[test]
fn a_model_opens_in_a_tab_that_is_duck_molds_studio() {
    let test = TestFolder::new("models-tab");
    let (studio, game) = studio(&test);
    let mut screen = harness(StudioApp::new(studio), false);
    screen.run_steps(2);
    // No model is open: no row of tabs.
    assert!(screen.query_by_label("Game").is_none());
    assert_eq!(screen.state().model_in_front(), None);

    screen.state_mut().do_command(open("art/crate.toml"));
    screen.run_steps(3);
    // The tabs: the game, and the model, which is in front.
    screen.get_by_label("Game");
    screen.get_by_role_and_label(Role::Button, "Model crate.toml");
    assert_eq!(
        screen.state().model_in_front().as_deref(),
        Some("art/crate.toml")
    );
    // What is under them is Duck Mold's Studio: its parts list, its
    // modes, its Model section, and Export, which reads "Send to game".
    screen.get_by_label("Parts");
    screen.get_by_label("Pose");
    screen.get_by_label("Model");
    screen.get_by_role_and_label(Role::Button, "Export");
    assert_eq!(screen.query_all_by_label("Send to game").count(), 1);
    screen.get_by_label("the game's assets (crate.glb)");
    // None of what Duck Mold's own window has for other files.
    screen.get_by_label("File").click();
    screen.run_steps(2);
    assert!(screen.query_by_label_contains("New model").is_none());
    assert!(screen.query_by_label_contains("Open...").is_none());
    screen.key_press(egui::Key::Escape);
    screen.run_steps(2);
    // Nothing of Duck Forge's own Build mode shows through.
    assert!(screen.query_by_label("Feel").is_none());

    // The game's tab, and back.
    screen.get_by_label("Game").click();
    screen.run_steps(2);
    assert_eq!(screen.state().model_in_front(), None);
    assert!(screen.query_by_label("Parts").is_none());
    screen
        .get_by_role_and_label(Role::Button, "Model crate.toml")
        .click();
    screen.run_steps(2);
    screen.get_by_label("Parts");

    // The cross closes the tab; with none left, the row goes.
    screen
        .get_by_role_and_label(Role::Button, "Close crate.toml")
        .click();
    screen.run_steps(2);
    assert!(screen.query_by_label("Game").is_none());
    assert!(screen.query_by_label("Parts").is_none());
    assert_eq!(recipe_text(&game), CRATE, "looking changed nothing");
}

#[test]
fn an_edit_in_the_tab_is_sent_to_the_game_and_undone_with_duck_molds_undo() {
    let test = TestFolder::new("models-edit");
    let (studio, game) = studio(&test);
    let mut screen = harness(StudioApp::new(studio), false);
    screen.state_mut().do_command(open("art/crate.toml"));
    screen.run_steps(3);
    let sent = game.join("assets/crate.glb");
    assert!(!sent.exists(), "opening a model sends nothing");

    // An edit as an agent writes it: made by the tab's own Duck Mold
    // Studio, so it is one of its Undo's steps.
    screen.state_mut().do_command(Command::ModelEdit {
        path: String::from("art/crate.toml"),
        edit: String::from("set box --color red"),
    });
    screen.run_steps(3);
    assert_eq!(
        recipe_text(&game),
        CRATE.replace("color = \"wood\"", "color = \"red\"")
    );
    {
        let app = screen.state_mut();
        let tab = app.models.app("art/crate.toml").unwrap();
        assert!(tab.studio().unwrap().can_undo().is_some());
    }
    // It was sent to the game by itself (the model is exported after
    // every change here), and Duck Forge's Studio says so.
    assert!(sent.is_file());
    screen.get_by_label_contains("Sent assets/crate.glb to the game (");
    // What was sent is a file the Studio's own engine reads, picture
    // and all: Duck Mold's writer never gets ahead of the game's reader.
    let model = duckforge::glb::GlbModel::load(&sent).unwrap();
    assert_eq!(model.triangles.len(), 12);
    let picture = model.picture.as_ref().expect("its image came along");
    assert!(picture.width >= 4 && picture.height >= 4);
    assert!(
        model
            .triangles
            .iter()
            .any(|triangle| triangle.on_picture.is_some())
    );
    assert!(
        model
            .triangles
            .iter()
            .any(|triangle| triangle.color == [0xD0, 0x45, 0x3A]),
        "the box is red"
    );
    // And the Studio's look at the game's assets has it as changed: a
    // game that is running is told on the next frame.
    let first = std::fs::read(&sent).unwrap();

    // Ctrl+Z in the tab is Duck Mold's Undo: the colour is back, and
    // the game is sent the model again.
    screen.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    screen.run_steps(3);
    assert_eq!(recipe_text(&game), CRATE);
    assert_ne!(std::fs::read(&sent).unwrap(), first);
    let model = duckforge::glb::GlbModel::load(&sent).unwrap();
    let red = [0xD0, 0x45, 0x3A];
    assert!(model.triangles.iter().all(|triangle| triangle.color != red));

    // An edit Duck Mold won't make is a problem in plain words, and
    // nothing changes.
    screen.state_mut().do_command(Command::ModelEdit {
        path: String::from("art/crate.toml"),
        edit: String::from("set box --color purple"),
    });
    screen.run_steps(2);
    let problem = screen.state().studio.problem.clone().expect("a problem");
    assert!(problem.what.contains("purple"), "{}", problem.what);
    assert_eq!(recipe_text(&game), CRATE);

    // The same Undo, carried on where Duck Mold's own Studio keeps it:
    // an edit made here with the tab closed can be undone from a tab
    // opened later.
    screen.state_mut().do_command(Command::ModelClose {
        path: String::from("art/crate.toml"),
    });
    screen.run_steps(2);
    screen.state_mut().do_command(Command::ModelEdit {
        path: String::from("art/crate.toml"),
        edit: String::from("set box --size 2,1,1"),
    });
    assert!(recipe_text(&game).contains("size = [2, 1, 1]"));
}

#[test]
fn a_model_file_opens_its_recipe_or_is_only_looked_at() {
    let test = TestFolder::new("models-glb");
    let (studio, game) = studio(&test);
    let mut screen = harness(StudioApp::new(studio), false);
    // Sent once, so the game has the model's file.
    screen
        .state_mut()
        .studio
        .run(Command::ModelSend {
            path: String::from("art/crate.toml"),
        })
        .unwrap();
    // A click on the file in Files opens the recipe it was made from.
    screen.state_mut().do_command(open("assets/crate.glb"));
    screen.run_steps(3);
    assert_eq!(
        screen.state().model_in_front().as_deref(),
        Some("art/crate.toml")
    );
    screen.get_by_role_and_label(Role::Button, "Model crate.toml");

    // A model with no recipe in the game opens for looking only.
    std::fs::copy(game.join("assets/crate.glb"), game.join("assets/other.glb")).unwrap();
    screen.state_mut().do_command(open("assets/other.glb"));
    screen.run_steps(3);
    assert_eq!(
        screen.state().model_in_front().as_deref(),
        Some("assets/other.glb")
    );
    screen.get_by_role_and_label(Role::Button, "Model other.glb");
    {
        let app = screen.state_mut();
        let tab = app.models.app("assets/other.glb").unwrap();
        assert!(tab.studio().unwrap().look_only());
    }
    // What isn't a model is said, and no tab opens.
    screen.state_mut().do_command(open("main.rs"));
    screen.run_steps(2);
    let problem = screen.state().studio.problem.clone().expect("a problem");
    assert_eq!(problem.what, "main.rs isn't a 3D model.");
    assert_eq!(
        screen.state().studio.open.as_ref().unwrap().models.tabs,
        ["art/crate.toml", "assets/other.glb"]
    );
}

/// Draws the tab, for a person to look at: run with
/// `STUDIO_SCREENSHOTS=<folder> cargo test -p studio_app --test models -- --ignored`.
#[test]
#[ignore = "draws pictures for a person to look at"]
fn pictures_of_the_tab() {
    let Some(folder) = std::env::var_os("STUDIO_SCREENSHOTS") else {
        return;
    };
    let folder = PathBuf::from(folder);
    std::fs::create_dir_all(&folder).unwrap();
    let test = TestFolder::new("models-pictures");
    let (studio, _game) = studio(&test);
    let mut screen = harness(StudioApp::new(studio), true);
    screen.state_mut().do_command(open("art/crate.toml"));
    screen.run_steps(4);
    let image = screen.render().unwrap();
    image.save(folder.join("model-tab.png")).unwrap();
}
