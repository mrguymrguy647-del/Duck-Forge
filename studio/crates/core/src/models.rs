//! Models: Duck Mold's recipes in a game's folder.
//!
//! A game's 3D models are made with Duck Mold. A model is a recipe (a
//! `.toml` or `.json` text file, `art/robot.toml`), and what the game
//! loads is the `.glb` Duck Mold writes from it (`assets/robot.glb`).
//! The Studio opens a recipe in a tab that is Duck Mold's own Studio,
//! and can run Duck Mold's edits and its export itself, with no window:
//! this file is that part. Nothing here draws anything.
//!
//! - An edit is written as it follows `duck-mold edit <recipe>` on the
//!   command line (`set head --color red`), and is Duck Mold's own edit:
//!   it changes only the lines it must, and it is one step of Duck
//!   Mold's Undo, the one kept on disk for each recipe. So an edit made
//!   here can be undone in Duck Mold's own Studio, and the other way
//!   round.
//! - "Send to game" writes the `.glb` where the recipe's `export` line
//!   says, or into the game's `assets` folder if it has none. The Studio
//!   already watches that folder, so a running game takes the new model
//!   at once.

use std::path::{Path, PathBuf};

use crate::Problem;

/// The models open in tabs, for one game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Models {
    /// Each tab's file, as a place in the game's folder
    /// (`art/robot.toml`), in the order of the tabs.
    pub tabs: Vec<String>,
    /// The tab in front. `None`: the game itself.
    pub front: Option<String>,
}

impl Models {
    /// Opens a tab for `place` (or finds the one it has) and brings it
    /// to the front.
    pub fn open(&mut self, place: &str) {
        if !self.tabs.iter().any(|tab| tab == place) {
            self.tabs.push(place.to_string());
        }
        self.front = Some(place.to_string());
    }

    /// Closes the tab for `place`. If it was in front, the tab before
    /// it comes to the front (the game, if it was the first). True if
    /// there was such a tab.
    pub fn close(&mut self, place: &str) -> bool {
        let Some(at) = self.tabs.iter().position(|tab| tab == place) else {
            return false;
        };
        self.tabs.remove(at);
        if self.front.as_deref() == Some(place) {
            self.front = at.checked_sub(1).map(|before| self.tabs[before].clone());
        }
        true
    }
}

/// What kind of file a model's tab can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A Duck Mold recipe: it can be changed.
    Recipe,
    /// A model file (`.glb`, `.gltf`, `.obj`): it can only be looked at.
    Model,
}

/// What kind of model file `file` is, if it is one. A `.toml` or
/// `.json` file is a recipe only if it reads as one (it has `parts`):
/// a game has other files with those endings, `Cargo.toml` for one.
pub fn kind_of(file: &Path) -> Option<Kind> {
    let ending = file.extension()?.to_str()?.to_lowercase();
    match ending.as_str() {
        "glb" | "gltf" | "obj" => Some(Kind::Model),
        "toml" | "json" => {
            let syntax = duck_mold_core::source::Syntax::from_path(file)?;
            let text = std::fs::read_to_string(file).ok()?;
            top_level(&text, syntax, "parts").map(|_| Kind::Recipe)
        }
        _ => None,
    }
}

/// A field at the top of a recipe, if the text reads and has it.
fn top_level(
    text: &str,
    syntax: duck_mold_core::source::Syntax,
    field: &str,
) -> Option<duck_mold_core::source::Value> {
    let node = duck_mold_core::source::parse(text, syntax).ok()?;
    let duck_mold_core::source::Value::Table(entries) = node.value else {
        return None;
    };
    entries
        .into_iter()
        .find(|entry| entry.key == field)
        .map(|entry| entry.node.value)
}

/// Where a recipe's `.glb` goes: where its `export` line says, from the
/// recipe's own folder. `None` if it has no such line.
pub fn export_of(recipe: &Path) -> Option<PathBuf> {
    let syntax = duck_mold_core::source::Syntax::from_path(recipe)?;
    let text = std::fs::read_to_string(recipe).ok()?;
    match top_level(&text, syntax, "export")? {
        duck_mold_core::source::Value::Str(line) => {
            Some(duck_mold_core::tools::export_path(recipe, Some(&line)))
        }
        _ => None,
    }
}

/// The folders of a game that aren't looked through for recipes: what
/// Cargo builds, and hidden folders.
fn skipped(name: &str) -> bool {
    name == "target" || name.starts_with('.')
}

