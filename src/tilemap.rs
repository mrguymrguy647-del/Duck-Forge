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

use std::fs;
use std::io;
use std::path::Path;

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

/// What each letter of a level means. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Legend {
    letters: Vec<(char, Meaning)>,
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
        let text = fs::read_to_string(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "level", &error))?;
        TileMap::read(&text, tile_size, legend, &format!("{} ", path.display()))
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

    #[test]
    fn mistakes_name_the_row_and_column() {
        let error = TileMap::parse("..\n.#x", 16.0, &legend()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "row 2, column 3: 'x' isn't in the legend (it has # = f p c . and space)"
        );
        let folder = std::env::temp_dir().join(format!("duckforge-map-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
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
        let _ = fs::remove_dir_all(folder);
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
