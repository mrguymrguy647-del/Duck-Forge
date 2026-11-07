# duckforge for AI assistants: building and testing games without a screen

**New here? Read [QUICKSTART.md](QUICKSTART.md) first** (one page). This
guide is the full reference: come back to it for details. If your agent
uses skills, [skills/duckforge/SKILL.md](../skills/duckforge/SKILL.md)
has the whole workflow in under 150 lines.

This guide is for an AI assistant (or anyone) building games with duckforge
on a machine with no screen, like a cloud Linux container. You can't watch
the game, so the engine lets you **run it headless, look at pictures of
it, read what it reports, and test it automatically**. Everything here has
been tried on such a machine: `examples/dodge.rs` was built by following
this guide, and the stories in it are real.

**Starting a new game? Copy `examples/template/`** (`tools/new_game.sh
mygame platformer`). It's a small, commented platformer: the level is
text read into a `TileMap`, the duck and tiles are Duck Quill sprite
sheets, the sky and hills are `Background` layers with parallax, and it
has a camera, sounds, a pause screen, reports, checks, play tests and a
bot that records `playthroughs/template.json`, all passing:
`cargo test --no-default-features --example template`.
For a top-down game (walls, chasing enemies, coins, 4 or 8 directions)
copy `examples/starter_topdown/`; for a shooter (fast bullets with
`swept_aabb`, enemy waves, particle explosions) copy
`examples/starter_shooter/`; for 3D, a three-lane runner (textured
blocks, a billboard duck, lighting, a HUD over the 3D) copy
`examples/starter_runner3d/`. All have labels, checks, a playthrough in
`playthroughs/` made by a bot, and play tests, and pass
`tools/verify.sh`. Section 3 has an even smaller template.

