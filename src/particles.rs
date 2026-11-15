//! Weather and other small things in the air, in 3D: snow, rain, dust,
//! embers. Thousands of them are one mesh, drawn in one go, hidden behind
//! walls like any mesh (depth tested), lit or glowing.
//!
//! ```
//! # use duckforge::prelude::*;
//! # let mut canvas = Canvas::new(90, 160);
//! # let camera = Camera3D::new(vec3(0.0, 2.0, 0.0));
//! // In `new`: 2,000 flakes in a box 30 x 12 x 30 round the middle.
//! let mut snow = Particles::snow(2000, [vec3(-15.0, 0.0, -15.0), vec3(15.0, 12.0, 15.0)])
//!     .with_wind(vec3(0.8, 0.0, 0.2));
//! // In `update`:
//! snow.update(1.0 / 60.0);
//! // In `draw`, after the world:
//! snow.draw(&mut canvas, &camera);
//! ```
//!
//! Flakes that fall out of the bottom of the box come back in at the top
//! (and out of one side, in at the other), so the weather never runs
//! out. [`move_to`](Particles::move_to) carries the box with the camera:
//! the flakes stay where they are in the world, and the ones left behind
//! come back in ahead. [`set_cover`](Particles::set_cover) keeps them out
//! from under the blocks of a [`BlockWorld`]: no snow in a cave.

use crate::blocks::BlockWorld;
use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::{Vec3, vec3};
use crate::render3d::{Camera3D, Mesh, Transform};
use crate::rng::Rng;

/// One flake (or drop, or mote).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Particle {
    at: Vec3,
    /// How fast it falls, in units a second (negative rises).
    fall: f32,
    /// Where it is in its sway, so they don't all sway together.
    phase: f32,
}

/// The highest solid block in each column of a block world: no particle
/// is drawn below it.
#[derive(Debug, Clone, PartialEq)]
struct Cover {
    width: usize,
    depth: usize,
    /// The top of the highest block in each column (0: none), x fastest.
    tops: Vec<f32>,
}

/// Snow, rain or anything else that drifts through the air. See the
/// [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Particles {
    particles: Vec<Particle>,
    /// The box they live in: its lowest corner and its size.
    low: Vec3,
    size: Vec3,
    wind: Vec3,
    /// How far each one sways from side to side, and how often a second.
    sway: f32,
    sway_rate: f32,
    /// Across, in world units (at least about a pixel on screen).
    width: f32,
    /// Along the way it moves, in world units: drops of rain are long.
    /// 0 for a square that faces the camera.
    streak: f32,
    color: Color,
    /// Shaded by the scene's light instead of drawn at full brightness.
    lit: bool,
    cover: Option<Cover>,
    /// Seconds of `update` so far, for the sway.
    time: f32,
}

impl Particles {
    /// `count` particles spread through the box from `area[0]` to
    /// `area[1]`, standing still, white, 0.05 across. The same `count` and
    /// `area` always give the same particles; change them with
    /// [`seeded`](Particles::seeded).
    pub fn new(count: usize, area: [Vec3; 2]) -> Particles {
        let low = vec3(
            area[0].x.min(area[1].x),
            area[0].y.min(area[1].y),
            area[0].z.min(area[1].z),
        );
        let high = vec3(
            area[0].x.max(area[1].x),
            area[0].y.max(area[1].y),
            area[0].z.max(area[1].z),
        );
        let mut particles = Particles {
            particles: vec![
                Particle {
                    at: low,
                    fall: 0.0,
                    phase: 0.0
                };
                count
            ],
            low,
            size: high - low,
            wind: Vec3::ZERO,
            sway: 0.0,
            sway_rate: 1.0,
            width: 0.05,
            streak: 0.0,
            color: Color::WHITE,
            lit: false,
            cover: None,
            time: 0.0,
        };
        particles.scatter(1, (0.0, 0.0));
        particles
    }

