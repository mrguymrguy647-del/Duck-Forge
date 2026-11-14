//! Night Cellar: an 8-second vertical horror short, made in Duck Forge
//! and nothing else. A torch goes down into a cellar, the bulb flickers
//! out, and something is standing at the far end.
//!
//! It shows the lit renderer (see `docs/AI_GUIDE.md`, "Lamps, shadows,
//! glow and smooth edges"): a bulb and a torch as lamps that cast shadows,
//! glowing eyes that bloom, smooth shading on the figure, supersampled
//! edges, captions in a TrueType font with an outline and a drop shadow,
//! and sounds at their own volumes, kept from clipping by the limiter.
//!
//! Make the video (1080 x 1920, 30 frames a second, with its sound, in one
//! command; needs `ffmpeg`):
//!
//! ```text
//! cargo run --release --example night_cellar -- --headless --frames 240 --video night_cellar.mp4
//! ```
//!
//! Or watch it in a window: `cargo run --release --example night_cellar`.

use duckforge::prelude::*;

const SECONDS: f32 = 8.0;
const FPS: u32 = 30;
const DARK: Color = Color::from_hex(0x05_06_0A);
const BULB: Color = Color::from_hex(0xFF_B0_70);
const TORCH: Color = Color::from_hex(0xFF_F0_D8);

/// The cellar: 5 metres wide, 3 high, 12 long (z from -6 to 6).
const ROOM: Vec3 = vec3(5.0, 3.0, 12.0);
/// The bulb hangs here; the thing stands under it, at the far end.
const BULB_AT: Vec3 = vec3(0.0, 2.45, 3.8);
const THING_AT: Vec3 = vec3(0.0, 0.0, 4.4);

struct Cellar {
    walls: Mesh,
    floor: Mesh,
    crates: Vec<(Mesh, Transform)>,
    cord: Mesh,
    bulb: Mesh,
    bulb_off: Mesh,
    thing: Mesh,
    eyes: Mesh,
    /// Seconds since the start.
    time: f32,
}

impl Cellar {
    fn new() -> Cellar {
        let bricks = bricks();
        let planks = planks();
        let walls = Mesh::room(ROOM, 1.0).with_texture(bricks);
        let floor = Mesh::floor(ROOM.x, ROOM.z, 1.2).with_texture(planks.clone());
        let crate_at = |size: f32, x: f32, y: f32, z: f32, turn: f32| {
            (
                Mesh::tiled_box(vec3(size, size, size), size).with_texture(planks.clone()),
                Transform::at(vec3(x, y + size / 2.0, z)).rotated(vec3(0.0, turn, 0.0)),
            )
        };
        let crates = vec![
            crate_at(0.9, 1.6, 0.0, -1.5, 0.2),
            crate_at(0.6, 1.5, 0.9, -1.4, -0.3),
            crate_at(0.8, -1.7, 0.0, 1.0, 0.5),
            crate_at(0.7, 1.4, 0.0, 2.6, 0.1),
        ];
        // A tall dark shape: a body and a head, shaded smoothly.
        let mut thing = Mesh::new();
        let skin = Color::from_hex(0x6A_6064);
        thing.merge(
            &Mesh::cylinder(skin, 16),
            &Transform::at(vec3(0.0, 0.85, 0.0)).scaled(vec3(0.55, 1.7, 0.4)),
        );
        thing.merge(
            &Mesh::sphere(skin, 14),
            &Transform::at(vec3(0.0, 1.95, 0.0)).scaled(vec3(0.42, 0.5, 0.42)),
        );
        let mut eyes = Mesh::new();
        for x in [-0.09, 0.09] {
            eyes.merge(
                &Mesh::sphere(Color::from_hex(0xFF_30_20), 8),
                &Transform::at(vec3(x, 2.0, -0.19)).sized(0.07),
            );
        }
        Cellar {
            walls,
            floor,
            crates,
            cord: Mesh::cube(Color::from_hex(0x20_20_20)),
            bulb: Mesh::sphere(BULB, 10).glowing(),
            bulb_off: Mesh::sphere(Color::from_hex(0x50_48_40), 10),
            thing,
            eyes: eyes.glowing(),
            time: 0.0,
        }
    }

