//! Play tests for Duck Craft: `cargo test --example duck_craft`.
//!
//! Most of them use a flat floor of stone (`arena`), so they don't depend
//! on the island's hills.

use super::*;
use std::f32::consts::PI;

/// The top of the arena's floor: the duck's feet stand at this height.
const FLOOR: i32 = 20;

/// The island with its first chunk (x and z 0 to 15) cleared to a flat
/// stone floor, the duck in its middle looking along +z, level.
fn arena(change: impl FnOnce(&mut World)) -> DuckCraft {
    let mut world = World::generate();
    for z in 0..CHUNK as i32 {
        for x in 0..CHUNK as i32 {
            for y in 1..TALL as i32 {
                world.set(x, y, z, if y < FLOOR { Block::Stone } else { Block::Air });
            }
        }
    }
    change(&mut world);
    let mut game = DuckCraft::new(Rc::new(Art::load().unwrap()), world);
    game.position = vec3(8.5, FLOOR as f32, 8.5);
    game.yaw = 0.0;
    game.pitch = 0.0;
    game.ducklings.clear(); // tests that want ducklings add them
    game
}

/// Presses these keys on the first frame and never lets go; the run
/// lasts `frames` frames.
fn hold(keys: &[&str], frames: u64) -> String {
    let presses: Vec<String> = keys
        .iter()
        .map(|k| format!(r#"{{"frame": 1, "press": "{k}"}}"#))
        .collect();
    format!(
        r#"{{"events": [{}], "frames": {frames}}}"#,
        presses.join(", ")
    )
}

/// Taps a key on frame 2.
fn tap(key: &str) -> String {
    format!(
        r#"{{"events": [{{"frame": 2, "press": "{key}"}}, {{"frame": 3, "release": "{key}"}}]}}"#
    )
}

#[test]
fn the_island_has_land_sea_trees_ores_and_caves() {
    let world = World::generate();
    let count = |kind: Block| {
        let blocks = world.grid.blocks().iter();
        blocks.filter(|&&b| b == kind as BlockId).count()
    };
    for kind in [
        Block::Grass,
        Block::Sand,
        Block::Water,
        Block::Log,
        Block::Leaves,
        Block::CoalOre,
        Block::GoldOre,
    ] {
        assert!(count(kind) > 0, "no {kind:?} on the island");
    }
    // The edges are sea.
    for i in 0..SIZE as i32 {
        assert_eq!(world.get(i, SEA, 0), Block::Water, "edge at x {i}");
        assert_eq!(world.get(0, SEA, i), Block::Water, "edge at z {i}");
    }
    // Caves: air below the ground somewhere.
    let caves = (0..SIZE as i32).any(|z| {
        (0..SIZE as i32).any(|x| {
            let h = World::ground_height(x, z);
            (1..h - 2).any(|y| world.get(x, y, z) == Block::Air)
        })
    });
    assert!(caves, "the island should have caves");
    // The duck starts on grass with room to stand.
    let s = world.spawn;
    let (x, y, z) = (s.x.floor() as i32, s.y as i32, s.z.floor() as i32);
    assert_eq!(world.get(x, y - 1, z), Block::Grass);
    assert_eq!(world.get(x, y, z), Block::Air);
    assert_eq!(world.get(x, y + 1, z), Block::Air);
}

#[test]
fn chunks_keep_only_faces_you_can_see() {
    let game = new_game().unwrap();
    let grid = &game.world.grid;
    let across = SIZE / CHUNK;
    assert_eq!(grid.chunks(), (across, across));
    let mut triangles = 0;
    for cz in 0..across {
        for cx in 0..across {
            triangles += grid.chunk_mesh(cx, cz).triangles.len();
        }
    }
    // Every block drawn whole would be millions of triangles; the faces
    // next to air are a few tens of thousands.
    assert!(triangles < 120_000, "{triangles} triangles");
    // A buried block adds nothing: the arena's floor is one layer of faces.
    let flat = arena(|_| {});
    let floor = flat.world.grid.chunk_mesh(0, 0);
    assert!(
        floor.triangles.len() <= 16 * 16 * 2 + 16 * 4 * 2 * 20,
        "{}",
        floor.triangles.len()
    );
}

#[test]
fn walking_forward_goes_the_way_the_duck_faces() {
    let played = PlayTest::new(config(), arena(|_| {}))
        .script(&hold(&["W"], 60))
        .frames(60)
        .run();
    let z = played.number("z");
    assert!(
        (12.0..13.0).contains(&z),
        "about 4.3 blocks in a second, from 8.5: {z}"
    );
    assert_eq!(played.number("x"), 8.5);
    assert_eq!(played.number("y"), FLOOR as f64);
    assert!(played.sound_frames("step").len() >= 2, "footsteps");
}

#[test]
fn a_jump_is_a_little_over_one_block_and_lands() {
    let mut sim = Simulation::new(config(), arena(|_| {}));
    sim.step();
    sim.press(Key::Space);
    sim.step();
    sim.release(Key::Space);
    let mut highest = 0.0f32;
    for _ in 0..60 {
        sim.step();
        highest = highest.max(sim.game().position.y - FLOOR as f32);
    }
    assert!((1.2..1.45).contains(&highest), "jumped {highest}");
    assert!(sim.game().on_ground);
    assert!(
        (sim.game().position.y - FLOOR as f32).abs() < 0.01,
        "back on the floor"
    );
}

#[test]
fn a_wall_stops_the_duck_and_a_step_is_hopped_up() {
    let wall = arena(|w| {
        for x in 0..CHUNK as i32 {
            w.set(x, FLOOR, 11, Block::Brick);
            w.set(x, FLOOR + 1, 11, Block::Brick);
        }
    });
    let played = PlayTest::new(config(), wall)
        .script(&hold(&["W"], 120))
        .frames(120)
        .run();
    let z = played.game().position.z;
    assert!(
        (10.6..=11.0 - HALF_WIDTH).contains(&z),
        "flush against the wall: {z}"
    );

    // A platform one block high from z = 11 on.
    let step = arena(|w| {
        for z in 11..CHUNK as i32 {
            for x in 0..CHUNK as i32 {
                w.set(x, FLOOR, z, Block::Planks);
            }
        }
    });
    let played = PlayTest::new(config(), step)
        .script(&hold(&["W"], 80))
        .frames(80)
        .run();
    let g = played.game();
    assert!(
        (g.position.y - (FLOOR as f32 + 1.0)).abs() < 0.01,
        "on the step: {:?} {:?}",
        g.position,
        g.velocity
    );
    assert!(played.number("z") > 12.0);
}

#[test]
fn digging_grass_takes_a_moment_and_gives_dirt() {
    // A grass block on the floor, two blocks ahead, at eye height minus one.
    let game = arena(|w| w.set(8, FLOOR + 1, 10, Block::Grass));
    let carried_before = game.carried[Block::Dirt as usize];
    let played = PlayTest::new(config(), game)
        .script(&hold(&["X"], 20))
        .frames(20)
        .run();
    assert_eq!(
        played.number("dug"),
        0.0,
        "not yet after a third of a second"
    );
    assert_eq!(played.text("looking_at"), "Grass");

    let game = arena(|w| w.set(8, FLOOR + 1, 10, Block::Grass));
    let played = PlayTest::new(config(), game)
        .script(&hold(&["X"], 40))
        .frames(40)
        .run();
    assert_eq!(played.number("dug"), 1.0);
    let game = played.game();
    assert_eq!(game.world.get(8, FLOOR + 1, 10), Block::Air);
    assert_eq!(game.carried[Block::Dirt as usize], carried_before + 1);
    assert_eq!(played.sound_frames("break").len(), 1);
}

#[test]
fn the_mouse_looks_round_while_the_window_holds_the_pointer() {
    // A grass block ahead, to check that the click taking the pointer back doesn't dig it.
    let mut sim = Simulation::new(config(), arena(|w| w.set(8, FLOOR + 1, 10, Block::Grass)));
    sim.step();
    assert!(sim.context().pointer_locked(), "held from the start");

    // Moving the mouse right turns right, however far it goes.
    sim.move_mouse_by(vec2(50.0, 0.0));
    sim.step();
    let yaw = sim.game().yaw;
    assert!((yaw - 50.0 * MOUSE_LOOK).abs() < 1e-5, "{yaw}");
    sim.move_mouse_by(vec2(-50.0, 0.0)); // and back, to look at the grass again
    sim.step();

    // Escape lets go of the pointer: now the mouse doesn't turn.
    sim.press(Key::Escape);
    sim.step();
    sim.release(Key::Escape);
    assert!(!sim.context().pointer_locked());
    assert!(sim.game().pointer_free);
    sim.move_mouse_by(vec2(80.0, 0.0));
    sim.step();
    assert!(sim.game().yaw.abs() < 1e-5);

    // A click takes it back, and holding that click doesn't dig.
    sim.press_mouse(MouseButton::Left);
    sim.run(40);
    assert!(sim.context().pointer_locked());
    assert_eq!(sim.game().world.get(8, FLOOR + 1, 10), Block::Grass);
    // The next click does.
    sim.release_mouse(MouseButton::Left);
    sim.step();
    sim.press_mouse(MouseButton::Left);
    sim.run(40);
    assert_eq!(sim.game().world.get(8, FLOOR + 1, 10), Block::Air);

    // Escape twice quits.
    for _ in 0..2 {
        sim.press(Key::Escape);
        sim.step();
        sim.release(Key::Escape);
        sim.step();
    }
    assert!(sim.quit_requested());
}

#[test]
fn a_lamp_lights_the_faces_round_it_and_glows_itself() {
    let game = arena(|w| w.set(8, FLOOR, 10, Block::Lamp));
    let mesh = game.world.grid.chunk_mesh(0, 0);
    let glows: Vec<f32> = (0..mesh.triangles.len()).map(|t| mesh.glow_of(t)).collect();
    // The lamp's own five faces (its underside is on the floor) glow fully...
    assert_eq!(glows.iter().filter(|&&g| g == 1.0).count(), 10);
    // ...the floor round it a little, less further away, and far off not at all.
    let partly: Vec<f32> = glows
        .iter()
        .copied()
        .filter(|&g| g > 0.0 && g < 1.0)
        .collect();
    assert!(partly.len() > 20, "{} faces lit", partly.len());
    assert!(partly.iter().any(|&g| g > 0.5) && partly.iter().any(|&g| g < 0.3));
    assert!(glows.iter().filter(|&&g| g == 0.0).count() > 400);
    // Digging it out puts the light out.
    let mut game = game;
    game.world.set(8, FLOOR, 10, Block::Air);
    game.update_chunks();
    let mesh = game.world.grid.chunk_mesh(0, 0);
    assert!((0..mesh.triangles.len()).all(|t| mesh.glow_of(t) == 0.0));
}

#[test]
fn bedrock_cant_be_dug() {
    let game = arena(|w| w.set(8, FLOOR + 1, 10, Block::Bedrock));
    let played = PlayTest::new(config(), game)
        .script(&hold(&["X"], 180))
        .frames(180)
        .run();
    assert_eq!(played.number("dug"), 0.0);
    assert_eq!(played.game().world.get(8, FLOOR + 1, 10), Block::Bedrock);
}

#[test]
fn gold_ore_counts_gold_and_rings() {
    let game = arena(|w| w.set(8, FLOOR + 1, 10, Block::GoldOre));
    let played = PlayTest::new(config(), game)
        .script(&hold(&["X"], 100))
        .frames(100)
        .run();
    assert_eq!(played.number("gold"), 1.0);
    assert_eq!(played.sound_frames("coin").len(), 1);
}

#[test]
fn placing_puts_the_block_in_hand_on_the_face_you_look_at() {
    // Looking down at the floor two blocks ahead.
    let mut game = arena(|_| {});
    game.pitch = -0.6;
    let planks = game.carried[Block::Planks as usize];
    let played = PlayTest::new(config(), game)
        .script(&tap("C"))
        .frames(10)
        .run();
    let game = played.game();
    assert_eq!(played.number("placed"), 1.0);
    assert_eq!(game.carried[Block::Planks as usize], planks - 1);
    let target = game
        .world
        .raycast(game.eye(), game.look_direction(), REACH)
        .unwrap();
    assert_eq!(
        target.block,
        Block::Planks,
        "the new block is the one in view"
    );
    assert_eq!(target.cell[1], FLOOR, "on top of the floor");
}

#[test]
fn a_block_is_never_placed_inside_the_duck() {
    let mut game = arena(|_| {});
    game.pitch = -1.55; // straight down, at the floor under the feet
    let played = PlayTest::new(config(), game)
        .script(&tap("C"))
        .frames(10)
        .run();
    assert_eq!(played.number("placed"), 0.0);
}

#[test]
fn the_ray_finds_the_first_block_and_its_face() {
    let world = arena(|w| w.set(8, FLOOR + 1, 11, Block::Brick)).world;
    let hit = world
        .raycast(
            vec3(8.5, FLOOR as f32 + 1.62, 8.5),
            vec3(0.0, 0.0, 1.0),
            REACH,
        )
        .unwrap();
    assert_eq!(hit.cell, [8, FLOOR + 1, 11]);
    assert_eq!(hit.normal, [0, 0, -1], "the face looking back at the duck");
    assert_eq!(hit.block, Block::Brick);
    // Out of reach.
    assert!(
        world
            .raycast(vec3(8.5, 25.0, 8.5), vec3(0.0, 0.0, 1.0), 2.0)
            .is_none()
    );
}

#[test]
fn picking_a_slot_changes_the_block_in_hand() {
    let played = PlayTest::new(config(), arena(|_| {}))
        .script(&tap("Digit6"))
        .frames(5)
        .run();
    assert_eq!(played.text("holding"), "Brick");
    assert_eq!(played.sound_frames("pick").len(), 1);
}

#[test]
fn the_playthrough_digs_builds_and_ends_behind_the_duck() {
    let played = PlayTest::new(config(), new_game().unwrap())
        .inputs("playthroughs/duck_craft.json")
        .run();
    assert_eq!(played.number("dug"), 2.0);
    assert_eq!(played.number("placed"), 7.0);
    assert!(played.game().behind_view);
}

/// Prints the island from above: `cargo test --example duck_craft -- --ignored print_map --nocapture`.
#[test]
#[ignore]
fn print_map() {
    let world = World::generate();
    for z in (0..SIZE as i32).rev() {
        let row: String = (0..SIZE as i32)
            .map(|x| {
                let mut top = TALL as i32 - 1;
                while top > 0 && world.get(x, top, z) == Block::Air {
                    top -= 1;
                }
                match world.get(x, top, z) {
                    Block::Water => '~',
                    Block::Sand => '.',
                    Block::Leaves | Block::Log => 'T',
                    _ => char::from_digit(((top - SEA).max(0) as u32 / 2).min(35), 36).unwrap(),
                }
            })
            .collect();
        println!("{row}");
    }
    println!("spawn {:?}", world.spawn);
}

/// Saves pictures from a few places, angles and times of day into
/// out/views: `cargo test --example duck_craft -- --ignored views`.
#[test]
#[ignore]
fn views() {
    std::fs::create_dir_all("out/views").unwrap();
    let shots = [
        ("spawn", None, f32::NAN, -0.25, false, 0.0),
        ("north", None, 0.0, -0.1, false, 0.0),
        ("west", None, -1.6, -0.2, false, 0.0),
        ("down", None, 2.5, -0.9, false, 0.0),
        ("behind", None, f32::NAN, -0.35, true, 0.0),
        ("hill", Some(vec3(20.5, 30.0, 47.5)), 2.4, -0.3, false, 0.0),
        ("sunset", None, 4.0, -0.05, false, 0.47 * DAY_SECONDS),
        ("night", None, 1.0, 0.1, false, 0.7 * DAY_SECONDS),
    ];
    // The duck itself, from the front, the side and above-behind, mid-stride.
    for (name, orbit, pitch) in [
        ("duck_front", 0.0, 0.0),
        ("duck_side", 1.57, 0.4),
        ("duck_back", 3.6, -0.5),
    ] {
        let mut game = arena(|_| {});
        game.behind_view = true;
        game.orbit = Some(orbit);
        game.pitch = pitch;
        game.show_help = false;
        game.time = 0.17; // arms and legs mid-swing
        game.velocity = vec3(0.0, 0.0, WALK_SPEED);
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture()
            .save_png(format!("out/views/{name}.png"))
            .unwrap();
    }
    // Ducklings, close up: walking, swimming, and one aimed at.
    {
        let mut game = arena(|w| {
            for x in 9..13 {
                for z in 10..13 {
                    w.set(x, FLOOR - 1, z, Block::Water);
                }
            }
        });
        let at = |x: f32, z: f32| vec3(x, FLOOR as f32, z);
        game.ducklings = vec![
            ducklings::Duckling::new(at(7.5, 10.5), 2.6),
            ducklings::Duckling::new(at(8.5, 11.0), 0.3),
            ducklings::Duckling::new(at(6.4, 11.6), -0.8),
            ducklings::Duckling::new(vec3(10.5, FLOOR as f32 - 0.6, 11.5), 1.2),
        ];
        game.pitch = -0.45;
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.run(20);
        sim.picture().save_png("out/views/ducklings.png").unwrap();
        // From behind the duck, with three following.
        let mut game = new_game().unwrap();
        for (i, d) in game.ducklings.iter_mut().take(3).enumerate() {
            d.follows = Some(i as u32 + 1);
        }
        game.behind_view = true;
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.press(Key::W);
        sim.run(150);
        sim.release(Key::W);
        sim.press(Key::Left);
        sim.run(70);
        sim.release(Key::Left);
        sim.run(30);
        sim.picture().save_png("out/views/parade.png").unwrap();
    }
    // The crafting menu, with some things to craft with.
    {
        let mut game = arena(|_| {});
        game.carried[Block::Log as usize] = 3;
        game.carried[Block::Sand as usize] = 6;
        game.carried[Block::Dirt as usize] = 1;
        game.coal = 2;
        game.gold = 1;
        game.menu = Some(1);
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture().save_png("out/views/crafting.png").unwrap();
    }
    // First person: the arm, level and looking down.
    for (name, pitch) in [("hand_level", 0.0), ("hand_down", -0.8)] {
        let mut game = arena(|_| {});
        game.pitch = pitch;
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture()
            .save_png(format!("out/views/{name}.png"))
            .unwrap();
    }
    // At night, round a few lamps on the arena's floor.
    let mut game = arena(|w| {
        for (x, z) in [(6, 12), (10, 13), (8, 16)] {
            w.set(x, FLOOR, z, Block::Lamp);
        }
        w.set(9, FLOOR, 12, Block::Planks);
    });
    game.pitch = -0.35;
    game.show_help = false;
    game.time = 0.7 * DAY_SECONDS;
    let mut sim = Simulation::new(config(), game);
    sim.step();
    sim.picture().save_png("out/views/lamps.png").unwrap();
    for (name, at, yaw, pitch, behind, time) in shots {
        let mut game = new_game().unwrap();
        if let Some(at) = at {
            game.position = at;
        }
        if !yaw.is_nan() {
            game.yaw = yaw;
        }
        game.pitch = pitch;
        game.behind_view = behind;
        game.show_help = false;
        game.time = time;
        let mut sim = Simulation::new(config(), game);
        sim.run(40);
        sim.picture()
            .save_png(format!("out/views/{name}.png"))
            .unwrap();
    }
}

#[test]
fn a_log_makes_four_planks() {
    let mut game = arena(|_| {});
    game.carried[Block::Log as usize] = 2;
    let planks = game.carried[Block::Planks as usize];
    // E opens the menu on the first recipe (log to planks); Space makes it.
    let script = r#"{"events": [
        {"frame": 2, "press": "E"}, {"frame": 3, "release": "E"},
        {"frame": 5, "press": "Space"}, {"frame": 6, "release": "Space"}
    ], "frames": 10}"#;
    let played = PlayTest::new(config(), game).script(script).run();
    let game = played.game();
    assert_eq!(game.carried[Block::Log as usize], 1);
    assert_eq!(game.carried[Block::Planks as usize], planks + 4);
    assert_eq!(played.sound_frames("craft").len(), 1);
    assert_eq!(
        played.number("y"),
        FLOOR as f64,
        "Space crafted, it didn't jump"
    );
    assert!(game.menu.is_some());
}

