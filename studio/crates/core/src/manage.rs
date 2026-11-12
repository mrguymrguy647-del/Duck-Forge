//! Deleting, renaming and duplicating a game: Home's card menu, as the
//! commands `game.delete`, `game.rename` and `game.duplicate`.
//!
//! A game is more than its folder. Each of these also takes care of its
//! tune and feel files (`examples/<game>.tune.json`, `.feel.json`), its
//! playthroughs, its `[[example]]` lines in `Cargo.toml`, and (for delete
//! and rename) what the Studio keeps about it: its history, its checks and
//! its sessions. Renaming and duplicating also change the places where
//! the game's code names itself: paths like `examples/<game>/level.txt`
//! and `playthroughs/<game>.json`, `--example <game>`, and its window
//! title. Nothing else in the code is touched.
//!
//! Deleting moves everything to the Recycle Bin, never away for good, and
//! the examples that came with Duck Forge can't be deleted or renamed
//! (they can be duplicated, and the copy changed).

use std::path::{Path, PathBuf};

use crate::folder::{CAME_WITH_DUCK_FORGE, Game, GamesFolder, name_from_title, title_of};
use crate::problem::Problem;
use crate::recycle::RecycleBin;

/// The game's own files: its code (a folder or one file), its tune and
/// feel files, and its playthroughs. The ones that are there.
pub fn own_files(folder: &GamesFolder, game: &Game) -> Vec<PathBuf> {
    let mut files = vec![game.folder.clone().unwrap_or_else(|| game.main.clone())];
    for end in ["tune.json", "feel.json"] {
        files.push(examples(folder).join(format!("{}.{end}", game.name)));
    }
    files.extend(folder.playthroughs(&game.name));
    files.retain(|file| file.exists());
    files
}

/// What the Studio keeps about the game: its history, its checks and its
/// sessions.
pub fn kept_about(folder: &GamesFolder, name: &str) -> Vec<PathBuf> {
    [
        folder.root().join("history").join(name),
        folder.check_folder(name),
        folder.sessions_folder(name),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

fn examples(folder: &GamesFolder) -> PathBuf {
    folder.root().join("examples")
}

fn the_game(folder: &GamesFolder, name: &str) -> Result<Game, Problem> {
    folder.game(name).ok_or_else(|| {
        Problem::new(
            format!("There's no game called {name}."),
            "Its name in files is like bee_maze: see the card on Home.",
        )
    })
}

fn not_one_that_came_with(name: &str, doing: &str) -> Result<(), Problem> {
    if CAME_WITH_DUCK_FORGE.contains(&name) {
        return Err(Problem::new(
            format!(
                "{} came with Duck Forge, so it can't be {doing}.",
                title_of(name)
            ),
            "Duplicate it instead, and change the copy.",
        ));
    }
    Ok(())
}

/// The name for a new or renamed game, from what was typed, if it's free.
pub fn free_name(folder: &GamesFolder, typed: &str) -> Result<String, Problem> {
    let name = name_from_title(typed).map_err(|why| Problem::new(why, "Then try again."))?;
    let manifest = std::fs::read_to_string(folder.root().join("Cargo.toml")).unwrap_or_default();
    let taken = folder.game(&name).is_some()
        || examples(folder).join(&name).exists()
        || examples(folder).join(format!("{name}.rs")).exists()
        || example_names(&manifest).contains(&name)
        || CAME_WITH_DUCK_FORGE.contains(&name.as_str());
    if taken {
        return Err(Problem::new(
            format!("There's already a game called {}.", title_of(&name)),
            "Pick another name.",
        ));
    }
    Ok(name)
}

/// Moves the game and what the Studio keeps about it to the Recycle Bin,
/// and takes its lines out of `Cargo.toml`. Returns what was moved.
pub fn delete(folder: &GamesFolder, name: &str, bin: &RecycleBin) -> Result<Vec<PathBuf>, Problem> {
    not_one_that_came_with(name, "deleted")?;
    let game = the_game(folder, name)?;
    let mut going = own_files(folder, &game);
    going.extend(kept_about(folder, name));
    bin.throw_away(&going).map_err(|why| {
        Problem::new(
            format!("{} couldn't be moved to the Recycle Bin.", game.title),
            "Nothing was deleted. Close any program that has its files open (File Explorer, \
             an editor), then try again.",
        )
        .with_details(why)
    })?;
    edit_manifest(folder, |manifest| without_example(manifest, name))?;
    Ok(going)
}

/// Renames a game: its files, its lines in `Cargo.toml`, what the Studio
/// keeps about it, and where its code names itself. Returns its new name.
pub fn rename(folder: &GamesFolder, name: &str, typed: &str) -> Result<String, Problem> {
    not_one_that_came_with(name, "renamed")?;
    let game = the_game(folder, name)?;
    let new = free_name(folder, typed)?;
    let moves = moves_for(folder, &game, &new);
    for (from, to) in moves.iter().chain(
        kept_about(folder, name)
            .iter()
            .map(|from| (from.clone(), from.with_file_name(&new)))
            .collect::<Vec<_>>()
            .iter(),
    ) {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| couldnt("rename", from, e))?;
        }
        std::fs::rename(from, to).map_err(|e| couldnt("rename", from, e))?;
    }
    let code = new_code(folder, &game, &new);
    retarget_code(&code, name, &new)?;
    edit_manifest(folder, |manifest| renamed_example(manifest, name, &new))?;
    Ok(new)
}

