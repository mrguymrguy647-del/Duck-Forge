//! Play tests for Duck Craft: `cargo test --example duck_craft`.
//!
//! Most of them use a flat floor of stone (`arena`), so they don't depend
//! on the island's hills.

use super::*;

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
    game
}

/// Holds these keys from the first frame for `frames` frames.
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
    let count = |kind: Block| world.blocks.iter().filter(|&&b| b == kind).count();
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
    assert_eq!(game.chunks.len(), (SIZE / CHUNK).pow(2));
    let triangles: usize = game
        .chunks
        .iter()
        .map(|(solid, _)| solid.triangles.len())
        .sum();
    // Every block drawn whole would be millions of triangles; the faces
    // next to air are a few tens of thousands.
    assert!(triangles < 120_000, "{triangles} triangles");
    // A buried block adds nothing: the arena's floor is one layer of faces.
    let flat = arena(|_| {});
    let (floor, _) = &flat.chunks[0];
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
