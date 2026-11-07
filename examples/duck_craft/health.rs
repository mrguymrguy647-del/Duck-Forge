//! Health, hunger and air, shown over the hotbar.
//!
//! - **Health** (10 hearts, 20 points): falling more than 3 blocks hurts,
//!   one point a block beyond that; so does running out of air under
//!   water, and an empty stomach (down to half a heart). When it's all
//!   gone, you faint and wake at the island's middle, keeping your things.
//! - **Hunger** (10 slices of bread, 20 points): goes down as you walk,
//!   run, jump and dig. When it's nearly full, health comes back by itself.
//! - **Food**: hold right click (or C) with food in hand to eat it: an
//!   apple (from leaves), seeds (from tall grass) or bread (three seeds,
//!   in the crafting grid).
//! - **Air** (bubbles): 15 seconds of it under water, shown while it's
//!   less than full.

use super::*;

pub const MAX_HEALTH: u32 = 20;
pub const MAX_FOOD: u32 = 20;
pub const MAX_AIR: f32 = 15.0; // seconds
const SAFE_FALL: f32 = 3.0; // blocks
const EAT_SECONDS: f32 = 1.2;
const HEAL_SECONDS: f32 = 4.0; // a point of health, when well fed
const STARVE_SECONDS: f32 = 4.0; // a point lost, when starving
const DROWN_SECONDS: f32 = 1.0; // two points lost, out of air
// Hunger: a point goes when this adds up to 1.
const TIRED_STANDING: f32 = 0.006; // a second
const TIRED_WALKING: f32 = 0.02; // a second
const TIRED_RUNNING: f32 = 0.05; // a second
const TIRED_JUMP: f32 = 0.05;
pub const TIRED_DIG: f32 = 0.025; // a block

/// How the duck is: health, hunger, air.
#[derive(Debug, Clone)]
pub struct Vitals {
    pub health: u32,
    pub food: u32,
    pub air: f32,
    /// Hunger to come: a food point goes each time this reaches 1.
    pub tiredness: f32,
    /// The highest point of this fall (`None`: on the ground or in water).
    pub fall_from: Option<f32>,
    /// Counting to the next heal, starve or drown.
    pub heal_clock: f32,
    pub hurt_clock: f32,
    /// Seconds of red flash left after getting hurt.
    pub hurt_flash: f32,
    pub fainted: bool,
    pub faints: u32,
}

impl Default for Vitals {
    fn default() -> Vitals {
        Vitals {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            air: MAX_AIR,
            tiredness: 0.0,
            fall_from: None,
            heal_clock: 0.0,
            hurt_clock: 0.0,
            hurt_flash: 0.0,
            fainted: false,
            faints: 0,
        }
    }
}

impl DuckCraft {
    /// Hurts the duck by `points`, for `why`.
    pub fn hurt(&mut self, ctx: &mut Context, points: u32, why: &str) {
        if self.vitals.fainted || points == 0 {
            return;
        }
        self.vitals.health = self.vitals.health.saturating_sub(points);
        self.vitals.hurt_flash = 0.35;
        ctx.play(Sound {
            name: "hurt",
            wave: Wave::Square,
            freq: 330.0,
            slide_to: 160.0,
            ms: 160.0,
            volume: 0.2,
            decay: 120.0,
            ..Default::default()
        });
        if self.vitals.health == 0 {
            self.vitals.fainted = true;
            self.vitals.faints += 1;
            self.close_inventory();
            self.digging = None;
            self.eating_seconds = 0.0;
            self.fainted_because = why.to_string();
            ctx.lock_pointer(false);
        }
    }

