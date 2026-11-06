# Phase 3 design

What the test games (Duck Dash, Duck Tower, Duck Dash 3D, Duck Craft)
needed, as small additions to what already works: text level maps, bot
playthroughs, verify, tune files, deterministic headless runs. Every
feature works headless and in a window, draws the same on the CPU and
GPU, and changes nothing for games that don't use it. Nothing here adds a
new library: PNG, JSON and WAV reading are already in the engine.

## 1. Sprite sheets (Duck Quill)

`quill export hero.json` writes `hero.png` and `hero.json` (see Duck
Quill's `docs/sprite-sheet-format.md`):

```json
{ "image": "hero.png", "frame": { "width": 16, "height": 16 },
  "columns": 4, "rows": 2, "scale": 1, "pivot": [8.0, 15.0],
  "animations": {
    "idle": { "row": 0, "frames": [0, 1], "fps": 4.0, "loop": true },
    "run":  { "row": 1, "frames": [4, 5, 6, 7], "fps": 12.0, "loop": true } } }
```

```rust
let hero = SpriteSheet::load("assets/hero.json")?;   // reads the PNG next to it
let run = hero.animation("run");                     // an Animation: frames, fps, looping
let frame = run.frame_at(self.time);                 // stops on the last frame if not looping
canvas.draw_at_pivot(frame, feet, hero.pivot(), Blit { flip_x, ..Default::default() });
// or, in one call:
hero.draw(canvas, "run", self.time, feet, Blit::default());
```

- `Animation` gains `looping` (`true` by default, so nothing changes).
- The pivot is in frame pixels, bottom centre when the JSON has none.
- **Size check.** If the PNG isn't `columns × width` by `rows × height`,
  `load` fails with a sentence:
  `hero.png is 768 x 192, but hero.json says 4 x 2 frames of 16 x 16 (64 x 32): was it saved with quill render --zoom 6? Use quill export`.
  The same check catches a hand-edited JSON.
- An unknown animation name panics listing the names that exist (a typo
  is a bug, like an index out of range).
- A small real sheet made by `quill export` goes in `tests/fixtures/`.

## 2. Tune files

Three fixes:

- **Code wins unless you mean it.** `new_game` stops copying the
  starter's tune file. A new game starts with the values in its code;
  `--write-tune` makes the file when you want one. And whenever a file
  value differs from the code's default, the summary says so:
  `sight = 100 from examples/x.tune.json (the code says 90)`.
- **Bots and replays use the same values.** The tune file is found by the
  program's name, so a bot in a test program with another name silently
  got none. Now a recording stores the values it was made with:
  `"tune": {"sight": 100}` next to `"expect"`. A replay with different
  values is a verify failure: `playthroughs/x.json was recorded with
  sight = 100, but this run used 90 (from the code): re-record it, or
  --accept`. And a game can name its file, so nothing depends on the
  program's name: `Config { tune_file: Some("examples/x.tune.json".into()), .. }`.
- **Impossible to miss.** The tuned values move to the summary's second
  line, in capitals: `TUNED from examples/x.tune.json: sight = 100
  (code 90), wasp_speed = 42`.

## 3. Video

```sh
cargo run --no-default-features --example dodge -- --headless --inputs playthroughs/dodge.json --video out/dodge.mp4
```

Every frame (60 per second) is drawn, enlarged by the largest whole
number that fits in 1920 x 1080 with sharp pixels (320 x 240 becomes
1280 x 960), centred on black, and piped to `ffmpeg` with the run's
`audio.wav`. Without `ffmpeg`, the frames are saved as
`video-frames/000001.png` with the WAV, and the exact `ffmpeg` command
is printed. `--gif` stays as it is for quick looks.

## 4. Labels in long levels

The "off screen in every checked frame" warning already skips games
whose camera moves. Now it also skips a label inside the area the
camera reaches in the run (the level's width, roughly), and still warns
for one that's off screen *and* outside everything the camera ever
showed: the real "drawn in the wrong coordinates" bug.

## 5. Looping sounds and music

```rust
let music = Wav::load("assets/theme.wav")?;            // a Duck Echo loop
self.music = ctx.play_loop(&music, 0.6);               // returns a LoopId
ctx.set_loop_volume(self.music, 0.3);                  // quieter during a pause
ctx.stop_loop(self.music);
self.hum = ctx.play_loop(&Sound { name: "hum", wave: Wave::Sine, freq: 80.0, ms: 500.0, ..Default::default() }, 0.2);
```

The whole file repeats with no gap. Headless runs mix loops into
`audio.wav` sample-exactly, and `log.json` records each one:
`{"loop": "theme", "start": 1, "stop": 900, "volume": [[1, 0.6], [400, 0.3]]}`.
Play tests get `played.loops()`. It comes off the guide's "can't do yet" list.

## 6. Small ones

`Rect::top_left()`, `top_right()`, `bottom_left()`, `bottom_right()`
(`center()` exists). `SpriteSheet` and the new tile and background types
in the prelude (`Wav`, `Sprite` and `Animation` already are).
`InputScript::from_json(text)` and `InputScript::from_file(path)` as
names for `parse` and `load`, and the cheat sheet lists all four.
`new_game duck_tower` gives "Duck Tower". The guide, quick start and
skill point to Duck Quill for art: paint, `quill export`, `SpriteSheet::load`.
The platformer starter gets a bot.

## 7. Tile maps

A level as text, one letter per tile:

```text
.........................
..c.........c.......c....
........=====.......#....
p.....e.........###.....g
#########..##############
```

```rust
let legend = Legend::new()
    .solid('#', "ground")          // tile name from the tileset
    .one_way('=', "plank")         // stand on it, jump up through it
    .spawn('p', "player")          // not a tile: a spawn point
    .spawn('c', "coin")
    .spawn('e', "enemy")
    .spawn('g', "goal");           // '.' and ' ' are always empty
let map = TileMap::parse(LEVEL, 16.0, &legend)?;   // or TileMap::load("levels/1.txt", ..)
let start = map.spawns("player")[0];               // the tile's bottom centre, in world pixels
let hit = map.move_and_slide(&mut self.body, self.vel * ctx.dt());
if hit.ground { self.vel.y = 0.0 }                 // also hit.ceiling, hit.wall_left, hit.wall_right
map.draw(canvas, &tiles, &self.camera);            // only the tiles the camera sees
```

`move_and_slide` moves one axis at a time, like the starters do now,
and passes up through one-way tiles but lands on them. Errors name the
place: `levels/1.txt row 3, column 17: 'x' isn't in the legend (it has
# = p c e g . and space)`. Rows may differ in length (the missing tiles
are empty).

## 8. Tilesets

Duck Quill has no tileset export, so a tileset is either:

```rust
let tiles = Tileset::grid(Sprite::load_png("assets/tiles.png")?, 16, 16, &["ground", "plank", "water"]);
let tiles = Tileset::load("assets/tiles.json")?;   // a Duck Quill sprite sheet: one animation per tile
```

The second reuses Duck Quill's sprite sheet export as it is: each
animation is a tile, named by the animation, and an animation with
several frames is an animated tile (water). No change to Duck Quill needed.

## 9. Scrolling backgrounds

```rust
let sky = Background::new(Sprite::load_png("assets/sky.png")?).parallax(0.2);
let hills = Background::new(Sprite::load_png("assets/hills.png")?).parallax(0.5).offset(vec2(0.0, 60.0));
// in draw, before push_camera:
canvas.draw_background(&sky, &self.camera);
canvas.draw_background(&hills, &self.camera);
```

The image repeats sideways with no seam (whole pixels, no gaps between
copies). `parallax(0.5)` moves at half the camera's speed; 0 is fixed
to the screen, 1 moves with the world. `.repeat_y(true)` also repeats
up and down. The platformer starter is rebuilt on the tile map, two
background layers and a `SpriteSheet`.

## 10. Textures on meshes

```rust
let atlas = Tileset::load("assets/blocks.json")?;         // grass_top, grass_side, dirt, stone, lava...
let grass = Mesh::block(&atlas, ["grass_top", "grass_side", "dirt"]); // top, sides, bottom: 12 triangles
let sign = Mesh::quad(1.0, 1.0).with_texture(Sprite::load_png("assets/sign.png")?);
let mut floor = Mesh::checkerboard(8, 1.0, Color::WHITE, Color::WHITE).with_texture(tile);
```

- Each vertex can have a `uv` (0 to 1 across the texture); a mesh can
  have one `texture` (a `Sprite`, shared, not copied).
  `add_vertex_uv(position, uv)` builds your own. A texture's colour is
  multiplied by the triangle's colour (white shows it as painted) and
  the lighting. See-through texture pixels (alpha below half) aren't drawn.
- Nearest-neighbour sampling: pixel art stays crisp.
- **Perspective-correct** on the CPU: `u/z`, `v/z` and `1/z` are blended
  across the triangle and divided per pixel (the CPU already blends 1/z
  for its depth buffer). The GPU does the same by itself. A test draws
  a textured scene both ways and compares pixels, like the flat one now.
- Flat-colour meshes are drawn exactly as today.

## 11. Lighting

```rust
canvas.set_lighting(Lighting {
    sun_direction: vec3(0.0, 0.3, -1.0),   // where the light comes from
    sun_color: Color::from_hex(0xfff2d0),
    sun_strength: 1.0,
    ambient: 0.6,                          // a side view, lit from the front
});
let lava = Mesh::block(&atlas, ["lava"; 3]).glowing();   // unlit: always full brightness
```

`Lighting::default()` is exactly today's light (the same direction, white,
ambient 0.35), so every existing picture stays the same to the pixel.
It lasts until changed (and is reset by nothing, so set it in `draw`
once). Flat shading per triangle, as now.

## 12. Sprites in 3D (billboards)

```rust
canvas.draw_billboard(&hero_frame, vec3(x, y, 0.0), 1.0, &self.camera3d); // feet at (x, y, 0), 1 unit tall
canvas.draw_billboard_with(&frame, at, 1.0, &camera, Billboard { pivot: sheet.pivot(), flip_x: true, ..Default::default() });
let (screen, depth) = camera.project(vec3(x, y, z), canvas)?;               // depth: distance along the view
```

A billboard is a flat sprite standing in the 3D world, turned to face
the camera around its vertical axis (so characters stay upright), sized
in world units, and depth-tested: cubes in front hide it, it hides cubes
behind it. Its pixels stay sharp squares. See-through pixels don't write
depth, so the sprite's outline is exact. On the GPU it's a textured quad
in the same pass as the meshes. `camera.project` gives the screen point
and its depth, for drawing a health bar above a 3D character or sorting
things yourself. `world_to_screen` stays as it is.

## 13. Mesh helpers

What the 3D examples built by hand, moved into `Mesh`:
`Mesh::cylinder(color, segments)`, `Mesh::cone(color, segments)`,
`Mesh::quad(w, h)`, `Mesh::block(atlas, faces)`, `mesh.merge(&other,
&transform)` (many parts in one mesh, one draw), and `mesh.bounds()`.
Built-in meshes keep their triangle counts.

## 14. Performance

Back-face culling already happens per triangle. Added: **frustum
culling** per mesh (its bounding sphere against the view, before any
vertex is transformed), on the CPU and GPU. Measured with stress3d and
a textured block world before and after; the goal is that textures cost
at most about 20% on the CPU.

## 15. The 3D starter

`new_game mygame runner3d`: a three-lane runner (closest to Duck Dash
3D, and a bot can play it: change lane when an obstacle is ahead). A
camera that follows from behind and above, a textured ground scrolling
forever, a player (a billboard duck), obstacles and coins as textured
blocks, a HUD over the 3D, sounds, looping music, labels, checks, a tune
file, a bot playthrough and play tests.

## 16. Docs

A real 3D section in the AI guide: units (1 unit = 1 metre, y up), the
camera, meshes and helpers, textures and atlases, lighting, billboards,
the 2D HUD over 3D, and limits (a few hundred thousand triangles a frame
on the CPU at 320 x 240). Cheat sheet entries for every new function
(its test calls them all), quick start, tutorial, skill, and this phase
in `docs/decisions.md`.

## Left out on purpose

- Shadows, smooth (per-vertex) shading, point lights, normal maps.
- Texture filtering other than nearest, mipmaps, more than one texture
  per mesh (use an atlas), see-through blending in 3D (alpha is cut-out).
- Slopes in tile maps, tile map editors, saving maps (they're text).
- Skeletal animation and 3D model files (OBJ, glTF).
- Streaming music from disk (a WAV is loaded whole: 1 minute of 44.1 kHz
  stereo is about 10 MB), crossfades beyond `set_loop_volume`, 3D sound.
- Changes to Duck Quill or Duck Echo.

## Open questions

1. **Video size.** Whole-number scaling with black bars (320 x 240 →
   1280 x 960 in a 1920 x 1080 frame) keeps every pixel the same size.
   Filling 1080 exactly needs 4.5x, which makes some pixels wider than
   others. I'd go with bars. OK?
2. **Tilesets.** Is "a Duck Quill sprite sheet with one animation per
   tile" a good enough tileset format, or would you rather Duck Quill
   gets a real tileset export later?
3. **Tune files in recordings.** Should a replay with different tune
   values be a verify *failure* (as above), or a warning like a changed
   ending?
4. **The 3D starter.** Three-lane runner (as above) or a free
   walk-around? The runner is easier for a bot and closer to Duck Dash 3D.
5. **GPU checks here.** This container has no GPU. I'd like to install
   Mesa's software Vulkan driver (`mesa-vulkan-drivers`, from the normal
   package mirrors) so the CPU/GPU comparison tests and side-by-side
   pictures can run here. OK? If not, I'll write exactly what to run on
   your Windows PC.
6. **Billboards.** Upright (turning only around the vertical axis, best
   for characters) as the default, with an option to fully face the
   camera (particles)?
7. **Branch.** This session can only push to `claude/peaceful-bohr-vniw0t`.
   It was exactly at main when this phase began, so I'm using it as the
   new branch, and won't merge it into main. Is that all right?
