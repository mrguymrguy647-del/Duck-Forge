---
name: duckforge
description: Use when making, changing or checking a 2D or 3D game in Rust with the duckforge engine, especially on a server with no screen.
---

# duckforge: make games you can't see

duckforge is a small Rust game engine. Every game also runs **headless**: no window, faster than real time,
saving pictures, sound, a short summary and a log. You never play the game; you read what it reports, look at a
contact sheet, and let tests and a fuzzer play it for you.

Read [QUICKSTART.md](../../docs/QUICKSTART.md) first (one page). The full reference is
[AI_GUIDE.md](../../docs/AI_GUIDE.md) (7: API cheat sheet, 11: 3D, 14: scene files).

## 1. Get it and build headless

```sh
git clone https://github.com/mrguymrguy647-del/game-engine-rust-language duckforge
cd duckforge
cargo build --no-default-features --examples
tools/verify.sh dodge
```

First build about 15 s, a one-line change under a second. `VERIFY OK` means it works here (Windows:
`tools\verify.bat dodge`). On a server **always** pass `--no-default-features`; engine options go after `--`.

## 2. Start from a starter

| Kind of game | Starter | Made from |
|---|---|---|
| Platformer (tile map, one-way platforms, coins) | `platformer` | `examples/template/` |
| Top-down (walls, chasing enemies, coins) | `topdown` | `examples/starter_topdown/` |
| Shooter (fast bullets, waves, explosions) | `shooter` | `examples/starter_shooter/` |
| 3D (three lanes, textured blocks, billboard duck) | `runner3d` | `examples/starter_runner3d/` |
| Something tiny | `tiny` | `examples/quickstart.rs` |

```sh
tools/new_game.sh mygame topdown
```

It copies the starter (and its feel file, not its tune file), renames its type, titles it ("Mygame"), points the
bot at `playthroughs/mygame.json`, adds it to `Cargo.toml`, records a playthrough and runs `verify` (Windows:
`tools\new_game.bat`). The platformer's and the top-down's level is two files, `level.txt` and
`level.legend.json`: change it freely. Their tests don't name its positions (they play tiny levels of their own,
and ask that the level's rules hold and the bot still wins on it).

## 3. Built in: use these, don't write your own

Details: the guide's section 7 (cheat sheet) and section 11 (3D).

- **Levels as files**, one letter per tile: `TileMap::load_level("examples/mygame/level.txt")?` (the letters'
  meanings and rules are in `level.legend.json` beside it); in a test, `TileMap::parse(text, 16.0, &legend)?`.
  `map.move_and_slide(&mut body, motion)` (ground, walls, ceiling; fast bodies never pass through);
  `map.spawns("player")`; `map.draw(canvas, &tiles, time)`; `map.broken_rules()`; `map.route(feet, goal, jump)`
  gives a bot its way, hop by hop.
- **Art** from Duck Quill: `quill export art/hero.json --sheet --out assets`, then
  `SpriteSheet::load("assets/hero.json")?` and `sheet.draw(canvas, "walk", time, feet, blit)` (by the pivot).
  Tiles: `Tileset::grid(&sheet, 16, 16, &names)`. 3D models from Duck Mold: `Mesh::load_glb(path)?` (they face +z).
- **Backgrounds**: `Background::new(image).parallax_xy(0.3, 0.1)`, drawn with `canvas.draw_background(&sky,
  &camera)` before `push_camera`; no seams.
- **Music and hums**: a Duck Echo `.wav` with `ctx.play_loop(&music, 0.6)`; `ctx.stop_loop(id)`.
- **3D**: `Mesh::block(&atlas, [top, sides, bottom])`, `.with_texture(sprite)`, `canvas.set_lighting(..)`,
  `.glowing()` / `mesh.set_glow(triangle, 1.0)`, `canvas.set_fog(Some(Fog { color: sky, near, far }))`,
  billboards for 2D characters, `camera.project(point, canvas)` for labels. Block worlds: `BlockWorld` +
  `BlockPalette` (visible faces only; `world.set` then `world.update_meshes(&palette)` rebuilds the changed
  chunks). Hands: `canvas.draw_mesh_in_view(..)` after the world; shadows, ghosts, glass:
  `canvas.draw_mesh_tinted(.., Color::rgba(0, 0, 0, 90))`, last. Mouse look: `ctx.lock_pointer(true)`, turn by
  `ctx.input.mouse_delta()` (Escape should free it; tests: `sim.move_mouse_by(vec2(dx, dy))`).

## 4. The loop

