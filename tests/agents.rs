//! AGENTS.md and CLAUDE.md, at the top of the repository, tell agents what
//! to run. Everything they name must be there: every example `--example`
//! runs, every crate `-p` names and test `--test` names, every option a
//! game or the Studio is given, every starter `new_game` makes, and every
//! file and folder.
//!
//! The copy of Duck Forge that comes with the Studio has no `studio/` or
//! `.github/` folder, so what AGENTS.md names in those is only checked
//! where they are.

use std::path::{Path, PathBuf};

const AGENTS: &str = include_str!("../AGENTS.md");
const CLAUDE: &str = include_str!("../CLAUDE.md");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The name both files use for the reader's own game, which doesn't exist yet.
const YOUR_GAME: &str = "mygame";

/// Options that are Cargo's or the test runner's, not a game's.
const CARGO_FLAGS: &[&str] = &[
    "--all",
    "--all-targets",
    "--example",
    "--examples",
    "--ignored",
    "--no-default-features",
    "--offline",
    "--test",
];

/// Both files, with file names that have spaces in them taken out, and
/// those names. They are written in double quotes, as a command line
/// needs them: `"tools\Run tests (for developers).bat"`.
fn texts() -> (String, Vec<String>) {
    let mut words = String::new();
    let mut spaced = Vec::new();
    for text in [AGENTS, CLAUDE] {
        for (i, part) in text.split('"').enumerate() {
            let quoted = i % 2 == 1;
            let a_file = part.ends_with(".bat") || part.ends_with(".sh") || part.ends_with(".md");
            if quoted && a_file && part.contains(' ') {
                spaced.push(part.to_owned());
            } else {
                words.push_str(part);
                words.push(' ');
            }
        }
    }
    (words, spaced)
}

/// The words of the text, split at spaces and the marks around code.
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || "`(),;".contains(c))
        .map(|word| word.trim_end_matches(['.', ':']))
        .filter(|word| !word.is_empty())
        .collect()
}

/// The `[package]` name in a Cargo.toml.
fn package_name(cargo_toml: &Path) -> Option<String> {
    let text = std::fs::read_to_string(cargo_toml).ok()?;
    let line = text.lines().find(|line| line.starts_with("name = "))?;
    Some(line["name = ".len()..].trim_matches('"').to_owned())
}

/// Whether this copy has the Studio's code (`studio/`).
fn studio_is_here() -> bool {
    Path::new(ROOT).join("studio/Cargo.toml").is_file()
}

