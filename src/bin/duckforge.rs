//! `duckforge`: Duck Forge's own tools, for files made anywhere.
//!
//! ```text
//! duckforge review any.mp4 [--out DIR] [--profiles DIR]
//! ```
//!
//! reviews a video the way a viewer sees it (see `duckforge::review`):
//! from its pixels and sound alone, since the engine didn't make it.

use std::path::PathBuf;
use std::process::ExitCode;

const HELP: &str = "duckforge review VIDEO [--out DIR] [--profiles DIR]

Checks a video the way a viewer sees it, and writes a review folder
(VIDEO's name with -review, or --out DIR): review.txt (short, worst
first, with times), review.json, overview.png, filmstrips of each flagged
moment and spectrogram.png. --profiles DIR also reads platform profiles
(TikTok, YouTube...) from DIR's .toml files. Needs ffmpeg.

For a game made with Duck Forge, run the game with --headless --review
instead: the engine then knows the camera, the text and the sounds, not
just the pixels.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("review")
        || args.iter().any(|a| a == "--help" || a == "-h")
    {
        println!("{HELP}");
        return if args.first().map(String::as_str) == Some("review") || args.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let mut video = None;
    let mut out = None;
    let mut profiles = None;
    let mut words = args[1..].iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--out" | "--profiles" => {
                let Some(value) = words.next() else {
                    eprintln!("duckforge: {word} needs a folder");
                    return ExitCode::FAILURE;
                };
                if word == "--out" {
                    out = Some(PathBuf::from(value));
                } else {
                    profiles = Some(PathBuf::from(value));
                }
            }
            other if other.starts_with("--") => {
                eprintln!("duckforge: unknown option {other}\n\n{HELP}");
                return ExitCode::FAILURE;
            }
            other => video = Some(PathBuf::from(other)),
        }
    }
    let Some(video) = video else {
        eprintln!("duckforge: which video? duckforge review VIDEO");
        return ExitCode::FAILURE;
    };
    match duckforge::review::review_video(&video, out.as_deref(), profiles.as_deref()) {
        Ok(review) => {
            println!("duckforge: {}", review.line());
            ExitCode::SUCCESS
        }
        Err(problem) => {
            eprintln!("duckforge: {problem}");
            ExitCode::FAILURE
        }
    }
}
