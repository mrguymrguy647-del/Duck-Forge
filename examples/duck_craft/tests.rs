//! Play tests for Duck Craft: `cargo test --example duck_craft`.
//!
//! Most of them use a flat floor of stone (`arena`), so they don't depend
//! on the island's hills.

use super::*;
use menus::{Mode, Stage};
use std::f32::consts::PI;

/// The top of the arena's floor: the duck's feet stand at this height.
const FLOOR: i32 = 20;

/// The island with its first chunk (x and z 0 to 15) cleared to a flat
/// stone floor, the duck in its middle looking along +z, level.
fn arena(change: impl FnOnce(&mut World)) -> DuckCraft {
    let mut world = World::generate(SEED);
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
    let world = World::generate(SEED);
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
            let h = World::ground_height(x, z, SEED);
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
    let carried_before = game.count(Item::Block(Block::Dirt));
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
    assert_eq!(game.count(Item::Block(Block::Dirt)), carried_before + 1);
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

    // Escape pauses and lets go of the pointer: now the mouse doesn't turn.
    sim.press(Key::Escape);
    sim.step();
    sim.release(Key::Escape);
    assert!(!sim.context().pointer_locked());
    assert!(matches!(sim.game().stage, Stage::Paused { .. }));
    sim.move_mouse_by(vec2(80.0, 0.0));
    sim.step();
    assert!(sim.game().yaw.abs() < 1e-5);
    let time = sim.game().time;
    sim.run(30);
    assert_eq!(sim.game().time, time, "the island waits while paused");

    // Clicking "Back to the island" takes the pointer back, and holding
    // that click doesn't dig.
    let back = menus::button_rect(0, 82.0);
    sim.move_mouse(vec2(back.x + 10.0, back.y + 9.0));
    sim.press_mouse(MouseButton::Left);
    sim.run(40);
    assert_eq!(sim.game().stage, Stage::Playing);
    assert!(sim.context().pointer_locked());
    assert_eq!(sim.game().world.get(8, FLOOR + 1, 10), Block::Grass);
    // The next click does.
    sim.release_mouse(MouseButton::Left);
    sim.step();
    sim.press_mouse(MouseButton::Left);
    sim.run(40);
    sim.release_mouse(MouseButton::Left);
    assert_eq!(sim.game().world.get(8, FLOOR + 1, 10), Block::Air);

    // Escape, then the last button (quit).
    for key in [Key::Escape, Key::S, Key::S, Key::S, Key::Enter] {
        sim.press(key);
        sim.step();
        sim.release(key);
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
    let planks = game.count(Item::Block(Block::Planks));
    let played = PlayTest::new(config(), game)
        .script(&tap("C"))
        .frames(10)
        .run();
    let game = played.game();
    assert_eq!(played.number("placed"), 1.0);
    assert_eq!(game.count(Item::Block(Block::Planks)), planks - 1);
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
        .script(&tap("Digit3"))
        .frames(5)
        .run();
    assert_eq!(
        played.text("holding"),
        "Brick",
        "the starting kit's third slot"
    );
    assert_eq!(played.sound_frames("pick").len(), 1);
}

#[test]
fn the_playthrough_digs_builds_and_ends_behind_the_duck() {
    let played = PlayTest::new(config(), title_game(None).unwrap())
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
    let world = World::generate(SEED);
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
    // The night: Hollows close, by a lamp, the Watcher far off; the pause
    // menu; the faint screen; creative's panel.
    {
        let night = at_time_of_day(0.78);
        // Three Hollows coming, seen by lamplight.
        let mut game = new_game().unwrap();
        game.time = night;
        game.last_time_of_day = game.time_of_day();
        game.spawn_wait = 1000.0;
        let s = game.world.spawn;
        let ahead = vec3(game.yaw.sin(), 0.0, game.yaw.cos());
        let side = vec3(game.yaw.cos(), 0.0, -game.yaw.sin());
        for (i, (f, r)) in [(4.0, -1.2), (6.5, 1.6), (9.0, -0.4)].iter().enumerate() {
            let at = game.free_spot(s + ahead * *f + side * *r);
            let mut h = Hollow::new(at, game.yaw + PI);
            h.chasing = i < 2;
            game.hollows.push(h);
        }
        let lamp = s + side * -2.0 + ahead * 1.0;
        game.world
            .set(lamp.x as i32, lamp.y as i32, lamp.z as i32, Block::Lamp);
        game.update_chunks();
        game.show_help = false;
        game.pitch = -0.05;
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture().save_png("out/views/hollows.png").unwrap();
        // One Hollow, close, from behind the duck.
        let mut game = hollow_ahead(3.0);
        game.behind_view = true;
        game.hollows[0].chasing = true;
        game.world.set(6, FLOOR, 10, Block::Lamp);
        game.update_chunks();
        game.show_help = false;
        game.orbit = Some(2.5);
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture()
            .save_png("out/views/hollow_close.png")
            .unwrap();
        // The Watcher, far off, off to one side.
        let mut game = new_game().unwrap();
        game.time = night;
        game.last_time_of_day = game.time_of_day();
        game.spawn_wait = 1000.0;
        game.watcher_night = game.day_number();
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.step();
        let mut ctx_game = sim.game().clone();
        let at = ctx_game.free_spot(
            ctx_game.world.spawn + vec3(ctx_game.yaw.sin() + 0.6, 0.0, ctx_game.yaw.cos()) * 20.0,
        );
        ctx_game.watcher = Some(Watcher::new(at, ctx_game.yaw + PI));
        ctx_game.pitch = 0.05;
        let mut sim = Simulation::new(config(), ctx_game);
        sim.step();
        sim.picture().save_png("out/views/watcher.png").unwrap();
        // The pause menu, and fainted.
        let mut game = new_game().unwrap();
        game.stage = Stage::Paused { selected: 1 };
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture().save_png("out/views/pause.png").unwrap();
        let mut game = arena(|_| {});
        game.vitals.health = 0;
        game.vitals.fainted = true;
        game.fainted_because = String::from("was caught by a Hollow");
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture().save_png("out/views/fainted.png").unwrap();
        // Creative's inventory.
        let mut game = creative(|_| {});
        game.screen = Some(inventory::Screen {
            book_open: true,
            ..Default::default()
        });
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture()
            .save_png("out/views/creative_inventory.png")
            .unwrap();
        // Badly hurt at night: the dark closing in.
        let mut game = new_game().unwrap();
        game.time = night;
        game.last_time_of_day = game.time_of_day();
        game.vitals.health = 3;
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        sim.step();
        sim.picture().save_png("out/views/hurt_night.png").unwrap();
    }
    // The inventory: with a log in the grid, a stack on the pointer, and
    // the recipe book open.
    for (name, book) in [("inventory", false), ("inventory_book", true)] {
        let mut game = arena(|_| {});
        game.inventory.set(Item::Block(Block::Log), 3);
        game.inventory.set(Item::Block(Block::Sand), 6);
        game.inventory.set(Item::Block(Block::Dirt), 40);
        game.inventory.set(Item::Seeds, 5);
        game.inventory.set(Item::Coal, 2);
        game.inventory.set(Item::Gold, 1);
        game.inventory.set(Item::Apple, 2);
        let mut screen = inventory::Screen::default();
        screen.grid[0] = Some(Stack::new(Item::Block(Block::Log), 1));
        screen.held = (!book).then(|| Stack::new(Item::Block(Block::Sand), 3));
        screen.book_open = book;
        game.screen = Some(screen);
        game.show_help = false;
        let mut sim = Simulation::new(config(), game);
        let mouse = inventory::spot_rect(inventory::Spot::Slot(20), book);
        sim.move_mouse(vec2(mouse.x + 9.0, mouse.y + 9.0));
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
    game.inventory.slots[0] = Some(Stack::new(Item::Block(Block::Sand), 1));
    game.slot = 0;
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
    let sand = game.count(Item::Block(Block::Sand));
    let played = PlayTest::new(config(), game).frames(60).run();
    let game = played.game();
    assert_eq!(game.count(Item::Block(Block::Sand)), sand + 1);
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
    game.inventory.set(Item::Gold, 3);
    game.inventory.set(Item::Coal, 5);
    game.inventory.set(Item::Block(Block::Log), 7);
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
    assert_eq!(loaded.inventory, game.inventory);
    assert_eq!(
        (
            loaded.count(Item::Gold),
            loaded.count(Item::Coal),
            loaded.imprints,
            loaded.slot
        ),
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
    game.inventory.set(Item::Gold, 9);
    game.save().unwrap();
    assert!(path.exists());
    let again = save::load_or_new(Some(path.clone())).unwrap();
    assert_eq!(again.count(Item::Gold), 9);
    assert_eq!(again.save_path.as_deref(), Some(path.as_path()));
    // A broken file: a new island, not a crash.
    std::fs::write(&path, b"DUCKCRAFT1 and then nonsense").unwrap();
    let fresh = save::load_or_new(Some(path.clone())).unwrap();
    assert_eq!(fresh.count(Item::Gold), 0);
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
    game.inventory.set(Item::Block(Block::Log), 3);
    game.inventory.set(Item::Block(Block::Sand), 6);
    game.inventory.set(Item::Coal, 4);
    game.inventory.set(Item::Gold, 2);
    game.inventory.set(Item::Seeds, 3);
    game.show_help = false;
    game.draw_pointer = true;
    game.vitals.food = 13;
    game.vitals.health = 16;
    let mut sim = Simulation::new(config(), game);
    sim.move_mouse(vec2(WIDTH / 2.0, HEIGHT / 2.0));
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
    let tap = |sim: &mut Simulation<DuckCraft>,
               key: Key,
               shoot: &mut dyn FnMut(&mut Simulation<DuckCraft>, usize)| {
        sim.press(key);
        shoot(sim, 1);
        sim.release(key);
        shoot(sim, 14);
    };
    shoot(&mut sim, 40);
    // Hungry: eat a loaf (hotbar slot 5).
    tap(&mut sim, Key::Digit5, &mut shoot);
    sim.press(Key::C);
    shoot(&mut sim, 80);
    sim.release(Key::C);
    shoot(&mut sim, 20);
    tap(&mut sim, Key::Digit1, &mut shoot);
    // Crafting in the inventory: open the recipe book, fill the grid from
    // it, take what it makes.
    use inventory::{Spot, spot_rect};
    let click = |sim: &mut Simulation<DuckCraft>,
                 spot: Spot,
                 shift: bool,
                 shoot: &mut dyn FnMut(&mut Simulation<DuckCraft>, usize)| {
        let book = sim.game().screen.as_ref().is_some_and(|s| s.book_open);
        let r = spot_rect(spot, book);
        let target = vec2(r.x + r.w / 2.0, r.y + r.h / 2.0);
        // Glide the pointer there, so the video shows it moving.
        let from = sim.game().mouse.unwrap_or(vec2(WIDTH / 2.0, HEIGHT / 2.0));
        for i in 1..=8 {
            sim.move_mouse(from + (target - from) * (i as f32 / 8.0));
            shoot(sim, 1);
        }
        if shift {
            sim.press(Key::LeftShift);
        }
        sim.press_mouse(MouseButton::Left);
        shoot(sim, 1);
        sim.release_mouse(MouseButton::Left);
        if shift {
            sim.release(Key::LeftShift);
        }
        shoot(sim, 10);
    };
    tap(&mut sim, Key::E, &mut shoot);
    click(&mut sim, Spot::Book, false, &mut shoot);
    click(&mut sim, Spot::Recipe(0), false, &mut shoot); // a log
    click(&mut sim, Spot::Result, true, &mut shoot); // planks, into the bag
    shoot(&mut sim, 10);
    click(&mut sim, Spot::Recipe(2), false, &mut shoot); // sand, sand, coal
    click(&mut sim, Spot::Result, false, &mut shoot); // glass, on the pointer
    click(&mut sim, Spot::Slot(16), false, &mut shoot); // put down in the bag
    click(&mut sim, Spot::Recipe(5), false, &mut shoot); // gold and glass
    click(&mut sim, Spot::Result, true, &mut shoot); // lamps
    click(&mut sim, Spot::Recipe(1), false, &mut shoot); // three seeds
    click(&mut sim, Spot::Result, true, &mut shoot); // bread
    shoot(&mut sim, 20);
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
            game.yaw += ((want - game.yaw + std::f32::consts::PI).rem_euclid(TAU)
                - std::f32::consts::PI)
                .clamp(-0.08, 0.08);
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
    eprintln!(
        "SHOWCASE {} pictures, met {met} ducklings, crafted {}",
        frame / 2,
        sim.game().crafted
    );
}

// ---- The inventory ----

/// The arena with the inventory screen open (the pointer free).
fn inventory_open(change: impl FnOnce(&mut DuckCraft)) -> DuckCraft {
    let mut game = arena(|_| {});
    game.screen = Some(inventory::Screen::default());
    change(&mut game);
    game
}

/// Clicks on spots, one every 4 frames from frame 2: (spot, "Left" or
/// "Right", Shift held?). The book is shut unless `book` says.
fn clicks(steps: &[(inventory::Spot, &str, bool)], book: bool) -> String {
    let mut events = Vec::new();
    for (n, &(spot, button, shift)) in steps.iter().enumerate() {
        let frame = 2 + n * 4;
        let r = inventory::spot_rect(spot, book);
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        events.push(format!(r#"{{"frame": {frame}, "mouse": [{x}, {y}]}}"#));
        if shift {
            events.push(format!(r#"{{"frame": {frame}, "press": "LeftShift"}}"#));
        }
        events.push(format!(
            r#"{{"frame": {}, "mouse_press": "{button}"}}"#,
            frame + 1
        ));
        events.push(format!(
            r#"{{"frame": {}, "mouse_release": "{button}"}}"#,
            frame + 2
        ));
        if shift {
            events.push(format!(
                r#"{{"frame": {}, "release": "LeftShift"}}"#,
                frame + 2
            ));
        }
    }
    format!(r#"{{"events": [{}]}}"#, events.join(", "))
}

fn run_clicks(game: DuckCraft, steps: &[(inventory::Spot, &str, bool)], book: bool) -> DuckCraft {
    let frames = 6 + steps.len() as u64 * 4;
    let played = PlayTest::new(config(), game)
        .script(&clicks(steps, book))
        .frames(frames)
        .run();
    played.game().clone()
}

#[test]
fn the_bag_fills_stacks_first_then_the_hotbar_then_the_backpack() {
    let mut bag = Inventory::empty();
    bag.slots[3] = Some(Stack::new(Item::Coal, 60));
    assert_eq!(bag.add(Item::Coal, 10), 0);
    assert_eq!(
        bag.slots[3],
        Some(Stack::new(Item::Coal, 64)),
        "the stack topped up first"
    );
    assert_eq!(
        bag.slots[0],
        Some(Stack::new(Item::Coal, 6)),
        "the rest in the first empty slot"
    );
    for i in 0..items::SLOTS {
        if bag.slots[i].is_none() {
            bag.slots[i] = Some(Stack::new(Item::Apple, 1));
        }
    }
    assert_eq!(bag.add(Item::Bread, 5), 5, "a full bag takes nothing");
    assert_eq!(bag.count(Item::Coal), 70);
    assert_eq!(bag.take(Item::Coal, 65), 65);
    assert_eq!(bag.count(Item::Coal), 5);
    // Item numbers come back as the same items (the save file).
    for item in [
        Item::Block(Block::Lamp),
        Item::Gold,
        Item::Bread,
        Item::Seeds,
    ] {
        assert_eq!(Item::from_id(item.id()), Some(item));
    }
}

#[test]
fn e_opens_the_inventory_and_lets_go_of_the_pointer_and_e_closes_it() {
    let played = PlayTest::new(config(), arena(|_| {}))
        .script(&tap("E"))
        .frames(5)
        .run();
    assert!(played.game().screen.is_some());
    let mut game = inventory_open(|g| {
        let screen = g.screen.as_mut().unwrap();
        screen.grid[2] = Some(Stack::new(Item::Coal, 3));
        screen.held = Some(Stack::new(Item::Apple, 2));
    });
    game.inventory.set(Item::Coal, 0);
    let played = PlayTest::new(config(), game)
        .script(&tap("E"))
        .frames(5)
        .run();
    let game = played.game();
    assert!(game.screen.is_none());
    assert_eq!(game.count(Item::Coal), 3, "the grid went back in the bag");
    assert_eq!(
        game.count(Item::Apple),
        2,
        "and so did the stack on the pointer"
    );
}

#[test]
fn clicks_pick_up_put_down_and_split_stacks() {
    use inventory::Spot;
    // Planks (32) are in hotbar slot 0: pick them up, put them in the backpack.
    let game = run_clicks(
        inventory_open(|_| {}),
        &[
            (Spot::Slot(0), "Left", false),
            (Spot::Slot(20), "Left", false),
        ],
        false,
    );
    assert_eq!(game.inventory.slots[0], None);
    assert_eq!(
        game.inventory.slots[20],
        Some(Stack::new(Item::Block(Block::Planks), 32))
    );
    // Right click takes half; right click on an empty slot puts down one.
    let game = run_clicks(
        inventory_open(|_| {}),
        &[
            (Spot::Slot(0), "Right", false),
            (Spot::Slot(30), "Right", false),
        ],
        false,
    );
    assert_eq!(
        game.inventory.slots[0],
        Some(Stack::new(Item::Block(Block::Planks), 16))
    );
    assert_eq!(
        game.inventory.slots[30],
        Some(Stack::new(Item::Block(Block::Planks), 1))
    );
    assert_eq!(
        game.screen.as_ref().unwrap().held,
        Some(Stack::new(Item::Block(Block::Planks), 15))
    );
    // A different stack: they swap.
    let game = run_clicks(
        inventory_open(|_| {}),
        &[
            (Spot::Slot(0), "Left", false),
            (Spot::Slot(2), "Left", false),
        ],
        false,
    );
    assert_eq!(
        game.inventory.slots[2],
        Some(Stack::new(Item::Block(Block::Planks), 32))
    );
    assert_eq!(
        game.screen.as_ref().unwrap().held,
        Some(Stack::new(Item::Block(Block::Brick), 32))
    );
}

#[test]
fn shift_click_sends_a_stack_across() {
    use inventory::Spot;
    let game = run_clicks(
        inventory_open(|_| {}),
        &[(Spot::Slot(1), "Left", true)],
        false,
    );
    assert_eq!(game.inventory.slots[1], None);
    assert_eq!(
        game.inventory.slots[9],
        Some(Stack::new(Item::Block(Block::Glass), 16)),
        "to the backpack's first slot"
    );
    let game = run_clicks(
        inventory_open(|g| {
            g.inventory.slots[30] = Some(Stack::new(Item::Apple, 5));
        }),
        &[(Spot::Slot(30), "Left", true)],
        false,
    );
    assert_eq!(
        game.inventory.slots[5],
        Some(Stack::new(Item::Apple, 5)),
        "to the hotbar's first empty slot"
    );
}

#[test]
fn a_log_in_the_grid_makes_planks() {
    use inventory::Spot;
    let game = inventory_open(|g| {
        g.inventory.slots[8] = Some(Stack::new(Item::Block(Block::Log), 2));
    });
    // Pick up the logs, put one in the grid (right click), the rest back,
    // then take the planks.
    let game = run_clicks(
        game,
        &[
            (Spot::Slot(8), "Left", false),
            (Spot::Grid(3), "Right", false),
            (Spot::Slot(8), "Left", false),
            (Spot::Result, "Left", false),
        ],
        false,
    );
    let screen = game.screen.as_ref().unwrap();
    assert_eq!(screen.held, Some(Stack::new(Item::Block(Block::Planks), 4)));
    assert_eq!(screen.grid[3], None, "the log was used");
    assert_eq!(
        game.inventory.slots[8],
        Some(Stack::new(Item::Block(Block::Log), 1))
    );
    assert_eq!(game.crafted, 4);
}

#[test]
fn the_recipe_book_fills_the_grid_and_shift_makes_all_it_can() {
    use inventory::Spot;
    let game = inventory_open(|g| {
        g.inventory.set(Item::Block(Block::Sand), 7);
        g.inventory.set(Item::Coal, 2);
        g.screen.as_mut().unwrap().book_open = true;
    });
    let glass = game.count(Item::Block(Block::Glass));
    // Sand, sand and coal (recipe 2) into the grid, then make glass.
    let game = run_clicks(
        game,
        &[
            (Spot::Recipe(2), "Left", false),
            (Spot::Result, "Left", true),
        ],
        true,
    );
    assert_eq!(
        game.count(Item::Block(Block::Glass)),
        glass + 2,
        "one lot: the grid had one of each"
    );
    // Again, keeping the grid's leftovers: fill it with two of each and
    // shift-click: two lots.
    let mut game = game;
    game.screen.as_mut().unwrap().grid = [
        Some(Stack::new(Item::Block(Block::Sand), 2)),
        Some(Stack::new(Item::Block(Block::Sand), 2)),
        Some(Stack::new(Item::Coal, 2)),
        None,
    ];
    let glass = game.count(Item::Block(Block::Glass));
    let game = run_clicks(game, &[(Spot::Result, "Left", true)], true);
    assert_eq!(game.count(Item::Block(Block::Glass)), glass + 4);
    assert!(
        game.screen
            .as_ref()
            .unwrap()
            .grid
            .iter()
            .all(|s| s.is_none())
    );
}

#[test]
fn recipes_match_in_any_order_and_only_exactly() {
    use inventory::matching_recipe;
    let s = |item| Some(Stack::new(item, 1));
    let sand = Item::Block(Block::Sand);
    let grid = [s(Item::Coal), None, s(sand), s(sand)];
    assert_eq!(
        matching_recipe(&grid).map(|r| r.makes.0),
        Some(Item::Block(Block::Glass))
    );
    let grid = [s(Item::Coal), s(sand), s(sand), s(sand)];
    assert!(
        matching_recipe(&grid).is_none(),
        "an extra sand matches nothing"
    );
    let grid = [s(Item::Seeds), s(Item::Seeds), s(Item::Seeds), None];
    assert_eq!(
        matching_recipe(&grid).map(|r| r.makes),
        Some((Item::Bread, 1))
    );
}

// ---- Health, hunger and air ----

/// Runs the game for `seconds` with nothing pressed.
fn wait(game: DuckCraft, seconds: f32) -> DuckCraft {
    let played = PlayTest::new(config(), game)
        .frames((seconds * 60.0) as u64)
        .run();
    played.game().clone()
}

#[test]
fn a_long_fall_hurts_and_a_short_one_doesnt() {
    // Six blocks: three past the safe three, so three points (a heart and a half).
    let mut game = arena(|_| {});
    game.position.y = FLOOR as f32 + 6.0;
    let game = wait(game, 2.0);
    assert_eq!(game.vitals.health, 17);
    assert!(game.on_ground);
    let mut game = arena(|_| {});
    game.position.y = FLOOR as f32 + 3.0;
    assert_eq!(wait(game, 2.0).vitals.health, 20);
}

#[test]
fn falling_into_water_doesnt_hurt() {
    let mut game = arena(|w| {
        for y in FLOOR - 4..FLOOR {
            w.set(8, y, 8, Block::Water);
        }
    });
    game.position.y = FLOOR as f32 + 8.0;
    let game = wait(game, 3.0);
    assert_eq!(game.vitals.health, 20);
}

#[test]
fn under_water_air_runs_out_then_it_hurts() {
    // A deep pool: the duck at its bottom.
    let mut game = arena(|w| {
        for z in 6..11 {
            for x in 6..11 {
                for y in FLOOR - 6..FLOOR + 1 {
                    w.set(x, y, z, Block::Water);
                }
            }
        }
    });
    game.position.y = FLOOR as f32 - 6.0;
    let game = wait(game, 10.0);
    assert!(
        game.vitals.air < health::MAX_AIR - 9.0,
        "air going: {}",
        game.vitals.air
    );
    assert_eq!(game.vitals.health, 20, "not yet");
    let game = wait(game, 8.0);
    assert_eq!(game.vitals.air, 0.0);
    assert!(game.vitals.health < 20, "out of air, it hurts");
}

#[test]
fn walking_makes_you_hungry() {
    let mut game = arena(|_| {});
    game.vitals.tiredness = 0.99;
    let played = PlayTest::new(config(), game)
        .script(&hold(&["W"], 60))
        .frames(60)
        .run();
    assert_eq!(played.number("food"), 19.0);
}

#[test]
fn eating_bread_fills_you_up() {
    let mut game = arena(|_| {});
    game.vitals.food = 10;
    game.slot = 4; // the starting kit's bread
    assert_eq!(game.holding().map(|s| s.item), Some(Item::Bread));
    let played = PlayTest::new(config(), game)
        .script(&hold(&["C"], 80))
        .frames(80)
        .run();
    let game = played.game();
    assert_eq!(game.vitals.food, 16);
    assert_eq!(game.count(Item::Bread), 2);
    assert_eq!(game.placed, 0, "bread isn't a block");
    assert!(!played.sound_frames("munch").is_empty());
    assert_eq!(played.sound_frames("gulp").len(), 1);
}

#[test]
fn a_full_duck_cant_eat() {
    let mut game = arena(|_| {});
    game.slot = 4;
    let played = PlayTest::new(config(), game)
        .script(&hold(&["C"], 100))
        .frames(100)
        .run();
    assert_eq!(played.game().count(Item::Bread), 3);
    assert_eq!(played.text("message"), "You're full");
}

#[test]
fn well_fed_heals_and_starving_hurts_but_not_to_fainting() {
    let mut game = arena(|_| {});
    game.vitals.health = 15;
    let game = wait(game, 4.5);
    assert_eq!(game.vitals.health, 16);
    let mut game = arena(|_| {});
    game.vitals.health = 3;
    game.vitals.food = 0;
    let game = wait(game, 30.0);
    assert_eq!(game.vitals.health, 1, "starving stops at half a heart");
    assert!(!game.vitals.fainted);
}

#[test]
fn fainting_and_waking_at_the_spawn() {
    let mut game = arena(|_| {});
    game.vitals.health = 2;
    game.position.y = FLOOR as f32 + 9.0;
    let planks = game.count(Item::Block(Block::Planks));
    let game = wait(game, 2.0);
    assert!(game.vitals.fainted);
    assert_eq!(game.fainted_because, "fell too far");
    let played = PlayTest::new(config(), game)
        .script(&tap("Space"))
        .frames(6)
        .run();
    let game = played.game();
    assert!(!game.vitals.fainted);
    assert_eq!(game.vitals.health, health::MAX_HEALTH);
    assert!(
        game.position.distance(game.world.spawn) < 0.01,
        "back at the spawn"
    );
    assert_eq!(
        game.count(Item::Block(Block::Planks)),
        planks,
        "you keep your things"
    );
    assert_eq!(game.vitals.faints, 1);
}

// ---- Plants ----

#[test]
fn the_island_grows_tall_grass_and_flowers_on_grass() {
    let world = World::generate(SEED);
    let count = |kind| world.plants.values().filter(|&&p| p == kind).count();
    assert!(
        count(Plant::TallGrass) > 200,
        "{} tall grass",
        count(Plant::TallGrass)
    );
    for flower in [Plant::YellowFlower, Plant::RedFlower, Plant::WhiteFlower] {
        assert!(count(flower) > 0, "no {flower:?}");
    }
    for &[x, y, z] in world.plants.keys() {
        assert_eq!(world.get(x, y, z), Block::Air);
        assert!(plants::grows_on(world.get(x, y - 1, z)));
    }
}

/// The arena with a plant two blocks ahead, the duck looking down at it.
fn plant_ahead(plant: Plant) -> DuckCraft {
    let mut game = arena(|w| {
        w.plants.insert([8, FLOOR, 10], plant);
        w.set(8, FLOOR - 1, 10, Block::Grass);
    });
    game.pitch = -0.6;
    game
}

#[test]
fn a_plant_breaks_at_once_and_you_walk_through_plants() {
    let played = PlayTest::new(config(), plant_ahead(Plant::RedFlower))
        .script(&tap("X"))
        .frames(5)
        .run();
    assert_eq!(played.number("picked"), 1.0);
    assert!(played.game().world.plant(8, FLOOR, 10).is_none());
    assert_eq!(played.sound_frames("snip").len(), 1);
    // Walking: straight through tall grass.
    let mut game = arena(|w| {
        for z in 9..13 {
            w.plants.insert([8, FLOOR, z], Plant::TallGrass);
        }
    });
    game.pitch = 0.0;
    let played = PlayTest::new(config(), game)
        .script(&hold(&["W"], 60))
        .frames(60)
        .run();
    assert!(played.number("z") > 12.0);
}

#[test]
fn a_block_placed_on_a_plant_takes_its_place() {
    let played = PlayTest::new(config(), plant_ahead(Plant::TallGrass))
        .script(&tap("C"))
        .frames(5)
        .run();
    let world = &played.game().world;
    assert_eq!(world.get(8, FLOOR, 10), Block::Planks);
    assert!(world.plant(8, FLOOR, 10).is_none());
}

#[test]
fn digging_the_grass_under_a_plant_takes_the_plant_too() {
    let mut game = plant_ahead(Plant::YellowFlower);
    game.world.set(8, FLOOR - 1, 10, Block::Air);
    assert!(game.world.plant(8, FLOOR, 10).is_none());
    assert!(game.world.plants_changed);
}

#[test]
fn saved_plants_stay_as_they_were() {
    let mut game = new_game().unwrap();
    let &[x, y, z] = game.world.plants.keys().next().unwrap();
    game.world.remove_plant(x, y, z);
    let plants = game.world.plants.len();
    let loaded = DuckCraft::from_bytes(Rc::new(Art::load().unwrap()), &game.to_bytes()).unwrap();
    assert_eq!(loaded.world.plants.len(), plants);
    assert!(
        loaded.world.plant(x, y, z).is_none(),
        "the picked one didn't grow back"
    );
}

/// Random clicks all over the inventory screen (the fuzzer only presses
/// keys): nothing is ever lost or made from nothing except by crafting,
/// and every stack holds 1 to 64.
#[test]
fn random_clicks_never_lose_or_copy_things() {
    use inventory::{RECIPES, Spot};
    let mut game = inventory_open(|g| {
        g.inventory.set(Item::Block(Block::Log), 9);
        g.inventory.set(Item::Block(Block::Sand), 70);
        g.inventory.set(Item::Coal, 12);
        g.inventory.set(Item::Gold, 3);
        g.inventory.set(Item::Seeds, 10);
        g.inventory.set(Item::Block(Block::Dirt), 64);
    });
    let mut seed: u64 = 12345;
    let mut random = |n: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n
    };
    // Everything there is, wherever it is.
    let total = |g: &DuckCraft| {
        let mut all: Vec<Stack> = g.inventory.slots.iter().flatten().copied().collect();
        let screen = g.screen.as_ref().unwrap();
        all.extend(screen.grid.iter().flatten().copied());
        all.extend(screen.held);
        let mut counts: Vec<(u8, u32)> = Vec::new();
        for stack in &all {
            assert!(
                (1..=stack.item.max_stack()).contains(&stack.count),
                "a stack of {}",
                stack.count
            );
            match counts.iter_mut().find(|(id, _)| *id == stack.item.id()) {
                Some((_, n)) => *n += stack.count,
                None => counts.push((stack.item.id(), stack.count)),
            }
        }
        counts.sort();
        counts
    };
    let mut before = total(&game);
    for _ in 0..5000 {
        let spot = match random(10) {
            0 => Spot::Grid(random(4)),
            1 => Spot::Result,
            2 => Spot::Recipe(random(RECIPES.len())),
            3 => Spot::Book,
            _ => Spot::Slot(random(items::SLOTS)),
        };
        let crafting = matches!(spot, Spot::Result);
        game.click_spot(spot, random(3) > 0, random(4) == 0);
        let after = total(&game);
        if !crafting {
            assert_eq!(before, after, "a click on {spot:?} changed what there is");
        }
        before = after;
    }
    // Closing puts everything back in the bag, and nothing goes missing.
    let everything = total(&game);
    game.close_inventory();
    game.screen = Some(inventory::Screen::default());
    assert_eq!(total(&game), everything);
}

// ---- Menus and modes ----

/// The time (in seconds) at a time of day (0 sunrise, 0.5 sunset, 0.75 midnight).
fn at_time_of_day(tod: f32) -> f32 {
    (tod - menus::DAY_START).rem_euclid(1.0) * DAY_SECONDS
}

#[test]
fn the_game_opens_on_the_title_and_enter_starts_a_survival_island() {
    let played = PlayTest::new(config(), title_game(None).unwrap())
        .frames(30)
        .run();
    assert_eq!(played.text("stage"), "title 0");
    assert_eq!(played.game().time, 0.0, "the island waits on the title");
    let headless = title_game(None).unwrap();
    assert!(
        !headless.title_choices().contains(&menus::TitleChoice::Quit),
        "no quitting a headless run from a menu"
    );
    let played = PlayTest::new(config(), title_game(None).unwrap())
        .script(&tap("Enter"))
        .frames(10)
        .run();
    assert_eq!(played.text("stage"), "playing");
    assert_eq!(played.text("mode"), "Survival");
    assert_eq!(
        played.game().world.seed,
        SEED,
        "headless islands are always the same"
    );
}

#[test]
fn the_second_choice_is_a_creative_island() {
    let script = r#"{"events": [
        {"frame": 2, "press": "S"}, {"frame": 3, "release": "S"},
        {"frame": 5, "press": "Enter"}, {"frame": 6, "release": "Enter"}
    ], "frames": 12}"#;
    let played = PlayTest::new(config(), title_game(None).unwrap())
        .script(script)
        .run();
    let game = played.game();
    assert_eq!(game.mode, Mode::Creative);
    assert_eq!(
        game.holding().map(|s| s.item),
        Some(Item::Block(Block::Grass))
    );
}

#[test]
fn a_new_island_over_a_saved_one_asks_first() {
    let mut game = title_game(None).unwrap();
    game.has_save = true; // as if there were one
    game.time = 500.0;
    // "Carry on" is first; the second is a new survival island.
    let script = r#"{"events": [
        {"frame": 2, "press": "S"}, {"frame": 3, "release": "S"},
        {"frame": 5, "press": "Enter"}, {"frame": 6, "release": "Enter"}
    ], "frames": 10}"#;
    let played = PlayTest::new(config(), game).script(script).run();
    let game = played.game().clone();
    assert!(
        matches!(game.stage, Stage::Title { confirm: true, .. }),
        "it asks"
    );
    assert_eq!(game.time, 500.0);
    let played = PlayTest::new(config(), game)
        .script(&tap("Enter"))
        .frames(6)
        .run();
    let game = played.game();
    assert_eq!(game.stage, Stage::Playing);
    assert!(game.time < 1.0, "a fresh island");
}

#[test]
fn the_pause_menu_switches_mode_and_music() {
    let mut game = arena(|_| {});
    game.stage = Stage::Paused { selected: 0 };
    let script = r#"{"events": [
        {"frame": 2, "press": "S"}, {"frame": 3, "release": "S"},
        {"frame": 4, "press": "Enter"}, {"frame": 5, "release": "Enter"},
        {"frame": 6, "press": "S"}, {"frame": 7, "release": "S"},
        {"frame": 8, "press": "Enter"}, {"frame": 9, "release": "Enter"},
        {"frame": 10, "press": "Escape"}, {"frame": 11, "release": "Escape"}
    ], "frames": 14}"#;
    let played = PlayTest::new(config(), game).script(script).run();
    let game = played.game();
    assert_eq!(game.mode, Mode::Creative);
    assert!(!game.music_on);
    assert_eq!(game.stage, Stage::Playing, "Escape went back to the island");
}

#[test]
fn music_starts_and_fades_with_the_light() {
    let played = PlayTest::new(config(), arena(|_| {})).frames(240).run();
    let loops = played.loops();
    let volume = |name: &str| {
        loops
            .iter()
            .find(|l| l.name == name)
            .and_then(|l| l.volumes.last())
            .map_or(0.0, |&(_, v)| v)
    };
    assert!(
        volume("music_day") > 0.3,
        "morning: the day's music, {}",
        volume("music_day")
    );
    assert!(volume("music_night") < 0.05);
    let mut game = arena(|_| {});
    game.time = at_time_of_day(0.75);
    let played = PlayTest::new(config(), game).frames(240).run();
    let loops = played.loops();
    let night = loops
        .iter()
        .find(|l| l.name == "music_night")
        .unwrap()
        .volumes
        .last()
        .unwrap()
        .1;
    assert!(night > 0.4, "midnight: the night's music, {night}");
}

#[test]
fn dusk_rings_a_bell_and_says_a_line_and_dawn_counts_the_day() {
    let mut game = arena(|_| {});
    game.time = at_time_of_day(0.499);
    game.last_time_of_day = 0.499;
    let played = PlayTest::new(config(), game).frames(60).run();
    assert_eq!(played.sound_frames("bell_dusk").len(), 1);
    assert_eq!(played.game().announcement.as_ref().unwrap().0, "Night");
    let mut game = arena(|_| {});
    game.time = DAY_SECONDS * 2.0 - menus::DAY_START * DAY_SECONDS - 0.2; // just before the third sunrise
    game.last_time_of_day = game.time_of_day();
    let played = PlayTest::new(config(), game).frames(60).run();
    assert_eq!(played.sound_frames("bell_dawn").len(), 1);
    assert_eq!(played.game().announcement.as_ref().unwrap().0, "Day 3");
}

// ---- Creative ----

fn creative(change: impl FnOnce(&mut World)) -> DuckCraft {
    let mut game = arena(change);
    game.set_mode(Mode::Creative);
    game
}

#[test]
fn creative_flies_on_a_double_space_and_lands_on_the_ground() {
    let script = r#"{"events": [
        {"frame": 2, "press": "Space"}, {"frame": 3, "release": "Space"},
        {"frame": 6, "press": "Space"}, {"frame": 50, "release": "Space"}
    ], "frames": 120}"#;
    let played = PlayTest::new(config(), creative(|_| {}))
        .script(script)
        .run();
    let game = played.game();
    assert!(game.flying, "still up there");
    assert!(game.position.y > FLOOR as f32 + 3.0, "{}", game.position.y);
    let script = r#"{"events": [{"frame": 2, "press": "LeftShift"}], "frames": 200}"#;
    let played = PlayTest::new(config(), game.clone()).script(script).run();
    assert!(
        !played.game().flying,
        "Shift brought it down, and the ground ended the flight"
    );
    assert_eq!(
        played.game().vitals.health,
        20,
        "no fall damage in creative"
    );
}

