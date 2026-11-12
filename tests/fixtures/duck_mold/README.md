# Duck Mold test models

Real models made by Duck Mold (`duck-mold`, its `claude/duck-mold-phase-1`
branch, commit 031e381), used by the tests of `Mesh::load_glb` and
`GlbModel` (`tests/duck_mold_models.rs` and the cheat sheet's test). They
are Duck Mold's own golden files: its tests check that its six example
recipes build to exactly these bytes. Since that commit they hold the
models' sockets too (reader version 3).

| Model | Triangles | What it has |
|---|---|---|
| `crate.glb` | 12 | One box: 8 corners, two colours. |
| `table.glb` | 60 | A top and four legs. |
| `tree.glb` | 70 | A trunk and three cones pushed into each other. |
| `house.glb` | 88 | A door on its front (+z), a chimney on its left (-x) and a skylight on its roof; one assembly, its roof. |
| `robot.glb` | 384 | 19 parts and four assemblies (its arms and legs); yellow eyes on its front (+z); 187 sockets. |
| `snowman.glb` | 600 | Balls, cones and coal: parts joined on curved surfaces. |

Each `<name>.inspect.json` is what `duck-mold inspect examples/<name>.toml
--json` said about it: its size, its box (`bounds`), its triangle count and
how many triangles each palette colour paints, and its tree of parts. The
tests check the loaded mesh, and the parts and assemblies the file lists,
against those numbers. `robot.sockets.json` is what `duck-mold inspect
examples/robot.toml --sockets --json` said: the report again, with every
socket and where it is, which the tests check `GlbModel::socket` finds.

To make them again, in a Duck Mold folder:

```sh
duck-mold export examples/robot.toml -o robot.glb
duck-mold inspect examples/robot.toml --json > robot.inspect.json
duck-mold inspect examples/robot.toml --sockets --json > robot.sockets.json
```

(and the first two for `crate`, `table`, `tree`, `house` and `snowman`).
