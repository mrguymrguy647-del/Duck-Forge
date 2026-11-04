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