#[test]
fn creative_breaks_at_once_drops_nothing_and_never_runs_out() {
    let game = creative(|w| w.set(8, FLOOR + 1, 10, Block::Stone));
    let cobble = game.count(Item::Block(Block::Cobblestone));
    let played = PlayTest::new(config(), game)
        .script(&tap("X"))
        .frames(5)
        .run();
    let game = played.game();
    assert_eq!(game.world.get(8, FLOOR + 1, 10), Block::Air);
    assert_eq!(game.count(Item::Block(Block::Cobblestone)), cobble);
    let mut game = game.clone();
    game.pitch = -0.6;
    let grass = game.count(Item::Block(Block::Grass));
    let played = PlayTest::new(config(), game)
        .script(&tap("C"))
        .frames(5)
        .run();
    assert_eq!(played.number("placed"), 1.0);
    assert_eq!(played.game().count(Item::Block(Block::Grass)), grass);
}

#[test]
fn the_creative_panel_hands_out_full_stacks_and_bins_what_you_hold() {
    use inventory::Spot;
    let mut game = creative(|_| {});
    game.screen = Some(inventory::Screen {
        book_open: true,
        ..Default::default()
    });
    let blade = Item::all().iter().position(|&i| i == Item::Blade).unwrap();
    let gold = Item::all().iter().position(|&i| i == Item::Gold).unwrap();
    game.click_spot(Spot::Palette(gold), true, false);
    assert_eq!(
        game.screen.as_ref().unwrap().held,
        Some(Stack::new(Item::Gold, 64))
    );
    game.click_spot(Spot::Palette(blade), true, false);
    assert_eq!(game.screen.as_ref().unwrap().held, None, "binned");
    game.click_spot(Spot::Palette(blade), true, true);
    assert_eq!(
        game.count(Item::Blade),
        1,
        "shift: into the bag, one (blades don't stack)"
    );
}

