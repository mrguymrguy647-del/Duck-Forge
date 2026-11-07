# Duck Craft's art and sounds

The block pictures and the duck's skin come from Duck Quill (`quill`). The files the game loads
are in `../assets/`. Run the commands in `examples/duck_craft`.

| Source | Makes | With |
|---|---|---|
| `make_blocks.py` | `blocks.json`, the Duck Quill script: one 16 x 16 pixel-art tile per animation (grass, dirt, stone, cobblestone, sand, logs, leaves with holes, planks, glass, brick, coal and gold ore, water, bedrock, cloud, lamp, and four stages of cracks) | `python3 art/make_blocks.py` |
| `blocks.json` | the atlas `assets/blocks.png` and `assets/blocks.json` | `quill export art/blocks.json --sheet --out assets` |
| `make_skin.py` | `skin.json`: the duck's skin, each body part's six faces unwrapped onto one 64 x 32 picture, like a Minecraft skin (the layout is in the script's docstring; `main.rs`'s `Duck::load` uses the same numbers) | `python3 art/make_skin.py` |
| `skin.json` | `assets/skin.png` | `quill render art/skin.json --out assets` |
| Mini Craft's `hum.toml` (Duck Echo) | `hum.wav`, the wind | copied from `../mini_craft/assets` |

To change a block, edit its function in `make_blocks.py` (a grid of palette
names, `.` for see-through), then run both commands. Every tile is in one
picture, so all the blocks of a chunk share one texture and draw as one mesh.
The digging, building and footstep sounds are made in code (`Sound { .. }`).
