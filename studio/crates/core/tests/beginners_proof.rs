//! The beginner's proof: `studio/proof/beginners-proof.json`, run with no
//! window, using only the Studio's commands.
//!
//! It makes a new platformer; paints a gap, planks and coins; freezes in
//! mid-jump over the gap, moves a plank under the duck and carries on onto
//! it; sets Feel to "Fast and bouncy" while playing; checks the game,
//! fixes what Check finds (a coin the bot can't get to, then a playthrough
//! from the older level), makes a clip; and plays Duck Craft with mouse
//! look (the mouse alone, then walking). The script says what Check must say at each point. This test
//! also replays the saved session of the jump, to see that the duck
//! landed on the plank rather than in the gap.
//!
//! The same file runs as `DuckForgeStudio --run studio/proof/beginners-proof.json`
//! (the release workflow does that on Windows, from the unzipped folder),
//! and the app's screenshot test follows it, taking a picture of the
//! Studio at each `"shot"`.

use std::path::{Path, PathBuf};

use studio_core::script::{Step, run_step};
use studio_core::settings::Settings;
use studio_core::test_folder::TestFolder;
use studio_core::{Command, Studio};

fn duck_forge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

/// Takes the game the proof made out of the folder again.
struct PutBack {
    root: PathBuf,
    manifest: String,
}

impl Drop for PutBack {
    fn drop(&mut self) {
        let _ = std::fs::write(self.root.join("Cargo.toml"), &self.manifest);
        let _ = std::fs::remove_dir_all(self.root.join("examples/gap_dash"));
        let _ = std::fs::remove_file(self.root.join("playthroughs/gap_dash.json"));
        for folder in ["out/verify", "out/sessions", "history"] {
            let _ = std::fs::remove_dir_all(self.root.join(folder).join("gap_dash"));
        }
        for end in ["feel.json", "tune.json"] {
            let _ = std::fs::remove_file(self.root.join(format!("examples/gap_dash.{end}")));
        }
    }
}

#[test]
fn a_beginner_can_make_paint_tune_check_and_clip_a_game() {
    let root = duck_forge();
    let _put_back = PutBack {
        manifest: std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
        root: root.clone(),
    };
    // Default settings are never saved: the person's own are never touched.
    let mut studio = Studio::new(Settings {
        games_folder: Some(root.clone()),
        ..Settings::default()
    });
    assert!(studio.folder.is_some(), "{:?}", studio.folder_problem);

    let script = std::fs::read_to_string(root.join("studio/proof/beginners-proof.json")).unwrap();
    let steps: Vec<Step> = serde_json::from_str(&script).unwrap();
    let mut jump_session = None;
    let mut craft = Vec::new();
    for (number, step) in steps.iter().enumerate() {
        let said = run_step(&mut studio, step).unwrap_or_else(|problem| {
            panic!(
                "step {} ({step:?}): {problem}\n{}",
                number + 1,
                problem.details
            )
        });
        println!("{:>3} {said}", number + 1);
        // (A game being made isn't open yet.)
        let Some(open) = studio.open.as_ref() else {
            continue;
        };
        // The session saved by the first "game.stop" is the one with the
        // jump onto the plank.
        if jump_session.is_none() && matches!(step, Step::Do(Command::GameStop)) {
            jump_session = open.last_session.clone();
        }
        // Duck Craft's picture at its two shots: before and after the
        // mouse turned the view.
        if matches!(step, Step::Shot(_)) && open.game.name == "duck_craft" {
            craft.push(studio.play().unwrap().picture.clone().unwrap().pixels);
        }
    }

    // The duck landed on the plank that was moved under it, with the coin
    // it took in the air, and ran on past the gap (which ends at x = 224):
    // replayed with no window, it never splashed.
    let replays = TestFolder::new("proof-replay");
    let out = replays.join("out");
    let replay = std::process::Command::new("cargo")
        .args([
            "run",
            "-q",
            "--no-default-features",
            "--example",
            "gap_dash",
            "--",
        ])
        .args(["--headless", "--inputs"])
        .arg(jump_session.expect("the session with the jump was saved"))
        .arg("--out")
        .arg(&out)
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        replay.status.success(),
        "{}{}",
        String::from_utf8_lossy(&replay.stdout),
        String::from_utf8_lossy(&replay.stderr)
    );
    let summary = std::fs::read_to_string(out.join("summary.txt")).unwrap();
    let last = summary
        .lines()
        .find(|line| line.starts_with("final: "))
        .unwrap_or_else(|| panic!("{summary}"));
    assert!(last.contains("splashes = 0"), "{last}");
    assert!(last.contains("coins = 2"), "{last}");
    assert!(last.contains("on_ground = true"), "{last}");
    let x: f32 = last
        .rsplit("x = ")
        .next()
        .and_then(|x| x.trim().parse().ok())
        .unwrap_or_else(|| panic!("{last}"));
    assert!(x > 224.0, "past the gap: {last}");

    // A clip was made.
    let clip = std::fs::read_dir(root.join("out/clips"))
        .unwrap()
        .flatten()
        .any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.starts_with("gap_dash-") && (name.ends_with(".mp4") || name.ends_with(".gif"))
        });
    assert!(clip, "a clip of Gap Dash in out/clips");
    for entry in std::fs::read_dir(root.join("out/clips")).unwrap().flatten() {
        if entry.file_name().to_string_lossy().starts_with("gap_dash-") {
            let path = entry.path();
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_dir_all(&path);
        }
    }

    // Duck Craft: with nothing touched most of the picture stays; the
    // mouse alone then turns the view, which changes most of it.
    assert_eq!(craft.len(), 4);
    let differ = |a: &[u32], b: &[u32]| a.iter().zip(b).filter(|(a, b)| a != b).count();
    let all = craft[0].len();
    let (idle, turned) = (differ(&craft[0], &craft[1]), differ(&craft[1], &craft[2]));
    println!(
        "Duck Craft: {idle} of {all} pixels changed with nothing touched, {turned} with the mouse"
    );
    // (Water, clouds and the day's title move by themselves.)
    assert!(
        idle < all / 4,
        "left alone, most of the picture should stay: {idle} of {all}"
    );
    assert!(
        turned > all / 2 && turned > idle * 3,
        "mouse look should change most of the picture: {turned} of {all} (idle: {idle})"
    );
}