#[test]
fn shift_crafts_as_many_as_you_can_and_gold_makes_lamps() {
    let mut game = arena(|_| {});
    game.gold = 3;
    game.carried[Block::Glass as usize] = 2;
    game.carried[Block::Lamp as usize] = 0;
    // Recipe 4 (gold and glass to lamps): down three times, then Shift+Space.
    let script = r#"{"events": [
        {"frame": 2, "press": "E"}, {"frame": 3, "release": "E"},
        {"frame": 4, "press": "S"}, {"frame": 5, "release": "S"},
        {"frame": 6, "press": "S"}, {"frame": 7, "release": "S"},
        {"frame": 8, "press": "S"}, {"frame": 9, "release": "S"},
        {"frame": 10, "press": "LeftShift"},
        {"frame": 11, "press": "Space"}, {"frame": 12, "release": "Space"}
    ], "frames": 14}"#;
    let played = PlayTest::new(config(), game).script(script).run();
    let game = played.game();
    assert_eq!(
        game.gold, 1,
        "two lamps' worth of glass, so two of the three gold"
    );
    assert_eq!(game.carried[Block::Glass as usize], 0);
    assert_eq!(game.carried[Block::Lamp as usize], 4);
    assert_eq!(
        played.number("z"),
        8.5,
        "S picked a recipe; the duck didn't walk"
    );
}

