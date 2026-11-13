//! The editor's commands on a scene's objects: changing the file (how the
//! scene starts), changing an object for this run only, selecting, and
//! how the view is set.

use duckforge::prelude::Vec2;
use duckforge::studio_link::protocol::DrawnScene;
use serde_json::{Map, Value};

use super::{OpenGame, Studio, name_from};
use crate::history::{FileEdit, NowEdit, Step};
use crate::play::PlayState;
use crate::problem::Problem;
use crate::scene_edit::{
    self, Needs, SceneDoc, SceneRow, Show, Tool, describe, in_file, line_without_id, pair,
    same_file,
};
use duckforge::scene::SceneFile;

fn no_scene() -> Problem {
    Problem::new(
        "This game has no scene to edit.",
        "Its objects are made in its code. A game with a scenes folder (like scene_demo) can be edited here.",
    )
}

impl OpenGame {
    /// What the frozen game says about the scene the editor shows: where
    /// each object is on the picture. `None` while the game plays.
    pub fn drawn_scene(&self) -> Option<&DrawnScene> {
        let doc = self.scene.as_ref()?;
        self.play
            .as_ref()?
            .scenes
            .iter()
            .find(|drawn| same_file(&drawn.file, &doc.name))
    }

    /// The Scene list.
    pub fn scene_rows(&self) -> Vec<SceneRow> {
        match &self.scene {
            Some(doc) => scene_edit::rows(doc, self.drawn_scene(), &self.selected),
            None => Vec::new(),
        }
    }

    /// The selected object's name for a message, or "3 objects".
    fn called(&self, ids: &[u64]) -> String {
        match ids {
            [] => String::from("nothing"),
            [id] => {
                let in_file = self
                    .scene
                    .as_ref()
                    .and_then(|doc| doc.object(*id))
                    .map(describe);
                let made = || {
                    let object = self.drawn_scene()?.object(*id)?;
                    Some(if object.name.is_empty() {
                        format!("the {}", object.kind)
                    } else {
                        object.name.clone()
                    })
                };
                in_file
                    .or_else(made)
                    .unwrap_or_else(|| format!("object {id}"))
            }
            many => format!("{} objects", many.len()),
        }
    }
}

impl Studio {
    /// Looks again at which scene the editor shows, and what its file
    /// says: the scene the frozen game is in, or the one shown already,
    /// or the game's first.
    pub(super) fn refresh_scene(&mut self) {
        let Some(root) = self.folder.as_ref().map(|f| f.root().to_path_buf()) else {
            return;
        };
        let Some(open) = &mut self.open else {
            return;
        };
        let in_game = open
            .play
            .as_ref()
            .and_then(|play| play.scenes.first())
            .and_then(|drawn| {
                open.scenes_seen
                    .keys()
                    .find(|path| same_file(&name_from(&root, path), &drawn.file))
                    .cloned()
            });
        let shown = open
            .scene
            .as_ref()
            .map(|doc| doc.path.clone())
            .filter(|path| open.scenes_seen.contains_key(path));
        let path = in_game
            .or(shown)
            .or_else(|| open.scenes_seen.keys().next().cloned());
        let Some(path) = path else {
            open.scene = None;
            open.selected.clear();
            return;
        };
        let text = &open.scenes_seen[&path];
        let same = open
            .scene
            .as_ref()
            .is_some_and(|doc| doc.path == path && doc.text == *text);
        if !same {
            if open.scene.as_ref().is_some_and(|doc| doc.path != path) {
                open.selected.clear();
            }
            open.scene = Some(SceneDoc::read(&path, &name_from(&root, &path), text));
        }
        // What Undo took away, or the game did, isn't selected any more.
        let drawn: Vec<u64> = open
            .drawn_scene()
            .map(|drawn| drawn.objects.iter().map(|object| object.id).collect())
            .unwrap_or_default();
        if let Some(doc) = &open.scene {
            open.selected
                .retain(|id| doc.object(*id).is_some() || drawn.contains(id));
        }
    }

