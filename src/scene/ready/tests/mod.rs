//! Tests of the ready-made kinds: what every one must be, and one file of
//! tests for each (`platformer_player.rs` for `platformer_player`).

mod platformer_player;

use super::*;

#[test]
fn every_ready_made_kind_is_listed_once_with_its_file() {
    let scripts = scripts();
    assert_eq!(
        scripts.names(),
        ALL.iter().map(|ready| ready.name).collect::<Vec<_>>()
    );
    for ready in ALL {
        let text = source(ready.name).expect("each has its file");
        assert!(
            text.contains(&format!("pub struct {} {{", ready.type_name)),
            "{}",
            ready.name
        );
        assert!(
            text.contains("use duckforge::prelude::*;"),
            "{} uses only what any game can",
            ready.name
        );
        assert!(!text.contains("crate::"), "{}", ready.name);
        assert_eq!(get(ready.name).map(|r| r.label), Some(ready.label));
    }
}

#[test]
fn a_games_copy_has_its_own_names_and_the_same_code() {
    for ready in ALL {
        let copy = copy_for_a_game(&ready, "bird", "Bird").unwrap();
        assert!(
            copy.starts_with(&format!("//! bird: {}.\n", ready.about)),
            "{copy}"
        );
        assert!(copy.contains(&format!("ready-made \"{}\"", ready.label)));
        assert!(copy.contains("pub struct Bird {"));
        assert!(copy.contains("impl Script for Bird {"));
        assert!(!copy.contains(ready.type_name));
        // Everything from the first line of code on is the same, but for
        // the type's name.
        let original = source(ready.name).unwrap().replace("\r\n", "\n");
        assert!(!copy.contains('\r'), "one kind of line ending");
        assert!(!original.contains("Bird"), "the test's name is free");
        let code = |text: &str| text[text.find("\nuse ").unwrap()..].to_string();
        assert_eq!(
            code(&copy).replace("Bird", ready.type_name),
            code(&original)
        );
        // And its first line says what the list says it does.
        assert!(original.starts_with(&format!("//! {}: {}.", ready.label, ready.about)));
    }
    assert_eq!(replace_word("A AB A_ BA A", "A", "Z"), "Z AB A_ BA Z");
}
