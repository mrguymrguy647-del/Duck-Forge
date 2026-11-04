//! Pollen Run: fly a bee around a hedged garden, pick pollen from the
//! flowers (up to three at a time; each one slows you down) and carry it
//! home to the hive. Wasps chase you when you come close; a sting costs a
//! life and drops what you carry back on the flowers.
//!
//!     cargo run --example pollen_run
//!
//! Arrow keys (or WASD) fly, P pauses, Space starts again after the end,
//! Escape quits. F1 shows the debug overlay, F10 records what you do.
//!
//! Made from examples/starter_topdown, following skills/duckforge/SKILL.md.
//!
//!     tools/verify.sh pollen_run

use duckforge::prelude::*;

// ---- Settings ----
//
// The `ctx.tune` ones are in examples/pollen_run.tune.json.

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;
const TILE: f32 = 16.0;
const BODY: f32 = 12.0;
const BEE_SPEED: f32 = 95.0;
const CARRY_SLOWDOWN: f32 = 0.12; // each pollen carried takes 12% off the speed
const CARRY_MAX: usize = 3;
const WASP_SPEED: f32 = 42.0;
const SIGHT: f32 = 80.0;
const START_LIVES: u32 = 3;
const SAFE_TIME: f32 = 1.5;

/// The garden: '#' hedge, '.' grass, 'p' the bee, 'h' the hive, 'f' a
/// flower, 'w' a wasp.
const MAP: [&str; 26] = [
    "####################################",
    "#................w.................#",
    "#..f......f.............f.......f..#",
    "#..............######..............#",
    "#...w.........................w....#",
    "#.....####................####.....#",
    "#.....####.......f........####.....#",
    "#..................................#",
    "#.f..............................f.#",
    "#...........#..........#...........#",
    "#...........#..........#...........#",
    "#..##.......#..........#.......##..#",
    "#..##...f...#.f..h.....#...f...##..#",
    "#..##.......#.....p..f.#.......##..#",
    "#..##.......#..........#.......##..#",
    "#...........#..........#...........#",
    "#...........#..........#...........#",
    "#.f..............................f.#",
    "#..................................#",
    "#.....####........f.......####.....#",
    "#.....####................####.....#",
    "#....w.........................w...#",
    "#..............######..............#",
    "#..f......f.............f.......f..#",
    "#..................................#",
    "####################################",
];

/// The bee, wings up and wings down.
const BEE: [&str; 6] = [
    "..ww........",
    "..ww...wwww.",
    "ykykykykykyk",
    "ykykykykykyk",
    ".ykyk..ykyk.",
    "............",
];

/// A wasp, with a sting.
const WASP: [&str; 6] = [
    "..ww........",
    "..ww...wwww.",
    "rkrkrkrkrkrk",
    "rkrkrkrkrkrk",
    ".rkrk..rkrk.",
    "...k.....k..",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Playing,
    Paused,
    Won,
    Lost,
}

struct Wasp {
    body: Rect,
    wander: Vec2,
    wander_time: f32,
    chasing: bool,
}

struct PollenRun {
    state: State,
    hedges: Vec<Vec<bool>>,
    bee: Rect,
    facing_left: bool,
    hive: Rect,
    wasps: Vec<Wasp>,
    flowers: Vec<Vec2>,
    /// The flowers the bee is carrying pollen from: they grow back if stung.
    carried: Vec<Vec2>,
    total: usize,
    delivered: usize,
    lives: u32,
    safe_time: f32,
    time: f32,
    bee_anim: Animation,
    wasp_anim: Animation,
    camera: Camera2D,
}

