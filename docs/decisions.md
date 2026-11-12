# Design decisions

This file explains, in plain language, the choices made while building
duckforge, and why. Each step has its own section, oldest first.

It has three parts:

- **Headless** (Steps 1 to 6): running games without a screen.
- **Phase 1**: comfortable for two kinds of game makers, a person
  learning Rust and an AI agent building games headless (sprites,
  transparency, a camera, sound, text, helpers, the debug overlay).
- **Phase 2**: efficient for an AI agent. Fewer rebuilds, fewer images,
  fewer commands and less reading per good game; mostly tooling around
  the engine.

## Step 1: The same inputs always give the same game

The goal: if you play a game twice with the same inputs, you get exactly
the same game, down to the last pixel. Replays, automatic tests and an AI
checking its own work all depend on this.

**A fixed timestep is now the default: 60 updates per second.** Before,
`update()` got `dt` = however long the last frame really took. Frame times
wobble a little, so the ball took slightly different steps on every run,
and differently again on a faster computer. Now every update moves the
game forward exactly 1/60 of a second. If the computer can only draw 30
frames a second, the loop runs two updates before each frame, so the game
keeps its speed. On a 120 Hz screen, every other frame gets no update.
`Config::timestep` chooses: `Timestep::Fixed { updates_per_second }` or
`Timestep::RealTime`, which is the old behaviour.

**The stress test keeps real time.** It measures frames per second by
counting updates. With a fixed timestep, updates and drawn frames are no
longer the same thing, so it asks for `Timestep::RealTime`.

**Frames a little early or late still get exactly one update.** Real
frames are never exactly 1/60 s apart. A simple fixed-timestep loop gives
one frame 0 updates and the next 2 when a frame is slightly early, and
that looks like a stutter. The loop's `Pacer` keeps the leftover time
halfway between two updates, so a frame can be up to 8 ms early or late
and still get one update.

**After a long freeze, the game slows down instead of catching up.** If a
frame took more than 0.1 s (the window was dragged, or the computer is too
slow), the missing time is dropped. Catching up would mean running dozens
of updates in a row, which makes the next frame even later.

**A seed in `Config`.** `seed: Some(n)` makes `ctx.rng` give the same
numbers on every run. With `None`, a game in a window gets a seed from the
clock, as before, so every game is different.

**`Simulation`: one core for windows, tests and headless runs.** Instead
of writing a second game loop for tests, the game now always runs inside a
`Simulation`, which knows nothing about windows or clocks: you press keys
and call `step()`. The window's loop is a thin layer around it that adds
the clock and the screen. A game can't behave differently in a test than
it does on screen, because it's the same code.

**A `Simulation` with no seed uses seed 1, not the clock.** In a window,
a new seed each time keeps the game fresh for the player. In a test there
is no player, and a test that gives a different answer each time it runs
is worse than useless. So a `Simulation` is repeatable unless you ask for
something else.

**Drawing only when a picture is needed.** `Simulation` calls the game's
`draw()` only when you ask for `picture()`. That's safe because `draw()`
takes `&self` and can't change the game, and it makes long test runs much
faster.

**A `Simulation` always draws 3D on the CPU.** Graphics cards from
different makers can differ by a pixel here and there, and a cloud machine
usually has no graphics card at all. The CPU renderer gives the same
picture everywhere.

**F3 and F12 are read straight from the keyboard.** They used to be checked
through the game's input after each update. With a fixed timestep a drawn
frame can have zero updates (or two), so the window now checks them once
per drawn frame itself. Games still see these keys too, as before.

**A bug found on the way: nearby seeds gave nearly the same game.** The
first test played Breakout with seed 7 and with seed 8, expecting
different games, and got identical pictures. The random number generator
(xorshift) starts badly from small seeds: its first numbers from seeds 7
and 8 are both about 0.0004, so the ball was served at almost the same
angle. `Rng::new` now scrambles the seed first, and a test checks that
seeds 1 to 1000 give first numbers spread over the whole range. Side
effect: the stress test's random layout changed a little, so the triangle
counts in the README's benchmark table changed by less than 2%. The FPS
figures there were measured with the old layout and were left as they are.

**What "the same" covers.** Same game, same inputs, same seed, same
computer and build: identical. On a different operating system, maths
functions like `sin` can round the last bit differently, so a long game
can slowly drift. A game also stops being repeatable if it makes decisions
with its own clock (`std::time::Instant`) or with the order of items in a
`HashMap`. Use `ctx.dt()`, `ctx.time()` and `ctx.rng` instead.

## Step 2: Running without a screen

The goal: build and run games on a plain Linux machine (like a cloud server
where an AI writes games) with no screen, no window libraries and no
graphics card, and still see what the game drew.

**The window is a cargo feature, `window`, on by default.** All the code
that uses minifb moved into `src/window.rs`, which is only compiled with
the feature. `cargo build --no-default-features` leaves out both the window
and the GPU renderer; the engine then needs nothing from the system but the
C library. We checked this two ways: `cargo tree` shows no window crates,
and `ldd` shows the program links no X11, Wayland or graphics libraries.

**Every game gets command-line options for free.** `run()` reads the
command line itself, so `--headless`, `--frames`, `--out`, `--seed` and
`--help` work in every game without changing its code. We thought about
making games opt in, but the point is that an AI (or a person) can test
*any* game the same way.

**Options the engine doesn't know are left for the game.** The stress test
has its own options (`--objects`, `--seconds`, `--renderer`). So unknown
words aren't errors; they're handed to the game, which reads them with
`duckforge::game_args()` (the command line minus the engine's options).

**...but near-misses of engine options are errors.** If unknown options
were silently ignored, a typo like `--frame 60` would run the default 600
frames, and you might not notice. So a word within two letters of an
engine option stops the program: `unknown option --frame: did you mean
--frames?`.

**`--frames` works in a window too**: the window closes after that many
updates. Handy for scripted checks. `--out` only makes sense without a
window, so using it there is an error rather than being ignored.

**Headless runs as fast as it can, not in real time.** There's nobody
watching, so there's no reason to wait: 10 seconds of Breakout takes a
few milliseconds. Because steps are fixed (step 1), the result is the same
as playing in real time.

**Headless always draws with the CPU**, even when the GPU feature is on,
for the same reasons as `Simulation` in step 1: the same picture on every
computer, and no graphics card needed.

**A headless run with no seed uses seed 1.** Running the same command twice
should give the same result; for a different game, pass `--seed`.

**Headless saves to `duckforge-out/` unless told otherwise**, and prints
exactly what it ran and which files it saved, so whoever ran it (person or
AI) knows where to look. The folder is in `.gitignore`.

**PNG, not BMP, for pictures.** PNG files are compressed without losing
anything (a Breakout frame is about 1.3 KB as PNG and 300 KB as BMP), and
every phone and browser can show them. We used the small `png` crate
rather than writing our own encoder: PNG needs a compression algorithm
(deflate) and checksums, which would be a big detour. F12 screenshots are
now PNG too. `Canvas::save_bmp` still works.

**Clear errors, printed without quotes.** When `main` returns an error,
Rust prints it with `{:?}`, which puts quotes around a plain string and
shows line breaks as `\n`. The engine's errors use a small `EngineError`
type that prints the message as it is. And if a window can't open, the
error now suggests `--headless`, because that's what you need on a
machine without a screen.

## Step 3: Recording and replaying inputs

The goal: a person's playthrough can be replayed by a program with no
screen, and a program's playthrough can be watched by a person, exactly.

**JSON, one event per line.** We chose JSON over a made-up text format
because every language and tool can read and write it, and an AI writes it
without thinking. The file is written by hand-made code, not a JSON
library's pretty-printer, so each event fits on one line
(`{"frame": 12, "press": "Space"}`). That keeps recordings short to read
and makes two recordings easy to compare. The format is in
`docs/input-format.md`.

**Events are "changes", not "what's held each frame".** A recording lists
when keys go down and up, not the full keyboard on every frame. That's how
people think about it ("hold Right from frame 30 to 90"), it keeps files
small, and it's easy to write by hand.

**Frames count from 1, like `ctx.frame()`.** An event on frame 10 is seen
by the game's tenth update. Frame 0 is refused with an explanation, rather
than quietly treated as frame 1.

**Strict reading, friendly messages.** A misspelled key, an unknown field
(`"preess"`), two actions in one event: each is an error that says which
event and what's wrong, and suggests the fix. Ignoring a mistake would mean
a replay that quietly does something else. The JSON is read with
`serde_json` into general JSON values and checked by hand, which gives
better messages than mapping it straight onto Rust types.

**Key names are case-insensitive.** `"space"` and `"Space"` both work.
Recordings are always written in the engine's own spelling.

**One path for every input.** In `Simulation`, `press`, `release`,
`move_mouse`, a replayed script and the window's real keyboard all go
through one method, `apply`. It remembers every change, so recording works
the same way however the input arrived, and replaying a recording records
the same events again (a test checks this).

**A recording always starts at frame 1.** You can press F10 halfway
through a game, but a replay has to start from the beginning: where the
ball is when you pressed F10 depends on everything before. So the engine
always remembers the inputs from the start (a few bytes per key press), and
the saved file covers the whole game up to the second F10. We thought
about recording only from F10 onwards, but such a file couldn't be
replayed correctly.

**The recording carries the seed.** A game in a window gets a new seed
every run. The recording stores it, and a replay uses it, unless `--seed`
says otherwise. The timestep is stored too; a replay at a different
timestep prints a warning, because the same inputs would land at different
moments of the game.

**The engine's own keys aren't recorded.** F3, F10 and F12 belong to the
window (FPS counter, recording, screenshots). Recording them would make a
replay start recording or take screenshots. Games still see these keys, as
before.

**During a windowed replay, the player's keys are ignored**, except the
engine's keys, so you can still take screenshots or record. When the script
runs out, the player takes over, and the engine says so.

**Closing the window saves a recording in progress.** Losing a long
playthrough because you closed the window would be painful.

**Checked end to end.** We played Breakout in a window (in a virtual
display), took a screenshot, then replayed the recording without a window:
the last frame was identical, pixel for pixel. Watching the same recording
in a window gave the same picture again.

## Step 4: Seeing the game

The goal: after a headless run, a person or an AI can quickly see what
happened, from pictures or from numbers, without watching the whole game.

**Five kinds of pictures, each for a different question.**
`last.png` answers "how did it end?". `--snapshot-every N` gives numbered
frames for "what happened around frame 300?". `--sheet` puts many frames
in one grid for "what happened over the whole run?", which is the quickest
way to spot a game that froze, a ball that flew off, or a level that never
changed. `--gif` is for "does it *move* right?", and plays on a phone.
`--crop` with `--zoom` answers "exactly which pixels are lit here?".

**The sheet shows 16 frames spread evenly, or the snapshots.** Sixteen
frames in a 4 x 4 grid fit comfortably on one screen. With
`--snapshot-every`, the sheet shows those frames instead, so the sheet and
the files match. It always ends on the last frame, even when the game
quit early. Above 64 frames, the sheet skips some, because cells would get
too small to read.

**Labels say the frame and the time.** "FRAME 600  10.0S", drawn with the
engine's own pixel font. The frame number is what scripts and logs use;
the time is what people think in.

**The GIF is shrunk and slowed to about 20 pictures a second.** A full-size
60-frames-a-second GIF of 10 seconds of Breakout would be several
megabytes. Shrinking to at most 200 pixels wide and keeping every third
frame gives about 400 KB that still shows the motion. Shrinking averages
each block of pixels, so thin lines fade rather than vanish. Long runs keep
their real speed but get choppier: at most 600 pictures.

**Close-ups enlarge by copying pixels, with no smoothing.** Each pixel
becomes a sharp `zoom` x `zoom` square, so you can count pixels: is the
ball touching the paddle, or one pixel above it? `--crop` without
`--zoom` enlarges 4 times; `--zoom` without `--crop` enlarges the whole
picture.

**`ctx.report("score", 12)`: games say what's going on, in words.**
Pixels are a slow way to learn the score. A game can report numbers,
yes/no values or text, and a headless run saves them in `log.json`.
In a window, reports are thrown away, so games can simply report every
frame.

**The log keeps only changes.** If a game reports its score every frame,
600 frames would mean 600 copies of the same number. `log.json` lists a
value only on the frames where it changed, so a whole game of Breakout
reads like a story in ten lines: launched on frame 30, brick on frame 85,
life lost on frame 149... It also has `final`, the last value of
everything, which is usually what a test wants.

**`log.json` also says what was run.** Game, seed, frames, whether the
game quit, how many inputs were replayed, and which files were saved, so
the folder explains itself.

**Old files are removed, but only ours.** If a run with
`--snapshot-every 10` is followed by one with `--snapshot-every 60`, the
old `frame-000010.png` would still be there, and look like part of the new
run. So each run deletes the files it would make (`frame-NNNNNN.png`,
`sheet.png` and so on) before starting. Any other file in the folder is
left alone.

**The example playthroughs were made by a bot.** `playthroughs/breakout-win.json`
and `breakout-lose.json` were recorded by a small program in Breakout's
tests that plays through `Simulation`: it predicts where the ball will
come down, moves the paddle there, and hits the ball off-center to aim at
the lowest brick left (or, to lose, it moves away from the ball). It won a
perfect game in 58 seconds. Run
`cargo test --example breakout -- --ignored make_playthroughs` to make new
ones if Breakout changes.

## Step 5: Automatic play tests

The goal: a plain `cargo test` can play a game (usually by replaying an
input script) and check how it went, using the values the game reports.

**A small builder, `PlayTest`.** A test reads like a sentence:

```rust
let played = PlayTest::new(config(), Breakout::new())
    .inputs("playthroughs/breakout-win.json")
    .run();
assert_eq!(played.text("state"), "Won");
assert!(played.number("score") >= 10.0);
```

`.inputs(file)` or `.script(json)` choose the inputs, `.frames(n)` and
`.seed(n)` override the script's, and `.save_to(folder)` also saves
pictures, for when a test fails and you want to look. Underneath it's just
`run_headless`, so a play test behaves exactly like
`--headless --inputs`.

**`run()` returns the finished `Simulation`.** Everything from the earlier
steps works on it: the reported values, `reported_at(frame, name)` for
the middle of the game, the game itself (`played.game()`), and its last
picture. We didn't add a new "result" type, because one would have
repeated all of that.

**Panicking getters, with helpful messages.** `played.number("score")`
returns a plain `f64`, so tests can use ordinary `assert!`s. If the game
never reported `"score"`, the test fails with a list of what it did report
(`the game never reported "lives"; it reported: any, name, presses`), and
if `"score"` was text, it says that. In a test, a panic is simply how a
test fails, and the most common mistake (a misspelled name) is fixed in
seconds. `#[track_caller]` makes the failure point at the test's line,
not at the engine.

**Play tests check reports, not pixels.** "The score is 2100" survives a
new font, a new color or a moved scoreboard; "pixel (40, 4) is white"
doesn't. Pictures are for people (and AIs) to look at; reports are for
tests to check.

**Play tests live next to the game.** Breakout's and the physics
sandbox's play tests are at the bottom of their example files, so
`cargo test` runs them (examples with tests are listed in `Cargo.toml`
with `test = true`). Relative paths like `playthroughs/...` work because
`cargo test` runs tests in the project folder.

**Scripts both recorded and written.** Breakout's tests replay the bot's
long playthroughs from files. The physics tests use a short file written
by hand (`playthroughs/physics-three-drops.json`) and scripts built in the
test itself, including one generated with a loop (20 showers of balls), to
show that a script is just text that any program can write.

**We broke the games on purpose to check the tests.** With the physics
floor removed, all three physics tests failed ("escaped" was 3, not 0);
with bricks worth no points, both Breakout playthrough tests failed. A
test that has never failed hasn't proved anything yet.

**What the physics sandbox reports.** `bodies` (how many things were
dropped), `moving` (how many are still faster than 5 pixels a second) and
`escaped` (how many left the box). Together they answer "did everything
fall, land and stay inside?" without any knowledge of how the physics
engine works inside.

## Step 6: A guide for AI game makers

The goal: an AI assistant on a machine with no screen can build the engine,
make a new game, and improve it, using only headless runs, pictures,
`log.json` and play tests. The guide is [`AI_GUIDE.md`](AI_GUIDE.md).

**The guide was tested by following it.** Before writing it, we built a
new game, `examples/dodge.rs`, the way the guide describes: no window,
only `--headless`, sheets, close-ups, `log.json` and play tests. Every
mistake made along the way went into the guide's "Common mistakes", with
the real numbers: reporting a value that changed all the time and buried
the log; treating the log's first entry (the starting values) as an event;
a sheet half-wasted on a long game-over screen; falling blocks covering
the score; two hits 0.6 seconds apart because there was no safe time; and
a bot that died because *it* was wrong, not the game, found by zooming in
on the frames before each hit. Then every command and code block in the
guide was run as written: the template game and its play tests compile
and pass, and the quoted outputs and logs are real.

**`Simulation::history(name)`**, added because of that work. Tests kept
asking "on which frames did `hits` change?", and the answer took six lines
of digging through `report_log`. Now it's
`played.history("hits")`, a list of `(frame, value)`.

**Written for an AI, so: facts first, exact commands, no guessing.** It
opens with a table of facts an AI would otherwise have to discover (y
points down, frames count from 1, speeds are per second, which characters
the font has). Commands are complete and copyable, and always include
`--no-default-features`, which on a server saves minutes per build. The API
cheat sheet lists real names and arguments, checked against the source.

**Judging "feel" with numbers.** An AI can't feel a game, so the guide
turns feel into things you can measure: how long the player takes to
cross the screen (1 to 2 seconds), how long a threat is visible before it
hits (at least the 0.25 s people need to react), whether doing nothing
loses (and not instantly), whether a simple bot can win, how the results
spread over ten seeds, whether hits come too close together, and what the
sheet looks like shrunk small.

**The template is a tiny real game,** "catch the falling star", rather
than an empty skeleton. It has a title screen, losing, restarting, random
numbers, collisions and reports, so it shows the shape of a whole game,
and it compiles without warnings (the first draft warned that `GameOver`
was never used).

**Dodge is kept as an example.** It's small (about 300 lines with its
tests), shows everything the guide talks about (states, a difficulty
ramp, safe time after a hit, a readable score, reports, play tests and a
bot), and is the worked example the guide points to.

# Phase 1: comfortable for people and AI agents

An AI agent (DeepSeek) built a full game with duckforge and listed what it
missed: sprites, a 2D camera, transparency, sound and some small helpers.
This phase adds them. The rule for every step: a few clear functions with
one obvious way to use them, nothing old breaks, and everything works
headless and on Windows.

## Step 0: One main branch

The newer work lived on two branches. `claude/peaceful-bohr-vniw0t` had
the headless tools (fixed timestep, `--headless`, record and replay,
snapshots, play tests, the AI guide); `claude/duckling-parade` had an older
copy of its first two commits plus three examples. Both were merged into
`main`. The two branches shared those first commits exactly, so the only
conflicts were the lists of examples in `Cargo.toml` and the README, and
both sides were kept. The three examples compiled and ran headless against
the newer engine without changes.

## Step 2 (done first): See-through colors

**Done before sprites.** Sprites need colors with an alpha (a PNG pixel
or a `'.'` in ASCII art is see-through), so `Color` got its alpha first.
The sprite step then uses `Color::rgba` and `with_alpha` instead of
inventing a second way to say "see-through".

**A public `a` field, not a separate type.** `Color` already had public
`r`, `g` and `b` fields. Adding a fourth, `pub a: u8`, is the most
obvious design: `color.a` reads like `color.r`. The cost: any code that
builds a color by naming its fields, `Color { r: 1, g: 2, b: 3 }`, stops
compiling (Rust wants every field named). The alternatives were worse:

- A second color type (`Rgba`) next to `Color` would mean two ways to say
  "a color" and conversions everywhere.
- Keeping alpha out of `Color` and passing it separately
  (`fill_rect_alpha(rect, color, 128)`) would double the drawing functions.
- A private `a` field breaks field-naming code just the same (outside code
  can't name a private field either).

Nothing in this repository built colors by naming fields, and the README,
tutorial and AI guide always used `Color::rgb`, so the cost is close to
zero in practice. The compiler's message (`missing field a`) points at the
exact line, and the fix is `Color::rgb(..)`. Code that *reads* `color.r`
or compares colors keeps working; `Color::rgb(..) == Color::rgb(..)` is
still true, because both are solid.

**`Color::rgb` stays solid, so old games look exactly the same.** It sets
`a` to 255, and so do `from_hex` and the named constants. To check, every
example was run headless for 240 frames before and after the change, with
a snapshot every 60 frames: all 50 pictures were byte-for-byte identical.
A unit test also draws one scene with `rgb` and with `rgba(.., 255)` and
compares every pixel.

**The fast path stays.** When alpha is 255, drawing writes the color
straight into the pixels, as before (a whole row at a time for
rectangles). Only see-through colors pay for mixing, and alpha 0 returns
before touching anything.

**Whole-number mixing.** Each channel becomes
`(new * a + old * (255 - a) + 127) / 255`. Integer math gives the same
answer on every computer and compiler, so replays and snapshot tests stay
exact. (Half red over blue is `(128, 0, 127)`, tested.)

**Each pixel is painted once.** With a see-through color, painting the
same pixel twice makes it darker. `draw_rect` used to draw four sides that
overlap at the corners; a see-through outline now draws sides that don't
overlap. (Solid outlines still use the old code, so their pixels can't
change.) Circles, lines, triangles and text already visited each pixel
once.

**What ignores alpha:** `clear` (a cleared canvas is always solid; cover
it with a see-through `fill_rect` to fade it instead), 3D meshes, and
saved pictures (the canvas itself is always solid).

## Step 1: Sprites

**Three ways to make a sprite, one way to draw it.** `from_pixels` for
code that computes pixels, `load_png` for artists' files, and
`from_ascii` for everyone else. `from_ascii` is the one an AI agent (or a
beginner) will use most: the art lives in the code, needs no files, shows
up in diffs, and a typo is a compile-time-visible string rather than a
corrupted binary. All three give the same `Sprite`, drawn with
`canvas.blit_sprite(&sprite, pos, Blit { .. })`.

**`'.'` means see-through, and nothing else does.** A space would have
been tempting, but editors strip trailing spaces and some fonts make them
invisible, so rows would silently change length. A row of the wrong
length or a letter missing from the palette panics with a sentence naming
the row, column and character, and `#[track_caller]` makes the panic point
at the game's line, not the engine's.

**Options in a struct with `Default`.** `Blit { flip_x: true,
..Default::default() }` names only what changes, so there's one drawing
function instead of `blit`, `blit_flipped`, `blit_scaled` and so on, and
new options can be added later without breaking anyone (a field with a
default). It's the same pattern as `Config` and `HeadlessOptions`.

**Shared pixels.** A sprite is a window (`left`, `top`, `width`,
`height`) onto pixels held in an `Arc`. `sub` and `frames` only make new
windows, so cutting a sheet into frames copies nothing, and cloning a
sprite is cheap. `Arc` rather than `Rc` keeps sprites `Send`, so a game
holding them can still be moved to another thread (as tests sometimes do).

**Stored the way the canvas wants it.** Each pixel is kept as the
canvas's own `0x00RRGGBB` number plus a separate alpha byte, and the
sheet remembers whether every alpha is 255. Then the commonest case, a
solid sprite drawn as it is, is one `copy_from_slice` per row. (A test
checks that this fast path gives exactly the pixels of setting each one
by hand.) A frame cut from a sheet that has any see-through pixel takes
the careful path, which is still quick; being exact per frame wasn't
worth the extra bookkeeping.

**Nearest-neighbour scaling.** Each canvas pixel takes the sprite pixel
under its center, so pixel art stays sharp at any scale, including
shrinking. Sizes and positions are rounded to whole pixels, computed in
`f64` and clamped, so a sprite at `1e30` or `NaN` draws nothing or draws
at 0 instead of overflowing. The column lookup is computed once per draw,
not per pixel.

**Tint and opacity use the same whole-number math as see-through
colors,** so a sprite at `opacity: 0.5` over blue gives exactly the same
pixel as `Color::rgba(.., 128)`, on every computer.

**No rotation.** Rotating pixel art with nearest-neighbour sampling looks
ragged and needs a different drawing loop. Flips cover the usual need
(facing left or right). It's listed under "what the engine can't do yet".

**`Animation` is just a list and a speed.** `frame_at(time)` loops; for a
one-shot animation, compare the time with `duration()`. The game keeps
its own clock (often `ctx.time()` minus when the animation started), so
pausing or slowing an animation needs no new API.

**`load_png` errors are one sentence.** A missing file says which path it
looked for (made absolute, because "relative to what?" is the usual
confusion); a file that isn't a PNG says so and suggests re-exporting it.
Both are `io::Error`s, so `?` in `main` works and prints just the
sentence.

## Step 3: A 2D camera

**`position` is the middle of the screen.** That's what a game means by
"look at the player": `camera.follow(player, ..)` needs no "minus half
the screen". So the camera needs the screen size, given once to
`Camera2D::new(width, height)`. A new camera looks at the middle of the
screen with zoom 1, which makes it change nothing at all: a game can add
a camera just for screen shake without moving anything (that's what
Duckling Parade does).

**Two layers, one stack.** `push_camera` / `pop_camera` and
`push_offset` / `pop_offset` push onto the same stack of *views* (a
scale and an offset), so they nest in any order: an offset inside a
camera is in world units. A pop with nothing pushed does nothing, rather
than panic. The engine clears the stack before and after `draw`, so a
forgotten pop spoils at most one frame of the game's own drawing, never
the FPS counter.

**Applied at the edges of every drawing function.** Each function turns
its positions and sizes into screen pixels first, then runs its
unchanged code (clipping, blending, the fast paths). With no camera the
view is "scale 1, offset 0", and every example still draws byte-identical
pictures (checked again with all 50 snapshots). A test draws one scene
of every shape in screen space, and the same scene moved by 50 pixels
through a camera that moves it back, and compares every pixel.

**What zoom does to each shape.** Rectangles, circles, triangles, text
and sprites grow with the zoom. Lines stay 1 pixel thin (only their ends
move), because a thin line is usually meant as a thin line. `set_pixel`
moves but stays one pixel. `clear`, `get_pixel` and 3D meshes are always
in screen pixels: `get_pixel` reads what's *on the screen*, and 3D has its
own camera. `draw_text_centered` still centers on the screen, which is
almost always what it's used for.

**A broken zoom acts like 1.** Zero, negative or NaN zoom would make the
world vanish or flip, with no clue why. It's treated as 1.0.

**Shake uses `ctx.rng`, so it needs `camera.update(ctx)`.** The wobble
must be random to look right, and must come from the game's seeded
generator so replays and play tests stay exact. `draw` can't change
anything (it takes `&self`), so the random offset is chosen in `update`.
The offset is rounded to whole pixels, so a shaking pixel-art scene stays
crisp. A shake fades out linearly, and a new shake only replaces a
weaker one, so a small bump can't cut a big explosion short.

Using `ctx.rng` has a side effect worth knowing: shaking takes random
numbers, so everything random *after* a shake comes out differently
than without the shake. Same seed and inputs still give the same game,
which is what matters. Duckling Parade's play tests (a bot plays whole
games) still pass after the switch.

**`follow` is frame-rate independent.** It closes `1 - e^(-dt/smoothing)`
of the gap each update, so after `smoothing` seconds about 63% of the gap
is gone however many updates that took (tested: one 1-second step lands
where sixty 1/60-second steps do).

**Duckling Parade switched from hand-made offsets.** It used to compute a
shake offset with `sin` and add `+ o` to every position in every drawing
function (about 30 places, plus an extra parameter on four functions and
a helper trait). Now it calls `self.camera.shake(3.2, 0.4)` on a bite,
`self.camera.update(ctx)` once per update, and draws the pond between
`push_camera` and `pop_camera`. The HUD and menus are drawn after the
pop, so they stay still while the pond shakes, as before.

## Step 4: Sound

**A synthesizer first, files second.** The AI that built a game with
duckforge had no way to make a sound file, and a beginner rarely has
good ones. A `Sound` is a handful of numbers (wave, pitch, slide,
length, volume, fade in, fade out), so a sound can be written in code,
read in a diff and tuned by changing a number. Four presets (`coin`,
`jump`, `hit`, `explosion`) cover most games; `ctx.beep(freq, ms)` is the
one-liner. `.wav` files are there for recorded sounds, through
`Wav::load` and `ctx.play_wav`.

**Sounds are played from `update`, through `ctx`.** That's where the game
decides things happen, and it puts every sound on a known frame. `draw`
has no `ctx`, so a sound can't be played from it by mistake (once per
drawn frame would depend on the computer's speed).

**One record, two outputs.** Each update, the engine collects the sounds
the game played, renders them to samples, and notes the frame. A window
sends those samples to the speakers. A headless run mixes them all into
`audio.wav` and lists them in `log.json`. Both use the same samples, so
what a test checks is what a player hears.

**Exact timing.** A sound played during update N starts at the first
sample of that update: `(N - 1) * 44100 / updates_per_second`, computed
with whole numbers so it can't drift (at 60 updates per second, 735
samples per update). Tested at 60, 50 and 144 updates per second, on
frames 1, 7 and 100.

