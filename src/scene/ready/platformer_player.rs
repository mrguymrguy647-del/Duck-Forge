//! Platformer player: runs, jumps, falls, and lands on one-way planks.
//! A ready-made script that comes with Duck Forge, written just as a
//! game's own scripts are. "Turn into code" in Duck Forge Studio gives a
//! game its own copy of this file, to change.

use duckforge::prelude::*;

#[derive(Default)]
pub struct PlatformerPlayer {
    // Each setting is a box in the Inspector.
    run_speed: f32,
    jump_height: f32,
    gravity: f32,
    fall_limit: f32,
    speed_up: f32,
    short_hops: bool,
    coyote: f32,
    jump_early: f32,
    stays_inside: bool,
    falls_back: bool,
    jump_sound: String,
    left: String,
    right: String,
    jump: String,
    idle_animation: String,
    run_animation: String,
    jump_animation: String,
    fall_animation: String,
    // Things it remembers while the game runs. These have no box.
    velocity: Vec2,
    on_ground: bool,
    /// Seconds since it last stood on something.
    off_ground: f32,
    /// Seconds since jump was pressed (large: not lately).
    jump_asked: f32,
}

impl PlatformerPlayer {
    /// How fast it's going, in pixels a second (down is positive).
    pub fn velocity(&self) -> Vec2 {
        self.velocity
    }

    /// Is it standing on something?
    pub fn on_ground(&self) -> bool {
        self.on_ground
    }
}

impl Script for PlatformerPlayer {
    /// The settings: a name, where it goes, and its value at the start.
    fn settings(&mut self, s: &mut Settings) {
        s.number("run_speed", &mut self.run_speed, 110.0)
            .range(0.0, 400.0)
            .label("Runs at")
            .help("Pixels a second");
        s.number("jump_height", &mut self.jump_height, 60.0)
            .range(0.0, 240.0)
            .label("Jumps this high")
            .help("Pixels, holding jump all the way up");
        s.number("gravity", &mut self.gravity, 900.0)
            .range(100.0, 3000.0)
            .label("Gravity")
            .help("How fast it falls faster: pixels a second, every second");
        s.number("fall_limit", &mut self.fall_limit, 400.0)
            .range(50.0, 1500.0)
            .label("Falls at most")
            .help("Pixels a second")
            .expert();
        s.number("speed_up", &mut self.speed_up, 0.12)
            .range(0.0, 1.0)
            .label("Reaches full speed in")
            .help("Seconds, from standing still")
            .expert();
        s.flag("short_hops", &mut self.short_hops, true)
            .help("Letting go of jump early makes a lower jump")
            .expert();
        s.number("coyote", &mut self.coyote, 0.08)
            .range(0.0, 0.5)
            .label("Jumps just after an edge")
            .help("Seconds after running off an edge that jump still works")
            .expert();
        s.number("jump_early", &mut self.jump_early, 0.1)
            .range(0.0, 0.5)
            .label("Jumps pressed early")
            .help("Seconds before landing that a jump pressed still counts")
            .expert();
        s.flag("stays_inside", &mut self.stays_inside, true)
            .label("Stays inside the scene")
            .help("The scene's left and right ends are walls");
        s.flag("falls_back", &mut self.falls_back, true)
            .label("Comes back when it falls out")
            .help("Falling out of the bottom of the scene puts it back where it started");
        s.sound("jump_sound", &mut self.jump_sound, "")
            .label("Jump sound")
            .help("Empty: Duck Forge's own jump sound");
        s.words("left", &mut self.left, "left")
            .group("Keys")
            .help("The action that runs left (left arrow or A)")
            .expert();
        s.words("right", &mut self.right, "right")
            .group("Keys")
            .help("The action that runs right (right arrow or D)")
            .expert();
        s.words("jump", &mut self.jump, "jump")
            .group("Keys")
            .help("The action that jumps (Space, up arrow or W)")
            .expert();
        s.words("idle_animation", &mut self.idle_animation, "idle")
            .group("Animations")
            .label("Standing");
        s.words("run_animation", &mut self.run_animation, "run")
            .group("Animations")
            .label("Running");
        s.words("jump_animation", &mut self.jump_animation, "jump")
            .group("Animations")
            .label("Going up");
        s.words("fall_animation", &mut self.fall_animation, "fall")
            .group("Animations")
            .label("Coming down");
    }

