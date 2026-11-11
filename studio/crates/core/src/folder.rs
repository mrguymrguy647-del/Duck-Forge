//! The games folder: where the games are, and where everything about
//! them lives.
//!
//! Every path the Studio uses, and every command it runs on a game, comes
//! from here and nowhere else. Today the games are the examples of one
//! Duck Forge folder (`examples/<game>`, with playthroughs, tune files and
//! `out/` beside them, as agents and the tool scripts expect). Which folder
//! that is, is a Studio setting, and if games move out of the engine's
//! folder later, this is the one file that changes.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::console::CommandLine;
use crate::problem::Problem;

/// The examples that come with Duck Forge. Home lists them apart from the
/// games a person made.
pub const CAME_WITH_DUCK_FORGE: &[&str] = &[
    "breakout",
    "dodge",
    "door_short",
    "door_show_3d",
    "duck_craft",
    "duck_peaks",
    "duckling_parade",
    "hello",
    "mini_craft",
    "physics",
    "pollen_run",
    "quickstart",
    "shapes",
    "sound_check",
    "starter_runner3d",
    "starter_shooter",
    "starter_topdown",
    "stress3d",
    "template",
    "world3d",
];

/// A starter to make a new game from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Starter {
    /// What `tools/new_game` calls it.
    pub kind: &'static str,
    /// What the Studio calls it.
    pub title: &'static str,
    /// The example it's copied from.
    pub example: &'static str,
    pub about: &'static str,
    /// Whether it has a tile level Build mode can paint.
    pub has_level: bool,
}

pub const STARTERS: &[Starter] = &[
    Starter {
        kind: "platformer",
        title: "Platformer",
        example: "template",
        about: "Run, jump, collect coins, reach the flag. Has a level you can paint.",
        has_level: true,
    },
    Starter {
        kind: "topdown",
        title: "Top-down",
        example: "starter_topdown",
        about: "Walk through rooms seen from above, pick things up, dodge enemies. Has a level you can paint.",
        has_level: true,
    },
    Starter {
        kind: "shooter",
        title: "Shooter",
        example: "starter_shooter",
        about: "Fly, shoot, dodge waves of enemies.",
        has_level: false,
    },
    Starter {
        kind: "runner3d",
        title: "3D runner",
        example: "starter_runner3d",
        about: "Run down three lanes in 3D, jump, collect coins.",
        has_level: false,
    },
    Starter {
        kind: "tiny",
        title: "Tiny",
        example: "quickstart",
        about: "The smallest game: one file to read from top to bottom.",
        has_level: false,
    },
];

/// One game in the folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    /// Its name in files and commands: `bee_maze`.
    pub name: String,
    /// Its name in words: "Bee Maze".
    pub title: String,
    /// Its main file.
    pub main: PathBuf,
    /// Its own folder, if it has one (a one-file game doesn't).
    pub folder: Option<PathBuf>,
    /// Whether it came with Duck Forge.
    pub came_with: bool,
}

/// What a game's card says on Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Card {
    ChecksPass,
    /// With why, in a few words: "2 problems", "never checked".
    NeedsALook(String),
}

