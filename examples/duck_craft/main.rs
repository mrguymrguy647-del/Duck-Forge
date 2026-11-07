//! Duck Craft: a little Minecraft-style world of blocks, made with
//! duckforge and Duck Quill.
//!
//! An island grows from noise: grass hills, sandy beaches, the sea, trees,
//! stone with coal and gold inside, and caves. Walk it as the duck, dig any
//! block, pick it up, and build with it. The sun crosses the sky; at night,
//! the lamps you place glow.
//!
//!     cargo run --release --example duck_craft
//!
//! - **W A S D** walk, **Shift** run, **Space** jump (and swim up)
//! - **mouse** or **arrow keys** look (the window holds on to the pointer;
//!   **Escape** lets go of it and a click takes it back)
//! - **left click** or **X** (hold) digs the block you look at;
//!   **right click** or **C** places the block in your hand
//! - **1 to 9** or the **mouse wheel** pick a block; **middle click** picks the one you look at
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
//!   above" shadow.
//! - The duck is a blocky character with a skin, like a Minecraft player
//!   (`Duck`, `art/make_skin.py`): boxes for the head, beak, body, arms and
//!   legs, each face painted from one 64 x 32 picture, limbs swinging as it
//!   walks. In first person you see your own arm holding the block in hand.
//!
//! Without a screen (or to test):
//!
//!     cargo run --release --no-default-features --example duck_craft -- --headless --inputs playthroughs/duck_craft.json --sheet-events --out out
//!     cargo test --no-default-features --example duck_craft

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
const SEED: u32 = 2026; // a different number grows a different island

const WALK_SPEED: f32 = 4.3; // blocks per second
const RUN_SPEED: f32 = 6.0;
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

const ASSETS: &str = "examples/duck_craft/assets";

// ---- Blocks ----

/// One kind of block. `Air` is nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// What's in the hotbar, slot 1 to 9.
const HOTBAR: [Block; 9] = [
    Block::Dirt,
    Block::Cobblestone,
    Block::Planks,
    Block::Log,
    Block::Glass,
    Block::Brick,
    Block::Sand,
    Block::Leaves,
    Block::Lamp,
];

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
    /// For each column, the lowest y with nothing above it that casts a
    /// shadow: air there and above is under the open sky.
    sky: Vec<i32>,
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
        if World::inside(x, y, z) {
            self.grid.set(x, y, z, block as BlockId);
            self.update_sky(x, z);
        }
    }

    /// Builds again the chunks that changed, shading each face by the light
    /// in the cell it looks into.
    fn update_chunks(&mut self, palette: &BlockPalette) {
        let sky = &self.sky;
        self.grid
            .update_meshes_shaded(palette, |grid, x, y, z| light(grid, sky, x, y, z));
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
    fn ground_height(x: i32, z: i32) -> i32 {
        let (fx, fz) = (x as f32, z as f32);
        let base = 0.5 * noise2(fx / 22.0, fz / 22.0, SEED)
            + 0.3 * noise2(fx / 11.0, fz / 11.0, SEED + 1)
            + 0.2 * noise2(fx / 5.0, fz / 5.0, SEED + 2);
        let hills = (noise2(fx / 16.0, fz / 16.0, SEED + 3) - 0.55).max(0.0) * 34.0;
        let mut h = 6.5 + base * 17.0 + hills;
        // An island: the land sinks under the sea near the edges.
        let edge = x.min(z).min(SIZE as i32 - 1 - x).min(SIZE as i32 - 1 - z) as f32;
        if edge < 12.0 {
            h = lerp(5.0, h, smooth(edge / 12.0));
        }
        (h.round() as i32).clamp(2, TALL as i32 - 12)
    }

    /// Grows the island.
    fn generate() -> World {
        let mut world = World {
            grid: BlockWorld::with_chunk_size(SIZE, TALL, SIZE, CHUNK),
            sky: vec![0; SIZE * SIZE],
            spawn: Vec3::ZERO,
            spawn_yaw: 0.0,
        };
        // Bedrock all the way down: the bottom layer's undersides aren't drawn.
        world.grid.set_below(Block::Bedrock as BlockId);
        let heights: Vec<i32> = (0..SIZE * SIZE)
            .map(|i| World::ground_height((i % SIZE) as i32, (i / SIZE) as i32))
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
                        let r = random01(x, y, z, SEED + 4);
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
                    let n = noise3(x as f32 / 8.0, y as f32 / 5.0, z as f32 / 8.0, SEED + 5);
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
                if on_grass && room && random01(x, 0, z, SEED + 6) < 0.014 {
                    trees.push((x, z));
                    world.grow_tree(x, h + 1, z, random01(x, 1, z, SEED + 6));
                }
            }
        }

        for z in 0..SIZE as i32 {
            for x in 0..SIZE as i32 {
                world.update_sky(x, z);
            }
        }

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
                    if corner && (dy == 1 || random01(x + dx, y, z + dz, SEED + 7) < 0.5) {
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
}

