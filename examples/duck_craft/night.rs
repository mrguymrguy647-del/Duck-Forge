//! The night's shapes.
//!
//! **Hollows** are what's left of ducks: grey, eyeless, a little taller
//! than you. They come up out of the dark: on the surface at night, and
//! in caves at any hour, never near a lamp (they won't go near the light).
//! They walk towards you when they see you, and hit hard. In the morning
//! the ones under the open sky come apart into ash. Hit one with your
//! wings (2) or a stone blade (6); it has 12. Sometimes it leaves coal.
//!
//! The **Watcher** stands far off, some nights, at the edge of what you can
//! see, and looks at you. Look back at it, or walk towards it, and it is
//! gone.
//!
//! In creative, they're still there, but they leave you alone.

use super::*;
use ducklings::ray_box;
use menus::{Mode, Stage, sky_at};

pub const HOLLOW: Body = Body {
    half_width: 0.3,
    height: 1.95,
};
pub const HOLLOW_SIZE: f32 = 1.12; // the duck's model, 1.12 times as big
pub const WATCHER_SIZE: f32 = 1.75;
pub const HOLLOW_HEALTH: i32 = 12;
pub const MAX_HOLLOWS: usize = 5;
const CHASE_SPEED: f32 = 2.9; // blocks a second: slower than you walk
const WANDER_SPEED: f32 = 0.8;
const SEES: f32 = 22.0; // how far off it notices you
const HIT_REACH: f32 = 1.3;
pub const HOLLOW_DAMAGE: u32 = 2;
const ATTACK_SECONDS: f32 = 1.1;
const SPAWN_NEAR: f32 = 14.0;
/// Seconds between tries at a Hollow coming up (one at a time, not a crowd).
const SPAWN_SECONDS: f32 = 3.0;
const SPAWN_FAR: f32 = 30.0;
/// Hollows keep at least this far from a lamp.
pub const LAMP_FEAR: f32 = 6.0;
const FADE_SECONDS: f32 = 1.6;
pub const WINGS_DAMAGE: i32 = 2;
pub const BLADE_DAMAGE: i32 = 6;
const SWING_SECONDS: f32 = 0.4;
const COAL_CHANCE: f32 = 0.5;

const KILL_LINES: [&str; 4] = [
    "It was someone, once.",
    "It came apart without a sound.",
    "Ash. There's always more where that came from.",
    "It didn't look surprised.",
];

#[derive(Debug, Clone)]
pub struct Hollow {
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub health: i32,
    pub on_ground: bool,
    in_water: bool,
    pub chasing: bool,
    attack_wait: f32,
    /// Seconds left of a lunge (its arms thrown forward).
    pub lunge: f32,
    /// Seconds left of the red flash from being hit.
    pub hurt: f32,
    /// Coming apart: seconds since it started.
    pub fading: Option<f32>,
    stride: f32,
    groan_wait: f32,
    wander: Option<f32>,
    wander_wait: f32,
}

impl Hollow {
    pub fn new(position: Vec3, yaw: f32) -> Hollow {
        Hollow {
            position,
            velocity: Vec3::ZERO,
            yaw,
            health: HOLLOW_HEALTH,
            on_ground: false,
            in_water: false,
            chasing: false,
            attack_wait: 1.0,
            lunge: 0.0,
            hurt: 0.0,
            fading: None,
            stride: 0.0,
            groan_wait: 2.0 + yaw.abs(),
            wander: None,
            wander_wait: 0.0,
        }
    }

