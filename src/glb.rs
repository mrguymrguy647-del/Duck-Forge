//! Models made with Duck Mold: reading its `.glb` files.
//!
//! [Duck Mold](https://github.com/mrguymrguy647-del/Duck-Mold) builds 3D
//! models from text recipes and saves them as `.glb` files (glTF, the
//! common 3D format, in one file). [`Mesh::load_glb`] loads one as a mesh,
//! ready to draw:
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # fn main() -> Result<(), duckforge::LoadError> {
//! let robot = Mesh::load_glb("assets/robot.glb")?;
//! # Ok(())
//! # }
//! ```
//!
//! [`GlbModel::load`] gives more: the model's name, its palette by name,
//! and which triangles belong to which part, and to which assembly (a
//! part made of parts, like a robot's arm), to recolour one.
//!
//! **Which way a model faces.** Duck Mold's directions are Duck Forge's (x
//! right, y up, z forward, 1 unit about a metre), and a model **faces
//! forward, +z**, standing on y = 0. So its `left` is -x: its own left.
//! A camera made with `Camera3D::new` looks along +z too, so it sees a
//! model from behind: put the camera on the model's +z side looking back
//! at it, or turn the model half round, `rotated(vec3(0.0, PI, 0.0))`.
//!
//! **Where this code comes from.** Everything between the two `// ====`
//! marker lines below is Duck Mold's reader (its
//! `docs/forge/duck_mold_glb.rs`), byte for byte, so the two projects
//! read the file the same way; the first marker says its version. Duck
//! Mold's tests run that reader against every model it makes. Change it
//! there first, then copy the lines between the markers here; Duck Mold's
//! `docs/forge-handoff.md` has the one-line check that the copies are the
//! same. Above the markers are this introduction and the imports; below
//! them, Duck Forge's own [`GlbModel::load`], [`GlbModel::to_mesh`],
//! [`GlbModel::part`], [`GlbPart::range`] and [`Mesh::load_glb`].
//! `docs/decisions.md` ("Models from Duck Mold") says why.
//!
//! The file format, and what is done to its numbers, is in Duck Mold's
//! `docs/forge-handoff.md`. In short: a `.glb` is a 12-byte header, a
//! block of JSON describing the model and a block of plain numbers. glTF's
//! "right" is -x where Duck Forge's is +x, so x is negated and each
//! triangle's last two corners swap, which keeps the corners clockwise
//! seen from the front. Colours are stored as *linear light* (0 to 1),
//! and turned back into the usual 0 to 255 exactly.
//!
//! It reads the narrow kind of `.glb` Duck Mold writes, not glTF in
//! general: anything else is refused with a sentence saying why.

use std::collections::HashMap;
use std::io;
use std::ops::Range;
use std::path::Path;

use crate::color::Color;
use crate::error::LoadError;
use crate::math::vec3;
use crate::render3d::Mesh;

// ==== Duck Mold's .glb reader, version 2 ====================================
//
// Version 2: a part made of parts (an "assembly") has a triangle range too,
// in `assemblies`; and sizes read from the file are checked before they are
// added or multiplied, so a damaged file is refused instead of overflowing.
// Version 1 was Duck Mold's Phase 0.

/// A model read from a Duck Mold `.glb`, in Duck Forge's directions:
/// x is right, y is up, z is forward, 1 unit is about a metre. The model
/// faces forward (+z) and stands on y = 0.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbModel {
    /// The model's name.
    pub name: String,
    /// The corner points. Corners in the same place are shared, so a box
    /// has 8.
    pub vertices: Vec<[f32; 3]>,
    /// The triangles.
    pub triangles: Vec<GlbTriangle>,
    /// The model's palette, by name.
    pub palette: Vec<GlbColor>,
    /// The model's parts and which triangles each owns, in order. Each
    /// part has a shape of its own; together they own every triangle,
    /// once each.
    pub parts: Vec<GlbPart>,
    /// The parts made of other parts (a robot's arm made of an upper arm,
    /// an elbow and a hand, say), each with the range of all its parts'
    /// triangles, outer ones before the parts inside them. A part inside
    /// one is named after it: `left_arm/hand` is in `left_arm`. Empty for
    /// a model with none, and for files from before version 2.
    pub assemblies: Vec<GlbPart>,
}

