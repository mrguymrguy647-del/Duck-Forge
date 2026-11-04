# The input script format

An input script is a playthrough written down: which keys were pressed and
let go, where the mouse moved, on which frame. duckforge writes one when
you record a game (F10 in the window, or `--record FILE`), and replays one
with `--inputs FILE`, in a window or with `--headless`.

Scripts are plain JSON, so people and programs can write them by hand too.

## An example

```json
{
  "duckforge_inputs": 1,
  "seed": 7,
  "updates_per_second": 60,
  "frames": 600,
  "events": [
    {"frame": 1, "press": "Space"},
    {"frame": 2, "release": "Space"},
    {"frame": 30, "press": "Right"},
    {"frame": 90, "release": "Right"},
    {"frame": 120, "mouse": [160, 120]},
    {"frame": 121, "mouse_press": "Left"},
    {"frame": 125, "mouse_release": "Left"},
    {"frame": 200, "scroll": -1}
  ]
}
```

This presses Space for one frame (a tap), holds the right arrow for one
second (60 frames), moves the mouse to the middle of a 320 x 240 canvas
and clicks there, then rolls the scroll wheel down once.

## The fields

Only `"events"` is required.

| Field | What it is |
|-------|------------|
| `"duckforge_inputs"` | The format version. Always `1` for now. |
| `"seed"` | The random seed the game ran with. A replay uses it, so random things (like Breakout's serve angle) happen the same way. `--seed` on the command line overrides it. |
| `"updates_per_second"` | The game's timestep when this was recorded. If the game now runs at a different rate, duckforge warns that the replay won't match. |
| `"frames"` | How long the recording lasted. `--headless` runs this many frames unless `--frames` says otherwise. Without either, it runs until two seconds after the last event (600 frames if there are no events). If the run stops before some events, the summary warns how many were skipped. |
| `"events"` | The list of events, described below. |

## Events

Every event is an object with a `"frame"` and exactly **one** action:

| Action | Example | What it does |
|--------|---------|--------------|
| `"press"` | `{"frame": 10, "press": "Space"}` | Starts holding a key. |
| `"release"` | `{"frame": 11, "release": "Space"}` | Lets go of a key. |
| `"mouse"` | `{"frame": 10, "mouse": [160, 120]}` | Moves the pointer to `[x, y]` in canvas pixels (the same coordinates the game draws with; fractions are fine). |
| `"mouse_press"` | `{"frame": 10, "mouse_press": "Left"}` | Starts holding a mouse button: `"Left"`, `"Right"` or `"Middle"`. |
| `"mouse_release"` | `{"frame": 12, "mouse_release": "Left"}` | Lets go of a mouse button. |
| `"scroll"` | `{"frame": 10, "scroll": 1}` | Rolls the wheel for one frame: positive is up (away from you), negative is down. |

### What "frame" means

Frames count from 1, exactly like `ctx.frame()` in the game: an event on
frame 10 is seen by the game's tenth `update()`. With the default
timestep there are 60 frames per second, so frame 600 is ten seconds in.

- **A tap** is a press on one frame and a release on a later frame. The
  game sees `was_pressed` on the press frame. (A press and a release on the
  *same* frame cancel out: the game never sees the key.)
- **Holding** a key means pressing it and releasing it later: every frame
  in between, `is_down` is true.
- Events can be listed in any order; they're sorted by frame when the
  script is read. Events on the same frame happen in the order written.
- Keys still held when the script ends stay held. (In a window, your own
  keyboard and mouse take over when the script ends.)

## Key names

Names are written as in the engine's `Key` type, and upper or lower case
both work (`"space"` is fine):

`A` to `Z`, `Digit0` to `Digit9` (the number keys along the top),
`Left`, `Right`, `Up`, `Down`, `Space`, `Enter`, `Escape`, `Tab`,
`Backspace`, `LeftShift`, `RightShift`, `LeftCtrl`, `RightCtrl`,
`LeftAlt`, `RightAlt`, and `F1` to `F12`.

F1, F3, F10 and F12 are the engine's own keys in a window (debug overlay, FPS counter,
recording, screenshots). They're never recorded, so a replay won't start
recording or take screenshots.

## Mistakes

duckforge refuses a script with a mistake rather than guessing, and says
what's wrong and where:

```text
Error: inputs.json: event 3 ({"frame":30,"press":"Spcae"}): unknown key "Spcae". Key names look like "Space", "Left", "A", "Digit1" or "F5"; docs/input-format.md lists them all
```

Unknown fields are mistakes too (`"preess"` instead of `"press"`), so a
typo can't be silently ignored.

## Recording

- **F10** in any game's window starts recording, and pressing it again
  saves `recording-1.json` (then `recording-2.json`, and so on) in the
  folder you ran the game from. The window title shows `[REC]` meanwhile.
- **`--record FILE`** records from the start and saves to FILE when the
  window closes (or when you press F10).

A replay always starts from the very beginning of the game, so a recording
holds **everything since the game started**, not just since F10: the
moment you pressed F10 depends on everything before it. Closing the window
while recording saves the recording; it's never lost.

## Replaying

```sh
# Watch it in a window (the title shows [REPLAY]; your keys are ignored
# until the script ends, then you take over):
cargo run --example breakout -- --inputs recording-1.json

# Or with no window, as fast as possible, saving the last frame:
cargo run --example breakout -- --headless --inputs recording-1.json --out replay
```

A replay is exact, down to the last pixel, as long as the game, the
timestep and the seed are the same (and the game doesn't use its own clock
or other randomness: see `docs/decisions.md`). From code,
`InputScript::load(path)` reads a script, and `Simulation::follow(script)`
or `HeadlessOptions { inputs: Some(script), .. }` replays it.
