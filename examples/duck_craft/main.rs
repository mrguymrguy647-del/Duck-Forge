//! Duck Craft: a little Minecraft-style world of blocks, made with
//! duckforge and Duck Quill. Nothing lasts. Build anyway.
//!
//! An island grows from noise: grass hills, sandy beaches, the sea, trees,
//! stone with coal and gold inside, and caves. Walk it as the duck, dig any
//! block, pick it up, and build with it. Keep it in your bag and craft it
//! into new things (coal and gold are worth finding). Mind your hearts
//! and your hunger: eat apples, seeds and bread. Tall grass and flowers
//! grow on the hills; ducklings waddle about, and follow you once you've
//! met. Sand falls. The sun crosses the sky; at night the dark comes in
//! close, and grey things that were ducks once come up out of it
//! (`night.rs`): keep a lamp lit, or meet them with a stone blade. Some
//! nights something stands far off and watches. Play in survival, or in
//! creative (fly, every block, nothing hurts), from the title screen
//! (`menus.rs`). The island is saved as you play.
//!
//!     cargo run --release --example duck_craft
//!
//! - **W A S D** walk, **Shift** run, **Space** jump (and swim up)
//! - **mouse** or **arrow keys** look (the window holds on to the pointer;
//!   **Escape** lets go of it and a click takes it back)
//! - **left click** or **X** (hold) digs the block you look at;
//!   **right click** or **C** places the block in your hand
//! - **1 to 9** or the **mouse wheel** pick a block; **middle click** picks the one you look at
//! - **E** the inventory (`inventory.rs`): your duck, a 2 x 2 crafting
//!   grid, the backpack and the hotbar, worked with the mouse (click,
//!   right click to split, Shift+click to send across); the book lists the
//!   recipes and fills the grid for you. Items and stacks: `items.rs`.
//! - **hold right click** (or C) with food in hand to eat it; hearts,
//!   bread and bubbles over the hotbar show health, hunger and air
//!   (`health.rs`)
//! - tall grass and flowers break with one click; grass sometimes gives
//!   seeds, leaves sometimes an apple (`plants.rs`)
//! - **right click** a duckling and it follows you; again, and it waits
//!   (`ducklings.rs`)
//! - **F5** saves (it also saves every 30 seconds and when you quit, to
//!   `duck_craft_save.dat`; delete that file for a new island: `save.rs`)
//! - **V** first person / behind the duck, **H** hides the help, **Escape** twice quits
//!
//! How it's made:
//!
//! - The world is a grid of blocks (`World`), 64 x 44 x 64, grown by
//!   `World::generate` from value noise with a fixed seed.
//! - The pictures are one Duck Quill atlas (`art/blocks.json`, written by
//!   `art/make_blocks.py`). The engine's `BlockWorld` holds the blocks and
//!   draws each 16 x 16 column of the world as one mesh (one draw), built
//!   face by face: only faces next to air (or water, glass, leaves) are
//!   kept, so a chunk is a few thousand triangles, not hundreds of
//!   thousands. Digging or building rebuilds only that chunk.
//! - Faces in caves and under overhangs are drawn darker: a cheap "no sky
//!   above" shadow. Lamps glow, and so do the faces round them (a glow
//!   level per face), so they light up the night. Far away, everything
//!   fades into the horizon's colour (fog).
//! - The duck is a blocky character with a skin, like a Minecraft player
//!   (`Duck`, `art/make_skin.py`): boxes for the head, beak, body, arms and
//!   legs, each face painted from one 64 x 32 picture, limbs swinging as it
//!   walks. In first person you see your own arm holding the block in hand.
//!   The ducklings are built the same way from their own skin
//!   (`art/make_duckling.py`).
//!
//! - The music and sounds are made from scratch by `audio/make_sounds.py`
//!   (sine waves, noise and a little reverb; Duck Echo isn't ready yet):
//!   a felt piano for the day, a drone for the night, a theme for the
//!   title, and the Hollows' groans, the Watcher's sting, a heartbeat.
//!
//! Without a screen (or to test):
//!
//!     cargo run --release --no-default-features --example duck_craft -- --headless --inputs playthroughs/duck_craft.json --sheet-events --out out
//!     cargo test --no-default-features --example duck_craft
//!
//! The night, from a recording (`--set start_time_of_day=0.47` starts the
//! first day just before sunset):
//!
//!     cargo run --release --no-default-features --example duck_craft -- --headless --set start_time_of_day=0.47 --inputs playthroughs/duck_craft_night.json --video out/night.mp4 --out out/night

use duckforge::LoadError;
use duckforge::prelude::*;
use std::f32::consts::TAU;
use std::rc::Rc;

// ---- Settings: change these first ----

const WIDTH: f32 = 320.0; // the screen, in pixels
const HEIGHT: f32 = 240.0;
/// The world: SIZE x TALL x SIZE blocks.
const SIZE: usize = 64;
const TALL: usize = 44;
const CHUNK: usize = 16; // chunks are 16 x 16 columns, the world's full height
const SEA: i32 = 12; // the top of the sea is the top of this layer
/// The island tests, replays and fuzz runs always grow. A new island in
/// the window gets a seed from the clock.
const SEED: u32 = 2026;

const WALK_SPEED: f32 = 4.3; // blocks per second
const RUN_SPEED: f32 = 6.0;
const FLY_SPEED: f32 = 10.0; // creative
const GRAVITY: f32 = 28.0;
const JUMP_SPEED: f32 = 8.6; // a little over one block high
const HALF_WIDTH: f32 = 0.3; // the duck is 0.6 wide...
const BODY_HEIGHT: f32 = 1.8; // ...and 1.8 tall,
const EYE_HEIGHT: f32 = 1.62; // with its eyes here.
const REACH: f32 = 5.0; // how far away you can dig and build
const TURN_SPEED: f32 = 2.4; // arrow keys, radians per second
const MOUSE_LOOK: f32 = 0.012; // radians per canvas pixel
const DAY_SECONDS: f32 = 420.0; // a whole day and night
const PLACE_REPEAT: f32 = 0.25; // seconds between blocks while held
const APPLE_CHANCE: f32 = 0.1; // of an apple, each time you break leaves

const ASSETS: &str = "examples/duck_craft/assets";
const HOTBAR_SLOT: f32 = 20.0; // pixels a slot, in the hotbar and the inventory

// ---- Blocks ----

/// One kind of block. `Air` is nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Block {
    Air,
    Grass,
    Dirt,
    Stone,
    Cobblestone,
    Sand,
    Log,
    Leaves,
    Planks,
    Glass,
    Brick,
    CoalOre,
    GoldOre,
    Water,
    Bedrock,
    Lamp,
}

const BLOCK_KINDS: usize = 16;

impl Block {
    const ALL: [Block; BLOCK_KINDS] = [
        Block::Air,
        Block::Grass,
        Block::Dirt,
        Block::Stone,
        Block::Cobblestone,
        Block::Sand,
        Block::Log,
        Block::Leaves,
        Block::Planks,
        Block::Glass,
        Block::Brick,
        Block::CoalOre,
        Block::GoldOre,
        Block::Water,
        Block::Bedrock,
        Block::Lamp,
    ];

    /// You bump into it, stand on it, and can dig it.
    fn solid(self) -> bool {
        !matches!(self, Block::Air | Block::Water)
    }

    /// It hides the faces of blocks next to it (glass and leaves don't).
    fn opaque(self) -> bool {
        self.solid() && !matches!(self, Block::Glass | Block::Leaves)
    }

    /// Its tiles in the atlas: top, sides, bottom.
    fn tiles(self) -> Option<[&'static str; 3]> {
        Some(match self {
            Block::Air => return None,
            Block::Grass => ["grass_top", "grass_side", "dirt"],
            Block::Dirt => ["dirt"; 3],
            Block::Stone => ["stone"; 3],
            Block::Cobblestone => ["cobblestone"; 3],
            Block::Sand => ["sand"; 3],
            Block::Log => ["log_top", "log_side", "log_top"],
            Block::Leaves => ["leaves"; 3],
            Block::Planks => ["planks"; 3],
            Block::Glass => ["glass"; 3],
            Block::Brick => ["brick"; 3],
            Block::CoalOre => ["coal_ore"; 3],
            Block::GoldOre => ["gold_ore"; 3],
            Block::Water => ["water"; 3],
            Block::Bedrock => ["bedrock"; 3],
            Block::Lamp => ["lamp"; 3],
        })
    }

    /// Seconds of digging to break it (`None`: it can't be broken).
    fn dig_time(self) -> Option<f32> {
        Some(match self {
            Block::Air | Block::Water | Block::Bedrock => return None,
            Block::Leaves | Block::Glass | Block::Lamp => 0.2,
            Block::Sand | Block::Dirt => 0.4,
            Block::Grass => 0.45,
            Block::Planks | Block::Log => 0.7,
            Block::Stone | Block::Cobblestone | Block::Brick => 1.0,
            Block::CoalOre => 1.2,
            Block::GoldOre => 1.5,
        })
    }

    /// What digging it gives you (ores are counted on their own).
    fn drop(self) -> Option<Block> {
        match self {
            Block::Grass => Some(Block::Dirt),
            Block::Stone => Some(Block::Cobblestone),
            Block::CoalOre | Block::GoldOre | Block::Air | Block::Water | Block::Bedrock => None,
            other => Some(other),
        }
    }

    fn name(self) -> &'static str {
        match self {
            Block::Air => "Air",
            Block::Grass => "Grass",
            Block::Dirt => "Dirt",
            Block::Stone => "Stone",
            Block::Cobblestone => "Cobblestone",
            Block::Sand => "Sand",
            Block::Log => "Log",
            Block::Leaves => "Leaves",
            Block::Planks => "Planks",
            Block::Glass => "Glass",
            Block::Brick => "Brick",
            Block::CoalOre => "Coal ore",
            Block::GoldOre => "Gold ore",
            Block::Water => "Water",
            Block::Bedrock => "Bedrock",
            Block::Lamp => "Lamp",
        }
    }
}

// ---- Noise: the same numbers for the same place, every time ----