// ---- The night's shapes ----

/// The arena at midnight, with a Hollow `ahead` blocks in front of the duck.
fn hollow_ahead(ahead: f32) -> DuckCraft {
    let mut game = arena(|_| {});
    game.time = at_time_of_day(0.75);
    game.last_time_of_day = game.time_of_day();
    game.spawn_wait = 1000.0; // no others
    game.hollows = vec![Hollow::new(vec3(8.5, FLOOR as f32, 8.5 + ahead), PI)];
    game
}

#[test]
fn a_hollow_walks_up_and_hits_hard() {
    let played = PlayTest::new(config(), hollow_ahead(5.0)).frames(240).run();
    let game = played.game();
    assert!(
        game.vitals.health <= 20 - night::HOLLOW_DAMAGE,
        "health {}",
        game.vitals.health
    );
    assert!(!played.sound_frames("hurt").is_empty());
}

#[test]
fn a_hollow_wont_come_near_a_lamp() {
    let mut game = hollow_ahead(9.0);
    game.world.set(8, FLOOR, 9, Block::Lamp);
    game.update_chunks();
    let played = PlayTest::new(config(), game).frames(360).run();
    let game = played.game();
    assert_eq!(game.vitals.health, 20);
    let hollow = &game.hollows[0];
    let lamp = vec2(8.5, 9.5);
    assert!(vec2(hollow.position.x, hollow.position.z).distance(lamp) > night::LAMP_FEAR - 0.8);
}

