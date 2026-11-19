//! A model saved again while the game runs: `AnimatedModel` takes its
//! file again and carries on (`reload_if_changed`), which is how a
//! change made in Duck Mold shows in a running game.

use std::path::{Path, PathBuf};

mod common;
use common::TestFolder;

use duckforge::LoadError;
use duckforge::glb::AnimatedModel;
use duckforge::prelude::*;

/// A private copy of the Duck Mold fixtures, for one test.
fn fixtures(test: &str) -> (TestFolder, PathBuf) {
    let folder = TestFolder::new(test);
    let models = folder.fixture("duck_mold");
    (folder, models)
}

/// Saves `from` over `to`, as Duck Mold's Export does.
fn save_over(from: &Path, to: &Path) {
    std::fs::copy(from, to).unwrap();
}

#[test]
fn a_model_saved_again_carries_on_where_it_was() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-reload-carries-on");
    let file = models.join("hero.glb");
    save_over(&models.join("robot.glb"), &file);
    let mut hero = AnimatedModel::load(&file)?;
    assert_eq!(hero.file(), Some(file.as_path()));

    // Mid-walk, slowed down, with its head turned by the game.
    hero.play("walk");
    hero.set_speed(0.5);
    hero.turn("head", vec3(0.0, 40.0, 0.0));
    for _ in 0..20 {
        hero.update(1.0 / 60.0);
    }
    let time = hero.time();
    assert!(time > 0.1, "{time}");
    let before = hero.mesh().vertices.clone();

    // The same file saved again: nothing about it changes.
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.playing(), Some("walk"));
    assert_eq!(hero.time(), time);
    assert_eq!(hero.mesh().vertices, before);
    // One more step moves it on exactly as it would have.
    let mut twin = AnimatedModel::load(&file)?;
    twin.play("walk");
    twin.set_speed(0.5);
    twin.turn("head", vec3(0.0, 40.0, 0.0));
    for _ in 0..21 {
        twin.update(1.0 / 60.0);
    }
    hero.update(1.0 / 60.0);
    assert_eq!(hero.time(), twin.time());
    assert_eq!(hero.mesh().vertices, twin.mesh().vertices);

    // Another file isn't its business.
    assert!(!hero.reload_if_changed(models.join("robot.glb")));
    assert!(!hero.reload_if_changed("assets/other.glb"));
    // Named with back slashes or a leading "./", it is the same file.
    let named = file.to_string_lossy().replace('/', "\\");
    assert!(hero.reload_if_changed(named) || !cfg!(windows));
    Ok(())
}

#[test]
fn a_new_model_in_the_same_file_is_taken() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-reload-new-model");
    let file = models.join("hero.glb");
    save_over(&models.join("robot.glb"), &file);
    let mut hero = AnimatedModel::load(&file)?;
    hero.play("walk");
    hero.turn("head", vec3(0.0, 40.0, 0.0));
    hero.update(0.25);
    let robot_triangles = hero.mesh().triangles.len();

    // The gnome saved over it: it walks too, so the walk carries on at
    // the same moment; it has a head, so the turn is kept.
    save_over(&models.join("gnome.glb"), &file);
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.model().name, "gnome");
    assert_ne!(hero.mesh().triangles.len(), robot_triangles);
    assert_eq!(hero.playing(), Some("walk"));
    assert_eq!(hero.time(), 0.25);
    // It stands exactly as a gnome that was walking that long with its
    // head turned (and not as one with its head straight).
    let mut gnome = AnimatedModel::load(models.join("gnome.glb"))?;
    gnome.set_blend(0.0);
    gnome.play("walk");
    gnome.update(0.25);
    assert_ne!(
        hero.mesh().vertices,
        gnome.mesh().vertices,
        "the head is still turned"
    );
    gnome.turn("head", vec3(0.0, 40.0, 0.0));
    assert_eq!(hero.mesh().vertices, gnome.mesh().vertices);

    // A model with no animations: what was playing is gone, so it
    // stands at rest, and a join it doesn't have is forgotten.
    save_over(&models.join("crate.glb"), &file);
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.model().name, "crate");
    assert_eq!(hero.playing(), None);
    assert_eq!(hero.mesh().triangles.len(), 12);
    let at_rest = AnimatedModel::load(models.join("crate.glb"))?;
    assert_eq!(hero.mesh().vertices, at_rest.mesh().vertices);
    hero.update(0.1);
    Ok(())
}

