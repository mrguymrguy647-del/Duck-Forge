# Phase 4 design: Duck Forge Studio

A friendly app for making games without a terminal, with the game playing
**inside** it. The level sits in the middle; Tab flips between Build and
Play on the same spot. This is the plan as approved, with the answers and
changes from the review folded in ("Decided", near the end). Numbers
marked *measured* were measured for this document; `docs/decisions.md`
has the numbers measured once each step was built.

The rules this design keeps:

- Everything the Studio makes is a plain file agents already read and
  write. Anything done in the Studio can be done by editing files, and
  the other way round.
- The game is its own program. The Studio shows its pictures exactly as
  drawn: whole-number scaling, no smoothing. No existing picture changes.
- The game-side link is a cargo feature that exported games, headless
  runs and agent builds don't include.
- Every Studio action is one named command, in one place.
- Same inputs give the same game, including after freeze and resume.

## The screens

**Home**

```text
+------------------------------------------------------------------------+
| (duck) Duck Forge Studio                               Theme    About  |
+------------------------------------------------------------------------+
|  [ + New game ]                    Duck Quill   draw tiles and sprites |
|                                    Duck Echo    make sounds and music  |
|  My games                                                              |
|  +------------+  +------------+  +------------+  +------------+        |
|  | (picture)  |  | (picture)  |  | (picture)  |  | (picture)  |        |
|  | Bee Maze   |  | Duck Peaks |  | Duck Craft |  | Dodge      |        |
|  | Needs a    |  | Checks     |  | Checks     |  | Needs a    |        |
|  | look       |  | pass       |  | pass       |  | look       |        |
|  +------------+  +------------+  +------------+  +------------+        |
|                                                                        |
|  Examples that came with Duck Forge  (show)                            |
+------------------------------------------------------------------------+
```

A card says "Checks pass" only if the last Check passed and nothing has
changed since. Otherwise "Needs a look", with why in small letters under
it ("2 problems", "changed since the last check", "never checked").

**New game** (two small pages)

```text
  New game: pick a starter                                      1 of 2
  +----------+ +----------+ +----------+ +----------+ +----------+
  | (picture)| | (picture)| | (picture)| | (picture)| | (picture)|
  |Platformer| | Top-down | | Shooter  | | 3D runner| |   Tiny   |
  +----------+ +----------+ +----------+ +----------+ +----------+
  Run, jump, collect coins, reach the flag. Has a level you can paint.
                                                           [ Next ]

  New game: name it                                             2 of 2
  Name:  [ Bee Maze                 ]
  Its folder will be examples/bee_maze
                                            [ Back ]   [ Make it ]
```

**The game screen, playing**

```text
+------------------------------------------------------------------------+
| < Home    Bee Maze       [ Build | PLAY ]      Check   Clip   Pop out  |
+------------------------------------------------------------------------+
|                                                                        |
|          +--------------------------------------------------+          |
|          |                                                  |          |
|          |      the game: 320 x 240 shown 3 times bigger    |          |
|          |      sharp square pixels, black around it        |          |
|          |                                                  |          |
|          +--------------------------------------------------+          |
|                                                                        |
+------------------------------------------------------------------------+
| Playing   frame 412   60 fps        Tab: freeze and build              |
+------------------------------------------------------------------------+
```

**The game screen, frozen (Build)**: the same picture, same place.

```text
+------------------------------------------------------------------------+
| < Home    Bee Maze       [ BUILD | Play ]  Undo Redo History  Check .. |
+------------+-----------------------------------------+-----------------+
| Tools      |                                         | Feel            |
|  Paint     |    the frozen frame with a grid on it   |  Duck           |
|  Erase     |    (the duck hangs in mid-jump)         |   Jump  [--|--] |
|  Fill      |                                         |   Run   [--|--] |
|  Select    |                                         |  World          |
| Tiles      |                                         |   Gravity [-|-] |
|  [#] grass |                                         |  Presets  v     |
|  [=] plank |                                         |  Show me the    |
| Things     |                                         |  code           |
|  Start     |                                         | Sounds          |
|  Coin      |                                         |  > jump  > coin |
+------------+-----------------------------------------+-----------------+
| Frozen on frame 412   Edits are saved as you make them   Tab: play on  |
+------------------------------------------------------------------------+
```

A game with no tile level (the shooter, the 3D runner, Mini Craft, Duck
Craft) shows, where Tools would be: "This game has no tile level, so
there's nothing to paint. You can still change how it feels."

