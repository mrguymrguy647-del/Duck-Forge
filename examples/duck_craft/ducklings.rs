//! Ducklings: little blocky ducks that live on the island.
//!
//! They waddle about, hop up steps, float on the sea and quack now and
//! then. **Right-click** one (or look at it and press **C**) and it
//! imprints on you, the way real ducklings do on the first duck they see:
//! from then on it follows you, in a line with the others you've met, each
//! behind the one before. Right-click it again and it stays where it is.
//! Left-click one and it hops and quacks; nothing hurts them.

use super::*;
use std::f32::consts::PI;

/// How many ducklings the island starts with.
pub const DUCKLINGS: usize = 6;
/// A duckling's body for bumping into blocks: 0.5 wide, 0.6 tall.
pub const DUCKLING: Body = Body {
    half_width: 0.25,
    height: 0.6,
};
const WADDLE_SPEED: f32 = 1.3; // wandering, blocks per second
const CATCH_UP_SPEED: f32 = 7.0; // following, at most
const HOP_SPEED: f32 = 7.8; // just over one block high
const GAP_BEHIND_DUCK: f32 = 1.3; // how far behind you the first one walks
const GAP_IN_LINE: f32 = 0.85; // and each after the one before

#[derive(Debug, Clone)]
pub struct Duckling {
    /// Its feet, in the middle of its body.
    pub position: Vec3,
    pub velocity: Vec3,
    /// Which way it faces (0 is +z, like the duck).
    pub yaw: f32,
    pub on_ground: bool,
    pub in_water: bool,
    /// When it imprinted on you: the line goes in this order. `None`: it
    /// wanders by itself.
    pub follows: Option<u32>,
    /// Wandering: the way it's going, or `None` standing still, and for
    /// how much longer.
    wander: Option<f32>,
    wander_seconds: f32,
    /// How far its legs have walked (for the waddle).
    stride: f32,
    quack_seconds: f32,
    /// Seconds left of a happy hop and quack (when you click it).
    pub happy: f32,
}

impl Duckling {
    pub fn new(position: Vec3, yaw: f32) -> Duckling {
        Duckling {
            position,
            velocity: Vec3::ZERO,
            yaw,
            on_ground: false,
            in_water: false,
            follows: None,
            wander: None,
            wander_seconds: 0.0,
            stride: 0.0,
            quack_seconds: 3.0 + yaw.abs() * 2.0,
            happy: 0.0,
        }
    }

    /// Sits still until it's clicked or follows you (for tests and
    /// pictures).
    #[cfg(test)]
    pub fn sitting(mut self) -> Duckling {
        self.wander = None;
        self.wander_seconds = f32::INFINITY;
        self
    }

    /// The box it fills, for the duck's aim.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let h = DUCKLING.half_width + 0.05;
        (
            self.position - vec3(h, 0.0, h),
            self.position + vec3(h, DUCKLING.height + 0.05, h),
        )
    }
}

/// Six ducklings on grass round the spawn, the same places every time.
pub fn hatch(world: &World) -> Vec<Duckling> {
    let mut ducklings = Vec::new();
    let [sx, _, sz] = [world.spawn.x as i32, 0, world.spawn.z as i32];
    let mut tries = 0;
    while ducklings.len() < DUCKLINGS && tries < 400 {
        tries += 1;
        let angle = random01(tries, 0, 0, SEED + 20) * TAU;
        let distance = 5.0 + random01(tries, 1, 0, SEED + 20) * 9.0;
        let (x, z) = (
            sx + (angle.sin() * distance) as i32,
            sz + (angle.cos() * distance) as i32,
        );
        let mut top = TALL as i32 - 1;
        while top > 0 && world.get(x, top, z) == Block::Air {
            top -= 1;
        }
        let on_grass = world.get(x, top, z) == Block::Grass;
        let feet = vec3(x as f32 + 0.5, top as f32 + 1.0, z as f32 + 0.5);
        let crowded = ducklings
            .iter()
            .any(|d: &Duckling| d.position.distance(feet) < 2.0);
        if on_grass && !crowded && !DUCKLING.hits(world, feet) {
            ducklings.push(Duckling::new(feet, angle));
        }
    }
    ducklings
}

