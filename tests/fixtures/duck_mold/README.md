# Duck Mold test models

Real models made by Duck Mold (`duck-mold`, its `claude/nifty-dirac-vt09ym`
branch), used by the tests of `Mesh::load_glb` and `GlbModel`
(`tests/duck_mold_models.rs` and the cheat sheet's test). They are Duck
Mold's own golden files: its tests check that its example recipes build
to exactly these bytes. They are laid out as a tree of parts (reader
version 4): every part is an object at the join it turns about. The
robot also moves (reader version 5): its file holds four animations
(`idle`, `walk`, `wave`, `jump`) and ten poses. So do the door (it
opens), the cart (its wheels roll) and the gnome (it walks with the
robot's walk, copied into its recipe from a file, waves and jumps).

`../duck_mold_before_tree/` holds three of the same models (`crate`,
`house`, `robot`) as Duck Mold wrote them before that, at the end of its
Phase 4: one object with one list of triangles. The tests load both and
check that they are the same model and draw the same picture, so models
already exported keep working. Duck Mold keeps the same three files in
its `tests/before-phase5/`.

| Model | Triangles | What it has |
|---|---|---|
| `crate.glb` | 12 | One box: 8 corners, two colours. |
| `table.glb` | 60 | A top and four legs. |
| `tree.glb` | 70 | A trunk and three cones pushed into each other. |
| `house.glb` | 120 | A door and two framed windows on its front (+z), a chimney on its left (-x) and a skylight on its roof; one assembly, its roof. |
| `robot.glb` | 384 | 19 parts and four assemblies (its arms and legs); yellow eyes on its front (+z); 187 sockets; four animations and ten poses; two far-away versions (288 and 276 triangles). |
| `snowman.glb` | 600 | Balls, cones and coal: parts joined on curved surfaces. |
| `plank_crate.glb` | 12 | One box with **images** (reader version 9): planks on its four sides and a lid on its top and bottom, painted with Duck Quill, all on one 128 x 64 picture inside the file. |
| `door.glb` | 60 | A frame and a door on a hinge; `open` (the handle turns, then the door swings 90 degrees towards the front). |
| `cart.glb` | 436 | A bed on four wheels (four assemblies), one spoke each so the turning shows; `roll` loops. |
| `gnome.glb` | 432 | The robot's six part names on another body: four assemblies (arms and legs); `walk` (the robot's), `wave` and `jump` (the starters'), each stood on the ground by Duck Mold's `fit-ground`; two far-away versions (228 and 216 triangles). |

Each `<name>.inspect.json` is what `duck-mold inspect examples/<name>.toml
--json` said about it: its size, its box (`bounds`), its triangle count and
how many triangles each palette colour paints, and its tree of parts. The
tests check the loaded mesh, and the parts and assemblies the file lists,
against those numbers. `robot.sockets.json` is what `duck-mold inspect
examples/robot.toml --sockets --json` said: the report again, with every
socket and where it is, which the tests check `GlbModel::socket` finds.

The robot, the gnome, the door and the cart also have **hit shapes**
(reader version 6): a box, a ball or a capsule round each part, carried
by the part's join. `<name>.colliders.json` is what `duck-mold inspect
examples/<name>.toml --colliders --json` said: every shape, its middle
and its size. `tests/duck_mold_hits.rs` checks the file against it,
then tests rays, segments and balls on each model at rest, in each pose
and through each animation.

The robot, the gnome, and Duck Mold's temple and barn also carry two
**far-away versions** (reader version 8): `lods = "auto"` in their
recipes. `tests/duck_mold_far.rs` reads the robot's and checks them
against `robot.inspect.json`'s `far_versions`.

Animations also carry **events** (reader version 7): names on keys a
game hears when the animation passes them. The robot's and the gnome's
`walk` have a `footstep` on each step, the robot's `jump` `takeoff` and
`land`, the door's `open` `opened`; each `<model>.<animation>.inspect.json`
lists them under `events`. `tests/duck_mold_events.rs` plays every
animation with updates of many sizes, forwards and backwards, looping
and once, and checks each event is heard exactly once each time its key
is passed.

`<model>.<animation>.inspect.json` (the robot's `idle`, `walk`,
`wave` and `jump`, the door's `open`, the cart's `roll`, the gnome's
`walk`, `wave` and `jump`) are what `duck-mold inspect examples/robot.toml
--animation <name> --json` said about each animation: its length, whether
it loops, how many frames Duck Mold looked at (thirty a second), and how
far the robot's lowest point goes below the ground and above it. The
tests play each animation frame by frame and check it against that.

`pictures/` holds each model that moves drawn at four moments of each
of its animations (`robot.walk.1.png` to `robot.walk.4.png`,
`door.open.1.png`, and so on), and the robot at rest, from the front and
from the side. A test draws them again and compares them
pixel for pixel. If a change to the renderer or the model is meant,
store the new ones with:

```sh
DUCKFORGE_ACCEPT_PICTURES=1 cargo test --test duck_mold_models
```

To make them again, in a Duck Mold folder:

```sh
duck-mold export examples/robot.toml -o robot.glb
duck-mold inspect examples/robot.toml --json > robot.inspect.json
duck-mold inspect examples/robot.toml --sockets --json > robot.sockets.json
duck-mold inspect examples/robot.toml --colliders --json > robot.colliders.json
duck-mold inspect examples/robot.toml --animation walk --json > robot.walk.inspect.json
```

(the first two for `crate`, `table`, `tree`, `house`, `snowman`, `door`,
`cart` and `gnome` too, the fourth for `gnome`, `door` and `cart`, and
the last for each animation above).

`plank_crate.glb` is the first model with **images** (reader version
9): its recipe puts a PNG on every face. The file says layout 2, holds
the images on one picture and says where every triangle is on it.
`tests/duck_mold_images.rs` checks it against its `inspect.json` (the
picture's size is `images.sheet`), draws it from every side, and
compares two stored pictures (`pictures/plank_crate.*.png`).