impl PollenRun {
    fn new() -> PollenRun {
        let mut hedges = Vec::new();
        let mut bee = Rect::new(0.0, 0.0, 0.0, 0.0);
        let mut hive = bee;
        let (mut wasps, mut flowers) = (vec![], vec![]);
        for (ty, row) in MAP.iter().enumerate() {
            hedges.push(row.chars().map(|c| c == '#').collect());
            for (tx, c) in row.chars().enumerate() {
                let center = vec2((tx as f32 + 0.5) * TILE, (ty as f32 + 0.5) * TILE);
                let body = Rect::from_center(center, BODY, BODY);
                match c {
                    'p' => bee = body,
                    'h' => hive = Rect::from_center(center, TILE, TILE),
                    'f' => flowers.push(center),
                    'w' => wasps.push(Wasp {
                        body,
                        wander: Vec2::ZERO,
                        wander_time: 0.0,
                        chasing: false,
                    }),
                    _ => {}
                }
            }
        }
        let wings = Color::from_hex(0xd8f0ff);
        let bee_sprite = Sprite::from_ascii(
            &BEE,
            &[
                ('w', wings),
                ('y', Color::from_hex(0xffd23a)),
                ('k', Color::BLACK),
            ],
        );
        let wasp_sprite = Sprite::from_ascii(
            &WASP,
            &[
                ('w', wings),
                ('r', Color::from_hex(0xe0502a)),
                ('k', Color::BLACK),
            ],
        );
        let mut camera = Camera2D::new(WIDTH, HEIGHT);
        camera.position = bee.center();
        PollenRun {
            state: State::Playing,
            hedges,
            bee,
            facing_left: false,
            hive,
            wasps,
            total: flowers.len(),
            flowers,
            carried: Vec::new(),
            delivered: 0,
            lives: START_LIVES,
            safe_time: 0.0,
            time: 0.0,
            bee_anim: Animation::new(bee_sprite.frames(6, 6), 12.0),
            wasp_anim: Animation::new(wasp_sprite.frames(6, 6), 10.0),
            camera,
        }
    }

    fn garden(&self) -> Rect {
        Rect::new(
            0.0,
            0.0,
            MAP[0].len() as f32 * TILE,
            MAP.len() as f32 * TILE,
        )
    }

    /// A hedge tile that `rect` overlaps, if any. Outside the map counts as hedge.
    fn hedge_touching(&self, rect: Rect) -> Option<Rect> {
        let tiles =
            |from: f32, to: f32| (from / TILE).floor() as i32..=((to - 0.01) / TILE).floor() as i32;
        for ty in tiles(rect.top(), rect.bottom()) {
            for tx in tiles(rect.left(), rect.right()) {
                let hedge = ty < 0
                    || tx < 0
                    || self
                        .hedges
                        .get(ty as usize)
                        .and_then(|row| row.get(tx as usize))
                        .copied()
                        .unwrap_or(true);
                if hedge {
                    return Some(Rect::new(tx as f32 * TILE, ty as f32 * TILE, TILE, TILE));
                }
            }
        }
        None
    }

    /// Moves `body` by `step`, one axis at a time, sliding along hedges.
    fn slide(&self, mut body: Rect, step: Vec2) -> Rect {
        body.x += step.x;
        if let Some(hedge) = self.hedge_touching(body) {
            body.x = if step.x > 0.0 {
                hedge.left() - body.w
            } else {
                hedge.right()
            };
        }
        body.y += step.y;
        if let Some(hedge) = self.hedge_touching(body) {
            body.y = if step.y > 0.0 {
                hedge.top() - body.h
            } else {
                hedge.bottom()
            };
        }
        body
    }

    fn fly_bee(&mut self, ctx: &mut Context, speed: f32) {
        let input = &ctx.input;
        let mut dir = vec2(
            (input.axis(Key::Left, Key::Right) + input.axis(Key::A, Key::D)).clamp(-1.0, 1.0),
            (input.axis(Key::Up, Key::Down) + input.axis(Key::W, Key::S)).clamp(-1.0, 1.0),
        );
        if dir.length() > 0.0 {
            dir = dir.normalized();
            if dir.x != 0.0 {
                self.facing_left = dir.x < 0.0;
            }
        }
        self.bee = self.slide(self.bee, dir * speed * ctx.dt());
    }

