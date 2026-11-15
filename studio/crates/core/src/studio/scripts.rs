//! Scripts in the Studio: New script, giving a script to objects (a
//! script dropped from Files), and taking it off again.

use std::path::Path;

use serde_json::{Map, Value, json};

use super::Studio;
use crate::game_files;
use crate::history::{FileEdit, Step};
use crate::problem::Problem;
use crate::scene_edit;
use crate::scripts::{
    Connected, PLAIN_KINDS, SCRIPTS_FILE, ScriptEntry, ScriptsFile, connect_main, new_name,
    skeleton,
};

impl Studio {
    /// The open game's `scripts.rs`, read from disk now. `None` if it
    /// hasn't one (or isn't a game with a folder).
    pub fn scripts_file(&self) -> Option<ScriptsFile> {
        let folder = self.open.as_ref()?.game.folder.as_ref()?;
        ScriptsFile::read(folder)
    }

    /// `script.new`: writes the script's file from the skeleton, adds it to
    /// `scripts.rs`, and opens it.
    pub(super) fn script_new(&mut self, typed: String, folder: String) -> Result<String, Problem> {
        let game_folder = self.files_folder()?;
        if self.open_game()?.scene.is_none() {
            return Err(Problem::new(
                "Scripts are for games made of scenes, and this one has none.",
                "Open the Scene demo, or a game with a scenes folder.",
            ));
        }
        let (name, type_name) = new_name(&typed)?;
        let folder = folder.trim().trim_matches('/').replace('\\', "/");
        let place = if folder.is_empty() {
            game_folder.clone()
        } else {
            game_files::resolve(&game_folder, &folder)?
        };
        if !place.is_dir() {
            return Err(Problem::new(
                format!("The game's folder has no folder {folder}."),
                "Right-click a folder in Files, or the game's own folder.",
            ));
        }
        let path = if folder.is_empty() {
            format!("{name}.rs")
        } else {
            format!("{folder}/{name}.rs")
        };
        let file = place.join(format!("{name}.rs"));
        let mut list = ScriptsFile::read(&game_folder).unwrap_or_default();
        if let Some(there) = list.get(&name) {
            return Err(Problem::new(
                format!(
                    "This game has a script called \"{name}\" already ({}).",
                    there.path
                ),
                "Give the new one another name.",
            ));
        }
        if file.exists() {
            return Err(Problem::new(
                format!("There's a file {path} already."),
                "Give the script another name, or put it in another folder.",
            ));
        }

        // Connecting the game to its scripts happens once, and isn't undone
        // (as a file copied in isn't): an empty list, and main.rs's lines.
        let scripts_path = game_folder.join(SCRIPTS_FILE);
        if !scripts_path.exists() {
            crate::level::write_whole(&scripts_path, &ScriptsFile::default().text())?;
        }
        let main_path = game_folder.join("main.rs");
        let mut tell = None;
        if let Ok(main) = std::fs::read_to_string(&main_path) {
            let (connected, done) = connect_main(&main);
            if connected != main {
                crate::level::write_whole(&main_path, &connected)?;
            }
            if let Connected::Tell { line } = done {
                tell = Some(line);
            }
        }

        // The script itself, and its line in scripts.rs: one step for Undo.
        let before = std::fs::read_to_string(&scripts_path).unwrap_or_default();
        list.entries.push(ScriptEntry {
            name: name.clone(),
            path: path.clone(),
            module: name.clone(),
            type_name: type_name.clone(),
        });
        let after = list.text();
        let code = skeleton(&name, &type_name);
        crate::level::write_whole(&file, &code)?;
        crate::level::write_whole(&scripts_path, &after)?;
        let open = self.open_game()?;
        open.undo.push(Step {
            title: format!("New script {name}"),
            files: vec![
                FileEdit {
                    path: file.clone(),
                    before: String::new(),
                    after: code,
                },
                FileEdit {
                    path: scripts_path,
                    before,
                    after,
                },
            ],
            ..Step::default()
        });
        open.files_selected = Some(path.clone());
        let opened = if self.opens_files {
            game_files::open_with_the_system(&file).is_ok()
        } else {
            true
        };
        let mut said = format!("New script {name}: {path}. Building it in...");
        if !opened {
            said.push_str(" (It couldn't be opened: double-click it in Files.)");
        }
        if let Some(line) = tell {
            said.push_str(&format!(
                " One line is needed in main.rs, where the scene is loaded (near line {line}): \
                 add .with_scripts(scripts::all()) after Scene::load(...)."
            ));
            if self.opens_files {
                let _ = game_files::open_with_the_system(&main_path);
            }
        }
        Ok(said)
    }