#[test]
fn missing_things_make_nothing() {
    let mut game = arena(|_| {});
    game.carried[Block::Sand as usize] = 5; // sand but no coal
    game.menu = Some(1);
    let glass = game.carried[Block::Glass as usize];
    let played = PlayTest::new(config(), game)
        .script(&tap("Space"))
        .frames(6)
        .run();
    assert_eq!(played.game().carried[Block::Glass as usize], glass);
    assert_eq!(played.game().carried[Block::Sand as usize], 5);
    assert_eq!(played.number("crafted"), 0.0);
}

#[test]
fn e_and_escape_close_the_menu() {
    for key in ["E", "Escape"] {
        let mut game = arena(|_| {});
        game.menu = Some(0);
        let played = PlayTest::new(config(), game)
            .script(&tap(key))
            .frames(6)
            .run();
        assert!(played.game().menu.is_none(), "{key} closes the menu");
    }
}

/// The arena with one duckling two blocks ahead of the duck, and the duck
/// looking down at it.
fn duckling_ahead() -> DuckCraft {
    let mut game = arena(|_| {});
    game.ducklings = vec![ducklings::Duckling::new(vec3(8.5, FLOOR as f32, 10.5), PI).sitting()];
    game.pitch = -0.55;
    game
}

#[test]
fn the_island_hatches_ducklings_on_grass() {
    let game = new_game().unwrap();
    assert_eq!(game.ducklings.len(), ducklings::DUCKLINGS);
    for d in &game.ducklings {
        let below = game.world.get(
            d.position.x.floor() as i32,
            d.position.y as i32 - 1,
            d.position.z.floor() as i32,
        );
        assert_eq!(below, Block::Grass);
        assert!(!ducklings::DUCKLING.hits(&game.world, d.position));
        assert!(d.follows.is_none());
    }
}

