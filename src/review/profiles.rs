//! Platform profiles: what each app or screen covers, and how small text
//! can be there. They're TOML files (in `review_profiles/`) because the
//! sources disagree and the apps change: edit them, or add your own.
//!
//! ```toml
//! name = "TikTok"
//! aspect = "9:16"            # only videos this shape (within 3%) use it
//! canvas = [1080, 1920]      # the sizes below are on this canvas
//! screen = "phone"           # phone, desktop or tv: how small text may be
//! margin_top = 160           # strips the app covers, in canvas pixels
//! margin_bottom = 480
//! margin_left = 60
//! margin_right = 0
//! min_text = 0.014           # smallest text, as a share of the height (optional)
//! source = "where the numbers came from"
//! checked = "2026-11-15"
//!
//! [[covered]]                # anything else covered: x, y, width, height
//! name = "like, comment and share buttons"
//! rect = [940, 820, 140, 620]
//! paused_only = false        # true: only while paused (a warning, not a flag)
//! ```

use crate::math::Rect;
use std::path::{Path, PathBuf};

/// The kind of screen a profile is watched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Phone,
    Desktop,
    Tv,
}

impl Screen {
    /// The smallest comfortable text, as a share of the picture's height.
    pub fn min_text(self) -> f32 {
        match self {
            Screen::Phone => 0.014,
            Screen::Desktop => 0.011,
            Screen::Tv => 0.022,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Screen::Phone => "phone",
            Screen::Desktop => "desktop",
            Screen::Tv => "TV",
        }
    }
}

/// An area a profile covers.
#[derive(Debug, Clone, PartialEq)]
pub struct Covered {
    pub name: String,
    /// On the profile's canvas.
    pub rect: Rect,
    /// Only covered while the video is paused.
    pub paused_only: bool,
}

/// One platform or screen. See the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    /// Width over height.
    pub aspect: f32,
    pub canvas: (f32, f32),
    pub screen: Screen,
    /// Top, right, bottom, left, in canvas pixels.
    pub margins: [f32; 4],
    pub covered: Vec<Covered>,
    /// The smallest text, as a share of the height.
    pub min_text: f32,
    pub source: String,
    pub checked: String,
    /// The file it came from (or "built in").
    pub file: String,
}

impl Profile {
    /// Whether a `width` x `height` picture is this profile's shape.
    pub fn matches(&self, width: f32, height: f32) -> bool {
        height > 0.0 && ((width / height) / self.aspect - 1.0).abs() <= 0.03
    }

    /// Every covered area, margins included, scaled to a `width` x
    /// `height` picture: its name, rectangle, and whether only paused.
    pub fn covered_on(&self, width: f32, height: f32) -> Vec<(String, Rect, bool)> {
        let (sx, sy) = (width / self.canvas.0, height / self.canvas.1);
        let [top, right, bottom, left] = self.margins;
        let mut out = Vec::new();
        let mut strip = |name: &str, r: Rect| {
            if r.w > 0.0 && r.h > 0.0 {
                out.push((name.to_string(), r, false));
            }
        };
        strip("top edge", Rect::new(0.0, 0.0, width, top * sy));
        strip(
            "bottom edge",
            Rect::new(0.0, height - bottom * sy, width, bottom * sy),
        );
        strip("left edge", Rect::new(0.0, 0.0, left * sx, height));
        strip(
            "right edge",
            Rect::new(width - right * sx, 0.0, right * sx, height),
        );
        for c in &self.covered {
            out.push((
                c.name.clone(),
                Rect::new(c.rect.x * sx, c.rect.y * sy, c.rect.w * sx, c.rect.h * sy),
                c.paused_only,
            ));
        }
        out
    }

    /// A profile for a game's own window: nothing covered, desktop text.
    pub fn game_window(width: usize, height: usize) -> Profile {
        Profile {
            name: format!("this game's window ({width}x{height})"),
            aspect: width as f32 / height.max(1) as f32,
            canvas: (width as f32, height as f32),
            screen: Screen::Desktop,
            margins: [0.0; 4],
            covered: Vec::new(),
            min_text: Screen::Desktop.min_text(),
            source: String::from("the game's Config"),
            checked: String::new(),
            file: String::from("built in"),
        }
    }

