# Duck Mold test models

Real models made by Duck Mold (`duck-mold`, its `claude/nifty-dirac-vt09ym`
branch, commit d029f9d), used by the tests of `Mesh::load_glb` and
`GlbModel` (`tests/duck_mold_models.rs` and the cheat sheet's test). They
are Duck Mold's own golden files: its tests check that its five example
recipes build to exactly these bytes.

| Model | Triangles | What it has |
|---|---|---|
| `crate.glb` | 12 | One box: 8 corners, two colours. |
| `table.glb` | 60 | A top and four legs. |
| `tree.glb` | 70 | A trunk and three cones pushed into each other. |
| `house.glb` | 76 | A door on its front (+z) and a chimney on its left (-x); one assembly, its roof. |
| `robot.glb` | 384 | 19 parts and four assemblies (its arms and legs); yellow eyes on its front (+z). |

Each `<name>.inspect.json` is what `duck-mold inspect examples/<name>.toml
--json` said about it: its size, its box (`bounds`), its triangle count and
how many triangles each palette colour paints, and its tree of parts. The
tests check the loaded mesh, and the parts and assemblies the file lists,
against those numbers.

To make them again, in a Duck Mold folder:

```sh
duck-mold export examples/robot.toml -o robot.glb
duck-mold inspect examples/robot.toml --json > robot.inspect.json
```

(and the same for `crate`, `table`, `tree` and `house`).
