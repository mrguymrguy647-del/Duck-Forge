//! `files.use`: a file dropped on the scene, or on an object's Picture or
//! Sound box. A picture (or a Duck Quill sheet) becomes a Picture object,
//! a sound a Sound object (music for the scene), a level a Tile map; on a
//! box, it becomes that object's picture or sound. A file from elsewhere
//! on the computer (dropped from File Explorer) is copied into the game's
//! `assets` folder first.

use std::path::{Path, PathBuf};

use duckforge::prelude::SpriteSheet;
use serde_json::{Map, Value, json};

use super::Studio;
use crate::game_files;
use crate::problem::Problem;

/// What a file can be in a scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Usable {
    /// A PNG, or a Duck Quill sheet (its `.json`): as the scene names it.
    Picture(String),
    /// A `.wav`.
    Sound(String),
    /// A level: a `.txt` with a `.legend.json` beside it.
    Level(String),
}

/// What the file at `place` in the game's `folder` can be in a scene. A
/// PNG with a Duck Quill sheet beside it (`hero.png` and `hero.json`) is
/// the sheet: that's the picture that moves.
pub fn usable(folder: &Path, place: &str) -> Result<Usable, Problem> {
    let lower = place.to_ascii_lowercase();
    let end = lower.rsplit_once('.').map_or("", |(_, end)| end);
    let name = place.rsplit('/').next().unwrap_or(place);
    match end {
        "png" => {
            let sheet = format!("{}.json", &place[..place.len() - 4]);
            Ok(if sheet_of(folder, &sheet).as_deref() == Some(name) {
                Usable::Picture(sheet)
            } else {
                Usable::Picture(place.to_string())
            })
        }
        "json" if lower.ends_with(".legend.json") => Err(Problem::new(
            format!("{name} is a level's legend."),
            "Drag the level itself (the .txt beside it).",
        )),
        "json" => match sheet_of(folder, place) {
            Some(_) if SpriteSheet::load(folder.join(place)).is_ok() => {
                Ok(Usable::Picture(place.to_string()))
            }
            _ => Err(Problem::new(
                format!("{name} isn't a picture or a Duck Quill sheet."),
                "Drag a .png picture, a sheet's .json, a .wav sound or a level.",
            )),
        },
        "wav" => Ok(Usable::Sound(place.to_string())),
        "ogg" | "mp3" | "flac" => Err(Problem::new(
            format!("{name} can't play in a game: games play .wav sounds."),
            "Save it as a .wav (Duck Echo makes .wav sounds), then drag that in.",
        )),
        "gif" | "bmp" | "jpg" | "jpeg" => Err(Problem::new(
            format!("{name} can't be drawn in a game: games draw .png pictures."),
            "Save it as a .png (Duck Quill makes .png pictures), then drag that in.",
        )),
        "txt" => {
            let legend = format!("{}.legend.json", &place[..place.len() - 4]);
            if folder.join(&legend).is_file() {
                Ok(Usable::Level(place.to_string()))
            } else {
                Err(Problem::new(
                    format!("{name} isn't a level: there's no {legend} beside it."),
                    "Drag a level (a .txt with a .legend.json beside it), a picture or a sound.",
                ))
            }
        }
        _ => Err(Problem::new(
            format!("{name} isn't something a scene can show or play."),
            "Drag a .png picture, a Duck Quill sheet, a .wav sound or a level.",
        )),
    }
}

/// The picture a Duck Quill sheet's `.json` names (its `"image"`), if it
/// is one.
fn sheet_of(folder: &Path, place: &str) -> Option<String> {
    let text = std::fs::read_to_string(folder.join(place)).ok()?;
    let sheet: Value = serde_json::from_str(&text).ok()?;
    sheet["image"].as_str().map(String::from)
}

