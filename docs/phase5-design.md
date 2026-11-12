# Phase 5 design: a real editor

Duck Forge Studio gets what it lacked next to Godot: a Files panel, a
Scene list, an Inspector, tools to move, turn and resize things, and a
Code panel. Underneath, games get **objects stored in scene files** that
the engine and the Studio both read.

This is Step 0: the plan, for review. Nothing is built yet. The pictures
are drawings made for this document, not the real app. Numbers marked
*measured* were measured for this document.

Contents: [the screens](#the-screens), then the ten questions in order:
[1 the scene file](#1-the-scene-file),
[2 code and scenes](#2-code-and-scenes-together),
[3 games without code](#3-games-without-code),
[4 the panels](#4-the-editors-panels),
[5 several scenes](#5-several-scenes-in-one-game),
[6 3D](#6-2d-first-then-3d),
[7 Beginner and Expert](#7-beginner-and-expert),
[8 what Duck Forge keeps](#8-what-duck-forge-keeps-that-godot-doesnt-have),
[9 the build steps](#9-the-build-steps),
[10 what's cut](#10-what-id-cut-or-postpone), then
[the rules and how they're measured](#the-rules-and-how-theyre-measured)
and [what I need you to decide](#what-i-need-you-to-decide).

## The short version

- **A scene is one plain-text file** (`scenes/level1.json`): a list of
  objects, one per line. Each has a name, a kind, a position and its own
  settings. A tile map is one kind of object, pointing at the same
  `level.txt` as today.
- **A kind is what an object does.** Duck Forge brings ready-made kinds
  (platformer player, coin, enemy patrol, door...). A game adds its own
  in Rust, in about fifteen lines, and their settings appear in the
  Inspector with no extra work.
- **A game needs no code** to use scenes: its `main.rs` is six lines the
  starter writes. "Turn into code" gives the game its own copy of a
  ready-made behaviour whenever you want to change it.
- **Games made in code keep working exactly as now.** They don't have to
  use scenes, and the Studio still gives them Files, Feel and the Code
  panel. They don't become editable by magic, though: see
  [section 10](#10-what-id-cut-or-postpone).
- **The frozen game stays the editor's picture.** You still freeze in
  mid-jump, change something, and play on from that frame.
- It's built in twelve small steps, each ending in a STOP and a hand
  checklist. Six need the stronger model throughout, three (and the
  final proof) can be built by a cheaper one from this design, and
  three are split between them.

## The screens

**New game.** Starters made with scenes come first; the starters made in
code are all still there. You pick Beginner or Expert here.

![New game](phase5-mockups/01-new-game.png)

**The editor, in Beginner.** Scene and Files on the left, the frozen
game in the middle, the Inspector on the right. The snail is selected:
the arrows move it, and the dashed line is its patrol, drawn from its
own settings.

![The editor in Beginner](phase5-mockups/02-editor-beginner.png)

**The editor, in Expert.** The same files and the same screen, with
every setting, the object's id and line in the file, and the Code and
Console panels. The Code panel shows the last build's mistake on its
line.

![The editor in Expert](phase5-mockups/03-editor-expert.png)

**Dragging a picture in.** From Files into the scene it becomes a new
object, snapped to the grid. Onto a Picture box in the Inspector it
becomes that object's picture. Sounds work the same way.

![Dragging a picture from Files](phase5-mockups/04-drag-a-picture-in.png)

**Frozen in mid-play.** The lift has moved since the scene started, so
you see it twice: itself, and an outline where it starts. The Inspector
says "Starts at" and "Now". The coin the duck took is still in the list,
in italics.

![Frozen in mid-play](phase5-mockups/05-frozen-in-mid-play.png)

**Add an object.** Ready-made kinds, plain kinds, and the game's own
kinds and saved things.

![Add an object](phase5-mockups/06-add-an-object.png)

**A tile map is one object.** Select it and you get today's paint tools
and palette, on the same `level.txt`.

![A tile map selected](phase5-mockups/07-tile-map-selected.png)

**The game's scenes.** Every scene, and how the game moves between them.
The arrows aren't drawn by hand: they're found in the scenes' doors,
buttons and rules.

![The game's scenes](phase5-mockups/08-scenes.png)

**Rules.** "When ... do ..." built from lists, stored as one readable
line each.

![Rules](phase5-mockups/09-rules.png)

**Turn into code.** It says exactly what it will write, shows the code
first, and Undo puts everything back.

![Turn into code](phase5-mockups/10-turn-into-code.png)

**Check understands scenes.** It names the scene and the object, shows
the spot, and "Show it in the scene" opens it selected.

![Check](phase5-mockups/11-check.png)

**The same panels in 3D.** The Studio flies its own camera through the
frozen moment. Positions have three numbers. The Model box is the slot
for Modular 3D's files.

![3D](phase5-mockups/12-3d.png)

**A game made in code.** No scene, so nothing to drag, and the Studio
says so. It lists what the game drew, and Files, Feel and the Code
panel all work.

![A game made in code](phase5-mockups/13-a-game-made-in-code.png)

## 1. The scene file

**JSON, one object per line**, in the game's own folder:

```text
examples/bee_maze/
  game.json                 the game: title, size, first scene, numbers
  scenes/level1.json        a scene: its objects and rules
  scenes/level1.txt         a tile map's letters, exactly as today
  scenes/level1.legend.json what the letters mean, exactly as today
  things/fast_snail.json    an object saved to use again (optional)
  assets/                   pictures and sounds
  main.rs                   six lines, or the whole game, as you like
```

```json
{
  "duckforge_scene": 1,
  "size": [1280, 240],
  "background": "#6ab4f0",
  "layers": ["far", "back", "world", "front", {"name": "hud", "stays_on_screen": true}],
  "next": "level2",
  "objects": [
    {"id": 1, "name": "sky", "kind": "background", "layer": "far", "picture": "assets/sky.png", "parallax": 0.2},
    {"id": 3, "name": "ground", "kind": "tilemap", "level": "scenes/level1.txt"},
    {"id": 4, "name": "duck", "kind": "platformer_player", "at": [40, 208], "picture": "assets/hero.json", "jump_speed": 330},
    {"id": 7, "name": "snail", "kind": "patrol", "at": [496, 208], "picture": "assets/snail.png", "speed": 30, "walks": 48},
    {"id": 8, "name": "lift", "kind": "moving_platform", "at": [304, 150], "picture": "assets/tiles.json", "animation": "plank", "moves": "up_down", "how_far": 64},
    {"id": 9, "name": "door", "kind": "door", "at": [1200, 208], "picture": "assets/door.png", "goes_to": "level2"},
    {"id": 10, "name": "sign", "at": [374, 208], "picture": "assets/sign.png"},
    {"id": 11, "name": "music", "kind": "sound", "sound": "assets/music.wav", "loops": true},
    {"id": 12, "name": "camera", "kind": "camera_follow", "target": "duck"}
  ],
  "rules": [
    "when duck touches coin: add 1 to score, remove coin, play assets/coin.wav",
    "when lives reaches 0: go to game over"
  ]
}
```

Why JSON: every other Duck Forge file is JSON already (legends, Feel,
tune, input scripts), the engine already reads it with no new library,
and agents write it without mistakes. The Studio always writes it the
same way (one object per line, keys in a fixed order, only what differs
from the default), so a change to one object is a change to one line.
Any JSON layout a person or agent writes by hand reads fine.

**What every object has:**

| Key | Meaning | If left out |
|---|---|---|
| `id` | A number that never changes and is never used again in this scene. The Studio, replays and Check tell objects apart by it, even after a rename | The Studio gives one |
| `name` | What you and the code call it. Several objects can share one ("coin") | The kind's name |
| `kind` | What it does: a ready-made kind or one of the game's own | `picture`: it's drawn and does nothing |
| `at` | Where its feet are, in the game's pixels: `[x, y]`, y downwards as everywhere in the engine | `[0, 0]` |
| `turn` | Degrees clockwise | 0 |
| `size` | How many times bigger: `2`, or `[2, 1]` | 1 |
| `flip` | Mirrored left to right | false |
| `picture` | A `.png`, or a Duck Quill sheet (`.json`) | none: nothing is drawn |
| `animation` | Which animation of the sheet | the first |
| `layer` | One of the scene's layers. Layers are drawn in the list's order; inside one, objects are drawn in the file's order | `world` |
| `shown`, `tint`, `opacity` | As in `Blit` today | shown, white, 1 |
| `box` | Its collision box, `[left, top, width, height]` inside its picture | the whole picture |
| `solid` | Other objects can't pass through it | false |
| `like` | A saved thing to start from: `"like": "fast_snail"` | none |

"Its feet" is the bottom middle of its picture, or the pivot a Duck
Quill sheet gives. That's where tile maps put things today, so an object
snapped to the grid stands on the tile under it.

**Its own settings** are every other key (`speed`, `walks`,
`goes_to`). A kind can't use one of the names above for a setting; the
engine says so plainly if it tries. Keys the engine doesn't know are
kept when the Studio saves, so a setting of a kind that isn't built yet,
or a note you added, survives.

**A saved thing** (`things/fast_snail.json`) is one object without an id
or a position. An object that says `"like": "fast_snail"` starts from it
and writes only what it changes. Change the thing and every object like
it changes. Spawners make copies of saved things.

**A tile map is one kind of object**: `{"kind": "tilemap", "level":
"scenes/level1.txt"}`. The engine reads it with `TileMap::load_level`,
the same code as today, and the legend file is found next to it as now.
Several tile maps in one scene are several layers of tiles.

- The legend's "thing" letters still work. Each painted thing becomes an
  object when the scene loads: like the saved thing of that name if
  there is one, or else of the kind of that name, or else a plain named
  object. So painting thirty coins on the grid stays as quick as it is
  now, and placed objects sit beside them.
- **Today's `level.txt` games don't change at all.** They don't have a
  scene file and don't need one. Their code, their files and their
  playthroughs stay as they are. The Studio shows them with the same
  panels: the tile map and its painted things listed in the Scene panel,
  and the paint tools as now.

**Paths** inside a scene are from the game's folder. **Colours** are
`"#rrggbb"`. **A mistake in the file** is one sentence with the line:
"scenes/level1.json, line 9: the kind 'patroll' isn't known. Did you
mean 'patrol'?" The game still starts, with that object drawn as a
marked box, so one typo never loses the scene.

## 2. Code and scenes together

Two rules decide everything here: **nothing possible in code becomes
impossible**, and **nothing made in the editor is locked away from
code**.

**A scene is a value your code holds.** The `Game` trait doesn't change.
A game that wants a scene loads one:

```rust
struct BeeMaze { scene: Scene, score: u32 }

impl Game for BeeMaze {
    fn update(&mut self, ctx: &mut Context) {
        self.scene.update(ctx);                       // every object does its thing
        let duck = self.scene.find("duck").unwrap();
        for coin in self.scene.touching(duck, "coin") {
            self.scene.remove(coin);
            self.score += 1;
            ctx.play(Sound::coin());
        }
    }
    fn draw(&self, canvas: &mut Canvas) {
        self.scene.draw(canvas);                      // layers, in order
        canvas.draw_text(&format!("Coins {}", self.score), vec2(4.0, 4.0), 1.0, Color::WHITE);
    }
    fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool {
        self.scene.reload(ctx, file)                  // the Studio's edits arrive here
    }
}
```

`Scene::load("examples/bee_maze/scenes/level1.json")?` reads it. What
code can do with one:

| | |
|---|---|
| Find | `scene.find("duck")` (the first of that name), `scene.all("coin")`, `scene.of_kind("patrol")`, `scene.ids()` |
| Read and change | `scene[id].at`, `.turn`, `.size`, `.picture`, `.layer`, `.shown`... every key in the file is a field. `scene.setting(id, "speed")`, `scene.set_setting(id, "speed", 60.0)` |
| Add and remove | `scene.add(Object::new("coin").at(vec2(120.0, 96.0)))`, `scene.add_like("fast_snail", at)`, `scene.remove(id)` |
| Touching | `scene.touching(id, "coin")` (by name or kind), `scene.overlaps(a, b)`, `scene.at_point(point)` |
| Moving | `scene.move_and_slide(id, movement)` gives today's `Contacts`: it stops at tile maps and solid objects |
| The rest | `scene.tilemap()` (today's `TileMap`), `scene.camera` (today's `Camera2D`), `scene.go_to("level2")`, `scene.number("score")`, `scene.draw_layer(canvas, "front")` to draw your own things between layers |

Objects made by code while playing (bullets, sparks) are ordinary
objects that just aren't in the file. Objects update in the file's
order, so the same inputs give the same game.

**A new kind, in Rust:**

```rust
#[derive(Default)]
pub struct Bee { speed: f32, bob: f32, home_y: f32, time: f32 }

impl Kind for Bee {
    fn settings(&mut self, s: &mut Settings) {
        s.number("speed", &mut self.speed, 40.0).range(0.0, 200.0);
        s.number("bob", &mut self.bob, 12.0).label("Bobs up and down by");
    }
    fn start(&mut self, me: Id, scene: &mut Scene, _ctx: &mut Context) {
        self.home_y = scene[me].at.y;
    }
    fn update(&mut self, me: Id, scene: &mut Scene, ctx: &mut Context) {
        self.time += ctx.dt();
        scene[me].at.x += self.speed * ctx.dt();
        scene[me].at.y = self.home_y + (self.time * 4.0).sin() * self.bob;
    }
}
```

and one line where the game starts: `game.kind::<Bee>("bee")`.

**Why its settings appear in the Inspector by themselves.** `settings`
is the one place a kind names its settings, and the engine calls it for
three jobs with the same code:

1. *To list them:* the engine runs it on an empty `Bee` and notes each
   name, its type, its default and its range. The Studio asks the
   running game for that list (`ask kinds`, over the link that exists)
   and draws the boxes. No list to keep in step, nothing to register
   twice.
2. *To load them:* the values from the scene file go into the fields.
3. *When you change one while the game is frozen:* the same call again,
   so `speed` changes and `time` (which isn't a setting) is kept.

It's the same idea as `ctx.tune`: the code asks for a value by name and
gives its default. It needs no new library and no "derive" magic, so the
first build stays as quick as it is.

The boxes a setting can be: a number (a slider when it has a range), a
whole number, a tick box, words, a choice from a list, a colour, a
picture, a sound, a point you can drag in the view, another object, a
scene, a saved thing, a key. Each can have `.label("...")`,
`.help("...")`, `.group("...")` and `.expert()` (hidden in Beginner).

The Studio keeps the last list it was given, so when the code doesn't
build the Inspector still works from the build before, and says so.

A kind can also draw itself (`fn draw(&self, me: &Object, canvas)`; the
default draws its picture) and code can reach a kind's own fields
(`scene.kind::<Bee>(id)`).

**The engine's old `Scene`.** The engine already has a type called
`Scene`: "what was on screen", made from `canvas.label` and saved as
`last.scene.json`. It becomes `OnScreen`. `sim.scene()` keeps working,
the saved files keep their names, and no example or test names the type
(*checked*). Objects in a scene label themselves, so `--describe` and
`sim.scene()` list them with no code.

## 3. Games without code

**A game made with scenes has this `main.rs`,** written by the starter:

```rust
use duckforge::prelude::*;

fn main() {
    let mut game = SceneGame::new("examples/bee_maze");   // reads game.json
    game.run();
}
```

`SceneGame` is an ordinary `Game` the engine brings: it holds the
current scene, the game's numbers, and moves between scenes. It still
has to be built by Rust like any game, which the Studio already does by
itself. You never have to open the file.

**Ready-made behaviours.** Each is a kind, with settings in the
Inspector:

| Kind | What it does | Main settings |
|---|---|---|
| Platformer player | Runs, jumps, falls, lands on one-way planks | Run speed, jump height, gravity, keys, which animations |
| Top-down player | Walks in four or eight directions | Speed, keys |
| Coin | Taken when the player touches it | Adds how much to which number, sound |
| Enemy patrol | Walks back and forth, turns at walls and edges | Speed, how far, hurts whom, what happens when jumped on |
| Moving platform | Up and down or side to side; carries what stands on it | Direction, how far, speed, wait |
| Door | Goes to another scene, or to another door | Goes to, needs which number (a key) |
| Spawner | Makes copies of a saved thing | Which thing, how often, at most how many |
| Camera follow | Keeps an object on screen | Target, smoothing, stays inside the scene |

Plain kinds that do nothing by themselves: Picture, Text, Button, Sound,
Area (an invisible box), Tile map, Background.

Each ready-made kind is one short file in the engine, written only with
what any game can use (the table in section 2). That's what makes "Turn
into code" honest.

**Rules**, for what the behaviours don't cover, are one sentence each:

```text
when duck touches coin: add 1 to score, remove coin, play assets/coin.wav
when duck touches water: take 1 from lives, restart
when lives reaches 0: go to game over
when no coin is left: show door, say The door is open!
when every 5 seconds: make fast_snail at spawner
```

- *When:* A touches B; a key is pressed; a number reaches a value; none
  of a name is left; every so many seconds; the scene starts; an object
  leaves the scene; a button is clicked.
- *Do:* add to, take from or set a number; remove, show or hide an
  object; go to a scene, or restart this one; play a sound; make a saved
  thing appear; move an object; change a setting; say words for a
  moment.

The Studio builds them from lists (the Rules picture above) and stores
the sentence. A sentence the engine can't read is one plain message with
the words it does know. Rules live in a scene, or in `game.json` for the
whole game.

**Both turn into Rust.**

- *A behaviour:* "Turn into code" copies the ready-made kind's own file
  into the game's folder under a new name, adds two lines to `main.rs`,
  and changes the object's kind. Same settings, same picture, and a test
  replays the last run to show the game plays the same. From then on
  it's the game's code.
- *Rules:* "Turn into code" writes each sentence as the few lines of
  Rust it stands for, in `rules.rs`, and takes the sentences out of the
  scene.
- It's one way. Going back is Undo or History, not a button.

**What's in Phase 5 and what waits:**

| In Phase 5 | Waits |
|---|---|
| The eight behaviours and the plain kinds | More behaviours (shooter ship, bullets, health bars, checkpoints, dialogue) |
| Turn a behaviour into code | Several behaviours on one object |
| Saved things | Rules with "and", "or", "else" or sums |
| Rules, as the **last** 2D step, and the first thing I'd move out if the phase runs long | Animation driven by rules; a timeline |

## 4. The editor's panels

One screen, as in the pictures. Everything is still the frozen game in
the middle.

| Panel | Where | What it does |
|---|---|---|
| **Scene** | Top left | The scene's objects in drawing order, with their kind. Click to select, drag to reorder, double-click to rename, a search box, **+ Add** |
| **Files** | Bottom left | The game's folder as a tree (built in the first-hour fixes). Click a picture to see it, a sound to hear it, a scene to open it, code to open it in the Code panel |
| **View** | Middle | The frozen game, with the Studio's own drawing on top: the selection, handles, the grid |
| **Inspector** | Right | The selected object's settings. Tabs for Feel (today's sliders) and Rules |
| **Code, Console, Problems** | Bottom, on request | See below |

**The view's tools** (the bar above the view, and one key each):

- **Select** (V): click an object; click again on a pile to get the one
  underneath; Shift+click adds; drag on empty space for a box.
- **Move** (G): drag the object, or an arrow to keep to one direction.
  Arrow keys nudge by one pixel, with Shift by one grid square.
- **Turn** (R): drag round the ring; snaps to 15 degrees.
- **Size** (S): drag a corner; Shift keeps the shape.
- **Snap** and **Grid**: on by default, to 8 pixels or to the tile map's
  squares. Positions are whole pixels unless you type otherwise. Alt
  places freely.
- **Frozen moment / As it starts**: see "Starts at and Now" below.
- Zoom, pan (middle mouse or Space+drag) and Whole scene, as Build mode
  has now.
- Selecting a tile map swaps the tools for today's Paint, Erase, Fill,
  Select area and Play from here.

Ctrl+C, Ctrl+V, Ctrl+D (duplicate), Delete, Ctrl+A, Ctrl+Z, Ctrl+Y all
work on the selection, one object or many. With several selected, the
Inspector shows the settings they share; changing one changes them all.

**How the view is drawn and clicked.** As now, the picture is the
game's own drawing of the frozen moment, so Build and Play can't look
different. Objects label themselves, so the Studio asks the game where
each one is on screen and does the clicking itself. While you drag, the
Studio draws the outline under the pointer at once and sends the change
to the frozen game a few times a second, as painting does now.

**Dragging from Files:**

| Drag | Onto | What happens |
|---|---|---|
| A picture, or a Duck Quill sheet | the scene | A new Picture object there, named after the file |
| A picture | a Picture box | That object gets the picture |
| A sound | the scene | A new Sound object (music for the scene) |
| A sound | a Sound box | That setting gets the sound |
| A level's `.txt` | the scene | A new Tile map |
| A saved thing | the scene | An object like it |
| A scene | a Door's or Button's "Goes to" | It goes there |
| Files from Windows | the scene | Copied into `assets/` first, then as above |

**Starts at and Now.** This is the one place the editor differs from
Godot's, because the picture is a game in mid-play. Every object has the
values the file gives it (how it starts) and the values it has right
now. For most objects they're the same, and there's nothing to think
about. When they differ:

- An object that's moved since the start (the duck, a snail, a lift)
  shows twice: itself, and an outline where it starts. Drag the
  **outline** to change the file. Drag the **object** to move it for
  this run only.
- A setting changed in the Inspector changes the file and the frozen
  game together, unless the game itself has changed that value since the
  start (lives went from 3 to 1). Then the Inspector shows both, and you
  choose which to change.
- An object the game has removed (a coin that was taken) stays in the
  Scene list in italics, so you can still select and edit it.
- **As it starts** shows the scene as the file says, with nothing moved:
  the plain "editor view". It's also how you look at a scene the game
  isn't in right now.

Either way you press Tab and play on from the same frame.

**The Code panel:**

- Opens the game's `.rs` files in tabs, with line numbers, Rust
  colouring, Find (Ctrl+F) and Go to line.
- Ctrl+S saves, and the save starts the build the Studio already does. A
  mistake shows on its line, in the same plain words as now, with "Copy
  this for an agent".
- "Show me the code" on an object, a kind or a setting opens the right
  file at the right line.
- "Open in my editor" stays, for anyone with VS Code.
- It is a plain editor. No suggestions as you type, no "go to
  definition", no debugger. I say why in section 10.

**Undo covers everything** done to scenes, levels, Feel and rules, as
one list, as now. Code has its own undo inside the Code panel, and
History's snapshots cover code too. Selecting something isn't an Undo
step.

**Every button is a named command**, in the table the Studio already
has, so agents and `--run` scripts can press all of them:

| Group | Commands |
|---|---|
| scene | `open`, `new`, `rename`, `duplicate`, `delete`, `set`, `play_from` |
| object | `add`, `set`, `move`, `turn`, `resize`, `rename`, `order`, `delete`, `duplicate`, `copy`, `paste`, `save_as_thing`, `turn_into_code`, `move_now` |
| select | `set`, `add`, `all`, `none` |
| view | `tool`, `snap`, `grid`, `show` (frozen moment or start), `zoom`, `pan`, `whole`, `fly` |
| rule | `add`, `set`, `delete`, `turn_into_code` |
| kind | `new` (writes a kind's skeleton in code) |
| code | `open`, `save`, `find`, `go_to` |
| files | the ones from the first-hour fixes, plus `use` (a file dropped on the scene or on a box) |
| studio | `mode` (Beginner or Expert) |

```json
{"do": "object.add", "kind": "patrol", "name": "snail", "at": [496, 208]}
{"do": "object.set", "ids": [7], "set": {"speed": 60}}
{"do": "files.use", "path": "assets/sign.png", "at": [128, 208]}
```

## 5. Several scenes in one game

```json
{ "duckforge_game": 1, "title": "Bee Maze", "width": 320, "height": 240,
  "first_scene": "title",
  "numbers": { "score": {"starts": 0, "kept": true},
               "lives": {"starts": 3, "kept": true},
               "coins": {"starts": 0} },
  "rules": ["when lives reaches 0: go to game over"] }
```

- **The scenes are the files in `scenes/`.** There's no second list to
  keep in step. `first_scene` says where the game starts, and a scene's
  `"next"` says what comes after it.
- **Moving between them:** a Door or a Button with "Goes to", a rule
  (`go to level2`, `go to next`, `restart`), or code
  (`scene.go_to("level2")`). The move happens between two updates, so a
  replay sees the same thing.
- **Numbers** (`score`, `lives`) belong to the game. "Kept" ones carry
  from scene to scene; the others start again in each scene. Behaviours,
  rules and code all read and change the same numbers
  (`scene.number("score")`).
- **A title screen** is a scene with Text and a Button. **A level
  select** is a scene with one Button per level; a Button can be locked
  until a level has been won, which the game keeps count of itself.
  "New scene" offers Empty, Title screen, Level select, and a copy of
  the scene you're in.
- **In the Studio:** a scene chooser in the top bar, and the Scenes page
  (the picture above) showing how they connect. "Play this scene" starts
  the game in that scene, as Play from here does for a tile.
- **Check** says when a scene can't be reached from the first one, and
  when something goes to a scene that isn't there.

Saving progress to disk (so level 3 is still unlocked tomorrow) waits:
it needs the save-file rules the engine already has for Duck Craft
(`ctx.is_headless()`), and a place to keep saves.

## 6. 2D first, then 3D

Everything above is built for 2D first. The same file and the same
panels carry 3D with three differences:

```json
{ "duckforge_scene": 1, "space": "3d",
  "objects": [
    {"id": 1, "name": "sun", "kind": "light", "direction": [0.4, -1, 0.3], "ambient": 0.35},
    {"id": 2, "name": "crate", "kind": "mesh", "shape": "cube", "at": [-1.5, 0.5, 6], "turn": [0, 20, 0], "picture": "assets/crate.png", "solid": true},
    {"id": 3, "name": "island", "kind": "blocks", "world": "scenes/island.blocks.txt"},
    {"id": 4, "name": "duck", "kind": "picture", "at": [0, 0, 4], "picture": "assets/duck.json", "faces_camera": true},
    {"id": 5, "name": "tree", "kind": "mesh", "model": "assets/tree.glb", "at": [3, 0, 9]},
    {"id": 6, "name": "camera", "kind": "camera", "follows": "duck"} ] }
```

1. **Three numbers** for `at`, `turn` and `size` (y is up, as in the
   engine's 3D).
2. **3D kinds:** Mesh (the engine's own shapes: cube, ball, cylinder,
   cone, pyramid, flat), Blocks (a block world, see below), Picture that
   faces the camera (today's billboard), Light, Camera.
3. **A fly camera in the frozen game.** Hold the right mouse button and
   use W A S D, as in Godot and Unity. The link's `view` message already
   asks the frozen game to draw the same moment through the Studio's
   camera; in 3D it carries a 3D camera. The Studio works out what you
   clicked with the engine's own camera maths, and draws three arrows to
   move along.

**A block world as a file** follows tile maps: `island.blocks.txt` is
layers of letters, bottom layer first, with a legend that says which
block each letter is. That's plain text an agent can write and git can
compare. Painting blocks by clicking in the editor is its own job and
isn't in this phase.

**3D models: the format I propose is glTF 2.0, as one `.glb` file.**

- It's the standard every 3D tool reads and writes (Blender, Windows'
  own 3D viewer), so a model from Modular 3D can be checked anywhere,
  and models from elsewhere can come in.
- The engine can read it with what it already has (`serde_json` and
  `png`). No new library.
- Duck Forge would read a stated part of it, and say plainly about the
  rest:

| | Read | Not read (the model still loads, with a note) |
|---|---|---|
| Shapes | Triangles with positions, normals, one set of texture points, vertex colours | Lines, points, morph targets |
| Looks | One base colour and one base colour picture (a PNG inside the file), drawn with sharp pixels; "emissive" means it glows; see-through as the file says | Shiny or rough surfaces, normal maps: the Duck Forge look has no use for them |
| Parts | The tree of named parts, each with a position, a turn and a size | Cameras and lights inside the file |
| Animation (a later stage) | Named clips that move, turn and resize parts. Limbs as rigid parts first | Skinned, bending meshes: after that, if wanted |
| Duck Forge's own | `extras.duckforge`: a collision box and where the feet are | |

- 1 unit is 1 block. The loader turns the model from glTF's directions
  to the engine's, so Modular 3D exports the standard and doesn't need
  to know.
- **The slot left for it now:** an object's `"model"` key; a Model box
  in the Inspector that takes a dropped `.glb`; Files calls `.glb` files
  "Model"; `Mesh::load_model(path)` in the engine. Until the reader is
  built, an object with a model is drawn as a marked box, and Check
  says "models can't be drawn yet". So scene files written now won't
  need changing.

Sprites and 2D animation keep coming from Duck Quill's JSON and PNG,
which the engine reads today.

**What of 3D is in Phase 5:** the last two steps, after 2D is done: 3D
scenes with Mesh, Light, Camera and facing pictures, the fly camera, and
moving things. Not in Phase 5: reading models, 3D animation, painting
block worlds, ready-made 3D behaviours.

## 7. Beginner and Expert

You pick one on the New game screen. One click on **Beginner | Expert**
in the top bar switches, any time. It's remembered for each game, in the
Studio's own settings, because it's about the person and not the game:
the game's files are byte for byte the same in both.

| | Beginner | Expert |
|---|---|---|
| Inspector | The settings you need most. "7 more settings are hidden. Show them" | Everything: turn, size, tint, box, the id, the line in the file, the kind's name in code |
| Bottom panels | Hidden until you ask ("Show me the code" opens the Code panel) | Code, Console and Problems, a click away |
| Messages | One plain sentence and what to try | The same sentence, with the details open and the command's name |
| Scene list | Objects and their kinds | Also ids, and what the game drew that isn't an object |
| Files | Pictures, sounds, scenes, things; code under one "Code" folder | The folder as it is on disk |
| Top bar | Build, Play, Undo, History, Check, Clip, Pop out | Also Timing and Console |

**Beginner hides things; it never blocks them.** Every hidden setting is
one "Show them" away, every panel one click away, and a file edited in
Expert, by hand or by an agent, works in Beginner. A kind says which of
its settings are for Expert (`.expert()`); if it says nothing, they all
show.

## 8. What Duck Forge keeps that Godot doesn't have

- **Editing in the frozen moment, and playing on from that exact
  frame.** Scenes are files the game loaded, so an edit reaches the
  frozen game through the same `file` message and `reload` hook as a
  painted tile does today. `Scene::reload` works out what changed,
  object by object, and applies only that, so the duck stays in
  mid-jump.
- **Replays stay exact.** A session already stores the text files as
  they were on frame 1 and every change with its frame. Scene files join
  that list with no new idea. Moving an object "for this run only" is
  one new kind of event with a frame number. The test that exists
  (freeze, edit, resume, replay, compare the last picture pixel for
  pixel) gets a scene edit in the middle.
- **Check and its bots understand scenes.**
  - Before running anything: every scene reads; every picture, sound,
    scene and object a setting names is there; every scene can be
    reached.
  - "The duck can't reach this coin": today's `map.reach`, with solid
    objects counted as ground and the player's own jump settings. Moving
    platforms make it less sure, and it says so ("looks out of reach
    unless the lift helps").
  - A ready-made bot plays games made of ready-made behaviours, with
    today's `map.route`. It's the starter's `bot.rs`, moved into the
    engine. Games with their own code keep their own bot.
  - Every problem names its scene and object, and one click selects it.
- **Every editor action is a named command** (the table in section 4)
  that a button, a key, a script and an agent all call the same way.
- **Plain files everywhere.** Scenes, things, rules, numbers: all text
  an agent can write without the Studio open, and git can compare.
  History snapshots them with the rest.
- **Agents lose nothing.** `cargo run`, `tools/verify`, `new_game`,
  `--fuzz`, `--video`, `--describe` and the Studio's commands work on a
  scene game as on any example. `new_game` gets the scene starters.

## 9. The build steps

Each step ends in a STOP, with the tests passing on Linux and Windows,
the measurements below, and a hand checklist for your PC with no
terminal. "Stronger" means the model that wrote this design. "Cheaper"
means a smaller model can build it from this document and the step
before, because the hard choices are already made and there's a pattern
to copy.

| | Step | When it's done you can | Who |
|---|---|---|---|
| 1 | **Scenes in the engine.** The file, `Scene`, drawing in layers, turned and resized pictures, a tile map as an object, `reload` that merges, mistakes in plain words. A small demo game | Open the demo in the Studio, play it, edit its scene file in Notepad while it's frozen, and see the change | Stronger |
| 2 | **Scene list, Inspector, select and move.** The link's new questions, the `object` and `select` commands, snap and grid, Undo, "Starts at and Now" | Click an object, drag it, change its name, position, picture and layer, undo, and play on from the frozen frame | Stronger |
| 3 | **The other tools.** Turn, Size, several at once, box select, copy, paste, duplicate, delete, nudge, reorder, Add (plain kinds) | Build a small picture of a level by hand | Cheaper |
| 4 | **Dragging from Files.** Onto the scene and onto Inspector boxes, and from Windows | Drag a picture in, drag a sound onto a box | Cheaper |
| 5 | **Kinds.** `Kind`, `Settings`, `SceneGame`, the Inspector's boxes drawn from the game's own list, "New kind in code", "Show me the code" | Add a kind's skeleton from the Studio, give it a setting in code, and see the box appear | Stronger |
| 6 | **Ready-made behaviours and saved things.** The eight, one small file and one test each; "Turn into code"; two scene starters | Make a platformer with coins, a snail, a lift and a door, with no code. Turn the snail into code | Cheaper, one behaviour at a time. The first one (platformer player) by the stronger model, as the pattern |
| 7 | **Several scenes.** `game.json`, numbers, going between scenes, Text and Button, the scene chooser, New scene, the Scenes page, Play this scene | Make a title, a level select and two levels, and play through them | Stronger for the engine half, cheaper for the Scenes page |
| 8 | **Check and bots.** Scene checks, reach with objects, the ready-made bot, problems that select their object, `tools/verify` | Put a coin out of reach and be told which; break a door and be told which | Stronger |
| 9 | **Beginner and Expert, and the Code panel** | Switch with one click; open, change and save code inside the Studio and see a mistake on its line | Cheaper |
| 10 | **Rules.** The sentences, the builder, turning them into code | Make "duck touches water: restart" without code | Stronger for reading the sentences, cheaper for the builder |
| 11 | **3D scenes.** Three numbers, Mesh, Light, Camera, facing pictures, drawing and reload | Open a 3D demo scene, edit its file, see the change | Stronger |
| 12 | **3D in the editor.** The fly camera in the frozen game, clicking and moving in 3D, the Model slot | Fly round a frozen 3D game, move a crate, play on | Stronger |
| | **The proof**, as at the end of Phase 4: from New game to a three-scene game with no code, then one behaviour turned into code, walked on GitHub's Windows machine and by you | | Cheaper |

After each step: a new section in the hand checklist (it carries on from
item 49), the numbers in `docs/decisions.md`, and screenshots of every
new screen.

Steps 3, 4, 9 and the proof are the ones to hand to a cheaper model
whole, with most of step 6 and the Studio halves of steps 7 and 10.
Steps 10, 11 and 12 can each be left for a later phase without undoing
anything before them.

## 10. What I'd cut or postpone

Bluntly, so nothing surprises you later.

**This won't be Godot.** Godot's editor is many people's work over many
years. Phase 5 gives Duck Forge its own version of the five panels,
working on 2D scenes, with eight behaviours. That's a real editor for
small games. It isn't a rival.

**Games made in code don't become editable.** The shooter, the 3D
runner and Duck Craft draw themselves from Rust, and no editor can drag
what only exists as code. They get Files, Feel, the Code panel and a
read-only list of what they drew. To edit one's objects in the Studio,
its objects have to move into a scene, which is a rewrite of that game,
by hand or by an agent, and not a button. I'd move one starter as part
of step 6, to show how, and leave the rest alone.

**Cut from Phase 5:**

- **No tree of objects inside objects.** Godot's scene is a tree; this
  one is a flat list with layers. A tree makes every tool, the file and
  the code harder, and the games Duck Forge makes rarely need it. A lift
  carries what stands on it because the lift does that, not because of a
  tree. The file leaves room for a `"parent"` later.
- **One behaviour per object.** No stacking "patrol" and "shoots" on one
  snail. For that you turn it into code.
- **The Code panel is a plain editor.** Suggestions as you type and "go
  to definition" mean running rust-analyzer, a large separate program,
  inside the Studio and keeping it alive on Windows. That's a phase of
  its own. Files over a few thousand lines will feel slow in the panel;
  "Open in my editor" is there for them.
- **Turning doesn't turn collision boxes.** The engine's boxes don't
  rotate (as in its physics today). Turn is for looks; a turned
  platform still collides as its upright box. The Inspector says so.
- **No screen-layout system.** Text and Button, placed by hand. No
  anchors, no containers that resize.
- **No animation editor, no particle editor, no sound editor.** Duck
  Quill and Duck Echo do those.
- **No saving progress to disk**, and **no Export button**. "Exported
  game" in this document means what `cargo build --release --example`
  gives. A game with scenes reads its files when it starts, as games
  read their pictures and levels today, so sharing a game means sharing
  its folder with it. A proper Export (one folder or one file, ready to
  send) is a small phase of its own and I'd do it soon after this one.
- **3D:** no models (the slot only), no 3D animation, no block painting,
  no ready-made 3D behaviours.
- **No gamepads, no macOS**, as before.

**First to go if the phase runs long:** Rules (step 10), then 3D in the
editor (step 12), then 3D scenes (step 11). The phase is still whole
without them.

**The risks I see:**

- *Starts at and Now* (step 2) is the hardest idea in the phase. If it
  confuses in your hands, the fallback is simple and still good: Build
  always shows "As it starts", and "Frozen moment" becomes a second
  view for looking and for tiles.
- *The Inspector from the game's own list* (step 5) depends on the game
  being built. A game whose code doesn't build shows the last list it
  had, which can be stale.
- *Dragging smoothness.* The frozen game redraws a few times a second
  while you drag. The outline under the pointer hides that, but a very
  big scene may still feel a beat behind.
- *Size.* Twelve steps is longer than Phase 4's five. Each is small on
  purpose.

## The rules, and how they're measured

| Rule | How it's kept |
|---|---|
| Agents lose nothing | Every step runs `tools/verify` on every example, `--fuzz` and a `--run` script of the new commands |
| Existing games and playthroughs pass, frame for frame | The playthrough tests that exist today, unchanged. No existing game's files are touched before step 6, and then only the one starter that's moved, as a new starter beside the old one |
| Exported games don't contain the editor | Everything the Studio needs inside a game (answering questions, the "As it starts" view) is behind the `studio` feature, which exported games don't turn on. A test builds without it and checks the names aren't in the program |
| No slower, no bigger | The table below, measured again at every step on Linux and on GitHub's Windows machine. An existing game more than 1% bigger or slower stops the step |
| The Duck Forge look | No drawing code changes. Objects are drawn with today's `blit_sprite`. Turned pictures use the same sharp-pixel rule as resized ones |
| Linux and Windows | The release workflow runs every test on Windows, as now |
| A hand checklist | After every step, numbered on from 49 |

*Measured* today, on this Linux machine, release builds with the usual
features:

| Game | Size of the program | 600,000 updates with nobody playing (three runs) |
|---|---|---|
| Platformer starter (`template`) | 6,386,216 bytes | 0.33, 0.34, 0.38 s |
| Shooter starter | 6,252,280 bytes | 0.50, 0.51, 0.59 s |
| 3D runner starter | 11,594,608 bytes | 2.30, 2.36, 2.64 s |

Step 1 adds drawing time to this table (headless runs draw only at the
end, so it needs a small timing option) and the Windows numbers. From
step 6 the new Platformer starter made with scenes is measured beside
today's. It does more work per object than hand-written code, so I
expect it a little bigger and a little slower; the target is within 10%,
and the real numbers will be reported whatever they are.

## What I need you to decide

1. **JSON for scenes.** I recommend it, for the reasons in section 1.
   The other choice is TOML, which allows comments but is new to the
   engine and to every existing file.
2. **Rules as sentences.** `"when duck touches coin: add 1 to score"`
   reads well and compares well, but needs careful error messages. The
   other choice is a small JSON shape per rule, which is duller and
   safer.
3. **Rules in this phase, or after?** I'd keep them as step 10 and cut
   them first if needed.
4. **3D in this phase (steps 11 and 12), or a Phase 5b?** The 2D editor
   is complete without them.
5. **glTF `.glb` for models.** Can Modular 3D write it? If it already
   has a format, send me a sample and I'll compare.
6. **A flat list, one behaviour per object** (section 10). Both can be
   added later without changing files made now.
7. **The plain Code panel**, with "Open in my editor" beside it.
8. **Names:** "Scene", "Inspector", "Files" as Godot has them; "kind"
   for what an object is; "saved thing" for an object you reuse;
   "numbers" for score and lives. Say if any reads wrong.
9. **Which starter moves to scenes in step 6:** I'd take the Platformer,
   as a new starter beside today's.

One thing to do first: the first-hour fixes
(`claude/studio-first-hour-fixes`: the Files panel, Empty, delete and
rename) aren't in main yet. Steps 2 and 4 build on that Files panel, so
it should be merged before Step 1 starts.
