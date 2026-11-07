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
   **Fixed in the engine:** `ctx.lock_pointer(true)` and `mouse_delta()`
   (the window moved to winit). The game now holds the pointer; Escape
   frees it and a click takes it back.
2. **No block-world builder.** The guide says to `merge` whole blocks into
   chunks, which keeps every hidden face between neighbours: far too many
   triangles once the player can dig. The atlas positions `Mesh::block`
   uses (`Sprite::in_sheet`) are private, so the game cuts the cube from
   `Mesh::block` into its six faces (`faces_of`) and copies only the faces
   next to air. A `Chunk`/`BlockWorld` helper in the engine would save
   every block game this work. **Fixed in the engine:** `BlockWorld` and
   `BlockPalette` (only visible faces, tiles from one atlas, rebuilds just
   the changed chunks), and `Tileset::uv_rect`,
   `Sprite::place_in_picture` and `whole_picture` say where tiles are.
   Duck Craft's world now uses them (the same pictures, to the pixel).
3. **A tile from an atlas can't be used as a repeating texture.** The sea
   and clouds needed a tile copied pixel by pixel into its own `Sprite`
   (`tile_picture`), because texture points beyond 1 repeat the whole
   atlas, not one tile. **Fixed in the engine:** a tile cut from the atlas
   already repeated on its own (only the whole atlas picture, the one
   `Mesh::block` uses, repeats as a whole); `atlas.tile("water")` now says
   so, and Duck Craft uses it instead of copying pixels.
4. **Glow is per mesh.** Lamps need their own mesh per chunk, and glowing
   blocks can't light their neighbours. No fog either, so the world ends
   with a hard edge at the sea. **Fixed in the engine:** a glow level per
   triangle (`mesh.set_glow`, `BlockKind::glowing` and `FaceLight` in a
   block world) and `canvas.set_fog`. Lamps are in the chunk mesh, the
   faces round a lamp glow a little at night, and the world fades into
   the horizon's colour.
5. **The default fuzz keys don't include W/A/S/D or the game's own keys**,
   so `verify`'s 50 runs stood still (`x always 32.5`). Passing
   `--fuzz-keys W,A,S,D,Space,X,C,...` found real coverage. The fuzzer
   could learn the keys from `was_pressed`/`is_down` calls. **Fixed in the
   engine:** `--fuzz` learns them with a few practice runs; Duck Craft's
   50 runs now walk (x 11 to 43), dig, build and pick every block.
6. **`mouse_position()` is (0, 0) when there is no mouse**, the same as the
   top-left corner, so a game can't tell "no mouse" from "mouse at the
   corner". The game tracks its own `mouse_used` flag instead. **Fixed in
   the engine:** `mouse_position()` gives `Option<Vec2>`, `None` until
   there's a mouse (and Duck Craft doesn't need the flag any more: it
   locks the pointer).
7. **`cargo test` doesn't rebuild the example binary.** Twice a replay ran
   an old `target/release/examples/duck_craft` and seemed to break
   repeatability. `tools/verify.sh` is safe (it builds first); running
   the binary by hand after `cargo test` isn't. A note in the skill would
   help. **Fixed in the docs:** the skill and the guide (play tests) say
   so, and a test keeps the skill saying it.
8. **Snapshots can't be saved from a test without a canvas copy.**
   `Canvas::save_png` exists and works (used by the `views` test); the
   skill doesn't mention it. **Fixed in the docs:** the skill, the guide's
   cheat sheet and play tests section show `sim.picture().save_png(..)`.

9. **`Mesh::merge` silently drops texture points from a mesh that has
   no texture yet.** The beak was merged into the head before
   `.with_texture(skin)` and came out plain white (`uvs_of` returns `None`
   without a texture). Either keep the points, or panic/warn. Cost: half
   an hour. **Fixed in the engine:** `merge` keeps the points; the duck
   now merges the beak first and paints the head once.
10. **`rotate_x`'s direction isn't in the guide.** `vec3(0, -1, 0)
    .rotate_x(a)` goes to `-z` for positive `a`; I guessed `+z` and the
    first-person arm pointed behind the camera. One line in section 11
    ("positive x rotation tips +z up") would do; the same for y and z.
    **Fixed in the docs:** section 11 says which way each turns (with this
    very example), and a test checks the words against the maths. One
    correction to the note above: a positive `rotate_x` sends `-y` to
    `+z` (z becomes `sin a`), so the `+z` guess was right and the arm
    pointed behind for some other reason. The docs and test now settle it.
11. **No way to draw a mesh in "view space".** A first-person hand has to
    be placed in the world every frame from the camera's position, yaw
    and pitch (`draw_hand`). A `canvas.draw_mesh_in_view(mesh, transform)`
    that ignores the camera would make hands, weapons and HUD props easy.
    **Fixed in the engine:** `canvas.draw_mesh_in_view(mesh, transform,
    camera)` (it takes the camera for its field of view and to keep the
    sun in the right place). `draw_hand` is half as long, and the arm no
    longer sinks into a wall you stand against.