    /// Whenever the game has played on, what was changed "for this run
    /// only" can't be undone any more: there's nothing to put it back to.
    pub(super) fn forget_moves_of_a_moment_gone(&mut self) {
        let Some(open) = &mut self.open else {
            return;
        };
        let frame = open
            .play
            .as_ref()
            .filter(|play| play.is_running())
            .map(|play| play.frame);
        if frame != open.frame_seen {
            open.frame_seen = frame;
            open.undo.forget_now();
        }
        if let Some(play) = &mut open.play {
            play.show_as_it_starts(open.view.show == Show::Start);
        }
    }

    /// Changes objects in the scene's file: how the scene starts. The
    /// game takes the new file at once, and it's a step for Undo.
    fn change_scene(
        &mut self,
        changes: Vec<(u64, Map<String, Value>)>,
        title: String,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        self.change_scene_keeping(changes, Vec::new(), title, stroke)
    }

    /// [`change_scene`](Self::change_scene), with what Undo has to put
    /// back in the frozen game besides the file (`now`).
    fn change_scene_keeping(
        &mut self,
        changes: Vec<(u64, Map<String, Value>)>,
        now: Vec<NowEdit>,
        title: String,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let after = doc.with_set(&changes)?;
        self.write_scene(after, now, title, stroke)
    }