/// Brings a file from elsewhere on the computer into the game's `assets`
/// folder: the place it has there. A file that's there already, the same
/// to the byte, isn't copied again. A sheet brings its picture, and a
/// picture its sheet, and if the picture had to be given another name
/// (`hero 2.png`) the sheet is changed to name it.
fn bring_in(folder: &Path, file: &Path) -> Result<String, Problem> {
    let assets = folder.join("assets");
    std::fs::create_dir_all(&assets).map_err(|error| {
        Problem::new(
            "The Studio couldn't make the game's assets folder.",
            "Check the game's folder isn't read-only, then try again.",
        )
        .with_details(error.to_string())
    })?;
    let lower = file.to_string_lossy().to_ascii_lowercase();
    let beside = |name: &str| file.parent().unwrap_or(Path::new("")).join(name);
    // A sheet and its picture come in together.
    let pair = if lower.ends_with(".json") {
        read_sheet(file).map(|(sheet, image)| (beside(&image), sheet))
    } else if lower.ends_with(".png") {
        let json = file.with_extension("json");
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        read_sheet(&json)
            .filter(|(_, image)| *image == name)
            .map(|(sheet, _)| (file.to_path_buf(), sheet))
    } else {
        None
    };
    let Some((picture, mut sheet)) = pair else {
        return bring_one(folder, file);
    };
    let picture = bring_one(folder, &picture)?;
    let picture_name = picture.rsplit('/').next().unwrap_or(&picture).to_string();
    sheet["image"] = json!(picture_name);
    let text = serde_json::to_string_pretty(&sheet).unwrap_or_default() + "\n";
    let place = format!("{}.json", &picture[..picture.len() - 4]);
    let target = folder.join(&place);
    match std::fs::read_to_string(&target) {
        Ok(there) if there == text => {}
        Ok(_) => return Ok(picture), // a different sheet has that name: just the picture
        Err(_) => std::fs::write(&target, &text).map_err(|error| {
            Problem::new(
                format!("The Studio couldn't save {place}."),
                "Check the game's folder isn't read-only, then try again.",
            )
            .with_details(error.to_string())
        })?,
    }
    Ok(place)
}

/// A Duck Quill sheet's JSON and the picture it names.
fn read_sheet(file: &Path) -> Option<(Value, String)> {
    let sheet: Value = serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()?;
    let image = sheet["image"].as_str()?.to_string();
    Some((sheet, image))
}

