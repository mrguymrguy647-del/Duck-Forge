//! The Studio itself, without its window: what's open, what's running,
//! and every action as one named command.
//!
//! A button, a key, a test, a script file and (later) an agent's tool all
//! do the same thing: `studio.run(Command::...)`. The app only draws what
//! this holds and turns clicks into commands.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use duckforge::prelude::{Key, MouseButton};
use duckforge::{InputAction, InputScript};
use serde::{Deserialize, Serialize};

use crate::assets::{self, SoundInfo};
use crate::build::{self, Build, BuildState, Rust, Watcher};
use crate::check::{self, Check, CheckResult};
use crate::clip::{Clip, ClipResult};
use crate::console::{Console, JobEvent, JobId, JobState};
use crate::feel::{self, CodeSpot, FeelFile, Slider};
use crate::folder::{CAME_WITH_DUCK_FORGE, Card, Game, GamesFolder, STARTERS, name_from_title};
use crate::game_files;
use crate::history::{FileEdit, History, Snapshot, Step, Undo};
use crate::level::{Cell, Level, LevelArt};
use crate::manage;
use crate::my_games;
use crate::play::{AfterHello, Ending, Play, PlayState, Timing};
use crate::problem::Problem;
use crate::recycle::RecycleBin;
use crate::settings::Settings;

/// How the game's character jumps, from its tune values (`jump_speed`,
/// `gravity`, `run_speed`), if it has all three: the starters' names.
fn jump_from(tunes: &std::collections::BTreeMap<String, f32>) -> Option<duckforge::prelude::Jump> {
    Some(duckforge::prelude::Jump {
        speed: *tunes.get("jump_speed")?,
        gravity: *tunes.get("gravity")?,
        run: *tunes.get("run_speed")?,
        // The starters' duck: a rough width is better than none.
        width: 10.0,
    })
}

/// The files in a game's assets folder: when each was changed, and its size.
fn assets_now(
    game_folder: Option<&std::path::Path>,
) -> std::collections::BTreeMap<PathBuf, (Option<std::time::SystemTime>, u64)> {
    let Some(folder) = game_folder else {
        return std::collections::BTreeMap::new();
    };
    std::fs::read_dir(folder.join("assets"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let data = entry.metadata().ok()?;
            data.is_file()
                .then(|| (entry.path(), (data.modified().ok(), data.len())))
        })
        .collect()
}

/// The game's scene files (`scenes/*.json` in its folder), with their
/// text: what the Studio last saw, to notice one changed outside it.
fn scenes_now(
    game_folder: Option<&std::path::Path>,
) -> std::collections::BTreeMap<PathBuf, String> {
    let Some(folder) = game_folder else {
        return std::collections::BTreeMap::new();
    };
    std::fs::read_dir(folder.join("scenes"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            Some((path, text))
        })
        .collect()
}

