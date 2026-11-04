//! Choose Your Door: a one-minute vertical video (like a YouTube Short),
//! drawn with duckforge. Four rounds: three doors, a countdown to pick one,
//! then the doors open one by one to show what was behind them.
//!
//! Watch it live:   cargo run --release --example door_short
//! Make the video:  FRAMES=folder cargo test --release --example door_short -- --ignored render
//!                  (saves one PNG per frame at 30 fps, plus cues.json: when each sound plays)
//!
//! The engine has no sound yet, so the animation only *announces* its
//! sounds: `cues()` lists every sound and the second it plays at, from the
//! same timing constants the drawing uses, so pictures and sound always line up.

// Doors are numbered 0..3 and several arrays are indexed by door, so plain index loops read best.
#![allow(clippy::needless_range_loop)]

use std::f32::consts::{PI, TAU};

use duckforge::prelude::*;

const WIDTH: f32 = 270.0;
const HEIGHT: f32 = 480.0;
const FPS: u32 = 30;

// ---- The timeline, in seconds ----

const INTRO: f32 = 3.0;
const ROUND_LEN: f32 = 13.0;
const END_START: f32 = INTRO + 4.0 * ROUND_LEN; // 55
const TOTAL: f32 = 60.0;
/// Within a round: the countdown runs from 1 to 6 seconds...
const COUNT_START: f32 = 1.0;
const COUNT_END: f32 = 6.0;
/// ...then each door opens at its own moment...
const OPEN_AT: [f32; 3] = [6.8, 8.0, 9.2];
const OPEN_TIME: f32 = 0.45;
/// ...and the winner is announced.
const WINNER_AT: f32 = 10.6;

// ---- Layout ----

const DOOR_W: f32 = 70.0;
const DOOR_H: f32 = 112.0;
const DOOR_TOP: f32 = 172.0;
const DOOR_GAP: f32 = 15.0;
const DOOR_COLORS: [Color; 3] = [
    Color::from_hex(0xd94848),
    Color::from_hex(0x3fa65a),
    Color::from_hex(0x3d7bd9),
];

const SKY_TOP: Color = Color::from_hex(0x2a0f52);
const SKY_BOTTOM: Color = Color::from_hex(0x0b3a66);
const FLOOR: Color = Color::from_hex(0x1a1433);
const FRAME: Color = Color::from_hex(0x5a3a1e);
const INSIDE: Color = Color::from_hex(0x120c22);
const GOLD: Color = Color::from_hex(0xffcf3a);
const GOLD_DARK: Color = Color::from_hex(0xc08a12);
const SHADOW: Color = Color::from_hex(0x0a0614);
const WIN_COLOR: Color = Color::from_hex(0x46d16b);
const OK_COLOR: Color = Color::from_hex(0xf2c94c);
const BAD_COLOR: Color = Color::from_hex(0xf0484a);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Coins,
    Pike,
    RubberDuck,
    Ghost,
    IceCream,
    Cake,
    Rocket,
    Rainbow,
    Bomb,
    Ducklings,
    Treasure,
    Storm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rating {
    Win,
    Ok,
    Bad,
}

