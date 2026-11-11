//! Tile maps: a level written as text, one letter per tile.
//!
//! ```text
//! ..c.........c.......
//! ........=====.......
//! p.....e.........###g
//! ####################
//! ```
//!
//! A [`Legend`] says what each letter is: a solid tile, a one-way platform
//! (stand on it, jump up through it), a tile that's only decoration, or a
//! spawn point (where the player, coins or enemies start). `.` and space
//! are always empty.
//!
//! ```
//! use duckforge::prelude::*;
//!
//! const LEVEL: &str = "
//! ..c.....
//! p..===..
//! ########";
//!
//! # fn main() -> Result<(), duckforge::LoadError> {
//! let legend = Legend::new()
//!     .solid('#', "ground")
//!     .one_way('=', "plank")
//!     .spawn('p', "player")
//!     .spawn('c', "coin");
//! let map = TileMap::parse(LEVEL, 16.0, &legend)?;
//! let start = map.spawn("player"); // the bottom centre of its tile
//! assert_eq!(start, vec2(8.0, 32.0));
//! # Ok(())
//! # }
//! ```
//!
//! Then, every update, [`TileMap::move_and_slide`] moves a body through the
//! level and says what it touched, and in `draw`, between `push_camera`
//! and `pop_camera`, [`TileMap::draw`] draws the tiles the camera sees.
//!
//! ## A level as two files
//!
//! A level can also live next to the game, where Duck Forge Studio (and
//! anyone with a text editor) can change it: the letters in `level.txt`,
//! and what they mean in `level.legend.json`:
//!
//! ```json
//! { "tile_size": 16,
//!   "tiles": "assets/tiles.json",
//!   "letters": {
//!     "#": {"kind": "solid",      "name": "grass"},
//!     "=": {"kind": "one_way",    "name": "plank"},
//!     "~": {"kind": "decoration", "name": "water"},
//!     "p": {"kind": "thing", "name": "player", "label": "Start",
//!           "at_least": 1, "at_most": 1, "start": true},
//!     "c": {"kind": "thing", "name": "coin", "label": "Coin"} } }
//! ```
//!
//! [`TileMap::load_level`] reads both. A "thing" is a spawn point. `tiles`
//! (optional) is the sprite sheet with the tiles' pictures, from the
//! legend's folder; `label` is what the Studio calls a thing; `at_least`
//! and `at_most` are rules [`TileMap::broken_rules`] checks; `start`
//! marks where the player begins. The engine notes the files it read, so
//! when one changes while the game runs, the game's
//! [`reload`](crate::engine::Game::reload) hook is called.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::canvas::Canvas;
use crate::error::LoadError;
use crate::math::{Rect, Vec2, vec2};
use crate::sprite::Blit;
use crate::tileset::Tileset;

/// What a tile does to things moving through the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    /// Nothing stops here (also decoration tiles, which are drawn).
    Empty,
    /// Walls, floors and ceilings: nothing passes through.
    Solid,
    /// A platform: things land on it from above, but pass through it going
    /// up or sideways.
    OneWay,
}

/// What one letter of a level means.
#[derive(Debug, Clone, PartialEq)]
enum Meaning {
    Tile(String, TileKind),
    Spawn(String),
}

/// What kind of thing a letter is, as a legend file says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LetterKind {
    Solid,
    OneWay,
    Decoration,
    /// A spawn point: where the player, a coin or an enemy starts.
    Thing,
}

impl LetterKind {
    /// The word a legend file uses: `solid`, `one_way`, `decoration` or
    /// `thing`.
    pub fn word(self) -> &'static str {
        match self {
            LetterKind::Solid => "solid",
            LetterKind::OneWay => "one_way",
            LetterKind::Decoration => "decoration",
            LetterKind::Thing => "thing",
        }
    }
}

/// One letter of a [`Legend`], with what a legend file can say about it.
#[derive(Debug, Clone, PartialEq)]
pub struct Letter {
    pub letter: char,
    pub kind: LetterKind,
    /// The tile's name in the tileset, or the thing's spawn name.
    pub name: String,
    /// What Duck Forge Studio calls it ("Start", "Coin"). `None` uses the
    /// name.
    pub label: Option<String>,
    /// The level needs at least this many.
    pub at_least: Option<u32>,
    /// The level can have at most this many.
    pub at_most: Option<u32>,
    /// It's where the player starts.
    pub start: bool,
}

impl Letter {
    /// Its label, or its name with the first letter made big.
    pub fn title(&self) -> String {
        match &self.label {
            Some(label) => label.clone(),
            None => {
                let mut chars = self.name.chars();
                match chars.next() {
                    Some(first) => first
                        .to_uppercase()
                        .chain(chars)
                        .collect::<String>()
                        .replace('_', " "),
                    None => String::new(),
                }
            }
        }
    }
}

/// What a legend file says about a letter beyond its meaning.
#[derive(Debug, Clone, Default)]
struct Extra {
    letter: char,
    label: Option<String>,
    at_least: Option<u32>,
    at_most: Option<u32>,
    start: bool,
}

/// What each letter of a level means. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Legend {
    letters: Vec<(char, Meaning)>,
    /// What a legend file says beyond the meaning (labels, rules).
    extra: Vec<Extra>,
    /// From a legend file: how big a tile is, and the tiles' sprite sheet.
    tile_size: Option<f32>,
    tiles: Option<PathBuf>,
}

impl Legend {
    /// A legend where only `.` and space mean something (empty).
    pub fn new() -> Legend {
        Legend::default()
    }

    /// `letter` is a solid tile, drawn with the tileset's tile `name`.
    pub fn solid(self, letter: char, name: &str) -> Legend {
        self.with(letter, Meaning::Tile(name.to_string(), TileKind::Solid))
    }

    /// `letter` is a one-way platform: things land on it from above and
    /// pass through it going up.
    pub fn one_way(self, letter: char, name: &str) -> Legend {
        self.with(letter, Meaning::Tile(name.to_string(), TileKind::OneWay))
    }

    /// `letter` is a tile that's only drawn (a window, grass, a sign):
    /// nothing bumps into it.
    pub fn decoration(self, letter: char, name: &str) -> Legend {
        self.with(letter, Meaning::Tile(name.to_string(), TileKind::Empty))
    }

    /// `letter` isn't a tile but a place where something starts, read back
    /// with [`TileMap::spawn`] or [`TileMap::spawns`]. Its tile is empty.
    pub fn spawn(self, letter: char, name: &str) -> Legend {
        self.with(letter, Meaning::Spawn(name.to_string()))
    }

