# duckforge quick start

duckforge is a small Rust game engine for 2D (and some 3D) games. Every game also runs **headless**:
no window, as fast as possible, saving pictures, sound and a log you can read. This page is all you need to
make a game. The full reference is [AI_GUIDE.md](AI_GUIDE.md); [TUTORIAL.md](../TUTORIAL.md) teaches Rust.

## Build without a screen

```sh
cargo test --no-default-features        # everything still works?
cargo run --no-default-features --example dodge -- --headless --sheet --out out/dodge
```

On a server always pass `--no-default-features`; game options go after `--`.

## A complete game

A game is a struct with two methods: `update` (move things, 60 times a second; `ctx` has the time,
input, random numbers, sound and reports) and `draw` (paint the canvas; it can't change the game).
Save this as `examples/catch.rs` (it's `examples/quickstart.rs`):

```rust
use duckforge::prelude::*;

/// Catch the falling star with the paddle.
struct Catch {
    paddle: Rect,
    star: Vec2,
    score: u32,
}

impl Game for Catch {
    fn update(&mut self, ctx: &mut Context) {
        let speed = ctx.input.axis(Key::Left, Key::Right) * 150.0;
        self.paddle.x = (self.paddle.x + speed * ctx.dt()).clamp(0.0, 288.0);
        self.star.y += 90.0 * ctx.dt();
        let caught = circle_vs_rect(self.star, 4.0, self.paddle);
        if caught {
            self.score += 1;
            ctx.play(Sound::coin());
        }
        if caught || self.star.y > 240.0 {
            self.star = vec2(ctx.rng.range(10.0, 310.0), 0.0);
        }
        ctx.report("score", self.score);
        ctx.report("paddle_x", self.paddle.x);
    }

    fn draw(&self, canvas: &mut Canvas) {
        canvas.clear(Color::BLACK);
        canvas.fill_rect(self.paddle, Color::WHITE);
        canvas.fill_circle(self.star, 4.0, Color::YELLOW);
        let score = format!("SCORE {}", self.score);
        canvas.draw_text(&score, vec2(4.0, 4.0), 1, Color::WHITE);
    }
}

fn new_game() -> Catch {
    let paddle = Rect::new(144.0, 220.0, 32.0, 6.0);
    let star = vec2(160.0, 0.0);
    Catch {
        paddle,
        star,
        score: 0,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(Config::default(), new_game())
}
```
Bigger starts: `tools/new_game.sh mygame topdown` makes `examples/mygame/` from a starter (`platformer`,
`topdown`, `shooter`, `runner3d`, `tiny`) and verifies it. Built in (guide 7, 11): tile maps, levels as
files, backgrounds, Quill sheets, music, 3D textures, billboards, block worlds, fog, mouse look.

## The loop

1. **Write** a little.
2. **Run headless**, with inputs if the game needs them:
   `cargo run --no-default-features --example catch -- --headless --inputs out/start.json --sheet-events --out out/catch`.
   An input script is `{"events": [{"frame": 10, "press": "Right"}, {"frame": 70, "release": "Right"}]}`.
3. **Read the summary** it prints (also in `summary.txt`; `--quiet` prints only problems). `log.json`
   has every `ctx.report` change and sound. Name things in `draw` with `canvas.label("star", rect)`,
   and add `--describe` to get the last frame in words.
4. **Look at** `out/catch/sheet.png` (the frames where reports changed). Add
   `--debug` to see hitboxes you marked with `ctx.debug_rect(rect)`.
5. **Pin it down with a play test** in the same file (and list the
   example in `Cargo.toml` with `test = true`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_right_moves_the_paddle_right() {
        let played = PlayTest::new(Config::default(), new_game())
            .script(r#"{"events": [{"frame": 1, "press": "Right"}]}"#)
            .frames(60)
            .run();
        assert_eq!(played.game().paddle.x, 288.0); // all the way right
    }
}
```

6. **Verify**: `tools/verify.sh catch` (`tools\verify.bat` on Windows) runs the play tests and
   `playthroughs/catch*.json`, plays 50 random games (`--fuzz`) looking for crashes, broken
   `ctx.check("on screen", ok)` rules and freezes, and ends with `VERIFY OK` or `VERIFY FAILED`.
   Fix one thing and go round again (same command, same result).

## With Duck Forge Studio

A person can make and change the same games with no terminal in Duck Forge Studio (`studio/`, an app):
the game plays inside it, Tab freezes it on its frame, and they paint the level (`level.txt`), move Feel
sliders (`examples/<game>.tune.json`) and press Check (the verify above, as a friendly list). It uses only
these plain files, so you can take turns. Their last play is `out/sessions/<game>/latest.json` (replay it
with `--inputs`). Installed, its folder is `Documents\Duck Forge`, building where `.cargo/config.toml` says.

## Five common mistakes

- `cargo run --example x --headless`: the flag goes to Cargo. Write `cargo run --example x -- --headless`.
- `is_down` for one-shot actions (start, shoot, sounds) repeats every frame: use `was_pressed` for those.
- Speeds without `ctx.dt()`: write `x += SPEED * ctx.dt()`, speeds per second.
- Randomness or time not from `ctx`: runs stop being repeatable.
- Drawing the score inside `push_camera` ... `pop_camera`: it moves with the world. Draw it after.