#[test]
fn right_clicking_a_duckling_makes_it_follow_and_again_lets_it_go() {
    let played = PlayTest::new(config(), duckling_ahead())
        .script(&tap("C"))
        .frames(8)
        .run();
    assert_eq!(played.number("ducklings_following"), 1.0);
    assert_eq!(played.number("placed"), 0.0, "no block placed behind it");
    assert!(!played.sound_frames("quack").is_empty());
    let mut game = duckling_ahead();
    game.ducklings[0].follows = Some(1);
    let played = PlayTest::new(config(), game)
        .script(&tap("C"))
        .frames(8)
        .run();
    assert_eq!(played.number("ducklings_following"), 0.0);
}

#[test]
fn digging_at_a_duckling_digs_nothing() {
    let played = PlayTest::new(config(), duckling_ahead())
        .script(&hold(&["X"], 90))
        .frames(90)
        .run();
    assert_eq!(played.number("dug"), 0.0);
    assert_eq!(played.text("looking_at"), "Duckling");
}

#[test]
fn followers_walk_after_the_duck_in_a_line() {
    let mut game = arena(|_| {});
    // Three ducklings behind the duck, imprinted in order.
    game.ducklings = (0..3)
        .map(|i| {
            let mut d = ducklings::Duckling::new(vec3(4.5 + i as f32, FLOOR as f32, 3.5), 0.0);
            d.follows = Some(i + 1);
            d
        })
        .collect();
    game.position = vec3(8.5, FLOOR as f32, 3.5);
    // Walk along +z for 2 seconds, then stand for 2.
    let script = r#"{"events": [{"frame": 1, "press": "W"}, {"frame": 100, "release": "W"}]}"#;
    let played = PlayTest::new(config(), game)
        .script(script)
        .frames(220)
        .run();
    let game = played.game();
    let duck = game.position;
    assert!(
        (10.0..14.0).contains(&duck.z),
        "the duck walked about 7 blocks: {duck:?}"
    );
    let mut line: Vec<_> = game.ducklings.iter().collect();
    line.sort_by_key(|d| d.follows);
    let mut leader = duck;
    for (i, d) in line.iter().enumerate() {
        let gap = vec2(d.position.x - leader.x, d.position.z - leader.z).length();
        assert!(gap < 2.0, "duckling {i} is {gap} from the one ahead");
        assert!(gap > 0.5, "duckling {i} keeps a little gap ({gap})");
        leader = d.position;
    }
}