fn hash(x: i32, y: i32, z: i32, seed: u32) -> u32 {
    let mut h = seed
        ^ (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ (z as u32).wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    h
}

/// A number from 0 to 1 for this grid point.
fn random01(x: i32, y: i32, z: i32, seed: u32) -> f32 {
    (hash(x, y, z, seed) >> 8) as f32 / (1u32 << 24) as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Smooth noise from 0 to 1 over a plane: random heights at whole numbers,
/// blended in between.
fn noise2(x: f32, z: f32, seed: u32) -> f32 {
    let (x0, z0) = (x.floor() as i32, z.floor() as i32);
    let (tx, tz) = (smooth(x - x0 as f32), smooth(z - z0 as f32));
    let at = |dx, dz| random01(x0 + dx, 0, z0 + dz, seed);
    lerp(
        lerp(at(0, 0), at(1, 0), tx),
        lerp(at(0, 1), at(1, 1), tx),
        tz,
    )
}

/// The same in 3D, for caves.
fn noise3(x: f32, y: f32, z: f32, seed: u32) -> f32 {
    let (x0, y0, z0) = (x.floor() as i32, y.floor() as i32, z.floor() as i32);
    let (tx, ty, tz) = (
        smooth(x - x0 as f32),
        smooth(y - y0 as f32),
        smooth(z - z0 as f32),
    );
    let at = |dx, dy, dz| random01(x0 + dx, y0 + dy, z0 + dz, seed);
    let plane = |dy| {
        lerp(
            lerp(at(0, dy, 0), at(1, dy, 0), tx),
            lerp(at(0, dy, 1), at(1, dy, 1), tx),
            tz,
        )
    };
    lerp(plane(0), plane(1), ty)
}

// ---- The world ----

/// The six directions a face can look: +x, -x, +y, -y, +z, -z.
const DIRECTIONS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// Every block of the world. Block (x, y, z) fills the cube from (x, y, z)
/// to (x + 1, y + 1, z + 1); y is up.
#[derive(Clone)]
struct World {
    /// The blocks, and the meshes that draw them (one per chunk).
    grid: BlockWorld,
    /// Where the lamps are: the faces near them glow a little.
    lamps: Vec<[i32; 3]>,
    /// Cells that changed, or are on top of one that did: sand there may
    /// have nothing under it now (see `settle_sand`).
    unsettled: Vec<[i32; 3]>,
    /// For each column, the lowest y with nothing above it that casts a
    /// shadow: air there and above is under the open sky.
    sky: Vec<i32>,
    /// Tall grass and flowers, by the cell they stand in.
    plants: std::collections::HashMap<[i32; 3], Plant>,
    /// A plant came or went: the plants' mesh needs building again.
    plants_changed: bool,
    /// The number the island grew from (the same number grows the same island).
    seed: u32,
    /// Where the duck starts: its feet, and which way it looks.
    spawn: Vec3,
    spawn_yaw: f32,
}

impl World {
    fn inside(x: i32, y: i32, z: i32) -> bool {
        (0..SIZE as i32).contains(&x)
            && (0..TALL as i32).contains(&y)
            && (0..SIZE as i32).contains(&z)
    }

    /// The block at (x, y, z): bedrock below the world, air everywhere
    /// else outside it.
    fn get(&self, x: i32, y: i32, z: i32) -> Block {
        Block::ALL[self.grid.get(x, y, z) as usize]
    }

    /// Changes a block. Its chunk is built again at the next `update_chunks`.
    fn set(&mut self, x: i32, y: i32, z: i32, block: Block) {
        if !World::inside(x, y, z) {
            return;
        }
        if self.get(x, y, z) == Block::Lamp || block == Block::Lamp {
            // A lamp came or went: the faces it lights change, in every
            // chunk it reaches.
            self.lamps.retain(|&lamp| lamp != [x, y, z]);
            if block == Block::Lamp {
                self.lamps.push([x, y, z]);
            }
            let reach = LAMP_REACH.ceil() as i32;
            for (dx, dz) in [
                (-reach, -reach),
                (-reach, reach),
                (reach, -reach),
                (reach, reach),
            ] {
                self.grid.mark_changed(x + dx, z + dz);
            }
        }
        self.grid.set(x, y, z, block as BlockId);
        self.update_sky(x, z);
        self.plants_after_set(x, y, z, block);
        self.unsettled.push([x, y, z]);
        self.unsettled.push([x, y + 1, z]);
    }

    /// Builds again the chunks that changed, shading each face by the light
    /// in the cell it looks into.
    fn update_chunks(&mut self, palette: &BlockPalette) {
        let (sky, lamps) = (&self.sky, &self.lamps);
        self.grid
            .update_meshes_shaded(palette, |grid, x, y, z| FaceLight {
                shade: light(grid, sky, x, y, z),
                glow: lamp_light(lamps, x, y, z),
            });
    }

    /// Whether the duck can't be in this cell: solid blocks, and the
    /// invisible walls round the island.
    fn blocks_body(&self, x: i32, y: i32, z: i32) -> bool {
        let outside = !(0..SIZE as i32).contains(&x) || !(0..SIZE as i32).contains(&z);
        outside || y < 0 || self.get(x, y, z).solid()
    }

    fn update_sky(&mut self, x: i32, z: i32) {
        let mut sky = 0;
        for y in (0..TALL as i32).rev() {
            let b = self.get(x, y, z);
            if b.opaque() || b == Block::Leaves {
                sky = y + 1;
                break;
            }
        }
        self.sky[z as usize * SIZE + x as usize] = sky;
    }

    /// The height of the ground at column (x, z), before trees.
    fn ground_height(x: i32, z: i32, seed: u32) -> i32 {
        let (fx, fz) = (x as f32, z as f32);
        let base = 0.5 * noise2(fx / 22.0, fz / 22.0, seed)
            + 0.3 * noise2(fx / 11.0, fz / 11.0, seed + 1)
            + 0.2 * noise2(fx / 5.0, fz / 5.0, seed + 2);
        let hills = (noise2(fx / 16.0, fz / 16.0, seed + 3) - 0.55).max(0.0) * 34.0;
        let mut h = 6.5 + base * 17.0 + hills;
        // An island: the land sinks under the sea near the edges.
        let edge = x.min(z).min(SIZE as i32 - 1 - x).min(SIZE as i32 - 1 - z) as f32;
        if edge < 12.0 {
            h = lerp(5.0, h, smooth(edge / 12.0));
        }
        (h.round() as i32).clamp(2, TALL as i32 - 12)
    }

    /// Grows the island.
    fn generate(seed: u32) -> World {
        let mut world = World {
            grid: BlockWorld::with_chunk_size(SIZE, TALL, SIZE, CHUNK),
            lamps: Vec::new(),
            unsettled: Vec::new(),
            sky: vec![0; SIZE * SIZE],
            plants: std::collections::HashMap::new(),
            plants_changed: true,
            seed,
            spawn: Vec3::ZERO,
            spawn_yaw: 0.0,
        };
        // Bedrock all the way down: the bottom layer's undersides aren't drawn.
        world.grid.set_below(Block::Bedrock as BlockId);
        let heights: Vec<i32> = (0..SIZE * SIZE)
            .map(|i| World::ground_height((i % SIZE) as i32, (i / SIZE) as i32, seed))
            .collect();
        let height = |x: i32, z: i32| heights[z as usize * SIZE + x as usize];

        // Rock, dirt and grass (sand by the sea), then the sea.
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let h = height(x, z);
                let beach = h <= SEA + 1;
                for y in 0..=h {
                    let block = if y == 0 {
                        Block::Bedrock
                    } else if y == h {
                        if beach { Block::Sand } else { Block::Grass }
                    } else if y >= h - 3 {
                        if beach { Block::Sand } else { Block::Dirt }
                    } else {
                        let r = random01(x, y, z, seed + 4);
                        if y < 10 && r < 0.012 {
                            Block::GoldOre
                        } else if y < 30 && r < 0.035 {
                            Block::CoalOre
                        } else {
                            Block::Stone
                        }
                    };
                    world.grid.set(x, y, z, block as BlockId);
                }
                for y in h + 1..=SEA {
                    world.grid.set(x, y, z, Block::Water as BlockId);
                }
            }
        }

        // Caves: wherever 3D noise is high, under land well above the sea.
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let h = height(x, z);
                if h <= SEA + 3 {
                    continue;
                }
                for y in 2..=h - 2 {
                    let n = noise3(x as f32 / 8.0, y as f32 / 5.0, z as f32 / 8.0, seed + 5);
                    if n > 0.7 {
                        world.grid.set(x, y, z, Block::Air as BlockId);
                    }
                }
            }
        }

        // Trees, on grass, not too close to each other.
        let mut trees: Vec<(i32, i32)> = Vec::new();
        for z in 3..SIZE as i32 - 3 {
            for x in 3..SIZE as i32 - 3 {
                let h = height(x, z);
                let on_grass = world.get(x, h, z) == Block::Grass;
                let room = trees
                    .iter()
                    .all(|&(tx, tz)| (tx - x).abs().max((tz - z).abs()) > 4);
                if on_grass && room && random01(x, 0, z, seed + 6) < 0.014 {
                    trees.push((x, z));
                    world.grow_tree(x, h + 1, z, random01(x, 1, z, seed + 6));
                }
            }
        }

        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                world.update_sky(x, z);
            }
        }
        world.grow_plants();

        // Start on grass as near the middle as there is, not under a tree.
        let middle = SIZE as i32 / 2;
        let mut best = (i32::MAX, middle, middle);
        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                let h = height(x, z);
                // No tree within three blocks, so the first view isn't leaves.
                let clear = (-3..=3).all(|dz| {
                    (-3..=3).all(|dx| {
                        (1..=7).all(|dy| {
                            !matches!(
                                world.get(x + dx, h + dy, z + dz),
                                Block::Log | Block::Leaves
                            )
                        })
                    })
                });
                let distance = (x - middle).pow(2) + (z - middle).pow(2);
                if world.get(x, h, z) == Block::Grass && clear && distance < best.0 {
                    best = (distance, x, z);
                }
            }
        }
        let (_, x, z) = best;
        world.spawn = vec3(x as f32 + 0.5, height(x, z) as f32 + 1.0, z as f32 + 0.5);
        // Face the way with the most land in view.
        let land_ahead = |yaw: f32| {
            let point = |d: f32| {
                (
                    (x as f32 + 0.5 + yaw.sin() * d) as i32,
                    (z as f32 + 0.5 + yaw.cos() * d) as i32,
                )
            };
            let tree_near = (1..8).any(|d| {
                let (lx, lz) = point(d as f32);
                (0..TALL as i32).any(|y| matches!(world.get(lx, y, lz), Block::Log | Block::Leaves))
            });
            if tree_near {
                return 0;
            }
            (3..24)
                .filter(|&d| {
                    let (lx, lz) = (
                        x as f32 + yaw.sin() * d as f32,
                        z as f32 + yaw.cos() * d as f32,
                    );
                    World::inside(lx as i32, 0, lz as i32) && height(lx as i32, lz as i32) > SEA
                })
                .count()
        };
        world.spawn_yaw = (0..16)
            .map(|i| i as f32 * TAU / 16.0)
            .max_by_key(|&yaw| land_ahead(yaw))
            .unwrap_or(0.0);
        world
    }

    /// A trunk 4 or 5 high from `base`, with a round crown of leaves.
    fn grow_tree(&mut self, x: i32, base: i32, z: i32, random: f32) {
        let trunk = if random < 0.5 { 4 } else { 5 };
        let top = base + trunk - 1;
        for dy in -2..=1 {
            let radius: i32 = if dy <= -1 { 2 } else { 1 };
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let corner = dx.abs() == radius && dz.abs() == radius;
                    let y = top + dy;
                    if corner && (dy == 1 || random01(x + dx, y, z + dz, self.seed + 7) < 0.5) {
                        continue;
                    }
                    if World::inside(x + dx, y, z + dz) && self.get(x + dx, y, z + dz) == Block::Air
                    {
                        self.grid.set(x + dx, y, z + dz, Block::Leaves as BlockId);
                    }
                }
            }
        }
        for y in base..=top {
            self.grid.set(x, y, z, Block::Log as BlockId);
        }
    }

    /// The first solid block along a ray (`direction` of length 1), at most
    /// `reach` away: which block, and which way the face that was hit
    /// looks. Steps from cell to cell, so it never misses one.
    fn raycast(&self, origin: Vec3, direction: Vec3, reach: f32) -> Option<RayHit> {
        let o = [origin.x, origin.y, origin.z];
        let d = [direction.x, direction.y, direction.z];
        let mut cell = o.map(|v| v.floor() as i32);
        let step = d.map(|v| if v > 0.0 { 1 } else { -1 });
        let delta = d.map(|v| {
            if v == 0.0 {
                f32::INFINITY
            } else {
                (1.0 / v).abs()
            }
        });
        let mut next = [0.0; 3];
        for i in 0..3 {
            next[i] = if d[i] > 0.0 {
                (cell[i] as f32 + 1.0 - o[i]) * delta[i]
            } else if d[i] < 0.0 {
                (o[i] - cell[i] as f32) * delta[i]
            } else {
                f32::INFINITY
            };
        }
        let mut normal = [0; 3];
        loop {
            let block = self.get(cell[0], cell[1], cell[2]);
            if block.solid() && World::inside(cell[0], cell[1], cell[2]) {
                return Some(RayHit {
                    cell,
                    normal,
                    block,
                    plant: None,
                });
            }
            if let Some(plant) = self.plant(cell[0], cell[1], cell[2]) {
                return Some(RayHit {
                    cell,
                    normal,
                    block,
                    plant: Some(plant),
                });
            }
            let axis = if next[0] < next[1] && next[0] < next[2] {
                0
            } else if next[1] < next[2] {
                1
            } else {
                2
            };
            if next[axis] > reach {
                return None;
            }
            cell[axis] += step[axis];
            next[axis] += delta[axis];
            normal = [0; 3];
            normal[axis] = -step[axis];
        }
    }
}

/// How bright a face next to this cell is: 1 under the open sky, less
/// in caves and under trees and overhangs.
/// How far a lamp's light reaches, in blocks.
const LAMP_REACH: f32 = 4.5;

/// How much the lamps light this cell (0 to 0.75), so the faces round a
/// lamp stay bright at night: brightest next to it, fading out by
/// `LAMP_REACH`.
fn lamp_light(lamps: &[[i32; 3]], x: i32, y: i32, z: i32) -> f32 {
    let cell = vec3(x as f32, y as f32, z as f32);
    lamps
        .iter()
        .map(|&[lx, ly, lz]| {
            let distance = cell.distance(vec3(lx as f32, ly as f32, lz as f32));
            (1.0 - distance / LAMP_REACH).max(0.0) * 0.75
        })
        .fold(0.0, f32::max)
}

fn light(grid: &BlockWorld, sky: &[i32], x: i32, y: i32, z: i32) -> f32 {
    if !World::inside(x, y, z) {
        return 1.0;
    }
    let top = sky[z as usize * SIZE + x as usize];
    if y >= top {
        1.0
    } else if y >= top - 3 && !Block::ALL[grid.get(x, top - 1, z) as usize].opaque() {
        0.8 // under leaves
    } else {
        0.55
    }
}