/// One triangle: three places in `vertices`, clockwise seen from the
/// front, and its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlbTriangle {
    /// The three corners.
    pub corners: [usize; 3],
    /// Red, green and blue, 0 to 255: one of the palette's colours, or
    /// the grey `180, 180, 180` of a face nobody coloured.
    pub color: [u8; 3],
}

/// One palette colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlbColor {
    /// Its name in the recipe.
    pub name: String,
    /// Red, green and blue, 0 to 255.
    pub rgb: [u8; 3],
}

/// One part of the model, or one assembly, and its triangles: those
/// from `first_triangle` on, `triangles` of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlbPart {
    /// Its name; a part inside another is written `left_arm/hand`.
    pub name: String,
    /// The first of its triangles, as a place in `triangles`.
    pub first_triangle: usize,
    /// How many triangles it has.
    pub triangles: usize,
}

/// Reads the bytes of a `.glb` file written by Duck Mold.
pub fn read_duck_mold_glb(bytes: &[u8]) -> Result<GlbModel, String> {
    // ---- The file: a 12-byte header, then blocks. Each block is its
    // length, a 4-letter kind, and its bytes. Duck Mold writes two: the
    // description as JSON, then the numbers.
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err("not a .glb file: it doesn't start with \"glTF\"".into());
    }
    if u32_at(bytes, 4)? != 2 {
        return Err("not glTF version 2".into());
    }
    if u32_at(bytes, 8)? as usize != bytes.len() {
        return Err("the file is cut short or has extra bytes".into());
    }
    let json_length = u32_at(bytes, 12)? as usize;
    if &bytes[16..20] != b"JSON" {
        return Err("the first block isn't JSON".into());
    }
    let json_end = 20 + json_length;
    let json = bytes
        .get(20..json_end)
        .ok_or("the JSON block is cut short")?;
    let bin_length = u32_at(bytes, json_end)? as usize;
    if bytes.get(json_end + 4..json_end + 8) != Some(b"BIN\0") {
        return Err("the second block isn't the numbers".into());
    }
    let bin = bytes
        .get(json_end + 8..json_end + 8 + bin_length)
        .ok_or("the block of numbers is cut short")?;
    let gltf: serde_json::Value = serde_json::from_slice(json)
        .map_err(|error| format!("the JSON block is broken: {error}"))?;

    // ---- The description. Duck Mold writes one mesh with one list of
    // triangles.
    let extras = &gltf["extras"]["duck_mold"];
    if extras["version"].as_i64() != Some(0) {
        return Err("this .glb wasn't written by Duck Mold (no extras.duck_mold version 0)".into());
    }
    let primitive = &gltf["meshes"][0]["primitives"][0];
    if primitive["mode"].as_u64().unwrap_or(4) != 4 {
        return Err("the mesh isn't a list of triangles".into());
    }
    // The numbers 5126 and 5125 are glTF's names for "32-bit decimal"
    // and "32-bit whole number".
    let positions = numbers(
        &gltf,
        bin,
        &primitive["attributes"]["POSITION"],
        "VEC3",
        5126,
    )?;
    let colors = numbers(
        &gltf,
        bin,
        &primitive["attributes"]["COLOR_0"],
        "VEC3",
        5126,
    )?;
    let indices = numbers(&gltf, bin, &primitive["indices"], "SCALAR", 5125)?;
    let corner_count = positions.len() / 12;
    if colors.len() != positions.len() || indices.len() % 12 != 0 {
        return Err("the lists of positions, colours and triangles don't match".into());
    }

    // ---- Points. glTF's "right" is -x and Duck Forge's is +x, so x is
    // negated. Corners in the same place become one vertex.
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut vertex_of_corner: Vec<usize> = Vec::with_capacity(corner_count);
    let mut seen: HashMap<[u32; 3], usize> = HashMap::new();
    for corner in 0..corner_count {
        let point = [
            // `+ 0.0` turns -0 into 0, so that the two count as one place.
            -f32_at(positions, corner * 3) + 0.0,
            f32_at(positions, corner * 3 + 1) + 0.0,
            f32_at(positions, corner * 3 + 2) + 0.0,
        ];
        let vertex = *seen.entry(point.map(f32::to_bits)).or_insert_with(|| {
            vertices.push(point);
            vertices.len() - 1
        });
        vertex_of_corner.push(vertex);
    }

    // ---- Triangles. Negating x turns every triangle inside out, so the
    // second and third corners swap: that gives Duck Forge's order,
    // clockwise seen from the front. All three corners of a triangle
    // carry the same colour; it is read from the first.
    let mut triangles = Vec::with_capacity(indices.len() / 12);
    for triangle in 0..indices.len() / 12 {
        let corner =
            |which: usize| u32_at(indices, (triangle * 3 + which) * 4).map(|at| at as usize);
        let (a, b, c) = (corner(0)?, corner(1)?, corner(2)?);
        if a >= corner_count || b >= corner_count || c >= corner_count {
            return Err(format!(
                "triangle {triangle} names a corner that isn't there"
            ));
        }
        triangles.push(GlbTriangle {
            corners: [
                vertex_of_corner[a],
                vertex_of_corner[c],
                vertex_of_corner[b],
            ],
            color: [
                srgb(f32_at(colors, a * 3)),
                srgb(f32_at(colors, a * 3 + 1)),
                srgb(f32_at(colors, a * 3 + 2)),
            ],
        });
    }

    // ---- What Duck Mold adds: the name, the palette, the parts.
    let mut palette = Vec::new();
    for entry in extras["palette"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = entry["name"]
            .as_str()
            .ok_or("a palette colour has no name")?;
        let hex = entry["hex"]
            .as_str()
            .ok_or("a palette colour has no hex code")?;
        let channel = |at: usize| {
            hex.get(at..at + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| format!("the palette colour \"{name}\" isn't written #RRGGBB"))
        };
        palette.push(GlbColor {
            name: name.to_owned(),
            rgb: [channel(1)?, channel(3)?, channel(5)?],
        });
    }
    let parts = part_list(&extras["parts"], "part", triangles.len())?;
    let assemblies = part_list(&extras["assemblies"], "assembly", triangles.len())?;

    Ok(GlbModel {
        name: extras["name"].as_str().unwrap_or("model").to_owned(),
        vertices,
        triangles,
        palette,
        parts,
        assemblies,
    })
}

