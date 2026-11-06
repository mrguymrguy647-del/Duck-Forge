//! Checks tools/new_game.sh: that a game made from each starter is renamed
//! properly, and (the slow, ignored test) that it builds and passes verify.
//! Both work in a temporary copy of the project, so nothing here changes.
//!
//!     cargo test --test new_game                  the quick checks
//!     cargo test --test new_game -- --ignored     also build and verify each game
//!
//! They need `bash`, so on Windows they say so and pass; tools/new_game.bat
//! is checked by hand there (see the README).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// (kind, starter, the starter's type, a folder starter?)
const STARTERS: [(&str, &str, &str, bool); 4] = [
    ("platformer", "template", "Template", false),
    ("topdown", "starter_topdown", "TopDown", true),
    ("shooter", "starter_shooter", "Shooter", true),
    ("tiny", "quickstart", "Catch", false),
];

fn have_bash() -> bool {
    Command::new("bash")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            fs::copy(&path, &target).unwrap();
        }
    }
}

/// A copy of what building and verifying a game needs (no `target/`).
fn project_copy(name: &str) -> PathBuf {
    let copy =
        std::env::temp_dir().join(format!("duckforge-new-game-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&copy);
    fs::create_dir_all(&copy).unwrap();
    for file in ["Cargo.toml", "Cargo.lock"] {
        fs::copy(Path::new(ROOT).join(file), copy.join(file)).unwrap();
    }
    for dir in ["src", "examples", "tools", "playthroughs"] {
        copy_dir(&Path::new(ROOT).join(dir), &copy.join(dir));
    }
    copy
}

fn new_game(project: &Path, name: &str, kind: &str, verify: bool) -> std::process::Output {
    let mut command = Command::new("bash");
    command
        .arg("tools/new_game.sh")
        .args([name, kind])
        .current_dir(project);
    if verify {
        // Its own target folder inside the copy, so it never waits for, or
        // disturbs, the build that's running this test.
        command.env("CARGO_TARGET_DIR", project.join("target"));
    } else {
        command.env("NEW_GAME_NO_VERIFY", "1");
    }
    command.output().unwrap()
}

#[test]
fn each_starter_becomes_a_renamed_game() {
    if !have_bash() {
        eprintln!("skipped: no bash here (on Windows, try tools\\new_game.bat by hand)");
        return;
    }
    let project = project_copy("quick");
    for (kind, starter, old_type, folder) in STARTERS {
        let name = format!("my_{kind}");
        let output = new_game(&project, &name, kind, false);
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{kind}: {said}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let files: Vec<PathBuf> = if folder {
            ["main.rs", "tests.rs", "bot.rs"]
                .iter()
                .map(|f| project.join("examples").join(&name).join(f))
                .collect()
        } else {
            vec![project.join("examples").join(format!("{name}.rs"))]
        };
        let new_type = format!("My{}{}", kind[..1].to_uppercase(), &kind[1..]);
        let text: String = files
            .iter()
            .map(|f| fs::read_to_string(f).unwrap())
            .collect();
        let word = |w: &str| {
            text.match_indices(w).any(|(i, _)| {
                let before = text[..i]
                    .chars()
                    .last()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                let after = text[i + w.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric() && c != '_');
                before && after
            })
        };
        assert!(!word(old_type), "{kind}: {old_type} is still there");
        assert!(!word(starter), "{kind}: {starter} is still there");
        assert!(
            text.contains(&format!("struct {new_type}")),
            "{kind}: no struct {new_type}"
        );
        if kind != "tiny" {
            assert!(
                text.contains(&format!("\"My {kind} - duckforge\"")),
                "{kind}: title"
            );
            // Not the starter's tune file: it would quietly override the
            // values written in the new game's code.
            assert!(
                !project.join(format!("examples/{name}.tune.json")).exists(),
                "{kind}: no tune file"
            );
        }
        if folder {
            assert!(
                text.contains(&format!("playthroughs/{name}.json")),
                "{kind}: bot path"
            );
        }
        let cargo = fs::read_to_string(project.join("Cargo.toml")).unwrap();
        assert!(cargo.contains(&format!("[[example]]\nname = \"{name}\"\ntest = true\n")));
        // The starter itself is untouched.
        let original = if folder {
            format!("examples/{starter}/main.rs")
        } else {
            format!("examples/{starter}.rs")
        };
        assert_eq!(
            fs::read_to_string(project.join(&original)).unwrap(),
            fs::read_to_string(Path::new(ROOT).join(&original)).unwrap()
        );
    }
    // A second game with the same name is refused, and so is a bad name.
    assert!(
        !new_game(&project, "my_topdown", "topdown", false)
            .status
            .success()
    );
    assert!(
        !new_game(&project, "Bad-Name", "topdown", false)
            .status
            .success()
    );
    assert!(!new_game(&project, "fine", "racing", false).status.success());
    let _ = fs::remove_dir_all(project);
}

#[test]
#[ignore] // builds and verifies four games: about a minute
fn each_starter_becomes_a_game_that_passes_verify() {
    if !have_bash() {
        eprintln!("skipped: no bash here (on Windows, try tools\\new_game.bat by hand)");
        return;
    }
    let project = project_copy("verify");
    for (kind, ..) in STARTERS {
        let name = format!("fresh_{kind}");
        let output = new_game(&project, &name, kind, true);
        let said = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{kind}:\n{said}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            said.contains(&format!("VERIFY OK: {name}")),
            "{kind}:\n{said}"
        );
    }
    let _ = fs::remove_dir_all(project);
}