// ---- Art ----

/// A 16 x 16 tile copied out of the atlas, as a picture of its own (for
/// textures that repeat, like the sea, and for the hotbar).
fn tile_picture(atlas: &Tileset, name: &str) -> Sprite {
    let tile = atlas.frame(name, 0.0);
    let mut pixels = Vec::with_capacity(tile.width() * tile.height());
    for y in 0..tile.height() {
        for x in 0..tile.width() {
            pixels.push(tile.get_pixel(x, y).unwrap_or(Color::rgba(0, 0, 0, 0)));
        }
    }
    Sprite::from_pixels(tile.width(), tile.height(), pixels)
}

/// Pictures, meshes and sounds, made once and shared.
struct Art {
    /// How each kind of block looks, for the world's chunks.
    palette: BlockPalette,
    /// Each kind of block as a whole cube (for flying crumbs).
    cubes: Vec<Option<Mesh>>,
    /// The four stages of cracks, drawn over a block being dug.
    cracks: Vec<Mesh>,
    /// Each block's picture for the hotbar.
    icons: Vec<Option<Sprite>>,
    /// The sea all round the island, out to the horizon.
    ocean: Mesh,
    clouds: Mesh,
    duck: Duck,
    wind: Wav,
}

impl Art {
    fn load() -> Result<Art, LoadError> {
        let atlas = Tileset::load(format!("{ASSETS}/blocks.json"))?;
        let mut kinds = Vec::new();
        let mut cubes = Vec::new();
        let mut icons = Vec::new();
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
                    icons.push(Some(tile_picture(&atlas, tiles[1])));
                }
                None => {
                    cubes.push(None);
                    icons.push(None);
                }
            }
        }
        let cracks = (1..=4)
            .map(|n| {
                let name = format!("crack_{n}");
                Mesh::block(&atlas, [name.as_str(); 3])
            })
            .collect();
        Ok(Art {
            palette: BlockPalette::new(&atlas, &kinds),
            cubes,
            cracks,
            icons,
            ocean: ocean(tile_picture(&atlas, "water")),
            clouds: clouds(tile_picture(&atlas, "cloud")),
            duck: Duck::load(Sprite::load_png(format!("{ASSETS}/skin.png"))?),
            wind: Wav::load(format!("{ASSETS}/hum.wav"))?,
        })
    }
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

    /// Draws the duck standing at `feet`, in `pose`.
    fn draw(&self, canvas: &mut Canvas, camera: &Camera3D, feet: Vec3, pose: Pose) {
        let Pose {
            yaw,
            pitch,
            swing,
            lift,
        } = pose;
        // A part's pivot in the world: its place on the body, turned.
        let at = |local: Vec3| feet + local.rotate_y(yaw);
        let part = |pivot: Vec3, rotation: Vec3| {
            Transform::at(at(pivot)).rotated(vec3(rotation.x, yaw + rotation.y, rotation.z))
        };
        canvas.draw_mesh(
            &self.body,
            &part(vec3(0.0, Self::HIP, 0.0), Vec3::ZERO),
            camera,
        );
        canvas.draw_mesh(
            &self.head,
            &part(vec3(0.0, Self::NECK, 0.0), vec3(pitch, 0.0, 0.0)),
            camera,
        );
        let hand = 6.0 * SKIN_PIXEL;
        let thigh = 2.0 * SKIN_PIXEL;
        canvas.draw_mesh(
            &self.leg,
            &part(vec3(-thigh, Self::HIP, 0.0), vec3(swing, 0.0, 0.0)),
            camera,
        );
        canvas.draw_mesh(
            &self.leg,
            &part(vec3(thigh, Self::HIP, 0.0), vec3(-swing, 0.0, 0.0)),
            camera,
        );
        canvas.draw_mesh(
            &self.arm,
            &part(
                vec3(-hand, Self::SHOULDER, 0.0),
                vec3(-swing - lift, 0.0, 0.1),
            ),
            camera,
        );
        canvas.draw_mesh(
            &self.arm,
            &part(
                vec3(hand, Self::SHOULDER, 0.0),
                vec3(swing - lift, 0.0, -0.1),
            ),
            camera,
        );
    }
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
    carried: [u32; BLOCK_KINDS],
    gold: u32,
    coal: u32,
    dug: u32,
    placed: u32,

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
    wind: Option<LoopId>,
}