**Check**

```text
  Check: Bee Maze                                      [ Check again ]
   ok  It builds
   ok  Its 9 play tests pass
   !   A coin looks out of reach (column 34, row 6)
   X   The bot can't finish: it falls in the water at column 23
   ok  50 random runs: no crashes, no freezes
  ------------------------------------------------------------------
  (the picture of the problem you clicked, with the spot circled)
  What to try: move a plank closer, or lower the coin.
                                                    Show details v
```

**The game stopped** (over the last picture, dimmed)

```text
        +----------------------------------------------------+
        |  The game stopped.                                 |
        |  Its code hit a problem on frame 412:              |
        |  "index out of bounds" in main.rs, line 210        |
        |                                                    |
        |  [ Restart ]   [ Restart and replay my moves ]     |
        |  Show details v            Copy this for an agent  |
        +----------------------------------------------------+
```

**Building, and Rust missing**

```text
  Building Bee Maze...  12 of 14 parts  [===========      ]
  The first build takes a few minutes. Later ones take seconds.

  Duck Forge needs Rust
  Rust is the free tool that turns a game's code into a program.
  It isn't on this computer yet, or Windows can't find it.
    1. Open the official install page and run the installer.
    2. Come back and press Check again.
  [ Open the Rust install page ]      [ Check again ]
  Show details v
```

**History**

```text
  History: Bee Maze
   Today 15:32   Painted 14 tiles               [ Go back to here ]
   Today 15:30   Feel: Fast and bouncy          [ Go back to here ]
   Today 15:21   Before playing                 [ Go back to here ]
   Yesterday     Made from the Platformer starter
```

## 1. Crates, and how the Studio builds a game

```text
Cargo.toml, src/           the engine, as now, plus a "studio" feature
src/studio_link/           the game's side of the link (only with it)
studio/                    its own cargo workspace, lock file and target
  crates/core/             commands, games, builds, the link, Check,
                           history. No window code. Tests run headless.
  crates/app/              DuckForgeStudio: eframe/egui 0.37, theme.rs
  brand/                   the Duck Forge icon, in Duck Quill's style
  release/                 READ ME FIRST.txt, Create desktop shortcut.bat
.github/workflows/studio-release.yml    the "Make a release" button
```

- The Studio is outside the engine's crate, so `cargo build` and
  `cargo test` at the top are exactly what they were. The baseline table
  is measured again at the end of each step.
- `studio/crates/core` uses the engine itself (headless, with `studio`),
  so it reads levels, legends, tilesets and input scripts with the
  engine's own code. There's no second reader to drift.
- The app copies Duck Quill: eframe 0.37, the same `theme.rs` (Night
  Indigo for "chosen", Duck Yellow main button with Ink Navy text), dark
  or light or "Same as Windows", the same release workflow shape.
- Games stay where they are, in `examples/<game>/`, with the tune file,
  playthroughs and `out/` in their usual places. Which folder that is, is
  a Studio setting, and every path and command comes from one file
  (`studio/crates/core/src/folder.rs`), so games can move out of the
  engine's folder later by changing that one place.

**Building.** The Studio runs `cargo build --example <game> --features
studio --message-format=json` and reads cargo's messages:

- A progress bar counts finished parts against the last build's count
  (the first build says "a few minutes" instead of a number).
- The Studio plays a **copy** of the built program. Windows won't let
  cargo replace a program that's running, and the next build must work
  while the game plays.
- Builds happen by themselves: the Studio looks at the game's `.rs`
  files, the engine and `Cargo.toml` twice a second. A build starts only
  once they have stayed unchanged for a second, so a half-saved edit
  never starts one. While you play, a finished build waits behind a "New
  build ready" button with Restart and "Restart and replay my moves". If
  the game wasn't running, it starts.
- **The Console panel** shows the command behind every button, as you'd
  type it, with its real output: the build, Check (`tools/verify`), Clip,
  New game (`tools/new_game`), the game itself. You can type the usual
  commands there too; they run in the games folder as in a terminal.
  Every terminal command keeps working outside the Studio as before.
- A mistake in the code shows one sentence, the file and the line, and
  the compiler's own hint if it has one: "main.rs, line 42: this name
  isn't known here. Did you mean `jump_speed`?" with Open the file and
  "Copy this for an agent". The full text is behind Show details.
- Debug builds are used (quick to build). *Measured:* Duck Craft takes
  9.9 ms a frame in a debug build and 8.0 to 9.0 in a release build, on
  this machine's CPU, so debug is fast enough.