    fn move_wasps(&mut self, ctx: &mut Context, speed: f32, sight: f32) {
        let target = self.bee.center();
        for i in 0..self.wasps.len() {
            let wasp = &mut self.wasps[i];
            let to_bee = target - wasp.body.center();
            wasp.chasing = to_bee.length() < sight;
            let dir = if wasp.chasing {
                to_bee.normalized()
            } else {
                wasp.wander_time -= ctx.dt();
                if wasp.wander_time <= 0.0 {
                    wasp.wander_time = ctx.rng.range(1.0, 2.5);
                    let angle = ctx.rng.range(0.0, std::f32::consts::TAU);
                    wasp.wander = vec2(angle.cos(), angle.sin());
                }
                wasp.wander * 0.5
            };
            self.wasps[i].body = self.slide(self.wasps[i].body, dir * speed * ctx.dt());
        }
    }

    fn pick_and_deliver(&mut self, ctx: &mut Context) {
        // Pick from the flowers you touch, while there's room in the bag.
        let mut i = 0;
        while i < self.flowers.len() {
            if self.carried.len() < CARRY_MAX && circle_vs_rect(self.flowers[i], 5.0, self.bee) {
                self.carried.push(self.flowers.swap_remove(i));
                ctx.play(Sound {
                    name: "pick",
                    wave: Wave::Sine,
                    freq: 700.0,
                    slide_to: 1100.0,
                    ms: 70.0,
                    volume: 0.25,
                    ..Default::default()
                });
            } else {
                i += 1;
            }
        }
        // Home: everything carried is delivered.
        if !self.carried.is_empty() && self.bee.overlaps(&self.hive) {
            self.delivered += self.carried.len();
            self.carried.clear();
            ctx.play(Sound::coin());
            if self.delivered == self.total {
                self.state = State::Won;
                ctx.beep(880.0, 300.0);
            }
        }
    }

    fn take_stings(&mut self, ctx: &mut Context) {
        self.safe_time = (self.safe_time - ctx.dt()).max(0.0);
        if self.safe_time > 0.0 {
            return;
        }
        let Some(wasp) = self.wasps.iter().find(|w| w.body.overlaps(&self.bee)) else {
            return;
        };
        let away = (self.bee.center() - wasp.body.center()).normalized();
        for _ in 0..4 {
            self.bee = self.slide(self.bee, away * 3.0);
        }
        // The pollen falls back onto its flowers.
        self.flowers.append(&mut self.carried);
        self.lives -= 1;
        self.safe_time = SAFE_TIME;
        self.camera.shake(4.0, 0.3);
        if self.lives == 0 {
            self.state = State::Lost;
            ctx.play(Sound::explosion());
        } else {
            ctx.play(Sound::hit());
        }
    }
}

impl Game for PollenRun {
    fn update(&mut self, ctx: &mut Context) {
        let bee_speed = ctx.tune("bee_speed", BEE_SPEED);
        let slowdown = ctx.tune("carry_slowdown", CARRY_SLOWDOWN);
        let wasp_speed = ctx.tune("wasp_speed", WASP_SPEED);
        let sight = ctx.tune("sight", SIGHT);

        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Playing if ctx.input.was_pressed(Key::P) => self.state = State::Paused,
            State::Paused if ctx.input.was_pressed(Key::P) => self.state = State::Playing,
            State::Won | State::Lost
                if ctx.input.was_pressed(Key::Space) || ctx.input.was_pressed(Key::Enter) =>
            {
                *self = PollenRun::new();
            }
            _ => {}
        }

        if self.state == State::Playing {
            self.time += ctx.dt();
            let speed = bee_speed * (1.0 - slowdown * self.carried.len() as f32);
            self.fly_bee(ctx, speed);
            self.move_wasps(ctx, wasp_speed, sight);
            self.take_stings(ctx);
            self.pick_and_deliver(ctx);
        } else {
            ctx.idle_ok();
        }

        let garden = self.garden();
        self.camera.follow(self.bee.center(), 0.15, ctx.dt());
        self.camera.position.x = self
            .camera
            .position
            .x
            .clamp(WIDTH / 2.0, garden.w - WIDTH / 2.0);
        self.camera.position.y = self
            .camera
            .position
            .y
            .clamp(HEIGHT / 2.0, garden.h - HEIGHT / 2.0);
        self.camera.update(ctx);