#[test]
fn a_hollow_comes_apart_in_the_morning_but_not_in_a_cave() {
    let mut game = hollow_ahead(6.0);
    game.time = 0.0; // morning
    let played = PlayTest::new(config(), game).frames(150).run();
    assert!(played.game().hollows.is_empty(), "gone to ash");
    // Under a roof, it stays.
    let mut game = hollow_ahead(6.0);
    game.time = 0.0;
    for z in 12..17 {
        for x in 6..11 {
            game.world.set(x, FLOOR + 3, z, Block::Stone);
        }
    }
    let played = PlayTest::new(config(), game).frames(150).run();
    assert_eq!(played.game().hollows.len(), 1);
}

#[test]
fn two_blows_of_a_blade_or_six_of_wings_put_a_hollow_down() {
    for (blade, frames) in [(true, 60), (false, 300)] {
        let mut game = hollow_ahead(2.2);
        game.hollows[0].position.y = FLOOR as f32;
        game.vitals.health = 20;
        if blade {
            game.inventory.slots[0] = Some(Stack::new(Item::Blade, 1));
        }
        let played = PlayTest::new(config(), game)
            .script(&hold(&["X"], frames))
            .frames(frames)
            .run();
        assert_eq!(played.number("kills"), 1.0, "blade {blade}");
        assert!(!played.sound_frames("hollow_die").is_empty());
    }
}