**Byte-identical `audio.wav`.** Rendering uses only plain arithmetic:
the sine wave is Bhaskara's 7th-century formula rather than the system's
`sin` (whose last bits can differ between computers and compilers),
noise comes from a small xorshift generator with a fixed seed, and
mixing adds samples in the order they were played, then rounds to
16 bits. Tested: the same seed and inputs give the same bytes, and a
different seed gives different bytes. (The sounds a game plays can still
depend on `ctx.rng`, and that's repeatable too.)

**The names are what tests use.** Each sound has a name: a preset's name,
`"beep"`, your `Sound`'s `name` field (or `"sound"`), or a `.wav` file's
name without `.wav`. `log.json` shows the name first, then the settings,
so a line like `{"frame": 450, "sound": "hit", ...}` reads at a glance.
`Simulation::sound_frames(name)` and
`Simulation::assert_sound(name, frame, within)` check them in play tests;
a failed check says which frames the sound *did* play on, or lists the
sounds that played instead.

**`audio.wav` only when there's something in it.** A silent game would
otherwise get a megabyte of silence per 10 seconds. `log.json` always
has `"sounds"` (empty if none), so an AI can tell "no sounds" from
"didn't look". The track lasts as long as the run, or longer if the last
sound rings on, so nothing is cut off.

**GIFs keep time with the sound.** GIF frame delays are whole
hundredths of a second. A GIF that shows every 4th frame at 60 updates
per second would need 6.67 hundredths, rounded to 7: 5% slow, and out of
step with `audio.wav` by the end. The GIF now picks a step that lasts a
whole number of hundredths (a multiple of 3 frames at 60), unless that
would more than double the step. When there's a GIF and an `audio.wav`,
the run prints the `ffmpeg` command that joins them (tested: a 1.5 s
run gives video and audio streams of exactly 1.5 s).

**Playback: `tinyaudio` on Windows and macOS, ALSA loaded at run time on
Linux.** `tinyaudio` is the lightest crate that plays sound on every
desktop (on Windows it uses DirectSound through `winapi`, nothing else).
But on Linux it links to ALSA when it's *built*, which needs the ALSA
development package and `pkg-config`; without them, a plain `cargo
build` fails. duckforge never needed system packages to build before, on
any computer, and an AI's cloud machine usually doesn't have them (this
one didn't, and couldn't install them). So on Linux, the engine opens
`libasound.so.2` when the game starts (with `dlopen`, through the tiny
`libc` crate) and looks up the six functions it needs. If the library
or a sound card is missing, the game prints one line ("no sound: ...")
and carries on silently. That's about 150 lines with a few `unsafe`
blocks, each with a comment saying why it's sound. A test plays a beep
into ALSA's "null" device, which needs no sound card, to check the
function signatures for real. The Windows code was checked to compile
with `cargo check --target x86_64-pc-windows-gnu`.

**Mixing in the window.** The speaker keeps a list of playing sounds
behind a `Mutex`. The sound card's thread asks for 1024 samples at a time
(about 23 ms, so sounds feel instant) and gets every playing sound added
together, clipped to the loudest possible value. At most 32 sounds play at
once; a 33rd replaces the oldest. In a window, the engine forgets sounds
once they're sent, so a long game doesn't fill memory.

**`audio` is a feature, on by default.** `--no-default-features` turns
it off along with the window and GPU. It only matters in a window:
without it, the window is silent, and headless runs save `audio.wav`
either way.

**What's left out:** looping, stopping a sound, changing its volume while
it plays, stereo, and music. Each would need a handle to a playing sound,
which is a bigger API than "play this". They're in the AI guide's list
of things the engine can't do yet.

## Step 5: Text and font

**Lowercase fits without moving anything.** Capitals and digits stay
exactly as they were (5 rows, 3 columns). Lowercase letters use the
bottom four rows (tall ones like `b d f h k l t` reach the top row), and
the tails of `g j p q y` use a sixth row that hangs into the 2-pixel gap
between lines. So line spacing, `text_width` and every capital stay the
same, and text with capitals and digits draws the same pixels as before.

**What did change:** text written in lowercase used to be drawn in
capitals; now it's drawn in lowercase, as written. In this repository,
only Dodge's "press space to start" changed (checked with the before and
after snapshots of every example). Its play tests check reports, not
pixels, so they pass unchanged.

**Every printable ASCII character.** Space to `~`, including `" # $ % &
* ; < > @ [ \ ] ^ _ ` { | } ~`, so an AI never has to remember which
symbols are safe. Tests check each one has a real glyph, that no two
characters look alike (it caught nothing, but it's cheap insurance for
future edits), and that only `g j p q y` hang below the line. Anything
outside ASCII is still drawn as `?`.

**`m` and `w` are the hard ones.** Three pixels is very narrow for
letters with three strokes. They were drawn several ways and compared in
words like "my vow" and "brown"; the chosen `m` has a solid top (so it
doesn't read as `n`) and `w` is a small capital W. Lowercase is readable
at scale 1 but clearer at scale 2; the AI guide says so.

**`text_height` and `line_height` are `Canvas::` functions, like
`text_width`.** They don't need a canvas (they only depend on the font),
so they're called `Canvas::text_height(2)` rather than
`canvas.text_height(2)`, the same way as the existing `text_width`. One
way to call all three. `text_height` is the height of a capital, which
is what you center on; `line_height` is the distance from one line's top
to the next.

**Multi-line text already worked.** `draw_text` already started a new
line at `'\n'` (and ignored the `'\r'` of Windows line endings), and
`text_width` already measured the widest line. Now the docs say so, and
`draw_text` uses `line_height` itself, so the two can't disagree.

## Step 6: Small helpers and hooks

**Optional hooks are default methods.** `Game::init` and `Game::resize`
have empty bodies in the trait, so every existing game compiles
unchanged and only games that want them write them. A test game that
writes neither still runs (tested).

**`init` runs inside the first step, just before the first `update`.**
Not in `Simulation::new`, because then a test couldn't set things up
(press a key, follow a script) before the game starts. `ctx.frame()` is
0 during `init`, and anything it reports or plays is recorded as frame 1,
so logs and play tests never see a "frame 0".

**`resize` needed a canvas that can change size.** Until now the window
stretched a fixed-size canvas, so there was nothing to resize. A new
`Config::resizable` (off by default, so old games look the same) makes
the canvas follow the window, `scale` window pixels per canvas pixel, and
tells the game. A minimized window (size 0) is ignored rather than
shrinking the canvas to 1 x 1. Headless runs never resize by themselves;
`Simulation::resize(w, h)` does it in tests. The mouse is mapped to the
current canvas size, not the starting one. Replays don't record window
resizes: a replay runs at the configured size. Adding a field to
`Config` would break code that lists every field without
`..Config::default()`, but every example, test and doc uses
`..Config::default()`, so in practice nothing broke.

**Collision helpers follow `overlaps`: touching doesn't count.** So
`circle_vs_circle` of two circles exactly touching, `circle_vs_rect` of a
circle resting on a box, and `intersection` of two boxes sharing an edge
are all "no". One rule for every check.

**`swept_aabb(rect, movement, other)` takes this frame's movement, not a
speed.** Then the result's `time` is simply "how far through the move",
0 to 1, and `rect.x + movement.x * hit.time` is where to stop. It uses
the "slab" method: for each axis, find the fractions of the move at which
the boxes start and stop overlapping on that axis; they collide where
both axes overlap at once. The normal is the axis that lined up last.
Two corner cases are decided on purpose: boxes that start already
overlapping hit at time 0 with the normal pointing the shortest way out
(so a game can push them apart), and a box sliding along another's edge
doesn't hit (so a player can walk along the top of a platform). Pressing
*into* an edge it's touching is a hit at time 0. A test shows the reason
it exists: a bullet moving 1000 pixels a frame past a 1-pixel wall,
where checking the end position finds nothing.

**`approach` and `remap` are the two numbers helpers games kept
rewriting.** `approach` never overshoots and treats a negative step as
positive, so acceleration and countdowns can't oscillate. `remap` isn't
clamped (clamping is one `.clamp()` away, and un-clamping isn't), and an
empty source range returns the start of the target instead of dividing
by zero. `remap` takes ranges as tuples, `(0.0, 100.0)`, rather than
`0.0..100.0`, because a Rust range means "end excluded" and remapping
includes both ends.

**Each helper has a doc comment with an example that runs as a test,**
so the examples can't go stale.

**Units are now written down.** 2D is in pixels with y down (gravity
about 500 to 1000 px/s² on a 320 x 240 canvas); 3D is in metres with y up
(gravity 9.8, a person 1.8 tall). It's in the physics module's docs, the
README, the AI guide's quick facts and the tutorial.

## Step 7: For the person learning Rust

**A tutorial chapter per feature, written as the features landed.**
Chapters 28 to 34 (see-through colors, sprites, the camera, sound, text,
collisions, starting your own game) each build a tiny running game and
explain the Rust ideas that show up for the first time: struct update
syntax with `..`, `..Default::default()`, lending with `&`, slices and
tuples, `Arc`, reborrowing `ctx`, `&'static str`, `Mutex`, `const`
arrays, default trait methods, `retain_mut`, and `Debug` versus
`Display` with `?` in `main`. Every complete example in those chapters
was compiled and run headless while writing it. Three old exercises
(screen shake, sprites, fixed timestep) asked the reader to build what
the engine now has, so they became exercises that *use* the camera,
sprites and sound instead.

**The debug overlay is the engine's, not the game's.** F1 toggles it in
any game; `--debug` turns it on from the start, and works headless, so
an AI sees hitboxes in its sheets and GIFs too. Games mark things with
`ctx.debug_rect(rect)` and `ctx.debug_circle(center, r)` during
`update` (where the game knows its hitboxes; `draw` has no `ctx`). Only
the latest update's shapes are kept, so nothing piles up, and marking
costs a push onto a list when the overlay is off. The shapes are in
world coordinates: the canvas remembers the first camera the game
pushed during `draw`, and the overlay uses it to put the outlines on
top of the things they mark (tested with a zoomed camera: a 10-pixel
world box outlines 20 screen pixels at the right place). The panel
shows the FPS, the frame number (so a picture can be matched to
`log.json`), the camera's position and zoom, and the mouse on the screen
and in the world. It's drawn after the game, in screen pixels, on a
see-through black panel, with outlines in magenta and cyan, colors few
games use. F1 is an engine key like F3, F10 and F12, so it isn't
recorded into input scripts.

**`LoadError` instead of `io::Error`.** When `main` returns an error,
Rust prints it with `{:?}`, and an `io::Error`'s `Debug` output is
`Custom { kind: NotFound, error: "..." }`: the one clear sentence buried
in a struct. `LoadError` prints just the sentence both ways, still works
with `?` in `main` (it's an `Error`), and converts into an `io::Error`
for code that wants one. `Sprite::load_png` and `Wav::load` were added
in this phase, so changing their return type broke nothing. Mistakes in
code (uneven ASCII rows, a letter missing from the palette, a sub-sprite
reaching outside its sheet, the wrong number of pixels, an empty
animation) panic with a sentence naming the problem, and
`#[track_caller]` makes the reported line the game's, not the engine's.

**`examples/template.rs` is a small platformer, not an empty
skeleton.** It shows how the new pieces fit together (sprite, animation,
camera that stops at the level's ends, jump and coin sounds, pause screen
with transparency, `approach` for running, `circle_vs_rect` for coins,
reports, a debug hitbox) in about 300 lines with comments, and its three
play tests (run, jump with its sound, pause) pass, so a copied game
starts with working tests. It's named at the top of the README and the
AI guide. The AI guide's tiny "catch the star" template stays, for when
a game needs none of that.

**`test.bat` for a double-click.** It runs `cargo test`, then `cargo
test --no-default-features`, stops at the first failure, and waits for
a key so the window stays open. `.gitattributes` keeps it in Windows
line endings, which `cmd.exe` needs, even when it's committed from Linux.

## Step 8: Proving it on Dodge

**Dodge uses everything new, without changing how it plays.** The
player is a duck drawn with `Sprite::from_ascii` that waddles (a
two-frame `Animation`) while it moves; the blocks are rocks, also ASCII
art. Both are drawn at scale 2 so their pictures are exactly their old
hitboxes (12 x 12 and 14 x 14), so the game's difficulty didn't change.
A `Camera2D` shakes the playfield on a hit (harder on the last one), and
the score strip is drawn after `pop_camera` so it stays still. See-through
colors give the rocks shadows that darken as they get close, make the
score strip let you glimpse rocks coming in, dim the GAME OVER band, and
replace the "blink" after a hit with a flicker to 30% opacity (you can
still see where you are). Sounds: a beep to start, `hit`, `explosion` on
the last hit, and `coin` every 10 rocks dodged. Every rock and the duck
are marked for the debug overlay.

**The old play tests pass unchanged**, including the one that a careful
bot survives a minute, even though camera shake now takes numbers from
`ctx.rng` after each hit. Two new ones check the sounds against the
reports: each hit has a `"hit"` on the same frame and the game-over hit
an `"explosion"`, and the 10th rock dodged plays `"coin"` on that frame.
A third replays `playthroughs/dodge.json`, recorded from the bot (20
seconds of dodging, then standing still until game over) by an ignored
test, so the recording can be remade with one command if the game
changes.

**What's still uncomfortable for an AI agent** (found while doing this
step, for the next phase):

1. **Shared randomness.** Screen shake draws from `ctx.rng`, so adding
   "juice" to a game changes every random event after it (where the next
   rock falls). Dodge's tests survived, but a game balanced by seed would
   change. A separate generator for effects, seeded from the same seed,
   would keep gameplay and decoration apart.
2. **The interesting frames fall between snapshots.** The sheet shows 16
   evenly spread frames: two caught the flicker after a hit by luck, but
   none caught a hit itself or the 0.3-second shake. Snapshots on chosen frames (`--snapshot-at 1425,1704`) or
   whenever a report changes (`--snapshot-on hits`) would show exactly
   the moments that matter.
3. **Sound can only be checked by its log.** `log.json` says which sound
   played when, but nothing says whether the mix clips or one sound
   drowns the others. A short summary in `log.json` (peak level, clipped
   samples, loudness per second) would let an AI catch that.
4. **ASCII art can't be checked before running.** A sprite only becomes
   visible once a game draws it. A way to save a sprite enlarged as a PNG
   (or a `--sprites` sheet of every sprite a game made) would make
   drawing art a faster loop.
5. **Platform movement is still hand-written.** The template resolves
   collisions one axis at a time in about 25 lines, the most error-prone
   code in it. `swept_aabb` finds the hit but doesn't slide; a
   `move_and_slide(rect, movement, &solids)` would cover most games.
6. **Levels as text.** Sprites from ASCII work well; levels would too
   (`Tilemap::from_ascii(rows, legend)` with solid tiles that the
   collision helpers understand).
7. **Labels in the debug overlay.** Outlines are there, but not what
   they are; `ctx.debug_text(pos, text)` would show an enemy's state or
   a timer next to it.
8. **Small text in pictures.** Lowercase at scale 1 is 4 pixels tall:
   readable in a window, hard in a shrunk GIF. Games meant to be judged
   from pictures should use scale 2 or capitals.
9. **Windows was compile-checked, not run.** The `tinyaudio` path builds
   for Windows (`cargo check --target x86_64-pc-windows-gnu`) but hasn't
   played a sound on a real Windows machine yet; `test.bat` there is the
   next check.

# Phase 2: efficient for an AI agent

Claude built a test game ("Duck Defender") with phase 1 and reported what
cost it time. The theme of this phase: an agent should need fewer
rebuilds, fewer images, fewer commands and less reading to make a good
game, while the engine stays small enough for a person learning Rust.

## Step 0: Main catches up

All of phase 1 was on `claude/peaceful-bohr-vniw0t`, and `main` was an
ancestor of it, so `main` was fast-forwarded (no merge commit), tested
(262 tests with default features, 245 headless, all passing) and pushed.
This phase continues on the same branch name, restarted from `main`.

## Step 1: Fixes from a review

**The docs said things that were no longer true.** The cheat sheet said
`Sprite::load_png` returns `io::Result` (it returns
`Result<Sprite, LoadError>` since phase 1). Reading the AI guide, README
and tutorial against the code found more: the tutorial still said "two
keys work in every game", listed two Cargo features and the old
`Cargo.toml`, called the font 3x5, showed headless output from before
`log.json` existed, and quoted `Color::lerp` without alpha; the README's
file table missed six new source files and four examples; the guide said
"about 140 tests" and had a sentence about `--` stranded under the wrong
heading; `docs/input-format.md` didn't list F1 as an engine key. All
fixed. Quoted code that was deliberately simplified (the `Game` trait,
`Canvas`, `fill_rect`, the game loop) now says what it leaves out. Every
complete example in the three documents was compiled to check it.

**Making the guide impossible to go stale.** Two tests now fail when the
docs and the code drift apart:

- `tests/cheat_sheet.rs` calls every function the cheat sheet mentions,
  the way the cheat sheet says to (so renaming or removing one breaks
  the build), and one of its tests reads the cheat sheet and fails if it
  names a function the file never calls (so adding a function to the
  guide without an example fails too). It was checked by adding a
  made-up function to the guide: the test named it.
- A test in `src/cli.rs` reads the AI guide and the README: every
  `--flag` they mention must be an engine option (or on a short, named
  list of Cargo's and the installer's flags), every engine option must
  be mentioned in both and in `--help`, and the parser must accept every
  option it lists. Tests are a better guard than care, because the docs
  are long and change in many places at once.

**The debug overlay hid the game's HUD.** Its info box was five lines in
the top-left corner, exactly where most games draw their score. Now it's
two short lines (`FRAME 600 FPS 60` and `CAM 685,600 X1 MOUSE 565,530`,
the mouse in world coordinates, which is what you need to place things),
on a lighter see-through background, in a corner of your choice:
`Config::debug_corner` in code, `--debug-corner top-right` on the
command line (which wins). The default is **bottom-right**: games
usually put score, lives and timers along the top (both of this repo's
games with a HUD do), and the engine's own F3 FPS counter already sits
bottom-left. The corner is a `Config` setting rather than a
`HeadlessOptions` one because it depends on the game's layout, which is
the same in a window and headless; one place to set it. Tests check each
corner, that the box stays small, and that the game shows through it.

## Step 2: A one-page quick start

**One page, read first.** The AI guide is about 900 lines: right as a
reference, too long to read before writing a first line of game.
`docs/QUICKSTART.md` (about 120 lines) has only what's needed to make
and check a game: what duckforge is, the two commands to build and run
headless, the `Game` trait, a complete small game, the loop (write, run
headless, read the summary and `log.json`, look at the sheet, write a
play test, fix), the five mistakes that cost the most time, and where
the details are. The README and the AI guide point to it in their first
lines; the tutorial points to it too.

**The game on the page can't rot.** It's `examples/quickstart.rs`, which
Cargo compiles and whose play test runs with every `cargo test`. A test
(`tests/quickstart.rs`) checks that the two code blocks on the page are
exactly that file, and that the page stays about one page long. The
flags test checks that every flag the page mentions exists.

**Which five mistakes.** The ones that cost the most time in practice,
not the most interesting: the missing `--` before game options, `is_down`
where `was_pressed` was meant (now also a sound every frame), speeds
without `ctx.dt()`, randomness or time from outside `ctx` (repeatability
is what every other tool depends on), and drawing the HUD inside the
camera. The rest are in the guide.

**The game is ~40 lines, not 30.** `cargo fmt` puts each field and
statement on its own line; squeezing it to 30 would mean either fewer
features (no sound, no score) or unformatted code, and both teach the
wrong thing.

## Step 3: Tuning values without rebuilding

**Why.** Most changes while making a game feel right are numbers: jump
speed, gravity, enemy speed. Each one cost an agent a rebuild (seconds to
a minute) and a person a restart. `ctx.tune("jump_speed", 330.0)` makes
a number changeable from outside, with the default still in the code, so
the game works with no file at all.

**Three sources, one order.** `--set name=value` beats the tune file,
which beats the default in code. `--set` is for trying a value for one
run (an agent can try five values with five commands and no rebuild); the
file is for keeping values. Numbers only, named by strings: no types,
nesting or expressions, so there's nothing to learn and nothing to
misparse.

