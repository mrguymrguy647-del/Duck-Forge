//! A game's pictures and sounds: opening a picture in Duck Quill, the
//! sounds the game has played, and how each was made.
//!
//! The Studio has no picture editor and no sound code of its own: Duck
//! Quill draws, the game plays its own sounds (the Studio asks it to play
//! one again), and Duck Echo's recipes are shown as text.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::console::CommandLine;
use crate::folder::GamesFolder;
use crate::problem::Problem;

/// Where to get Duck Quill.
pub const DUCK_QUILL_PAGE: &str = "https://github.com/mrguymrguy647-del/duck-quill/releases";

/// A sound the game has played, as it said it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SoundInfo {
    pub name: String,
    /// A `.wav`'s file, as the game named it.
    #[serde(default)]
    pub file: Option<String>,
    /// A synthesizer sound's values, as code.
    #[serde(default)]
    pub recipe: Option<String>,
}

/// How a sound was made, in words or code: a synthesizer sound's values;
/// for a `.wav`, Duck Echo's recipe `art/<name>.toml` if there is one, or
/// the line of `art/README.md` that names the file.
pub fn recipe(folder: &GamesFolder, game_folder: Option<&Path>, sound: &SoundInfo) -> String {
    if let Some(code) = &sound.recipe {
        return code.clone();
    }
    let Some(file) = &sound.file else {
        return String::from("No recipe was saved for this sound.");
    };
    let wav = folder.root().join(file);
    let stem = wav
        .file_stem()
        .map_or(String::new(), |stem| stem.to_string_lossy().into_owned());
    let art = game_folder
        .map(|own| own.join("art"))
        .unwrap_or_else(|| wav.parent().unwrap_or(Path::new("")).join("art"));
    if let Ok(text) = std::fs::read_to_string(art.join(format!("{stem}.toml"))) {
        return format!("Duck Echo recipe, art/{stem}.toml:\n\n{text}");
    }
    if let Ok(text) = std::fs::read_to_string(art.join("README.md")) {
        let name = wav
            .file_name()
            .map_or(String::new(), |name| name.to_string_lossy().into_owned());
        if let Some(line) = text.lines().find(|line| line.contains(&name)) {
            return format!("From art/README.md:\n\n{}", line.trim());
        }
    }
    String::from("No recipe was saved for this sound.")
}

/// Duck Quill's program: the one in the settings, or one next to the
/// Studio (in its folder, or a "Duck Quill" folder beside it).
pub fn find_duck_quill(chosen: Option<&Path>) -> Option<PathBuf> {
    if let Some(chosen) = chosen.filter(|path| path.is_file()) {
        return Some(chosen.to_path_buf());
    }
    let name = if cfg!(windows) {
        "DuckQuill.exe"
    } else {
        "duck-quill"
    };
    let studio = std::env::current_exe().ok()?;
    let here = studio.parent()?;
    [
        here.join(name),
        here.join("Duck Quill").join(name),
        here.parent()?.join("Duck Quill").join(name),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

/// The command that opens `picture` in Duck Quill (it opens a file named
/// on its command line).
pub fn open_in_quill(quill: &Path, picture: &Path, folder: &GamesFolder) -> CommandLine {
    let shown = picture
        .strip_prefix(folder.root())
        .unwrap_or(picture)
        .display()
        .to_string();
    CommandLine::new(quill, &[picture.to_string_lossy().as_ref()], folder.root())
        .shown_as(format!("\"{}\" {shown}", quill.display()))
}

/// What to say when Duck Quill can't be found.
pub fn no_duck_quill() -> Problem {
    Problem::new(
        "Where is Duck Quill?",
        "Choose DuckQuill.exe, or get it from its download page.",
    )
    .with_link("Get Duck Quill", DUCK_QUILL_PAGE)
}

/// The picture a tile sheet or sprite sheet JSON file goes with: the PNG
/// beside it, with the same name.
pub fn picture_of_sheet(sheet: &Path) -> PathBuf {
    sheet.with_extension("png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_says_how_it_was_made() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .canonicalize()
            .unwrap();
        let folder = GamesFolder::open(root.clone()).unwrap();
        let synth = SoundInfo {
            name: String::from("jump"),
            file: None,
            recipe: Some(String::from("Sound { .. }")),
        };
        assert_eq!(recipe(&folder, None, &synth), "Sound { .. }");
        let music = SoundInfo {
            name: String::from("music"),
            file: Some(String::from("examples/duck_peaks/assets/music.wav")),
            recipe: None,
        };
        let said = recipe(&folder, Some(&root.join("examples/duck_peaks")), &music);
        assert!(
            said.starts_with("Duck Echo recipe, art/music.toml"),
            "{said}"
        );
        let nothing = SoundInfo {
            name: String::from("x"),
            file: Some(String::from("nowhere/x.wav")),
            recipe: None,
        };
        assert_eq!(
            recipe(&folder, None, &nothing),
            "No recipe was saved for this sound."
        );
    }
}