    /// Reads a legend file (see the [module docs](self)). Errors say
    /// what's wrong in plain words.
    pub fn load(path: impl AsRef<Path>) -> Result<Legend, LoadError> {
        let path = path.as_ref();
        let text = crate::files::read_text(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "legend", &error))?;
        let mut legend = Legend::from_json(&text).map_err(|message| {
            LoadError::new(
                io::ErrorKind::InvalidData,
                format!("{}: {message}", path.display()),
            )
        })?;
        if let Some(tiles) = &legend.tiles {
            let folder = path.parent().unwrap_or(Path::new(""));
            legend.tiles = Some(folder.join(tiles));
        }
        Ok(legend)
    }

    /// Reads a legend from JSON text, as in a legend file.
    pub fn from_json(text: &str) -> Result<Legend, String> {
        let value: Value =
            serde_json::from_str(text).map_err(|error| format!("not valid JSON: {error}"))?;
        let Value::Object(top) = value else {
            return Err(String::from(
                "a legend is a JSON object like {\"letters\": {\"#\": ...}}",
            ));
        };
        if let Some(name) = top
            .keys()
            .find(|name| !["tile_size", "tiles", "letters"].contains(&name.as_str()))
        {
            return Err(format!(
                "unknown field {name:?} (a legend has tile_size, tiles and letters)"
            ));
        }
        let mut legend = Legend::new();
        if let Some(size) = top.get("tile_size") {
            match size.as_f64() {
                Some(size) if size > 0.0 => legend.tile_size = Some(size as f32),
                _ => {
                    return Err(String::from(
                        "\"tile_size\" must be a positive number, like 16",
                    ));
                }
            }
        }
        if let Some(tiles) = top.get("tiles") {
            let tiles = tiles
                .as_str()
                .ok_or("\"tiles\" names the tiles' sprite sheet, like \"assets/tiles.json\"")?;
            legend.tiles = Some(PathBuf::from(tiles));
        }
        let Some(Value::Object(letters)) = top.get("letters") else {
            return Err(String::from(
                "a legend needs \"letters\": an object saying what each letter means",
            ));
        };
        for (key, about) in letters {
            let mut chars = key.chars();
            let (Some(letter), None) = (chars.next(), chars.next()) else {
                return Err(format!("{key:?}: each key is one letter"));
            };
            if letter == '.' || letter == ' ' {
                return Err(format!(
                    "{key:?} always means an empty tile; pick another letter"
                ));
            }
            let Value::Object(about) = about else {
                return Err(format!(
                    "{key:?} needs an object like {{\"kind\": \"solid\", \"name\": \"grass\"}}"
                ));
            };
            if let Some(field) = about.keys().find(|field| {
                !["kind", "name", "label", "at_least", "at_most", "start"].contains(&field.as_str())
            }) {
                return Err(format!(
                    "{key:?} has an unknown field {field:?} (allowed: kind, name, label, \
                     at_least, at_most, start)"
                ));
            }
            let name = about
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| format!("{key:?} needs a \"name\""))?;
            let kind = match about.get("kind").and_then(Value::as_str) {
                Some("solid") => LetterKind::Solid,
                Some("one_way") => LetterKind::OneWay,
                Some("decoration") => LetterKind::Decoration,
                Some("thing") => LetterKind::Thing,
                other => {
                    return Err(format!(
                        "{key:?}: \"kind\" is {}, but it must be solid, one_way, decoration \
                         or thing",
                        other.map_or(String::from("missing"), |kind| format!("{kind:?}"))
                    ));
                }
            };
            let count = |field: &str| -> Result<Option<u32>, String> {
                match about.get(field) {
                    None => Ok(None),
                    Some(value) => value
                        .as_u64()
                        .map(|n| Some(n as u32))
                        .ok_or_else(|| format!("{key:?}: \"{field}\" must be a whole number")),
                }
            };
            let label = match about.get("label") {
                None => None,
                Some(label) => Some(
                    label
                        .as_str()
                        .ok_or_else(|| format!("{key:?}: \"label\" must be text"))?
                        .to_string(),
                ),
            };
            let start = about.get("start").and_then(Value::as_bool).unwrap_or(false);
            if legend.letters.iter().any(|(l, _)| *l == letter) {
                return Err(format!("the legend has {key:?} twice"));
            }
            let meaning = match kind {
                LetterKind::Solid => Meaning::Tile(name.to_string(), TileKind::Solid),
                LetterKind::OneWay => Meaning::Tile(name.to_string(), TileKind::OneWay),
                LetterKind::Decoration => Meaning::Tile(name.to_string(), TileKind::Empty),
                LetterKind::Thing => Meaning::Spawn(name.to_string()),
            };
            legend.letters.push((letter, meaning));
            legend.extra.push(Extra {
                letter,
                label,
                at_least: count("at_least")?,
                at_most: count("at_most")?,
                start,
            });
        }
        Ok(legend)
    }

    /// Every letter it knows: the tiles (solid, one-way, decoration), then
    /// the things, each in the order they were given (a legend file's in
    /// the letters' order).
    pub fn letters(&self) -> Vec<Letter> {
        let mut letters: Vec<Letter> = self
            .letters
            .iter()
            .map(|(letter, meaning)| {
                let (kind, name) = match meaning {
                    Meaning::Tile(name, TileKind::Solid) => (LetterKind::Solid, name),
                    Meaning::Tile(name, TileKind::OneWay) => (LetterKind::OneWay, name),
                    Meaning::Tile(name, TileKind::Empty) => (LetterKind::Decoration, name),
                    Meaning::Spawn(name) => (LetterKind::Thing, name),
                };
                let extra = self
                    .extra
                    .iter()
                    .find(|extra| extra.letter == *letter)
                    .cloned()
                    .unwrap_or_default();
                Letter {
                    letter: *letter,
                    kind,
                    name: name.clone(),
                    label: extra.label,
                    at_least: extra.at_least,
                    at_most: extra.at_most,
                    start: extra.start,
                }
            })
            .collect();
        letters.sort_by_key(|letter| letter.kind as u8); // stable: keeps the order within a kind
        letters
    }

    /// The tile size a legend file gives, if it gives one.
    pub fn tile_size(&self) -> Option<f32> {
        self.tile_size
    }

    /// The sprite sheet with the tiles' pictures, if a legend file names
    /// one (from the folder the game runs in).
    pub fn tiles_file(&self) -> Option<&Path> {
        self.tiles.as_deref()
    }

    fn with(mut self, letter: char, meaning: Meaning) -> Legend {
        assert!(
            letter != '.' && letter != ' ',
            "'{letter}' always means an empty tile; pick another letter"
        );
        assert!(
            !self.letters.iter().any(|(l, _)| *l == letter),
            "the legend has '{letter}' twice"
        );
        self.letters.push((letter, meaning));
        self
    }

    fn meaning(&self, letter: char) -> Option<&Meaning> {
        self.letters
            .iter()
            .find(|(l, _)| *l == letter)
            .map(|(_, meaning)| meaning)
    }

    /// The letters it knows, for error messages: `# = p c . and space`.
    fn known(&self) -> String {
        let mut letters: Vec<String> = self.letters.iter().map(|(l, _)| l.to_string()).collect();
        letters.push(String::from(". and space"));
        letters.join(" ")
    }
}

/// What [`TileMap::move_and_slide`] bumped into.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Contacts {
    /// It landed on something (solid, or a one-way platform from above).
    pub ground: bool,
    /// It hit its head.
    pub ceiling: bool,
    /// It was stopped going left.
    pub wall_left: bool,
    /// It was stopped going right.
    pub wall_right: bool,
}

/// A level made of tiles. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct TileMap {
    tile_size: f32,
    columns: usize,
    rows: usize,
    /// One entry per tile, row by row: an index into `tiles`, or `None`.
    cells: Vec<Option<usize>>,
    /// Every kind of tile the legend has: its name and what it does.
    tiles: Vec<(String, TileKind)>,
    /// Every spawn point: its name, and its tile's bottom centre.
    spawns: Vec<(String, Vec2)>,
    /// What each letter means.
    legend: Legend,
    /// The files it was read from (the level, then its legend), named as
    /// the engine names files: `examples/mygame/level.txt`.
    files: Vec<String>,
}

/// Where the things called one name are added and taken away between two
/// versions of a level: see [`TileMap::spawn_changes`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpawnChanges {
    pub added: Vec<Vec2>,
    pub removed: Vec<Vec2>,
}

/// How a character jumps, for [`TileMap::reach`]: the same numbers a
/// platformer's code uses (usually its `ctx.tune` values).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Jump {
    /// How fast it leaves the ground, upwards (world units a second).
    pub speed: f32,
    /// How fast it falls faster (world units a second, a second).
    pub gravity: f32,
    /// How fast it runs (world units a second).
    pub run: f32,
    /// How wide it is (world units), so it isn't sent through gaps it
    /// can't fit, or to jump from under a ledge its shoulder would hit
    /// (with a little to spare). 0 follows a point.
    pub width: f32,
}

/// Where a character can get to in a level: see [`TileMap::reach`].
#[derive(Debug, Clone, Default)]
pub struct Reach {
    tile_size: f32,
    /// Every tile it can be in.
    cells: HashSet<(i32, i32)>,
}

