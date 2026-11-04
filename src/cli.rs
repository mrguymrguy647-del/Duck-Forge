//! The command-line options every duckforge game understands.
//!
//! `run` reads them, so a game gets `--headless`, `--frames` and the rest
//! without writing any code. Options the engine doesn't know are left
//! alone, so a game can have its own too (see [`game_args`]).

use std::path::PathBuf;

use crate::engine::Corner;
use crate::pictures::Crop;

/// One option: its name, the name of its value (`None` for an on/off
/// switch), and what it does.
struct OptionInfo {
    name: &'static str,
    value: Option<&'static str>,
    help: &'static str,
}

const OPTIONS: &[OptionInfo] = &[
    OptionInfo {
        name: "--headless",
        value: None,
        help: "run without a window, drawing with the CPU, and save what was drawn",
    },
    OptionInfo {
        name: "--frames",
        value: Some("N"),
        help: "stop after N updates (with --headless the default is 600, 10 seconds)",
    },
    OptionInfo {
        name: "--out",
        value: Some("DIR"),
        help: "the folder --headless saves its files in (default: duckforge-out); audio.wav is saved there if the game played sounds",
    },
    OptionInfo {
        name: "--snapshot-every",
        value: Some("N"),
        help: "with --headless, save every Nth frame as frame-000060.png and so on",
    },
    OptionInfo {
        name: "--sheet",
        value: None,
        help: "with --headless, save sheet.png: many frames in a grid, with frame numbers",
    },
    OptionInfo {
        name: "--sheet-events",
        value: None,
        help: "make sheet.png from the frames where reported values changed (state, lives, score...), each captioned",
    },
    OptionInfo {
        name: "--gif",
        value: None,
        help: "with --headless, save run.gif: the whole run as a small animated GIF",
    },
    OptionInfo {
        name: "--crop",
        value: Some("X,Y,W,H"),
        help: "with --headless, also save an enlarged close-up of this area (closeup.png)",
    },
    OptionInfo {
        name: "--zoom",
        value: Some("N"),
        help: "how many times bigger close-ups are (default 4 with --crop)",
    },
    OptionInfo {
        name: "--inputs",
        value: Some("FILE"),
        help: "replay an input script (a recording) in a window or with --headless",
    },
    OptionInfo {
        name: "--accept",
        value: None,
        help: "with --headless --inputs FILE: write this run's final reported values into FILE's \"expect\" block",
    },
    OptionInfo {
        name: "--record",
        value: Some("FILE"),
        help: "record everything you do in the window to FILE (F10 also records)",
    },
    OptionInfo {
        name: "--seed",
        value: Some("N"),
        help: "the seed for random numbers, to make a run repeatable",
    },
    OptionInfo {
        name: "--debug",
        value: None,
        help: "show the debug overlay (F1 toggles it in a window); with --headless it's in every picture",
    },
    OptionInfo {
        name: "--debug-corner",
        value: Some("CORNER"),
        help: "where the debug overlay's box goes: top-left, top-right, bottom-left or bottom-right (default)",
    },
    OptionInfo {
        name: "--grid",
        value: Some("N"),
        help: "draw a light grid every N pixels, with coordinates, on last.png, snapshots and close-ups",
    },
    OptionInfo {
        name: "--grid-world",
        value: Some("N"),
        help: "like --grid, but every N world units through the game's camera, numbered in world coordinates",
    },
    OptionInfo {
        name: "--diff",
        value: Some("OLD_DIR NEW_DIR"),
        help: "compare two headless runs' pictures, scenes and reports (the game isn't run)",
    },
    OptionInfo {
        name: "--fuzz",
        value: Some("N"),
        help: "play N headless runs (seeds 1 to N) with random keys; report crashes, failed checks and freezes",
    },
    OptionInfo {
        name: "--fuzz-keys",
        value: Some("KEYS"),
        help: "the keys --fuzz presses, like Left,Right,Up,Space (default: arrows, Space, Z, X)",
    },
    OptionInfo {
        name: "--freeze-seconds",
        value: Some("S"),
        help: "--fuzz calls it a freeze when no reported value changes for S seconds while keys are pressed (default 10)",
    },
    OptionInfo {
        name: "--describe",
        value: None,
        help: "after a headless run, describe the last frame in words: every canvas.label, and the text drawn",
    },
    OptionInfo {
        name: "--quiet",
        value: None,
        help: "after a headless run, print only problems (failed checks, warnings); summary.txt still has everything",
    },
    OptionInfo {
        name: "--json",
        value: None,
        help: "after a headless run, print the summary as one line of JSON, for programs",
    },
    OptionInfo {
        name: "--tune",
        value: Some("FILE"),
        help: "read ctx.tune values from FILE (default: examples/<game>.tune.json, if it exists)",
    },
    OptionInfo {
        name: "--set",
        value: Some("NAME=N"),
        help: "set one ctx.tune value, over the tune file; repeat it for more",
    },
    OptionInfo {
        name: "--write-tune",
        value: None,
        help: "after the run, write every ctx.tune name the game used, with its value, to the tune file",
    },
    OptionInfo {
        name: "--help",
        value: None,
        help: "show this help and quit",
    },
];

