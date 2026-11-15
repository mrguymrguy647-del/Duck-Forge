//! Block worlds, like Minecraft's: a grid of blocks, drawn as one mesh per
//! chunk with only the faces you could see.
//!
//! Two parts, so the world can exist without any pictures (a test can
//! build and dig a world with no art loaded):
//!
//! - [`BlockWorld`] holds the blocks: a number per cell, `AIR` (0) or the
//!   kind of block. Change one with [`set`](BlockWorld::set), and the
//!   chunk it's in (and the next chunk, if it's on the edge) is marked to
//!   be built again.
//! - [`BlockPalette`] says how each kind looks: three tiles from one atlas
//!   (a Duck Quill sprite sheet), and whether you can see through it.
//!
//! ```
//! # use duckforge::prelude::*;
//! # let picture = Sprite::from_pixels(32, 16, vec![Color::GREEN; 32 * 16]);
//! # let atlas = Tileset::grid(&picture, 16, 16, &["stone", "glass"]);
//! // let atlas = Tileset::load("assets/blocks.json")?;
//! let palette = BlockPalette::new(
//!     &atlas,
//!     &[
//!         BlockKind::new("stone", ["stone"; 3]),             // block 1
//!         BlockKind::new("glass", ["glass"; 3]).see_through(), // block 2
//!     ],
//! );
//! let stone = palette.id("stone");
//! let mut world = BlockWorld::new(32, 16, 32);
//! world.set_below(stone); // no one sees the floor's underside
//! for z in 0..32 {
//!     for x in 0..32 {
//!         world.set(x, 0, z, stone); // a floor
//!     }
//! }
//! world.update_meshes(&palette); // in `update`, after changing blocks
//! // Chunk (0, 0): 16 x 16 tops, and the 32 sides on the world's edge.
//! // Two triangles a face.
//! assert_eq!(world.chunk_mesh(0, 0).triangles.len(), (16 * 16 + 32) * 2);
//! // In `draw`: world.draw(canvas, &camera);
//! ```
//!
//! Block (x, y, z) fills the cube from (x, y, z) to (x + 1, y + 1, z + 1),
//! y up. A chunk is `chunk_size` x `chunk_size` columns (16 unless you say),
//! the world's full height.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::{Vec2, Vec3, vec3};
use crate::render3d::{Camera3D, GlowMap, Mesh, Transform};
use crate::sprite::Sprite;
use crate::tileset::Tileset;

/// Which kind of block is in a cell: its place in the palette's list,
/// counting from 1. 0 is [`AIR`].
pub type BlockId = u8;

/// The empty cell.
pub const AIR: BlockId = 0;

/// The six directions a face can look, in this order: +x, -x, +y (up),
/// -y (down), +z, -z. [`BlockWorld::face_visible`] uses the same order.
pub const FACE_DIRECTIONS: [(i32, i32, i32); 6] = [
    (1, 0, 0),
    (-1, 0, 0),
    (0, 1, 0),
    (0, -1, 0),
    (0, 0, 1),
    (0, 0, -1),
];

/// How a face of a block is lit, from
/// [`update_meshes_shaded`](BlockWorld::update_meshes_shaded): `shade`
/// darkens it (1 as it is, 0.5 half as bright: a cave), and `glow` keeps it
/// at least that bright whatever the sun does (0 to 1: the light from a
/// lamp nearby, at night). A plain number is a shade with no glow.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceLight {
    pub shade: f32,
    pub glow: f32,
}

impl From<f32> for FaceLight {
    fn from(shade: f32) -> FaceLight {
        FaceLight { shade, glow: 0.0 }
    }
}

/// How one kind of block looks. See the [module docs](self).
#[derive(Debug, Clone, PartialEq)]
pub struct BlockKind {
    /// What the game calls it, for [`BlockPalette::id`].
    pub name: String,
    /// Tile names: `[top, sides, bottom]`, as for [`Mesh::block`].
    pub tiles: [String; 3],
    /// Glass, water, leaves: the blocks next to it show their faces.
    pub see_through: bool,
    /// Two of these side by side show the face between them (leaves).
    /// Without it, the face between two of the same see-through block is
    /// left out (two panes of glass make one window).
    pub inner_faces: bool,
    /// Drawn at full brightness, whatever the light (lamps, lava).
    pub glows: bool,
    /// Glows only in places: chosen colours of its tiles, or where a glow
    /// tile says (ore veins, a jack-o'-lantern's face). See
    /// [`glowing_where`](BlockKind::glowing_where).
    pub glow_mask: Option<GlowMask>,
}

/// Which pixels of a block's tiles glow, for
/// [`BlockKind::glowing_where`] and [`BlockKind::glowing_from`]: from 0
/// (lit as usual) to `strength` (1: full brightness whatever the light).
#[derive(Debug, Clone, PartialEq)]
pub enum GlowMask {
    /// The tiles' pixels of these colours glow.
    Colours { colours: Vec<Color>, strength: f32 },
    /// Glow tiles `[top, sides, bottom]`, the same size as the block's own,
    /// painted where it glows: the brighter the pixel, the more it glows.
    Tiles { tiles: [String; 3], strength: f32 },
}

impl BlockKind {
    /// A solid block called `name`, with tiles `[top, sides, bottom]`.
    pub fn new(name: &str, tiles: [&str; 3]) -> BlockKind {
        BlockKind {
            name: name.to_string(),
            tiles: tiles.map(String::from),
            see_through: false,
            inner_faces: false,
            glows: false,
            glow_mask: None,
        }
    }

    /// You can see through it (its tiles have see-through pixels, or it's
    /// water): the blocks next to it show their faces.
    pub fn see_through(mut self) -> BlockKind {
        self.see_through = true;
        self
    }

    /// See-through, and showing the faces between two of its own kind,
    /// like leaves (you see the leaves behind the leaves).
    pub fn inner_faces(mut self) -> BlockKind {
        self.see_through = true;
        self.inner_faces = true;
        self
    }