#[test]
fn a_duckling_floats_on_water() {
    let mut game = arena(|w| {
        for x in 6..12 {
            for z in 9..13 {
                for y in FLOOR - 3..FLOOR {
                    w.set(x, y, z, Block::Water);
                }
            }
        }
    });
    // Dropped into a pond three deep.
    game.ducklings = vec![ducklings::Duckling::new(
        vec3(8.5, FLOOR as f32 - 2.5, 10.5),
        0.0,
    )];
    let played = PlayTest::new(config(), game).frames(180).run();
    let d = &played.game().ducklings[0];
    assert!(d.in_water);
    assert!(
        (d.position.y - FLOOR as f32).abs() < 0.4,
        "bobs at the top: {}",
        d.position.y
    );
}

#[test]
fn the_ray_finds_a_ducklings_box() {
    let hit = ducklings::ray_box(
        vec3(0.0, 0.5, -5.0),
        vec3(0.0, 0.0, 1.0),
        vec3(-0.5, 0.0, -0.5),
        vec3(0.5, 1.0, 0.5),
    );
    assert_eq!(hit, Some(4.5));
    assert!(
        ducklings::ray_box(
            vec3(2.0, 0.5, -5.0),
            vec3(0.0, 0.0, 1.0),
            vec3(-0.5, 0.0, -0.5),
            vec3(0.5, 1.0, 0.5)
        )
        .is_none()
    );
}