    /// Falls, air, hunger, healing and eating, each frame (after `walk`).
    pub fn update_body(&mut self, ctx: &mut Context, was_on_ground: bool) {
        let dt = ctx.dt();
        self.vitals.hurt_flash = (self.vitals.hurt_flash - dt).max(0.0);

        // Falls: from the top of the fall to where it lands.
        let y = self.position.y;
        if self.on_ground || self.in_water {
            if let (Some(top), true) = (self.vitals.fall_from, self.on_ground && !was_on_ground) {
                let blocks = top - y;
                if blocks > SAFE_FALL && !self.in_water {
                    let points = (blocks - SAFE_FALL).round() as u32;
                    self.hurt(ctx, points, "fell too far");
                }
            }
            self.vitals.fall_from = None;
        } else {
            let top = self.vitals.fall_from.get_or_insert(y);
            *top = top.max(y);
        }

        // Air: lost under water, back quickly above it.
        let eye = self.eye();
        let under = self.world.get(
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        ) == Block::Water;
        if under {
            self.vitals.air = (self.vitals.air - dt).max(0.0);
        } else {
            self.vitals.air = (self.vitals.air + dt * 6.0).min(MAX_AIR);
        }

        // Hunger.
        let moving = self.walking && (self.on_ground || self.in_water);
        let running = moving && ctx.input.is_down(Key::LeftShift);
        self.vitals.tiredness += dt
            * if running {
                TIRED_RUNNING
            } else if moving {
                TIRED_WALKING
            } else {
                TIRED_STANDING
            };
        if was_on_ground && !self.on_ground && self.velocity.y > 0.0 {
            self.vitals.tiredness += TIRED_JUMP;
        }
        while self.vitals.tiredness >= 1.0 {
            self.vitals.tiredness -= 1.0;
            self.vitals.food = self.vitals.food.saturating_sub(1);
        }

        // Healing when well fed; hurting when starving or drowning.
        self.vitals.heal_clock += dt;
        self.vitals.hurt_clock += dt;
        if self.vitals.food >= MAX_FOOD - 2 && self.vitals.health < MAX_HEALTH {
            if self.vitals.heal_clock >= HEAL_SECONDS {
                self.vitals.heal_clock = 0.0;
                self.vitals.health += 1;
                self.vitals.tiredness += 0.6; // healing makes you hungry
            }
        } else {
            self.vitals.heal_clock = 0.0;
        }
        if self.vitals.air <= 0.0 {
            if self.vitals.hurt_clock >= DROWN_SECONDS {
                self.vitals.hurt_clock = 0.0;
                self.hurt(ctx, 2, "ran out of air");
            }
        } else if self.vitals.food == 0 && self.vitals.health > 1 {
            if self.vitals.hurt_clock >= STARVE_SECONDS {
                self.vitals.hurt_clock = 0.0;
                self.hurt(ctx, 1, "was too hungry");
            }
        } else {
            self.vitals.hurt_clock = 0.0;
        }

        self.eat(ctx);
    }

    /// Hold right click (or C) with food in hand to eat it.
    fn eat(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        let holding_food = self.holding().and_then(|s| s.item.food());
        let pressing = (input.is_mouse_down(MouseButton::Right) || input.is_down(Key::C))
            && self.screen.is_none();
        let (Some(food), true) = (holding_food, pressing) else {
            self.eating_seconds = 0.0;
            return;
        };
        if self.vitals.food >= MAX_FOOD {
            if input.was_mouse_pressed(MouseButton::Right) || input.was_pressed(Key::C) {
                self.say("You're full");
            }
            self.eating_seconds = 0.0;
            return;
        }
        let before = self.eating_seconds;
        self.eating_seconds += ctx.dt();
        if (before / 0.25).floor() != (self.eating_seconds / 0.25).floor() {
            ctx.play(Sound {
                name: "munch",
                wave: Wave::Noise,
                freq: 1200.0,
                ms: 60.0,
                volume: 0.12,
                decay: 50.0,
                ..Default::default()
            });
        }
        if self.eating_seconds >= EAT_SECONDS {
            self.eating_seconds = 0.0;
            if let Some(item) = self.inventory.take_one_from(self.slot) {
                self.vitals.food = (self.vitals.food + food).min(MAX_FOOD);
                self.eaten += 1;
                self.say(format!("Ate {}", item.name()));
                ctx.play(Sound {
                    name: "gulp",
                    wave: Wave::Sine,
                    freq: 500.0,
                    slide_to: 250.0,
                    ms: 120.0,
                    volume: 0.15,
                    ..Default::default()
                });
            }
        }
    }

