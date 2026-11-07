# Issues met while making Duck Craft

A log for improving Duck Forge, Duck Quill and Duck Echo, kept while
building the game. Newest at the bottom. "Fixed" means worked round in the
game; the tool still has the gap.

## Duck Forge

(1 to 8 from the first session; 9 on from making the skinned duck.)

1. **No pointer lock.** The window can't hide and hold the mouse pointer,
   so mouse look stops at the window's edge. Worked round with
   "edge turning" (`edge_turn` in `main.rs`): resting the pointer near an
   edge keeps turning. A real fix needs the engine to lock the pointer
   (minifb can't; it may need another window library or raw input).
2. **No block-world builder.** The guide says to `merge` whole blocks into
   chunks, which keeps every hidden face between neighbours: far too many
   triangles once the player can dig. The atlas positions `Mesh::block`
   uses (`Sprite::in_sheet`) are private, so the game cuts the cube from
   `Mesh::block` into its six faces (`faces_of`) and copies only the faces
   next to air. A `Chunk`/`BlockWorld` helper in the engine would save
   every block game this work.
3. **A tile from an atlas can't be used as a repeating texture.** The sea
   and clouds needed a tile copied pixel by pixel into its own `Sprite`
   (`tile_picture`), because texture points beyond 1 repeat the whole
   atlas, not one tile.
4. **Glow is per mesh.** Lamps need their own mesh per chunk, and glowing
   blocks can't light their neighbours. No fog either, so the world ends
   with a hard edge at the sea.
5. **The default fuzz keys don't include W/A/S/D or the game's own keys**,
   so `verify`'s 50 runs stood still (`x always 32.5`). Passing
   `--fuzz-keys W,A,S,D,Space,X,C,...` found real coverage. The fuzzer
   could learn the keys from `was_pressed`/`is_down` calls.
6. **`mouse_position()` is (0, 0) when there is no mouse**, the same as the
   top-left corner, so a game can't tell "no mouse" from "mouse at the
   corner". The game tracks its own `mouse_used` flag instead.
7. **`cargo test` doesn't rebuild the example binary.** Twice a replay ran
   an old `target/release/examples/duck_craft` and seemed to break
   repeatability. `tools/verify.sh` is safe (it builds first); running
   the binary by hand after `cargo test` isn't. A note in the skill would
   help.
8. **Snapshots can't be saved from a test without a canvas copy.**
   `Canvas::save_png` exists and works (used by the `views` test); the
   skill doesn't mention it.

9. **`Mesh::merge` silently drops texture points from a mesh that has
   no texture yet.** The beak was merged into the head before
   `.with_texture(skin)` and came out plain white (`uvs_of` returns `None`
   without a texture). Either keep the points, or panic/warn. Cost: half
   an hour.
10. **`rotate_x`'s direction isn't in the guide.** `vec3(0, -1, 0)
    .rotate_x(a)` goes to `-z` for positive `a`; I guessed `+z` and the
    first-person arm pointed behind the camera. One line in section 11
    ("positive x rotation tips +z up") would do; the same for y and z.
11. **No way to draw a mesh in "view space".** A first-person hand has to
    be placed in the world every frame from the camera's position, yaw
    and pitch (`draw_hand`). A `canvas.draw_mesh_in_view(mesh, transform)`
    that ignores the camera would make hands, weapons and HUD props easy.
12. **Meshes can't be drawn with a per-draw tint or transparency**, so a
    shadow blob under the duck or a see-through selection box isn't
    possible; the selection is 12 `draw_line`s instead.

## Duck Quill

1. **First build is slow (about 8 minutes)**, because `cargo build
   --release -p quill-cli` also builds the app's GUI crates. A leaner
   CLI-only build (or a prebuilt binary in the release zip) would help
   agents on servers.
2. **`export` needs animations.** A script of still tiles (one `define`
   each) is refused: "this script has no animations". Every tile needs a
   one-frame animation that `use`s it. `quill atlas` is the other route,
   but one script per tile.
3. **No noise or speckle fill for pixel art.** Grass, dirt, stone and
   leaves were generated pixel by pixel in Python (`art/make_blocks.py`).
   A `speckle`/`noise` command on a pixel canvas, seeded, would let
   agents do it in the script.
4. **A skin needs the layout twice.** The skin's face layout lives in
   `make_skin.py` and again in `main.rs`; Duck Quill could export a JSON
   of named regions (like the atlas's `tiles`) that the engine reads, or
   the engine could take a Minecraft-style skin layout directly.
5. **`check` warned that ~26 palette colours were "never used"** though
   they are used inside `define`d symbols (via `pixels` `colors`). Worth
   a look: either a false warning or symbols aren't scanned.

## Duck Echo

- Not used: it isn't built here. The wind loop is Mini Craft's; the dig,
  break, place, step, splash and pick sounds are `Sound { .. }` in code.
