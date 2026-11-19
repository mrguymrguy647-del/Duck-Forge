# Pip and the crates

Everything Pip's game draws was made with the other two Duck apps. The
files here are what they were made from; the game loads only what is in
`../assets`.

| Made with | Source (here) | Makes | With (run in `examples/pip_walk`) |
|---|---|---|---|
| Duck Quill | `textures/pip_face.json`, a script | `textures/pip_face.png`: Pip's hair, eyes, cheeks and smile, see-through everywhere else | `quill render art/textures/pip_face.json --transparent --out art/textures` |
| Duck Quill | `textures/planks.json`, `textures/lid.json` | `planks.png`, `lid.png`: the crate's sides and its top | the same, for each |
| Duck Mold | `pip.toml`, a recipe | `../assets/pip.glb`: 316 triangles, the face on the front of the head, two animations (`idle`, `walk`), a `footstep` on each step, a hit shape for every part and two far-away versions | `duck-mold export art/pip.toml` |
| Duck Mold | `plank_crate.toml` | `../assets/plank_crate.glb`: one box with planks on its sides and a lid on top | `duck-mold export art/plank_crate.toml` |

Each recipe says where its model belongs (`export =
"../assets/pip.glb"`), so `duck-mold export` needs no `-o`.

`pip_face.quill` holds the face's layers, for painting on in Duck
Quill's window.

## Changing Pip while the game runs

Start the game (`cargo run --example pip_walk`) and walk Pip about. Then,
with the game still running, any of these:

- **In Duck Mold's Studio:** open `art/pip.toml`, tick *Export after
  every change* in the Model section, and change something: make the hat
  taller, paint the jacket another colour. Pip in the game has it as
  soon as the mouse is let go.
- **In Duck Forge Studio:** play the game there and click `art/pip.toml`
  in the Files panel. It opens in a tab that is Duck Mold's Studio, and
  every change is sent to the game by itself.
- **From a terminal:** `duck-mold edit art/pip.toml set hat_crown
  --height 0.2`, then `duck-mold export art/pip.toml`.
- **The face:** in either Studio, select the head and press *Paint in
  Duck Quill*. Paint, save there, and Pip has the new face: first on the
  model in the Studio, and in the game as soon as it is exported.

Pip carries on mid-step through all of them. The line in the game that
does it is in `main.rs`, in `reload`:
`self.pip.reload_if_changed(file)`.
