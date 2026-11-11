//! A game's level as Build mode edits it: the letters of `level.txt`,
//! with the legend from `level.legend.json` (read with the engine's own
//! code), one cell per letter.
//!
//! Every edit is written to the file at once, whole, through a temporary
//! file that then takes its place, so a crash can't leave half a level.
//! Nothing here knows about the game running: the Studio sends the new
//! text to it.

use std::path::{Path, PathBuf};

use duckforge::prelude::{
    Blit, Canvas, Color, Legend, Letter, LetterKind, SpriteSheet, TileMap, Tileset, vec2,
};

use crate::problem::Problem;

/// A cell of the level: its column and row, counted from 0 at the top
/// left, as the engine counts them.
pub type Cell = (i32, i32);

/// The most cells one fill changes (a fill of a huge empty level stops
/// there, rather than taking a long time).
const MOST_FILLED: usize = 20_000;

/// A level and its legend.
#[derive(Debug, Clone)]
pub struct Level {
    /// The letters' file on disk.
    pub path: PathBuf,
    /// Its name as the game reads it, from the games folder:
    /// `examples/bee_maze/level.txt`.
    pub name: String,
    /// The legend's file, and its name.
    pub legend_path: PathBuf,
    pub legend_name: String,
    /// Every letter: the tiles, then the things.
    pub letters: Vec<Letter>,
    pub tile_size: f32,
    /// The tiles' sprite sheet, if the legend names one.
    pub tiles_file: Option<PathBuf>,
    legend: Legend,
    grid: Vec<Vec<char>>,
    columns: usize,
    /// The text as last read or written, to notice changes made outside.
    on_disk: String,
}

/// A picture the Studio drew itself (the whole level, a tile for the
/// palette): row by row, each pixel `0x00RRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

impl Level {
    /// The level of a game whose folder has `level.txt`. `root` is the
    /// games folder, for the level's name.
    pub fn load(root: &Path, path: &Path) -> Result<Level, Problem> {
        let text = std::fs::read_to_string(path).map_err(|error| {
            Problem::new(
                "The level's file couldn't be read.",
                "Check it's there, then open the game again.",
            )
            .with_details(format!("{}: {error}", path.display()))
        })?;
        let legend_path = TileMap::legend_file_for(path);
        let legend = Legend::load(&legend_path).map_err(|error| {
            Problem::new(
                "The level's legend (what each letter means) has a mistake.",
                "Look at the details, fix level.legend.json, and save it.",
            )
            .with_details(error.to_string())
        })?;
        let name_of = |file: &Path| {
            file.strip_prefix(root)
                .unwrap_or(file)
                .to_string_lossy()
                .replace('\\', "/")
        };
        let mut level = Level {
            name: name_of(path),
            legend_name: name_of(&legend_path),
            path: path.to_path_buf(),
            legend_path,
            letters: legend.letters(),
            tile_size: legend.tile_size().unwrap_or(16.0),
            tiles_file: legend.tiles_file().map(Path::to_path_buf),
            legend,
            grid: Vec::new(),
            columns: 0,
            on_disk: String::new(),
        };
        level.take_text(&text);
        Ok(level)
    }

    fn take_text(&mut self, text: &str) {
        let mut lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
        while lines.first().is_some_and(|l| l.trim().is_empty()) {
            lines.remove(0);
        }
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        self.grid = lines.iter().map(|l| l.chars().collect()).collect();
        self.columns = self.grid.iter().map(Vec::len).max().unwrap_or(0);
        self.on_disk = text.to_string();
    }

    pub fn columns(&self) -> usize {
        self.columns
    }

    pub fn rows(&self) -> usize {
        self.grid.len()
    }