/// A game's name in words: `bee_maze` becomes "Bee Maze".
pub fn title_of(name: &str) -> String {
    name.split('_')
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut letters = word.chars();
            match letters.next() {
                Some(first) => first.to_uppercase().chain(letters).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The name a new game gets in files, from what a person typed: "Bee
/// Maze!" becomes `bee_maze`. `Err` says what's wrong, plainly.
pub fn name_from_title(typed: &str) -> Result<String, String> {
    let mut name = String::new();
    for letter in typed.trim().chars() {
        if letter.is_ascii_alphanumeric() {
            name.push(letter.to_ascii_lowercase());
        } else if (letter == ' ' || letter == '_' || letter == '-')
            && !name.ends_with('_')
            && !name.is_empty()
        {
            name.push('_');
        }
    }
    let name = name.trim_end_matches('_').to_string();
    match name.chars().next() {
        None => Err(String::from("Give your game a name, like Bee Maze.")),
        Some(first) if !first.is_ascii_lowercase() => Err(String::from(
            "A game's name starts with a letter, like Bee Maze.",
        )),
        Some(_) => Ok(name),
    }
}

/// The games folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamesFolder {
    root: PathBuf,
}

impl GamesFolder {
    /// Opens `root` as the games folder, or says why it isn't one.
    pub fn open(root: impl Into<PathBuf>) -> Result<GamesFolder, Problem> {
        let root: PathBuf = root.into();
        let manifest = std::fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
        if !manifest.contains("name = \"duckforge\"") || !root.join("examples").is_dir() {
            return Err(Problem::new(
                format!("{} isn't a Duck Forge folder.", root.display()),
                "Choose the folder that holds Duck Forge: the one with Cargo.toml and an \
                 examples folder in it.",
            ));
        }
        Ok(GamesFolder { root })
    }

    /// The folder to use when no setting says: `duckforge` next to the
    /// Studio's own program (how the release zip is laid out), or the
    /// nearest Duck Forge folder above where the Studio was started.
    pub fn find() -> Option<PathBuf> {
        let mut places = Vec::new();
        if let Ok(program) = std::env::current_exe() {
            if let Some(beside) = program.parent() {
                places.push(beside.join("duckforge"));
            }
            places.extend(program.ancestors().skip(1).map(Path::to_path_buf));
        }
        if let Ok(here) = std::env::current_dir() {
            places.extend(here.ancestors().map(Path::to_path_buf));
        }
        places
            .into_iter()
            .find(|place| GamesFolder::open(place.clone()).is_ok())
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Every game in the folder, by name.
    pub fn games(&self) -> Vec<Game> {
        let mut games = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.root.join("examples")) else {
            return games;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            let (main, folder) = if path.is_dir() && path.join("main.rs").is_file() {
                (path.join("main.rs"), Some(path.clone()))
            } else if path.extension().is_some_and(|e| e == "rs") && path.is_file() {
                (path.clone(), None)
            } else {
                continue;
            };
            games.push(Game {
                title: title_of(&name),
                came_with: CAME_WITH_DUCK_FORGE.contains(&&*name),
                name: name.into_owned(),
                main,
                folder,
            });
        }
        games.sort_by(|a, b| a.name.cmp(&b.name));
        games
    }

    pub fn game(&self, name: &str) -> Option<Game> {
        self.games().into_iter().find(|game| game.name == name)
    }

    /// The files a build of this game depends on: its own code, the
    /// engine's, and `Cargo.toml`. When one changes, the game is rebuilt.
    pub fn code_files(&self, game: &Game) -> Vec<PathBuf> {
        let mut files = vec![self.root.join("Cargo.toml")];
        match &game.folder {
            Some(folder) => rust_files(folder, &mut files),
            None => files.push(game.main.clone()),
        }
        rust_files(&self.root.join("src"), &mut files);
        files.sort();
        files
    }

    /// Every file that's part of a game: its code, pictures, sounds, tune
    /// file and playthroughs. A Check is out of date when one is newer.
    pub fn game_files(&self, game: &Game) -> Vec<PathBuf> {
        let mut files = Vec::new();
        match &game.folder {
            Some(folder) => all_files(folder, &mut files),
            None => files.push(game.main.clone()),
        }
        files.push(self.tune_file(&game.name));
        files.extend(self.playthroughs(&game.name));
        files.retain(|file| file.is_file());
        files.sort();
        files
    }

    pub fn tune_file(&self, game: &str) -> PathBuf {
        self.root.join("examples").join(format!("{game}.tune.json"))
    }

    /// The game's recorded playthroughs: `<game>.json`, `<game>-*.json` and
    /// `<game>_*.json`, but not a file that's another game's (the same rule
    /// as `tools/verify`).
    pub fn playthroughs(&self, game: &str) -> Vec<PathBuf> {
        let others: Vec<String> = self
            .games()
            .into_iter()
            .map(|other| other.name)
            .filter(|other| other != game)
            .collect();
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(self.root.join("playthroughs")) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(stem) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some(stem) = stem.strip_suffix(".json") else {
                continue;
            };
            let mine = stem == game
                || stem
                    .strip_prefix(game)
                    .is_some_and(|rest| rest.starts_with('-') || rest.starts_with('_'));
            // A file named after another example, up to its first `-`, is
            // that example's: mini_craft.json isn't a playthrough of mini.
            let owner = stem.split('-').next().unwrap_or(stem);
            if mine && !others.iter().any(|other| other == owner) {
                found.push(path);
            }
        }
        found.sort();
        found
    }

    /// Builds the game with its side of the Studio link.
    pub fn build_command(&self, cargo: &Path, game: &str) -> CommandLine {
        CommandLine::new(
            cargo,
            &[
                "build",
                "--example",
                game,
                "--features",
                "studio",
                "--message-format=json-render-diagnostics",
            ],
            &self.root,
        )
        .shown_as(format!("cargo build --example {game} --features studio"))
    }

    /// The whole check for one game: `tools/verify`, as a person runs it.
    pub fn check_command(&self, game: &str, fuzz_runs: u32) -> CommandLine {
        self.tool("verify", &[game, &fuzz_runs.to_string()])
    }

    /// Makes a new game from a starter: `tools/new_game`. The Studio
    /// builds and checks it itself afterwards, so the script only copies.
    pub fn new_game_command(&self, name: &str, kind: &str) -> CommandLine {
        self.tool("new_game", &[name, kind])
            .with_env("NEW_GAME_NO_VERIFY", "1")
    }

    /// One of the scripts in `tools/`: the `.bat` on Windows, the `.sh`
    /// elsewhere.
    fn tool(&self, tool: &str, args: &[&str]) -> CommandLine {
        if cfg!(windows) {
            let script = format!("tools\\{tool}.bat");
            let mut words = vec!["/C", script.as_str()];
            words.extend(args);
            CommandLine::new("cmd", &words, &self.root)
                .shown_as(format!("{script} {}", args.join(" ")))
        } else {
            let script = format!("tools/{tool}.sh");
            CommandLine::new(self.root.join(&script), args, &self.root)
                .shown_as(format!("{script} {}", args.join(" ")))
        }
    }

    /// Where a Check saves its pictures and summaries.
    pub fn check_folder(&self, game: &str) -> PathBuf {
        self.root.join("out").join("verify").join(game)
    }

    /// Where the Studio saves every play session, as an input script.
    pub fn sessions_folder(&self, game: &str) -> PathBuf {
        self.root.join("out").join("sessions").join(game)
    }

    /// Where clips are saved.
    pub fn clips_folder(&self) -> PathBuf {
        self.root.join("out").join("clips")
    }

    /// Where the Studio keeps the copies of built games that it plays
    /// (Windows won't let cargo replace a program that's running).
    pub fn run_folder(&self) -> PathBuf {
        self.root.join("target").join("studio-run")
    }

    /// The newest change to any of the game's files.
    pub fn last_changed(&self, game: &Game) -> Option<SystemTime> {
        self.game_files(game)
            .iter()
            .filter_map(|file| std::fs::metadata(file).ok()?.modified().ok())
            .max()
    }

    /// A picture of the game for its card: the last frame of its latest
    /// checked playthrough, if it has been checked.
    pub fn card_picture(&self, game: &str) -> Option<PathBuf> {
        let folder = self.check_folder(game);
        let named = folder.join(game).join("last.png");
        if named.is_file() {
            return Some(named);
        }
        let mut others: Vec<PathBuf> = std::fs::read_dir(&folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path().join("last.png"))
            .filter(|picture| picture.is_file())
            .collect();
        others.sort();
        // Never checked with a playthrough: the picture in the docs, if
        // the game has one.
        let in_docs = self.root.join("docs").join(format!("{game}.png"));
        others
            .into_iter()
            .next()
            .or_else(|| in_docs.is_file().then_some(in_docs))
    }

    /// The command that records a game's first playthrough with its bot,
    /// if it has one (`bot.rs` with a `record_playthrough` test), as
    /// `tools/new_game` does.
    pub fn first_playthrough_command(&self, cargo: &Path, game: &Game) -> Option<CommandLine> {
        let bot = game.folder.as_ref()?.join("bot.rs");
        if !bot.is_file() || !self.playthroughs(&game.name).is_empty() {
            return None;
        }
        Some(
            CommandLine::new(
                cargo,
                &[
                    "test",
                    "-q",
                    "--no-default-features",
                    "--example",
                    &game.name,
                    "--",
                    "--ignored",
                    "record_playthrough",
                ],
                &self.root,
            )
            .shown_as(format!(
                "cargo test -q --no-default-features --example {} -- --ignored record_playthrough",
                game.name
            )),
        )
    }
}