        ctx.check("bee inside the garden", garden.contains(self.bee.center()));
        ctx.check(
            "bee never inside a hedge",
            self.hedge_touching(self.bee).is_none(),
        );
        let wasps_clear = self
            .wasps
            .iter()
            .all(|w| self.hedge_touching(w.body).is_none());
        ctx.check("wasps never inside a hedge", wasps_clear);
        ctx.check(
            "pollen adds up",
            self.delivered + self.carried.len() + self.flowers.len() == self.total,
        );
        ctx.check(
            "never carrying more than the max",
            self.carried.len() <= CARRY_MAX,
        );
        ctx.check("lives never above the start", self.lives <= START_LIVES);

        ctx.debug_rect(self.bee);
        ctx.debug_rect(self.hive);
        for wasp in &self.wasps {
            ctx.debug_rect(wasp.body);
            if wasp.chasing {
                ctx.debug_circle(wasp.body.center(), sight);
            }
        }

        ctx.report("state", format!("{:?}", self.state));
        ctx.report("delivered", self.delivered);
        ctx.report("carried", self.carried.len());
        ctx.report("lives", self.lives);
        ctx.report("x", self.bee.x.round());
        ctx.report("y", self.bee.y.round());
        ctx.report("chasing", self.wasps.iter().filter(|w| w.chasing).count());
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::from_hex(0x3f8f3a)); // grass

        canvas.push_camera(&self.camera);
        let view = self.camera.visible_area();
        let first = |v: f32| (v / TILE).floor().max(0.0) as usize;
        let (tx0, ty0) = (first(view.left()), first(view.top()));
        let (tx1, ty1) = (
            (view.right() / TILE) as usize + 1,
            (view.bottom() / TILE) as usize + 1,
        );
        for (ty, row) in self.hedges.iter().enumerate().take(ty1).skip(ty0) {
            for (tx, &hedge) in row.iter().enumerate().take(tx1).skip(tx0) {
                let tile = Rect::new(tx as f32 * TILE, ty as f32 * TILE, TILE, TILE);
                if hedge {
                    canvas.fill_rect(tile, Color::from_hex(0x1f5a24));
                    canvas.fill_circle(tile.center(), 6.0, Color::from_hex(0x2a7030));
                } else if (tx * 7 + ty * 3) % 11 == 0 {
                    canvas.fill_rect(
                        Rect::new(tile.x + 5.0, tile.y + 9.0, 1.0, 3.0),
                        Color::from_hex(0x57a84e),
                    );
                }
            }
        }
        // The hive: amber with dark stripes.
        canvas.fill_rect(self.hive, Color::from_hex(0xd99a2b));
        for i in 0..3 {
            let y = self.hive.y + 4.0 + i as f32 * 4.0;
            canvas.fill_rect(
                Rect::new(self.hive.x, y, TILE, 1.0),
                Color::from_hex(0x7a4a10),
            );
        }
        canvas.label("hive", self.hive);
        for &flower in &self.flowers {
            for (dx, dy) in [(-3.0, 0.0), (3.0, 0.0), (0.0, -3.0), (0.0, 3.0)] {
                canvas.fill_circle(flower + vec2(dx, dy), 2.5, Color::from_hex(0xf2a0d0));
            }
            canvas.fill_circle(flower, 2.0, Color::from_hex(0xffe14a));
            canvas.label("flower", Rect::from_center(flower, 10.0, 10.0));
        }
        let scaled = Blit {
            scale: 2.0,
            ..Default::default()
        };
        for wasp in &self.wasps {
            let frame = self.wasp_anim.frame_at(self.time);
            let flip = Blit {
                flip_x: wasp.body.center().x > self.bee.center().x,
                ..scaled
            };
            canvas.blit_sprite(frame, wasp.body.center() - vec2(6.0, 6.0), flip);
            canvas.label("wasp", wasp.body);
        }
        let blink = self.safe_time > 0.0 && (self.safe_time * 10.0) as i32 % 2 == 0;
        if !blink {
            let frame = self.bee_anim.frame_at(self.time);
            let blit = Blit {
                flip_x: self.facing_left,
                ..scaled
            };
            canvas.blit_sprite(frame, self.bee.center() - vec2(6.0, 6.0), blit);
            // The pollen you carry, as yellow dots above you.
            for i in 0..self.carried.len() {
                let dot = vec2(self.bee.x + 2.0 + i as f32 * 4.0, self.bee.y - 4.0);
                canvas.fill_circle(dot, 1.5, Color::from_hex(0xffe14a));
            }
        }
        canvas.label("bee", self.bee);
        canvas.pop_camera();

        canvas.fill_rect(
            Rect::new(0.0, 0.0, WIDTH, 12.0),
            Color::BLACK.with_alpha(170),
        );
        let hud = format!(
            "HONEY {}/{}   CARRYING {}/{}   LIVES {}",
            self.delivered,
            self.total,
            self.carried.len(),
            CARRY_MAX,
            self.lives
        );
        canvas.draw_text(&hud, vec2(4.0, 3.0), 1, Color::WHITE);
        let (title, hint, color) = match self.state {
            State::Playing => return,
            State::Paused => ("PAUSED", "press P to carry on", Color::WHITE),
            State::Won => (
                "SWEET!",
                "every flower is honey. Space plays again",
                Color::YELLOW,
            ),
            State::Lost => (
                "STUNG!",
                "press Space to try again",
                Color::from_hex(0xff6060),
            ),
        };
        canvas.fill_rect(
            Rect::new(0.0, 90.0, WIDTH, 56.0),
            Color::BLACK.with_alpha(170),
        );
        canvas.draw_text_centered(title, 100.0, 3, color);
        canvas.draw_text_centered(hint, 130.0, 1, Color::GRAY);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Pollen Run - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), PollenRun::new())
}