#[test]
fn sand_falls_when_the_block_under_it_is_dug() {
    // A pillar: dirt at eye height minus one, three sand on top.
    let game = arena(|w| {
        w.set(8, FLOOR + 1, 10, Block::Dirt);
        for y in FLOOR + 2..FLOOR + 5 {
            w.set(8, y, 10, Block::Sand);
        }
        w.set(8, FLOOR, 10, Block::Stone); // what the dirt stood on
    });
    // Dig for half a second: long enough for dirt (0.4 s), then let go.
    let script = r#"{"events": [{"frame": 1, "press": "X"}, {"frame": 30, "release": "X"}]}"#;
    let played = PlayTest::new(config(), game)
        .script(script)
        .frames(120)
        .run();
    let game = played.game();
    // The dirt went; the sand all fell one block and stacked again.
    assert_eq!(game.world.get(8, FLOOR + 1, 10), Block::Sand);
    assert_eq!(game.world.get(8, FLOOR + 2, 10), Block::Sand);
    assert_eq!(game.world.get(8, FLOOR + 3, 10), Block::Sand);
    assert_eq!(game.world.get(8, FLOOR + 4, 10), Block::Air);
    assert!(game.falling.is_empty());
    assert!(played.sound_frames("thud").len() >= 3);
}

#[test]
fn sand_placed_in_the_air_falls_and_sinks_through_water() {
    let mut game = arena(|w| {
        w.set(8, FLOOR + 2, 11, Block::Brick); // something high up to place against
        w.set(8, FLOOR - 1, 10, Block::Water); // a pool two deep under it
        w.set(8, FLOOR - 2, 10, Block::Water);
    });
    game.carried[Block::Sand as usize] = 1;
    game.slot = HOTBAR.iter().position(|&b| b == Block::Sand).unwrap();
    game.pitch = 0.25; // looking up a little, at the brick's near face
    let target = game
        .world
        .raycast(game.eye(), game.look_direction(), REACH)
        .unwrap();
    assert_eq!(target.block, Block::Brick);
    assert_eq!(target.normal, [0, 0, -1]);
    let played = PlayTest::new(config(), game)
        .script(&tap("C"))
        .frames(90)
        .run();
    let world = &played.game().world;
    assert_eq!(played.number("placed"), 1.0);
    // Placed at (8, FLOOR + 2, 10), it fell through the air and the water
    // to the stone at the bottom of the pool.
    assert_eq!(world.get(8, FLOOR + 2, 10), Block::Air);
    assert_eq!(world.get(8, FLOOR - 2, 10), Block::Sand);
    assert_eq!(world.get(8, FLOOR - 1, 10), Block::Water);
}