    pub fn bounds(&self) -> (Vec3, Vec3) {
        let h = HOLLOW.half_width + 0.05;
        (
            self.position - vec3(h, 0.0, h),
            self.position + vec3(h, HOLLOW.height, h),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Watcher {
    pub position: Vec3,
    pub yaw: f32,
    /// Seconds it's been looked at (it goes at a fifth of a second).
    seen: f32,
    age: f32,
}

impl Watcher {
    pub fn new(position: Vec3, yaw: f32) -> Watcher {
        Watcher {
            position,
            yaw,
            seen: 0.0,
            age: 0.0,
        }
    }
}

/// A blow landing on the duck: how hard, and from where.
struct Blow {
    from: Vec3,
}

impl DuckCraft {
    /// Whether it's dark enough at (x, y, z) for something to come up out
    /// of it: night under the open sky, or no sky at all (a cave), and no
    /// lamp near.
    pub fn dark_at(&self, x: i32, y: i32, z: i32) -> bool {
        let daylight = sky_at(self.time_of_day()).daylight;
        let sky = self.world.sky[z as usize * SIZE + x as usize];
        let open = y >= sky;
        let dark = if open {
            daylight < 0.15
        } else {
            light(&self.world.grid, &self.world.sky, x, y, z) < 0.7
        };
        let cell = vec3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
        let lamp_near = self.world.lamps.iter().any(|&[lx, ly, lz]| {
            let lamp = vec3(lx as f32 + 0.5, ly as f32 + 0.5, lz as f32 + 0.5);
            lamp.distance(cell) < LAMP_FEAR + 2.0
        });
        dark && !lamp_near
    }

    /// Whether the duck is under a roof of rock (not just a tree).
    pub fn underground(&self) -> bool {
        let eye = self.eye();
        let [x, y, z] = [eye.x, eye.y, eye.z].map(|v| v.floor() as i32);
        World::inside(x, y, z) && light(&self.world.grid, &self.world.sky, x, y, z) < 0.7
    }

    /// Tries a few places for a Hollow to come up, round the duck.
    fn spawn_hollow(&mut self, ctx: &mut Context) {
        for _ in 0..8 {
            let angle = ctx.rng.range(0.0, TAU);
            let distance = ctx.rng.range(SPAWN_NEAR, SPAWN_FAR);
            let x = (self.position.x + angle.sin() * distance).floor() as i32;
            let z = (self.position.z + angle.cos() * distance).floor() as i32;
            if !World::inside(x, 0, z) {
                continue;
            }
            // Floors in this column near the duck's height (the surface,
            // or a cave below it).
            let near = self.position.y as i32;
            let floors: Vec<i32> = (1.max(near - 12)..(TALL as i32 - 2).min(near + 12))
                .filter(|&y| {
                    let feet = vec3(x as f32 + 0.5, y as f32, z as f32 + 0.5);
                    self.world.get(x, y - 1, z).solid()
                        && self.world.get(x, y, z) != Block::Water
                        && !HOLLOW.hits(&self.world, feet)
                })
                .collect();
            let Some(&y) = floors.get(ctx.rng.range(0.0, floors.len() as f32) as usize) else {
                continue;
            };
            if !self.dark_at(x, y, z) {
                continue;
            }
            // In a cave, only when the duck is underground too: caves are
            // dangerous to be in, not to stand above.
            let open = y >= self.world.sky[z as usize * SIZE + x as usize];
            if !open && !self.underground() {
                continue;
            }
            let feet = vec3(x as f32 + 0.5, y as f32, z as f32 + 0.5);
            let facing = (self.position.x - feet.x).atan2(self.position.z - feet.z);
            self.hollows.push(Hollow::new(feet, facing));
            return;
        }
    }

    /// Hollows: coming up, walking, hitting, fading.
    pub fn update_hollows(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let art = self.art.clone();
        let daylight = sky_at(self.time_of_day()).daylight;

        self.spawn_wait -= dt;
        if self.spawn_wait <= 0.0 {
            self.spawn_wait = SPAWN_SECONDS;
            if self.hollows.iter().filter(|h| h.fading.is_none()).count() < MAX_HOLLOWS {
                self.spawn_hollow(ctx);
            }
        }

        let hunting = self.mode == Mode::Survival && !self.vitals.fainted;
        let duck = self.position;
        let mut blows = Vec::new();
        for hollow in &mut self.hollows {
            if let Some(t) = &mut hollow.fading {
                *t += dt;
                hollow.position.y -= dt * 0.4; // sinking as it goes
                continue;
            }
            hollow.hurt = (hollow.hurt - dt).max(0.0);
            hollow.lunge = (hollow.lunge - dt).max(0.0);
            hollow.attack_wait -= dt;

            // Morning: under the open sky, it comes apart.
            let [cx, cy, cz] = [
                hollow.position.x,
                hollow.position.y + 1.0,
                hollow.position.z,
            ]
            .map(|v| v.floor() as i32);
            let open =
                World::inside(cx, cy, cz) && cy >= self.world.sky[cz as usize * SIZE + cx as usize];
            if daylight > 0.4 && open {
                hollow.fading = Some(0.0);
                continue;
            }

            // Where it wants to go.
            let to_duck = vec3(duck.x - hollow.position.x, 0.0, duck.z - hollow.position.z);
            let distance = to_duck.length();
            hollow.chasing = hunting && distance < SEES && (duck.y - hollow.position.y).abs() < 8.0;
            let mut wish = if hollow.chasing && distance > 0.8 {
                to_duck.normalized() * CHASE_SPEED
            } else if hollow.chasing {
                Vec3::ZERO
            } else {
                hollow.wander_wait -= dt;
                if hollow.wander_wait <= 0.0 {
                    hollow.wander =
                        (ctx.rng.range(0.0, 1.0) < 0.5).then(|| ctx.rng.range(0.0, TAU));
                    hollow.wander_wait = ctx.rng.range(2.0, 5.0);
                }
                hollow.wander.map_or(Vec3::ZERO, |yaw| {
                    vec3(yaw.sin(), 0.0, yaw.cos()) * WANDER_SPEED
                })
            };
            // Lamps: it won't come nearer than LAMP_FEAR.
            for &[lx, ly, lz] in &self.world.lamps {
                let lamp = vec3(lx as f32 + 0.5, ly as f32, lz as f32 + 0.5);
                let away = vec3(hollow.position.x - lamp.x, 0.0, hollow.position.z - lamp.z);
                let d = away.length();
                if d < LAMP_FEAR && (ly as f32 - hollow.position.y).abs() < 6.0 {
                    let push = if d > 0.01 {
                        away * (1.0 / d)
                    } else {
                        vec3(1.0, 0.0, 0.0)
                    };
                    wish = push * CHASE_SPEED * 1.2;
                }
            }

            let grip = if hollow.on_ground || hollow.in_water {
                12.0
            } else {
                6.0
            };
            let swim = if hollow.in_water { 0.5 } else { 1.0 };
            hollow.velocity.x = approach(hollow.velocity.x, wish.x * swim, grip * dt);
            hollow.velocity.z = approach(hollow.velocity.z, wish.z * swim, grip * dt);
            if wish.length() > 0.1 {
                let target = wish.x.atan2(wish.z);
                let turn = (target - hollow.yaw + std::f32::consts::PI).rem_euclid(TAU)
                    - std::f32::consts::PI;
                hollow.yaw += turn.clamp(-5.0 * dt, 5.0 * dt);
            }
            if hollow.in_water {
                hollow.velocity.y = approach(hollow.velocity.y, 1.0, 10.0 * dt);
            } else {
                hollow.velocity.y = (hollow.velocity.y - GRAVITY * dt).max(-30.0);
            }
            let world = &self.world;
            let mut position = hollow.position;
            let blocked_x = HOLLOW.move_axis(world, &mut position, 0, hollow.velocity.x * dt);
            let blocked_z = HOLLOW.move_axis(world, &mut position, 2, hollow.velocity.z * dt);
            let falling = hollow.velocity.y <= 0.0;
            let landed = HOLLOW.move_axis(world, &mut position, 1, hollow.velocity.y * dt);
            if landed {
                hollow.velocity.y = 0.0;
            }
            hollow.on_ground = landed && falling;
            // Up a step, like the duck.
            if (blocked_x || blocked_z)
                && wish.length() > 0.1
                && (hollow.on_ground || hollow.in_water)
            {
                let up = position + vec3(0.0, 1.05, 0.0);
                if !HOLLOW.hits(world, up) && !HOLLOW.hits(world, up + wish.normalized() * 0.3) {
                    hollow.velocity.y = JUMP_SPEED * 0.92;
                } else if !hollow.chasing {
                    hollow.wander_wait = 0.0;
                }
            }
            if blocked_x && hollow.on_ground {
                hollow.velocity.x = 0.0;
            }
            if blocked_z && hollow.on_ground {
                hollow.velocity.z = 0.0;
            }
            hollow.position = position;
            hollow.in_water = world.get(
                position.x.floor() as i32,
                (position.y + 0.6).floor() as i32,
                position.z.floor() as i32,
            ) == Block::Water;
            hollow.stride += vec2(hollow.velocity.x, hollow.velocity.z).length() * dt;

            // Close enough: it hits.
            let reach = vec2(duck.x - position.x, duck.z - position.z).length();
            if hollow.chasing
                && reach < HIT_REACH
                && (duck.y - position.y).abs() < 1.6
                && hollow.attack_wait <= 0.0
            {
                hollow.attack_wait = ATTACK_SECONDS;
                hollow.lunge = 0.35;
                blows.push(Blow { from: position });
            }

            // Now and then, a groan: near is loud, far is muffled.
            hollow.groan_wait -= dt;
            if hollow.groan_wait <= 0.0 {
                hollow.groan_wait = ctx.rng.range(5.0, 12.0);
                let heard = position.distance(duck);
                if heard < 12.0 {
                    ctx.play_wav(&art.sounds.groan);
                } else if heard < 28.0 {
                    ctx.play_wav(&art.sounds.groan_far);
                }
            }
        }
        // Gone once faded (ash crumbs as it goes).
        let mut ash = Vec::new();
        for hollow in &self.hollows {
            if let Some(t) = hollow.fading {
                if t < dt * 1.5 {
                    ash.push(hollow.position);
                }
            }
        }
        for at in ash {
            if at.distance(duck) < 30.0 && daylight > 0.4 {
                ctx.play_wav(&art.sounds.hollow_die);
            }
            self.ash_cloud(ctx, at);
        }
        // Gone once faded; and ones left far behind just go (more come).
        self.hollows.retain(|h| {
            h.fading.is_none_or(|t| t < FADE_SECONDS) && h.position.distance(duck) < 56.0
        });

        for blow in blows {
            self.take_blow(ctx, blow);
        }

        for hollow in &self.hollows {
            if hollow.fading.is_none() {
                ctx.check(
                    "hollows are never inside a block",
                    !HOLLOW.hits(&self.world, hollow.position),
                );
            }
        }
        ctx.report(
            "hollows",
            self.hollows.iter().filter(|h| h.fading.is_none()).count(),
        );
        ctx.report("kills", self.kills);
    }

    fn take_blow(&mut self, ctx: &mut Context, blow: Blow) {
        if self.mode != Mode::Survival || self.vitals.fainted {
            return;
        }
        let away = vec3(
            self.position.x - blow.from.x,
            0.0,
            self.position.z - blow.from.z,
        );
        let push = if away.length() > 0.01 {
            away.normalized()
        } else {
            vec3(0.0, 0.0, 1.0)
        };
        self.velocity = self.velocity + push * 6.0 + vec3(0.0, 4.5, 0.0);
        self.hurt(ctx, HOLLOW_DAMAGE, "was caught by a Hollow");
    }

    /// Grey crumbs where a Hollow came apart.
    fn ash_cloud(&mut self, ctx: &mut Context, at: Vec3) {
        for _ in 0..14 {
            let velocity = vec3(
                ctx.rng.range(-1.2, 1.2),
                ctx.rng.range(0.5, 3.0),
                ctx.rng.range(-1.2, 1.2),
            );
            let offset = vec3(
                ctx.rng.range(-0.3, 0.3),
                ctx.rng.range(0.2, 1.8),
                ctx.rng.range(-0.3, 0.3),
            );
            self.crumbs.push(Crumb {
                position: at + offset,
                velocity,
                age: 0.0,
                block: Block::Bedrock,
            });
        }
    }

    /// Which Hollow the duck is looking at, if one's nearer than any
    /// block, within reach.
    pub fn aimed_hollow(&self) -> Option<(usize, f32)> {
        let origin = self.eye();
        let direction = self.look_direction();
        let block = self.target.map_or(REACH, |hit| {
            let middle = vec3(
                hit.cell[0] as f32 + 0.5,
                hit.cell[1] as f32 + 0.5,
                hit.cell[2] as f32 + 0.5,
            );
            middle.distance(origin) - 0.5
        });
        let mut best: Option<(usize, f32)> = None;
        for (i, hollow) in self.hollows.iter().enumerate() {
            if hollow.fading.is_some() {
                continue;
            }
            let (low, high) = hollow.bounds();
            if let Some(t) = ray_box(origin, direction, low, high) {
                if t <= REACH.min(block) && best.is_none_or(|(_, b)| t < b) {
                    best = Some((i, t));
                }
            }
        }
        best
    }

    /// Left click (or X) at a Hollow: a blow, with whatever's in hand.
    /// Returns whether one was in view.
    pub fn strike(&mut self, ctx: &mut Context) -> bool {
        let Some(i) = self.aimed_hollow_now else {
            return false;
        };
        let input = &ctx.input;
        let left = ctx.pointer_locked() && !self.left_click_locked;
        let held = (left && input.is_mouse_down(MouseButton::Left)) || input.is_down(Key::X);
        if !held || self.swing_wait > 0.0 {
            return true;
        }
        self.swing_wait = SWING_SECONDS;
        self.swing = 0.25;
        let blade = self.holding().is_some_and(|s| s.item == Item::Blade);
        let damage = match (self.mode, blade) {
            (Mode::Creative, _) => HOLLOW_HEALTH,
            (_, true) => BLADE_DAMAGE,
            (_, false) => WINGS_DAMAGE,
        };
        let from = self.position;
        let hollow = &mut self.hollows[i];
        hollow.health -= damage;
        hollow.hurt = 0.3;
        let away = vec3(hollow.position.x - from.x, 0.0, hollow.position.z - from.z);
        let push = if away.length() > 0.01 {
            away.normalized()
        } else {
            vec3(0.0, 0.0, 1.0)
        };
        // A stagger back, not a throw.
        hollow.velocity = vec3(push.x * 4.5, 3.0, push.z * 4.5);
        hollow.attack_wait = hollow.attack_wait.max(0.6);
        self.vitals.tiredness += 0.03;
        if hollow.health <= 0 {
            hollow.fading = Some(0.0);
            let at = hollow.position;
            self.kills += 1;
            ctx.play_wav(&self.art.sounds.hollow_die);
            self.ash_cloud(ctx, at);
            if ctx.rng.range(0.0, 1.0) < COAL_CHANCE && self.mode == Mode::Survival {
                self.give(Item::Coal, 1);
                self.say("It leaves only coal.");
            } else {
                let line = KILL_LINES[self.kills as usize % KILL_LINES.len()];
                self.say(line);
            }
        } else {
            ctx.play_wav(&self.art.sounds.hollow_hurt);
        }
        true
    }

    // ---- The Watcher ----

    /// Some nights, once, far off: it stands and looks. Look at it, or go
    /// to it, and it's gone.
    pub fn update_watcher(&mut self, ctx: &mut Context) {
        let dt = ctx.dt();
        let daylight = sky_at(self.time_of_day()).daylight;
        let night = self.day_number();
        if self.watcher.is_none() && daylight < 0.05 && self.watcher_night != night {
            self.watcher_night = night;
            // Whether it comes is fixed for each night of each island.
            if random01(night as i32, 0, 0, self.world.seed.wrapping_add(40)) < WATCHER_CHANCE {
                self.place_watcher(ctx);
            }
        }
        let eye = self.eye();
        let look = self.look_direction();
        let Some(watcher) = &mut self.watcher else {
            return;
        };
        watcher.age += dt;
        let head = watcher.position + vec3(0.0, 1.6 * WATCHER_SIZE, 0.0);
        let to = head - eye;
        let looking = look.dot(to.normalized()) > 0.992; // within 7 degrees
        watcher.seen = if looking {
            watcher.seen + dt
        } else {
            (watcher.seen - dt).max(0.0)
        };
        let close = vec2(to.x, to.z).length() < 14.0;
        let gone = watcher.seen > 0.2 || close || watcher.age > 120.0 || daylight > 0.2;
        if gone {
            if daylight <= 0.2 {
                ctx.play_wav(&self.art.sounds.sting);
                self.scare = 0.6;
            }
            self.watcher = None;
        }
        ctx.report("watcher", self.watcher.is_some());
    }

    /// At the edge of the view (off to one side, so it's seen from the
    /// corner of the eye), 24 to 32 blocks off, on the ground.
    pub fn place_watcher(&mut self, ctx: &mut Context) {
        for _ in 0..12 {
            let side = if ctx.rng.range(0.0, 1.0) < 0.5 {
                -1.0
            } else {
                1.0
            };
            let angle = self.yaw + side * ctx.rng.range(0.45, 0.85);
            let distance = ctx.rng.range(24.0, 32.0);
            let x = (self.position.x + angle.sin() * distance).floor() as i32;
            let z = (self.position.z + angle.cos() * distance).floor() as i32;
            if !World::inside(x, 0, z) {
                continue;
            }
            let mut top = TALL as i32 - 1;
            while top > 0 && !self.world.get(x, top, z).solid() {
                top -= 1;
            }
            if self.world.get(x, top, z) == Block::Water || top <= SEA {
                continue;
            }
            let position = vec3(x as f32 + 0.5, top as f32 + 1.0, z as f32 + 0.5);
            let yaw = (self.position.x - position.x).atan2(self.position.z - position.z);
            self.watcher = Some(Watcher::new(position, yaw));
            return;
        }
    }

    // ---- Drawing ----

    pub fn draw_night(&self, canvas: &mut Canvas, camera: &Camera3D) {
        let art = &self.art;
        for (i, hollow) in self.hollows.iter().enumerate() {
            if hollow.position.distance(camera.position) > 70.0 {
                continue;
            }
            let fade = hollow.fading.map_or(0.0, |t| (t / FADE_SECONDS).min(1.0));
            if fade == 0.0 {
                self.draw_shadow(canvas, camera, hollow.position, 0.8);
            }
            let swing = (hollow.stride * 5.0).sin() * 0.6;
            let to_duck = self.eye() - (hollow.position + vec3(0.0, 1.8, 0.0));
            let pose = Pose {
                yaw: hollow.yaw,
                // Head down, or up at you.
                pitch: if hollow.chasing {
                    (-to_duck.y / to_duck.length().max(0.1)).clamp(-0.6, 0.6)
                } else {
                    0.35
                },
                swing,
                lift: if hollow.chasing {
                    1.35 + hollow.lunge * 1.5
                } else {
                    0.15
                },
            };
            for (mesh, place) in art.hollow.parts_sized(hollow.position, pose, HOLLOW_SIZE) {
                if hollow.hurt > 0.0 {
                    canvas.draw_mesh_tinted(mesh, &place, camera, Color::rgba(255, 60, 50, 255));
                } else if fade > 0.0 {
                    let alpha = ((1.0 - fade) * 255.0) as u8;
                    canvas.draw_mesh_tinted(
                        mesh,
                        &place,
                        camera,
                        Color::rgba(150, 146, 140, alpha),
                    );
                } else {
                    canvas.draw_mesh(mesh, &place, camera);
                }
            }
            if self.aimed_hollow_now == Some(i) {
                let (low, high) = hollow.bounds();
                draw_box_edges(canvas, camera, low, high, Color::rgba(20, 20, 24, 200));
            }
            // (Labels only while playing: a menu over the world has its own.)
            if let (Stage::Playing, Some(rect)) = (
                &self.stage,
                screen_box(camera, canvas, hollow.position, 0.7, HOLLOW.height),
            ) {
                canvas.label("hollow", rect);
            }
        }
        if let Some(watcher) = &self.watcher {
            let pose = Pose {
                yaw: watcher.yaw,
                pitch: 0.25, // looking down at you, a little
                swing: 0.0,
                lift: -0.08,
            };
            for (mesh, place) in art
                .watcher
                .parts_sized(watcher.position, pose, WATCHER_SIZE)
            {
                canvas.draw_mesh(mesh, &place, camera);
            }
            // Its eyes, drawn over everything (the fog doesn't hide them).
            let forward = vec3(watcher.yaw.sin(), 0.0, watcher.yaw.cos());
            let right = vec3(watcher.yaw.cos(), 0.0, -watcher.yaw.sin());
            let head = watcher.position
                + vec3(0.0, (20.0 + 4.5) * SKIN_PIXEL * WATCHER_SIZE, 0.0)
                + forward * 4.0 * SKIN_PIXEL * WATCHER_SIZE;
            for side in [-1.5, 1.5] {
                let eye = head + right * side * SKIN_PIXEL * WATCHER_SIZE;
                if let Some(p) = camera.world_to_screen(eye, canvas) {
                    canvas.fill_rect(
                        Rect::new(p.x.round(), p.y.round(), 1.0, 1.0),
                        Color::rgb(240, 240, 255),
                    );
                }
            }
            // (Labels only while playing: a menu over the world has its own.)
            if let (Stage::Playing, Some(rect)) = (
                &self.stage,
                screen_box(camera, canvas, watcher.position, 1.2, 3.0),
            ) {
                canvas.label("watcher", rect);
            }
        }
    }
}

/// The chance, each night, that the Watcher comes.
pub const WATCHER_CHANCE: f32 = 0.5;