    /// Changes the scene's file with `change` (adding, taking out or
    /// reordering objects): a step for Undo, and the game takes it at
    /// once. Says what `change` said: the objects it made, say.
    fn edit_scene<R>(
        &mut self,
        title: String,
        change: impl FnOnce(&mut SceneFile, &str) -> Result<R, Problem>,
    ) -> Result<R, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let short = doc.short_name().to_string();
        let (after, said) = doc.edited(|file| change(file, &short))?;
        self.write_scene(after, Vec::new(), title, None)?;
        Ok(said)
    }

    /// Writes the scene's new text, sends it to the game, and makes it a
    /// step for Undo.
    fn write_scene(
        &mut self,
        after: String,
        now: Vec<NowEdit>,
        title: String,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let (path, name, before) = (doc.path.clone(), doc.name.clone(), doc.text.clone());
        if after == before {
            return Ok(String::from("Nothing changed"));
        }
        crate::level::write_whole(&path, &after)?;
        open.scenes_seen.insert(path.clone(), after.clone());
        open.scene = Some(SceneDoc::read(&path, &name, &after));
        if let Some(play) = &mut open.play {
            play.send_file(&name, &after);
        }
        open.undo.push(Step {
            title: title.clone(),
            files: vec![FileEdit {
                path,
                before,
                after,
            }],
            stroke,
            now,
        });
        open.edited_since_snapshot = true;
        Ok(title)
    }

    /// Changes objects as they are on the frozen frame, for this run
    /// only: the file is left alone. A step for Undo until the game
    /// plays on.
    fn change_now(
        &mut self,
        changes: Vec<(u64, Map<String, Value>)>,
        title: String,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        if open.play_state() != Some(&PlayState::Frozen) {
            return Err(Problem::new(
                "The game isn't frozen.",
                "Freeze it first (Tab). An object can be changed for this run only while the game stands still.",
            ));
        }
        if open.view.show == Show::Start {
            return Err(Problem::new(
                "The view shows the scene as it starts.",
                "Switch to Frozen moment to change an object for this run only.",
            ));
        }
        let name = doc.name.clone();
        let mut edits = Vec::new();
        for (id, set) in changes {
            let now = open
                .drawn_scene()
                .and_then(|drawn| drawn.object(id))
                .filter(|object| !object.gone)
                .ok_or_else(|| {
                    Problem::new(
                        format!("{} isn't in the game right now.", open.called(&[id])),
                        "The game has removed it, or hasn't drawn a frozen picture yet. Its start can still be changed.",
                    )
                })?;
            let starts = doc
                .object(id)
                .map(|object| object.to_json(true))
                .unwrap_or_default();
            let before: Map<String, Value> = set
                .keys()
                .map(|key| {
                    let value = now.now.get(key).or_else(|| starts.get(key));
                    (key.clone(), value.cloned().unwrap_or(Value::Null))
                })
                .collect();
            edits.push(NowEdit {
                file: name.clone(),
                id,
                before,
                after: set,
            });
        }
        if let Some(play) = &mut open.play {
            for edit in &edits {
                play.send_object(&edit.file, edit.id, edit.after.clone());
            }
        }
        open.undo.push(Step {
            title: title.clone(),
            stroke,
            now: edits,
            ..Step::default()
        });
        Ok(title)
    }

    /// Undo or Redo of a step's "for this run only" changes.
    pub(super) fn apply_now(&mut self, step: &Step, before: bool) {
        let Some(play) = self.open.as_mut().and_then(|open| open.play.as_mut()) else {
            return;
        };
        for edit in &step.now {
            let set = if before { &edit.before } else { &edit.after };
            play.send_object(&edit.file, edit.id, set.clone());
        }
    }

    pub(super) fn object_set(
        &mut self,
        ids: Vec<u64>,
        set: Map<String, Value>,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let who = self.open_game()?.called(&ids);
        let title = match set.keys().next() {
            Some(key) if set.len() == 1 && ids.len() == 1 => format!("Change {who}'s {key}"),
            _ => format!("Change {who}"),
        };
        let changes = ids.into_iter().map(|id| (id, set.clone())).collect();
        self.change_scene(changes, title, stroke)
    }

    pub(super) fn object_set_now(
        &mut self,
        id: u64,
        set: Map<String, Value>,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let who = self.open_game()?.called(&[id]);
        let title = if set.len() == 1 && set.contains_key("at") {
            format!("Move {who} for this run")
        } else {
            format!("Change {who} for this run")
        };
        self.change_now(vec![(id, set)], title, stroke)
    }

    /// Moves where objects start: all by `by`, or the first to `to` and
    /// the rest along with it.
    pub(super) fn object_move(
        &mut self,
        ids: Vec<u64>,
        by: Option<[f32; 2]>,
        to: Option<[f32; 2]>,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let starts = |id: u64| {
            doc.object(id).map(|object| object.at).ok_or_else(|| {
                Problem::new(
                    format!("{} has no object with id {id}.", doc.short_name()),
                    "Pick one from the Scene list. (An object the game made while playing isn't in the file.)",
                )
            })
        };
        let by = match (by, to, ids.first()) {
            (Some([x, y]), None, _) => Vec2::new(x, y),
            (None, Some([x, y]), Some(first)) => Vec2::new(x, y) - starts(*first)?,
            (_, _, None) => {
                return Err(Problem::new(
                    "Nothing is selected to move.",
                    "Give the objects' ids, like \"ids\": [7].",
                ));
            }
            _ => {
                return Err(Problem::new(
                    "A move needs either \"by\" or \"to\", not both and not neither.",
                    "Like \"by\": [8, 0] (how far) or \"to\": [96, 208] (where).",
                ));
            }
        };
        let mut changes = Vec::new();
        for id in &ids {
            let set = Map::from_iter([(String::from("at"), pair(starts(*id)? + by))]);
            changes.push((*id, set));
        }
        let title = format!("Move {}", open.called(&ids));
        self.change_scene(changes, title, stroke)
    }

    pub(super) fn object_rename(&mut self, id: u64, name: String) -> Result<String, Problem> {
        let name = name.trim().to_string();
        let who = self.open_game()?.called(&[id]);
        let (title, value) = if name.is_empty() {
            (format!("Take {who}'s name away"), Value::Null)
        } else {
            (format!("Rename {who} to {name}"), Value::from(name))
        };
        let set = Map::from_iter([(String::from("name"), value)]);
        self.change_scene(vec![(id, set)], title, None)
    }

    /// "Start here": where each object is right now becomes where it
    /// starts.
    pub(super) fn object_start_here(&mut self, ids: Vec<u64>) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let mut changes = Vec::new();
        // Undo has to leave each object where it is now, with its old
        // start back: once they're the same, the game can't tell "it
        // walked here" from "it starts here".
        let mut stays = Vec::new();
        for id in &ids {
            let start = doc.object(*id).map(|object| object.at);
            let now = scene_edit::at_now(doc, open.drawn_scene(), *id);
            if let Some(now) = now
                && start != Some(now)
            {
                let at = Map::from_iter([(String::from("at"), pair(now))]);
                stays.push(NowEdit {
                    file: doc.name.clone(),
                    id: *id,
                    before: at.clone(),
                    after: at.clone(),
                });
                changes.push((*id, at));
            }
        }
        if changes.is_empty() {
            return Ok(format!("{} is already where it starts", open.called(&ids)));
        }
        let title = format!("Start {} here", open.called(&ids));
        self.change_scene_keeping(changes, stays, title, None)
    }

    /// "Back to its start": each object goes to where the file starts it,
    /// on the frozen frame.
    pub(super) fn object_back_to_start(&mut self, ids: Vec<u64>) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let mut changes = Vec::new();
        for id in &ids {
            let moved = open
                .drawn_scene()
                .and_then(|drawn| drawn.object(*id))
                .is_some_and(|object| !object.gone && object.now.contains_key("at"));
            if let Some(object) = doc.object(*id)
                && moved
            {
                changes.push((*id, Map::from_iter([(String::from("at"), pair(object.at))])));
            }
        }
        if changes.is_empty() {
            return Ok(format!("{} is already where it starts", open.called(&ids)));
        }
        let title = format!("Put {} back where it starts", open.called(&ids));
        self.change_now(changes, title, None)
    }

    /// Turns objects: all by `by` degrees, or the first to `to` and the
    /// rest by as much as it turned. Changes the file.
    pub(super) fn object_turn(
        &mut self,
        ids: Vec<u64>,
        by: Option<f32>,
        to: Option<f32>,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let file = doc.file.as_ref().ok_or_else(no_scene)?;
        let short = doc.short_name();
        let first = ids.first().ok_or_else(|| {
            Problem::new(
                "Nothing is selected to turn.",
                "Give the objects' ids, like \"ids\": [7].",
            )
        })?;
        let by = match (by, to) {
            (Some(by), None) => by,
            (None, Some(to)) => to - in_file(file, *first, short)?.turn,
            _ => {
                return Err(Problem::new(
                    "A turn needs either \"by\" or \"to\", not both and not neither.",
                    "Like \"by\": 15 (how far, clockwise) or \"to\": 90 (which way).",
                ));
            }
        };
        let mut changes = Vec::new();
        for id in &ids {
            let turn = scene_edit::tidy_turn(in_file(file, *id, short)?.turn + by);
            let turn = (turn * 100.0).round() / 100.0;
            changes.push((*id, Map::from_iter([(String::from("turn"), number(turn))])));
        }
        let title = format!("Turn {}", open.called(&ids));
        self.change_scene(changes, title, stroke)
    }

    /// Makes objects bigger or smaller: all `by` so many times, or the
    /// first `to` a size and the rest by as much. Each grows round its own
    /// `at`, which stays where it is. Changes the file.
    pub(super) fn object_resize(
        &mut self,
        ids: Vec<u64>,
        by: Option<[f32; 2]>,
        to: Option<[f32; 2]>,
        stroke: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let file = doc.file.as_ref().ok_or_else(no_scene)?;
        let short = doc.short_name();
        let first = ids.first().ok_or_else(|| {
            Problem::new(
                "Nothing is selected to size.",
                "Give the objects' ids, like \"ids\": [7].",
            )
        })?;
        let usable = |n: f32| n.is_finite() && n > 0.0;
        let factor = match (by, to) {
            (Some([x, y]), None) if usable(x) && usable(y) => Vec2::new(x, y),
            (None, Some([x, y])) if usable(x) && usable(y) => {
                let now = in_file(file, *first, short)?.scale;
                Vec2::new(x / now.x, y / now.y)
            }
            _ => {
                return Err(Problem::new(
                    "A size needs either \"by\" or \"to\", with two numbers bigger than nothing.",
                    "Like \"by\": [2, 2] (twice as big) or \"to\": [1.5, 1] (that size).",
                ));
            }
        };
        let mut changes = Vec::new();
        for (index, id) in ids.iter().enumerate() {
            let now = in_file(file, *id, short)?.scale;
            let size = match (index, to) {
                (0, Some([x, y])) => Vec2::new(x, y),
                _ => Vec2::new(now.x * factor.x, now.y * factor.y),
            };
            let tidy = |n: f32| ((n * 1000.0).round() / 1000.0).clamp(0.01, 64.0);
            let value = serde_json::json!([number(tidy(size.x)), number(tidy(size.y))]);
            changes.push((*id, Map::from_iter([(String::from("scale"), value)])));
        }
        let title = format!("Size {}", open.called(&ids));
        self.change_scene(changes, title, stroke)
    }

    /// Changes where objects come in the file's order, which is the order
    /// they're drawn in within their layer (the last on top): `to` the
    /// "front" or the "back", one "forward" or "backward", or just
    /// `before` another object.
    pub(super) fn object_order(
        &mut self,
        ids: Vec<u64>,
        to: Option<String>,
        before: Option<u64>,
    ) -> Result<String, Problem> {
        let open = self.open_game()?;
        let who = open.called(&ids);
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let file = doc.file.as_ref().ok_or_else(no_scene)?;
        let order: Vec<u64> = file.objects.iter().map(|o| o.id().number()).collect();
        for id in &ids {
            in_file(file, *id, doc.short_name())?;
        }
        let moving = |id: &u64| ids.contains(id);
        let first = order.iter().position(moving).unwrap_or(0);
        let last = order.iter().rposition(moving).unwrap_or(0);
        let others_after = |from: usize| order[from..].iter().filter(|id| !moving(id));
        let (place, words) = match (to.as_deref(), before) {
            (None, Some(before)) => (Some(before), format!("Move {who} in the list")),
            (Some("front"), None) => (None, format!("Bring {who} to the front")),
            (Some("back"), None) => (
                order.iter().copied().find(|id| !moving(id)),
                format!("Send {who} to the back"),
            ),
            (Some("forward"), None) => {
                // After the next object that isn't moving.
                let mut after = others_after(last + 1);
                after.next();
                (after.next().copied(), format!("Bring {who} forward"))
            }
            (Some("backward"), None) => (
                order[..first].iter().rev().copied().find(|id| !moving(id)),
                format!("Send {who} backward"),
            ),
            _ => {
                return Err(Problem::new(
                    "Where to? \"to\" is front, back, forward or backward; or give \"before\" and an object's id.",
                    "Like \"to\": \"front\" (drawn on top) or \"before\": 7.",
                ));
            }
        };
        if to.as_deref() == Some("backward") && place.is_none() {
            return Ok(format!("{who} is already at the back"));
        }
        self.edit_scene(words.clone(), |file, _| {
            let ids: Vec<_> = file
                .objects
                .iter()
                .map(|o| o.id())
                .filter(|id| ids.contains(&id.number()))
                .collect();
            let before = place.and_then(|place| {
                file.objects
                    .iter()
                    .map(|o| o.id())
                    .find(|id| id.number() == place)
            });
            file.move_before(&ids, before).map_err(|wrong| {
                Problem::new("The Studio couldn't move that in the list.", "Try again.")
                    .with_details(wrong)
            })
        })?;
        Ok(words)
    }

    /// Takes objects out of the scene's file. Objects the game made while
    /// playing aren't in the file, so they're left.
    pub(super) fn object_delete(&mut self, ids: Vec<u64>) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let in_the_file: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| doc.object(*id).is_some())
            .collect();
        if in_the_file.is_empty() {
            return Err(Problem::new(
                "There's nothing in the scene's file to delete.",
                "Objects the game makes while playing aren't in the file: the game takes them away itself.",
            ));
        }
        let left = ids.len() - in_the_file.len();
        let title = format!("Delete {}", open.called(&in_the_file));
        self.edit_scene(title.clone(), |file, _| {
            let gone: Vec<_> = file
                .objects
                .iter()
                .map(|o| o.id())
                .filter(|id| in_the_file.contains(&id.number()))
                .collect();
            for id in gone {
                file.remove(id);
            }
            Ok(())
        })?;
        let open = self.open_game()?;
        open.selected.retain(|id| !in_the_file.contains(id));
        Ok(match left {
            0 => title,
            1 => format!("{title} (1 made by the game was left)"),
            n => format!("{title} ({n} made by the game were left)"),
        })
    }

    /// Copies of objects, each just after its own in the file's order and
    /// one grid square to the right. The copies are selected.
    pub(super) fn object_duplicate(&mut self, ids: Vec<u64>) -> Result<String, Problem> {
        let open = self.open_game()?;
        let step = open.view.snap_step;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let file = doc.file.as_ref().ok_or_else(no_scene)?;
        // In the file's order, so the copies keep it.
        let originals: Vec<u64> = file
            .objects
            .iter()
            .map(|o| o.id().number())
            .filter(|id| ids.contains(id))
            .collect();
        if originals.is_empty() {
            return Err(Problem::new(
                "There's nothing in the scene's file to duplicate.",
                "Pick objects in the Scene list. (Objects the game makes while playing can be copied and pasted.)",
            ));
        }
        let title = format!("Duplicate {}", open.called(&originals));
        let copies = self.edit_scene(title.clone(), |file, short| {
            let mut copies = Vec::new();
            for id in &originals {
                let original = in_file(file, *id, short)?;
                let mut line = line_without_id(original);
                if line.contains_key("at") {
                    line.insert(String::from("at"), pair(original.at + Vec2::new(step, 0.0)));
                }
                let index = file.objects.iter().position(|o| o.id().number() == *id);
                let before = index.and_then(|i| file.objects.get(i + 1)).map(|o| o.id());
                let copy = file.add(&line, before).map_err(|wrong| {
                    Problem::new("The Studio couldn't copy that object.", "Try again.")
                        .with_details(wrong)
                })?;
                copies.push(copy.number());
            }
            Ok(copies)
        })?;
        self.open_game()?.selected = copies;
        Ok(title)
    }

    /// Copies objects to the Studio's clipboard: their lines as the file
    /// has them, or as the game has them now for objects it made. Nothing
    /// changes, so it isn't a step for Undo.
    pub(super) fn object_copy(&mut self, ids: Vec<u64>) -> Result<String, Problem> {
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let file = doc.file.as_ref().ok_or_else(no_scene)?;
        let mut lines: Vec<Map<String, Value>> = file
            .objects
            .iter()
            .filter(|o| ids.contains(&o.id().number()))
            .map(line_without_id)
            .collect();
        if let Some(drawn) = open.drawn_scene() {
            for id in &ids {
                if let Some(made) = drawn
                    .object(*id)
                    .filter(|o| o.origin == duckforge::studio_link::protocol::Origin::Made)
                {
                    let mut line = made.now.clone();
                    line.remove("id");
                    lines.push(line);
                }
            }
        }
        if lines.is_empty() {
            return Err(Problem::new(
                "Nothing is selected to copy.",
                "Select objects first.",
            ));
        }
        let said = format!("Copied {}", open.called(&ids));
        self.clipboard = lines;
        self.pastes = 0;
        Ok(said)
    }

    /// Pastes objects: from `text` if it's objects' lines (from a scene
    /// file, say), or else the Studio's clipboard. With `at`, the first
    /// goes there and the rest keep their places round it; without, each
    /// paste goes one more grid square to the right. They go last in the
    /// file (drawn on top), and are selected.
    pub(super) fn object_paste(
        &mut self,
        at: Option<[f32; 2]>,
        text: Option<String>,
    ) -> Result<String, Problem> {
        let from_text = text.as_deref().and_then(scene_edit::lines_from_text);
        let lines = match from_text {
            Some(lines) => lines,
            None if !self.clipboard.is_empty() => {
                self.pastes += 1;
                self.clipboard.clone()
            }
            None => {
                return Err(Problem::new(
                    "There's nothing to paste.",
                    "Copy objects first (Ctrl+C), or copy lines of a scene file.",
                ));
            }
        };
        let open = self.open_game()?;
        let step = open.view.snap_step;
        let first = lines
            .iter()
            .find_map(|line| line.get("at").and_then(scene_edit::point_of));
        let shift = match (at, first) {
            (Some([x, y]), Some(first)) => Vec2::new(x, y) - first,
            _ => Vec2::new(step * self.pastes.max(1) as f32, 0.0),
        };
        let count = lines.len();
        let title = if count == 1 {
            String::from("Paste 1 object")
        } else {
            format!("Paste {count} objects")
        };
        let pasted = self.edit_scene(title.clone(), |file, _| {
            let mut pasted = Vec::new();
            for line in &lines {
                let mut line = line.clone();
                if let Some(at) = line.get("at").and_then(scene_edit::point_of) {
                    line.insert(String::from("at"), pair(at + shift));
                }
                let id = file.add(&line, None).map_err(|wrong| {
                    Problem::new(
                        "What's being pasted isn't an object the Studio can read.",
                        "Copy whole lines of a scene file, or objects in the Studio.",
                    )
                    .with_details(wrong)
                })?;
                pasted.push(id.number());
            }
            Ok(pasted)
        })?;
        self.open_game()?.selected = pasted;
        Ok(title)
    }

    /// Adds an object of a kind: where `at` says, or in the middle of what
    /// the view shows. A picture or a background needs `"picture"` in
    /// `set`, a tile map `"level"`. Anything else in `set` goes on its
    /// line too. It's selected.
    pub(super) fn object_add(
        &mut self,
        kind: String,
        at: Option<[f32; 2]>,
        name: Option<String>,
        set: Map<String, Value>,
    ) -> Result<String, Problem> {
        let kind = kind.trim().to_lowercase();
        let plain = scene_edit::plain_kind(&kind);
        if let Some(why) = plain.and_then(|plain| plain.not_yet) {
            return Err(Problem::new(
                why,
                "Picture, Sound, Area, Background and Tile map can be added now.",
            ));
        }
        let chosen_key = match plain.map(|plain| plain.needs) {
            Some(Needs::Picture) => Some("picture"),
            Some(Needs::Sound) => Some("sound"),
            Some(Needs::Level) => Some("level"),
            _ => None,
        };
        let chosen = chosen_key
            .and_then(|key| set.get(key))
            .and_then(Value::as_str);
        if let (Some(key), None) = (chosen_key, chosen) {
            let example = match key {
                "level" => "level.txt",
                "sound" => "assets/music.wav",
                _ => "assets/crate.png",
            };
            return Err(Problem::new(
                format!(
                    "A {} needs a {key} to show.",
                    plain.map_or(kind.as_str(), |p| p.title)
                ),
                format!("Choose one, like \"set\": {{\"{key}\": \"{example}\"}}."),
            ));
        }
        let open = self.open_game()?;
        let view = open.view;
        let doc = open.scene.as_ref().ok_or_else(no_scene)?;
        let file = doc.file.as_ref().ok_or_else(no_scene)?;
        let layers: Vec<&str> = file.layers.iter().map(|l| l.name.as_str()).collect();
        let layer = match kind.as_str() {
            "background" => ["far", "back"]
                .into_iter()
                .find(|l| layers.contains(l))
                .or(layers.first().copied()),
            _ => ["world"]
                .into_iter()
                .find(|l| layers.contains(l))
                .or(layers.first().copied()),
        }
        .unwrap_or("world")
        .to_string();
        let at = match at {
            Some([x, y]) => Vec2::new(x, y),
            None => {
                let middle = open.drawn_scene().map(|drawn| {
                    let size = open
                        .play
                        .as_ref()
                        .and_then(|play| play.picture.as_ref())
                        .map_or(Vec2::new(320.0, 240.0), |p| {
                            Vec2::new(p.width as f32, p.height as f32)
                        });
                    drawn.to_scene(size * 0.5)
                });
                let middle =
                    middle.unwrap_or_else(|| file.size.unwrap_or(Vec2::new(320.0, 240.0)) * 0.5);
                let middle = if kind == "area" {
                    middle - Vec2::new(scene_edit::AREA_SIZE, scene_edit::AREA_SIZE) * 0.5
                } else {
                    middle
                };
                view.place(middle, false)
            }
        };
        let mut line = scene_edit::new_line(&kind, at, chosen, &layer);
        for (key, value) in set {
            line.insert(key, value);
        }
        if let Some(name) = name.filter(|name| !name.trim().is_empty()) {
            line.insert(String::from("name"), Value::from(name.trim()));
        }
        let called = line
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(&kind)
            .to_string();
        let title = format!("Add {called}");
        let id = self.edit_scene(title.clone(), |file, _| {
            file.add(&line, None).map_err(|wrong| {
                Problem::new(
                    format!("The Studio couldn't add {called} like that."),
                    "Check the values, then try again.",
                )
                .with_details(wrong)
            })
        })?;
        self.open_game()?.selected = vec![id.number()];
        Ok(title)
    }

    pub(super) fn select(&mut self, ids: Vec<u64>, keep: bool) -> Result<String, Problem> {
        let open = self.open_game()?;
        if !keep {
            open.selected.clear();
        }
        for id in ids {
            if !open.selected.contains(&id) {
                open.selected.push(id);
            }
        }
        Ok(match open.selected.len() {
            0 => String::from("Nothing selected"),
            _ => format!("Selected {}", open.called(&open.selected)),
        })
    }

    /// Everything that can be dragged: the file's objects but for tile
    /// maps and backgrounds, and what the game has made.
    pub(super) fn select_all(&mut self) -> Result<String, Problem> {
        let open = self.open_game()?;
        let ids = open
            .scene_rows()
            .iter()
            .filter(|row| !matches!(row.kind.as_str(), "tilemap" | "background"))
            .map(|row| row.id)
            .collect();
        self.select(ids, false)
    }

    pub(super) fn view_tool(&mut self, tool: Tool) -> Result<String, Problem> {
        self.open_game()?.view.tool = tool;
        Ok(String::from(match tool {
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Turn => "Turn",
            Tool::Size => "Size",
        }))
    }

    pub(super) fn view_snap(&mut self, on: bool, step: Option<f32>) -> Result<String, Problem> {
        let view = &mut self.open_game()?.view;
        if let Some(step) = step {
            if !(1.0..=256.0).contains(&step) {
                return Err(Problem::new(
                    "The grid's squares are between 1 and 256 pixels.",
                    "Like \"step\": 8.",
                ));
            }
            view.snap_step = step;
        }
        view.snap = on;
        Ok(if on {
            format!("Snap to {} pixels", view.snap_step)
        } else {
            String::from("Snap is off")
        })
    }

    pub(super) fn view_grid(&mut self, on: bool) -> Result<String, Problem> {
        self.open_game()?.view.grid = on;
        Ok(String::from(if on {
            "The grid shows"
        } else {
            "The grid is hidden"
        }))
    }

    pub(super) fn view_show(&mut self, what: Show) -> Result<String, Problem> {
        let open = self.open_game()?;
        open.view.show = what;
        if let Some(play) = &mut open.play {
            play.show_as_it_starts(what == Show::Start);
        }
        Ok(String::from(match what {
            Show::Frozen => "Showing the frozen moment",
            Show::Start => "Showing the scene as it starts",
        }))
    }
}

/// A number as the scene file writes it: whole if it's whole.
fn number(n: f32) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e9 {
        Value::from(n as i64)
    } else {
        Value::from((f64::from(n) * 1000.0).round() / 1000.0)
    }
}