impl Reach {
    /// Can it get to the thing at `point` (a spawn point: the bottom
    /// centre of its tile)?
    pub fn can_reach(&self, point: Vec2) -> bool {
        let cell = (
            (point.x / self.tile_size).floor() as i32,
            ((point.y - 0.5) / self.tile_size).floor() as i32,
        );
        self.cells.contains(&cell)
    }

    /// Which of `points` it can't get to.
    pub fn missing(&self, points: &[Vec2]) -> Vec<Vec2> {
        points
            .iter()
            .copied()
            .filter(|point| !self.can_reach(*point))
            .collect()
    }

    /// Can it be in the tile at `column`, `row`?
    pub fn has(&self, column: i32, row: i32) -> bool {
        self.cells.contains(&(column, row))
    }
}

/// One step of a [`route`](TileMap::route): from standing in one tile to
/// standing in another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hop {
    /// Where its feet are before: the bottom centre of the tile it stands
    /// in.
    pub from: Vec2,
    /// Where its feet are after: the bottom centre of the tile it lands in.
    pub to: Vec2,
    /// It jumps. (Otherwise it walks there, or walks off an edge and
    /// falls.)
    pub jump: bool,
    /// How fast it moves sideways on the way (left is negative): a part
    /// of [`Jump::run`], or nothing for straight up or straight down.
    pub speed: f32,
}

/// What an arc passed through (every tile it was in), and the tile it
/// landed in, if it landed.
type Flown = (Vec<(i32, i32)>, Option<(i32, i32)>);

/// One thing a character can do from a tile, and where it takes it.
struct Flight {
    hop: Hop,
    /// Every tile it's in on the way.
    cells: Vec<(i32, i32)>,
    /// The tile it lands in, if it lands.
    landed: Option<(i32, i32)>,
}

/// How far inside a tile's edge something must be to count as in it: so
/// standing exactly on a floor isn't touching the floor's row, and a
/// rounding error of a millionth of a pixel can't catch on a wall.
const EDGE: f32 = 0.001;

impl TileMap {
    /// Reads a level from text, one row per line, one letter per tile,
    /// each tile `tile_size` world units (pixels, without zoom) square.
    /// Blank lines at the start and end are ignored, so a level can start
    /// on the line after `"`. Rows can be different lengths: the missing
    /// tiles are empty.
    ///
    /// A letter that isn't in the legend is an error naming its row and
    /// column (both counted from 1): `row 3, column 17: 'x' isn't in the
    /// legend (it has # = p c . and space)`.
    pub fn parse(text: &str, tile_size: f32, legend: &Legend) -> Result<TileMap, LoadError> {
        TileMap::read(text, tile_size, legend, "")
    }