**The file is found by the program's name.** `examples/<game>.tune.json`,
where `<game>` comes from the running program (`target/.../examples/dodge`
is `dodge`). Test programs are named like `dodge-1a2b3c4d5e6f7a8b`, so the
hash is stripped and `cargo test --example dodge` reads the same file:
play tests test the tuned game, not the defaults. (Checked by setting
`jump_speed` to 0 in the template's file: its jump test failed.) A
missing default file is fine; a missing file named with `--tune` is an
error, because then you asked for it.

**Hot reload in a window, numbers only.** Twice a second the window
checks the tune file's modified time and, if it changed, reads it again;
a broken file prints one line and keeps the old values. The game sees
the new numbers the next time it calls `ctx.tune`, so the advice is to
read tune values at the top of `update`. Nothing else is reloaded: no
code, no assets, no state, which is what keeps this simple and safe.
Replays and recordings don't capture live edits; a recording plays with
whatever values are in the file when it's replayed.

**A name is known only when the code reading it runs.** That has two
consequences, both found by trying it on the template: `--write-tune`
only writes names the game asked for during that run, and a `--set` for a
name read only in rarely-run code looks unused. So the template reads all
its tune values at the top of `update`, the docs recommend it, and the
warning for an unused `--set` says "never asked for in this run" and
suggests checking the spelling (it's usually a typo, like `jumpspeed`).

**`log.json` records every value and its source** (`"from": "default"`,
`"file"` or `"set"`), so a run can be reproduced exactly, and the summary
prints the values that didn't come from the code.

**`ctx.tune` takes `&self`.** It records which names were used, which
needs to change something. With `&mut self`,
`ctx.report("jump", ctx.tune("jump_speed", 330.0))` would be a borrow
error, the kind that costs a beginner (and an agent) a confused rebuild.
The list of used names sits in a `RefCell`, so `tune` can take `&self`;
the tutorial explains `RefCell` with exactly this example.

**Tests:** `--set` beats the file and the file beats the default; the
same seed and tune values give an identical run (log and pictures), and a
different value gives a different one; `--write-tune` keeps the file's
names and adds new ones; bad files and bad `--set` values give one
sentence each; and the game name loses the test hash.

## Step 4: Labels and scene dumps

**Words are cheaper than pictures.** Looking at an image costs an agent
far more than reading a few lines, and "is the duck on the ground?" is a
question about positions, not pixels. So a game can name what it draws:
`canvas.label("duck", rect)`. A label draws nothing; the canvas records
its name, its rect as given (world coordinates inside `push_camera`) and
where that is on screen (through the same view the drawing uses), and
whether any of it is on screen. The canvas also records each line of
text drawn, with its screen area, so the HUD can be read without
pixels. After the game's `draw`, the simulation keeps these, with the
camera, as a `Scene` (the overlay's own text is left out).

**Three ways to read a scene.** `--describe` prints the last frame in a
few lines, with repeats grouped (`rock x4 at ...`, `(+6 off screen)`), so
the usual check after a change needs no image at all. Every snapshot and
`last.png` gets a matching `.scene.json` with exact numbers. And tests can
use `sim.scene()`, which draws the current frame if needed.

**Warnings that name the frame and the label.** The run checks scenes
from frame 1, 16 frames spread over the run, every snapshot and the last
frame (drawing those extra frames costs a few milliseconds, and drawing
never changes the game). It warns about:

- a kind of label that's in every checked frame but never on screen
  (usually drawn outside the camera, or with the wrong coordinates);
- text drawn partly outside the canvas (each text once);
- labels overlapping on frame 1, a common spawn bug. One label entirely
  inside another (an enemy inside a "level" area) doesn't count, or a
  game that labels its level would warn about everything.

A test runs a game made to have all three problems and checks the exact
warnings; another checks a clean game has none, and Dodge and the
template run with no warnings.

**Why "every checked frame" and not "any frame".** A label off screen in
one frame is normal (a bullet leaving, a level scrolling). Off screen in
every checked frame, while present in all of them, is almost always a
bug.

**Labels in the overlay** are green outlines with the name above them,
next to the magenta and cyan `debug_rect` / `debug_circle` shapes. Labels
are for "what is this", debug shapes for "what does the game think"
(hitboxes, ranges); the overlay shows both.

**The template and Dodge label their main objects** (player, ground,
platforms, coins; duck, rocks), so their scenes and `--describe` are
useful, and they serve as examples to copy.

**A bug the extra drawing found.** Comparing every example's pictures
before and after this step found one change: stress3d's frame 60 now
showed "CPU" in its corner. The game prints `ctx.renderer_name()`, which
the engine only learned the first time a frame was drawn. The new scene
checks draw frame 1, so the name was known sooner. Drawing must never
change what the game sees, so now a canvas that draws 3D on the CPU (as
every headless run does) tells the context its name before the first
update. A test checks that drawing extra frames, or not, gives the game
exactly the same view. Every other example's pictures are unchanged.

## Step 5: Grid and diff

**`--grid N` makes pictures measurable.** An agent looking at a picture
often needs a position ("is the duck at 150 or 160?"). A faint line every
N pixels, numbered, answers that by eye. The numbers sit along the
bottom and left edges: the first try put them along the top, where they
covered Dodge's score, the same lesson as the debug box. The grid is
drawn on copies made for saving (`last.png`, snapshots and close-ups, so
a close-up shows real coordinates), never on the game's own picture, the
scenes, the sheet or the GIF (shrunk, the numbers would be unreadable).
Lines are see-through white, so the picture stays visible under them.

**`--diff OLD_DIR NEW_DIR` answers "did my fix change only what I meant
to?"** It needs no game, so it runs before the engine starts one (any
example can run it). For each picture in both folders: the number of
changed pixels, the box around them, and a copy of the new picture,
darkened, with the changed pixels in magenta (in `NEW_DIR/diff/`). For
each scene file in both: labels that moved (matched by name, in drawing
order), appeared or disappeared. From `log.json`: reported values that
ended differently. Unchanged pictures are listed on one line, and
pictures only in one run are listed too. A test builds two folders by
hand and checks the whole report, line by line.

**Comparing apples with apples.** Trying `--diff` on the template right
after adding `--grid` gave "5619 pixels changed" in every picture: one
run had a grid and the other didn't. So `log.json` now records what was
drawn on the pictures (`"drawn_on_pictures": {"grid": 32, "debug":
false}`), and `--diff` adds a note when the two runs differ, instead of
leaving the agent to work out why everything changed.

**A real check.** Running the template with a jump twice, once with
`--set jump_speed=420`, `--diff` reported 184 changed pixels in a 10 x 53
box and "player moved 40,151 to 40,112": the change, located and
explained, without opening an image. (An earlier attempt with the jump
under a platform reported "unchanged", which was right: the duck bumped
its head either way.)

**Two-word values.** `--diff` takes two folders, and the option parser
took one value per option. An option's value description can now be
several words (`"OLD_DIR NEW_DIR"`), and the parser takes that many;
`--diff=a` gets a clear error instead.

## Step 6: Checks and fuzzing

**`ctx.check(name, condition)` states a rule.** Most bugs an agent can't
see are a rule quietly broken: the player outside the level, a count
gone wrong. A check costs one line, never stops the game, and records
only the first frame each rule fails (once per name, so a rule broken
every frame doesn't flood anything). It shows up where an agent already
looks: the summary (`CHECK FAILED`), `log.json` (`"checks"`, with how
many rules there are, so "no failures" can be told from "no checks"),
the debug overlay (in red, so a person playing sees it too), and a line
in the console in a window.

**Play tests fail on a failed check by default.** A check is a test that
runs in every play test for free; making it opt-out (`.ignore_checks()`)
means nobody has to remember to look. The message names the rule and the
frame, and says which `--frames` replays to just before it.

**`--fuzz N` plays the game N times with random, realistic inputs.**
Each run uses seed 1, 2, ... N and a script of taps (2 to 8 frames) and
holds (a quarter of a second to two seconds) of the fuzz keys, with
pauses between, never a key pressed while it's still held. Random keys
every frame would mostly test pressing and releasing; real play holds
directions and taps buttons.

**Each run is a separate process of the game's own program.** `run`
receives one game value, and making more would need games to be
`Clone`, a new requirement that would break existing games. Running the
program again (`--headless --inputs script`) gives every run a fresh
game, catches crashes naturally (the run exits with an error, and the
engine's panic hook prints the frame it happened on), and lets runs go
in parallel on every core with no shared state. The cost, a few
milliseconds to start each run, is small: 100 runs of 30 seconds of the
template took 0.3 s on 4 cores.

**Three kinds of problem.** A panic (message, source line, frame), a
failed check (rule, frame), and a freeze: no reported value changed for
`--freeze-seconds` (10 by default) although keys were being pressed. A
freeze needs a game that reports something; a game that reports nothing
can't be judged, so it's never called frozen. A menu that waits for a key
the fuzzer doesn't press counts as a freeze too (Dodge's title screen did,
in 1 run of 100, when Space wasn't pressed for 10 seconds): the shrunk
replay shows which it is, and `--fuzz-keys` can include the start key.

**A run reports all its problems**, every failed check and a freeze, not
just the first, so one problem can't hide another in the same run. (A
panic ends a run, so nothing after it can be seen.)

**Grouped, saved and shrunk.** Problems are grouped by kind and rule (or
panic message): one line per different problem, naming the first seed and
how many other seeds hit it. Every failing run's inputs are saved as
`seed-N.json`, a normal input script with the seed, so `--inputs` replays
it exactly. The first run of each problem is shrunk: cut after the
problem's frame, then key presses removed in chunks, then one by one,
keeping each removal after which the same problem still happens (at most
150 runs). Shrinking every failing run would cost that many runs each;
one per problem gives the short replay that matters. On a test game with
three planted bugs, the crash shrank to exactly the three Z presses that
caused it, the broken rule to holding Right, and the freeze to the one X
press.

**Not tested end to end by `cargo test`.** Fuzzing needs a built game
program to run, which a unit test doesn't have. The pieces are tested
(the input generator is repeatable and realistic, freeze detection, reading
panics, keeping presses and releases together while shrinking, the
options), and the whole was tried on a planted-bug game, the template
(100 runs, no problems) and Dodge (100 runs, the title-screen freeze).

## Fixes from a review of steps 0 to 6

**The grid covered the debug box.** With `--grid` and `--debug`
together, the grid's numbers and lines were drawn over the overlay's
box, and both became unreadable. The simulation now remembers where it
drew the box, and the grid skips any number that would touch it and puts
the box's pixels back exactly as they were (tested: identical with and
without the grid).

**`--grid-world N`.** Games with a `Camera2D` place everything in world
coordinates, and a screen-pixel grid made the agent convert by hand.
`--grid-world 50` draws a line every 50 world units, through the frame's
camera (the first one the game pushed, shake included), numbered in world
coordinates, the same numbers as the game's positions and the scene
files' `world` rects. It's a second flag rather than a switch on `--grid`
so one word says what you get; using both is an error. Without a camera
it's the same as `--grid`. Numbers stay at least 24 screen pixels apart
whatever the zoom.

**`--diff` reported invisible moves.** A frame could be "unchanged" in
pixels and still have "2 of 2 frog moved", because a label had moved by
a fraction of a pixel. A move now counts only if it's at least one pixel
on screen (the world move times the camera's zoom), so the scene report
agrees with the pictures. At zoom 4, a 0.4-unit move is 1.6 pixels and
still counts (tested both ways).

**Waiting on purpose isn't a freeze.** `--fuzz` called Dodge's title
screen a freeze when Space wasn't pressed for 10 seconds. A game now
says when waiting is expected: `ctx.idle_ok()` in `update`, on every
frame it's true (title, pause, game over). The frames are recorded in
`log.json` as stretches (`"idle": [[1, 9], [1805, 1844]]`), and the
fuzzer treats them like a change, so the 10 seconds only count outside
them. The alternative, `--freeze-ignore state=Paused` on the command
line, would make every fuzz command repeat knowledge the game already
has, and depends on how a game names its states; one call in the code
says it once. Dodge and the template call it on their waiting screens,
and both now fuzz clean over 100 runs.

## Step 7: Short, clear output

A run used to print one long line per kind of thing, and the useful
part (did anything go wrong?) could be anywhere in it. Now every
headless run builds one summary (`src/summary.rs`), prints it, and saves
it as `summary.txt`. It's at most about 15 lines, in a fixed order: the
run itself (frames, time, seed, input events replayed), the final
reported values, the lowest and highest of each number that changed, the
frames where text or yes/no values changed, how many times each sound
played, tune values that didn't come from the code, the checks, the
problems, and where the files are. A test builds the busiest summary it
can (30 values, 30 sounds, 5 changing states, 9 failed checks, 9
warnings) and checks it stays within 15 lines.

**Cut, never lost.** Long lists stop with "and 20 more (see log.json)",
a state that changed 20 times shows its first three and last two
changes, and problems past the third become "and 15 more problems".
`log.json` has everything, and `--json` prints the summary uncut.

**Three ways to print it.** Normal prints the summary. `--quiet` prints
only the problem lines (`CHECK FAILED: ...`, `warning: ...`), so no
output means all is well, which is what a script like `verify` wants.
`--json` prints it as one line of JSON with `"ok"` first, for programs.
`summary.txt` is the same in all three. Using both flags is an error:
they'd fight over the same output.

**Warnings are gathered in one place.** A replay recorded at a
different speed, and a `--set` name the game never asked for, used to be
printed on their own lines; now they're warnings like the scene's, so
they appear in the summary, in `log.json` and in `--quiet`.

**One-line panics.** A crash printed Rust's message over several lines,
and on servers that set `RUST_BACKTRACE=1` (this one does) about 30 more
lines of backtrace, with the frame number last. Now a headless run's
panic is one line, `duckforge: the game panicked on frame 25 at
examples/x.rs:13:22: index out of bounds ...`, giving the frame, the
file and line, and the message. `RUST_BACKTRACE=full` brings the long
version back. `--fuzz` reads the new line (tested), and other errors were
already one line naming the file.

## Step 8: One command for the whole loop

`tools/verify.sh <game>` (and `tools\verify.bat <game>` on Windows) is
the check to run after every change. It builds the example headless,
runs its play tests, replays every `playthroughs/<game>.json` and
`playthroughs/<game>-*.json` with `--sheet --debug --quiet`, fuzzes 50
runs (a second argument asks for more), and prints one line per step and
a last line, `VERIFY OK` or `VERIFY FAILED` with exit code 1. A failed
step prints its first six lines of detail, and everything it saved is
in `out/verify/<game>/` (now in `.gitignore`).

**A script, not an engine flag.** Running the play tests means running
`cargo test`, which a game's own program can't do from inside itself,
and the script is short enough to read and change. It only strings
together commands an agent could type, so nothing it does is hidden.
Playthroughs are matched as `<game>.json` and `<game>-*.json`, not
`<game>*.json`, so `dodge` doesn't pick up a game called `dodge2`.

**`--quiet` made it simple.** Each replay's output is empty when all is
well, so "did anything print?" is the whole test, and the summary's
`final:` line from `summary.txt` gives the report its one line of
results.

**Two tests keep the scripts honest:** every flag they use exists, and
both use the same flags. The batch file couldn't be run here (no
Windows); it mirrors the shell script line by line.

**What it found right away.** On its first run over the examples:

- Breakout's "press Space to launch" screen was called a freeze. It now
  calls `ctx.idle_ok()` when not playing, like Dodge.
- The physics sandbox was called frozen after the fuzzer pressed arrow
  keys it never reads. The engine now notes every key a game asks about
  (`is_down`, `was_pressed`, `was_released`, `axis`) and writes them to
  `log.json` as `"keys_read"`; the fuzzer only counts presses of those
  keys when looking for a freeze. (Tested both ways.) Noting a key costs
  one set insert per question, and drawing can't ask, so runs stay
  identical.
- The quick start's Catch reported only its score, so moving the paddle
  while stars fell past looked like nothing happening. It now also
  reports `paddle_x`. A game whose reports don't show the player's
  actions can't be checked by anything that reads reports, so that's a
  lesson for the guide too. The quick start page now ends its loop with
  `verify`, and stays at 125 lines.

## Step 9: Build speed

**Measured** on this cloud machine (4 cores, Rust 1.97), building
`examples/dodge.rs` from an empty target folder, then again after
changing one constant in it:

| Build | Clean build | One-line change |
|---|---|---|
| Headless (`--no-default-features`), before | 15.2 s | 0.46 s |
| Headless, after | 11.4 s | 0.37 s |
| Default features (window, GPU, sound), before | 87.2 s | 0.89 s |
| Default features, after | 66.1 s | 0.55 s |

The one-line rebuild was already fast: only the example is compiled
again, incrementally, and linked. The clean build is what an agent pays
on every fresh cloud machine. Headless, `cargo build --timings` showed
half of it is the engine itself (7.5 s); `png`, `serde_json`, `gif` and
the compression crates are 1 to 3 s each, mostly in parallel.

**What changed: less debug information.** Debug builds wrote full
debug info (for stepping through code in a debugger, which nothing here
uses). Now the engine and games keep only line numbers
(`debug = "line-tables-only"`), which is all a backtrace needs, and the
libraries keep none. The machine code is the same (same `opt-level`),
so games run exactly as fast and runs stay identical; panic messages
still name the file and line (they never came from debug info).
Headless clean builds are 25% faster, default ones 24%.

**Tried or considered, and left alone:**

- Leaving out wgpu's OpenGL backend (`gles`) would cut the default build
  further, but on machines without Vulkan, DirectX 12 or Metal the GPU
  renderer would be gone and 3D would fall back to the slower CPU
  renderer. That's slowing games, so no.
- Optimizing the libraries more (`opt-level = 2` or `3` for
  dependencies) would make PNG saving a little faster but every clean
  build slower.
- A faster linker (mold, lld) helps link times, but can't be required
  on Windows and every machine. Rust 1.90+ already uses lld by default
  on x86-64 Linux.

**Keeping it that way.** `tests/dependencies.rs` runs `cargo tree` and
lists every crate the headless build compiles (18 today: duckforge,
png, gif, serde_json and what they use, the same on Linux, Windows and
macOS). A new crate, even one that arrives with an update of `png`,
fails the test with a message saying to measure the clean build first
and, if it's worth it, add the crate to the list. If `cargo tree` can't
run, the test says so and passes, since that isn't the engine's fault.

## Step 10: Genre starters

Two more games to copy, next to the platformer template:
`examples/starter_topdown.rs` and `examples/starter_shooter.rs`, about
700 lines each with their tests (the template is 360). Each uses sprites drawn with
letters, the camera, sounds, labels, `ctx.check` rules, `ctx.tune` with a
tune file, `ctx.idle_ok()` on waiting screens, a recorded playthrough in
`playthroughs/`, and play tests, and passes `tools/verify.sh` with 200
fuzz runs. The quick start lists them.

**Top-down.** The level is a text map (`'#'`, `'c'`, `'e'`, `'p'`), which
is easy to read, change and get right without an editor. Walls are a
grid of booleans, and `wall_touching` only looks at the tiles a body
covers, so collision stays cheap however big the map gets. Movement is
8-direction by default and normalized (a test checks diagonals aren't
faster); `DIAGONALS = false` gives 4-direction movement that follows the
key pressed last. Enemies wander until you're within `sight`, then
chase; sliding along walls (one axis at a time) means they don't get
stuck on corners as often. A hit costs a life, knocks you back and gives
1.5 seconds of safety (tested).

**Shooter.** Bullets move 480 px/s, 8 pixels a frame, more than their
6-pixel length, so they're checked with `swept_aabb` along the whole
move; a play test fires at 3000 px/s (50 pixels a frame, five times an
enemy's height) and still hits. Waves grow from 6 to 18 enemies that fly
in, sway, dive at the ship and shoot. Explosions use their own `Rng`,
seeded once, so adding or removing particles never changes what the
enemies do (the guide's "effects that use `ctx.rng` change the game"
mistake, avoided). The hitbox is smaller than the ship's picture, which
feels fair. The window is tall (240 x 320).

**What building them found.** Verify and the tests caught five problems
before they were committed, which is the point of the tools:

- The top-down game first reported the tile you stand on, to keep
  `log.json` short. The fuzzer then called a short key press a freeze:
  you moved, but not into another tile, so no report changed. It now
  reports `x` and `y` in pixels, like the template.
- The shooter's enemy fire rate used `wave - 1` before the first wave
  (wave 0): an overflow panic, found by the first play test.
- The shooter's rule "no bullet inside an enemy" failed on frame 164 of
  the playthrough: bullets were checked, then the enemies moved onto
  them. Enemies now move first, then bullets are checked against where
  enemies are now.
- `verify`'s first lines for a test that didn't compile named the error
  but not the file and line; it now also prints the `-->` lines.
- The playthrough bots needed a step before reading reports (there are
  none before the first update).

The playthroughs are made by bots in ignored tests (`record_playthrough`):
the top-down one walks the shortest path to the nearest coin that keeps
two tiles from enemies (a breadth-first search on the map); the shooter
one holds fire, flies under the nearest enemy and sidesteps shots.

## Step 11: A skill

`skills/duckforge/SKILL.md` is the workflow for an agent that loads
skills: frontmatter with the name and a one-line "Use when..."
description, then nine short sections: get it and build headless, pick
a starter, the loop with `verify`, tune files, labels and `--describe`,
rules and fuzzing, playthroughs, common mistakes, and showing your work.
It's 150 lines and links to the quick start and the AI guide for
everything else.

**It's not a third copy of the guide.** It says what to run and in what
order, with one example each, and points to the guide for the details.
The mistakes list keeps only the ones that cost the most time (eleven,
from the guide's thirty or so), plus the newest lesson: report what the
player does, or the fuzzer calls moving a freeze.

**It can't go stale without a test failing.** `tests/skill.rs` checks the
frontmatter and length, that every link and every path it names
(`examples/...`, `tools/...`, `playthroughs/...`) exists, that every
`--example` it runs exists, that it only uses `cargo build`, `run` and
`test`, and that every function it calls (`ctx.tune(`,
`canvas.label(`...) is called in `tests/cheat_sheet.rs`, which in turn
fails if a function stops existing. The flags test in `src/cli.rs`
checks every flag it mentions is a real one. Placeholder names for the
reader's own game (`mygame`) are skipped. A deliberate typo in each kind
(an example, a function, a file, a flag) was tried, and each failed with
a message naming the problem.

## Step 12: Proving it with Pollen Run

`examples/pollen_run.rs` was made from the top-down starter by following
only `skills/duckforge/SKILL.md` and the quick start: copy the starter
and its tune file, add it to `Cargo.toml`, then the loop with
`tools/verify.sh`. A bee picks pollen from flowers (three at most, each
slowing it by 12%) and carries it to the hive, while wasps chase it; a
sting costs a life and puts the pollen back on its flowers. It has six
rules (`"pollen adds up"`: delivered + carried + on flowers is always the
total), five play tests, a tune file, labels and a bot-made playthrough,
and passes verify with 200 fuzz runs and `--release --fuzz 200`.

**What the loop found and what was fixed.**

- *Balance (tuning).* The bot's first playthrough was stung out after
  3 of 18 pollen: with a full bag the bee flew at 52 px/s, the wasps at
  50, so it couldn't escape. Five tune settings tried by editing the tune
  file and re-running the bot: slowdown 0.12, wasp speed 42 and sight 80
  let the bot deliver 15 of 18 in 70 seconds. Copied back into the code.
- *A copied tune file quietly changed the game.* The starter's file set
  `sight` to 100 (the new code said 90), and its `player_speed` and
  `enemy_speed` meant nothing here, with no warning. The summary now warns
  about tune-file names the game never asks for, and `--write-tune` says
  which ones it kept.
- *A false alarm.* "label wasp is off screen in every checked frame" came
  up on every short run, since the wasps start out of view. That warning
  now only applies when the camera never moves; in a scrolling game,
  things off screen are normal. (It would have failed `verify`'s
  `--quiet` replay.)
- *verify hid the useful line.* A failed play test showed where it
  panicked but not the message (`47.5 vs 30.4`), which is on the next
  line. It now shows both. (The failure was my test hard-coding a 60%
  speed limit; it now uses the constant.)
- *The mp4 was tiny:* 160 x 120, because the GIF is shrunk to stay small.
  The printed `ffmpeg` command now enlarges it 4 times with sharp pixels
  (640 x 480 for a 320 x 240 game).
- The fuzzer found nothing in 400 runs. That's good, but see below.

**What still felt slow or uncomfortable** (not fixed; candidates for later):

1. *Starting a game is five hand edits*: copy two files, add three lines
   to `Cargo.toml`, change the title, change the playthrough path in the
   bot test, then rename `TopDown`/`Enemy` everywhere. A
   `tools/new_game.sh <name> <starter>` would make it one command.
2. *The bot is the slowest part to write.* Every playthrough here came
   from a bot in an ignored test (a breadth-first search for the top-down
   games), about 60 lines, and how well it plays decides what the
   playthrough shows. This one stops when every path passes near a wasp.
3. *Playthroughs go stale silently.* After changing a tune value or
   anything that uses `ctx.rng`, replaying the old inputs tells a
   different story, and `verify` still says `ok` (it only checks that
   nothing broke). Re-recording is easy to forget.
4. *The fuzzer gives no sense of coverage.* "No problems found" doesn't
   say whether any random run ever delivered pollen or reached the end.
   The range of each report across all runs (delivered 0 to 2, say)
   would show what the random player reached.
5. *The contact sheet spreads 16 frames evenly*, so a 70-second run can
   skip the moment of a sting. Frames chosen where a report changes
   (`lives`, `state`) would show the events.
6. *Small sprites are hard to judge on the sheet*: a 12-pixel bee in a
   16-frame grid, and with `--debug` the labels' names cover it.
7. *The Windows batch file has never run on Windows* (there's none here);
   it mirrors the shell script line by line and a test checks it uses
   the same flags, but that's all.
8. *Starters are long* (about 700 lines with tests and bot), more to read
   before the first change than the 360-line template.

What was not slow: a one-line change rebuilds in under half a second,
and `verify` takes 2 to 3 seconds including 200 fuzz runs, so the loop
itself never got in the way.

# Phase 2.5: rough edges found by reviews

## Fix 1: Playthroughs cut short silently

A hand-written playthrough with presses up to frame 898 and no
`"frames"` ran for 600 frames (the default for any headless run), so 39
of its 118 events were never replayed, and `verify` still said ok.

Now a script without `"frames"` runs until two seconds after its last
event (`InputScript::run_length`), on the command line and in play
tests, so you see what the last press did. `"frames"` and `--frames`
still win when given. Two seconds rather than "exactly the last event":
a script whose last event is a release would otherwise stop on the frame
that shows nothing new.

And if a run stops before some events anyway (a `--frames` or `"frames"`
that's too short), the summary warns, naming the file, how many events
were skipped and the frame they need: `playthroughs/hand.json: 39 of its
118 input events come after frame 600, ...`. It's an ordinary warning,
so it's in `log.json` and `--quiet`, and `verify` fails that playthrough
with the message. A game that quits early isn't warned about: the events
after a quit can't matter. (`HeadlessOptions` gained `inputs_file`, so the
warning can name the file.)

## Fix 2: Playthroughs that go stale

After a change to a tune value, or to anything that uses `ctx.rng`, an
old playthrough replays the same keys into a different game. It still
ran without errors, so `verify` said ok while the playthrough no longer
showed what it was recorded to show.

**A playthrough now says how it ends.** An input script can have an
`"expect"` block: the game's reported values at the end, like
`{"delivered": 15, "lives": 0, "state": "Lost"}`. `sim.recording()`
writes it, so every way of recording gets it for free: F10 and
`--record` in a window, and the starters' `record_playthrough` bots.
A replay of the script's own length compares its final values with it.
A difference isn't an error; the summary says `playthrough changed:
delivered 15 to 9, state "Lost" to "Won"; if that's right, accept it
with --accept (or re-record it)`, and so do `log.json`
(`"playthrough_changed"`) and `--json`.

**`verify` calls it a warning, not a failure:** the line reads `changed`,
followed by the exact command to accept the new ending, and the run ends
`VERIFY OK with 1 warning(s)` with exit code 0. A changed ending is often
what you wanted (you made the wasps slower on purpose); a failure would
push an agent to accept without looking. The warning makes it visible
and the decision explicit.

**`--accept`** (with `--headless --inputs FILE`) writes the run's final
values into the file's `"expect"`. The existing playthroughs were given
their blocks this way; each file gained one line, since the format is
the same one `to_json` already wrote.

Only final values are compared, not the whole history: they're what a
person would call "how it ended", they're short enough to read in the
file, and any change along the way that matters usually changes them.
Positions are included when the game reports them, so a different route
to the same score is still noticed.

`InputScript` gained a public `expect` field. Code that builds an
`InputScript` with every field written out needs `expect: None` (one
test here did); code using `..InputScript::default()` is unaffected.

## Fix 3: Fuzzing's coverage

"No problems found" after 200 runs of Pollen Run said nothing about
whether any random player ever delivered pollen. Now `--fuzz` ends with
coverage lines, added up over all runs from each run's `log.json`:

```text
duckforge: coverage: carried 0 to 3, chasing 0 to 3, delivered 0 to 2, lives 0 to 3, x 16 to 548, y 16 to 388
duckforge: coverage: state reached Lost, Playing; every run lasted all 1800 frames
```

- each reported number's lowest and highest (`always 0` if it never moved);
- each text or yes/no value reached;
- `never Won (where playthroughs/breakout-win.json ends)`: text values the
  game's playthroughs end on (their `"expect"` blocks, from Fix 2) that no
  random run reached. That's the honest way to say "never": the engine
  can't know every state a game has, but it knows the ones you've shown
  it matter;
- how far runs got: all the way, or the furthest frame and how many runs
  ended early (the game quit, or crashed; a crash leaves no log, so it only
  adds its frame).

`verify` prints the same lines, as `coverage:`, under its fuzz step. On
Breakout it shows straight away that random keys never clear more than
13 bricks or win, so the late game is only covered by `breakout-win.json`.

## Fix 4: A contact sheet of what happened

A 70-second run spread over 16 frames is a frame every 4.5 seconds; the
Pollen Run sheet skipped most of the moment the bee lost three lives.
`--sheet-events` picks the frames by what happened instead
(`src/sheet.rs`):

- the first and the last frame;
- frames where a text or yes/no report changed (`state`), then frames
  where a number changed (`lives`, `score`, `delivered`), spread out if
  there are more than fit;
- evenly spread frames to fill the rest, up to 16, chosen to fill the
  biggest gaps.

Under each frame's number is what changed, like `LIVES 1` or
`STATE LOST, LIVES 0` (or `START` and `END`). A value that changes more
than 20 times in a run (a position, a timer, `carried` in Pollen Run)
isn't an event, so it stops counting once it passes 20, and frames kept
only for it are dropped.

**Keeping the pictures.** Which frames matter is only known at the end
(a value is only "churn" once it has changed 21 times), and runs can't be
rewound, so while the game runs a copy is kept of each frame that might
be chosen: at most 20 per value, plus the 16 spread frames. For a
320 x 240 game that's about 300 KB each, a few megabytes in practice.
Running the game a second time instead would double the time of every
run, and needs a game that can be built twice.

`verify` now uses `--sheet-events` for every playthrough. Plain `--sheet`
is unchanged (evenly spread, or the snapshots with `--snapshot-every`).

## Fix 5: Small sprites on the sheet, and debug label names

A sheet frame of a 320 x 240 game is shown at its own size, but a
12-pixel bee in it is still a smudge. `--sheet-follow NAME` also saves
`sheet-follow.png`: the same frames (evenly spread, or the event frames
with `--sheet-events`, with the same captions), each cut to the third of
the screen around the label `NAME` and enlarged three times, so the bee
is 36 pixels across. The label's position comes from each frame's scene,
which the sheet now keeps alongside the picture. A frame where the label
is missing (the player is dead, say) is centred where it was last seen
and captioned `(no bee)`. The normal `sheet.png` is still saved.

The captions under sheet frames were the smallest text, too small to
read once the sheet is shown on a screen; they now use the same size as
the frame numbers, cut to the width of the cell.

**Debug label names.** With `--debug`, each label's name was drawn just
above its box, but at the top of the screen it was pushed inside the box,
and green text on a green lawn couldn't be read. The simplest readable
fix: the name always sits just outside the box (above it, or below it
when there's no room above) on a small dark strip. It can still cover a
neighbour, but never the thing it names. (Smaller text isn't possible:
the font is already the smallest that reads; leaving names off small
labels would hide exactly the ones that are hard to tell apart.)

## Fix 7: Shorter starters

The starters were single files of about 700 lines: the game, then its
play tests, then the bot that records the playthrough. The bot was
already at the very end, so moving only the bot wouldn't change what a
reader meets first. Each starter is now a folder that Cargo finds by
itself (`examples/starter_topdown/main.rs` is the example
`starter_topdown`):

| | Before | After |
|---|---|---|
| `starter_topdown`: the file you open | 710 lines (game 537, tests 98, bot 75) | `main.rs` 546 (the game, and where the rest is) |
| | | `tests.rs` 100: the play tests |
| | | `bot.rs` 75: `record_playthrough` and its path-finding |
| `starter_shooter`: the file you open | 687 lines (game 537, tests 94, bot 56) | `main.rs` 546, `tests.rs` 97, `bot.rs` 56 |

The game part itself is the same 537 lines (plus a short note at the end
saying where the tests and the bot are): it's the game that's meant to be
read and changed, and cutting it would remove features the starters
exist to show. What changed is that opening the starter now shows only
the game. `main.rs` ends with `#[cfg(test)] mod tests;` and
`#[cfg(test)] mod bot;`, and both files start with `use super::*;`, so
they can still reach everything private in the game. Re-running both
bots wrote byte-for-byte the same playthroughs.

The template (360 lines, no bot) and the quick start game stay single
files: the quick start's point is a whole game with its test on one page.
`verify` accepts either layout.

## Fix 6: One command to start a game

Starting from a starter was five hand edits (copy two files, add three
lines to `Cargo.toml`, change the title, change the bot's playthrough
path, rename the main type everywhere). Now:

```sh
tools/new_game.sh bee_maze topdown      # or platformer, shooter, tiny
```

copies the starter (a file or a folder) and its tune file, renames the
game's type (`TopDown` becomes `BeeMaze`, or `BeeMazeGame` if the starter
already uses that word, as with `enemy` and the top-down `Enemy`), sets
the title (`"Bee maze - duckforge"`), replaces the starter's name in
commands and paths (`playthroughs/bee_maze.json`), adds the `[[example]]`
entry, records a first playthrough with the starter's bot (so verify has
something to replay), and runs `tools/verify.sh bee_maze`. A name that's
taken or isn't lowercase letters, digits and `_` is refused before
anything is written.

**Whole words only.** Replacements use "not next to a letter, digit or
`_`" on both sides, written as plain extended regular expressions, which
work in both GNU and BSD `sed` (Linux and macOS). The title is replaced
before the type, since `"Template - duckforge"` contains `Template`; the
quick test caught that order mistake.

**Windows.** `tools/new_game.bat` does the same, but the text edits are in
`tools/new_game.ps1`: a batch file can't safely replace whole words
inside files, and PowerShell comes with Windows. It's run with
`-ExecutionPolicy Bypass`, which needs no administrator rights. It writes
UTF-8 without a byte order mark, like the files it read.

**Tests.** `tests/new_game.rs` copies the project (without `target/`)
to a temporary folder. The quick test makes a game from each of the four
starters with `NEW_GAME_NO_VERIFY=1` and checks the type and starter
name are gone, the new type, title, tune file, bot path and `Cargo.toml`
entry are there, the starter itself is unchanged, and that a taken name,
a bad name and an unknown starter are refused. The slow test, ignored by
default (`cargo test --test new_game -- --ignored`, about 30 seconds),
makes all four games and runs `verify` on each in its own target folder;
all four pass. Both need `bash`, so on Windows they say so and pass.
There's no CI here, so the slow test is run by hand before a release.

## Fix 8: The Windows scripts, read line by line

`tools/verify.bat`, `tools/new_game.bat` and `tools/new_game.ps1` have
never been run on Windows (there's none here, and running a downloaded
PowerShell for Linux to try the helper was, reasonably, not allowed in
this sandbox). So they were read line by line for the usual traps:
paths with backslashes and spaces, quoting, `errorlevel`, brackets in
`if` blocks, and how cargo's output is read. What was found and fixed:

- **`!` disappeared from messages.** The scripts use delayed expansion
  (`!var!`), so every line echoed from a file lost its `!`, and text
  between two `!` vanished: `assert_eq!(a, vec![1])` came out mangled in
  the very messages that say what failed. The failure details (the lines
  around "panicked", compile errors) and the first lines shown under a
  failed step are now printed by PowerShell, which copies them exactly.
- **Compile errors without a place.** A build failure showed `error` lines
  but not the `--> examples/x.rs:12:5` lines under them; it now shows
  both, like `verify.sh`.
- **"no tests" when they didn't compile.** Now "did not run", like `verify.sh`.
- **An empty `-Title ""`** for the tiny starter (it has no title). Windows
  PowerShell 5, which comes with Windows, can't take an empty argument with
  `-File` ("Missing an argument for parameter"); the argument is now only
  passed when there's a title.
- Both scripts now also show the `left:` / `right:` lines of a failed
  `assert_eq!` (so does `verify.sh`).

Checked and fine: `cd /d "%~dp0.."` (any drive, spaces in the path),
every path quoted, `if errorlevel` used inside blocks (it's read when
the line runs, unlike `%errorlevel%`), brackets in messages escaped with
`^`, `findstr` letter ranges avoided (its `[a-z]` also matches capitals,
so the name check spells the letters out), the `test result:` line read
from cargo, and `.gitattributes` giving `.bat` files Windows line endings
(cmd's labels can break without them; a `git clone` gets them, a ZIP
download may not, which the README says).

**What a test can check without Windows.** `tests/windows_scripts.rs`
checks that brackets balance outside quotes (an unclosed `(` makes cmd
read the rest of the file as one block), that every `goto` and
`call :label` has its label, that `new_game.bat` passes only parameters
`new_game.ps1` declares, and that `.gitattributes` still asks for CRLF.
Each was tried with a planted mistake. The flags test already checks the
two verify scripts use the same engine flags.

**How to test on Windows** is in the README ("Testing the Windows
scripts"): seven steps, from a normal `verify` run to a changed
playthrough, a failing test, `new_game` and wrong arguments, each with
the output to expect.

**Follow-up to Fix 4: the rarest numbers first.** Making the deliverables
for this phase showed the event sheet of a fresh top-down game skipping
"lives 2": the starter's `chasing` count changed 19 times (just under the
limit of 20) and its frames, spread out, crowded out the two rarer
`lives` frames. Numbers are now taken by how rarely they changed: every
`lives` change first (3 in the run), then `coins` (9), then `chasing`
(19), each spread out only if it doesn't all fit. The sheet now shows all
three lives lost and every coin. (Tested with a run like that one.)

# Phase 3: what real test games found

An AI agent built Duck Dash (a 2D auto-runner) and started Duck Tower (a
platformer with painted backgrounds and a boss), Duck Dash 3D (a
three-lane runner) and Duck Craft (a side-view mining game: 3D cubes,
2D characters), using Duck Quill for art and Duck Echo for sound. This
phase fixes what they had to work around. The plan is in
`docs/phase3-design.md`.

## Step 0: Baseline

Measured on the cloud machine (4 cores, Rust 1.97) before any change, to
compare against at the end:

| What | Time |
|---|---|
| Headless clean build of `dodge` (empty target folder) | 10.8 s |
| ...after changing one line of `dodge` | 0.30 s |
| Headless test suite from an empty target folder (`cargo test --no-default-features`) | 32.2 s (345 tests) |
| Headless test suite, already built | 11.2 s |
| Default-features test suite, already built | 11.3 s (362 tests) |
| `stress3d`, 500 objects: 21,846 triangles, CPU, 320 x 240 | 3.0 ms per frame |
| `stress3d`, 2,000 objects: 89,412 triangles, CPU, 320 x 240 | 8.6 ms per frame |

The stress numbers come from a new ignored test in `examples/stress3d.rs`
(`cargo test --release --no-default-features --example stress3d --
--ignored --nocapture`), which draws every frame on the CPU. stress3d's own
`--seconds` benchmark needs a window: a headless run only draws the frames
it saves, so it would time the game logic, not the drawing. There's no
graphics card here (no Vulkan driver, no EGL), so the GPU renderer can't
be measured in this container.

All 15 examples pass `tools/verify.sh`.

Duck Quill's format, read from its repository (still named `art-app`;
the `quill` command is on its `phase-5` branch, `art-cli` on `main`, and
the export format is the same on both): `quill export` writes a PNG with
one row per animation and a JSON with `frame` size, `columns`, `rows`,
`scale`, `pivot` (in scaled pixels, bottom centre by default) and
`animations` (each with `row`, `frames`, `fps` and `loop`). It has **no
tileset or atlas export**. Duck Echo writes plain WAV files (16-bit,
24-bit or float), which `Wav::load` already reads, with no loop points.

## Step 1: The design

`docs/phase3-design.md`, approved with changes: whole-number video
scaling with black bars; a Duck Quill sprite sheet as the tileset
format for now (kept easy to extend for a real export later); tune
changes in a recording are a warning while the ending still matches,
and the likely cause of a failure when it doesn't; the three-lane runner
as the 3D starter; Mesa's software Vulkan driver installed here for
CPU/GPU comparisons; upright billboards by default. Also: fast moves in
`move_and_slide` split into steps, animated tiles take the time,
parallax on both axes, a clear error for merging meshes with different
textures, and a sprite sheet test at scale 2.

## Step 2.1: Duck Quill sprite sheets

`SpriteSheet::load("hero.json")` reads `quill export`'s two files: the
animations by name (`sheet.animation("run")`, with `fps` and `looping`),
the pivot (bottom centre unless the JSON says otherwise), and drawing by
the pivot (`sheet.draw`, or `canvas.draw_at_pivot` for any sprite), which
keeps the feet in place when mirrored or enlarged.

**The size check.** A game once got a sheet six times too big, with no
warning: the PNG had been saved with `quill render --zoom 6` instead of
`quill export`, so every frame was cut from the wrong place. `load` now
checks the PNG is exactly `columns × width` by `rows × height` and, when
it's a whole number of times too big, says so:
`duck.png is 96 x 64, but duck.json says 3 x 2 frames of 16 x 16 (48 x 32); it's exactly 2 times too big: was it saved with quill render --zoom 2? Use quill export ...`.

`Animation` gained a public `looping` field (true by default: nothing
changes for existing games); a non-looping animation stays on its last
frame. Only the engine built `Animation` with a struct literal, so no game
breaks. The tests use real `quill export` sheets at scale 1 and 2
(`tests/fixtures/quill/`, with the script that made them): at scale 2 the
frames and the pivot are doubled, and drawing either by its pivot covers
exactly the same pixels.

## Step 2.2: Tune files that can't surprise you

Three ways the tune file caught the test games out, and what changed:

- **`new_game` copied the starter's tune file**, so speeds written in the
  new game's code were quietly replaced by the starter's. It no longer
  copies it; a new game uses its code's values until `--write-tune` makes
  a file.
- **Bots and replays read different values.** The tune file is found by
  the program's name, and a bot in a test program with another name got
  none. Two fixes: a game can name its file (`Config { tune_file:
  Some("examples/mygame.tune.json".into()), .. }`), read by windows,
  headless runs, bots and play tests alike (a missing named file just
  means the code's values); and every recording now stores the tune
  values it was made with (`"tune": {...}` next to `"expect"`). A replay
  with other values is, as decided: a **warning** if it still ends as
  expected (`playthrough tuned: recorded with other tune values (wobble
  0.5 to 0), and it still ends as expected`), so you can tune without
  re-recording; and a **failure** if the ending changed too, naming the
  tune values as the likely cause and how to re-record or `--accept`.
  `--accept` stores the current tune values with the new ending.
  Existing playthroughs have no `"tune"` block, so nothing changes for
  them until they're recorded again.
- **The tuned values were easy to miss**, on a line near the end of the
  summary. They're now the second line, in capitals, and say when a value
  isn't the code's: `TUNED from examples/x.tune.json: sight = 100 (code
  says 90)`. `verify` prints that line under each playthrough.

## Step 2.4: `--video`, every frame, sharp

`--gif` was made for a chat message: small, about 20 pictures a second,
no sound. To show a game to a person properly, `--video out/run.mp4`
saves every frame at the game's own frame rate, with the sound.

- **No new dependency.** Encoding H.264 in Rust would be a heavy crate;
  `ffmpeg` is already what the GIF note pointed people to, so the run
  pipes its raw frames (RGB, the game's own size) to an `ffmpeg` process
  as they're drawn, and adds `audio.wav` in a second pass at the end
  (copying the picture, so nothing is encoded twice). `DUCKFORGE_FFMPEG`
  names another `ffmpeg`. Without one, the run doesn't fail: it saves the
  frames as PNGs and prints the exact command that joins them.
- **Sharp pixels, as decided:** the game is enlarged by the biggest whole
  number that fits in 1920 x 1080 (320 x 240 becomes 1280 x 960, four
  times) with nearest-neighbour scaling, and centred on black. Filling
  the height exactly (4.5 times) would make some pixels wider than
  others. A game bigger than 1080p is kept as it is, rounded to even
  sizes, which H.264 needs.
- **Cost:** Dodge's 30.7-second playthrough (1,842 frames) takes 7.7 s
  with `--video`, a 1920 x 1080, 60 fps file with sound. Drawing never
  changes the game, so the video can't change what happens.

## Step 2.5: Off-screen labels in long levels

Phase 2 stopped the "off screen in every checked frame" warning in any
game whose camera moves, which also hid the bug it was made for (a label
drawn in screen pixels through a camera, or the reverse). The test games
showed the other half: a camera that hasn't moved yet in a short run, with
enemies further along a long level, warned about every one of them.

Now the warning looks at where the world is, not at whether the camera
moved. The world is everything the camera showed in the checked frames,
plus every label of a kind that was on screen at least once (those are
drawn in the right coordinates, and the level, its ground and its coins
show how far it goes). A label kind that's never on screen is only
reported when it's also outside all of that, with its position and the
world's: `label "ghost" at -500,0 is off screen in every checked frame
(1, 15, 30), and outside the world the camera showed (1,0 to 94,8)`.
Pollen Run's wasps, the starters and Dodge stay quiet, on full and short
runs. What it can't know: a level that labels nothing but its far-off
enemies. Labelling the level (or, from Step 3, drawing a tile map, which
labels itself) fixes that.

## Step 2.6: Looping sounds and music

Duck Dash wanted music and Duck Craft a hum, and a long `.wav` played
once was the only way. Now `ctx.play_loop(&music, 0.6)` returns a
`LoopId`; `ctx.set_loop_volume(id, 0.3)` and `ctx.stop_loop(id)` change
it. It takes a `Wav` (a Duck Echo export loops with no gap: the whole
file repeats, sample after sample) or a synthesizer `Sound`.

- **Same as one-off sounds, everywhere.** A loop starts, changes and
  stops at the first sample of the frame the game asked in, so headless
  runs mix it into `audio.wav` exactly (a test checks a 1,000-sample ramp
  goes round with no gap, halves on frame 20 and stops on frame 40), the
  window's speaker plays the same thing, and `--video` gets it for free.
  A loop still playing at the end stops with the run.
- **Recorded, not only heard.** `log.json` has `"loops"`, one line each:
  `{"loop": "theme", "file": "...", "start": 1, "stop": 40, "volume":
  [[1, 1], [20, 0.5]]}`; play tests read `played.loops()` (with
  `volume_at(frame)` and `is_playing()`); the summary adds `loops: theme
  from frame 1 to frame 40` to its sounds line. Volume changes are only
  recorded when the volume changes, so a fade is a line per frame of the
  fade, not per frame of the game.
- **Left out:** pitch changes while playing, stereo panning, loop points
  inside a file (Duck Echo has none), and crossfades (lower one loop's
  volume while raising another's). A window forgets stopped loops and old
  volumes, like it forgets one-off sounds, so hours of play don't fill
  memory.
- **Fixed on the way:** the window ignored `Config::tune_file` (Step 2.2)
  and only looked for `examples/<game>.tune.json`. The window, headless
  runs and `Simulation` now choose the tune file with the same function.

## Step 2.7: Small ones

- `Rect::top_left()`, `top_right()`, `bottom_left()`, `bottom_right()`,
  next to `center()`: the test games wrote `vec2(r.x, r.bottom())` by
  hand for feet and ledges.
- `InputScript::from_file(path)` and `InputScript::from_json(text)`, the
  names the test games guessed first, as plain other names for `load`
  and `parse` (the cheat sheet lists all four), and `InputScript` is now
  in the prelude, since bots and play tests use it.
- `new_game duck_tower` makes "Duck Tower", not "Duck tower". Its test
  then caught a real bug: the shooter starter became "My MyShooter -
  duckforge", because the type was renamed after the title was set and
  the new title contains the old type's name. The title is now set aside
  while renaming (`new_game.sh` and `new_game.ps1`, which `new_game.bat`
  runs).

## Step 2.8: The docs point to Duck Quill

The guide has a paragraph on art from Duck Quill (paint, `quill export
... --sheet`, `SpriteSheet::load`, draw by the pivot, and why `quill
render --zoom` pictures are refused), and the cheat sheet lists
`SpriteSheet`, `draw_at_pivot`, `Animation::looping`, `Config::tune_file`
and what recordings now do with tune values, each called in
`tests/cheat_sheet.rs` (on the real Duck Quill export in
`tests/fixtures/quill`). The quick start (still 125 lines), the skill
(149), the tutorial (sprite sheets and loops, for beginners) and the
README say the same in fewer words. Duck Echo is named as the place
sounds and music come from.

**2.3, a bot for the platformer starter, moves into Step 3.4**, where the
starter is rebuilt on the tile map, background layers and a sprite sheet:
a bot written now would be thrown away with the old level a step later.

## Step 3.1: Tile maps from text

Every test game wrote its own level grid, its own "which tile am I in",
and its own collision, and three of the four had a bug in it (a seam
that snagged the player, a fast fall through a floor, a platform you
could stand on from below). `TileMap` does it once:

- **A level is text**, one letter per tile, with a `Legend`:
  `.solid('#', "ground")`, `.one_way('=', "plank")`, `.decoration('f',
  "flower")` (drawn, never bumped into), `.spawn('p', "player")`. `.` and
  space are always empty, rows can differ in length, and blank lines
  around a level are ignored, so it can start on the line after `"`. A
  letter the legend doesn't have is an error with its row and column,
  and the file: `levels/1.txt row 3, column 17: 'x' isn't in the legend
  (it has # = p c . and space)`.
- **Spawn points** are the bottom centre of their tile, where feet go:
  `map.spawn("player")` (which panics naming the spawn points there are,
  rather than giving a player at 0,0) and `map.spawns("coin")`.
- **`move_and_slide(&mut body, velocity * dt)`** moves sideways, then up
  or down, and reports `ground`, `ceiling`, `wall_left` and `wall_right`.
  As decided, it moves in steps of at most half a tile, so a body moving
  300 pixels in one frame stops at a wall one tile thick, and one falling
  2,000 pixels lands on a floor or a one-way platform instead of passing
  through (tested). Tiles count only when a body is inside them by more
  than a thousandth of a pixel, so standing exactly on a floor isn't
  touching the floor's row, and walking along a floor of separate tiles
  never snags on the seams. One-way platforms are landed on only by feet
  that were at or above their top before the step.
- **Drawing** happens between `push_camera` and `pop_camera`, like the
  rest of the world: `map.draw(canvas, &tiles, self.time)`. The time is a
  parameter, as decided, so animated tiles move, and a run stays exactly
  repeatable (the game keeps the time in `update`). It asks the canvas
  what's visible (`canvas.visible_area()`, now public), so a 5,000-tile
  level draws 21 columns. Each tile is stretched to whole pixels that
  meet its neighbour's, so there are no seams at any zoom, 1.3 included.
  It labels the whole map `"map"`, which tells the summary's off-screen
  warning (Step 2.5) how big the world is.
- **Changing a level while playing**: `clear_tile` and `set_tile` (a
  broken block, a door). Outside the map is empty, so bodies can leave
  it; a game adds walls or a `ctx.check`.

## Step 3.2: Tilesets

A `Tileset` is a list of named tiles, each an `Animation`, filled in one
of three ways: `Tileset::grid(&sheet, 16, 16, &["ground", "", "plank"])`
(a picture cut in a grid, `""` skipping a cell), `Tileset::load(json)`
(a Duck Quill sprite sheet, as agreed: one animation per tile, several
frames for an animated tile), or `insert(name, animation)`. When Duck
Quill gets a real tileset export, it's one more way to fill the list.
A tile the map uses but the tileset lacks panics on the first draw,
naming the tiles there are.

## Step 3.3: Repeating background layers

`Background::new(picture)` with `.parallax(0.5)` (both directions, as
far things move slower both ways), `.parallax_xy(0.5, 0.1)` (as decided,
for Duck Tower's climb), `.offset(vec2(0.0, 120.0))`, `.repeat_y(true)`
and `.repeat_x(false)`. `canvas.draw_background(&layer, &camera)` draws
in screen pixels wherever it's called, even inside a camera. The copies
go at whole pixels, edge to edge, so a test that checks every pixel of
the screen against the repeated picture finds no seam, at camera
positions with fractions and negative ones, and when the camera climbs
40 pixels the far wall at 0.25 moves down exactly 10. Backgrounds aren't
zoomed with the camera (a zoomed parallax layer would need its own
scale; no test game wanted it), but they do shake with it.

## Step 3.4: The platformer starter, rebuilt

`examples/template.rs` (a level of rectangles in code, a duck drawn with
letters, its own collision loop) is now `examples/template/`, built on
Steps 2 and 3, in the same folder layout as the other starters:

- `main.rs`: the level is an 80 x 15 text `LEVEL` (grass, dirt, bricks,
  one-way planks, water pools, a start, 8 coins and a flag) read into a
  `TileMap` with a `Legend`; `move_and_slide` replaces the hand-written
  collision; the duck is a `SpriteSheet` (`idle`, `run`, `jump`, `fall`)
  drawn by its pivot; the tiles, the coins and the flag come from one
  Duck Quill sheet (as a `Tileset` and as a `SpriteSheet`); a sky
  (parallax 0.1) and hills (0.4) are `Background` layers. Falling in the
  water puts the duck back at the start; the flag ends the level. 372
  lines, about the same as before.
- `art/`: the four Duck Quill scripts that made the pictures (written as
  `pixels` rows and symbols), with the commands to make them again;
  `assets/`: the pictures, 4 KB in all. Painting them found one thing
  worth knowing: `quill render` puts white paper behind a picture unless
  told `--transparent`, which showed as a white band above the hills.
- `tests.rs`: 8 play tests (running, jumping, a plank jumped through from
  below and stood on, the brick wall, the water, pausing, and the
  recorded playthrough reaching the flag with every coin).
- `bot.rs` (this is 2.3): it heads for the leftmost coin, then the flag,
  and jumps for coins above it, at walls and at water, or when it's been
  stuck for half a second. It reached the flag with all 8 coins on its
  first run, in 11.8 s of game time; `playthroughs/template.json` is its
  recording.

`template.tune.json` is gone: it only repeated the code's values, and now
that the summary prints a `TUNED` line for every value from a file, it
was noise. `tools/new_game.sh mygame platformer` copies the folder like
the other starters, and its test now checks `main.rs`, `tests.rs` and
`bot.rs` for the platformer too.

Checked at the end of Step 3: all 15 examples pass `verify`; a game made
from each of the four starters passes it too (23 s for all four); 401
tests pass (383 headless); the headless clean build of `dodge` takes
10.5 s (10.8 s at Step 0) and a one-line change 0.27 s (0.30 s).

## After Step 3: `verify mygame 0`

`tools/verify.sh mygame 0` failed: `--fuzz 0` is an error, so the fuzz step
reported `FAILED`. Zero runs now skips the step (`fuzz  skipped: 0 runs
asked for`) without counting as a problem, and a count that isn't a
number (`verify.sh mygame lots`) is a usage error, exit code 2, in
`verify.sh` and `verify.bat` alike (there's no `verify.ps1`: the batch
file is plain `cmd`). The README's Windows checklist gets a step for it,
and its `new_game` step now expects "Bee Maze" (Step 2.7) and no tune
file (Step 2.2), and checks the shooter's title.

# Phase 3, Step 4: the 3D upgrade

## Step 4.0: A GPU to compare with, and the "before" numbers

Mesa's software Vulkan driver (lavapipe: `apt-get install
mesa-vulkan-drivers`) gives this container a Vulkan device, `llvmpipe`.
The 8 GPU tests that used to skip here now run, and pass. Two small
additions make comparisons possible:

- **`DUCKFORGE_RENDERER=gpu` now applies to headless runs too.** A
  headless run still draws 3D on the CPU unless asked, so its pictures
  are the same on every computer; with the variable set, the same command
  gives the GPU's pictures, to put side by side.
- **`Simulation::with_renderer(config, game, Renderer::Gpu)`**, next to
  `Simulation::new` (always the CPU), for tests that compare the two.

Before any 3D change, every 3D example (`world3d`, `stress3d`,
`door_show_3d`) was saved headless at frames 40 to 240 with both
renderers, to check later that the default lighting leaves every picture
identical. The CPU and GPU already agree: `world3d` has no pixel more
than 3 apart (242 pixels differ by 1 to 3).

stress3d's timing test now times both renderers
(`cargo test --release --example stress3d -- --ignored --nocapture`):

| Before Step 4 | 500 objects (21,846 triangles) | 2,000 objects (89,412) |
|---|---|---|
| CPU, 320 x 240 | 2.85 ms per frame | 7.99 ms |
| GPU (lavapipe, which is software too) | 4.34 ms | 12.06 ms |

lavapipe runs on the same CPU cores, so its numbers say nothing about a
real graphics card; it's here for correctness, not speed.

## Steps 4.1 and 4.2: Textures and lighting

**Textures.** A mesh can have one `texture` (a `Sprite`, shared, not
copied) and texture coordinates for each triangle's corners (0 to 1
across the picture, repeating beyond). The coordinates live on the mesh
(`mesh.uvs`, one entry per triangle, `None` for a plain one), not on
`Triangle` or on each vertex as the design first said: `Triangle` gaining
a field would have broken `door_show_3d`, which builds `Triangle {
corners, color }` itself, and per-corner coordinates let a cube keep its
8 corners and still show a whole picture on every face. Every built-in
shape now comes with them (same vertex and triangle counts), so
`Mesh::cube(Color::WHITE).with_texture(picture)` is a crate, and
`add_triangle_uv` / `add_quad_uv` build your own.

- The texture's colour is multiplied by the triangle's colour (white
  shows it as painted) and the light. Nearest pixel, so pixel art stays
  sharp. Pixels with alpha below half aren't drawn at all, depth
  included, so what's behind shows through (tested).
- On the CPU, `u/z`, `v/z` and `1/z` are blended across the screen and
  divided per pixel: perspective-correct. The test looks at a long floor
  from one end; the colours swap exactly where its middle line is.
- On the GPU, each vertex carries its texture point and two flags
  (textured, glowing); textures are uploaded once, cached by picture
  (kept alive while cached, so an address can't be reused by another
  picture), and bound per mesh; a 1 x 1 white texture stands in for
  meshes without one. `textureSampleLevel` rather than `textureSample`,
  which isn't allowed inside the shader's `if`.

**Lighting.** `canvas.set_lighting(Lighting { sun_direction, sun_color,
sun_strength, ambient })`, lasting until changed; `Mesh::glowing()`
ignores it. `Lighting::default()` is the old light, and the arithmetic is
arranged so the default gives the old numbers bit for bit: each colour's
brightness is `ambient + (1 - ambient) * (sunlight * share)`, where
`share` (the sun's colour times its strength) is exactly 1.0 by default,
and with white light the CPU still uses the very same `lerp` as before.
**Checked:** all 42 saved pictures (frames 40 to 240 and the last frame
of `world3d`, `stress3d` and `door_show_3d`, on the CPU and the GPU) are
byte for byte the same as before Step 4.

**CPU and GPU agree** on a new test scene with textured floor, crate and
ball, see-through texels, a glowing lamp and an orange sun: no pixel
differs by more than 3 on lavapipe. An ignored test saves the two side by
side (`cargo test --release --lib cpu_and_gpu_side_by_side -- --ignored
--nocapture` writes `out/cpu-vs-gpu.png`), for checking a real graphics
card. stress3d (untextured) on the CPU: 2.91 and 8.09 ms per frame (2.85
and 7.99 before; the same within noise).

**Mesh helpers (4.4) came along**, since the tests needed them:
`Mesh::quad(w, h)`, `cylinder`, `cone`, `block(&atlas, [top, sides,
bottom])`, `merge(&other, &transform)` and `bounds()`. `merge` with two
different textures panics: "these meshes have different textures, and a
mesh has only one; put their pictures in one atlas"; `Mesh::block` with
tiles from two pictures says the same. A plain mesh merged with a
textured one keeps its colours. Block tiles are sampled a hundredth of a
pixel inside their edges, so a neighbouring tile never bleeds in.

## Steps 4.3 and 4.5: Billboards and frustum culling

**Billboards.** `canvas.draw_billboard(&sprite, feet, height, &camera)`
stands a sprite in the world, `height` units tall, its feet (bottom
centre) at `feet`, upright and turned to face the camera, as decided.
`draw_billboard_with(.., Billboard { pivot, flip_x, face_camera })` sets
a pivot (a sprite sheet's: `Some(sheet.pivot())`), mirrors it, or turns
it to face the camera fully (tilting back when the camera looks down,
for sparks and coins). It's a textured, glowing quad drawn through
`draw_mesh` with the camera's own turn, so it's depth-tested like any
mesh on the CPU and the GPU alike, its see-through pixels write no depth
(its outline is exact), its pixels stay sharp, and it isn't shaded (it
looks as painted). Each quad is made once per picture and kept on the
canvas, so the GPU uploads it once; its texture is the whole sheet a
frame was cut from, so all of a character's frames share one texture.
`camera.project(point, canvas)` gives the screen point and its depth
along the view, for a health bar or sorting.

**Frustum culling.** Before a mesh's corners are transformed, a ball
around it (around its bounding box: one quick pass) is checked against
the four sides of the view and the near plane; a mesh entirely outside
is skipped. On the GPU each mesh's ball is found once, when it's
uploaded, and checked for every copy. It only skips meshes that would
draw nothing, so the 42 saved pictures are still byte-identical.

**Numbers** (`cargo test --release --example stress3d -- --ignored
--nocapture --test-threads=1`; one test at a time, or they slow each
other down):

| | Before Step 4 | After |
|---|---|---|
| stress3d, 500 objects, CPU | 2.85 ms | 2.89 to 3.56 ms (noisy; 3.0 at 200 frames) |
| stress3d, 2,000 objects, CPU | 7.99 ms | 8.12 to 8.17 ms |
| stress3d, 500 objects, GPU (lavapipe) | 4.34 ms | 5.65 to 5.84 ms |
| stress3d, 2,000 objects, GPU (lavapipe) | 12.06 ms | 12.81 to 12.91 ms |
| block world, 4,096 blocks, textured, CPU | (new) | 4.83 to 4.98 ms |
| block world, the same plain, CPU | (new) | 3.85 to 4.04 ms |

- stress3d keeps every object on screen, so culling can't help it; on
  the CPU it costs nothing measurable.
- **Textures cost about 20%** on the CPU in the block world (the goal
  was "about 20%"). The first version cost 48%: one division per pixel
  instead of two, whole-number colour maths instead of rounding floats,
  and reading the texture's lists directly brought it down.
- **Culling in the block world** (16 chunks of 16 x 16 merged blocks,
  seen from the middle looking along +x): 5.94 ms textured with it,
  6.35 ms without (measured once by switching it off), so about 7%. Less
  than one might hope, because the CPU already skips back faces and
  off-screen triangles cheaply; the saving grows with the world.
- On lavapipe the GPU path is about 6% slower than before: its vertices
  are bigger (texture point and flags), and every copy is checked
  against the view. lavapipe runs on these same CPU cores, so this needs
  checking on a real graphics card (it's on the Windows list).

## Step 4.6: The 3D starter: a three-lane runner

`tools/new_game.sh mygame runner3d` (and `new_game.bat`) starts from
`examples/starter_runner3d/`, as agreed: the closest to Duck Dash 3D, and
a game a bot can play.

- **The game:** the duck runs along +z down a three-lane road; Left and
  Right change lane (it slides there), Space jumps; rows of crates (one
  or two per row, never three) and lines of gold coins come from the
  game's own random numbers; a crate costs one of three lives (then a
  second of blinking); the speed rises from 8 to 14 units a second. The
  camera follows from behind and above.
- **What it shows:** the road and grass are long strips whose texture
  repeats (texture points beyond 1), redrawn under the duck at whole
  repeats so they never end; crates and coins are `Mesh::block`s from
  one Duck Quill atlas, lit by a warm sun with `set_lighting`; the duck
  is a billboard drawn by its sprite sheet's pivot; the HUD is 2D over
  the 3D; Duck Echo's coin, jump and hit sounds and a looping hum; labels
  (the duck and the next crate in each lane, found with
  `camera.project`), three checks, reports, a tune list.
- **The hum** is the `engine_hum` preset made loopable (`art/hum.toml`):
  no fades, and cut from a 3-second render at the steady part, 2 seconds
  long so its notes and wobble come round together. Rendered straight,
  its end was 3,761 steps from its start (a click every 2 seconds); cut,
  4, smaller than an ordinary step (15). Duck Echo has no loop points,
  so the starter's `art/loop_cut.py` does the cut.
- **The bot** (`bot.rs`) plays a minute and records
  `playthroughs/starter_runner3d.json`. Its first version lost all three
  lives, and its failures taught two things worth knowing about the
  game: a jump has to be timed to put the crate under its top (1/3 s
  after taking off, so `speed / 3` units away), and above 10.5 units a
  second one jump is longer than the gap between rows (7 units then),
  so two crates in a row in one lane can't both be jumped. The rows are
  now 8 apart with a top speed of 14, and the bot looks for the best lane
  anywhere, sliding through a neighbour only when it's clear for long
  enough. It now runs the minute losing one life: 734 m and 159 coins.
- **8 play tests** (speeding up and the hum starting, lane changes,
  jumping, three crates ending the game and stopping the hum, jumping a
  crate, coins, every row leaving a way through, the playthrough).
- **CPU and GPU:** the replay's pictures at frames 1,200, 2,400 and the
  last differ in 16 to 22 pixels (0.03%) by more than 3.
- Found while painting it: a see-through pixel in the road tile's top row
  showed the sky through the road as thin blue lines.

The test that makes a game from every starter and verifies it now covers
five starters, in 27 s.

## Step 4.7: The 3D docs

The AI guide gets a 3D section (11): units and directions, the camera
and `project`, meshes and `merge`, textures and atlases, **merging blocks
into chunks** (one mesh per 16 x 16 chunk, not one per block and not one
for the whole world, so frustum culling can still skip the chunks behind
the camera, with the block-world numbers), lighting, billboards, the HUD
over the 3D and labels found with `project` (only what matters: far-off
things overlap on screen in perspective, which the summary would report),
the CPU and the GPU (`DUCKFORGE_RENDERER=gpu` headless, Mesa's driver on
Linux), how much the CPU can draw, and 3D mistakes. The cheat sheet lists
every new function, and `tests/cheat_sheet.rs` calls each one. "Can't do
yet" gains 3D's limits: no shadows, no per-pixel light, no fog, one
texture per mesh, no model files, no animated models. The tutorial
explains textures (and why blending texture points straight across the
screen bends a road), light, billboards and culling for a beginner; the
quick start, skill and README name the runner starter; the README has a
five-step check for a real graphics card. The guide's intro also stopped
saying `new_game` copies the starter's tune file (it hasn't since 2.2).

Checked at the end of Step 4: 425 tests pass (406 headless); all 16
examples pass `verify`; a game made from each of the five starters passes
it too; the 42 saved 3D pictures from before Step 4 are byte for byte the
same on the CPU and the GPU; the headless clean build of `dodge` takes
10.5 s and a one-line change 0.27 s (10.8 s and 0.30 s at Step 0).

## Step 5: Proving it with two games

Both games were made the way the skill says: `tools/new_game.sh`, then
change the copied starter, run, look, test, `verify`. What I read was
`SKILL.md`, `QUICKSTART.md` and the starter's own code (the skill sends
you there). The skill and quick start don't list the tile map,
background, texture or billboard functions at all; a fresh agent would
need section 7 and section 11 of the AI guide.

**(a) Duck Peaks** (`examples/duck_peaks/`, `new_game duck_peaks
platformer`): climb a mountain at sunset. A 100 x 30 tile level, two
screens tall (rock terraces, planks, two water chasms, ten coins, a flag
on the summit), a camera that follows up as well as across, three
painted Duck Quill layers (a sunset sky, far snowy mountains, a pine
forest) moving at their own speeds both ways, new rock and snow tiles,
the duck's sprite sheet, and a 4-second Duck Echo music loop (one layer
per note; every note fades before the next, so it ends in silence and
loops with no click). The starter's bot climbed the new level unchanged
on its first run: 10 coins and the flag in 15.6 s, no falls. 8 play
tests; `verify duck_peaks 200`: OK. `new_game` to a verified game: 6 s.

**(b) Mini Craft** (`examples/mini_craft/`, `new_game mini_craft
runner3d`, then rewritten): a 20 x 20 world written as text, one letter
per column (grass 1 to 3 high, stone pillars, trees, lava, gems), built
from one Duck Quill atlas of 8 tiles into blocks and merged into four
10 x 10 chunks, one mesh each; the lava pool is its own glowing mesh
(merging it into a chunk is exactly what `merge` refuses); six spinning
gems; the duck as a billboard, with a fixed camera behind and above, so
pillars and trees hide it (a play test checks the duck's middle shows
stone when it's behind the pillar). A path-finding bot collects all six
gems in 15.8 s; 7 play tests; `verify mini_craft 200`: OK; the CPU and
GPU pictures differ in at most 6 pixels.

Videos (`--video`): 1920 x 1080, 60 fps, with sound; 15.6 s of Duck
Peaks took 14 s to make, 15.8 s of Mini Craft 10 s.

### What still felt slow or uncomfortable

Honestly, as the agent that made both:

1. **The docs a fresh agent reads first don't cover the new things.**
   The skill's own description still says "a 2D game"; neither it nor
   the quick start names `TileMap`, `Background`, `parallax_xy`,
   `with_texture`, `Mesh::block`, `merge` or `draw_billboard`. I knew
   them; a newcomer would be reading the AI guide or the starter's code.
2. **Background offsets for a level that scrolls up are arithmetic by
   hand.** A layer's offset is in screen pixels at the camera's start, so
   for a climb I had to work out the scroll range (240 down to 0) and
   pick each offset so the sky covers the screen all the way up and the
   forest stands on the ground. An "anchor this layer to world height y"
   option would remove that.
3. **`new_game` copies tests that break with the first new level.** The
   starter's play tests name level coordinates (the plank at columns 8 to
   11, the bricks at 16): five of eight needed new numbers for Duck
   Peaks, and Mini Craft's were all rewritten. It also renames the
   starter's name in prose: "the template" became "the duck_peaks".
4. **The art and sound pipeline is three tools and glue.** Duck Quill
   scripts (JSON) generated by small Python programs, `quill render`
   leaving `.quill` files to delete, `--transparent` needed for layers
   (white paper otherwise), Duck Echo recipes (TOML), and two Python
   helpers of my own because Duck Echo can't make a loop or a file of an
   exact length. Music is one layer per note: 24 layers for 4 seconds.
5. **I can't hear the music or the loops.** I checked what can be
   measured (length, silence at the ends, no jump at the seam, peak),
   not whether it sounds good. The same goes for how the games feel to
   play: the bots and contact sheets show that they work, not that
   they're fun.
6. **Play tests that depend on exact frames are fragile.** Two of my
   tests failed on a one-frame difference (a jump timed a little late, a
   key still held one frame after the lava). A "hold this key until the
   game reports X" helper would make them sturdier; each game defines its
   own `hold()` and `tap()` again.
7. **3D has no block-world movement.** Mini Craft's walking, stepping up
   one block, walls and lava are 40 lines of my own; a 3D counterpart of
   `TileMap::move_and_slide` (a grid of column heights) would cover most
   block games. Labels for 3D things need `camera.project` and a small
   `screen_box` helper, now copied into two games.
8. **Merged chunks keep hidden faces.** The faces between neighbouring
   blocks are still in the chunk meshes (several times the triangles
   needed). Fine for 20 x 20; a real Duck Craft needs them removed.
9. **Fuzzing reaches the start of a level, not the end.** In both games
   200 random runs never reached the goal (Duck Peaks: 3 of 10 coins at
   most; Mini Craft: 3 of 6 gems), so "no problems found" says little
   about the summit. Fuzzing that starts from points along the bot's
   playthrough would cover the rest.
10. **Geometry is where the time went**, not the tools: camera offsets,
    parallax arithmetic, jump timing against row spacing. Building is
    0.3 s and `verify` 2 to 6 s, so the loop itself is quick.

# Phase 3: summary

Phase 3 fixed what four test games (Duck Dash, Duck Tower, Duck Dash 3D,
Duck Craft) ran into, then proved the result with two new games built
from the skill.

- **Step 2, quick fixes:** Duck Quill sprite sheets (`SpriteSheet::load`,
  drawing by the pivot); tune files that can't surprise (not copied by
  `new_game`, named in `Config`, recorded in playthroughs, a warning when
  only the values changed, a `TUNED` line); `--video` (every frame,
  sharp, 1080p, with sound); off-screen label warnings that understand
  long levels; looping sounds and music; `Rect` corners,
  `InputScript::from_json`, Title Case game names.
- **Step 3, 2D:** `TileMap` from text with a `Legend` (solid, one-way,
  decoration, spawn points; errors with row and column), `move_and_slide`
  in half-tile steps, `Tileset`, `Background` with parallax on both axes;
  the platformer starter rebuilt on them, with a bot.
- **Step 4, 3D:** textures (perspective-correct, see-through texels),
  `Lighting` and glowing meshes, billboards, `quad`, `cylinder`, `cone`,
  `block`, `merge` (one atlas) and `bounds`, frustum culling, the
  `runner3d` starter, a 3D section in the guide; lavapipe for CPU/GPU
  comparisons, `DUCKFORGE_RENDERER` in headless runs. Every 3D picture
  from before Phase 3 is byte-identical.
- **Step 5:** Duck Peaks and Mini Craft, above.
- **Bugs found and fixed on the way:** `new_game` titling the shooter "My
  MyShooter"; the window ignoring `Config::tune_file`; `verify mygame 0`
  failing; a see-through pixel in the runner's road; white paper behind
  painted layers.

| | Step 0 (main) | End of Phase 3 |
|---|---|---|
| Headless clean build of `dodge` | 10.8 s | 10.5 s |
| ...after a one-line change | 0.30 s | 0.26 to 0.30 s |
| Headless test suite from an empty target folder | 31.2 s, 345 tests | 38.6 s, 421 tests (4 more example programs) |
| Headless test suite, already built | 2.0 s | 2.6 s |
| All tests, already built | (362 tests) | 3.2 s, 440 tests |
| stress3d, 500 objects, CPU | 3.0 ms per frame | 2.9 ms |
| stress3d, 2,000 objects, CPU | 8.6 ms | 8.1 ms |
| stress3d, 2,000 objects, GPU (lavapipe) | (no GPU here) | 13.4 ms |
| Block world, 4,096 textured blocks, CPU | (no textures) | 4.9 ms (4.0 plain) |
| Examples passing `verify` | 15 | 18 |
| Starters | 4 | 5 |

The suite times for main were measured again for this table, the same
way as now (Step 0's "already built" figure of 11.2 s was taken
differently and can't be compared). The engine's `src/` grew by about
4,700 lines (tile maps, tilesets, backgrounds, sprite sheets, video,
loops, textures, lighting, billboards, culling, and their tests and
comments), with no new dependencies. Still to check on a real graphics
card: the README's five-step list.

---

# Fixing what Duck Craft found

Duck Craft (a bigger Minecraft-style game, `examples/duck_craft/`) kept a
log of everything the engine made hard: `examples/duck_craft/ISSUES.md`.
These are the fixes.

## Fix 1: Pointer lock, and a new window library

First-person mouse look needs the window to hide the mouse pointer and hold
on to it, so the mouse can keep turning you round after the pointer would
have reached the edge of the screen. minifb can't do that, so Duck Craft
turned you while the pointer rested near the window's edge, like a joystick.

**The window moved from minifb to winit** (to open the window, read the
keyboard and mouse, and hold the pointer) **and softbuffer** (to put our
pixels in it). winit is what most Rust games and wgpu's own examples use,
and it reports movement straight from the mouse (`DeviceEvent::MouseMotion`),
which keeps coming when the pointer can't move. softbuffer is the small
library that goes with it for "here is a block of pixels, show it", which
is all minifb did for us. Other choices:

- *Keep minifb and add raw mouse input ourselves*: three versions of
  system code (Windows, X11, macOS), all unsafe, each to test on its own
  computer. winit already is that code, tested by many projects.
- *SDL2*: does everything, but it's a C library to install or build, and
  the engine has avoided anything a beginner can't get with `cargo build`.

Only `src/window.rs` changed: games use our own `Key` and `MouseButton`,
so not one example needed a change for the move. winit reports events ("A
went down"), so `window.rs` keeps the state (what's held, what was tapped
since the last frame, how far the mouse moved) and hands it to the
`Simulation` once per frame, as before. A key tapped and let go between
two frames now still counts as a press (minifb only told us what was held
at that moment). The canvas is stretched to the window by a 20-line
`stretch` (minifb's `AspectRatioStretch` did it before), and the window
waits out the rest of each frame to keep to `target_fps`.

**The game's side** is one call, `ctx.lock_pointer(true)`, and the
movement is read with `ctx.input.mouse_delta()`, in canvas pixels like
everything else (right and down are positive). The lock is game state,
like `quit`: a headless run or a test sees the same `pointer_locked()`, and
a recording holds the movement as `"mouse_motion": [dx, dy]` events
(`Simulation::move_mouse_by` in tests), so a replay turns exactly the same
way. The window takes the pointer only while it's the active window, so
Alt+Tab always gives it back.

How the pointer is held depends on the system: winit's `Locked` (the
pointer stays put) on macOS; `Confined` (it can't leave the window) on
Windows, where the window also puts it back in the middle every frame. On
Linux (X11) it's only hidden and put back in the middle every frame: an X11
grab makes the X server report each mouse movement **twice** (seen with
xdotool on Xvfb: 10 pixels of movement arrived as two 10s). A very fast
flick could leave the window between two frames there, but the movement
still counts, and the next frame brings it back.

**Costs.** A first build with the window (the default) took 61 s with
minifb and takes 67 s now, on 4 cores. winit's Wayland support would add
another 17 s (84 s), so it's left out: Wayland desktops run X11 windows
through XWayland. Headless builds don't build the window at all, so they
are unchanged (11 s). On Linux, winit needs `libxkbcommon-x11` (part of
every desktop; on a bare server the window says to install it, instead of
crashing, and `--headless` still works). `EventLoop` can only be made once
per program, so `duckforge::run` can open one window per program (no game
did more).

Tested here under Xvfb with xdotool: the window opens, shows the canvas
stretched with black bars, replays a playthrough at 60 frames per second,
and Duck Craft turns with the mouse, frees the pointer on Escape and takes
it back on a click. Not tested here: a real Windows or macOS desktop.

## Fix 2: `merge` keeps texture points

`Mesh::merge` copied a part's texture points only if the part already had
its texture, so `head.merge(&beak, ..)` then `.with_texture(skin)` left the
beak plain white, with no warning. Now the points always come along. The
one case that still drops them is on purpose: a plain part merged into a
mesh that already has a texture stays plain, as it was drawn before
(otherwise a red cube merged into a chunk would suddenly be painted with
the atlas). Both cases are in `merge`'s docs.

## Fix 3: `BlockWorld`, and where atlas tiles are

The guide said to `merge` whole blocks into chunks. That keeps every face
between two blocks (hidden, but drawn), which is fine for a few thousand
blocks and far too many triangles for a world you can dig. Duck Craft cut
`Mesh::block`'s cube into faces itself and copied only the visible ones,
and needed `Sprite::in_sheet`, which was private.

`src/blocks.rs` now does that for every block game, in two parts:

- **`BlockPalette`**: how each kind looks (three tiles from one atlas,
  see-through or not, faces between two of its own kind or not, glowing
  or not). Blocks are numbered from 1 in the order given (`AIR` is 0), so
  a game's own `enum Block` can be `block as BlockId`.
- **`BlockWorld`**: the grid (one byte a cell) and a mesh per chunk. `set`
  marks the block's chunk, and the next chunk when the block is on its
  edge; `update_meshes` rebuilds just those. `draw` draws them, each chunk
  skipped whole when it's out of view.

They're apart so a world can be made and tested with no pictures loaded
(Duck Craft's world tests don't load art). Chunks are columns the world's
full height, like Duck Craft's; `with_chunk_size` changes the width. A
face shows next to air or a see-through block, except between two of the
same see-through kind (glass, water), unless the kind has `inner_faces`
(leaves). `update_meshes_shaded` takes a brightness for each face from
the cell it looks into, which is how Duck Craft keeps its darker caves.
Glowing blocks go in a second mesh per chunk, because a mesh glows or it
doesn't (fix 9 changes that).

Atlas positions are public now: `Sprite::whole_picture()`,
`Sprite::place_in_picture()` (`[left, top, width, height]` in pixels) and
`Tileset::uv_rect(name)` (the same as texture points).

Duck Craft's `World` keeps its blocks in a `BlockWorld`; its own
`faces_of`, `face_shows`, `build_chunk` and `rebuild_around` (about 120
lines) are gone, and the playthrough's last picture is the same to the
pixel. Not done: a block raycast (Duck Craft keeps its own), and a light
model in the engine (a closure is simpler and lets each game choose).

## Fix 4: The fuzzer presses the keys the game reads

`--fuzz` pressed the arrows, Space, Z and X unless told otherwise, so
Duck Craft's 50 runs stood still (`x always 32.5`): it walks with W A S D.
The engine already noted every key a game asks about (`keys_read` in
`log.json`, used to tell a freeze from presses the game ignores), so the
fuzzer now learns from it. Before the runs, a practice run presses the old
default keys plus Enter; each next practice run presses the keys found so
far, until no new key turns up (at most 4, each at most 15 seconds). A
game that reads Enter on its title screen and W A S D once playing is
found in two. Escape is never learned (it usually quits, ending a run
early), and nor are the engine's F keys. A game that reads no key gets the
old defaults. The keys are sorted in `Key::ALL`'s order, so the runs are
the same every time; the summary line says they were learned.

The cost is the practice runs, a few seconds before the real ones (they
run one after another, since each depends on the last). Duck Craft's
coverage went from standing still to `x 11 to 43`, every block in the
hotbar held, digging and building.

## Fix 5: `mouse_position()` gives an `Option`

`mouse_position()` gave (0, 0) until the mouse had been over the window,
which is also the top-left corner, so a game couldn't tell them apart.
It now gives `Option<Vec2>`: `None` until the pointer has been over the
window (or a script moved it), then `Some` from then on, even when the
pointer is outside the game area. This changes the public API, so every
caller changed with it: `hello` (`if let Some(mouse)`), `physics` (drops
things only where there's a mouse inside the walls), the debug overlay
(`MOUSE -` with no mouse), the cheat sheet, the guide and the tutorial.
A separate `mouse_known()` next to the old function was the other choice;
it keeps old code compiling, but nothing makes a game check it, and the
whole point was that (0, 0) was easy to use by mistake.

## Fix 6: One atlas tile repeating over a big surface

Duck Craft copied the water and cloud tiles pixel by pixel into pictures
of their own (`tile_picture`), because its sea, textured with the atlas,
repeated the whole atlas. It turned out the engine could already do it: a
sprite *cut from* a picture (a tile, a frame) repeats on its own as a
texture, on the CPU (`wrap` stays inside the sprite's rectangle) and the
GPU (each sprite gets its own texture, set to repeat). Only the whole
atlas picture, which `Mesh::block` uses so blocks can share one texture,
repeats as a whole. Nothing said so, and getting the tile took
`atlas.frame(name, 0.0).clone()`. So the fix is a name and the docs:
`Tileset::tile(name)` gives the tile's picture, sharing the atlas's
pixels, and says it repeats on its own, with an example. Tests draw a
floor with one tile repeated six times on the CPU and on the GPU
(lavapipe here): the tile next to it in the atlas never shows, and the
two pictures match. Duck Craft's sea, clouds and hotbar icons use it now,
with the same picture to the pixel.

## Fix 7: Meshes in view space, for hands and held things

Duck Craft placed its first-person arm in the world every frame from the
camera's position, yaw and pitch, and the arm sank into walls when you
stood against one. `canvas.draw_mesh_in_view(&mesh, &transform, &camera)`
places a mesh relative to the camera instead (x right, y up, z ahead).

It draws the mesh with a camera at the origin looking along +z (so view
space *is* that camera's world), with the same field of view, and with
the sun turned into view space for that draw, so the hand is lit by the
world's sun and the light moves across it as you turn. It takes the
camera for those two reasons; the request said `(mesh, transform)`, but
without the camera the hand would be lit as if you always faced the same
way. The first view-space draw after any world draw empties the depth
buffer (on the GPU, the batch so far is drawn first and the next one
starts empty), so view meshes are always in front of the world but still
hide each other properly (the block in the hand). The catch, in its docs:
draw them after the world. Building one `Transform` for view-to-world
instead was the other way, but a `Transform`'s three angles can't hold a
camera turn followed by a mesh turn in general.

Tests: the hand stays on the same pixels however the camera turns, shows
through a wall that is nearer than it, and is bright with the sun behind
the camera and dark when turned to face the sun; the GPU draws the same
picture. The 42 baseline 3D pictures are unchanged (`draw_mesh` itself
only resets a flag).

## Fix 8: Tint and see-through, per draw

A mesh's colours were fixed when it was built, and nothing in 3D could
be see-through except whole texels of a texture. Duck Craft wanted a
shadow under the duck and a glassy selection box (it drew 12 lines).
`canvas.draw_mesh_tinted(&mesh, &transform, &camera, tint)` multiplies
the mesh's colours by `tint` for that one draw, and `tint`'s alpha is how
solid it is. One colour does both, the same way `Blit` tints sprites.

See-through drawing follows the usual rule: it's hidden by nearer things
already drawn, but doesn't write the depth buffer, so it hides nothing
drawn after it. "Draw see-through meshes last" is in the docs. Sorting
them automatically would need every see-through draw kept until the end
of the frame, which the CPU renderer (it draws at once) can't do without
a big change.

- **CPU:** the two triangle fillers take an opacity; at 255 they run the
  same code as before, so the 42 baseline pictures are unchanged. Below
  255 a pixel is mixed with `color::blend`. The triangle filler covers
  the pixels on an edge that two triangles share twice, which is
  invisible for solid triangles but showed as a darker diagonal line
  through see-through squares. Each see-through mesh now marks the pixels
  it painted, so each is mixed once. Changing the filler's edge rule
  would have moved pixels in every existing picture.
- **GPU:** each object carries its tint next to its matrix, the shader
  multiplies by it, and a second pipeline (alpha blending, no depth
  write) draws the see-through objects after the solid ones in the same
  batch, one at a time, in drawing order. A solid object drawn after
  see-through ones first draws them, so the GPU follows the drawing order
  like the CPU.

Tests: exact colours on the CPU (half blue over red is `(127, 0, 128)`),
an invisible tint changes nothing, a white tint gives exactly
`draw_mesh`'s picture, a see-through cube hides nothing drawn after it,
and the GPU matches the CPU. Duck Craft draws a soft shadow under the duck
(smaller and fainter as it jumps) and a faint glassy box over the block
you look at; its CPU and GPU pictures differ in 0.18% of pixels. The
CPU benchmarks (`stress3d`'s `frame_time`, two runs each) were 5.7 to 6.4
ms for the textured block world before and 6.2 to 6.9 ms after, about the
spread between runs.

## Fix 9: Glow per triangle, and distance fog

**Glow.** A mesh glowed or it didn't, so Duck Craft kept a second mesh per
chunk for its lamps (and `BlockWorld` did the same in fix 3), and a lamp
couldn't light anything round it. Meshes now have a glow level per
triangle (`mesh.glow`, set with `set_glow`; `glow_of` reads it, and says 1
for a `.glowing()` mesh): 0 is lit as before, 1 ignores the light
exactly as `.glowing()` does, and in between keeps the triangle at least
that bright, per colour channel (`max(light, glow)`). "At least" rather
than "plus" means a lamp's light can't make a sunny face brighter than
white, and does nothing in daylight; it shows at night.

- `merge` no longer refuses to mix glowing and plain meshes (it said
  "draw them as two meshes"): glowing parts keep glowing, triangle by
  triangle.
- The GPU carries the level in each corner's colour alpha, which was
  unused (always 255). A full glow still sets the `GLOWING` flag, so it
  takes exactly the old path.
- `BlockWorld` is back to one mesh per chunk: `BlockKind::glowing` blocks
  glow by their triangles, and the shade closure may return
  `FaceLight { shade, glow }` instead of a number (a plain `f32` still
  works, through `From<f32>`). `chunk_glow_mesh` is gone (it never left
  this branch). Duck Craft keeps a list of its lamps and gives faces
  within 4.5 blocks a glow of up to 0.75; placing or digging a lamp marks
  the chunks it reaches.

**Fog.** `canvas.set_fog(Some(Fog { color, near, far }))`, apart from
`Lighting` so the struct literals games already write keep compiling.
The amount is by distance straight ahead of the camera (the same depth
the CPU's depth buffer holds as `1 / z`; the GPU works it out from the
camera's direction, now sent with the other per-batch settings), from
none at `near` to all at `far`, mixed with `color::blend` on the CPU and
`mix` on the GPU. It applies to every 3D pixel, glowing or not, and before
see-through mixing. Duck Craft uses the horizon's colour from 40 to 120
blocks, so the sea fades out instead of ending at a hard edge.

The 42 baseline pictures are unchanged (no glow and no fog run the old
code). Tests: glow levels 0, 0.6 and 1 on the CPU (exact values), merging
glowing and plain parts both ways, fog amounts and exact fogged colours,
a GPU picture with glow levels and fog matching the CPU's, `BlockWorld`
with a lamp and a half-lit face, and Duck Craft's lamp lighting the floor
round it (and going out when dug).

## Fix 10: The docs Duck Craft wished for

- **Which way rotations turn**, in the guide's section 11: `rotate_y`
  turns forward towards right, `rotate_x` forward towards up (so a point
  below swings forward), `rotate_z` right towards up, and the camera's
  yaw and pitch agree. A test reads those sentences from the guide and
  checks each against the maths, so the words can't drift from the code.
  (Duck Craft's log said `vec3(0, -1, 0).rotate_x(a)` goes to `-z`; it goes
  to `+z`. The log now says so too.)
- **`Canvas::save_png`** is in the skill, the cheat sheet (and its test)
  and the guide's play-test section, as `sim.picture().save_png(..)`.
- **`cargo test` doesn't rebuild the game's own program**: running
  `target/.../examples/x` by hand after `cargo test` runs old code. The
  skill and the guide say to use `tools/verify.sh` or `cargo run`, and a
  skill test keeps the warning there. The skill had to stay within 150
  lines, so its 3D and mistakes sections were tightened to fit.
- The tutorial's 3D chapter has a short section on what a block game
  needed (visible faces only, hands in view space, see-through, glow and
  fog, mouse look), and the quick start lists the new features.

## More fixes Duck Craft asked for

**verify replays `<game>_*.json` too.** A `duck_craft_night.json` was
never replayed, because verify only looked for `<game>.json` and
`<game>-*.json`, and it went out of date without anyone noticing. Now
`<game>_*.json` is replayed as well. That would also catch another
game's files (`mini_craft.json` for a game called `mini`), so a file whose
name, up to its first `-`, is another example's name is left to that
example. `tools/verify.sh --list-playthroughs <game>` (and the same in
`verify.bat`) only lists what would be replayed; a test runs it in a small
pretend project. "Every file in the playthroughs folder" wasn't possible:
in this repository the folder holds every game's playthroughs.

**Replays use the tune values they were recorded with.** A recording
stores the `ctx.tune` values it was made with, but a replay only warned
when they differed, so a playthrough recorded with
`--set start_time_of_day=0.47` replayed wrong without the same `--set`.
`Simulation::follow` now hands the recording's values to the tunes, and
they come between the command line and the game's own tune file: `--set`,
then `--tune FILE`, then the recording, then the game's tune file, then
the code. A recorded value only counts as the recording's when it changes
something (most recordings store the values everyone uses anyway), so the
summary's TUNED line names just those: `time = 0.47 (from the recording;
code says 0)`, and `log.json` says `"from": "recording"`. The catch: to
try a new value against an old playthrough, use `--set` or `--tune FILE`;
editing the game's tune file no longer changes a replay that recorded
that value. This fixes issue 31.

**`Simulation::with_context`.** A test could only step the whole game,
so a game method taking `&mut Context` (a trick that rolls `ctx.rng` and
plays a sound) couldn't be called on its own. `sim.with_context(|game,
ctx| game.trick(ctx))` runs it between frames with the simulation's own
context, then records what it reported, played and did with its loops on
the frame just played, the same way a step does (that part of `step_by`
is now one function both use). Reports from the same frame are merged
into one entry, so `log.json` doesn't list frame 5 twice. Before the
first step it's frame 0, after the game's `init`.

**`BlockWorld` skips chunks lost in fog.** `draw` handed every chunk to
`draw_mesh`, which skips chunks outside the view but not ones so far
away they'd be nothing but fog: a world 192 blocks across is 144 chunks,
mostly sea, all drawn every frame. Now a chunk is skipped when its
nearest point is past the fog's `far` edge, measured the way the fog
measures, straight ahead along the camera's view (the box's corner
furthest back along that direction, so a camera looking straight down
skips nothing below it: everything there is near). Each chunk's box is
worked out when it's built. `draw_within(canvas, camera, far)` does the
same with any distance, fog or not, and both say how many chunks they
drew. The picture changes in one way, in the docs: where a skipped chunk
would have been, the background shows instead of the fog's colour, so
games clear to (or fog with) the sky's colour. A test draws the 144-chunk
world level and looking straight down, and checks every pixel is the
same as drawing everything, except fog-coloured ones that became
background.

---

# Phase 4, Step 0: main brought up to date, and the baseline

Before Duck Forge Studio, `main` (still at the end of Phase 3) needed
everything since: `claude/duck-craft-audio` (all the engine fixes: the ten
Duck Craft fixes, stereo sound, the fog culling, `Simulation::with_context`,
the verify and replay fixes, the smoother sound in a window) and
`claude/duck-craft-2` (the newest Duck Craft). The two were merged into one
branch from `main`. Nothing clashed: one branch only touched the engine,
the other only Duck Craft, and nothing needed fixing afterwards.

Checked after the merge: it builds with and without the default features;
every test passes both ways (529 headless, 562 with everything); clippy is
clean both ways; `cargo check --target x86_64-pc-windows-gnu --all-targets`
passes; and all 20 examples pass `tools/verify.sh` (50 fuzz runs each).

One thing was added: Duck Craft had no way to time a frame, so
`examples/duck_craft/tests.rs` has an ignored `frame_time` test like
stress3d's. It replays `playthroughs/duck_craft.json`, drawing every frame
with each renderer, and prints the usual frame and the slowest
(`cargo test --release --example duck_craft -- --ignored frame_time
--nocapture`).

**The baseline.** Phase 4 must not make these noticeably worse. They were
measured on the 4-core cloud machine the work is done on, which is slower
and less steady than the one behind the Phase 3 table (so don't compare
the two tables), and whose only "graphics card" is llvmpipe, a pretend
one drawn by the CPU. Frame times were measured three times; the table
gives the range.

| | Baseline |
|---|---|
| Headless clean build of `dodge` | 13.1 s |
| ...after touching `src/lib.rs` | 0.8 s |
| Headless test suite from an empty target folder | 74.3 s, 529 tests |
| Headless test suite, already built | 21.0 s (Duck Craft's 72 tests are 17.8 s of it) |
| All tests, already built | 18.0 s, 562 tests |
| stress3d, 500 objects, CPU, 320 x 240 | 2.5 to 3.5 ms per frame |
| stress3d, 2,000 objects, CPU | 7.9 to 9.2 ms |
| stress3d, 2,000 objects, GPU (llvmpipe) | 14.5 to 19.1 ms |
| Block world, 4,096 textured blocks, CPU | 5.2 to 5.7 ms |
| Duck Craft playthrough, CPU, 320 x 240 | 8.0 to 9.0 ms per frame (middle 7.6 to 8.5, slowest 15 to 21) |
| Duck Craft playthrough, GPU (llvmpipe) | 13.6 to 14.8 ms (slowest 30 to 85) |
| Release game, Linux: `dodge` | 6.1 MB (4.3 MB stripped) |
| Release game, Linux: `duck_peaks` | 6.2 MB (4.4 MB stripped) |
| Release game, Linux: `starter_runner3d` | 11.5 MB (8.5 MB stripped) |
| Release game, Linux: `duck_craft` | 11.8 MB (8.8 MB stripped), plus 6.0 MB of assets |
| Release game, headless only: `dodge`, `duck_craft` | 2.2 MB, 2.7 MB |
| Examples passing `verify` | 20 |

How each was measured: the build and suite times with `cargo clean`
first, then `cargo build --no-default-features --example dodge` and
`cargo test --no-default-features`; the frame times with the ignored
tests in `stress3d` (run with `--test-threads=1`, so the two tests don't
slow each other) and `duck_craft`; the sizes from
`cargo build --release --example <game>` and `strip`. The 3D games are
bigger because they carry the graphics-card renderer. Windows `.exe`
sizes aren't here: this machine can check a Windows build but can't link
one, so Step 2's release workflow will be the first to give them.

---

# Phase 4, Step 2: the app, Home, and the game inside the Studio

`docs/phase4-design.md` is the design, with the review's answers folded
in. This is what was built, what was decided on the way, and the numbers.

## What was built

- **The engine's side** (`src/studio_link`, behind the new `studio`
  feature): a game started with `--studio ADDRESS` opens no window. It
  connects to the Studio and plays one "tick" at a time, sending each
  picture back. It can also run fast without drawing, follow a script,
  hand over its recording, and open its own window on the frame it's on.
- **Three fixes from Duck Craft's list:** `Game::quit` (issue 17),
  `ctx.is_headless()` and `ctx.in_studio()` (16), and a second `--` for a
  game's own options (18). Duck Craft uses all three.
- **`--clip FILE`**, the end of a headless run as a video or a GIF.
- **`studio/crates/core`:** everything the Studio does, with no window.
- **`studio/crates/app`:** the window: Home, New game, the game screen
  (Build | Play, Check, Clip, Pop out, Timing, Console).
- **The release workflow** (`.github/workflows/studio-release.yml`) with
  a "Make a release" button, and the Duck Forge icon (`studio/brand`).

## Decisions made on the way

**The Studio is a second cargo workspace, not part of the engine's.**
`studio/` has its own `Cargo.toml`, lock file and target folder, and the
engine's `Cargo.toml` doesn't know it exists. So `cargo build` and
`cargo test` at the top do exactly what they did, egui never reaches a
game's dependency list, and the test that pins the headless build's
crates (`tests/dependencies.rs`) is untouched. The Studio's core uses the
engine as a library (headless, with `studio`), so it reads input scripts
and link messages with the engine's own code.

**The `studio` feature adds no libraries.** The link is `std`'s TCP on
`127.0.0.1` and the JSON the engine already had. Without the feature, a
game given `--studio` says so and stops; nothing of the link is in an
ordinary, headless or exported build.

**New game and Check run the tool scripts.** The Studio doesn't have its
own copy of what `tools/new_game` and `tools/verify` do: it runs them
(the `.bat` on Windows) and reads verify's short report into sentences.
That keeps one implementation, and it's what the Console shows: the very
command a person or an agent would type. The price is that Check is only
as detailed as verify's report.

**One command layer.** Every action is a `Command` in
`studio/crates/core/src/studio.rs`, named like `game.toggle`, with one
table listing them all. Buttons, Tab, tests, `--run` scripts and (later)
an agent's tools all call `studio.run(command)`. A test reads every
command from JSON and checks the table matches.

**The games folder is one type.** `folder.rs` is the only place that
knows games are `examples/<game>` with playthroughs, tune files and
`out/` beside them, and the only place that builds a command line. Which
folder it is, is a setting.

**The Studio keeps the time, and never bursts.** Each of the Studio's own
frames sends at most one tick. How many updates it asks for follows the
same rule as the window's `Pacer`, with two limits of its own: a gap over
0.1 s between two Studio frames (its window being dragged or resized, on
Windows) is skipped and the game carries on with one update; and one tick
never asks for more than 3 updates. If the game hasn't answered the last
tick, no new one is sent; the next one asks for the updates owed (again,
never more than 3), so a game that draws slowly still moves at its own
speed, as in its own window. The game's side holds its sound when no tick
has come for 0.1 s (it fades out over one block and waits), so a frozen
or dragged Studio is quiet.

**Zero or one frame of added delay.** After sending the tick, the Studio
lays out its panels and then waits up to 10 ms for the picture before
painting the game. If it comes (the game's frame time plus the link), it's
shown in that same Studio frame, as in the game's own window. If not, the
last picture stays one more frame. The Timing line counts those as "late".

**Two recordings, one truth.** The Studio writes down every input it
sends, with its frame, so a session survives the game crashing. While the
game is alive, the Studio asks it for its own recording instead (that one
also has the reported values at the end, so a replay that ends
differently is noticed). A test checks the two hold the same events.

**A press and its release never cancel.** Both arriving between two
updates would cancel in the engine, and the game would never see the key.
The Studio holds the release back one tick, so the game sees a one-frame
tap, as in its own window.

**The Studio holds the pointer, not the game.** The game only says it
wants it (`ctx.lock_pointer`). The Studio takes it when you click in the
game, and lets go on Tab, when its window isn't the active one, and when
the game stops asking or stops. Mouse movement is divided by the scale
the game is shown at, the same numbers `window.rs` gives.

**A new game gets its first playthrough and its first Check by
themselves.** The first screenshot run showed why: a fresh platformer's
Check said "Some of its play tests fail", because one test replays a
playthrough the bot hadn't recorded yet. `tools/new_game` does both when
run by hand; the Studio skipped them to open the game sooner. Now it opens
the game, builds it, and then has the bot record and Check run in the
background. The card says "Checks pass" a few seconds later.

**A crash is explained from what the game printed.** Rust's own
"panicked at file:line" and the line after it become "Its code hit a
problem on frame 412: "..." in main.rs, line 210". Without one, a line
starting "Error:" is quoted. Otherwise the Studio says the game didn't
say why (an early version quoted the last line printed, which was about
the sound card).

**A GIF clip shows 50 pictures a second, not 60.** GIF timing is in
hundredths of a second and viewers slow anything under 2 down, so the
fastest a GIF plays everywhere is 50 a second. A 60-a-second game's clip
keeps five frames of every six, at its real speed. The MP4 keeps them all.

## How every screen is screenshotted with no window

`studio/crates/app/tests/screens.rs` uses `egui_kittest`: it runs the
app's own drawing code against a pretend 1280 x 800 screen at 100%
scaling and renders the result to a picture with wgpu, which needs no
display (here it draws with llvmpipe, a graphics card pretended by the
CPU). The Studio underneath is the real one. The run makes a real new
game from the platformer starter, builds it, plays it through the link
(pressing its keys), presses Tab, runs a real Check, makes a real clip,
types a command into the Console, ends the game from outside, and plays
Duck Craft. Three screens can't be reached honestly on a working
computer, so their state is put in place by hand: a build that failed
(the message is a real compiler error's words), a build in progress, and
no Rust. Pop out isn't in the pictures: it opens a real window.

```sh
cd studio
STUDIO_SCREENSHOTS=../docs/studio-screens cargo test -p studio_app --test screens -- --nocapture
```

It takes about 40 seconds once everything is built, saves the
pictures in `docs/studio-screens/`, and puts the repository back as it
was (the game it made is deleted, `Cargo.toml` restored). Without
`STUDIO_SCREENSHOTS` only a quick test runs, which checks Home and the
New game dialog by their words, with no rendering.

## The numbers

**How the frame rate and the delay were measured.**
`DuckForgeStudio --timing <game> 20` plays a game through the link with
no window. It runs the same code the app runs on each of its frames
(`Studio::begin_frame`, then `Studio::wait_for_picture`), sixty times a
second, and prints the app's own Timing line every five seconds (after one
second to settle). The same line is in the app, under the Timing button,
so the numbers can be read on any PC. In it:

- **updates a second** is how fast the game's world moves (60 is right);
- **pictures a second** is how many new pictures arrived;
- **longest gap** is the longest wait between two pictures;
- **game** is the game's own time for a tick, updating and drawing, as it
  says in each picture's header;
- **link** is the rest of the round trip: sending the tick, and the
  picture coming back (320 x 240 pixels, 307 KB, every frame);
- **late** counts the pictures that weren't there within 10 ms, and so
  were shown one Studio frame later.

So the **added delay** is 0 frames for every picture that isn't late, and
1 frame (16.7 ms) for one that is. It's never 2: a tick isn't sent until
the last one is answered.

What this doesn't measure is the Studio painting the picture on a real
screen (copying 320 x 240 pixels to the graphics card and one textured
rectangle), and the monitor. With no window, the measuring loop's own
frames come a little slower than 60 a second (it sleeps between them, and
sleeps run long), which is why "pictures a second" reads 58 or 59 while
the game still gets its 60 updates: about one tick in forty carries two.
In the app the frames come from the screen itself.

On the 4-core cloud machine (Linux, no real graphics card; games built
the way the Studio builds them, the Studio in release):

| Game | Drawn by | Updates a second | Pictures a second | Game | Link | Late |
|---|---|---|---|---|---|---|
| Duck Peaks | the CPU | 59.9 to 60.0 | 59.2 to 59.4 | 0.39 ms | 0.53 to 0.61 ms | 0 or 1 of 296 |
| 3D runner | llvmpipe (a pretend graphics card) | 59.9 to 60.0 | 59.0 to 59.4 | 1.6 ms | 0.47 to 0.52 ms | 0 or 1 of 296 |
| Duck Craft | the CPU | 59.9 to 60.0 | 55.7 to 59.3 | 10.7 to 13.3 ms | 0.47 to 0.52 ms | 208 to 277 of about 290 |
| Duck Craft | llvmpipe | 59.8 to 60.0 | 28.6 to 29.4 | 23.6 to 25.6 ms | 0.49 to 0.55 ms | all |

On GitHub's Windows machine (the release workflow prints these on every
run; it has no graphics card either, so "Microsoft Basic Render Driver"
is Windows' pretend one):

| Game | Drawn by | Updates a second | Pictures a second | Game | Link | Late |
|---|---|---|---|---|---|---|
| Duck Peaks | the CPU | 59.9 to 60.1 | 58.3 to 58.5 | 0.36 ms | 0.58 to 0.64 ms | 2 of 292 |
| 3D runner | Microsoft Basic Render Driver | 60.0 to 60.1 | 58.1 to 58.7 | 2.8 to 3.0 ms | 0.61 to 0.75 ms | 0 to 2 of 290 |
| Duck Craft | the CPU | 59.9 to 60.2 | 58.0 to 58.3 | 12.6 to 13.8 ms | 0.61 ms | 61 to 83 of 290 |
| Duck Craft | Microsoft Basic Render Driver | 59.9 to 60.0 | 27.6 to 27.8 | 26.7 to 28.5 ms | 0.55 to 0.78 ms | all |

Read together: the link costs about half a millisecond a frame. Duck
Peaks and the 3D runner play at a steady 60 with no added delay. Duck
Craft's world always moves at 60; drawn by the CPU its pictures keep up
but most arrive a frame late, because on these machines the game alone
takes 11 to 14 ms and the Studio only waits 10; drawn by a pretend
graphics card it shows 28 pictures a second, which is that card's speed
(the game takes 25 ms a frame with or without the Studio). Neither machine
says what an RTX 3090 does: that's the first thing to read off the Timing
line on a real PC (the checklist's item 4).

A prototype settled how the picture travels: a local TCP connection moved
a 320 x 240 picture in 0.15 ms, a pair of pipes in 0.30 ms, so TCP it is.
The first version of the link took 1.0 ms a frame; sending the canvas
straight from the game's memory instead of copying it first halved that.

**Before and after.** Same machine and same commands as Step 0's
baseline:

| | Step 0 | After Step 2 |
|---|---|---|
| Headless clean build of `dodge` | 13.1 s | 13.0 s |
| ...after touching `src/lib.rs` | 0.8 s | 0.7 s |
| Headless test suite from an empty target folder | 74.3 s, 529 tests | 65.5 s, 533 tests |
| Headless test suite, already built | 21.0 s | 20.7 s |
| All tests, already built | 18.0 s, 562 tests | 18.4 s, 567 tests |
| stress3d, 500 objects, CPU | 2.5 to 3.5 ms | 2.5 to 2.9 ms |
| stress3d, 2,000 objects, CPU | 7.9 to 9.2 ms | 7.1 to 7.5 ms |
| stress3d, 2,000 objects, GPU (llvmpipe) | 14.5 to 19.1 ms | 15.3 to 17.1 ms |
| Block world, 4,096 textured blocks, CPU | 5.2 to 5.7 ms | 5.0 to 5.1 ms |
| Duck Craft playthrough, CPU | 8.0 to 9.0 ms (slowest 15 to 21) | 8.5 to 8.7 ms (slowest 19 to 20) |
| Duck Craft playthrough, GPU (llvmpipe) | 13.6 to 14.8 ms | 12.4 to 12.7 ms |
| Release game: `dodge` | 6,078,912 bytes (4,319,328 stripped) | 6,069,304 (4,317,488) |
| Release game: `duck_peaks` | 6,210,264 (4,422,496) | 6,203,944 (4,422,736) |
| Release game: `starter_runner3d` | 11,450,680 (8,534,040) | 11,436,704 (8,531,976) |
| Release game: `duck_craft` | 11,787,376 (8,831,592) | 11,780,264 (8,831,280) |
| Release game, headless only: `dodge`, `duck_craft` | 2,230,536 and 2,726,336 | 2,247,864 and 2,743,728 |
| Examples passing `verify` | 20 | 20 |

Nothing got noticeably worse. The headless programs are 17 KB bigger:
that's `--clip`. The windowed ones were 180 KB bigger for a while: the
pop-out work had split the window's event pump over two functions, and
winit's event code is built once for every place that asks, so it was in
the program twice. One function now does the asking (`pump` in
`src/window.rs`) and the sizes are back.

New numbers, for next time:

| | |
|---|---|
| A release game built with the link (`--features studio`): `dodge`, `duck_craft` | 6,186,328 and 11,900,832 bytes (about 117 KB more than without) |
| `DuckForgeStudio.exe` (Windows, release) | 16,284,672 bytes |
| The download, `DuckForgeStudio-0.1.0-windows.zip`, with Duck Forge in it | 13,127,410 bytes |
| The Studio's tests | 38, and the engine has 8 more with `--features studio` (575 in all) |
| A new game from nothing to "Checks pass", from a freshly unzipped folder | 85 s on the cloud machine, 2 min 10 s on GitHub's Windows machine |

## What Windows has and hasn't shown

The release workflow runs on one of GitHub's Windows machines. There,
with no screen: the Studio's tests pass (a real game is built, played
through the link, frozen, stepped, resumed, and its saved session replays
to the same picture, pixel for pixel); `DuckForgeStudio.exe` builds and
starts; the three games play through the link at the speeds in the table
above; the zip has what it should; and from the unzipped folder (whose
name has spaces, and where nothing was built beforehand) a new platformer
is made, built, played, given its first playthrough, checked ("Checks
pass") and clipped. The clip is a GIF there, as that machine has no
ffmpeg; here it's an MP4 with sound.

What that machine can't show, and nothing else has yet:

- **Anything on a real screen.** The Studio's window has never been
  opened on Windows. The pictures in `docs/studio-screens` are drawn on
  Linux with no window.
- **The pointer hold, the window being dragged or resized, sound, Pop
  out, and sharp pixels at 125% and 150%.** Their logic is tested (the
  clock, the scaling arithmetic, the input mapping, the sound's fade),
  but not the real thing. Pop out has only been tested through the link's
  messages, never with a window on a screen.
- **A real graphics card.** Both machines draw 3D with a pretend one.

The checklist at the end of `docs/phase4-design.md` (17 short items)
covers all of it.

## Known gaps after Step 2

- **Build mode has no editor yet.** Frozen, it offers "Play one frame",
  "Play ten frames" and Restart. Painting the level is Step 3.
- **F1, F3 and F12** (the debug overlay, the FPS counter, screenshots)
  aren't handled inside the Studio yet; they reach the game as plain keys.
- **While popped out**, the Studio's own recording doesn't see the inputs
  (the game's own window takes them). The game's recording has them, so a
  session is whole as long as the game doesn't crash while popped out.
- **QUICKSTART.md and the skill** are at their line limits (125 and 150)
  and say nothing about the Studio yet. They get their lines at the end
  of the phase, when Step 3's level files need room there too. The
  README and `docs/AI_GUIDE.md` (section 13) do cover it.
- **The icon** is a plain flat duck and anvil, simpler than Duck Quill's.
- **The "Make a release" button** only appears in the Actions tab once
  the workflow file is on `main`. Until then, every push that touches the
  workflow keeps the zip on its run's page.

---

# Phase 4, Steps 3 and 4: Build mode, levels as files, Feel, History

`docs/phase4-design.md` sections 4 to 9 are the design. This is what was
built, where it differs from the design and why, and the numbers.

## The two fixes from the Step 2 review

- **Home's cards showed the debug overlay's boxes.** Check replays
  playthroughs with `--debug`, and the card used that run's `last.png`.
  A headless run that draws the overlay or the grid on `last.png` now also
  saves `last.plain.png` (the same frame as the game drew it), and the
  cards use it. No existing picture changed; it's one more file.
- **F1, F3 and F12 inside the Studio** do what they do in a game's own
  window: F1 has the game draw its debug overlay (over the link; a frozen
  game draws its frame again, so it shows at once), F3 shows the Timing
  line (the Studio's frame counter), F12 saves the game's picture at its
  own size in `out/screenshots/`. As in the window, the keys still reach
  the game too, so recordings are the same either way.

## Levels as files

A level is `level.txt` (the letters, exactly as the `LEVEL` text was) and
`level.legend.json` (what each letter means, the tile size, the tiles'
sprite sheet, labels, and rules). `TileMap::load_level` reads both.
`template`, `duck_peaks` and `starter_topdown` were converted; the top-down
starter moved from its own wall grid onto `TileMap`. Each game's
playthrough was recorded again by its own bot and has the same inputs, the
same length and the same ending as before (compared field by field); the
recordings now also carry the level they were made on.

**A legend's letters come out tiles first, then things.** The JSON
reader keeps an object's keys sorted, not in the file's order, so "in the
order written" wasn't available without a new dependency feature. Tiles,
then things, each sorted, is what the palette shows.

**The engine notes the text files a game reads** (`src/files.rs`), per
thread (the engine runs a game, and each test, on one thread). That's how
`reload` knows which files matter and how a recording can keep them.
Text given instead of the disk (by the Studio or a replay) is forgotten
when the run ends, so the next game on that thread reads the disk again.

**One new hook, `Game::reload(ctx, file) -> bool`,** called between two
updates when a file the game read changes: in a window when it changes on
disk (looked at twice a second, like the tune file), in the Studio through
the link's new `file` message, and in a replay on its frame. The starters'
`reload` keeps the duck where it is, keeps collected coins collected, and
adds and removes only what changed (`spawn_changes`).

**Changes made while running are part of the recording.** An input script
can now have `"files"` (the texts on frame 1) and `file` and `tune` events
(`docs/input-format.md`); on the same frame, changes come before inputs.
`Simulation::change_file` and `change_tunes` make them, recordings keep
them, replays apply them on their frame. Old scripts read exactly as
before. A replay of a game that read a file before the replay could give
it the recorded text calls `reload` before the first update (and warns if
the game has none). A window records an edited tune file the same way now,
so F10 recordings replay exactly too.

**Each edit is its own event, not one per freeze.** The design merged the
edits of one freeze into one change. But the game read each of them
(`reload` runs at once, so the frozen picture shows it), and two reloads
aren't always the same as one; recording each, in order, on the same frame
is exact whatever the game's `reload` does. To keep sessions small, a
paint stroke goes to the game at most five times a second (and once more
at its end), not every frame.

## Build mode

**The picture is the game's own drawing.** Each picture the game sends
says where its tile map was drawn (`TileMap::draw` notes it; a game that
draws its own tiles, like the top-down starter, calls `map.mark_drawn`).
The Studio puts its grid there, so the squares sit exactly on the game's
tiles at any scale, and the pointer's square is the game's square.

**Panning the game's own picture (the design's `view`) wasn't built.**
The frozen frame shows what the game's camera saw. For the rest of the
level, "Whole level" shows all of it drawn by the Studio with the engine's
own sprites: the same pixels for tiles, and badges for things the tile
sheet has no picture of, but none of the game's backgrounds. This is the
fallback the design named. A game-drawn `view` needs every game to draw
through a camera the engine can swap, which would be a change to how games
are written; it's left for when it's clearly needed.

**Tools:** Paint (drag; right-drag erases), Erase, Fill, Things (click to
put, drag to move, right-click to take away; a thing the level has one of,
like the Start, moves rather than doubles), Select (drag a box, drag it to
move it), Play from here. Every click is a command (`level.paint`,
`level.fill`...), the same ones a script or an agent uses.

**Play from here starts the game again with the Start moved, without
changing the file.** The design had the game take the moved level through
`reload`, but `reload` keeps the duck where it is. Instead the Studio
starts the game with `DUCKFORGE_GIVE_FILES` naming a small JSON file of
texts; the engine reads those instead of the disk from the start, and the
session's recording keeps them, so it replays.

**Every edit is saved at once**, whole: written to a file beside it,
which then takes its place, so a crash can't leave half a level.

## Play tests that survive level edits

The starters' tests now come in two kinds. How the game plays (running,
jumping, planks, walls, water, pausing; walking, diagonals, hits, the last
coin) is tested on tiny levels written in the test, with the real legend's
letters. The real level is tested for what must stay true: its rules hold
(`broken_rules`), the top-down level is closed and every coin can be
walked to (`reach_on_foot`, exact), and the platformers' bot can still win
on it, with every coin. Painting the level can't break the first kind,
and breaks the second only when the level really changed what matters.

**"A coin looks out of reach" is a warning in Check, not a test.**
`map.reach` follows the arc a point takes at a few speeds, cautiously. A
first version wrongly called most of Duck Peaks' coins unreachable (it
checked for walls before checking for landing); fixed, it agrees with the
bot on both platformers. But a cautious reckoning can still be wrong, and
as a test it could block "Checks pass" for a level that plays fine. So
Check shows it as a warning, with a picture of the spot ringed, and the
bot winning is the test.

**A recorded playthrough replays on the level it was made on** (its
`"files"`), so painting the level doesn't make it fail. Check says "The
recorded playthrough is from an older level" and offers "Record it again",
which has the game's bot record it on the level as it is now.

The games also start without a Start or a flag (a level being painted),
and their cameras cope with a level smaller than the screen (they used to
panic there).

## Feel

The game says its `ctx.tune` values when the Studio asks (a new `tunes`
question: each value, its default in the code, and where it came from).
Every one is a slider; `examples/<game>.feel.json` (optional) gives
labels, groups, ranges and presets. A slider sends the value at once (a
`tune` change in the session) and writes the tune file, whole. One drag is
one step for Undo. Editing the tune file by hand moves the sliders.
"code" finds `tune("name"` in the game's code and shows the lines around
it. The recording's `"tune"` block keeps the values as they were on frame
1, and the changes come after, on their frames, so a replay with Feel
changes in the middle is exact (the real-game test changes the jump while
frozen and replays to the same picture).

The starters and Duck Peaks have feel files with presets, and
`tools/new_game` copies a starter's.

## Pictures and sounds

**Pictures** open in Duck Quill (found next to the Studio, or chosen once
with Choose...). The Studio watches the game's `assets` folder; when a
picture changes, the game gets a `file` message for it and the starters'
`reload` loads their art again, and the palette is drawn again. A
replay knows a reload happened then, but not what the picture looked
like: pictures aren't copied into the session (the design's `"copy"`), so
a replay draws today's pictures. The game itself plays out the same.

**Sounds:** the game keeps the latest of each sound it has played, by
name, and says them when asked (`sounds`). Play asks the game to play it
again, outside the game, so nothing it does changes. Recipe shows a
synthesizer sound's values as code, or Duck Echo's `art/<name>.toml`, or
the line of `art/README.md` that names the file.

## Undo and History

**Undo** steps name the files they changed, with their text before and
after, so one kind of step covers the level and the tune file. A paint
stroke or a slider drag is one step; a change made outside the Studio is a
step called "Changed outside the Studio". Undoing the first Feel change
removes the tune file it made, and the game gets its values from the code
again.

**History** is plain folders, `history/<game>/0007 Painted 14 tiles/`, each
with `files.txt` (every file the game had then, with a short fingerprint),
`about.json` (title and time), and copies of the files that changed since
the one before. Any snapshot can be put back: each file comes from the
newest copy at or before it. Snapshots are taken when the game is first
opened in the Studio, when you play after changes, on a preset, before a
Check, before going back, and every five minutes while there are changes;
"Go back to here" takes one of now first. Snapshots older than a week are
thinned to one a day (their copies move into the next one). It covers the
game's folder (not `target` or `out`), its tune and feel files and its
playthroughs. `history/` is in `.gitignore`. Times show in UTC: the Studio
has no time-zone library.

## Windows line endings

The Windows workflow found two real problems the Linux tests couldn't:
Git on Windows checks `level.txt` out with `\r\n` at the end of each
line. The Studio wrote the level back with `\n`, so Undo didn't put back
exactly what was there; and Check called every example's playthrough
"from an older level", because the recordings, made on Linux, have `\n`.

**Line endings don't count.** The engine gives a game every text file
with its lines ending `\n`, whatever the file has (read from disk, given
by the Studio or a replay, or changed while running), so a recording made
on Windows is the same as one made on Linux, and a replay on Windows
doesn't call `reload` for a level that only looks different. The Studio
saves a level with the line endings its file had (painting changes only
the cells painted, in Git's eyes too), Undo puts back the file's text
exactly, and Check compares levels without regard to line endings. Both
were tested here with the three games' level files given `\r\n`
endings: the engine's and the Studio's whole test suites pass.

## The link

Version 2: `overlay` (F1), `file` (a changed file; answered with
`changed`), `tune`, `play_sound`; questions `tunes`, `sounds`, `files`;
and each picture says where its tile maps were drawn.

## New commands

`game.debug_overlay`, `game.screenshot`, `level.paint`, `level.erase`,
`level.fill`, `level.place_thing`, `level.move_thing`,
`level.delete_thing`, `level.move_area`, `level.resize`,
`level.play_from`, `feel.set`, `feel.preset`, `feel.show_code`,
`assets.open_in_quill`, `assets.play_sound`, `assets.show_recipe`,
`check.record_again`, `edit.snapshot`, `edit.go_back_to`, `edit.undo`,
`edit.redo`. `DuckForgeStudio --commands` lists them all.

## The numbers

**Through the link**, measured as in Step 2 (`DuckForgeStudio --timing
<game> 20`, the Studio in release, on the 4-core cloud machine). The link
now also says where tile maps were drawn, in every picture; it costs
nothing you can see:

| Game | Drawn by | Updates a second | Pictures a second | Game | Link | Late |
|---|---|---|---|---|---|---|
| Duck Peaks | the CPU | 60.0 | 59.4 | 0.44 ms | 0.59 to 0.63 ms | 0 of 297 |
| 3D runner | llvmpipe | 59.9 to 60.2 | 59.0 to 59.4 | 1.8 to 2.0 ms | 0.53 to 0.57 ms | 0 or 1 of 296 |
| Duck Craft | the CPU | 59.9 to 60.1 | 54.0 to 59.2 | 11.3 to 14.2 ms | 0.53 to 0.60 ms | 257 to 286 of about 290 |

Duck Craft's late pictures are as in Step 2: the game alone takes longer
than the 10 ms the Studio waits, on this machine.

**Before and after.** Same machine and same commands as Step 0's
baseline. The first measurement ran while the Studio was being built in
the background, and was slower everywhere; these are from a second,
quiet run.

| | Step 0 | After Step 2 | After Steps 3 and 4 |
|---|---|---|---|
| Headless clean build of `dodge` | 13.1 s | 13.0 s | 13.8 s |
| ...after touching `src/lib.rs` | 0.8 s | 0.7 s | 0.8 s |
| Headless test suite from an empty target folder | 74.3 s, 529 tests | 65.5 s, 533 tests | 71.1 s, 544 tests |
| Headless test suite, already built | 21.0 s | 20.7 s | 21.4 s |
| All tests, already built | 18.0 s, 562 tests | 18.4 s, 567 tests | 22.4 s, 578 tests |
| stress3d, 500 objects, CPU | 2.5 to 3.5 ms | 2.5 to 2.9 ms | 3.10 to 3.15 ms |
| stress3d, 2,000 objects, CPU | 7.9 to 9.2 ms | 7.1 to 7.5 ms | 7.6 to 8.4 ms |
| stress3d, 2,000 objects, GPU (llvmpipe) | 14.5 to 19.1 ms | 15.3 to 17.1 ms | 14.1 to 19.7 ms |
| Block world, 4,096 textured blocks, CPU | 5.2 to 5.7 ms | 5.0 to 5.1 ms | 4.3 to 5.2 ms |
| Duck Craft playthrough, CPU | 8.0 to 9.0 ms (slowest 15 to 21) | 8.5 to 8.7 ms (slowest 19 to 20) | 8.6 to 9.2 ms (slowest 18 to 27) |
| Duck Craft playthrough, GPU (llvmpipe) | 13.6 to 14.8 ms | 12.4 to 12.7 ms | 13.2 to 14.0 ms |
| Release game: `dodge` | 6,078,912 bytes (4,319,328 stripped) | 6,069,304 (4,317,488) | 6,218,200 (4,423,520) |
| Release game: `duck_peaks` | 6,210,264 (4,422,496) | 6,203,944 (4,422,736) | 6,388,856 (4,557,416) |
| Release game: `starter_runner3d` | 11,450,680 (8,534,040) | 11,436,704 (8,531,976) | 11,584,784 (8,639,288) |
| Release game: `duck_craft` | 11,787,376 (8,831,592) | 11,780,264 (8,831,280) | 11,930,864 (8,941,256) |
| Release game, headless only: `dodge`, `duck_craft` | 2,230,536 and 2,726,336 | 2,247,864 and 2,743,728 | 2,357,120 and 2,858,312 |
| Examples passing `verify` | 20 | 20 | 20 |

**The times** are the same within this machine's noise. "All tests,
already built" is 4 s slower, and that is Duck Craft's own tests, which
took between 14.5 and 18 s on this machine in both Step 2's build and
this one, run after run; nothing in them changed. The frame times are
within the spread Step 0 saw.

**The programs are bigger: about 150 KB a game, 110 KB headless.** This
is new code every game carries, not a copy of something: recordings that
keep file and tune changes and replay them on their frame (the script
reader and writer grew), the file layer (noting files, giving texts,
`DUCKFORGE_GIVE_FILES`), the level legend and its rules, and `reach`.
Looking at the program's functions, the growth is spread over many of
them and nothing appears twice, which was what went wrong in Step 2.
Duck Peaks grew a little more (185 KB) because it now reads its level
from files and has a `reload`. That's 1 to 3 per cent of a windowed game
and 4 to 5 per cent of a headless one; whether that is "noticeably
worse" is a judgment call, so it's said plainly here.

New numbers, for next time:

| | |
|---|---|
| A release game built with the link: `dodge`, `duck_craft` | 6,372,256 and 12,076,216 bytes |
| `DuckForgeStudio` (Linux, release) | 32,182,632 bytes (Windows: on the release workflow's page) |
| The Studio's tests | 49 (38 after Step 2), and the engine has 8 more with `--features studio` |
| The engine's tests now | 545 headless, 553 with `--features studio` |

## What Windows has shown

The release workflow passed on GitHub's Windows machine (commit
25e628b): the Studio's tests, including the real-game test that paints
the template's level (checked out with `\r\n`) while frozen, changes the
jump, undoes both and replays the session to the same picture; the
release build; the timings; the zip; and the beginner's path from the
unzipped folder, which ended with "Checks pass" (it builds, its 9 play
tests pass, its playthrough plays out as before, 50 random runs) and a
clip, in 2 min 52 s (2 min 10 s at Step 2). The two runs before it failed
on the line endings described above; both are fixed.

Its timings, as in Step 2's table:

| Game | Drawn by | Updates a second | Pictures a second | Game | Link | Late |
|---|---|---|---|---|---|---|
| Duck Peaks | the CPU | 60.1 | 58.6 to 58.7 | 0.36 ms | 0.67 to 0.84 ms | 0 or 2 of 293 |
| 3D runner | Microsoft Basic Render Driver | 59.9 to 60.1 | 55.5 to 56.3 | 3.57 ms | 0.70 to 0.96 ms | 9 to 13 of about 280 |
| Duck Craft | the CPU | 59.9 to 60.0 | 57.2 to 57.9 | 12.7 to 13.9 ms | 0.59 to 0.76 ms | 58 to 83 of about 290 |
| Duck Craft | Microsoft Basic Render Driver | 59.8 to 59.9 | 27.8 | 27.4 to 28.0 ms | 0.77 ms | all |

Duck Peaks and Duck Craft are as they were. The 3D runner showed more
late pictures than in Step 2 (0 to 2): the game itself took longer there
(3.6 ms against 2.8 to 3.0) and its slowest round trips were near 40 ms,
which looks like the shared machine: nothing in how 3D is drawn
changed, and on the cloud machine it is 0 or 1 late, as before. It's one
run, so it's on the list to watch. `DuckForgeStudio.exe` is 17,300,480
bytes (16,284,672 at Step 2) and the download 13,534,302 bytes.

## What isn't proved yet

- **Nothing here ran on a real Windows PC with a mouse.** The workflow
  runs the Studio's tests on Windows (now including painting a real level
  while frozen and replaying it exactly), but painting by hand, the
  sliders, Duck Quill opening, sounds playing and History's buttons need
  your PC: checklist items 18 to 26 in `docs/phase4-design.md`.
- **Duck Quill wasn't available here**, so opening a picture in it was
  tested only up to the command the Studio runs. Picking up a saved
  picture was tested by the asset watch logic, not with Quill.
- **No sound card here**: Play on a sound sends the message; hearing it
  is on your list.
- **The game-drawn `view`** (panning the game's own picture) isn't built;
  Whole level is the Studio's drawing of the tiles.
- **Pictures changed during a session** aren't copied into it: a replay
  draws today's pictures.
- **QUICKSTART.md and the skill** are still at their line limits; they get
  the Studio in Step 5, with the beginner's walk-through.

---

# Phase 4, Step 5: two fixes, and the beginner's proof

## Fix 1: the bots play the game as it feels today

**What was wrong.** The platformer starters' bots were a handful of rules
that happened to work with the Feel values in the code: "jump when a coin
is above and near". With the starter's own "Floaty" preset the duck
overshot a plank, came down beside it, and jumped on the spot under a
coin for ever; `the_bot_can_still_win_on_the_level` failed, and Check
with it. A faster run on a level with extra planks failed the same way.
The top-down starter's bot was worse: it had never won at all. Its
recorded playthrough ended "Lost" with 9 coins, and no test looked.

**The bot now follows a route.** `TileMap::route(start, goal, jump)` is
`reach` with memory: the fewest `Hop`s (a walk to the next tile, a walk
off an edge, a jump) from the tile the character stands in to the thing
it wants. The bot builds `jump` from `ctx.tune` every frame, so it plays
with today's Feel; asks for the route to the next coin; and flies each
hop along its arc (it steers by where the arc is at the height it's at,
which stays right whatever the fall limit), braking in time to land
still. When there's no route it says so in plain words, with the place
and how high and far its jump goes.

**Caution has to point the right way, each time.** The first version
failed 28 times in about 400 winnable cases, and planning with "a
slightly lower jump" made it worse: the real duck, jumping a few pixels
higher, landed on planks the plan passed under. What works, found by
tracing each failure:

- The route is planned with the real jump, but a jump only counts if one
  a tenth lower lands on the same level too. So a route never hangs on a
  jump's last pixels.
- `Jump` has a `width`. The jumper bumps into things as if it were an
  eighth of a tile wider on each side and nearly a tile tall; it lands
  only on what something a quarter of a tile narrower on each side would
  catch, and reaches only what something four fifths of a tile tall
  would. (One padding for both, the first try, sent the duck to jump from
  under a brick, and to ledges only its padding could catch.)
- An arc that passes within that margin of a ledge's edge isn't used: the
  duck might catch it or might not.
- The bot counts on nine tenths of its running speed (the duck takes an
  eighth of a second to get going), starts a jump from the middle of its
  tile and not while moving the wrong way, and on the hop that passes
  through a coin keeps to the arc all the way down.

`Jump { .. }` literals need the new field: `width: 0.0` is the old
behaviour. `reach` (Check's "looks out of reach" warning) uses the same
arcs, so with a width it's a little more cautious than it was.

**How sure.** Swept while writing it, with the bot allowed to say "no
way" but never to set off and fail: every combination of the three Feel
sliders over their whole ranges on the starter's level (792: 367 won, 425
no way, none failed); 5,500 randomly painted levels with random Feel
(2,304 winnable, none failed); 2,000 on Duck Peaks' taller level (none
failed). Duck Peaks keeps a 40-round version of that as a play test
(under a second).

**The top-down bot** takes the cheapest path to a coin where a tile near
an enemy costs more (the nearer, the more), with the berth it gives them
taken from the two speeds, and it never stands still. It wins with the
code's values and with "Easy" and "Hard", and across the sliders wherever
the player is clearly faster than the enemies; where they're as fast or
faster it's caught, as a person would be.

**Every preset is tested.** Each starter has
`the_bot_can_win_with_every_feel_preset` (`feel_presets` reads the feel
file), and `the_bot_can_still_win_on_the_level` tries the code's values
when the current ones fail, so it can say which it is. Check shows the
bot's own sentence as the line: "The bot can't win with the Feel values
as they are now", "...with one of the Feel presets", or "...on the level
as it is now" (the level first, if several are true), with what to do and
a picture of the place.

**The bot is cautious, on purpose.** It will say "no way" to a jump a
practised player can make. That's the right way round for "can a
beginner's level be finished", and the message gives the jump's height
and length so it can be judged.

## Fix 2: a new game never trips on a playthrough it doesn't have

A game made and edited before its first playthrough was recorded (or
whose bot couldn't win just then) showed "could not read
playthroughs/pond_hop.json". Three things, any of which would have done:
the starters' playthrough test passes when there's no recording yet;
Check has the game's bot record one first when it's missing (once per
Check); and if there still isn't one, Check's line is "No playthrough
recorded yet", with a Record it button if the game has a bot. The
playthrough tests also asserted "8 coins", which failed the moment a coin
was added and the playthrough recorded again: they now ask for the flag
and no coins left.

## The proof

`studio/proof/beginners-proof.json` is the brief's Step 5 as a list of the
Studio's own commands, 87 steps: a new platformer ("Gap Dash"), its first
build, playthrough and Check; a gap, planks and coins painted; a run and a
jump; frozen in mid-air over the gap; a plank moved under the duck;
carried on, landing on it; "Fast and bouncy" set while playing; Check,
which finds a coin the bot can't get to; a plank painted to fix it; Check
again, which passes and says the playthrough is from the older level;
recorded again; a clip; then Duck Craft: a picture, another a moment
later with nothing touched, then the mouse moved and nothing else, then
walking on.

Two step kinds were added so a script can prove itself: `{"expect":
"check", "passed": false, "says": "..."}` stops the script if the last
Check doesn't say that, and `{"shot": "a name"}` marks a picture.

It runs three ways, from the one file:

- **A test in the Studio's core** (`beginners_proof.rs`, about 20
  seconds) runs it with no window, then replays the saved session of the
  jump headless and reads its summary: no splash, both coins, on the
  ground, past the gap. It also checks a clip was made, and that the
  mouse alone turned Duck Craft's view: left alone, about a sixth of the
  picture changes in a third of a second (water, clouds, the day's
  title); with the mouse moved and nothing else, 86 per cent does. What
  it can't show is the pointer being held and given back in a real
  window. The release workflow runs it on
  Windows with the other tests.
- **The app's screenshot test** follows it and saves the Studio's screen
  at each shot: `docs/studio-screens/proof-01...14`.
- **The release workflow** runs it with the real `DuckForgeStudio.exe`
  from the unzipped folder, in place of the shorter first-game script.

**What the proof found** (fixed as it went):

- Check's line was "one of the Feel presets" when the level couldn't be
  won at all; the level now comes first.
- With Check open, the game was squeezed to its smallest size between the
  Build and Feel panels, just when the place Check names needs finding.
  Check now takes Feel's place while it's open, and long lines are cut
  short in the list rather than widening the panel.
- The bot's line had no picture. It gets one of the place it names,
  ringed, wide enough to show the ground and what's around.
- Nine separate "looks out of reach" warnings for one low jump are now
  one line.
- Check says places as "column 14, row 3"; Build mode didn't show which
  tile the pointer was on. The status bar does now.
- A level that only works with the Feel it was painted for fails the
  preset test, rightly, but the message didn't say what to do. It does
  now: change the level, or change (or take out) that preset in the feel
  file.
- A script's keys: the Studio lets go of held keys when a frozen game
  carries on, unless they're really held. Right for a person; a script
  presses them again.

## The numbers

**Through the link** (`DuckForgeStudio --timing <game> 20`, the Studio
in release, the cloud machine), as before:

| Game | Drawn by | Updates a second | Pictures a second | Game | Link | Late |
|---|---|---|---|---|---|---|
| Duck Peaks | the CPU | 59.9 to 60.0 | 59.2 to 59.4 | 0.46 to 0.52 ms | 0.66 to 0.70 ms | 0 or 1 of 296 |
| 3D runner | llvmpipe | 59.9 to 60.1 | 59.2 to 59.3 | 2.0 to 2.1 ms | 0.61 to 0.64 ms | 0 or 1 of 296 |
| Duck Craft | the CPU | 60.0 | 53.8 to 55.6 | 11.9 to 12.8 ms | 0.63 to 0.69 ms | 246 to 270 of about 275 |

**Before and after.** The same commands as Step 0's baseline. This time
the whole machine was slower than on the day of Steps 3 and 4: the first
column of today's figures looked 8 to 10 per cent worse everywhere,
including 3D frame times in code this step didn't touch. So the commit
from the end of Steps 3 and 4 was measured again today, turn and turn
about with this one, twice each. Those are the last two columns, and the
ones to compare.

| | Step 0 | After Steps 3 and 4 (that day) | Steps 3 and 4, measured today | Step 5, measured today |
|---|---|---|---|---|
| Headless clean build of `dodge` | 13.1 s | 13.8 s | 13.9 and 15.5 s | 13.6 and 15.4 s |
| ...after touching `src/lib.rs` | 0.8 s | 0.8 s | | 0.9 s |
| Headless test suite from an empty target folder | 74.3 s, 529 tests | 71.1 s, 544 tests | | 77.4 s, 553 tests |
| Headless test suite, already built | 21.0 s | 21.4 s | 21.3 and 23.1 s | 23.3 and 22.7 s |
| All tests, already built | 18.0 s, 562 tests | 22.4 s, 578 tests | | 26.1 s, 587 tests |
| stress3d, 500 objects, CPU | 2.5 to 3.5 ms | 3.10 to 3.15 ms | 3.12 and 3.18 ms | 3.04 and 3.12 ms |
| stress3d, 2,000 objects, CPU | 7.9 to 9.2 ms | 7.6 to 8.4 ms | 8.48 and 9.06 ms | 8.25 and 8.37 ms |
| stress3d, 2,000 objects, GPU (llvmpipe) | 14.5 to 19.1 ms | 14.1 to 19.7 ms | 22.6 and 21.4 ms | 19.7 and 20.8 ms |
| Block world, 4,096 textured blocks, CPU | 5.2 to 5.7 ms | 4.3 to 5.2 ms | 4.77 and 5.09 ms | 5.05 and 5.05 ms |
| Duck Craft playthrough, CPU | 8.0 to 9.0 ms | 8.6 to 9.2 ms | 9.23 and 9.22 ms | 9.08 and 8.46 ms |
| Duck Craft playthrough, GPU (llvmpipe) | 13.6 to 14.8 ms | 13.2 to 14.0 ms | 13.5 and 14.4 ms | 13.6 and 14.1 ms |
| Release game: `dodge` | 6,078,912 bytes (4,319,328 stripped) | 6,218,200 (4,423,520) | | 6,228,000 (4,428,424) |
| Release game: `duck_peaks` | 6,210,264 (4,422,496) | 6,388,856 (4,557,416) | | 6,397,640 (4,561,768) |
| Release game: `starter_runner3d` | 11,450,680 (8,534,040) | 11,584,784 (8,639,288) | | 11,594,552 (8,642,472) |
| Release game: `duck_craft` | 11,787,376 (8,831,592) | 11,930,864 (8,941,256) | | 11,942,720 (8,945,520) |
| Release game, headless only: `dodge`, `duck_craft` | 2,230,536 and 2,726,336 | 2,357,120 and 2,858,312 | | 2,343,704 and 2,844,744 |
| Examples passing `verify` | 20 | 20 | | 20 |

Side by side, nothing got slower: builds and frame times are the same
within the two rounds' spread. The already-built test run is about a
second longer, which is the nine new play tests (each starter's bot
playing its level once for each Feel preset, and Duck Peaks' forty
painted levels). The programs are about 10 KB bigger with a window (the
route) and 13 KB smaller headless.

New numbers:

| | |
|---|---|
| A release game built with the link: `dodge`, `duck_craft` | 6,381,000 and 12,085,832 bytes |
| `DuckForgeStudio` (Linux, release) | 32,240,584 bytes |
| The engine's tests | 553 headless, 561 with `--features studio` |
| The Studio's tests | 51 (49 after Steps 3 and 4) |
| The beginner's proof, with no window | about 20 s here once the games are built; 3 min 26 s on GitHub's Windows machine from the unzipped folder, building the new game and Duck Craft from nothing |

## What Windows has shown

The release workflow passed on GitHub's Windows machine at the proof's
commit (1fb77be), first time: the Studio's tests (now with the proof as
a test), the build, the timings, the zip, and all 83 steps of the proof
run by `DuckForgeStudio.exe` from the unzipped folder, ending with
"Checks pass" and a clip. The clip there is a GIF (366 KB): that machine
has no ffmpeg, so it shows the fallback works. `DuckForgeStudio.exe` is
17,356,800 bytes and the download 13,573,505. Duck Craft drawn by the
CPU showed 58.4 to 58.5 pictures a second with 34 to 53 late of 292,
a little better than before. The 3D runner's line (the one to watch from
last time) is in that run's "timings" download; it wasn't read here.

The proof then grew to 87 steps (Duck Craft's mouse look on its own,
with a picture before for comparison) and the workflow passed again at
that commit (affcffd): the proof as a test with the stricter Duck Craft
check, and the whole script from the unzipped folder in 3 min 29 s.

## What would still confuse a beginner

Said plainly, from walking the proof and reading its pictures:

1. **Before anything: Rust.** The Studio says so in plain words and links
   to it, but installing a programming language is still the first thing
   a non-programmer must do, and the first build takes a minute or two
   with only "Building it for the first time" to look at.
2. **The Studio doesn't edit code.** Painting, Feel, pictures and sounds
   are covered; anything else (a new enemy, a double jump, a second
   level) needs an agent or an editor. And once the game's rules change,
   the bot doesn't know: it's code in `bot.rs`, and "the bot can't win"
   then means "the bot needs teaching", which Check can't tell apart.
3. **The bot is careful.** It may say a coin can't be got to when a
   practised player can get it. The message gives the jump's height and
   length, but a beginner may still read it as "my level is broken".
4. **Presets are a file.** A level that needs "Fast and bouncy" fails
   Check until the other presets are changed in `<game>.feel.json` by
   hand; the Studio has no preset editor.
5. **Feel doesn't change the code.** The sliders write a tune file; "code"
   shows the line with the old default and says so, but "why does it
   still say 330" is a fair question.
6. **One level.** There's no list of levels, and no button for a new one.
   Resizing the level is a command without a control in Build mode.
7. **Only two starters have a level to paint** (the platformer and the
   top-down one). In the shooter and the 3D runner, Build mode has Feel,
   sounds and pictures, and nothing to paint.
8. **Whole level** is the Studio's drawing of the tiles, without the
   game's backgrounds, so it looks plainer than the game.
9. **Sounds** lists only what the game has played so far; a sound that
   hasn't happened yet isn't there.
10. **Duck Quill** is a separate download the Studio has to be pointed at
    once, and it wasn't here to try.
11. **Check takes a few seconds and says "50 random runs"**: what those
    are, and why "no problems found" doesn't mean the level is good, isn't
    explained in the Studio.
12. **History** shows times in UTC and keeps snapshots as numbered
    folders: fine for an agent, odd for a person in another time zone.
13. **A clip is the last ten seconds**, as a GIF if ffmpeg isn't
    installed (much bigger, no sound).
14. **Nothing here ran on a real Windows PC with a mouse, a sound card
    and a graphics card.** The workflow proves the commands on Windows;
    painting by hand, hearing sounds and the feel of mouse look are the
    checklist in `docs/phase4-design.md`.

# Windows: tests, a double-click, an installer and new versions

On a Windows 11 PC, `test.bat` said 333 passed and 5 failed, all in the
engine's own unit tests (so the integration tests and the examples'
tests had never run on Windows: `cargo test` stops at the first test
program that fails). This part makes the tests pass there, gives a
person something to double-click, an installer, and update notices.

## The five failures

**`a_recording_beats_the_games_file_but_not_the_command_line` read an
empty file.** Two tests in `src/tune.rs` wrote `order.tune.json` in the
same temporary folder (named only by the test program's process number,
the same for every test in it), and `cargo test` runs a file's tests at
the same time: one read the file while the other was writing it. A real
bug in the tests, fixed.

**"Access is denied (os error 5)" on `tests/fixtures/quill/scale1/duck.json`**,
in four tests that read it (three in `src/spritesheet.rs` and the one in
`src/tileset.rs`), while a fifth test read it fine. What we know:

- On Linux, a trace of every file the engine's 340 unit tests open shows
  that the fixture is only ever *read*. One test (the "wrong size" one)
  also copied it with `fs::copy`, which on Windows is the system's
  CopyFileEx, a different call that also copies a file's attributes and
  hidden streams. The full `cargo test` (591 tests) writes nothing in
  the project folder but `target/`.
- On GitHub's Windows machine, a program that copies the fixture with
  `fs::copy` over and over while six threads read it gave no error in
  210,000 reads; nor with Windows Defender's real-time scanning switched
  on (it slowed the copies five times, so it was scanning), nor with the
  files in Downloads marked as downloaded from the internet, as Explorer
  marks the files in an unzipped download. The Phase 4 engine's unit
  tests, unchanged, passed fifteen times in a row there, with Defender on
  and off.

So **it didn't reproduce on GitHub's Windows machine, and the cause isn't
proved.** "Access is denied" for a file that exists and is only being
read is what Windows says while another program holds it in a way that
blocks reading, or while it's being replaced or deleted. The likeliest
causes on a home PC are a program that isn't on GitHub's machines: a
virus scanner other than Defender, a backup program, or OneDrive (which,
on many Windows 11 PCs, syncs Documents and Desktop, where a download is
often unzipped) holding a newly unzipped file for a moment. That fits a
timing failure that hit several tests at once.

**Made safe anyway.** Every test in the engine, its integration tests and
the Studio now writes only in a `TestFolder` of its own
(`src/test_folder.rs`, `tests/common/mod.rs`,
`studio_core::test_folder`): named by the test, the process, the time
and a count, so no two tests or runs share one (Windows soon gives a
process number to another program); deleted when the test passes, kept
when it fails. No test copies a fixture with `fs::copy`; each Quill test
loads a private copy in its own folder, read and written again, and that
one read of the shared file waits and tries again for up to two seconds
if Windows says "Access is denied". The Studio's tests used fixed
settings file names with no process number, set through an environment
variable shared by every test in the program; settings now remember the
file they're saved in (`Settings::file`), so a test's settings are never
saved anywhere, and no test sets the variable. One more shared name was
found on the way: the Studio's level tests named their folders by the
length of the level's text.

**The Windows workflow runs the engine's tests**, `cargo test` and
`cargo test --no-default-features`, five times in a row (one green run
proves little for a test that fails now and then), and runs whenever the
engine, its tests, tools or examples change, not only the Studio.

## Something to double-click

`test.bat` is now `tools\Run tests (for developers).bat`: the engine's
tests both ways, then the Studio's (skipped in a copy with no Studio
code). `Start Duck Forge Studio.bat`, at the top of the folder, builds
the Studio (`cargo build --release -p studio_app` in `studio/`) and opens
it, saying what it's doing. Both call `tools\rust_check.bat` first: if
`cargo` isn't on the PATH it looks where rustup puts it (a window opened
right after installing Rust may not know yet), and if Rust really isn't
there it says so in plain words with the official page, offers to open
it, and installs nothing. `tests/windows_scripts.rs` checks all of them
(brackets, labels, Windows line endings, Rust checked before cargo).

## The installer

**Inno Setup 6.** One `DuckForgeStudioSetup.exe`, from a short text
script (`studio/release/DuckForgeStudio.iss`) that a person can read.
It installs for one person with no administrator rights
(`PrivilegesRequired=lowest`), so `{autopf}` is
`%LOCALAPPDATA%\Programs\Duck Forge Studio`, a folder they can write
to; makes Start menu and desktop shortcuts and the entry in Windows
Settings, Apps; closes the Studio if it's open; and its Pascal script
checks for `cargo.exe` and offers the Rust page. It's free, it's on
GitHub's Windows machines or a `choco install` away, and it has done
per-user installs and upgrades in place for decades. WiX makes an MSI,
which needs a separate bootstrapper to be one `.exe` and is awkward per
user; NSIS would do, with a less readable script.

The fixed `AppId` makes a newer installer replace an older install.
`[InstallDelete]` empties the install folder's `duckforge` first, so no
file from an older version is left in it. Uninstalling removes the
program, that copy, the shortcuts, and `%LOCALAPPDATA%\Duck Forge\build`
(built programs only).

## Where the games are

Before this, an unzipped Studio made games in the `duckforge` folder
beside it: inside its own folder, which a newer version would replace.
Now **an installed or unzipped Studio never makes anything in its own
copy of Duck Forge.** The first time it starts, it copies that copy to
`Documents\Duck Forge` (the real Documents, asked of Windows with
`SHGetKnownFolderPath`, since OneDrive often moves it) and everything
happens there. It's still a whole Duck Forge folder, laid out as before,
so `tools/verify`, `tools/new_game`, `cargo run`, `--fuzz`, `--video` and
agents work in it unchanged.

A newer Studio brings that folder up to date the next time it starts
(`studio_core::my_games`): `.came-with-studio.json` remembers a
fingerprint (FNV-1a, the same on every computer) of each file that came
with the Studio. A file still as it came is replaced by the new one; a
file the person changed is kept, and Home says which; their games,
playthroughs, tune files, `out/` and `history/` aren't in the list at all,
so they're never touched; files the new version dropped go if unchanged;
a game they deleted stays deleted. `Cargo.toml` is the one file both
write (`tools/new_game` adds a game's lines), so the new one is used and
the person's games' `[[example]]` lines are added back. A Duck Forge
folder the Studio didn't make is used as it is and never changed.

Builds go to `%LOCALAPPDATA%\Duck Forge\build`, through a
`.cargo/config.toml` the Studio writes in the games folder: a build is
gigabytes, and Documents may be copied to OneDrive. Every cargo command
in the folder, the Studio's, the tools', a terminal's or an agent's,
reads that file. Settings stay where they were,
`%APPDATA%\Duck Forge Studio\settings.json`. Choosing the Studio's own
copy as the games folder is refused. A Studio built from the code (with
`Start Duck Forge Studio.bat` or `cargo run`) has no copy beside it, and
uses the Duck Forge folder it's in, as before.

## New versions

When the window opens (not for `--run` or the tests), the Studio reads
`https://api.github.com/repos/mrguymrguy647-del/Duck-Forge/releases` in
the background, with the `curl.exe` that comes with Windows: no new
library, and the request is what any download sends, with nothing about
the person. The newest `studio-vX.Y.Z` that isn't a draft or a
pre-release is compared with the running version, as numbers (0.10 is
newer than 0.9). If it's newer, Home shows a banner: the version, "What's
new" (the release's page), **Update** and "Not now". Update downloads
that release's `DuckForgeStudioSetup.exe` to the temporary folder,
checks its size, starts it and closes the Studio; a release without an
installer opens its page instead. Nothing is downloaded without that
click. No internet, an error page or a list it can't read: nothing is
shown. Settings, "Tell me when a new version is out", turns it off (on
unless turned off). All of it is in `studio_core::update` and the app;
nothing is in the engine or in games. `DuckForgeStudio
--check-for-updates` says what the check finds, and `DUCK_FORGE_RELEASES`
points it at another list: the workflow uses that to try the real `curl`
on Windows with a pretend newer release, the same version, and an
address nothing answers at. The unit tests use a pretend internet,
including no internet, up to date, drafts, pre-releases, the engine's
own tags, a cut-short download and a release with no installer.

## The numbers

| | |
|---|---|
| Engine code that games are built from, changed | none: one `#[cfg(test)]` line in `lib.rs` |
| Dodge, Duck Peaks, Duck Craft in release, and headless Duck Peaks: machine code (`.text`), constants (`.rodata`, `.data.rel.ro`) | byte for byte the same as Phase 4's |
| Their sizes | 48, 40, 40 and 0 bytes smaller (only LLVM's internal symbol names differ, `.llvm.<number>`, and the numbers came out shorter) |
| Duck Craft, 1,200 frames headless, three runs | 0.52, 0.52, 0.54 s before; 0.51, 0.52, 0.54 s after |
| Engine tests on Linux | 591 pass with everything on |
| The Studio starting with no window (`--run` of nothing), here: built from the code | 0.014 to 0.018 s |
| The same, installed, the first time (makes `Documents\Duck Forge`: 234 files, 10.5 MB) | 0.05 to 0.06 s |
| The same, installed, later (checks the folder is up to date) | 0.03 to 0.04 s |

# Small Studio fixes from the first hour on a real PC

## Tests and \r\n

The quickstart test split `docs/QUICKSTART.md` on "```rust\n"; a
Windows checkout (`core.autocrlf=true`, Git's default there) gives
every text file `\r\n`, so it found no code blocks: the one failure in
all five Windows runs. It now reads `\r\n` as `\n`. To find any other
test that assumes `\n`, the repository was cloned on Linux with
`core.autocrlf=true` (every text file then has `\r\n`, as on that PC)
and every engine and Studio test run there. That showed two more
things: `tools/*.sh` can't run with `\r\n` (Git Bash and WSL on Windows
would hit it), so `.gitattributes` keeps `*.sh` on `\n` as it keeps
`*.bat` on `\r\n`; and two tests of the games folder's build setting
failed whenever `CARGO_TARGET_DIR` was set, so they check the folder's
own setting now.

## Delete, rename and duplicate

`studio_core::manage`, as `game.delete`, `game.rename` and
`game.duplicate`. A game is its code (a folder or one file), its tune and
feel files, its playthroughs, its `[[example]]` table in `Cargo.toml`,
and what the Studio keeps about it (`history/<game>`, `out/verify/<game>`,
`out/sessions/<game>`). Rename and duplicate also change where the code
names itself, and only that: `examples/<game>/` and `examples/<game>.`,
`playthroughs/<game>` followed by `.`, `-` or `_`, `--example <game>` as a
whole word, `out/verify/<game>`, `out/sessions/<game>`, and the window title
`"<Title> - duckforge"`. A variable that happens to be called like the
game isn't touched, and neither is another game whose name starts the
same (bee_maze_two when renaming bee_maze). The game's type keeps its
name: two examples can each have a `struct Game`. The examples that came
with Duck Forge can't be deleted or renamed (a newer Studio would bring
them back, and the list of what came with it would be wrong); they can
be duplicated, which is a good way to start from one.

Deleting goes to the Recycle Bin (`studio_core::recycle`): on Windows
`SHFileOperationW` with "allow undo", which is what the Recycle Bin is,
plus "warn if it can't be recycled", so on a drive without one Windows
asks before deleting anything for good; on Linux the freedesktop.org
Trash (`~/.local/share/Trash`, with its `.trashinfo`), on macOS
`~/.Trash`. If something can't go to the bin it's reported and nothing
is deleted. Clips stay in `out/clips`.

## The Empty starter

`examples/empty/main.rs`: a window, a background colour, an empty
`update` and `draw`, and a comment saying where to start. It's first in
New game and in `tools/new_game` (`empty`). It has no tests or bot, so
`tools/verify` says "0 passed" and "no playthroughs", and passes. Its
words avoid the word "empty" itself, since `tools/new_game` replaces the
starter's name in sentences.

## The Files panel

`studio_core::game_files` and the `files.*` commands; the app only draws
(`files_panel.rs`), so the scene dragging planned for later can use the
same commands. Paths are places inside the open game's folder, with `/`,
and anything reaching outside it (`..`, a drive, a whole path, a colon)
is refused. Kinds come from the name: `.rs` code, PNG/GIF/BMP/JPEG
pictures, WAV/OGG/MP3/FLAC sounds, a `.txt` that's `level*.txt` or has a
`.legend.json` beside it is a level. A WAV plays in the Studio itself
(Windows' `PlaySoundW`, nothing to install); other sounds and every other
file open in their own program (`explorer <file>`, so no console window
flashes). Dropped files are read and written again into the folder, so
they're ordinary files there (not read-only, without the downloaded
mark), and a name that's taken gets " 2". `main.rs` can't be deleted or
renamed there. Drops are taken anywhere on a game's screen, and the
window says where they'll go while files are dragged over it.

## Thumbnails

A card with no picture showed two letters until the game had a checked
playthrough. Now `out/thumbnails/<game>.png` is made the first time it
plays two seconds in the Studio (or stops after at least ten frames),
from the picture the Studio already has; or, after a Check that drew
nothing (no playthrough), by running it two seconds with no window
(`game.thumbnail`). Check pictures still come first; the docs' picture
last. Empty has one in `docs/` for New game.

## Home's tiny copy

On a 2576 x 1080 window, a tiny copy of Home showed in the bottom-right
corner. It didn't show here: Home drawn with no window at 2576 x 1080 at
100, 125 and 150 per cent scaling and with the Console open, and the
real app on a 2576 x 1080 virtual X screen, resized from its first size
as maximizing does. Nothing in the app draws in that corner (no extra
viewport, no layer painter, no picture of the screen). So it's in the
path only Windows takes (DirectX 12, the window compositor, the
maximized window) and isn't fixed; checklist item 49 asks for a
screenshot.

# Phase 5: a real editor

`docs/phase5-design.md` is the plan. These are the choices made as each
step was built, and the numbers measured.

## Step 1: scenes in the engine

**What a scene is in the engine.** Two types. `SceneFile` is the file:
parsed, with its problems, and written back the same way every time
(one object per line, the common keys in a fixed order, then the
object's own in alphabetical order, and only what differs from the
usual). `Scene` is one being played: the objects as they are now, and
the `SceneFile` as last read (how they start). Tools, and the Studio
from step 2, use `SceneFile` without running anything.

**Why the old `Scene` was renamed.** The engine had a `Scene` already:
what a frame showed (`canvas.label`, `last.scene.json`). Two types with
one name is a trap, and the new one is what people will mean, so the old
one is `OnScreen` in `src/on_screen.rs`. `sim.scene()` still works (with
`sim.on_screen()` beside it), the saved files keep their names, and
nothing in the examples, tests or Studio named the type.

**Update order isn't drawing order.** Objects are drawn in the file's
order, inside their layer, and listed by `ids()` in the order of their
ids. Dragging an object up the Scene list for looks can't change how the
game plays. Ids from the file are small numbers; objects made while
playing get ids from 2^32, and things painted on a tile map from 2^48
(with the map's id, the row and the column inside, so the same square
always has the same id). The three never collide, and by id the file's
objects come first.

**Ids for files written by hand.** An object without an id gets the next
number after the highest id in the file, going down the file. That's
the same every time, so a replay and a reload agree. It isn't stable
when a line is added above, which is why `tools/write_ids.rs` writes
them in. It was first an option of every game (`--write-ids`), and that
made every existing game 100,000 bytes bigger, because `run` then named
the scene code and the linker kept it. As a tool of its own it costs
existing games nothing.

**`scale`, and anchors.** An object's "how many times bigger" is
`scale`; `size` is only a scene's size in pixels, and an object that
says `size` is told the new name. `at` is where the object's anchor is.
A picture stands on its feet (the bottom middle, or a Duck Quill sheet's
pivot); a tile map and a background are placed by their top-left corner,
as text, buttons and areas will be; any object can say another with
`anchor`.

**A kind is only a name, for now.** `picture`, `tilemap` and
`background` mean something to the engine. Any other kind is a name the
game's code asks for (`scene.of_kind("coin")`), so a kind nothing knows
isn't a mistake yet: that check arrives with kinds (step 5) and Check
(step 8), which can tell a typo from a kind the code handles.

**Starts and now.** The rule for taking a changed file in mid-play is
one comparison per key: if the file's value for a key changed, and the
object's own value is still what the file had, the object takes the new
value. Otherwise the game has changed it since the start, and the object
keeps what it has; `starts_as` has the file's. It's done on the JSON of
the three objects (how the file had it, how it has it now, how it is),
so every key, the kind's own settings included, follows the one rule
with no list to keep. Position is one key (`at`), so an object that has
moved at all keeps its whole position.

**Painted things are objects.** Each "thing" letter of a tile map's
level becomes an object: named after the thing, of that kind, standing
in its square, with the tiles' picture of that name if the sheet has one
(the starter's sheet has `coin` and `flag`) or a tile-sized box if not.
The game asks for coins once, painted or placed. When the level is
painted in the Studio, new ones appear, erased ones go, and ones the
game took stay gone.

**Turned and stretched pictures.** `Canvas::draw_placed` is new: a
pivot, a scale each way and a turn. With no turn and one scale it calls
`blit_sprite`, so nothing that was drawn before is drawn differently.
Otherwise it goes through the canvas pixels under the turned picture
and takes the sprite pixel under each one's centre: the same sharp-pixel
rule as resizing. A collision box doesn't turn.

**Mistakes.** Only a file that isn't JSON (or is from a newer Duck
Forge) is an error. Everything smaller is a sentence in
`scene.problems()` with the file and the line of the object, and is
drawn round. The line is found by walking the text for the start of each
object, so it's right however the file is laid out. A file that isn't
JSON, saved while the game runs, changes nothing and is said once.

**The demo.** `examples/scene_demo`: a scene (`scenes/pond.json`), a
level where today's games keep theirs (`level.txt`, so the Studio's
paint tools already work on it), pictures, a bot, play tests and 120
lines of code. Its tests of how the game plays use small scenes written
in the test, in a copy of the game's folder, so editing the real scene
can't break them.

**The Studio.** One change: it looks at the open game's `scenes/*.json`
twice a second, and a file saved from outside is sent to the game
(frozen or playing) and is one step for Undo, as a level edited outside
already was. A test freezes the real demo, saves its scene from
outside, plays on, undoes it and replays the session with no window to
the same last picture.

## Step 1: the numbers

Release builds with the usual features, on this Linux machine. "Before"
is the code before Step 1 with only `--time` added, so both are timed
the same way (`--headless --time`: every frame drawn, nothing saved).
Times are the lowest and highest of three runs (six for the shooter's
drawing).

| Game | Size before | Size after | 600,000 updates, before | after | Drawing a frame, before | after |
|---|---|---|---|---|---|---|
| Platformer starter | 6,391,536 | 6,393,376 | 0.32 to 0.36 s | 0.31 to 0.37 s | 80 to 83 µs | 75 to 76 µs |
| Shooter starter | 6,257,544 | 6,259,768 | 0.46 to 0.57 s | 0.47 to 0.58 s | 18.2 to 19.5 µs | 17.6 to 19.9 µs |
| 3D runner starter | 11,600,200 | 11,602,168 | 2.17 to 2.28 s | 2.17 to 2.27 s | 1,471 to 1,499 µs | 1,464 to 1,488 µs |
| Scene demo | | 6,612,816 | | 0.87 to 0.91 s | | 76 to 82 µs |

- Existing games: about 2,000 bytes bigger (0.03%), no slower.
- `--time` itself added 5,320 bytes to every game (the Step 0 table had
  the Platformer starter at 6,386,216).
- The scene demo against the Platformer starter, which it resembles:
  3.4% bigger; the same time to draw a frame; 1.5 µs an update against
  0.55. Finding the duck by name and asking what it touches, every
  update, is where that goes. A frame has 16,667 µs.
- Engine tests: 331 unit tests (24 new for scenes), 6 play tests in the
  demo, 1 new in the cheat sheet's test, 1 new in the Studio's.

On GitHub's Windows machine (run 43197202570, release builds, the same
`--headless --time`), after Step 1. There's no "before" from that
machine (the option that measures is new), so these are what later steps
are measured against there:

| Game | Size | An update | Drawing a frame |
|---|---|---|---|
| Platformer starter | 3,888,640 bytes | 1.70 to 1.77 µs | 104 to 106 µs |
| Shooter starter | 3,770,368 bytes | 2.44 to 2.55 µs | 20.9 to 21.3 µs |
| 3D runner starter | 9,190,912 bytes | 1.97 to 2.25 µs | 1,424 to 1,438 µs |
| Scene demo | 4,103,168 bytes | 4.03 to 4.09 µs | 104.7 to 105.5 µs |

The scene demo there is 5.5% bigger than the Platformer starter, draws a
frame in the same time, and takes 4 µs an update against 1.7. That run
also passed the engine's tests five times both ways, the Studio's tests
(the scene edited while frozen, replayed exactly, among them), the zip,
the installer, an upgrade over the top and the uninstall.

## Step 2: selecting and moving objects in the Studio

**The game says where its objects are, only when it's frozen.** A scene
that's drawn on a tick with no updates (which is what a frozen game's
picture is) notes, for each object, where it drew it on the canvas,
where it would be if it were still as the file has it, which keys have
changed since the start, and which objects are gone. That goes to the
Studio with the picture (`Frame.scenes`; the link is version 3). While
the game plays nothing is noted and nothing is sent, so playing costs
what it did. All of it is behind the `studio` feature: an ordinary build
has none of it. The Studio now makes sure a frozen game's picture is
always one drawn while frozen (after "Play one frame" it asks for one
more picture with no update), so the outlines never lag behind.

**The Studio still draws nothing of the game.** "As it starts" could
have been the Studio drawing the scene file itself. Instead the game
draws it: a copy of its scene put back to the file's values, through the
camera of the frozen moment. So the start looks exactly as the game
would draw it, with the game's own pictures and tile map, and Build and
Play still can't look different. The game's own scene isn't touched.

**"For this run only" goes through `reload`.** A scene is a value the
game holds; the engine can't reach into it. So a change to an object as
it is now (`ToGame::Object`) is left on a waiting list, the game's
`reload` hook is called with the scene's file, and `scene.reload` picks
up what's waiting for it (`Scene::set_now` does the change). A game that
already reloads its scene needs nothing new. The change is a new kind of
event in a session's recording (`"object"`, `docs/input-format.md`), so
a replay makes it on the same frame: the real-game test drags the duck,
puts it on a coin, plays on, and the headless replay ends on the same
picture.

**The Studio writes scene files the engine's one way.** An edit reads
the file with the engine's reader, sets keys on the object
(`SceneFile::set`: the same checks as reading, so a wrong value is said
in the same words), and writes the whole file as `tools/write_ids`
does. So the first edit of a hand-spaced file tidies it, and writes any
ids that were missing (they're the ids the engine gave anyway). A file
with a mistake isn't written at all: writing drops what couldn't be
read, and a silent loss is worse than a message. A missing
`"duckforge_scene"` line doesn't count (`SceneFile::would_be_lost`).

**What a drag changes is decided when it begins**, and the status bar
says which. The dashed outline: the file. The object: the file if it's
where it starts, this run only if the game has moved it. The other
choice was a switch ("edit the start" / "edit this moment"), which is
one more thing to be in the wrong state of.

**Undo.** A change to the file is a step like a paint stroke (one drag
is one step, by its stroke). A change for this run only is a step too,
but only until the game plays on: after that there's no moment to put
it back to, so those steps leave the list (a step that also changed a
file keeps that part). Undo of "Start here" puts the old start back
*and* leaves the object where it stands; without that, the game would
see "it's still where it starts" and move it back with the start.

**The tile map is picked in the list**, not by clicking the ground, and
things painted on it aren't picked at all. Clicking empty ground
selects nothing. Otherwise every missed click would switch to painting.

**A click picks what's on top; clicking it again picks what's under.**
With the scene demo's duck standing in front of the sign, that's the
only way to reach the sign without the list.

**Engine additions an agent might use:** `SceneFile::set`,
`SceneFile::would_be_lost`, `Object::to_json`, `Object::setting`,
`Scene::set_now`, and (with the `studio` feature) `scene::drawn`.

## Step 2: the numbers

Release builds with the usual features, on this Linux machine.

| Game | Size after Step 1 | after Step 2 |
|---|---|---|
| Platformer starter | 6,393,376 | 6,380,712 |
| Shooter starter | 6,259,768 | 6,252,512 |
| 3D runner starter | 11,602,168 | 11,586,464 |
| Scene demo | 6,612,816 | 6,606,040 |

Every game is a little smaller (7,000 to 16,000 bytes). None has
anything of the editor in it: the Studio's side of the link is only in
builds with the `studio` feature, as before.

Speed, with `--headless --time`. This machine was about a third slower
on the day than when Step 1's table was made, and uneven (the same
program varied by 10%), so Step 1's code was built again and the two
were run turn about. Lowest and highest of three to five runs each:

| Game | Drawing a frame, Step 1 | Step 2 | An update, Step 1 | Step 2 |
|---|---|---|---|---|
| Platformer starter | 104 to 110 µs | 102 to 115 µs | 1.46 to 1.67 µs | 1.40 to 1.78 µs |
| Shooter starter | 25.1 to 26.7 µs | 24.8 to 25.7 µs | 1.53 to 1.74 µs | 1.60 to 1.71 µs |
| 3D runner starter | 2,036 to 2,148 µs | 2,039 to 2,139 µs | 2.57 to 3.44 µs | 2.73 to 3.48 µs |
| Scene demo | 104 to 112 µs | 116 to 119 µs | 2.97 to 3.48 µs | 3.43 to 3.57 µs |

- The three games that existed before the phase: the same.
- The scene demo: about 6% slower to draw and to update here, though
  the code it runs in an ordinary build is the same as in Step 1
  (drawing was split into two functions; nothing was added). Building
  each commit in turn puts the change at the engine commit of this
  step, and the compiled program shows the compiler arranging text
  drawing differently (`Canvas::draw_text` is half the size it was).
  Forcing the old arrangement of the scene's functions made no
  difference. So it's the compiler's choice, not work the scene does,
  and this machine can't settle 6% today. GitHub's Windows machine is
  steadier: its numbers are below.
- Tests: the engine 614 (headless) and 649 (with a window), 343 with the
  link; the Studio's core 70 unit tests and 6 that play real games; the
  app 11 unit tests and 9 screen tests, one of which drives the scene
  editor with a pretend mouse. All of them also pass in a copy of the
  folder with Windows line endings.
- `main` gained Duck Mold's sockets while this step was built; it's
  merged in (no clashes), and the counts above include its tests.

On GitHub's Windows machine (run 43248333794, release builds, the same
`--headless --time`), after Step 2:

| Game | Size | An update | Drawing a frame |
|---|---|---|---|
| Platformer starter | 3,876,864 bytes | 1.23 to 1.33 µs | 82.1 to 82.8 µs |
| Shooter starter | 3,768,320 bytes | 1.67 to 1.74 µs | 16.4 to 16.7 µs |
| 3D runner starter | 9,181,696 bytes | 1.43 to 1.64 µs | 1,165 to 1,184 µs |
| Scene demo | 4,096,000 bytes | 3.06 to 3.17 µs | 85.3 to 86.6 µs |

- Every game is smaller than after Step 1 there too (2,000 to 12,000
  bytes).
- GitHub gives a different machine each run (this one was about a fifth
  faster than Step 1's; the run before it, on the same code, was
  slower), so times are compared inside one run. The scene demo against
  the Platformer starter: after Step 1 it drew a frame in the same time
  (105 µs against 105); after Step 2 it takes 4% longer (86 against
  82). That agrees with the 6% seen on Linux. It's the compiler's
  arrangement of the same code, about 4 µs of a frame's 16,667, and
  it's written down here so the next steps are measured against it.
- That run's Studio tests found two tests changing the scene demo's file
  at the same moment (each saw the other's edit). They take turns now.


## Step 3: the other tools

**Every action is a command, and every change is one step of Undo.**
New: `object.add`, `object.turn`, `object.resize`, `object.order`,
`object.delete`, `object.duplicate`, `object.copy`, `object.paste`, and
`view.tool` `turn` and `size`. The Studio changes the file the same way
for all of them: read it with the engine's reader, change its objects
(`SceneFile::add`, `remove`, `move_before`, `set`), write it whole the
engine's one way, send it to the frozen game. Copy is the one command
that changes nothing, so it isn't a step.

**Turn and Size change the file, always.** Step 2's rule for a drag (the
file if the object is where it starts, this run only if the game has
moved it) is about *where* things are, because that's what a game
changes. Games almost never change an object's turn or size, so these
change how it starts, and the reload merge gives the frozen object the
same value (unless the game had changed that key itself). Nudging with
the arrow keys is a move, so it follows Step 2's rule.

**Both turn and grow round the object's `at`**, which is where the
engine turns it and what its anchor puts on the ground. So a crate made
bigger still stands on the ground, and a sign turned leans on its foot.
The ring is drawn round that point. For Size, the corners are handles; a
corner level with the feet (the bottom corners of something standing)
changes only the width, because there's no height between it and the
feet to stretch. Other editors keep the opposite corner still instead;
for a platformer, keeping the feet still is what you'd want almost
every time.

**What snaps.** Turning snaps to 15 degrees, the turn itself (not how
far it moved), so a sign at -8 lands on 0, 15, 30. With several
selected, the first snaps and the others turn by as much, keeping their
differences. Sizes snap to quarters (1, 1.25, 1.5, 2), never below a
quarter: pixel art looks right at those. Alt is free for both: whole
degrees, hundredths of a size. The grid's own snapping is for places.

**Turned objects are clicked where they're drawn.** A frozen picture now
also says what each object turns round and how far it's turned (and the
same for where it starts), so a click, a box and an outline all follow
the turned picture. That's four more fields in the link's JSON, with
defaults when they're missing, so the link's version is unchanged; and,
like the rest of it, only in builds with the `studio` feature.

**A box selects what it touches**, not only what's wholly inside: a box
round the middle of a pile gets the pile. It starts only from empty
space, so a drag on an object still moves it. Shift adds to what's
selected.

**Copy and paste go through Windows' clipboard too**, as lines of a scene
file. So objects can be pasted from one game into another, from the
Studio into a scene file in Notepad, and from Notepad into the Studio.
On Windows, Ctrl+C, Ctrl+X and Ctrl+V reach the app as Copy, Cut and
Paste rather than as keys, so that's what the Studio listens for. A paste
goes where the pointer is if it's over the game, or else one more grid
square to the right each time; copies go last in the file, drawn on top,
and are selected. An object the game made while playing can be copied
(as it is now) and pasted: that's one way to keep something the game
made. Names are kept as they are: names needn't be different (four coins
called "coin" are normal), and `scene.find` finds the first.

