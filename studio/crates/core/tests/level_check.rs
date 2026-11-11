//! Check's own lines about a level, on the platformer starter's real
//! level, changed only in memory (the file is never written).

use std::path::{Path, PathBuf};

use duckforge::prelude::Jump;
use studio_core::check::{RowAction, RowStatus, level_rows};
use studio_core::folder::GamesFolder;
use studio_core::level::Level;

fn duck_forge() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn the_level_says_its_broken_rules_far_coins_and_old_playthroughs() {
    let root = duck_forge();
    let folder = GamesFolder::open(root.clone()).unwrap();
    let file = root.join("examples/template/level.txt");
    let on_disk = std::fs::read_to_string(&file).unwrap();
    let mut level = Level::load(&root, &file).unwrap();
    let jump = Jump {
        speed: 330.0,
        gravity: 900.0,
        run: 110.0,
        width: 10.0,
    };

    // As it is: nothing to say. (Its playthrough was recorded on it.)
    let rows = level_rows(&folder, "template", &level, Some(jump));
    assert!(rows.is_empty(), "{rows:#?}");

    // A coin high in the sky, and no flag.
    assert!(level.place_thing((5, 0), 'c'));
    let flag = level.cells_with('g')[0];
    assert!(level.set(flag, '.'));
    let rows = level_rows(&folder, "template", &level, Some(jump));
    let said: Vec<(&str, RowStatus)> = rows.iter().map(|r| (r.say.as_str(), r.status)).collect();
    assert!(
        said.contains(&(
            "The level needs at least 1 Flag; it has none.",
            RowStatus::Failed
        )),
        "{said:?}"
    );
    assert!(
        said.contains(&(
            "A coin looks out of reach (column 5, row 0)",
            RowStatus::Warning
        )),
        "{said:?}"
    );
    let far = rows.iter().find(|r| r.step == "level reach").unwrap();
    assert!(
        far.picture.as_ref().is_some_and(|p| p.is_file()),
        "a picture of the spot"
    );
    let old = rows
        .iter()
        .find(|r| r.action == Some(RowAction::RecordAgain))
        .unwrap();
    assert_eq!(old.status, RowStatus::Warning);
    assert!(
        old.say.contains("\"template\" is from an older level"),
        "{}",
        old.say
    );

    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        on_disk,
        "never written"
    );
    let _ = std::fs::remove_file(far.picture.as_ref().unwrap());
}