/// One file into `assets`, unless the same one is there already.
fn bring_one(folder: &Path, file: &Path) -> Result<String, Problem> {
    let name = file
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let there = folder.join("assets").join(&name);
    let same = match (std::fs::read(&there), std::fs::read(file)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same {
        return Ok(format!("assets/{name}"));
    }
    let added = game_files::add(folder, "assets", &[file.to_path_buf()])?;
    added.into_iter().next().ok_or_else(|| {
        Problem::new(
            format!("The Studio couldn't copy {name} into the game."),
            "Try again, or copy it into the game's assets folder in File Explorer.",
        )
    })
}

impl Studio {
    /// `files.use`: puts a file in the scene. Without `ids`, a new object
    /// of the file (where `at` says, or in the middle of the view). With
    /// `ids`, those objects get it as their `key` (`picture` or `sound`,
    /// by what the file is). A path outside the game's folder is copied
    /// into its `assets` first. Each is one step of Undo; a file copied in
    /// stays when it's undone.
    pub(super) fn files_use(
        &mut self,
        path: String,
        at: Option<[f32; 2]>,
        ids: Vec<u64>,
        key: Option<String>,
    ) -> Result<String, Problem> {
        let folder = self.files_folder()?;
        let given = PathBuf::from(&path);
        let outside = given.is_absolute() && !given.starts_with(&folder);
        let (place, copied) = if outside {
            if !given.is_file() {
                return Err(Problem::new(
                    format!("{} isn't there.", given.display()),
                    "Drop files that are on this computer.",
                ));
            }
            (bring_in(&folder, &given)?, true)
        } else if given.is_absolute() {
            let inside = given.strip_prefix(&folder).unwrap_or(&given);
            (inside.to_string_lossy().replace('\\', "/"), false)
        } else {
            let real = game_files::resolve(&folder, &path)?;
            if !real.is_file() {
                return Err(Problem::new(
                    format!("The game's folder has no file {path}."),
                    "Pick one from Files.",
                ));
            }
            (path.replace('\\', "/"), false)
        };
        let usable = usable(&folder, &place)?;
        let said = if ids.is_empty() {
            let (kind, set) = match &usable {
                Usable::Picture(picture) => (
                    "picture",
                    Map::from_iter([(String::from("picture"), json!(picture))]),
                ),
                Usable::Sound(sound) => (
                    "sound",
                    Map::from_iter([
                        (String::from("sound"), json!(sound)),
                        (String::from("loops"), json!(true)),
                    ]),
                ),
                Usable::Level(level) => (
                    "tilemap",
                    Map::from_iter([(String::from("level"), json!(level))]),
                ),
            };
            let at = if kind == "tilemap" { None } else { at };
            self.object_add(String::from(kind), at, None, set)?
        } else {
            let (wanted, value) = match &usable {
                Usable::Picture(picture) => ("picture", json!(picture)),
                Usable::Sound(sound) => ("sound", json!(sound)),
                Usable::Level(_) => {
                    return Err(Problem::new(
                        "A level can't go in an object's box.",
                        "Drop it on the scene: it becomes a tile map.",
                    ));
                }
            };
            let key = key.unwrap_or_else(|| String::from(wanted));
            if key != wanted {
                return Err(Problem::new(
                    format!("That's a {wanted}, and this box wants a {key}."),
                    format!("Drop a {key} on it."),
                ));
            }
            let mut set = Map::from_iter([(key.clone(), value)]);
            if key == "picture" {
                // Another picture has other animations.
                set.insert(String::from("animation"), Value::Null);
            }
            self.object_set(ids, set, None)?
        };
        Ok(if copied {
            format!("Copied it in as {place}. {said}")
        } else {
            said
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_folder::TestFolder;

    fn game() -> TestFolder {
        let folder = TestFolder::new("use-file");
        for (place, from) in [
            ("assets/hero.json", "examples/scene_demo/assets/hero.json"),
            ("assets/hero.png", "examples/scene_demo/assets/hero.png"),
            ("assets/sign.png", "examples/scene_demo/assets/sign.png"),
            ("assets/coin.wav", "examples/scene_demo/assets/coin.wav"),
            ("level.txt", "examples/scene_demo/level.txt"),
            ("level.legend.json", "examples/scene_demo/level.legend.json"),
        ] {
            let from = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(from);
            let to = folder.join(place);
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(from, to).unwrap();
        }
        std::fs::write(folder.join("notes.txt"), "hello").unwrap();
        std::fs::write(folder.join("assets/music.ogg"), "not really").unwrap();
        folder
    }

    #[test]
    fn each_file_is_what_a_scene_can_make_of_it() {
        let folder = game();
        let root: &Path = &folder;
        assert_eq!(
            usable(root, "assets/sign.png"),
            Ok(Usable::Picture("assets/sign.png".into()))
        );
        assert_eq!(
            usable(root, "assets/hero.png"),
            Ok(Usable::Picture("assets/hero.json".into())),
            "a picture with its sheet beside it is the sheet"
        );
        assert_eq!(
            usable(root, "assets/hero.json"),
            Ok(Usable::Picture("assets/hero.json".into()))
        );
        assert_eq!(
            usable(root, "assets/coin.wav"),
            Ok(Usable::Sound("assets/coin.wav".into()))
        );
        assert_eq!(
            usable(root, "level.txt"),
            Ok(Usable::Level("level.txt".into()))
        );
        let said = |place: &str| usable(root, place).unwrap_err().what;
        assert!(said("assets/music.ogg").contains(".wav"));
        assert!(said("notes.txt").contains("isn't a level"));
        assert!(said("level.legend.json").contains("legend"));
    }

    #[test]
    fn a_file_from_elsewhere_comes_in_once_and_a_sheet_brings_its_picture() {
        let folder = game();
        let root: &Path = &folder;
        let elsewhere = TestFolder::new("use-file-elsewhere");
        let sign = elsewhere.join("sign.png");
        std::fs::copy(root.join("assets/sign.png"), &sign).unwrap();
        // The same sign: not copied again.
        assert_eq!(bring_in(root, &sign).unwrap(), "assets/sign.png");
        // Another picture with that name: it comes in beside.
        std::fs::copy(root.join("assets/hero.png"), &sign).unwrap();
        assert_eq!(bring_in(root, &sign).unwrap(), "assets/sign 2.png");

        // A sheet whose picture's name is taken: both come in, and the
        // sheet names its picture's new name.
        let hero_png = elsewhere.join("hero.png");
        std::fs::copy(root.join("assets/sign.png"), &hero_png).unwrap(); // a different hero.png
        let mut sheet: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join("assets/hero.json")).unwrap())
                .unwrap();
        sheet["frame"] = json!({"width": 1, "height": 1});
        std::fs::write(elsewhere.join("hero.json"), sheet.to_string()).unwrap();
        let place = bring_in(root, &elsewhere.join("hero.json")).unwrap();
        assert_eq!(place, "assets/hero 2.json");
        let brought: Value =
            serde_json::from_str(&std::fs::read_to_string(root.join(&place)).unwrap()).unwrap();
        assert_eq!(brought["image"], "hero 2.png");
        assert!(root.join("assets/hero 2.png").is_file());
    }
}
