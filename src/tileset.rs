//! Tilesets: the pictures a [`TileMap`](crate::tilemap::TileMap) draws its
//! tiles with, each found by its name.
//!
//! A tileset is a list of named tiles, each an [`Animation`] (one frame
//! for most tiles, several for water or lava). Make one from:
//!
//! - a picture cut into a grid: [`Tileset::grid`], naming the tiles left
//!   to right, top to bottom;
//! - a Duck Quill sprite sheet: [`Tileset::load`], where each animation is
//!   a tile named after it (Duck Quill has no tileset export yet; this
//!   uses its sprite sheet export as it is);
//! - or tile by tile, with [`Tileset::insert`].
//!
//! Each of these only fills the list, so a new kind of file (a Duck Quill
//! tileset export, say) is one more way to fill it.

use std::path::Path;

use crate::error::LoadError;
use crate::sprite::{Animation, Sprite};
use crate::spritesheet::SpriteSheet;

/// Named tile pictures. See the [module docs](self).
#[derive(Debug, Clone, Default)]
pub struct Tileset {
    /// In the order they were added.
    tiles: Vec<(String, Animation)>,
}

impl Tileset {
    /// An empty tileset, to [`insert`](Tileset::insert) tiles into.
    pub fn new() -> Tileset {
        Tileset::default()
    }

    /// Cuts `sheet` into `tile_width` x `tile_height` tiles and names them
    /// in order, left to right, then top to bottom. An empty name (`""`)
    /// skips a tile; tiles after the last name are left out.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let sheet = Sprite::from_pixels(48, 16, vec![Color::GREEN; 48 * 16]);
    /// // let sheet = Sprite::load_png("assets/tiles.png")?;
    /// let tiles = Tileset::grid(&sheet, 16, 16, &["ground", "", "plank"]);
    /// assert_eq!(tiles.names().collect::<Vec<_>>(), ["ground", "plank"]);
    /// ```
    pub fn grid(sheet: &Sprite, tile_width: usize, tile_height: usize, names: &[&str]) -> Tileset {
        let mut tiles = Tileset::new();
        for (name, picture) in names.iter().zip(sheet.frames(tile_width, tile_height)) {
            if !name.is_empty() {
                tiles.insert(name, Animation::new(vec![picture], 1.0));
            }
        }
        tiles
    }

    /// Reads a Duck Quill sprite sheet (`quill export tiles.json --sheet`)
    /// as a tileset: each animation is a tile with the animation's name,
    /// and one with several frames is an animated tile.
    pub fn load(path: impl AsRef<Path>) -> Result<Tileset, LoadError> {
        Ok(Tileset::from_sheet(&SpriteSheet::load(path)?))
    }

    /// The same, from a sprite sheet already loaded.
    pub fn from_sheet(sheet: &SpriteSheet) -> Tileset {
        let mut tiles = Tileset::new();
        for name in sheet.names() {
            tiles.insert(name, sheet.animation(name).clone());
        }
        tiles
    }

    /// Adds the tile `name` (or replaces it, if there is one).
    pub fn insert(&mut self, name: &str, picture: Animation) {
        match self.tiles.iter_mut().find(|(n, _)| n == name) {
            Some(tile) => tile.1 = picture,
            None => self.tiles.push((name.to_string(), picture)),
        }
    }

    /// The tile called `name`, if there is one.
    pub fn get(&self, name: &str) -> Option<&Animation> {
        self.tiles
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, picture)| picture)
    }

    /// Every tile's name, in the order they were added.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tiles.iter().map(|(name, _)| name.as_str())
    }

    /// The picture of tile `name` at `time` seconds. Panics, naming the
    /// tiles there are, if there's no such tile.
    #[track_caller]
    pub fn frame(&self, name: &str, time: f32) -> &Sprite {
        match self.get(name) {
            Some(picture) => picture.frame_at(time),
            None => panic!(
                "the tileset has no tile {name:?}; it has: {}",
                self.names().collect::<Vec<_>>().join(", ")
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    #[test]
    fn a_grid_is_cut_and_named_in_order() {
        let mut pixels = vec![Color::GREEN; 8 * 4];
        for y in 0..4 {
            for x in 4..8 {
                pixels[y * 8 + x] = Color::BLUE;
            }
        }
        let sheet = Sprite::from_pixels(8, 4, pixels);
        let tiles = Tileset::grid(&sheet, 4, 4, &["ground", "water"]);
        assert_eq!(tiles.frame("water", 0.0).get_pixel(0, 0), Some(Color::BLUE));
        assert_eq!(
            tiles.frame("ground", 9.0).get_pixel(3, 3),
            Some(Color::GREEN)
        );
        assert!(tiles.get("lava").is_none());
    }

    #[test]
    fn a_duck_quill_sheet_is_a_tileset_with_animated_tiles() {
        let tiles = Tileset::load("tests/fixtures/quill/scale1/duck.json").unwrap();
        assert_eq!(tiles.names().collect::<Vec<_>>(), ["idle", "jump"]);
        let idle = tiles.get("idle").unwrap();
        assert_eq!(idle.frames.len(), 2, "two frames: an animated tile");
        assert_ne!(
            tiles.frame("idle", 0.0).get_pixel(8, 8),
            None,
            "frames are 16 x 16"
        );
        // At 4 frames a second, the second frame shows from 0.25 s.
        assert!(std::ptr::eq(tiles.frame("idle", 0.3), &idle.frames[1]));
    }
}