#[test]
fn in_creative_hollows_leave_you_alone() {
    let mut game = hollow_ahead(3.0);
    game.set_mode(Mode::Creative);
    let played = PlayTest::new(config(), game).frames(240).run();
    assert_eq!(played.game().vitals.health, 20);
    assert!(!played.game().hollows[0].chasing);
}

#[test]
fn hollows_come_up_in_the_dark_but_not_by_day() {
    let mut game = new_game().unwrap();
    game.time = at_time_of_day(0.75);
    game.last_time_of_day = game.time_of_day();
    game.spawn_wait = 0.0;
    let played = PlayTest::new(config(), game).frames(600).run();
    assert!(
        played.number("hollows") >= 2.0,
        "{}",
        played.number("hollows")
    );
    let mut game = new_game().unwrap();
    game.time = at_time_of_day(0.2); // noon
    let played = PlayTest::new(config(), game).frames(600).run();
    assert_eq!(played.number("hollows"), 0.0);
}

#[test]
fn the_watcher_goes_when_you_look_at_it() {
    let mut game = arena(|_| {});
    game.time = at_time_of_day(0.75);
    game.last_time_of_day = game.time_of_day();
    game.spawn_wait = 1000.0;
    game.watcher_night = game.day_number(); // no other one
    let at = vec3(8.5, FLOOR as f32, 8.5 + 26.0);
    game.watcher = Some(Watcher::new(at, PI));
    // Off to the side: it stays.
    game.yaw = 0.9;
    let played = PlayTest::new(config(), game.clone()).frames(60).run();
    assert!(played.game().watcher.is_some());
    // Straight at its head: gone, with a sting.
    let head = at + vec3(0.0, 1.6 * night::WATCHER_SIZE, 0.0) - game.eye();
    game.yaw = head.x.atan2(head.z);
    game.pitch = (head.y / vec2(head.x, head.z).length()).atan();
    let played = PlayTest::new(config(), game).frames(30).run();
    assert!(played.game().watcher.is_none());
    assert_eq!(played.sound_frames("sting").len(), 1);
}