    /// Drawn at full brightness, whatever the light.
    pub fn glowing(mut self) -> BlockKind {
        self.glows = true;
        self
    }

    /// Glows only on these colours of its tiles, written `"#rrggbb"`: the
    /// red veins of an ore, and not the stone round them. Full strength
    /// unless [`glow_strength`](BlockKind::glow_strength) says.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let ore = BlockKind::new("ore", ["ore", "ore", "ore"]).glowing_where(["#ff3030"]);
    /// let dim = BlockKind::new("ember", ["ore", "ore", "ore"])
    ///     .glowing_where(["#ff3030", "#ffa020"])
    ///     .glow_strength(0.6);
    /// ```
    ///
    /// # Panics
    ///
    /// If a colour isn't written `"#rrggbb"`.
    #[track_caller]
    pub fn glowing_where<S: AsRef<str>>(
        mut self,
        colours: impl IntoIterator<Item = S>,
    ) -> BlockKind {
        let colours = colours
            .into_iter()
            .map(|word| {
                let word = word.as_ref();
                Color::from_hex_str(word).unwrap_or_else(|| {
                    panic!(
                        "BlockKind::glowing_where: {word:?} isn't a colour; write it \"#rrggbb\""
                    )
                })
            })
            .collect();
        self.glow_mask = Some(GlowMask::Colours {
            colours,
            strength: self.mask_strength(),
        });
        self
    }

    /// Glows where the atlas tile `tile` says, on every face: a second
    /// tile the same size as its own, black where it doesn't glow and
    /// bright where it does (a lantern's face, a screen's picture).
    pub fn glowing_from(self, tile: &str) -> BlockKind {
        self.glowing_from_tiles([tile, tile, tile])
    }

    /// The same with a glow tile for each of `[top, sides, bottom]`.
    pub fn glowing_from_tiles(mut self, tiles: [&str; 3]) -> BlockKind {
        self.glow_mask = Some(GlowMask::Tiles {
            tiles: tiles.map(String::from),
            strength: self.mask_strength(),
        });
        self
    }

    /// How strongly its glowing pixels glow, 0 to 1 (1 unless you say):
    /// after [`glowing_where`](BlockKind::glowing_where) or
    /// [`glowing_from`](BlockKind::glowing_from).
    pub fn glow_strength(mut self, strength: f32) -> BlockKind {
        if let Some(GlowMask::Colours { strength: s, .. } | GlowMask::Tiles { strength: s, .. }) =
            &mut self.glow_mask
        {
            *s = strength.clamp(0.0, 1.0);
        }
        self
    }

    fn mask_strength(&self) -> f32 {
        match &self.glow_mask {
            Some(GlowMask::Colours { strength, .. } | GlowMask::Tiles { strength, .. }) => {
                *strength
            }
            None => 1.0,
        }
    }
}

/// The atlas's glow map for every kind with a [`GlowMask`], or `None` if
/// none has one. A pixel two kinds light glows the more of the two.
#[track_caller]
fn glow_map_of(atlas: &Tileset, kinds: &[BlockKind], sheet: &Sprite) -> Option<GlowMap> {
    let mut map: Option<GlowMap> = None;
    for kind in kinds {
        let Some(mask) = &kind.glow_mask else {
            continue;
        };
        let map = map.get_or_insert_with(|| GlowMap::new(sheet.width(), sheet.height()));
        for (face, tile_name) in kind.tiles.iter().enumerate() {
            let (_, [left, top, w, h]) = atlas.frame(tile_name, 0.0).in_sheet();
            let mut raise = |x: usize, y: usize, level: f32| {
                if level > map.level(left + x, top + y) {
                    map.set(left + x, top + y, level);
                }
            };
            match mask {
                GlowMask::Colours { colours, strength } => {
                    let wanted: Vec<u32> = colours.iter().map(|c| c.to_u32()).collect();
                    for y in 0..h {
                        for x in 0..w {
                            let (rgb, alpha) = sheet.texel(left + x, top + y);
                            if alpha >= 128 && wanted.contains(&(rgb & 0x00ff_ffff)) {
                                raise(x, y, *strength);
                            }
                        }
                    }
                }
                GlowMask::Tiles { tiles, strength } => {
                    let glow = atlas.frame(&tiles[face], 0.0);
                    assert!(
                        glow.width() == w && glow.height() == h,
                        "BlockPalette::new: block {:?}'s glow tile {:?} is {}x{}, but its tile {:?} \
                         is {w}x{h}; make them the same size",
                        kind.name,
                        tiles[face],
                        glow.width(),
                        glow.height(),
                        tile_name
                    );
                    let levels = GlowMap::from_picture(glow, *strength);
                    for y in 0..h {
                        for x in 0..w {
                            raise(x, y, levels.level(x, y));
                        }
                    }
                }
            }
        }
    }
    map
}

/// One face of a block, ready to copy into a chunk: corners round the
/// block's middle, two triangles, and where each corner is in the atlas.
#[derive(Debug, Clone, Default)]
struct Face {
    corners: Vec<Vec3>,
    triangles: Vec<[usize; 3]>,
    uvs: Vec<[Vec2; 3]>,
}

/// How every kind of block looks: tiles from one atlas. See the
/// [module docs](self).
#[derive(Debug, Clone)]
pub struct BlockPalette {
    /// Index 0 is air (`None`); kind `n` is at index `n`.
    kinds: Vec<Option<(BlockKind, [Face; 6])>>,
    texture: Sprite,
    /// Which atlas pixels glow, for the kinds with a [`GlowMask`].
    glow_map: Option<GlowMap>,
}

