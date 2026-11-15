# The robot you can click

The robot is a model made with Duck Mold (`duck-mold`), which builds 3D
models from text recipes. `robot.toml` is its recipe (Duck Mold's own
example, from its `claude/duck-mold-phase-6` branch); the file the game
loads is `../assets/robot.glb`.

| Source | Makes | With (run in `examples/robot_click`) |
|---|---|---|
| `robot.toml` | `robot.glb`: 384 triangles, four animations, eight poses, 15 hit shapes, and footstep events on its walk | `duck-mold export art/robot.toml -o assets/robot.glb` |

Two lines of the recipe are for this game:

- `colliders = "parts"` near the top gives every part a hit shape: a
  box, a ball or a capsule that moves with it. The head says
  `collider = "sphere"`; the eyes, the chest panel and the antenna's
  tip say `collider = "none"`, so a click there names the part behind.
  To see them: `duck-mold render art/robot.toml --colliders`, or the
  Hits switch in Duck Mold Studio.
- `event = "footstep"` on two keys of `[animations.walk]`: the moments
  a foot comes down. `duck-mold inspect art/robot.toml --animation walk`
  says when each is heard.

To make it click differently, change a part's `collider` (or run
`duck-mold edit art/robot.toml set-collider left_arm/hand sphere`); to
give it more sounds, put events on other keys (`duck-mold edit
art/robot.toml set-event jump 2 takeoff`). Export again, and run the
game.