/// The folder `--headless` saves its files in when there's no `--out`.
pub(crate) const DEFAULT_OUT: &str = "duckforge-out";

/// The engine's options, as read from the command line.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Options {
    pub headless: bool,
    pub help: bool,
    pub frames: Option<u64>,
    pub out: Option<PathBuf>,
    pub seed: Option<u32>,
    pub inputs: Option<PathBuf>,
    pub record: Option<PathBuf>,
    pub snapshot_every: Option<u64>,
    pub sheet: bool,
    pub sheet_events: bool,
    pub gif: bool,
    pub crop: Option<Crop>,
    pub zoom: Option<usize>,
    pub debug: bool,
    pub debug_corner: Option<Corner>,
    pub tune: Option<PathBuf>,
    pub set: Vec<(String, f32)>,
    pub write_tune: bool,
    pub describe: bool,
    pub quiet: bool,
    pub accept: bool,
    pub json: bool,
    pub grid: Option<usize>,
    pub grid_world: bool,
    pub diff: Option<(PathBuf, PathBuf)>,
    pub fuzz: Option<u32>,
    pub fuzz_keys: Option<Vec<crate::input::Key>>,
    pub freeze_seconds: Option<f32>,
}

impl Options {
    /// Reads the options this program was started with.
    pub(crate) fn from_command_line() -> Result<Options, String> {
        Options::parse(std::env::args().skip(1)) // skip the program's own name
    }

