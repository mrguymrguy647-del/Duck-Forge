//! Mini Craft: started from the runner3d starter with tools/new_game.
//!
//! A small world of blocks: walk the duck round grass hills, stone
//! pillars, trees and a pool of glowing lava, and collect the six gems.
//! Lava sends you back to the start.
//!
//!     cargo run --example mini_craft
//!
//! Arrow keys (or WASD) walk: Left/Right along x, Up/Down along z. The duck
//! steps up one block at a time; pillars and tree trunks stop it. Escape
//! quits; after collecting every gem, Space starts again.
//!
//! - The world is text (`WORLD`, one letter per column), built into blocks
//!   from one Duck Quill atlas (`art/blocks.json`) and merged into four
//!   chunks, one mesh each. The lava is its own glowing mesh.
//! - The duck is a billboard: pillars and trees in front of it hide it.
//!
//! Without a screen (or to test):
//!
//!     cargo run --no-default-features --example mini_craft -- --headless --inputs playthroughs/mini_craft.json --sheet-events --out out
//!     cargo test --no-default-features --example mini_craft

use duckforge::LoadError;
use duckforge::prelude::*;

// ---- Settings: change these first ----

const WIDTH: f32 = 320.0; // the screen, in pixels
const HEIGHT: f32 = 240.0;
const WALK_SPEED: f32 = 3.5; // blocks per second
const RADIUS: f32 = 0.3; // how wide the duck is, from its middle
const CLIMB_SPEED: f32 = 6.0; // how fast it steps up or drops down
const CHUNK: usize = 10; // chunks are 10 x 10 columns
/// Where the camera is, from the duck: behind (-z), to the left (-x), up.
const CAMERA_OFFSET: Vec3 = vec3(-4.5, 6.0, -6.5);

const ASSETS: &str = "examples/mini_craft/assets";

/// The world from above, the far side (z = 19) at the top, x going right.
/// `.` grass 1 block high, `2` and `3` higher grass, `S` a stone pillar,
/// `T` a tree, `L` lava, `g` a gem on the grass (`H` on 3-high grass), `p`
/// the start.
const WORLD: &str = "
....................
.............222222.
......T......223332.
...........T.223H32.
...S.........223332.
..........g.2222222.
.........S...222222.
.....g..............
..T.................
.......LLLL.....S...
.......LLLLg........
....S..LLLL.........
....................
..............S.....
.......g............
......T..........g..
............T.......
.................T..
.p..................
....................";

/// One column of the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Column {
    /// Grass on top of dirt, this many blocks high.
    Grass(u8),
    /// A stone pillar, 4 high: too high to climb.
    Stone,
    /// A tree: a trunk with leaves round its top. Too high to climb.
    Tree,
    /// A block of lava: walking onto it sends you back to the start.
    Lava,
}

impl Column {
    /// The height you stand at on this column (lava's is the ground's).
    fn height(self) -> f32 {
        match self {
            Column::Grass(h) => h as f32,
            Column::Stone | Column::Tree => 4.0,
            Column::Lava => 1.0,
        }
    }
}

/// The world's columns, its gems and where the duck starts.
#[derive(Debug, Clone)]
struct World {
    size: usize,
    /// `columns[z * size + x]`.
    columns: Vec<Column>,
    gems: Vec<(usize, usize)>,
    start: (usize, usize),
}

impl World {
    fn parse(text: &str) -> World {
        let rows: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let size = rows.len();
        let mut world = World {
            size,
            columns: vec![Column::Grass(1); size * size],
            gems: Vec::new(),
            start: (0, 0),
        };
        for (row, line) in rows.iter().enumerate() {
            assert_eq!(
                line.len(),
                size,
                "every row of WORLD is {size} long; row {} isn't",
                row + 1
            );
            let z = size - 1 - row; // the first row is the far side
            for (x, letter) in line.chars().enumerate() {
                let column = match letter {
                    '.' | 'g' | 'p' => Column::Grass(1),
                    '2' => Column::Grass(2),
                    '3' | 'H' => Column::Grass(3),
                    'S' => Column::Stone,
                    'T' => Column::Tree,
                    'L' => Column::Lava,
                    other => panic!(
                        "WORLD row {}, column {}: {other:?} isn't a column",
                        row + 1,
                        x + 1
                    ),
                };
                world.columns[z * size + x] = column;
                match letter {
                    'g' | 'H' => world.gems.push((x, z)),
                    'p' => world.start = (x, z),
                    _ => {}
                }
            }
        }
        world
    }

