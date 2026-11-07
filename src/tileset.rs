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
use crate::math::Rect;
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

    /// Tile `name`'s picture (its first frame), sharing the atlas's
    /// pixels: nothing is copied. Use it as a texture that repeats over a
    /// big surface (the sea, clouds, a floor): texture points past 1 start
    /// the tile again, never showing the atlas tiles next to it, on the CPU
    /// and the GPU. Also good for drawing the tile in 2D, like an icon.
    /// Panics like [`frame`](Tileset::frame).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let picture = Sprite::from_pixels(32, 16, vec![Color::BLUE; 32 * 16]);
    /// # let atlas = Tileset::grid(&picture, 16, 16, &["water", "sand"]);
    /// // A 40 x 40 square of sea, the water tile repeated once a block:
    /// let mut sea = Mesh::new().with_texture(atlas.tile("water"));
    /// let corners = [vec3(0.0, 0.0, 0.0), vec3(0.0, 0.0, 40.0), vec3(40.0, 0.0, 40.0), vec3(40.0, 0.0, 0.0)];
    /// let [a, b, c, d] = corners.map(|corner| sea.add_vertex(corner));
    /// let uv = [vec2(0.0, 0.0), vec2(0.0, 40.0), vec2(40.0, 40.0), vec2(40.0, 0.0)];
    /// sea.add_quad_uv(a, b, c, d, uv, Color::WHITE);
    /// ```
    #[track_caller]
    pub fn tile(&self, name: &str) -> Sprite {
        self.frame(name, 0.0).clone()
    }

    /// Where tile `name` is in the atlas picture
    /// ([`whole_picture`](Sprite::whole_picture)), as texture points: the
    /// rectangle's corner and size are fractions (0 to 1) of the picture.
    /// The point `(u, v)` of the tile (each 0 to 1) is at
    /// `(rect.x + u * rect.w, rect.y + v * rect.h)`, for a mesh textured
    /// with the whole picture. Panics like [`frame`](Tileset::frame).
    #[track_caller]
    pub fn uv_rect(&self, name: &str) -> Rect {
        let tile = self.frame(name, 0.0);
        let picture = tile.whole_picture();
        let [left, top, width, height] = tile.place_in_picture();
        let (w, h) = (picture.width() as f32, picture.height() as f32);
        Rect::new(
            left as f32 / w,
            top as f32 / h,
            width as f32 / w,
            height as f32 / h,
        )
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
    fn a_tile_says_where_it_is_in_the_atlas() {
        let sheet = Sprite::from_pixels(64, 32, vec![Color::GREEN; 64 * 32]);
        let tiles = Tileset::grid(&sheet, 16, 16, &["a", "b", "", "", "", "c"]);
        let c = tiles.frame("c", 0.0);
        assert_eq!(c.place_in_picture(), [16, 16, 16, 16]);
        assert_eq!(c.whole_picture().width(), 64);
        assert_eq!(tiles.uv_rect("b"), Rect::new(0.25, 0.0, 0.25, 0.5));
        assert_eq!(tiles.uv_rect("c"), Rect::new(0.25, 0.5, 0.25, 0.5));
        // A sprite that wasn't cut is its own whole picture.
        assert_eq!(sheet.place_in_picture(), [0, 0, 64, 32]);
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
