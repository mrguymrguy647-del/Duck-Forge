# The turntable's robot

The robot is a model made with Duck Mold (`duck-mold`), which builds 3D
models from text recipes and needs no window. `robot.toml` is its recipe
(Duck Mold's own example, from its `claude/nifty-dirac-vt09ym` branch,
commit d107b0e); the file the game loads is `../assets/robot.glb`.

| Source | Makes | With (run in `examples/turntable`) |
|---|---|---|
| `robot.toml` | `robot.glb`: 384 triangles in four palette colours | `duck-mold export art/robot.toml -o assets/robot.glb` |

`duck-mold inspect art/robot.toml` reports on it in words before you
export: its size (0.90 wide, 1.83 tall, 0.39 deep, in metres), its parts,
and whether any part floats or overlaps another.

The robot faces +z, as every Duck Mold model does, and stands on y = 0.
Its own left is -x: its left arm is on your right as it faces you.