/// Where a ray hit a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RayHit {
    cell: [i32; 3],
    /// Which way the face that was hit looks (one of the six directions).
    normal: [i32; 3],
    block: Block,
    /// A plant hit instead of a block (`block` is then air).
    plant: Option<Plant>,
}

impl RayHit {
    /// What was hit, by name.
    fn name(&self) -> &'static str {
        self.plant.map_or(self.block.name(), |p| p.name())
    }
}

// ---- Art ----

/// Pictures, meshes and sounds, made once and shared.
struct Art {
    /// How each kind of block looks, for the world's chunks.
    palette: BlockPalette,
    /// Each kind of block as a whole cube (for flying crumbs).
    cubes: Vec<Option<Mesh>>,
    /// The four stages of cracks, drawn over a block being dug.
    cracks: Vec<Mesh>,
    /// Each item's picture, for the hotbar and the inventory.
    item_icons: std::collections::HashMap<Item, Sprite>,
    /// Items that aren't blocks, as flat pictures to hold in the hand.
    item_cards: std::collections::HashMap<Item, Mesh>,
    /// Hearts, bread and bubbles over the hotbar.
    hud: Tileset,
    /// Each plant's picture in the atlas (by `Plant as usize`).
    plant_uvs: Vec<Rect>,
    /// The sea all round the island, out to the horizon.
    ocean: Mesh,
    clouds: Mesh,
    duck: Duck,
    /// The night's shapes: the duck's body in other skins.
    hollow: Duck,
    watcher: Duck,
    sounds: Sounds,
    duckling: DucklingModel,
    /// A flat dark disc, drawn see-through under the duck.
    shadow: Mesh,
    /// A white cube, drawn faintly over the block you look at.
    highlight: Mesh,
    wind: Wav,
}

impl Art {
    fn load() -> Result<Art, LoadError> {
        let atlas = Tileset::load(format!("{ASSETS}/blocks.json"))?;
        let mut kinds = Vec::new();
        let mut cubes = Vec::new();
        for block in Block::ALL {
            match block.tiles() {
                Some(tiles) => {
                    // In `Block::ALL`'s order, so a block's number in the
                    // palette is `block as BlockId` (air is 0).
                    let kind = BlockKind::new(block.name(), tiles);
                    kinds.push(match block {
                        Block::Water | Block::Glass => kind.see_through(),
                        Block::Leaves => kind.inner_faces(),
                        Block::Lamp => kind.glowing(),
                        _ => kind,
                    });
                    let mut cube = Mesh::block(&atlas, tiles);
                    if block == Block::Lamp {
                        cube = cube.glowing();
                    }
                    cubes.push(Some(cube));
                }
                None => cubes.push(None),
            }
        }
        let cracks = (1..=4)
            .map(|n| {
                let name = format!("crack_{n}");
                Mesh::block(&atlas, [name.as_str(); 3])
            })
            .collect();
        let others = [
            Item::Gold,
            Item::Coal,
            Item::Apple,
            Item::Seeds,
            Item::Bread,
            Item::Blade,
        ];
        let carried_blocks = Block::ALL
            .into_iter()
            .filter(|b| *b != Block::Air)
            .map(Item::Block);
        let item_icons = carried_blocks
            .chain(others)
            .map(|item| (item, atlas.tile(item.tile())))
            .collect();
        let item_cards = others
            .into_iter()
            .map(|item| (item, item_card(atlas.tile(item.tile()))))
            .collect();
        Ok(Art {
            palette: BlockPalette::new(&atlas, &kinds),
            cubes,
            cracks,
            item_icons,
            item_cards,
            hud: Tileset::load(format!("{ASSETS}/hud.json"))?,
            plant_uvs: Plant::ALL.iter().map(|p| atlas.uv_rect(p.tile())).collect(),
            ocean: ocean(atlas.tile("water")),
            clouds: clouds(atlas.tile("cloud")),
            duck: Duck::load(Sprite::load_png(format!("{ASSETS}/skin.png"))?),
            hollow: Duck::load(Sprite::load_png(format!("{ASSETS}/hollow.png"))?),
            watcher: Duck::load(Sprite::load_png(format!("{ASSETS}/watcher.png"))?),
            sounds: Sounds::load()?,
            duckling: DucklingModel::load(Sprite::load_png(format!("{ASSETS}/duckling.png"))?),
            shadow: Mesh::cylinder(Color::BLACK, 16).glowing(),
            highlight: Mesh::cube(Color::WHITE).glowing(),
            wind: Wav::load(format!("{ASSETS}/hum.wav"))?,
        })
    }
}

/// Music and the sounds made from files (`audio/make_sounds.py`).
struct Sounds {
    music_day: Wav,
    music_night: Wav,
    music_title: Wav,
    groan: Wav,
    groan_far: Wav,
    hollow_hurt: Wav,
    hollow_die: Wav,
    sting: Wav,
    heartbeat: Wav,
    bell_dusk: Wav,
    bell_dawn: Wav,
}

impl Sounds {
    fn load() -> Result<Sounds, LoadError> {
        let wav = |name: &str| Wav::load(format!("{ASSETS}/{name}.wav"));
        Ok(Sounds {
            music_day: wav("music_day")?,
            music_night: wav("music_night")?,
            music_title: wav("music_title")?,
            groan: wav("groan")?,
            groan_far: wav("groan_far")?,
            hollow_hurt: wav("hollow_hurt")?,
            hollow_die: wav("hollow_die")?,
            sting: wav("sting")?,
            heartbeat: wav("heartbeat")?,
            bell_dusk: wav("bell_dusk")?,
            bell_dawn: wav("bell_dawn")?,
        })
    }
}

/// An item that isn't a block, as a flat picture a block across (both
/// sides), standing up round its middle: for holding in the hand.
fn item_card(picture: Sprite) -> Mesh {
    let mut mesh = Mesh::new().with_texture(picture);
    let h = 0.5;
    let i: Vec<usize> = [
        vec3(-h, -h, 0.0),
        vec3(-h, h, 0.0),
        vec3(h, h, 0.0),
        vec3(h, -h, 0.0),
    ]
    .into_iter()
    .map(|c| mesh.add_vertex(c))
    .collect();
    let uv = [
        vec2(0.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 0.0),
        vec2(1.0, 1.0),
    ];
    mesh.add_quad_uv(i[0], i[1], i[2], i[3], uv, Color::WHITE);
    mesh.add_quad_uv(
        i[3],
        i[2],
        i[1],
        i[0],
        [uv[3], uv[2], uv[1], uv[0]],
        Color::WHITE,
    );
    mesh
}

/// A flat square, both sides, from `low` to `high` (x and z) at height
/// `y`, its texture repeating once a block.
fn add_flat(mesh: &mut Mesh, low: Vec2, high: Vec2, y: f32, color: Color) {
    let corners = [
        vec3(low.x, y, low.y),
        vec3(low.x, y, high.y),
        vec3(high.x, y, high.y),
        vec3(high.x, y, low.y),
    ];
    let uv = [
        vec2(0.0, 0.0),
        vec2(0.0, high.y - low.y),
        vec2(high.x - low.x, high.y - low.y),
        vec2(high.x - low.x, 0.0),
    ];
    let i: Vec<usize> = corners.iter().map(|&c| mesh.add_vertex(c)).collect();
    mesh.add_quad_uv(i[0], i[1], i[2], i[3], uv, color); // seen from above
    mesh.add_quad_uv(i[3], i[2], i[1], i[0], [uv[3], uv[2], uv[1], uv[0]], color); // and below
}

/// The sea round the island, out past where you can see: four flat strips.
fn ocean(water: Sprite) -> Mesh {
    let mut mesh = Mesh::new().with_texture(water);
    let (far, size, y) = (220.0, SIZE as f32, SEA as f32 + 1.0);
    let strips = [
        (vec2(-far, -far), vec2(size + far, 0.0)),
        (vec2(-far, size), vec2(size + far, size + far)),
        (vec2(-far, 0.0), vec2(0.0, size)),
        (vec2(size, 0.0), vec2(size + far, size)),
    ];
    for (low, high) in strips {
        add_flat(&mut mesh, low, high, y, Color::WHITE);
    }
    mesh
}

/// How far the clouds' pattern goes before it repeats (they drift by it).
const CLOUD_PERIOD: f32 = 128.0;
const CLOUD_HEIGHT: f32 = 50.0;

/// Blocky clouds: squares of 8 blocks where noise is high, in a pattern
/// that repeats every `CLOUD_PERIOD` blocks so they can drift for ever.
fn clouds(white: Sprite) -> Mesh {
    let mut mesh = Mesh::new().with_texture(white).glowing();
    let cell = 8.0;
    let cells = (CLOUD_PERIOD / cell) as i32;
    for cz in -14i32..22 {
        for cx in -30i32..22 {
            let (px, pz) = (cx.rem_euclid(cells), cz.rem_euclid(cells));
            let n = noise2(px as f32 / 2.5, pz as f32 / 2.5, SEED + 8);
            let n = if px == 0 || pz == 0 {
                n.min(random01(px, 0, pz, SEED + 9)) // no seam where it repeats
            } else {
                n
            };
            if n > 0.6 {
                let low = vec2(cx as f32 * cell, cz as f32 * cell);
                add_flat(
                    &mut mesh,
                    low,
                    low + vec2(cell, cell),
                    CLOUD_HEIGHT,
                    Color::WHITE,
                );
            }
        }
    }
    mesh
}

// ---- The duck: a blocky character with a skin, like a Minecraft player ----

/// Pixels per block in the skin: a part 8 pixels wide is half a block.
const SKIN_PIXEL: f32 = 1.0 / 16.0;
const SKIN_SIZE: (f32, f32) = (64.0, 32.0);

/// A box `size` pixels big (wide, tall, deep), its lowest corner at
/// `offset` pixels from the part's pivot, each face textured from the skin
/// where the part is unwrapped at `(u, v)` (the same layout as
/// `art/make_skin.py`: top and bottom above, then +x, front, -x, back).
fn skin_box(size: [f32; 3], offset: [f32; 3], u: f32, v_: f32) -> Mesh {
    let [w, h, d] = size;
    let low = vec3(offset[0], offset[1], offset[2]) * SKIN_PIXEL;
    let high = low + vec3(w, h, d) * SKIN_PIXEL;
    let mut mesh = Mesh::new();
    let corner = |x: bool, y: bool, z: bool| {
        vec3(
            if x { high.x } else { low.x },
            if y { high.y } else { low.y },
            if z { high.z } else { low.z },
        )
    };
    let v: Vec<usize> = [
        corner(false, false, false),
        corner(true, false, false),
        corner(true, true, false),
        corner(false, true, false),
        corner(false, false, true),
        corner(true, false, true),
        corner(true, true, true),
        corner(false, true, true),
    ]
    .into_iter()
    .map(|c| mesh.add_vertex(c))
    .collect();
    // A face's patch of the skin as texture points: bottom-left, top-left,
    // top-right, bottom-right (the order add_quad_uv wants), a hundredth
    // of a pixel inside the patch so neighbours never bleed in.
    let patch = |x: f32, y: f32, pw: f32, ph: f32| {
        let (sw, sh) = SKIN_SIZE;
        let inset = 0.01;
        let (l, t, r, b) = (
            (x + inset) / sw,
            (y + inset) / sh,
            (x + pw - inset) / sw,
            (y + ph - inset) / sh,
        );
        [vec2(l, b), vec2(l, t), vec2(r, t), vec2(r, b)]
    };
    let white = Color::WHITE;
    // Corners go clockwise seen from outside: bottom-left, top-left,
    // top-right, bottom-right.
    mesh.add_quad_uv(
        v[0],
        v[3],
        v[2],
        v[1],
        patch(u + 2.0 * d + w, v_ + d, w, h),
        white,
    ); // back (-z)
    mesh.add_quad_uv(v[5], v[6], v[7], v[4], patch(u + d, v_ + d, w, h), white); // front (+z)
    mesh.add_quad_uv(
        v[4],
        v[7],
        v[3],
        v[0],
        patch(u + d + w, v_ + d, d, h),
        white,
    ); // -x
    mesh.add_quad_uv(v[1], v[2], v[6], v[5], patch(u, v_ + d, d, h), white); // +x
    mesh.add_quad_uv(v[3], v[7], v[6], v[2], patch(u + d, v_, w, d), white); // top
    mesh.add_quad_uv(v[0], v[1], v[5], v[4], patch(u + d + w, v_, w, d), white); // bottom
    mesh
}