/// A list of parts (or of assemblies) with their triangles, each checked
/// against the `count` of triangles there are. A missing list is empty.
fn part_list(list: &serde_json::Value, what: &str, count: usize) -> Result<Vec<GlbPart>, String> {
    let a = if what.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    };
    let mut parts = Vec::new();
    for entry in list.as_array().map(Vec::as_slice).unwrap_or_default() {
        let whole = |field: &str| {
            entry[field]
                .as_u64()
                .and_then(|number| usize::try_from(number).ok())
                .ok_or_else(|| format!("{a} {what} has no {}", field.replace('_', " ")))
        };
        let part = GlbPart {
            name: entry["name"]
                .as_str()
                .ok_or_else(|| format!("{a} {what} has no name"))?
                .to_owned(),
            first_triangle: whole("first_triangle")?,
            triangles: whole("triangles")?,
        };
        // Checked, so a damaged file can't overflow the sum.
        if part
            .first_triangle
            .checked_add(part.triangles)
            .is_none_or(|end| end > count)
        {
            return Err(format!(
                "the {what} \"{}\" names triangles that aren't there",
                part.name
            ));
        }
        parts.push(part);
    }
    Ok(parts)
}

/// The bytes of one list of numbers, found through the description: an
/// "accessor" says how many there are and of what kind, and points at a
/// "buffer view", which says where in the block of numbers they start.
fn numbers<'a>(
    gltf: &serde_json::Value,
    bin: &'a [u8],
    accessor: &serde_json::Value,
    kind: &str,
    component: u64,
) -> Result<&'a [u8], String> {
    let accessor =
        &gltf["accessors"][accessor.as_u64().ok_or("a list of numbers is missing")? as usize];
    if accessor["type"].as_str() != Some(kind)
        || accessor["componentType"].as_u64() != Some(component)
    {
        return Err("a list of numbers isn't the kind Duck Mold writes".into());
    }
    let per: usize = if kind == "VEC3" { 12 } else { 4 };
    let count = accessor["count"]
        .as_u64()
        .ok_or("a list of numbers has no count")?;
    let view = &gltf["bufferViews"][accessor["bufferView"]
        .as_u64()
        .ok_or("a list of numbers has no place")? as usize];
    // Checked sums: a damaged file can claim more numbers than any file
    // could hold, and that must be a refusal, not a crash.
    let span = usize::try_from(count)
        .ok()
        .and_then(|count| count.checked_mul(per))
        .zip(
            view["byteOffset"]
                .as_u64()
                .unwrap_or(0)
                .checked_add(accessor["byteOffset"].as_u64().unwrap_or(0))
                .and_then(|start| usize::try_from(start).ok()),
        )
        .and_then(|(length, start)| Some(start..start.checked_add(length)?));
    span.and_then(|span| bin.get(span))
        .ok_or_else(|| "a list of numbers runs past the end of the file".to_owned())
}