12. **Meshes can't be drawn with a per-draw tint or transparency**, so a
    shadow blob under the duck or a see-through selection box isn't
    possible; the selection is 12 `draw_line`s instead.
    **Fixed in the engine:** `canvas.draw_mesh_tinted(mesh, transform,
    camera, Color::rgba(..))` (the colour multiplies, its alpha is how
    solid). Duck Craft now has a soft shadow under the duck and a faint
    glassy box over the block you look at (the edge lines stay).

## Found while fixing the engine

13. **On X11, a grabbed pointer reports every mouse movement twice.** With
    winit's `Confined` grab, each raw movement arrived twice (Xvfb and
    xdotool: 10 pixels became 20). The engine doesn't grab on Linux; it
    hides the pointer and puts it back in the middle every frame.
14. **winit panics instead of returning an error when `libxkbcommon-x11`
    is missing** (a bare Linux server). The engine catches it and says
    what to install, and to use `--headless`.
15. **winit can make only one event loop per program**, so
    `duckforge::run` can open one window per program. No game needs two.

## Found while adding crafting, ducklings, falling sand and saving

16. **A game can't tell it's running headless.** Saving must never happen
    in a replay or fuzz run (or the next replay would start from a saved
    island and stop being repeatable), but `Context` doesn't say. `main`
    looks for `--headless` in the command line itself. A
    `ctx.is_headless()` (or a `Config` flag the CLI fills in) would do.
17. **No hook when the window closes.** Closing the window with its X
    button ends the program without calling the game, so the last moments
    aren't saved; the game autosaves every 30 seconds to make up for it.
    A `Game::quit(&mut self, ctx)` called on any way out would fix it.
18. **The command line refuses the game's own options.** `duckforge::run`
    parses the arguments and stops at any it doesn't know, so the game
    can't offer `--new-world`; to start again you delete the save file.
    Letting options after `--` (or unknown `--game-*` ones) through to the
    game would help.
19. **The fuzzer's freeze rule fires on menus.** With the crafting menu
    open, keys that do nothing in the menu (A, D, digits at first) looked
    like a frozen game. That was partly right: those keys now do things
    in the menu, and the menu's row and the message on screen are
    reported. But a game with a pause screen will meet the same rule; a
    way to say "paused" (`ctx.report_paused(true)`) that the fuzzer
    respects would be fairer.
20. **`Simulation` tests have no way to step until something is true.**
    The showcase and the bot both write `for _ in 0..N { if done { break } sim.step() }`.
    A `sim.run_until(limit, |game| ...)` would read better and stop
    off-by-one waits.
21. **My own slip, not the engine's:** the tests' `hold` helper presses a
    key and never lets go (its comment said "for N frames"). Two new tests
    failed because of it before I saw why; the comment is fixed and the
    tests that need a release write it out.

## Found while making it more like Minecraft (inventory, health, plants)

22. **The fuzzer never uses the mouse.** It found the inventory screen (E)
    but never clicked in it, so `held` stayed "nothing" in every run. The
    game's own `random_clicks_never_lose_or_copy_things` test does 5,000
    random clicks instead. A `--fuzz-mouse` (random moves, clicks and
    drags, within the canvas) would cover mouse-driven screens.
23. **No way to draw 3D into part of the screen.** The duck in the
    inventory is drawn with `draw_mesh_in_view` at a depth worked out so
    it lands in its box (`draw_duck_preview`); the box can't clip it.
    A `canvas.draw_mesh_in_rect(mesh, transform, rect)` (its own little
    camera and viewport) would make previews, item spinners and
    character screens easy.
24. **`draw_mesh_in_view` is lit by the scene's light**, so the preview
    duck is dark at night unless the game swaps the light just before
    drawing it (it does). Fine, but worth a sentence in its docs.
25. **No way to ask the canvas for an atlas tile's texture points
    without the `Tileset`.** The plants keep their own copy of
    `atlas.uv_rect(tile)` for each kind, taken when the art loads.
    `BlockPalette` could hand them out, since it has the atlas.
26. **The engine has no "plant" (crossed pictures) block shape.**
    `BlockWorld` draws only cubes, so tall grass and flowers live in
    their own map and their own mesh (`plants.rs`), rebuilt whole when
    one changes. A `BlockKind::crossed()` shape would let them sit in the
    grid like any other block, chunk by chunk.
27. **The windowed game shows the system's pointer, recorded videos
    don't.** The showcase draws its own arrow (`draw_pointer`), off in
    the real game. A video option to draw the pointer would help every
    game with menus.

## Duck Quill

0. **(This round) More than 52 colours in one pixel script.** The atlas
   now has 63 colours, and `pixels` rows use one character a colour, so
   `make_blocks.py` ran out of letters and uses digits and symbols too.
   A `colors` map of short names (two characters) would read better.
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