    /// The column at (x, z), or `None` outside the world.
    fn column(&self, x: i32, z: i32) -> Option<Column> {
        let inside = (0..self.size as i32).contains(&x) && (0..self.size as i32).contains(&z);
        inside.then(|| self.columns[z as usize * self.size + x as usize])
    }

    /// The column under a point (blocks are centred on whole numbers).
    fn column_at(&self, x: f32, z: f32) -> Option<Column> {
        self.column(x.round() as i32, z.round() as i32)
    }
}

/// Pictures, meshes and sounds, made once.
#[derive(Clone)]
struct Art {
    duck: SpriteSheet,
    /// One merged mesh per chunk of 10 x 10 columns (lava left out).
    chunks: Vec<Mesh>,
    /// Every lava block, merged into one glowing mesh.
    lava: Mesh,
    gem: Mesh,
    gem_sound: Wav,
    lava_sound: Wav,
    wind: Wav,
}

impl Art {
    fn load(world: &World) -> Result<Art, LoadError> {
        let atlas = Tileset::load(format!("{ASSETS}/blocks.json"))?;
        let grass = Mesh::block(&atlas, ["grass_top", "grass_side", "dirt"]);
        let dirt = Mesh::block(&atlas, ["dirt"; 3]);
        let stone = Mesh::block(&atlas, ["stone"; 3]);
        let log = Mesh::block(&atlas, ["log"; 3]);
        let leaves = Mesh::block(&atlas, ["leaves"; 3]);
        let lava_block = Mesh::block(&atlas, ["lava"; 3]).glowing();

        // All blocks of a chunk in one mesh: one draw per chunk, and
        // chunks off screen are skipped whole. (Glowing lava can't be
        // merged with the rest: a mesh glows or it doesn't.)
        let chunks_across = world.size.div_ceil(CHUNK);
        let mut chunks = vec![Mesh::new(); chunks_across * chunks_across];
        let mut lava = Mesh::new();
        let at =
            |x: usize, y: i32, z: usize| Transform::at(vec3(x as f32, y as f32 + 0.5, z as f32));
        for z in 0..world.size {
            for x in 0..world.size {
                let chunk = &mut chunks[(z / CHUNK) * chunks_across + x / CHUNK];
                match world.columns[z * world.size + x] {
                    Column::Grass(h) => {
                        for y in 0..h as i32 - 1 {
                            chunk.merge(&dirt, &at(x, y, z));
                        }
                        chunk.merge(&grass, &at(x, h as i32 - 1, z));
                    }
                    Column::Stone => {
                        for y in 0..4 {
                            chunk.merge(&stone, &at(x, y, z));
                        }
                    }
                    Column::Tree => {
                        chunk.merge(&grass, &at(x, 0, z));
                        for y in 1..4 {
                            chunk.merge(&log, &at(x, y, z));
                        }
                        // Leaves round the top of the trunk, and one on top.
                        for (dx, dz) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                            let place = vec3(x as f32 + dx as f32, 3.5, z as f32 + dz as f32);
                            chunk.merge(&leaves, &Transform::at(place));
                        }
                        chunk.merge(&leaves, &at(x, 4, z));
                    }
                    Column::Lava => {
                        // A little lower than the grass round it.
                        let place = Transform::at(vec3(x as f32, 0.4, z as f32))
                            .scaled(vec3(1.0, 0.8, 1.0));
                        lava.merge(&lava_block, &place);
                    }
                }
            }
        }
        Ok(Art {
            duck: SpriteSheet::load(format!("{ASSETS}/hero.json"))?,
            chunks,
            lava,
            gem: Mesh::block(&atlas, ["gem"; 3]).glowing(),
            gem_sound: Wav::load(format!("{ASSETS}/coin.wav"))?,
            lava_sound: Wav::load(format!("{ASSETS}/hit.wav"))?,
            wind: Wav::load(format!("{ASSETS}/hum.wav"))?,
        })
    }
}