/// A 32-bit whole number, stored smallest byte first.
fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    let four = bytes.get(at..at + 4).ok_or("the file is cut short")?;
    Ok(u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
}

/// The 32-bit decimal number number `at` in a list.
fn f32_at(list: &[u8], at: usize) -> f32 {
    let four = &list[at * 4..at * 4 + 4];
    f32::from_le_bytes([four[0], four[1], four[2], four[3]])
}

/// glTF stores colours as linear light, from 0 to 1. This turns one back
/// into the usual 0 to 255 of `Color::rgb` (the sRGB curve). It gives
/// back exactly the number the palette had.
fn srgb(linear: f32) -> u8 {
    let curved = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (curved * 255.0).round().clamp(0.0, 255.0) as u8
}

// ==== end of Duck Mold's .glb reader, version 2 =============================

// ---- Duck Forge's side: files, meshes and parts ------------------------------

impl GlbModel {
    /// Loads a `.glb` file made by Duck Mold (`duck-mold export`).
    ///
    /// A missing file, or one that isn't a Duck Mold model, is a
    /// [`LoadError`]: one sentence saying what's wrong and what to do.
    pub fn load(path: impl AsRef<Path>) -> Result<GlbModel, LoadError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "model", &error))?;
        read_duck_mold_glb(&bytes).map_err(|reason| {
            LoadError::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{} isn't a model Duck Forge can load ({reason}); only .glb files made by \
                     Duck Mold can be loaded: make one with duck-mold export recipe.toml",
                    path.display()
                ),
            )
        })
    }

    /// The model as a [`Mesh`], ready to draw: its points, and its
    /// triangles with their colours, in the file's order (so a part's
    /// [`range`](GlbPart::range) picks out its triangles in the mesh too).
    pub fn to_mesh(&self) -> Mesh {
        let mut mesh = Mesh::new();
        for &[x, y, z] in &self.vertices {
            mesh.add_vertex(vec3(x, y, z));
        }
        for triangle in &self.triangles {
            let [a, b, c] = triangle.corners;
            let [red, green, blue] = triangle.color;
            mesh.add_triangle(a, b, c, Color::rgb(red, green, blue));
        }
        mesh
    }

    /// The part with this name (`"head"`, or `"left_arm/hand"` for a part
    /// inside another), or the assembly (`"left_arm"`: the whole arm), if
    /// the model has one.
    pub fn part(&self, name: &str) -> Option<&GlbPart> {
        self.parts
            .iter()
            .chain(&self.assemblies)
            .find(|part| part.name == name)
    }
}

impl GlbPart {
    /// Which of the model's triangles are this part's (or, for an
    /// assembly, belong to the parts inside it): use it on
    /// `mesh.triangles[part.range()]` to recolour the part.
    pub fn range(&self) -> Range<usize> {
        self.first_triangle..self.first_triangle + self.triangles
    }
}

impl Mesh {
    /// Loads a model made with Duck Mold (a `.glb` file from `duck-mold
    /// export`) as a mesh, with its colours. It faces +z and stands on
    /// y = 0 (see [the module's introduction](crate::glb)). Load it once,
    /// in your game's `new`, and draw it every frame.
    ///
    /// For its palette and parts, use [`GlbModel::load`] instead.
    pub fn load_glb(path: impl AsRef<Path>) -> Result<Mesh, LoadError> {
        Ok(GlbModel::load(path)?.to_mesh())
    }
}