**Duplicate** puts each copy just after its own object and a grid
square to the right, so it's drawn at the same depth. **Delete** takes
objects out of the file; objects the game made aren't in the file, so
they're left, and the status bar says so.

**Reordering is the file's order.** That's the drawing order within a
layer and nothing else: objects update in the order of their ids (Step
1), so dragging a row never changes how the game plays. Moving an object
past one on another layer changes nothing you can see, which is right:
layers come first. The list's right-click menu has the four usual
moves, and rows can be dragged.

**Add offers the plain kinds** the design lists. Picture, Area,
Background and Tile map can be added now; the engine already draws
three of them, and an Area is a box the game can ask about, which the
Studio shows dashed because the game draws nothing. Text, Button and
Sound are listed, greyed, with the step that brings them (7, 7 and 4),
because an object that shows nothing and does nothing would only
confuse. The ready-made kinds (step 6) and the game's own (step 5) come
later; `object.add` already takes any kind name, for agents and for
games like the scene demo whose code knows "coin". A Picture or a
Background asks which picture; a Tile map asks which level (a `.txt`
with a `.legend.json` beside it). The new object goes in the middle of
what the view shows, on the grid, and is selected.

**The Inspector for several** shows what they share: picture, layer,
turn, size, and the three on-and-off keys (a box half-ticked when they
differ). A change sets it on all of them. Position isn't there: drag
them, or nudge them, and they move together.

