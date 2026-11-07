//! The title screen, the pause menu, the two ways to play, the music, and
//! the lines the island says at dusk and dawn.
//!
//! - **Title**: the island turns slowly behind the menu. Carry on (if
//!   there's a save), a new island in survival or in creative, quit.
//!   A new island over a saved one asks first.
//! - **Pause** (**Escape** while playing): back to the island, switch
//!   between survival and creative, music on or off, save and quit.
//! - **Survival**: health, hunger, the night's shapes. **Creative**: fly
//!   (Space twice; Space up, Shift down), every block from the panel in
//!   the inventory, blocks break at once and never run out, nothing hurts.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Survival,
    Creative,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Survival => "Survival",
            Mode::Creative => "Creative",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// `confirm`: "a new island" was picked once over a saved one.
    Title {
        selected: usize,
        confirm: bool,
    },
    Playing,
    Paused {
        selected: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleChoice {
    Continue,
    New(Mode),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseChoice {
    Resume,
    Mode,
    Music,
    SaveAndQuit,
}

const PAUSE: [PauseChoice; 4] = [
    PauseChoice::Resume,
    PauseChoice::Mode,
    PauseChoice::Music,
    PauseChoice::SaveAndQuit,
];

/// Music volumes, at most.
const DAY_MUSIC: f32 = 0.45;
const NIGHT_MUSIC: f32 = 0.6;
const TITLE_MUSIC: f32 = 0.6;
const WIND: f32 = 0.18;

/// The loops that are always running (quiet when not wanted).
#[derive(Debug, Clone)]
pub struct Music {
    title: LoopId,
    day: LoopId,
    night: LoopId,
    wind: LoopId,
    heart: LoopId,
    /// The volume each was last set to: title, day, night, wind, heart.
    volumes: [f32; 5],
}

const DUSK_LINES: [&str; 6] = [
    "The light goes. It always does.",
    "Night again. They remember the way here.",
    "Somewhere in the dark, something that was a duck is walking.",
    "Nothing you build keeps the night out. Build anyway.",
    "The sea is black now. It was always this deep.",
    "Light a lamp. It won't matter, but light it.",
];
const DAWN_LINES: [&str; 6] = [
    "Nothing has changed.",
    "The sun came back. It doesn't know why either.",
    "The island didn't notice you were gone.",
    "Another one.",
    "Still here. So is everything else.",
    "The night left nothing behind. It never does.",
];

/// A button's box on the title or pause screen.
pub fn button_rect(i: usize, top: f32) -> Rect {
    Rect::new(WIDTH / 2.0 - 75.0, top + i as f32 * 22.0, 150.0, 18.0)
}

const TITLE_TOP: f32 = 118.0;
const PAUSE_TOP: f32 = 82.0;

impl DuckCraft {
    /// What the title screen offers.
    pub fn title_choices(&self) -> Vec<TitleChoice> {
        let mut choices = Vec::new();
        if self.has_save {
            choices.push(TitleChoice::Continue);
        }
        choices.push(TitleChoice::New(Mode::Survival));
        choices.push(TitleChoice::New(Mode::Creative));
        // Only a window can be quit from a menu (a headless run plays on).
        if self.save_path.is_some() {
            choices.push(TitleChoice::Quit);
        }
        choices
    }

    fn title_label(&self, choice: TitleChoice, confirm: bool) -> String {
        match choice {
            TitleChoice::Continue => format!("Carry on (day {})", self.day_number()),
            TitleChoice::New(_) if confirm => String::from("Forget the old island?"),
            TitleChoice::New(mode) => format!("New island: {}", mode.name().to_lowercase()),
            TitleChoice::Quit => String::from("Quit"),
        }
    }

    /// Up and down, Enter (or Space, or a click) to pick.
    fn menu_input(ctx: &Context, selected: &mut usize, count: usize, top: f32) -> bool {
        let input = &ctx.input;
        let pressed = |keys: &[Key]| keys.iter().any(|&k| input.was_pressed(k));
        if pressed(&[Key::W, Key::Up]) {
            *selected = (*selected + count - 1) % count;
        }
        if pressed(&[Key::S, Key::Down]) {
            *selected = (*selected + 1) % count;
        }
        let mut clicked = false;
        if let Some(point) = input.mouse_position() {
            for i in 0..count {
                if button_rect(i, top).contains(point) {
                    if input.mouse_delta().length() > 0.0 {
                        *selected = i; // hovering picks it
                    }
                    if input.was_mouse_pressed(MouseButton::Left) {
                        *selected = i;
                        clicked = true;
                    }
                }
            }
        }
        clicked || pressed(&[Key::Enter, Key::Space])
    }

    pub fn update_title(&mut self, ctx: &mut Context) {
        let Stage::Title {
            mut selected,
            mut confirm,
        } = self.stage.clone()
        else {
            return;
        };
        ctx.lock_pointer(false);
        let choices = self.title_choices();
        let before = selected;
        let chosen = Self::menu_input(ctx, &mut selected, choices.len(), TITLE_TOP);
        if selected != before {
            confirm = false;
            ctx.play(inventory::click(800.0));
        }
        self.stage = Stage::Title { selected, confirm };
        if !chosen {
            return;
        }
        ctx.play(inventory::click(600.0));
        match choices[selected] {
            TitleChoice::Continue => self.start_playing(ctx),
            TitleChoice::New(mode) => {
                if self.has_save && !confirm {
                    self.stage = Stage::Title {
                        selected,
                        confirm: true,
                    };
                    return;
                }
                if self.has_save {
                    // A new seed: a different island.
                    self.new_island(clock_seed());
                }
                self.set_mode(mode);
                if let Err(error) = self.save() {
                    eprintln!("duck_craft: couldn't save: {error}");
                }
                self.has_save = self.save_path.is_some();
                self.start_playing(ctx);
            }
            TitleChoice::Quit => ctx.quit(),
        }
    }

    fn start_playing(&mut self, ctx: &mut Context) {
        self.stage = Stage::Playing;
        ctx.lock_pointer(true);
        self.left_click_locked = true; // the click that chose doesn't dig
        let day = self.day_number();
        self.announce(format!("Day {day}"), "The island is where you left it.");
        if self.time == 0.0 {
            // When a new island's first day starts: just after sunrise,
            // unless tuned (`--set start_time_of_day=0.7` starts at night,
            // for testing the night).
            let start = ctx.tune("start_time_of_day", DAY_START);
            self.time = (start - DAY_START).rem_euclid(1.0) * DAY_SECONDS;
            self.last_time_of_day = self.time_of_day();
            self.announce("Day 1", "You are here. That's all anyone knows.");
            self.show_help = true;
        }
    }

    /// Throws this island away for a new one grown from `seed`, keeping
    /// the music and where the game saves.
    pub fn new_island(&mut self, seed: u32) {
        let mut fresh = DuckCraft::new(self.art.clone(), World::generate(seed));
        fresh.save_path = self.save_path.clone();
        fresh.music = self.music.clone();
        fresh.music_on = self.music_on;
        fresh.draw_pointer = self.draw_pointer;
        fresh.stage = self.stage.clone();
        *self = fresh;
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode {
            return;
        }
        self.mode = mode;
        self.flying = false;
        self.vitals.fall_from = None;
        if mode == Mode::Creative {
            // The things you carried wait in the backpack; the hotbar
            // fills with blocks.
            let carried: Vec<Stack> = self.inventory.slots[..HOTBAR_SLOTS]
                .iter()
                .flatten()
                .copied()
                .collect();
            let kit = items::creative_kit();
            self.inventory.slots[..HOTBAR_SLOTS].copy_from_slice(&kit.slots[..HOTBAR_SLOTS]);
            for stack in carried {
                let range = HOTBAR_SLOTS..SLOTS;
                let free = range.clone().find(|&i| self.inventory.slots[i].is_none());
                if let Some(i) = free {
                    self.inventory.slots[i] = Some(stack);
                }
            }
        }
    }

    pub fn update_pause(&mut self, ctx: &mut Context) {
        let Stage::Paused { mut selected } = self.stage.clone() else {
            return;
        };
        if ctx.input.was_pressed(Key::Escape) {
            self.resume(ctx);
            return;
        }
        let before = selected;
        let chosen = Self::menu_input(ctx, &mut selected, PAUSE.len(), PAUSE_TOP);
        if selected != before {
            ctx.play(inventory::click(800.0));
        }
        self.stage = Stage::Paused { selected };
        if !chosen {
            return;
        }
        ctx.play(inventory::click(600.0));
        match PAUSE[selected] {
            PauseChoice::Resume => self.resume(ctx),
            PauseChoice::Mode => {
                let other = match self.mode {
                    Mode::Survival => Mode::Creative,
                    Mode::Creative => Mode::Survival,
                };
                self.set_mode(other);
            }
            PauseChoice::Music => self.music_on = !self.music_on,
            PauseChoice::SaveAndQuit => {
                if let Err(error) = self.save() {
                    eprintln!("duck_craft: couldn't save: {error}");
                }
                ctx.quit();
            }
        }
    }

    pub fn pause(&mut self, ctx: &mut Context) {
        self.stage = Stage::Paused { selected: 0 };
        self.digging = None;
        self.eating_seconds = 0.0;
        self.pointer_free = true;
        ctx.lock_pointer(false);
    }

    fn resume(&mut self, ctx: &mut Context) {
        self.stage = Stage::Playing;
        self.pointer_free = false;
        ctx.lock_pointer(true);
        self.left_click_locked = true;
    }

    // ---- The music ----

    /// Starts the loops (once) and fades each towards where it should be:
    /// the title's on the title screen, day and night by the light, the
    /// heartbeat when hurt.
    pub fn update_music(&mut self, ctx: &mut Context) {
        let daylight = sky_at(self.time_of_day()).daylight;
        let art = self.art.clone();
        let sounds = &art.sounds;
        self.music.get_or_insert_with(|| Music {
            title: ctx.play_loop(&sounds.music_title, 0.0),
            day: ctx.play_loop(&sounds.music_day, 0.0),
            night: ctx.play_loop(&sounds.music_night, 0.0),
            wind: ctx.play_loop(&art.wind, 0.0),
            heart: ctx.play_loop(&sounds.heartbeat, 0.0),
            volumes: [0.0; 5],
        });
        let on = if self.music_on { 1.0 } else { 0.0 };
        let (title, playing, paused) = match &self.stage {
            Stage::Title { .. } => (1.0, 0.0, false),
            Stage::Playing => (0.0, 1.0, false),
            Stage::Paused { .. } => (0.0, 1.0, true),
        };
        let hush = if paused { 0.35 } else { 1.0 };
        let hurt = if self.mode == Mode::Survival && !self.vitals.fainted && playing > 0.0 {
            ((7.0 - self.vitals.health as f32) / 6.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let music = self.music.as_mut().expect("started above");
        let targets = [
            title * TITLE_MUSIC * on,
            playing * daylight * DAY_MUSIC * on * hush,
            playing * (1.0 - daylight) * NIGHT_MUSIC * on * hush,
            (playing + title * 0.5) * WIND * hush,
            hurt * 0.8 * hush,
        ];
        let ids = [music.title, music.day, music.night, music.wind, music.heart];
        let step = ctx.dt() / 2.0; // fades take about two seconds
        for i in 0..5 {
            let now = music.volumes[i];
            let next = approach(now, targets[i], step.max(if i == 4 { 0.05 } else { 0.0 }));
            // Only tell the mixer when it really changed (keeps logs short).
            if (next - now).abs() > 0.004 || (next == targets[i] && now != targets[i]) {
                ctx.set_loop_volume(ids[i], next);
                music.volumes[i] = next;
            }
        }
    }

    // ---- Days and nights ----

    /// Which day it is, from 1.
    pub fn day_number(&self) -> u32 {
        (self.time / DAY_SECONDS + DAY_START).floor() as u32 + 1
    }

    /// A line in the middle of the screen for a few seconds.
    pub fn announce(&mut self, title: impl Into<String>, line: impl Into<String>) {
        self.announcement = Some((title.into(), line.into(), 5.0));
    }

    /// Dusk and dawn: a bell, and a line.
    pub fn day_and_night(&mut self, ctx: &mut Context) {
        let now = self.time_of_day();
        let before = self.last_time_of_day;
        self.last_time_of_day = now;
        let crossed = |at: f32| {
            if before <= now {
                before < at && now >= at
            } else {
                before < at || now >= at // wrapped past midnight's end
            }
        };
        let seed = self.world.seed as usize;
        let pick = |lines: &[&'static str], n: u32| lines[(n as usize * 7 + seed) % lines.len()];
        if crossed(DUSK) {
            ctx.play_wav(&self.art.sounds.bell_dusk);
            let line = pick(&DUSK_LINES, self.day_number());
            self.announce("Night", line);
        }
        if crossed(DAWN) {
            ctx.play_wav(&self.art.sounds.bell_dawn);
            let day = self.day_number();
            let line = pick(&DAWN_LINES, day);
            self.announce(format!("Day {day}"), line);
        }
        if let Some((_, _, seconds)) = &mut self.announcement {
            *seconds -= ctx.dt();
            if *seconds <= 0.0 {
                self.announcement = None;
            }
        }
    }

    // ---- Drawing ----

    /// The title over the turning island, and its buttons.
    pub fn draw_title(&self, canvas: &mut Canvas) {
        let Stage::Title { selected, confirm } = self.stage else {
            return;
        };
        // Dark at the top and bottom, so the words stand out.
        for i in 0..12 {
            let a = (150 - i * 12) as u8;
            canvas.fill_rect(
                Rect::new(0.0, i as f32 * 4.0, WIDTH, 4.0),
                Color::rgba(0, 0, 0, a),
            );
            canvas.fill_rect(
                Rect::new(0.0, HEIGHT - (i + 1) as f32 * 4.0, WIDTH, 4.0),
                Color::rgba(0, 0, 0, a),
            );
        }
        let title = "DUCK CRAFT";
        let w = Canvas::text_width(title, 3);
        let at = vec2((WIDTH - w) / 2.0, 34.0);
        canvas.draw_text(title, at + vec2(2.0, 2.0), 3, Color::rgb(20, 10, 10));
        canvas.draw_text(title, at, 3, Color::rgb(232, 226, 210));
        canvas.label("title", Rect::new(at.x, at.y, w, 21.0));
        let line = "nothing lasts. build anyway.";
        canvas.draw_text_centered(line, 65.0, 1, Color::rgba(0, 0, 0, 200));
        canvas.draw_text_centered(line, 64.0, 1, Color::rgb(200, 186, 170));
        let choices = self.title_choices();
        for (i, &choice) in choices.iter().enumerate() {
            let label = self.title_label(choice, confirm && i == selected);
            draw_button(
                canvas,
                button_rect(i, TITLE_TOP),
                &label,
                i == selected,
                confirm && i == selected,
            );
        }
        let hint = format!("Island {:08x}", self.world.seed);
        canvas.draw_text(
            &hint,
            vec2(4.0, HEIGHT - 10.0),
            1,
            Color::rgb(110, 105, 100),
        );
        self.draw_menu_pointer(canvas);
    }

    pub fn draw_pause(&self, canvas: &mut Canvas) {
        let Stage::Paused { selected } = self.stage else {
            return;
        };
        canvas.fill_rect(
            Rect::new(0.0, 0.0, WIDTH, HEIGHT),
            Color::rgba(0, 0, 0, 150),
        );
        canvas.draw_text_centered("Paused", 44.0, 2, Color::rgb(232, 226, 210));
        let day = format!(
            "Day {}. The island waits. It's good at that.",
            self.day_number()
        );
        canvas.draw_text_centered(&day, 64.0, 1, Color::rgb(170, 160, 150));
        for (i, &choice) in PAUSE.iter().enumerate() {
            let label = match choice {
                PauseChoice::Resume => String::from("Back to the island"),
                PauseChoice::Mode => format!("Mode: {}", self.mode.name()),
                PauseChoice::Music => {
                    format!("Music: {}", if self.music_on { "on" } else { "off" })
                }
                PauseChoice::SaveAndQuit => String::from(if self.save_path.is_some() {
                    "Save and quit"
                } else {
                    "Quit"
                }),
            };
            draw_button(
                canvas,
                button_rect(i, PAUSE_TOP),
                &label,
                i == selected,
                false,
            );
        }
        canvas.label(
            "pause_menu",
            Rect::new(WIDTH / 2.0 - 75.0, PAUSE_TOP, 150.0, 86.0),
        );
        self.draw_menu_pointer(canvas);
    }

    fn draw_menu_pointer(&self, canvas: &mut Canvas) {
        if let (Some(p), true) = (self.mouse, self.draw_pointer) {
            canvas.fill_triangle(
                p + vec2(-1.0, -1.0),
                p + vec2(-1.0, 12.0),
                p + vec2(9.0, 8.0),
                Color::BLACK,
            );
            canvas.fill_triangle(p, p + vec2(0.0, 10.0), p + vec2(7.0, 7.0), Color::WHITE);
        }
    }

    /// "Day 3" and its line, in the middle, fading.
    pub fn draw_announcement(&self, canvas: &mut Canvas) {
        let Some((title, line, seconds)) = &self.announcement else {
            return;
        };
        let alpha =
            (seconds.min(1.0) * (5.0 - seconds).min(0.6) / 0.6 * 255.0).clamp(0.0, 255.0) as u8;
        // Below the middle while the help is up (it fills the top).
        let top = if self.show_help && self.screen.is_none() {
            140.0
        } else {
            46.0
        };
        canvas.draw_text_centered(title, top + 1.0, 2, Color::rgba(0, 0, 0, alpha / 2));
        canvas.draw_text_centered(title, top, 2, Color::rgba(236, 230, 214, alpha));
        canvas.draw_text_centered(line, top + 20.0, 1, Color::rgba(0, 0, 0, alpha / 2));
        canvas.draw_text_centered(line, top + 19.0, 1, Color::rgba(200, 190, 176, alpha));
    }

    /// Dark edges: faint at night, closing in and going red when hurt.
    pub fn draw_vignette(&self, canvas: &mut Canvas) {
        let night = 1.0 - sky_at(self.time_of_day()).daylight;
        let hurt = if self.mode == Mode::Survival {
            ((8.0 - self.vitals.health as f32) / 8.0).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let strength = (night * 0.45 + hurt * 0.8).min(1.0);
        if strength < 0.02 {
            return;
        }
        // A beat in it when badly hurt.
        let beat = if hurt > 0.0 {
            1.0 + 0.25 * hurt * (self.time * TAU).sin().max(0.0).powi(8)
        } else {
            1.0
        };
        let red = (hurt * 120.0) as u8;
        for ring in 0..10 {
            let k = ring as f32 / 10.0;
            let alpha = ((1.0 - k).powi(2) * 200.0 * strength * beat).min(255.0) as u8;
            let inset = k * 46.0;
            let t = 4.6;
            let c = Color::rgba(red / 2, 0, 0, alpha);
            canvas.fill_rect(Rect::new(inset, inset, WIDTH - 2.0 * inset, t), c);
            canvas.fill_rect(
                Rect::new(inset, HEIGHT - inset - t, WIDTH - 2.0 * inset, t),
                c,
            );
            canvas.fill_rect(
                Rect::new(inset, inset + t, t, HEIGHT - 2.0 * (inset + t)),
                c,
            );
            canvas.fill_rect(
                Rect::new(WIDTH - inset - t, inset + t, t, HEIGHT - 2.0 * (inset + t)),
                c,
            );
        }
    }
}

/// A grey button, lit when picked; reddish when it's asking to be sure.
fn draw_button(canvas: &mut Canvas, r: Rect, label: &str, selected: bool, warning: bool) {
    let fill = match (selected, warning) {
        (_, true) => Color::rgba(120, 40, 36, 230),
        (true, false) => Color::rgba(110, 108, 104, 230),
        (false, false) => Color::rgba(46, 45, 44, 210),
    };
    canvas.fill_rect(r, fill);
    canvas.draw_rect(
        r,
        if selected {
            Color::rgb(230, 224, 210)
        } else {
            Color::rgb(16, 16, 16)
        },
    );
    let color = if selected {
        Color::WHITE
    } else {
        Color::rgb(190, 186, 178)
    };
    let w = Canvas::text_width(label, 1);
    canvas.draw_text(label, vec2(r.x + (r.w - w) / 2.0, r.y + 5.0), 1, color);
    canvas.label("button", r);
}

/// A seed from the clock, for a new island.
pub fn clock_seed() -> u32 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    (nanos as u32) ^ ((nanos >> 32) as u32)
}

/// The light at a time of day (0 sunrise, 0.25 noon, 0.5 sunset, 0.75
/// midnight): where the sun is, how much daylight (0 to 1), and how much
/// sunset glow.
#[derive(Debug, Clone, Copy)]
pub struct Sky {
    pub sun: Vec3,
    pub daylight: f32,
    pub dusk: f32,
}

pub fn sky_at(time_of_day: f32) -> Sky {
    let angle = time_of_day * TAU;
    let sun = vec3(angle.cos(), angle.sin(), 0.35).normalized();
    Sky {
        sun,
        daylight: smooth(((sun.y + 0.15) / 0.4).clamp(0.0, 1.0)),
        dusk: (1.0 - (sun.y.abs() / 0.3).min(1.0)) * 0.8,
    }
}

/// When the island calls it dusk and dawn (as times of day).
pub const DUSK: f32 = 0.5;
pub const DAWN: f32 = 0.0; // (noticed as the day wraps round)
/// The game starts this far into the first day (just after sunrise).
pub const DAY_START: f32 = 0.05;