    /// The level as the file's text: one line per row.
    pub fn text(&self) -> String {
        let mut text: String = self
            .grid
            .iter()
            .map(|row| row.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        text.push('\n');
        text
    }

    /// The letter in a cell: `.` for empty (and outside the level).
    pub fn letter_at(&self, (column, row): Cell) -> char {
        if column < 0 || row < 0 {
            return '.';
        }
        match self
            .grid
            .get(row as usize)
            .and_then(|r| r.get(column as usize))
        {
            Some(' ') | None => '.',
            Some(&letter) => letter,
        }
    }

    /// What a letter means, if the legend knows it.
    pub fn letter(&self, letter: char) -> Option<&Letter> {
        self.letters.iter().find(|l| l.letter == letter)
    }

    /// The letter for a tile or thing called `name` (`grass`, `coin`).
    pub fn letter_named(&self, name: &str) -> Option<&Letter> {
        self.letters.iter().find(|l| l.name == name)
    }

    pub fn is_inside(&self, (column, row): Cell) -> bool {
        column >= 0 && row >= 0 && (column as usize) < self.columns && (row as usize) < self.rows()
    }

    /// Puts `letter` in a cell. False if nothing changed (or it's outside).
    pub fn set(&mut self, cell: Cell, letter: char) -> bool {
        if !self.is_inside(cell) || self.letter_at(cell) == letter {
            return false;
        }
        let row = &mut self.grid[cell.1 as usize];
        while row.len() <= cell.0 as usize {
            row.push('.');
        }
        row[cell.0 as usize] = letter;
        true
    }

    /// Paints `letter` on every cell. How many changed.
    pub fn paint(&mut self, cells: &[Cell], letter: char) -> usize {
        cells.iter().filter(|&&cell| self.set(cell, letter)).count()
    }

    /// Fills the cell's area (the cells joined to it, up, down, left or
    /// right, with the same letter) with `letter`. How many changed.
    pub fn fill(&mut self, start: Cell, letter: char) -> usize {
        if !self.is_inside(start) {
            return 0;
        }
        let was = self.letter_at(start);
        if was == letter {
            return 0;
        }
        let mut queue = vec![start];
        let mut changed = 0;
        while let Some((column, row)) = queue.pop() {
            if changed >= MOST_FILLED || self.letter_at((column, row)) != was {
                continue;
            }
            if self.set((column, row), letter) {
                changed += 1;
                queue.extend([
                    (column - 1, row),
                    (column + 1, row),
                    (column, row - 1),
                    (column, row + 1),
                ]);
            }
        }
        changed
    }

    /// Puts a thing in a cell. A thing the level can have only one of (the
    /// Start) moves there instead of being added. False if nothing changed.
    pub fn place_thing(&mut self, cell: Cell, letter: char) -> bool {
        if !self.is_inside(cell) {
            return false;
        }
        if self.letter(letter).and_then(|l| l.at_most) == Some(1) {
            for other in self.cells_with(letter) {
                if other != cell {
                    self.set(other, '.');
                }
            }
        }
        self.set(cell, letter)
    }

    /// Moves what's in `from` to `to` (leaving `from` empty).
    pub fn move_thing(&mut self, from: Cell, to: Cell) -> bool {
        let letter = self.letter_at(from);
        if letter == '.' || from == to || !self.is_inside(to) {
            return false;
        }
        self.set(from, '.');
        self.set(to, letter);
        true
    }

    /// Moves a block of cells (`size` columns by rows, from `from`) so its
    /// top left is at `to`. What was in the block's old place is emptied.
    pub fn move_area(&mut self, from: Cell, size: (i32, i32), to: Cell) -> usize {
        let mut block = Vec::new();
        for row in 0..size.1 {
            for column in 0..size.0 {
                let cell = (from.0 + column, from.1 + row);
                block.push(((column, row), self.letter_at(cell)));
            }
        }
        let mut changed = 0;
        for ((column, row), _) in &block {
            changed += usize::from(self.set((from.0 + column, from.1 + row), '.'));
        }
        for ((column, row), letter) in block {
            changed += usize::from(self.set((to.0 + column, to.1 + row), letter));
        }
        changed
    }

    /// Makes the level `columns` by `rows`, adding empty cells on the
    /// right and at the bottom, or taking them away there.
    pub fn resize(&mut self, columns: usize, rows: usize) -> bool {
        let (columns, rows) = (columns.clamp(1, 1000), rows.clamp(1, 1000));
        if (columns, rows) == (self.columns, self.rows()) {
            return false;
        }
        self.grid.resize_with(rows, Vec::new);
        for row in &mut self.grid {
            row.resize(columns, '.');
        }
        self.columns = columns;
        true
    }

    /// Every cell holding `letter`, row by row.
    pub fn cells_with(&self, letter: char) -> Vec<Cell> {
        let mut cells = Vec::new();
        for (row, letters) in self.grid.iter().enumerate() {
            for (column, &l) in letters.iter().enumerate() {
                if l == letter {
                    cells.push((column as i32, row as i32));
                }
            }
        }
        cells
    }

    /// The level with its Start moved to `cell`: for "Play from here",
    /// which plays it that way once, without changing the file.
    pub fn with_start_at(&self, cell: Cell) -> Option<String> {
        let start = self.letters.iter().find(|l| l.start)?;
        let mut moved = self.clone();
        for other in moved.cells_with(start.letter) {
            moved.set(other, '.');
        }
        moved.set(cell, start.letter);
        Some(moved.text())
    }

    /// Writes the level to its file, whole: first to a file beside it,
    /// which then takes the old one's place.
    pub fn save(&mut self) -> Result<(), Problem> {
        let text = self.text();
        write_whole(&self.path, &text)?;
        self.on_disk = text;
        Ok(())
    }

    /// Reads the file again if it changed outside the Studio (an editor,
    /// an agent). True if it did.
    pub fn reload_if_changed(&mut self) -> bool {
        match std::fs::read_to_string(&self.path) {
            Ok(text) if text != self.on_disk => {
                self.take_text(&text);
                true
            }
            _ => false,
        }
    }

    /// The legend's rules the level breaks, in plain words (or why it
    /// can't be read at all).
    pub fn broken_rules(&self) -> Vec<String> {
        match TileMap::parse(&self.text(), self.tile_size, &self.legend) {
            Ok(map) => map.broken_rules(),
            Err(error) => vec![error.to_string()],
        }
    }

    /// The level read by the engine, as the game would read it.
    pub fn map(&self) -> Result<TileMap, Problem> {
        TileMap::parse(&self.text(), self.tile_size, &self.legend).map_err(|error| {
            Problem::new("The level can't be read.", "Look at the details.")
                .with_details(error.to_string())
        })
    }
}

/// Writes `text` to `path` whole: to a file beside it first, which then
/// takes its place, so the file is never half written.
pub fn write_whole(path: &Path, text: &str) -> Result<(), Problem> {
    let beside = path.with_extension("studio-saving");
    std::fs::write(&beside, text)
        .and_then(|()| std::fs::rename(&beside, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&beside);
            Problem::new(
                format!("The Studio couldn't save {}.", path.display()),
                "Check the file isn't open in another program, then try again.",
            )
            .with_details(error.to_string())
        })
}

