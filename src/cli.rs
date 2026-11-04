//! The command-line options every duckforge game understands.
//!
//! `run` reads them, so a game gets `--headless`, `--frames` and the rest
//! without writing any code. Options the engine doesn't know are left
//! alone, so a game can have its own too (see [`game_args`]).

use std::path::PathBuf;

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
        help: "the folder --headless saves its files in (default: duckforge-out)",
    },
    OptionInfo {
        name: "--inputs",
        value: Some("FILE"),
        help: "replay an input script (a recording) in a window or with --headless",
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
        if options.out.is_some() && !options.headless {
            return Err(String::from(
                "--out only works with --headless (a window doesn't save files)",
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
        let value =
            match (option.value, attached) {
                (None, None) => None,
                (None, Some(_)) => return Err(format!("{} doesn't take a value", option.name)),
                (Some(_), Some(value)) => Some(value),
                (Some(what), None) => Some(words.next().ok_or_else(|| {
                    format!("{} needs a value: {} {what}", option.name, option.name)
                })?),
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
