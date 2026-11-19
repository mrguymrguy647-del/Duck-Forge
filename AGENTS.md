# Duck Forge, for AI agents

Duck Forge (the `duckforge` crate) is a small game engine in Rust: 2D, 3D,
sound and physics, written to be read. Every game also runs **headless**,
with no window, and saves pictures, sound and a log to read. Duck Forge
Studio, in `studio/`, is the app for people. Duck Mold, a separate
program, makes the `.glb` models games load.

## Making games with it

Read `docs/QUICKSTART.md` first (one page). `docs/AI_GUIDE.md` is the full
guide, and `skills/duckforge/SKILL.md` is the same workflow as a skill.

```sh
cargo build --no-default-features --examples   # no window, graphics card or sound needed
cargo run --no-default-features --example dodge -- --headless --sheet --out out/dodge
tools/verify.sh dodge              # build, play tests, playthroughs, fuzzing: ends in VERIFY OK
tools/new_game.sh mygame topdown   # a new game from a starter, renamed, verified
```

On Windows: `tools\verify.bat dodge` and `tools\new_game.bat mygame topdown`.
The starters are `empty`, `platformer`, `topdown`, `shooter`, `runner3d` and `tiny`.

- On a machine with no screen, pass `--no-default-features` to every
  cargo command. Options for the game go after `--`.
- Read `summary.txt` in the `--out` folder, then look at `sheet.png`.
- `cargo test` doesn't rebuild the game's own program. Use
  `tools/verify.sh` or `cargo run` to try a change.
- What runs make goes in `out/` or `duckforge-out/`, which git ignores.
- A person may have the same folder open in Duck Forge Studio. It reads
  and writes the same files; leave its `history/` folder alone.

## Working on the code

One crate at the top, and the Studio in a Cargo workspace of its own
(its own lock file and target folder):

| Folder | What it is |
|---|---|
| `src/` | The engine. `src/cli.rs` has the options every game understands |
| `examples/` | The games and starters, with their play tests (`test = true` in `Cargo.toml`) |
| `playthroughs/` | Recorded key presses that games are replayed with |
| `tests/` | Tests that keep the docs, scripts and Duck Mold models true |
| `tools/` | `verify` and `new_game`, for Linux, Mac and Windows |
| `studio/` | Duck Forge Studio: `studio_core` (no window) and `studio_app` (the window) |
| `docs/` | The guides; `docs/decisions.md` says why things are as they are |

```sh
cargo test --no-default-features      # the engine's tests with no window, as a server runs them
cargo test                            # the same with the window, graphics card and sound
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
cd studio && cargo test -p studio_core -p studio_app   # the Studio's (they build and play real games)
```

**Before a commit:** both test runs, `cargo fmt --all` and both clippy
runs with `-D warnings`, all clean. If you changed `studio/`, or engine
code the Studio uses, its tests and
`cargo clippy -p studio_core -p studio_app --all-targets -- -D warnings`
in `studio/` too. On Windows, `"tools\Run tests (for developers).bat"`
runs every test. CI is the "Studio release" workflow
(`.github/workflows/studio-release.yml`): it runs the engine's tests both
ways five times on Windows, then the Studio's.

**With no internet:** run `cargo fetch` once (and once in `studio/`,
which gets Duck Mold from GitHub). After that,
`cargo build --offline --no-default-features --examples` and
`cargo test --offline --no-default-features` download nothing.

Stored results that tests compare with, and how to accept a change you
meant:

- A playthrough that now ends differently: `tools/verify.sh` says
  `changed:` and prints the command that accepts it, like
  `cargo run --no-default-features --example dodge -- --headless --inputs playthroughs/dodge.json --accept`.
  Or record it again with the game's bot:
  `cargo test --no-default-features --example dodge -- --ignored record`.
- Pictures of Duck Mold models, in `tests/fixtures/duck_mold/pictures/`:
  `DUCKFORGE_ACCEPT_PICTURES=1 cargo test --no-default-features --test duck_mold_models`
  (and `--test duck_mold_images`). Look at every picture that changed.

Other rules:

- **The docs are tested.** Every game option must be in `src/cli.rs`, the
  table in `README.md` and `docs/AI_GUIDE.md`. `tests/cheat_sheet.rs` calls every
  function the guide's cheat sheet names. `docs/QUICKSTART.md`'s game must be
  `examples/quickstart.rs`. `skills/duckforge/SKILL.md` stays under 150 lines.
- **Same seed, same inputs, same result**, to the byte. Games take random
  numbers only from `ctx.rng`.
- **No new libraries in the headless build** without a reason:
  `tests/dependencies.rs` lists them all, and fails on a new one.
- **Games get no bigger and no slower**: more than 1% stops a change
  (`docs/decisions.md` has the numbers). `--headless --time` times one.
- `tools/verify.sh` and `tools/verify.bat` do the same thing with the same
  options; `.bat` files keep Windows line endings (`.gitattributes`).
- Words for people, in messages and docs, are plain English with no jargon.
- The `.glb` reader (`src/glb.rs`) follows Duck Mold's writer; its test
  models are Duck Mold's own (`tests/fixtures/duck_mold/README.md`). The
  Studio gets Duck Mold at a fixed tag (`studio/Cargo.toml`); the engine
  never uses it.