/// Adds every `.rs` file under `folder`.
fn rust_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let mut all = Vec::new();
    all_files(folder, &mut all);
    files.extend(
        all.into_iter()
            .filter(|f| f.extension().is_some_and(|e| e == "rs")),
    );
}

/// Adds every file under `folder`.
fn all_files(folder: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            all_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The real Duck Forge folder this crate lives in.
    pub(crate) fn duck_forge() -> GamesFolder {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        GamesFolder::open(root.canonicalize().unwrap()).unwrap()
    }

    #[test]
    fn the_folder_lists_its_games_and_knows_which_came_with_duck_forge() {
        let folder = duck_forge();
        let games = folder.games();
        let peaks = games.iter().find(|g| g.name == "duck_peaks").unwrap();
        assert_eq!(peaks.title, "Duck Peaks");
        assert!(peaks.came_with && peaks.folder.is_some());
        let hello = games.iter().find(|g| g.name == "hello").unwrap();
        assert!(hello.folder.is_none(), "a one-file game");
        // The list of what came with Duck Forge is every example here: a
        // new example must be added to it (or it shows up under My games).
        let names: Vec<&str> = games.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, CAME_WITH_DUCK_FORGE);
        for starter in STARTERS {
            assert!(names.contains(&starter.example), "{starter:?}");
        }
    }

    #[test]
    fn a_folder_that_isnt_duck_forge_is_refused_plainly() {
        let problem = GamesFolder::open(std::env::temp_dir()).unwrap_err();
        assert!(
            problem.what.contains("isn't a Duck Forge folder"),
            "{problem}"
        );
    }

    #[test]
    fn playthroughs_follow_verifys_rule() {
        let folder = duck_forge();
        let names = |game: &str| -> Vec<String> {
            folder
                .playthroughs(game)
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            names("duck_craft"),
            ["duck_craft.json", "duck_craft_night.json"]
        );
        assert_eq!(
            names("breakout"),
            ["breakout-lose.json", "breakout-win.json"]
        );
        assert_eq!(names("hello"), Vec::<String>::new());
        // The same as tools/verify.sh lists, where there's a shell to ask.
        if cfg!(unix) {
            for game in ["duck_craft", "breakout", "physics", "template"] {
                let listed = std::process::Command::new(folder.root().join("tools/verify.sh"))
                    .args(["--list-playthroughs", game])
                    .current_dir(folder.root())
                    .output()
                    .unwrap();
                let listed: Vec<String> = String::from_utf8_lossy(&listed.stdout)
                    .lines()
                    .map(|line| line.rsplit('/').next().unwrap_or(line).to_string())
                    .collect();
                assert_eq!(names(game), listed, "{game}");
            }
        }
    }

    #[test]
    fn names_and_titles_go_both_ways() {
        assert_eq!(title_of("bee_maze"), "Bee Maze");
        assert_eq!(name_from_title("Bee Maze"), Ok(String::from("bee_maze")));
        assert_eq!(
            name_from_title("  Bee   Maze! 2 "),
            Ok(String::from("bee_maze_2"))
        );
        assert!(name_from_title("").is_err());
        assert!(
            name_from_title("3 Ducks")
                .unwrap_err()
                .contains("starts with a letter")
        );
    }

    #[test]
    fn commands_are_the_ones_a_person_would_type() {
        let folder = duck_forge();
        let build = folder.build_command(Path::new("cargo"), "dodge");
        assert_eq!(build.shown, "cargo build --example dodge --features studio");
        let check = folder.check_command("dodge", 50);
        let new = folder.new_game_command("bee_maze", "platformer");
        if cfg!(windows) {
            assert_eq!(check.shown, "tools\\verify.bat dodge 50");
            assert_eq!(new.shown, "tools\\new_game.bat bee_maze platformer");
        } else {
            assert_eq!(check.shown, "tools/verify.sh dodge 50");
            assert_eq!(new.shown, "tools/new_game.sh bee_maze platformer");
        }
    }

    #[test]
    fn code_files_are_the_games_and_the_engines() {
        let folder = duck_forge();
        let game = folder.game("duck_peaks").unwrap();
        let files = folder.code_files(&game);
        let has = |end: &str| files.iter().any(|f| f.ends_with(end));
        assert!(has("examples/duck_peaks/main.rs") && has("examples/duck_peaks/bot.rs"));
        assert!(has("src/tilemap.rs") && has("Cargo.toml"));
        assert!(!has("examples/dodge.rs"), "not another game's code");
    }
}