// ---- Bugs found in review (each was real) ----

#[test]
fn a_full_bag_never_destroys_what_is_in_the_grid() {
    use inventory::Spot;
    let mut game = inventory_open(|g| {
        for i in 0..items::SLOTS {
            g.inventory.slots[i] = Some(Stack::new(Item::Block(Block::Brick), 64));
        }
        g.screen.as_mut().unwrap().grid[0] = Some(Stack::new(Item::Block(Block::Log), 61));
        g.screen.as_mut().unwrap().book_open = true;
    });
    game.click_spot(Spot::Recipe(2), true, false);
    assert_eq!(
        game.screen.as_ref().unwrap().grid[0],
        Some(Stack::new(Item::Block(Block::Log), 61)),
        "not thrown away"
    );
    game.close_inventory();
    assert_eq!(
        game.kept_grid[0],
        Some(Stack::new(Item::Block(Block::Log), 61)),
        "kept for next time"
    );
    game.screen = Some(inventory::Screen {
        grid: std::mem::take(&mut game.kept_grid),
        ..Default::default()
    });
    assert_eq!(game.screen.as_ref().unwrap().grid[0].unwrap().count, 61);
}

#[test]
fn waking_up_never_puts_the_duck_inside_blocks_built_at_the_spawn() {
    let mut game = new_game().unwrap();
    let s = game.world.spawn;
    for dy in 0..3 {
        for dz in -1..=1 {
            for dx in -1..=1 {
                game.world.set(
                    s.x as i32 + dx,
                    s.y as i32 + dy,
                    s.z as i32 + dz,
                    Block::Brick,
                );
            }
        }
    }
    game.position = s + vec3(0.0, 3.0, 0.0); // standing on top of the pile
    game.vitals.health = 0;
    game.vitals.fainted = true;
    let played = PlayTest::new(config(), game)
        .script(&tap("Space"))
        .frames(10)
        .run();
    let game = played.game();
    assert!(!game.vitals.fainted);
    assert!(
        !game.body_hits(game.position),
        "standing somewhere it fits: {:?}",
        game.position
    );
}