**Don't copy by hand: `tools/new_game.sh mygame topdown`** (or
`platformer`, `shooter`, `runner3d`, `tiny`) copies the starter (not its
tune file: the new game uses its code's values until you make one),
renames its main type (`TopDown` becomes `Mygame`), sets the window title,
points its bot at `playthroughs/mygame.json`, adds the `[[example]]` entry
to `Cargo.toml`, records a first playthrough with the bot, and runs
`verify`, which passes. On Windows: `tools\new_game.bat mygame topdown`.

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
11. [3D](#11-3d)
12. [What the engine can't do yet](#12-what-the-engine-cant-do-yet)

---

## 1. Quick facts

| Thing | Fact |
|-------|------|
| Canvas | 320 x 240 pixels by default (`Config::width`, `height`). The window shows it 3x bigger. |
| Coordinates | Pixels, `(0, 0)` is the **top-left**, **y grows downward**. `Rect::new(x, y, w, h)`: `x, y` is the top-left corner. |
| Time | `update` runs **60 times per second of game time**, always with `ctx.dt() == 1.0 / 60.0` (the default; `Config::timestep` changes it). Speeds are in pixels **per second**: multiply by `ctx.dt()`. |
| Units | 2D: **pixels**, y down (gravity about `vec2(0.0, 500.0)` to `1000.0` px/s²). 3D: **metres**, y up (gravity `vec3(0.0, -9.8, 0.0)`, a person is 1.8 tall). |
| Frames | Counted from **1**: `ctx.frame()` is 1 during the first `update`. Frame 600 = 10 seconds. Scripts and logs use the same numbers. |
| Randomness | Use `ctx.rng` only (`range`, `range_i32`, `chance`). Headless runs use seed 1 unless told otherwise, so **the same command gives the same result**. |
| Drawing | `draw(&self, canvas)` can't change the game. Start it with `canvas.clear(...)`. Later draws go on top. |
| Text | Built-in 3-pixel-wide font with every printable ASCII character (space to `~`: capitals, lowercase, digits, all punctuation). Anything else (accents, emoji) becomes `?`. At scale 1, capitals are 5 px tall (`Canvas::text_height(1)`), lines are 7 px apart (`Canvas::line_height(1)`), and `g j p q y` hang 1 px below. `'\n'` starts a new line. |
| Colors | `Color::rgb(r, g, b)` (solid), `Color::rgba(r, g, b, a)` and `color.with_alpha(a)` (a: 0 invisible to 255 solid; every drawing function blends), `Color::from_hex(0x14_14_28)`, constants `BLACK WHITE GRAY RED ORANGE YELLOW GREEN BLUE PURPLE`, `a.lerp(b, t)`. Build colors with these functions, never `Color { r, g, b }` (it has a 4th field, `a`). |
| Engine keys | In a window: F1 = debug overlay, F3 = FPS counter, F10 = record, F12 = screenshot. Escape quitting is each game's choice (`ctx.quit()`). |
| Output | `--headless` writes `last.png`, `last.scene.json` and `log.json` (plus what you ask for) to `duckforge-out/`, or the folder in `--out`. |
| Labels | `canvas.label("player", rect)` in `draw` names what's there (it draws nothing). Then `--describe` says in words what's on screen, every picture gets a `.scene.json` with each label's world and screen position, and the summary warns about labels never on screen, text off the edge, and labels overlapping on frame 1. Label your main objects. |

## 2. Set up on Linux

You need Rust 1.87 or newer, and nothing else. No X11, no graphics drivers,
no sound card, no system packages. (If `cargo` is missing:
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
then `. "$HOME/.cargo/env"`.)

Build and run **without the window, GPU and sound playback code**, with
`--no-default-features`. Without it, Cargo also builds the window library
(winit and softbuffer), the GPU library (wgpu) and the speaker code: a first build of
about a minute instead of about 11 seconds (on 4 cores), and a window
can't open on a server anyway. Sounds still work: headless runs save
them in `audio.wav`. After the first build, changing a line in your game
and building again takes under half a second.

```sh
cargo build --no-default-features --examples
cargo test --no-default-features               # about 250 tests, under a minute
cargo run --no-default-features --example breakout -- --headless --sheet --out check
```

The last command should print something like:

```text
duckforge: ran "Breakout - duckforge" for 600 frames (10.0 s of game time) in 0.05 s, seed 1
final: bricks_left = 60, lives = 3, score = 0, state = "Serving"
saved in check: last.png, last.scene.json, sheet.png, log.json, summary.txt
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

**Art from Duck Quill.** Small art can go straight in the code
(`Sprite::from_ascii`). For characters with animations, paint them with
Duck Quill (its `quill` program needs no window either): write the
painting script with an `animations` block, then

```sh
quill export art/hero.json --sheet --out assets   # assets/hero.png and assets/hero.json
```

and load the pair with `SpriteSheet::load("assets/hero.json")?`. Each
animation comes by its name, with its speed and whether it loops, and the
sheet has a **pivot**, the point that stands on the ground (the bottom
centre unless the script says otherwise). Draw by the pivot, so the
feet stay put whichever animation plays:
`sheet.draw(canvas, "walk", self.time, self.feet, Blit { flip_x: self.facing_left, ..Default::default() })`.
Export with `quill export`, not `quill render --zoom`: a zoomed picture
doesn't match its JSON, and `load` says so (`it's exactly 4 times too
big`). `--scale 2` exports are fine: the frames and pivot say so. Duck
Echo makes the sounds: a `.wav` for `Wav::load`, and loops for
`ctx.play_loop`.

**Good examples to read**, smallest first: `examples/hello.rs` (a square
you move), `examples/dodge.rs` (a complete small game with play tests and
a bot, built with this guide), `examples/breakout.rs` (a bigger game, with
a bot that makes recordings), `examples/physics.rs` (physics bodies),
`examples/world3d.rs` (3D shapes and a fly camera) and
`examples/starter_runner3d/` (a whole 3D game; see section 11).

## 4. The loop: write, run, look, read, test, fix

You can't play the game, so you work in a loop. Each turn takes seconds.

**The default check is one command:**

```sh
tools/verify.sh mygame          # tools\verify.bat mygame on Windows
```

```text
verify dodge
  build                        ok: built without a window
  play tests                   ok: 8 passed; 0 failed; 1 ignored
  dodge.json                   ok: 1844 frames; dodged = 60, hits = 3, lives = 0, seconds = 29, state = "GameOver"
  fuzz 50 runs                 ok: no problems found
VERIFY OK: dodge in 2 s (pictures and summaries in out/verify/dodge)
```

It builds the game headless, runs its play tests, replays every
`playthroughs/mygame.json` and `playthroughs/mygame-*.json` with a
contact sheet and the debug overlay, plays 50 random games (`--fuzz`;
`tools/verify.sh mygame 200` for more, `0` to skip it), and prints one line per step. A
failed step shows its first few lines of detail (the failing test, the
check that broke, the fuzz problem with its replay command) and the run
ends with `VERIFY FAILED` and exit code 1. Everything it saved is in
`out/verify/mygame/`: each playthrough's `sheet.png`, `last.png`,
`summary.txt` and `log.json`, and the fuzz scripts. Run it after every
change; the steps below are what it does, and how to dig into a failure.

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
crash? A panic is one line with the frame, the place and the message:
`duckforge: the game panicked on frame 88 at examples/mygame.rs:40:9: attempt to subtract with overflow`.
Run `--frames 87` to see the picture just before it. (Set
`RUST_BACKTRACE=full` to get Rust's full message and backtrace too.)

Every run prints a short summary, at most about 15 lines, and saves it
as `summary.txt`: the final reported values, the lowest and highest of
each number, the frames where text values changed, how many times each
sound played, failed checks, warnings, and where the files are. **Read it
first**; open pictures and `log.json` only when it raises a question.

```text
duckforge: ran "Dodge - duckforge" for 1844 frames (30.7 s of game time) in 0.57 s, seed 1, replaying 432 of 432 input events
final: dodged = 60, hits = 3, lives = 0, seconds = 29, state = "GameOver"
lowest to highest: dodged 0 to 60, hits 0 to 3, lives 0 to 3, seconds 0 to 29
state: "Playing" on frame 1, "GameOver" on 1784
sounds: beep x1, coin x6, explosion x1, hit x2
checks: 2 rules, none failed
saved in out/dodge: run.gif, last.png, last.scene.json, sheet.png, audio.wav, log.json, summary.txt
```

`--quiet` prints only the problems (`CHECK FAILED: ...` and
`warning: ...` lines), so no output means nothing went wrong. `--json`
prints the summary as one line of JSON (with `"ok": true` when there are
no failed checks or warnings) for a program to read. Errors are always one
line, naming the file and frame when there is one.

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

**Playthroughs say how they end.** A recording (F10, `--record`, or a
bot's `sim.recording()`) saves an `"expect"` block with the game's final
reported values: `"expect": {"delivered": 15, "lives": 0, "state": "Lost"}`.
A replay that ends differently (you changed a tune value, or code that
uses `ctx.rng`) prints `playthrough changed: delivered 15 to 9, state
"Lost" to "Won"`, and `verify` shows it as a warning with the command to
take the new ending: add `--accept` to the replay
(`--headless --inputs playthroughs/x.json --accept`) and it rewrites the
file's `"expect"`. Re-record instead if the inputs no longer make sense.
Only a replay of the script's own length is compared.

### Step 3b: Read the scene before looking at pictures

If the game labels its objects (`canvas.label("rock", rect)` in `draw`),
add `--describe` to the run. It prints the last frame in words, which is
often enough to check a change without opening an image:

```text
frame 600, camera 160,120 zoom 1
rock x4 at 45,213 264,141 302,70 222,-1
duck at 154,216 12x12
text: "TIME 10.0" at 4,4; "LIVES 3" at 289,4
```

`last.scene.json` (and `frame-NNNNNN.scene.json` for each snapshot) has
the same as JSON, with each label's world rect, screen rect and
`on_screen`. The summary also warns, naming the frame and the label,
about:

- a label that's off screen in every checked frame (the first, 16 spread
  over the run, every snapshot, the last) *and* outside the world:
  everything the camera showed, plus every label of a kind that was on
  screen at least once. An enemy far ahead in a long level is fine (label
  the level or its ground, or use a tile map, which labels itself); one
  at -500,0 that should be at 500,0 is not;
- text drawn partly off the screen;
- labels that overlap on frame 1 (things spawned on top of each other; a
  label entirely inside another, like an enemy inside a "level", doesn't
  count).

### Step 4: Look

Add `--debug` to see what the game *thinks* is there: hitboxes and zones
it marked with `ctx.debug_rect` / `ctx.debug_circle` are outlined, every
label gets a green outline with its name just outside it (above, or below
at the top of the screen, on a dark strip, never over the thing), and a
small box in the bottom-right corner shows the frame number, camera and
mouse. When a collision looks
wrong, this answers "is the hitbox where the sprite is?" at a glance.

Open, in this order:

1. **`sheet.png`**: the whole run on one page, 16 frames labelled with
   frame numbers and seconds. Look for: a screen that never changes (stuck
   game), things in the wrong place, text you can't read, objects drawn
   over the score, missing feedback (did anything show that the player was
   hit?). Evenly spread frames can skip the moment that matters, so for a
   playthrough use **`--sheet-events`** instead of `--sheet`: the frames
   where a reported value changed (text values like `state` first, then
   numbers like `lives` or `score`), plus the first and last, filled up
   evenly to 16, each with what changed under it (`LIVES 2`,
   `STATE LOST, LIVES 0`). A value that changes more than 20 times (a
   position, a timer) isn't an event and is ignored. `verify` uses it.
   Small sprites are hard to judge in a shrunk sheet: add
   **`--sheet-follow player`** (any label name) to also save
   `sheet-follow.png`, the same frames each cut to the third of the
   screen around that label and enlarged three times, so a 12-pixel
   character fills a good part of its cell.
2. **`last.png`**: how it ended.
3. **A close-up**, when you need to see exact pixels: is the ball touching
   the paddle, or one pixel above? `--crop X,Y,W,H --zoom 4` saves
   `closeup.png` of the last frame, and with `--snapshot-every N` also
   `closeup-000060.png` and so on for every snapshot.
4. **`run.gif`**: for motion. It's for people, mostly.

**Reading positions off a picture:** add `--grid 32` (any size from 2):
a faint line every 32 pixels, numbered along the bottom and left edges,
on `last.png`, the snapshots and the close-ups (not on the sheet or GIF,
which are shrunk). For a game with a `Camera2D`, `--grid-world 50` puts
the lines every 50 *world* units, through the camera, numbered with world
coordinates: the same numbers as the game's positions and the labels in
the scene files. The grid never covers the `--debug` box. The game's own picture, scenes and `--diff` baselines
aren't affected, but compare runs made with the same `--grid`.

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
  "files": ["run.gif", "last.png", "sheet.png", "log.json", "summary.txt"],
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

### Step 5b: Check the sounds

If the game played anything, `log.json` has a `"sounds"` list, one line
per sound with its frame and name (`ctx.beep` is `"beep"`, presets are
`"coin"`, `"jump"`, `"hit"`, `"explosion"`, your own `Sound` uses its
`name`, a `.wav` its file name):

```json
"sounds": [
  {"frame": 450, "sound": "hit", "wave": "square", "freq": 330, "slide_to": 70, "ms": 150, "volume": 0.4, "attack": 1, "decay": 120}
]
```

Line them up with `changes`: every hit should have a `"hit"` on the same
frame; a sound every frame (a `play` inside an `if is_down` instead of
`was_pressed`) shows up as hundreds of lines. `audio.wav` has them all
mixed, starting at sample `(frame - 1) * 735` at 60 updates per second.
With `--gif`, the run prints the `ffmpeg` command that joins `run.gif`
and `audio.wav` into a video for a person.

### Step 6: Write play tests

When something works, pin it down with a play test
([section 6](#6-play-tests)), so it stays working while you change other
things. When something's broken, write the test first, watch it fail,
then fix it.

### Step 6a: Fuzz it

Write down the rules your game must never break, with `ctx.check`, then
let the engine play it a few hundred times with random keys:

```sh
cargo run --release --no-default-features --example mygame -- --fuzz 200
```

```text
duckforge: seed 14: PANIC on frame 192: index out of bounds: the len is 0 but the index is 3 (examples/mygame.rs:20:22) -- replay: --inputs fuzz/seed-14.json (3 key presses, 193 frames); also seeds 30, 43, 49
duckforge: seed 2: CHECK "player on screen" failed on frame 336 -- replay: --inputs fuzz/seed-2.json (1 key press, 337 frames)
duckforge: fuzzed 200 runs of 30 s (seeds 1 to 200, keys Left,Right,Space, the ones the game reads) on 4 cores in 2.1 s: 2 different problems in 41 of the runs; scripts saved in fuzz
```

Each run uses the next seed and random taps and holds of the keys the
game reads: first, a few short practice runs press the keys found so far
and note every key the game asks about (`is_down`, `was_pressed`,
`axis`...), until no new one turns up (Escape and the engine's F keys are
left out). `--fuzz-keys Left,Right,Space` picks them instead. It reports, once per different problem, the first
seed and frame of: a **panic** (with the message and line), a **failed
check**, or a **freeze**: no reported value changed for 10 seconds
(`--freeze-seconds`) while keys the game reads were pressed (presses of
keys the game never asks about don't count). A freeze can be a stuck
game, or a menu waiting for a key the fuzzer didn't press. Tell the
engine when waiting is expected: call `ctx.idle_ok()` in `update` on
every frame of a title, pause or game-over screen, and those frames never
count towards a freeze. Every failing run's inputs are saved in `fuzz/` (or
`--out`), and one per problem is shrunk to the fewest key presses that
still cause it, so `--inputs fuzz/seed-14.json --sheet --debug` shows the
bug in a short replay. Runs go in parallel on every core; `--frames` sets
their length (1800, 30 seconds, by default). Fix, then fuzz again with
the same command: same seeds, same inputs.

"No problems found" only counts if the random player got somewhere, so
fuzzing ends with what the runs reached, added up over all of them:

```text
duckforge: coverage: bricks_left 47 to 60, lives 0 to 3, score 0 to 460
duckforge: coverage: state reached GameOver, Playing, Serving; never Won (where playthroughs/breakout-win.json ends); every run lasted all 1800 frames
```

Each reported number's lowest and highest, every text (or yes/no) value
reached, the values your playthroughs end on that no random run reached,
and how far the runs got (`furthest frame 900; 12 of 200 runs ended early`
when games quit or crash). Here, no random run ever won Breakout, or
cleared more than 13 bricks: a bug in the late game would go unnoticed.
Cover that part with a playthrough or a play test that starts there.
`verify` prints the same lines under its fuzz step.

### Step 6b: Tune without rebuilding

Speeds, gravity and timers that you'll adjust go through `ctx.tune`.
Then trying a value is one command, with no rebuild:

```sh
cargo run --no-default-features --example mygame -- --headless --inputs out/start.json \
    --set jump_speed=400 --set gravity=1100 --sheet --out out/try
```

When a value is right, put it in `examples/mygame.tune.json` (or run once
with `--write-tune` to create the file with every value, then edit it).
The summary prints which values came from the file or `--set`, and warns
about a `--set` name the game never asked for (usually a typo), and
about names in the tune file it never asked for (often left over from a
copied starter: delete them, or a value like `sight` may quietly differ
from the code's default).

### Step 7: Fix, and go round again

Change one thing at a time, and rerun the same command. Then check that
the change did only what you meant, in one command:

```sh
cargo run --no-default-features --example mygame -- --diff out/before out/after
```

```text
frame-000015.png: 184 pixels changed (0.2%), in 40,146 10x30; see diff/frame-000015.png
last.png: 184 pixels changed (0.2%), in 40,112 10x53; see diff/last.png
unchanged: frame-000030.png
last.scene.json: player moved 40,151 to 40,112
reported at the end: score 10 to 12
```

It compares every picture both runs saved (pixels changed, and the box
around them), the scene files (labels that moved at least a pixel on
screen, appeared or disappeared) and the final reported values. Changed pictures get a copy
in `out/after/diff/` with the changed pixels in magenta. Keep a "before"
folder by running once with `--out out/before` before you change the
code. Because runs are
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
  long to run when `--frames` isn't given). Without `"frames"`, a replay
  runs until two seconds after the last event, so a hand-written script
  is never cut short. If a run stops before some events anyway (a short
  `--frames` or `"frames"`), the summary warns, naming the file and how
  many events were skipped, and `verify` fails.
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
  with `"frames"` runs that long, one without runs until two seconds after
  its last event, and with no script it's 600.
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
`fn draw(&self, canvas: &mut Canvas)`. Optional:
`fn init(&mut self, ctx: &mut Context)` (once, before the first update;
reports and sounds from it count as frame 1) and
`fn resize(&mut self, ctx: &mut Context, width: f32, height: f32)` (the
canvas changed size: only with `Config { resizable: true, .. }` in a
window, or `sim.resize(w, h)` in a test).

**`Config`** (use `..Config::default()` for the rest): `title`, `width`,
`height`, `scale` (window size multiplier, 3), `target_fps` (60),
`show_fps`, `renderer`, `timestep` (`Timestep::Fixed { updates_per_second: 60 }`),
`seed` (`None`), `resizable` (`false`: the canvas keeps its size),
`debug_corner` (`Corner::BottomRight`; also `TopLeft`, `TopRight`,
`BottomLeft`).

**`Context`** (`ctx`): `ctx.dt()` (seconds per update), `ctx.time()`,
`ctx.frame()`, `ctx.width()` / `ctx.height()` (as `f32`), `ctx.quit()`,
`ctx.report(name, value)`, `ctx.input`, `ctx.rng`,
`ctx.lock_pointer(true)` (hide the mouse pointer and hold it in the window,
for first-person mouse look: read `mouse_delta()`, which keeps counting;
`false` gives it back; give the player a way out, like Escape),
`ctx.pointer_locked()`.

**`ctx.input`**: `is_down(key)`, `was_pressed(key)` (one frame per
press), `was_released(key)`, `axis(Key::Left, Key::Right)` (-1, 0 or 1),
`mouse_position()` (an `Option<Vec2>`: `None` until the mouse has been over
the window or a script moved it; `if let Some(mouse) = ...`),
`mouse_delta()` (canvas pixels moved this frame, right
and down positive; the one for mouse look), `is_mouse_down(button)`,
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
`Canvas::text_width(text, scale)` (widest line), `Canvas::text_height(scale)`
(a capital's height), `Canvas::line_height(scale)` (from one line's top to
the next; `'\n'` in `draw_text` uses it), `width()`, `height()`. Any color with
alpha below 255 is drawn see-through (`clear` ignores alpha).

**Sprites**: `Sprite::from_ascii(&["..yy", "yyyo"], &[('y', Color::YELLOW), ('o', Color::ORANGE)])`
(one character per pixel, `'.'` see-through; draw small art straight in
code, no files needed), `Sprite::load_png(path)?` (returns
`Result<Sprite, LoadError>`; the error prints as one sentence),
`Sprite::from_pixels(w, h, vec_of_colors)`. `sprite.width()`,
`height()`, `get_pixel(x, y)` (an `Option<Color>`; `.a` is 0 where
see-through), `sub(x, y, w, h)` and `frames(w, h)` (cut a sheet, no
copying). Draw with
`canvas.blit_sprite(&sprite, top_left, Blit { flip_x, flip_y, scale, tint, opacity, ..Default::default() })`
or `Blit::default()`; scaling is nearest-neighbour. Animations:
`Animation::new(frames, fps)`, `anim.frame_at(seconds)` (loops, unless
`anim.looping = false`: then it stays on the last frame),
`index_at(seconds)`, `duration()`.

**Sprite sheets** (from Duck Quill's `quill export`):
`let sheet = SpriteSheet::load("assets/hero.json")?;` then
`sheet.draw(canvas, "walk", seconds, feet, Blit::default())` (draws the
animation's frame at that time with its pivot on `feet`, a `Vec2`; with
`flip_x` the pivot is mirrored too). `sheet.animation("walk")` (an
`&Animation`; a misspelt name panics, listing the real ones),
`sheet.get("walk")` (an `Option`), `names()`, `pivot()`, `frame_size()`,
`scale()`, `image()`. Any sprite by a pivot:
`canvas.draw_at_pivot(&sprite, at, pivot, blit)`.

**Tile maps** (a level as text, one letter per tile; `examples/template/`
uses one): `let legend = Legend::new().solid('#', "grass").one_way('=', "plank").decoration('~', "water").spawn('p', "player");`
(`.` and space are empty), then
`let map = TileMap::parse(LEVEL, 16.0, &legend)?;` (or
`TileMap::load("levels/1.txt", 16.0, &legend)?`; a wrong letter is an
error with its row and column). `map.spawn("player")` (the bottom centre
of its tile; panics naming the spawns there are if missing),
`map.spawns("coin")` (a `Vec<Vec2>`). Every update:
`let touched = map.move_and_slide(&mut body_rect, velocity * ctx.dt());`
then `touched.ground`, `touched.ceiling`, `touched.wall_left`,
`touched.wall_right` (`Contacts`); it moves in steps of at most half a
tile, so nothing passes through a wall, and lands on one-way tiles only
from above. In `draw`, between `push_camera` and `pop_camera`:
`map.draw(canvas, &tiles, self.time)` (only the tiles on screen;
`self.time` animates water; it labels the map `"map"`). Also
`map.bounds()`, `columns()`, `rows()`, `tile_size()`,
`cell_at(point)` (column and row), `tile_rect(column, row)`,
`tile_at(column, row)` (`Option<(&str, TileKind)>`), `kind_at(point)`
(`TileKind::Empty`, `Solid` or `OneWay`), `clear_tile(column, row)`,
`set_tile(column, row, "brick", TileKind::Solid)`.

**Tilesets**: `Tileset::load("assets/tiles.json")?` (a Duck Quill sprite
sheet, one animation per tile, named like the legend's tiles),
`Tileset::from_sheet(&sheet)`, `Tileset::grid(&sprite, 16, 16, &["grass", "", "plank"])`
(cut left to right, top to bottom; `""` skips one), or `Tileset::new()`
then `tiles.insert("lava", animation)`. `tiles.get(name)`, `names()`,
`frame(name, seconds)`, `tiles.tile(name)` (the tile's picture, sharing
the atlas's pixels: as a mesh texture it repeats on its own past texture
point 1, for the sea, clouds or a floor), `tiles.uv_rect(name)` (where the tile is in the
atlas picture, as texture points from 0 to 1, for your own meshes). Any
sprite: `sprite.whole_picture()` (the atlas it was cut from) and
`sprite.place_in_picture()` (`[left, top, width, height]` in pixels).

**Backgrounds**: `let sky = Background::new(sprite).parallax(0.2);`
(0 stays on the screen, 1 moves with the world; both directions),
`.parallax_xy(0.5, 0.1)` (its own amount sideways and up and down),
`.offset(vec2(0.0, 120.0))`, `.repeat_y(true)`, `.repeat_x(false)`. In
`draw`, first: `canvas.draw_background(&sky, &self.camera)` (in screen
pixels, no seams, not zoomed). `canvas.visible_area()` is the part of
the world on screen right now (inside a camera, in world units).

**Camera** (`Camera2D`): `Camera2D::new(screen_w, screen_h)` starts with
world = screen. Fields `position` (world point at screen center), `zoom`
(1.0), `screen_size`. `camera.follow(target, smoothing_seconds, ctx.dt())`
(0.0 snaps, 0.2 smooth), `camera.shake(pixels, seconds)` then
`camera.update(ctx)` every update (uses `ctx.rng`, so replays match),
`is_shaking()`, `screen_to_world(p)` (the mouse: pass
a point from `ctx.input.mouse_position()`), `world_to_screen(p)`, `visible_area()`.
In `draw`: `canvas.push_camera(&self.camera)`, draw the world,
`canvas.pop_camera()`, then draw the HUD in screen pixels.
`canvas.push_offset(v)` / `pop_offset()` shift a group of drawings.
Lines stay 1 px at any zoom; `clear`, `get_pixel` and 3D ignore the
camera; the engine resets the camera stack every frame.

**Math**: `vec2(x, y)` / `Vec2` (`+ - * f32`, `length()`, `normalized()`,
`distance(other)`, `dot(other)`, `Vec2::ZERO`).
`Rect::new(x, y, w, h)`, `Rect::from_center(center, w, h)`, `left()`,
`right()`, `top()`, `bottom()`, `center()`, `top_left()`, `top_right()`,
`bottom_left()`, `bottom_right()` (corners as `Vec2`), `overlaps(&other)`,
`contains(point)`, `inset(by)`, `expand(by)`, `intersection(&other)`
(`Option<Rect>`). `vec3(x, y, z)` / `Vec3` for 3D (y is up).

**Collisions and helpers** (all in the prelude):
`circle_vs_rect(center, radius, rect)`, `circle_vs_circle(a, ra, b, rb)`
(touching doesn't count, like `overlaps`),
`swept_aabb(rect, movement_this_frame, other)` gives `Option<Hit>` with
`hit.time` (0 to 1 through the move) and `hit.normal` (which way the
surface faces). Use it for anything moving more than its own size per
frame (bullets), or it passes through thin walls.
`approach(current, target, step)` (never overshoots; use
`step = rate * ctx.dt()`), `remap(value, (from_a, from_b), (to_a, to_b))`
(not clamped).

**Tuning** (change numbers without rebuilding):
`let jump = ctx.tune("jump_speed", 330.0);` gives `--set jump_speed=400`
if given, else the tune file's value, else the default. The tune file is
`examples/<game>.tune.json` (a JSON object of numbers, like
`{"jump_speed": 400}`; found by the program's name, also in `cargo test`)
or `--tune FILE`. `--write-tune` writes every name the game asked for,
with its value. Read all tune values at the top of `update`: a name is
only known once the code reading it runs. `log.json` has
`"tune": {"jump_speed": {"value": 400, "from": "set"}, ...}`. In play
tests: `PlayTest::new(..).tune("jump_speed", 400.0)` or
`.tune_file("x.json")`. A window re-reads the file when it changes.
To name the file yourself (so a bot in a test program with another name
reads it too): `Config { tune_file: Some("examples/mygame.tune.json".into()), ..Default::default() }`;
a named file that doesn't exist yet just means the code's values.
Recordings store the tune values they were made with. Replayed with
other values, a playthrough that still ends as expected is a warning
(`playthrough tuned: ... (speed 90 to 120)`), so you can tune without
recording again; one that ends differently fails and names the changed
values as the likely cause. `--accept` stores the new ending and values.
The summary's second line says which values aren't the code's:
`TUNED from examples/mygame.tune.json: speed = 120 (code says 90)`.

**Checks** (rules that must always hold): in `update`,
`ctx.check("player inside level", level.contains(player.center()))`.
The first frame a rule fails is recorded, once per name; it never stops
the game. `sim.failed_checks()` lists them (each `name`, `frame`); the
summary prints `CHECK FAILED`, `log.json` has
`"checks": {"rules": 2, "failed": [{"name": ..., "frame": ...}]}`, the
debug overlay shows them in red, and a `PlayTest` **fails** if any check
failed (add `.ignore_checks()` when a failure is expected).
`ctx.idle_ok()` (call it every frame the game is waiting on purpose:
title, pause, game over) stops `--fuzz` calling that a freeze.

**Reports**: `ctx.report("name", value)` with a number (any integer or
float type), a `bool`, a `&str` or a `String`. Enums: report
`format!("{:?}", self.state)`.

**Sound**: `ctx.beep(freq_hz, ms)`, `ctx.play(Sound::coin())` (also
`jump()`, `hit()`, `explosion()`), or your own:
`ctx.play(Sound { name: "laser", wave: Wave::Square, freq: 1200.0, slide_to: 200.0, ms: 150.0, volume: 0.3, ..Default::default() })`.
`Wave::Square` / `Sine` / `Triangle` / `Noise`; `attack` and `decay` are
fade in/out in ms; `slide_to: 0.0` means no slide. Files:
`let w = Wav::load("assets/x.wav")?;` (8/16/24/32-bit or float, any rate,
mono or stereo) then `ctx.play_wav(&w)`. Music and other repeating
sounds: `let music: LoopId = ctx.play_loop(&w, 0.6);` (a `Wav`, or a
`Sound` passed as it is; it repeats with no gap until stopped),
`ctx.set_loop_volume(music, 0.3)`, `ctx.stop_loop(music)`; keep the
`LoopId` in your game (`Option<LoopId>`). Play sounds in `update` only
(`draw` has no `ctx`). Checking: `sim.sounds()` (each has `frame` and
`name`), `sim.sound_frames("coin")`, and in play tests
`played.assert_sound("coin", frame, within_frames)`; loops with
`sim.loops()` (each has `name`, `start`, `stop`, `volumes`,
`.volume_at(frame)` and `.is_playing()`). Headless runs mix loops into
`audio.wav` and list them in `log.json` under `"loops"`.

**Physics** (`use duckforge::physics::{Body, BodyId, PhysicsWorld, Shape};`):
`PhysicsWorld::new(vec2(0.0, 400.0))` (gravity, y down),
`world.add(Body::ball(position, radius))` or
`Body::block(center, size)`, then `.fixed()`, `.with_velocity(v)`,
`.with_bounce(0.5)`, `.with_friction(0.6)`, `.with_mass(m)`. In `update`:
`world.step(ctx.dt())`. Read bodies with `world.get(id)` (`position`,
`velocity`, `shape`), `world.bodies()`, `world.contacts()`,
`world.touching(a, b)`, `world.remove(id)`. Boxes don't rotate. Works the
same in 3D with `Vec3`.

**3D** (section 11 explains it): `Camera3D::new(position)` (looks along
+z) or `Camera3D::looking_at(position, target)`, `camera.fly(ctx, speed)`,
`camera.project(point, canvas)` (`Option<(Vec2, f32)>`: the screen
point and its depth) and `camera.world_to_screen(point, canvas)`.
Meshes (1 unit big, centred): `Mesh::cube(color)`, `Mesh::sphere(color,
segments)`, `Mesh::pyramid(color)`, `Mesh::cylinder(color, segments)`,
`Mesh::cone(color, segments)`, `Mesh::quad(width, height)` (standing,
facing -z), `Mesh::checkerboard(tiles, tile_size, a, b)`,
`Mesh::block(&atlas, ["grass_top", "grass_side", "dirt"])` (top, sides,
bottom from one `Tileset`). Block worlds: `BlockPalette::new(&atlas,
&[BlockKind::new("stone", ["stone"; 3]), BlockKind::new("glass",
["glass"; 3]).see_through(), BlockKind::new("leaves", ["leaves";
3]).inner_faces(), BlockKind::new("lamp", ["lamp"; 3]).glowing()])`
(blocks numbered from 1; `AIR` is 0), `palette.id("stone")`,
`BlockWorld::new(width, height, depth)` (chunks of 16 x 16 columns),
`world.set(x, y, z, id)`, `world.get(x, y, z)`, `world.set_below(id)`,
`world.update_meshes(&palette)` in `update` (builds only the chunks that
changed; `update_meshes_shaded(&palette, |world, x, y, z| light)` darkens
faces), `world.draw(canvas, &camera)`, `world.chunk_mesh(cx, cz)`. Build your own with `add_vertex`,
`add_triangle`, `add_quad`, `add_triangle_uv(a, b, c, [uv; 3], color)`,
`add_quad_uv(a, b, c, d, [uv; 4], color)` (corners clockwise seen from
the front). `.with_texture(sprite)` (colours times the texture; white
shows it as painted), `.glowing()` (ignores the light),
`.with_color(color)`. `chunk.merge(&mesh, &transform)` (many meshes in
one; two different textures panic: use one atlas; texture points come
along, so merging parts first and calling `.with_texture` once works;
a plain part merged into a textured mesh stays plain), `mesh.bounds()`
(lowest and highest corner).
`Transform::at(position).rotated(vec3(x, y, z)).scaled(vec3(x, y, z))`
or `.sized(scale)`. In `draw`: `canvas.draw_mesh(&mesh, &transform,
&camera)`. `canvas.draw_mesh_tinted(&mesh, &transform, &camera,
Color::rgba(255, 120, 120, 255))` (colours times the tint; its alpha is
how solid: 255 solid, 128 half see-through; see-through meshes don't hide
what's drawn after them, so draw them after the solid world).
First-person hands and held things:
`canvas.draw_mesh_in_view(&mesh, &transform, &camera)` (the transform is
relative to the camera: x right, y up, z ahead; it stays put on the
screen, is lit by the world's sun, and is drawn in front of the 3D drawn
before it, so draw it after the world). Light: `canvas.set_lighting(Lighting { sun_direction,
sun_color, sun_strength, ambient })` (or `..Lighting::default()`, the
light every canvas starts with), `canvas.lighting()`. Sprites in 3D:
`canvas.draw_billboard(&sprite, feet, height, &camera)`, or
`canvas.draw_billboard_with(&sprite, feet, height, &camera, Billboard {
pivot: Some(sheet.pivot()), flip_x, face_camera, })`. Headless runs draw
3D on the CPU unless `DUCKFORGE_RENDERER=gpu`;
`Simulation::with_renderer(config, game, Renderer::Gpu)` compares.

**Headless from code**: `duckforge::run_headless(config, game, &HeadlessOptions { frames, out, inputs, sheet, gif, ..Default::default() })`,
and `Simulation::new(config, game)` with `press`, `release`,
`move_mouse`, `move_mouse_by(vec2(dx, dy))` (a locked pointer's movement),
`press_mouse`, `release_mouse`, `scroll`, `step`,
`run(frames)`, `game()`, `picture()`, `recording()`, `follow(script)`,
`resize(w, h)`. Scripts: `InputScript::load(path)` or its other name
`InputScript::from_file(path)`; `InputScript::parse(text)` or
`InputScript::from_json(text)`; `script.save(path)`.

**Command line**: `--headless`, `--frames N`, `--out DIR`, `--seed N`,
`--inputs FILE`, `--snapshot-every N`, `--sheet`, `--gif`,
`--video FILE` (every frame, sharp, 1080p, with sound; needs ffmpeg),
`--crop X,Y,W,H`, `--zoom N`, `--grid N` (a coordinate grid on saved
pictures), `--grid-world N` (the same in world units, through the
camera), `--describe`, `--debug`, `--debug-corner top-right` (where
the overlay's box goes; bottom-right by default), `--tune FILE`,
`--set NAME=N` (repeatable), `--write-tune`, `--accept` (with
`--inputs FILE`: store this ending as the file's `"expect"`), `--record FILE` (window
only), `--diff OLD_DIR NEW_DIR` (compare two runs; any example runs it,
the game isn't started), `--fuzz N` (presses the keys the game reads, or
`--fuzz-keys Left,Right,Space`; `--freeze-seconds S`; see section 4), `--help`.
They go after `--` with `cargo run`: `cargo run --example x -- --headless`.

**Labels and scenes**: in `draw`, `canvas.label("name", rect)` (draws
nothing; inside `push_camera` the rect is in the world). After a step,
`sim.scene()` gives a `Scene` with `frame`, `camera` (`position`, `zoom`),
`labels` (each `name`, `world`, `screen`, `on_screen`), `texts` (each
`text`, `screen`), `scene.labels_named("rock")` and `scene.describe()`
(the same words as `--describe`).

**Debug overlay**: `ctx.debug_rect(rect)` and `ctx.debug_circle(center, r)`
in `update` (world coordinates; only the latest update's shapes are
shown). `--debug` draws them into every headless picture (`last.png`,
snapshots, `sheet.png`, `run.gif`), plus a small see-through box in the
bottom-right corner: `FRAME 600 FPS 60` and
`CAM 685,600 X1 MOUSE 565,530` (the mouse in world coordinates). Move the
box with `Config { debug_corner: Corner::TopRight, .. }` or
`--debug-corner top-right` if the game has a HUD there. In code:
`HeadlessOptions { debug: true, .. }` or `sim.set_debug_overlay(true)`.

**Errors**: `Sprite::load_png` and `Wav::load` return
`Result<_, LoadError>`; a `LoadError` prints as one sentence (with the full
path it looked for), even when `main` returns it with `?`.

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

**Text that becomes `?`.** The font has every printable ASCII character,
but nothing else: `é`, `£`, `→` and emoji become `?`. `draw_text_centered`
centers the text horizontally; you choose `y` (for vertical centering,
`y = middle - Canvas::text_height(scale) / 2.0`). Lowercase is only 4
pixels tall, so at scale 1 it's harder to read than capitals: use scale 2,
or capitals, for anything important.

**Reporting, playing sounds or marking debug shapes in `draw`.** You
can't: `draw` gets no `ctx`. Do all three in `update`.

**The score drawn inside the camera.** Everything between `push_camera`
and `pop_camera` moves and shakes with the world. Draw the HUD after
`pop_camera`, in screen pixels.

**A sound inside `if is_down(...)`.** It plays again every frame the key
is held: `log.json` shows the same sound on dozens of frames in a row.
Use `was_pressed` for sounds.

**Effects that use `ctx.rng` change the game.** Screen shake (and any
random particles you make with `ctx.rng`) take random numbers, so every
random event after them comes out differently. Runs stay repeatable, but
adding a shake can change where the next enemy appears; re-check
balance tests after adding effects.

**Reporting a value that changes all the time.** `dodge` first reported
how many blocks were on screen. It changed every few frames, and buried
the interesting lines in `log.json`. Report things a test or a person
cares about. Counters of events (`hits`, `dodged`) are better than
values that churn (`blocks_on_screen`).

**Reports that don't show what the player does.** The quick start's
Catch first reported only its score. Moving the paddle while stars fell
past changed nothing in `log.json`, so `--fuzz` called it a freeze, and
a bug that stopped the paddle would have been invisible too. Report the
player's position (or whatever their keys change), as well as the score.

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

For a real video, add **`--video show/run.mp4`**: every frame at the
game's full frame rate, each pixel a sharp square (the game is enlarged
by the biggest whole number that fits in 1920 x 1080, with black bars),
with the sound. It needs `ffmpeg`; without it the frames are saved as
PNGs and the exact command to join them is printed.

If the game makes sounds, the run also saves `show/audio.wav` and, with
`--gif`, prints an `ffmpeg` command that joins the GIF and the sound into
`run.mp4`, in step (a GIF has no sound). `examples/dodge.rs` with
`--inputs playthroughs/dodge.json` is a complete example: 30.7 seconds of
play, 10 sounds, and a video whose picture and sound are both exactly
30.7 seconds long.

If they can run it, they can watch the same game in a window:
`cargo run --release --example mygame -- --inputs playthroughs/good-run.json`.
They can also send *you* a game: F10 in the window records it to
`recording-1.json`, and you replay it headless, exactly as they played it.

## 11. 3D

**Units and directions.** 1 unit is about a metre; x is right, **y is
up**, z is forward (away from a camera with no turn). The built-in
meshes are 1 unit big and centred on (0, 0, 0), so `Transform::at(p)
.sized(2.0)` is a 2-metre one at `p`. Gravity is about `-9.8` on y; in a
game, choose what feels right (the runner starter uses 18 with a jump
speed of 6: one unit high).

**The camera.** `Camera3D::looking_at(position, target)` is usually all
you need: to follow a player from behind and above, set it every frame
from the player's position (`starter_runner3d`'s `camera()`).
`camera.project(point, canvas)` gives where a world point is on screen
and how far along the view (`None` behind the camera): use it for a
health bar over a character, a label, or sorting.

**Meshes.** Built in: cube, sphere, pyramid, cylinder, cone, quad and
checkerboard. Make a shape from parts with `merge` (each part placed by a
transform), and draw it as one mesh: one draw instead of dozens. Every
triangle is one flat colour times the light (flat shading).

**Textures and atlases.** `mesh.with_texture(sprite)` paints a picture on
it, where its texture coordinates say (every built-in shape has them: a
cube shows the whole picture on each face, a checkerboard on each
square). Points beyond 1 repeat the picture: a long road is one quad
whose coordinates go from 0 to 40 along it. Pixels stay sharp squares;
see-through pixels aren't drawn (a fence has holes). **A mesh has one
texture**, so put many pictures in one *atlas*: a Duck Quill sheet with
one 16 x 16 animation per tile (`quill export blocks.json --sheet`),
loaded with `Tileset::load`. `Mesh::block(&atlas, [top, sides, bottom])`
makes a block from three tiles of it.

**Block worlds: `BlockWorld`.** Drawing 10,000 blocks one by one is slow
on both renderers, and most of their faces are hidden by the next block.
`BlockWorld` keeps a grid of blocks and draws it as one mesh per chunk
(16 x 16 columns, the world's full height) with only the faces next to
air or a see-through block. Say how each kind looks once, with tiles from
one atlas:

```rust
// Art, made once (blocks are numbered from 1 in this order; AIR is 0):
let palette = BlockPalette::new(&atlas, &[
    BlockKind::new("grass", ["grass_top", "grass_side", "dirt"]),
    BlockKind::new("glass", ["glass"; 3]).see_through(),
]);
let grass = palette.id("grass");
// The world, which needs no art (tests can build one without pictures):
let mut world = BlockWorld::new(64, 32, 64);
world.set(x, y, z, grass);
// In update, after digging or building (only the changed chunks are rebuilt):
world.update_meshes(&palette);
// In draw:
world.draw(canvas, &camera);
```

`set` marks the block's chunk, and the chunk next door when it's on the
edge, so `update_meshes` rebuilds one or two chunks after a dig. Chunks
are kept separate rather than one world-sized mesh: before drawing a
mesh, the engine checks a ball around it against the edges of the view
and skips it if it's entirely outside (*frustum culling*), so chunks
behind the camera cost nothing. `update_meshes_shaded(&palette, |world,
x, y, z| brightness)` darkens each face by the cell it looks into (Duck
Craft makes caves darker this way). To build your own meshes from the
atlas instead, `tiles.uv_rect(name)` says where a tile is, and
`chunk.merge(&Mesh::block(..), &transform)` still works for small things.
(Measured: 4,096 textured blocks in 16 chunks take about 5 ms a frame on
the CPU at 320 x 240.)

**Light.** `canvas.set_lighting(Lighting { sun_direction: vec3(-0.5, 1.0,
-0.3), sun_color: Color::from_hex(0xfff0d8), sun_strength: 1.0, ambient:
0.55 })` in `draw`, before the meshes: `sun_direction` is where the light
comes *from*; `ambient` is how bright faces turned away from the sun are
(0.35 by default; higher looks softer). It stays until changed.
`.glowing()` meshes ignore it (lava, lamps, screens). There are no
shadows.

**Sprites in 3D (billboards).** `canvas.draw_billboard(&sprite, feet,
height, &camera)` stands a sprite in the world, `height` units tall, its
feet at `feet`, turned towards the camera and upright. It's
depth-tested like a mesh (a crate in front hides it), its see-through
pixels show what's behind, and it isn't shaded (it looks as painted). For
an animated character: `let frame = sheet.animation("run").frame_at(t);`
and `Billboard { pivot: Some(sheet.pivot()), flip_x, ..Default::default() }`.
`face_camera: true` turns it fully towards a camera that looks down
(sparks, coins); upright is better for characters.

**The HUD over 3D.** Draw 3D first, then 2D: the score, the lives, a
pause screen. 2D drawn before the 3D (a sky) ends up behind it. Labels
for tools (`canvas.label`) are in screen pixels: find them with
`camera.project` (`starter_runner3d` labels the duck and the next crate
in each lane). Label only what matters: far-off things overlap on screen
in perspective, and the summary would call that "spawned on top of each
other".

**The CPU and the GPU.** In a window, 3D goes to the graphics card if
there is one; headless runs always use the CPU, so pictures are the same
on every computer. Both give the same picture (tests compare them: at
most a few edge pixels differ, by a shade). To see the GPU's pictures
headless, set `DUCKFORGE_RENDERER=gpu` (Linux without a graphics card:
install Mesa's `mesa-vulkan-drivers`, a software one).

**How much it can draw.** On the CPU at 320 x 240, about 90,000
triangles take 8 ms a frame (`stress3d`, 2,000 objects), so a few hundred
thousand triangles a frame is the practical limit; textures add about
20% where they cover the screen. Bigger canvases cost more in
proportion to their pixels. A graphics card draws far more; check with
`cargo run --release --example stress3d -- --objects 20000`.

**Mistakes to avoid in 3D:**

- Corners in the wrong order: a triangle's corners go **clockwise seen
  from the front**, or it's invisible from where you look (only fronts
  are drawn). If a face is missing, swap two corners.
- Merging meshes with different textures panics: give them one atlas.
- A camera inside a mesh sees nothing of it (only fronts are drawn,
  and its fronts face out). With the target at the camera's own
  position, `looking_at` just looks along +z; keep them apart.
- Reports and labels from `draw`: reports belong in `update`; labels
  in `draw`, in screen pixels.

## 12. What the engine can't do yet

Plan around these. Don't spend time looking for them:

- Sprites can't rotate (only flip and scale).
- One-off sounds can't be stopped or changed once started (loops can:
  `play_loop`, `set_loop_volume`, `stop_loop`). No pitch changes while
  playing, and no crossfades beyond changing volumes yourself.
- No stereo panning: every sound is mono.
- Physics boxes don't rotate (balls and axis-aligned boxes only).
- No running in a web browser.
- One built-in pixel font (ASCII only), and no other fonts.
- No gamepads.
- 3D: no shadows, no smooth (per-pixel) lighting, no fog, one texture
  per mesh, no model files (build shapes in code, or merge the built-in
  ones), no animated 3D models (use billboards for characters).

For anything else, read the source: it's small, commented, and the
tutorial (`TUTORIAL.md`) explains every part.