/// The pictures Build mode needs: each tile's and thing's, and the whole
/// level drawn the way the engine draws tiles.
pub struct LevelArt {
    sheet: Option<SpriteSheet>,
    tiles: Option<Tileset>,
}

/// The colour behind tiles in the Studio's own pictures.
const BACKDROP: Color = Color::rgb(0x20, 0x24, 0x3a);

impl LevelArt {
    /// The tiles' sprite sheet, if the legend names one that can be read.
    pub fn load(level: &Level) -> LevelArt {
        let sheet = level
            .tiles_file
            .as_ref()
            .and_then(|file| SpriteSheet::load(file).ok());
        let tiles = sheet.as_ref().map(Tileset::from_sheet);
        LevelArt { sheet, tiles }
    }

    /// One letter's picture, `size` pixels square: the tile, the thing's
    /// picture from the sheet, or a marker with its letter.
    pub fn picture_of(&self, letter: &Letter, size: usize) -> Pixels {
        let mut canvas = Canvas::new(size, size);
        canvas.clear(BACKDROP);
        self.draw_letter(&mut canvas, letter, 0.0, 0.0, size as f32);
        pixels_of(&canvas)
    }

    /// The whole level, one pixel per world unit, tiles drawn as the
    /// engine draws them (no backgrounds: those are the game's).
    pub fn draw_level(&self, level: &Level) -> Pixels {
        let size = level.tile_size;
        let width = (level.columns() as f32 * size).max(1.0) as usize;
        let height = (level.rows() as f32 * size).max(1.0) as usize;
        let mut canvas = Canvas::new(width, height);
        canvas.clear(BACKDROP);
        // Tiles first, then things on top.
        for kind_pass in [false, true] {
            for row in 0..level.rows() as i32 {
                for column in 0..level.columns() as i32 {
                    let Some(letter) = level.letter(level.letter_at((column, row))) else {
                        continue;
                    };
                    if (letter.kind == LetterKind::Thing) != kind_pass {
                        continue;
                    }
                    let (x, y) = (column as f32 * size, row as f32 * size);
                    self.draw_letter(&mut canvas, letter, x, y, size);
                }
            }
        }
        pixels_of(&canvas)
    }

