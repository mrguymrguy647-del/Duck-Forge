# Duck Peaks' art and music

Painted with Duck Quill (`quill`) and composed with Duck Echo
(`duck-echo`); neither needs a window. These are the sources; the files
the game loads are in `../assets/`. Run the commands in
`examples/duck_peaks`.

| Source | Makes | With |
|---|---|---|
| `hero.json` | the duck: `idle`, `run`, `jump`, `fall` | `quill export art/hero.json --sheet --out assets` |
| `tiles.json` | one animation per tile: `grass`, `dirt`, `brick`, `plank`, `water`, `coin`, `flag`, `rock`, `snow` | `quill export art/tiles.json --sheet --out assets` |
| `sky.json` | the sunset sky, 360 tall so it covers the whole climb | `quill render art/sky.json --out assets` |
| `mountains.json` | far snowy peaks, see-through above | `quill render art/mountains.json --transparent --out assets` |
| `forest.json` | near pines, see-through above | `quill render art/forest.json --transparent --out assets` |
| `music.toml` | the 4-second music loop | `duck-echo render art/music.toml -o music-raw.wav --seed 1`, then `python3 art/loop_fit.py music-raw.wav assets/music.wav 4.0` |

Delete the `.quill` files `quill render` leaves next to the pictures.
The three layers repeat sideways: copies past each edge are painted into
the scripts, so the edges meet. Every tile is 16 x 16, the duck too.