    /// Reads the options from a list of words. Words that aren't engine
    /// options are skipped: they belong to the game.
    pub(crate) fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        for (name, value) in try_split(args)?.0 {
            let value = value.as_deref().unwrap_or("");
            match name {
                "--headless" => options.headless = true,
                "--help" => options.help = true,
                "--frames" => {
                    let frames = value
                        .parse()
                        .map_err(|_| format!("--frames needs a whole number, not {value:?}"))?;
                    if frames == 0 {
                        return Err(String::from("--frames must be at least 1"));
                    }
                    options.frames = Some(frames);
                }
                "--out" => options.out = Some(PathBuf::from(value)),
                "--inputs" => options.inputs = Some(PathBuf::from(value)),
                "--sheet" => options.sheet = true,
                "--sheet-events" => options.sheet_events = true,
                "--gif" => options.gif = true,
                "--debug" => options.debug = true,
                "--debug-corner" => options.debug_corner = Some(Corner::parse(value)?),
                "--tune" => options.tune = Some(PathBuf::from(value)),
                "--set" => options.set.push(crate::tune::parse_set(value)?),
                "--write-tune" => options.write_tune = true,
                "--describe" => options.describe = true,
                "--quiet" => options.quiet = true,
                "--accept" => options.accept = true,
                "--json" => options.json = true,
                "--fuzz" => {
                    let runs = value
                        .parse()
                        .ok()
                        .filter(|runs| *runs >= 1)
                        .ok_or_else(|| {
                            format!("--fuzz needs a number of runs, at least 1, not {value:?}")
                        })?;
                    options.fuzz = Some(runs);
                }
                "--fuzz-keys" => options.fuzz_keys = Some(crate::fuzz::parse_keys(value)?),
                "--freeze-seconds" => {
                    let seconds = value
                        .parse()
                        .ok()
                        .filter(|s: &f32| s.is_finite() && *s > 0.0)
                        .ok_or_else(|| {
                            format!(
                                "--freeze-seconds needs a number of seconds above 0, not {value:?}"
                            )
                        })?;
                    options.freeze_seconds = Some(seconds);
                }
                "--grid" | "--grid-world" => {
                    if options.grid.is_some() {
                        return Err(String::from("use either --grid or --grid-world, not both"));
                    }
                    options.grid_world = name == "--grid-world";
                    let every = value
                        .parse()
                        .ok()
                        .filter(|every| (2..=1000).contains(every))
                        .ok_or_else(|| {
                            format!("{name} needs a number from 2 to 1000, not {value:?}")
                        })?;
                    options.grid = Some(every);
                }
                "--diff" => {
                    let (old, new) = value.split_once('\n').unwrap_or((value, ""));
                    options.diff = Some((PathBuf::from(old), PathBuf::from(new)));
                }
                "--crop" => options.crop = Some(Crop::parse(value)?),
                "--snapshot-every" => {
                    let every = value.parse().map_err(|_| {
                        format!("--snapshot-every needs a whole number, not {value:?}")
                    })?;
                    if every == 0 {
                        return Err(String::from("--snapshot-every must be at least 1"));
                    }
                    options.snapshot_every = Some(every);
                }
                "--zoom" => {
                    let zoom = value
                        .parse()
                        .ok()
                        .filter(|zoom| (1..=32).contains(zoom))
                        .ok_or_else(|| {
                            format!("--zoom needs a whole number from 1 to 32, not {value:?}")
                        })?;
                    options.zoom = Some(zoom);
                }
                "--record" => options.record = Some(PathBuf::from(value)),
                "--seed" => {
                    let seed = value.parse().map_err(|_| {
                        format!("--seed needs a whole number from 0 to 4294967295, not {value:?}")
                    })?;
                    options.seed = Some(seed);
                }
                _ => unreachable!("try_split only returns known options"),
            }
        }
        let saves_files = [
            ("--out", options.out.is_some()),
            ("--snapshot-every", options.snapshot_every.is_some()),
            ("--sheet", options.sheet),
            ("--sheet-events", options.sheet_events),
            ("--gif", options.gif),
            ("--crop", options.crop.is_some()),
            ("--zoom", options.zoom.is_some()),
            ("--describe", options.describe),
            ("--quiet", options.quiet),
            ("--accept", options.accept),
            ("--json", options.json),
            ("--grid", options.grid.is_some() && !options.grid_world),
            ("--grid-world", options.grid_world),
        ];
        if let Some((name, _)) = saves_files.iter().find(|(_, used)| *used) {
            // --fuzz runs headless by itself, and --out is where its scripts go.
            let fuzz_out = *name == "--out" && options.fuzz.is_some();
            if !options.headless && !fuzz_out {
                return Err(format!(
                    "{name} only works with --headless (a window doesn't save files)"
                ));
            }
        }
        let fuzz_only = [
            ("--fuzz-keys", options.fuzz_keys.is_some()),
            ("--freeze-seconds", options.freeze_seconds.is_some()),
        ];
        if let Some((name, _)) = fuzz_only.iter().find(|(_, used)| *used) {
            if options.fuzz.is_none() {
                return Err(format!("{name} only works with --fuzz N"));
            }
        }
        if options.accept && options.inputs.is_none() {
            return Err(String::from(
                "--accept needs --inputs FILE: it writes the run's ending into that playthrough",
            ));
        }
        if options.quiet && options.json {
            return Err(String::from(
                "use either --quiet or --json, not both (--json already prints only the summary)",
            ));
        }
        if options.record.is_some() && options.headless {
            return Err(String::from(
                "--record only works in a window: it records what a person does",
            ));
        }
        Ok(options)
    }
}

