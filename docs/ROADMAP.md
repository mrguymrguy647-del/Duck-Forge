# Duck Forge roadmap

Where Duck Forge is, and what could come next. The reasons behind every
step are in [decisions.md](decisions.md), under the headings named here;
how to use each part is in [AI_GUIDE.md](AI_GUIDE.md).

## Done

| What | Where to read about it |
|---|---|
| Phase 1: a small engine people and AI agents are comfortable with: fixed steps, seeds, headless runs, input scripts, play tests | decisions.md, "Phase 1" |
| Phase 2: efficient for an AI agent: sprites, camera, sound, text, scenes in words, fuzzing, tuning | "Phase 2", "Phase 2.5" |
| Phase 3: what real test games found: tile maps, tilesets, video, then 3D (textures, light, billboards) and Duck Craft | "Phase 3", "Phase 3, Step 4", "Fixing what Duck Craft found" |
| Phase 4: Duck Forge Studio, an app for people: Home, Build mode, levels, Feel, History, Windows installer | "Phase 4 ..." |
| Phase 5: a real editor: scenes, the Inspector, scripts, the first ready-made behaviour | "Phase 5 ..." |
| Duck Mold models: `.glb` models, sockets, animations, hit shapes, far-away versions | "Models from Duck Mold" and Duck Mold's phases |
| Lit 3D, fonts and video for shorts | "Lit 3D, fonts and video for shorts" |
| **Duck Echo's Phase 3 (Duck Forge's side, steps 5 to 9): sounds from Duck Echo, end to end** | "Duck Echo's Phase 3, step 5" to "step 9" |

Duck Echo's Phase 3 on Duck Forge's side, in one list:

- Step 5: a `.wav` that changes while the game runs is heard from the
  next update, and replays stay exact (the sound store).
- Step 6: OGG music, decoded while it plays, the same bytes on Windows
  and Linux.
- Step 7: sets of variants (footsteps that differ) and facts (a hit on
  the frame it lands).
- Step 8: Duck Forge Studio's Inspector lists music and sets with their
  facts, and opens a sound's recipe in Duck Echo Studio.
- Step 9: Duck Craft end to end, and these docs.

## Waiting for review (on their own branches)

- Review mode (`--review`, `duckforge review any.mp4`) and the "Pick a
  Door: Blocks" improvements (glow masks, particles, sky, held items,
  pixel fonts, faster lit 3D, sound groups and ducking): branch
  `claude/peaceful-bohr-vniw0t`.

## Next, if wanted (none of it is started)

Sound, after Duck Echo's Phase 3:

- Replays on another computer play the sounds they heard only if the
  sound store (`out/sound-store/`) goes with them; it isn't in git. A
  game could keep its playthroughs' sounds beside them.
- One-off sounds can't be stopped or changed once started; no pitch
  changes, no moving pans, no crossfades beyond changing volumes.
- A set's list is read once: more variants need a restart.
- Duck Echo Studio keeps its own Undo for a game opened from Duck Forge
  Studio; an agent working on the same game through Duck Echo's MCP
  server keeps another (Duck Echo's step 4 note).

The engine (from AI_GUIDE.md section 12):

- Turned sprites' collision boxes, rotating physics boxes.
- Objects inside objects in scenes; 3D scene files drawn.
- Gamepads; running in a web browser.
- Right-to-left and joined scripts in fonts.
- 3D on the graphics card with lamps and shadows; skinned characters,
  morph targets, `.gltf` text files.

The Studio:

- More ready-made behaviours (the platformer player is the pattern).