/// Every recipe in a game's folder, a few folders deep, as places in it
/// (`art/robot.toml`), in alphabetical order.
pub fn recipes_in(game: &Path) -> Vec<String> {
    fn look(folder: &Path, place: &str, depth: usize, found: &mut Vec<String>) {
        if depth > 4 || found.len() >= 500 {
            return;
        }
        let mut entries: Vec<_> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            let inside = if place.is_empty() {
                name.clone()
            } else {
                format!("{place}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                if !skipped(&name) {
                    look(&path, &inside, depth + 1, found);
                }
            } else if kind_of(&path) == Some(Kind::Recipe) {
                found.push(inside);
            }
        }
    }
    let mut found = Vec::new();
    look(game, "", 0, &mut found);
    found
}

/// The recipe a model file in a game was made from, as a place in the
/// game's folder: the recipe whose `export` line names that file, or
/// else one of the same name in `art` or beside it. `None` if there is
/// none: the file can then only be looked at.
pub fn recipe_for(game: &Path, model: &str) -> Option<String> {
    let same = |a: &Path, b: &Path| match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    };
    let file = game.join(model);
    let recipes = recipes_in(game);
    if let Some(found) = recipes
        .iter()
        .find(|recipe| export_of(&game.join(recipe)).is_some_and(|export| same(&export, &file)))
    {
        return Some(found.clone());
    }
    let stem = Path::new(model).file_stem()?.to_str()?;
    let beside = Path::new(model)
        .parent()
        .map(|folder| folder.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let mut places = Vec::new();
    for ending in ["toml", "json"] {
        places.push(format!("art/{stem}.{ending}"));
        places.push(if beside.is_empty() {
            format!("{stem}.{ending}")
        } else {
            format!("{beside}/{stem}.{ending}")
        });
    }
    places.into_iter().find(|place| recipes.contains(place))
}

/// The problem for a file that isn't a recipe, when one is needed.
fn not_a_recipe(place: &str) -> Problem {
    Problem::new(
        format!("{place} isn't a Duck Mold recipe."),
        "Name a model's recipe: a .toml or .json file with parts, like art/robot.toml.",
    )
}

/// Reads one edit written as it follows `duck-mold edit <recipe>`
/// (`set head --color red`) into Duck Mold's own edits: usually one, a
/// few commands are more. The problem says what couldn't be read.
pub fn read_edit(words: &str) -> Result<Vec<duck_mold_core::edit::Edit>, Problem> {
    duck_mold_cli::edit::edits_from_words(words).map_err(|why| {
        Problem::new(
            format!("Duck Mold can't read the edit \"{words}\": {why}."),
            "Write it as it follows `duck-mold edit <recipe>`, like: set head --color red. \
             Duck Mold's guide (docs/AI_GUIDE.md there) lists every edit.",
        )
    })
}

/// Makes Duck Mold's edits to a recipe: `words` is one edit as it
/// follows `duck-mold edit <recipe>` (`set head --color red`). Each is
/// one step of Duck Mold's Undo, kept in `history` (Duck Mold's own
/// settings folder) if there is one. Says what changed.
///
/// If the words can't be read, or an edit can't be made (it would leave
/// the recipe with a mistake), nothing is written and the problem says
/// why, in Duck Mold's words.
pub fn edit(
    recipe: &Path,
    place: &str,
    words: &str,
    history: Option<&Path>,
) -> Result<String, Problem> {
    if kind_of(recipe) != Some(Kind::Recipe) {
        return Err(not_a_recipe(place));
    }
    let edits = read_edit(words)?;
    let mut studio = duck_mold_view::studio::Studio::open(recipe, false);
    if let Some(history) = history {
        studio.keep_history_in(history);
    }
    let mut said = Vec::new();
    for edit in &edits {
        match studio.apply(edit) {
            Ok(done) => said.push(done),
            Err(why) => {
                return Err(Problem::new(
                    format!("Duck Mold didn't make the edit \"{words}\" to {place}: {why}."),
                    "Nothing was changed. Fix the edit and run it again.",
                ));
            }
        }
    }
    Ok(said.join(". "))
}

/// A model that was sent to a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    /// The `.glb` that was written.
    pub file: PathBuf,
    /// How big it is, in bytes.
    pub bytes: usize,
    /// True if the recipe's own `export` line said where: without one,
    /// the model went to the game's `assets` folder.
    pub by_its_line: bool,
}