/// What the game is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    Won,
}

#[derive(Clone)]
struct MiniCraft {
    state: State,
    world: World,
    art: Art,
    /// Where the duck is: x and z across the world, y its feet.
    x: f32,
    y: f32,
    z: f32,
    moving: bool,
    facing_left: bool,
    gems: Vec<(usize, usize)>,
    collected: u32,
    /// Times the duck walked into the lava.
    burns: u32,
    time: f32,
    wind: Option<LoopId>,
}

impl MiniCraft {
    fn new(art: Art, world: World) -> MiniCraft {
        let (sx, sz) = world.start;
        MiniCraft {
            state: State::Playing,
            gems: world.gems.clone(),
            x: sx as f32,
            y: 1.0,
            z: sz as f32,
            world,
            art,
            moving: false,
            facing_left: false,
            collected: 0,
            burns: 0,
            time: 0.0,
            wind: None,
        }
    }

    /// The ground under the duck's middle.
    fn ground(&self) -> f32 {
        self.world
            .column_at(self.x, self.z)
            .map_or(1.0, Column::height)
    }

    /// Whether the duck, centred at (x, z), would be in a column too high
    /// to step onto (or outside the world).
    fn blocked(&self, x: f32, z: f32) -> bool {
        let standing = self.ground();
        [
            (-RADIUS, -RADIUS),
            (RADIUS, -RADIUS),
            (-RADIUS, RADIUS),
            (RADIUS, RADIUS),
        ]
        .iter()
        .any(|&(dx, dz)| match self.world.column_at(x + dx, z + dz) {
            Some(column) => column.height() > standing + 1.0,
            None => true,
        })
    }

    fn play(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let speed = ctx.tune("walk_speed", WALK_SPEED);
        self.time += dt;
        if self.wind.is_none() {
            self.wind = Some(ctx.play_loop(&self.art.wind, 0.25));
        }

        // Walk, one direction at a time, so a wall only stops that one.
        let across = ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
        let along = ctx.input.axis(Key::Down, Key::Up) + ctx.input.axis(Key::S, Key::W);
        let (across, along) = (across.clamp(-1.0, 1.0), along.clamp(-1.0, 1.0));
        self.moving = across != 0.0 || along != 0.0;
        if across != 0.0 {
            self.facing_left = across < 0.0;
        }
        let next_x = self.x + across * speed * dt;
        if !self.blocked(next_x, self.z) {
            self.x = next_x;
        }
        let next_z = self.z + along * speed * dt;
        if !self.blocked(self.x, next_z) {
            self.z = next_z;
        }
        // Step up or drop down to the ground, quickly but not at once.
        self.y = approach(self.y, self.ground(), CLIMB_SPEED * dt);

        // Lava: back to the start.
        if self.world.column_at(self.x, self.z) == Some(Column::Lava) {
            self.burns += 1;
            ctx.play_wav(&self.art.lava_sound);
            let (sx, sz) = self.world.start;
            (self.x, self.y, self.z) = (sx as f32, 1.0, sz as f32);
        }

        // Gems: keep the ones the duck isn't touching.
        let (x, z) = (self.x, self.z);
        let before = self.gems.len();
        self.gems
            .retain(|&(gx, gz)| (gx as f32 - x).abs() > 0.6 || (gz as f32 - z).abs() > 0.6);
        for _ in self.gems.len()..before {
            self.collected += 1;
            ctx.play_wav(&self.art.gem_sound);
        }
        if self.gems.is_empty() {
            self.state = State::Won;
        }
    }

    fn camera(&self) -> Camera3D {
        let duck = vec3(self.x, self.y, self.z);
        Camera3D::looking_at(duck + CAMERA_OFFSET, duck + vec3(0.0, 0.5, 0.0))
    }
}