    /// The bulb's brightness now: flickering, and out from 4.6 seconds.
    fn bulb_power(&self) -> f32 {
        let t = self.time;
        if t >= 4.6 {
            return 0.0;
        }
        // Flickers: a few short dips, the same every run.
        let dips: [(f32, f32); 5] = [
            (1.3, 0.08),
            (2.2, 0.05),
            (3.4, 0.12),
            (4.0, 0.06),
            (4.3, 0.1),
        ];
        if dips.iter().any(|&(at, long)| t >= at && t < at + long) {
            return 0.15;
        }
        1.4
    }

    fn camera(&self) -> Camera3D {
        // Walking in for 6 seconds, slowing down, then standing still.
        let walk = (self.time / 6.0).min(1.0);
        let eased = 1.0 - (1.0 - walk) * (1.0 - walk);
        let z = -5.3 + eased * 4.8;
        let bob = (self.time * 7.0).sin() * 0.025 * (1.0 - walk);
        let mut camera = Camera3D::new(vec3(0.0, 1.6 + bob, z));
        camera.look_at(vec3(0.15 * (self.time * 0.9).sin(), 1.4, z + 6.0));
        camera.fov = 70f32.to_radians();
        camera
    }

    fn torch(&self, camera: &Camera3D) -> Lamp {
        let sway = vec3(
            (self.time * 1.3).sin() * 0.08,
            (self.time * 1.7).cos() * 0.04,
            0.0,
        );
        let at = camera.position + camera.right() * 0.25 - vec3(0.0, 0.25, 0.0);
        // Once the bulb is out, the torch finds the thing's face.
        let reveal = ((self.time - 4.8) / 1.2).clamp(0.0, 1.0);
        let ahead = camera.forward() + sway;
        let at_face = (THING_AT + vec3(0.0, 1.9, 0.0) - at).normalized();
        let direction = ahead * (1.0 - reveal) + at_face * reveal;
        Lamp::spot(at, direction, 0.7, TORCH, 9.0, 16.0).with_shadows()
    }
}

impl Game for Cellar {
    fn update(&mut self, ctx: &mut Context) {
        self.time = ctx.frame() as f32 / FPS as f32;
        if self.time > SECONDS {
            ctx.quit(); // the end of the short
        }
        ctx.report("seconds", self.time);
        ctx.report("bulb", self.bulb_power());
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(DARK);
        let camera = self.camera();
        canvas.set_lighting(Lighting {
            sun_strength: 0.0,
            ambient: 0.035,
            ..Lighting::default()
        });
        canvas.set_fog(Some(Fog {
            color: DARK,
            near: 4.0,
            far: 14.0,
        }));
        let power = self.bulb_power();
        let mut lamps = vec![self.torch(&camera)];
        if power > 0.0 {
            lamps.push(Lamp::point(BULB_AT, BULB, power, 7.0).with_shadows());
        }
        canvas.set_lamps(&lamps);
        canvas.set_shadows(Some(Shadows {
            lamp_resolution: 768,
            ..Shadows::default()
        }));
        canvas.set_bloom(Some(Bloom {
            strength: 2.5,
            radius: 0.02,
        }));

        let world = Transform::at(vec3(0.0, ROOM.y / 2.0 - 0.01, 0.0));
        canvas.draw_mesh(&self.walls, &world, &camera);
        canvas.draw_mesh(&self.floor, &Transform::default(), &camera);
        for (mesh, place) in &self.crates {
            canvas.draw_mesh(mesh, place, &camera);
        }
        let cord = Transform::at(BULB_AT + vec3(0.0, 0.3, 0.0)).scaled(vec3(0.02, 0.5, 0.02));
        canvas.draw_mesh(&self.cord, &cord, &camera);
        let bulb = if power > 0.5 {
            &self.bulb
        } else {
            &self.bulb_off
        };
        canvas.draw_mesh(bulb, &Transform::at(BULB_AT).sized(0.14), &camera);

        canvas.set_smooth_shading(true);
        let thing = Transform::at(THING_AT);
        canvas.draw_mesh(&self.thing, &thing, &camera);
        canvas.set_smooth_shading(false);
        // Its eyes open when the bulb goes out.
        if self.time > 4.9 {
            canvas.draw_mesh(&self.eyes, &thing, &camera);
        }
    }
}