/// Turns `from` towards `to` (radians) by at most `step`, the short way.
fn turn_towards(from: f32, to: f32, step: f32) -> f32 {
    let difference = (to - from + PI).rem_euclid(TAU) - PI;
    from + difference.clamp(-step, step)
}

fn quack(loud: bool) -> Sound {
    Sound {
        name: "quack",
        wave: Wave::Square,
        freq: 620.0,
        slide_to: 380.0,
        ms: 95.0,
        volume: if loud { 0.16 } else { 0.07 },
        decay: 60.0,
        ..Default::default()
    }
}

impl DuckCraft {
    /// Moves every duckling: wandering, or following its leader.
    pub fn update_ducklings(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        // The line: you first, then each follower in the order it imprinted.
        let mut line: Vec<usize> = (0..self.ducklings.len())
            .filter(|&i| self.ducklings[i].follows.is_some())
            .collect();
        line.sort_by_key(|&i| self.ducklings[i].follows);
        let mut leaders = vec![None; self.ducklings.len()];
        for (place, &i) in line.iter().enumerate() {
            leaders[i] = Some(if place == 0 {
                (self.position, GAP_BEHIND_DUCK)
            } else {
                (self.ducklings[line[place - 1]].position, GAP_IN_LINE)
            });
        }

        let player = self.position;
        for (i, duckling) in self.ducklings.iter_mut().enumerate() {
            // Where it wants to go, and how fast.
            let wish = match leaders[i] {
                Some((leader, gap)) => {
                    let to = vec3(
                        leader.x - duckling.position.x,
                        0.0,
                        leader.z - duckling.position.z,
                    );
                    let distance = to.length();
                    if distance > 24.0 {
                        // Lost: it finds you (just behind you, if there's room).
                        let behind = player - vec3(self.yaw.sin(), 0.0, self.yaw.cos()) * 1.2;
                        if !DUCKLING.hits(&self.world, behind) {
                            duckling.position = behind;
                            duckling.velocity = Vec3::ZERO;
                        }
                        Vec3::ZERO
                    } else if distance > gap {
                        to.normalized() * ((distance - gap) * 3.5).min(CATCH_UP_SPEED)
                    } else {
                        Vec3::ZERO
                    }
                }
                None => {
                    duckling.wander_seconds -= dt;
                    if duckling.wander_seconds <= 0.0 {
                        let walk = ctx.rng.range(0.0, 1.0) < 0.6;
                        duckling.wander = walk.then(|| ctx.rng.range(0.0, TAU));
                        duckling.wander_seconds = if walk {
                            ctx.rng.range(1.5, 4.0)
                        } else {
                            ctx.rng.range(1.0, 3.0)
                        };
                    }
                    match duckling.wander {
                        Some(yaw) => vec3(yaw.sin(), 0.0, yaw.cos()) * WADDLE_SPEED,
                        None => Vec3::ZERO,
                    }
                }
            };
            let swimming = if duckling.in_water { 0.6 } else { 1.0 };
            let grip = if duckling.on_ground || duckling.in_water {
                14.0
            } else {
                4.0
            };
            duckling.velocity.x = approach(duckling.velocity.x, wish.x * swimming, grip * dt);
            duckling.velocity.z = approach(duckling.velocity.z, wish.z * swimming, grip * dt);
            if wish.length() > 0.1 {
                duckling.yaw = turn_towards(duckling.yaw, wish.x.atan2(wish.z), 8.0 * dt);
            }

            // Floating: water pushes it up until it bobs at the top.
            if duckling.in_water {
                let deep = self.world.get(
                    duckling.position.x.floor() as i32,
                    (duckling.position.y + 0.35).floor() as i32,
                    duckling.position.z.floor() as i32,
                ) == Block::Water;
                let target = if deep { 1.8 } else { 0.0 };
                duckling.velocity.y = approach(duckling.velocity.y, target, 12.0 * dt);
            } else {
                duckling.velocity.y = (duckling.velocity.y - GRAVITY * dt).max(-30.0);
            }

            let mut position = duckling.position;
            let world = &self.world;
            let blocked_x = DUCKLING.move_axis(world, &mut position, 0, duckling.velocity.x * dt);
            let blocked_z = DUCKLING.move_axis(world, &mut position, 2, duckling.velocity.z * dt);
            let falling = duckling.velocity.y <= 0.0;
            let landed = DUCKLING.move_axis(world, &mut position, 1, duckling.velocity.y * dt);
            if landed {
                duckling.velocity.y = 0.0;
            }
            duckling.on_ground = landed && falling;
            // A step in the way: hop up it (like the duck).
            let pushing = wish.length() > 0.1;
            if (blocked_x || blocked_z) && pushing && (duckling.on_ground || duckling.in_water) {
                let up = position + vec3(0.0, 1.05, 0.0);
                if !DUCKLING.hits(world, up + wish.normalized() * 0.3) && !DUCKLING.hits(world, up)
                {
                    duckling.velocity.y = HOP_SPEED;
                } else if leaders[i].is_none() {
                    duckling.wander_seconds = 0.0; // a wall: go another way
                }
            }
            if blocked_x && duckling.on_ground {
                duckling.velocity.x = 0.0;
            }
            if blocked_z && duckling.on_ground {
                duckling.velocity.z = 0.0;
            }
            duckling.position = position;
            duckling.in_water = world.get(
                position.x.floor() as i32,
                (position.y + 0.1).floor() as i32,
                position.z.floor() as i32,
            ) == Block::Water;

            let going = vec2(duckling.velocity.x, duckling.velocity.z).length();
            duckling.stride += going * dt;

            // Clicked: a happy hop.
            if duckling.happy > 0.0 {
                duckling.happy -= dt;
            }

            // Quack now and then (more often when it's with you).
            duckling.quack_seconds -= dt;
            if duckling.quack_seconds <= 0.0 {
                let near = duckling.position.distance(player) < 12.0;
                duckling.quack_seconds = if duckling.follows.is_some() {
                    ctx.rng.range(4.0, 10.0)
                } else {
                    ctx.rng.range(6.0, 16.0)
                };
                if near {
                    ctx.play(quack(false));
                }
            }
        }

        // Don't stand in each other: a gentle push apart.
        for a in 0..self.ducklings.len() {
            for b in a + 1..self.ducklings.len() {
                let (pa, pb) = (self.ducklings[a].position, self.ducklings[b].position);
                let apart = vec3(pa.x - pb.x, 0.0, pa.z - pb.z);
                let distance = apart.length();
                if distance < 0.5 && (pa.y - pb.y).abs() < 0.6 {
                    let push = if distance > 1e-4 {
                        apart * (1.0 / distance)
                    } else {
                        vec3(1.0, 0.0, 0.0)
                    };
                    let shove = push * ((0.5 - distance) * 0.5);
                    for (i, sign) in [(a, 1.0), (b, -1.0)] {
                        let moved = self.ducklings[i].position + shove * sign;
                        if !DUCKLING.hits(&self.world, moved) {
                            self.ducklings[i].position = moved;
                        }
                    }
                }
            }
        }

        for duckling in &self.ducklings {
            ctx.check(
                "ducklings are never inside a block",
                !DUCKLING.hits(&self.world, duckling.position),
            );
        }
        ctx.report(
            "ducklings_following",
            self.ducklings
                .iter()
                .filter(|d| d.follows.is_some())
                .count(),
        );
    }