/// The duck's parts, each a mesh round its own pivot (the joint it
/// swings from), all textured with the skin.
struct Duck {
    /// Pivot at the neck; the beak is merged in.
    head: Mesh,
    /// Pivot at the hips.
    body: Mesh,
    /// Pivot at the shoulder; hangs down.
    arm: Mesh,
    /// Pivot at the hip; hangs down.
    leg: Mesh,
}

/// How the duck stands: turned to `yaw`, its head tilted by `pitch`, limbs
/// swung by `swing` (radians, legs one way and arms the other) and its arms
/// raised by `lift`.
#[derive(Debug, Clone, Copy)]
struct Pose {
    yaw: f32,
    pitch: f32,
    swing: f32,
    lift: f32,
}

impl Duck {
    /// 1 pixel = 1/16 block: legs 10, body 10, head 8: 28 pixels, 1.75
    /// blocks tall.
    const HIP: f32 = 10.0 * SKIN_PIXEL;
    const NECK: f32 = 20.0 * SKIN_PIXEL;
    const SHOULDER: f32 = 19.0 * SKIN_PIXEL;

    fn load(skin: Sprite) -> Duck {
        let mut head = skin_box([8.0, 8.0, 8.0], [-4.0, 0.0, -4.0], 0.0, 0.0);
        let beak = skin_box([4.0, 2.0, 3.0], [-2.0, 2.0, 4.0], 32.0, 0.0);
        head.merge(&beak, &Transform::default());
        Duck {
            head: head.with_texture(skin.clone()),
            body: skin_box([8.0, 10.0, 4.0], [-4.0, 0.0, -2.0], 0.0, 16.0)
                .with_texture(skin.clone()),
            arm: skin_box([4.0, 10.0, 4.0], [-2.0, -10.0, -2.0], 24.0, 16.0)
                .with_texture(skin.clone()),
            leg: skin_box([4.0, 10.0, 4.0], [-2.0, -10.0, -2.0], 40.0, 16.0).with_texture(skin),
        }
    }

    /// Each part and where it goes, for the duck standing at `feet` in
    /// `pose` (in the world, or in view space for the inventory's picture).
    fn parts(&self, feet: Vec3, pose: Pose) -> [(&Mesh, Transform); 6] {
        self.parts_sized(feet, pose, 1.0)
    }

    /// The same, `size` times as big (a Hollow is 1.12, the Watcher 1.75).
    fn parts_sized(&self, feet: Vec3, pose: Pose, size: f32) -> [(&Mesh, Transform); 6] {
        let Pose {
            yaw,
            pitch,
            swing,
            lift,
        } = pose;
        // A part's pivot: its place on the body, turned.
        let part = |pivot: Vec3, rotation: Vec3| {
            Transform::at(feet + (pivot * size).rotate_y(yaw))
                .rotated(vec3(rotation.x, yaw + rotation.y, rotation.z))
                .sized(size)
        };
        let hand = 6.0 * SKIN_PIXEL;
        let thigh = 2.0 * SKIN_PIXEL;
        [
            (&self.body, part(vec3(0.0, Self::HIP, 0.0), Vec3::ZERO)),
            (
                &self.head,
                part(vec3(0.0, Self::NECK, 0.0), vec3(pitch, 0.0, 0.0)),
            ),
            (
                &self.leg,
                part(vec3(-thigh, Self::HIP, 0.0), vec3(swing, 0.0, 0.0)),
            ),
            (
                &self.leg,
                part(vec3(thigh, Self::HIP, 0.0), vec3(-swing, 0.0, 0.0)),
            ),
            (
                &self.arm,
                part(
                    vec3(-hand, Self::SHOULDER, 0.0),
                    vec3(-swing - lift, 0.0, 0.1),
                ),
            ),
            (
                &self.arm,
                part(
                    vec3(hand, Self::SHOULDER, 0.0),
                    vec3(swing - lift, 0.0, -0.1),
                ),
            ),
        ]
    }

    /// Draws the duck standing at `feet`, in `pose`.
    fn draw(&self, canvas: &mut Canvas, camera: &Camera3D, feet: Vec3, pose: Pose) {
        for (mesh, place) in self.parts(feet, pose) {
            canvas.draw_mesh(mesh, &place, camera);
        }
    }
}

/// The size of something that walks about and bumps into blocks: the
/// duck, a duckling. Its position is its feet, in the middle of the box.
#[derive(Debug, Clone, Copy)]
struct Body {
    half_width: f32,
    height: f32,
}

/// The duck's body.
const PLAYER: Body = Body {
    half_width: HALF_WIDTH,
    height: BODY_HEIGHT,
};

