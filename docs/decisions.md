# Design decisions

This file explains, in plain language, the choices made while building
duckforge, and why. Each step has its own section, oldest first.

The first part (Steps 1 to 6) taught duckforge to run games without a
screen. The second part ("Phase 2") makes it comfortable for two kinds of
game makers: a person learning Rust, and an AI agent building and testing
games headless.

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

# Phase 2: comfortable for people and AI agents

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