impl Game for MiniCraft {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Playing => self.play(ctx),
            State::Won => {
                ctx.idle_ok();
                if ctx.input.was_pressed(Key::Space) {
                    *self = MiniCraft::new(self.art.clone(), self.world.clone());
                }
            }
        }
        let size = self.world.size as f32;
        ctx.check(
            "inside the world",
            (-0.5..size - 0.5).contains(&self.x) && (-0.5..size - 0.5).contains(&self.z),
        );
        ctx.check("never inside a pillar or tree", self.ground() <= 3.0);

        ctx.report("state", format!("{:?}", self.state));
        ctx.report("gems", self.collected);
        ctx.report("burns", self.burns);
        ctx.report("x", (self.x * 10.0).round() / 10.0);
        ctx.report("z", (self.z * 10.0).round() / 10.0);
    }

    fn draw(&self, canvas: &mut Canvas) {
        let art = &self.art;
        let camera = self.camera();
        canvas.clear(Color::from_hex(0x8ec8f0));
        canvas.fill_rect(Rect::new(0.0, 0.0, WIDTH, 50.0), Color::from_hex(0x6cb0e4));
        canvas.set_lighting(Lighting {
            sun_direction: vec3(-0.4, 1.0, -0.7),
            sun_color: Color::from_hex(0xfff4e0),
            sun_strength: 1.0,
            ambient: 0.5,
        });
        let still = Transform::default();
        for chunk in &art.chunks {
            canvas.draw_mesh(chunk, &still, &camera);
        }
        canvas.draw_mesh(&art.lava, &still, &camera);
        for &(gx, gz) in &self.gems {
            let ground = self
                .world
                .column(gx as i32, gz as i32)
                .map_or(1.0, Column::height);
            let bob = (self.time * 3.0).sin() * 0.1;
            let spin = Transform::at(vec3(gx as f32, ground + 0.5 + bob, gz as f32))
                .rotated(vec3(0.0, self.time * 2.0, 0.0))
                .sized(0.4);
            canvas.draw_mesh(&art.gem, &spin, &camera);
        }

        // The duck, standing in the world: things in front of it hide it.
        let animation = if self.moving { "run" } else { "idle" };
        let frame = art.duck.animation(animation).frame_at(self.time);
        let look = Billboard {
            pivot: Some(art.duck.pivot()),
            flip_x: self.facing_left,
            ..Default::default()
        };
        canvas.draw_billboard_with(frame, vec3(self.x, self.y, self.z), 1.0, &camera, look);

        // Labels, in screen pixels, for tools that can't see the picture.
        if let Some(rect) = screen_box(&camera, canvas, vec3(self.x, self.y, self.z), 0.6, 1.0) {
            canvas.label("player", rect);
        }

        // The HUD.
        let total = self.collected as usize + self.gems.len();
        canvas.draw_text(
            &format!("GEMS {}/{total}", self.collected),
            vec2(4.0, 4.0),
            1,
            Color::WHITE,
        );
        if self.burns > 0 {
            canvas.draw_text(
                &format!("BURNS {}", self.burns),
                vec2(4.0, 13.0),
                1,
                Color::from_hex(0xffa060),
            );
        }
        if self.state == State::Won {
            canvas.fill_rect(
                Rect::new(0.0, 90.0, WIDTH, 60.0),
                Color::BLACK.with_alpha(170),
            );
            canvas.draw_text_centered("ALL GEMS!", 100.0, 3, Color::YELLOW);
            canvas.draw_text_centered("Space to play again", 130.0, 1, Color::WHITE);
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
    // Pixels per unit at that distance, from the height on screen.
    let half = width / 2.0 * (bottom.y - top.y).abs() / height;
    Some(Rect::new(
        bottom.x - half,
        top.y,
        half * 2.0,
        bottom.y - top.y,
    ))
}

/// The game, with its world and art.
fn new_game() -> Result<MiniCraft, LoadError> {
    let world = World::parse(WORLD);
    let art = Art::load(&world)?;
    Ok(MiniCraft::new(art, world))
}

fn config() -> Config {
    Config {
        title: String::from("Mini Craft - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), new_game()?)
}

// Play tests: `cargo test --example mini_craft`. The bot that records
// playthroughs/mini_craft.json: `cargo test --example mini_craft -- --ignored`.
#[cfg(test)]
mod tests;

#[cfg(test)]
mod bot;