#[test]
fn sand_landing_on_the_duck_is_caught() {
    let mut game = arena(|w| w.set(8, FLOOR + 3, 8, Block::Sand));
    // Hold the sand up with nothing, then let it go: it lands on the duck.
    game.world.unsettled.push([8, FLOOR + 3, 8]);
    let sand = game.carried[Block::Sand as usize];
    let played = PlayTest::new(config(), game).frames(60).run();
    let game = played.game();
    assert_eq!(game.carried[Block::Sand as usize], sand + 1);
    assert_eq!(game.world.get(8, FLOOR + 1, 8), Block::Air);
}

#[test]
fn a_saved_island_loads_back_the_same() {
    let mut game = new_game().unwrap();
    // Change things: a hole, a lamp, things carried, a follower, a place.
    let [x, z] = [game.world.spawn.x as i32, game.world.spawn.z as i32];
    let y = game.world.spawn.y as i32 - 1;
    game.world.set(x + 2, y, z, Block::Air);
    game.world.set(x - 2, y + 1, z, Block::Lamp);
    game.gold = 3;
    game.coal = 5;
    game.carried[Block::Log as usize] = 7;
    game.ducklings[2].follows = Some(4);
    game.imprints = 4;
    game.time = 123.5;
    game.yaw = 1.25;
    game.slot = 6;
    let bytes = game.to_bytes();
    assert!(
        bytes.len() < 60_000,
        "runs keep it small: {} bytes",
        bytes.len()
    );

    let art = Rc::new(Art::load().unwrap());
    let loaded = DuckCraft::from_bytes(art, &bytes).unwrap();
    assert_eq!(loaded.world.grid.blocks(), game.world.grid.blocks());
    assert_eq!(loaded.world.get(x + 2, y, z), Block::Air);
    assert_eq!(loaded.world.lamps.len(), game.world.lamps.len());
    assert_eq!(loaded.carried, game.carried);
    assert_eq!(
        (loaded.gold, loaded.coal, loaded.imprints, loaded.slot),
        (3, 5, 4, 6)
    );
    assert_eq!(loaded.position, game.position);
    assert_eq!((loaded.time, loaded.yaw), (123.5, 1.25));
    assert_eq!(loaded.ducklings.len(), game.ducklings.len());
    assert_eq!(loaded.ducklings[2].follows, Some(4));
    assert_eq!(loaded.ducklings[0].position, game.ducklings[0].position);
    assert!(loaded.world.unsettled.is_empty());
}