impl Rating {
    fn color(self) -> Color {
        match self {
            Rating::Win => WIN_COLOR,
            Rating::Ok => OK_COLOR,
            Rating::Bad => BAD_COLOR,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // used by the render test
    fn sound(self) -> &'static str {
        match self {
            Rating::Win => "win",
            Rating::Ok => "ok",
            Rating::Bad => "bad",
        }
    }
}

struct Round {
    question: &'static str,
    doors: [(Item, Rating, &'static str); 3],
}

const ROUNDS: [Round; 4] = [
    Round {
        question: "FIND THE\nTREASURE",
        doors: [
            (Item::Pike, Rating::Bad, "CHOMP!"),
            (Item::Coins, Rating::Win, "RICH!"),
            (Item::RubberDuck, Rating::Ok, "SQUEAK"),
        ],
    },
    Round {
        question: "PICK YOUR\nDESSERT",
        doors: [
            (Item::IceCream, Rating::Ok, "YUM"),
            (Item::Ghost, Rating::Bad, "BOO!"),
            (Item::Cake, Rating::Win, "CAKE!"),
        ],
    },
    Round {
        question: "WHERE ARE\nYOU GOING?",
        doors: [
            (Item::Rocket, Rating::Win, "SPACE!"),
            (Item::Bomb, Rating::Bad, "BOOM!"),
            (Item::Rainbow, Rating::Ok, "PRETTY"),
        ],
    },
    Round {
        question: "LAST ROUND!\nONE DOOR HAS\nA MILLION",
        doors: [
            (Item::Ducklings, Rating::Ok, "AWW"),
            (Item::Storm, Rating::Bad, "ZAP!"),
            (Item::Treasure, Rating::Win, "MILLION"),
        ],
    },
];

/// Every sound in the video and the second it starts, built from the same
/// timeline the pictures use.
#[cfg_attr(not(test), allow(dead_code))] // used by the render test
fn cues() -> Vec<(f32, &'static str)> {
    let mut cues = vec![
        (0.0, "pop"),
        (0.35, "pop"),
        (0.7, "pop"),
        (1.0, "title"),
        (1.0, "music"),
    ];
    for (i, round) in ROUNDS.iter().enumerate() {
        let b = INTRO + i as f32 * ROUND_LEN;
        cues.push((b + 0.05, "whoosh"));
        for k in 0..5 {
            cues.push((
                b + COUNT_START + k as f32,
                if k == 4 { "tick_last" } else { "tick" },
            ));
        }
        cues.push((b + COUNT_END, "drumroll"));
        for (d, &(_, rating, _)) in round.doors.iter().enumerate() {
            cues.push((b + OPEN_AT[d], "creak"));
            cues.push((b + OPEN_AT[d] + 0.25, rating.sound()));
        }
        cues.push((b + WINNER_AT, "sting"));
    }
    cues.push((END_START, "whoosh"));
    for k in 0..4 {
        cues.push((END_START + 0.6 + 0.45 * k as f32, "pop"));
    }
    cues.push((END_START + 2.6, "fanfare"));
    cues.push((TOTAL, "end"));
    cues
}

/// The whole video is a function of time: nothing to remember but the clock.
struct Short {
    t: f32,
}

impl Game for Short {
    fn update(&mut self, ctx: &mut Context) {
        self.t += ctx.dt();
        if self.t > TOTAL + 1.0 || ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        let t = self.t;
        draw_background(canvas, t);
        if t < INTRO {
            draw_intro(canvas, t);
        } else if t < END_START {
            let i = ((t - INTRO) / ROUND_LEN) as usize;
            draw_round(canvas, i, t - INTRO - i as f32 * ROUND_LEN, t);
        } else {
            draw_end(canvas, t - END_START, t);
        }
    }
}

// ---- Scenes ----

fn draw_background(canvas: &mut Canvas, t: f32) {
    for y in 0..HEIGHT as i32 {
        let k = y as f32 / HEIGHT;
        canvas.fill_rect(
            Rect::new(0.0, y as f32, WIDTH, 1.0),
            SKY_TOP.lerp(SKY_BOTTOM, k),
        );
    }
    // Twinkling stars, always in the same places.
    let mut rng = Rng::new(99);
    for i in 0..45 {
        let p = vec2(rng.range(0.0, WIDTH), rng.range(0.0, HEIGHT * 0.75));
        let twinkle = (t * 2.0 + i as f32 * 1.7).sin();
        if twinkle > 0.2 {
            let c = SKY_TOP.lerp(Color::WHITE, 0.4 + twinkle * 0.5);
            canvas.fill_rect(Rect::from_center(p, 1.0, 1.0), c);
            if twinkle > 0.9 {
                canvas.fill_rect(Rect::from_center(p, 3.0, 1.0), c);
                canvas.fill_rect(Rect::from_center(p, 1.0, 3.0), c);
            }
        }
    }
    // The stage floor.
    let floor_y = DOOR_TOP + DOOR_H;
    canvas.fill_rect(Rect::new(0.0, floor_y, WIDTH, HEIGHT - floor_y), FLOOR);
    canvas.fill_rect(
        Rect::new(0.0, floor_y, WIDTH, 2.0),
        Color::from_hex(0x3a2d6b),
    );
    for k in 0..9 {
        let x = -40.0 + k as f32 * 44.0;
        canvas.draw_line(
            vec2(WIDTH / 2.0 + (x - WIDTH / 2.0) * 0.35, floor_y + 2.0),
            vec2(x, HEIGHT),
            Color::from_hex(0x221a40),
        );
    }
}

fn draw_intro(canvas: &mut Canvas, t: f32) {
    let words = [
        ("CHOOSE", Color::WHITE),
        ("YOUR", GOLD),
        ("DOOR", Color::WHITE),
    ];
    for (i, (word, color)) in words.iter().enumerate() {
        let k = pop(t - 0.35 * i as f32);
        if k > 0.0 {
            let scale = (6.0 * k).round().max(1.0) as u32;
            let y = 26.0 + i as f32 * 38.0 + (6.0 - scale as f32) * 2.5;
            big_text(canvas, word, y, scale, *color);
        }
    }
    // Three closed doors slide up into place.
    let rise = ease_out((t - 0.9) / 0.6);
    for d in 0..3 {
        let offset = vec2(0.0, (1.0 - rise) * 260.0 + d as f32 * 20.0 * (1.0 - rise));
        draw_door(canvas, d, 0.0, offset, None);
    }
    if t > 1.6 {
        let text = "4 ROUNDS - KEEP SCORE!";
        if (t * 4.0).sin() > -0.5 {
            big_text(canvas, text, 320.0, 2, GOLD);
        }
        big_text(canvas, "COMMENT YOUR PICKS", 350.0, 2, Color::WHITE);
    }
}

fn draw_round(canvas: &mut Canvas, i: usize, rt: f32, t: f32) {
    let round = &ROUNDS[i];
    let winner = round
        .doors
        .iter()
        .position(|d| d.1 == Rating::Win)
        .unwrap_or(0);

    // Header slides down at the start of the round.
    let drop = ease_out(rt / 0.5);
    let top = -40.0 + 70.0 * drop;
    big_text(
        canvas,
        &format!("ROUND {} OF 4", i + 1),
        top - 12.0,
        2,
        GOLD,
    );
    let lines = round.question.lines().count() as f32;
    let scale = if lines > 2.0 { 3 } else { 4 };
    let line_h = 7.0 * scale as f32;
    let q_top = 52.0 + (3.0 - lines) * line_h * 0.5 + (1.0 - drop) * -120.0;
    for (n, line) in round.question.lines().enumerate() {
        big_text(canvas, line, q_top + n as f32 * line_h, scale, Color::WHITE);
    }

    // The doors drop in from above, one after another.
    for d in 0..3 {
        let fall = ease_out_back((rt - 0.15 * d as f32) / 0.55);
        let mut offset = vec2(0.0, (1.0 - fall) * -320.0);
        // A nervous wiggle in the last second of the countdown and just before opening.
        let before_open = OPEN_AT[d] - rt;
        if (COUNT_END - 1.0..COUNT_END).contains(&rt) || (0.0..0.25).contains(&before_open) {
            offset.x += ((t * 60.0 + d as f32).sin() * 1.6).round();
        }
        let open = ease_in_out((rt - OPEN_AT[d]) / OPEN_TIME);
        let content = round.doors[d];
        draw_door(
            canvas,
            d,
            open,
            offset,
            Some((content.0, content.1, rt - OPEN_AT[d], t)),
        );

        // Under an open door: a badge and what it was.
        let shown = rt - OPEN_AT[d] - 0.25;
        if shown > 0.0 {
            let k = pop(shown);
            let center = door_rect(d).center() + vec2(0.0, DOOR_H / 2.0 + 18.0);
            badge(canvas, center, 9.0 * k, content.1);
            let label_scale = if content.2.len() > 8 { 1 } else { 2 };
            let w = Canvas::text_width(content.2, label_scale);
            let pos = vec2((center.x - w / 2.0).round(), center.y + 15.0);
            if k > 0.6 {
                outlined_text(canvas, content.2, pos, label_scale, content.1.color());
            }
        }
    }

    // The countdown.
    if (COUNT_START..COUNT_END).contains(&rt) {
        let left = COUNT_END - rt;
        let number = left.ceil() as u32;
        let within = left - left.floor();
        let center = vec2(WIDTH / 2.0, 384.0);
        let urgent = number <= 2;
        let ring_color = if urgent { BAD_COLOR } else { GOLD };
        canvas.fill_circle(center, 34.0, SHADOW);
        canvas.fill_circle(center, 31.0, Color::from_hex(0x2b2155));
        arc(canvas, center, 30.0, within, ring_color, 4.0);
        let scale = (6.0 + (1.0 - within).min(0.15) * -10.0 + 1.5).round() as u32;
        let text = number.to_string();
        let w = Canvas::text_width(&text, scale);
        outlined_text(
            canvas,
            &text,
            vec2(center.x - w / 2.0, center.y - 2.5 * scale as f32),
            scale,
            Color::WHITE,
        );
        if (t * 3.0).sin() > -0.6 {
            big_text(canvas, "PICK A DOOR!", 432.0, 2, Color::WHITE);
        }
    } else if (COUNT_END..OPEN_AT[0]).contains(&rt) {
        let k = pop(rt - COUNT_END);
        let scale = (6.0 * k).round().max(1.0) as u32;
        big_text(canvas, "OPEN!", 370.0, scale, GOLD);
    }

    // The winner.
    if rt >= WINNER_AT {
        let pulse = (t * 8.0).sin() * 0.5 + 0.5;
        let r = door_rect(winner);
        for g in 0..3 {
            let grow = 4.0 + g as f32 * 2.0 + pulse * 2.0;
            let rect = Rect::new(r.x - grow, r.y - grow, r.w + 2.0 * grow, r.h + 2.0 * grow);
            canvas.draw_rect(
                rect,
                GOLD.lerp(Color::WHITE, pulse * (1.0 - g as f32 * 0.3)),
            );
        }
        let k = pop(rt - WINNER_AT);
        let scale = (3.0 * k).round().max(1.0) as u32;
        big_text(
            canvas,
            &format!("DOOR {} WINS!", winner + 1),
            400.0,
            scale,
            GOLD,
        );
        sparkles(canvas, r.center(), t, 50.0);
    }
}

fn draw_end(canvas: &mut Canvas, et: f32, t: f32) {
    big_text(
        canvas,
        "HOW DID",
        50.0 + (1.0 - ease_out(et / 0.4)) * -100.0,
        5,
        Color::WHITE,
    );
    big_text(
        canvas,
        "YOU DO?",
        90.0 + (1.0 - ease_out(et / 0.4)) * -100.0,
        5,
        GOLD,
    );
    let rows = [
        ("4 WINS", "LEGEND", WIN_COLOR),
        ("3 WINS", "LUCKY", Color::from_hex(0x8be08f)),
        ("2 WINS", "NOT BAD", OK_COLOR),
        ("0-1 WINS", "UNLUCKY", BAD_COLOR),
    ];
    for (i, (wins, title, color)) in rows.iter().enumerate() {
        let k = pop(et - 0.6 - 0.45 * i as f32);
        if k <= 0.0 {
            continue;
        }
        let y = 160.0 + i as f32 * 34.0;
        let slide = (1.0 - k) * 60.0;
        let panel = Rect::new(20.0 + slide, y, WIDTH - 40.0, 26.0);
        canvas.fill_rect(
            Rect::new(panel.x + 2.0, panel.y + 2.0, panel.w, panel.h),
            SHADOW,
        );
        canvas.fill_rect(panel, Color::from_hex(0x2b2155));
        canvas.fill_rect(Rect::new(panel.x, panel.y, 4.0, panel.h), *color);
        outlined_text(canvas, wins, vec2(panel.x + 12.0, y + 8.0), 2, Color::WHITE);
        let w = Canvas::text_width(title, 2);
        outlined_text(
            canvas,
            title,
            vec2(panel.right() - 10.0 - w, y + 8.0),
            2,
            *color,
        );
    }
    if et > 2.6 {
        let k = pop(et - 2.6);
        let scale = (3.0 * k).round().max(1.0) as u32;
        big_text(canvas, "COMMENT", 320.0, scale, Color::WHITE);
        big_text(canvas, "YOUR SCORE!", 346.0, scale, GOLD);
        // A few small trophies of confetti.
        let mut rng = Rng::new(5);
        for i in 0..40 {
            let x = rng.range(0.0, WIDTH);
            let speed = rng.range(40.0, 90.0);
            let y = (rng.range(0.0, HEIGHT) + (et - 2.6) * speed) % HEIGHT;
            let c = [
                GOLD,
                WIN_COLOR,
                BAD_COLOR,
                Color::from_hex(0x6fb3ff),
                Color::WHITE,
            ][i % 5];
            let wob = ((t * 5.0 + i as f32).sin() * 2.0).round();
            canvas.fill_rect(Rect::new(x + wob, y, 2.0, 3.0), c);
        }
    }
    if et > 3.4 && (t * 3.0).sin() > -0.4 {
        big_text(
            canvas,
            "MADE WITH DUCKFORGE",
            440.0,
            1,
            Color::from_hex(0x9b8fd0),
        );
    }
}

// ---- Doors ----

fn door_rect(d: usize) -> Rect {
    let left = (WIDTH - 3.0 * DOOR_W - 2.0 * DOOR_GAP) / 2.0;
    Rect::new(
        left + d as f32 * (DOOR_W + DOOR_GAP),
        DOOR_TOP,
        DOOR_W,
        DOOR_H,
    )
}

/// Draws door `d` opened by `open` (0 = shut, 1 = wide open), with whatever is behind it.
fn draw_door(
    canvas: &mut Canvas,
    d: usize,
    open: f32,
    offset: Vec2,
    content: Option<(Item, Rating, f32, f32)>,
) {
    let r = moved(door_rect(d), offset);
    // The frame and the number above.
    canvas.fill_rect(
        Rect::new(r.x - 5.0, r.y - 5.0, r.w + 10.0, r.h + 5.0),
        SHADOW,
    );
    canvas.fill_rect(Rect::new(r.x - 4.0, r.y - 4.0, r.w + 8.0, r.h + 4.0), FRAME);
    canvas.fill_rect(
        Rect::new(r.x - 4.0, r.y - 4.0, r.w + 8.0, 2.0),
        Color::from_hex(0x7a5530),
    );
    let number = (d + 1).to_string();
    outlined_text(
        canvas,
        &number,
        vec2(r.center().x - 4.5, r.y - 24.0),
        3,
        Color::WHITE,
    );

    // Behind the door: a dark room with a glow the color of the result.
    canvas.fill_rect(r, INSIDE);
    if let Some((item, rating, since, t)) = content {
        if open > 0.0 {
            let glow = rating.color();
            for (k, radius) in [30.0, 24.0, 17.0].into_iter().enumerate() {
                canvas.fill_circle(r.center(), radius, INSIDE.lerp(glow, 0.12 + k as f32 * 0.1));
            }
            let s = ease_out_back((since - 0.1) / 0.4).max(0.0);
            if s > 0.0 {
                draw_item(canvas, item, r.center(), s, t);
            }
        }
    }

    // The door itself, swinging open on its left hinge.
    let width = r.w * (open * PI / 2.0).cos();
    if width > 0.8 {
        let color = DOOR_COLORS[d];
        let dark = color.lerp(Color::BLACK, 0.3 + open * 0.3);
        let face = color.lerp(Color::BLACK, open * 0.35);
        let door = Rect::new(r.x, r.y, width, r.h);
        canvas.fill_rect(door, face);
        let k = width / r.w;
        for (y, h) in [(10.0, 40.0), (60.0, 42.0)] {
            let panel = Rect::new(r.x + 9.0 * k, r.y + y, (r.w - 18.0) * k, h);
            canvas.fill_rect(panel, dark);
            canvas.fill_rect(
                Rect::new(
                    panel.x + 2.0 * k,
                    panel.y + 2.0,
                    (panel.w - 4.0 * k).max(0.0),
                    h - 4.0,
                ),
                face.lerp(Color::WHITE, 0.08),
            );
        }
        canvas.fill_circle(
            vec2(r.x + width - 9.0 * k, r.y + 58.0),
            3.2 * k.max(0.4),
            GOLD,
        );
        canvas.fill_rect(Rect::new(r.x + width - 1.0, r.y, 1.0, r.h), dark);
        // A shine on a shut door.
        if open == 0.0 {
            canvas.draw_line(
                vec2(r.x + 4.0, r.y + 4.0),
                vec2(r.x + 4.0, r.bottom() - 4.0),
                color.lerp(Color::WHITE, 0.3),
            );
        }
    }
}

fn badge(canvas: &mut Canvas, c: Vec2, r: f32, rating: Rating) {
    if r < 1.0 {
        return;
    }
    canvas.fill_circle(c, r + 1.5, SHADOW);
    canvas.fill_circle(c, r, rating.color());
    let k = r / 9.0;
    match rating {
        Rating::Win => {
            thick_line(
                canvas,
                c + vec2(-4.0, 0.0) * k,
                c + vec2(-1.0, 3.5) * k,
                Color::WHITE,
            );
            thick_line(
                canvas,
                c + vec2(-1.0, 3.5) * k,
                c + vec2(4.5, -3.5) * k,
                Color::WHITE,
            );
        }
        Rating::Bad => {
            thick_line(
                canvas,
                c + vec2(-3.5, -3.5) * k,
                c + vec2(3.5, 3.5) * k,
                Color::WHITE,
            );
            thick_line(
                canvas,
                c + vec2(3.5, -3.5) * k,
                c + vec2(-3.5, 3.5) * k,
                Color::WHITE,
            );
        }
        Rating::Ok => {
            thick_line(
                canvas,
                c + vec2(-4.0, 0.0) * k,
                c + vec2(4.0, 0.0) * k,
                Color::WHITE,
            );
        }
    }
}

// ---- What's behind the doors ----

fn draw_item(canvas: &mut Canvas, item: Item, c: Vec2, s: f32, t: f32) {
    let p = |x: f32, y: f32| c + vec2(x, y) * s;
    let circle = |canvas: &mut Canvas, x: f32, y: f32, r: f32, color: Color| {
        canvas.fill_circle(c + vec2(x, y) * s, r * s, color)
    };
    match item {
        Item::Coins => {
            let pile = [
                (-14.0, 18.0),
                (0.0, 18.0),
                (14.0, 18.0),
                (-7.0, 8.0),
                (7.0, 8.0),
                (0.0, -2.0),
                (-20.0, 26.0),
                (20.0, 26.0),
                (0.0, 28.0),
            ];
            for &(x, y) in &pile {
                coin(canvas, p(x, y), 7.0 * s);
            }
            sparkles(canvas, c, t, 26.0 * s);
        }
        Item::Treasure => {
            // An open chest overflowing with gold.
            canvas.fill_rect(
                Rect::from_center(p(0.0, 18.0), 50.0 * s, 26.0 * s),
                Color::from_hex(0x7a4a22),
            );
            canvas.fill_rect(
                Rect::from_center(p(0.0, 18.0), 50.0 * s, 4.0 * s),
                GOLD_DARK,
            );
            canvas.fill_rect(
                Rect::from_center(p(0.0, -10.0), 50.0 * s, 12.0 * s),
                Color::from_hex(0x8d5a2b),
            );
            for &(x, y) in &[
                (-16.0, 4.0),
                (-5.0, 2.0),
                (6.0, 3.0),
                (17.0, 5.0),
                (-10.0, -4.0),
                (1.0, -5.0),
                (12.0, -3.0),
            ] {
                coin(canvas, p(x, y), 6.0 * s);
            }
            canvas.fill_rect(Rect::from_center(p(0.0, 18.0), 8.0 * s, 8.0 * s), GOLD);
            sparkles(canvas, c, t, 32.0 * s);
        }
        Item::Pike => {
            let body = Color::from_hex(0x4d6b3f);
            let belly = Color::from_hex(0x9bb36c);
            canvas.fill_triangle(
                p(16.0, 0.0),
                p(30.0, -10.0),
                p(30.0, 10.0),
                Color::from_hex(0x2f4527),
            );
            for (i, r) in [7.0, 9.0, 10.0, 10.0, 9.0, 7.0].into_iter().enumerate() {
                let x = -16.0 + i as f32 * 6.5;
                circle(canvas, x, 0.0, r + 1.0, Color::from_hex(0x23331d));
            }
            for (i, r) in [7.0, 9.0, 10.0, 10.0, 9.0, 7.0].into_iter().enumerate() {
                let x = -16.0 + i as f32 * 6.5;
                circle(canvas, x, 0.0, r, body);
                circle(canvas, x, 4.0, r * 0.45, belly);
            }
            // A wide open mouth full of teeth, snapping.
            let snap = ((t * 10.0).sin() * 0.5 + 0.5) * 4.0;
            canvas.fill_triangle(
                p(-24.0, -6.0 - snap),
                p(-24.0, 6.0 + snap),
                p(-10.0, 0.0),
                INSIDE,
            );
            for k in 0..3 {
                let x = -22.0 + k as f32 * 4.0;
                canvas.fill_triangle(
                    p(x, -5.0 - snap * 0.8),
                    p(x + 2.0, -5.0 - snap * 0.8),
                    p(x + 1.0, -1.0),
                    Color::WHITE,
                );
                canvas.fill_triangle(
                    p(x, 5.0 + snap * 0.8),
                    p(x + 2.0, 5.0 + snap * 0.8),
                    p(x + 1.0, 1.0),
                    Color::WHITE,
                );
            }
            circle(canvas, -9.0, -5.0, 2.5, Color::from_hex(0xf4e27a));
            circle(canvas, -9.0, -5.0, 1.2, Color::BLACK);
        }
        Item::RubberDuck => {
            let bob = (t * 4.0).sin() * 2.0;
            let c2 = p(0.0, bob);
            canvas.fill_circle(c2 + vec2(2.0, 8.0) * s, 15.0 * s, Color::from_hex(0xd9a21a));
            canvas.fill_circle(c2 + vec2(2.0, 8.0) * s, 14.0 * s, GOLD);
            canvas.fill_circle(
                c2 + vec2(-7.0, -10.0) * s,
                10.0 * s,
                Color::from_hex(0xd9a21a),
            );
            canvas.fill_circle(c2 + vec2(-7.0, -10.0) * s, 9.0 * s, GOLD);
            canvas.fill_circle(c2 + vec2(6.0, 6.0) * s, 7.0 * s, Color::from_hex(0xf0b72a));
            canvas.fill_triangle(
                c2 + vec2(-15.0, -12.0) * s,
                c2 + vec2(-15.0, -6.0) * s,
                c2 + vec2(-24.0, -8.0) * s,
                Color::from_hex(0xf07a2a),
            );
            canvas.fill_circle(c2 + vec2(-9.0, -13.0) * s, 1.8 * s, Color::BLACK);
        }
        Item::Ghost => {
            let bob = (t * 3.0).sin() * 3.0;
            let g = |x: f32, y: f32| c + vec2(x, y + bob) * s;
            canvas.fill_circle(g(0.0, -8.0), 17.0 * s, Color::from_hex(0xdfe6ff));
            canvas.fill_circle(g(0.0, -8.0), 16.0 * s, Color::WHITE);
            canvas.fill_rect(
                Rect::new(g(-16.0, -8.0).x, g(0.0, -8.0).y, 32.0 * s, 20.0 * s),
                Color::WHITE,
            );
            for k in 0..4 {
                let wave = (t * 6.0 + k as f32 * 1.5).sin() * 1.5;
                canvas.fill_circle(
                    g(-12.0 + k as f32 * 8.0, 12.0 + wave),
                    4.2 * s,
                    Color::WHITE,
                );
            }
            canvas.fill_rect(
                Rect::from_center(g(-6.0, -10.0), 4.0 * s, 7.0 * s),
                Color::BLACK,
            );
            canvas.fill_rect(
                Rect::from_center(g(6.0, -10.0), 4.0 * s, 7.0 * s),
                Color::BLACK,
            );
            canvas.fill_circle(g(0.0, 1.0), 4.0 * s, Color::BLACK);
        }
        Item::IceCream => {
            canvas.fill_triangle(
                p(-11.0, 2.0),
                p(11.0, 2.0),
                p(0.0, 34.0),
                Color::from_hex(0xd39a5a),
            );
            for k in 0..3 {
                let y = 8.0 + k as f32 * 8.0;
                canvas.draw_line(
                    p(-9.0 + k as f32 * 3.0, y),
                    p(9.0 - k as f32 * 3.0, y),
                    Color::from_hex(0xa8743c),
                );
            }
            circle(canvas, 0.0, -2.0, 12.0, Color::from_hex(0xf59ac0));
            circle(canvas, 0.0, -18.0, 10.0, Color::from_hex(0x9fe0c5));
            circle(canvas, 4.0, -21.0, 3.0, Color::from_hex(0xc8f3e2));
            circle(canvas, 0.0, -30.0, 3.5, Color::from_hex(0xe0303a));
        }
        Item::Cake => {
            canvas.fill_rect(
                Rect::from_center(p(0.0, 30.0), 54.0 * s, 4.0 * s),
                Color::from_hex(0xcfd6e0),
            );
            canvas.fill_rect(
                Rect::from_center(p(0.0, 19.0), 46.0 * s, 18.0 * s),
                Color::from_hex(0x8a4b2b),
            );
            canvas.fill_rect(
                Rect::from_center(p(0.0, 12.0), 46.0 * s, 5.0 * s),
                Color::from_hex(0xf7a8c8),
            );
            canvas.fill_rect(
                Rect::from_center(p(0.0, 0.0), 32.0 * s, 16.0 * s),
                Color::from_hex(0x9a5733),
            );
            canvas.fill_rect(
                Rect::from_center(p(0.0, -6.0), 32.0 * s, 5.0 * s),
                Color::from_hex(0xf7a8c8),
            );
            for k in 0..5 {
                circle(
                    canvas,
                    -18.0 + k as f32 * 9.0,
                    15.0,
                    2.2,
                    Color::from_hex(0xf7a8c8),
                );
            }
            for k in 0..4 {
                circle(
                    canvas,
                    -12.0 + k as f32 * 8.0,
                    -3.0,
                    2.0,
                    Color::from_hex(0xf7a8c8),
                );
            }
            canvas.fill_rect(
                Rect::from_center(p(0.0, -14.0), 3.0 * s, 10.0 * s),
                Color::from_hex(0x7ec8ff),
            );
            let flicker = (t * 20.0).sin() * 0.6;
            circle(canvas, flicker * 0.5, -22.0, 3.2, Color::from_hex(0xff9a2a));
            circle(canvas, flicker * 0.5, -21.5, 1.6, Color::from_hex(0xfff07a));
        }
        Item::Rocket => {
            let shake = (t * 40.0).sin() * 0.8;
            let r = |x: f32, y: f32| c + vec2(x + shake, y - 4.0) * s;
            let flame = 8.0 + (t * 30.0).sin() * 3.0;
            canvas.fill_triangle(
                r(-6.0, 18.0),
                r(6.0, 18.0),
                r(0.0, 18.0 + flame * 1.6),
                Color::from_hex(0xff8a2a),
            );
            canvas.fill_triangle(
                r(-3.5, 18.0),
                r(3.5, 18.0),
                r(0.0, 18.0 + flame),
                Color::from_hex(0xfff07a),
            );
            canvas.fill_triangle(
                r(-8.0, 8.0),
                r(-8.0, 20.0),
                r(-16.0, 22.0),
                Color::from_hex(0xe0303a),
            );
            canvas.fill_triangle(
                r(8.0, 8.0),
                r(8.0, 20.0),
                r(16.0, 22.0),
                Color::from_hex(0xe0303a),
            );
            canvas.fill_rect(
                Rect::new(r(-8.0, -14.0).x, r(-8.0, -14.0).y, 16.0 * s, 34.0 * s),
                Color::from_hex(0xeef1f6),
            );
            canvas.fill_triangle(
                r(-8.0, -14.0),
                r(8.0, -14.0),
                r(0.0, -30.0),
                Color::from_hex(0xe0303a),
            );
            canvas.fill_circle(r(0.0, -3.0), 4.5 * s, Color::from_hex(0x5a6a7a));
            canvas.fill_circle(r(0.0, -3.0), 3.5 * s, Color::from_hex(0x6fc3ff));
        }
        Item::Rainbow => {
            let colors = [0xe0303a, 0xff8a2a, 0xffd23a, 0x46d16b, 0x3d9bff, 0x7a5cff];
            for (k, &hex) in colors.iter().enumerate() {
                let radius = 26.0 - k as f32 * 3.2;
                for step in 0..=36 {
                    let a = PI + step as f32 * PI / 36.0;
                    canvas.fill_circle(
                        p(0.0, 12.0) + vec2(a.cos(), a.sin()) * radius * s,
                        1.9 * s,
                        Color::from_hex(hex),
                    );
                }
            }
            for x in [-22.0, 22.0] {
                circle(canvas, x - 5.0, 14.0, 5.0, Color::WHITE);
                circle(canvas, x + 4.0, 13.0, 6.0, Color::WHITE);
                circle(canvas, x, 9.0, 5.0, Color::WHITE);
            }
        }
        Item::Bomb => {
            circle(canvas, 0.0, 6.0, 16.0, Color::from_hex(0x2a2a33));
            circle(canvas, -5.0, 0.0, 4.0, Color::from_hex(0x5a5a68));
            canvas.fill_rect(
                Rect::from_center(p(8.0, -10.0), 8.0 * s, 6.0 * s),
                Color::from_hex(0x5a5a68),
            );
            canvas.draw_line(p(10.0, -13.0), p(14.0, -20.0), Color::from_hex(0xc9a36a));
            canvas.draw_line(p(14.0, -20.0), p(19.0, -22.0), Color::from_hex(0xc9a36a));
            let spark = p(19.0, -23.0);
            let size = 3.0 + (t * 25.0).sin() * 1.5;
            for a in 0..6 {
                let ang = a as f32 * TAU / 6.0 + t * 8.0;
                canvas.draw_line(
                    spark,
                    spark + vec2(ang.cos(), ang.sin()) * size * s,
                    if a % 2 == 0 {
                        GOLD
                    } else {
                        Color::from_hex(0xff6a2a)
                    },
                );
            }
        }
        Item::Ducklings => {
            for k in 0..3 {
                let x = -18.0 + k as f32 * 18.0;
                let bob = (t * 6.0 + k as f32).sin() * 2.0;
                let d = |dx: f32, dy: f32| c + vec2(x + dx, 10.0 + bob + dy) * s;
                canvas.fill_circle(d(1.0, 2.0), 7.0 * s, Color::from_hex(0xd9a21a));
                canvas.fill_circle(d(1.0, 2.0), 6.2 * s, Color::from_hex(0xffde5a));
                canvas.fill_circle(d(-3.0, -6.0), 4.6 * s, Color::from_hex(0xffde5a));
                canvas.fill_triangle(
                    d(-7.0, -7.0),
                    d(-7.0, -4.0),
                    d(-11.0, -5.5),
                    Color::from_hex(0xf07a2a),
                );
                canvas.fill_rect(
                    Rect::from_center(d(-4.0, -7.0), 1.5 * s, 1.5 * s),
                    Color::BLACK,
                );
            }
            let heart_y = -18.0 + (t * 2.0).sin() * 2.0;
            circle(canvas, -3.0, heart_y, 3.5, BAD_COLOR);
            circle(canvas, 3.0, heart_y, 3.5, BAD_COLOR);
            canvas.fill_triangle(
                p(-6.4, heart_y + 1.0),
                p(6.4, heart_y + 1.0),
                p(0.0, heart_y + 8.0),
                BAD_COLOR,
            );
        }
        Item::Storm => {
            let flash = (t * 7.0).sin() > 0.2;
            for &(x, y, r) in &[
                (-12.0, -10.0, 10.0),
                (2.0, -16.0, 12.0),
                (14.0, -9.0, 9.0),
                (0.0, -6.0, 11.0),
            ] {
                circle(canvas, x, y, r, Color::from_hex(0x6a7186));
            }
            if flash {
                canvas.fill_triangle(
                    p(2.0, -2.0),
                    p(-8.0, 14.0),
                    p(0.0, 14.0),
                    Color::from_hex(0xfff07a),
                );
                canvas.fill_triangle(
                    p(-2.0, 12.0),
                    p(6.0, 12.0),
                    p(-4.0, 30.0),
                    Color::from_hex(0xfff07a),
                );
            }
            for k in 0..5 {
                let x = -18.0 + k as f32 * 9.0;
                let y = 8.0 + ((t * 60.0 + k as f32 * 7.0) % 20.0);
                canvas.draw_line(p(x, y), p(x - 1.5, y + 4.0), Color::from_hex(0x6fb3ff));
            }
        }
    }
}

fn coin(canvas: &mut Canvas, c: Vec2, r: f32) {
    canvas.fill_circle(c, r + 0.8, GOLD_DARK);
    canvas.fill_circle(c, r, GOLD);
    canvas.fill_circle(c, r * 0.55, Color::from_hex(0xffe27a));
    canvas.fill_rect(
        Rect::from_center(c - vec2(r * 0.35, r * 0.35), 1.5, 1.5),
        Color::WHITE,
    );
}

fn sparkles(canvas: &mut Canvas, c: Vec2, t: f32, spread: f32) {
    let mut rng = Rng::new(17);
    for i in 0..8 {
        let p = c + vec2(rng.range(-spread, spread), rng.range(-spread, spread));
        let k = (t * 4.0 + i as f32 * 0.9).sin();
        if k > 0.3 {
            let size = 1.0 + (k - 0.3) * 4.0;
            canvas.draw_line(p - vec2(size, 0.0), p + vec2(size, 0.0), Color::WHITE);
            canvas.draw_line(p - vec2(0.0, size), p + vec2(0.0, size), Color::WHITE);
        }
    }
}

// ---- Small helpers ----

/// Centered text with a dark outline, so it reads on any background.
fn big_text(canvas: &mut Canvas, text: &str, y: f32, scale: u32, color: Color) {
    let x = ((WIDTH - Canvas::text_width(text, scale)) / 2.0).round();
    outlined_text(canvas, text, vec2(x, y.round()), scale, color);
}

fn outlined_text(canvas: &mut Canvas, text: &str, pos: Vec2, scale: u32, color: Color) {
    let s = (scale as f32 / 2.0).ceil().max(1.0);
    for (dx, dy) in [
        (-1.0, 0.0),
        (1.0, 0.0),
        (0.0, -1.0),
        (0.0, 1.0),
        (1.0, 1.0),
        (2.0, 2.0),
    ] {
        canvas.draw_text(text, pos + vec2(dx * s, dy * s), scale, SHADOW);
    }
    canvas.draw_text(text, pos, scale, color);
}

/// A thick ring that is `fraction` of the way round, starting at the top.
fn arc(canvas: &mut Canvas, c: Vec2, r: f32, fraction: f32, color: Color, thickness: f32) {
    let steps = (60.0 * fraction).ceil() as i32;
    for i in 0..=steps {
        let a = -PI / 2.0 + i as f32 / 60.0 * TAU;
        canvas.fill_circle(c + vec2(a.cos(), a.sin()) * r, thickness / 2.0, color);
    }
}

fn thick_line(canvas: &mut Canvas, a: Vec2, b: Vec2, color: Color) {
    for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
        canvas.draw_line(a + vec2(dx, dy), b + vec2(dx, dy), color);
    }
}

fn moved(r: Rect, by: Vec2) -> Rect {
    Rect::new(r.x + by.x, r.y + by.y, r.w, r.h)
}

fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Overshoots a little and settles, like something landing with a bounce.
fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c = 1.7;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}

/// 0 before `x` reaches 0, then a quick bouncy grow to 1.
fn pop(x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else {
        ease_out_back(x / 0.3)
    }
}

fn config() -> Config {
    Config {
        title: String::from("Choose Your Door - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        scale: 2,
        timestep: Timestep::Fixed {
            updates_per_second: FPS,
        },
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Short { t: 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_round_has_one_win_one_ok_and_one_bad() {
        for round in &ROUNDS {
            for rating in [Rating::Win, Rating::Ok, Rating::Bad] {
                assert_eq!(round.doors.iter().filter(|d| d.1 == rating).count(), 1);
            }
        }
    }

    #[test]
    fn the_video_is_one_minute_and_the_cues_are_in_order_inside_it() {
        assert_eq!(END_START + 5.0, TOTAL);
        let cues = cues();
        assert!(cues.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(cues.iter().all(|c| (0.0..=TOTAL).contains(&c.0)));
    }

    /// Not a real test: renders every frame and the sound cues.
    #[test]
    #[ignore]
    fn render() {
        let dir = std::env::var("FRAMES").unwrap_or_else(|_| String::from("target/door_frames"));
        let every: u64 = std::env::var("EVERY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        std::fs::create_dir_all(&dir).unwrap();
        let mut sim = Simulation::new(config(), Short { t: 0.0 });
        let frames = (TOTAL * FPS as f32) as u64;
        for frame in 0..frames {
            if frame % every == 0 {
                sim.picture()
                    .save_png(format!("{dir}/{frame:05}.png"))
                    .unwrap();
            }
            sim.step();
        }
        let json: Vec<String> = cues()
            .iter()
            .map(|(t, name)| format!("  {{\"t\": {t:.3}, \"sound\": \"{name}\"}}"))
            .collect();
        std::fs::write(
            format!("{dir}/cues.json"),
            format!("[\n{}\n]\n", json.join(",\n")),
        )
        .unwrap();
    }
}
