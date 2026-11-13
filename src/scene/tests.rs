//! Tests of scenes being played: finding, drawing, touching, moving, and
//! taking a changed file in mid-play.

use std::path::Path;

use super::*;
use crate::test_folder::{TestFolder, read_shared};

const RED: Color = Color::rgb(230, 30, 30);
const BLUE: Color = Color::rgb(30, 60, 230);
const YELLOW: Color = Color::rgb(240, 220, 40);
const GREEN: Color = Color::rgb(30, 200, 60);

fn png(path: &Path, width: usize, height: usize, colour: Color) {
    let mut canvas = Canvas::new(width, height);
    canvas.clear(colour);
    canvas.save_png(path).unwrap();
}

/// A game's folder with a few pictures, the platformer starter's duck and
/// tiles, and a small level.
fn game(name: &str) -> TestFolder {
    let folder = TestFolder::new(name);
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::create_dir_all(folder.join("scenes")).unwrap();
    png(&folder.join("assets/red.png"), 8, 8, RED);
    png(&folder.join("assets/blue.png"), 8, 8, BLUE);
    png(&folder.join("assets/wide.png"), 4, 2, YELLOW);
    for file in ["tiles.json", "tiles.png", "hero.json", "hero.png"] {
        let from = Path::new("examples/template/assets").join(file);
        std::fs::write(folder.join("assets").join(file), read_shared(&from)).unwrap();
    }
    folder.file("level.txt", "........\n..c..c..\np......g\n########\n");
    folder.file(
        "level.legend.json",
        r##"{"tile_size": 16, "tiles": "assets/tiles.json", "letters": {
            "#": {"kind": "solid", "name": "grass"},
            "c": {"kind": "thing", "name": "coin"},
            "p": {"kind": "thing", "name": "player", "start": true},
            "g": {"kind": "thing", "name": "flag"}}}"##,
    );
    folder
}

fn scene_of(folder: &TestFolder, objects: &str) -> Scene {
    let text = format!(
        "{{\"duckforge_scene\": 1, \"background\": \"#102030\", \"objects\": [{objects}]}}"
    );
    Scene::from_text(&text, folder).unwrap()
}

fn drawn(scene: &Scene) -> Canvas {
    let mut canvas = Canvas::new(128, 64);
    scene.draw(&mut canvas);
    canvas
}

fn numbers(ids: &[Id]) -> Vec<u64> {
    ids.iter().map(|id| id.number()).collect()
}

#[test]
fn objects_are_found_by_name_kind_and_id() {
    let folder = game("scene-find");
    let mut scene = scene_of(
        &folder,
        r#"{"id": 5, "name": "box", "at": [20, 40], "picture": "assets/red.png"},
           {"id": 2, "name": "box", "at": [60, 40], "picture": "assets/blue.png", "scale": 2, "anchor": "top_left"},
           {"id": 9, "name": "gem", "kind": "coin", "at": [100, 40], "picture": "assets/red.png", "box": [2, 2, 4, 4], "flip": true, "worth": 5}"#,
    );
    assert!(scene.problems().is_empty(), "{:?}", scene.problems());
    assert_eq!(scene.find("box"), Some(Id(2)), "the lowest id of that name");
    assert_eq!(numbers(&scene.all("box")), [2, 5]);
    assert_eq!(numbers(&scene.of_kind("coin")), [9]);
    assert_eq!(numbers(&scene.ids()), [2, 5, 9], "by id");
    assert_eq!(
        numbers(&scene.ids_as_drawn()),
        [5, 2, 9],
        "as the file has them"
    );
    assert_eq!(scene.find("nobody"), None);
    assert_eq!(scene.len(), 3);

    // Where they are: a picture stands on its feet unless it says otherwise.
    assert_eq!(scene.rect(Id(5)), Rect::new(16.0, 32.0, 8.0, 8.0));
    assert_eq!(scene.rect(Id(2)), Rect::new(60.0, 40.0, 16.0, 16.0));
    assert_eq!(scene.picture_rect(Id(9)), Rect::new(96.0, 32.0, 8.0, 8.0));
    assert_eq!(
        scene.rect(Id(9)),
        Rect::new(98.0, 34.0, 4.0, 4.0),
        "its box"
    );
    assert_eq!(scene[Id(9)].number("worth"), Some(5.0));

    // Touching: by name or by kind, and only what overlaps.
    assert!(scene.touching(Id(9), "box").is_empty());
    scene[Id(5)].at = vec2(100.0, 40.0);
    assert_eq!(numbers(&scene.touching(Id(9), "box")), [5]);
    assert_eq!(numbers(&scene.touching(Id(5), "coin")), [9]);
    assert!(scene.overlaps(Id(5), Id(9)) && !scene.overlaps(Id(2), Id(9)));
    // Under a point: the one drawn on top first.
    assert_eq!(numbers(&scene.at_point(vec2(100.0, 36.0))), [9, 5]);
    assert!(scene.at_point(vec2(2.0, 2.0)).is_empty());

    // Removing one gives it back; asking for it afterwards says so.
    let gem = scene.remove(Id(9)).unwrap();
    assert_eq!(gem.name, "gem");
    assert!(!scene.has(Id(9)) && scene.get(Id(9)).is_none());
    assert!(scene.remove(Id(9)).is_none());
    assert_eq!(
        scene.starts_as(Id(9)).unwrap().at,
        vec2(100.0, 40.0),
        "the file still has it"
    );
}