#[test]
fn saving_keeps_what_is_on_the_pointer_and_in_the_grid_and_falling_sand() {
    let mut game = inventory_open(|g| {
        g.inventory.set(Item::Gold, 0);
        g.inventory.set(Item::Coal, 0);
        let screen = g.screen.as_mut().unwrap();
        screen.held = Some(Stack::new(Item::Gold, 10));
        screen.grid[1] = Some(Stack::new(Item::Coal, 5));
    });
    game.falling.push(FallingBlock {
        x: 3,
        z: 3,
        y: FLOOR as f32 + 4.3,
        speed: 2.0,
        block: Block::Sand,
    });
    let loaded = DuckCraft::from_bytes(game.art.clone(), &game.to_bytes()).unwrap();
    assert_eq!(loaded.count(Item::Gold), 10);
    assert_eq!(loaded.count(Item::Coal), 5);
    assert_eq!(
        loaded.world.get(3, FLOOR + 4, 3),
        Block::Sand,
        "back in the world"
    );
    assert!(
        loaded.world.unsettled.contains(&[3, FLOOR + 4, 3]),
        "and it falls again"
    );
}

#[test]
fn a_save_made_while_fainted_wakes_at_the_spawn() {
    let mut game = new_game().unwrap();
    game.position = vec3(20.5, 30.0, 20.5);
    game.vitals.health = 0;
    game.vitals.fainted = true;
    let loaded = DuckCraft::from_bytes(game.art.clone(), &game.to_bytes()).unwrap();
    assert_eq!(loaded.vitals.health, health::MAX_HEALTH);
    assert!(loaded.position.distance(loaded.world.spawn) < 0.01);
}