    /// Reads a profile from TOML text.
    pub fn parse(text: &str, file: &str) -> Result<Profile, String> {
        let tables = parse_toml(text).map_err(|e| format!("{file}: {e}"))?;
        let top = &tables[0].1;
        let get = |key: &str| top.iter().find(|(k, _)| k == key).map(|(_, v)| v);
        let number = |key: &str| -> Result<f32, String> {
            match get(key) {
                None => Ok(0.0),
                Some(Value::Number(n)) => Ok(*n),
                Some(_) => Err(format!("{file}: {key} must be a number")),
            }
        };
        let text_of = |key: &str| match get(key) {
            Some(Value::Text(t)) => t.clone(),
            _ => String::new(),
        };
        let name = match get("name") {
            Some(Value::Text(t)) if !t.trim().is_empty() => t.clone(),
            _ => return Err(format!("{file}: needs a name (name = \"...\")")),
        };
        let canvas = match get("canvas") {
            Some(Value::Numbers(n)) if n.len() == 2 && n[0] > 0.0 && n[1] > 0.0 => (n[0], n[1]),
            _ => return Err(format!("{file}: needs canvas = [width, height]")),
        };
        let aspect = match get("aspect") {
            None => canvas.0 / canvas.1,
            Some(Value::Text(t)) => parse_aspect(t).ok_or(format!(
                "{file}: aspect must look like \"9:16\", not \"{t}\""
            ))?,
            Some(Value::Number(n)) if *n > 0.0 => *n,
            Some(_) => return Err(format!("{file}: aspect must look like \"9:16\"")),
        };
        let screen = match text_of("screen").to_lowercase().as_str() {
            "phone" | "" => Screen::Phone,
            "desktop" => Screen::Desktop,
            "tv" => Screen::Tv,
            other => {
                return Err(format!(
                    "{file}: screen must be phone, desktop or tv, not \"{other}\""
                ));
            }
        };
        let margins = [
            number("margin_top")?,
            number("margin_right")?,
            number("margin_bottom")?,
            number("margin_left")?,
        ];
        let min_text = match number("min_text")? {
            n if n > 0.0 => n,
            _ => screen.min_text(),
        };
        let mut covered = Vec::new();
        for (table, entries) in &tables[1..] {
            if table != "covered" {
                return Err(format!(
                    "{file}: only [[covered]] tables are known, not [[{table}]]"
                ));
            }
            let rect = match entries.iter().find(|(k, _)| k == "rect").map(|(_, v)| v) {
                Some(Value::Numbers(n)) if n.len() == 4 => Rect::new(n[0], n[1], n[2], n[3]),
                _ => {
                    return Err(format!(
                        "{file}: each [[covered]] needs rect = [x, y, width, height]"
                    ));
                }
            };
            let name = match entries.iter().find(|(k, _)| k == "name").map(|(_, v)| v) {
                Some(Value::Text(t)) => t.clone(),
                _ => String::from("covered area"),
            };
            let paused_only = matches!(
                entries
                    .iter()
                    .find(|(k, _)| k == "paused_only")
                    .map(|(_, v)| v),
                Some(Value::Bool(true))
            );
            covered.push(Covered {
                name,
                rect,
                paused_only,
            });
        }
        Ok(Profile {
            name,
            aspect,
            canvas,
            screen,
            margins,
            covered,
            min_text,
            source: text_of("source"),
            checked: text_of("checked"),
            file: file.to_string(),
        })
    }
}

