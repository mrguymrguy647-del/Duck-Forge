# The template's art

Painted with Duck Quill (`quill`, which needs no window). These are the
painting scripts; the pictures the game loads are in `../assets/`.

| Script | Makes | With |
|---|---|---|
| `hero.json` | the duck: `idle`, `run`, `jump`, `fall` | `quill export hero.json --sheet --out ../assets` |
| `tiles.json` | one animation per tile: `grass`, `dirt`, `brick`, `plank`, `water`, `coin`, `flag` | `quill export tiles.json --sheet --out ../assets` |
| `sky.json` | the sky, repeating sideways | `quill render sky.json --out ../assets` |
| `hills.json` | the hills, see-through above them | `quill render hills.json --transparent --out ../assets` |

Run the commands in this folder, then delete the `.quill` files `render`
leaves next to the pictures. The tile names are the ones the game's
`Legend` uses; a new tile is a new animation here and a new letter there.
Every tile is 16 x 16 pixels, the duck too (its feet on the bottom row:
that's its pivot, where the game puts it).