/// The Studio's crates: `studio/crates/<folder>`.
fn studio_crates() -> Vec<PathBuf> {
    match std::fs::read_dir(Path::new(ROOT).join("studio/crates")) {
        Ok(entries) => entries.map(|entry| entry.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn claude_md_brings_in_agents_md_and_agents_md_stays_short() {
    assert!(
        CLAUDE.starts_with("@AGENTS.md"),
        "CLAUDE.md starts with @AGENTS.md, so Claude Code reads the same text"
    );
    let lines = AGENTS.lines().count();
    assert!(
        (60..=110).contains(&lines),
        "AGENTS.md is {lines} lines; keep it between 60 and 110"
    );
}

#[test]
fn every_example_crate_and_test_named_is_there() {
    let (text, _) = texts();
    let words = words(&text);
    let cargo_toml = std::fs::read_to_string(Path::new(ROOT).join("Cargo.toml")).unwrap();
    let (mut examples, mut crates, mut tests) = (0, 0, 0);
    for pair in words.windows(2) {
        let name = pair[1];
        match pair[0] {
            "--example" if name != YOUR_GAME => {
                examples += 1;
                let examples = Path::new(ROOT).join("examples");
                assert!(
                    examples.join(format!("{name}.rs")).is_file()
                        || examples.join(name).join("main.rs").is_file()
                        || cargo_toml.contains(&format!("name = \"{name}\"")),
                    "AGENTS.md runs --example {name}, which isn't an example"
                );
            }
            "-p" => {
                crates += 1;
                let found = package_name(&Path::new(ROOT).join("Cargo.toml")).as_deref()
                    == Some(name)
                    || studio_crates().iter().any(|krate| {
                        package_name(&krate.join("Cargo.toml")).as_deref() == Some(name)
                    });
                assert!(
                    found || !studio_is_here(),
                    "AGENTS.md names the crate {name}, which isn't one"
                );
            }
            "--test" => {
                tests += 1;
                let file = format!("{name}.rs");
                let found = Path::new(ROOT).join("tests").join(&file).is_file()
                    || studio_crates()
                        .iter()
                        .any(|krate| krate.join("tests").join(&file).is_file());
                assert!(found, "AGENTS.md names the test {name}, which isn't one");
            }
            _ => {}
        }
    }
    assert!(examples >= 2, "found the examples ({examples})");
    assert!(crates >= 2, "found the crates ({crates})");
    assert!(tests >= 2, "found the tests ({tests})");
}

#[test]
fn every_option_named_is_one_a_game_the_studio_or_cargo_has() {
    // A game's options are the `name: "--..."` lines in src/cli.rs; the
    // Studio's are the `Some("--...")` lines in its main.rs.
    let engine = include_str!("../src/cli.rs");
    let studio = std::fs::read_to_string(Path::new(ROOT).join("studio/crates/app/src/main.rs"));
    let game_options: Vec<&str> = engine
        .split("name: \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|name| name.starts_with("--"))
        .collect();
    assert!(game_options.len() >= 20, "found the game options");
    assert!(game_options.contains(&"--headless"));
    let (text, _) = texts();
    let mut checked = 0;
    for word in words(&text) {
        let Some(name) = word.strip_prefix("--") else {
            continue;
        };
        if !name.starts_with(|c: char| c.is_ascii_lowercase()) {
            continue; // `--` itself, before a game's options
        }
        let option = word.split('=').next().unwrap();
        checked += 1;
        let the_studios = match &studio {
            Ok(studio) => studio.contains(&format!("Some(\"{option}\")")),
            Err(_) => true, // no Studio here to ask
        };
        assert!(
            game_options.contains(&option) || CARGO_FLAGS.contains(&option) || the_studios,
            "AGENTS.md names {option}, which no game, Studio or Cargo command has"
        );
    }
    assert!(checked >= 10, "found the options ({checked})");
}

#[test]
fn every_starter_named_is_one_new_game_makes() {
    // The usage line of each script: `<name> <empty|platformer|...>`.
    let usage = |name: &str, script: &str| -> String {
        let line = script
            .lines()
            .find(|line| line.contains("usage:"))
            .unwrap_or_else(|| panic!("{name} has a usage line"));
        line.replace('^', "").replace(['<', '>'], "|")
    };
    let sh = usage("new_game.sh", include_str!("../tools/new_game.sh"));
    let bat = usage("new_game.bat", include_str!("../tools/new_game.bat"));
    let (text, _) = texts();
    let words = words(&text);
    let mut runs = 0;
    for three in words.windows(3) {
        if three[0].ends_with("new_game.sh") || three[0].ends_with("new_game.bat") {
            runs += 1;
            for (name, usage) in [("new_game.sh", &sh), ("new_game.bat", &bat)] {
                assert!(
                    usage.contains(&format!("|{}|", three[2])),
                    "AGENTS.md makes a game from the starter {}, which {name} doesn't have",
                    three[2]
                );
            }
        }
    }
    assert!(runs >= 2, "found new_game ({runs})");
    // And the list of starters names every one there is: the last
    // `|...|` group of the usage line.
    let starters = sh.trim_end_matches('"').trim_end_matches('|');
    let starters = starters.rsplit("| |").next().unwrap();
    for starter in starters.split('|') {
        assert!(
            AGENTS.contains(&format!("`{starter}`")),
            "AGENTS.md should list the starter {starter}"
        );
    }
}

#[test]
fn every_file_and_folder_named_is_there() {
    let (text, spaced) = texts();
    let folders = [
        "src/",
        "docs/",
        "tests/",
        "tools/",
        "examples/",
        "skills/",
        "studio/",
        "playthroughs/",
        ".github/",
    ];
    // Not in the copy that comes with the Studio.
    let only_here = ["studio/", ".github/"];
    let mut checked = 0;
    let named = words(&text)
        .into_iter()
        .map(|word| word.replace('\\', "/"))
        .filter(|word| {
            folders.iter().any(|folder| word.starts_with(folder))
                || word.ends_with(".md")
                || word == "Cargo.toml"
                || word == ".gitattributes"
        })
        // `@AGENTS.md` is CLAUDE.md bringing in AGENTS.md.
        .filter(|word| !word.starts_with('@'))
        .filter(|word| !word.contains(YOUR_GAME) && !word.contains(['<', '*']))
        .chain(spaced.iter().map(|name| name.replace('\\', "/")));
    for path in named {
        let missing_folder = only_here
            .iter()
            .any(|folder| path.starts_with(folder) && !Path::new(ROOT).join(folder).is_dir());
        if missing_folder {
            continue;
        }
        checked += 1;
        assert!(
            Path::new(ROOT).join(&path).exists(),
            "AGENTS.md names {path}, which isn't there"
        );
    }
    assert!(checked >= 15, "found the paths ({checked})");
}