    /// Which duckling the duck is looking at, if one is nearer than the
    /// block it's looking at.
    pub fn aimed_duckling(&self) -> Option<usize> {
        let origin = self.eye();
        let direction = self.look_direction();
        let block_distance = self.target.map_or(REACH, |hit| {
            let middle = vec3(
                hit.cell[0] as f32 + 0.5,
                hit.cell[1] as f32 + 0.5,
                hit.cell[2] as f32 + 0.5,
            );
            middle.distance(origin) - 0.5
        });
        let mut best: Option<(usize, f32)> = None;
        for (i, duckling) in self.ducklings.iter().enumerate() {
            let (low, high) = duckling.bounds();
            if let Some(t) = ray_box(origin, direction, low, high) {
                if t <= REACH.min(block_distance) && best.is_none_or(|(_, b)| t < b) {
                    best = Some((i, t));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// Clicks on the duckling in view: right (or C) imprints it or lets it
    /// go; left (or X) makes it hop. Returns whether there was one.
    pub fn touch_duckling(&mut self, ctx: &mut Context) -> bool {
        let Some(i) = self.aimed else {
            return false;
        };
        let input = &ctx.input;
        let right = input.was_mouse_pressed(MouseButton::Right) || input.was_pressed(Key::C);
        let left = (ctx.pointer_locked() && input.was_mouse_pressed(MouseButton::Left))
            || input.was_pressed(Key::X);
        if right {
            if self.ducklings[i].follows.is_some() {
                self.ducklings[i].follows = None;
                self.ducklings[i].wander_seconds = 5.0;
                self.ducklings[i].wander = None;
                self.say("The duckling waits here");
            } else {
                self.imprints += 1;
                self.ducklings[i].follows = Some(self.imprints);
                let count = self
                    .ducklings
                    .iter()
                    .filter(|d| d.follows.is_some())
                    .count();
                self.say(if count == 1 {
                    String::from("A duckling follows you!")
                } else {
                    format!("{count} ducklings follow you!")
                });
            }
            self.ducklings[i].happy = 0.5;
            if self.ducklings[i].on_ground {
                self.ducklings[i].velocity.y = 5.0;
            }
            ctx.play(quack(true));
            self.place_wait = PLACE_REPEAT;
        } else if left {
            self.ducklings[i].happy = 0.5;
            if self.ducklings[i].on_ground {
                self.ducklings[i].velocity.y = 5.0;
            }
            ctx.play(quack(true));
        }
        true
    }

    pub fn draw_ducklings(&self, canvas: &mut Canvas, camera: &Camera3D) {
        for (i, duckling) in self.ducklings.iter().enumerate() {
            if duckling.position.distance(camera.position) > 60.0 {
                continue;
            }
            self.draw_shadow(canvas, camera, duckling.position, 0.45);
            let waddle = (duckling.stride * 9.0).sin();
            let pose = DucklingPose {
                yaw: duckling.yaw,
                swing: waddle * 0.7,
                // A little side to side as it walks, and a nod.
                roll: waddle
                    * 0.08
                    * (vec2(duckling.velocity.x, duckling.velocity.z).length() / WADDLE_SPEED)
                        .min(1.5),
                nod: if duckling.happy > 0.0 {
                    (duckling.happy * 25.0).sin() * 0.4
                } else {
                    (duckling.stride * 4.5).sin() * 0.12
                },
                swimming: duckling.in_water,
            };
            self.art
                .duckling
                .draw(canvas, camera, duckling.position, pose);
            if self.aimed == Some(i) {
                let (low, high) = duckling.bounds();
                draw_box_edges(canvas, camera, low, high, Color::rgba(20, 20, 24, 200));
            }
            if let Some(rect) = screen_box(camera, canvas, duckling.position, 0.5, 0.6) {
                canvas.label("duckling", rect);
            }
        }
    }
}

/// Where a ray from `origin` (along `direction`, length 1) first enters the
/// box from `low` to `high`, as a distance, or `None` if it misses.
pub fn ray_box(origin: Vec3, direction: Vec3, low: Vec3, high: Vec3) -> Option<f32> {
    let o = [origin.x, origin.y, origin.z];
    let d = [direction.x, direction.y, direction.z];
    let (l, h) = ([low.x, low.y, low.z], [high.x, high.y, high.z]);
    let (mut near, mut far) = (0.0f32, f32::INFINITY);
    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < l[i] || o[i] > h[i] {
                return None;
            }
            continue;
        }
        let (a, b) = ((l[i] - o[i]) / d[i], (h[i] - o[i]) / d[i]);
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return None;
        }
    }
    Some(near)
}

// ---- The duckling model ----

/// How a duckling stands.
#[derive(Debug, Clone, Copy)]
pub struct DucklingPose {
    pub yaw: f32,
    /// Legs, one way and the other.
    pub swing: f32,
    /// Rocking side to side.
    pub roll: f32,
    /// Head down (+) or up.
    pub nod: f32,
    /// Swimming: no legs, sitting low.
    pub swimming: bool,
}

/// A duckling's parts, each round its own pivot, textured with its skin
/// (`art/make_duckling.py`).
pub struct DucklingModel {
    /// Body with its tail; pivot under the middle of the body.
    body: Mesh,
    /// Head with its beak; pivot at the neck.
    head: Mesh,
    /// Pivot at the hip.
    leg: Mesh,
}

impl DucklingModel {
    // In skin pixels (1/16 block): legs 2 tall, body 5, head 4.
    const HIP: f32 = 2.0 * SKIN_PIXEL;
    const NECK: (f32, f32) = (6.0 * SKIN_PIXEL, 2.5 * SKIN_PIXEL); // up, forward

