# Design decisions

This file explains, in plain language, the choices made while teaching
duckforge to run games without a screen, and why. Each step of that work
has its own section, oldest first.

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