**When something is missing** the Studio explains and offers the official
page. It never installs anything by itself.

| What it finds | What it says and offers |
|---|---|
| No `cargo` (it also looks in `.cargo\bin` in your user folder) | The "Duck Forge needs Rust" screen above, with rust-lang.org/tools/install |
| The build fails with "linker `link.exe` not found" | "Rust needs Microsoft's free C++ Build Tools to finish a build. The Rust installer usually adds them." A button to Microsoft's download page, and Check again |
| Rust is older than the engine needs | "Your Rust is from <date>; Duck Forge needs a newer one." A button that runs `rustup update` with its output shown |
| The parts can't be downloaded (first build, no internet) | "Couldn't download the parts Duck Forge is built from. Check the internet connection, then Try again." |

## 2. The command layer

Every action is one value of one type, `Command`, in
`studio/crates/core/src/commands.rs`. A button, a key, a test, a script
file and (in Phase 7) an MCP tool all call the same
`studio.run(command)`, which returns what changed or a plain-language
problem (what happened, a next step, and the raw details).

```json
{"do": "level.paint", "cells": [[14, 9], [15, 9]], "tile": "plank"}
{"do": "feel.preset", "name": "Fast and bouncy"}
{"do": "game.freeze"}
```

| Group | Commands |
|---|---|
| home | `open`, `choose_folder`, `check_again` |
| game | `new`, `open`, `build`, `play`, `freeze`, `toggle` (Tab), `step`, `restart`, `replay_my_moves`, `pop_out`, `bring_back`, `stop`, `input` |
| level | `paint`, `erase`, `fill`, `move_area`, `place_thing`, `move_thing`, `delete_thing`, `resize`, `play_from` |
| feel | `set`, `preset`, `show_code` |
| check | `run`, `show_problem` |
| clip | `make` |
| assets | `open_in_quill`, `play_sound`, `show_recipe` |
| edit | `undo`, `redo`, `snapshot`, `go_back_to` |
| console | `run` (a typed command) |

One table gives each command its name, a plain title, what it needs, and
whether Undo covers it. The MCP tool list is made from that table later,
so a new command is one new entry. `DuckForgeStudio --run script.json`
runs a list of commands with no window; Step 5 and agents use that. A
script can also wait: `{"wait": "playing"}`, `{"wait": "frames",
"frames": 120}`, `{"wait": "idle"}`. (The level, feel, assets and edit
commands arrive with Steps 3 and 4.)

## 3. The game link

**Transport: a loopback TCP connection.** The Studio listens on
`127.0.0.1` on a free port and starts the game with
`--studio 127.0.0.1:<port>` and a one-time key. The game connects back.
It's plain `std`, the same on Windows and Linux, needs no new library,
and leaves the game's own printing alone (the Studio keeps that for Show
details).

*Measured* with a throwaway program (not in the repository): ask another
process for a 320 x 240 frame (300 KB) and read it back, 3,000 times, on
this Linux machine.

| | Average | 99 in 100 under | Slowest |
|---|---|---|---|
| Loopback TCP | 0.15 ms | 0.4 to 0.6 ms | 2.7 to 3.5 ms |
| stdin/stdout pipes | 0.30 ms | 0.6 to 0.7 ms | 3 to 20 ms |

0.15 ms is under 1% of a 16.7 ms frame, so shared memory isn't needed.
It must be measured again on Windows (Step 2).

**Messages.** Each is a kind, a length, then the body. Everything is JSON
text except the picture.