1. **Change one thing** in `examples/mygame/main.rs` (or `examples/mygame.rs`).
2. **Verify**: `tools/verify.sh mygame` builds, runs the play tests, replays every `playthroughs/mygame*.json`
   with a contact sheet, fuzzes 50 runs, and prints one line per step. Fix the first `FAILED` line first.
3. **Read** `out/verify/mygame/mygame/summary.txt`, then **look** at `sheet.png` next to it. A run by hand:

```sh
cargo run --no-default-features --example mygame -- --headless --inputs playthroughs/mygame.json --sheet-events --debug --out out/look
```

4. **Compare**: run with `--out out/before`, change the code, run with `--out out/after`, then add `--diff
   out/before out/after` to a run.
5. **Pin it down** with a play test (copy one from the starter), and go round again. Same seed and inputs, same
   result.

`--quiet` prints only problems; `--json` prints the summary as JSON. A panic is one line with the frame: replay
with `--frames N` just before it.

## 5. Tune without rebuilding

`let speed = ctx.tune("player_speed", 90.0);` at the top of `update`; try `--set player_speed=120`; keep values
in `examples/mygame.tune.json` (`--write-tune` writes them all). Replays use a recording's own values; `--set`
ones that still give its expected ending are only a `tuned` warning.

## 6. Labels: know what's on screen without pictures

In `draw`, name things right after drawing them: `canvas.label("enemy", rect)`. Add `--describe` to a headless
run to get the last frame in words; every saved picture gets a `.scene.json`; the summary warns about labels off
screen and outside the world; `--sheet-follow enemy` zooms each sheet frame in on that label. `--grid 20`
(`--grid-world 32` in world units) draws coordinates on saved pictures.

## 7. Rules and fuzzing

State what must always be true, in `update`: `ctx.check("player inside the map",
world.contains(player.center()));`. Then let the engine play: `--fuzz 200` (with `--release`) reports crashes,
broken rules and freezes with a shrunk, replayable script (`--inputs fuzz/seed-14.json --sheet --debug`), then
`coverage:` lines: what the runs reached. Judge "no problems found" by them: random runs rarely reach a level's
end. Call `ctx.idle_ok()` on waiting screens; it presses the keys the game reads (`--fuzz-keys Left,Right,Space`
picks others).

## 8. Playthroughs

A playthrough is an input script in `playthroughs/mygame.json`: `{"events": [{"frame": 10, "press": "Right"},
{"frame": 70, "release": "Right"}]}` (frames start at 1). Write one, press F10 in a window, or let a bot save
`sim.recording()` (`cargo test --no-default-features --example mygame -- --ignored`). On `changed`: `--accept`,
or re-record.

## 9. With a person using Duck Forge Studio

The Studio (an app; installed, its folder is `Documents\Duck Forge`) reads and writes the same files as you, so you can take
turns, even while their game is open (it rebuilds when code changes and reloads a changed level):

- they paint `examples/mygame/level.txt`, move Feel sliders (`examples/mygame.tune.json`; the sliders' labels
  and presets are `examples/mygame.feel.json`) and go back in time in `history/` (leave it alone);
- what they last played is `out/sessions/mygame/latest.json`: replay it with `--inputs` to see what they saw;
- their Check is `tools/verify.sh` and the level's own lines; what it said is `out/verify/mygame/check.json`.

Keep Check true after your change: the bot in `bot.rs` must still win on their level, with every Feel preset, so
teach it any rule you add (a double jump, an enemy), and keep the game's `reload` working. With no window:
`cargo run -p studio_app -- --run script.json` in `studio/` (guide 13).

## 10. Common mistakes (and the quick start's five)

- Report the player's position, not only the score, or `--fuzz` calls moving a freeze. `draw` has no `ctx`:
  report, play sounds and check in `update`.
- Randomness not from `ctx.rng` breaks repeatable runs; effects get their own `Rng::new(1)`. Fast bullets pass
  through things: `swept_aabb`.
- Without `test = true` in `Cargo.toml` the play tests never run.
- `cargo test` doesn't rebuild the game's own program: after it, running `target/.../examples/mygame` runs old
  code. Use `tools/verify.sh` or `cargo run`. A test keeps a picture with
  `sim.picture().save_png("out/x.png")?`.

## 11. Show your work

```sh
cargo run --release --no-default-features --example mygame -- --headless --inputs playthroughs/mygame.json --sheet-events --video out/show/run.mp4 --out out/show
```

`--video` saves every frame, sharp, 1080p, with sound (needs `ffmpeg`). Send the sheet, the video and the verify
report.