    pub fn load(skin: Sprite) -> DucklingModel {
        let mut body =
            skin_box([6.0, 5.0, 8.0], [-3.0, 0.0, -4.0], 0.0, 0.0).with_texture(skin.clone());
        let tail =
            skin_box([2.0, 2.0, 1.0], [-1.0, 4.0, -5.0], 56.0, 0.0).with_texture(skin.clone());
        body.merge(&tail, &Transform::default());
        let mut head =
            skin_box([4.0, 4.0, 4.0], [-2.0, 0.0, -2.0], 28.0, 0.0).with_texture(skin.clone());
        let beak =
            skin_box([3.0, 1.0, 2.0], [-1.5, 1.0, 2.0], 44.0, 0.0).with_texture(skin.clone());
        head.merge(&beak, &Transform::default());
        let leg = skin_box([1.0, 2.0, 1.0], [-0.5, -2.0, -0.5], 28.0, 8.0).with_texture(skin);
        DucklingModel { body, head, leg }
    }

    pub fn draw(&self, canvas: &mut Canvas, camera: &Camera3D, feet: Vec3, pose: DucklingPose) {
        let sit = if pose.swimming { -Self::HIP } else { 0.0 };
        let at = |local: Vec3| feet + local.rotate_y(pose.yaw);
        let part = |pivot: Vec3, rotation: Vec3| {
            Transform::at(at(pivot)).rotated(vec3(rotation.x, pose.yaw + rotation.y, rotation.z))
        };
        canvas.draw_mesh(
            &self.body,
            &part(vec3(0.0, Self::HIP + sit, 0.0), vec3(0.0, 0.0, pose.roll)),
            camera,
        );
        let (neck_up, neck_forward) = Self::NECK;
        canvas.draw_mesh(
            &self.head,
            &part(
                vec3(
                    0.0,
                    Self::HIP + neck_up - 1.0 * SKIN_PIXEL + sit,
                    neck_forward,
                ),
                vec3(pose.nod, 0.0, pose.roll),
            ),
            camera,
        );
        if !pose.swimming {
            let side = 1.5 * SKIN_PIXEL;
            canvas.draw_mesh(
                &self.leg,
                &part(vec3(-side, Self::HIP, 0.0), vec3(pose.swing, 0.0, 0.0)),
                camera,
            );
            canvas.draw_mesh(
                &self.leg,
                &part(vec3(side, Self::HIP, 0.0), vec3(-pose.swing, 0.0, 0.0)),
                camera,
            );
        }
    }
}