#[test]
fn a_pose_is_kept_by_name() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-reload-pose");
    let file = models.join("hero.glb");
    save_over(&models.join("robot.glb"), &file);
    let mut hero = AnimatedModel::load(&file)?;
    hero.set_blend(0.0);
    hero.set_pose("wave");
    hero.update(0.1);
    let waving = hero.mesh().vertices.clone();
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.mesh().vertices, waving);
    // The gnome has no pose called "wave"... or has it? Whichever it
    // is, nothing crashes and the model is the gnome.
    save_over(&models.join("gnome.glb"), &file);
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.model().name, "gnome");
    Ok(())
}

#[test]
fn a_file_that_cant_be_read_leaves_the_model_as_it_was() -> Result<(), LoadError> {
    let (_folder, models) = fixtures("duck-mold-reload-broken");
    let file = models.join("hero.glb");
    save_over(&models.join("robot.glb"), &file);
    let mut hero = AnimatedModel::load(&file)?;
    hero.play("walk");
    hero.update(0.25);
    let before = hero.mesh().vertices.clone();

    // Not a model any more: the old one stays, and the game needn't be
    // started again (the reason is on the terminal).
    std::fs::write(&file, b"not a model").unwrap();
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.model().name, "robot");
    assert_eq!(hero.mesh().vertices, before);
    assert_eq!(hero.playing(), Some("walk"));
    // Gone altogether: the same.
    std::fs::remove_file(&file).unwrap();
    assert!(hero.reload_if_changed(&file));
    assert_eq!(hero.model().name, "robot");

    // A model that was never loaded from a file has none to take again.
    let mut made = AnimatedModel::new(hero.model().clone());
    assert_eq!(made.file(), None);
    assert!(!made.reload_if_changed(&file));
    Ok(())
}

/// A game with a robot, for the test below: it counts what `reload`
/// was asked, and passes it to the robot.
struct RobotGame {
    robot: AnimatedModel,
    asked: Vec<String>,
}

impl Game for RobotGame {
    fn update(&mut self, ctx: &mut Context) {
        self.robot.play("walk");
        self.robot.update(ctx.dt());
    }

    fn draw(&self, _canvas: &mut Canvas) {}

    fn reload(&mut self, _ctx: &mut Context, file: &Path) -> bool {
        self.asked.push(file.to_string_lossy().replace('\\', "/"));
        self.robot.reload_if_changed(file)
    }
}

#[test]
fn the_games_reload_hook_passes_a_saved_model_on() -> Result<(), LoadError> {
    use duckforge::simulation::Simulation;

    let (_folder, models) = fixtures("duck-mold-reload-in-a-game");
    let file = models.join("hero.glb");
    save_over(&models.join("robot.glb"), &file);
    let game = RobotGame {
        robot: AnimatedModel::load(&file)?,
        asked: Vec::new(),
    };
    let mut sim = Simulation::new(Config::default(), game);
    for _ in 0..10 {
        sim.step();
    }
    let time = sim.game().robot.time();
    assert!(time > 0.0);

    // What Duck Forge Studio, or the game's own window, does when the
    // file is saved again: tell the game which file.
    save_over(&models.join("gnome.glb"), &file);
    let name = file.to_string_lossy().replace('\\', "/");
    assert!(sim.change_file(&name, ""));
    assert_eq!(sim.game().asked, std::slice::from_ref(&name));
    assert_eq!(sim.game().robot.model().name, "gnome");
    assert_eq!(sim.game().robot.playing(), Some("walk"));
    assert_eq!(sim.game().robot.time(), time, "the walk didn't start again");
    sim.step();
    assert!(sim.game().robot.time() > time);

    // A file that isn't the robot's: the game says it didn't take it.
    assert!(!sim.change_file("assets/other.glb", ""));
    Ok(())
}