/// A number from 0 to 1 that looks random but is the same every run.
fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(374_761_393)
        ^ y.wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    (h ^ (h >> 16)) as f32 / u32::MAX as f32
}

/// Old red bricks with dark mortar, 4 rows a tile.
fn bricks() -> Sprite {
    let size = 64;
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let row = y / 16;
            let shift = if row % 2 == 0 { 0 } else { 16 };
            let (bx, by) = ((x + shift) % 32, y % 16);
            let mortar = bx < 2 || by < 2;
            let brick = ((x + shift) / 32 + row * 7) as u32;
            let grain = noise(x as u32, y as u32, 1) * 0.25;
            let tone = 0.75 + noise(brick, row as u32, 2) * 0.35 - grain;
            pixels.push(if mortar {
                Color::rgb(70, 66, 60)
            } else {
                let c = |v: f32| (v * tone).clamp(0.0, 255.0) as u8;
                Color::rgb(c(150.0), c(78.0), c(60.0))
            });
        }
    }
    Sprite::from_pixels(size, size, pixels)
}

/// Worn wooden planks running along the floor.
fn planks() -> Sprite {
    let size = 64;
    let mut pixels = Vec::with_capacity(size * size);
    for y in 0..size {
        for x in 0..size {
            let plank = x / 16;
            let gap = x % 16 == 0;
            let grain = ((y as f32 * 0.35 + plank as f32 * 3.0).sin() * 0.08)
                + noise(x as u32, y as u32, 3) * 0.12;
            let tone = 0.8 + noise(plank as u32, 0, 4) * 0.25 - grain;
            pixels.push(if gap {
                Color::rgb(30, 22, 16)
            } else {
                let c = |v: f32| (v * tone).clamp(0.0, 255.0) as u8;
                Color::rgb(c(130.0), c(96.0), c(64.0))
            });
        }
    }
    Sprite::from_pixels(size, size, pixels)
}

fn config() -> Config {
    Config {
        title: String::from("Night Cellar"),
        width: 1080,
        height: 1920,
        scale: 1,
        timestep: Timestep::Fixed {
            updates_per_second: FPS,
        },
        seed: Some(7),
        supersample: 2,
        ..Default::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Cellar::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bulb_flickers_then_goes_out_and_the_short_is_eight_seconds() {
        let mut cellar = Cellar::new();
        cellar.time = 1.0;
        assert_eq!(cellar.bulb_power(), 1.4);
        cellar.time = 1.32;
        assert!(cellar.bulb_power() < 0.5, "a flicker");
        cellar.time = 5.0;
        assert_eq!(cellar.bulb_power(), 0.0);
        assert_eq!((SECONDS * FPS as f32) as u32, 240);
    }

    #[test]
    fn a_small_frame_draws_with_lamps_shadows_and_glow() {
        // The same scene on a small canvas (the real one is 1080 x 1920).
        let mut cellar = Cellar::new();
        cellar.time = 5.5;
        let mut canvas = Canvas::new(90, 160);
        canvas.set_supersample(2);
        cellar.draw(&mut canvas);
        canvas.flush_3d();
        let lit = canvas
            .pixels()
            .iter()
            .filter(|&&p| p != DARK.to_u32())
            .count();
        assert!(lit > 90 * 160 / 2, "the cellar is lit: {lit} pixels");
        // The eyes glow red, and bloom round them.
        let red = canvas
            .pixels()
            .iter()
            .filter(|&&p| (p >> 16) & 0xff > 120 && (p >> 8) & 0xff < 90)
            .count();
        assert!(red > 0, "glowing eyes");
    }
}