    /// Snow: flakes 0.06 across, falling 0.6 to 1.4 units a second and
    /// swaying gently.
    pub fn snow(count: usize, area: [Vec3; 2]) -> Particles {
        let mut snow = Particles::new(count, area);
        snow.sway = 0.25;
        snow.sway_rate = 0.9;
        snow.width = 0.06;
        snow.scatter(1, (0.6, 1.4));
        snow
    }

    /// Rain: thin drops 0.4 long, falling 9 to 13 units a second, pale blue.
    pub fn rain(count: usize, area: [Vec3; 2]) -> Particles {
        let mut rain = Particles::new(count, area);
        rain.width = 0.015;
        rain.streak = 0.4;
        rain.color = Color::rgb(170, 190, 220);
        rain.scatter(1, (9.0, 13.0));
        rain
    }

    /// Spreads the particles through the box again, at random from `seed`,
    /// each falling between `fall.0` and `fall.1` units a second.
    fn scatter(&mut self, seed: u32, fall: (f32, f32)) {
        let mut rng = Rng::new(seed);
        let (low, size) = (self.low, self.size);
        for p in &mut self.particles {
            p.at = low
                + vec3(
                    rng.next_f32() * size.x,
                    rng.next_f32() * size.y,
                    rng.next_f32() * size.z,
                );
            p.fall = fall.0 + rng.next_f32() * (fall.1 - fall.0);
            p.phase = rng.next_f32() * std::f32::consts::TAU;
        }
    }

    /// The same kind of particles, spread differently: two snowfalls with
    /// different seeds don't line up.
    pub fn seeded(mut self, seed: u32) -> Particles {
        let (slowest, fastest) = self.fall_range();
        self.scatter(
            seed.wrapping_mul(2_654_435_761).wrapping_add(1),
            (slowest, fastest),
        );
        self
    }

    fn fall_range(&self) -> (f32, f32) {
        let falls = self.particles.iter().map(|p| p.fall);
        let slowest = falls.clone().fold(f32::INFINITY, f32::min);
        let fastest = falls.fold(f32::NEG_INFINITY, f32::max);
        if slowest.is_finite() {
            (slowest, fastest)
        } else {
            (0.0, 0.0)
        }
    }

    /// Each falls between `slowest` and `fastest` units a second (negative
    /// rises: smoke, embers, bubbles).
    pub fn with_fall(mut self, slowest: f32, fastest: f32) -> Particles {
        let mut rng = Rng::new(7);
        for p in &mut self.particles {
            p.fall = slowest + rng.next_f32() * (fastest - slowest);
        }
        self
    }

    /// Blown along by `wind`, in units a second.
    pub fn with_wind(mut self, wind: Vec3) -> Particles {
        self.wind = wind;
        self
    }

    /// Changes the wind while it snows (a gust).
    pub fn set_wind(&mut self, wind: Vec3) {
        self.wind = wind;
    }

    /// Sways `distance` units from side to side, `rate` times a second.
    pub fn with_sway(mut self, distance: f32, rate: f32) -> Particles {
        self.sway = distance.max(0.0);
        self.sway_rate = rate;
        self
    }

    /// `width` across, in world units. Far away, a particle is never drawn
    /// smaller than about a pixel, so distant snow doesn't vanish.
    pub fn with_size(mut self, width: f32) -> Particles {
        self.width = width.max(0.0);
        self
    }

    /// Drawn as a streak `length` long along the way it moves (rain); 0 for
    /// a square facing the camera (snow).
    pub fn with_streak(mut self, length: f32) -> Particles {
        self.streak = length.max(0.0);
        self
    }

    /// Their colour.
    pub fn with_color(mut self, color: Color) -> Particles {
        self.color = color;
        self
    }

    /// Shaded by the scene's light and lamps, like the world, instead of
    /// drawn at full brightness. Snow at night wants this.
    pub fn lit(mut self) -> Particles {
        self.lit = true;
        self
    }

    /// How many particles there are.
    pub fn len(&self) -> usize {
        self.particles.len()
    }

    /// True if there are none.
    pub fn is_empty(&self) -> bool {
        self.particles.is_empty()
    }