/// Makes a copy of a game under a new name: its files and playthroughs,
/// with its code pointing at the copy's own, and its lines in
/// `Cargo.toml`. Returns the copy's name.
pub fn duplicate(folder: &GamesFolder, name: &str, typed: &str) -> Result<String, Problem> {
    let game = the_game(folder, name)?;
    let new = free_name(folder, typed)?;
    for (from, to) in moves_for(folder, &game, &new) {
        copy_all(&from, &to).map_err(|e| couldnt("copy", &from, e))?;
    }
    let code = new_code(folder, &game, &new);
    retarget_code(&code, name, &new)?;
    edit_manifest(folder, |manifest| with_example(manifest, &new))?;
    Ok(new)
}

/// Where each of the game's own files goes under the new name.
fn moves_for(folder: &GamesFolder, game: &Game, new: &str) -> Vec<(PathBuf, PathBuf)> {
    let examples = examples(folder);
    let mut moves = Vec::new();
    match &game.folder {
        Some(code) => moves.push((code.clone(), examples.join(new))),
        None => moves.push((game.main.clone(), examples.join(format!("{new}.rs")))),
    }
    for end in ["tune.json", "feel.json"] {
        let from = examples.join(format!("{}.{end}", game.name));
        if from.exists() {
            moves.push((from, examples.join(format!("{new}.{end}"))));
        }
    }
    for playthrough in folder.playthroughs(&game.name) {
        let file = playthrough
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let rest = file.strip_prefix(&game.name).unwrap_or(&file);
        moves.push((
            playthrough.clone(),
            playthrough.with_file_name(format!("{new}{rest}")),
        ));
    }
    moves
}

/// The code files of the game under its new name.
fn new_code(folder: &GamesFolder, game: &Game, new: &str) -> Vec<PathBuf> {
    let examples = examples(folder);
    if game.folder.is_some() {
        let mut files = Vec::new();
        let mut folders = vec![examples.join(new)];
        while let Some(dir) = folders.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    folders.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    files.push(path);
                }
            }
        }
        files
    } else {
        vec![examples.join(format!("{new}.rs"))]
    }
}