#[test]
fn saving_writes_a_file_that_the_game_carries_on_from() {
    let path = std::env::temp_dir().join(format!("duck_craft_test_{}.dat", std::process::id()));
    let mut game = save::load_or_new(Some(path.clone())).unwrap();
    game.gold = 9;
    game.save().unwrap();
    assert!(path.exists());
    let again = save::load_or_new(Some(path.clone())).unwrap();
    assert_eq!(again.gold, 9);
    assert_eq!(again.save_path.as_deref(), Some(path.as_path()));
    // A broken file: a new island, not a crash.
    std::fs::write(&path, b"DUCKCRAFT1 and then nonsense").unwrap();
    let fresh = save::load_or_new(Some(path.clone())).unwrap();
    assert_eq!(fresh.gold, 0);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn tests_and_replays_never_save() {
    let game = new_game().unwrap();
    assert!(game.save_path.is_none());
    let played = PlayTest::new(config(), arena(|_| {}))
        .script(&tap("F5"))
        .frames(5)
        .run();
    assert!(played.game().message.contains("off"));
}

#[test]
fn number_keys_and_a_d_pick_recipes_in_the_menu() {
    let mut game = arena(|_| {});
    game.menu = Some(0);
    let played = PlayTest::new(config(), game)
        .script(&tap("Digit4"))
        .frames(5)
        .run();
    assert_eq!(played.number("menu_row"), 3.0);
    assert_eq!(played.text("holding"), "Planks", "the hotbar didn't change");
    let mut game = arena(|_| {});
    game.menu = Some(0);
    let played = PlayTest::new(config(), game)
        .script(&tap("A"))
        .frames(5)
        .run();
    assert_eq!(
        played.number("menu_row"),
        4.0,
        "A goes up, round to the last"
    );
}

/// Pictures for a showcase video of crafting, ducklings and falling sand,
/// one every other frame, in out/showcase:
/// `cargo test --example duck_craft -- --ignored showcase`, then
/// `ffmpeg -framerate 30 -i out/showcase/%04d.png -vf scale=1280:960:flags=neighbor showcase.mp4`.
#[test]
#[ignore]
fn showcase() {
    let dir = "out/showcase";
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let mut game = new_game().unwrap();
    game.carried[Block::Log as usize] = 3;
    game.carried[Block::Sand as usize] = 6;
    game.coal = 4;
    game.gold = 2;
    game.show_help = false;
    let mut sim = Simulation::new(config(), game);
    let mut frame = 0;
    let mut shoot = |sim: &mut Simulation<DuckCraft>, steps: usize| {
        for _ in 0..steps {
            sim.step();
            if sim.game().time > 0.0 && frame % 2 == 0 || frame == 0 {
                let n = frame / 2;
                sim.picture().save_png(format!("{dir}/{n:04}.png")).unwrap();
            }
            frame += 1;
        }
    };
    let mut tap = |sim: &mut Simulation<DuckCraft>, key: Key, shoot: &mut dyn FnMut(&mut Simulation<DuckCraft>, usize)| {
        sim.press(key);
        shoot(sim, 1);
        sim.release(key);
        shoot(sim, 14);
    };
    shoot(&mut sim, 40);
    // Crafting: planks from logs, glass from sand and coal, lamps from gold.
    tap(&mut sim, Key::E, &mut shoot);
    sim.press(Key::LeftShift);
    tap(&mut sim, Key::Space, &mut shoot);
    sim.release(Key::LeftShift);
    shoot(&mut sim, 20);
    tap(&mut sim, Key::Digit2, &mut shoot);
    tap(&mut sim, Key::Space, &mut shoot);
    tap(&mut sim, Key::Space, &mut shoot);
    shoot(&mut sim, 10);
    tap(&mut sim, Key::Digit4, &mut shoot);
    sim.press(Key::LeftShift);
    tap(&mut sim, Key::Space, &mut shoot);
    sim.release(Key::LeftShift);
    shoot(&mut sim, 30);
    tap(&mut sim, Key::E, &mut shoot);

    // Meet the three nearest ducklings: look at each and right-click.
    let mut met = 0;
    for _ in 0..3 {
        let g = sim.game();
        let eye = g.eye();
        let nearest = (0..g.ducklings.len())
            .filter(|&i| g.ducklings[i].follows.is_none())
            .min_by(|&a, &b| {
                let da = g.ducklings[a].position.distance(eye);
                let db = g.ducklings[b].position.distance(eye);
                da.total_cmp(&db)
            });
        let Some(i) = nearest else { break };
        // Walk up to it (giving up after 1.5 s), then look straight at it.
        for _ in 0..90 {
            let g = sim.game();
            let to = g.ducklings[i].position - g.position;
            if vec2(to.x, to.z).length() < 2.2 {
                break;
            }
            let want = to.x.atan2(to.z);
            let game = sim.game_mut();
            game.yaw += ((want - game.yaw + std::f32::consts::PI).rem_euclid(TAU) - std::f32::consts::PI).clamp(-0.08, 0.08);
            sim.press(Key::W);
            shoot(&mut sim, 1);
        }
        sim.release(Key::W);
        shoot(&mut sim, 8);
        let g = sim.game();
        let to = g.ducklings[i].position + vec3(0.0, 0.3, 0.0) - g.eye();
        let game = sim.game_mut();
        game.yaw = to.x.atan2(to.z);
        game.pitch = (to.y / vec2(to.x, to.z).length()).atan();
        shoot(&mut sim, 12);
        tap(&mut sim, Key::C, &mut shoot);
        if sim.game().ducklings[i].follows.is_some() {
            met += 1;
        }
        shoot(&mut sim, 20);
    }

    // A walk with the line behind, seen from behind the duck.
    tap(&mut sim, Key::V, &mut shoot);
    sim.game_mut().pitch = -0.3;
    sim.press(Key::W);
    shoot(&mut sim, 90);
    sim.press(Key::Left);
    shoot(&mut sim, 80);
    sim.release(Key::Left);
    shoot(&mut sim, 60);
    sim.release(Key::W);
    sim.press(Key::Right);
    shoot(&mut sim, 95);
    sim.release(Key::Right);
    shoot(&mut sim, 90);
    eprintln!("SHOWCASE {} pictures, met {met} ducklings, crafted {}", frame / 2, sim.game().crafted);
}
