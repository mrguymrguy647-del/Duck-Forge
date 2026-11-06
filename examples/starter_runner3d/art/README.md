# The 3D runner's art and sounds

The pictures come from Duck Quill (`quill`), the sounds from Duck Echo
(`duck-echo`); neither needs a window. These are their sources; the files
the game loads are in `../assets/`.

| Source | Makes | With (run in `examples/starter_runner3d`) |
|---|---|---|
| `blocks.json` | the atlas: `road`, `grass`, `crate`, `gold`, one 16 x 16 tile each | `quill export art/blocks.json --sheet --out assets` |
| `hero.json` | the duck: `idle`, `run`, `jump`, `fall` (the platformer's) | `quill export art/hero.json --sheet --out assets` |
| Duck Echo's presets | `coin.wav`, `jump.wav`, `hit.wav` | `duck-echo render preset:coin -o assets/coin.wav --seed 1` (and `jump`, `hit`) |
| `hum.toml` | `hum.wav`, the loop that plays while running | `duck-echo render art/hum.toml -o hum-long.wav --seed 1`, then `python3 art/loop_cut.py hum-long.wav assets/hum.wav` |

The road, grass, crates and coins are textures from one atlas: the road
and grass repeat along the ground (texture points beyond 1 repeat the
tile), and a crate is `Mesh::block(&atlas, ["crate"; 3])`. A new block is
a new tile here and one line in `Art::load`.

`hum.toml` explains why the loop is cut from a longer sound: the end of a
loop must meet its start, and the first moments of a filtered sound
aren't steady yet.