fn retarget_code(files: &[PathBuf], old: &str, new: &str) -> Result<(), Problem> {
    for file in files {
        let text = std::fs::read_to_string(file).map_err(|e| couldnt("read", file, e))?;
        let changed = retarget(&text, old, new);
        if changed != text {
            std::fs::write(file, changed).map_err(|e| couldnt("write", file, e))?;
        }
    }
    Ok(())
}

/// The code with each place it names the game by `old` naming it `new`:
/// paths (`examples/old/`, `examples/old.tune.json`, `playthroughs/old.json`,
/// `out/verify/old`...), `--example old`, and the window title
/// `"Old - duckforge"`. A word that only looks like the name (a variable
/// called `coin` in a game called coin) is left alone.
pub fn retarget(text: &str, old: &str, new: &str) -> String {
    let mut out = text.to_string();
    let title = |name: &str| format!("\"{} - duckforge\"", title_of(name));
    out = out.replace(&title(old), &title(new));
    for (before, ends) in [
        ("examples/", &["/", "."][..]),
        ("playthroughs/", &[".", "-", "_"][..]),
        ("out/verify/", &["/", "\"", "`", " "][..]),
        ("out/sessions/", &["/", "\"", "`", " "][..]),
    ] {
        for end in ends {
            out = out.replace(
                &format!("{before}{old}{end}"),
                &format!("{before}{new}{end}"),
            );
        }
    }
    // --example old, but not --example old_two.
    let flag = format!("--example {old}");
    let mut result = String::with_capacity(out.len());
    let mut rest = out.as_str();
    while let Some(at) = rest.find(&flag) {
        let after = &rest[at + flag.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        result.push_str(&rest[..at]);
        result.push_str(if whole { "--example " } else { &flag });
        if whole {
            result.push_str(new);
        }
        rest = after;
    }
    result.push_str(rest);
    result
}

fn couldnt(what: &str, path: &Path, error: std::io::Error) -> Problem {
    Problem::new(
        format!("The Studio couldn't {what} {}.", path.display()),
        "Close any program that has the game's files open, then try again.",
    )
    .with_details(error.to_string())
}

fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)?.flatten() {
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(to, std::fs::read(from)?)
    }
}

fn edit_manifest(folder: &GamesFolder, change: impl Fn(&str) -> String) -> Result<(), Problem> {
    let path = folder.root().join("Cargo.toml");
    let text = std::fs::read_to_string(&path).map_err(|e| couldnt("read", &path, e))?;
    let changed = change(&text);
    if changed != text {
        std::fs::write(&path, changed).map_err(|e| couldnt("write", &path, e))?;
    }
    Ok(())
}

/// The names of the `[[example]]` tables in a `Cargo.toml`.
pub fn example_names(manifest: &str) -> Vec<String> {
    tables(manifest)
        .into_iter()
        .filter_map(|table| table.example_name)
        .collect()
}

/// One table of a `Cargo.toml`, as lines (with their line ends).
struct Table<'a> {
    lines: Vec<&'a str>,
    /// For an `[[example]]` table, its name.
    example_name: Option<String>,
}

/// The file cut into tables: whatever comes before the first header,
/// then each header with the lines after it.
fn tables(manifest: &str) -> Vec<Table<'_>> {
    let mut tables = vec![Table {
        lines: Vec::new(),
        example_name: None,
    }];
    for line in manifest.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            tables.push(Table {
                lines: Vec::new(),
                example_name: None,
            });
        }
        let table = tables.last_mut().expect("there's always one");
        if table
            .lines
            .first()
            .is_some_and(|first| first.trim() == "[[example]]")
            && let Some(value) = line.trim().strip_prefix("name")
        {
            let value = value.trim_start().trim_start_matches('=').trim();
            table.example_name = Some(value.trim_matches('"').to_string());
        }
        table.lines.push(line);
    }
    tables
}