impl BlockPalette {
    /// The kinds of block, numbered from 1 in this order (0 is
    /// [`AIR`]). Every tile must be in `atlas`, and from one picture.
    ///
    /// # Panics
    ///
    /// If a tile isn't in the atlas, the tiles come from different
    /// pictures, there are no kinds or more than 255, two have the same
    /// name, or a glow tile isn't the size of the tile it lights.
    #[track_caller]
    pub fn new(atlas: &Tileset, kinds: &[BlockKind]) -> BlockPalette {
        assert!(
            !kinds.is_empty() && kinds.len() <= 255,
            "BlockPalette::new: give 1 to 255 kinds of block, not {}",
            kinds.len()
        );
        let mut texture: Option<Sprite> = None;
        let mut list = vec![None];
        for (i, kind) in kinds.iter().enumerate() {
            assert!(
                !kinds[..i].iter().any(|other| other.name == kind.name),
                "BlockPalette::new: two kinds of block are called {:?}",
                kind.name
            );
            let tiles = [&kind.tiles[0], &kind.tiles[1], &kind.tiles[2]].map(String::as_str);
            let cube = Mesh::block(atlas, tiles);
            let picture = cube.texture.clone().expect("Mesh::block has a texture");
            if let Some(first) = &texture {
                assert!(
                    first.same_pixels(&picture),
                    "BlockPalette::new: block {:?}'s tiles are from a different picture than \
                     the others; put every block's tiles in one atlas",
                    kind.name
                );
            }
            texture = Some(picture);
            list.push(Some((kind.clone(), faces_of(&cube))));
        }
        let texture = texture.expect("at least one kind");
        let glow_map = glow_map_of(atlas, kinds, &texture);
        BlockPalette {
            kinds: list,
            texture,
            glow_map,
        }
    }

