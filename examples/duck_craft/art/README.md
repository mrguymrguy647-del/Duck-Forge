# Duck Craft's art and sounds

The block pictures and the duck's skin come from Duck Quill (`quill`). The files the game loads
are in `../assets/`. Run the commands in `examples/duck_craft`.

| Source | Makes | With |
|---|---|---|
| `make_blocks.py` | `blocks.json`, the Duck Quill script: one 16 x 16 pixel-art tile per animation (grass, dirt, stone, cobblestone, sand, logs, leaves with holes, planks, glass, brick, coal and gold ore, water, bedrock, cloud, lamp, four stages of cracks; tall grass and three flowers; and the items: apple, seeds, bread, gold, coal, the stone blade) | `python3 art/make_blocks.py` |
| `blocks.json` | the atlas `assets/blocks.png` and `assets/blocks.json` | `quill export art/blocks.json --sheet --out assets` |
| `make_skin.py` | `skin.json`: the duck's skin, each body part's six faces unwrapped onto one 64 x 32 picture, like a Minecraft skin (the layout is in the script's docstring; `main.rs`'s `Duck::load` uses the same numbers) | `python3 art/make_skin.py` |
| `skin.json` | `assets/skin.png` | `quill render art/skin.json --out assets` |
| `make_hud.py` | `hud.json`: the 9 x 9 icons over the hotbar: hearts, slices of bread (hunger) and bubbles (air), full, half and empty | `python3 art/make_hud.py` |
| `hud.json` | `assets/hud.png` and `assets/hud.json` | `quill export art/hud.json --sheet --out assets` |
| `make_monsters.py` | `hollow.json` and `watcher.json`: the night's shapes, in the duck's skin layout (a Hollow: ash-grey, eyes gone to holes, beak broken off, the suit rotted; the Watcher: black, two points of light) | `python3 art/make_monsters.py` |
| `hollow.json`, `watcher.json` | `assets/hollow.png`, `assets/watcher.png` | `quill render art/hollow.json --out assets` (and the same for `watcher`) |
| `make_duckling.py` | `duckling.json`: the ducklings' skin, the same kind of layout (body with tail, head with beak, legs; `ducklings.rs` uses the same numbers) | `python3 art/make_duckling.py` |
| `duckling.json` | `assets/duckling.png` | `quill render art/duckling.json --out assets` |
| Mini Craft's `hum.toml` (Duck Echo) | `hum.wav`, the wind | copied from `../mini_craft/assets` |

To change a block, edit its function in `make_blocks.py` (a grid of palette
names, `.` for see-through), then run both commands. Every tile is in one
picture, so all the blocks of a chunk share one texture and draw as one mesh.
The digging, building, footstep, crafting, thud and quack sounds are made in code (`Sound { .. }`).

## Music and sounds

`../audio/make_sounds.py` makes every `.wav` in `../assets` except `hum.wav`
(Mini Craft's wind), from nothing but sine waves, noise and arithmetic
(numpy and scipy), because Duck Echo isn't ready yet:

| File | What |
|---|---|
| `music_day.wav` | "Still Water": slow felt piano over a soft pad, D minor, 64 s, loops with no seam |
| `music_night.wav` | "Nothing Waits": a low drone, a minor second that comes and goes, wind, a far bell; 64 s |
| `music_title.wav` | "The Island Doesn't Remember": a falling piano figure over the drone; 32 s |
| `groan.wav`, `groan_far.wav` | a Hollow, near and far (the engine plays a file at one loudness, so distance is a second file) |
| `hollow_hurt.wav`, `hollow_die.wav` | a Hollow hit, and coming apart into ash |
| `sting.wav` | the Watcher, seen |
| `heartbeat.wav` | one beat, looped when the duck is badly hurt |
| `bell_dusk.wav`, `bell_dawn.wav` | nightfall and morning |

Run it in `examples/duck_craft`: `python3 audio/make_sounds.py` (about 40 s).
The game fades the loops itself: the title's on the title screen, day and
night by the light, the heartbeat by how hurt you are.