#[test]
fn an_impossible_place_in_a_save_starts_at_the_spawn() {
    let mut game = new_game().unwrap();
    game.position = vec3(8.5, 1.0e9, 8.5);
    let loaded = DuckCraft::from_bytes(game.art.clone(), &game.to_bytes()).unwrap();
    assert!(loaded.position.distance(loaded.world.spawn) < 0.01);
}

#[test]
fn sand_falling_under_the_sea_leaves_water_not_air() {
    let mut game = arena(|w| {
        w.set(4, FLOOR + 2, 4, Block::Water);
        w.set(4, FLOOR + 1, 4, Block::Sand);
        w.set(4, FLOOR, 4, Block::Dirt);
    });
    game.world.unsettled.clear();
    game.world.set(4, FLOOR, 4, Block::Air);
    let game = wait(game, 1.0);
    assert_eq!(game.world.get(4, FLOOR, 4), Block::Sand);
    assert_eq!(game.world.get(4, FLOOR + 1, 4), Block::Water);
}

#[test]
fn changing_slot_mid_bite_starts_a_new_bite() {
    let mut game = arena(|_| {});
    game.vitals.food = 10;
    game.slot = 4; // bread
    game.inventory.slots[5] = Some(Stack::new(Item::Apple, 3));
    // Eat bread for 1.1 s (not quite a bite), then switch to the apple.
    let script = r#"{"events": [
        {"frame": 1, "press": "C"},
        {"frame": 66, "press": "Digit6"}, {"frame": 67, "release": "Digit6"}
    ], "frames": 80}"#;
    let played = PlayTest::new(config(), game).script(script).run();
    assert_eq!(
        played.game().count(Item::Apple),
        3,
        "no apple eaten 0.1 s after switching"
    );
}

#[test]
fn the_watcher_comes_on_some_nights_and_finds_somewhere_to_stand() {
    // On the test island it comes on the first night: from most ways the
    // duck can face, it finds ground at the edge of the view.
    let mut found = 0;
    for i in 0..16 {
        let mut game = new_game().unwrap();
        game.time = at_time_of_day(0.7);
        game.last_time_of_day = game.time_of_day();
        game.spawn_wait = 1000.0;
        game.yaw = i as f32 * TAU / 16.0;
        game.pitch = -0.6; // looking at the ground, not at it
        let mut sim = Simulation::new(config(), game);
        sim.step();
        found += sim.game().watcher.is_some() as i32;
    }
    assert!(
        found >= 8,
        "it found somewhere to stand {found} times in 16"
    );
}
