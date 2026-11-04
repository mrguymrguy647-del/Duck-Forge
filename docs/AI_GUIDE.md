# duckforge for AI assistants: building and testing games without a screen

This guide is for an AI assistant (or anyone) building games with duckforge
on a machine with no screen, like a cloud Linux container. You can't watch
the game, so the engine lets you **run it headless, look at pictures of
it, read what it reports, and test it automatically**. Everything here has
been tried on such a machine: `examples/dodge.rs` was built by following
this guide, and the stories in it are real.

Contents:

1. [Quick facts](#1-quick-facts)
2. [Set up on Linux](#2-set-up-on-linux)
3. [Make a new game](#3-make-a-new-game)
4. [The loop: write, run, look, read, test, fix](#4-the-loop-write-run-look-read-test-fix)
5. [Input scripts in two minutes](#5-input-scripts-in-two-minutes)
6. [Play tests](#6-play-tests)
7. [API cheat sheet](#7-api-cheat-sheet)
8. [Common mistakes](#8-common-mistakes)
9. [Does the game feel good?](#9-does-the-game-feel-good)
10. [Showing your work to a person](#10-showing-your-work-to-a-person)
11. [What the engine can't do yet](#11-what-the-engine-cant-do-yet)

---

## 1. Quick facts

| Thing | Fact |
|-------|------|
| Canvas | 320 x 240 pixels by default (`Config::width`, `height`). The window shows it 3x bigger. |
| Coordinates | Pixels, `(0, 0)` is the **top-left**, **y grows downward**. `Rect::new(x, y, w, h)`: `x, y` is the top-left corner. |
| Time | `update` runs **60 times per second of game time**, always with `ctx.dt() == 1.0 / 60.0` (the default; `Config::timestep` changes it). Speeds are in pixels **per second**: multiply by `ctx.dt()`. |
| Frames | Counted from **1**: `ctx.frame()` is 1 during the first `update`. Frame 600 = 10 seconds. Scripts and logs use the same numbers. |
| Randomness | Use `ctx.rng` only (`range`, `range_i32`, `chance`). Headless runs use seed 1 unless told otherwise, so **the same command gives the same result**. |
| Drawing | `draw(&self, canvas)` can't change the game. Start it with `canvas.clear(...)`. Later draws go on top. |
| Text | Built-in 3x5 pixel font: `A-Z 0-9 . , : ! ? - + = / ( ) '` and space. Lowercase is drawn as uppercase; anything else becomes `?`. Scale 1 is 5 px tall. |
| Colors | `Color::rgb(r, g, b)` (solid), `Color::rgba(r, g, b, a)` and `color.with_alpha(a)` (a: 0 invisible to 255 solid; every drawing function blends), `Color::from_hex(0x14_14_28)`, constants `BLACK WHITE GRAY RED ORANGE YELLOW GREEN BLUE PURPLE`, `a.lerp(b, t)`. Build colors with these functions, never `Color { r, g, b }` (it has a 4th field, `a`). |
| Engine keys | In a window: F3 = FPS counter, F10 = record, F12 = screenshot. Escape quitting is each game's choice (`ctx.quit()`). |
| Output | `--headless` writes `last.png` and `log.json` (plus what you ask for) to `duckforge-out/`, or the folder in `--out`. |

## 2. Set up on Linux

You need Rust 1.87 or newer, and nothing else. No X11, no graphics drivers,
no system packages. (If `cargo` is missing:
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
then `. "$HOME/.cargo/env"`.)

Build and run **without the window and GPU code**, with
`--no-default-features`. Without it, Cargo also builds the window library
(minifb) and the GPU library (wgpu): minutes longer, possibly needing
system libraries, and a window can't open on a server anyway:

```sh
cargo build --no-default-features --examples
cargo test --no-default-features               # about 140 tests, a few seconds
cargo run --no-default-features --example breakout -- --headless --sheet --out check
```

The last command should print something like:

```text
duckforge: ran "Breakout - duckforge" without a window for 600 frames (10.0 s of game time) in 0.01 s, seed 1
duckforge: saved in check: last.png, sheet.png, log.json
duckforge: the game reported bricks_left = 60, lives = 3, score = 0, state = "Serving"
```

Open `check/sheet.png`: you should see Breakout. If you can't view images,
you can still do almost everything with `log.json` and play tests.

**Tip:** put `--no-default-features` in every `cargo` command on a
server, or Cargo rebuilds the engine with different features each time
you switch. For long headless runs or big 3D scenes, add `--release`.

## 3. Make a new game

A game is a struct that implements `Game`. Put it in
`examples/<name>.rs` inside this repository; Cargo finds it by itself.
Here is a complete starting point. Copy it, rename it, and grow it:

```rust
//! Catch: move left and right to catch the falling star. Miss one, and
//! it's game over.
//!
//! Run it with:   cargo run --example mygame
//! Controls:      Left/Right or A/D move, Space starts, Escape quits.

use duckforge::prelude::*;

const WIDTH: f32 = 320.0;
const HEIGHT: f32 = 240.0;
const PLAYER_SPEED: f32 = 200.0; // pixels per second
const STAR_SPEED: f32 = 80.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ready,
    Playing,
    GameOver,
}

struct MyGame {
    state: State,
    player: Rect,
    star: Rect,
    score: u32,
}

impl MyGame {
    fn new() -> Self {
        Self {
            state: State::Ready,
            player: Rect::new(150.0, 210.0, 24.0, 8.0),
            star: Rect::new(156.0, -8.0, 8.0, 8.0),
            score: 0,
        }
    }

    fn drop_new_star(&mut self, rng: &mut Rng) {
        self.star.x = rng.range(0.0, WIDTH - self.star.w);
        self.star.y = -self.star.h;
    }
}

impl Game for MyGame {
    fn update(&mut self, ctx: &mut Context) {
        if ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
        match self.state {
            State::Ready => {
                if ctx.input.was_pressed(Key::Space) {
                    self.state = State::Playing;
                    self.drop_new_star(&mut ctx.rng);
                }
            }
            State::Playing => {
                let direction =
                    ctx.input.axis(Key::Left, Key::Right) + ctx.input.axis(Key::A, Key::D);
                self.player.x += direction.clamp(-1.0, 1.0) * PLAYER_SPEED * ctx.dt();
                self.player.x = self.player.x.clamp(0.0, WIDTH - self.player.w);

                // Each star falls a little faster than the one before.
                self.star.y += (STAR_SPEED + self.score as f32 * 5.0) * ctx.dt();
                if self.star.overlaps(&self.player) {
                    self.score += 1;
                    self.drop_new_star(&mut ctx.rng);
                } else if self.star.top() > HEIGHT {
                    self.state = State::GameOver;
                }
            }
            State::GameOver => {
                if ctx.input.was_pressed(Key::Space) {
                    *self = MyGame::new(); // restart
                }
            }
        }

        // What tests and log.json can see. Report what matters.
        ctx.report("state", format!("{:?}", self.state));
        ctx.report("score", self.score);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::from_hex(0x10_14_24));
        if self.state == State::Playing {
            canvas.fill_rect(self.star, Color::YELLOW);
        }
        canvas.fill_rect(self.player, Color::WHITE);
        canvas.draw_text(&format!("SCORE {}", self.score), vec2(4.0, 4.0), 1, Color::WHITE);
        let message = match self.state {
            State::Ready => "PRESS SPACE TO START",
            State::GameOver => "GAME OVER - SPACE TO RETRY",
            State::Playing => "",
        };
        canvas.draw_text_centered(message, HEIGHT / 2.0, 2, Color::WHITE);
    }
}

fn config() -> Config {
    Config {
        title: String::from("Catch - duckforge"),
        width: WIDTH as usize,
        height: HEIGHT as usize,
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), MyGame::new())
}
```

Then:

```sh
cargo run --no-default-features --example mygame -- --headless --sheet --out out/mygame
```

Keep `config()` as its own function: play tests need the same settings
as `main`. To run the tests inside an example with `cargo test`, add it to
`Cargo.toml`:

```toml
[[example]]
name = "mygame"
test = true
```

**A game in its own project** works too: `cargo new mygame`, then in its
`Cargo.toml` add
`duckforge = { path = "../game-engine-rust-language", default-features = false }`
(or a `git = "..."` dependency), and put the code in `src/main.rs`. Every
option in this guide works the same way, with `cargo run -- --headless ...`.

**Good examples to read**, smallest first: `examples/hello.rs` (a square
you move), `examples/dodge.rs` (a complete small game with play tests and
a bot, built with this guide), `examples/breakout.rs` (a bigger game, with
a bot that makes recordings), `examples/physics.rs` (physics bodies), and
`examples/world3d.rs` (3D).

## 4. The loop: write, run, look, read, test, fix

You can't play the game, so you work in a loop. Each turn takes seconds.

### Step 1: Build

```sh
cargo build --no-default-features --example mygame
```

Read the whole error. Rust's errors usually say exactly what to change. A
very common one in games is `cannot borrow ... as mutable because it is
also borrowed as immutable`: see [Common mistakes](#8-common-mistakes).

### Step 2: Run with nobody playing

```sh
cargo run --no-default-features --example mygame -- --headless --sheet --out out/idle
```

Does the title screen show? Does anything move by itself? Does the game
crash? A panic prints its message and the line, like any Rust program.

### Step 3: Play it with a script

Most games need input before anything happens. Write a script
([section 5](#5-input-scripts-in-two-minutes)) that does what a player
would: start the game, move around, shoot. Then:

```sh
cargo run --no-default-features --example mygame -- --headless \
    --inputs out/start.json --frames 1200 --sheet --gif --out out/play
```

`--frames` is how long to run (60 per second). Choose it to cover the
interesting part: in the first run of `dodge`, half the sheet was the same
GAME OVER screen, because the run was twice as long as the game.

### Step 4: Look

Open, in this order:

1. **`sheet.png`**: the whole run on one page, 16 frames labelled with
   frame numbers and seconds. Look for: a screen that never changes (stuck
   game), things in the wrong place, text you can't read, objects drawn
   over the score, missing feedback (did anything show that the player was
   hit?).
2. **`last.png`**: how it ended.
3. **A close-up**, when you need to see exact pixels: is the ball touching
   the paddle, or one pixel above? `--crop X,Y,W,H --zoom 4` saves
   `closeup.png` of the last frame, and with `--snapshot-every N` also
   `closeup-000060.png` and so on for every snapshot.
4. **`run.gif`**: for motion. It's for people, mostly.

To see one particular moment, stop the run there: `--frames 427` makes
`last.png` frame 427. To see the moments before something happened, add
`--snapshot-every 4` and look at the last few.

Some things can't be seen in a sheet at all, like a blink every few
frames. Count pixels in close-ups instead. Checking that `dodge`'s player
blinks after a hit meant counting yellow pixels in each 4th frame's
close-up: on, on, off, off, on, on, off, off...

### Step 5: Read `log.json`

```json
{
  "game": "Dodge - duckforge",
  "seed": 1,
  "updates_per_second": 60,
  "frames": 1320,
  "seconds": 22.0,
  "game_quit": false,
  "inputs": {"events": 2, "replayed": 2},
  "files": ["run.gif", "last.png", "sheet.png", "log.json"],
  "final": {"dodged": 28, "hits": 3, "lives": 0, "seconds": 17, "state": "GameOver"},
  "changes": [
    {"frame": 1, "dodged": 0, "hits": 0, "lives": 3, "seconds": 0, "state": "Ready"},
    {"frame": 30, "state": "Playing"},
    {"frame": 450, "hits": 1, "lives": 2, "seconds": 7},
    ...
  ]
}
```

`final` is the end state. `changes` has one line per frame where any
reported value changed, so it reads like a story. This is the fastest way
to answer "what happened?", and the only way that doesn't need images.
Read it critically:

- **Events too close together.** In `dodge`'s first version, lives went
  from 3 to 1 on frames 450 and 487, 0.6 seconds apart: one hit turned
  into two, because there was no safe time after a hit.
- **Nothing happening for a long time.** That's a boring stretch, or a
  stuck game.
- **Numbers that don't add up.** Score 2100 means every brick was broken?
  Check `bricks_left` too.

### Step 6: Write play tests

When something works, pin it down with a play test
([section 6](#6-play-tests)), so it stays working while you change other
things. When something's broken, write the test first, watch it fail,
then fix it.

### Step 7: Fix, and go round again

Change one thing at a time, and rerun the same command. Because runs are
repeatable, any difference comes from your change and not from luck.

## 5. Input scripts in two minutes

A script is JSON: a list of events, each on a frame (counting from 1, 60
per second), with exactly one action. The full format is in
[`input-format.md`](input-format.md).

```json
{"events": [
  {"frame": 30, "press": "Space"},
  {"frame": 31, "release": "Space"},
  {"frame": 60, "press": "Right"},
  {"frame": 120, "release": "Right"},
  {"frame": 150, "mouse": [160, 120]},
  {"frame": 151, "mouse_press": "Left"},
  {"frame": 153, "mouse_release": "Left"},
  {"frame": 200, "scroll": -1}
]}
```

- **Tap a key:** press on frame N, release on frame N+1 or later. The game
  sees `was_pressed` on frame N. A press and release on the **same** frame
  cancel out: the game never sees the key.
- **Hold a key:** press on frame N, release on frame M. `is_down` is true
  from N to M-1. One second of holding = 60 frames.
- **Mouse positions** are in canvas pixels, the same coordinates you draw
  with.
- **Key names:** `A` to `Z`, `Digit0` to `Digit9`, `Left`, `Right`, `Up`,
  `Down`, `Space`, `Enter`, `Escape`, `Tab`, `Backspace`, `LeftShift`,
  `RightShift`, `LeftCtrl`, `RightCtrl`, `LeftAlt`, `RightAlt`, `F1` to
  `F12`. Upper or lower case both work.
- Optional top-level fields: `"seed"` (use this seed), `"frames"` (how
  long to run when `--frames` isn't given).
- A mistake stops the run with a message naming the event and the fix:
  `event 3 ({"frame":30,"press":"Spcae"}): unknown key "Spcae"...`.

**Scripts are text, so programs can write them.** Generate long or
repetitive ones with a loop (in a test, or a small script) instead of by
hand. `examples/physics.rs` builds twenty showers of balls this way.

**Or let a bot play, and record it.** For anything that needs reacting to
the game (catching a ball, dodging), write a bot in a test, as in
`examples/breakout.rs` and `examples/dodge.rs`. Each frame it reads the game
through `sim.game()`, presses keys with `sim.press(key)` /
`sim.release(key)`, then calls `sim.step()`. Afterwards,
`sim.recording().save("playthroughs/x.json")` writes a script that replays
the bot's game exactly.

**A person can record one for you:** in a window, F10 starts and stops
recording to `recording-1.json`, and `--inputs recording-1.json` replays
it, with or without a window.

## 6. Play tests

A play test is a normal `#[test]` that plays the game and checks what it
reported. Put them at the bottom of the game's file:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = r#"{"events": [
        {"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"}
    ]}"#;

    #[test]
    fn it_waits_for_space() {
        let played = PlayTest::new(config(), MyGame::new()).frames(300).run();
        assert_eq!(played.text("state"), "Ready");
    }

    #[test]
    fn space_starts_the_game() {
        let played = PlayTest::new(config(), MyGame::new())
            .script(START)
            .frames(60)
            .run();
        assert_eq!(played.text("state"), "Playing");
        assert_eq!(played.number("score"), 0.0);
    }

    #[test]
    fn missing_the_star_ends_the_game() {
        // Start, then run to the far left and stay there: the star (which
        // can fall anywhere) is missed sooner or later.
        let script = r#"{"events": [
            {"frame": 10, "press": "Space"}, {"frame": 11, "release": "Space"},
            {"frame": 12, "press": "Left"}
        ]}"#;
        let played = PlayTest::new(config(), MyGame::new())
            .script(script)
            .frames(60 * 60)
            .run();
        assert_eq!(played.text("state"), "GameOver");
    }
}
```

Run them with `cargo test --no-default-features --example mygame`.

- `.inputs("playthroughs/x.json")` replays a file (paths start from the
  project folder), and `.script(json)` replays a string.
- `.frames(n)` and `.seed(n)` change the run. Without `.frames`, a script
  with `"frames"` runs that long; otherwise 600.
- `.save_to("out/test-x")` also saves `last.png`, `sheet.png` and
  `log.json`, to look at when the test fails.
- `.run()` returns the finished `Simulation`:
  - `played.number(name)`, `played.flag(name)` and `played.text(name)` give
    the latest reported value. They fail with a list of the names that
    were reported, if you misspell one.
  - `played.history(name)` gives `(frame, value)` for every change. **The
    first entry is the starting value**, not the first change.
  - `played.reported_at(frame, name)` gives the value at a frame.
  - `played.game()` gives the game itself, and `played.picture()` the last
    frame.

**Tests worth writing for any game:**

| Test | Why |
|------|-----|
| Nobody presses anything: the title screen waits | The game doesn't start, crash or lose by itself |
| The start key starts it | The main path works |
| Doing nothing loses, but not instantly (`history`: first hit after a few seconds) | There's a challenge, and time to learn |
| A simple bot can survive or win | The game is fair: winning is possible |
| After a hit, no second hit for a moment | No unfair double hits |
| A recorded playthrough still wins | Changes didn't break a whole game |
| It gets harder over time | There's a difficulty curve |

`examples/dodge.rs` has most of these, and `examples/breakout.rs` replays
recorded playthroughs (`playthroughs/breakout-win.json`).

**Break the game on purpose once** (remove the floor, make points worth
nothing) and check your tests fail. A test that has never failed hasn't
proved anything.

**Check reports, not pixels.** "The score is 2100" survives a new font or
color; "pixel (40, 4) is white" doesn't. Use pictures to look, and
reports to test.

## 7. API cheat sheet

Everything below comes with `use duckforge::prelude::*;`, except
`duckforge::physics::{...}` and `duckforge::{run_headless, HeadlessOptions}`. `cargo doc --no-default-features --open`
has the rest.

**`Game`**: `fn update(&mut self, ctx: &mut Context)` and
`fn draw(&self, canvas: &mut Canvas)`.

**`Config`** (use `..Config::default()` for the rest): `title`, `width`,
`height`, `scale` (window size multiplier, 3), `target_fps` (60),
`show_fps`, `renderer`, `timestep` (`Timestep::Fixed { updates_per_second: 60 }`),
`seed` (`None`).

**`Context`** (`ctx`): `ctx.dt()` (seconds per update), `ctx.time()`,
`ctx.frame()`, `ctx.width()` / `ctx.height()` (as `f32`), `ctx.quit()`,
`ctx.report(name, value)`, `ctx.input`, `ctx.rng`.

**`ctx.input`**: `is_down(key)`, `was_pressed(key)` (one frame per
press), `was_released(key)`, `axis(Key::Left, Key::Right)` (-1, 0 or 1),
`mouse_position()`, `mouse_delta()`, `is_mouse_down(button)`,
`was_mouse_pressed(button)`, `was_mouse_released(button)`, `scroll()`.
`MouseButton::Left`, `Right` or `Middle`.

**`ctx.rng`**: `range(min, max)` (an `f32`, max excluded),
`range_i32(min, max)`, `chance(0.25)`, `next_f32()`.

**`Canvas`**: `clear(color)`, `fill_rect(rect, color)`,
`draw_rect(rect, color)` (outline), `fill_circle(center, radius, color)`,
`draw_line(from, to, color)`, `fill_triangle(a, b, c, color)`,
`set_pixel(x, y, color)`, `get_pixel(x, y)`,
`draw_text(text, pos, scale, color)`,
`draw_text_centered(text, y, scale, color)`,
`Canvas::text_width(text, scale)`, `width()`, `height()`. Any color with
alpha below 255 is drawn see-through (`clear` ignores alpha).

**Sprites**: `Sprite::from_ascii(&["..yy", "yyyo"], &[('y', Color::YELLOW), ('o', Color::ORANGE)])`
(one character per pixel, `'.'` see-through; draw small art straight in
code, no files needed), `Sprite::load_png(path)?` (returns `io::Result`),
`Sprite::from_pixels(w, h, vec_of_colors)`. `sprite.width()`,
`height()`, `get_pixel(x, y)` (an `Option<Color>`; `.a` is 0 where
see-through), `sub(x, y, w, h)` and `frames(w, h)` (cut a sheet, no
copying). Draw with
`canvas.blit_sprite(&sprite, top_left, Blit { flip_x, flip_y, scale, tint, opacity, ..Default::default() })`
or `Blit::default()`; scaling is nearest-neighbour. Animations:
`Animation::new(frames, fps)`, `anim.frame_at(seconds)` (loops),
`index_at(seconds)`, `duration()`.

**Camera** (`Camera2D`): `Camera2D::new(screen_w, screen_h)` starts with
world = screen. Fields `position` (world point at screen center), `zoom`
(1.0), `screen_size`. `camera.follow(target, smoothing_seconds, ctx.dt())`
(0.0 snaps, 0.2 smooth), `camera.shake(pixels, seconds)` then
`camera.update(ctx)` every update (uses `ctx.rng`, so replays match),
`is_shaking()`, `screen_to_world(p)` (the mouse: pass
`ctx.input.mouse_position()`), `world_to_screen(p)`, `visible_area()`.
In `draw`: `canvas.push_camera(&self.camera)`, draw the world,
`canvas.pop_camera()`, then draw the HUD in screen pixels.
`canvas.push_offset(v)` / `pop_offset()` shift a group of drawings.
Lines stay 1 px at any zoom; `clear`, `get_pixel` and 3D ignore the
camera; the engine resets the camera stack every frame.

**Math**: `vec2(x, y)` / `Vec2` (`+ - * f32`, `length()`, `normalized()`,
`distance(other)`, `dot(other)`, `Vec2::ZERO`).
`Rect::new(x, y, w, h)`, `Rect::from_center(center, w, h)`, `left()`,
`right()`, `top()`, `bottom()`, `center()`, `overlaps(&other)`,
`contains(point)`. `vec3(x, y, z)` / `Vec3` for 3D (y is up).

**Reports**: `ctx.report("name", value)` with a number (any integer or
float type), a `bool`, a `&str` or a `String`. Enums: report
`format!("{:?}", self.state)`.

**Physics** (`use duckforge::physics::{Body, BodyId, PhysicsWorld, Shape};`):
`PhysicsWorld::new(vec2(0.0, 400.0))` (gravity, y down),
`world.add(Body::ball(position, radius))` or
`Body::block(center, size)`, then `.fixed()`, `.with_velocity(v)`,
`.with_bounce(0.5)`, `.with_friction(0.6)`, `.with_mass(m)`. In `update`:
`world.step(ctx.dt())`. Read bodies with `world.get(id)` (`position`,
`velocity`, `shape`), `world.bodies()`, `world.contacts()`,
`world.touching(a, b)`, `world.remove(id)`. Boxes don't rotate. Works the
same in 3D with `Vec3`.

**3D**: `Camera3D::new(position)` (looks along +z) or
`Camera3D::looking_at(position, target)`, `camera.fly(ctx, speed)`.
Meshes: `Mesh::cube(color)`, `Mesh::sphere(color, segments)`,
`Mesh::pyramid(color)`, `Mesh::checkerboard(tiles, tile_size, a, b)`.
`Transform::at(position).rotated(vec3(x, y, z)).sized(scale)`.
In `draw`: `canvas.draw_mesh(&mesh, &transform, &camera)`. Headless runs
draw 3D on the CPU, so there's no graphics card to worry about.

**Headless from code**: `duckforge::run_headless(config, game, &HeadlessOptions { frames, out, inputs, sheet, gif, ..Default::default() })`,
and `Simulation::new(config, game)` with `press`, `release`,
`move_mouse`, `press_mouse`, `release_mouse`, `scroll`, `step`,
`run(frames)`, `game()`, `picture()`, `recording()`, `follow(script)`.

**Command line**: `--headless`, `--frames N`, `--out DIR`, `--seed N`,
`--inputs FILE`, `--snapshot-every N`, `--sheet`, `--gif`,
`--crop X,Y,W,H`, `--zoom N`, `--record FILE` (window only), `--help`.
They go after `--` with `cargo run`: `cargo run --example x -- --headless`.

## 8. Common mistakes

Each is real: made while building these games, or a trap the engine was
designed around.

**Running a window on a server.** `could not open a window ... run the
game with --headless instead`. Add `--headless`, and build with
`--no-default-features`.

**Forgetting `--` before the game's options.** `cargo run --example x
--headless` passes `--headless` to Cargo, which refuses it with
`error: unexpected argument '--headless' found`. Write
`cargo run --example x -- --headless`.

**Moving by a fixed amount per update.** `x += 3.0` works, but every speed
then depends on the update rate. Write speeds per second and multiply by
`ctx.dt()`, as in `x += SPEED * ctx.dt()`.

**`is_down` vs `was_pressed`.** Use `is_down` for holding (moving),
`was_pressed` for one-shot actions (jump, shoot, start). With `is_down`,
"Space to start" would also fire the first shot.

**Forgetting `canvas.clear` at the top of `draw`.** Everything smears,
because the canvas keeps last frame's picture.

**Drawing order.** Later draws cover earlier ones. Draw the background,
then objects, then the score and messages. Even then, text over busy
game objects is hard to read: in `dodge`, white score text over a red
block coming in at the top was a blur. The fix was a dark strip behind
the score.

**Text that disappears or becomes `?`.** The font has capitals, digits and
`. , : ! ? - + = / ( ) '`. No other symbols. `draw_text_centered` centers
the text horizontally; you choose `y`.

**Reporting in `draw`.** You can't: `draw` gets no `ctx`. Report in
`update`.

**Reporting a value that changes all the time.** `dodge` first reported
how many blocks were on screen. It changed every few frames, and buried
the interesting lines in `log.json`. Report things a test or a person
cares about. Counters of events (`hits`, `dodged`) are better than
values that churn (`blocks_on_screen`).

**Reading the first entry of `history` or `changes` as an event.** The
first entry holds the starting values. `dodge`'s test for "the first hit
comes after a few seconds" first found `hits: 0` on frame 1. Skip it with
`.history("hits").get(1)`, or compare values.

**Pressing and releasing on the same frame in a script.** The game never
sees the key. Release at least one frame later.

**Frame 0 in a script.** Frames count from 1. The engine refuses frame 0
with an explanation.

**Things that break repeatability:** `std::time::Instant` or
`SystemTime` in game logic, any random number generator other than
`ctx.rng`, and decisions based on the order of a `HashMap`'s items. With
these, the same script gives different games, and tests flicker between
passing and failing.

**The borrow checker in a bot or in `update`:**

```text
error[E0502]: cannot borrow `sim` as mutable because it is also borrowed as immutable
```

You're holding `let game = sim.game();` while calling `sim.press(...)`.
Look first, then act: copy what you need out of `game` (positions,
state, the target) into local variables, and call `sim.press` after the
last use of `game`. Inside a game, the same thing happens when you call a
`&mut self` method while looping over one of your own lists:
`for enemy in &self.enemies { self.add_points(1); }`. (Changing a
*different field*, like `self.score += 1`, is fine: Rust can see the two
don't overlap.) Collect what to change first, or loop over indices.

**Blaming the game when the bot is wrong.** `dodge`'s first bot died
quickly, and it looked like the game was too hard. Close-ups of the frames
before each hit (`--frames 427 --snapshot-every 8 --crop 0,150,320,90
--zoom 2`) showed the real cause: as soon as a block drew level with the
player, the bot no longer counted it as dangerous, walked back toward the
middle, and ran into its side. When a bot or a test fails, look at the
frames just before the failure before changing the game.

**Comparing floats exactly.** Positions after many updates are rarely
whole numbers. Use ranges, `assert!((x - 100.0).abs() < 0.5)`, or test
reported whole-number counters instead.

**Example tests that don't run.** Tests inside `examples/x.rs` only run
with `cargo test` if `Cargo.toml` has `[[example]] name = "x"` and
`test = true`.

**Old output files.** Each headless run deletes the files it makes
(`frame-NNNNNN.png`, `sheet.png`, ...) from its `--out` folder before
starting, and never touches anything else. So use a separate folder for
each kind of run, or older runs' pictures are replaced.

## 9. Does the game feel good?

You can't feel the game, but you can measure most of what makes it feel
good or bad. The numbers below are rules of thumb for a 320 x 240 canvas
at 60 updates per second.

### Speed

- **The player should cross the screen in about 1 to 2 seconds.** Slower
  feels sluggish; faster is hard to control. Breakout's paddle moves at
  240 px/s (1.3 s across); Dodge's player at 200 px/s (1.6 s).
- **Threats need to be visible long enough to react to.** People need
  about 0.25 s to react, and more to decide. Dodge's blocks start at
  90 px/s and take 2.7 s to fall the whole screen. Calculate it: distance
  divided by speed.
- **Measure, don't guess:** report positions or times, or step a
  `Simulation` and check `sim.game()`.

### Difficulty

Find out with bots and scripts, not with one run:

1. **Doing nothing should lose, but not instantly.** Dodge's play test
   checks that standing still loses within 30 seconds, and that the first
   hit comes after at least 3. If doing nothing *wins*, there's no game.
2. **A simple bot should be able to win or survive.** If a bot that can
   see everything can't win, a person can't either. Dodge's careful bot
   survives a minute; Breakout's bot wins a perfect game.
3. **Try many seeds.** One run can be lucky. Run the bot on seeds 1 to 10
   and look at the spread. For Dodge, the careful bot was first hit after
   1 to 3 minutes, and runs ended after 2.5 to 3 minutes. That's a good
   curve for a quick arcade game. A human is worse than a bot, so expect
   them to last a third to a half as long.
4. **It should get harder.** Check it with a test: speeds or spawn rates
   later in the game compared with the start.
5. **No unfair moments.** Look in `log.json` for bad events close
   together (two hits in under a second), deaths right at the start, or
   threats that appear on top of the player.

### Readability

Look at `sheet.png` shrunk small, the way a person glances at a screen:

- **Can you tell the player from everything else at a glance?** Use a
  color nothing else uses (Dodge: a yellow player, red blocks, a dark blue
  background).
- **Sizes:** moving things at least about 5 pixels; the player usually
  10 to 20. Text: scale 1 (5 px) for the score, 2 for messages, 3 to 4
  for titles.
- **Contrast:** light on dark or dark on light. Gray text on a dark
  background (`Color::GRAY`) is fine for hints, not for important things.
- **Nothing important covered:** the score and messages must never be
  drawn under game objects. Give the score its own strip.
- **Every state says what to do:** "PRESS SPACE TO START", "GAME OVER",
  "PRESS SPACE TO TRY AGAIN". A person seeing a frozen screen with no text
  thinks the game crashed.

### Feedback

Every important event should be visible and, ideally, impossible to
miss:

- **Getting hit:** a blink (Dodge: the player blinks for the safe second
  after a hit), a flash, or particles.
- **Scoring:** the score changes, and something happens where it happened
  (Breakout: particles fly off the broken brick).
- **Losing a life or the game:** a pause, a message, a clear change of
  scene.

Check feedback with close-ups or pixel counts on the frames right after
the event (from `log.json`, you know exactly which frame that is).

### Pacing and controls

- **Something should change every few seconds.** In `changes`, long gaps
  with nothing new mean boring stretches.
- **A round should be short.** For a small arcade game, 30 seconds to 3
  minutes, then a restart that takes one key press.
- **Controls:** support both the arrow keys and WASD (`axis(Key::Left,
  Key::Right) + axis(Key::A, Key::D)`), Space or Enter to start, and
  Escape to quit. Pause (P) is a kindness.

## 10. Showing your work to a person

A person can't read a log as fast as you can, but they can look at a
picture or a GIF in a second, even on a phone:

```sh
cargo run --release --no-default-features --example mygame -- --headless \
    --inputs playthroughs/good-run.json --sheet --gif --out show
```

Send them `show/sheet.png` (the whole run on one page) and `show/run.gif`
(about 20 pictures a second, small enough for a chat message). Tell them
what to look at: "the score goes up to 120 by frame 900, and the GIF shows
the new enemy at 0:12".

If they can run it, they can watch the same game in a window:
`cargo run --release --example mygame -- --inputs playthroughs/good-run.json`.
They can also send *you* a game: F10 in the window records it to
`recording-1.json`, and you replay it headless, exactly as they played it.

## 11. What the engine can't do yet

Plan around these. Don't spend time looking for them:

- Sprites can't rotate (only flip and scale).
- No sound.
- Physics boxes don't rotate (balls and axis-aligned boxes only).
- No running in a web browser.
- One built-in pixel font, capitals only.
- No gamepads.

For anything else, read the source: it's small, commented, and the
tutorial (`TUTORIAL.md`) explains every part.