    /// Once, when the object starts.
    fn start(&mut self, _me: Id, _scene: &mut Scene, _ctx: &mut Context) {
        self.jump_asked = f32::MAX;
        self.off_ground = f32::MAX;
    }

    /// Sixty times a second, while the game plays.
    fn update(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {
        let dt = ctx.dt();

        // Run: -1, 0 or 1 from the actions, then ease the speed towards it.
        let run = ctx.input.action_axis(&self.left, &self.right);
        let speed_up = self.run_speed / self.speed_up.max(0.001);
        self.velocity.x = approach(self.velocity.x, run * self.run_speed, speed_up * dt);
        if run != 0.0 {
            scene[me].flip = run < 0.0; // face the way it runs
        }

        // Jump: from something to stand on, or just after leaving it, and
        // a jump pressed just before landing counts too.
        self.jump_asked += dt;
        if ctx.input.action_pressed(&self.jump) {
            self.jump_asked = 0.0;
        }
        let can_jump = self.on_ground || self.off_ground <= self.coyote;
        if can_jump && self.jump_asked <= self.jump_early {
            // The speed that rises jump_height under this gravity, in
            // steps of dt (gravity is added before each step).
            let half_step = self.gravity * dt / 2.0;
            let speed =
                half_step + (half_step * half_step + 2.0 * self.gravity * self.jump_height).sqrt();
            self.velocity.y = -speed;
            self.jump_asked = f32::MAX;
            self.off_ground = f32::MAX;
            if self.jump_sound.is_empty() {
                ctx.play(Sound::jump());
            } else {
                scene.play_sound(ctx, &self.jump_sound);
            }
        }
        // Let go early on the way up: a lower jump.
        if self.short_hops && self.velocity.y < 0.0 && ctx.input.action_released(&self.jump) {
            self.velocity.y *= 0.5;
        }
        self.velocity.y = (self.velocity.y + self.gravity * dt).min(self.fall_limit);

        // The scene moves it, and says what it bumped into: the ground (the
        // tile map, one-way planks from above) or a solid object.
        let touched = scene.move_and_slide(me, self.velocity * dt);
        self.on_ground = touched.ground;
        if touched.ground {
            self.off_ground = 0.0;
        } else {
            self.off_ground += dt;
        }
        if touched.ground || touched.ceiling {
            self.velocity.y = 0.0;
        }
        if touched.wall_left || touched.wall_right {
            self.velocity.x = 0.0;
        }

        // The scene's ends, and the way back from falling out of it.
        if let Some(size) = scene.size() {
            let body = scene.rect(me);
            if self.stays_inside && body.w < size.x {
                let fix = (0.0 - body.x).max(0.0) + (size.x - body.right()).min(0.0);
                if fix != 0.0 {
                    scene[me].at.x += fix;
                    self.velocity.x = 0.0;
                }
            }
            if self.falls_back && body.y > size.y + 64.0 {
                if let Some(start) = scene.starts_as(me).map(|start| start.at) {
                    scene[me].at = start;
                }
                self.velocity = Vec2::ZERO;
                ctx.play(Sound::hit());
            }
        }

        // Which of its animations it shows, if its picture has it.
        let animation = if !self.on_ground && self.velocity.y < 0.0 {
            &self.jump_animation
        } else if !self.on_ground {
            &self.fall_animation
        } else if run != 0.0 {
            &self.run_animation
        } else {
            &self.idle_animation
        };
        if scene.has_animation(me, animation) {
            scene[me].animation = Some(animation.clone());
        }
    }
}