    fn draw_letter(&self, canvas: &mut Canvas, letter: &Letter, x: f32, y: f32, size: f32) {
        if letter.kind != LetterKind::Thing {
            if let Some(tiles) = &self.tiles
                && tiles.get(&letter.name).is_some()
            {
                let sprite = tiles.frame(&letter.name, 0.0);
                let scale = size / sprite.width().max(1) as f32;
                canvas.blit_sprite(
                    sprite,
                    vec2(x, y),
                    Blit {
                        scale,
                        ..Blit::default()
                    },
                );
                return;
            }
            // No picture: a plain block, solid ones lighter.
            let color = match letter.kind {
                LetterKind::Solid => Color::rgb(0x5a, 0x5a, 0x78),
                LetterKind::OneWay => Color::rgb(0x9a, 0x70, 0x40),
                _ => Color::rgb(0x40, 0x58, 0x80),
            };
            canvas.fill_rect(duckforge::prelude::Rect::new(x, y, size, size), color);
            canvas.fill_rect(
                duckforge::prelude::Rect::new(x, y, size, (size / 6.0).max(1.0)),
                Color::rgb(0x80, 0x80, 0xa0),
            );
            return;
        }
        if let Some(sheet) = &self.sheet
            && sheet.get(&letter.name).is_some()
        {
            // Things stand on the bottom middle of their tile.
            sheet.draw(
                canvas,
                &letter.name,
                0.0,
                vec2(x + size / 2.0, y + size),
                Blit::default(),
            );
            return;
        }
        // A marker: a round badge with the thing's first letter.
        let color = if letter.start {
            Color::rgb(0xff, 0xcc, 0x2e) // Duck Yellow
        } else {
            let hue = letter
                .name
                .bytes()
                .fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
            Color::rgb(
                0x80 + (hue % 0x60) as u8,
                0x60 + (hue / 7 % 0x80) as u8,
                0xa0 + (hue / 13 % 0x50) as u8,
            )
        };
        let centre = vec2(x + size / 2.0, y + size / 2.0);
        canvas.fill_circle(centre, size * 0.4, color);
        let initial: String = letter.title().chars().take(1).collect();
        let width = Canvas::text_width(&initial, 1);
        canvas.draw_text(
            &initial,
            vec2(centre.x - width / 2.0, centre.y - 3.0),
            1,
            Color::rgb(0x1b, 0x14, 0x46),
        );
    }
}

fn pixels_of(canvas: &Canvas) -> Pixels {
    Pixels {
        width: canvas.width() as u32,
        height: canvas.height() as u32,
        pixels: canvas.pixels().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGEND: &str = r##"{"tile_size": 16, "letters": {
        "#": {"kind": "solid", "name": "ground"},
        "p": {"kind": "thing", "name": "player", "label": "Start", "at_most": 1, "at_least": 1, "start": true},
        "c": {"kind": "thing", "name": "coin"}}}"##;

    fn level(text: &str) -> (PathBuf, Level) {
        let folder = std::env::temp_dir().join(format!(
            "studio-level-{}-{}",
            std::process::id(),
            text.len()
        ));
        std::fs::create_dir_all(folder.join("examples/a")).unwrap();
        let path = folder.join("examples/a/level.txt");
        std::fs::write(&path, text).unwrap();
        std::fs::write(folder.join("examples/a/level.legend.json"), LEGEND).unwrap();
        let level = Level::load(&folder, &path).unwrap();
        (folder, level)
    }

    #[test]
    fn edits_change_the_letters_and_are_saved_whole() {
        let (folder, mut level) = level("\np...\n####\n");
        assert_eq!(level.name, "examples/a/level.txt");
        assert_eq!((level.columns(), level.rows()), (4, 2));
        assert_eq!(level.letter_at((0, 0)), 'p');

        assert_eq!(level.paint(&[(1, 0), (2, 0), (9, 9)], 'c'), 2);
        assert!(
            !level.set((1, 0), 'c'),
            "the same letter again is no change"
        );
        assert!(level.place_thing((3, 0), 'p'), "the Start moves");
        assert_eq!(level.text(), ".ccp\n####\n");
        assert_eq!(level.fill((0, 1), '.'), 4);
        assert!(level.move_thing((3, 0), (0, 1)));
        assert_eq!(level.text(), ".cc.\np...\n");
        assert_eq!(level.move_area((1, 0), (2, 1), (2, 1)), 4);
        assert_eq!(level.text(), "....\np.cc\n");
        assert!(level.resize(5, 3));
        assert_eq!(level.text(), ".....\np.cc.\n.....\n");
        assert_eq!(level.broken_rules(), Vec::<String>::new());
        assert_eq!(
            level.with_start_at((4, 2)).unwrap(),
            ".....\n..cc.\n....p\n"
        );

        level.save().unwrap();
        assert_eq!(std::fs::read_to_string(&level.path).unwrap(), level.text());
        assert!(!level.reload_if_changed(), "its own save isn't a change");
        std::fs::write(&level.path, "p#\n").unwrap();
        assert!(level.reload_if_changed());
        assert_eq!(level.text(), "p#\n");
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn the_whole_level_and_the_palette_are_drawn() {
        let (folder, level) = level("p.c\n###\n");
        let art = LevelArt::load(&level);
        let picture = art.draw_level(&level);
        assert_eq!((picture.width, picture.height), (48, 32));
        let at = |x: u32, y: u32| picture.pixels[(y * picture.width + x) as usize];
        assert_ne!(at(8, 24), at(8, 8), "a ground tile below, the Start above");
        let start = art.picture_of(level.letter('p').unwrap(), 16);
        assert_eq!(
            start.pixels[8 * 16 + 8] & 0x00ff_ff00,
            0x00ff_cc00,
            "a yellow Start badge"
        );
        std::fs::remove_dir_all(folder).unwrap();
    }
}