/// `"9:16"` or `"19.5:9"` as width over height.
fn parse_aspect(text: &str) -> Option<f32> {
    let (w, h) = text.split_once(':')?;
    let (w, h): (f32, f32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (w > 0.0 && h > 0.0).then_some(w / h)
}

/// The profiles that ship with Duck Forge.
const BUILT_IN: [(&str, &str); 10] = [
    (
        "tiktok.toml",
        include_str!("../../review_profiles/tiktok.toml"),
    ),
    (
        "instagram_reels.toml",
        include_str!("../../review_profiles/instagram_reels.toml"),
    ),
    (
        "youtube_shorts.toml",
        include_str!("../../review_profiles/youtube_shorts.toml"),
    ),
    (
        "vertical_common.toml",
        include_str!("../../review_profiles/vertical_common.toml"),
    ),
    (
        "youtube_tv_16x9.toml",
        include_str!("../../review_profiles/youtube_tv_16x9.toml"),
    ),
    (
        "square_feed.toml",
        include_str!("../../review_profiles/square_feed.toml"),
    ),
    (
        "feed_4x5.toml",
        include_str!("../../review_profiles/feed_4x5.toml"),
    ),
    (
        "desktop_16x9.toml",
        include_str!("../../review_profiles/desktop_16x9.toml"),
    ),
    (
        "ultrawide_21x9.toml",
        include_str!("../../review_profiles/ultrawide_21x9.toml"),
    ),
    (
        "phone_landscape_notch.toml",
        include_str!("../../review_profiles/phone_landscape_notch.toml"),
    ),
];

/// Every profile: the built-in ones, then the `.toml` files in `folder`
/// (`--profiles DIR`, or `review_profiles/` here if there is one), a
/// file with a built-in one's name replacing it. Also says what couldn't
/// be read.
pub fn load(folder: Option<&Path>) -> (Vec<Profile>, Vec<String>) {
    let mut found: Vec<(String, Profile)> = Vec::new();
    let mut problems = Vec::new();
    for (name, text) in BUILT_IN {
        match Profile::parse(text, &format!("built in {name}")) {
            Ok(p) => found.push((name.to_string(), p)),
            Err(e) => problems.push(e),
        }
    }
    let default = PathBuf::from("review_profiles");
    let folder = folder
        .map(Path::to_path_buf)
        .or(default.is_dir().then_some(default));
    if let Some(folder) = folder {
        match std::fs::read_dir(&folder) {
            Ok(entries) => {
                let mut files: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| p.extension().is_some_and(|x| x == "toml"))
                    .collect();
                files.sort();
                for path in files {
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let text = match std::fs::read_to_string(&path) {
                        Ok(t) => t,
                        Err(e) => {
                            problems.push(format!("could not read {}: {e}", path.display()));
                            continue;
                        }
                    };
                    match Profile::parse(&text, &path.display().to_string()) {
                        Ok(p) => match found.iter_mut().find(|(n, _)| *n == name) {
                            Some(slot) => slot.1 = p,
                            None => found.push((name, p)),
                        },
                        Err(e) => problems.push(e),
                    }
                }
            }
            Err(e) => problems.push(format!(
                "could not read the folder {}: {e}",
                folder.display()
            )),
        }
    }
    (found.into_iter().map(|(_, p)| p).collect(), problems)
}

/// The profiles a `width` x `height` picture is judged by: those its
/// shape matches, scaled to it. A game with none gets its own window.
pub fn matching(all: &[Profile], width: usize, height: usize, game: bool) -> Vec<Profile> {
    let mut out: Vec<Profile> = all
        .iter()
        .filter(|p| p.matches(width as f32, height as f32))
        .cloned()
        .collect();
    if out.is_empty() && game {
        out.push(Profile::game_window(width, height));
    }
    out
}

/// A value in the small part of TOML profiles use.
#[derive(Debug, Clone, PartialEq)]
enum Value {
    Text(String),
    Number(f32),
    Numbers(Vec<f32>),
    Bool(bool),
}

