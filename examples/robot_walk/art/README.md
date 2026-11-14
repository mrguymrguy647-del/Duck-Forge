# The walking robot

The robot is a model made with Duck Mold (`duck-mold`), which builds 3D
models from text recipes and needs no window. `robot.toml` is its recipe
(Duck Mold's own example, from its `claude/duck-mold-phase-5` branch); the
file the game loads is `../assets/robot.glb`.

| Source | Makes | With (run in `examples/robot_walk`) |
|---|---|---|
| `robot.toml` | `robot.glb`: 384 triangles in four palette colours, three animations (`idle`, `walk`, `wave`) and six poses | `duck-mold export art/robot.toml -o assets/robot.glb` |

The end of the recipe is how it moves: its **poses** (which parts are
turned, and how far) and its **animations** (poses, each reached at a
time). To look at them without running the game:

```sh
duck-mold inspect art/robot.toml                      # names its poses and animations
duck-mold inspect art/robot.toml --animation walk     # checks every frame of the walk
duck-mold render art/robot.toml --animation walk --view right   # a strip of frames, as a picture
```

To change how it walks, change a pose in `robot.toml` (or with
`duck-mold edit art/robot.toml set-pose step_left left_leg --turn 35,0,0`),
export again, and run the game.

The robot faces +z, as every Duck Mold model does, and stands on y = 0.
Its own left is -x: its left arm, the one it waves with, is on your
right as it faces you. An animation never moves the robot along the
ground: the game does that, and plays `walk` while it does.
