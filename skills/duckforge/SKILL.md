---
name: duckforge
description: Use when making, changing or checking a 2D game in Rust with the duckforge engine, especially on a server with no screen.
---

# duckforge: make games you can't see

duckforge is a small Rust game engine. Every game also runs **headless**:
no window, faster than real time, saving pictures, sound, a short summary
and a log. You never play the game; you read what it reports, look at a
contact sheet, and let tests and a fuzzer play it for you.

Read [QUICKSTART.md](../../docs/QUICKSTART.md) first (one page). The full
reference is [AI_GUIDE.md](../../docs/AI_GUIDE.md): section 7 is the API
cheat sheet, section 8 the common mistakes.

## 1. Get it and build headless

```sh
git clone https://github.com/mrguymrguy647-del/game-engine-rust-language duckforge
cd duckforge
cargo build --no-default-features --examples
tools/verify.sh dodge
```

The first build takes about 15 s, a one-line change under a second.
`VERIFY OK` means the engine works here (on Windows: `tools\verify.bat
dodge`). On a server **always** pass `--no-default-features`. Engine
options go after `--`: `cargo run --no-default-features --example dodge -- --headless`.

## 2. Start from a starter

| Kind of game | Starter | Made from |
|---|---|---|
| Platformer (gravity, platforms, coins) | `platformer` | `examples/template.rs` |
| Top-down (walls, chasing enemies, coins) | `topdown` | `examples/starter_topdown/` |
| Shooter (fast bullets, waves, explosions) | `shooter` | `examples/starter_shooter/` |
| Something tiny | `tiny` | `examples/quickstart.rs` |

```sh
tools/new_game.sh mygame topdown
```

It copies the starter (not its tune file), renames its type, titles it
("Mygame"), points the bot at `playthroughs/mygame.json`, adds it to
`Cargo.toml`, records a playthrough and runs `verify` (Windows:
`tools\new_game.bat`). Art: Duck Quill, `quill export art/hero.json --sheet
--out assets`, then `SpriteSheet::load("assets/hero.json")?` and
`sheet.draw(canvas, "walk", time, feet, blit)` (by the pivot: feet stay
put). Music: a Duck Echo `.wav` with `ctx.play_loop(&music, 0.6)`.

## 3. The loop

1. **Change one thing** in `examples/mygame/main.rs` (or `examples/mygame.rs`).
2. **Verify**: `tools/verify.sh mygame`. It builds, runs the play tests,
   replays every `playthroughs/mygame*.json` with a contact sheet, fuzzes
   50 runs, and prints one line per step. Fix the first `FAILED` line
   first; it shows the failing test, rule or crash with its frame.
3. **Read** `out/verify/mygame/mygame/summary.txt` (final values, ranges,
   text changes, sounds, problems), then **look** at `sheet.png` next to
   it. A run by hand:

```sh
cargo run --no-default-features --example mygame -- --headless --inputs playthroughs/mygame.json --sheet-events --debug --out out/look
```

4. **Compare** before and after a change: run once with `--out out/before`,
   change the code, run with `--out out/after`, then
   `cargo run --no-default-features --example mygame -- --diff out/before out/after`.
5. **Pin it down** with a play test (copy one from the bottom of the
   starter), and go round again. Same seed and inputs, same result.

`--quiet` prints only problems (no output: all is well). `--json` prints
the summary as one line of JSON. A panic is one line with the frame:
replay with `--frames N` just before it.

## 4. Tune without rebuilding

Read numbers you'll adjust at the top of `update`:
`let speed = ctx.tune("player_speed", 90.0);`. Try values with
`--set player_speed=120` (repeatable); keep them in
`examples/mygame.tune.json`. `--write-tune` writes every name the game
asked for. The summary warns about `--set` or tune-file names nobody asked for.
Recordings keep their tune values: new values that still give the expected ending are only a `tuned` warning.

## 5. Labels: know what's on screen without pictures

In `draw`, name things right after drawing them:
`canvas.label("enemy", rect)`. Add `--describe` to a headless run to get
the last frame in words; every saved picture gets a `.scene.json`; the
summary warns about labels off screen and outside the world; `--sheet-follow enemy` zooms
each sheet frame in on that label. `--grid 20` (`--grid-world 32` in world
units) draws coordinates on saved pictures.

## 6. Rules and fuzzing

State what must always be true, in `update`:
`ctx.check("player inside the map", world.contains(player.center()));`.
A broken rule is reported once with its first frame (summary, `log.json`,
play tests fail). Then let the engine play:

```sh
cargo run --release --no-default-features --example mygame -- --fuzz 200
```

It reports crashes, broken rules and freezes with a replayable, shrunk
script (`--inputs fuzz/seed-14.json --sheet --debug` shows it), then
`coverage:` lines: each number's range and each text value reached across
all runs. Judge "no problems found" by them. Call `ctx.idle_ok()` on
waiting screens. `--fuzz-keys Left,Right,Space` picks the keys.

## 7. Playthroughs

A playthrough is an input script in `playthroughs/mygame.json`:
`{"events": [{"frame": 10, "press": "Right"}, {"frame": 70, "release": "Right"}]}`
(frames start at 1). Write one, press F10 in a window, or let a bot in
an ignored test save `sim.recording()` (`record_playthrough`; run with
`cargo test --no-default-features --example mygame -- --ignored`).
Recordings store their ending (`"expect"`). When `verify` says `changed`,
add `--accept` to the replay to keep the new ending, or re-record.

## 8. Common mistakes

- `cargo run --example x --headless`: the flag goes to Cargo. Write
  `cargo run --example x -- --headless`.
- `is_down` for one-shot actions (start, shoot, sounds): they repeat every
  frame. Use `was_pressed`.
- Speeds without `ctx.dt()`: write `x += SPEED * ctx.dt()`.
- Randomness or time not from `ctx` (`ctx.rng`, `ctx.time()`): runs stop
  being repeatable, and so do playthroughs and fuzz seeds.
- Reports that don't show what the player does: report the player's
  position, not only the score, or `--fuzz` calls moving a freeze.
- Reporting, playing sounds or `ctx.check` in `draw`: `draw` has no `ctx`.
  Do it in `update`.
- The HUD drawn between `push_camera` and `pop_camera` moves with the
  world. Draw it after `pop_camera`.
- Bullets moving more than their own size per frame pass through things:
  use `swept_aabb` (see the shooter starter).
- Effects drawing from `ctx.rng` change the game; give them their own
  `Rng::new(1)`.
- Forgetting `test = true` in `Cargo.toml`: the play tests never run.

## 9. Show your work

```sh
cargo run --release --no-default-features --example mygame -- --headless --inputs playthroughs/mygame.json --sheet-events --video out/show/run.mp4 --out out/show
```

`--video` saves every frame, sharp, 1080p, with sound (needs `ffmpeg`). Send the sheet, the video and the verify report.
