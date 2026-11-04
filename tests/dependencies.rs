//! Keeps the headless build small and quick to compile.
//!
//! Every crate the headless build (`--no-default-features`) compiles is
//! listed here. A new one, even one pulled in by updating `png` or `gif`,
//! makes this test fail, so someone looks at what it costs before it
//! slows down every clean build (and every new cloud machine). See
//! docs/decisions.md, "Step 9: Build speed".

use std::collections::BTreeSet;
use std::process::Command;

/// The crates a headless build compiles today: duckforge, its three
/// dependencies (png, gif and serde_json), and theirs.
const HEADLESS_CRATES: &[&str] = &[
    "adler2",
    "bitflags",
    "cfg-if",
    "color_quant",
    "crc32fast",
    "duckforge",
    "fdeflate",
    "flate2",
    "gif",
    "itoa",
    "memchr",
    "miniz_oxide",
    "png",
    "serde_core",
    "serde_json",
    "simd-adler32",
    "weezl",
    "zmij",
];

#[test]
fn the_headless_build_gets_no_new_dependencies() {
    // `cargo test` tells the test which cargo is running it.
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| String::from("cargo"));
    let output = Command::new(cargo)
        .args([
            "tree",
            "--offline",
            "--no-default-features",
            "--edges",
            "normal",
            "--prefix",
            "none",
            "--format",
            "{p}",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    let output = match output {
        Ok(output) if output.status.success() => output,
        // Not a problem with the engine: say so, but don't fail.
        other => {
            eprintln!("skipped: couldn't run `cargo tree`: {other:?}");
            return;
        }
    };
    let crates: BTreeSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .map(String::from)
        .collect();
    let allowed: BTreeSet<String> = HEADLESS_CRATES.iter().map(|c| c.to_string()).collect();
    let new: Vec<&String> = crates.difference(&allowed).collect();
    assert!(
        new.is_empty(),
        "the headless build now also compiles {new:?}. Each new crate makes every clean \
         build slower. Measure a clean `cargo build --no-default-features --example dodge` \
         before and after (docs/decisions.md, Step 9, has the numbers), and if it's worth \
         it, add the crates to HEADLESS_CRATES in tests/dependencies.rs."
    );
    let gone: Vec<&String> = allowed.difference(&crates).collect();
    assert!(
        gone.is_empty(),
        "{gone:?} are no longer compiled: remove them from HEADLESS_CRATES"
    );
}