impl Body {
    /// Whether the body, with its feet at `feet`, overlaps anything solid.
    fn hits(&self, world: &World, feet: Vec3) -> bool {
        let low = feet - vec3(self.half_width, 0.0, self.half_width);
        let high = feet + vec3(self.half_width, self.height, self.half_width);
        let shrink = 1e-4;
        for y in low.y.floor() as i32..=(high.y - shrink).floor() as i32 {
            for z in low.z.floor() as i32..=(high.z - shrink).floor() as i32 {
                for x in low.x.floor() as i32..=(high.x - shrink).floor() as i32 {
                    if world.blocks_body(x, y, z) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Moves the body at `position` along one axis (0 x, 1 y, 2 z), in steps
    /// small enough never to pass through a block, stopping flush against
    /// one. Returns whether something stopped it.
    fn move_axis(&self, world: &World, position: &mut Vec3, axis: usize, distance: f32) -> bool {
        let steps = (distance.abs() / 0.4).ceil().max(1.0) as i32;
        let step = distance / steps as f32;
        let half = [self.half_width, 0.0, self.half_width];
        for _ in 0..steps {
            let mut moved = *position;
            match axis {
                0 => moved.x += step,
                1 => moved.y += step,
                _ => moved.z += step,
            }
            if !self.hits(world, moved) {
                *position = moved;
                continue;
            }
            // Flush against the block it ran into.
            let gap = 1e-3;
            let value = |v: Vec3| [v.x, v.y, v.z][axis];
            let edge = if step > 0.0 {
                let top = if axis == 1 { self.height } else { half[axis] };
                (value(moved) + top).floor() - top - gap
            } else {
                (value(moved) - half[axis]).floor() + 1.0 + half[axis] + gap
            };
            let mut flush = *position;
            match axis {
                0 => flush.x = edge,
                1 => flush.y = edge,
                _ => flush.z = edge,
            }
            if !self.hits(world, flush) {
                *position = flush;
            }
            return true;
        }
        false
    }
}

// ---- Falling sand ----

/// Sand with nothing under it, on its way down.
#[derive(Debug, Clone, Copy)]
struct FallingBlock {
    /// The column it's falling in.
    x: i32,
    z: i32,
    /// The height of its bottom face.
    y: f32,
    speed: f32,
    block: Block,
}

impl DuckCraft {
    /// Sand that has lost what held it up starts to fall (the sand above
    /// it follows a frame later, and so on up the pile).
    fn settle_sand(&mut self) {
        for [x, y, z] in std::mem::take(&mut self.world.unsettled) {
            let block = self.world.get(x, y, z);
            if block == Block::Sand && !self.world.get(x, y - 1, z).solid() && y > 0 {
                // Under the sea, water fills in behind it.
                let wet = DIRECTIONS
                    .iter()
                    .filter(|&&(_, dy, _)| dy >= 0)
                    .any(|&(dx, dy, dz)| self.world.get(x + dx, y + dy, z + dz) == Block::Water);
                self.world
                    .set(x, y, z, if wet { Block::Water } else { Block::Air });
                self.falling.push(FallingBlock {
                    x,
                    z,
                    y: y as f32,
                    speed: 0.0,
                    block,
                });
            }
        }
    }

    /// Moves falling blocks down; one that lands becomes a block again
    /// (through water: sand sinks), unless something stands in its way,
    /// and then you get it.
    fn drop_falling(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let mut landed = Vec::new();
        for (i, falling) in self.falling.iter_mut().enumerate() {
            falling.speed = (falling.speed + GRAVITY * dt).min(30.0);
            let next = falling.y - falling.speed * dt;
            // The highest solid cell it would pass into, if any.
            let from = falling.y.floor() as i32 - 1;
            let to = next.floor() as i32;
            let floor = (to..=from)
                .rev()
                .find(|&y| y < 0 || self.world.get(falling.x, y, falling.z).solid());
            match floor {
                Some(y) => {
                    falling.y = (y + 1) as f32;
                    landed.push(i);
                }
                None => falling.y = next,
            }
        }
        for &i in landed.iter().rev() {
            let falling = self.falling.remove(i);
            let [x, y, z] = [falling.x, falling.y as i32, falling.z];
            let low = vec3(x as f32, y as f32, z as f32);
            let in_the_way = boxes_overlap(self.position, PLAYER, low)
                || self
                    .ducklings
                    .iter()
                    .any(|d| boxes_overlap(d.position, ducklings::DUCKLING, low));
            if in_the_way || !World::inside(x, y, z) {
                self.give(Item::Block(falling.block), 1);
                self.say(format!(
                    "{} fell on you: you caught it",
                    falling.block.name()
                ));
            } else {
                self.world.set(x, y, z, falling.block);
            }
            ctx.play(Sound {
                name: "thud",
                wave: Wave::Noise,
                freq: 250.0,
                ms: 90.0,
                volume: 0.18,
                decay: 70.0,
                ..Default::default()
            });
        }
    }
}

/// Whether a body standing at `feet` overlaps the block whose low corner
/// is `low`.
fn boxes_overlap(feet: Vec3, body: Body, low: Vec3) -> bool {
    let (b_low, b_high) = (
        feet - vec3(body.half_width, 0.0, body.half_width),
        feet + vec3(body.half_width, body.height, body.half_width),
    );
    let high = low + vec3(1.0, 1.0, 1.0);
    b_low.x < high.x
        && b_high.x > low.x
        && b_low.y < high.y
        && b_high.y > low.y
        && b_low.z < high.z
        && b_high.z > low.z
}

// ---- The game ----

/// A crumb of a broken block, flying.
#[derive(Debug, Clone, Copy)]
struct Crumb {
    position: Vec3,
    velocity: Vec3,
    age: f32,
    block: Block,
}

#[derive(Clone)]
struct DuckCraft {
    art: Rc<Art>,
    world: World,
    /// Per chunk (z * across + x): the solid mesh and the glowing one.

    /// The duck's feet, in the middle of its body.
    position: Vec3,
    velocity: Vec3,
    /// Where it looks: turning (0 looks along +z) and up/down.
    yaw: f32,
    pitch: f32,
    on_ground: bool,
    in_water: bool,
    walking: bool,
    behind_view: bool,
    show_help: bool,
    /// The left button took the pointer back: it doesn't dig until it's let go.
    left_click_locked: bool,
    /// Escape gave the pointer back: the picture says how to carry on.
    pointer_free: bool,
    /// For tests and pictures: in the behind view, look at the duck from
    /// this angle round it (radians; 0 is from the front) instead of from behind.
    orbit: Option<f32>,

    /// Which hotbar slot is in your hand (0 to 8).
    slot: usize,
    /// How many of each block you carry.
    inventory: Inventory,
    dug: u32,
    placed: u32,
    /// Where the game saves (`None`: it doesn't), and when it last did.
    save_path: Option<std::path::PathBuf>,
    last_save: f32,
    /// Every plant, as one mesh.
    plant_mesh: Mesh,
    /// Plants picked, all told.
    picked: u32,
    /// Sand on its way down.
    falling: Vec<FallingBlock>,
    /// The ducklings on the island.
    ducklings: Vec<Duckling>,
    /// The duckling you're looking at, if it's nearer than any block.
    aimed: Option<usize>,
    /// How many times a duckling has imprinted on you (orders the line).
    imprints: u32,
    /// Health, hunger and air.
    vitals: health::Vitals,
    /// Where the game is: the title, playing, or paused.
    stage: Stage,
    mode: Mode,
    /// The island came from a save file (the title offers to carry on).
    has_save: bool,
    music: Option<menus::Music>,
    music_on: bool,
    /// Creative: flying (Space twice).
    flying: bool,
    /// Seconds since Space was last pressed (for the double tap).
    space_tap: f32,
    /// "Day 3" and a line, and seconds left on screen.
    announcement: Option<(String, String, f32)>,
    /// Last frame's time of day (for noticing dusk and dawn).
    last_time_of_day: f32,
    hollows: Vec<Hollow>,
    watcher: Option<Watcher>,
    /// The day number the Watcher last had its chance on.
    watcher_night: u32,
    /// Seconds to the next try at a Hollow coming up.
    spawn_wait: f32,
    /// Hollows put down, all told.
    kills: u32,
    /// Seconds until the next blow can land, and of the arm's swing.
    swing_wait: f32,
    swing: f32,
    /// The Hollow in view (nearer than any block), this frame.
    aimed_hollow_now: Option<usize>,
    /// Seconds left of the Watcher's scare (a jolt and a flash).
    scare: f32,
    /// Seconds on the title screen (it turns the view).
    title_spin: f32,
    /// What made the duck faint last (for the faint screen).
    fainted_because: String,
    /// Things eaten, all told.
    eaten: u32,
    /// Seconds spent eating what's in hand (0: not eating).
    eating_seconds: f32,
    /// Blocks made by crafting, all told.
    crafted: u32,
    /// The inventory screen, when it's open.
    screen: Option<inventory::Screen>,
    /// What was left in the crafting grid when the bag was too full to take
    /// it back (it's there when the inventory opens again).
    kept_grid: [Option<Stack>; 4],
    /// Where the mouse pointer is (for drawing the inventory screen).
    mouse: Option<Vec2>,
    /// Draw the pointer ourselves while it's free (for recorded videos,
    /// which don't show the system's pointer).
    draw_pointer: bool,

    /// The block being dug, and how long it's been dug for.
    digging: Option<[i32; 3]>,
    dig_seconds: f32,
    place_wait: f32,
    step_wait: f32,
    /// The block looked at this frame.
    target: Option<RayHit>,
    crumbs: Vec<Crumb>,
    /// A line at the bottom, and how long it has left.
    message: String,
    message_seconds: f32,
    time: f32,
}

impl DuckCraft {
    fn new(art: Rc<Art>, mut world: World) -> DuckCraft {
        world.update_chunks(&art.palette);
        let ducklings = ducklings::hatch(&world);
        world.unsettled.clear();
        let plant_mesh = art.plant_mesh(&world);
        world.plants_changed = false;
        DuckCraft {
            plant_mesh,
            picked: 0,
            save_path: None,
            last_save: 0.0,
            falling: Vec::new(),
            ducklings,
            aimed: None,
            imprints: 0,
            art,
            position: world.spawn,
            yaw: world.spawn_yaw,
            world,
            velocity: Vec3::ZERO,
            pitch: -0.25,
            on_ground: false,
            in_water: false,
            walking: false,
            behind_view: false,
            show_help: true,
            left_click_locked: false,
            pointer_free: false,
            orbit: None,
            slot: 0,
            inventory: items::starting_kit(),
            dug: 0,
            placed: 0,
            crafted: 0,
            eating_seconds: 0.0,
            vitals: health::Vitals::default(),
            stage: Stage::Playing,
            mode: Mode::Survival,
            has_save: false,
            music: None,
            music_on: true,
            flying: false,
            space_tap: 10.0,
            announcement: None,
            last_time_of_day: menus::DAY_START,
            hollows: Vec::new(),
            watcher: None,
            watcher_night: 0,
            spawn_wait: 3.0,
            kills: 0,
            swing_wait: 0.0,
            swing: 0.0,
            aimed_hollow_now: None,
            scare: 0.0,
            title_spin: 0.0,
            fainted_because: String::new(),
            eaten: 0,
            screen: None,
            kept_grid: [None; 4],
            mouse: None,
            draw_pointer: false,
            digging: None,
            dig_seconds: 0.0,
            place_wait: 0.0,
            step_wait: 0.0,
            target: None,
            crumbs: Vec::new(),
            message: String::new(),
            message_seconds: 0.0,
            time: 0.0,
        }
    }

    fn eye(&self) -> Vec3 {
        self.position + vec3(0.0, EYE_HEIGHT, 0.0)
    }

    fn look_direction(&self) -> Vec3 {
        vec3(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    /// Whether the duck's body, with its feet at `feet`, overlaps anything
    /// solid.
    fn body_hits(&self, feet: Vec3) -> bool {
        PLAYER.hits(&self.world, feet)
    }

    /// Moves the duck along one axis (0 x, 1 y, 2 z); `true` if something
    /// stopped it.
    fn move_axis(&mut self, axis: usize, distance: f32) -> bool {
        PLAYER.move_axis(&self.world, &mut self.position, axis, distance)
    }

    /// The nearest place to `feet` the duck fits (not inside a block):
    /// straight up first, then the columns round about.
    fn free_spot(&self, feet: Vec3) -> Vec3 {
        let (cx, cz) = (feet.x.floor() as i32, feet.z.floor() as i32);
        for ring in 0..8i32 {
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dz.abs()) != ring || !World::inside(cx + dx, 0, cz + dz) {
                        continue;
                    }
                    let column = if ring == 0 {
                        feet
                    } else {
                        vec3(
                            (cx + dx) as f32 + 0.5,
                            feet.y.floor(),
                            (cz + dz) as f32 + 0.5,
                        )
                    };
                    for up in 0..TALL as i32 {
                        let at = column + vec3(0.0, up as f32, 0.0);
                        if at.y > TALL as f32 {
                            break;
                        }
                        if !self.body_hits(at) {
                            return at;
                        }
                    }
                }
            }
        }
        feet
    }

    fn say(&mut self, text: impl Into<String>) {
        self.message = text.into();
        self.message_seconds = 2.5;
    }

    /// What's in the hotbar slot in your hand.
    fn holding(&self) -> Option<Stack> {
        self.inventory.slots[self.slot]
    }

    /// The block in your hand, if it's a block.
    fn holding_block(&self) -> Option<Block> {
        self.holding().and_then(|stack| stack.item.block())
    }

    /// Puts things in the bag; says so if they don't all fit.
    fn give(&mut self, item: Item, count: u32) {
        if self.inventory.add(item, count) > 0 {
            self.say("Your bag is full");
        }
    }

    /// How many of `item` you carry.
    fn count(&self, item: Item) -> u32 {
        self.inventory.count(item)
    }

    /// Builds again the chunks the last dig or build changed.
    fn update_chunks(&mut self) {
        self.world.update_chunks(&self.art.palette);
        if self.world.plants_changed {
            self.plant_mesh = self.art.plant_mesh(&self.world);
            self.world.plants_changed = false;
        }
    }

    fn walk(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let input = &ctx.input;
        // While the inventory is open the duck stands still (it still
        // falls and floats).
        let free = self.screen.is_none() && !self.vitals.fainted;
        let axis = |minus, plus| if free { input.axis(minus, plus) } else { 0.0 };
        let down = |key| free && input.is_down(key);

        // Look: the mouse (while the window holds the pointer), or the arrow keys.
        let mouse = if ctx.pointer_locked() && free {
            input.mouse_delta()
        } else {
            Vec2::ZERO
        };
        self.yaw += mouse.x * MOUSE_LOOK + axis(Key::Left, Key::Right) * TURN_SPEED * dt;
        self.pitch += -mouse.y * MOUSE_LOOK + axis(Key::Down, Key::Up) * TURN_SPEED * dt;
        self.pitch = self.pitch.clamp(-1.55, 1.55);
        self.yaw = self.yaw.rem_euclid(TAU);

        // Walk along the ground, the way you're facing.
        let ahead = vec3(self.yaw.sin(), 0.0, self.yaw.cos());
        let right = vec3(self.yaw.cos(), 0.0, -self.yaw.sin());
        let wish = ahead * axis(Key::S, Key::W) + right * axis(Key::A, Key::D);
        self.walking = wish.length() > 0.0;
        // Creative: Space twice quickly to fly (and again to stop).
        self.space_tap += dt;
        if self.mode == Mode::Creative && free && input.was_pressed(Key::Space) {
            if self.space_tap < 0.3 {
                self.flying = !self.flying;
                self.space_tap = 10.0;
            } else {
                self.space_tap = 0.0;
            }
        }
        let running = down(Key::LeftShift) && !self.flying;
        let mut speed = if running { RUN_SPEED } else { WALK_SPEED };
        if self.flying {
            speed = FLY_SPEED;
        } else if self.in_water {
            speed *= 0.55;
        }
        let wish = if self.walking {
            wish.normalized() * speed
        } else {
            Vec3::ZERO
        };
        // Quick to start and stop on the ground, floaty in the air.
        let grip = if self.on_ground || self.flying {
            30.0
        } else {
            9.0
        };
        self.velocity.x = approach(self.velocity.x, wish.x, grip * dt);
        self.velocity.z = approach(self.velocity.z, wish.z, grip * dt);

        // Jump, swim, fall.
        let jump = down(Key::Space);
        if self.flying {
            // Space up, Shift down, and no falling.
            let rise = (jump as i32 - down(Key::LeftShift) as i32) as f32;
            self.velocity.y = approach(self.velocity.y, rise * FLY_SPEED * 0.8, 40.0 * dt);
        } else if self.in_water {
            self.velocity.y -= GRAVITY * 0.25 * dt;
            if jump {
                self.velocity.y = approach(self.velocity.y, 3.2, 25.0 * dt);
            }
            self.velocity.y = self.velocity.y.max(-2.5);
        } else {
            self.velocity.y -= GRAVITY * dt;
            if jump && self.on_ground {
                self.velocity.y = JUMP_SPEED;
            }
            self.velocity.y = self.velocity.y.max(-40.0);
        }

        let was_in_water = self.in_water;
        let blocked_x = self.move_axis(0, self.velocity.x * dt);
        let blocked_z = self.move_axis(2, self.velocity.z * dt);
        // Against a wall on the ground, stop; in the air, keep pushing, so a
        // hop up a step carries on over it.
        if blocked_x && self.on_ground {
            self.velocity.x = 0.0;
        }
        if blocked_z && self.on_ground {
            self.velocity.z = 0.0;
        }
        let falling = self.velocity.y <= 0.0;
        let landed = self.move_axis(1, self.velocity.y * dt);
        if landed {
            self.velocity.y = 0.0;
        }
        self.on_ground = landed && falling;
        if self.flying && self.on_ground {
            self.flying = false; // landed
        }

        // Walking into a step one block high hops up it.
        if (blocked_x || blocked_z) && self.on_ground && self.walking && !self.flying {
            let up = self.position + vec3(0.0, 1.05, 0.0) + wish.normalized() * 0.3;
            if !self.body_hits(up) && !self.body_hits(self.position + vec3(0.0, 1.05, 0.0)) {
                self.velocity = vec3(wish.x, JUMP_SPEED, wish.z); // keep going forward
            }
        }

        let middle = self.position + vec3(0.0, 0.6, 0.0);
        self.in_water = self.world.get(
            middle.x.floor() as i32,
            middle.y.floor() as i32,
            middle.z.floor() as i32,
        ) == Block::Water;
        if self.in_water && !was_in_water && self.velocity.y < -3.0 {
            ctx.play(Sound {
                name: "splash",
                wave: Wave::Noise,
                freq: 1400.0,
                slide_to: 300.0,
                ms: 260.0,
                volume: 0.25,
                decay: 200.0,
                ..Default::default()
            });
        }

        // Footsteps.
        self.step_wait -= dt;
        let moving = self.velocity.x.abs() + self.velocity.z.abs() > 1.0;
        if self.on_ground && moving && self.step_wait <= 0.0 {
            self.step_wait = if running { 0.28 } else { 0.38 };
            ctx.play(Sound {
                name: "step",
                wave: Wave::Noise,
                freq: 380.0,
                ms: 45.0,
                volume: 0.09,
                decay: 40.0,
                ..Default::default()
            });
        }
    }

    fn pick_block(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        let digits = [
            Key::Digit1,
            Key::Digit2,
            Key::Digit3,
            Key::Digit4,
            Key::Digit5,
            Key::Digit6,
            Key::Digit7,
            Key::Digit8,
            Key::Digit9,
        ];
        let before = self.slot;
        for (slot, key) in digits.into_iter().enumerate() {
            if input.was_pressed(key) {
                self.slot = slot;
            }
        }
        let scroll = input.scroll();
        if scroll > 0.0 {
            self.slot = (self.slot + 8) % 9;
        } else if scroll < 0.0 {
            self.slot = (self.slot + 1) % 9;
        }
        if input.was_mouse_pressed(MouseButton::Middle) {
            // The block you look at: its hotbar slot if it has one, else
            // swapped in from the backpack.
            if let Some(hit) = self.target {
                let wanted = Item::Block(hit.block.drop().unwrap_or(hit.block));
                if let Some(slot) = self.inventory.hotbar_slot_of(wanted) {
                    self.slot = slot;
                } else if let Some(i) = (HOTBAR_SLOTS..SLOTS)
                    .find(|&i| self.inventory.slots[i].is_some_and(|s| s.item == wanted))
                {
                    self.inventory.slots.swap(i, self.slot);
                    self.say(wanted.name());
                }
            }
        }
        if self.slot != before {
            self.eating_seconds = 0.0; // a new thing in hand starts a new bite
            let name = self.holding().map_or("Empty hand", |s| s.item.name());
            self.say(name);
            ctx.play(Sound {
                name: "pick",
                wave: Wave::Triangle,
                freq: 900.0,
                ms: 30.0,
                volume: 0.12,
                ..Default::default()
            });
        }
    }

    fn dig(&mut self, ctx: &mut Context) {
        // The click that took the pointer back doesn't dig.
        let left = ctx.pointer_locked() && !self.left_click_locked;
        let held =
            (left && ctx.input.is_mouse_down(MouseButton::Left)) || ctx.input.is_down(Key::X);
        let Some(hit) = self.target.filter(|_| held) else {
            self.digging = None;
            self.dig_seconds = 0.0;
            return;
        };
        // A plant breaks at once.
        if let Some(plant) = hit.plant {
            let [x, y, z] = hit.cell;
            self.world.remove_plant(x, y, z);
            self.update_chunks();
            self.picked += 1;
            self.digging = None;
            self.dig_seconds = 0.0;
            if plant == Plant::TallGrass && ctx.rng.range(0.0, 1.0) < plants::SEED_CHANCE {
                self.give(Item::Seeds, 1);
            }
            ctx.play(Sound {
                name: "snip",
                wave: Wave::Noise,
                freq: 2000.0,
                slide_to: 900.0,
                ms: 70.0,
                volume: 0.15,
                decay: 60.0,
                ..Default::default()
            });
            return;
        }
        if self.digging != Some(hit.cell) {
            self.digging = Some(hit.cell);
            self.dig_seconds = 0.0;
        }
        let Some(needed) = hit.block.dig_time() else {
            if (left && ctx.input.was_mouse_pressed(MouseButton::Left))
                || ctx.input.was_pressed(Key::X)
            {
                self.say("Bedrock can't be broken");
            }
            return;
        };
        // Creative: a block breaks at once, one every quarter second while
        // held.
        let creative = self.mode == Mode::Creative;
        if creative && self.swing_wait > 0.0 {
            return;
        }
        let needed = if creative { 0.0 } else { needed };
        let before = self.dig_seconds;
        self.dig_seconds += ctx.dt();
        // A scratch every quarter second while digging.
        if (before / 0.25).floor() != (self.dig_seconds / 0.25).floor() {
            ctx.play(Sound {
                name: "dig",
                wave: Wave::Noise,
                freq: 600.0,
                ms: 50.0,
                volume: 0.14,
                decay: 45.0,
                ..Default::default()
            });
        }
        if self.dig_seconds < needed {
            return;
        }

        // Broken.
        let [x, y, z] = hit.cell;
        self.world.set(x, y, z, Block::Air);
        // Water next to the hole flows in (one block of it).
        let wet = DIRECTIONS
            .iter()
            .filter(|&&(_, dy, _)| dy >= 0)
            .any(|&(dx, dy, dz)| self.world.get(x + dx, y + dy, z + dz) == Block::Water);
        if wet {
            self.world.set(x, y, z, Block::Water);
        }
        self.update_chunks();
        self.digging = None;
        self.dig_seconds = 0.0;
        self.dug += 1;
        self.vitals.tiredness += health::TIRED_DIG;
        self.swing = 0.25;
        if creative {
            self.swing_wait = 0.25;
        }
        // In creative, things don't drop (you have them all already).
        let block = if creative { Block::Air } else { hit.block };
        match block {
            Block::GoldOre => {
                self.give(Item::Gold, 1);
                ctx.play(Sound::coin());
                self.say(format!("Gold! You have {}", self.count(Item::Gold)));
            }
            Block::CoalOre => {
                self.give(Item::Coal, 1);
                self.say(format!("Coal: {}", self.count(Item::Coal)));
            }
            _ => {}
        }
        if let Some(drop) = block.drop() {
            self.give(Item::Block(drop), 1);
        }
        // Now and then an apple falls out of the leaves.
        if block == Block::Leaves && ctx.rng.range(0.0, 1.0) < APPLE_CHANCE {
            self.give(Item::Apple, 1);
            self.say("An apple!");
        }
        ctx.play(Sound {
            name: "break",
            wave: Wave::Noise,
            freq: 900.0,
            slide_to: 150.0,
            ms: 140.0,
            volume: 0.3,
            decay: 120.0,
            ..Default::default()
        });
        let middle = vec3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
        for _ in 0..10 {
            let velocity = vec3(
                ctx.rng.range(-2.0, 2.0),
                ctx.rng.range(1.0, 4.5),
                ctx.rng.range(-2.0, 2.0),
            );
            let offset = vec3(
                ctx.rng.range(-0.3, 0.3),
                ctx.rng.range(-0.3, 0.3),
                ctx.rng.range(-0.3, 0.3),
            );
            self.crumbs.push(Crumb {
                position: middle + offset,
                velocity,
                age: 0.0,
                block: hit.block,
            });
        }
    }

    fn place(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        let pressed = input.was_mouse_pressed(MouseButton::Right) || input.was_pressed(Key::C);
        let held = input.is_mouse_down(MouseButton::Right) || input.is_down(Key::C);
        self.place_wait -= ctx.dt();
        if !(pressed || (held && self.place_wait <= 0.0)) {
            return;
        }
        let Some(hit) = self.target else {
            return;
        };
        let Some(block) = self.holding_block() else {
            return; // nothing to build with in this hand (food is eaten)
        };
        self.place_wait = PLACE_REPEAT;
        // On a plant, the block takes its place; on a block, it goes on the
        // face you look at.
        let [x, y, z] = if hit.plant.is_some() {
            hit.cell
        } else {
            [0, 1, 2].map(|i| hit.cell[i] + hit.normal[i])
        };
        if !World::inside(x, y, z) {
            if pressed {
                self.say("That's the edge of the world");
            }
            return;
        }
        if self.world.get(x, y, z).solid() {
            return;
        }
        // Not inside the duck, a duckling or a Hollow.
        let cell = vec3(x as f32, y as f32, z as f32);
        let overlaps = boxes_overlap(self.position, PLAYER, cell)
            || self
                .ducklings
                .iter()
                .any(|d| boxes_overlap(d.position, ducklings::DUCKLING, cell))
            || self
                .hollows
                .iter()
                .any(|h| h.fading.is_none() && boxes_overlap(h.position, night::HOLLOW, cell));
        if overlaps {
            return;
        }
        self.world.set(x, y, z, block);
        if self.mode == Mode::Survival {
            self.inventory.take_one_from(self.slot);
        }
        self.placed += 1;
        self.update_chunks();
        ctx.play(Sound {
            name: "place",
            wave: Wave::Square,
            freq: 170.0,
            slide_to: 110.0,
            ms: 70.0,
            volume: 0.2,
            decay: 50.0,
            ..Default::default()
        });
    }

    fn camera(&self) -> Camera3D {
        if matches!(self.stage, Stage::Title { .. }) {
            // Slowly round the island, looking at where you'll start.
            let middle = self.world.spawn + vec3(0.0, 2.0, 0.0);
            let angle = self.title_spin * 0.04 + self.world.spawn_yaw + std::f32::consts::PI;
            let from = middle + vec3(angle.sin() * 26.0, 11.0, angle.cos() * 26.0);
            let mut camera = Camera3D::looking_at(from, middle);
            camera.fov = 70f32.to_radians();
            return camera;
        }
        let eye = self.eye();
        let mut camera = Camera3D::new(eye);
        camera.yaw = self.yaw;
        camera.pitch = self.pitch;
        camera.fov = 70f32.to_radians();
        // A jolt when hurt, a shudder when scared.
        let jolt = self.vitals.hurt_flash;
        camera.pitch += (jolt * 40.0).sin() * jolt * 0.12;
        let shudder = self.scare;
        camera.yaw += (shudder * 70.0).sin() * shudder * 0.05;
        camera.pitch += (shudder * 53.0).cos() * shudder * 0.04;
        if let (true, Some(orbit)) = (self.behind_view, self.orbit) {
            let around = self.yaw + orbit;
            let from =
                self.position + vec3(around.sin(), 0.0, around.cos()) * 3.0 + vec3(0.0, 1.3, 0.0);
            return Camera3D::looking_at(from, self.position + vec3(0.0, 0.9, 0.0));
        }
        if self.behind_view {
            // Back from the eyes, but not through a wall.
            let back = self.look_direction() * -1.0;
            let mut distance = 0.0;
            while distance < 4.0 {
                let p = eye + back * (distance + 0.3);
                if self
                    .world
                    .get(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32)
                    .solid()
                {
                    break;
                }
                distance += 0.1;
            }
            camera.position = eye + back * distance;
        }
        camera
    }

    /// Where the day is: 0 sunrise, 0.25 noon, 0.5 sunset, 0.75 midnight.
    fn time_of_day(&self) -> f32 {
        if matches!(self.stage, Stage::Title { .. }) {
            return 0.512; // the title is always just after sunset
        }
        (self.time / DAY_SECONDS + menus::DAY_START).fract()
    }
}

impl Game for DuckCraft {
    fn init(&mut self, ctx: &mut Context) {
        if ctx.in_studio() {
            ctx.studio_note("Inside the Studio, Duck Craft starts a fresh island and doesn't save");
        }
    }

    /// Any way out of the game saves the island being played: the pause
    /// menu, the window's close button, the end of a run. (Without a save
    /// path, as in a replay or inside the Studio, saving does nothing.)
    fn quit(&mut self, _ctx: &mut Context) {
        if !matches!(self.stage, Stage::Title { .. }) {
            if let Err(error) = self.save() {
                eprintln!("duck_craft: couldn't save: {error}");
            }
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        self.mouse = ctx.input.mouse_position();
        self.update_music(ctx);
        match self.stage {
            Stage::Title { .. } => {
                // The island turns behind the menu; nothing else moves.
                self.title_spin += ctx.dt();
                self.update_title(ctx);
                self.report_stage(ctx);
                return;
            }
            Stage::Paused { .. } => {
                self.update_pause(ctx);
                self.report_stage(ctx);
                return;
            }
            Stage::Playing => {}
        }
        // The window holds the mouse pointer, for looking round. Escape
        // pauses (and lets go of it); a click takes it back.
        if ctx.frame() == 1 {
            ctx.lock_pointer(true); // a game that starts straight in play
        }
        let screen_was_open = self.screen.is_some();
        self.inventory_screen(ctx);
        if screen_was_open || self.screen.is_some() {
            // (Escape closed the inventory; it doesn't also pause.)
        } else if ctx.input.was_pressed(Key::Escape) {
            self.pause(ctx);
            self.report_stage(ctx);
            return;
        } else if !ctx.pointer_locked()
            && ctx.input.was_mouse_pressed(MouseButton::Left)
            && !self.vitals.fainted
        {
            ctx.lock_pointer(true);
            self.left_click_locked = true;
        }
        if !ctx.input.is_mouse_down(MouseButton::Left) {
            self.left_click_locked = false;
        }
        self.pointer_free = !ctx.pointer_locked();
        let dt = ctx.dt();
        self.time += dt;
        self.swing_wait = (self.swing_wait - dt).max(0.0);
        self.swing = (self.swing - dt).max(0.0);
        self.scare = (self.scare - dt).max(0.0);
        if ctx.input.was_pressed(Key::V) {
            self.behind_view = !self.behind_view;
        }
        if ctx.input.was_pressed(Key::H) {
            self.show_help = !self.show_help;
        }
        if self.time > 8.0 && self.show_help && self.dug + self.placed > 0 {
            self.show_help = false; // they've got the idea
        }
        self.day_and_night(ctx);

        let was_on_ground = self.on_ground;
        self.walk(ctx);
        self.target = self.world.raycast(self.eye(), self.look_direction(), REACH);
        self.aimed = self.aimed_duckling();
        self.aimed_hollow_now = self.aimed_hollow().map(|(i, _)| i);
        if self.aimed_hollow_now.is_some() {
            self.aimed = None; // a Hollow is nearer than any duckling
        }
        if self.aimed.is_some() || self.aimed_hollow_now.is_some() {
            self.target = None; // something stands in the way
        }
        if self.vitals.fainted {
            self.update_fainted(ctx);
        } else if self.mode == Mode::Survival {
            self.update_body(ctx, was_on_ground);
        }
        if self.screen.is_none() && !self.vitals.fainted {
            self.pick_block(ctx);
            if !self.strike(ctx) && !self.touch_duckling(ctx) {
                self.dig(ctx);
                self.place(ctx);
            }
        }
        self.update_ducklings(ctx);
        self.update_hollows(ctx);
        self.update_watcher(ctx);
        self.maybe_save(ctx);
        self.settle_sand();
        self.drop_falling(ctx);
        self.update_chunks();

        for crumb in &mut self.crumbs {
            crumb.velocity.y -= GRAVITY * 0.6 * dt;
            crumb.position += crumb.velocity * dt;
            crumb.age += dt;
        }
        self.crumbs.retain(|c| c.age < 0.7);
        self.message_seconds -= dt;

        // Rules that must always hold.
        let p = self.position;
        let size = SIZE as f32;
        ctx.check(
            "the duck stays on the island",
            (0.0..size).contains(&p.x) && (0.0..size).contains(&p.z) && p.y >= 0.0,
        );
        ctx.check("the duck is never inside a block", !self.body_hits(p));

        ctx.report("x", (p.x * 10.0).round() / 10.0);
        ctx.report("y", (p.y * 10.0).round() / 10.0);
        ctx.report("z", (p.z * 10.0).round() / 10.0);
        ctx.report("yaw", (self.yaw * 100.0).round() / 100.0);
        ctx.report("dug", self.dug);
        ctx.report("placed", self.placed);
        ctx.report("crafted", self.crafted);
        self.report_stage(ctx);
        ctx.report("day", self.day_number());
        ctx.report("flying", self.flying);
        ctx.report("health", self.vitals.health);
        ctx.report("food", self.vitals.food);
        ctx.report("air", self.vitals.air.ceil());
        ctx.report("fainted", self.vitals.fainted);
        ctx.report("eaten", self.eaten);
        ctx.report("picked", self.picked);
        ctx.check(
            "health is never over the top",
            self.vitals.health <= health::MAX_HEALTH,
        );
        ctx.report("inventory_open", self.screen.is_some());
        ctx.report(
            "held",
            self.screen
                .as_ref()
                .and_then(|s| s.held)
                .map_or("nothing", |s| s.item.name()),
        );
        ctx.report("view", if self.behind_view { "behind" } else { "eyes" });
        ctx.report("help", self.show_help);
        ctx.report(
            "message",
            if self.message_seconds > 0.0 {
                self.message.as_str()
            } else {
                ""
            },
        );
        ctx.report("gold", self.count(Item::Gold));
        ctx.report("coal", self.count(Item::Coal));
        ctx.report(
            "holding",
            self.holding().map_or("nothing", |s| s.item.name()),
        );
        ctx.report("on_ground", self.on_ground);
        ctx.report("in_water", self.in_water);
        ctx.report(
            "looking_at",
            if self.aimed.is_some() {
                "Duckling"
            } else {
                self.target.map_or("nothing", |t| t.name())
            },
        );
    }

    fn draw(&self, canvas: &mut Canvas) {
        let art = &self.art;
        let camera = self.camera();

        // The sky, the sun or moon, and stars, all behind the world.
        let menus::Sky {
            sun,
            daylight,
            dusk,
        } = sky_at(self.time_of_day());
        let night_sky = Color::from_hex(0x03040a);
        let day_sky = Color::from_hex(0x86c4f2);
        let sky = night_sky.lerp(day_sky, daylight);
        let horizon_glow = Color::from_hex(0xd8703a);
        canvas.clear(sky);
        // A lighter band down to the horizon (red at sunrise and sunset).
        let horizon = camera
            .world_to_screen(
                camera.position + vec3(camera.yaw.sin(), 0.0, camera.yaw.cos()) * 100.0,
                canvas,
            )
            .map_or(HEIGHT / 2.0, |p| p.y);
        let band_color = sky
            .lerp(Color::from_hex(0xd8ecfa), 0.5 * daylight)
            .lerp(horizon_glow, dusk);
        for i in 0..8 {
            let t = i as f32 / 8.0;
            let y = horizon - 64.0 + t * 64.0;
            canvas.fill_rect(Rect::new(0.0, y, WIDTH, 8.0), sky.lerp(band_color, t));
        }
        canvas.fill_rect(Rect::new(0.0, horizon, WIDTH, HEIGHT), band_color);
        // Far things fade into the horizon's colour. By day you see across
        // the sea; at night the dark comes in close; in a cave, closer.
        let eye = camera.position;
        let [ex, ey, ez] = [eye.x, eye.y, eye.z].map(|v| v.floor() as i32);
        let underground = World::inside(ex, ey, ez)
            && ey < self.world.sky[ez as usize * SIZE + ex as usize] - 2
            && light(&self.world.grid, &self.world.sky, ex, ey, ez) < 0.7;
        let (near, far) = if underground {
            (3.0, 22.0)
        } else {
            (6.0 + 34.0 * daylight, 30.0 + 90.0 * daylight)
        };
        let fog_color = if underground {
            Color::from_hex(0x050506)
        } else {
            band_color
        };
        canvas.set_fog(Some(Fog {
            color: fog_color,
            near,
            far,
        }));
        if daylight < 0.9 {
            let alpha = ((1.0 - daylight) * 170.0) as u8;
            for i in 0..60 {
                let a = random01(i, 0, 0, 77) * TAU;
                let h = random01(i, 1, 0, 77) * 0.9 + 0.08;
                let star = vec3(
                    a.cos() * (1.0 - h * h).sqrt(),
                    h,
                    a.sin() * (1.0 - h * h).sqrt(),
                );
                if let Some(p) = camera.world_to_screen(camera.position + star * 100.0, canvas) {
                    canvas.fill_rect(
                        Rect::new(p.x, p.y, 1.0, 1.0),
                        Color::WHITE.with_alpha(alpha),
                    );
                }
            }
        }
        if let Some(p) = camera.world_to_screen(camera.position + sun * 100.0, canvas) {
            canvas.fill_rect(
                Rect::from_center(p, 18.0, 18.0),
                Color::from_hex(0xfff2a0).with_alpha(120),
            );
            canvas.fill_rect(Rect::from_center(p, 12.0, 12.0), Color::from_hex(0xfffbe0));
        }
        if let Some(p) = camera.world_to_screen(camera.position - sun * 100.0, canvas) {
            // A pale moon, with a dark bite out of it.
            canvas.fill_rect(Rect::from_center(p, 9.0, 9.0), Color::from_hex(0xc8ccd4));
            canvas.fill_rect(
                Rect::new(p.x - 4.5, p.y - 4.5, 4.0, 9.0),
                sky.lerp(Color::from_hex(0xc8ccd4), 0.15),
            );
        }
        // The light: the sun by day, the moon (weak and cold) by night.
        let (direction, color) = if sun.y > -0.1 {
            (
                sun,
                Color::from_hex(0xfff4e0).lerp(horizon_glow, dusk * 0.6),
            )
        } else {
            (sun * -1.0, Color::from_hex(0x8090c0))
        };
        canvas.set_lighting(Lighting {
            sun_direction: direction,
            sun_color: color,
            sun_strength: 0.16 + 0.84 * daylight,
            ambient: 0.13 + 0.42 * daylight,
        });
        let still = Transform::default();
        canvas.draw_mesh(&art.ocean, &still, &camera);
        self.world.grid.draw(canvas, &camera);
        canvas.draw_mesh(&self.plant_mesh, &Transform::default(), &camera);
        // Clouds glow (so their undersides aren't grey), dimmed for the night.
        let drift = (self.time * 0.8).rem_euclid(CLOUD_PERIOD);
        let shade = (90.0 + 165.0 * daylight) as u8;
        // (Tinted as it's drawn: copying the whole mesh every frame to
        // colour it was slow.)
        let tint = Color::rgb(shade, shade, (shade as f32 * 1.05).min(255.0) as u8);
        canvas.draw_mesh_tinted(
            &art.clouds,
            &Transform::at(vec3(drift, 0.0, 0.0)),
            &camera,
            tint,
        );

        // Cracks on the block being dug.
        if let (Some(cell), Some(hit)) = (self.digging, self.target) {
            if let Some(needed) = hit.block.dig_time() {
                let stage = ((self.dig_seconds / needed) * 4.0).floor() as usize;
                let middle = vec3(
                    cell[0] as f32 + 0.5,
                    cell[1] as f32 + 0.5,
                    cell[2] as f32 + 0.5,
                );
                let place = Transform::at(middle).sized(1.004);
                canvas.draw_mesh(&art.cracks[stage.min(3)], &place, &camera);
            }
        }
        for falling in &self.falling {
            if let Some(cube) = &art.cubes[falling.block as usize] {
                let middle = vec3(
                    falling.x as f32 + 0.5,
                    falling.y + 0.5,
                    falling.z as f32 + 0.5,
                );
                canvas.draw_mesh(cube, &Transform::at(middle), &camera);
            }
        }
        for crumb in &self.crumbs {
            if let Some(cube) = &art.cubes[crumb.block as usize] {
                let place = Transform::at(crumb.position)
                    .rotated(vec3(crumb.age * 6.0, crumb.age * 4.0, 0.0))
                    .sized(0.13);
                canvas.draw_mesh(cube, &place, &camera);
            }
        }

        // The duck, when you're behind it: limbs swing as it walks, the
        // arms reach out when it digs or builds, the head follows the view.
        if self.behind_view {
            let going =
                (self.velocity.x * self.velocity.x + self.velocity.z * self.velocity.z).sqrt();
            let stride = (going / WALK_SPEED).min(1.5);
            let swing = (self.time * 9.0).sin() * 0.8 * stride;
            let airborne = !self.on_ground && !self.in_water;
            let lift = if self.digging.is_some() || self.place_wait > 0.0 || self.swing > 0.0 {
                1.3 + (self.time * 18.0).sin() * 0.4
            } else if airborne {
                0.6
            } else {
                0.0
            };
            let pose = Pose {
                yaw: self.yaw,
                pitch: self.pitch,
                swing,
                lift,
            };
            art.duck.draw(canvas, &camera, self.position, pose);
            // (Labels only while playing: a menu over the world has its own.)
            if let (Stage::Playing, Some(rect)) = (
                &self.stage,
                screen_box(&camera, canvas, self.position, 1.0, 1.75),
            ) {
                canvas.label("player", rect);
            }
        }

        // See-through things last (they don't hide what's drawn after
        // them): the duck's shadow on the ground under it, and a faint
        // glassy box over the block you look at.
        if self.behind_view {
            self.draw_shadow(canvas, &camera, self.position, 0.8);
        }
        self.draw_ducklings(canvas, &camera);
        self.draw_night(canvas, &camera);
        if let Some(hit) = self.target.filter(|t| t.plant.is_none()) {
            let [x, y, z] = hit.cell.map(|v| v as f32);
            let place = Transform::at(vec3(x + 0.5, y + 0.5, z + 0.5)).sized(1.006);
            let glass = Color::rgba(255, 255, 255, 36);
            canvas.draw_mesh_tinted(&art.highlight, &place, &camera, glass);
        }

        // The edges of the block you're looking at.
        if let Some(hit) = self.target {
            let [x, y, z] = hit.cell.map(|v| v as f32);
            let e = 0.002;
            // A plant's box is smaller than a block.
            let (inset, height) = hit.plant.map_or((0.0, 1.0), |p| (0.15, p.height()));
            let low = vec3(x + inset - e, y - e, z + inset - e);
            let high = vec3(x + 1.0 - inset + e, y + height + e, z + 1.0 - inset + e);
            draw_box_edges(canvas, &camera, low, high, Color::rgba(20, 20, 24, 200));
            if let Some((p, _)) = camera.project(vec3(x + 0.5, y + 0.5, z + 0.5), canvas) {
                canvas.label("target", Rect::from_center(p, 4.0, 4.0));
            }
        }

        // Under water, everything is blue.
        let eye = camera.position;
        if self.world.get(
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        ) == Block::Water
        {
            canvas.fill_rect(
                Rect::new(0.0, 0.0, WIDTH, HEIGHT),
                Color::rgba(30, 70, 170, 120),
            );
        }

        if matches!(self.stage, Stage::Title { .. }) {
            self.draw_title(canvas);
            return;
        }
        if !self.behind_view {
            self.draw_hand(canvas, &camera);
        }
        self.draw_vignette(canvas);
        self.draw_hud(canvas);
        if self.mode == Mode::Survival {
            self.draw_body(canvas);
        }
        self.draw_announcement(canvas);
        self.draw_inventory(canvas, &camera, self.mouse);
        if self.scare > 0.0 {
            // The Watcher, seen: a white flash, gone almost at once.
            let alpha = ((self.scare - 0.45).max(0.0) / 0.15 * 200.0) as u8;
            canvas.fill_rect(
                Rect::new(0.0, 0.0, WIDTH, HEIGHT),
                Color::rgba(255, 255, 255, alpha),
            );
        }
        self.draw_pause(canvas);
    }
}

impl DuckCraft {
    /// Where the game is (title, playing, paused) and how it's played.
    fn report_stage(&self, ctx: &mut Context) {
        let stage = match &self.stage {
            Stage::Title { selected, .. } => format!("title {selected}"),
            Stage::Playing => String::from("playing"),
            Stage::Paused { selected } => format!("paused {selected}"),
        };
        ctx.report("stage", stage);
        ctx.report("mode", self.mode.name());
    }

    /// A soft round shadow on the ground under something standing at
    /// `feet`, `size` across, fainter and smaller the higher it is.
    fn draw_shadow(&self, canvas: &mut Canvas, camera: &Camera3D, feet: Vec3, size: f32) {
        let [x, z] = [feet.x, feet.z];
        let ground = (0..=(feet.y.floor() as i32).min(TALL as i32))
            .rev()
            .find(|&y| {
                self.world
                    .get(x.floor() as i32, y - 1, z.floor() as i32)
                    .solid()
            });
        if let Some(y) = ground {
            let height = feet.y - y as f32;
            let size = (size - height * 0.12).max(size * 0.35);
            let alpha = (110.0 - height * 18.0).max(20.0) as u8;
            let place = Transform::at(vec3(x, y as f32 + 0.01, z)).scaled(vec3(size, 0.004, size));
            canvas.draw_mesh_tinted(
                &self.art.shadow,
                &place,
                camera,
                Color::rgba(0, 0, 0, alpha),
            );
        }
    }

    /// Your own right arm, bottom right of the view, holding the block in
    /// hand. It bobs as you walk and swings when you dig or build.
    fn draw_hand(&self, canvas: &mut Canvas, camera: &Camera3D) {
        let art = &self.art;
        let bob = if self.walking && self.on_ground {
            (self.time * 10.0).sin() * 0.03
        } else {
            0.0
        };
        let swing = if self.digging.is_some() || self.place_wait > 0.0 || self.swing > 0.0 {
            (self.time * 18.0).sin().max(0.0) * 0.7
        } else {
            0.0
        };
        // In view space (x right, y up, z ahead): it moves and turns with
        // the camera, and is drawn in front of the world. The shoulder is
        // below the bottom of the view; the arm points forward and up into
        // it, like a hand held out.
        let shoulder = vec3(0.36, -0.6 + bob, 0.1);
        let reach = std::f32::consts::FRAC_PI_2 + 0.6 - swing;
        let arm = Transform::at(shoulder).rotated(vec3(reach, 0.0, 0.0));
        canvas.draw_mesh_in_view(&art.duck.arm, &arm, camera);
        // The hand is 10 pixels down the arm: what it holds sits just past it.
        let hand = shoulder + vec3(0.0, -11.0 * SKIN_PIXEL, 0.0).rotate_x(reach);
        match self.holding().map(|s| s.item) {
            Some(Item::Block(block)) => {
                if let Some(cube) = &art.cubes[block as usize] {
                    let place = Transform::at(hand).rotated(vec3(0.0, 0.5, 0.0)).sized(0.24);
                    canvas.draw_mesh_in_view(cube, &place, camera);
                }
            }
            Some(item) => {
                if let Some(card) = art.item_cards.get(&item) {
                    let eating = self.eating_seconds > 0.0;
                    let nibble = if eating {
                        (self.time * 30.0).sin() * 0.02
                    } else {
                        0.0
                    };
                    let at = if eating {
                        vec3(0.05, -0.28 + nibble, 0.45)
                    } else {
                        hand + vec3(0.0, 0.08, 0.0)
                    };
                    let place = Transform::at(at).rotated(vec3(0.0, -0.6, 0.15)).sized(0.32);
                    canvas.draw_mesh_in_view(card, &place, camera);
                }
            }
            None => {}
        }
    }

    /// A stack's picture with its count in the corner (none for one),
    /// the picture's top left at `at`.
    fn draw_stack(&self, canvas: &mut Canvas, stack: Option<Stack>, at: Vec2) {
        let Some(stack) = stack else {
            return;
        };
        if let Some(icon) = self.art.item_icons.get(&stack.item) {
            canvas.blit_sprite(icon, at, Blit::default());
        }
        if stack.count > 1 {
            let text = stack.count.to_string();
            let w = Canvas::text_width(&text, 1);
            let corner = at + vec2(17.0 - w, 11.0);
            canvas.draw_text(&text, corner + vec2(1.0, 1.0), 1, Color::rgb(40, 40, 40));
            canvas.draw_text(&text, corner, 1, Color::WHITE);
        }
    }

    fn draw_hud(&self, canvas: &mut Canvas) {
        let shadow = Color::rgba(0, 0, 0, 160);

        // The crosshair.
        let (cx, cy) = (WIDTH / 2.0, HEIGHT / 2.0);
        for (rect, color) in [
            (Rect::new(cx - 5.0, cy - 1.0, 11.0, 3.0), shadow),
            (Rect::new(cx - 1.0, cy - 5.0, 3.0, 11.0), shadow),
            (Rect::new(cx - 4.0, cy, 9.0, 1.0), Color::WHITE),
            (Rect::new(cx, cy - 4.0, 1.0, 9.0), Color::WHITE),
        ] {
            canvas.fill_rect(rect, color);
        }

        // (In first person the arm and the block in hand are drawn in 3D,
        // see `draw_hand`.)

        // The hotbar: nine framed slots, the one in hand outlined.
        let slot_size = HOTBAR_SLOT;
        let left = (WIDTH - slot_size * 9.0) / 2.0;
        let top = HEIGHT - slot_size - 3.0;
        canvas.fill_rect(
            Rect::new(
                left - 2.0,
                top - 2.0,
                slot_size * 9.0 + 4.0,
                slot_size + 4.0,
            ),
            Color::rgba(0, 0, 0, 140),
        );
        for i in 0..HOTBAR_SLOTS {
            let x = left + i as f32 * slot_size;
            let slot = Rect::new(x + 1.0, top + 1.0, slot_size - 2.0, slot_size - 2.0);
            canvas.fill_rect(slot, Color::rgba(110, 110, 120, 140));
            canvas.draw_rect(slot, Color::rgba(40, 40, 46, 200));
            self.draw_stack(canvas, self.inventory.slots[i], vec2(x + 3.0, top + 3.0));
            if i == self.slot {
                canvas.draw_rect(
                    Rect::new(x - 1.0, top - 1.0, slot_size + 2.0, slot_size + 2.0),
                    Color::WHITE,
                );
                canvas.draw_rect(Rect::new(x, top, slot_size, slot_size), Color::WHITE);
            }
        }
        canvas.label("hotbar", Rect::new(left, top, slot_size * 9.0, slot_size));

        // Gold, coal and the clock, top left.
        let hours = (self.time_of_day() * 24.0 + 6.0) % 24.0;
        let clock = format!(
            "{:02}:{:02}",
            hours as u32,
            ((hours.fract()) * 60.0) as u32 / 10 * 10
        );
        let status = format!("Day {}  {clock}", self.day_number());
        canvas.draw_text(&status, vec2(5.0, 5.0), 1, Color::BLACK);
        canvas.draw_text(&status, vec2(4.0, 4.0), 1, Color::from_hex(0xfff2a0));

        // A message over the hotbar.
        if self.message_seconds > 0.0 {
            let alpha = (self.message_seconds.min(1.0) * 255.0) as u8;
            let y = top - 36.0; // above the hearts, bread and bubbles
            let w = Canvas::text_width(&self.message, 1);
            canvas.fill_rect(
                Rect::new((WIDTH - w) / 2.0 - 3.0, y - 2.0, w + 6.0, 11.0),
                Color::rgba(0, 0, 0, alpha / 2),
            );
            canvas.draw_text(
                &self.message,
                vec2((WIDTH - w) / 2.0, y),
                1,
                Color::WHITE.with_alpha(alpha),
            );
        }

        if self.pointer_free {
            let text = "Click to play    Esc again: quit";
            let w = Canvas::text_width(text, 1) + 8.0;
            let panel = Rect::new((WIDTH - w) / 2.0, HEIGHT / 2.0 + 14.0, w, 13.0);
            canvas.fill_rect(panel, Color::rgba(0, 0, 0, 170));
            canvas.draw_text_centered(text, panel.y + 3.0, 1, Color::WHITE);
        }

        let quiet = self.stage != Stage::Playing || self.vitals.fainted;
        if self.show_help && self.screen.is_none() && !quiet {
            let last = match self.mode {
                Mode::Survival => "Lamps keep the dark away. Mostly.",
                Mode::Creative => "Space twice: fly (Shift down)",
            };
            let lines = [
                "DUCK CRAFT",
                "WASD walk   Shift run   Space jump",
                "Mouse or arrows: look   Esc: pause",
                "Click / X: dig, hit   Right / C: build",
                "1-9 or wheel: pick   E: bag and crafting",
                "Hold right click with food: eat",
                "Right click a duckling: it follows you",
                "V: view   F5: save   H: hide this",
                last,
            ];
            let panel = Rect::new(46.0, 26.0, 228.0, 107.0);
            canvas.fill_rect(panel, Color::rgba(0, 0, 0, 160));
            for (i, line) in lines.iter().enumerate() {
                let color = if i == 0 {
                    Color::from_hex(0xffd45a)
                } else {
                    Color::WHITE
                };
                canvas.draw_text_centered(line, panel.y + 6.0 + i as f32 * 11.0, 1, color);
            }
        }
    }
}

/// The twelve edges of the box from `low` to `high`, as lines.
fn draw_box_edges(canvas: &mut Canvas, camera: &Camera3D, low: Vec3, high: Vec3, color: Color) {
    let corner = |i: usize| {
        vec3(
            if i & 1 == 0 { low.x } else { high.x },
            if i & 2 == 0 { low.y } else { high.y },
            if i & 4 == 0 { low.z } else { high.z },
        )
    };
    let points: Vec<Option<Vec2>> = (0..8)
        .map(|i| camera.world_to_screen(corner(i), canvas))
        .collect();
    for a in 0..8 {
        for bit in [1, 2, 4] {
            let b = a | bit;
            if b != a {
                if let (Some(p), Some(q)) = (points[a], points[b]) {
                    canvas.draw_line(p, q, color);
                }
            }
        }
    }
}

/// The screen rectangle of something `width` wide and `height` tall
/// standing at `feet`, or `None` if it's behind the camera.
fn screen_box(
    camera: &Camera3D,
    canvas: &Canvas,
    feet: Vec3,
    width: f32,
    height: f32,
) -> Option<Rect> {
    let (bottom, _) = camera.project(feet, canvas)?;
    let (top, _) = camera.project(feet + vec3(0.0, height, 0.0), canvas)?;
    let half = width / 2.0 * (bottom.y - top.y).abs() / height;
    Some(Rect::new(
        bottom.x - half,
        top.y,
        half * 2.0,
        bottom.y - top.y,
    ))
}

/// The game, with its world and art.
/// A new island, never saved, already playing (for tests).
#[cfg(test)]
fn new_game() -> Result<DuckCraft, LoadError> {
    save::load_or_new(None)
}

/// The game as it starts: on the title screen. Without a save path it's
/// the headless game (the same island every time, never saved), which
/// replays and fuzz runs play.
fn title_game(save: Option<std::path::PathBuf>) -> Result<DuckCraft, LoadError> {
    let mut game = save::load_or_new(save)?;
    game.stage = Stage::Title {
        selected: 0,
        confirm: false,
    };
    Ok(game)
}

fn config() -> Config {
    Config {
        title: String::from("Duck Craft - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Only the game in its own window saves: a headless run (a replay, a
    // fuzz run, a video) and a game inside Duck Forge Studio always start
    // from the same new island, so they can be replayed exactly.
    let fresh = duckforge::is_headless() || duckforge::in_studio();
    let save = (!fresh).then(|| std::path::PathBuf::from(save::SAVE_FILE));
    let game = title_game(save)?;
    duckforge::run(config(), game)
}

// Play tests: `cargo test --example duck_craft`. The bot that records
// playthroughs/duck_craft.json: `cargo test --example duck_craft -- --ignored`.
mod ducklings;
mod health;
mod inventory;
mod items;
mod menus;
mod night;
mod plants;
mod save;

use ducklings::{Duckling, DucklingModel};
use items::{HOTBAR_SLOTS, Inventory, Item, SLOTS, Stack};
use menus::{Mode, Stage, sky_at};
use night::{Hollow, Watcher};
use plants::Plant;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;