| Studio to game | What it does |
|---|---|
| `tick` | "Here are the inputs since last time; run N updates; send the picture." Inputs are input-script events without the frame number |
| `run` | Run N updates without drawing (fast). Used by "replay my moves" now, the future trail later |
| `file` | A file the game loaded has changed: here is its new content |
| `tune` | Feel values to change |
| `view` | Draw the frozen moment through this camera, at this size (Build mode's pan and zoom) |
| `ask` | Send back the scene (labels and positions), reported values, tune values, files loaded, or sounds played |
| `sound` | Play this sound |
| `window` | Open your own window, or close it (Pop out) |
| `quit` | Call the game's `quit`, then end |

| Game to Studio | What it holds |
|---|---|
| `hello` | The game's title, size, updates per second, seed, renderer, and the files it loaded |
| `frame` | The frame number, which tick it answers, how long update and draw took, whether the game wants the pointer, then `Canvas::pixels()` as it is |
| `answer` | The reply to an `ask` |
| `applied` | While popped out: the inputs the game's own window applied, by frame |
| `bye` | Why it's ending |

Between them these already cover what later phases need from the link:
stopping and carrying on at an exact frame (`tick`), loading changed
data into a paused game (`file`), changes recorded by frame, fast runs
without drawing (`run`), and positions (`ask`).

**Who keeps time: the Studio.** While playing, each time the Studio draws
itself it works out how many 60-a-second updates are due (the same rule
as the engine's own `Pacer`), sends one `tick`, lays out its panels, then
waits for the frame just before it paints the game. One clock means no
slow drift between two 60 Hz clocks, and the Studio always knows which
frame each input landed on. That is also what exact freeze and resume
need.

**When the Studio can't draw** (on Windows, dragging or resizing a window
blocks its drawing), no ticks are sent, so the game simply waits on its
frame: a clean freeze. The game holds its sound after a tenth of a second
without a tick. When drawing comes back, a gap longer than a tenth of a
second is skipped, not caught up on: the game carries on with one update.
A shorter hiccup is caught up on, but never by more than three updates in
one go.

**Delay.** If the frame comes back before the Studio paints (the game's
frame time plus about half a millisecond for the link, as measured; the
Studio waits up to 10 ms), the picture is on screen in the same refresh
as in the game's own window: no added delay. If it's late, the last
picture stays one more refresh and the new one shows next time: one frame
of added delay, never more.

**How it's measured.** The Studio stamps every tick, and the frame says
which tick it answers. A Timing line (off by default) shows, over the
last five seconds: frames per second, the longest gap between new
pictures, the game's own update and draw time, the link's time, and how
many frames were late. The target is 60, no gap over 20 ms, and under 1
late frame in 100, for Duck Peaks, the 3D runner and Duck Craft. Step 2
reports these from the Windows build machine (which has no graphics
card, so it draws on the CPU) and you read them on your
PC (see the checklist at the end). Until there's a Studio to measure,
the delay above is worked out, not measured; only the link's 0.15 ms and
the games' frame times are.

**Showing the picture.** The pixels go into one texture, drawn with
nearest-pixel sampling at the biggest whole number of *screen* pixels
that fits, so a game pixel is a sharp square at 100%, 125% and 150%
Windows scaling. Black fills the rest.

**The mouse, and Duck Craft.** The game never touches the real pointer
in the Studio. It only says "I want the pointer held"
(`ctx.lock_pointer(true)`, as now), and the Studio does the holding, the
same way `window.rs` does it today:

- The pointer over the game is sent as a position in game pixels. Clicks
  and the wheel are sent as they are.
- When the game wants the pointer and you click in the game, the Studio
  hides it, keeps it inside the window, and puts it back in the middle of
  the game every frame (on Windows; other systems lock it where they
  can). Mouse movement then goes to the game as `mouse_motion` in game
  pixels, the same numbers `window.rs` gives, so looking round feels the
  same as in the game's own window.
- **Tab while the pointer is held:** the game freezes on that frame. The
  Studio lets go of the pointer and shows it in the middle of the game,
  ready for the Build tools. The game isn't told anything, because it
  isn't running, so it can't react (no pause menu opens).
- **Tab again:** the Studio holds the pointer again before the first new
  update. Movement made while frozen is thrown away, so the view doesn't
  jump. Keys you let go of while frozen are released on that first
  update, so nothing sticks.
- The Studio also lets go when its window stops being the active one
  (Alt+Tab, the Windows key): it freezes, and stays frozen until Tab.
- The Studio lets go if the game stops asking (Duck Craft's inventory),
  stops, hangs or is popped out. Because the Studio owns the pointer, a
  stuck game can never keep it.
- Tab belongs to the Studio in every game. No example uses it today.
  Escape goes to the game (Duck Craft's own menu).

**Sound.** The game plays its own sound, as in a window. While frozen,
loops fade out over one block and wait; they fade back in on resume.
That's outside the game's state, so replays aren't affected.

**Crash and hang.** If the game's process ends or the connection closes
without a `bye`, the Studio shows "The game stopped" with the reason in
one sentence (the first line of the game's error), Restart, and "Restart
and replay my moves". If a tick gets no answer for two seconds, it shows
"The game isn't answering" with Wait and Restart; Restart ends the
process. If the Studio itself closes or dies, the game sees the
connection close, calls `quit` and ends.

**Pop out.** The same game, on the same frame, opens its own window (the
`window.rs` code) and keeps its own time. The Studio shows "Playing in
its own window" and Bring it back. Feel, level changes and Check still
work, and the inputs still go into the session's recording.

**Three engine fixes the Studio needs** (from Duck Craft's issue list):

- 16: `ctx.is_headless()` and `ctx.in_studio()`. Headless is true for
  `--headless`, fuzz runs and tests.
- 17: `Game::quit(&mut self, ctx)`, empty by default, called once on
  every way out: the window's close button, `ctx.quit()`, `--frames`
  running out, the Studio stopping or restarting the game. (Not when the
  program is killed or crashes.)
- 18: Everything after a second `--` is the game's own:
  `-- --seed 3 -- --new-world`. The game reads it with
  `duckforge::game_options()`.

## 4. Freeze, edit, resume

**Freeze** is the Studio not sending ticks. The game's program stays
running and waits, so everything is kept exactly: positions, speeds,
the frame number, the random numbers, held keys, timers. Nothing is
saved and loaded back, so nothing can be lost or rounded.

**Edits reach the frozen game** through one new hook:

```rust
/// A file the game loaded has changed. Load it again and carry on.
/// Return false (the default) if the game can't do that without a restart.
fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool { false }
```

The engine notes every file a game loads (levels, legends, sprite
sheets, tilesets, sounds), so it knows which changes matter. The three
starters with levels get a short `reload`: read the level again, keep the
duck, the score and what's been collected, and add or remove only the
things that changed (`old_map.spawn_changes(&new_map, "coin")`). A game
without the hook gets "This game reads its files only when it starts"
with Restart.

The frozen game then draws once more, so you see the plank appear under
the duck before you press Tab.

**How a session still replays exactly.** The Studio saves every session
as an input script in `out/sessions/<game>/`, with three additions to
the format (old scripts read exactly as before):

```json
{ "duckforge_inputs": 1, "seed": 7, "updates_per_second": 60,
  "files": { "examples/bee_maze/level.txt": "....\n..p.\n####\n" },
  "events": [
    {"frame": 380, "press": "Space"},
    {"frame": 413, "file": "examples/bee_maze/level.txt",
                   "text": "....\n..p=\n####\n"},
    {"frame": 413, "tune": {"jump_speed": 380}} ] }
```

- `"files"` holds the text files as they were on frame 1, so a replay
  doesn't depend on what's on disk now.
- A `"file"` event is a change, applied just before that update by
  calling `reload`. Many edits made during one freeze become one event:
  the game never ran on the ones in between.
- A `"tune"` event is a Feel change, applied just before that update.
- Changed pictures and sounds are kept as copies next to the script and
  named in the event (`"copy": "413-tiles.png"`).

A replay gives the engine the same files, inputs and changes on the same
frames, so it plays out the same. A test in Step 2 freezes a real game,
steps it, resumes, then replays the saved session headless and compares
the last picture, pixel for pixel. Step 3 adds the level change in the
middle to that test.

A session with changes in the middle can be replayed and clipped, but it
can't be saved as the game's playthrough (a playthrough is inputs on the
level as it is now). "Keep this run" says so.

**Clip** replays the session headless and saves its last ten seconds
(`--clip FILE`, a new engine option anyone can use). With `ffmpeg`
installed it's an MP4 with sound. Without, it's a GIF: full size, sharp,
at the game's own speed (a GIF can show at most 50 pictures a second, so
a 60-a-second game leaves out every sixth frame), with a line saying
"install ffmpeg for sound" and the official page. `ffmpeg` isn't bundled.

## 5. Levels as files

The level text and its legend move out of `main.rs`, next to the game:

```text
examples/bee_maze/level.txt            the letters, exactly as now
examples/bee_maze/level.legend.json    what each letter means
```

```json
{ "tile_size": 16,
  "tiles": "assets/tiles.json",
  "letters": {
    "#": {"kind": "solid",      "name": "grass"},
    "=": {"kind": "one_way",    "name": "plank"},
    "~": {"kind": "decoration", "name": "water"},
    "p": {"kind": "thing", "name": "player", "label": "Start",
          "at_least": 1, "at_most": 1, "start": true},
    "c": {"kind": "thing", "name": "coin", "label": "Coin"},
    "g": {"kind": "thing", "name": "flag", "label": "Flag",
          "at_least": 1, "at_most": 1} } }
```

- One call loads both: `TileMap::load_level("examples/bee_maze/level.txt")`.
  `Legend::new().solid(...)` in code keeps working.
- The names are the tileset's names, as now, so the palette shows each
  tile's own picture. `label` is what the Studio calls a thing.
- Loaded at start, and again when it changes: in the Studio through the
  link, in a plain window by looking at the file twice a second (like the
  tune file today). So hand edits show up either way.
- Converted: `template` and `duck_peaks` (their `LEVEL` text) and
  `starter_topdown` (its `MAP`, which also moves onto `TileMap`). A test
  checks each file reads to the same map as the old text before the old
  text is deleted, and each game's pictures and playthrough stay the same.
- `new_game` already copies the starter's folder and renames its paths,
  so a new game gets its own level files.
- Games without a tile level are left alone. Build mode offers Feel and
  says so.

**Build mode's picture is the game's own drawing.** Frozen, you see the
frame the game drew. When you pan or zoom, the Studio asks the frozen
game to draw the same moment through the Studio's camera at the size
needed (`view`), still at a whole-number scale. The Studio only draws
what's on top: the grid, the selection, the tile under the pointer. So a
level never looks different in Build and Play. This is the riskiest part
of Step 3. If it doesn't hold up, the Studio draws the tiles with the
engine's own `TileMap::draw`, which gives the same pixels for tiles but
not for backgrounds.

**Play from here** restarts the game with the Start thing moved to the
tile you clicked, for that run only. The file isn't changed.

## 6. Play tests that survive level edits

Today's tests break when a tile moves: "8 coins", "the duck's feet are at
y = 208", a playthrough recorded key by key. Three changes:

1. **Tests of how the game plays use their own tiny levels**, written in
   the test (`TileMap::parse("p..=..\n######")`): jumping, the plank, the
   wall, the water. They never see the real level, so editing it can't
   break them.
2. **Tests of the real level check what must stay true:**
   - the legend's rules hold: one Start, at least one Flag, every letter
     known (`map.broken_rules()`, from `at_least` and `at_most`);
   - every coin and the flag can be reached: `map.reach(start, jump)`
     works out where the duck can stand, from how high and far it jumps
     (taken from the Feel values). It's cautious, so a miss is a warning,
     "a coin looks out of reach", with the spot shown;
   - **the bot can still win.** The starters' bots already look at the
     game, not at a script, so the test plays the bot live on the current
     level and checks it ends "Won" with every coin. That's the real
     proof, and its failure says where the bot got stuck.
3. **The recorded playthrough knows which level it was made on** (a short
   fingerprint in the script). After an edit, Check says "The recorded
   playthrough is from an older level" and offers Record it again with
   the bot, instead of failing.

## 7. Live Feel

Values stay where they are: `ctx.tune("jump_speed", 330.0)` in code and
`examples/<game>.tune.json` on disk. One new optional file describes
them, `examples/<game>.feel.json`:

```json
{ "values": {
    "jump_speed": {"label": "Jump height", "group": "Duck",
                   "from": 150, "to": 500, "step": 5},
    "gravity":    {"label": "Gravity", "group": "World",
                   "from": 300, "to": 1500, "step": 10} },
  "presets": {
    "Fast and bouncy": {"run_speed": 150, "jump_speed": 380},
    "Floaty":          {"gravity": 500, "jump_speed": 260} } }
```

- Every value the game asks for shows up, described or not. Without a
  description it gets its name tidied ("jump_speed" becomes "Jump
  speed") and a range of nothing to twice its default.
- Moving a slider sends the value over the link at once (recorded as a
  `tune` event) and writes the tune file shortly after. Editing the tune
  file by hand moves the slider.
- A preset is several values at once, and one step for Undo.
- **Show me the code** finds `tune("jump_speed"` in the game's files and
  shows that line with a few around it, and Open the file.

## 8. Undo and history

- **No Save button.** Every edit goes to its file as it's made (written
  whole, then swapped in, so a crash can't leave half a file).
- **Undo and Redo** step through this session's edits (paint strokes,
  moved things, Feel changes), across freezes. A change made outside the
  Studio becomes one step, "Changed outside the Studio".
- **History** is plain folders, `history/<game>/0007 Painted 14 tiles/`,
  each holding the files that changed since the one before and a small
  list of the rest. A snapshot is taken when you go from Build to Play
  after changes, on a preset, before a Check, before going back, and
  every five minutes while there are changes (the autosave).
- **Go back to here** first takes a snapshot of now, then puts the old
  files back, so going back can itself be undone.
- It covers the game's folder, its tune and feel files and its
  playthroughs. No git is needed or touched; `history/` is ignored by git
  for those who use it. The oldest snapshots are thinned to one a day.

## 9. Duck Quill and Duck Echo

- **Open in Duck Quill:** double-click a tile or sprite and the Studio
  starts `DuckQuill.exe` with that picture (Quill already opens a file
  named on its command line). The first time, it looks for Quill next to
  its own folder, then asks "Where is Duck Quill?" with a file picker and
  a link to its download page.
- **Picked up live:** when the sheet's file changes, the palette and the
  running game load it again (the `reload` hook; starters reload their
  art in one line).
- **Sounds:** the Sounds list is the `.wav` files the game loaded plus
  the synthesizer sounds it has played. Play asks the game to play it, so
  the Studio has no sound code of its own.
- **Recipe:** for a synthesizer sound, its `Sound { ... }` values as
  code. For a `.wav`, the Duck Echo recipe `art/<name>.toml` if there is
  one, or the line of `art/README.md` that names the file. Otherwise "No
  recipe was saved for this sound."

## 10. What's left out, and room for later

Left out of Phase 4: sharing or exporting a finished game; editing code
inside the Studio (it opens the file, or copies the error for an agent);
more than one level per game; tile layers; painting 3D worlds; gamepads;
macOS; installing or updating Rust or Duck Forge for you.

Room left for later phases, without building it now:

- **Phase 5 (games without code):** behaviours and rules would be more
  text files the game loads, changed through the same `file` message and
  `reload` hook.
- **Phase 6:** `run` (fast updates without drawing) plus the exact
  session recording give the future trail and timeline scrubbing: restart
  and run to any frame. Several games can run at once, one link each, for
  side-by-side versions and ghost ducks. `ask` already reports positions.
- **Phase 7 (the Rubber Duck):** each command becomes an MCP tool from
  the command table. `--run` already drives the Studio with no person.

## Decided

The open questions, as answered in the review:

1. **Where games live.** In `examples/` of one Duck Forge folder, for
   Phase 4. The folder is a Studio setting and isn't hard-coded, so games
   can move out of the engine's folder later.
2. **Tab always belongs to the Studio.** A game can't use Tab inside the
   Studio (it still can in its own window).
3. **Clip.** An MP4 with sound when `ffmpeg` is found; otherwise a
   full-size, full-speed, sharp GIF and a line saying "install ffmpeg for
   sound" with the official page. `ffmpeg` isn't bundled.
4. **Duck Craft** starts a fresh island and doesn't save inside the
   Studio, and says so on screen: "Inside the Studio, Duck Craft starts a
   fresh island and doesn't save" (the game gives the line with
   `ctx.studio_note`, and the Studio shows it under the top bar).
5. **Rust.** The official installer; the other Windows flavour is only
   mentioned in Show details.
6. **Duck Echo** has no window app yet, only its command line. Recipes
   are shown as text.
7. **Home.** The examples that come with Duck Forge are under "Examples
   that came with Duck Forge"; everything else is under My games.

And four changes: the game never stutters or fast-forwards when the
Studio can't draw (section 3); automatic builds wait for the files to
have been unchanged for a second (section 1); every screen is
screenshotted headless at each stop (`docs/decisions.md` says how); and
the Console panel (section 1).

## What you test on Windows after Step 2

I can check that a Windows build compiles here, but not link or run it.
So the release workflow does that on one of GitHub's Windows machines,
every time it runs, with no screen: it runs the Studio's tests (they
build a real game, play it through the link, freeze and resume it, and
replay the session exactly), builds `DuckForgeStudio.exe`, plays three
games through the link and prints their Timing lines, makes the zip, and
then walks the beginner's path in the unzipped folder: a new game from a
starter, its first build, its first playthrough and Check, and a clip.
What that can't show is anything that needs a real screen, mouse, sound
card or graphics card. These need your PC:

1. The zip opens, `DuckForgeStudio.exe` starts, and no black console
   window appears. (Windows may say "Windows protected your PC" the first
   time: More info, Run anyway.)
2. Starting a game shows **no** Windows Firewall question.
3. Open Duck Peaks: the first build shows the progress bar and finishes.
   Change a number in its `main.rs` and save: it rebuilds by itself while
   the game is still playing.
4. Turn on Timing. For Duck Peaks, the 3D runner and Duck Craft, write
   down: updates and pictures a second, longest gap, late frames, and
   the renderer's name (it should name the RTX 3090, not the CPU).
5. The pixels are sharp squares with Windows scaling at 100%, 125% and
   150%, and after dragging the window to another screen.
6. Duck Craft: click in the game. The pointer hides, and looking round
   is smooth and as fast as in Pop out. The pointer never escapes to a
   second screen.
7. Duck Craft: Tab. The game freezes and the pointer comes back in the
   middle. Tab again: the view doesn't jump, and no key is stuck.
8. Alt+Tab away and back while playing: the game is frozen, and the
   pointer is free.
9. Sound comes from the game, goes quiet when frozen, and the game's
   closing line says `0 late refills`.
10. End the game from Task Manager: "The game stopped" appears, and
    Restart works. Close the Studio: no game is left in Task Manager.
11. Pop out opens the game in its own window on the same frame, and
    Bring it back returns it.
12. On a Windows account without Rust (if you have one to try): the
    "Duck Forge needs Rust" screen appears, and the button opens the
    official page.
13. While the game plays, drag the Studio's window around for a few
    seconds, then let go. Do the same resizing it. The game should
    freeze cleanly while you drag (no stutter, and its sound goes quiet)
    and carry on at normal speed the moment you let go, with no burst of
    fast-forward. The frame number in the status bar should not jump.
14. Home, New game: pick the platformer, give it a name. It opens and
    plays, and a little later its card on Home says "Checks pass".
15. Check: the list appears. Click the playthrough's line: its picture
    shows.
16. Clip: it says where the file is, and "Show it in its folder" opens
    File Explorer there. Without ffmpeg it's a GIF, with a line about
    how to get sound.
17. Console: it shows the command behind each button you pressed. Type
    `cargo --version` and press Run: the version appears.

## What you test on Windows after Steps 3 and 4

The release workflow's tests now also paint a real level while the game
is frozen, change its jump, undo both, and replay the session to the same
picture. These need your PC and your mouse:

18. Open the platformer you made (or Duck Peaks), press Tab, pick Brick
    and drag across a few tiles: the bricks appear in the frozen picture
    as you drag (a few times a second), on the grid's squares exactly.
    Right-drag erases. Tab: the duck bumps into them.
19. Things: pick Coin and click an empty tile; drag the Start somewhere
    else. Select: drag a box, then drag the box. Fill: click the sky.
    Ctrl+Z undoes each stroke, Ctrl+Y redoes it.
20. Whole level: the whole level shows; painting there shows up in the
    game view too. Zoom + and - keep the tiles sharp.
21. Play from here: click a tile; the game starts again from there, and
    `level.txt` is unchanged.
22. Feel: move Jump height while frozen, Tab, jump: higher. The tune file
    `examples/<game>.tune.json` has the new value. Presets work. "code"
    shows the line, and Open the file opens it in your editor.
23. Sounds: Play plays the game's sound; Recipe shows how it was made.
24. Pictures: with Duck Quill next to the Studio (or chosen with
    Choose...), clicking tiles.png opens it in Duck Quill. Change a tile
    and save: the palette and the frozen game show it.
25. History: after some edits and a Tab, History lists snapshots. Go back
    to an earlier one: the level is as it was, and the game shows it.
26. Edit `level.txt` in Notepad while the game is frozen in the Studio and
    save: the Studio shows the change, and Undo says "Changed outside the
    Studio".

## What you test on Windows after Step 5

The release workflow now walks the whole beginner's proof on Windows with
no window (`studio/proof/beginners-proof.json`). By hand, with your mouse:

27. A new platformer, Feel preset "Floaty", then Check: Checks pass (the
    bot plays with the Feel as it is).
28. Drag Jump height down to about 150 and Check: "The bot can't win with
    the Feel values as they are now", with why. The preset "As it was
    made" puts it right.
29. Put a coin high in the sky and Check: "The bot can't win on the level
    as it is now". Click the line: a picture with the coin ringed, and
    the game still at a good size beside it. Point at a tile in Build
    mode: the status bar says its column and row, as Check does. Paint
    planks up to the coin, Check again, then "Record it again".
30. Make a new game and start painting at once, before it has finished
    getting ready; then Check. It never says "could not read
    playthroughs": the bot records one first.
31. Run, jump, and press Tab in mid-air. With Select, drag a plank under
    the duck. Tab: it lands on the plank.
32. Duck Craft: the mouse turns the view; Tab gives the pointer back.