/// The `Cargo.toml` without the game's `[[example]]` table.
pub fn without_example(manifest: &str, name: &str) -> String {
    let mut out = String::new();
    for table in tables(manifest) {
        if table.example_name.as_deref() == Some(name) {
            // Its lines go, and the blank line that was before it.
            if out.ends_with("\n\n") || out.ends_with("\r\n\r\n") {
                out.truncate(out.trim_end_matches(['\r', '\n']).len());
                out.push_str(if manifest.contains("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                });
            }
            // The blank lines after it, which separated it from the next
            // table, stay with the table before.
            let blank: String = table
                .lines
                .iter()
                .rev()
                .take_while(|line| line.trim().is_empty())
                .copied()
                .collect();
            out.push_str(&blank);
            continue;
        }
        for line in table.lines {
            out.push_str(line);
        }
    }
    out
}

/// The `Cargo.toml` with the game's `[[example]]` table under its new name.
pub fn renamed_example(manifest: &str, old: &str, new: &str) -> String {
    let mut out = String::new();
    for table in tables(manifest) {
        for line in &table.lines {
            if table.example_name.as_deref() == Some(old) && line.trim().starts_with("name") {
                out.push_str(&line.replace(&format!("\"{old}\""), &format!("\"{new}\"")));
            } else {
                out.push_str(line);
            }
        }
    }
    out
}

/// The `Cargo.toml` with an `[[example]]` table for a new game added at
/// the end, as `tools/new_game` adds it.
pub fn with_example(manifest: &str, name: &str) -> String {
    if example_names(manifest).iter().any(|known| known == name) {
        return manifest.to_string();
    }
    let end = if manifest.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut out = manifest.to_string();
    if !out.ends_with('\n') {
        out.push_str(end);
    }
    out.push_str(&format!(
        "{end}[[example]]{end}name = \"{name}\"{end}test = true{end}"
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    const MANIFEST: &str = "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"dodge\"\ntest = true\n\n[[example]]\nname = \"bee_maze\"\ntest = true\n\n[profile.dev]\nopt-level = 1\n\n[[example]]\nname = \"bee_maze_two\"\ntest = true\n";

    /// A small Duck Forge folder with a game called bee_maze, made as
    /// tools/new_game makes one.
    fn folder_with_a_game(test: &TestFolder) -> GamesFolder {
        let write = |name: &str, text: &str| {
            let path = test.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("Cargo.toml", MANIFEST);
        write("examples/dodge.rs", "fn main() {}\n");
        write(
            "examples/bee_maze/main.rs",
            "const LEVEL: &str = \"examples/bee_maze/level.txt\";\n\
             // tools/verify.sh bee_maze; cargo run --example bee_maze -- --headless\n\
             let bee_maze = 1; // a variable that only looks like the name\n\
             title: String::from(\"Bee Maze - duckforge\"),\n\
             feel_presets(\"examples/bee_maze.feel.json\")\n\
             // not another game: examples/bee_maze_two/main.rs, --example bee_maze_two\n",
        );
        write(
            "examples/bee_maze/bot.rs",
            "sim.recording().save(\"playthroughs/bee_maze.json\").unwrap();\n",
        );
        write("examples/bee_maze/level.txt", "p..#\n");
        write("examples/bee_maze/assets/tiles.png", "picture");
        write("examples/bee_maze.tune.json", "{\"jump\": 3}\n");
        write("examples/bee_maze.feel.json", "{}\n");
        write("examples/bee_maze_two/main.rs", "fn main() {}\n");
        write("playthroughs/bee_maze.json", "{}\n");
        write("playthroughs/bee_maze-win.json", "{}\n");
        write("playthroughs/bee_maze_two.json", "{\"not\": \"ours\"}\n");
        write("history/bee_maze/1/level.txt", "p..#\n");
        write("out/verify/bee_maze/check.json", "{}\n");
        write("out/sessions/bee_maze/latest.json", "{}\n");
        GamesFolder::open(test.to_path_buf()).unwrap()
    }

    fn read(test: &TestFolder, name: &str) -> String {
        std::fs::read_to_string(test.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
    }

    #[test]
    fn deleting_moves_everything_to_the_recycle_bin_and_takes_out_its_lines() {
        let test = TestFolder::new("manage-delete");
        let folder = folder_with_a_game(&test);
        let bin = RecycleBin::Folder(test.join("Recycle Bin"));
        let moved = delete(&folder, "bee_maze", &bin).unwrap();
        assert_eq!(moved.len(), 8, "{moved:?}");
        for gone in [
            "examples/bee_maze",
            "examples/bee_maze.tune.json",
            "examples/bee_maze.feel.json",
            "playthroughs/bee_maze.json",
            "playthroughs/bee_maze-win.json",
            "history/bee_maze",
            "out/verify/bee_maze",
            "out/sessions/bee_maze",
        ] {
            assert!(!test.join(gone).exists(), "{gone}");
        }
        // In the bin, not deleted.
        assert_eq!(read(&test, "Recycle Bin/bee_maze/level.txt"), "p..#\n");
        assert!(test.join("Recycle Bin/bee_maze.tune.json").is_file());
        // Another game whose name starts the same is untouched.
        assert!(test.join("examples/bee_maze_two/main.rs").is_file());
        assert_eq!(
            read(&test, "playthroughs/bee_maze_two.json"),
            "{\"not\": \"ours\"}\n"
        );
        assert_eq!(
            read(&test, "Cargo.toml"),
            "[package]\nname = \"duckforge\"\n\n[[example]]\nname = \"dodge\"\ntest = true\n\n[profile.dev]\nopt-level = 1\n\n[[example]]\nname = \"bee_maze_two\"\ntest = true\n"
        );
        assert!(folder.game("bee_maze").is_none());
    }

    #[test]
    fn what_came_with_duck_forge_cant_be_deleted_or_renamed() {
        let test = TestFolder::new("manage-came-with");
        let folder = folder_with_a_game(&test);
        let bin = RecycleBin::Folder(test.join("bin"));
        let problem = delete(&folder, "dodge", &bin).unwrap_err();
        assert_eq!(
            problem.what,
            "Dodge came with Duck Forge, so it can't be deleted."
        );
        assert!(
            rename(&folder, "dodge", "Rocks")
                .unwrap_err()
                .what
                .contains("can't be renamed")
        );
        assert!(test.join("examples/dodge.rs").is_file());
        assert!(!test.join("bin").exists());
        assert!(
            delete(&folder, "nope", &bin)
                .unwrap_err()
                .what
                .contains("no game called nope")
        );
    }

    #[test]
    fn renaming_moves_everything_and_points_the_code_at_its_new_name() {
        let test = TestFolder::new("manage-rename");
        let folder = folder_with_a_game(&test);
        assert_eq!(rename(&folder, "bee_maze", "Bee Run!").unwrap(), "bee_run");
        let main = read(&test, "examples/bee_run/main.rs");
        assert!(main.contains("\"examples/bee_run/level.txt\""), "{main}");
        assert!(
            main.contains("cargo run --example bee_run -- --headless"),
            "{main}"
        );
        assert!(
            main.contains("let bee_maze = 1;"),
            "a variable is left alone: {main}"
        );
        assert!(main.contains("\"Bee Run - duckforge\""), "{main}");
        assert!(main.contains("\"examples/bee_run.feel.json\""), "{main}");
        assert!(
            main.contains("examples/bee_maze_two/main.rs, --example bee_maze_two"),
            "another game's names stay: {main}"
        );
        assert!(read(&test, "examples/bee_run/bot.rs").contains("\"playthroughs/bee_run.json\""));
        for moved in [
            "examples/bee_run/assets/tiles.png",
            "examples/bee_run.tune.json",
            "examples/bee_run.feel.json",
            "playthroughs/bee_run.json",
            "playthroughs/bee_run-win.json",
            "history/bee_run/1/level.txt",
            "out/verify/bee_run/check.json",
            "out/sessions/bee_run/latest.json",
        ] {
            assert!(test.join(moved).exists(), "{moved}");
        }
        assert!(!test.join("examples/bee_maze").exists());
        assert!(test.join("playthroughs/bee_maze_two.json").exists());
        let manifest = read(&test, "Cargo.toml");
        assert!(
            manifest.contains("name = \"bee_run\"\ntest = true"),
            "{manifest}"
        );
        assert!(!manifest.contains("\"bee_maze\""), "{manifest}");
        assert!(manifest.contains("\"bee_maze_two\""), "{manifest}");
        // A name that's taken is refused, and nothing changes.
        let problem = rename(&folder, "bee_run", "Bee Maze Two").unwrap_err();
        assert_eq!(problem.what, "There's already a game called Bee Maze Two.");
        assert!(test.join("examples/bee_run/main.rs").is_file());
    }

    #[test]
    fn duplicating_makes_a_copy_that_points_at_its_own_files() {
        let test = TestFolder::new("manage-duplicate");
        let folder = folder_with_a_game(&test);
        assert_eq!(
            duplicate(&folder, "bee_maze", "Bee Maze 2").unwrap(),
            "bee_maze_2"
        );
        // The original is as it was.
        assert!(
            read(&test, "examples/bee_maze/main.rs").contains("\"examples/bee_maze/level.txt\"")
        );
        let copy = read(&test, "examples/bee_maze_2/main.rs");
        assert!(copy.contains("\"examples/bee_maze_2/level.txt\""), "{copy}");
        assert!(copy.contains("\"Bee Maze 2 - duckforge\""), "{copy}");
        assert_eq!(read(&test, "examples/bee_maze_2/level.txt"), "p..#\n");
        assert_eq!(
            read(&test, "examples/bee_maze_2.tune.json"),
            "{\"jump\": 3}\n"
        );
        assert!(test.join("playthroughs/bee_maze_2-win.json").is_file());
        assert!(
            !test.join("history/bee_maze_2").exists(),
            "a copy starts its own history"
        );
        assert!(
            read(&test, "Cargo.toml")
                .ends_with("\n\n[[example]]\nname = \"bee_maze_2\"\ntest = true\n")
        );
        // An example that came with Duck Forge can be duplicated.
        assert_eq!(duplicate(&folder, "dodge", "My Dodge").unwrap(), "my_dodge");
        assert!(test.join("examples/my_dodge.rs").is_file());
        assert_eq!(folder.game("my_dodge").map(|g| g.came_with), Some(false));
    }

    #[test]
    fn manifest_edits_keep_everything_else() {
        let crlf = MANIFEST.replace('\n', "\r\n");
        let without = without_example(&crlf, "dodge");
        assert!(!without.contains("\"dodge\""));
        assert!(
            without.starts_with(
                "[package]\r\nname = \"duckforge\"\r\n\r\n[[example]]\r\nname = \"bee_maze\""
            ),
            "{without:?}"
        );
        assert!(
            !without.contains('\n')
                || without.matches('\n').count() == without.matches("\r\n").count()
        );
        assert_eq!(
            example_names(MANIFEST),
            ["dodge", "bee_maze", "bee_maze_two"]
        );
        assert_eq!(with_example(MANIFEST, "dodge"), MANIFEST, "already there");
        assert!(
            with_example(&crlf, "x")
                .ends_with("\r\n\r\n[[example]]\r\nname = \"x\"\r\ntest = true\r\n")
        );
        let renamed = renamed_example(MANIFEST, "bee_maze", "bee_run");
        assert!(
            renamed.contains("name = \"bee_run\"") && renamed.contains("name = \"bee_maze_two\"")
        );
        assert!(
            renamed.contains("[package]\nname = \"duckforge\""),
            "the package keeps its name"
        );
    }
}
