//! Keeps skills/duckforge/SKILL.md true: short, with its frontmatter, and
//! naming only files, examples, commands and functions that exist. (Its
//! flags are checked against the engine's in src/cli.rs.)

use std::path::Path;

const SKILL: &str = include_str!("../skills/duckforge/SKILL.md");
const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Names the skill uses for the reader's own game, which don't exist yet.
const YOUR_GAME: &[&str] = &["mygame", "x"];

#[test]
fn it_has_frontmatter_and_stays_short() {
    let lines: Vec<&str> = SKILL.lines().collect();
    assert_eq!(lines[0], "---");
    assert_eq!(lines[1], "name: duckforge");
    let description = lines[2]
        .strip_prefix("description: ")
        .expect("a description");
    assert!(description.starts_with("Use when"), "say when to use it");
    assert!(description.len() < 200, "one line");
    assert_eq!(lines[3], "---");
    assert!(
        lines.len() <= 150,
        "SKILL.md is {} lines; keep it under 150",
        lines.len()
    );
}

#[test]
fn its_links_and_files_exist() {
    let here = Path::new(ROOT).join("skills/duckforge");
    let links: Vec<&str> = SKILL
        .split("](")
        .skip(1)
        .map(|rest| rest.split(')').next().unwrap())
        .collect();
    assert!(links.iter().any(|l| l.ends_with("QUICKSTART.md")));
    assert!(links.iter().any(|l| l.ends_with("AI_GUIDE.md")));
    for link in links {
        assert!(
            here.join(link).exists(),
            "SKILL.md links to {link}, which doesn't exist"
        );
    }
    // Every path it names, like `examples/starter_shooter/` or tools/verify.sh.
    let mut checked = 0;
    for word in SKILL.split(|c: char| c.is_whitespace() || "`(),:".contains(c)) {
        let word = word.trim_end_matches('.').replace('\\', "/");
        let named = ["examples/", "tools/", "playthroughs/", "docs/"]
            .iter()
            .any(|dir| word.starts_with(dir));
        let yours = YOUR_GAME
            .iter()
            .any(|game| word.contains(&format!("/{game}")));
        if named && !yours && !word.contains('*') {
            assert!(
                Path::new(ROOT).join(&word).exists(),
                "SKILL.md names {word}, which doesn't exist"
            );
            checked += 1;
        }
    }
    assert!(checked >= 8, "found the paths ({checked})");
}

#[test]
fn its_commands_exist() {
    let words: Vec<&str> = SKILL.split_whitespace().collect();
    for pair in words.windows(2) {
        if pair[0] == "--example" {
            let name = pair[1].trim_matches('`');
            if !YOUR_GAME.contains(&name) {
                let file = Path::new(ROOT).join(format!("examples/{name}.rs"));
                assert!(
                    file.exists(),
                    "SKILL.md runs --example {name}, which doesn't exist"
                );
            }
        }
        if pair[0] == "cargo" || pair[0] == "`cargo" {
            let sub = pair[1].trim_matches(|c: char| c == '`' || c.is_ascii_punctuation());
            assert!(
                ["build", "run", "test"].contains(&sub),
                "SKILL.md uses `cargo {sub}`: only build, run and test are needed"
            );
        }
    }
    for script in ["tools/verify.sh", "tools\\verify.bat"] {
        assert!(SKILL.contains(script), "SKILL.md should show {script}");
    }
}

#[test]
fn every_function_it_calls_is_in_the_cheat_sheet_test() {
    // tests/cheat_sheet.rs calls every function in the guide's cheat sheet
    // and fails if one stops existing, so anything called there is real.
    let cheat_sheet = include_str!("cheat_sheet.rs");
    // Calls that belong to Rust, to the example code shown, or to the reader's game.
    let not_engine = ["config", "center", "contains"];
    let mut calls = Vec::new();
    for (i, span) in SKILL.split('`').enumerate() {
        if i % 2 == 0 {
            continue; // outside backticks
        }
        let mut start = None;
        for (j, b) in span.bytes().enumerate() {
            let word = b.is_ascii_alphanumeric() || b == b'_';
            match (word, start) {
                (true, None) => start = Some(j),
                (false, Some(s)) => {
                    if b == b'(' {
                        calls.push(&span[s..j]);
                    }
                    start = None;
                }
                _ => {}
            }
        }
    }
    assert!(calls.len() >= 10, "found the calls: {calls:?}");
    for call in calls {
        if not_engine.contains(&call) {
            continue;
        }
        assert!(
            cheat_sheet.contains(&format!("{call}(")),
            "SKILL.md calls {call}(), which tests/cheat_sheet.rs never does: check the name"
        );
    }
}

/// The two things Duck Craft's maker wished the skill had said.
#[test]
fn it_says_how_to_save_a_picture_and_that_cargo_test_doesnt_rebuild_the_game() {
    assert!(SKILL.contains("save_png"), "Canvas::save_png, for tests");
    assert!(
        SKILL.contains("`cargo test` doesn't rebuild the game's own program"),
        "the old-binary trap"
    );
    assert!(SKILL.contains("tools/verify.sh"));
}
