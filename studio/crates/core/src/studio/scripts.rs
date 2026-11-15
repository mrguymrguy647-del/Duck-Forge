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
use duckforge::scene::ready::{self, ReadyMade};

/// What "Turn into code" will write, worked out before anything is: the
/// Studio shows it, and `script.turn_into_code` does exactly this.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnPlan {
    /// The ready-made kind the objects have.
    pub ready: ReadyMade,
    /// The objects it turns: those asked for.
    pub ids: Vec<u64>,
    /// Every object of the scene with that ready-made kind.
    pub all_with_it: Vec<u64>,
    /// The script's name (the objects' new kind), and its type.
    pub name: String,
    pub type_name: String,
    /// Its file, from the game's folder: `scripts/duck.rs`.
    pub path: String,
    /// The file's text.
    pub code: String,
    /// Whether `main.rs` gets its lines (`mod scripts;` and
    /// `.with_scripts(scripts::all())`).
    pub connects_main: bool,
}

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

    /// What turning objects' ready-made behaviour into code would do:
    /// the script it writes, named `name` (or after the first object).
    pub fn turn_into_code_plan(
        &self,
        ids: &[u64],
        name: Option<&str>,
    ) -> Result<TurnPlan, Problem> {
        let open = self
            .open
            .as_ref()
            .ok_or_else(|| Problem::new("No game is open.", "Open a game made of scenes first."))?;
        let game_folder = open.game.folder.clone().ok_or_else(|| {
            Problem::new(
                "This game has no folder of its own to write code in.",
                "Open a game from your games folder.",
            )
        })?;
        let doc = open.scene.as_ref().ok_or_else(super::objects::no_scene)?;
        let mut kind: Option<&ReadyMade> = None;
        let mut first_name = String::new();
        for id in ids {
            let object = doc.object(*id).ok_or_else(|| {
                Problem::new(
                    format!("The scene's file has no object with the id {id}."),
                    "Pick an object of the scene.",
                )
            })?;
            let Some(this) = ready::get(&object.kind) else {
                return Err(Problem::new(
                    format!(
                        "{} doesn't use one of Duck Forge's ready-made behaviours, so there's nothing to turn into code.",
                        object.name
                    ),
                    "Pick an object whose Kind is a ready-made one, like Platformer player.",
                ));
            };
            if kind.is_some_and(|kind| kind.name != this.name) {
                return Err(Problem::new(
                    "Those objects use different ready-made behaviours.",
                    "Turn one behaviour into code at a time: pick objects of one kind.",
                ));
            }
            if kind.is_none() {
                first_name = object.name.clone();
            }
            kind = Some(this);
        }
        let ready = *kind.ok_or_else(|| {
            Problem::new(
                "No object is picked.",
                "Pick the object whose behaviour you want as code.",
            )
        })?;
        let all_with_it: Vec<u64> = doc
            .file
            .iter()
            .flat_map(|file| &file.objects)
            .filter(|object| object.kind == ready.name)
            .map(|object| object.id().number())
            .collect();

        // Its name: the one asked for, or the object's ("duck"), made
        // unique ("duck 2") if the game has a script or file of that name.
        let list = ScriptsFile::read(&game_folder).unwrap_or_default();
        let taken = |name: &str| {
            list.get(name).is_some()
                || game_folder
                    .join("scripts")
                    .join(format!("{name}.rs"))
                    .exists()
        };
        let (name, type_name) = match name {
            Some(typed) => {
                let (name, type_name) = new_name(typed)?;
                if taken(&name) {
                    return Err(Problem::new(
                        format!("This game has a script called \"{name}\" already."),
                        "Give the new one another name.",
                    ));
                }
                (name, type_name)
            }
            None => {
                let base = new_name(&first_name)
                    .or_else(|_| new_name(&format!("my {}", ready.name)))?
                    .0;
                let mut number = 1;
                loop {
                    let typed = if number == 1 {
                        base.clone()
                    } else {
                        format!("{base} {number}")
                    };
                    let (name, type_name) = new_name(&typed)?;
                    if !taken(&name) {
                        break (name, type_name);
                    }
                    number += 1;
                }
            }
        };
        let code = ready::copy_for_a_game(&ready, &name, &type_name).ok_or_else(|| {
            Problem::new(
                format!("Duck Forge has no file for {}.", ready.label),
                "This is a mistake in Duck Forge: please say so.",
            )
        })?;
        let connects_main = std::fs::read_to_string(game_folder.join("main.rs"))
            .is_ok_and(|main| connect_main(&main).0 != main);
        Ok(TurnPlan {
            ready,
            ids: ids.to_vec(),
            all_with_it,
            path: format!("scripts/{name}.rs"),
            name,
            type_name,
            code,
            connects_main,
        })
    }

    /// `script.turn_into_code`: gives the game its own copy of the
    /// objects' ready-made behaviour, as a script of its own: the file,
    /// its line in `scripts.rs` (and `main.rs` connected, the first
    /// time), and the objects' kind. Their settings stay as they are. One
    /// step for Undo, which puts every file back.
    pub(super) fn script_turn_into_code(
        &mut self,
        ids: Vec<u64>,
        name: Option<String>,
    ) -> Result<String, Problem> {
        let plan = self.turn_into_code_plan(&ids, name.as_deref())?;
        let game_folder = self.files_folder()?;
        let main_path = game_folder.join("main.rs");
        let main_before = std::fs::read_to_string(&main_path).unwrap_or_default();
        let main_after = if main_before.is_empty() {
            main_before.clone()
        } else {
            let (text, done) = connect_main(&main_before);
            if let Connected::Tell { line } = done {
                return Err(Problem::new(
                    "Duck Forge can't tell where main.rs loads the scene, so the game wouldn't use the new code.",
                    format!(
                        "Add .with_scripts(scripts::all()) after Scene::load(...) in main.rs (near line {line}), then turn it into code again."
                    ),
                ));
            }
            text
        };
        let scripts_path = game_folder.join(SCRIPTS_FILE);
        let scripts_before = std::fs::read_to_string(&scripts_path).unwrap_or_default();
        let mut list = ScriptsFile::parse(&scripts_before);
        list.entries.push(ScriptEntry {
            name: plan.name.clone(),
            path: plan.path.clone(),
            module: plan.name.clone(),
            type_name: plan.type_name.clone(),
        });
        let scripts_after = list.text();
        let file = game_folder.join(&plan.path);

        // The scene's change first: if it can't be made, nothing is written.
        let changes: Vec<(u64, Map<String, Value>)> = plan
            .ids
            .iter()
            .map(|id| {
                (
                    *id,
                    Map::from_iter([(String::from("kind"), json!(plan.name))]),
                )
            })
            .collect();
        let open = self.open_game()?;
        let doc = open.scene.as_ref().ok_or_else(super::objects::no_scene)?;
        let scene_after = doc.with_set(&changes)?;
        let who = open.called(&plan.ids);

        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).map_err(|error| {
                Problem::new(
                    format!("The folder for {} couldn't be made.", plan.path),
                    error.to_string(),
                )
            })?;
        }
        let mut files = vec![FileEdit {
            path: file.clone(),
            before: String::new(),
            after: plan.code.clone(),
        }];
        crate::level::write_whole(&file, &plan.code)?;
        crate::level::write_whole(&scripts_path, &scripts_after)?;
        files.push(FileEdit {
            path: scripts_path,
            before: scripts_before,
            after: scripts_after,
        });
        if main_after != main_before {
            crate::level::write_whole(&main_path, &main_after)?;
            files.push(FileEdit {
                path: main_path,
                before: main_before,
                after: main_after,
            });
        }
        let title = format!(
            "Turn {who}'s {} into code ({})",
            plan.ready.label, plan.path
        );
        self.write_scene_with(scene_after, Vec::new(), files, title, None)?;
        if let Some(open) = self.open.as_mut() {
            open.files_selected = Some(plan.path.clone());
        }
        Ok(format!(
            "{who} now runs {}, this game's own copy of the {}. Building it in...",
            plan.path, plan.ready.label
        ))
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