    /// Fainted: wait for Space, Enter or a click, then wake at the spawn.
    pub fn update_fainted(&mut self, ctx: &mut Context) {
        let input = &ctx.input;
        let wake = input.was_pressed(Key::Space)
            || input.was_pressed(Key::Enter)
            || input.was_mouse_pressed(MouseButton::Left);
        if wake {
            let faints = self.vitals.faints;
            self.vitals = Vitals {
                faints,
                ..Vitals::default()
            };
            self.position = self.world.spawn;
            self.velocity = Vec3::ZERO;
            self.yaw = self.world.spawn_yaw;
            self.pitch = -0.25;
            ctx.lock_pointer(true);
            self.left_click_locked = true; // that click doesn't dig
            self.say("You wake up on the island");
        }
    }

    /// Hearts, bread and bubbles over the hotbar; a red flash when hurt.
    pub fn draw_body(&self, canvas: &mut Canvas) {
        let hud = &self.art.hud;
        let left = (WIDTH - HOTBAR_SLOT * 9.0) / 2.0;
        let right = left + HOTBAR_SLOT * 9.0;
        let y = HEIGHT - HOTBAR_SLOT - 3.0 - 12.0;
        let shaky = self.vitals.health <= 4 && !self.vitals.fainted;
        // Ten icons for 20 points: full, half or empty.
        let icon = |points: u32, i: u32| match points.saturating_sub(i * 2) {
            0 => "empty",
            1 => "half",
            _ => "full",
        };
        for i in 0..10 {
            let shake = if shaky {
                ((self.time * 30.0 + i as f32 * 1.7).sin() * 1.2).round()
            } else {
                0.0
            };
            let at = vec2(left + i as f32 * 8.0, y + shake);
            let name = format!("heart_{}", icon(self.vitals.health, i));
            canvas.blit_sprite(hud.frame(&name, 0.0), at, Blit::default());
            // Bread from the right, like the hearts from the left.
            let at = vec2(right - 9.0 - i as f32 * 8.0, y);
            let name = format!("bread_{}", icon(self.vitals.food, i));
            canvas.blit_sprite(hud.frame(&name, 0.0), at, Blit::default());
        }
        if self.vitals.air < MAX_AIR {
            let bubbles = (self.vitals.air / MAX_AIR * 10.0).ceil() as usize;
            for i in 0..bubbles {
                let at = vec2(right - 9.0 - i as f32 * 8.0, y - 10.0);
                canvas.blit_sprite(hud.frame("bubble", 0.0), at, Blit::default());
            }
        }
        canvas.label("health", Rect::new(left, y, 80.0, 9.0));
        canvas.label("hunger", Rect::new(right - 80.0, y, 80.0, 9.0));

        if self.vitals.hurt_flash > 0.0 {
            let alpha = (self.vitals.hurt_flash / 0.35 * 90.0) as u8;
            canvas.fill_rect(
                Rect::new(0.0, 0.0, WIDTH, HEIGHT),
                Color::rgba(200, 20, 20, alpha),
            );
        }
        if self.vitals.fainted {
            canvas.fill_rect(
                Rect::new(0.0, 0.0, WIDTH, HEIGHT),
                Color::rgba(110, 10, 10, 150),
            );
            canvas.draw_text_centered("You fainted!", 80.0, 2, Color::WHITE);
            let why = format!("The duck {}", self.fainted_because);
            canvas.draw_text_centered(&why, 104.0, 1, Color::rgb(255, 210, 210));
            let button = Rect::new(WIDTH / 2.0 - 60.0, 128.0, 120.0, 20.0);
            canvas.fill_rect(button, Color::rgb(120, 120, 120));
            canvas.draw_rect(button, Color::rgb(30, 30, 30));
            canvas.draw_text_centered("Wake up (Space)", 134.0, 1, Color::WHITE);
            canvas.label("wake_up", button);
        }
    }
}
