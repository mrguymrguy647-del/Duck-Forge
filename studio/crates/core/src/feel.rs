//! Feel: the game's `ctx.tune` values as sliders.
//!
//! The values stay where they always were: `ctx.tune("jump_speed", 330.0)`
//! in the code and `examples/<game>.tune.json` on disk. One optional file
//! describes them, `examples/<game>.feel.json`:
//!
//! ```json
//! { "values": {
//!     "jump_speed": {"label": "Jump height", "group": "Duck",
//!                    "from": 150, "to": 500, "step": 5} },
//!   "presets": {
//!     "Fast and bouncy": {"run_speed": 150, "jump_speed": 380} } }
//! ```
//!
//! Every value the game asks for shows up, described or not: without a
//! description its name is tidied ("jump_speed" becomes "Jump speed") and
//! it goes from nothing to twice its default.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::folder::{Game, GamesFolder};
use crate::level::write_whole;
use crate::problem::Problem;

/// One `ctx.tune` value, as the game said it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TuneValue {
    pub name: String,
    pub value: f32,
    /// The value written in the code.
    pub default: f32,
    /// Where the value came from: `default`, `file`, `set`, `recording`,
    /// `changed while running`.
    pub source: String,
}

/// What a feel file says about one value.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Described {
    pub label: Option<String>,
    pub group: Option<String>,
    pub from: Option<f32>,
    pub to: Option<f32>,
    pub step: Option<f32>,
}

/// A game's feel file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeelFile {
    #[serde(default)]
    pub values: BTreeMap<String, Described>,
    #[serde(default)]
    pub presets: BTreeMap<String, BTreeMap<String, f32>>,
}

/// One slider in the Feel panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Slider {
    pub name: String,
    pub label: String,
    pub group: String,
    pub value: f32,
    pub default: f32,
    pub from: f32,
    pub to: f32,
    pub step: f32,
}

/// "jump_speed" as words: "Jump speed".
pub fn tidy(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The game's feel file, `examples/<game>.feel.json` (empty if there
/// isn't one, or a problem if it can't be read).
pub fn load_feel(folder: &GamesFolder, game: &str) -> Result<FeelFile, Problem> {
    let path = feel_file(folder, game);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Ok(FeelFile::default());
    };
    serde_json::from_str(&text).map_err(|error| {
        Problem::new(
            format!("{} has a mistake.", path.display()),
            "Fix it and save: the Feel panel reads it again.",
        )
        .with_details(error.to_string())
    })
}

pub fn feel_file(folder: &GamesFolder, game: &str) -> PathBuf {
    folder
        .root()
        .join("examples")
        .join(format!("{game}.feel.json"))
}

/// Where the game's tune values are saved (the engine reads this file).
pub fn tune_file(folder: &GamesFolder, game: &str) -> PathBuf {
    folder
        .root()
        .join("examples")
        .join(format!("{game}.tune.json"))
}

/// The sliders: every value the game asked for, described by the feel
/// file where it says, in the feel file's order of groups.
pub fn sliders(tunes: &[TuneValue], feel: &FeelFile) -> Vec<Slider> {
    let mut sliders: Vec<Slider> = tunes
        .iter()
        .map(|tune| {
            let about = feel.values.get(&tune.name).cloned().unwrap_or_default();
            let top = if tune.default.abs() > 0.0 {
                tune.default.abs() * 2.0
            } else {
                tune.value.abs().max(1.0) * 2.0
            };
            let from = about
                .from
                .unwrap_or(if tune.default < 0.0 { -top } else { 0.0 });
            let to = about.to.unwrap_or(top).max(from + f32::EPSILON);
            Slider {
                name: tune.name.clone(),
                label: about.label.unwrap_or_else(|| tidy(&tune.name)),
                group: about.group.unwrap_or_else(|| String::from("Values")),
                value: tune.value,
                default: tune.default,
                from,
                to,
                step: about.step.unwrap_or(0.0),
            }
        })
        .collect();
    let order: Vec<&String> = feel.values.keys().collect();
    // Described groups by name, then the rest ("Values"); within a group,
    // as the feel file lists them.
    sliders.sort_by_key(|slider| {
        (
            slider.group == "Values",
            slider.group.clone(),
            order
                .iter()
                .position(|name| **name == slider.name)
                .unwrap_or(usize::MAX),
            slider.name.clone(),
        )
    });
    sliders
}

