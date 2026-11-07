# Open issues met while making Duck Craft

A log for improving Duck Forge, Duck Quill and Duck Echo, kept while
building the game. Only open issues are here: resolved ones are taken out
(git history has them). Duck Forge's numbers stay as they were, so a gap
is an issue that's been resolved. "The game works round it" means the
game copes; the tool still has the gap.

## Duck Forge

16. **A game can't tell it's running headless.** Saving must never happen
    in a replay or fuzz run (or the next replay would start from a saved
    island and stop being repeatable), but `Context` doesn't say. `main`
    looks for `--headless` in the command line itself. A
    `ctx.is_headless()` (or a `Config` flag the CLI fills in) would do.
17. **No hook when the window closes.** Closing the window with its X
    button ends the program without calling the game, so the last moments
    aren't saved. The pause menu saves when you quit from it, and the game
    autosaves every 30 seconds, but the close button still loses up to
    30 s. A `Game::quit(&mut self, ctx)` called on any way out would fix it.
18. **The command line refuses the game's own options.** `duckforge::run`
    parses the arguments and stops at any it doesn't know, so a game can't
    offer options like `--new-world`. (Duck Craft no longer needs that one:
    the title screen offers a new island.) Letting options after `--` (or
    unknown `--game-*` ones) through to the game would help.
19. **The fuzzer's freeze rule fires on menus.** Keys that do nothing in a
    menu look like a frozen game. Duck Craft gets round it (every menu key
    does something, and the menu's state is reported), but a game with a
    plain pause screen will meet the same rule; a way to say "paused"
    (`ctx.report_paused(true)`) that the fuzzer respects would be fairer.
20. **`Simulation` tests have no way to step until something is true.**
    The bots and the showcase write `for _ in 0..N { if done { break } sim.step() }`.
    A `sim.run_until(limit, |game| ...)` would read better and stop
    off-by-one waits.
22. **The fuzzer never uses the mouse.** It finds the inventory screen (E)
    but never clicks in it, and it can't aim, so it never fights. The
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
25. **No way to ask for an atlas tile's texture points without the
    `Tileset`.** The plants keep their own copy of `atlas.uv_rect(tile)`
    for each kind, taken when the art loads. `BlockPalette` could hand
    them out, since it has the atlas.
26. **The engine has no "plant" (crossed pictures) block shape.**
    `BlockWorld` draws only cubes, so tall grass and flowers live in
    their own map and their own mesh (`plants.rs`), rebuilt whole when
    one changes. A `BlockKind::crossed()` shape would let them sit in the
    grid like any other block, chunk by chunk.
27. **The windowed game shows the system's pointer, recorded videos
    don't.** The showcase draws its own arrow (`draw_pointer`), off in
    the real game. A video option to draw the pointer would help every
    game with menus.
28. **`play_wav` has no volume.** A far-off Hollow should groan quietly,
    but a `.wav` plays at one loudness; the game ships a second, quieter,
    muffled file (`groan_far.wav`). `ctx.play_wav_at(&wav, volume)` (and
    one day a position for stereo) would do.
29. **A `Wav` can't be made from samples in code.** The music had to be
    made in Python and shipped as files (5 MB of `.wav`), where a
    `Wav::from_samples(&[f32], rate)` would let a game make sounds at
    start. (The engine already resamples anything it loads, which made
    16 kHz music files easy.)
30. **Loading 5 MB of sound in every play test** (each `arena` loads the
    art) took the tests from 0.5 s to about 4 s. A load cache in
    `Wav::load` (or the engine sharing loaded files) would bring it back.
31. **Replays don't apply the tune values they were recorded with.** The
    night showcase starts at dusk (`start_time_of_day`); its recording
    notes the value, but the replay needs `--set start_time_of_day=0.47`
    again, or it plays a different game. An option to use the recorded
    values (or a warning that blocks the run) would help.
32. **Labels under a menu make `verify` fail.** The title and pause
    screens are drawn over the world, and a duckling's label under a
    button counts as "spawned on top of each other". The game labels
    world things only while playing; a way to mark a menu as a layer
    above the world would be cleaner.
34. **Fuzz runs all start at the same time of day.** The night (and the
    Hollows) only got fuzzed by adding a tune value for the starting
    time and fuzzing with `--set start_time_of_day=0.6`. That works well;
    worth a line in the skill ("fuzz the hard parts with --set").
35. **`set_loop_volume` every frame fills the log.** Fading five loops
    for a whole run would write thousands of volume changes; the game
    only sends a change when it's bigger than 0.004. The engine could
    skip changes too small to hear.
36. **`Simulation` has no `frame()`;** `sim.context().frame()` works.

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
   `make_skin.py` and again in the game (`skin_box` calls); Duck Quill
   could export a JSON of named regions (like the atlas's `tiles`) that
   the engine reads, or the engine could take a Minecraft-style skin
   layout directly.
5. **`check` warned that ~26 palette colours were "never used"** though
   they are used inside `define`d symbols (via `pixels` `colors`). Worth
   a look: either a false warning or symbols aren't scanned.
6. **More than 52 colours in one pixel script.** The atlas has 63
   colours, and `pixels` rows use one character a colour, so
   `make_blocks.py` ran out of letters and uses digits and symbols too.
   A `colors` map of short names (two characters) would read better.

## Duck Echo

- Not used: it isn't ready. The music and the longer sounds come from
  `audio/make_sounds.py` (numpy and scipy, written for this game); the
  wind loop is Mini Craft's; short sounds (dig, place, step, quack,
  click) are `Sound { .. }` in code. What it took, for Duck Echo's
  sake: notes with harmonics and decays for a felt piano, inharmonic
  partials for bells, band-passed saws for voices, a Schroeder reverb,
  and loops made seamless by rendering three passes and keeping the
  middle one (the drones tuned to whole cycles per loop, the wind made
  periodic in the frequency domain).