/// The tables in `text`: the top one ("") first, then each `[[name]]`,
/// with their keys and values in order.
#[allow(clippy::type_complexity)]
fn parse_toml(text: &str) -> Result<Vec<(String, Vec<(String, Value)>)>, String> {
    let mut tables: Vec<(String, Vec<(String, Value)>)> = vec![(String::new(), Vec::new())];
    for (n, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim().to_string();
        if line.is_empty() {
            continue;
        }
        let at = n + 1;
        if let Some(name) = line.strip_prefix("[[").and_then(|l| l.strip_suffix("]]")) {
            tables.push((name.trim().to_string(), Vec::new()));
            continue;
        }
        if line.starts_with('[') {
            return Err(format!("line {at}: only [[covered]] tables are known"));
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {at}: expected key = value"));
        };
        let key = key.trim().trim_matches('"').to_string();
        let value = value.trim();
        let parsed = if let Some(t) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            Value::Text(t.replace("\\\"", "\"").replace("\\\\", "\\"))
        } else if value == "true" || value == "false" {
            Value::Bool(value == "true")
        } else if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let numbers: Result<Vec<f32>, _> = inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.replace('_', "").parse::<f32>())
                .collect();
            Value::Numbers(
                numbers.map_err(|_| format!("line {at}: {key} must be a list of numbers"))?,
            )
        } else {
            match value.replace('_', "").parse::<f32>() {
                Ok(n) if n.is_finite() => Value::Number(n),
                _ => {
                    return Err(format!(
                        "line {at}: can't read the value of {key} ({value})"
                    ));
                }
            }
        };
        tables
            .last_mut()
            .expect("the top table")
            .1
            .push((key, parsed));
    }
    Ok(tables)
}

/// The line without a `#` comment (outside quotes).
fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => return &line[..i],
            _ => {}
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_profile_reads() {
        let (all, problems) = load(Some(Path::new("no such folder for this test")));
        assert_eq!(all.len(), BUILT_IN.len());
        assert_eq!(problems.len(), 1, "{problems:?}"); // only the missing folder
        let tiktok = all.iter().find(|p| p.name == "TikTok").unwrap();
        assert!(tiktok.matches(1080.0, 1920.0));
        assert!(tiktok.matches(720.0, 1280.0));
        assert!(!tiktok.matches(1080.0, 1350.0));
        assert_eq!(tiktok.screen, Screen::Phone);
        assert_eq!(tiktok.covered.len(), 1);
        assert!(!tiktok.source.is_empty() && !tiktok.checked.is_empty());
    }

    #[test]
    fn profiles_scale_to_the_video() {
        let (all, _) = load(None);
        let tiktok = all.iter().find(|p| p.name == "TikTok").unwrap();
        // Half size: the bottom strip is 240 px, the buttons start at x 470.
        let areas = tiktok.covered_on(540.0, 960.0);
        let bottom = areas.iter().find(|(n, ..)| n == "bottom edge").unwrap();
        assert_eq!(bottom.1, Rect::new(0.0, 720.0, 540.0, 240.0));
        let buttons = areas.iter().find(|(n, ..)| n.contains("buttons")).unwrap();
        assert_eq!(buttons.1.x, 470.0);
        let vertical = matching(&all, 1080, 1920, false);
        assert_eq!(
            vertical.len(),
            4,
            "TikTok, Reels, Shorts and the common area"
        );
        let tv = matching(&all, 1280, 720, false);
        assert!(tv.iter().any(|p| p.screen == Screen::Tv));
        // A game of a shape no file knows is judged in its own window.
        let odd = matching(&all, 300, 200, true);
        assert_eq!(odd.len(), 1);
        assert!(odd[0].name.contains("300x200"));
    }

    #[test]
    fn a_users_file_adds_or_replaces() {
        let folder =
            std::env::temp_dir().join(format!("duckforge-profiles-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("tiktok.toml"),
            "name = \"TikTok 2027\"\naspect = \"9:16\"\ncanvas = [1080, 1920]\nmargin_bottom = 660 # the ad template\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("kiosk.toml"),
            "name = \"kiosk\"\ncanvas = [768, 1024]\nscreen = \"tv\"\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("broken.toml"),
            "name = \"broken\"\ncanvas = [1, 2, 3]\n",
        )
        .unwrap();
        let (all, problems) = load(Some(&folder));
        let _ = std::fs::remove_dir_all(&folder);
        assert!(
            all.iter()
                .any(|p| p.name == "TikTok 2027" && p.margins[2] == 660.0)
        );
        assert!(!all.iter().any(|p| p.name == "TikTok"));
        assert!(
            all.iter()
                .any(|p| p.name == "kiosk" && p.screen == Screen::Tv)
        );
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("canvas"), "{problems:?}");
    }
}