**Small things.** Whatever Undo takes away is no longer selected. The
Scene list has a search box. Nudges in a row (less than a second and a
half apart, same selection) are one step of Undo, like a drag.

**Engine additions an agent might use:** `SceneFile::add`, `remove`,
`move_before`; `scene::line_text` (one object's line as a file writes
it); `scene::USUAL_LAYER`.

## Step 3: the numbers

Release builds with the usual features, on this Linux machine, Step 2's
code and Step 3's built the same way side by side:

| Game | Size after Step 2 | after Step 3 | The same, built as one piece |
|---|---|---|---|
| Platformer starter | 6,385,384 | 6,399,232 | +808 bytes |
| Shooter starter | 6,255,352 | 6,272,680 | +792 bytes |
| 3D runner starter | 11,591,352 | 11,605,104 | +800 bytes |
| Scene demo | 6,614,440 | 6,622,440 | +808 bytes |

Built as usual, every game is 8,000 to 17,000 bytes bigger (0.1 to
0.3%). None of that is Step 3's code: Step 3 changed only the engine's
scene files, and no new function reaches these games. The compiler
splits the engine into 16 parts to build them at once, and decides what
to copy into what (inlining) part by part. New code anywhere in the
engine moves where the parts are cut, so the window, picture-saving and
fuzzing code was arranged differently: 320 bytes more machine code, and
the rest is the names of 52 more pieces in the file's symbol table.
Built as one piece (`codegen-units = 1`, which takes the arrangement out
of it), every function in every game is byte for byte the same size as
after Step 2, and the files differ by 800 bytes of the tables used to
unwind after a crash. Step 2 measured the other way for the same reason
(every game a little smaller). Export, the phase after this one, is
where the games get built for sending; stripping the names and building
as one piece there would make them smaller than all of these.