#[test]
#[should_panic(expected = "the scene has no object with the id 44")]
fn asking_for_an_object_that_isnt_there_says_so() {
    let folder = game("scene-gone");
    let scene = scene_of(&folder, r#"{"id": 1, "name": "box"}"#);
    let _ = &scene[Id(44)];
}

#[test]
fn objects_are_drawn_in_layers_and_in_the_files_order() {
    let folder = game("scene-draw");
    let both = |first: &str, second: &str| {
        format!(
            r#"{{"id": 1, "name": "red", "at": [20, 40], "picture": "assets/red.png"{first}}},
               {{"id": 2, "name": "blue", "at": [24, 40], "picture": "assets/blue.png"{second}}}"#
        )
    };
    let scene = scene_of(&folder, &both("", ""));
    let canvas = drawn(&scene);
    assert_eq!(
        canvas.get_pixel(1, 1),
        Some(Color::from_hex(0x102030)),
        "the background"
    );
    assert_eq!(canvas.get_pixel(17, 34), Some(RED));
    assert_eq!(
        canvas.get_pixel(22, 34),
        Some(BLUE),
        "the later one is on top"
    );
    assert_eq!(canvas.get_pixel(26, 34), Some(BLUE));

    // On a layer drawn later, the red one is on top whatever the order.
    let scene = scene_of(&folder, &both(r#", "layer": "front""#, ""));
    assert_eq!(drawn(&scene).get_pixel(22, 34), Some(RED));
    assert_eq!(numbers(&scene.ids_as_drawn()), [2, 1]);

    // Hidden isn't drawn, but is still there.
    let scene = scene_of(&folder, &both("", r#", "shown": false"#));
    assert_eq!(
        drawn(&scene).get_pixel(26, 34),
        Some(Color::from_hex(0x102030))
    );
    assert_eq!(numbers(&scene.touching(Id(1), "blue")), [2]);

    // The camera moves the world, and not a layer that stays on screen.
    let mut scene = scene_of(&folder, &both("", r#", "layer": "hud""#));
    scene.camera.position = vec2(64.0 + 10.0, 32.0);
    let canvas = drawn(&scene);
    assert_eq!(canvas.get_pixel(7, 34), Some(RED), "ten pixels to the left");
    assert_eq!(canvas.get_pixel(26, 34), Some(BLUE), "where it was");

    // One layer at a time, for a game that draws its own things between.
    let mut canvas = Canvas::new(128, 64);
    canvas.clear(Color::BLACK);
    scene.draw_layer(&mut canvas, "hud");
    assert_eq!(canvas.get_pixel(26, 34), Some(BLUE));
    assert_eq!(canvas.get_pixel(7, 34), Some(Color::BLACK));

    // What was drawn is named, for tools that can't see the picture.
    let mut canvas = drawn(&scene);
    let (labels, _) = canvas.take_scene();
    let names: Vec<&str> = labels.iter().map(|label| label.name.as_str()).collect();
    assert_eq!(names, ["red", "blue"]);
}

#[test]
fn pictures_can_be_turned_and_stretched() {
    let folder = game("scene-turn");
    let background = Some(Color::from_hex(0x102030));
    // 4 x 2, twice as big, on its middle: 8 wide and 4 tall...
    let flat = scene_of(
        &folder,
        r#"{"id": 1, "name": "plank", "at": [64, 32], "picture": "assets/wide.png", "scale": 2, "anchor": "middle"}"#,
    );
    let canvas = drawn(&flat);
    assert_eq!(canvas.get_pixel(60, 31), Some(YELLOW));
    assert_eq!(canvas.get_pixel(63, 28), background);
    // ...and turned a quarter round, 4 wide and 8 tall.
    let turned = scene_of(
        &folder,
        r#"{"id": 1, "name": "plank", "at": [64, 32], "picture": "assets/wide.png", "scale": 2, "anchor": "middle", "turn": 90}"#,
    );
    let canvas = drawn(&turned);
    assert_eq!(canvas.get_pixel(60, 31), background);
    assert_eq!(canvas.get_pixel(63, 28), Some(YELLOW));
    assert_eq!(canvas.get_pixel(63, 35), Some(YELLOW));
    assert_eq!(canvas.get_pixel(63, 36), background);
    let yellow = canvas
        .pixels()
        .iter()
        .filter(|&&p| p == YELLOW.to_u32())
        .count();
    assert_eq!(yellow, 32, "the same 32 pixels, turned");

    // Stretched one way only: three times as wide, as tall as it was.
    let stretched = scene_of(
        &folder,
        r#"{"id": 1, "name": "bar", "at": [0, 0], "picture": "assets/red.png", "scale": [3, 1], "anchor": "top_left"}"#,
    );
    let canvas = drawn(&stretched);
    assert_eq!(canvas.get_pixel(23, 7), Some(RED));
    assert_eq!(canvas.get_pixel(24, 7), background);
    assert_eq!(canvas.get_pixel(23, 8), background);
    assert_eq!(stretched.rect(Id(1)), Rect::new(0.0, 0.0, 24.0, 8.0));
}

#[test]
fn a_tile_map_is_one_object_and_its_painted_things_are_objects_too() {
    let folder = game("scene-map");
    let mut scene = scene_of(
        &folder,
        r#"{"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"},
           {"id": 2, "name": "duck", "at": [24, 20], "picture": "assets/hero.json"},
           {"id": 3, "name": "crate", "at": [80, 48], "picture": "assets/red.png", "solid": true}"#,
    );
    assert!(scene.problems().is_empty(), "{:?}", scene.problems());
    assert_eq!(scene.tilemap().unwrap().columns(), 8);
    assert_eq!(scene.size(), Some(vec2(128.0, 64.0)), "the tile map's");

    // The level's letters for things are objects, named after the thing.
    let coins = scene.of_kind("coin");
    assert_eq!(coins.len(), 2);
    assert!(coins.iter().all(|id| id.is_painted()));
    let coin = &scene[coins[0]];
    assert_eq!(coin.at, vec2(40.0, 32.0), "the bottom middle of its square");
    assert_eq!(
        coin.picture.as_deref(),
        Some("assets/tiles.json"),
        "the tiles' sheet has a coin"
    );
    assert_eq!(coin.animation.as_deref(), Some("coin"));
    let start = scene.find("player").unwrap();
    assert_eq!(
        scene.rect(start),
        Rect::new(0.0, 32.0, 16.0, 16.0),
        "no picture: its square"
    );
    assert_eq!(
        scene.find("flag").map(|flag| scene[flag].at),
        Some(vec2(120.0, 48.0))
    );
    // By id: the file's objects first. Drawn: right after their tile map.
    assert_eq!(numbers(&scene.ids())[..3], [1, 2, 3]);
    let drawn_order = scene.ids_as_drawn();
    assert_eq!(drawn_order[0], Id(1));
    assert!(drawn_order[1..5].iter().all(|id| id.is_painted()));
    assert_eq!(numbers(&drawn_order[5..]), [2, 3]);
    // The same squares get the same ids every time.
    let again = scene_of(
        &folder,
        r#"{"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"}"#,
    );
    assert_eq!(again.of_kind("coin"), coins);

    // The duck falls onto the grass, and walks into the crate.
    let duck = Id(2);
    let touched = scene.move_and_slide(duck, vec2(0.0, 100.0));
    assert!(touched.ground && !touched.wall_right);
    assert_eq!(scene[duck].at, vec2(24.0, 48.0));
    let touched = scene.move_and_slide(duck, vec2(100.0, 0.0));
    assert!(touched.wall_right);
    assert_eq!(
        scene[duck].at.x, 68.0,
        "its 16 pixels end where the crate starts"
    );
    assert_eq!(numbers(&scene.touching(duck, "coin")), Vec::<u64>::new());

    // The tiles are drawn: grass along the bottom.
    let canvas = drawn(&scene);
    assert_ne!(canvas.get_pixel(4, 60), Some(Color::from_hex(0x102030)));
}

#[test]
fn a_changed_file_is_taken_in_mid_play() {
    let folder = game("scene-reload");
    let text = |background: &str, objects: &str| {
        format!(
            "{{\"duckforge_scene\": 1, \"background\": \"{background}\", \"objects\": [{objects}]}}"
        )
    };
    let mut scene = Scene::from_text(
        &text(
            "#102030",
            r#"{"id": 1, "name": "coin", "at": [10, 10], "picture": "assets/red.png"},
               {"id": 2, "name": "duck", "at": [20, 40], "picture": "assets/blue.png", "lives": 3},
               {"id": 3, "name": "lamp", "at": [50, 10], "picture": "assets/red.png"}"#,
        ),
        &folder,
    )
    .unwrap();
    // The game plays: the duck walks, loses lives, and takes the lamp.
    scene[Id(2)].at.x = 60.0;
    scene[Id(2)].set_number("lives", 1.0);
    scene.remove(Id(3));
    let spark = scene.add(Object::new("spark").at(vec2(5.0, 5.0)));

    // The file changes: everything moves, the duck gets more lives and a
    // speed, and a sign is added.
    scene.take_text(&text(
        "#203040",
        r##"{"id": 1, "name": "coin", "at": [12, 10], "picture": "assets/red.png", "tint": "#ff0000"},
           {"id": 4, "name": "sign", "at": [90, 40], "picture": "assets/blue.png"},
           {"id": 2, "name": "duck", "at": [30, 40], "picture": "assets/blue.png", "lives": 5, "speed": 9},
           {"id": 3, "name": "lamp", "at": [55, 10], "picture": "assets/red.png"}"##,
    ));
    assert!(scene.problems().is_empty(), "{:?}", scene.problems());
    // What the game hadn't touched takes the change.
    assert_eq!(scene[Id(1)].at, vec2(12.0, 10.0));
    assert_eq!(scene[Id(1)].tint, Color::from_hex(0xff0000));
    assert_eq!(scene.background, Some(Color::from_hex(0x203040)));
    // What the game had changed keeps what it has now; the file's new
    // value is how it starts.
    assert_eq!(
        scene[Id(2)].at,
        vec2(60.0, 40.0),
        "the duck stays where it walked to"
    );
    assert_eq!(scene[Id(2)].number("lives"), Some(1.0));
    assert_eq!(scene.starts_as(Id(2)).unwrap().at, vec2(30.0, 40.0));
    assert_eq!(scene.starts_as(Id(2)).unwrap().number("lives"), Some(5.0));
    assert_eq!(
        scene[Id(2)].number("speed"),
        Some(9.0),
        "a new key is taken"
    );
    // New objects appear; what the game removed stays gone; what the game
    // made stays.
    assert!(scene.has(Id(4)));
    assert!(!scene.has(Id(3)) && scene.starts_as(Id(3)).is_some());
    assert!(scene.has(spark) && !spark.is_from_the_file());
    assert_eq!(scene.starts_as(spark), None);
    // Drawn in the file's new order, then what the game made; listed by id
    // as before.
    assert_eq!(scene.ids_as_drawn(), [Id(1), Id(4), Id(2), spark]);
    assert_eq!(scene.ids(), [Id(1), Id(2), Id(4), spark]);

    // Taken out of the file, an object goes. A background the game set
    // itself is kept.
    scene.background = Some(Color::BLACK);
    scene.take_text(&text(
        "#ffffff",
        r#"{"id": 2, "name": "duck", "at": [30, 40], "picture": "assets/blue.png", "lives": 5}"#,
    ));
    assert_eq!(scene.ids(), [Id(2), spark]);
    assert_eq!(scene[Id(2)].number("speed"), None, "a key taken out goes");
    assert_eq!(scene.background, Some(Color::BLACK));

    // A file with a mistake changes nothing, and says what's wrong...
    scene.take_text("{\n  \"objects\": [\n    {\"name\": \"duck\",}\n  ]\n}");
    assert_eq!(scene.ids(), [Id(2), spark]);
    assert_eq!(scene.problems().len(), 1);
    assert!(
        scene.problems()[0].contains("line 3: this isn't JSON a scene can be read from")
            && scene.problems()[0].ends_with("(the scene is kept as it was)"),
        "{:?}",
        scene.problems()
    );
    // ...until it's put right.
    scene.take_text(&text("#ffffff", r#"{"id": 2, "name": "duck"}"#));
    assert!(scene.problems().is_empty(), "{:?}", scene.problems());
}

#[test]
fn the_scenes_files_are_read_again_when_they_change() {
    let folder = game("scene-files");
    let path = folder.file(
        "scenes/level1.json",
        r#"{"duckforge_scene": 1, "objects": [
            {"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"},
            {"id": 2, "name": "box", "at": [20, 40], "picture": "assets/red.png"}]}"#,
    );
    let mut scene = Scene::load(&path).unwrap();
    assert!(scene.problems().is_empty(), "{:?}", scene.problems());
    assert_eq!(scene.folder(), &*folder, "the folder above scenes/");
    assert_eq!(scene.file(), crate::files::name_of(&path));
    let level = folder.join("level.txt");
    assert!(scene.reads_from(&path) && scene.reads_from(&level));
    assert!(scene.reads_from(&folder.join("level.legend.json")));
    assert!(scene.reads_from(&folder.join("assets/red.png")));
    assert!(
        scene.reads_from(&folder.join("assets/tiles.png")),
        "the tiles' picture"
    );
    assert!(!scene.reads_from(&folder.join("assets/blue.png")));
    assert!(!scene.reload_file(&folder.join("other.txt")));

    // The scene file, saved with the box somewhere else.
    std::fs::write(
        &path,
        r#"{"duckforge_scene": 1, "objects": [
            {"id": 1, "name": "ground", "kind": "tilemap", "level": "level.txt"},
            {"id": 2, "name": "box", "at": [70, 40], "picture": "assets/red.png"}]}"#,
    )
    .unwrap();
    assert!(scene.reload_file(&path));
    assert_eq!(scene[Id(2)].at, vec2(70.0, 40.0));

    // The level, painted: the game took the first coin; the second is
    // painted out and a third painted in.
    let coins = scene.of_kind("coin");
    scene.remove(coins[0]);
    std::fs::write(&level, "c.......\n..c.....\np......g\n########\n").unwrap();
    assert!(scene.reload_file(&level));
    let now = scene.of_kind("coin");
    assert_eq!(
        now.len(),
        1,
        "the taken one stays gone, the erased one goes"
    );
    assert_eq!(scene[now[0]].at, vec2(8.0, 16.0), "the new one");
    assert!(!scene.has(coins[0]) && !scene.has(coins[1]));
    assert_eq!(scene.tilemap().unwrap().spawns("coin").len(), 2);

    // A picture, saved again in another colour.
    assert_eq!(drawn(&scene).get_pixel(68, 34), Some(RED));
    png(&folder.join("assets/red.png"), 8, 8, GREEN);
    assert!(scene.reload_file(&folder.join("assets/red.png")));
    assert_eq!(drawn(&scene).get_pixel(68, 34), Some(GREEN));
    crate::files::forget_given();
}

#[test]
fn what_cant_be_loaded_is_said_and_drawn_round() {
    let folder = game("scene-problems");
    let scene = scene_of(
        &folder,
        r#"{"id": 1, "name": "ghost", "at": [40, 40], "picture": "assets/nope.png"},
           {"id": 2, "name": "duck", "at": [80, 40], "picture": "assets/hero.json", "animation": "swim"},
           {"id": 3, "name": "ground", "kind": "tilemap", "level": "nope.txt"},
           {"id": 4, "name": "sea", "kind": "tilemap"}"#,
    );
    let said = scene.problems().join("\n");
    assert!(said.contains("\"ghost\" has the picture \"assets/nope.png\", which couldn't be loaded, so it's drawn as a marked box"), "{said}");
    assert!(said.contains("\"duck\" asks for the animation \"swim\", which assets/hero.json doesn't have (it has fall, idle, jump, run); it shows \"idle\""), "{said}");
    assert!(
        said.contains("the tile map \"ground\" couldn't read its level \"nope.txt\""),
        "{said}"
    );
    assert!(
        said.contains("the tile map \"sea\" needs \"level\""),
        "{said}"
    );
    assert_eq!(scene.problems().len(), 4, "{said}");
    // The ghost is a marked box where it would stand; the duck shows its
    // first animation; nothing stops the scene being drawn.
    let canvas = drawn(&scene);
    assert_eq!(canvas.get_pixel(32, 24), Some(Color::rgb(255, 0, 255)));
    assert_eq!(scene.rect(Id(1)), Rect::new(32.0, 24.0, 16.0, 16.0));
    assert_ne!(canvas.get_pixel(80, 34), Some(Color::from_hex(0x102030)));
    assert!(scene.tilemap().is_none());
}

#[test]
fn a_scene_file_can_say_its_layers_and_keep_what_the_engine_doesnt_know() {
    let folder = game("scene-layers");
    let scene = Scene::from_text(
        r#"{"duckforge_scene": 1, "next": "level2", "layers": ["below", "above"], "objects": [
            {"id": 1, "name": "a", "layer": "above", "at": [20, 40], "picture": "assets/red.png"},
            {"id": 2, "name": "b", "layer": "below", "at": [24, 40], "picture": "assets/blue.png"},
            {"id": 3, "name": "c", "layer": "nowhere", "at": [24, 20], "picture": "assets/blue.png"}]}"#,
        &folder,
    )
    .unwrap();
    assert_eq!(scene.next(), Some("level2"));
    assert_eq!(scene.layers().len(), 2);
    assert_eq!(
        drawn(&scene).get_pixel(22, 34),
        Some(RED),
        "the layer above is on top"
    );
    assert_eq!(scene.problems().len(), 1);
    assert!(scene.problems()[0].contains("\"c\" is on the layer \"nowhere\""));
    // It's still drawn, on the last layer (this scene has no "world").
    assert_eq!(drawn(&scene).get_pixel(22, 14), Some(BLUE));
}

#[test]
fn what_cant_be_loaded_is_said_with_its_objects_line() {
    let folder = game("scene-problem-lines");
    let path = folder.file(
        "scenes/level1.json",
        "{\n  \"duckforge_scene\": 1,\n  \"objects\": [\n    {\"id\": 1, \"name\": \"box\", \"picture\": \"assets/red.png\"},\n    {\"id\": 2, \"name\": \"ghost\", \"picture\": \"assets/nope.png\"},\n    {\"id\": 3, \"name\": \"duck\", \"picture\": \"assets/hero.json\", \"animation\": \"swim\"},\n    {\"id\": 4, \"name\": \"ground\", \"kind\": \"tilemap\", \"level\": \"nope.txt\"}\n  ]\n}\n",
    );
    let scene = Scene::load(&path).unwrap();
    let file = scene.file().to_string();
    let said = scene.problems().join("\n");
    assert!(
        said.contains(&format!(
            "{file}, line 5: \"ghost\" has the picture \"assets/nope.png\""
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "{file}, line 6: \"duck\" asks for the animation \"swim\""
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "{file}, line 7: the tile map \"ground\" couldn't read its level"
        )),
        "{said}"
    );
    assert_eq!(scene.starts().line_of(Id(2)), Some(5));
    assert_eq!(scene.starts().line_of(Id(9)), None);
    crate::files::forget_given();
}

/// A game that is one scene, to play scenes through the engine.
struct OneScene {
    scene: Scene,
}

impl crate::engine::Game for OneScene {
    fn update(&mut self, ctx: &mut Context) {
        self.scene.update(ctx);
        // The duck walks right, a pixel an update.
        if let Some(duck) = self.scene.find("duck") {
            self.scene[duck].at.x += 1.0;
        }
        // A coin the duck is on is taken.
        if let Some(duck) = self.scene.find("duck") {
            for coin in self.scene.touching(duck, "coin") {
                self.scene.remove(coin);
            }
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        self.scene.draw(canvas);
    }

    fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool {
        self.scene.reload(ctx, file)
    }
}

fn small_screen() -> crate::engine::Config {
    crate::engine::Config {
        width: 128,
        height: 64,
        ..crate::engine::Config::default()
    }
}

fn one_scene(folder: &TestFolder) -> (std::path::PathBuf, OneScene) {
    let path = folder.file(
        "scenes/level1.json",
        r##"{"duckforge_scene": 1, "background": "#102030", "objects": [
            {"id": 1, "name": "duck", "at": [20, 40], "picture": "assets/blue.png"},
            {"id": 2, "name": "coin", "at": [34, 40], "picture": "assets/red.png"},
            {"id": 3, "name": "lamp", "at": [100, 20], "picture": "assets/red.png"},
            {"id": 4, "name": "score", "at": [2, 2], "anchor": "top_left", "picture": "assets/red.png", "layer": "hud"}]}"##,
    );
    let scene = Scene::load(&path).unwrap();
    (path, OneScene { scene })
}

#[test]
fn an_object_moved_for_this_run_only_keeps_its_start_and_replays_exactly() {
    use crate::simulation::Simulation;
    let folder = game("scene-move-now");
    let (path, game) = one_scene(&folder);
    let file = crate::files::name_of(&path);
    let mut sim = Simulation::new(small_screen(), game);
    sim.run(5);
    assert_eq!(sim.game().scene[Id(1)].at, vec2(25.0, 40.0));

    // Dragged "for this run only": the lamp moves now; the file's lamp,
    // and so where it starts, stays.
    let set: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(r#"{"at": [60, 30], "flip": true}"#).unwrap();
    assert!(sim.change_object(&file, 3, &set), "the scene took it");
    let scene = &sim.game().scene;
    assert_eq!(scene[Id(3)].at, vec2(60.0, 30.0));
    assert!(scene[Id(3)].flip);
    assert_eq!(scene.starts_as(Id(3)).unwrap().at, vec2(100.0, 20.0));
    // An object that isn't there, or a scene the game hasn't got: nothing.
    assert!(sim.change_object(&file, 77, &set));
    assert!(!sim.change_object("some/other/scene.json", 3, &set));
    sim.run(5);
    let last = sim.picture().pixels().to_vec();

    // It's in the recording, on the frame after it was made, and a replay
    // of the recording ends on the same picture.
    let recording = sim.recording();
    let json = recording.to_json();
    assert!(
        json.contains(&format!(
            "{{\"frame\": 6, \"object\": 3, \"in\": {:?}, \"set\": {{\"at\":[60,30],\"flip\":true}}}}",
            file
        )),
        "{json}"
    );
    let again = crate::script::InputScript::parse(&json).unwrap();
    assert_eq!(again.changes, recording.changes, "written and read back");
    crate::files::forget_given();
    let (_, game) = one_scene(&folder);
    let mut replay = Simulation::new(small_screen(), game);
    replay.follow(again);
    replay.run(10);
    assert_eq!(replay.game().scene[Id(3)].at, vec2(60.0, 30.0));
    assert!(replay.picture().pixels() == last, "the same last picture");
    crate::files::forget_given();
}

#[cfg(feature = "studio")]
#[test]
fn a_frozen_picture_says_where_every_object_is_and_where_it_starts() {
    use crate::canvas::SceneLook;
    use crate::scene::drawn::Origin;
    use crate::simulation::Simulation;
    let folder = game("scene-drawn");
    let (path, game) = one_scene(&folder);
    let mut sim = Simulation::new(small_screen(), game);
    sim.run(10); // the duck walks 10 pixels, onto the coin, and takes it
    sim.with_context(|game, _| {
        game.scene.camera.position.x += 4.0;
        game.scene.add(
            Object::new("spark")
                .at(vec2(70.0, 40.0))
                .with_picture("assets/red.png"),
        );
    });

    // Playing, scenes say nothing.
    sim.picture();
    assert!(sim.drawn_scenes().is_empty());

    // Frozen, each says where its objects are on the picture.
    sim.set_scene_look(SceneLook {
        note: true,
        as_it_starts: false,
    });
    let frozen = sim.picture().pixels().to_vec();
    let scenes = sim.drawn_scenes();
    assert_eq!(scenes.len(), 1);
    let scene = &scenes[0];
    assert_eq!(scene.file, crate::files::name_of(&path));
    assert!(!scene.as_it_starts);
    assert_eq!(
        (scene.scale, scene.offset),
        (1.0, vec2(-4.0, 0.0)),
        "the camera is 4 right"
    );
    assert_eq!(scene.to_scene(vec2(22.0, 40.0)), vec2(26.0, 40.0));

    // The duck has moved: it's where it is, and its start is elsewhere.
    let duck = scene.object(1).unwrap();
    assert_eq!(duck.rect, Some(Rect::new(22.0, 32.0, 8.0, 8.0)));
    assert_eq!(duck.start_rect, Some(Rect::new(12.0, 32.0, 8.0, 8.0)));
    assert_eq!(duck.now.get("at"), Some(&serde_json::json!([30, 40])));
    assert_eq!(duck.now.len(), 1, "only what changed: {:?}", duck.now);
    // It turns round its feet (its "at"), on the canvas; so does its start.
    assert_eq!(duck.pivot, Some(vec2(26.0, 40.0)));
    assert_eq!(duck.start_pivot, Some(vec2(16.0, 40.0)));
    assert_eq!((duck.turn, duck.start_turn), (0.0, 0.0));
    assert_eq!(duck.origin, Origin::File);
    // The lamp hasn't: one place, nothing changed.
    let lamp = scene.object(3).unwrap();
    assert_eq!(lamp.rect, Some(Rect::new(92.0, 12.0, 8.0, 8.0)));
    assert!(lamp.start_rect.is_none() && lamp.now.is_empty() && !lamp.gone);
    // The score stays on the screen whatever the camera does.
    let score = scene.object(4).unwrap();
    assert!(score.stays_on_screen);
    assert_eq!(score.rect, Some(Rect::new(2.0, 2.0, 8.0, 8.0)));
    // The coin was taken: gone, with where it starts.
    let coin = scene.object(2).unwrap();
    assert!(coin.gone && coin.rect.is_none() && coin.pivot.is_none());
    assert_eq!(coin.start_pivot, Some(vec2(30.0, 40.0)));
    assert_eq!(coin.start_rect, Some(Rect::new(26.0, 32.0, 8.0, 8.0)));
    // The spark was made while playing: its whole line is "now".
    let spark = scene
        .objects
        .iter()
        .find(|object| object.name == "spark")
        .unwrap();
    assert_eq!(spark.origin, Origin::Made);
    assert!(spark.start_rect.is_none() && spark.now.contains_key("picture"));
    // In drawing order, the gone ones last.
    let names: Vec<&str> = scene.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["duck", "lamp", "spark", "score", "coin"]);
    // It survives the link's JSON.
    assert_eq!(
        &crate::scene::drawn::DrawnScene::from_json(&scene.to_json()),
        scene
    );

    // As it starts: the file's objects where the file has them, the coin
    // back, no spark, on the same camera; and the game itself untouched.
    sim.set_scene_look(SceneLook {
        note: true,
        as_it_starts: true,
    });
    let starts = sim.picture().pixels().to_vec();
    assert_ne!(starts, frozen);
    let scene = &sim.drawn_scenes()[0];
    assert!(scene.as_it_starts);
    let names: Vec<&str> = scene.objects.iter().map(|o| o.name.as_str()).collect();
    assert_eq!(names, ["duck", "coin", "lamp", "score"]);
    let duck = scene.object(1).unwrap();
    assert_eq!(duck.rect, Some(Rect::new(12.0, 32.0, 8.0, 8.0)));
    assert!(duck.start_rect.is_none() && duck.now.is_empty());
    assert!(!scene.object(2).unwrap().gone);
    assert_eq!(
        sim.game().scene[Id(1)].at,
        vec2(30.0, 40.0),
        "the game hasn't changed"
    );
    sim.set_scene_look(SceneLook::default());
    assert!(
        sim.picture().pixels() == frozen,
        "back to the frozen moment"
    );
    crate::files::forget_given();
}

#[test]
fn a_scene_files_object_changes_by_its_keys_or_says_whats_wrong() {
    let mut file = SceneFile::parse(
        r#"{"objects": [
  {"id": 3, "name": "snail", "kind": "patrol", "at": [40, 208], "speed": 30, "turn": 10}
]}"#,
    )
    .unwrap();
    let set = |text: &str| serde_json::from_str::<serde_json::Map<_, _>>(text).unwrap();
    file.set(
        Id(3),
        &set(r#"{"at": [96, 200], "speed": 60, "layer": "front", "turn": null}"#),
    )
    .unwrap();
    let snail = file.object(Id(3)).unwrap();
    assert_eq!(snail.at, Vec2::new(96.0, 200.0));
    assert_eq!(snail.number("speed"), Some(60.0));
    assert_eq!(snail.layer, "front");
    assert_eq!(snail.turn, 0.0);
    assert_eq!(snail.id(), Id(3), "its id is its own, whatever is set");

    let wrong = file.set(Id(3), &set(r#"{"at": "left"}"#)).unwrap_err();
    assert!(wrong.contains("\"at\""), "{wrong}");
    assert_eq!(file.object(Id(3)).unwrap().at, Vec2::new(96.0, 200.0));
    assert!(file.set(Id(9), &set("{}")).unwrap_err().contains("id 9"));
}

#[test]
fn tools_add_remove_and_reorder_a_scene_files_objects() {
    let mut file = SceneFile::parse(
        r#"{"objects": [
  {"id": 1, "name": "sky", "kind": "background", "picture": "sky.png"},
  {"id": 2, "name": "sign", "at": [40, 208]},
  {"id": 5, "name": "duck", "at": [10, 208]}
]}"#,
    )
    .unwrap();
    let line = |text: &str| serde_json::from_str::<serde_json::Map<_, _>>(text).unwrap();
    let names =
        |file: &SceneFile| -> Vec<String> { file.objects.iter().map(|o| o.name.clone()).collect() };

    // A new object gets the next id, whatever its line says.
    let crate_id = file
        .add(
            &line(r#"{"id": 2, "name": "crate", "at": [96, 208]}"#),
            Some(Id(5)),
        )
        .unwrap();
    assert_eq!(crate_id, Id(6));
    assert_eq!(names(&file), ["sky", "sign", "crate", "duck"]);
    let last = file
        .add(
            &line(r#"{"name": "area", "kind": "area", "box": [0, 0, 32, 32]}"#),
            None,
        )
        .unwrap();
    assert_eq!(last, Id(7));
    assert_eq!(names(&file).last().unwrap(), "area");
    let wrong = file.add(&line(r#"{"at": "here"}"#), None).unwrap_err();
    assert!(wrong.contains("\"at\""), "{wrong}");
    assert_eq!(file.objects.len(), 5, "nothing added");

    // Reordering keeps the moved objects' own order.
    file.move_before(&[Id(7), Id(2)], Some(Id(6))).unwrap();
    assert_eq!(names(&file), ["sky", "sign", "area", "crate", "duck"]);
    file.move_before(&[Id(1)], None).unwrap();
    assert_eq!(names(&file), ["sign", "area", "crate", "duck", "sky"]);
    assert!(file.move_before(&[Id(1)], Some(Id(1))).is_err());
    assert!(file.move_before(&[Id(1)], Some(Id(99))).is_err());

    let gone = file.remove(Id(6)).unwrap();
    assert_eq!(gone.name, "crate");
    assert!(file.remove(Id(6)).is_none());
    assert_eq!(names(&file), ["sign", "area", "duck", "sky"]);

    // Written and read again, the same.
    let again = SceneFile::parse(&file.to_text()).unwrap();
    assert_eq!(names(&again), names(&file));
    assert_eq!(again.object(Id(7)).unwrap().kind, "area");
}

#[test]
fn a_sound_object_plays_when_the_scene_starts_and_follows_its_file() {
    use crate::simulation::Simulation;
    let folder = game("scene-sound");
    let samples = crate::sound::wav_bytes((0..2205).map(|i| (i % 50) as i16 * 100), 2205);
    std::fs::write(folder.join("assets/music.wav"), &samples).unwrap();
    std::fs::write(folder.join("assets/ding.wav"), &samples).unwrap();
    let (path, _) = one_scene(&folder);
    let name = crate::files::name_of(&path);
    let text = |music: &str| {
        format!(
            r##"{{"duckforge_scene": 1, "objects": [
            {{"id": 1, "name": "duck", "at": [20, 40], "picture": "assets/blue.png"}},
            {music}
            {{"id": 6, "name": "ding", "kind": "sound", "sound": "assets/ding.wav"}}]}}"##
        )
    };
    std::fs::write(
        &path,
        text(r#"{"id": 5, "name": "music", "kind": "sound", "sound": "assets/music.wav", "loops": true, "volume": 0.5},"#),
    )
    .unwrap();
    let scene = Scene::load(&path).unwrap();
    assert!(scene.problems().is_empty(), "{:?}", scene.problems());
    let mut sim = Simulation::new(small_screen(), OneScene { scene });
    sim.run(3);
    // The music loops from the first update; the ding plays once.
    let loops = sim.loops().to_vec();
    assert_eq!(loops.len(), 1);
    assert_eq!(
        (loops[0].name.as_str(), loops[0].start, loops[0].stop),
        ("music", 1, None)
    );
    assert_eq!(loops[0].volumes, [(1, 0.5)]);
    assert_eq!(sim.sound_frames("ding"), [1]);

    // Louder, from the file: the same loop, a new volume.
    let louder = text(
        r#"{"id": 5, "name": "music", "kind": "sound", "sound": "assets/music.wav", "loops": true, "volume": 1},"#,
    );
    assert!(sim.change_file(&name, &louder));
    sim.run(2);
    assert_eq!(sim.loops().len(), 1);
    assert_eq!(sim.loops()[0].volumes, [(1, 0.5), (4, 1.0)]);
    assert_eq!(sim.sound_frames("ding"), [1], "the ding isn't played again");

    // Hidden: it stops. Taken out of the file: nothing starts again.
    let hidden = text(
        r#"{"id": 5, "name": "music", "kind": "sound", "sound": "assets/music.wav", "loops": true, "shown": false},"#,
    );
    assert!(sim.change_file(&name, &hidden));
    sim.run(1);
    assert_eq!(sim.loops()[0].stop, Some(6));
    assert!(sim.change_file(&name, &text("")));
    sim.run(2);
    assert_eq!(sim.loops().len(), 1);

    // A sound that isn't there is said, with its line, and is silent.
    // (A file of its own: the one above is read from the change now.)
    let missing = text(
        r#"{"id": 5, "name": "music", "kind": "sound", "sound": "assets/gone.wav", "loops": true},"#,
    );
    let other = folder.file("scenes/missing.json", &missing);
    let scene = Scene::load(&other).unwrap();
    let said = scene.problems().join("\n");
    assert!(
        said.contains("line 3") && said.contains("\"music\" has the sound \"assets/gone.wav\""),
        "{said}"
    );
    assert!(said.contains("so it's silent"), "{said}");
}