/// The command-line words meant for the game: everything except the
/// engine's own options and their values.
///
/// A game with options of its own should read them from here instead of
/// from `std::env::args()`, so it doesn't trip over `--headless` and friends.
pub fn game_args() -> Vec<String> {
    match try_split(std::env::args().skip(1)) {
        Ok((_, game)) => game,
        // `run` reports the problem properly; here, just don't hand the game
        // the engine's words.
        Err(_) => std::env::args()
            .skip(1)
            .filter(|word| find(word).is_none())
            .collect(),
    }
}

/// The engine's options (each with its value, if it takes one), and the
/// words that belong to the game.
type Split = (Vec<(&'static str, Option<String>)>, Vec<String>);

/// Splits command-line words into the engine's options and the game's words.
fn try_split(args: impl IntoIterator<Item = String>) -> Result<Split, String> {
    let mut engine = Vec::new();
    let mut game = Vec::new();
    let mut words = args.into_iter();
    while let Some(word) = words.next() {
        // `--frames=600` works as well as `--frames 600`.
        let (name, attached) = match word.split_once('=') {
            Some((name, value)) => (name.to_string(), Some(value.to_string())),
            None => (word.clone(), None),
        };
        let Some(option) = find(&name) else {
            if let Some(near) = near_miss(&name) {
                return Err(format!("unknown option {name}: did you mean {near}?"));
            }
            game.push(word);
            continue;
        };
        // A value written as two words, like "OLD_DIR NEW_DIR", takes the
        // next two words (joined with a line break, which paths never have).
        let words_wanted = option.value.map_or(0, |what| what.split(' ').count());
        let value = match (option.value, attached) {
            (None, None) => None,
            (None, Some(_)) => return Err(format!("{} doesn't take a value", option.name)),
            (Some(what), Some(_)) if words_wanted > 1 => {
                return Err(format!(
                    "{} needs {words_wanted} values: {} {what}",
                    option.name, option.name
                ));
            }
            (Some(_), Some(value)) => Some(value),
            (Some(what), None) => {
                let mut values = Vec::new();
                for _ in 0..words_wanted {
                    values.push(words.next().ok_or_else(|| {
                        format!("{} needs a value: {} {what}", option.name, option.name)
                    })?);
                }
                Some(values.join("\n"))
            }
        };
        engine.push((option.name, value));
    }
    Ok((engine, game))
}

fn find(name: &str) -> Option<&'static OptionInfo> {
    OPTIONS.iter().find(|option| option.name == name)
}

/// An engine option `name` is probably a typo of, like `--frame` for
/// `--frames`. A typo would otherwise be silently handed to the game.
fn near_miss(name: &str) -> Option<&'static str> {
    if !name.starts_with("--") {
        return None;
    }
    OPTIONS
        .iter()
        .map(|option| option.name)
        .find(|known| edit_distance(name, known) <= 2)
}

/// How many single-letter changes (add, remove or swap one letter) turn `a` into `b`.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    // `row[j]` is the distance between what we've seen of `a` and the first `j` letters of `b`.
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, &cb) in b.iter().enumerate() {
            let substitute = diagonal + usize::from(ca != cb);
            diagonal = row[j + 1];
            row[j + 1] = substitute.min(row[j] + 1).min(row[j + 1] + 1);
        }
    }
    row[b.len()]
}