    /// Reads a level from a text file, like [`parse`](TileMap::parse).
    /// Errors name the file too: `levels/1.txt row 3, column 17: ...`.
    pub fn load(
        path: impl AsRef<Path>,
        tile_size: f32,
        legend: &Legend,
    ) -> Result<TileMap, LoadError> {
        let path = path.as_ref();
        let text = crate::files::read_text(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "level", &error))?;
        let mut map = TileMap::read(&text, tile_size, legend, &format!("{} ", path.display()))?;
        map.files = vec![crate::files::name_of(path)];
        Ok(map)
    }

    /// Reads a level and its legend from two files next to each other:
    /// `level.txt` (the letters) and `level.legend.json` (what they mean,
    /// and the tile size). See the [module docs](self).
    ///
    /// ```no_run
    /// # use duckforge::prelude::*;
    /// # fn main() -> Result<(), duckforge::LoadError> {
    /// let map = TileMap::load_level("examples/mygame/level.txt")?;
    /// let start = map.spawn("player");
    /// # Ok(())
    /// # }
    /// ```
    pub fn load_level(path: impl AsRef<Path>) -> Result<TileMap, LoadError> {
        let path = path.as_ref();
        let legend_path = TileMap::legend_file_for(path);
        let legend = Legend::load(&legend_path)?;
        let tile_size = legend.tile_size.unwrap_or(16.0);
        let mut map = TileMap::load(path, tile_size, &legend)?;
        map.files.push(crate::files::name_of(&legend_path));
        Ok(map)
    }

    /// The legend file that goes with a level file: `level.txt` has
    /// `level.legend.json`.
    pub fn legend_file_for(level: &Path) -> PathBuf {
        let stem = level.file_stem().map_or(String::from("level"), |stem| {
            stem.to_string_lossy().into_owned()
        });
        level.with_file_name(format!("{stem}.legend.json"))
    }

    /// Was this map read from `file` (its level or its legend file)? For a
    /// game's [`reload`](crate::engine::Game::reload):
    ///
    /// ```ignore
    /// fn reload(&mut self, ctx: &mut Context, file: &Path) -> bool {
    ///     if !self.map.reads_from(file) {
    ///         return false;
    ///     }
    ///     // read the level again, keeping the player where it is
    /// }
    /// ```
    pub fn reads_from(&self, file: &Path) -> bool {
        let name = crate::files::name_of(file);
        self.files.contains(&name)
    }

    /// The files it was read from: the level, then its legend (empty for a
    /// map made with [`parse`](TileMap::parse)).
    pub fn files(&self) -> &[String] {
        &self.files
    }

    /// What each letter means.
    pub fn legend(&self) -> &Legend {
        &self.legend
    }

    /// Where the things called `name` were added and taken away in
    /// `newer`, a new version of this level. For a game's
    /// [`reload`](crate::engine::Game::reload), to keep what's been
    /// collected: take away the removed coins, add the new ones.
    pub fn spawn_changes(&self, newer: &TileMap, name: &str) -> SpawnChanges {
        let old = self.spawns(name);
        let new = newer.spawns(name);
        SpawnChanges {
            added: new.iter().copied().filter(|at| !old.contains(at)).collect(),
            removed: old.iter().copied().filter(|at| !new.contains(at)).collect(),
        }
    }

    /// The legend's rules the level breaks, in plain words: "The level
    /// needs at least 1 Flag; it has none." Empty if it keeps them all.
    /// The rules are a legend file's `at_least` and `at_most`.
    pub fn broken_rules(&self) -> Vec<String> {
        let mut broken = Vec::new();
        for letter in self.legend.letters() {
            let count = match letter.kind {
                LetterKind::Thing => self.spawns(&letter.name).len() as u32,
                _ => {
                    let tile = self.tiles.iter().position(|(name, _)| *name == letter.name);
                    self.cells
                        .iter()
                        .filter(|cell| cell.is_some() && *cell == &tile)
                        .count() as u32
                }
            };
            let title = letter.title();
            let has = match count {
                0 => String::from("none"),
                n => n.to_string(),
            };
            if let Some(least) = letter.at_least
                && count < least
            {
                broken.push(format!(
                    "The level needs at least {least} {title}; it has {has}."
                ));
            }
            if let Some(most) = letter.at_most
                && count > most
            {
                broken.push(format!(
                    "The level can have at most {most} {title}; it has {has}."
                ));
            }
        }
        broken
    }

    /// Where a platformer's character can get to, starting with its feet
    /// at `start`, running and jumping as `jump` says. It's cautious:
    /// it tries jumps at a few speeds, left and right, and walking off
    /// edges, following the arc a point would take. So something it can't
    /// reach is *probably* out of reach: a warning to look at, not proof.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let legend = Legend::new().solid('#', "ground").spawn('c', "coin").spawn('p', "player");
    /// let level = "c........\n.........\n.........\n.........\n.........\n.p......c\n#########";
    /// let map = TileMap::parse(level, 16.0, &legend).unwrap();
    /// // A jump about 60 pixels high, nearly four tiles.
    /// let jump = Jump { speed: 330.0, gravity: 900.0, run: 110.0, width: 10.0 };
    /// let reach = map.reach(map.spawn("player"), jump);
    /// let coins = map.spawns("coin");
    /// assert!(!reach.can_reach(coins[0]), "five tiles up is too high");
    /// assert!(reach.can_reach(coins[1]), "a walk away");
    /// ```
    pub fn reach(&self, start: Vec2, jump: Jump) -> Reach {
        let mut reach = Reach {
            tile_size: self.tile_size,
            cells: HashSet::new(),
        };
        let Some(first) = self.land_below(start) else {
            return reach;
        };
        let mut queue = vec![first];
        let mut standing: HashSet<(i32, i32)> = HashSet::from([first]);
        while let Some(cell) = queue.pop() {
            reach.cells.insert(cell);
            for flight in self.flights_from(cell, jump) {
                reach.cells.extend(flight.cells);
                if let Some(landed) = flight.landed
                    && standing.insert(landed)
                {
                    queue.push(landed);
                }
            }
        }
        reach
    }

    /// The way from standing at `start` to the thing at `goal` (a spawn
    /// point), as [`Hop`]s from one tile to stand in to the next: the
    /// fewest hops, each one a walk to the next tile, a walk off an edge,
    /// or a jump, landing somewhere it can stand. The last hop passes
    /// through the goal's tile. It only counts on jumps with a little to
    /// spare (one a tenth lower must land on the same level too). `None`
    /// if there's no such way: [`reach`](TileMap::reach) can't get there,
    /// or only by a jump with nothing to spare, or by falling out of the
    /// level. A bot follows it: the starters' `bot.rs` do.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let legend = Legend::new().solid('#', "ground").one_way('=', "plank").spawn('c', "coin").spawn('p', "player");
    /// let level = ".....c...\n....==...\n.........\n.p.......\n#########";
    /// let map = TileMap::parse(level, 16.0, &legend).unwrap();
    /// let jump = Jump { speed: 330.0, gravity: 900.0, run: 110.0, width: 10.0 };
    /// let route = map.route(map.spawn("player"), map.spawn("coin"), jump).unwrap();
    /// let last = route.last().unwrap();
    /// assert!(last.jump, "up onto the plank");
    /// assert_eq!(last.to.y, 16.0, "standing on it, by the coin");
    /// ```
    pub fn route(&self, start: Vec2, goal: Vec2, jump: Jump) -> Option<Vec<Hop>> {
        let goal = self.cell_at(vec2(goal.x, goal.y - 0.5));
        let first = self.land_below(start)?;
        if first == goal {
            return Some(Vec::new());
        }
        // Breadth first, so the first way found has the fewest hops.
        let mut came: HashMap<(i32, i32), ((i32, i32), Hop)> = HashMap::new();
        let mut queue = VecDeque::from([first]);
        let mut seen: HashSet<(i32, i32)> = HashSet::from([first]);
        while let Some(cell) = queue.pop_front() {
            for flight in self.flights_from(cell, jump) {
                let Some(landed) = flight.landed else {
                    continue;
                };
                if flight.hop.jump && !self.lands_lower_too(&flight.hop, landed, jump) {
                    continue;
                }
                if landed == goal || flight.cells.contains(&goal) {
                    let mut hops = vec![flight.hop];
                    let mut at = cell;
                    while let Some((before, hop)) = came.get(&at) {
                        hops.push(*hop);
                        at = *before;
                    }
                    hops.reverse();
                    return Some(hops);
                }
                if seen.insert(landed) {
                    came.insert(landed, (cell, flight.hop));
                    queue.push_back(landed);
                }
            }
        }
        None
    }

    /// A jump to count on: one a little lower (95% of the speed, so a
    /// tenth less height) comes down on the same level, in the same tile
    /// or the next. Otherwise the hop hangs on the jump's last pixels:
    /// just clearing a wall, or just reaching (or just missing) a ledge.
    fn lands_lower_too(&self, hop: &Hop, landed: (i32, i32), jump: Jump) -> bool {
        let lower = vec2(hop.speed, -jump.speed * 0.95);
        matches!(
            self.arc(hop.from, lower, jump).1,
            Some((column, row)) if row == landed.1 && (column - landed.0).abs() <= 1
        )
    }

    /// Everything a character standing in `cell` can do next: walk to the
    /// tile beside it, walk off an edge, or jump, at a few speeds each way.
    fn flights_from(&self, (column, row): (i32, i32), jump: Jump) -> Vec<Flight> {
        let size = self.tile_size;
        let feet_of =
            |(column, row): (i32, i32)| vec2((column as f32 + 0.5) * size, (row + 1) as f32 * size);
        let feet = feet_of((column, row));
        let fractions = [0.0, 0.25, 0.5, 0.75, 1.0];
        let mut flights = Vec::new();
        let mut add = |jumps: bool, speed: f32, (cells, landed): Flown| {
            flights.push(Flight {
                hop: Hop {
                    from: feet,
                    to: landed.map_or(feet, feet_of),
                    jump: jumps,
                    speed,
                },
                cells,
                landed,
            });
        };
        for direction in [-1.0, 1.0] {
            // Walking: to the next tile, or off the edge.
            let next = (column + direction as i32, row);
            if self.is_inside_column(next.0) && self.kind(next.0, next.1) != TileKind::Solid {
                if self.can_stand(next.0, next.1) {
                    add(false, direction * jump.run, (vec![next], Some(next)));
                } else {
                    let edge = vec2(
                        // It falls once all of it is past the edge.
                        (column as f32 + 0.5 + direction * 0.5) * size
                            + direction * (jump.width / 2.0 + 0.5),
                        feet.y,
                    );
                    for fraction in fractions {
                        let speed = direction * fraction * jump.run;
                        add(false, speed, self.arc(edge, vec2(speed, 0.0), jump));
                    }
                }
            }
            // Jumping.
            for fraction in fractions {
                if fraction == 0.0 && direction > 0.0 {
                    continue; // straight up only once
                }
                let speed = direction * fraction * jump.run;
                add(true, speed, self.arc(feet, vec2(speed, -jump.speed), jump));
            }
        }
        flights
    }

    /// Where a top-down character can walk to from `start`: every tile
    /// that isn't solid, joined to it up, down, left or right.
    pub fn reach_on_foot(&self, start: Vec2) -> Reach {
        let size = self.tile_size;
        let mut reach = Reach {
            tile_size: size,
            cells: HashSet::new(),
        };
        let first = self.cell_at(vec2(start.x, start.y - 0.5));
        let mut queue = vec![first];
        while let Some((column, row)) = queue.pop() {
            let inside = self.is_inside_column(column) && row >= 0 && (row as usize) < self.rows;
            if !inside
                || self.kind(column, row) == TileKind::Solid
                || !reach.cells.insert((column, row))
            {
                continue;
            }
            queue.extend([
                (column - 1, row),
                (column + 1, row),
                (column, row - 1),
                (column, row + 1),
            ]);
        }
        reach
    }

    fn is_inside_column(&self, column: i32) -> bool {
        column >= 0 && (column as usize) < self.columns
    }

    /// Something can stand in this tile: it isn't solid, and the one below
    /// is solid or a one-way platform.
    fn can_stand(&self, column: i32, row: i32) -> bool {
        self.kind(column, row) != TileKind::Solid
            && matches!(
                self.kind(column, row + 1),
                TileKind::Solid | TileKind::OneWay
            )
    }

    /// The tile something with its feet at `feet` comes to stand in, after
    /// falling straight down if it must.
    fn land_below(&self, feet: Vec2) -> Option<(i32, i32)> {
        let (column, mut row) = self.cell_at(vec2(feet.x, feet.y - 0.5));
        while (row as usize) < self.rows + 1 && row >= -1 {
            if self.can_stand(column, row) {
                return Some((column, row));
            }
            row += 1;
        }
        None
    }

    /// Follows a point thrown from `from` at `speed`: every tile a
    /// character there would be in, and the tile it lands in (if it does,
    /// rather than hitting something solid or falling out of the level).
    fn arc(&self, from: Vec2, speed: Vec2, jump: Jump) -> Flown {
        let size = self.tile_size;
        let (gravity, half) = (jump.gravity, jump.width / 2.0);
        // The same for its height: it bumps its head as if it were a tile
        // tall, nearly, and reaches things as if it were four fifths of one.
        let tall = size - 0.5;
        // A little to spare, each way: it's taken to bump into things as
        // if it were an eighth of a tile wider on each side, and to land
        // only on what something a quarter of a tile narrower on each
        // side would catch.
        let spare = if half > 0.0 { size / 8.0 } else { 0.0 };
        let (bumps, lands) = (half + spare, (half - spare * 2.0).max(0.0));
        let dt = 1.0 / 120.0;
        let bottom = (self.rows + 1) as f32 * size;
        let mut cells: Vec<(i32, i32)> = Vec::new();
        let mut mark = |cell: (i32, i32)| {
            if cells.last() != Some(&cell) && !cells.contains(&cell) {
                cells.push(cell);
            }
        };
        let mut previous = from;
        for step in 1..=600 {
            let t = step as f32 * dt;
            let at = vec2(
                from.x + speed.x * t,
                from.y + speed.y * t + gravity * t * t / 2.0,
            );
            // Going down across the top of a tile it can land on (under
            // its middle, or under either side of it): landed.
            if at.y > previous.y {
                let top_row = (at.y / size).floor() as i32;
                if (previous.y / size).floor() as i32 != top_row {
                    let catches = |side: f32| {
                        let column = ((at.x + side) / size).floor() as i32;
                        (matches!(
                            self.kind(column, top_row),
                            TileKind::Solid | TileKind::OneWay
                        ) && self.kind(column, top_row - 1) != TileKind::Solid)
                            .then_some((column, top_row - 1))
                    };
                    if let Some(cell) = [0.0, -lands, lands].into_iter().find_map(catches) {
                        mark(cell);
                        return (cells, Some(cell));
                    }
                    // Passing so close to an edge that it might catch it
                    // or might not: not an arc to count on.
                    if [-bumps, bumps].into_iter().find_map(catches).is_some() {
                        return (cells, None);
                    }
                }
            }
            let (column, row) = self.cell_at(vec2(at.x, at.y - 0.5));
            let head = self.cell_at(vec2(at.x, at.y - size * 0.8));
            // Its feet and its head, at its middle and at both sides.
            let blocked = [0.0, -bumps, bumps].into_iter().any(|side| {
                [at.y - 0.5, at.y - tall].into_iter().any(|y| {
                    let (column, row) = self.cell_at(vec2(at.x + side, y));
                    self.kind(column, row) == TileKind::Solid
                })
            });
            if !self.is_inside_column(column) || blocked || at.y > bottom {
                return (cells, None);
            }
            mark((column, row));
            mark(head);
            previous = at;
        }
        (cells, None)
    }

    fn read(
        text: &str,
        tile_size: f32,
        legend: &Legend,
        place: &str,
    ) -> Result<TileMap, LoadError> {
        let invalid = |message: String| LoadError::new(io::ErrorKind::InvalidData, message);
        if !(tile_size.is_finite() && tile_size > 0.0) {
            return Err(invalid(format!(
                "{place}tile size {tile_size} isn't a size: use a positive number like 16.0"
            )));
        }
        let mut lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
        // Line numbers in errors count from the first row of the level.
        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let rows = lines.len();
        let columns = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let mut map = TileMap {
            tile_size,
            columns,
            rows,
            cells: vec![None; columns * rows],
            tiles: Vec::new(),
            spawns: Vec::new(),
            legend: legend.clone(),
            files: Vec::new(),
        };
        for (row, line) in lines.iter().enumerate() {
            for (column, letter) in line.chars().enumerate() {
                if letter == '.' || letter == ' ' {
                    continue;
                }
                match legend.meaning(letter) {
                    Some(Meaning::Tile(name, kind)) => {
                        let tile = map.tile_index(name, *kind);
                        map.cells[row * columns + column] = Some(tile);
                    }
                    Some(Meaning::Spawn(name)) => {
                        let at = vec2(
                            (column as f32 + 0.5) * tile_size,
                            (row as f32 + 1.0) * tile_size,
                        );
                        map.spawns.push((name.clone(), at));
                    }
                    None => {
                        return Err(invalid(format!(
                            "{place}row {}, column {}: '{letter}' isn't in the legend (it has {})",
                            row + 1,
                            column + 1,
                            legend.known()
                        )));
                    }
                }
            }
        }
        Ok(map)
    }

    /// The number of the tile called `name`, adding it if it's new.
    fn tile_index(&mut self, name: &str, kind: TileKind) -> usize {
        match self.tiles.iter().position(|(n, k)| n == name && *k == kind) {
            Some(index) => index,
            None => {
                self.tiles.push((name.to_string(), kind));
                self.tiles.len() - 1
            }
        }
    }

    /// How many tiles across.
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// How many tiles down.
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// How big each tile is, in world units.
    pub fn tile_size(&self) -> f32 {
        self.tile_size
    }

    /// The whole map, in world units, from (0, 0). Handy for keeping the
    /// camera inside the level, or a `ctx.check` that the player is.
    pub fn bounds(&self) -> Rect {
        Rect::new(
            0.0,
            0.0,
            self.columns as f32 * self.tile_size,
            self.rows as f32 * self.tile_size,
        )
    }

    /// Every spawn point called `name`, row by row from the top: the bottom
    /// centre of its tile, where a character's feet go.
    pub fn spawns(&self, name: &str) -> Vec<Vec2> {
        self.spawns
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, at)| *at)
            .collect()
    }

    /// The first spawn point called `name` (see [`spawns`](TileMap::spawns)).
    /// Panics, naming the spawn points the map has, if there isn't one: a
    /// level without its player can't be played.
    #[track_caller]
    pub fn spawn(&self, name: &str) -> Vec2 {
        match self.spawns.iter().find(|(n, _)| n == name) {
            Some((_, at)) => *at,
            None => {
                let mut names: Vec<&str> = Vec::new();
                for (n, _) in &self.spawns {
                    if !names.contains(&n.as_str()) {
                        names.push(n);
                    }
                }
                panic!(
                    "the map has no spawn point {name:?}; it has: {}",
                    if names.is_empty() {
                        String::from("none")
                    } else {
                        names.join(", ")
                    }
                );
            }
        }
    }

    /// The column and row of the tile at a world position (they can be
    /// outside the map: negative, or past the last column or row).
    pub fn cell_at(&self, point: Vec2) -> (i32, i32) {
        (
            (point.x / self.tile_size).floor() as i32,
            (point.y / self.tile_size).floor() as i32,
        )
    }

    /// The area of the tile at `column`, `row`, in world units.
    pub fn tile_rect(&self, column: i32, row: i32) -> Rect {
        Rect::new(
            column as f32 * self.tile_size,
            row as f32 * self.tile_size,
            self.tile_size,
            self.tile_size,
        )
    }

    /// The name and kind of the tile at `column`, `row`, or `None` for an
    /// empty one (or one outside the map).
    pub fn tile_at(&self, column: i32, row: i32) -> Option<(&str, TileKind)> {
        let index = self.cell_index(column, row)?;
        self.cells[index].map(|tile| (self.tiles[tile].0.as_str(), self.tiles[tile].1))
    }

    /// What the tile at a world position does ([`TileKind::Empty`] outside
    /// the map).
    pub fn kind_at(&self, point: Vec2) -> TileKind {
        let (column, row) = self.cell_at(point);
        self.kind(column, row)
    }

    /// Empties the tile at `column`, `row` (a broken block, a collected
    /// coin tile). Outside the map, nothing happens.
    pub fn clear_tile(&mut self, column: i32, row: i32) {
        if let Some(index) = self.cell_index(column, row) {
            self.cells[index] = None;
        }
    }

    /// Puts the tile `name` that does `kind` at `column`, `row` (a block
    /// that appears, a door that closes). Outside the map, nothing happens.
    pub fn set_tile(&mut self, column: i32, row: i32, name: &str, kind: TileKind) {
        if let Some(index) = self.cell_index(column, row) {
            self.cells[index] = Some(self.tile_index(name, kind));
        }
    }

    fn cell_index(&self, column: i32, row: i32) -> Option<usize> {
        let inside = column >= 0
            && row >= 0
            && (column as usize) < self.columns
            && (row as usize) < self.rows;
        inside.then(|| row as usize * self.columns + column as usize)
    }

    fn kind(&self, column: i32, row: i32) -> TileKind {
        self.tile_at(column, row)
            .map_or(TileKind::Empty, |(_, kind)| kind)
    }

    /// The columns and rows of the tiles `rect` is inside (touching an edge
    /// doesn't count).
    fn cells_under(
        &self,
        rect: Rect,
    ) -> (std::ops::RangeInclusive<i32>, std::ops::RangeInclusive<i32>) {
        let first = self.cell_at(vec2(rect.left() + EDGE, rect.top() + EDGE));
        let last = self.cell_at(vec2(rect.right() - EDGE, rect.bottom() - EDGE));
        (first.0..=last.0, first.1..=last.1)
    }

    /// Whether any tile of `kind` is under `rect`, and the first one's area.
    fn hit(&self, rect: Rect, kind: TileKind) -> Option<Rect> {
        let (columns, rows) = self.cells_under(rect);
        for row in rows {
            for column in columns.clone() {
                if self.kind(column, row) == kind {
                    return Some(self.tile_rect(column, row));
                }
            }
        }
        None
    }

    /// Moves `body` by `motion` (world units, usually `velocity *
    /// ctx.dt()`), stopping at solid tiles and landing on one-way
    /// platforms, and says what it touched:
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let map = TileMap::parse("..\n##", 16.0, &Legend::new().solid('#', "ground")).unwrap();
    /// # let mut body = Rect::new(2.0, 0.0, 12.0, 14.0);
    /// # let mut velocity = vec2(0.0, 200.0);
    /// # let dt = 1.0 / 60.0;
    /// let touched = map.move_and_slide(&mut body, velocity * dt);
    /// if touched.ground || touched.ceiling {
    ///     velocity.y = 0.0;
    /// }
    /// if touched.wall_left || touched.wall_right {
    ///     velocity.x = 0.0;
    /// }
    /// ```
    ///
    /// It moves sideways, then up or down, in steps of at most half a tile,
    /// so even a very fast body (a dash, a long fall) can't pass through a
    /// wall or a floor one tile thick. Sliding along a floor or a wall
    /// doesn't snag on the seams between tiles. Outside the map is empty:
    /// bodies can leave it (add walls, or a `ctx.check`, to stop that).
    pub fn move_and_slide(&self, body: &mut Rect, motion: Vec2) -> Contacts {
        let mut touched = Contacts::default();
        if !(motion.x.is_finite() && motion.y.is_finite()) {
            return touched;
        }
        let step = self.tile_size / 2.0;
        let steps = (motion.x.abs().max(motion.y.abs()) / step).ceil().max(1.0) as usize;
        let mut each = motion * (1.0 / steps as f32);
        for _ in 0..steps {
            // Sideways: only solid tiles stop it.
            if each.x != 0.0 {
                body.x += each.x;
                if let Some(wall) = self.hit(*body, TileKind::Solid) {
                    if each.x > 0.0 {
                        body.x = wall.left() - body.w;
                        touched.wall_right = true;
                    } else {
                        body.x = wall.right();
                        touched.wall_left = true;
                    }
                    each.x = 0.0;
                }
            }
            // Up or down: solid tiles, and one-way platforms when coming
            // down onto them from above.
            if each.y != 0.0 {
                let feet_before = body.bottom();
                body.y += each.y;
                if let Some(block) = self.hit(*body, TileKind::Solid) {
                    if each.y > 0.0 {
                        body.y = block.top() - body.h;
                        touched.ground = true;
                    } else {
                        body.y = block.bottom();
                        touched.ceiling = true;
                    }
                    each.y = 0.0;
                } else if each.y > 0.0 {
                    if let Some(platform) = self.platform_landed_on(*body, feet_before) {
                        body.y = platform - body.h;
                        touched.ground = true;
                        each.y = 0.0;
                    }
                }
            }
        }
        touched
    }

    /// The top of a one-way platform that a body falling to `body` crossed
    /// with its feet (which were at `feet_before`), if any.
    fn platform_landed_on(&self, body: Rect, feet_before: f32) -> Option<f32> {
        let (columns, _) = self.cells_under(body);
        let last_row = self.cell_at(vec2(0.0, body.bottom() - EDGE)).1;
        let first_row = self.cell_at(vec2(0.0, feet_before - EDGE)).1 + 1;
        for row in first_row..=last_row {
            let top = row as f32 * self.tile_size;
            if feet_before > top + EDGE {
                continue; // its feet were already below this row's top
            }
            for column in columns.clone() {
                if self.kind(column, row) == TileKind::OneWay {
                    return Some(top);
                }
            }
        }
        None
    }

    /// The columns and rows of the tiles that can be seen on `canvas`.
    fn visible_cells(&self, canvas: &Canvas) -> (std::ops::Range<i32>, std::ops::Range<i32>) {
        let seen = canvas.visible_area();
        let (c0, r0) = self.cell_at(seen.top_left());
        let (c1, r1) = self.cell_at(seen.bottom_right());
        let clamp = |v: i32, most: usize| v.clamp(0, most as i32);
        (
            clamp(c0, self.columns)..clamp(c1 + 1, self.columns),
            clamp(r0, self.rows)..clamp(r1 + 1, self.rows),
        )
    }

    /// Tells the canvas where this map is on screen, for tools (Duck Forge
    /// Studio puts its grid there in Build mode), without drawing it.
    /// [`draw`](TileMap::draw) does this itself; call it in a game that
    /// draws its own tiles, between `push_camera` and `pop_camera`.
    pub fn mark_drawn(&self, canvas: &mut Canvas) {
        let view = canvas.view();
        let corner = view.apply(Vec2::ZERO);
        canvas.note_map(crate::canvas::DrawnMap {
            file: self.files.first().cloned().unwrap_or_default(),
            x: corner.x,
            y: corner.y,
            scale: view.apply(vec2(1.0, 0.0)).x - corner.x,
            tile_size: self.tile_size,
            columns: self.columns,
            rows: self.rows,
        });
    }

    /// Draws the map's tiles with `tiles`, at `time` seconds (the game's
    /// time, kept in `update`: it moves animated tiles like water). Call it
    /// between `push_camera` and `pop_camera`, like the rest of the world.
    /// Only the tiles the camera can see are drawn, so a level thousands of
    /// tiles long costs no more than one screen of it.
    ///
    /// Each tile's picture is stretched to whole pixels that meet the next
    /// tile's exactly, so there are no seams, at any zoom. It also labels
    /// the whole map as `"map"`, so the run summary knows how big the world
    /// is (see [`Canvas::label`]).
    ///
    /// Panics, naming the tiles the tileset has, if a tile in the map isn't
    /// in the tileset: a missing picture is a mistake to fix, not to draw
    /// around.
    pub fn draw(&self, canvas: &mut Canvas, tiles: &Tileset, time: f32) {
        canvas.label("map", self.bounds());
        let (columns, rows) = self.visible_cells(canvas);
        let view = canvas.view();
        self.mark_drawn(canvas);
        canvas.set_view(crate::camera::View::SCREEN);
        // Tile edges in whole screen pixels, shared by neighbours.
        let edge_x =
            |column: i32| (view.apply(vec2(column as f32 * self.tile_size, 0.0)).x).round();
        let edge_y = |row: i32| (view.apply(vec2(0.0, row as f32 * self.tile_size)).y).round();
        for row in rows {
            let (top, bottom) = (edge_y(row), edge_y(row + 1));
            for column in columns.clone() {
                let Some(tile) = self.cells[row as usize * self.columns + column as usize] else {
                    continue;
                };
                let name = &self.tiles[tile].0;
                let sprite = tiles.frame(name, time);
                let (left, right) = (edge_x(column), edge_x(column + 1));
                if sprite.width() == 0 || right <= left || bottom <= top {
                    continue;
                }
                // Square tiles: one scale fits both sides (to the pixel).
                let scale = (right - left) / sprite.width() as f32;
                canvas.blit_sprite(
                    sprite,
                    vec2(left, top),
                    Blit {
                        scale,
                        ..Blit::default()
                    },
                );
            }
        }
        canvas.set_view(view);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use crate::camera::Camera2D;
    use crate::color::Color;
    use crate::sprite::{Animation, Sprite};

    fn legend() -> Legend {
        Legend::new()
            .solid('#', "ground")
            .one_way('=', "plank")
            .decoration('f', "flower")
            .spawn('p', "player")
            .spawn('c', "coin")
    }

    const LEVEL: &str = "
..c.....c
p...===..
#####f...
";

    #[test]
    fn a_level_is_read_from_text() {
        let map = TileMap::parse(LEVEL, 16.0, &legend()).unwrap();
        assert_eq!((map.columns(), map.rows()), (9, 3));
        assert_eq!(map.bounds(), Rect::new(0.0, 0.0, 144.0, 48.0));
        assert_eq!(
            map.spawn("player"),
            vec2(8.0, 32.0),
            "the bottom centre of its tile"
        );
        assert_eq!(map.spawns("coin"), [vec2(40.0, 16.0), vec2(136.0, 16.0)]);
        assert_eq!(map.tile_at(0, 2), Some(("ground", TileKind::Solid)));
        assert_eq!(map.tile_at(4, 1), Some(("plank", TileKind::OneWay)));
        assert_eq!(map.tile_at(5, 2), Some(("flower", TileKind::Empty)));
        assert_eq!(map.tile_at(0, 1), None, "a spawn point's tile is empty");
        assert_eq!(map.tile_at(-1, 0), None);
        assert_eq!(map.kind_at(vec2(70.0, 20.0)), TileKind::OneWay);
        // Rows can be shorter than the longest: the rest is empty.
        let ragged = TileMap::parse("#\n###", 8.0, &legend()).unwrap();
        assert_eq!((ragged.columns(), ragged.tile_at(2, 0)), (3, None));
    }

    const LEGEND_FILE: &str = r##"{
  "tile_size": 8,
  "tiles": "assets/tiles.json",
  "letters": {
    "#": {"kind": "solid", "name": "ground"},
    "=": {"kind": "one_way", "name": "plank"},
    "p": {"kind": "thing", "name": "player", "label": "Start", "at_least": 1, "at_most": 1, "start": true},
    "c": {"kind": "thing", "name": "coin"},
    "g": {"kind": "thing", "name": "flag", "at_least": 1}
  }
}"##;

    #[test]
    fn a_level_and_its_legend_are_read_from_two_files() {
        let folder = crate::test_folder::TestFolder::new("level");
        let level = folder.join("level.txt");
        std::fs::write(&level, "p.c.\n####\n").unwrap();
        std::fs::write(folder.join("level.legend.json"), LEGEND_FILE).unwrap();

        let map = TileMap::load_level(&level).unwrap();
        assert_eq!(map.tile_size(), 8.0, "from the legend file");
        assert_eq!(map.spawn("player"), vec2(4.0, 8.0));
        assert_eq!(map.files().len(), 2);
        assert!(map.reads_from(&level));
        assert!(map.reads_from(&folder.join("level.legend.json")));
        assert!(!map.reads_from(&folder.join("other.txt")));
        assert_eq!(
            map.legend().tiles_file(),
            Some(folder.join("assets/tiles.json").as_path())
        );
        let letters = map.legend().letters();
        let kinds: Vec<char> = letters.iter().map(|l| l.letter).collect();
        assert_eq!(kinds, ['#', '=', 'c', 'g', 'p'], "tiles first, then things");
        let start = &letters[4];
        assert_eq!(
            (start.letter, start.kind, start.title()),
            ('p', LetterKind::Thing, String::from("Start"))
        );
        assert!(start.start);
        assert_eq!(
            letters[2].title(),
            "Coin",
            "no label: the name, with a capital"
        );

        // The rules: one Start, at least one flag.
        assert_eq!(
            map.broken_rules(),
            ["The level needs at least 1 Flag; it has none."]
        );
        let two_starts = TileMap::parse("p.pg\n####", 8.0, map.legend()).unwrap();
        assert_eq!(
            two_starts.broken_rules(),
            ["The level can have at most 1 Start; it has 2."]
        );

        // What a new version adds and takes away.
        let newer = TileMap::parse("p..c\n####", 8.0, map.legend()).unwrap();
        let changes = map.spawn_changes(&newer, "coin");
        assert_eq!(changes.added, [vec2(28.0, 8.0)]);
        assert_eq!(changes.removed, [vec2(20.0, 8.0)]);

        let error = Legend::from_json(r##"{"letters": {"#": {"kind": "rock", "name": "x"}}}"##)
            .unwrap_err();
        assert!(
            error.contains("must be solid, one_way, decoration or thing"),
            "{error}"
        );
        let error = Legend::from_json(r###"{"letters": {"##": {"kind": "solid", "name": "x"}}}"###)
            .unwrap_err();
        assert!(error.contains("one letter"), "{error}");
    }

    #[test]
    fn reach_follows_jumps_and_falls() {
        let legend = Legend::new()
            .solid('#', "ground")
            .one_way('=', "plank")
            .spawn('p', "player")
            .spawn('c', "coin");
        let jump = Jump {
            speed: 330.0,
            gravity: 900.0,
            run: 110.0,
            width: 0.0,
        };
        // A jump of about 60 pixels: three tiles up is fine, five isn't.
        let map = TileMap::parse(
            "\
c.........c
...........
...........
.....c.....
...........
...........
.p.........
###########",
            16.0,
            &legend,
        )
        .unwrap();
        let reach = map.reach(map.spawn("player"), jump);
        let coins = map.spawns("coin");
        assert!(reach.can_reach(coins[2]), "three tiles up");
        assert_eq!(
            reach.missing(&coins),
            [coins[0], coins[1]],
            "seven tiles up"
        );

        // A plank three tiles up is a step: from it, the high coin.
        let map = TileMap::parse(
            "\
...c.......
...........
...........
...........
..===......
...........
.p.........
###########",
            16.0,
            &legend,
        )
        .unwrap();
        let reach = map.reach(map.spawn("player"), jump);
        assert!(reach.can_reach(map.spawn("coin")));

        // A gap three tiles wide can be jumped; seven can't.
        let map =
            TileMap::parse(".p.....c...........c\n####...####.......##", 16.0, &legend).unwrap();
        let reach = map.reach(map.spawn("player"), jump);
        let coins = map.spawns("coin");
        assert!(reach.can_reach(coins[0]));
        assert!(!reach.can_reach(coins[1]));

        // On foot (top-down): walls stop it.
        let map = TileMap::parse("p.#c\n..#.\n####", 16.0, &legend).unwrap();
        let reach = map.reach_on_foot(map.spawn("player"));
        assert!(!reach.can_reach(map.spawn("coin")));
        assert!(reach.has(1, 1));
    }

    #[test]
    fn mistakes_name_the_row_and_column() {
        let error = TileMap::parse("..\n.#x", 16.0, &legend()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "row 2, column 3: 'x' isn't in the legend (it has # = f p c . and space)"
        );
        let folder = crate::test_folder::TestFolder::new("map");
        let file = folder.join("1.txt");
        fs::write(&file, "\n\n####\n#??#\n").unwrap();
        let error = TileMap::load(&file, 16.0, &legend())
            .unwrap_err()
            .to_string();
        assert!(
            error.ends_with(
                "1.txt row 2, column 2: '?' isn't in the legend (it has # = f p c . and space)"
            ),
            "blank lines before the level don't count: {error}"
        );
        let missing = TileMap::load(folder.join("nope.txt"), 16.0, &legend()).unwrap_err();
        assert!(
            missing
                .to_string()
                .starts_with("couldn't find the level file"),
            "{missing}"
        );
        let panicked = std::panic::catch_unwind(|| {
            TileMap::parse(LEVEL, 16.0, &legend())
                .unwrap()
                .spawn("goal");
        });
        let message = *panicked.unwrap_err().downcast::<String>().unwrap();
        assert_eq!(
            message,
            "the map has no spawn point \"goal\"; it has: coin, player"
        );
    }

    /// A box with walls, a floor, a ceiling and a one-way platform:
    ///
    /// ```text
    ///  0 ##########
    ///  1 #........#
    ///  2 #........#
    ///  3 #...===..#
    ///  4 #........#
    ///  5 ##########
    /// ```
    fn room() -> TileMap {
        let text = "##########\n#........#\n#........#\n#...===..#\n#........#\n##########";
        TileMap::parse(text, 16.0, &legend()).unwrap()
    }

    #[test]
    fn bodies_land_bump_and_slide() {
        let map = room();
        // Falling onto the floor (its top is at y = 80).
        let mut body = Rect::new(20.0, 60.0, 10.0, 12.0);
        let touched = map.move_and_slide(&mut body, vec2(0.0, 30.0));
        assert!(touched.ground && !touched.ceiling);
        assert_eq!(body.bottom(), 80.0);
        // Walking along the floor, over the seams between floor tiles,
        // touches nothing.
        for _ in 0..40 {
            let touched = map.move_and_slide(&mut body, vec2(2.0, 0.0));
            assert!(!touched.wall_right, "snagged at x = {}", body.x);
        }
        // Into the right wall (its left side is at x = 144).
        let touched = map.move_and_slide(&mut body, vec2(100.0, 0.0));
        assert!(touched.wall_right);
        assert_eq!(body.right(), 144.0);
        // And the left one, and the ceiling (its bottom is at y = 16).
        let touched = map.move_and_slide(&mut body, vec2(-500.0, -500.0));
        assert!(touched.wall_left && touched.ceiling);
        assert_eq!((body.left(), body.top()), (16.0, 16.0));
    }

    #[test]
    fn one_way_platforms_are_landed_on_from_above_only() {
        let map = room();
        // Jumping up from below, through the platform (y 48 to 64).
        let mut body = Rect::new(70.0, 66.0, 10.0, 12.0);
        let touched = map.move_and_slide(&mut body, vec2(0.0, -40.0));
        assert_eq!(touched, Contacts::default(), "nothing stops it going up");
        assert_eq!(body.y, 26.0);
        // Coming down: it lands on top.
        let touched = map.move_and_slide(&mut body, vec2(0.0, 40.0));
        assert!(touched.ground);
        assert_eq!(body.bottom(), 48.0);
        // Standing on it, it stays standing (gravity every frame).
        let touched = map.move_and_slide(&mut body, vec2(0.0, 1.0));
        assert!(touched.ground);
        assert_eq!(body.bottom(), 48.0);
        // Walking sideways off the end of it (at x = 112), it falls.
        let mut walker = Rect::new(108.0, 36.0, 10.0, 12.0);
        let touched = map.move_and_slide(&mut walker, vec2(20.0, 5.0));
        assert!(!touched.ground);
        assert!((walker.bottom() - 53.0).abs() < 0.001, "{walker:?}");
    }

    #[test]
    fn a_very_fast_body_never_passes_through_a_tile() {
        // A wall and a floor one tile thick, and a body crossing a whole
        // screen in one frame (a dash at 18,000 pixels a second).
        let map = TileMap::parse("....#....\n.........\n#########", 16.0, &legend()).unwrap();
        let mut dasher = Rect::new(2.0, 2.0, 8.0, 8.0);
        let touched = map.move_and_slide(&mut dasher, vec2(300.0, 0.0));
        assert!(touched.wall_right);
        assert_eq!(dasher.right(), 64.0, "stopped at the wall, not past it");
        let mut faller = Rect::new(100.0, -500.0, 8.0, 8.0);
        let touched = map.move_and_slide(&mut faller, vec2(0.0, 2000.0));
        assert!(touched.ground);
        assert_eq!(faller.bottom(), 32.0, "on the floor, not under it");
        // The same for a one-way platform.
        let planks = TileMap::parse(".....\n=====", 16.0, &legend()).unwrap();
        let mut faller = Rect::new(20.0, -900.0, 8.0, 8.0);
        assert!(planks.move_and_slide(&mut faller, vec2(0.0, 1500.0)).ground);
        assert_eq!(faller.bottom(), 16.0);
    }

    fn tileset() -> Tileset {
        let solid = |c: Color| Sprite::from_pixels(4, 4, vec![c; 16]);
        let mut tiles = Tileset::new();
        tiles.insert("ground", Animation::new(vec![solid(Color::GREEN)], 1.0));
        tiles.insert("plank", Animation::new(vec![solid(Color::ORANGE)], 1.0));
        tiles.insert(
            "flower",
            Animation::new(vec![solid(Color::RED), solid(Color::BLUE)], 2.0),
        );
        tiles
    }

    #[test]
    fn drawing_shows_the_tiles_the_camera_sees_and_animates_them() {
        let map = TileMap::parse(LEVEL, 4.0, &legend()).unwrap();
        let mut canvas = Canvas::new(36, 12);
        map.draw(&mut canvas, &tileset(), 0.0);
        assert_eq!(canvas.get_pixel(1, 9), Some(Color::GREEN));
        assert_eq!(canvas.get_pixel(17, 5), Some(Color::ORANGE));
        assert_eq!(
            canvas.get_pixel(21, 9),
            Some(Color::RED),
            "flower, first frame"
        );
        assert_eq!(canvas.get_pixel(1, 1), Some(Color::BLACK), "empty");
        map.draw(&mut canvas, &tileset(), 0.5);
        assert_eq!(
            canvas.get_pixel(21, 9),
            Some(Color::BLUE),
            "half a second later"
        );
        // Through a camera that zooms in on the planks (x 16 to 28, y 4
        // to 8), 3 times: the tiles meet with no gaps.
        let mut camera = Camera2D::new(36.0, 12.0);
        camera.position = vec2(22.0, 6.0);
        camera.zoom = 3.0;
        let mut zoomed = Canvas::new(36, 12);
        zoomed.push_camera(&camera);
        map.draw(&mut zoomed, &tileset(), 0.0);
        zoomed.pop_camera();
        for x in 0..36 {
            assert_eq!(zoomed.get_pixel(x, 0), Some(Color::ORANGE), "x = {x}");
        }
    }

    #[test]
    fn only_tiles_on_screen_are_looked_at() {
        let long = format!("{}\n{}", ".".repeat(5000), "#".repeat(5000));
        let map = TileMap::parse(&long, 16.0, &legend()).unwrap();
        let mut canvas = Canvas::new(320, 240);
        let mut camera = Camera2D::new(320.0, 240.0);
        camera.position = vec2(40_000.0, 120.0);
        canvas.push_camera(&camera);
        let (columns, rows) = map.visible_cells(&canvas);
        assert_eq!((columns, rows), (2490..2511, 0..2));
        canvas.pop_camera();
        // Odd zooms leave no seams between tiles either.
        camera.zoom = 1.3;
        canvas.push_camera(&camera);
        map.draw(&mut canvas, &tileset(), 0.0);
        canvas.pop_camera();
        // The floor (world y 16 to 32) is on screen from y -15.2 to 5.6.
        let floor = 2;
        for x in 0..320 {
            assert_eq!(canvas.get_pixel(x, floor), Some(Color::GREEN), "x = {x}");
        }
    }

    #[test]
    #[should_panic(expected = "the tileset has no tile \"plank\"; it has: ground")]
    fn a_tile_missing_from_the_tileset_is_named() {
        let map = TileMap::parse("=", 4.0, &legend()).unwrap();
        let mut tiles = Tileset::new();
        tiles.insert(
            "ground",
            Animation::new(vec![Sprite::from_pixels(4, 4, vec![Color::GREEN; 16])], 1.0),
        );
        map.draw(&mut Canvas::new(8, 8), &tiles, 0.0);
    }
}