/// A file's name as games name it: from the Duck Forge folder, with
/// forward slashes (`examples/bee_maze/scenes/level1.json`).
fn name_from(root: &std::path::Path, file: &std::path::Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// "1 tile", "3 tiles".
fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

mod objects;

/// Everything the Studio can be asked to do. In a script file each is a
/// JSON object whose `"do"` is the command's name:
/// `{"do": "game.open", "name": "duck_peaks"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do")]
pub enum Command {
    /// Go to Home (stops the game that's open).
    #[serde(rename = "home.open")]
    HomeOpen,
    /// Use this folder as the games folder.
    #[serde(rename = "home.choose_folder")]
    ChooseFolder { folder: PathBuf },
    /// Look for Rust again (after installing it).
    #[serde(rename = "home.check_again")]
    CheckAgain,
    /// Make a new game from a starter and open it.
    #[serde(rename = "game.new")]
    GameNew { starter: String, name: String },
    /// Open a game: build it if needed, then play it.
    #[serde(rename = "game.open")]
    GameOpen { name: String },
    /// Move a game (and what the Studio keeps about it) to the Recycle
    /// Bin, and take its lines out of Cargo.toml. Not one that came with
    /// Duck Forge.
    #[serde(rename = "game.delete")]
    GameDelete { name: String },
    /// Give a game a new name: `to` as typed, like "Bee Run".
    #[serde(rename = "game.rename")]
    GameRename { name: String, to: String },
    /// Make a copy of a game under a new name.
    #[serde(rename = "game.duplicate")]
    GameDuplicate { name: String, to: String },
    /// Make the game's card picture now, from two seconds of it running
    /// with no window.
    #[serde(rename = "game.thumbnail")]
    GameThumbnail { name: String },
    /// Build the open game now.
    #[serde(rename = "game.build")]
    GameBuild,
    /// Play: carry on from the frozen frame.
    #[serde(rename = "game.play")]
    GamePlay,
    /// Freeze on the frame the game is on (Build).
    #[serde(rename = "game.freeze")]
    GameFreeze,
    /// Tab: freeze if playing, play if frozen.
    #[serde(rename = "game.toggle")]
    GameToggle,
    /// While frozen: play this many more frames, then stay frozen.
    #[serde(rename = "game.step")]
    GameStep { frames: u32 },
    /// Start the game again from the beginning (with the newest build).
    #[serde(rename = "game.restart")]
    GameRestart,
    /// Start again and replay this session's moves, to get back to where
    /// it was.
    #[serde(rename = "game.replay_my_moves")]
    GameReplayMyMoves,
    /// Open the game in its own window, on the frame it's on.
    #[serde(rename = "game.pop_out")]
    GamePopOut,
    /// Close the game's own window and bring it back inside.
    #[serde(rename = "game.bring_back")]
    GameBringBack,
    /// Stop the game.
    #[serde(rename = "game.stop")]
    GameStop,
    /// F1: show the game's debug overlay on its pictures, or hide it.
    /// Without `on`, it switches.
    #[serde(rename = "game.debug_overlay")]
    GameDebugOverlay {
        #[serde(default)]
        on: Option<bool>,
    },
    /// F12: save the game's picture as it is now, at its own size.
    #[serde(rename = "game.screenshot")]
    GameScreenshot,
    /// Press or let go of a key or button, or move the mouse, written as
    /// in an input script: `{"do": "game.input", "press": "Space"}`.
    #[serde(rename = "game.input")]
    GameInput {
        #[serde(flatten)]
        event: serde_json::Map<String, serde_json::Value>,
    },
    /// Run the whole check for the open game.
    #[serde(rename = "check.run")]
    CheckRun,
    /// Show (or hide, with no line) one line of the check, with its picture.
    #[serde(rename = "check.show_problem")]
    CheckShowProblem { line: Option<usize> },
    /// Have the game's bot record its playthrough again, on the level as
    /// it is now, then check again.
    #[serde(rename = "check.record_again")]
    CheckRecordAgain,
    /// Make a ten-second clip of the last run.
    #[serde(rename = "clip.make")]
    ClipMake,
    /// Run a line typed into the Console panel, in the games folder.
    #[serde(rename = "console.run")]
    ConsoleRun { line: String },
    /// Paint a tile (or a thing) on cells: `{"do": "level.paint",
    /// "cells": [[14, 9], [15, 9]], "tile": "plank"}`. Edits with the same
    /// `stroke` (one press of the mouse) are one step for Undo.
    #[serde(rename = "level.paint")]
    LevelPaint {
        cells: Vec<Cell>,
        tile: String,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Empty cells.
    #[serde(rename = "level.erase")]
    LevelErase {
        cells: Vec<Cell>,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Fill the area around a cell (the cells joined to it with the same
    /// letter) with a tile.
    #[serde(rename = "level.fill")]
    LevelFill { at: Cell, tile: String },
    /// Put a thing in a cell. One the level has only one of (the Start)
    /// moves there.
    #[serde(rename = "level.place_thing")]
    LevelPlaceThing { at: Cell, thing: String },
    /// Move what's in one cell to another.
    #[serde(rename = "level.move_thing")]
    LevelMoveThing { from: Cell, to: Cell },
    /// Empty one cell.
    #[serde(rename = "level.delete_thing")]
    LevelDeleteThing { at: Cell },
    /// Move a block of cells (`size`: columns and rows) so its top left
    /// is at `to`.
    #[serde(rename = "level.move_area")]
    LevelMoveArea {
        from: Cell,
        size: (i32, i32),
        to: Cell,
    },
    /// Make the level this many columns and rows.
    #[serde(rename = "level.resize")]
    LevelResize { columns: usize, rows: usize },
    /// Play from this cell: the game starts again with its Start there,
    /// this time only (the file isn't changed).
    #[serde(rename = "level.play_from")]
    LevelPlayFrom { at: Cell },
    /// Change one Feel value: `{"do": "feel.set", "name": "jump_speed",
    /// "value": 380}`. The game takes it at once; the tune file is saved.
    /// Changes with the same `stroke` (one drag of a slider) are one step
    /// for Undo.
    #[serde(rename = "feel.set")]
    FeelSet {
        name: String,
        value: f32,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Use one of the feel file's presets: several values at once.
    #[serde(rename = "feel.preset")]
    FeelPreset { name: String },
    /// Show where the code asks for a value, with the lines around it.
    #[serde(rename = "feel.show_code")]
    FeelShowCode { name: String },
    /// Open a picture (a tile sheet, a sprite sheet) in Duck Quill.
    #[serde(rename = "assets.open_in_quill")]
    AssetsOpenInQuill { file: PathBuf },
    /// Have the game play one of its sounds again.
    #[serde(rename = "assets.play_sound")]
    AssetsPlaySound { name: String },
    /// Show how a sound was made: its values, or Duck Echo's recipe.
    #[serde(rename = "assets.show_recipe")]
    AssetsShowRecipe { name: String },
    /// Take a snapshot of the game's files in its history.
    #[serde(rename = "edit.snapshot")]
    EditSnapshot { title: String },
    /// Put the game's files back as they were in a snapshot (a snapshot
    /// of now is taken first, so this can be undone too).
    #[serde(rename = "edit.go_back_to")]
    EditGoBackTo { number: u32 },
    /// Undo the last edit (a paint stroke, a Feel change...).
    #[serde(rename = "edit.undo")]
    EditUndo,
    /// Redo what was undone.
    #[serde(rename = "edit.redo")]
    EditRedo,
    /// Files panel: pick a file (a picture shows, a sound's details show).
    /// Without `path`, nothing is picked. Paths are inside the open game's
    /// folder, like `assets/duck.png`.
    #[serde(rename = "files.select")]
    FilesSelect {
        #[serde(default)]
        path: Option<String>,
    },
    /// Open a file in the program Windows uses for it (with an empty
    /// path, the game's folder in File Explorer).
    #[serde(rename = "files.open")]
    FilesOpen { path: String },
    /// Play a sound file.
    #[serde(rename = "files.play")]
    FilesPlay { path: String },
    /// Copy files from anywhere on the computer into the game's folder
    /// (`to`: one of its folders; the game's own folder if left out).
    #[serde(rename = "files.add")]
    FilesAdd {
        files: Vec<PathBuf>,
        #[serde(default)]
        to: String,
    },
    /// Make a new folder in the game's folder: `{"path": "assets/music"}`.
    #[serde(rename = "files.new_folder")]
    FilesNewFolder { path: String },
    /// Give a file or folder a new name, in the same folder.
    #[serde(rename = "files.rename")]
    FilesRename { path: String, to: String },
    /// Move a file or folder to the Recycle Bin.
    #[serde(rename = "files.delete")]
    FilesDelete { path: String },
    /// Change objects of the scene, by their keys as the scene file has
    /// them: `{"do": "object.set", "ids": [7], "set": {"layer": "front"}}`.
    /// It changes the file (how the scene starts); the game takes it at
    /// once. A key set to `null` goes back to its usual value. Changes
    /// with the same `stroke` (one drag) are one step for Undo.
    #[serde(rename = "object.set")]
    ObjectSet {
        ids: Vec<u64>,
        set: serde_json::Map<String, serde_json::Value>,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Move where objects start: `by` so many pixels, or the first one
    /// `to` a place with the others along.
    #[serde(rename = "object.move")]
    ObjectMove {
        ids: Vec<u64>,
        #[serde(default)]
        by: Option<[f32; 2]>,
        #[serde(default)]
        to: Option<[f32; 2]>,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Give an object a name (an empty one takes its name away).
    #[serde(rename = "object.rename")]
    ObjectRename { id: u64, name: String },
    /// Change an object as it is on the frozen frame, for this run only:
    /// its file is left alone, so the next run starts as before.
    #[serde(rename = "object.set_now")]
    ObjectSetNow {
        id: u64,
        set: serde_json::Map<String, serde_json::Value>,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Move an object on the frozen frame, for this run only.
    #[serde(rename = "object.move_now")]
    ObjectMoveNow {
        id: u64,
        to: [f32; 2],
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Where each object is right now becomes where it starts.
    #[serde(rename = "object.start_here")]
    ObjectStartHere { ids: Vec<u64> },
    /// Each object goes back to where it starts, on the frozen frame.
    #[serde(rename = "object.back_to_start")]
    ObjectBackToStart { ids: Vec<u64> },
    /// Add an object: `{"do": "object.add", "kind": "picture", "set":
    /// {"picture": "assets/crate.png"}}`. Without `at`, it goes in the
    /// middle of what the view shows. A picture or a background needs a
    /// `picture`, a tile map a `level`.
    #[serde(rename = "object.add")]
    ObjectAdd {
        kind: String,
        #[serde(default)]
        at: Option<[f32; 2]>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        set: serde_json::Map<String, serde_json::Value>,
    },
    /// Turn objects: `by` so many degrees clockwise, or the first `to` a
    /// turn and the rest by as much.
    #[serde(rename = "object.turn")]
    ObjectTurn {
        ids: Vec<u64>,
        #[serde(default)]
        by: Option<f32>,
        #[serde(default)]
        to: Option<f32>,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Make objects bigger or smaller: `by` so many times across and
    /// down, or the first `to` a size and the rest by as much. Each keeps
    /// its `at` where it is.
    #[serde(rename = "object.resize")]
    ObjectResize {
        ids: Vec<u64>,
        #[serde(default)]
        by: Option<[f32; 2]>,
        #[serde(default)]
        to: Option<[f32; 2]>,
        #[serde(default)]
        stroke: Option<u64>,
    },
    /// Change where objects come in the scene's order (the drawing order
    /// within a layer): `to` "front", "back", "forward" or "backward", or
    /// `before` another object.
    #[serde(rename = "object.order")]
    ObjectOrder {
        ids: Vec<u64>,
        #[serde(default)]
        to: Option<String>,
        #[serde(default)]
        before: Option<u64>,
    },
    /// Take objects out of the scene.
    #[serde(rename = "object.delete")]
    ObjectDelete { ids: Vec<u64> },
    /// Copies of objects, just after each, one grid square to the right.
    #[serde(rename = "object.duplicate")]
    ObjectDuplicate { ids: Vec<u64> },
    /// Copy objects to the Studio's clipboard.
    #[serde(rename = "object.copy")]
    ObjectCopy { ids: Vec<u64> },
    /// Paste objects: `text` (lines of a scene file), or else what was
    /// copied. With `at`, the first goes there.
    #[serde(rename = "object.paste")]
    ObjectPaste {
        #[serde(default)]
        at: Option<[f32; 2]>,
        #[serde(default)]
        text: Option<String>,
    },
    /// Select these objects (and no others).
    #[serde(rename = "select.set")]
    SelectSet { ids: Vec<u64> },
    /// Select these objects too.
    #[serde(rename = "select.add")]
    SelectAdd { ids: Vec<u64> },
    /// Select every object that can be dragged.
    #[serde(rename = "select.all")]
    SelectAll,
    #[serde(rename = "select.none")]
    SelectNone,
    /// The view's tool: `select` or `move`.
    #[serde(rename = "view.tool")]
    ViewTool { tool: crate::scene_edit::Tool },
    /// Snap dragged objects to the grid, and how big its squares are.
    #[serde(rename = "view.snap")]
    ViewSnap {
        on: bool,
        #[serde(default)]
        step: Option<f32>,
    },
    /// Show or hide the grid.
    #[serde(rename = "view.grid")]
    ViewGrid { on: bool },
    /// What a frozen picture shows: `frozen` (the game as it is) or
    /// `start` (the scene as its file has it).
    #[serde(rename = "view.show")]
    ViewShow { what: crate::scene_edit::Show },
}

/// One command, for lists of what the Studio can do (the Console's help
/// now, an agent's tools later).
pub struct CommandInfo {
    pub name: &'static str,
    pub title: &'static str,
    /// What it needs, in words. Empty if nothing.
    pub needs: &'static str,
}

/// Every command, in one table.
pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo {
        name: "home.open",
        title: "Go to Home",
        needs: "",
    },
    CommandInfo {
        name: "home.choose_folder",
        title: "Choose the games folder",
        needs: "folder: the Duck Forge folder",
    },
    CommandInfo {
        name: "home.check_again",
        title: "Look for Rust again",
        needs: "",
    },
    CommandInfo {
        name: "game.new",
        title: "Make a new game",
        needs: "starter: empty, platformer, topdown, shooter, runner3d or tiny; name: like Bee Maze",
    },
    CommandInfo {
        name: "game.open",
        title: "Open a game",
        needs: "name: like duck_peaks",
    },
    CommandInfo {
        name: "game.delete",
        title: "Move a game to the Recycle Bin",
        needs: "name: like bee_maze (not one that came with Duck Forge)",
    },
    CommandInfo {
        name: "game.rename",
        title: "Rename a game",
        needs: "name: like bee_maze; to: its new name, like Bee Run",
    },
    CommandInfo {
        name: "game.duplicate",
        title: "Make a copy of a game",
        needs: "name: like duck_peaks; to: the copy's name, like My Peaks",
    },
    CommandInfo {
        name: "game.thumbnail",
        title: "Make the game's card picture now",
        needs: "name: like bee_maze",
    },
    CommandInfo {
        name: "game.build",
        title: "Build the game now",
        needs: "",
    },
    CommandInfo {
        name: "game.play",
        title: "Play",
        needs: "",
    },
    CommandInfo {
        name: "game.freeze",
        title: "Freeze (Build)",
        needs: "",
    },
    CommandInfo {
        name: "game.toggle",
        title: "Switch between Build and Play (Tab)",
        needs: "",
    },
    CommandInfo {
        name: "game.step",
        title: "Play a few more frames, frozen",
        needs: "frames: how many",
    },
    CommandInfo {
        name: "game.restart",
        title: "Restart the game",
        needs: "",
    },
    CommandInfo {
        name: "game.replay_my_moves",
        title: "Restart and replay my moves",
        needs: "",
    },
    CommandInfo {
        name: "game.pop_out",
        title: "Pop out into its own window",
        needs: "",
    },
    CommandInfo {
        name: "game.bring_back",
        title: "Bring the game back inside",
        needs: "",
    },
    CommandInfo {
        name: "game.stop",
        title: "Stop the game",
        needs: "",
    },
    CommandInfo {
        name: "game.debug_overlay",
        title: "Show or hide the debug overlay (F1)",
        needs: "on: true or false (without it, it switches)",
    },
    CommandInfo {
        name: "game.screenshot",
        title: "Save a screenshot (F12)",
        needs: "",
    },
    CommandInfo {
        name: "game.input",
        title: "Press a key, as in an input script",
        needs: "one of press, release, mouse, mouse_motion, mouse_press, mouse_release, scroll",
    },
    CommandInfo {
        name: "check.run",
        title: "Check the game",
        needs: "",
    },
    CommandInfo {
        name: "check.show_problem",
        title: "Show one line of the check",
        needs: "line: which one, from 0 (none hides it)",
    },
    CommandInfo {
        name: "check.record_again",
        title: "Record the playthrough again with the bot",
        needs: "",
    },
    CommandInfo {
        name: "clip.make",
        title: "Make a clip of the last run",
        needs: "",
    },
    CommandInfo {
        name: "console.run",
        title: "Run a typed command",
        needs: "line: the command",
    },
    CommandInfo {
        name: "level.paint",
        title: "Paint a tile or thing on cells",
        needs: "cells: [[column, row], ...]; tile: its name, like grass",
    },
    CommandInfo {
        name: "level.erase",
        title: "Empty cells",
        needs: "cells: [[column, row], ...]",
    },
    CommandInfo {
        name: "level.fill",
        title: "Fill an area with a tile",
        needs: "at: [column, row]; tile: its name",
    },
    CommandInfo {
        name: "level.place_thing",
        title: "Put a thing in a cell",
        needs: "at: [column, row]; thing: its name, like coin",
    },
    CommandInfo {
        name: "level.move_thing",
        title: "Move what's in a cell",
        needs: "from and to: [column, row]",
    },
    CommandInfo {
        name: "level.delete_thing",
        title: "Empty one cell",
        needs: "at: [column, row]",
    },
    CommandInfo {
        name: "level.move_area",
        title: "Move a block of cells",
        needs: "from: [column, row]; size: [columns, rows]; to: [column, row]",
    },
    CommandInfo {
        name: "level.resize",
        title: "Change the level's size",
        needs: "columns and rows",
    },
    CommandInfo {
        name: "level.play_from",
        title: "Play from a cell, this time only",
        needs: "at: [column, row]",
    },
    CommandInfo {
        name: "feel.set",
        title: "Change a Feel value",
        needs: "name, like jump_speed; value",
    },
    CommandInfo {
        name: "feel.preset",
        title: "Use a Feel preset",
        needs: "name: one of the feel file's presets",
    },
    CommandInfo {
        name: "feel.show_code",
        title: "Show where the code asks for a value",
        needs: "name, like jump_speed",
    },
    CommandInfo {
        name: "assets.open_in_quill",
        title: "Open a picture in Duck Quill",
        needs: "file: the picture",
    },
    CommandInfo {
        name: "assets.play_sound",
        title: "Play one of the game's sounds",
        needs: "name, like jump",
    },
    CommandInfo {
        name: "assets.show_recipe",
        title: "Show how a sound was made",
        needs: "name, like music",
    },
    CommandInfo {
        name: "edit.snapshot",
        title: "Take a snapshot for the history",
        needs: "title: a few words",
    },
    CommandInfo {
        name: "edit.go_back_to",
        title: "Go back to a snapshot",
        needs: "number: the snapshot's number",
    },
    CommandInfo {
        name: "edit.undo",
        title: "Undo the last edit",
        needs: "",
    },
    CommandInfo {
        name: "edit.redo",
        title: "Redo what was undone",
        needs: "",
    },
    CommandInfo {
        name: "files.select",
        title: "Pick a file in the Files panel",
        needs: "path: like assets/duck.png (none to pick nothing)",
    },
    CommandInfo {
        name: "files.open",
        title: "Open a file in its own program",
        needs: "path: like level.txt",
    },
    CommandInfo {
        name: "files.play",
        title: "Play a sound file",
        needs: "path: like assets/jump.wav",
    },
    CommandInfo {
        name: "files.add",
        title: "Copy files into the game's folder",
        needs: "files: the files' full paths; to: one of its folders (optional)",
    },
    CommandInfo {
        name: "files.new_folder",
        title: "Make a folder in the game's folder",
        needs: "path: like assets/music",
    },
    CommandInfo {
        name: "files.rename",
        title: "Rename a file or folder",
        needs: "path: like assets/duck.png; to: its new name, like hero.png",
    },
    CommandInfo {
        name: "files.delete",
        title: "Move a file or folder to the Recycle Bin",
        needs: "path: like assets/old.png (not main.rs)",
    },
    CommandInfo {
        name: "object.set",
        title: "Change objects of the scene",
        needs: "ids: like [7]; set: keys as the scene file has them, like {\"layer\": \"front\"}",
    },
    CommandInfo {
        name: "object.move",
        title: "Move where objects start",
        needs: "ids: like [7]; by: like [8, 0], or to: like [96, 208]",
    },
    CommandInfo {
        name: "object.rename",
        title: "Name an object",
        needs: "id: like 7; name: like snail",
    },
    CommandInfo {
        name: "object.set_now",
        title: "Change an object for this run only",
        needs: "id: like 7; set: keys, like {\"at\": [96, 208]}",
    },
    CommandInfo {
        name: "object.move_now",
        title: "Move an object for this run only",
        needs: "id: like 7; to: like [96, 208]",
    },
    CommandInfo {
        name: "object.start_here",
        title: "Start objects where they are now",
        needs: "ids: like [7]",
    },
    CommandInfo {
        name: "object.back_to_start",
        title: "Put objects back where they start",
        needs: "ids: like [7]",
    },
    CommandInfo {
        name: "object.add",
        title: "Add an object",
        needs: "kind: picture, area, background or tilemap; set: like {\"picture\": \"assets/crate.png\"}; at: like [96, 208] (or the middle of the view)",
    },
    CommandInfo {
        name: "object.turn",
        title: "Turn objects",
        needs: "ids: like [7]; by: degrees clockwise, like 15, or to: like 90",
    },
    CommandInfo {
        name: "object.resize",
        title: "Make objects bigger or smaller",
        needs: "ids: like [7]; by: like [2, 2] (twice as big), or to: like [1.5, 1]",
    },
    CommandInfo {
        name: "object.order",
        title: "Change where objects come in the list (drawing order)",
        needs: "ids: like [7]; to: front, back, forward or backward; or before: an id",
    },
    CommandInfo {
        name: "object.delete",
        title: "Delete objects",
        needs: "ids: like [7, 9]",
    },
    CommandInfo {
        name: "object.duplicate",
        title: "Duplicate objects",
        needs: "ids: like [7]",
    },
    CommandInfo {
        name: "object.copy",
        title: "Copy objects",
        needs: "ids: like [7]",
    },
    CommandInfo {
        name: "object.paste",
        title: "Paste objects",
        needs: "at: like [96, 208] (optional); text: lines of a scene file (optional)",
    },
    CommandInfo {
        name: "select.set",
        title: "Select objects",
        needs: "ids: like [7, 9]",
    },
    CommandInfo {
        name: "select.add",
        title: "Select objects too",
        needs: "ids: like [4]",
    },
    CommandInfo {
        name: "select.all",
        title: "Select every object",
        needs: "",
    },
    CommandInfo {
        name: "select.none",
        title: "Select nothing",
        needs: "",
    },
    CommandInfo {
        name: "view.tool",
        title: "Pick the view's tool",
        needs: "tool: select, move, turn or size",
    },
    CommandInfo {
        name: "view.snap",
        title: "Snap to the grid",
        needs: "on: true or false; step: the squares' size, like 8",
    },
    CommandInfo {
        name: "view.grid",
        title: "Show the grid",
        needs: "on: true or false",
    },
    CommandInfo {
        name: "view.show",
        title: "Show the frozen moment, or the scene as it starts",
        needs: "what: frozen or start",
    },
];

/// Which screen is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Home,
    Game,
}

/// A game's card on Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HomeCard {
    pub game: Game,
    pub card: Card,
    pub picture: Option<PathBuf>,
}

/// A new game being made.
#[derive(Debug)]
pub struct Making {
    pub name: String,
    pub title: String,
    pub job: JobId,
}

/// What's still to do for a game that was just made, once it's open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FirstSteps {
    /// Its first build is running: next, its bot records a playthrough.
    WaitingForBuild,
    /// The bot is recording the first playthrough: next, a first Check.
    Recording(JobId),
}

/// The game that's open, and everything going on with it.
pub struct OpenGame {
    pub game: Game,
    /// The build in progress, or the last one.
    pub build: Option<Build>,
    /// The newest built copy, ready to play.
    pub program: Option<PathBuf>,
    /// A newer build is ready than the one being played.
    pub new_build_ready: bool,
    pub play: Option<Play>,
    pub check: Option<Check>,
    pub check_result: Option<CheckResult>,
    /// The check's line whose picture is showing.
    pub shown_line: Option<usize>,
    pub clip: Option<Clip>,
    pub clip_result: Option<Result<ClipResult, Problem>>,
    /// The last session saved, to make clips from and to replay.
    pub last_session: Option<PathBuf>,
    /// The last finished session's recording, for "replay my moves".
    last_recording: Option<InputScript>,
    watcher: Watcher,
    watched: Vec<PathBuf>,
    looked: Instant,
    /// Start playing as soon as there's a build.
    play_when_built: bool,
    /// For a game just made: its first playthrough and first Check.
    first_steps: Option<FirstSteps>,
    /// Check has just had the bot try to record a first playthrough: it
    /// doesn't try again in the same Check.
    tried_first_recording: bool,
    /// Its level, if it has one (`level.txt` in its folder).
    pub level: Option<Level>,
    /// Why its level couldn't be read.
    pub level_problem: Option<Problem>,
    /// The pictures of its tiles and things.
    pub level_art: Option<LevelArt>,
    /// The game said it reads its level only when it starts: changes need
    /// a restart to show.
    pub level_needs_restart: bool,
    /// The level changed and the game hasn't been sent it yet (during a
    /// paint stroke it's sent a few times a second, not every frame).
    level_unsent: bool,
    level_sent_at: Option<Instant>,
    /// When the level's file was last looked at for outside changes.
    level_looked: Instant,
    /// This session's edits, for Undo and Redo.
    pub undo: Undo,
    /// The game's tune values as it last said them (name, value).
    pub tunes: std::collections::BTreeMap<String, f32>,
    /// The game's feel file (labels, ranges, presets), and what's wrong
    /// with it if it can't be read.
    pub feel: FeelFile,
    pub feel_problem: Option<Problem>,
    /// The tune file's text as last seen, to notice outside changes.
    tune_text: String,
    /// The game's history (snapshots), and its snapshots as last read.
    pub history: History,
    pub snapshots: Vec<Snapshot>,
    /// Something was edited since the last snapshot.
    edited_since_snapshot: bool,
    last_autosave: Instant,
    /// "Show me the code": where a value is asked for.
    pub code_spot: Option<(String, CodeSpot)>,
    /// A sound's recipe being shown: its name, and the recipe.
    pub sound_recipe: Option<(String, String)>,
    /// The game's picture and sound files as last seen (when each was
    /// changed, and its size), to notice one saved in Duck Quill.
    assets_seen: std::collections::BTreeMap<PathBuf, (Option<std::time::SystemTime>, u64)>,
    /// The game's scene files as last seen, to notice one changed outside
    /// the Studio (in an editor, or by an agent).
    scenes_seen: std::collections::BTreeMap<PathBuf, String>,
    /// The scene the editor shows: the one the frozen game is in, or the
    /// game's first. `None` for a game with no scene files.
    pub scene: Option<crate::scene_edit::SceneDoc>,
    /// The selected objects, by id, in the order they were picked.
    pub selected: Vec<u64>,
    /// The view's tool, snapping, grid, and what it shows.
    pub view: crate::scene_edit::SceneView,
    /// The frame the game was on when last looked at.
    frame_seen: Option<u64>,
    /// Goes up when the level's pictures change, so the app draws its
    /// palette again.
    pub art_version: u64,
    /// The file picked in the Files panel, as its place in the game's
    /// folder (`assets/duck.png`).
    pub files_selected: Option<String>,
    /// Whether it's been looked at if the game needs a card picture.
    thumbnail_looked: bool,
}

impl OpenGame {
    /// The Feel panel's sliders: every value the game asked for.
    pub fn sliders(&self) -> Vec<Slider> {
        let tunes: Vec<feel::TuneValue> = self
            .play
            .as_ref()
            .map(|play| play.tunes.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|mut tune| {
                if let Some(value) = self.tunes.get(&tune.name) {
                    tune.value = *value;
                }
                tune
            })
            .collect();
        feel::sliders(&tunes, &self.feel)
    }

    /// The sounds the game has played, as it last said them.
    pub fn sounds(&self) -> &[SoundInfo] {
        self.play.as_ref().map_or(&[], |play| &play.sounds)
    }
}

impl OpenGame {
    /// What's wrong with the build, if it failed.
    pub fn build_problem(&self) -> Option<&Problem> {
        match self.build.as_ref().map(|build| &build.state) {
            Some(BuildState::Failed(problem)) => Some(problem),
            _ => None,
        }
    }

    pub fn is_building(&self) -> bool {
        matches!(
            self.build.as_ref().map(|build| &build.state),
            Some(BuildState::Running)
        )
    }

    pub fn play_state(&self) -> Option<&PlayState> {
        self.play.as_ref().map(|play| &play.state)
    }

    /// For a game that was just made: what the Studio is still doing to
    /// get it ready, in words. A first Check follows by itself.
    pub fn getting_ready(&self) -> Option<&'static str> {
        match self.first_steps {
            Some(FirstSteps::WaitingForBuild) => Some("Building it for the first time"),
            Some(FirstSteps::Recording(_)) => Some("Recording its playthrough with the bot"),
            None => None,
        }
    }
}

/// The Studio.
pub struct Studio {
    pub settings: Settings,
    pub folder: Option<GamesFolder>,
    /// Why there's no games folder, if there isn't.
    pub folder_problem: Option<Problem>,
    /// Something to say once about the games folder: that it was made in
    /// Documents, or brought up to date by this version.
    pub folder_note: Option<String>,
    pub rust: Result<Rust, Problem>,
    pub console: Console,
    pub screen: Screen,
    pub cards: Vec<HomeCard>,
    pub making: Option<Making>,
    pub open: Option<OpenGame>,
    /// The last thing that went wrong with a command, for the app to show.
    pub problem: Option<Problem>,
    /// The keys and mouse buttons really held right now, as the app sees
    /// them. Play uses them when carrying on from a freeze: whatever the
    /// game still thinks is held but isn't, is let go, so nothing sticks.
    pub keys_down: Vec<Key>,
    /// Where deleted games and files go: the Recycle Bin (tests use a
    /// folder of their own).
    pub recycle_bin: RecycleBin,
    /// Card pictures being made by running a game with no window, and for
    /// which game.
    thumbnail_jobs: Vec<(JobId, String)>,
    pub buttons_down: Vec<MouseButton>,
    /// F1: the game draws its debug overlay. Kept across restarts.
    pub debug_overlay: bool,
    /// Objects copied (Ctrl+C), as lines of a scene file, and how many
    /// times they've been pasted since.
    pub clipboard: Vec<serde_json::Map<String, serde_json::Value>>,
    pastes: u32,
}

impl Studio {
    /// Starts the Studio with these settings: finds the games folder and
    /// Rust, and lists the games.
    pub fn new(settings: Settings) -> Studio {
        let mut studio = Studio {
            settings,
            folder: None,
            folder_problem: None,
            folder_note: None,
            rust: build::find_rust(),
            console: Console::default(),
            screen: Screen::Home,
            cards: Vec::new(),
            making: None,
            open: None,
            problem: None,
            debug_overlay: false,
            keys_down: Vec::new(),
            recycle_bin: RecycleBin::System,
            thumbnail_jobs: Vec::new(),
            buttons_down: Vec::new(),
            clipboard: Vec::new(),
            pastes: 0,
        };
        studio.console.path = studio.rust.as_ref().ok().and_then(|rust| rust.path.clone());
        let wanted = studio.games_folder_to_use();
        match wanted {
            Some(root) => match GamesFolder::open(root) {
                Ok(folder) => studio.folder = Some(folder),
                Err(problem) => studio.folder_problem = Some(problem),
            },
            None => {
                studio.folder_problem = Some(Problem::new(
                    "The Studio doesn't know where Duck Forge is yet.",
                    "Choose the Duck Forge folder: the one with Cargo.toml and an examples \
                     folder in it.",
                ));
            }
        }
        studio.refresh_cards();
        studio
    }

    /// Which games folder to open as the Studio starts.
    ///
    /// An installed (or unzipped) Studio has its own copy of Duck Forge
    /// beside it, which a new version replaces: the games are kept in the
    /// person's own folder instead (`Documents\Duck Forge`), made from that
    /// copy the first time and brought up to date by a newer Studio (see
    /// [`my_games`]). A folder chosen on Home is used as it is. A Studio
    /// built from the code uses the Duck Forge folder it's in.
    fn games_folder_to_use(&mut self) -> Option<PathBuf> {
        let studios_own = my_games::came_with_the_studio();
        let chosen = self.settings.games_folder.clone();
        let Some(own) = studios_own else {
            return chosen.or_else(GamesFolder::find);
        };
        match chosen {
            Some(chosen) if !my_games::is_the_studios_own(&chosen, &own) => {
                // Theirs. If the Studio made it, it may need bringing up to date.
                if chosen.join(my_games::RECORD).is_file() {
                    self.prepare_my_games(&own, &chosen);
                }
                Some(chosen)
            }
            _ => {
                let Some(games) = my_games::my_games_folder() else {
                    self.folder_problem = Some(Problem::new(
                        "The Studio couldn't find your Documents folder.",
                        "Choose a folder for your games on Home.",
                    ));
                    return None;
                };
                self.prepare_my_games(&own, &games).then_some(games)
            }
        }
    }

    /// Makes or updates the person's games folder from the Studio's own
    /// copy of Duck Forge. False (with the problem kept) if it couldn't.
    fn prepare_my_games(&mut self, own: &Path, games: &Path) -> bool {
        let version = env!("CARGO_PKG_VERSION");
        let build = my_games::build_folder();
        match my_games::prepare(own, games, version, build.as_deref()) {
            Ok(prepared) => {
                self.folder_note = prepared.say(games);
                true
            }
            Err(problem) => {
                self.folder_problem = Some(problem);
                false
            }
        }
    }

    /// The open game's own folder, for the Files panel's commands.
    fn files_folder(&mut self) -> Result<PathBuf, Problem> {
        let game = &self.open_game()?.game;
        game.folder.clone().ok_or_else(|| {
            Problem::new(
                format!(
                    "{} is one file ({}), with no folder of its own for pictures and sounds.",
                    game.title,
                    game.main.file_name().unwrap_or_default().to_string_lossy()
                ),
                "Games made from a starter (all but Tiny) have a folder of their own.",
            )
        })
    }

    /// Moves a game to the Recycle Bin (closing it first, if it's open).
    fn delete_game(&mut self, name: &str) -> Result<String, Problem> {
        let folder = self.folder()?;
        if CAME_WITH_DUCK_FORGE.contains(&name) {
            // Says why, without closing anything.
            return manage::delete(&folder, name, &self.recycle_bin).map(|_| String::new());
        }
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.game.name == name)
        {
            self.close_game();
            self.screen = Screen::Home;
        }
        manage::delete(&folder, name, &self.recycle_bin)?;
        self.refresh_cards();
        Ok(format!(
            "Moved {} to the Recycle Bin",
            crate::folder::title_of(name)
        ))
    }

    /// Renames a game (closing it first, if it's open).
    fn rename_game(&mut self, name: &str, to: &str) -> Result<String, Problem> {
        let folder = self.folder()?;
        if CAME_WITH_DUCK_FORGE.contains(&name) {
            return manage::rename(&folder, name, to);
        }
        manage::free_name(&folder, to)?;
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.game.name == name)
        {
            self.close_game();
            self.screen = Screen::Home;
        }
        let new = manage::rename(&folder, name, to)?;
        self.refresh_cards();
        Ok(format!(
            "Renamed {} to {}",
            crate::folder::title_of(name),
            crate::folder::title_of(&new)
        ))
    }

    /// Makes a game's card picture by running it for two seconds with no
    /// window (in the background; Home shows it when it's done).
    fn make_thumbnail(&mut self, name: &str) -> Result<String, Problem> {
        let folder = self.folder()?;
        let rust = self.rust()?;
        if folder.game(name).is_none() {
            return Err(Problem::new(
                format!("There's no game called {name}."),
                "Its name in files is like bee_maze: see the card on Home.",
            ));
        }
        let command = folder.thumbnail_command(&rust.cargo, name);
        let job = self
            .console
            .start("Card picture", &command, crate::console::Keep::Everything);
        self.thumbnail_jobs.push((job, name.to_string()));
        Ok(format!(
            "Making a card picture of {}",
            crate::folder::title_of(name)
        ))
    }

    /// The first time a game has played two seconds (or stops sooner), or
    /// its first Check found no picture: if its card has none, it gets one.
    fn first_card_picture(&mut self, stopping: bool) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        if open.thumbnail_looked {
            return;
        }
        let Some(picture) = open.play.as_ref().and_then(|play| play.picture.as_ref()) else {
            return;
        };
        if picture.frame < 120 && !(stopping && picture.frame >= 10) {
            return;
        }
        open.thumbnail_looked = true;
        if folder.card_picture(&open.game.name).is_none()
            && save_picture(&folder.thumbnail_file(&open.game.name), picture).is_ok()
        {
            self.refresh_cards();
        }
    }

    /// Reads the games and how their last checks went, for Home.
    pub fn refresh_cards(&mut self) {
        self.cards = match &self.folder {
            Some(folder) => folder
                .games()
                .into_iter()
                .map(|game| HomeCard {
                    card: check::card(folder, &game),
                    picture: folder.card_picture(&game.name),
                    game,
                })
                .collect(),
            None => Vec::new(),
        };
    }

    fn folder(&self) -> Result<GamesFolder, Problem> {
        self.folder.clone().ok_or_else(|| {
            self.folder_problem.clone().unwrap_or_else(|| {
                Problem::new(
                    "There's no games folder yet.",
                    "Choose the Duck Forge folder.",
                )
            })
        })
    }

    fn rust(&self) -> Result<Rust, Problem> {
        self.rust.clone()
    }

    fn open_game(&mut self) -> Result<&mut OpenGame, Problem> {
        self.open
            .as_mut()
            .ok_or_else(|| Problem::new("No game is open.", "Open a game from Home first."))
    }

    /// Does one command. Says what happened in a few words, or what went
    /// wrong. A problem is also kept in `problem`, for the app to show.
    pub fn run(&mut self, command: Command) -> Result<String, Problem> {
        let result = self.do_command(command);
        if let Err(problem) = &result {
            self.problem = Some(problem.clone());
        }
        result
    }

    fn do_command(&mut self, command: Command) -> Result<String, Problem> {
        match command {
            Command::HomeOpen => {
                self.close_game();
                self.screen = Screen::Home;
                self.refresh_cards();
                Ok(String::from("Home"))
            }
            Command::ChooseFolder { folder } => {
                if let Some(own) = my_games::came_with_the_studio()
                    && my_games::is_the_studios_own(&folder, &own)
                {
                    return Err(Problem::new(
                        "That's the copy of Duck Forge that came with the Studio.",
                        "Installing a new version replaces it, so nothing made there could be \
                         kept. Your games folder is in Documents (Duck Forge), or choose \
                         another folder of your own.",
                    ));
                }
                let opened = GamesFolder::open(folder.clone())?;
                self.folder_note = None;
                self.close_game();
                self.settings.games_folder = Some(opened.root().to_path_buf());
                self.settings.save();
                self.folder = Some(opened);
                self.folder_problem = None;
                self.screen = Screen::Home;
                self.refresh_cards();
                Ok(format!("The games folder is {}", folder.display()))
            }
            Command::CheckAgain => {
                self.rust = build::find_rust();
                self.console.path = self.rust.as_ref().ok().and_then(|rust| rust.path.clone());
                let rust = self.rust()?;
                if let Some(open) = &mut self.open
                    && open.program.is_none()
                    && !open.is_building()
                {
                    open.play_when_built = true;
                    self.start_build()?;
                }
                Ok(format!("Found {}", rust.version))
            }
            Command::GameNew { starter, name } => self.new_game(&starter, &name),
            Command::GameDelete { name } => self.delete_game(&name),
            Command::GameRename { name, to } => self.rename_game(&name, &to),
            Command::GameDuplicate { name, to } => {
                let folder = self.folder()?;
                let new = manage::duplicate(&folder, &name, &to)?;
                self.refresh_cards();
                Ok(format!(
                    "Made {}, a copy of {}",
                    crate::folder::title_of(&new),
                    crate::folder::title_of(&name)
                ))
            }
            Command::GameThumbnail { name } => self.make_thumbnail(&name),
            Command::FilesSelect { path } => {
                let folder = self.files_folder()?;
                if let Some(place) = &path
                    && !game_files::resolve(&folder, place)?.exists()
                {
                    return Err(no_such_file(place));
                }
                self.open_game()?.files_selected = path.clone();
                Ok(match path {
                    Some(place) => format!("Picked {place}"),
                    None => String::from("Nothing picked"),
                })
            }
            Command::FilesOpen { path } => {
                let real = game_files::resolve(&self.files_folder()?, &path)?;
                if !real.exists() {
                    return Err(no_such_file(&path));
                }
                game_files::open_with_the_system(&real)?;
                Ok(format!("Opened {path}"))
            }
            Command::FilesPlay { path } => {
                let real = game_files::resolve(&self.files_folder()?, &path)?;
                if !real.is_file() {
                    return Err(no_such_file(&path));
                }
                game_files::play_sound(&real)?;
                Ok(format!("Playing {path}"))
            }
            Command::FilesAdd { files, to } => {
                let added = game_files::add(&self.files_folder()?, &to, &files)?;
                let open = self.open_game()?;
                open.files_selected = added.first().cloned();
                Ok(match added.as_slice() {
                    [one] => format!("Added {one}"),
                    many => format!("Added {} files", many.len()),
                })
            }
            Command::FilesNewFolder { path } => {
                let made = game_files::new_folder(&self.files_folder()?, &path)?;
                self.open_game()?.files_selected = Some(made.clone());
                Ok(format!("Made the folder {made}"))
            }
            Command::FilesRename { path, to } => {
                let new = game_files::rename(&self.files_folder()?, &path, &to)?;
                let open = self.open_game()?;
                if open.files_selected.as_deref() == Some(path.as_str()) {
                    open.files_selected = Some(new.clone());
                }
                Ok(format!("Renamed {path} to {new}"))
            }
            Command::FilesDelete { path } => {
                let folder = self.files_folder()?;
                game_files::delete(&folder, &path, &self.recycle_bin)?;
                let open = self.open_game()?;
                let inside = |picked: &String| {
                    picked == &path
                        || picked.starts_with(&format!("{}/", path.trim_end_matches('/')))
                };
                if open.files_selected.as_ref().is_some_and(inside) {
                    open.files_selected = None;
                }
                Ok(format!("Moved {path} to the Recycle Bin"))
            }
            Command::GameOpen { name } => {
                let folder = self.folder()?;
                let game = folder.game(&name).ok_or_else(|| {
                    Problem::new(
                        format!("There's no game called {name}."),
                        "Pick one from Home.",
                    )
                })?;
                self.close_game();
                let watched = folder.code_files(&game);
                // Its level, if it has one in its folder.
                let (level, level_problem) = match game
                    .folder
                    .as_ref()
                    .map(|own| own.join("level.txt"))
                    .filter(|file| file.is_file())
                {
                    Some(file) => match Level::load(folder.root(), &file) {
                        Ok(level) => (Some(level), None),
                        Err(problem) => (None, Some(problem)),
                    },
                    None => (None, None),
                };
                let level_art = level.as_ref().map(LevelArt::load);
                let game_folder = game.folder.clone();
                let (feel, feel_problem) = match feel::load_feel(&folder, &name) {
                    Ok(feel) => (feel, None),
                    Err(problem) => (FeelFile::default(), Some(problem)),
                };
                // Its history starts the first time it's opened here.
                let history = History::new(folder.root(), &game);
                if history.snapshots().is_empty() {
                    let _ = history.take("When it was first opened in the Studio");
                }
                self.open = Some(OpenGame {
                    check_result: check::load(&folder, &game.name),
                    game,
                    build: None,
                    program: None,
                    new_build_ready: false,
                    play: None,
                    check: None,
                    shown_line: None,
                    clip: None,
                    clip_result: None,
                    last_session: None,
                    last_recording: None,
                    watcher: Watcher::new(&watched),
                    watched,
                    looked: Instant::now(),
                    play_when_built: true,
                    first_steps: None,
                    tried_first_recording: false,
                    level,
                    level_problem,
                    level_art,
                    level_needs_restart: false,
                    level_unsent: false,
                    level_sent_at: None,
                    level_looked: Instant::now(),
                    undo: Undo::default(),
                    tunes: std::collections::BTreeMap::new(),
                    feel,
                    feel_problem,
                    tune_text: std::fs::read_to_string(feel::tune_file(&folder, &name))
                        .unwrap_or_default(),
                    snapshots: history.snapshots(),
                    history,
                    edited_since_snapshot: false,
                    last_autosave: Instant::now(),
                    code_spot: None,
                    sound_recipe: None,
                    assets_seen: assets_now(game_folder.as_deref()),
                    scenes_seen: scenes_now(game_folder.as_deref()),
                    scene: None,
                    selected: Vec::new(),
                    view: crate::scene_edit::SceneView::default(),
                    frame_seen: None,
                    art_version: 0,
                    files_selected: None,
                    thumbnail_looked: false,
                });
                self.screen = Screen::Game;
                self.problem = None;
                // Without Rust the game screen says so; that's not this
                // command failing.
                if self.rust.is_ok() {
                    self.start_build()?;
                }
                Ok(format!("Opened {name}"))
            }
            Command::GameBuild => {
                self.start_build()?;
                Ok(String::from("Building"))
            }
            Command::GamePlay => {
                let (keys, buttons) = (self.keys_down.clone(), self.buttons_down.clone());
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.state == PlayState::Frozen => {
                        play.resume(&keys, &buttons);
                        Ok(String::from("Playing"))
                    }
                    Some(play) if play.is_running() => Ok(String::from("Already playing")),
                    _ => {
                        self.start_play(AfterHello::Play, None, None)?;
                        Ok(String::from("Starting"))
                    }
                }
            }
            Command::GameFreeze => {
                self.freeze();
                Ok(String::from("Frozen"))
            }
            Command::GameToggle => {
                let frozen = matches!(
                    self.open_game()?.play_state(),
                    Some(PlayState::Frozen) | Some(PlayState::Stopped { .. }) | None
                );
                if frozen {
                    // Going from Build to Play after changes: a snapshot.
                    let title = self
                        .open_game()?
                        .undo
                        .next_undo()
                        .map_or(String::from("Before playing"), String::from);
                    self.snapshot_if_edited(&title);
                }
                self.do_command(if frozen {
                    Command::GamePlay
                } else {
                    Command::GameFreeze
                })
            }
            Command::GameStep { frames } => {
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.state == PlayState::Frozen => {
                        play.step(frames, &[]);
                        Ok(format!("Played {frames} more frames"))
                    }
                    _ => Err(Problem::new(
                        "The game isn't frozen.",
                        "Freeze it first (Tab), then step.",
                    )),
                }
            }
            Command::GameRestart => {
                self.stop_play();
                self.start_play(AfterHello::Play, None, None)?;
                Ok(String::from("Restarting"))
            }
            Command::GameReplayMyMoves => {
                self.stop_play();
                let script = self
                    .open_game()?
                    .last_recording
                    .clone()
                    .filter(|script| script.frames.unwrap_or(0) > 0)
                    .ok_or_else(|| {
                        Problem::new(
                            "There are no moves to replay yet.",
                            "Press Restart to start the game from the beginning.",
                        )
                    })?;
                let frames = script.frames.unwrap_or(0);
                let seed = script.seed;
                self.start_play(AfterHello::Replay(Box::new(script), frames), seed, None)?;
                Ok(format!("Replaying {frames} frames"))
            }
            Command::GamePopOut => {
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.is_running() => {
                        play.pop_out();
                        Ok(String::from("Popping out"))
                    }
                    _ => Err(Problem::new("The game isn't running.", "Press Play first.")),
                }
            }
            Command::GameBringBack => {
                if let Some(play) = &mut self.open_game()?.play {
                    play.bring_back();
                }
                Ok(String::from("Bringing it back"))
            }
            Command::GameStop => {
                self.stop_play();
                Ok(String::from("Stopped"))
            }
            Command::GameDebugOverlay { on } => {
                let on = on.unwrap_or(!self.debug_overlay);
                self.debug_overlay = on;
                if let Some(play) = self.open.as_mut().and_then(|open| open.play.as_mut()) {
                    play.set_debug_overlay(on);
                }
                Ok(String::from(if on {
                    "Showing the debug overlay"
                } else {
                    "Hiding the debug overlay"
                }))
            }
            Command::GameScreenshot => self.screenshot(),
            Command::LevelPaint {
                cells,
                tile,
                stroke,
            } => {
                let letter = self.letter_named(&tile)?;
                let is_thing = self
                    .level()?
                    .letter(letter)
                    .is_some_and(|l| l.kind == duckforge::prelude::LetterKind::Thing);
                self.edit_level(stroke, |level| {
                    let changed = if is_thing && cells.len() == 1 {
                        usize::from(level.place_thing(cells[0], letter))
                    } else {
                        level.paint(&cells, letter)
                    };
                    (changed, format!("Painted {}", count(changed, "tile")))
                })
            }
            Command::LevelErase { cells, stroke } => self.edit_level(stroke, |level| {
                let changed = level.paint(&cells, '.');
                (changed, format!("Erased {}", count(changed, "tile")))
            }),
            Command::LevelFill { at, tile } => {
                let letter = self.letter_named(&tile)?;
                self.edit_level(None, |level| {
                    let changed = level.fill(at, letter);
                    (changed, format!("Filled {}", count(changed, "tile")))
                })
            }
            Command::LevelPlaceThing { at, thing } => {
                let letter = self.letter_named(&thing)?;
                let title = self
                    .level()?
                    .letter(letter)
                    .map(|l| l.title())
                    .unwrap_or_default();
                self.edit_level(None, |level| {
                    let changed = usize::from(level.place_thing(at, letter));
                    (
                        changed,
                        format!("Put a {title} at column {}, row {}", at.0, at.1),
                    )
                })
            }
            Command::LevelMoveThing { from, to } => self.edit_level(None, |level| {
                let what = level
                    .letter(level.letter_at(from))
                    .map(|l| l.title())
                    .unwrap_or_else(|| String::from("a tile"));
                let changed = usize::from(level.move_thing(from, to));
                (changed, format!("Moved {what}"))
            }),
            Command::LevelDeleteThing { at } => self.edit_level(None, |level| {
                let what = level
                    .letter(level.letter_at(at))
                    .map(|l| l.title())
                    .unwrap_or_else(|| String::from("a tile"));
                let changed = level.paint(&[at], '.');
                (changed, format!("Took away {what}"))
            }),
            Command::LevelMoveArea { from, size, to } => self.edit_level(None, |level| {
                let changed = level.move_area(from, size, to);
                (changed, String::from("Moved an area"))
            }),
            Command::LevelResize { columns, rows } => self.edit_level(None, |level| {
                let changed = usize::from(level.resize(columns, rows));
                (changed, format!("Made the level {columns} by {rows}"))
            }),
            Command::LevelPlayFrom { at } => {
                let level = self.level()?;
                let text = level.with_start_at(at).ok_or_else(|| {
                    Problem::new(
                        "This level has no Start to move.",
                        "Its legend needs a thing with \"start\": true.",
                    )
                })?;
                let given = std::collections::BTreeMap::from([(level.name.clone(), text)]);
                self.stop_play();
                self.start_play(AfterHello::Play, None, Some(given))?;
                Ok(format!("Playing from column {}, row {}", at.0, at.1))
            }
            Command::FeelSet {
                name,
                value,
                stroke,
            } => self.set_feel(
                std::collections::BTreeMap::from([(name.clone(), value)]),
                stroke,
                format!("Feel: {}", feel::tidy(&name)),
            ),
            Command::FeelPreset { name } => {
                let values = self
                    .open_game()?
                    .feel
                    .presets
                    .get(&name)
                    .cloned()
                    .ok_or_else(|| {
                        Problem::new(
                            format!("The feel file has no preset called {name}."),
                            "Pick one from the Presets list.",
                        )
                    })?;
                let said = self.set_feel(values, None, format!("Feel: {name}"))?;
                self.snapshot(&format!("Feel: {name}"));
                Ok(said)
            }
            Command::FeelShowCode { name } => {
                let folder = self.folder()?;
                let open = self.open_game()?;
                let spot = feel::find_code(&folder, &open.game, &name).ok_or_else(|| {
                    Problem::new(
                        format!("The Studio couldn't find tune(\"{name}\" in the game's code."),
                        "The value may be asked for under another name, or in a file outside the game's folder.",
                    )
                })?;
                let said = format!("{}, line {}", spot.file.display(), spot.line);
                open.code_spot = Some((name, spot));
                Ok(said)
            }
            Command::AssetsOpenInQuill { file } => {
                let folder = self.folder()?;
                let file = if file.is_absolute() {
                    file
                } else {
                    folder.root().join(file)
                };
                let quill = assets::find_duck_quill(self.settings.duck_quill.as_deref())
                    .ok_or_else(assets::no_duck_quill)?;
                let command = assets::open_in_quill(&quill, &file, &folder);
                self.console
                    .start("Duck Quill", &command, crate::console::Keep::Everything);
                Ok(format!("Opened {} in Duck Quill", file.display()))
            }
            Command::AssetsPlaySound { name } => {
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.is_running() => {
                        play.play_sound(&name);
                        Ok(format!("Played {name}"))
                    }
                    _ => Err(Problem::new(
                        "The game isn't running.",
                        "Play it, then try again: the game plays its own sounds.",
                    )),
                }
            }
            Command::AssetsShowRecipe { name } => {
                let folder = self.folder()?;
                let open = self.open_game()?;
                let sound = open
                    .sounds()
                    .iter()
                    .find(|sound| sound.name == name)
                    .cloned()
                    .ok_or_else(|| {
                        Problem::new(
                            format!("The game hasn't played a sound called {name} yet."),
                            "Play the game until it does.",
                        )
                    })?;
                let recipe = assets::recipe(&folder, open.game.folder.as_deref(), &sound);
                open.sound_recipe = Some((name.clone(), recipe));
                Ok(format!("The recipe for {name}"))
            }
            Command::EditSnapshot { title } => match self.snapshot(&title) {
                true => Ok(format!("Snapshot: {title}")),
                false => Ok(String::from("Nothing changed since the last snapshot")),
            },
            Command::EditGoBackTo { number } => {
                let open = self.open_game()?;
                let before = (
                    open.level.as_ref().map(|level| level.on_disk().to_string()),
                    open.tune_text.clone(),
                );
                let changed = open.history.go_back_to(number)?;
                open.snapshots = open.history.snapshots();
                let title = open
                    .snapshots
                    .iter()
                    .find(|s| s.number == number)
                    .map_or(String::new(), |s| s.title.clone());
                // Take the files in: the level, the Feel values. (Code changes
                // build by themselves.)
                let mut files = Vec::new();
                if let (Some(level), Some(old)) = (&mut open.level, before.0)
                    && level.reload_if_changed()
                {
                    files.push(FileEdit {
                        path: level.path.clone(),
                        before: old,
                        after: level.on_disk().to_string(),
                    });
                    open.level_unsent = true;
                }
                open.undo.push(Step {
                    title: format!("Went back to: {title}"),
                    files,
                    stroke: None,
                    now: Vec::new(),
                });
                self.send_level();
                self.take_in_tune_file(None);
                Ok(format!(
                    "Went back to \"{title}\" ({} files changed)",
                    changed.len()
                ))
            }
            Command::ObjectSet { ids, set, stroke } => self.object_set(ids, set, stroke),
            Command::ObjectMove {
                ids,
                by,
                to,
                stroke,
            } => self.object_move(ids, by, to, stroke),
            Command::ObjectRename { id, name } => self.object_rename(id, name),
            Command::ObjectSetNow { id, set, stroke } => self.object_set_now(id, set, stroke),
            Command::ObjectMoveNow { id, to, stroke } => {
                let at = crate::scene_edit::pair(duckforge::prelude::Vec2::new(to[0], to[1]));
                let set = serde_json::Map::from_iter([(String::from("at"), at)]);
                self.object_set_now(id, set, stroke)
            }
            Command::ObjectStartHere { ids } => self.object_start_here(ids),
            Command::ObjectBackToStart { ids } => self.object_back_to_start(ids),
            Command::ObjectAdd {
                kind,
                at,
                name,
                set,
            } => self.object_add(kind, at, name, set),
            Command::ObjectTurn {
                ids,
                by,
                to,
                stroke,
            } => self.object_turn(ids, by, to, stroke),
            Command::ObjectResize {
                ids,
                by,
                to,
                stroke,
            } => self.object_resize(ids, by, to, stroke),
            Command::ObjectOrder { ids, to, before } => self.object_order(ids, to, before),
            Command::ObjectDelete { ids } => self.object_delete(ids),
            Command::ObjectDuplicate { ids } => self.object_duplicate(ids),
            Command::ObjectCopy { ids } => self.object_copy(ids),
            Command::ObjectPaste { at, text } => self.object_paste(at, text),
            Command::SelectSet { ids } => self.select(ids, false),
            Command::SelectAdd { ids } => self.select(ids, true),
            Command::SelectAll => self.select_all(),
            Command::SelectNone => self.select(Vec::new(), false),
            Command::ViewTool { tool } => self.view_tool(tool),
            Command::ViewSnap { on, step } => self.view_snap(on, step),
            Command::ViewGrid { on } => self.view_grid(on),
            Command::ViewShow { what } => self.view_show(what),
            Command::EditUndo => {
                let step = self.open_game()?.undo.undo().ok_or_else(|| {
                    Problem::new(
                        "There's nothing to undo.",
                        "Edits made in Build mode can be undone.",
                    )
                })?;
                self.apply_files(&step, true)?;
                self.apply_now(&step, true);
                Ok(format!("Undid: {}", step.title))
            }
            Command::EditRedo => {
                let step = self.open_game()?.undo.redo().ok_or_else(|| {
                    Problem::new("There's nothing to redo.", "Undo something first.")
                })?;
                self.apply_files(&step, false)?;
                self.apply_now(&step, false);
                Ok(format!("Redid: {}", step.title))
            }
            Command::GameInput { event } => {
                let action = action_from(&event)?;
                let open = self.open_game()?;
                match &mut open.play {
                    Some(play) if play.is_running() => {
                        play.queue(action);
                        Ok(String::from("Sent"))
                    }
                    _ => Err(Problem::new("The game isn't running.", "Press Play first.")),
                }
            }
            Command::CheckRun => {
                let folder = self.folder()?;
                self.rust()?;
                self.snapshot_if_edited("Before a Check");
                if self.open_game()?.getting_ready().is_some() {
                    // A game just made is checked by itself in a moment.
                    return Ok(String::from("It's checked as soon as it's ready"));
                }
                // No playthrough yet (the bot couldn't win when the game was
                // made, say): its bot records one first, once, so Check has
                // something to replay.
                let rust = self.rust()?;
                let open = self.open_game()?;
                let tried = std::mem::replace(&mut open.tried_first_recording, false);
                if !tried
                    && let Some(command) = folder.first_playthrough_command(&rust.cargo, &open.game)
                {
                    open.tried_first_recording = true;
                    let job = self.console.start(
                        "First playthrough",
                        &command,
                        crate::console::Keep::Everything,
                    );
                    self.open_game()?.first_steps = Some(FirstSteps::Recording(job));
                    return Ok(String::from(
                        "Recording its first playthrough with the bot, then checking",
                    ));
                }
                let name = self.open_game()?.game.name.clone();
                let check = Check::start(&mut self.console, &folder, &name);
                let open = self.open_game()?;
                open.check = Some(check);
                open.shown_line = None;
                Ok(String::from("Checking"))
            }
            Command::CheckRecordAgain => {
                let folder = self.folder()?;
                let rust = self.rust()?;
                let open = self.open_game()?;
                let command = folder
                    .record_playthrough_command(&rust.cargo, &open.game)
                    .ok_or_else(|| {
                        Problem::new(
                            "This game has no bot to record a playthrough.",
                            "Record one yourself: play it and press F10, or ask an agent to \
                             write a bot (bot.rs).",
                        )
                    })?;
                let job = self.console.start(
                    "Record the playthrough",
                    &command,
                    crate::console::Keep::Everything,
                );
                // When it's done, the check runs again (as for a new game).
                self.open_game()?.first_steps = Some(FirstSteps::Recording(job));
                Ok(String::from("Recording the playthrough again"))
            }
            Command::CheckShowProblem { line } => {
                let open = self.open_game()?;
                let lines = open
                    .check_result
                    .as_ref()
                    .map_or(0, |result| result.rows.len());
                open.shown_line = line.filter(|line| *line < lines);
                Ok(String::from("Shown"))
            }
            Command::ClipMake => self.make_clip(),
            Command::ConsoleRun { line } => {
                let folder = self.folder()?;
                self.console.run_typed(&line, folder.root());
                Ok(format!("Running {line}"))
            }
        }
    }

    fn new_game(&mut self, starter: &str, typed: &str) -> Result<String, Problem> {
        let folder = self.folder()?;
        self.rust()?;
        let starter = STARTERS
            .iter()
            .find(|known| known.kind == starter)
            .ok_or_else(|| {
                Problem::new(
                    format!("There's no starter called {starter}."),
                    "Pick one of: empty, platformer, topdown, shooter, runner3d, tiny.",
                )
            })?;
        let name =
            name_from_title(typed).map_err(|why| Problem::new(why, "Then press Make it."))?;
        if folder.game(&name).is_some() {
            return Err(Problem::new(
                format!(
                    "There's already a game called {}.",
                    crate::folder::title_of(&name)
                ),
                "Pick another name.",
            ));
        }
        let command = folder.new_game_command(&name, starter.kind);
        let job = self
            .console
            .start("New game", &command, crate::console::Keep::Everything);
        self.making = Some(Making {
            title: crate::folder::title_of(&name),
            name,
            job,
        });
        Ok(String::from("Making the game"))
    }

    fn start_build(&mut self) -> Result<(), Problem> {
        let folder = self.folder()?;
        let rust = self.rust()?;
        let name = self.open_game()?.game.name.clone();
        if self.open_game()?.is_building() {
            return Ok(());
        }
        let expected = self.settings.build_parts.get(&name).copied().unwrap_or(0);
        let build = Build::start(&mut self.console, &folder, &rust, &name, expected);
        self.open_game()?.build = Some(build);
        Ok(())
    }

    /// Starts the built game. With no build yet, it starts when there is one.
    /// Changes Feel values: the game takes them at once, the tune file is
    /// saved, and it's a step for Undo.
    fn set_feel(
        &mut self,
        values: std::collections::BTreeMap<String, f32>,
        stroke: Option<u64>,
        title: String,
    ) -> Result<String, Problem> {
        let folder = self.folder()?;
        let open = self.open_game()?;
        let path = feel::tune_file(&folder, &open.game.name);
        let (before, after) = feel::write_tune_values(&path, &values)?;
        open.tune_text = after.clone();
        open.tunes
            .extend(values.iter().map(|(name, value)| (name.clone(), *value)));
        if let Some(play) = &mut open.play {
            play.send_tunes(values);
        }
        open.undo.push(Step {
            title: title.clone(),
            files: vec![FileEdit {
                path,
                before,
                after,
            }],
            stroke,
            now: Vec::new(),
        });
        open.edited_since_snapshot = true;
        Ok(title)
    }

    /// Reads the tune file again if it changed (Undo, going back, an
    /// editor), and sends the game the values that changed. `before` is
    /// its text before, for a step for Undo (a change made outside).
    fn take_in_tune_file(&mut self, before: Option<String>) {
        let Some(folder) = self.folder.clone() else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        let path = feel::tune_file(&folder, &open.game.name);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text == open.tune_text {
            return;
        }
        let old = std::mem::replace(&mut open.tune_text, text.clone());
        let values = feel::read_tune_file(&path);
        // A value the file doesn't have is the one in the code.
        let defaults: std::collections::BTreeMap<String, f32> = open
            .play
            .as_ref()
            .map(|play| {
                play.tunes
                    .iter()
                    .map(|t| (t.name.clone(), t.default))
                    .collect()
            })
            .unwrap_or_default();
        let mut wanted = defaults;
        wanted.extend(values);
        let changed: std::collections::BTreeMap<String, f32> = wanted
            .into_iter()
            .filter(|(name, value)| open.tunes.get(name) != Some(value))
            .collect();
        open.tunes
            .extend(changed.iter().map(|(name, value)| (name.clone(), *value)));
        if let Some(play) = &mut open.play {
            play.send_tunes(changed);
        }
        if before.is_some() {
            open.undo.push(Step {
                title: String::from("Feel changed outside the Studio"),
                files: vec![FileEdit {
                    path,
                    before: old,
                    after: text,
                }],
                stroke: None,
                now: Vec::new(),
            });
            open.edited_since_snapshot = true;
        }
    }

    /// Takes a snapshot of the open game's files. True if anything had
    /// changed since the last one.
    fn snapshot(&mut self, title: &str) -> bool {
        let Some(open) = &mut self.open else {
            return false;
        };
        open.edited_since_snapshot = false;
        open.last_autosave = Instant::now();
        let taken = matches!(open.history.take(title), Ok(Some(_)));
        if taken {
            open.snapshots = open.history.snapshots();
        }
        taken
    }

    /// Takes a snapshot if something was edited since the last one.
    fn snapshot_if_edited(&mut self, title: &str) {
        if self
            .open
            .as_ref()
            .is_some_and(|open| open.edited_since_snapshot)
        {
            self.snapshot(title);
        }
    }

    /// The open game's level, or why there isn't one.
    fn level(&mut self) -> Result<&mut Level, Problem> {
        let open = self.open_game()?;
        if let Some(problem) = &open.level_problem {
            return Err(problem.clone());
        }
        open.level.as_mut().ok_or_else(|| {
            Problem::new(
                "This game has no tile level, so there's nothing to paint.",
                "You can still change how it feels.",
            )
        })
    }

    /// The letter for a tile or thing's name in the open level.
    fn letter_named(&mut self, name: &str) -> Result<char, Problem> {
        let level = self.level()?;
        match level.letter_named(name) {
            Some(letter) => Ok(letter.letter),
            None => {
                let names: Vec<&str> = level.letters.iter().map(|l| l.name.as_str()).collect();
                Err(Problem::new(
                    format!("This level has no tile or thing called {name}."),
                    format!("Pick one of: {}.", names.join(", ")),
                ))
            }
        }
    }

    /// Makes an edit to the level: `change` edits it and says how many
    /// cells changed and what it did, in words. A change is saved to the
    /// file at once, becomes a step for Undo, and goes to the game.
    fn edit_level(
        &mut self,
        stroke: Option<u64>,
        change: impl FnOnce(&mut Level) -> (usize, String),
    ) -> Result<String, Problem> {
        let level = self.level()?;
        // The file exactly as it is (Undo puts back every byte of it).
        let before = level.on_disk().to_string();
        let (changed, said) = change(level);
        if changed == 0 {
            return Ok(String::from("Nothing changed"));
        }
        self.open_game()?.edited_since_snapshot = true;
        let level = self.level()?;
        level.save()?;
        let step = Step {
            title: said.clone(),
            files: vec![FileEdit {
                path: level.path.clone(),
                before,
                after: level.on_disk().to_string(),
            }],
            stroke,
            now: Vec::new(),
        };
        let open = self.open_game()?;
        open.undo.push(step);
        open.level_unsent = true;
        // A paint stroke goes to the game a few times a second, not every
        // frame; anything else at once.
        let recently = open
            .level_sent_at
            .is_some_and(|at| at.elapsed() < Duration::from_millis(200));
        if stroke.is_none() || !recently {
            self.send_level();
        }
        Ok(said)
    }

    /// Sends the level's text to the running game, if it hasn't had it.
    fn send_level(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        if !open.level_unsent {
            return;
        }
        open.level_unsent = false;
        open.level_sent_at = Some(Instant::now());
        let (Some(level), Some(play)) = (&open.level, &mut open.play) else {
            return;
        };
        // The game names the file as it read it (the picture says).
        let name = play
            .maps
            .first()
            .map(|map| map.file.clone())
            .filter(|file| !file.is_empty())
            .unwrap_or_else(|| level.name.clone());
        play.send_file(&name, level.on_disk());
    }

    /// Writes a step's files back (Undo: the "before" texts; Redo: the
    /// "after"), then takes the change in: the level reads its file again
    /// and the game gets it.
    fn apply_files(&mut self, step: &Step, before: bool) -> Result<(), Problem> {
        for edit in &step.files {
            let text = if before { &edit.before } else { &edit.after };
            if text.is_empty() {
                // It wasn't there (a tune file made by the first Feel change).
                let _ = std::fs::remove_file(&edit.path);
            } else {
                crate::level::write_whole(&edit.path, text)?;
            }
        }
        let root = self.folder()?.root().to_path_buf();
        let open = self.open_game()?;
        if let Some(level) = &mut open.level
            && level.reload_if_changed()
        {
            open.level_unsent = true;
        }
        // A scene file put back: the game takes it, and it isn't a change
        // from outside.
        let scenes = scenes_now(open.game.folder.as_deref());
        for (file, text) in &scenes {
            if open.scenes_seen.get(file) != Some(text)
                && let Some(play) = &mut open.play
            {
                play.send_file(&name_from(&root, file), text);
            }
        }
        open.scenes_seen = scenes;
        open.edited_since_snapshot = true;
        self.send_level();
        self.take_in_tune_file(None);
        Ok(())
    }

    /// F12: saves the game's picture, as the game drew it, in
    /// `out/screenshots/`. Says where.
    fn screenshot(&mut self) -> Result<String, Problem> {
        let folder = self.folder()?;
        let open = self.open_game()?;
        let picture = open
            .play
            .as_ref()
            .and_then(|play| play.picture.as_ref())
            .ok_or_else(|| {
                Problem::new(
                    "There's no picture to save yet.",
                    "Play the game, then press F12.",
                )
            })?;
        let mut canvas =
            duckforge::prelude::Canvas::new(picture.width as usize, picture.height as usize);
        for (i, pixel) in picture.pixels.iter().enumerate() {
            let (x, y) = (i % picture.width as usize, i / picture.width as usize);
            let color = duckforge::prelude::Color::rgb(
                (pixel >> 16) as u8,
                (pixel >> 8) as u8,
                *pixel as u8,
            );
            canvas.set_pixel(x as i32, y as i32, color);
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let saved_in = folder.screenshots_folder();
        let path = saved_in.join(format!("{}-{stamp}.png", open.game.name));
        std::fs::create_dir_all(&saved_in)
            .and_then(|()| canvas.save_png(&path))
            .map_err(|problem| {
                Problem::new(
                    "The screenshot couldn't be saved.",
                    "Check there's space on the disk, then press F12 again.",
                )
                .with_details(format!("{}: {problem}", path.display()))
            })?;
        Ok(format!(
            "Saved a screenshot: out/screenshots/{}",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
    }

    fn start_play(
        &mut self,
        then: AfterHello,
        seed: Option<u32>,
        given: Option<std::collections::BTreeMap<String, String>>,
    ) -> Result<(), Problem> {
        let folder = self.folder()?;
        let open = self.open_game()?;
        let Some(program) = open.program.clone() else {
            open.play_when_built = true;
            if !open.is_building() {
                self.start_build()?;
            }
            return Ok(());
        };
        let name = open.game.name.clone();
        let mut play = Play::start(
            &mut self.console,
            &folder,
            &name,
            &program,
            seed,
            then,
            given,
        )?;
        play.set_debug_overlay(self.debug_overlay);
        let open = self.open_game()?;
        // The level as the game will read it, for the Studio's own record.
        if let Some(level) = &open.level {
            let legend = std::fs::read_to_string(&level.legend_path).unwrap_or_default();
            play.note_files_at_start(std::collections::BTreeMap::from([
                (level.name.clone(), level.on_disk().to_string()),
                (level.legend_name.clone(), legend),
            ]));
        }
        open.level_needs_restart = false;
        open.play = Some(play);
        open.new_build_ready = false;
        Ok(())
    }

    /// Freezes the game and saves the session so far.
    fn freeze(&mut self) {
        let (Some(open), Some(folder)) = (&mut self.open, &self.folder) else {
            return;
        };
        let Some(play) = &mut open.play else {
            return;
        };
        if play.state == PlayState::Playing {
            play.freeze();
            // Let the tick in flight land, so the recording is whole.
            play.wait_for_picture(&self.console, Duration::from_millis(250));
            if let Ok(path) = play.save_session(&self.console, folder) {
                open.last_session = Some(path);
            }
        }
        // For the Feel panel and the Sounds list.
        play.ask_about_feel();
    }

    /// Stops the running game, keeping its session.
    fn stop_play(&mut self) {
        self.first_card_picture(true);
        let (Some(open), Some(folder)) = (&mut self.open, &self.folder) else {
            return;
        };
        let Some(play) = &mut open.play else {
            return;
        };
        if play.is_running() {
            if play.hello.is_some() {
                let recording = play.recording(&self.console, Duration::from_millis(500));
                open.last_recording = Some(recording);
                if let Ok(path) = play.save_session(&self.console, folder) {
                    open.last_session = Some(path);
                }
            }
            play.stop(&mut self.console);
        }
    }

    fn close_game(&mut self) {
        self.stop_play();
        if let Some(open) = self.open.take() {
            for job in [
                open.build
                    .as_ref()
                    .filter(|b| b.state == BuildState::Running)
                    .map(|b| b.job),
                open.check
                    .as_ref()
                    .filter(|c| c.result.is_none())
                    .map(|c| c.job),
                open.clip
                    .as_ref()
                    .filter(|c| c.result.is_none())
                    .map(|c| c.job),
            ]
            .into_iter()
            .flatten()
            {
                self.console.stop(job);
            }
        }
    }

    fn make_clip(&mut self) -> Result<String, Problem> {
        let folder = self.folder()?;
        let rust = self.rust()?;
        // The run so far, if a game is running; else the last one saved.
        if let Some(open) = &mut self.open {
            let playing = open
                .play
                .as_mut()
                .filter(|play| play.is_running() && play.frame > 0);
            if let Some(play) = playing
                && let Ok(path) = play.save_session(&self.console, &folder)
            {
                open.last_session = Some(path);
            }
        }
        let open = self.open_game()?;
        let name = open.game.name.clone();
        let session = open
            .last_session
            .clone()
            .or_else(|| {
                let latest = folder.sessions_folder(&name).join("latest.json");
                latest.is_file().then_some(latest)
            })
            .ok_or_else(|| {
                Problem::new(
                    "There's no run to make a clip of yet.",
                    "Play the game for a moment, then press Clip: a clip is the last ten \
                     seconds of what you played.",
                )
            })?;
        let clip = Clip::start(&mut self.console, &folder, &rust.cargo, &name, &session);
        let open = self.open_game()?;
        open.clip = Some(clip);
        open.clip_result = None;
        Ok(String::from("Making a clip"))
    }

    /// Takes in whatever has happened since last time: output from the
    /// commands running, a finished build, changed code, the game's
    /// messages. Call it often; it never waits.
    pub fn update(&mut self, now: Instant) {
        let events = self.console.poll();
        for (job, event) in &events {
            self.take_job_event(*job, event);
        }
        self.watch_code(now);
        self.watch_level(now);
        self.first_card_picture(false);
        if let Some(open) = &mut self.open
            && let Some(play) = &mut open.play
        {
            let was_running = play.is_running();
            play.pump(&self.console);
            if was_running && !play.is_running() {
                // It ended by itself: keep what the Studio recorded, so
                // "replay my moves" can get back to just before.
                let recording = play.recording(&self.console, Duration::ZERO);
                open.last_recording = Some(recording);
                if let Some(folder) = &self.folder
                    && let Ok(path) = play.save_session(&self.console, folder)
                {
                    open.last_session = Some(path);
                }
            }
        }
        self.refresh_scene();
        self.forget_moves_of_a_moment_gone();
    }

    fn take_job_event(&mut self, job: JobId, event: &JobEvent) {
        // A new game being made.
        if self.making.as_ref().is_some_and(|making| making.job == job) {
            match event {
                JobEvent::Ended(Some(0)) => {
                    let name = self
                        .making
                        .take()
                        .map(|making| making.name)
                        .unwrap_or_default();
                    let _ = self.run(Command::GameOpen { name });
                    // Once it's built: a first playthrough from its bot,
                    // then a first Check, as tools/new_game does.
                    if let Some(open) = &mut self.open {
                        open.first_steps = Some(FirstSteps::WaitingForBuild);
                    }
                }
                JobEvent::Ended(_) | JobEvent::CouldntStart(_) => {
                    self.making = None;
                    self.problem = Some(
                        Problem::new(
                            "The new game couldn't be made.",
                            "Look at the details, or copy them for an agent.",
                        )
                        .with_details(self.console.tail(job, 40)),
                    );
                }
                JobEvent::Line(..) => {}
            }
            return;
        }
        let Some(folder) = self.folder.clone() else {
            return;
        };
        if let Some(at) = self.thumbnail_jobs.iter().position(|(id, _)| *id == job) {
            if matches!(event, JobEvent::Ended(_) | JobEvent::CouldntStart(_)) {
                let (_, name) = self.thumbnail_jobs.remove(at);
                if folder.keep_thumbnail(&name) {
                    self.refresh_cards();
                }
            }
            return;
        }
        let Some(open) = &mut self.open else {
            return;
        };
        if open.first_steps == Some(FirstSteps::Recording(job)) {
            if matches!(event, JobEvent::Ended(_) | JobEvent::CouldntStart(_)) {
                open.first_steps = None;
                let _ = self.run(Command::CheckRun);
            }
            return;
        }
        if let Some(build) = open.build.as_mut().filter(|build| build.job == job) {
            if build.take(event) {
                let mut start = false;
                let mut built = false;
                if let BuildState::Done { program } = &build.state {
                    self.settings
                        .build_parts
                        .insert(build.game.clone(), build.parts.max(1));
                    self.settings.save();
                    match build::copy_to_play(&folder, program) {
                        Ok(copy) => {
                            built = true;
                            open.program = Some(copy);
                            let running = open.play.as_ref().is_some_and(Play::is_running);
                            open.new_build_ready = running;
                            start = !running
                                && (open.play_when_built
                                    || matches!(
                                        open.play_state(),
                                        Some(PlayState::Stopped { .. })
                                    ));
                        }
                        Err(problem) => build.state = BuildState::Failed(problem),
                    }
                }
                let first_steps = built && open.first_steps == Some(FirstSteps::WaitingForBuild);
                if start {
                    open.play_when_built = false;
                    if let Err(problem) = self.start_play(AfterHello::Play, None, None) {
                        self.problem = Some(problem);
                    }
                }
                if first_steps {
                    self.record_first_playthrough();
                }
            }
            return;
        }
        if let Some(check) = open.check.as_mut().filter(|check| check.job == job) {
            if check.take(event, &folder) {
                match check.result.take() {
                    Some(Ok(mut result)) => {
                        // The level's own lines: rules, reach, old playthroughs.
                        if let Some(level) = &open.level {
                            let jump = jump_from(&open.tunes);
                            let rows = check::level_rows(&folder, &open.game.name, level, jump);
                            check::add_level_rows(&mut result, rows);
                            check::picture_the_place(&mut result, &folder, level);
                            let _ = check::save(&folder, &result);
                        }
                        open.check_result = Some(result);
                    }
                    Some(Err(problem)) => self.problem = Some(problem),
                    None => {}
                }
                open.check = None;
                // Checked, and still no picture for its card (no
                // playthrough to replay): two seconds of it running.
                let name = open.game.name.clone();
                if folder.card_picture(&name).is_none() {
                    let _ = self.make_thumbnail(&name);
                }
            }
            return;
        }
        if let Some(clip) = open.clip.as_mut().filter(|clip| clip.job == job)
            && clip.take(event)
        {
            open.clip_result = clip.result.take();
            open.clip = None;
        }
    }

    /// For a game just made: has its bot record a first playthrough (so
    /// Check has something to replay), then checks it.
    fn record_first_playthrough(&mut self) {
        let (Some(folder), Ok(rust)) = (self.folder.clone(), self.rust.clone()) else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        match folder.first_playthrough_command(&rust.cargo, &open.game) {
            Some(command) => {
                let job = self.console.start(
                    "First playthrough",
                    &command,
                    crate::console::Keep::Everything,
                );
                open.first_steps = Some(FirstSteps::Recording(job));
            }
            None => {
                open.first_steps = None;
                let _ = self.run(Command::CheckRun);
            }
        }
    }

    /// Keeps the level and the game in step: sends the level after a
    /// paint stroke's last change, takes in the game's answers, and reads
    /// the file again if it was changed outside the Studio (an editor, an
    /// agent), which becomes a step for Undo.
    fn watch_level(&mut self, now: Instant) {
        let Some(open) = &mut self.open else {
            return;
        };
        if let Some(play) = &mut open.play {
            for (_, took_it) in play.take_file_answers() {
                open.level_needs_restart |= !took_it;
            }
        }
        let quiet = open
            .level_sent_at
            .is_none_or(|at| now.saturating_duration_since(at) >= Duration::from_millis(200));
        if open.level_unsent && quiet {
            self.send_level();
            return;
        }
        // Outside changes, twice a second (with the code).
        if now.saturating_duration_since(open.level_looked) < Duration::from_millis(500) {
            return;
        }
        open.level_looked = now;
        // The game's tune values, as it said them.
        if let Some(play) = &open.play {
            for tune in &play.tunes {
                open.tunes.entry(tune.name.clone()).or_insert(tune.value);
            }
        }
        // An autosave every five minutes while there are changes.
        if open.edited_since_snapshot
            && now.saturating_duration_since(open.last_autosave) > Duration::from_secs(300)
        {
            self.snapshot("Autosave");
            return;
        }
        let tune_text = std::fs::read_to_string(
            self.folder
                .as_ref()
                .map(|f| feel::tune_file(f, &open.game.name))
                .unwrap_or_default(),
        )
        .unwrap_or_default();
        if tune_text != open.tune_text {
            let before = open.tune_text.clone();
            self.take_in_tune_file(Some(before));
            return;
        }
        let Some(open) = &mut self.open else {
            return;
        };
        // A picture saved in Duck Quill (or anything in its assets folder):
        // the game loads its art again, and the palette is drawn again.
        let assets = assets_now(open.game.folder.as_deref());
        if assets != open.assets_seen {
            let changed: Vec<PathBuf> = assets
                .iter()
                .filter(|(file, seen)| open.assets_seen.get(*file) != Some(seen))
                .map(|(file, _)| file.clone())
                .collect();
            open.assets_seen = assets;
            open.art_version += 1;
            if let Some(level) = &open.level {
                open.level_art = Some(LevelArt::load(level));
            }
            if let (Some(play), Some(root)) = (
                &mut open.play,
                self.folder.as_ref().map(|f| f.root().to_path_buf()),
            ) {
                for file in changed {
                    let name = file
                        .strip_prefix(&root)
                        .unwrap_or(&file)
                        .to_string_lossy()
                        .replace('\\', "/");
                    let text = std::fs::read_to_string(&file).unwrap_or_default();
                    play.send_file(&name, &text);
                }
            }
        }
        // A scene file saved outside the Studio: the game takes it (frozen
        // or playing), and it's a step for Undo.
        let scenes = scenes_now(open.game.folder.as_deref());
        if scenes != open.scenes_seen {
            let root = self.folder.as_ref().map(|f| f.root().to_path_buf());
            for (file, text) in &scenes {
                let before = open.scenes_seen.get(file).cloned().unwrap_or_default();
                if before == *text {
                    continue;
                }
                open.undo.push(Step {
                    title: String::from("Changed outside the Studio"),
                    files: vec![FileEdit {
                        path: file.clone(),
                        before,
                        after: text.clone(),
                    }],
                    stroke: None,
                    now: Vec::new(),
                });
                open.edited_since_snapshot = true;
                if let (Some(play), Some(root)) = (&mut open.play, &root) {
                    play.send_file(&name_from(root, file), text);
                }
            }
            open.scenes_seen = scenes;
        }
        if let Some(level) = &mut open.level {
            let before = level.on_disk().to_string();
            if level.reload_if_changed() {
                open.undo.push(Step {
                    title: String::from("Changed outside the Studio"),
                    files: vec![FileEdit {
                        path: level.path.clone(),
                        before,
                        after: level.on_disk().to_string(),
                    }],
                    stroke: None,
                    now: Vec::new(),
                });
                open.level_unsent = true;
                self.send_level();
            }
        }
    }

    /// Twice a second, looks at the game's code. Once a change has stayed
    /// unchanged for a second, the game is built again.
    fn watch_code(&mut self, now: Instant) {
        let Some(open) = &mut self.open else {
            return;
        };
        if now.saturating_duration_since(open.looked) < Duration::from_millis(500) {
            return;
        }
        open.looked = now;
        if open.watcher.look(&open.watched, now) && self.rust.is_ok() {
            if open.is_building() {
                // Changed again mid-build: build once more when this one ends.
                if let Some(build) = &open.build {
                    self.console.stop(build.job);
                }
                open.build = None;
            }
            let _ = self.start_build();
        }
    }

    /// The start of one of the app's own frames: takes in what's new, and
    /// (while playing) sends the game its tick with what the player did.
    pub fn begin_frame(&mut self, now: Instant, actions: &[InputAction]) {
        self.update(now);
        if let Some(play) = self.open.as_mut().and_then(|open| open.play.as_mut()) {
            play.begin_frame(now, actions);
        }
    }

    /// Waits up to `patience` for the picture of the tick this frame sent.
    /// Returns true if there's a new picture to show.
    pub fn wait_for_picture(&mut self, patience: Duration) -> bool {
        match self.open.as_mut().and_then(|open| open.play.as_mut()) {
            Some(play) => play.wait_for_picture(&self.console, patience),
            None => false,
        }
    }

    pub fn play(&self) -> Option<&Play> {
        self.open.as_ref().and_then(|open| open.play.as_ref())
    }

    /// The Timing line of the game that's playing.
    pub fn timing(&self, now: Instant) -> Timing {
        self.play()
            .map_or(Timing::default(), |play| play.timing(now))
    }

    /// Whether the game that's open stopped by crashing.
    pub fn crashed(&self) -> Option<&Problem> {
        match self.play().map(|play| &play.state) {
            Some(PlayState::Stopped {
                ending: Ending::Crashed,
                problem: Some(problem),
            }) => Some(problem),
            _ => None,
        }
    }

    /// Whether any command the Studio started is still running.
    pub fn is_busy(&self) -> bool {
        self.console
            .entries()
            .iter()
            .any(|entry| entry.state == JobState::Running && entry.title != "Play")
    }
}

impl Drop for Studio {
    /// Closing the Studio ends the game politely (so it can save), then
    /// stops everything else it started.
    fn drop(&mut self) {
        self.close_game();
        self.settings.save();
    }
}

/// An input, from the fields of a `game.input` command: the same words as
/// an event in an input script.
fn action_from(event: &serde_json::Map<String, serde_json::Value>) -> Result<InputAction, Problem> {
    let mut event = event.clone();
    event.insert(String::from("frame"), serde_json::Value::from(1));
    let script = serde_json::json!({ "events": [event] }).to_string();
    InputScript::parse(&script)
        .ok()
        .and_then(|script| script.events.first().map(|event| event.action))
        .ok_or_else(|| {
            Problem::new(
                "That input can't be read.",
                "Write it as in an input script, like {\"do\": \"game.input\", \"press\": \
                 \"Space\"}. docs/input-format.md lists them all.",
            )
        })
}

/// "There's no such file" for the Files panel's commands.
fn no_such_file(place: &str) -> Problem {
    Problem::new(
        format!("There's no {place} in the game's folder."),
        "Pick a file in the Files panel.",
    )
}

/// Saves a picture of the game as a PNG (for a card).
fn save_picture(path: &std::path::Path, picture: &crate::play::Picture) -> std::io::Result<()> {
    use duckforge::canvas::Canvas;
    use duckforge::color::Color;
    let (width, height) = (picture.width as usize, picture.height as usize);
    let mut canvas = Canvas::new(width.max(1), height.max(1));
    for (i, rgb) in picture.pixels.iter().enumerate().take(width * height) {
        let (x, y) = ((i % width) as i32, (i / width) as i32);
        let [_, r, g, b] = rgb.to_be_bytes();
        canvas.set_pixel(x, y, Color::rgb(r, g, b));
    }
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    canvas.save_png(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_command_reads_from_json_and_is_in_the_table() {
        let written = [
            r#"{"do": "home.open"}"#,
            r#"{"do": "home.choose_folder", "folder": "C:/Duck Forge"}"#,
            r#"{"do": "home.check_again"}"#,
            r#"{"do": "game.new", "starter": "platformer", "name": "Bee Maze"}"#,
            r#"{"do": "game.open", "name": "duck_peaks"}"#,
            r#"{"do": "game.delete", "name": "bee_maze"}"#,
            r#"{"do": "game.rename", "name": "bee_maze", "to": "Bee Run"}"#,
            r#"{"do": "game.duplicate", "name": "duck_peaks", "to": "My Peaks"}"#,
            r#"{"do": "game.thumbnail", "name": "bee_maze"}"#,
            r#"{"do": "game.build"}"#,
            r#"{"do": "game.play"}"#,
            r#"{"do": "game.freeze"}"#,
            r#"{"do": "game.toggle"}"#,
            r#"{"do": "game.step", "frames": 5}"#,
            r#"{"do": "game.restart"}"#,
            r#"{"do": "game.replay_my_moves"}"#,
            r#"{"do": "game.pop_out"}"#,
            r#"{"do": "game.bring_back"}"#,
            r#"{"do": "game.stop"}"#,
            r#"{"do": "game.debug_overlay", "on": true}"#,
            r#"{"do": "game.screenshot"}"#,
            r#"{"do": "game.input", "press": "Space"}"#,
            r#"{"do": "check.run"}"#,
            r#"{"do": "check.show_problem", "line": 2}"#,
            r#"{"do": "check.record_again"}"#,
            r#"{"do": "clip.make"}"#,
            r#"{"do": "console.run", "line": "cargo test"}"#,
            r#"{"do": "level.paint", "cells": [[14, 9], [15, 9]], "tile": "plank", "stroke": 3}"#,
            r#"{"do": "level.erase", "cells": [[14, 9]]}"#,
            r#"{"do": "level.fill", "at": [0, 12], "tile": "grass"}"#,
            r#"{"do": "level.place_thing", "at": [3, 12], "thing": "coin"}"#,
            r#"{"do": "level.move_thing", "from": [3, 12], "to": [4, 12]}"#,
            r#"{"do": "level.delete_thing", "at": [4, 12]}"#,
            r#"{"do": "level.move_area", "from": [1, 1], "size": [3, 2], "to": [5, 1]}"#,
            r#"{"do": "level.resize", "columns": 90, "rows": 15}"#,
            r#"{"do": "level.play_from", "at": [20, 12]}"#,
            r#"{"do": "feel.set", "name": "jump_speed", "value": 380, "stroke": 4}"#,
            r#"{"do": "feel.preset", "name": "Floaty"}"#,
            r#"{"do": "feel.show_code", "name": "gravity"}"#,
            r#"{"do": "assets.open_in_quill", "file": "examples/template/assets/tiles.png"}"#,
            r#"{"do": "assets.play_sound", "name": "jump"}"#,
            r#"{"do": "assets.show_recipe", "name": "music"}"#,
            r#"{"do": "edit.snapshot", "title": "Before a big change"}"#,
            r#"{"do": "edit.go_back_to", "number": 3}"#,
            r#"{"do": "edit.undo"}"#,
            r#"{"do": "edit.redo"}"#,
            r#"{"do": "files.select", "path": "assets/duck.png"}"#,
            r#"{"do": "files.open", "path": "level.txt"}"#,
            r#"{"do": "files.play", "path": "assets/jump.wav"}"#,
            r#"{"do": "files.add", "files": ["C:/Users/me/Desktop/duck.png"], "to": "assets"}"#,
            r#"{"do": "files.new_folder", "path": "assets/music"}"#,
            r#"{"do": "files.rename", "path": "assets/duck.png", "to": "hero.png"}"#,
            r#"{"do": "files.delete", "path": "assets/old.png"}"#,
            r#"{"do": "object.set", "ids": [7], "set": {"layer": "front", "speed": 60}}"#,
            r#"{"do": "object.move", "ids": [7, 9], "by": [8, 0]}"#,
            r#"{"do": "object.rename", "id": 7, "name": "snail"}"#,
            r#"{"do": "object.set_now", "id": 7, "set": {"flip": true}}"#,
            r#"{"do": "object.move_now", "id": 9, "to": [96, 208], "stroke": 3}"#,
            r#"{"do": "object.start_here", "ids": [9]}"#,
            r#"{"do": "object.back_to_start", "ids": [9]}"#,
            r#"{"do": "object.add", "kind": "picture", "set": {"picture": "assets/crate.png"}}"#,
            r#"{"do": "object.turn", "ids": [4], "by": 15, "stroke": 2}"#,
            r#"{"do": "object.resize", "ids": [4], "to": [2, 1.5]}"#,
            r#"{"do": "object.order", "ids": [4], "to": "front"}"#,
            r#"{"do": "object.delete", "ids": [4, 5]}"#,
            r#"{"do": "object.duplicate", "ids": [4]}"#,
            r#"{"do": "object.copy", "ids": [4]}"#,
            r#"{"do": "object.paste", "at": [96, 208]}"#,
            r#"{"do": "select.set", "ids": [7, 9]}"#,
            r#"{"do": "select.add", "ids": [4]}"#,
            r#"{"do": "select.all"}"#,
            r#"{"do": "select.none"}"#,
            r#"{"do": "view.tool", "tool": "move"}"#,
            r#"{"do": "view.snap", "on": true, "step": 16}"#,
            r#"{"do": "view.grid", "on": false}"#,
            r#"{"do": "view.show", "what": "start"}"#,
        ];
        let mut names = Vec::new();
        for text in written {
            let command: Command =
                serde_json::from_str(text).unwrap_or_else(|e| panic!("{text}: {e}"));
            let back = serde_json::to_value(&command).unwrap();
            names.push(back["do"].as_str().unwrap().to_string());
        }
        let table: Vec<&str> = COMMANDS.iter().map(|info| info.name).collect();
        assert_eq!(
            names, table,
            "the table lists every command, in the same order"
        );
        assert!(serde_json::from_str::<Command>(r#"{"do": "game.fly"}"#).is_err());
    }

    #[test]
    fn inputs_are_written_as_in_an_input_script() {
        let read = |text: &str| {
            let Command::GameInput { event } = serde_json::from_str(text).unwrap() else {
                panic!("an input");
            };
            action_from(&event)
        };
        assert_eq!(
            read(r#"{"do": "game.input", "press": "Space"}"#),
            Ok(InputAction::Press(Key::Space))
        );
        assert_eq!(
            read(r#"{"do": "game.input", "mouse_press": "Left"}"#),
            Ok(InputAction::MousePress(MouseButton::Left))
        );
        assert!(read(r#"{"do": "game.input", "press": "Spcae"}"#).is_err());
    }

    /// A small games folder: a game called bee_maze (with a folder) and
    /// dodge, which came with Duck Forge.
    fn small_games_folder(test: &crate::test_folder::TestFolder) -> PathBuf {
        let root = test.join("Duck Forge");
        for (name, text) in [
            (
                "Cargo.toml",
                "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"bee_maze\"\ntest = true\n",
            ),
            ("examples/dodge.rs", "fn main() {}\n"),
            (
                "examples/bee_maze/main.rs",
                "const LEVEL: &str = \"examples/bee_maze/level.txt\";\n",
            ),
            ("examples/bee_maze/assets/duck.png", "png"),
            ("Desktop/song.wav", "RIFF"),
        ] {
            let path = if name.starts_with("Desktop") {
                test.join(name)
            } else {
                root.join(name)
            };
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        root
    }

    #[test]
    fn home_and_files_panel_commands_do_their_work() {
        let test = crate::test_folder::TestFolder::new("studio-home-files");
        let root = small_games_folder(&test);
        let mut studio = Studio::new(Settings {
            games_folder: Some(root.clone()),
            ..Settings::default()
        });
        studio.recycle_bin = RecycleBin::Folder(test.join("Recycle Bin"));
        // Studio::new gives Rust a look: no building here.
        studio.rust = Err(Problem::new("No Rust in this test.", ""));
        let run = |studio: &mut Studio, json: &str| {
            let command: Command = serde_json::from_str(json).unwrap();
            studio.run(command)
        };

        assert_eq!(
            run(
                &mut studio,
                r#"{"do": "game.duplicate", "name": "bee_maze", "to": "Bee Maze 2"}"#
            )
            .unwrap(),
            "Made Bee Maze 2, a copy of Bee Maze"
        );
        assert!(
            studio
                .cards
                .iter()
                .any(|card| card.game.name == "bee_maze_2")
        );
        assert_eq!(
            run(
                &mut studio,
                r#"{"do": "game.rename", "name": "bee_maze_2", "to": "Bee Run"}"#
            )
            .unwrap(),
            "Renamed Bee Maze 2 to Bee Run"
        );
        assert!(root.join("examples/bee_run/main.rs").is_file());
        let problem = run(&mut studio, r#"{"do": "game.delete", "name": "dodge"}"#).unwrap_err();
        assert!(problem.what.contains("came with Duck Forge"), "{problem}");
        assert_eq!(
            run(&mut studio, r#"{"do": "game.delete", "name": "bee_run"}"#).unwrap(),
            "Moved Bee Run to the Recycle Bin"
        );
        assert!(test.join("Recycle Bin/bee_run/main.rs").is_file());
        assert!(!studio.cards.iter().any(|card| card.game.name == "bee_run"));

        // The Files panel needs an open game.
        let problem = run(
            &mut studio,
            r#"{"do": "files.new_folder", "path": "music"}"#,
        )
        .unwrap_err();
        assert_eq!(problem.what, "No game is open.");
        run(&mut studio, r#"{"do": "game.open", "name": "bee_maze"}"#).unwrap();
        let song = test.join("Desktop/song.wav");
        let add = serde_json::json!({"do": "files.add", "files": [song], "to": "assets"});
        assert_eq!(
            run(&mut studio, &add.to_string()).unwrap(),
            "Added assets/song.wav"
        );
        assert_eq!(
            studio.open.as_ref().unwrap().files_selected.as_deref(),
            Some("assets/song.wav")
        );
        run(
            &mut studio,
            r#"{"do": "files.new_folder", "path": "assets/music"}"#,
        )
        .unwrap();
        run(
            &mut studio,
            r#"{"do": "files.rename", "path": "assets/duck.png", "to": "hero.png"}"#,
        )
        .unwrap();
        run(
            &mut studio,
            r#"{"do": "files.select", "path": "assets/hero.png"}"#,
        )
        .unwrap();
        run(
            &mut studio,
            r#"{"do": "files.delete", "path": "assets/hero.png"}"#,
        )
        .unwrap();
        assert_eq!(studio.open.as_ref().unwrap().files_selected, None);
        assert!(test.join("Recycle Bin/hero.png").is_file());
        let problem =
            run(&mut studio, r#"{"do": "files.select", "path": "nope.png"}"#).unwrap_err();
        assert_eq!(problem.what, "There's no nope.png in the game's folder.");
        let problem = run(&mut studio, r#"{"do": "files.delete", "path": "main.rs"}"#).unwrap_err();
        assert!(problem.what.contains("where the game starts"));
        let lines: Vec<String> = crate::game_files::tree(&root.join("examples/bee_maze"))
            .into_iter()
            .map(|entry| entry.path)
            .collect();
        assert_eq!(
            lines,
            ["assets", "assets/music", "assets/song.wav", "main.rs"]
        );

        // A one-file game has no folder for files.
        run(&mut studio, r#"{"do": "game.open", "name": "dodge"}"#).unwrap();
        let problem = run(
            &mut studio,
            r#"{"do": "files.new_folder", "path": "music"}"#,
        )
        .unwrap_err();
        assert!(problem.what.contains("is one file (dodge.rs)"), "{problem}");
    }

    #[test]
    fn commands_that_need_a_game_say_so_plainly() {
        // Default settings are never saved: the person's own are never touched.
        let mut studio = Studio::new(Settings::default());
        let problem = studio.run(Command::GameToggle).unwrap_err();
        assert_eq!(problem.what, "No game is open.");
        assert_eq!(studio.problem, Some(problem));
        let problem = studio.run(Command::GameOpen {
            name: String::from("no_such_game"),
        });
        assert!(
            problem
                .unwrap_err()
                .what
                .contains("no game called no_such_game")
        );
    }
}