    /// Where each one is now (sway left out), in the order they were made.
    pub fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.particles.iter().map(|p| p.at)
    }

    /// The box they live in: its lowest and highest corner.
    pub fn area(&self) -> [Vec3; 2] {
        [self.low, self.low + self.size]
    }

    /// Moves the box so its middle is at `middle` (the camera, say). The
    /// particles stay where they are in the world; those now outside the
    /// box come back in on its other side.
    pub fn move_to(&mut self, middle: Vec3) {
        self.low = middle - self.size * 0.5;
        self.wrap_all();
    }

    /// Keeps them out from under `world`'s blocks: a particle below the
    /// top of the highest block in its column isn't drawn (in a cave, under
    /// a roof, inside the ground). The world is where
    /// [`BlockWorld::draw`] puts it, block (x, y, z) from (x, y, z) to
    /// (x + 1, y + 1, z + 1). Call it again after changing blocks.
    pub fn set_cover(&mut self, world: &BlockWorld) {
        let [width, height, depth] = world.size();
        let mut tops = vec![0.0; width * depth];
        for z in 0..depth {
            for x in 0..width {
                if let Some(y) = (0..height as i32)
                    .rev()
                    .find(|&y| world.get(x as i32, y, z as i32) != crate::blocks::AIR)
                {
                    tops[z * width + x] = y as f32 + 1.0;
                }
            }
        }
        self.cover = Some(Cover { width, depth, tops });
    }

    /// Stops keeping them out from under blocks.
    pub fn clear_cover(&mut self) {
        self.cover = None;
    }

    /// Whether a particle at `at` is under cover (not drawn).
    fn covered(&self, at: Vec3) -> bool {
        let Some(cover) = &self.cover else {
            return false;
        };
        let (x, z) = (at.x.floor(), at.z.floor());
        if x < 0.0 || z < 0.0 || x >= cover.width as f32 || z >= cover.depth as f32 {
            return false;
        }
        at.y < cover.tops[z as usize * cover.width + x as usize]
    }

    /// Moves every particle on by `dt` seconds: falling, and blown by the
    /// wind.
    pub fn update(&mut self, dt: f32) {
        self.time += dt;
        let wind = self.wind;
        for p in &mut self.particles {
            p.at += vec3(wind.x, wind.y - p.fall, wind.z) * dt;
        }
        self.wrap_all();
    }

    fn wrap_all(&mut self) {
        let (low, size) = (self.low, self.size);
        let wrap = |v: f32, low: f32, size: f32| {
            if size > 0.0 {
                low + (v - low).rem_euclid(size)
            } else {
                low
            }
        };
        for p in &mut self.particles {
            p.at = vec3(
                wrap(p.at.x, low.x, size.x),
                wrap(p.at.y, low.y, size.y),
                wrap(p.at.z, low.z, size.z),
            );
        }
    }

    /// Where particle `p` is drawn: its place plus its sway.
    fn drawn_at(&self, p: &Particle) -> Vec3 {
        if self.sway == 0.0 {
            return p.at;
        }
        let t = self.time * self.sway_rate * std::f32::consts::TAU + p.phase;
        p.at + vec3(t.sin(), 0.0, (t * 0.77).cos()) * self.sway
    }

    /// The mesh that draws them for `camera` on a canvas `height` pixels
    /// tall: two triangles each, facing the camera.
    pub(crate) fn mesh_for(&self, camera: &Camera3D, height: f32) -> Mesh {
        let mut mesh = Mesh::new();
        mesh.vertices.reserve(self.particles.len() * 4);
        mesh.triangles.reserve(self.particles.len() * 2);
        let (right, up, forward) = (camera.right(), camera.up(), camera.forward());
        // How many world units a pixel is, one unit ahead of the camera.
        let per_pixel = (camera.fov.clamp(0.02, 3.1) / 2.0).tan() * 2.0 / height.max(1.0);
        let motion = |fall: f32| vec3(self.wind.x, self.wind.y - fall, self.wind.z);
        for p in &self.particles {
            let at = self.drawn_at(p);
            if self.covered(at) {
                continue;
            }
            let ahead = (at - camera.position).dot(forward);
            if ahead <= 0.05 {
                continue; // behind the camera, or touching it
            }
            // Never thinner than about a pixel.
            let half = self.width.max(ahead * per_pixel * 1.2) * 0.5;
            let (side, along) = if self.streak > 0.0 {
                // A streak along the way it moves, as wide as it is across.
                let way = motion(p.fall).normalized();
                let to_camera = (camera.position - at).normalized();
                let side = to_camera.cross(way).normalized();
                if side.length() < 0.5 {
                    (right * half, up * half) // falling straight at the camera
                } else {
                    (side * half, way * (self.streak * 0.5).max(half))
                }
            } else {
                (right * half, up * half)
            };
            let first = mesh.vertices.len();
            for corner in [
                at - side - along,
                at + side - along,
                at + side + along,
                at - side + along,
            ] {
                mesh.add_vertex(corner);
            }
            mesh.add_triangle(first, first + 2, first + 1, self.color);
            mesh.add_triangle(first, first + 3, first + 2, self.color);
        }
        if !self.lit {
            mesh = mesh.glowing();
        }
        mesh
    }

    /// Draws them all, as one mesh: hidden behind anything nearer, in fog
    /// if there's fog. Gives how many were drawn (not under cover, not
    /// behind the camera).
    pub fn draw(&self, canvas: &mut Canvas, camera: &Camera3D) -> usize {
        let mesh = self.mesh_for(camera, canvas.height() as f32);
        let drawn = mesh.triangles.len() / 2;
        if drawn > 0 {
            canvas.draw_mesh(&mesh, &Transform::default(), camera);
        }
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{BlockKind, BlockPalette};

    fn area() -> [Vec3; 2] {
        [vec3(-5.0, 0.0, -5.0), vec3(5.0, 6.0, 5.0)]
    }

    fn inside(p: Vec3, [low, high]: [Vec3; 2]) -> bool {
        (low.x..=high.x).contains(&p.x)
            && (low.y..=high.y).contains(&p.y)
            && (low.z..=high.z).contains(&p.z)
    }

    #[test]
    fn snow_falls_blows_and_never_leaves_its_box() {
        let mut snow = Particles::snow(500, area()).with_wind(vec3(2.0, 0.0, 0.0));
        assert_eq!(snow.len(), 500);
        assert_eq!(
            snow,
            Particles::snow(500, area()).with_wind(vec3(2.0, 0.0, 0.0)),
            "the same every time"
        );
        assert_ne!(
            snow,
            Particles::snow(500, area())
                .with_wind(vec3(2.0, 0.0, 0.0))
                .seeded(3)
        );
        let before: Vec<Vec3> = snow.positions().collect();
        snow.update(0.1);
        let after: Vec<Vec3> = snow.positions().collect();
        let moved = before.iter().zip(&after).filter(|(a, b)| {
            // Down and along the wind (unless it came back in at the top or side).
            b.y < a.y && b.x > a.x
        });
        assert!(moved.count() > 450, "nearly all fell and blew");
        for _ in 0..600 {
            snow.update(1.0 / 60.0);
        }
        assert!(
            snow.positions().all(|p| inside(p, area())),
            "all still in the box"
        );
        // And spread through it, not piled up at an edge.
        let low = snow.positions().filter(|p| p.y < 3.0).count();
        assert!((150..350).contains(&low), "{low} of 500 in the lower half");
    }

    #[test]
    fn the_box_follows_the_camera_and_the_flakes_stay_put() {
        let mut snow = Particles::snow(200, area());
        let still: Vec<Vec3> = snow.positions().collect();
        snow.move_to(vec3(1.0, 3.0, 0.0)); // a small step: most stay put
        let stayed = snow
            .positions()
            .zip(&still)
            .filter(|(a, b)| a == *b)
            .count();
        assert!(stayed > 150, "{stayed} of 200 stayed where they were");
        assert_eq!(snow.area(), [vec3(-4.0, 0.0, -5.0), vec3(6.0, 6.0, 5.0)]);
        assert!(snow.positions().all(|p| inside(p, snow.area())));
    }

    #[test]
    fn a_thousand_flakes_are_one_mesh_hidden_behind_a_wall() {
        let snow = Particles::snow(1000, area());
        let camera = Camera3D::new(vec3(0.0, 3.0, -8.0));
        let mesh = snow.mesh_for(&camera, 160.0);
        assert_eq!(
            mesh.triangles.len(),
            2000,
            "two triangles a flake, one mesh"
        );
        assert!(mesh.glowing);
        // A wall right in front of the camera's left half: no snow there.
        let mut canvas = Canvas::new(160, 90);
        canvas.clear(Color::BLACK);
        let wall = Mesh::cube(Color::rgb(200, 0, 0)).glowing();
        canvas.draw_mesh(
            &wall,
            &Transform::at(vec3(-1.0, 3.0, -6.0)).sized(2.0),
            &camera,
        );
        let drawn = snow.draw(&mut canvas, &camera);
        assert_eq!(drawn, 1000);
        canvas.flush_3d();
        let white = |x: i32, y: i32| canvas.get_pixel(x, y) == Some(Color::WHITE);
        let left = (10..60)
            .flat_map(|x| (20..70).map(move |y| (x, y)))
            .filter(|&(x, y)| white(x, y))
            .count();
        let right = (100..150)
            .flat_map(|x| (20..70).map(move |y| (x, y)))
            .filter(|&(x, y)| white(x, y))
            .count();
        assert_eq!(left, 0, "the wall hides the flakes behind it");
        assert!(right > 20, "{right} flakes on the open side");
    }

    #[test]
    fn no_snow_under_blocks() {
        let atlas = crate::tileset::Tileset::grid(
            &crate::sprite::Sprite::from_pixels(1, 1, vec![Color::GRAY]),
            1,
            1,
            &["stone"],
        );
        let palette = BlockPalette::new(&atlas, &[BlockKind::new("stone", ["stone"; 3])]);
        let mut world = BlockWorld::new(10, 6, 10);
        // A roof at y = 4 over x from 0 to 4.
        for z in 0..10 {
            for x in 0..5 {
                world.set(x, 4, z, palette.id("stone"));
            }
        }
        let mut snow = Particles::snow(2000, [Vec3::ZERO, vec3(10.0, 6.0, 10.0)]);
        snow.set_cover(&world);
        let camera = Camera3D::looking_at(vec3(5.0, 3.0, -20.0), vec3(5.0, 3.0, 5.0));
        let mesh = snow.mesh_for(&camera, 400.0);
        for corner in mesh.vertices.chunks(4) {
            let middle = (corner[0] + corner[2]) * 0.5;
            assert!(
                !((0.0..5.0).contains(&middle.x)
                    && (0.0..10.0).contains(&middle.z)
                    && middle.y < 5.0),
                "a flake under the roof at {middle:?}"
            );
        }
        let all = snow.positions().count();
        let shown = mesh.triangles.len() / 2;
        assert!(
            shown < all * 3 / 4 && shown > all / 3,
            "{shown} of {all} shown"
        );
        snow.clear_cover();
        assert_eq!(snow.mesh_for(&camera, 400.0).triangles.len(), 4000);
    }

    #[test]
    fn rain_falls_fast_as_streaks_and_lit_particles_take_the_light() {
        let rain = Particles::rain(10, area());
        let camera = Camera3D::new(vec3(0.0, 3.0, -8.0));
        let mesh = rain.mesh_for(&camera, 900.0);
        let quad = &mesh.vertices[0..4];
        let tall = (quad[3] - quad[0]).length();
        let wide = (quad[1] - quad[0]).length();
        assert!(tall > wide * 5.0, "a streak: {tall} tall, {wide} wide");
        assert!(rain.fall_range().0 >= 9.0);
        assert!(
            !Particles::snow(10, area())
                .lit()
                .mesh_for(&camera, 90.0)
                .glowing
        );
    }
}