impl DuckCraft {
    fn new(art: Rc<Art>, mut world: World) -> DuckCraft {
        world.update_chunks(&art.palette);
        let mut carried = [0; BLOCK_KINDS];
        carried[Block::Planks as usize] = 32;
        carried[Block::Glass as usize] = 16;
        carried[Block::Brick as usize] = 32;
        carried[Block::Lamp as usize] = 8;
        DuckCraft {
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
            slot: 2,
            carried,
            gold: 0,
            coal: 0,
            dug: 0,
            placed: 0,
            digging: None,
            dig_seconds: 0.0,
            place_wait: 0.0,
            step_wait: 0.0,
            target: None,
            crumbs: Vec::new(),
            message: String::new(),
            message_seconds: 0.0,
            time: 0.0,
            wind: None,
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
        let low = feet - vec3(HALF_WIDTH, 0.0, HALF_WIDTH);
        let high = feet + vec3(HALF_WIDTH, BODY_HEIGHT, HALF_WIDTH);
        let shrink = 1e-4;
        for y in low.y.floor() as i32..=(high.y - shrink).floor() as i32 {
            for z in low.z.floor() as i32..=(high.z - shrink).floor() as i32 {
                for x in low.x.floor() as i32..=(high.x - shrink).floor() as i32 {
                    if self.world.blocks_body(x, y, z) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Moves the duck along one axis (0 x, 1 y, 2 z), in steps small enough
    /// never to pass through a block, stopping flush against one. Returns
    /// whether something stopped it.
    fn move_axis(&mut self, axis: usize, distance: f32) -> bool {
        let steps = (distance.abs() / 0.4).ceil().max(1.0) as i32;
        let step = distance / steps as f32;
        for _ in 0..steps {
            let mut moved = self.position;
            let half = [HALF_WIDTH, 0.0, HALF_WIDTH];
            match axis {
                0 => moved.x += step,
                1 => moved.y += step,
                _ => moved.z += step,
            }
            if !self.body_hits(moved) {
                self.position = moved;
                continue;
            }
            // Flush against the block it ran into.
            let gap = 1e-3;
            let value = |v: Vec3| [v.x, v.y, v.z][axis];
            let edge = if step > 0.0 {
                let top = if axis == 1 { BODY_HEIGHT } else { half[axis] };
                (value(moved) + top).floor() - top - gap
            } else {
                (value(moved) - half[axis]).floor() + 1.0 + half[axis] + gap
            };
            let mut flush = self.position;
            match axis {
                0 => flush.x = edge,
                1 => flush.y = edge,
                _ => flush.z = edge,
            }
            if !self.body_hits(flush) {
                self.position = flush;
            }
            return true;
        }
        false
    }

    fn say(&mut self, text: impl Into<String>) {
        self.message = text.into();
        self.message_seconds = 2.5;
    }

    fn holding(&self) -> Block {
        HOTBAR[self.slot]
    }

    /// Builds again the chunks the last dig or build changed.
    fn update_chunks(&mut self) {
        self.world.update_chunks(&self.art.palette);
    }

    fn walk(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let input = &ctx.input;

        // Look: the mouse (while the window holds the pointer), or the arrow keys.
        let mouse = if ctx.pointer_locked() {
            input.mouse_delta()
        } else {
            Vec2::ZERO
        };
        self.yaw += mouse.x * MOUSE_LOOK + input.axis(Key::Left, Key::Right) * TURN_SPEED * dt;
        self.pitch += -mouse.y * MOUSE_LOOK + input.axis(Key::Down, Key::Up) * TURN_SPEED * dt;
        self.pitch = self.pitch.clamp(-1.55, 1.55);
        self.yaw = self.yaw.rem_euclid(TAU);

        // Walk along the ground, the way you're facing.
        let ahead = vec3(self.yaw.sin(), 0.0, self.yaw.cos());
        let right = vec3(self.yaw.cos(), 0.0, -self.yaw.sin());
        let wish = ahead * input.axis(Key::S, Key::W) + right * input.axis(Key::A, Key::D);
        self.walking = wish.length() > 0.0;
        let running = input.is_down(Key::LeftShift);
        let mut speed = if running { RUN_SPEED } else { WALK_SPEED };
        if self.in_water {
            speed *= 0.55;
        }
        let wish = if self.walking {
            wish.normalized() * speed
        } else {
            Vec3::ZERO
        };
        // Quick to start and stop on the ground, floaty in the air.
        let grip = if self.on_ground { 30.0 } else { 9.0 };
        self.velocity.x = approach(self.velocity.x, wish.x, grip * dt);
        self.velocity.z = approach(self.velocity.z, wish.z, grip * dt);

        // Jump, swim, fall.
        let jump = input.is_down(Key::Space);
        if self.in_water {
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

        // Walking into a step one block high hops up it.
        if (blocked_x || blocked_z) && self.on_ground && self.walking {
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
            if let Some(hit) = self.target {
                let wanted = hit.block.drop().unwrap_or(hit.block);
                if let Some(slot) = HOTBAR.iter().position(|&b| b == wanted) {
                    self.slot = slot;
                }
            }
        }
        if self.slot != before {
            let name = self.holding().name();
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
        match hit.block {
            Block::GoldOre => {
                self.gold += 1;
                ctx.play(Sound::coin());
                self.say(format!("Gold! You have {}", self.gold));
            }
            Block::CoalOre => {
                self.coal += 1;
                self.say(format!("Coal: {}", self.coal));
            }
            _ => {}
        }
        if let Some(drop) = hit.block.drop() {
            self.carried[drop as usize] += 1;
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
        self.place_wait = PLACE_REPEAT;
        let block = self.holding();
        let [x, y, z] = [0, 1, 2].map(|i| hit.cell[i] + hit.normal[i]);
        if !World::inside(x, y, z) {
            if pressed {
                self.say("That's the edge of the world");
            }
            return;
        }
        if self.world.get(x, y, z).solid() {
            return;
        }
        if self.carried[block as usize] == 0 {
            if pressed {
                self.say(format!("No {} left: dig some", block.name()));
            }
            return;
        }
        // Not inside the duck.
        let low = self.position - vec3(HALF_WIDTH, 0.0, HALF_WIDTH);
        let high = self.position + vec3(HALF_WIDTH, BODY_HEIGHT, HALF_WIDTH);
        let overlaps = (x as f32) < high.x
            && (x as f32 + 1.0) > low.x
            && (y as f32) < high.y
            && (y as f32 + 1.0) > low.y
            && (z as f32) < high.z
            && (z as f32 + 1.0) > low.z;
        if overlaps {
            return;
        }
        self.world.set(x, y, z, block);
        self.carried[block as usize] -= 1;
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
        let eye = self.eye();
        let mut camera = Camera3D::new(eye);
        camera.yaw = self.yaw;
        camera.pitch = self.pitch;
        camera.fov = 70f32.to_radians();
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
        (self.time / DAY_SECONDS + 0.05).fract()
    }
}

impl Game for DuckCraft {
    fn update(&mut self, ctx: &mut Context) {
        // The window holds the mouse pointer, for looking round. Escape lets
        // go of it (and quits if it's already free); a click takes it back.
        if ctx.frame() == 1 {
            ctx.lock_pointer(true);
        }
        if ctx.input.was_pressed(Key::Escape) {
            if ctx.pointer_locked() {
                ctx.lock_pointer(false);
            } else {
                ctx.quit();
            }
        } else if !ctx.pointer_locked() && ctx.input.was_mouse_pressed(MouseButton::Left) {
            ctx.lock_pointer(true);
            self.left_click_locked = true;
        }
        if !ctx.input.is_mouse_down(MouseButton::Left) {
            self.left_click_locked = false;
        }
        self.pointer_free = !ctx.pointer_locked();
        let dt = ctx.dt();
        self.time += dt;
        if self.wind.is_none() {
            self.wind = Some(ctx.play_loop(&self.art.wind, 0.2));
        }
        if ctx.input.was_pressed(Key::V) {
            self.behind_view = !self.behind_view;
        }
        if ctx.input.was_pressed(Key::H) {
            self.show_help = !self.show_help;
        }
        if self.time > 8.0 && self.show_help && self.dug + self.placed > 0 {
            self.show_help = false; // they've got the idea
        }

        self.walk(ctx);
        self.target = self.world.raycast(self.eye(), self.look_direction(), REACH);
        self.pick_block(ctx);
        self.dig(ctx);
        self.place(ctx);

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
        ctx.report("gold", self.gold);
        ctx.report("coal", self.coal);
        ctx.report("holding", self.holding().name());
        ctx.report("on_ground", self.on_ground);
        ctx.report("in_water", self.in_water);
        ctx.report(
            "looking_at",
            self.target.map_or("nothing", |t| t.block.name()),
        );
    }

    fn draw(&self, canvas: &mut Canvas) {
        let art = &self.art;
        let camera = self.camera();

        // The sky, the sun or moon, and stars, all behind the world.
        let day = self.time_of_day();
        let angle = day * TAU;
        let sun = vec3(angle.cos(), angle.sin(), 0.35).normalized();
        let daylight = smooth(((sun.y + 0.15) / 0.4).clamp(0.0, 1.0));
        let dusk = (1.0 - (sun.y.abs() / 0.3).min(1.0)) * 0.8;
        let night_sky = Color::from_hex(0x0b1226);
        let day_sky = Color::from_hex(0x86c4f2);
        let sky = night_sky.lerp(day_sky, daylight);
        let horizon_glow = Color::from_hex(0xf2a05a);
        canvas.clear(sky);
        // A lighter band down to the horizon (orange at sunrise and sunset).
        let horizon = camera
            .world_to_screen(
                camera.position + vec3(self.yaw.sin(), 0.0, self.yaw.cos()) * 100.0,
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
        if daylight < 0.9 {
            let alpha = ((1.0 - daylight) * 255.0) as u8;
            for i in 0..90 {
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
            canvas.fill_rect(Rect::from_center(p, 9.0, 9.0), Color::from_hex(0xe8ecf4));
            canvas.fill_rect(
                Rect::new(p.x - 2.0, p.y - 2.0, 2.0, 2.0),
                Color::from_hex(0xc0c6d4),
            );
        }

        // The light: the sun by day, the moon (weak and blue) by night.
        let (direction, color) = if sun.y > -0.1 {
            (
                sun,
                Color::from_hex(0xfff4e0).lerp(horizon_glow, dusk * 0.6),
            )
        } else {
            (sun * -1.0, Color::from_hex(0xa8b8e8))
        };
        canvas.set_lighting(Lighting {
            sun_direction: direction,
            sun_color: color,
            sun_strength: 0.25 + 0.75 * daylight,
            ambient: 0.28 + 0.27 * daylight,
        });

        let still = Transform::default();
        canvas.draw_mesh(&art.ocean, &still, &camera);
        self.world.grid.draw(canvas, &camera);
        // Clouds glow (so their undersides aren't grey), dimmed for the night.
        let drift = (self.time * 0.8).rem_euclid(CLOUD_PERIOD);
        let shade = (90.0 + 165.0 * daylight) as u8;
        let clouds = art.clouds.clone().with_color(Color::rgb(
            shade,
            shade,
            (shade as f32 * 1.05).min(255.0) as u8,
        ));
        canvas.draw_mesh(&clouds, &Transform::at(vec3(drift, 0.0, 0.0)), &camera);

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
            let lift = if self.digging.is_some() || self.place_wait > 0.0 {
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
            if let Some(rect) = screen_box(&camera, canvas, self.position, 1.0, 1.75) {
                canvas.label("player", rect);
            }
        }

        // The edges of the block you're looking at.
        if let Some(hit) = self.target {
            let [x, y, z] = hit.cell.map(|v| v as f32);
            let e = 0.002;
            let corner = |i: usize| {
                vec3(
                    if i & 1 == 0 { x - e } else { x + 1.0 + e },
                    if i & 2 == 0 { y - e } else { y + 1.0 + e },
                    if i & 4 == 0 { z - e } else { z + 1.0 + e },
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
                            canvas.draw_line(p, q, Color::rgba(20, 20, 24, 200));
                        }
                    }
                }
            }
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

        if !self.behind_view {
            self.draw_hand(canvas, &camera);
        }
        self.draw_hud(canvas);
    }
}

impl DuckCraft {
    /// Your own right arm, bottom right of the view, holding the block in
    /// hand. It bobs as you walk and swings when you dig or build.
    fn draw_hand(&self, canvas: &mut Canvas, camera: &Camera3D) {
        let art = &self.art;
        let eye = self.eye();
        let bob = if self.walking && self.on_ground {
            (self.time * 10.0).sin() * 0.03
        } else {
            0.0
        };
        let swing = if self.digging.is_some() || self.place_wait > 0.0 {
            (self.time * 18.0).sin().max(0.0) * 0.7
        } else {
            0.0
        };
        // The shoulder, by the right edge of the view, then the arm points
        // forward and a little down (rotation about x, before the yaw).
        // The shoulder is below the bottom of the view; the arm points
        // forward and up into it, like a hand held out.
        // The offset is in view space (x right, y up, z ahead), tilted with
        // the view and turned with it, so the arm stays put on screen.
        let offset = vec3(0.36, -0.6 + bob, 0.1)
            .rotate_x(self.pitch)
            .rotate_y(self.yaw);
        let shoulder = eye + offset;
        let reach = std::f32::consts::FRAC_PI_2 + 0.6 - swing + self.pitch;
        let arm = Transform::at(shoulder).rotated(vec3(reach, self.yaw, 0.0));
        canvas.draw_mesh(&art.duck.arm, &arm, camera);
        let block = self.holding();
        if self.carried[block as usize] > 0 {
            if let Some(cube) = &art.cubes[block as usize] {
                // The hand is 10 pixels down the arm: the block sits just past it.
                let hand = shoulder
                    + vec3(0.0, -11.0 * SKIN_PIXEL, 0.0)
                        .rotate_x(reach)
                        .rotate_y(self.yaw);
                let place = Transform::at(hand)
                    .rotated(vec3(0.0, self.yaw + 0.5, 0.0))
                    .sized(0.24);
                canvas.draw_mesh(cube, &place, camera);
            }
        }
    }

    fn draw_hud(&self, canvas: &mut Canvas) {
        let art = &self.art;
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

        // The hotbar.
        let slot_size = 22.0;
        let left = (WIDTH - slot_size * 9.0) / 2.0;
        let top = HEIGHT - slot_size - 3.0;
        canvas.fill_rect(
            Rect::new(
                left - 2.0,
                top - 2.0,
                slot_size * 9.0 + 4.0,
                slot_size + 4.0,
            ),
            Color::rgba(0, 0, 0, 120),
        );
        for (i, &block) in HOTBAR.iter().enumerate() {
            let x = left + i as f32 * slot_size;
            let slot = Rect::new(x + 1.0, top + 1.0, slot_size - 2.0, slot_size - 2.0);
            canvas.fill_rect(slot, Color::rgba(90, 90, 100, 150));
            let count = self.carried[block as usize];
            if let Some(icon) = &art.icons[block as usize] {
                let opacity = if count > 0 { 1.0 } else { 0.3 };
                canvas.blit_sprite(
                    icon,
                    vec2(x + 3.0, top + 3.0),
                    Blit {
                        opacity,
                        ..Default::default()
                    },
                );
            }
            if count > 0 {
                let text = count.to_string();
                let w = Canvas::text_width(&text, 1);
                let at = vec2(x + slot_size - 2.0 - w, top + slot_size - 9.0);
                canvas.draw_text(&text, at + vec2(1.0, 1.0), 1, Color::BLACK);
                canvas.draw_text(&text, at, 1, Color::WHITE);
            }
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
        let status = format!("GOLD {}  COAL {}  {clock}", self.gold, self.coal);
        canvas.draw_text(&status, vec2(5.0, 5.0), 1, Color::BLACK);
        canvas.draw_text(&status, vec2(4.0, 4.0), 1, Color::from_hex(0xfff2a0));

        // A message over the hotbar.
        if self.message_seconds > 0.0 {
            let alpha = (self.message_seconds.min(1.0) * 255.0) as u8;
            let y = top - 14.0;
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

        if self.show_help {
            let lines = [
                "DUCK CRAFT",
                "WASD walk   Shift run   Space jump",
                "Mouse or arrows: look   Esc: free the mouse",
                "Click / X: dig    Right click / C: build",
                "1-9 or wheel: pick a block",
                "V: view   H: hide this",
            ];
            let panel = Rect::new(52.0, 26.0, 216.0, 74.0);
            canvas.fill_rect(panel, Color::rgba(0, 0, 0, 150));
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
fn new_game() -> Result<DuckCraft, LoadError> {
    Ok(DuckCraft::new(Rc::new(Art::load()?), World::generate()))
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
    duckforge::run(config(), new_game()?)
}

// Play tests: `cargo test --example duck_craft`. The bot that records
// playthroughs/duck_craft.json: `cargo test --example duck_craft -- --ignored`.
#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;