    /// A script dropped from Files: onto objects (`ids`), they get it;
    /// onto empty space, a new object with it there.
    pub(super) fn use_script(
        &mut self,
        entry: ScriptEntry,
        at: Option<[f32; 2]>,
        ids: Vec<u64>,
    ) -> Result<String, Problem> {
        let name = entry.name.clone();
        if ids.is_empty() {
            // Nothing to see yet: a box like an Area's, until it has a picture.
            let size = scene_edit::AREA_SIZE as i64;
            let set = Map::from_iter([(String::from("box"), json!([0, 0, size, size]))]);
            return self.object_add(name, at, None, set);
        }
        let list = self.scripts_file().unwrap_or_default();
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(super::objects::no_scene)?;
        let mut changes = Vec::new();
        for id in &ids {
            let object = doc.object(*id).ok_or_else(|| {
                Problem::new(
                    format!("The scene's file has no object with the id {id}."),
                    "Give a script to an object of the scene's file.",
                )
            })?;
            let kind = object.kind.as_str();
            let called = &object.name;
            let has_a_script = list.get(kind).is_some();
            if kind != "picture" && kind != "area" && !has_a_script {
                return Err(if PLAIN_KINDS.contains(&kind) {
                    Problem::new(
                        format!("{called} is a {kind}, which has a job of its own already."),
                        "A script beside it comes with several scripts per object, later. Give the script to a Picture or an Area.",
                    )
                } else {
                    Problem::new(
                        format!(
                            "{called}'s kind is \"{kind}\", which the game's code looks for, so a script can't take its place."
                        ),
                        "Give the script to a Picture or an Area, or to a copy of it.",
                    )
                });
            }
            let mut set = Map::from_iter([(String::from("kind"), json!(name))]);
            if has_a_script && kind != name {
                // The script it had goes, with its settings.
                for setting in open.script_settings(kind) {
                    if object.has(&setting) {
                        set.insert(setting, Value::Null);
                    }
                }
            }
            changes.push((*id, set));
        }
        let who = open.called(&ids);
        self.change_scene(changes, format!("Give {who} the script {name}"), None)
    }

    /// `script.remove`: the objects' scripts come off, with their
    /// settings. An object with no picture becomes an Area, so it can
    /// still be seen and picked.
    pub(super) fn script_remove(&mut self, ids: Vec<u64>) -> Result<String, Problem> {
        let list = self.scripts_file().unwrap_or_default();
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(super::objects::no_scene)?;
        let mut changes = Vec::new();
        let mut names = Vec::new();
        for id in &ids {
            let Some(object) = doc.object(*id) else {
                continue;
            };
            let kind = object.kind.clone();
            let known = list.get(&kind).is_some()
                || open
                    .game_scripts()
                    .is_some_and(|scripts| scripts.get(&kind).is_some());
            if !known {
                continue;
            }
            let back = if object.picture.is_some() {
                Value::Null
            } else {
                json!("area")
            };
            let mut set = Map::from_iter([(String::from("kind"), back)]);
            for setting in open.script_settings(&kind) {
                if object.has(&setting) {
                    set.insert(setting, Value::Null);
                }
            }
            if !names.contains(&kind) {
                names.push(kind);
            }
            changes.push((*id, set));
        }
        if changes.is_empty() {
            return Err(Problem::new(
                "None of those objects has a script.",
                "Pick an object whose Script box names one.",
            ));
        }
        let who = open.called(&changes.iter().map(|(id, _)| *id).collect::<Vec<_>>());
        let title = format!("Take {} off {who}", names.join(" and "));
        self.change_scene(changes, title, None)
    }

    /// After Files renamed or deleted something: `scripts.rs` follows (a
    /// script moved keeps its name; one deleted leaves the list). Objects
    /// keep their kind either way.
    pub(super) fn scripts_follow_files(&mut self, from: &str, to: Option<&str>) {
        let Some(folder) = self.open.as_ref().and_then(|open| open.game.folder.clone()) else {
            return;
        };
        let Some(mut list) = ScriptsFile::read(&folder) else {
            return;
        };
        let from = from.trim_end_matches('/');
        let inside = |path: &str| path == from || path.starts_with(&format!("{from}/"));
        let before = list.clone();
        match to {
            Some(to) => {
                for entry in &mut list.entries {
                    if inside(&entry.path) {
                        entry.path = format!("{to}{}", &entry.path[from.len()..]);
                    }
                }
            }
            None => list.entries.retain(|entry| !inside(&entry.path)),
        }
        if list != before {
            let _ = crate::level::write_whole(&folder.join(SCRIPTS_FILE), &list.text());
        }
    }
}

/// Is `path` (from the game's folder) a `.rs` file?
pub(super) fn is_code(path: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|end| end.eq_ignore_ascii_case("rs"))
}
