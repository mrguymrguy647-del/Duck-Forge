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
use crate::scene_edit::{self, SceneDoc, SceneRow, Show, Tool, describe, pair, same_file};

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
