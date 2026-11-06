# Mini Craft's art and sounds

The pictures come from Duck Quill (`quill`), the sounds from Duck Echo
(`duck-echo`). These are the sources; the files the game loads are in
`../assets/`. Run the commands in `examples/mini_craft`.

| Source | Makes | With |
|---|---|---|
| `blocks.json` | the atlas, one 16 x 16 tile each: `grass_top`, `grass_side`, `dirt`, `stone`, `lava`, `log`, `leaves`, `gem` | `quill export art/blocks.json --sheet --out assets` |
| `hero.json` | the duck: `idle`, `run`, `jump`, `fall` | `quill export art/hero.json --sheet --out assets` |
| Duck Echo's presets | `coin.wav` (a gem), `hit.wav` (lava) | `duck-echo render preset:coin -o assets/coin.wav --seed 1` (and `hit`) |
| `hum.toml` | `hum.wav`, the wind that loops quietly | `duck-echo render art/hum.toml -o hum-long.wav --seed 1`, then `python3 art/loop_cut.py hum-long.wav assets/hum.wav` |

Every block is `Mesh::block(&atlas, [top, sides, bottom])`; all the
tiles are in one picture, so the blocks of a chunk merge into one mesh.