Speed, with `--headless --time`, the two builds run turn about, lowest
and highest of three runs each:

| Game | Drawing a frame, Step 2 | Step 3 | An update, Step 2 | Step 3 |
|---|---|---|---|---|
| Platformer starter | 75.7 to 79.7 µs | 77.8 to 78.9 µs | 0.90 to 1.19 µs | 0.93 to 1.05 µs |
| Shooter starter | 17.4 to 19.2 µs | 17.3 to 17.8 µs | 0.95 to 1.08 µs | 0.96 to 1.00 µs |
| 3D runner starter | 1,445 to 1,492 µs | 1,417 to 1,470 µs | 1.73 to 2.11 µs | 1.52 to 2.07 µs |
| Scene demo | 74.1 to 77.3 µs | 73.4 to 75.4 µs | 2.02 to 2.27 µs | 1.88 to 1.97 µs |

The same speed for all four.

- Tests: the engine 615 (headless) and 650 (with a window), 344 with the
  link; the Studio's core 77 unit tests and 7 that play real games; the
  app 13 unit tests and 10 screen tests, two of which drive the scene
  editor with a pretend mouse and keyboard.

# Models from Duck Mold

Duck Mold (a sister tool: 3D models from text recipes, no window) saves
its models as `.glb` files. Until now Duck Forge couldn't load any model
file. Now `Mesh::load_glb(path)` loads a Duck Mold model as a mesh, with
its colours, ready to draw.

