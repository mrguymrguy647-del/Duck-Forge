//! Keeps docs/QUICKSTART.md true: its game and its play test must be
//! exactly `examples/quickstart.rs`, which Cargo compiles and tests.

#[test]
fn the_quick_start_game_is_the_quickstart_example() {
    let page = include_str!("../docs/QUICKSTART.md");
    let example = include_str!("../examples/quickstart.rs");
    let blocks: Vec<&str> = page
        .split("```rust\n")
        .skip(1)
        .map(|rest| rest.split("```").next().unwrap())
        .collect();
    assert_eq!(blocks.len(), 2, "the page has the game and its play test");
    assert_eq!(
        format!("{}\n{}", blocks[0], blocks[1]),
        example,
        "docs/QUICKSTART.md and examples/quickstart.rs differ: copy the example into the page"
    );
}

#[test]
fn the_quick_start_stays_short() {
    let lines = include_str!("../docs/QUICKSTART.md").lines().count();
    assert!(
        lines <= 125,
        "QUICKSTART.md is {lines} lines; keep it to one page (about 120)"
    );
}
