//! tools/verify replays every playthrough of a game: `<game>.json`,
//! `<game>-*.json` and `<game>_*.json` (a `duck_craft_night.json` went out
//! of date once because only the first two were replayed), but not files
//! that belong to another example with a longer name.

use std::fs;
use std::path::Path;

const SH: &str = include_str!("../tools/verify.sh");
const BAT: &str = include_str!("../tools/verify.bat");

/// The three kinds of file name, in both scripts.
#[test]
fn both_scripts_look_for_the_same_playthroughs() {
    for (name, text, patterns) in [
        (
            "verify.sh",
            SH,
            [
                r#""playthroughs/$game.json""#,
                r#"playthroughs/"$game"-*.json"#,
                r#"playthroughs/"$game"_*.json"#,
            ],
        ),
        (
            "verify.bat",
            BAT,
            [
                r#""playthroughs\%game%.json""#,
                r#""playthroughs\%game%-*.json""#,
                r#""playthroughs\%game%_*.json""#,
            ],
        ),
    ] {
        for pattern in patterns {
            assert!(text.contains(pattern), "{name} should look for {pattern}");
        }
        assert!(text.contains("--list-playthroughs"), "{name} can list them");
    }
}

/// Runs `verify.sh --list-playthroughs` in a small pretend project, so
/// nothing is built.
#[cfg(unix)]
#[test]
fn verify_sh_replays_every_playthrough_of_the_game_and_no_other() {
    let root = std::env::temp_dir().join(format!("duckforge-verify-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let touch = |path: &str| {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    };
    fs::create_dir_all(root.join("tools")).unwrap();
    fs::write(root.join("tools/verify.sh"), SH).unwrap();
    for file in [
        "examples/mini.rs",
        "examples/mini_craft/main.rs",
        "examples/duck_craft/main.rs",
        "playthroughs/duck_craft.json",
        "playthroughs/duck_craft-win.json",
        "playthroughs/duck_craft_night.json",
        "playthroughs/duckling.json",
        "playthroughs/mini.json",
        "playthroughs/mini-fast.json",
        "playthroughs/mini_dark.json",
        "playthroughs/mini_craft.json",
        "playthroughs/mini_craft-win.json",
    ] {
        touch(file);
    }
    let list = |game: &str| -> Vec<String> {
        let out = std::process::Command::new("bash")
            .arg(root.join("tools/verify.sh"))
            .args(["--list-playthroughs", game])
            .output()
            .expect("bash runs");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        let mut lines: Vec<String> = String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(String::from)
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(
        list("duck_craft"),
        [
            "playthroughs/duck_craft-win.json",
            "playthroughs/duck_craft.json",
            "playthroughs/duck_craft_night.json",
        ]
    );
    // mini_craft's files are mini_craft's, not mini's.
    assert_eq!(
        list("mini"),
        [
            "playthroughs/mini-fast.json",
            "playthroughs/mini.json",
            "playthroughs/mini_dark.json",
        ]
    );
    assert_eq!(
        list("mini_craft"),
        [
            "playthroughs/mini_craft-win.json",
            "playthroughs/mini_craft.json"
        ]
    );
    let _ = fs::remove_dir_all(&root);
    assert!(!Path::new(&root).exists());
}