## What was built

- `src/glb.rs`: the reader, `Mesh::load_glb`, and `GlbModel` (the
  model's name, palette, parts and assemblies) with `GlbModel::load`,
  `to_mesh`, `part(name)` and `GlbPart::range`.
- `examples/turntable/`: the robot from Duck Mold's examples, facing
  the camera and slowly turning (Space stops it, Left/Right turn it by
  hand). Its recipe is in `art/`, its `.glb` in `assets/`.
- `tests/duck_mold_models.rs` and `tests/fixtures/duck_mold/`: Duck
  Mold's five golden models with the reports `duck-mold inspect --json`
  gave for them, checked against each other. A test in `src/gpu.rs`
  draws the robot on the GPU and the CPU and compares them.
- `Mesh::cube`'s faces are called near (-z) and far (+z) in its
  comments, not front and back; the guide says models face +z.

## Decisions made on the way

**The reader is Duck Mold's, copied byte for byte, not written again.**
Duck Mold ships a reader meant for this (`docs/forge/duck_mold_glb.rs`),
which its own tests run against every model it makes, and which shares
no code with its exporter. Everything between its two `// ====` marker
lines is the same in `glb.rs`, so both projects read the file the same
way, and the first marker names the reader's version (2). Duck Mold's
`docs/forge-handoff.md` has the one-line check that the two copies are
the same; a change starts there and is copied here. Outside the markers
are Duck Forge's own introduction and imports, and, after the reader,
the functions that tie it to the engine.

The first copy found two things, both now fixed in Duck Mold and so
here: sizes inside the file were added and multiplied without checks,
so a damaged file claiming more numbers (or a part claiming more
triangles) than any file could hold overflowed, a panic in a debug
build, instead of being refused (now tested on both sides); and a part
made of parts (a robot's arm) had no triangle range of its own. Duck
Mold now lists those *assemblies* in the file, each with the triangles of
every part inside it, so `model.part("left_arm")` recolours a whole arm.
Duck Mold regenerated its golden files for that, and the copies in
`tests/fixtures/duck_mold/` and `examples/turntable/assets/` are the new
ones.

**No new dependency.** The reader needs `serde_json`, which the engine
already uses for sprite sheets, and the standard library.
`tests/dependencies.rs` didn't change.

**`Mesh::load_glb` for the usual case, `GlbModel` for the rest.** Most
games want a mesh to draw: `Mesh::load_glb`, named like
`Sprite::load_png`. `GlbModel` is for the palette and the parts, kept out
of the prelude (`duckforge::glb::GlbModel`) since few games need it. The
mesh keeps the file's triangle order, so a part's `range()` picks out
its triangles in the mesh too, and recolouring a part (a team colour, a
hit flash on one arm) is a loop over `mesh.triangles[part.range()]`.
`part(name)` finds a part or an assembly: one name, whichever it is.

**Corners in the same place become one vertex.** Duck Mold's file gives
every face corners of its own (glTF needs that for flat shading); the
reader merges them, so the crate is 8 vertices and 12 triangles, like
`Mesh::cube`. Nothing is lost: Duck Forge lights each triangle by its own
direction, worked out from its corners.

**The example puts the camera in front, rather than turning the model.**
A model faces +z, and so does a camera with no turn: it would see the
robot's back. The turntable's camera stands on the robot's +z side
looking back at it, so the robot faces it with no turn, and the code has
no half turn for a reader to wonder about. The guide shows both ways.

**Near and far, not front and back.** Duck Mold decided a model faces
+z, so its front is +z and its own left is -x (a character's left arm is
its left). `Mesh::cube` called its -z face "front", because that's what
a camera with no turn sees. Only comments said so, and a cube looks the
same from both sides, but two words for one direction would confuse the
next person. The comments now say near (-z) and far (+z).

**Fixed on the way:** the guide's list of what the engine can't do still
said "no fog", though `canvas.set_fog` has existed since Phase 3.

## The numbers

| | |
|---|---|
| Loading the robot (384 triangles, 35 KB), 200 times in a row | 0.13 ms each in a plain build, 0.08 ms with `--release` |
| Duck Mold's five models | the triangles, size, box, and triangles per palette colour that `duck-mold inspect` reported, for all five; every part a closed solid facing outwards; the crate exactly 1 cubic metre |
| Facing, drawn | the house's door seen only from its +z side, its chimney (on its own left, -x) on your right as you face it; the robot's eyes seen from in front, not from a camera with no turn until the robot is turned half round |
| Engine tests on Linux | 607 pass with everything on, 572 headless (`--no-default-features`); none fail. The GPU tests ran on a software driver, not skipped |
| GPU against CPU (Mesa's software Vulkan driver, lavapipe) | the turntable's first frame and frame 200: the same PNG, byte for byte; the robot under a coloured sun: within the GPU tests' usual 2% |

# Sockets from Duck Mold

Duck Mold's Phase 1 keeps a model's *sockets* in its `.glb`: the named
points, each facing a way, where its recipe joined its parts. A game can
now put things where the model says: a hat on `head.top`, a sword in
`right_arm.grip`.

## What was built

- The reader went to Duck Mold's version 3, copied between the markers
  byte for byte as before: `GlbModel.sockets`, none for older files, a
  damaged entry refused with a sentence.
- Duck Forge's own side: `GlbModel::socket(name)`, and
  `GlbSocket::position()` and `direction()` as `Vec3`s.
- `examples/turntable/`: the robot wears a top hat, put on its
  `head.top` at run time and moved and turned with it. A test checks the
  hat is centred on top of the robot, and still is half a turn later.
- `tests/fixtures/duck_mold/`: Duck Mold's six golden models (a snowman
  joined, and the house got a skylight), and `robot.sockets.json`, its
  list of the robot's 187 sockets, which a test checks `socket()` finds
  where it says.

## Decisions made on the way

**`position()` and `direction()` are in the model's own space.** The
same space as the mesh's corners, so the transform the model is drawn
with moves a socket the same way: `place.apply(socket.position())`.
Nothing in the engine had to learn about sockets.

**Names are Duck Mold's.** `head.top` is the part `head`'s built-in
`top`; a part inside an assembly keeps its path (`left_arm/hand.bottom`),
an assembly's own socket follows its name (`left_arm.grip`), and the
whole model's are bare (`top`). Unnamed parts have none in the file. One
lookup, `socket(name)`, takes them all, as `part(name)` takes parts and
assemblies; `duck-mold inspect --sockets` lists every name.

**The hat is a top hat.** The robot's antenna stands on `head.top` too,
so a hat there must cover it: the crown is taller than the antenna.

## The numbers

| | |
|---|---|
| The robot's file | 35.7 KB before sockets, 50 KB with its 187 |
| Engine tests on Linux | 620 pass with everything on, 577 headless (`--no-default-features`); none fail |