/// The help text for `--help`.
pub(crate) fn help() -> String {
    let mut text = String::from("duckforge options (every game has these):\n");
    for option in OPTIONS {
        let name = match option.value {
            Some(value) => format!("{} {value}", option.name),
            None => option.name.to_string(),
        };
        text += &format!("  {name:<16} {}\n", option.help);
    }
    text += "\nExample: cargo run --example breakout -- --headless --frames 600 --out shots\n";
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Options, String> {
        Options::parse(line.split_whitespace().map(String::from))
    }

    #[test]
    fn options_are_read() {
        let options = parse("--headless --frames 600 --out shots --seed 7").unwrap();
        assert_eq!(
            options,
            Options {
                headless: true,
                frames: Some(600),
                out: Some(PathBuf::from("shots")),
                seed: Some(7),
                help: false,
                ..Options::default()
            }
        );
        assert_eq!(parse("").unwrap(), Options::default());
        assert_eq!(parse("--frames=30").unwrap().frames, Some(30));
    }

    /// The flags (`--something`) written in a document.
    fn flags_in(text: &str) -> std::collections::BTreeSet<String> {
        let mut flags = std::collections::BTreeSet::new();
        for (i, _) in text.match_indices("--") {
            let rest = &text[i + 2..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '-')
                .collect();
            let preceded_by_dash = text[..i].ends_with('-');
            if name.starts_with(|c: char| c.is_ascii_lowercase()) && !preceded_by_dash {
                flags.insert(format!("--{}", name.trim_end_matches('-')));
            }
        }
        flags
    }

    /// Flags in the docs that aren't the engine's: Cargo's, the Rust
    /// installer's, and the stress3d example's own options.
    const NOT_ENGINE_FLAGS: &[&str] = &[
        "--example",
        "--examples",
        "--no-default-features",
        "--features",
        "--release",
        "--open",
        "--test",
        "--ignored",
        "--proto",
        "--tlsv1",
        "--objects",
        "--seconds",
        "--renderer",
    ];

    #[test]
    fn every_flag_in_the_docs_exists_and_every_flag_is_documented() {
        let docs = [
            ("docs/AI_GUIDE.md", include_str!("../docs/AI_GUIDE.md")),
            ("README.md", include_str!("../README.md")),
        ];
        // The quick start and the skill mention only some flags, but they
        // must exist.
        let skill = include_str!("../skills/duckforge/SKILL.md");
        for needed in [
            "--headless",
            "--inputs",
            "--sheet",
            "--describe",
            "--set",
            "--fuzz",
        ] {
            assert!(
                flags_in(skill).contains(needed),
                "SKILL.md should show {needed}"
            );
        }
        let some_flags = [
            ("docs/QUICKSTART.md", include_str!("../docs/QUICKSTART.md")),
            ("skills/duckforge/SKILL.md", skill),
        ];
        for (name, text) in some_flags {
            for flag in flags_in(text) {
                assert!(
                    OPTIONS.iter().any(|o| o.name == flag)
                        || NOT_ENGINE_FLAGS.contains(&flag.as_str()),
                    "{name} mentions {flag}, but the engine has no such option"
                );
            }
        }
        let engine: Vec<&str> = OPTIONS.iter().map(|option| option.name).collect();
        let help = help();
        for (name, text) in docs {
            let in_doc = flags_in(text);
            for flag in &in_doc {
                assert!(
                    engine.contains(&flag.as_str()) || NOT_ENGINE_FLAGS.contains(&flag.as_str()),
                    "{name} mentions {flag}, but the engine has no such option"
                );
            }
            for flag in &engine {
                assert!(in_doc.contains(*flag), "{flag} isn't mentioned in {name}");
            }
        }
        for flag in &engine {
            assert!(help.contains(flag), "{flag} isn't in --help");
        }
    }

    #[test]
    fn the_verify_scripts_use_real_flags_and_the_same_ones() {
        let sh = flags_in(include_str!("../tools/verify.sh"));
        let bat = flags_in(include_str!("../tools/verify.bat"));
        for flag in &sh {
            assert!(
                OPTIONS.iter().any(|o| o.name == flag) || NOT_ENGINE_FLAGS.contains(&flag.as_str()),
                "tools/verify.sh uses {flag}, but the engine has no such option"
            );
        }
        assert_eq!(
            sh, bat,
            "tools/verify.sh and tools/verify.bat should do the same"
        );
        for needed in [
            "--headless",
            "--inputs",
            "--sheet-events",
            "--debug",
            "--quiet",
            "--fuzz",
        ] {
            assert!(sh.iter().any(|f| f == needed), "verify should use {needed}");
        }
    }

    #[test]
    fn the_parser_accepts_every_option_it_lists() {
        // A sensible value for each option that takes one.
        let value = |name: &str| match name {
            "--out" => "shots",
            "--inputs" | "--record" => "run.json",
            "--crop" => "0,0,10,10",
            "--debug-corner" => "top-left",
            "--tune" => "game.tune.json",
            "--set" => "speed=2",
            "--diff" => "old new",
            "--fuzz-keys" => "Left,Space",
            _ => "2",
        };
        for option in OPTIONS {
            let mut words = vec![option.name.to_string()];
            if option.value.is_some() {
                words.extend(value(option.name).split(' ').map(String::from));
            }
            // Most options are for --headless; --record only works in a window.
            if option.name != "--record" {
                words.push(String::from("--headless"));
            }
            if option.name == "--accept" {
                words.extend([String::from("--inputs"), String::from("p.json")]);
            }
            if option.name == "--fuzz-keys" || option.name == "--freeze-seconds" {
                words.extend([String::from("--fuzz"), String::from("3")]);
            }
            let parsed = Options::parse(words.clone());
            assert!(parsed.is_ok(), "{words:?}: {parsed:?}");
            assert_ne!(
                parsed.unwrap(),
                Options::default(),
                "{words:?} changed nothing"
            );
        }
        // Anything else belongs to the game, and is left alone.
        assert_eq!(
            Options::parse([String::from("--my-game-option")]),
            Ok(Options::default())
        );
    }

    #[test]
    fn diff_takes_two_folders() {
        let options = parse("--diff out/a out/b").unwrap();
        assert_eq!(
            options.diff,
            Some((PathBuf::from("out/a"), PathBuf::from("out/b")))
        );
        assert!(
            parse("--diff out/a")
                .unwrap_err()
                .contains("needs a value: --diff OLD_DIR NEW_DIR")
        );
        assert!(
            parse("--diff=out/a")
                .unwrap_err()
                .contains("needs 2 values")
        );
        assert_eq!(parse("--headless --grid 32").unwrap().grid, Some(32));
        assert!(
            parse("--headless --grid 1")
                .unwrap_err()
                .contains("from 2 to 1000")
        );
    }

    #[test]
    fn set_can_be_repeated() {
        let options =
            parse("--headless --set speed=2 --set=gravity=900.5 --tune t.json --write-tune")
                .unwrap();
        assert_eq!(
            options.set,
            [
                (String::from("speed"), 2.0),
                (String::from("gravity"), 900.5)
            ]
        );
        assert_eq!(options.tune, Some(PathBuf::from("t.json")));
        assert!(options.write_tune);
        assert!(parse("--set speed").unwrap_err().contains("NAME=NUMBER"));
    }

    #[test]
    fn quiet_and_json_choose_what_a_headless_run_prints() {
        let quiet = parse("--headless --quiet").unwrap();
        assert!(quiet.quiet && !quiet.json);
        let json = parse("--headless --json --describe").unwrap();
        assert!(json.json && json.describe);
        assert!(
            parse("--quiet")
                .unwrap_err()
                .contains("only works with --headless")
        );
        assert!(
            parse("--json")
                .unwrap_err()
                .contains("only works with --headless")
        );
        assert!(
            parse("--headless --quiet --json")
                .unwrap_err()
                .contains("not both")
        );
    }

    #[test]
    fn debug_corners_are_read() {
        let corner = |words: &str| parse(words).map(|o| o.debug_corner);
        assert_eq!(
            corner("--debug-corner top-right"),
            Ok(Some(Corner::TopRight))
        );
        assert_eq!(
            corner("--debug-corner=BOTTOM_LEFT"),
            Ok(Some(Corner::BottomLeft))
        );
        assert_eq!(corner(""), Ok(None));
        assert!(
            corner("--debug-corner middle")
                .unwrap_err()
                .contains("top-left, top-right")
        );
    }

    #[test]
    fn bad_values_get_clear_errors() {
        assert!(parse("--frames ten").unwrap_err().contains("whole number"));
        assert!(parse("--frames 0").unwrap_err().contains("at least 1"));
        assert!(parse("--frames").unwrap_err().contains("needs a value"));
        assert!(parse("--seed -1").unwrap_err().contains("--seed"));
        assert!(
            parse("--headless=yes")
                .unwrap_err()
                .contains("doesn't take a value")
        );
        assert!(
            parse("--out shots")
                .unwrap_err()
                .contains("only works with --headless")
        );
        let error = parse("--headless --record r.json").unwrap_err();
        assert!(error.contains("only works in a window"));
        assert!(
            parse("--sheet")
                .unwrap_err()
                .contains("only works with --headless")
        );
        assert!(
            parse("--headless --crop 1,2,3")
                .unwrap_err()
                .contains("x,y,w,h")
        );
        assert!(
            parse("--headless --zoom 0")
                .unwrap_err()
                .contains("1 to 32")
        );
        assert!(
            parse("--headless --snapshot-every 0")
                .unwrap_err()
                .contains("at least 1")
        );
        let pictures =
            parse("--headless --snapshot-every 60 --sheet --gif --crop 1,2,3,4 --zoom 8");
        let pictures = pictures.unwrap();
        assert_eq!(pictures.snapshot_every, Some(60));
        assert!(pictures.sheet && pictures.gif);
        assert_eq!(pictures.crop.map(|crop| crop.width), Some(3));
        assert_eq!(pictures.zoom, Some(8));
        let replay = parse("--inputs win.json --record again.json").unwrap();
        assert_eq!(replay.inputs, Some(PathBuf::from("win.json")));
        assert_eq!(replay.record, Some(PathBuf::from("again.json")));
    }

    #[test]
    fn typos_are_caught_but_game_options_are_left_alone() {
        assert_eq!(
            parse("--headless --frame 60").unwrap_err(),
            "unknown option --frame: did you mean --frames?"
        );
        assert!(parse("--hedless").unwrap_err().contains("--headless"));
        // The stress test's own options are nothing like the engine's.
        assert!(parse("--objects 5000 --seconds 10 --renderer cpu").is_ok());
    }

    #[test]
    fn the_game_gets_the_words_the_engine_doesnt_use() {
        let words = "--objects 500 --headless --frames 60 --renderer cpu --seed=3";
        let (engine, game) = try_split(words.split_whitespace().map(String::from)).unwrap();
        assert_eq!(game, ["--objects", "500", "--renderer", "cpu"]);
        assert_eq!(engine.len(), 3);
    }

    #[test]
    fn edit_distance_counts_single_letter_changes() {
        assert_eq!(edit_distance("--frames", "--frames"), 0);
        assert_eq!(edit_distance("--frame", "--frames"), 1);
        assert_eq!(edit_distance("--fames", "--frames"), 1);
        assert_eq!(edit_distance("--seeds", "--seed"), 1);
        assert_eq!(edit_distance("--objects", "--out"), 5);
    }

    #[test]
    fn help_lists_every_option() {
        let text = help();
        for option in OPTIONS {
            assert!(text.contains(option.name));
        }
    }
}