/// "Send to game": builds a recipe and writes its `.glb` where its
/// `export` line says, or else in the game's `assets` folder under the
/// recipe's name. Nothing is written if the recipe has a mistake: the
/// game keeps the last model that worked.
pub fn send(game: &Path, recipe: &Path, place: &str) -> Result<Sent, Problem> {
    if kind_of(recipe) != Some(Kind::Recipe) {
        return Err(not_a_recipe(place));
    }
    let by_its_line = export_of(recipe).is_some();
    let into_assets = (!by_its_line).then(|| {
        let name = recipe
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        game.join("assets").join(format!("{name}.glb"))
    });
    if let Some(folder) = into_assets.as_deref().and_then(Path::parent) {
        let _ = std::fs::create_dir_all(folder);
    }
    match duck_mold_core::tools::export_file(recipe, into_assets.as_deref()) {
        Ok(exported) => Ok(Sent {
            file: exported.path,
            bytes: exported.bytes,
            by_its_line,
        }),
        Err(failure) => {
            let report = failure.report();
            let first = report
                .errors
                .first()
                .map(|issue| {
                    let line = issue
                        .line
                        .map_or_else(String::new, |line| format!(" (line {line})"));
                    format!("{}{line}", issue.message)
                })
                .unwrap_or_else(|| String::from("it can't be built"));
            let details: Vec<String> = report
                .errors
                .iter()
                .map(|issue| issue.to_string())
                .collect();
            Err(Problem::new(
                format!("{place} wasn't sent to the game: {first}."),
                "The game keeps the last model that worked. Fix the recipe and send it again.",
            )
            .with_details(details.join("\n")))
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    pub(crate) const CRATE: &str = "version = 0\nname = \"crate\"\n\n[palette]\nwood = \"#B07A3F\"\nred = \"#D0453A\"\n\n[[parts]]\nname = \"box\"\nshape = \"box\"\nsize = [1, 1, 1]\ncolor = \"wood\"\n";

    /// A game's folder with a crate's recipe in `art`.
    fn game(test: &str) -> (TestFolder, PathBuf) {
        let folder = TestFolder::new(test);
        let game = folder.join("my_game");
        std::fs::create_dir_all(game.join("art")).unwrap();
        std::fs::create_dir_all(game.join("assets")).unwrap();
        std::fs::write(game.join("art/crate.toml"), CRATE).unwrap();
        std::fs::write(game.join("Cargo.toml"), "[package]\nname = \"my_game\"\n").unwrap();
        std::fs::write(game.join("level.json"), "{\"tiles\": []}").unwrap();
        (folder, game)
    }

    #[test]
    fn a_recipe_is_told_from_the_games_other_files() {
        let (_folder, game) = game("models-kinds");
        assert_eq!(kind_of(&game.join("art/crate.toml")), Some(Kind::Recipe));
        assert_eq!(kind_of(&game.join("Cargo.toml")), None);
        assert_eq!(kind_of(&game.join("level.json")), None);
        assert_eq!(kind_of(&game.join("assets/robot.glb")), Some(Kind::Model));
        assert_eq!(kind_of(&game.join("assets/duck.png")), None);
        assert_eq!(kind_of(&game.join("art/gone.toml")), None);
        // Only recipes are listed, and nothing Cargo built.
        std::fs::create_dir_all(game.join("target/debug")).unwrap();
        std::fs::write(game.join("target/debug/copy.toml"), CRATE).unwrap();
        std::fs::write(
            game.join("art/lamp.json"),
            "{\"version\": 0, \"parts\": []}",
        )
        .unwrap();
        assert_eq!(recipes_in(&game), ["art/crate.toml", "art/lamp.json"]);
    }

    #[test]
    fn tabs_open_come_to_the_front_and_close() {
        let mut models = Models::default();
        models.open("art/crate.toml");
        models.open("art/robot.toml");
        models.open("art/crate.toml");
        assert_eq!(models.tabs, ["art/crate.toml", "art/robot.toml"]);
        assert_eq!(models.front.as_deref(), Some("art/crate.toml"));
        // Closing the one in front shows the game, if it was the first.
        assert!(models.close("art/crate.toml"));
        assert_eq!(models.front, None);
        models.open("art/lamp.toml");
        assert!(models.close("art/lamp.toml"));
        assert_eq!(models.front.as_deref(), Some("art/robot.toml"));
        assert!(!models.close("art/lamp.toml"));
    }

    #[test]
    fn an_edit_is_duck_molds_own_and_one_step_of_its_undo() {
        let (folder, game) = game("models-edit");
        let recipe = game.join("art/crate.toml");
        let history = folder.join("duck-mold/undo");
        let said = edit(
            &recipe,
            "art/crate.toml",
            "set box --color red",
            Some(&history),
        )
        .unwrap();
        assert!(said.contains("box"), "{said}");
        let text = std::fs::read_to_string(&recipe).unwrap();
        assert_eq!(text, CRATE.replace("color = \"wood\"", "color = \"red\""));
        // Duck Mold's own Studio, opened on the recipe later, takes the
        // change back: the Undo is the same one.
        let mut studio = duck_mold_view::studio::Studio::open(&recipe, false);
        studio.keep_history_in(&history);
        assert!(studio.can_undo().is_some());
        studio.undo().unwrap();
        assert_eq!(std::fs::read_to_string(&recipe).unwrap(), CRATE);

        // Words Duck Mold can't read, and an edit it won't make: a
        // sentence each, and nothing written.
        let problem = edit(&recipe, "art/crate.toml", "paint it red", Some(&history)).unwrap_err();
        assert!(
            problem
                .what
                .contains("can't read the edit \"paint it red\""),
            "{}",
            problem.what
        );
        let problem = edit(
            &recipe,
            "art/crate.toml",
            "set box --color purple",
            Some(&history),
        )
        .unwrap_err();
        assert!(problem.what.contains("purple"), "{}", problem.what);
        assert_eq!(std::fs::read_to_string(&recipe).unwrap(), CRATE);
        // Not a recipe at all.
        let problem = edit(
            &game.join("Cargo.toml"),
            "Cargo.toml",
            "set box --color red",
            None,
        )
        .unwrap_err();
        assert_eq!(problem.what, "Cargo.toml isn't a Duck Mold recipe.");
    }

    #[test]
    fn send_writes_into_the_game_and_the_engine_reads_it() {
        let (_folder, game) = game("models-send");
        let recipe = game.join("art/crate.toml");
        // Without an export line: the game's assets, under its name.
        let sent = send(&game, &recipe, "art/crate.toml").unwrap();
        assert_eq!(sent.file, game.join("assets/crate.glb"));
        assert!(!sent.by_its_line);
        assert_eq!(
            sent.bytes as u64,
            std::fs::metadata(&sent.file).unwrap().len()
        );
        // The Studio's own engine loads what the Duck Mold it names
        // wrote: "Send to game" never writes a file the game can't read.
        let model = duckforge::glb::GlbModel::load(&sent.file).unwrap();
        assert_eq!(model.name, "crate");
        assert_eq!(model.triangles.len(), 12);
        assert_eq!(
            recipe_for(&game, "assets/crate.glb").as_deref(),
            Some("art/crate.toml")
        );

        // With the line: wherever it says, from the recipe's folder.
        edit(
            &recipe,
            "art/crate.toml",
            "set-export ../assets/models/box.glb",
            None,
        )
        .unwrap();
        let sent = send(&game, &recipe, "art/crate.toml").unwrap();
        assert!(sent.by_its_line);
        assert_eq!(sent.file, game.join("assets/models/box.glb"));
        assert!(sent.file.is_file());
        assert_eq!(
            recipe_for(&game, "assets/models/box.glb").as_deref(),
            Some("art/crate.toml"),
            "found by its export line, whatever it is called"
        );
        assert_eq!(recipe_for(&game, "assets/other.glb"), None);

        // A recipe with a mistake isn't sent: the old file stays.
        let before = std::fs::read(&sent.file).unwrap();
        let text = std::fs::read_to_string(&recipe).unwrap();
        std::fs::write(&recipe, text.replace("color = \"wood\"", "color = \"mud\"")).unwrap();
        let problem = send(&game, &recipe, "art/crate.toml").unwrap_err();
        assert!(
            problem
                .what
                .starts_with("art/crate.toml wasn't sent to the game:"),
            "{}",
            problem.what
        );
        assert!(problem.what.contains("mud"), "{}", problem.what);
        assert_eq!(std::fs::read(&sent.file).unwrap(), before);
    }
}