    /// The number of the block called `name`, to [`set`](BlockWorld::set).
    ///
    /// # Panics
    ///
    /// If there's no such kind (the message lists them).
    #[track_caller]
    pub fn id(&self, name: &str) -> BlockId {
        match self
            .kinds
            .iter()
            .position(|kind| matches!(kind, Some((kind, _)) if kind.name == name))
        {
            Some(id) => id as BlockId,
            None => panic!(
                "the block palette has no block {name:?}; it has: {}",
                self.kinds
                    .iter()
                    .flatten()
                    .map(|(kind, _)| kind.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }

    /// How block `id` looks, or `None` for air (and numbers past the last kind).
    pub fn kind(&self, id: BlockId) -> Option<&BlockKind> {
        self.kinds
            .get(id as usize)
            .and_then(|kind| kind.as_ref())
            .map(|(kind, _)| kind)
    }

    /// The atlas picture every block is painted from.
    pub fn texture(&self) -> &Sprite {
        &self.texture
    }

    /// Which pixels of the atlas glow, if any kind has a [`GlowMask`].
    pub fn glow_map(&self) -> Option<&GlowMap> {
        self.glow_map.as_ref()
    }

    /// Whether a block's face shows when `neighbour` is next to it: next
    /// to air, or to a see-through block (but not between two of the same
    /// see-through kind, unless it has [`inner_faces`](BlockKind::inner_faces)).
    pub fn face_shows(&self, block: BlockId, neighbour: BlockId) -> bool {
        match self.kind(neighbour) {
            None => true,
            Some(next) if !next.see_through => false,
            Some(next) => block != neighbour || next.inner_faces,
        }
    }
}

/// A grid of blocks, and the meshes that draw it. See the [module docs](self).
#[derive(Debug, Clone)]
pub struct BlockWorld {
    size: [usize; 3],
    chunk_size: usize,
    /// Chunks across x and along z.
    chunks_x: usize,
    chunks_z: usize,
    blocks: Vec<BlockId>,
    /// What [`get`](BlockWorld::get) gives below the world.
    below: BlockId,
    /// Each chunk's mesh.
    meshes: Vec<Mesh>,
    /// Each chunk's mesh's lowest and highest corner, worked out when it's
    /// built, for skipping chunks hidden in fog.
    bounds: Vec<(Vec3, Vec3)>,
    /// Which chunks need building again.
    changed: Vec<bool>,
}

impl BlockWorld {
    /// A world `width` x `height` x `depth` blocks, all air, in chunks of
    /// 16 x 16 columns.
    ///
    /// # Panics
    ///
    /// If a side is 0.
    #[track_caller]
    pub fn new(width: usize, height: usize, depth: usize) -> BlockWorld {
        BlockWorld::with_chunk_size(width, height, depth, 16)
    }

    /// The same, with chunks of `chunk_size` x `chunk_size` columns. Bigger
    /// chunks mean fewer meshes to draw but more to build after each change.
    ///
    /// # Panics
    ///
    /// If a side or `chunk_size` is 0.
    #[track_caller]
    pub fn with_chunk_size(
        width: usize,
        height: usize,
        depth: usize,
        chunk_size: usize,
    ) -> BlockWorld {
        assert!(
            width > 0 && height > 0 && depth > 0 && chunk_size > 0,
            "BlockWorld: every side and the chunk size must be at least 1"
        );
        let chunks_x = width.div_ceil(chunk_size);
        let chunks_z = depth.div_ceil(chunk_size);
        BlockWorld {
            size: [width, height, depth],
            chunk_size,
            chunks_x,
            chunks_z,
            blocks: vec![AIR; width * height * depth],
            below: AIR,
            meshes: vec![Mesh::new(); chunks_x * chunks_z],
            bounds: vec![(Vec3::ZERO, Vec3::ZERO); chunks_x * chunks_z],
            changed: vec![true; chunks_x * chunks_z],
        }
    }

    /// Says what's below the world (`get` with y below 0): [`AIR`] unless
    /// you say. Give it a solid block (bedrock) and the bottom layer's
    /// undersides, which no one can see, aren't drawn.
    pub fn set_below(&mut self, block: BlockId) {
        self.below = block;
        self.changed.fill(true);
    }

    /// Width, height and depth, in blocks.
    pub fn size(&self) -> [usize; 3] {
        self.size
    }

    /// How many columns a chunk is across (and along).
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// How many chunks there are across x, and along z.
    pub fn chunks(&self) -> (usize, usize) {
        (self.chunks_x, self.chunks_z)
    }

    /// Every cell's block, x fastest, then z, then y (layer by layer from
    /// the bottom): the cell (x, y, z) is at `(y * depth + z) * width + x`.
    /// For counting blocks, or saving the world.
    pub fn blocks(&self) -> &[BlockId] {
        &self.blocks
    }

    /// Whether (x, y, z) is a cell of the world.
    pub fn inside(&self, x: i32, y: i32, z: i32) -> bool {
        let [w, h, d] = self.size;
        (0..w as i32).contains(&x) && (0..h as i32).contains(&y) && (0..d as i32).contains(&z)
    }

    fn index(&self, x: i32, y: i32, z: i32) -> usize {
        let [w, _, d] = self.size;
        (y as usize * d + z as usize) * w + x as usize
    }

    /// The block at (x, y, z): [`AIR`] outside the world, except below it
    /// (see [`set_below`](BlockWorld::set_below)).
    pub fn get(&self, x: i32, y: i32, z: i32) -> BlockId {
        if self.inside(x, y, z) {
            self.blocks[self.index(x, y, z)]
        } else if y < 0 {
            self.below
        } else {
            AIR
        }
    }

    /// Puts `block` at (x, y, z) ([`AIR`] digs it out). Its chunk is built
    /// again at the next [`update_meshes`](BlockWorld::update_meshes), and
    /// so is the chunk next door when the cell is on a chunk's edge (its
    /// face there may now show, or hide). Cells outside the world are
    /// ignored.
    pub fn set(&mut self, x: i32, y: i32, z: i32, block: BlockId) {
        if !self.inside(x, y, z) {
            return;
        }
        let index = self.index(x, y, z);
        if self.blocks[index] == block {
            return;
        }
        self.blocks[index] = block;
        self.mark_changed(x, z);
    }

    /// Marks the chunk holding column (x, z) to be built again, and its
    /// neighbours if the column is on its edge. `set` does this; call it
    /// yourself when something else a chunk's look depends on changed
    /// (like the light your `shade` reads).
    pub fn mark_changed(&mut self, x: i32, z: i32) {
        let n = self.chunk_size as i32;
        for (dx, dz) in [(0, 0), (-1, 0), (1, 0), (0, -1), (0, 1)] {
            let (cx, cz) = ((x + dx).div_euclid(n), (z + dz).div_euclid(n));
            if (0..self.chunks_x as i32).contains(&cx) && (0..self.chunks_z as i32).contains(&cz) {
                self.changed[cz as usize * self.chunks_x + cx as usize] = true;
            }
        }
    }

    /// Builds again every chunk that changed since last time, and says how
    /// many. Call it in `update` after changing blocks (it's quick when
    /// nothing changed). A new world builds every chunk the first time.
    pub fn update_meshes(&mut self, palette: &BlockPalette) -> usize {
        self.update_meshes_shaded(palette, |_, _, _, _| 1.0)
    }

    /// The same, with each face lit by `shade`: it's given the world and
    /// the cell the face looks into, and gives a brightness from 0 to 1
    /// (darker in a cave), or a [`FaceLight`] with a glow too (lit by a
    /// lamp). The scene's [`Lighting`](crate::render3d::Lighting) still
    /// shades by direction on top.
    pub fn update_meshes_shaded<L: Into<FaceLight>>(
        &mut self,
        palette: &BlockPalette,
        shade: impl Fn(&BlockWorld, i32, i32, i32) -> L,
    ) -> usize {
        let mut built = 0;
        for cz in 0..self.chunks_z {
            for cx in 0..self.chunks_x {
                let i = cz * self.chunks_x + cx;
                if self.changed[i] {
                    self.meshes[i] = self.build_chunk(palette, cx, cz, &shade);
                    self.bounds[i] = self.meshes[i].bounds();
                    self.changed[i] = false;
                    built += 1;
                }
            }
        }
        built
    }

    /// One chunk's mesh. Glowing blocks glow triangle by triangle.
    fn build_chunk<L: Into<FaceLight>>(
        &self,
        palette: &BlockPalette,
        cx: usize,
        cz: usize,
        shade: &impl Fn(&BlockWorld, i32, i32, i32) -> L,
    ) -> Mesh {
        let mut mesh = Mesh::new().with_texture(palette.texture.clone());
        if let Some(map) = &palette.glow_map {
            mesh = mesh.with_glow_map(map.clone());
        }
        let n = self.chunk_size;
        let [w, h, d] = self.size;
        for y in 0..h as i32 {
            for z in (cz * n).min(d) as i32..((cz + 1) * n).min(d) as i32 {
                for x in (cx * n).min(w) as i32..((cx + 1) * n).min(w) as i32 {
                    let block = self.get(x, y, z);
                    let Some(Some((kind, faces))) = palette.kinds.get(block as usize) else {
                        continue; // air, or a number the palette doesn't have
                    };
                    let middle = vec3(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                    for (face, &(dx, dy, dz)) in faces.iter().zip(&FACE_DIRECTIONS) {
                        let (nx, ny, nz) = (x + dx, y + dy, z + dz);
                        if !palette.face_shows(block, self.get(nx, ny, nz)) {
                            continue;
                        }
                        let light: FaceLight = shade(self, nx, ny, nz).into();
                        let bright = (light.shade.clamp(0.0, 1.0) * 255.0) as u8;
                        let color = Color::rgb(bright, bright, bright);
                        let glow = if kind.glows { 1.0 } else { light.glow };
                        let first = mesh.vertices.len();
                        for &corner in &face.corners {
                            mesh.add_vertex(corner + middle);
                        }
                        for (corners, uvs) in face.triangles.iter().zip(&face.uvs) {
                            let [a, b, c] = corners.map(|i| first + i);
                            mesh.add_triangle_uv(a, b, c, *uvs, color);
                            if glow > 0.0 {
                                mesh.set_glow(mesh.triangles.len() - 1, glow);
                            }
                            if palette.glow_map.is_some() {
                                mesh.texel_glow.push(kind.glow_mask.is_some());
                            }
                        }
                    }
                }
            }
        }
        mesh
    }

    /// The mesh of chunk (cx, cz), as last built (by
    /// [`update_meshes`](BlockWorld::update_meshes)). Glowing blocks glow
    /// triangle by triangle ([`Mesh::glow_of`]).
    ///
    /// # Panics
    ///
    /// If there's no such chunk.
    pub fn chunk_mesh(&self, cx: usize, cz: usize) -> &Mesh {
        &self.meshes[self.chunk_index(cx, cz)]
    }

    #[track_caller]
    fn chunk_index(&self, cx: usize, cz: usize) -> usize {
        assert!(
            cx < self.chunks_x && cz < self.chunks_z,
            "there's no chunk ({cx}, {cz}): the world has {} x {}",
            self.chunks_x,
            self.chunks_z
        );
        cz * self.chunks_x + cx
    }

    /// Whether a chunk changed since its meshes were last built.
    pub fn chunk_changed(&self, cx: usize, cz: usize) -> bool {
        self.changed[self.chunk_index(cx, cz)]
    }

    /// Draws every chunk as last built, and says how many it drew. Chunks
    /// out of the camera's view are skipped whole, and so are chunks lost
    /// in the canvas's fog ([`Canvas::set_fog`]): every point of them is
    /// past the fog's `far` edge, where they'd be drawn as nothing but
    /// fog. A world 192 blocks across is 144 chunks, mostly sea; with fog
    /// only the ones you could see are drawn.
    ///
    /// What that does to the picture: where a skipped chunk would have
    /// been, the background shows (whatever was drawn before the world,
    /// like the sky) instead of the fog's colour. Clear to the fog's colour,
    /// or make the fog the sky's colour, and nothing changes.
    pub fn draw(&self, canvas: &mut Canvas, camera: &Camera3D) -> usize {
        let far = canvas.fog().map_or(f32::INFINITY, |fog| fog.far);
        self.draw_within(canvas, camera, far)
    }

    /// Like [`draw`](BlockWorld::draw), skipping every chunk whose nearest
    /// point is more than `far` ahead of the camera (along the way it
    /// looks, as fog measures distance), fog or not: a view distance.
    /// Chunks behind the camera or below it aren't skipped by this (the
    /// camera can look straight down); the camera's view skips them anyway.
    pub fn draw_within(&self, canvas: &mut Canvas, camera: &Camera3D, far: f32) -> usize {
        let eye = camera.position;
        let (x, y, z) = (
            eye.x.floor() as i32,
            eye.y.floor() as i32,
            eye.z.floor() as i32,
        );
        if self.get(x, y, z) != AIR {
            canvas.note_inside(format!("a block at {x}, {y}, {z}"));
        }
        let mut drawn = 0;
        for (mesh, &(low, high)) in self.meshes.iter().zip(&self.bounds) {
            if mesh.triangles.is_empty() || nearest_ahead(camera, low, high) > far {
                continue;
            }
            canvas.draw_mesh(mesh, &Transform::default(), camera);
            drawn += 1;
        }
        drawn
    }

    /// Whether the face of the block at (x, y, z) looking in direction
    /// `face` (an index into [`FACE_DIRECTIONS`]) is drawn.
    pub fn face_visible(
        &self,
        palette: &BlockPalette,
        x: i32,
        y: i32,
        z: i32,
        face: usize,
    ) -> bool {
        let block = self.get(x, y, z);
        let (dx, dy, dz) = FACE_DIRECTIONS[face];
        palette.kind(block).is_some() && palette.face_shows(block, self.get(x + dx, y + dy, z + dz))
    }
}

/// How far ahead of the camera, along the way it looks, the nearest point
/// of the box from `low` to `high` is (negative if some of it is behind the
/// camera). That's the distance fog uses, so a box whose nearest point is
/// past the fog's far edge is all fog.
fn nearest_ahead(camera: &Camera3D, low: Vec3, high: Vec3) -> f32 {
    let ahead = camera.forward();
    // The corner furthest back along `ahead`, axis by axis.
    let corner = vec3(
        if ahead.x > 0.0 { low.x } else { high.x },
        if ahead.y > 0.0 { low.y } else { high.y },
        if ahead.z > 0.0 { low.z } else { high.z },
    );
    (corner - camera.position).dot(ahead)
}

/// Splits [`Mesh::block`]'s cube into its six faces, in
/// [`FACE_DIRECTIONS`] order, so a chunk can copy just the ones that show.
fn faces_of(cube: &Mesh) -> [Face; 6] {
    let mut faces: [Face; 6] = Default::default();
    for (i, triangle) in cube.triangles.iter().enumerate() {
        let points = triangle.corners.map(|c| cube.vertices[c]);
        let middle = (points[0] + points[1] + points[2]) * (1.0 / 3.0);
        // The cube is 1 wide round (0, 0, 0): a face's middle is 0.5 out.
        let direction = FACE_DIRECTIONS
            .iter()
            .position(|&(dx, dy, dz)| vec3(dx as f32, dy as f32, dz as f32).dot(middle) > 0.49)
            .expect("every face of the cube is half a block out");
        let face = &mut faces[direction];
        // Share corners between the face's two triangles.
        let corners =
            points.map(
                |p| match face.corners.iter().position(|c| c.distance(p) < 1e-4) {
                    Some(index) => index,
                    None => {
                        face.corners.push(p);
                        face.corners.len() - 1
                    }
                },
            );
        face.triangles.push(corners);
        face.uvs
            .push(cube.uvs[i].expect("Mesh::block gives every triangle texture points"));
    }
    faces
}

/// The ore's stone and its veins, for the glow-mask tests.
#[cfg(test)]
pub(crate) const ORE_STONE: Color = Color::from_hex(0x60_60_60);
#[cfg(test)]
pub(crate) const ORE_VEIN: Color = Color::from_hex(0xff_30_30);

/// An atlas of two 4 x 4 tiles: "ore" (grey stone with four red vein
/// pixels down a diagonal) and "glow" (black, one white pixel top left),
/// and a palette of "ore" (glowing where it's red), "rock" (the same tile,
/// no glow) and "lantern" (the ore tile, glowing where "glow" says, at
/// half strength).
#[cfg(test)]
pub(crate) fn ore_palette() -> BlockPalette {
    let mut pixels = vec![Color::BLACK; 8 * 4];
    for y in 0..4 {
        for x in 0..4 {
            pixels[y * 8 + x] = if x == y { ORE_VEIN } else { ORE_STONE };
        }
    }
    pixels[4] = Color::WHITE;
    let atlas = Tileset::grid(&Sprite::from_pixels(8, 4, pixels), 4, 4, &["ore", "glow"]);
    BlockPalette::new(
        &atlas,
        &[
            BlockKind::new("ore", ["ore"; 3]).glowing_where(["#ff3030"]),
            BlockKind::new("rock", ["ore"; 3]),
            BlockKind::new("lantern", ["ore"; 3])
                .glowing_from("glow")
                .glow_strength(0.5),
        ],
    )
}

/// An ore block (left) and a rock block with the same tile (right),
/// face on, at night: the sun behind them and a dim ambient light. Only
/// the ore's red veins should come out bright.
#[cfg(test)]
pub(crate) fn draw_ore_scene(canvas: &mut Canvas) {
    let palette = ore_palette();
    let mut world = BlockWorld::new(3, 1, 1);
    world.set(0, 0, 0, palette.id("ore"));
    world.set(2, 0, 0, palette.id("rock"));
    world.update_meshes(&palette);
    canvas.clear(Color::BLACK);
    canvas.set_lighting(crate::render3d::Lighting {
        sun_direction: vec3(0.0, 0.0, 1.0),
        ambient: 0.1,
        ..crate::render3d::Lighting::default()
    });
    let camera = Camera3D::new(vec3(1.5, 0.5, -2.2));
    world.draw(canvas, &camera);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three 2 x 2 tiles side by side: stone (grey), glass (blue), leaves (green).
    fn palette() -> BlockPalette {
        let mut pixels = Vec::new();
        for _ in 0..2 {
            for color in [Color::GRAY, Color::BLUE, Color::GREEN] {
                pixels.extend([color, color]);
            }
        }
        let atlas = Tileset::grid(
            &Sprite::from_pixels(6, 2, pixels),
            2,
            2,
            &["stone", "glass", "leaves"],
        );
        BlockPalette::new(
            &atlas,
            &[
                BlockKind::new("stone", ["stone"; 3]),
                BlockKind::new("glass", ["glass"; 3]).see_through(),
                BlockKind::new("leaves", ["leaves"; 3]).inner_faces(),
                BlockKind::new("lamp", ["stone"; 3]).glowing(),
            ],
        )
    }

    fn triangles(world: &BlockWorld) -> usize {
        world.chunk_mesh(0, 0).triangles.len()
    }

    #[test]
    fn only_faces_next_to_air_are_built() {
        let palette = palette();
        let stone = palette.id("stone");
        let mut world = BlockWorld::new(8, 8, 8);
        world.set(3, 3, 3, stone);
        world.update_meshes(&palette);
        assert_eq!(triangles(&world), 12, "a lone block: 6 faces");
        world.set(4, 3, 3, stone);
        world.update_meshes(&palette);
        assert_eq!(
            triangles(&world),
            20,
            "two side by side: the faces between them go"
        );
        // Bury the first block: only the second's outside faces are left.
        for (dx, dy, dz) in FACE_DIRECTIONS {
            world.set(3 + dx, 3 + dy, 3 + dz, stone);
        }
        world.update_meshes(&palette);
        assert!(!world.face_visible(&palette, 3, 3, 3, 0));
        assert!(world.face_visible(&palette, 4, 3, 3, 0));
        // 7 blocks in a plus shape: 6 arms x 5 faces each.
        assert_eq!(triangles(&world), 6 * 5 * 2);
    }

    #[test]
    fn see_through_blocks_show_their_neighbours_faces() {
        let palette = palette();
        let (stone, glass, leaves) = (
            palette.id("stone"),
            palette.id("glass"),
            palette.id("leaves"),
        );
        assert!(palette.face_shows(stone, AIR));
        assert!(palette.face_shows(stone, glass), "stone behind glass");
        assert!(!palette.face_shows(glass, stone));
        assert!(
            !palette.face_shows(glass, glass),
            "two panes make one window"
        );
        assert!(palette.face_shows(leaves, leaves), "leaves behind leaves");
        assert!(palette.face_shows(glass, leaves));
    }

    #[test]
    fn faces_are_painted_with_their_tile() {
        let palette = palette();
        let mut world = BlockWorld::new(4, 4, 4);
        world.set(1, 1, 1, palette.id("leaves"));
        world.update_meshes(&palette);
        let mesh = world.chunk_mesh(0, 0);
        assert!(
            mesh.texture
                .as_ref()
                .unwrap()
                .same_pixels(palette.texture())
        );
        // Every texture point is inside the leaves tile: x from 4 to 6 of 6.
        for i in 0..mesh.triangles.len() {
            for point in mesh.uvs_of(i).unwrap() {
                assert!((4.0 / 6.0..=1.0).contains(&point.x), "{point:?}");
                assert!((0.0..=1.0).contains(&point.y));
            }
        }
        // Corners are where the block is.
        let (low, high) = mesh.bounds();
        assert_eq!((low, high), (vec3(1.0, 1.0, 1.0), vec3(2.0, 2.0, 2.0)));
    }

    #[test]
    fn a_change_builds_only_its_chunk_and_the_one_next_door() {
        let palette = palette();
        let stone = palette.id("stone");
        let mut world = BlockWorld::new(32, 4, 32);
        assert_eq!(world.chunks(), (2, 2));
        assert_eq!(
            world.update_meshes(&palette),
            4,
            "the first time, all of them"
        );
        assert_eq!(world.update_meshes(&palette), 0, "nothing changed");
        world.set(5, 0, 5, stone);
        assert!(world.chunk_changed(0, 0) && !world.chunk_changed(1, 0));
        assert_eq!(world.update_meshes(&palette), 1);
        world.set(15, 0, 5, stone); // on the edge next to chunk (1, 0)
        assert_eq!(world.update_meshes(&palette), 2);
        world.set(0, 0, 0, stone); // the world's edge: no chunk past it
        assert_eq!(world.update_meshes(&palette), 1);
        world.set(0, 0, 0, stone); // the same block again: no change
        world.set(99, 0, 0, stone); // outside: ignored
        assert_eq!(world.update_meshes(&palette), 0);
        assert_eq!(world.get(99, 0, 0), AIR);
    }

    #[test]
    fn glowing_blocks_shade_and_the_floor_below() {
        let palette = palette();
        let mut world = BlockWorld::new(4, 4, 4);
        world.set(0, 0, 0, palette.id("lamp"));
        world.set(2, 0, 0, palette.id("stone"));
        world.set_below(palette.id("stone"));
        // Darker under y = 1 (the sides), full light above.
        world.update_meshes_shaded(&palette, |_, _, y, _| if y >= 1 { 1.0 } else { 0.5 });
        let mesh = world.chunk_mesh(0, 0);
        assert_eq!(
            mesh.triangles.len(),
            20,
            "no undersides: there's stone below"
        );
        assert!(!mesh.glowing);
        let glowing = (0..20).filter(|&t| mesh.glow_of(t) == 1.0).count();
        assert_eq!(glowing, 10, "the lamp's triangles, and only those");
        let colors: Vec<Color> = mesh.triangles.iter().map(|t| t.color).collect();
        assert!(colors.contains(&Color::rgb(255, 255, 255)), "the top");
        assert!(colors.contains(&Color::rgb(127, 127, 127)), "the sides");
        // A face can glow a little: lit by the lamp next to it.
        world.set_below(AIR);
        world.update_meshes_shaded(&palette, |_, x, _, _| FaceLight {
            shade: 1.0,
            glow: if x == 1 { 0.5 } else { 0.0 },
        });
        let mesh = world.chunk_mesh(0, 0);
        let half = (0..mesh.triangles.len())
            .filter(|&t| mesh.glow_of(t) == 0.5)
            .count();
        assert_eq!(half, 2, "the stone's face towards the lamp");
    }

    #[test]
    fn a_glow_mask_lights_only_the_chosen_colours_of_a_tile() {
        let palette = ore_palette();
        let map = palette.glow_map().expect("the ore has a glow mask");
        // The ore tile's diagonal is red: those pixels glow fully, the
        // stone round them not at all.
        for y in 0..4 {
            for x in 0..4 {
                let level = map.level(x, y);
                if x == y {
                    assert_eq!(level, 1.0, "the vein at ({x}, {y})");
                } else if (x, y) != (0, 0) {
                    assert_eq!(level, 0.0, "the stone at ({x}, {y})");
                }
            }
        }
        // The glow tile is never drawn, so never glows.
        assert!((4..8).all(|x| (0..4).all(|y| map.level(x, y) == 0.0)));
        // The chunk: only the ore's faces use the map.
        let mut world = BlockWorld::new(3, 1, 1);
        world.set(0, 0, 0, palette.id("ore"));
        world.set(2, 0, 0, palette.id("rock"));
        world.update_meshes(&palette);
        let mesh = world.chunk_mesh(0, 0);
        assert_eq!(mesh.texel_glow.len(), mesh.triangles.len());
        assert_eq!(mesh.texel_glow.iter().filter(|&&on| on).count(), 12);
        assert!((0..mesh.triangles.len()).all(|t| mesh.glow_of(t) == 0.0));
        // Kinds without a mask: no map at all.
        assert!(super::tests::palette().glow_map().is_none());
    }

    #[test]
    fn a_glow_tile_says_where_and_glow_strength_how_much() {
        let palette = ore_palette();
        let map = palette.glow_map().unwrap();
        // The lantern's glow tile is white top left only, at half
        // strength; the ore's vein there glows fully, so the more wins.
        assert_eq!(map.level(0, 0), 1.0);
        let kind = palette.kind(palette.id("lantern")).unwrap();
        assert_eq!(
            kind.glow_mask,
            Some(GlowMask::Tiles {
                tiles: ["glow".to_string(), "glow".to_string(), "glow".to_string()],
                strength: 0.5
            })
        );
        // On its own, the lantern glows by half where the tile is white.
        let atlas = Tileset::grid(
            &Sprite::from_pixels(
                8,
                4,
                (0..32)
                    .map(|i| match i {
                        4 => Color::WHITE,
                        _ if i % 8 >= 4 => Color::BLACK,
                        _ => Color::GRAY,
                    })
                    .collect(),
            ),
            4,
            4,
            &["face", "glow"],
        );
        let lantern = BlockPalette::new(
            &atlas,
            &[BlockKind::new("lantern", ["face"; 3])
                .glowing_from("glow")
                .glow_strength(0.5)],
        );
        let map = lantern.glow_map().unwrap();
        assert!((map.level(0, 0) - 0.5).abs() < 0.01);
        assert_eq!(map.level(1, 0), 0.0);
    }

    #[test]
    #[should_panic(expected = "isn't a colour")]
    fn a_glow_colour_must_be_written_as_hex() {
        let _ = BlockKind::new("ore", ["ore"; 3]).glowing_where(["red"]);
    }

    #[test]
    fn only_the_vein_pixels_come_out_bright() {
        let mut canvas = Canvas::new(96, 48);
        draw_ore_scene(&mut canvas);
        canvas.flush_3d();
        let (mut bright, mut face, mut bright_on_the_rock) = (0, 0, 0);
        for y in 0..48 {
            for x in 0..96 {
                let c = canvas.get_pixel(x, y).unwrap();
                if c == Color::BLACK {
                    continue;
                }
                let brightest = c.r.max(c.g).max(c.b);
                if brightest > 0x40 {
                    // A glowing pixel is the vein's own colour, exactly.
                    assert_eq!(c, ORE_VEIN, "({x}, {y})");
                    bright += 1;
                    if x >= 48 {
                        bright_on_the_rock += 1;
                    }
                } else if x < 48 {
                    face += 1;
                }
            }
        }
        assert!(bright > 0, "the veins glow");
        assert_eq!(
            bright_on_the_rock, 0,
            "the rock, with the same tile, doesn't glow"
        );
        // Four of the sixteen pixels are vein: about a quarter of the face.
        let share = bright as f32 / (bright + face) as f32;
        assert!(
            (0.15..0.35).contains(&share),
            "{:.0}% bright",
            share * 100.0
        );
    }

    #[test]
    fn veins_glow_the_same_in_the_lit_renderer_and_feed_bloom() {
        let plain = {
            let mut canvas = Canvas::new(96, 48);
            draw_ore_scene(&mut canvas);
            canvas.flush_3d();
            canvas.pixels().to_vec()
        };
        // Smooth shading turns the lit renderer on; flat faces look the same.
        let mut lit = Canvas::new(96, 48);
        lit.set_smooth_shading(true);
        draw_ore_scene(&mut lit);
        lit.flush_3d();
        let differ = plain
            .iter()
            .zip(lit.pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(differ, 0, "{differ} pixels differ");
        // With bloom, the veins light the stone round them, and the rock
        // (whose tile has the same red, not glowing) stays dark.
        let mut bloom = Canvas::new(96, 48);
        bloom.set_bloom(Some(crate::lights::Bloom::default()));
        draw_ore_scene(&mut bloom);
        bloom.flush_3d();
        let sum = |pixels: &[u32], left: bool| -> u64 {
            (0..48)
                .flat_map(|y| (0..96).map(move |x| (x, y)))
                .filter(|&(x, _)| (x < 48) == left)
                .map(|(x, y)| {
                    let p = pixels[y * 96 + x];
                    ((p >> 16) & 0xff) as u64 + ((p >> 8) & 0xff) as u64 + (p & 0xff) as u64
                })
                .sum()
        };
        assert!(
            sum(bloom.pixels(), true) > sum(&plain, true),
            "bloom round the veins"
        );
        assert_eq!(
            sum(bloom.pixels(), false),
            sum(&plain, false),
            "none on the rock"
        );
    }

    /// A sea of stone 192 blocks across (144 chunks), seen through fog from
    /// 10 to 30 blocks, from the middle. `look_down` looks straight down.
    fn foggy_world(look_down: bool) -> (BlockWorld, Camera3D, crate::render3d::Fog) {
        let palette = palette();
        let stone = palette.id("stone");
        let mut world = BlockWorld::new(192, 4, 192);
        for z in 0..192 {
            for x in 0..192 {
                world.set(x, 0, z, stone);
            }
        }
        world.update_meshes(&palette);
        let mut camera = Camera3D::new(vec3(96.0, 6.0, 96.0));
        camera.pitch = if look_down {
            -std::f32::consts::FRAC_PI_2
        } else {
            -0.2
        };
        let fog = crate::render3d::Fog {
            color: Color::rgb(200, 220, 255),
            near: 10.0,
            far: 30.0,
        };
        (world, camera, fog)
    }

    #[test]
    fn chunks_lost_in_fog_are_not_drawn_and_the_rest_are() {
        for look_down in [false, true] {
            let (world, camera, fog) = foggy_world(look_down);
            let picture = |all: bool| {
                let mut canvas = Canvas::new(80, 60);
                canvas.clear(Color::BLACK); // not the fog's colour: skipped chunks show
                canvas.set_fog(Some(fog));
                let drawn = if all {
                    world.draw_within(&mut canvas, &camera, f32::INFINITY)
                } else {
                    world.draw(&mut canvas, &camera)
                };
                (drawn, canvas)
            };
            let (drawn, culled) = picture(false);
            let (every, full) = picture(true);
            assert_eq!(every, 144);
            if look_down {
                // Looking down, the whole floor is 6 ahead: nothing is lost in
                // the fog, however far away sideways.
                assert_eq!(drawn, 144);
            } else {
                assert!(drawn < 100, "{drawn} of 144 chunks drawn");
            }
            // Every pixel is the same, except where a chunk that would only
            // have been fog was skipped: there the background shows.
            let fog_colour = fog.color.to_u32();
            let mut stone = 0;
            for (a, b) in culled.pixels().iter().zip(full.pixels()) {
                assert!(a == b || (*a == 0 && *b == fog_colour), "{a:06x} {b:06x}");
                stone += usize::from(*a != 0 && *a != fog_colour);
            }
            assert!(
                stone > 500,
                "the floor inside the fog is still drawn: {stone} pixels"
            );
        }
    }

    #[test]
    fn draw_within_skips_chunks_past_a_view_distance() {
        let (world, camera, _) = foggy_world(false);
        let mut canvas = Canvas::new(80, 60);
        let near = world.draw_within(&mut canvas, &camera, 20.0);
        let further = world.draw_within(&mut canvas, &camera, 60.0);
        assert!(near < further && further < 144, "{near} {further}");
        // Without fog, draw draws them all.
        assert_eq!(world.draw(&mut canvas, &camera), 144);
    }

    #[test]
    #[should_panic(expected = "no block \"dirt\"; it has: stone, glass, leaves, lamp")]
    fn an_unknown_block_name_lists_the_ones_there_are() {
        palette().id("dirt");
    }

    #[test]
    fn a_world_of_blocks_is_drawn() {
        let palette = palette();
        let mut world = BlockWorld::new(3, 3, 3);
        world.set(1, 1, 1, palette.id("leaves"));
        world.update_meshes(&palette);
        let mut canvas = Canvas::new(40, 30);
        canvas.clear(Color::BLACK);
        let mut camera = Camera3D::new(vec3(1.5, 1.5, -2.0));
        camera.look_at(vec3(1.5, 1.5, 1.5));
        world.draw(&mut canvas, &camera);
        let middle = Color::from_hex(canvas.pixels()[15 * 40 + 20]);
        assert!(
            middle.g > middle.r + 40 && middle.g > middle.b + 40,
            "green: {middle:?}"
        );
    }
}