/// The values in a tune file.
pub fn read_tune_file(path: &Path) -> BTreeMap<String, f32> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    let Ok(serde_json::Value::Object(values)) = serde_json::from_str(&text) else {
        return BTreeMap::new();
    };
    values
        .into_iter()
        .filter_map(|(name, value)| Some((name, value.as_f64()? as f32)))
        .collect()
}

/// Writes `values` into the tune file, keeping the others there, whole.
/// Returns the file's text before and after.
pub fn write_tune_values(
    path: &Path,
    values: &BTreeMap<String, f32>,
) -> Result<(String, String), Problem> {
    let before = std::fs::read_to_string(path).unwrap_or_default();
    let mut all = read_tune_file(path);
    all.extend(values.iter().map(|(name, value)| (name.clone(), *value)));
    let lines: Vec<String> = all
        .iter()
        .map(|(name, value)| {
            format!(
                "  {}: {}",
                serde_json::Value::from(name.as_str()),
                number(*value)
            )
        })
        .collect();
    let after = format!("{{\n{}\n}}\n", lines.join(",\n"));
    write_whole(path, &after)?;
    Ok((before, after))
}

/// A number as briefly as it can be written: `380`, not `380.0`.
fn number(value: f32) -> String {
    if value.fract() == 0.0 && value.abs() < 1e9 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// Where a value is asked for in the code, with a few lines around it.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeSpot {
    pub file: PathBuf,
    /// The line, counted from 1.
    pub line: usize,
    /// The lines around it, each with its number.
    pub lines: Vec<(usize, String)>,
}

/// Finds `tune("name"` in the game's code.
pub fn find_code(folder: &GamesFolder, game: &Game, name: &str) -> Option<CodeSpot> {
    let wanted = format!("tune(\"{name}\"");
    for file in folder.code_files(game) {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        if let Some(index) = lines.iter().position(|line| line.contains(&wanted)) {
            let from = index.saturating_sub(3);
            let to = (index + 4).min(lines.len());
            return Some(CodeSpot {
                file,
                line: index + 1,
                lines: (from..to).map(|i| (i + 1, lines[i].to_string())).collect(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tune(name: &str, value: f32, default: f32) -> TuneValue {
        TuneValue {
            name: name.to_string(),
            value,
            default,
            source: String::from("default"),
        }
    }

    #[test]
    fn every_value_is_a_slider_described_or_not() {
        let feel: FeelFile = serde_json::from_str(
            r#"{"values": {"jump_speed": {"label": "Jump height", "group": "Duck", "from": 150, "to": 500, "step": 5}},
                "presets": {"Floaty": {"gravity": 500}}}"#,
        )
        .unwrap();
        let sliders = sliders(
            &[
                tune("gravity", 900.0, 900.0),
                tune("jump_speed", 330.0, 330.0),
            ],
            &feel,
        );
        assert_eq!(sliders[0].label, "Jump height", "described ones first");
        assert_eq!(
            (sliders[0].from, sliders[0].to, sliders[0].step),
            (150.0, 500.0, 5.0)
        );
        assert_eq!(sliders[1].label, "Gravity");
        assert_eq!(
            (sliders[1].from, sliders[1].to),
            (0.0, 1800.0),
            "nothing to twice the default"
        );
        assert_eq!(feel.presets["Floaty"]["gravity"], 500.0);
        assert!(serde_json::from_str::<FeelFile>(r#"{"valuse": {}}"#).is_err());
        assert_eq!(tidy("run_speed"), "Run speed");
    }

    #[test]
    fn tune_values_are_written_whole_keeping_the_others() {
        let folder = crate::test_folder::TestFolder::new("feel");
        let path = folder.join("a.tune.json");
        std::fs::write(&path, "{\"gravity\": 900}\n").unwrap();
        let (before, after) = write_tune_values(
            &path,
            &BTreeMap::from([(String::from("jump_speed"), 380.5)]),
        )
        .unwrap();
        assert_eq!(before, "{\"gravity\": 900}\n");
        assert_eq!(
            after,
            "{\n  \"gravity\": 900,\n  \"jump_speed\": 380.5\n}\n"
        );
        assert_eq!(read_tune_file(&path)["jump_speed"], 380.5);
    }
}