// ---- Play tests: `cargo test --example pollen_run` ----

#[cfg(test)]
mod tests {
    use super::*;

    fn hold(keys: &[&str], frames: u64) -> String {
        let presses: Vec<String> = keys
            .iter()
            .map(|k| format!(r#"{{"frame": 1, "press": "{k}"}}"#))
            .collect();
        format!(
            r#"{{"events": [{}], "frames": {frames}}}"#,
            presses.join(", ")
        )
    }

    /// A garden with no wasps and the given flowers, the bee in its place.
    fn quiet_garden(flowers: Vec<Vec2>) -> PollenRun {
        let mut game = PollenRun::new();
        game.wasps.clear();
        game.total = flowers.len();
        game.flowers = flowers;
        game
    }

    #[test]
    fn the_map_is_closed_with_one_bee_and_one_hive() {
        for row in MAP {
            assert_eq!(row.len(), MAP[0].len());
            assert!(row.starts_with('#') && row.ends_with('#'));
        }
        assert!(
            MAP[0]
                .chars()
                .chain(MAP[MAP.len() - 1].chars())
                .all(|c| c == '#')
        );
        assert_eq!(MAP.concat().matches('p').count(), 1);
        assert_eq!(MAP.concat().matches('h').count(), 1);
    }

    #[test]
    fn the_bag_holds_three() {
        let start = PollenRun::new().bee.center();
        let row = (1..=4).map(|i| start + vec2(i as f32 * 8.0, 0.0)).collect();
        let played = PlayTest::new(config(), quiet_garden(row))
            .script(&hold(&["Right"], 40))
            .frames(40)
            .run();
        assert_eq!(played.number("carried"), 3.0);
        assert_eq!(played.game().flowers.len(), 1, "no room for the fourth");
        assert_eq!(played.sound_frames("pick").len(), 3);
    }

    #[test]
    fn carrying_slows_the_bee() {
        let distance = |carried: usize| {
            let mut game = quiet_garden(vec![]);
            game.carried = vec![Vec2::ZERO; carried];
            game.total = carried;
            let start = game.bee.center();
            let played = PlayTest::new(config(), game)
                .script(&hold(&["Right"], 30))
                .frames(30)
                .run();
            played.game().bee.center().distance(start)
        };
        let (light, full) = (distance(0), distance(3));
        let expected = light * (1.0 - 3.0 * CARRY_SLOWDOWN);
        assert!((full - expected).abs() < 1.0, "{light} vs {full}");
    }

    #[test]
    fn the_hive_turns_pollen_into_honey() {
        let mut game = quiet_garden(vec![]);
        game.carried = vec![Vec2::ZERO; 2];
        game.total = 3; // one more somewhere, so it isn't won yet
        game.flowers = vec![vec2(40.0, 40.0)];
        // The hive is up and to the left of the start.
        let played = PlayTest::new(config(), game)
            .script(&hold(&["Left", "Up"], 30))
            .frames(30)
            .run();
        assert_eq!(played.number("delivered"), 2.0);
        assert_eq!(played.number("carried"), 0.0);
        assert_eq!(played.sound_frames("coin").len(), 1);
        assert_eq!(played.text("state"), "Playing");
    }

    #[test]
    fn a_sting_drops_the_pollen_back_on_its_flowers() {
        let mut game = PollenRun::new();
        let picked: Vec<Vec2> = game.flowers.drain(..2).collect();
        game.carried = picked.clone();
        game.wasps[0].body = game.bee; // right on the bee
        let mut sim = Simulation::new(config(), game);
        sim.step();
        assert_eq!(sim.number("lives"), 2.0);
        assert_eq!(sim.number("carried"), 0.0);
        assert!(picked.iter().all(|f| sim.game().flowers.contains(f)));
        sim.assert_sound("hit", 1, 0);
    }

    /// Makes playthroughs/pollen_run.json: a bot fills its bag at the
    /// nearest flowers, flies home, and keeps two tiles from wasps.
    /// `cargo test --example pollen_run -- --ignored`
    #[test]
    #[ignore]
    fn record_playthrough() {
        let mut sim = Simulation::new(config(), PollenRun::new());
        sim.step();
        while sim.text("state") == "Playing" && sim.context().frame() < 90 * 60 {
            let game = sim.game();
            let goals: Vec<Vec2> = if game.carried.len() < CARRY_MAX && !game.flowers.is_empty() {
                game.flowers.clone()
            } else {
                vec![game.hive.center()]
            };
            let next = next_step(game, &goals);
            let me = game.bee.center();
            let keys = [
                (Key::Left, next.x < me.x - 1.0),
                (Key::Right, next.x > me.x + 1.0),
                (Key::Up, next.y < me.y - 1.0),
                (Key::Down, next.y > me.y + 1.0),
            ];
            for (key, held) in keys {
                if held {
                    sim.press(key);
                } else {
                    sim.release(key);
                }
            }
            sim.step();
        }
        for key in [Key::Left, Key::Right, Key::Up, Key::Down] {
            sim.release(key);
        }
        sim.run(60);
        sim.recording()
            .save("playthroughs/pollen_run.json")
            .unwrap();
    }

    /// The center of the next tile on the shortest path to the nearest goal,
    /// keeping two tiles from wasps (a breadth-first search).
    fn next_step(game: &PollenRun, goals: &[Vec2]) -> Vec2 {
        let tile_of = |p: Vec2| ((p.x / TILE) as usize, (p.y / TILE) as usize);
        let center =
            |(x, y): (usize, usize)| vec2((x as f32 + 0.5) * TILE, (y as f32 + 0.5) * TILE);
        let start = tile_of(game.bee.center());
        let goals: Vec<(usize, usize)> = goals.iter().map(|&g| tile_of(g)).collect();
        let mut came_from = std::collections::HashMap::from([(start, start)]);
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some(tile) = queue.pop_front() {
            if goals.contains(&tile) {
                let mut step = tile;
                while came_from[&step] != start && step != start {
                    step = came_from[&step];
                }
                return center(step);
            }
            let (x, y) = tile;
            for next in [(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)] {
                let near_wasp = game
                    .wasps
                    .iter()
                    .any(|w| w.body.center().distance(center(next)) < 2.0 * TILE);
                if !game.hedges[next.1][next.0] && !near_wasp && !came_from.contains_key(&next) {
                    came_from.insert(next, tile);
                    queue.push_back(next);
                }
            }
        }
        game.bee.center()
    }
}
