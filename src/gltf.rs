//! Any glTF model (`.glb`): its meshes with their textures, its parts as a
//! tree of nodes, and its node animations, so a model can be textured and
//! can move.
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # use duckforge::gltf::GltfModel;
//! # fn main() -> Result<(), duckforge::LoadError> {
//! # let mut canvas = Canvas::new(64, 64);
//! # let camera = Camera3D::new(vec3(0.0, 1.0, -4.0));
//! let windmill = GltfModel::load("assets/windmill.glb")?; // once, in new()
//! let seconds = 1.5; // in draw(): how far into the animation
//! windmill.draw_animated(&mut canvas, &Transform::default(), &camera, "spin", seconds);
//! # Ok(())
//! # }
//! ```
//!
//! **What's read.** Triangles (`POSITION`, `TEXCOORD_0`, `COLOR_0`), each
//! primitive's material colour (`baseColorFactor`) and texture
//! (`baseColorTexture`, a PNG inside the file, beside it, or in a
//! `data:` address), the nodes' tree with their places, turns and sizes,
//! and animations of those (`translation`, `rotation`, `scale`; `LINEAR`,
//! `STEP`, and `CUBICSPLINE` read as a straight line between its values).
//! An animation loops: at `seconds` past its end it starts again.
//!
//! **What isn't** (each one said in [`GltfModel::warnings`], and the rest of
//! the model still loads): JPEG textures (the model is drawn in its
//! colours), skinned characters (bones bending one mesh: the mesh is
//! drawn in its resting shape), morph targets, cameras and lights.
//! Animate a character by its parts, each part a node, as Duck Mold makes
//! them.
//!
//! **Directions.** glTF's "right" is -x where Duck Forge's is +x, so x is
//! negated (and each triangle's corners turned round so they stay
//! clockwise from the front), as for Duck Mold's models: a model faces
//! +z, y is up, 1 unit is a metre.
//!
//! **One texture per mesh.** A Duck Forge mesh has one texture, so a model
//! is drawn as one mesh per glTF primitive (each with its own material).

use std::path::Path;

use serde_json::Value;

use crate::canvas::Canvas;
use crate::color::Color;
use crate::error::LoadError;
use crate::math::{Mat4, Vec2, Vec3, vec2, vec3};
use crate::render3d::{Camera3D, Mesh, Transform};
use crate::sprite::Sprite;

/// A glTF model: meshes on a tree of nodes, and animations that move the
/// nodes. Load it with [`GltfModel::load`]; draw it with
/// [`draw`](GltfModel::draw) or [`draw_animated`](GltfModel::draw_animated).
#[derive(Debug, Clone)]
pub struct GltfModel {
    /// The file's name, without `.glb`.
    pub name: String,
    nodes: Vec<Node>,
    roots: Vec<usize>,
    parts: Vec<Part>,
    animations: Vec<GltfAnimation>,
    /// What the file has that Duck Forge doesn't read (a JPEG texture, a
    /// skin), one sentence each. Empty when everything was read.
    pub warnings: Vec<String>,
}

/// A node: a place in the model's tree, which can hold a mesh and children.
#[derive(Debug, Clone)]
struct Node {
    name: String,
    children: Vec<usize>,
    rest: Place,
}

/// Where a node is relative to its parent.
#[derive(Debug, Clone, Copy)]
enum Place {
    Parts {
        translation: Vec3,
        /// A quaternion: x, y, z, w.
        rotation: [f32; 4],
        scale: Vec3,
    },
    Matrix(Mat4),
}

/// One mesh of the model, held by a node.
#[derive(Debug, Clone)]
struct Part {
    node: usize,
    mesh: Mesh,
}

/// One of a model's animations: its name and how long it lasts.
#[derive(Debug, Clone)]
pub struct GltfAnimation {
    /// Its name in the file (`"animation 2"` if it has none).
    pub name: String,
    /// How long it lasts, in seconds (its last key).
    pub seconds: f32,
    channels: Vec<Channel>,
}

/// One animated value of one node.
#[derive(Debug, Clone)]
struct Channel {
    node: usize,
    what: Animated,
    times: Vec<f32>,
    /// One value per time (3 numbers, or 4 for a rotation).
    values: Vec<[f32; 4]>,
    step: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Animated {
    Translation,
    Rotation,
    Scale,
}

impl GltfModel {
    /// Loads a `.glb` file (glTF in one file). A missing file, or one that
    /// isn't a glTF model, is a [`LoadError`] saying why; parts Duck Forge
    /// can't read are left out and listed in [`warnings`](GltfModel::warnings).
    pub fn load(path: impl AsRef<Path>) -> Result<GltfModel, LoadError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .map_err(|error| crate::sprite::missing_file_error(path, "model", &error))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        GltfModel::from_glb(&name, &bytes, path.parent()).map_err(|reason| {
            LoadError::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "{} isn't a model Duck Forge can load ({reason}); it reads .glb files \
                     (glTF 2.0 in one file): export as .glb from Blender or another 3D program",
                    path.display()
                ),
            )
        })
    }

    /// A model from a `.glb` file's bytes, called `name`. Textures in
    /// separate files are looked for in `folder`.
    pub fn from_glb(name: &str, bytes: &[u8], folder: Option<&Path>) -> Result<GltfModel, String> {
        let (gltf, bin) = split_glb(bytes)?;
        let mut warnings = Vec::new();
        let images = read_images(&gltf, bin, folder, &mut warnings)?;

        // The nodes and their tree.
        let node_list = list(&gltf["nodes"]);
        let mut nodes = Vec::with_capacity(node_list.len());
        let mut has_parent = vec![false; node_list.len()];
        let mut parts = Vec::new();
        for (i, node) in node_list.iter().enumerate() {
            let children: Vec<usize> = list(&node["children"])
                .iter()
                .filter_map(Value::as_u64)
                .map(|c| c as usize)
                .collect();
            for &c in &children {
                if c >= node_list.len() {
                    return Err(format!("node {i} has a child that isn't there"));
                }
                has_parent[c] = true;
            }
            if node.get("skin").is_some() {
                warnings.push(format!(
                    "node {i} is a skinned mesh (bones): it's drawn in its resting shape"
                ));
            }
            if let Some(mesh) = node["mesh"].as_u64() {
                for mesh in read_mesh(&gltf, bin, mesh as usize, &images, &mut warnings)? {
                    parts.push(Part { node: i, mesh });
                }
            }
            nodes.push(Node {
                name: node["name"].as_str().unwrap_or("").to_string(),
                children,
                rest: read_place(node)?,
            });
        }
        let scene = gltf["scene"].as_u64().unwrap_or(0) as usize;
        let mut roots: Vec<usize> = list(&gltf["scenes"][scene]["nodes"])
            .iter()
            .filter_map(Value::as_u64)
            .map(|n| n as usize)
            .filter(|&n| n < nodes.len())
            .collect();
        if roots.is_empty() {
            roots = (0..nodes.len()).filter(|&n| !has_parent[n]).collect();
        }
        // A loop in the tree would never end.
        let mut seen = vec![false; nodes.len()];
        let mut stack = roots.clone();
        while let Some(n) = stack.pop() {
            if std::mem::replace(&mut seen[n], true) {
                return Err(String::from("its nodes' tree goes round in a loop"));
            }
            stack.extend(&nodes[n].children);
        }

        let mut animations = Vec::new();
        for (a, animation) in list(&gltf["animations"]).iter().enumerate() {
            animations.push(read_animation(
                &gltf,
                bin,
                animation,
                a,
                nodes.len(),
                &mut warnings,
            )?);
        }
        if !list(&gltf["cameras"]).is_empty() {
            warnings.push(String::from("its cameras are left out"));
        }
        if parts.is_empty() {
            return Err(String::from("it has no triangles to draw"));
        }
        Ok(GltfModel {
            name: name.to_string(),
            nodes,
            roots,
            parts,
            animations,
            warnings,
        })
    }

    /// The model's animations.
    pub fn animations(&self) -> &[GltfAnimation] {
        &self.animations
    }

    /// The names of the model's nodes (its parts), in the file's order.
    pub fn node_names(&self) -> Vec<&str> {
        self.nodes.iter().map(|n| n.name.as_str()).collect()
    }

    /// The model's meshes in its resting pose, each placed where its node
    /// puts it: ready to draw with `draw_mesh`, or to merge.
    pub fn meshes(&self) -> Vec<Mesh> {
        self.posed(None, 0.0)
    }

    /// The model's meshes posed `seconds` into `animation` (by name; it
    /// loops). An animation the model hasn't is the resting pose.
    pub fn meshes_at(&self, animation: &str, seconds: f32) -> Vec<Mesh> {
        self.posed(self.find(animation), seconds)
    }

    /// Where the node called `name` is (its middle, the point it turns
    /// round), `seconds` into `animation` (`None`: resting), in the
    /// model's own space: `place.apply(point)` puts it in the world. To
    /// fix something to a moving part.
    pub fn node_position(&self, name: &str, animation: Option<&str>, seconds: f32) -> Option<Vec3> {
        let node = self.nodes.iter().position(|n| n.name == name)?;
        let worlds = self.world_matrices(animation.and_then(|a| self.find(a)), seconds);
        Some(worlds[node].transform_point(Vec3::ZERO))
    }

    /// The model as one mesh in its resting pose, if all its textured parts
    /// share one picture (a mesh has one texture). Otherwise `None`: draw
    /// it with [`draw`](GltfModel::draw).
    pub fn to_mesh(&self) -> Option<Mesh> {
        let mut whole = Mesh::new();
        for mesh in self.meshes() {
            if let (Some(a), Some(b)) = (&whole.texture, &mesh.texture) {
                if !a.same_pixels(b) {
                    return None;
                }
            }
            whole.merge(&mesh, &Transform::default());
        }
        Some(whole)
    }

    /// Draws the model in its resting pose, placed by `transform`.
    pub fn draw(&self, canvas: &mut Canvas, transform: &Transform, camera: &Camera3D) {
        for mesh in self.posed(None, 0.0) {
            canvas.draw_mesh(&mesh, transform, camera);
        }
    }

    /// Draws the model `seconds` into its animation called `animation`
    /// (looping), placed by `transform`.
    pub fn draw_animated(
        &self,
        canvas: &mut Canvas,
        transform: &Transform,
        camera: &Camera3D,
        animation: &str,
        seconds: f32,
    ) {
        for mesh in self.meshes_at(animation, seconds) {
            canvas.draw_mesh(&mesh, transform, camera);
        }
    }

    fn find(&self, animation: &str) -> Option<usize> {
        self.animations.iter().position(|a| a.name == animation)
    }

    /// Every part's mesh, moved by its node at that moment.
    fn posed(&self, animation: Option<usize>, seconds: f32) -> Vec<Mesh> {
        let worlds = self.world_matrices(animation, seconds);
        self.parts
            .iter()
            .map(|part| {
                let m = worlds[part.node];
                Mesh {
                    vertices: part
                        .mesh
                        .vertices
                        .iter()
                        .map(|&v| m.transform_point(v))
                        .collect(),
                    ..part.mesh.clone()
                }
            })
            .collect()
    }

    /// Each node's place in the model, `seconds` into `animation`.
    fn world_matrices(&self, animation: Option<usize>, seconds: f32) -> Vec<Mat4> {
        let mut places: Vec<Place> = self.nodes.iter().map(|n| n.rest).collect();
        if let Some(animation) = animation.and_then(|a| self.animations.get(a)) {
            let t = if animation.seconds > 0.0 && seconds.is_finite() {
                seconds.rem_euclid(animation.seconds)
            } else {
                0.0
            };
            for channel in &animation.channels {
                let value = channel.at(t);
                if let Place::Parts {
                    translation,
                    rotation,
                    scale,
                } = &mut places[channel.node]
                {
                    match channel.what {
                        Animated::Translation => *translation = vec3(value[0], value[1], value[2]),
                        Animated::Rotation => *rotation = value,
                        Animated::Scale => *scale = vec3(value[0], value[1], value[2]),
                    }
                }
            }
        }
        let mut worlds = vec![Mat4::IDENTITY; self.nodes.len()];
        let mut stack: Vec<(usize, Mat4)> =
            self.roots.iter().map(|&r| (r, Mat4::IDENTITY)).collect();
        while let Some((n, parent)) = stack.pop() {
            let world = parent * places[n].matrix();
            worlds[n] = world;
            for &child in &self.nodes[n].children {
                stack.push((child, world));
            }
        }
        worlds
    }
}

impl Place {
    fn matrix(&self) -> Mat4 {
        match *self {
            Place::Matrix(m) => m,
            Place::Parts {
                translation,
                rotation,
                scale,
            } => Mat4::translation(translation) * rotation_matrix(rotation) * Mat4::scaling(scale),
        }
    }
}

/// A rotation matrix from a quaternion (x, y, z, w).
fn rotation_matrix(q: [f32; 4]) -> Mat4 {
    let length = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if !(length > 0.0 && length.is_finite()) {
        return Mat4::IDENTITY;
    }
    let [x, y, z, w] = q.map(|v| v / length);
    Mat4 {
        columns: [
            [
                1.0 - 2.0 * (y * y + z * z),
                2.0 * (x * y + z * w),
                2.0 * (x * z - y * w),
                0.0,
            ],
            [
                2.0 * (x * y - z * w),
                1.0 - 2.0 * (x * x + z * z),
                2.0 * (y * z + x * w),
                0.0,
            ],
            [
                2.0 * (x * z + y * w),
                2.0 * (y * z - x * w),
                1.0 - 2.0 * (x * x + y * y),
                0.0,
            ],
            [0.0, 0.0, 0.0, 1.0],
        ],
    }
}

impl Channel {
    /// The value at `t` seconds: between the keys either side of it.
    fn at(&self, t: f32) -> [f32; 4] {
        let keys = &self.times;
        if keys.is_empty() {
            return [0.0, 0.0, 0.0, 1.0];
        }
        let after = keys.partition_point(|&k| k <= t);
        if after == 0 {
            return self.values[0];
        }
        if after >= keys.len() {
            return self.values[keys.len() - 1];
        }
        let (a, b) = (after - 1, after);
        if self.step {
            return self.values[a];
        }
        let span = keys[b] - keys[a];
        let f = if span > 0.0 {
            (t - keys[a]) / span
        } else {
            0.0
        };
        let (va, vb) = (self.values[a], self.values[b]);
        if self.what == Animated::Rotation {
            slerp(va, vb, f)
        } else {
            [0, 1, 2, 3].map(|i| va[i] + (vb[i] - va[i]) * f)
        }
    }
}

/// The turn a share `f` of the way from `a` to `b`, the short way round.
fn slerp(a: [f32; 4], mut b: [f32; 4], f: f32) -> [f32; 4] {
    let mut dot: f32 = (0..4).map(|i| a[i] * b[i]).sum();
    if dot < 0.0 {
        b = b.map(|v| -v);
        dot = -dot;
    }
    if dot > 0.9995 {
        return [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * f);
    }
    let angle = dot.clamp(-1.0, 1.0).acos();
    let sin = angle.sin();
    let (ka, kb) = (((1.0 - f) * angle).sin() / sin, (f * angle).sin() / sin);
    [0, 1, 2, 3].map(|i| a[i] * ka + b[i] * kb)
}

/// A JSON list, or an empty one.
fn list(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

/// A `.glb` file's description (JSON) and its block of numbers.
fn split_glb(bytes: &[u8]) -> Result<(Value, &[u8]), String> {
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
            .ok_or_else(|| String::from("the file is cut short"))
    };
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err(String::from(
            "it isn't a .glb file: it doesn't start with \"glTF\" (a .gltf text file with \
             separate .bin files needs exporting as one .glb)",
        ));
    }
    if u32_at(4)? != 2 {
        return Err(String::from("it isn't glTF version 2"));
    }
    let json_length = u32_at(12)?;
    if bytes.get(16..20) != Some(b"JSON") {
        return Err(String::from("its first block isn't JSON"));
    }
    let json_end = 20usize
        .checked_add(json_length)
        .ok_or("the file is cut short")?;
    let json = bytes
        .get(20..json_end)
        .ok_or("the JSON block is cut short")?;
    let gltf: Value =
        serde_json::from_slice(json).map_err(|e| format!("its JSON block is broken: {e}"))?;
    let bin = match u32_at(json_end) {
        Ok(length) if bytes.get(json_end + 4..json_end + 8) == Some(b"BIN\0") => bytes
            .get(json_end + 8..(json_end + 8).saturating_add(length))
            .ok_or("the block of numbers is cut short")?,
        _ => &[],
    };
    Ok((gltf, bin))
}

/// The numbers of accessor `index`, as decimals, and how many make one
/// item (3 for a VEC3).
fn floats(gltf: &Value, bin: &[u8], index: usize) -> Result<(Vec<f32>, usize), String> {
    let accessor = &gltf["accessors"][index];
    if accessor.is_null() {
        return Err(format!("accessor {index} isn't there"));
    }
    if accessor.get("sparse").is_some() {
        return Err(String::from("it uses sparse accessors, which aren't read"));
    }
    let per = match accessor["type"].as_str() {
        Some("SCALAR") => 1,
        Some("VEC2") => 2,
        Some("VEC3") => 3,
        Some("VEC4") => 4,
        Some("MAT4") => 16,
        other => {
            return Err(format!(
                "accessor {index} has a type it can't read: {other:?}"
            ));
        }
    };
    let component = accessor["componentType"].as_u64().unwrap_or(0);
    let size = match component {
        5120 | 5121 => 1,
        5122 | 5123 => 2,
        5125 | 5126 => 4,
        _ => return Err(format!("accessor {index} has an unknown kind of number")),
    };
    let normalized = accessor["normalized"].as_bool().unwrap_or(false);
    let count = accessor["count"]
        .as_u64()
        .ok_or("an accessor has no count")? as usize;
    let Some(view_index) = accessor["bufferView"].as_u64() else {
        // All zeros, as the spec says (but not more than a file could hold).
        if count > bin.len().max(1 << 20) {
            return Err(String::from(
                "an accessor claims more numbers than the file holds",
            ));
        }
        return Ok((vec![0.0; count * per], per));
    };
    let view = &gltf["bufferViews"][view_index as usize];
    if view["buffer"].as_u64().unwrap_or(0) != 0 || bin.is_empty() {
        return Err(String::from(
            "its numbers are in a separate file; export it as one .glb",
        ));
    }
    let start = (view["byteOffset"].as_u64().unwrap_or(0) as usize)
        .saturating_add(accessor["byteOffset"].as_u64().unwrap_or(0) as usize);
    let stride = match view["byteStride"].as_u64() {
        Some(s) if s > 0 => s as usize,
        _ => per * size,
    };
    // Checked before anything is made: a damaged file can claim more
    // numbers than any file could hold, and that must be a refusal, not a
    // crash.
    let fits = count == 0
        || (count - 1)
            .checked_mul(stride)
            .and_then(|last| last.checked_add(start)?.checked_add(per * size))
            .is_some_and(|end| end <= bin.len());
    if !fits {
        return Err(String::from(
            "a list of numbers runs past the end of the file",
        ));
    }
    let mut out = Vec::with_capacity(count * per);
    for item in 0..count {
        for c in 0..per {
            let at = start + item * stride + c * size;
            let b = bin
                .get(at..at + size)
                .ok_or("a list of numbers runs past the end of the file")?;
            let v = match component {
                5126 => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                5123 => {
                    let v = u16::from_le_bytes([b[0], b[1]]) as f32;
                    if normalized { v / 65535.0 } else { v }
                }
                5122 => {
                    let v = i16::from_le_bytes([b[0], b[1]]) as f32;
                    if normalized {
                        (v / 32767.0).max(-1.0)
                    } else {
                        v
                    }
                }
                5121 => {
                    let v = b[0] as f32;
                    if normalized { v / 255.0 } else { v }
                }
                _ => {
                    let v = b[0] as i8 as f32;
                    if normalized { (v / 127.0).max(-1.0) } else { v }
                }
            };
            out.push(v);
        }
    }
    Ok((out, per))
}

/// Whole numbers (triangle corners) from accessor `index`.
fn whole_numbers(gltf: &Value, bin: &[u8], index: usize) -> Result<Vec<usize>, String> {
    let accessor = &gltf["accessors"][index];
    let component = accessor["componentType"].as_u64().unwrap_or(0);
    let count = accessor["count"]
        .as_u64()
        .ok_or("an accessor has no count")? as usize;
    let size = match component {
        5121 => 1,
        5123 => 2,
        5125 => 4,
        _ => return Err(String::from("its triangle corners aren't whole numbers")),
    };
    let view = &gltf["bufferViews"][accessor["bufferView"]
        .as_u64()
        .ok_or("corners have no place")? as usize];
    let start = (view["byteOffset"].as_u64().unwrap_or(0) as usize)
        .saturating_add(accessor["byteOffset"].as_u64().unwrap_or(0) as usize);
    let fits = count
        .checked_mul(size)
        .and_then(|length| length.checked_add(start))
        .is_some_and(|end| end <= bin.len());
    if !fits {
        return Err(String::from("the corners run past the end of the file"));
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let at = start + i * size;
        let b = bin
            .get(at..at + size)
            .ok_or("the corners run past the end of the file")?;
        out.push(match size {
            1 => b[0] as usize,
            2 => u16::from_le_bytes([b[0], b[1]]) as usize,
            _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize,
        });
    }
    Ok(out)
}

/// Every image in the file, decoded (`None` for one that couldn't be).
fn read_images(
    gltf: &Value,
    bin: &[u8],
    folder: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Result<Vec<Option<Sprite>>, String> {
    let mut images = Vec::new();
    for (i, image) in list(&gltf["images"]).iter().enumerate() {
        let bytes: Option<Vec<u8>> = if let Some(view) = image["bufferView"].as_u64() {
            let view = &gltf["bufferViews"][view as usize];
            let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
            let length = view["byteLength"].as_u64().unwrap_or(0) as usize;
            Some(
                bin.get(start..start.saturating_add(length))
                    .ok_or("an image runs past the end of the file")?
                    .to_vec(),
            )
        } else if let Some(uri) = image["uri"].as_str() {
            if let Some(data) = uri.strip_prefix("data:") {
                data.split_once(";base64,").and_then(|(_, b64)| base64(b64))
            } else {
                folder.and_then(|f| std::fs::read(f.join(uri)).ok())
            }
        } else {
            None
        };
        let decoded = bytes.and_then(|bytes| {
            if bytes.starts_with(&[0xFF, 0xD8]) {
                warnings.push(format!(
                    "image {i} is a JPEG, which isn't read (save the model's textures as PNG): \
                     its parts are drawn in their colours"
                ));
                return None;
            }
            match Sprite::from_png_bytes(&bytes) {
                Ok(sprite) => Some(sprite),
                Err(e) => {
                    warnings.push(format!("image {i} couldn't be read as a PNG ({e})"));
                    None
                }
            }
        });
        if decoded.is_none()
            && !warnings
                .iter()
                .any(|w| w.starts_with(&format!("image {i} ")))
        {
            warnings.push(format!("image {i} wasn't found"));
        }
        images.push(decoded);
    }
    Ok(images)
}

/// Decodes base64 (for textures written into the file as text).
fn base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    };
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut bits, mut count) = (0u32, 0);
    for c in text
        .bytes()
        .filter(|&c| c != b'=' && !c.is_ascii_whitespace())
    {
        bits = (bits << 6) | value(c)? as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// A node's place: its translation, rotation and scale (or matrix), turned
/// into Duck Forge's directions (x negated).
fn read_place(node: &Value) -> Result<Place, String> {
    let numbers = |key: &str| -> Option<Vec<f32>> {
        node[key]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect())
    };
    if let Some(m) = numbers("matrix") {
        if m.len() != 16 {
            return Err(String::from("a node's matrix isn't 16 numbers"));
        }
        let mut columns = [[0.0f32; 4]; 4];
        for (c, column) in columns.iter_mut().enumerate() {
            column.copy_from_slice(&m[c * 4..c * 4 + 4]);
        }
        // Negating x on both sides: S * M * S, with S = (-1, 1, 1).
        for (c, column) in columns.iter_mut().enumerate() {
            for (r, value) in column.iter_mut().enumerate() {
                if (r == 0) != (c == 0) {
                    *value = -*value;
                }
            }
        }
        return Ok(Place::Matrix(Mat4 { columns }));
    }
    let three = |key: &str, default: f32| match numbers(key).as_deref() {
        Some([x, y, z]) => vec3(*x, *y, *z),
        _ => vec3(default, default, default),
    };
    let t = three("translation", 0.0);
    let rotation = match numbers("rotation").as_deref() {
        Some([x, y, z, w]) => to_forge_rotation([*x, *y, *z, *w]),
        _ => [0.0, 0.0, 0.0, 1.0],
    };
    Ok(Place::Parts {
        translation: vec3(-t.x, t.y, t.z),
        rotation,
        scale: three("scale", 1.0),
    })
}

/// A glTF rotation in Duck Forge's directions: mirrored in x, a turn about
/// x stays, turns about y and z go the other way.
fn to_forge_rotation([x, y, z, w]: [f32; 4]) -> [f32; 4] {
    [x, -y, -z, w]
}

/// One mesh's primitives, each as a Duck Forge mesh.
fn read_mesh(
    gltf: &Value,
    bin: &[u8],
    index: usize,
    images: &[Option<Sprite>],
    warnings: &mut Vec<String>,
) -> Result<Vec<Mesh>, String> {
    let mesh = &gltf["meshes"][index];
    if mesh.is_null() {
        return Err(format!("mesh {index} isn't there"));
    }
    let mut meshes = Vec::new();
    for primitive in list(&mesh["primitives"]) {
        if primitive["mode"].as_u64().unwrap_or(4) != 4 {
            warnings.push(format!(
                "mesh {index} has lines or points, which are left out"
            ));
            continue;
        }
        if primitive.get("targets").is_some() {
            warnings.push(format!("mesh {index} has morph targets, which aren't read"));
        }
        let attributes = &primitive["attributes"];
        let position = attributes["POSITION"]
            .as_u64()
            .ok_or("a mesh has no positions")?;
        let (points, per) = floats(gltf, bin, position as usize)?;
        if per != 3 {
            return Err(String::from("its positions aren't x, y, z"));
        }
        let corner_count = points.len() / 3;
        let uvs = match attributes["TEXCOORD_0"].as_u64() {
            Some(a) => Some(floats(gltf, bin, a as usize)?.0),
            None => None,
        };
        let colors = match attributes["COLOR_0"].as_u64() {
            Some(a) => Some(floats(gltf, bin, a as usize)?),
            None => None,
        };
        let corners: Vec<usize> = match primitive["indices"].as_u64() {
            Some(a) => whole_numbers(gltf, bin, a as usize)?,
            None => (0..corner_count).collect(),
        };
        // The material: a colour, and maybe a texture.
        let material = primitive["material"]
            .as_u64()
            .map(|m| &gltf["materials"][m as usize]);
        let pbr = material.map(|m| &m["pbrMetallicRoughness"]);
        let factor = pbr
            .and_then(|p| p["baseColorFactor"].as_array())
            .map(|f| [0, 1, 2].map(|i| f.get(i).and_then(Value::as_f64).unwrap_or(1.0) as f32))
            .unwrap_or([1.0; 3]);
        let texture = pbr
            .and_then(|p| p["baseColorTexture"]["index"].as_u64())
            .and_then(|t| gltf["textures"][t as usize]["source"].as_u64())
            .and_then(|i| images.get(i as usize).cloned().flatten());

        let mut out = Mesh::new();
        for c in 0..corner_count {
            out.add_vertex(vec3(
                -points[c * 3] + 0.0,
                points[c * 3 + 1],
                points[c * 3 + 2],
            ));
        }
        let uv_of = |c: usize| {
            uvs.as_ref()
                .and_then(|u| Some(vec2(*u.get(c * 2)?, *u.get(c * 2 + 1)?)))
                .unwrap_or(Vec2::ZERO)
        };
        let color_of = |c: usize| {
            let tint = match &colors {
                Some((values, per)) => {
                    [0, 1, 2].map(|i| values.get(c * per + i).copied().unwrap_or(1.0))
                }
                None => [1.0; 3],
            };
            let [r, g, b] = [0, 1, 2].map(|i| to_srgb(factor[i] * tint[i]));
            Color::rgb(r, g, b)
        };
        for triangle in corners.chunks_exact(3) {
            let [a, b, c] = [triangle[0], triangle[1], triangle[2]];
            if a >= corner_count || b >= corner_count || c >= corner_count {
                return Err(format!("mesh {index} names a corner that isn't there"));
            }
            // x was negated, so the corners turn round to stay clockwise.
            if texture.is_some() && uvs.is_some() {
                out.add_triangle_uv(a, c, b, [uv_of(a), uv_of(c), uv_of(b)], color_of(a));
            } else {
                out.add_triangle(a, c, b, color_of(a));
            }
        }
        if let Some(texture) = texture.filter(|_| uvs.is_some()) {
            out = out.with_texture(texture);
        }
        meshes.push(out);
    }
    Ok(meshes)
}

/// Linear light (0 to 1) into the usual 0 to 255 (the sRGB curve).
fn to_srgb(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let curved = if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (curved * 255.0).round().clamp(0.0, 255.0) as u8
}

fn read_animation(
    gltf: &Value,
    bin: &[u8],
    animation: &Value,
    index: usize,
    node_count: usize,
    warnings: &mut Vec<String>,
) -> Result<GltfAnimation, String> {
    let name = animation["name"]
        .as_str()
        .map(String::from)
        .unwrap_or_else(|| format!("animation {index}"));
    let samplers = list(&animation["samplers"]);
    let mut channels = Vec::new();
    let mut seconds = 0.0f32;
    for channel in list(&animation["channels"]) {
        let target = &channel["target"];
        let Some(node) = target["node"].as_u64().map(|n| n as usize) else {
            continue;
        };
        if node >= node_count {
            return Err(format!(
                "the animation \"{name}\" moves a node that isn't there"
            ));
        }
        let what = match target["path"].as_str() {
            Some("translation") => Animated::Translation,
            Some("rotation") => Animated::Rotation,
            Some("scale") => Animated::Scale,
            other => {
                warnings.push(format!(
                    "the animation \"{name}\" changes {}, which isn't read",
                    other.unwrap_or("something unknown")
                ));
                continue;
            }
        };
        let sampler = &samplers
            .get(channel["sampler"].as_u64().unwrap_or(u64::MAX) as usize)
            .ok_or_else(|| format!("the animation \"{name}\" has a channel with no sampler"))?;
        let times = floats(
            gltf,
            bin,
            sampler["input"].as_u64().ok_or("a sampler has no times")? as usize,
        )?
        .0;
        let (raw, per) = floats(
            gltf,
            bin,
            sampler["output"]
                .as_u64()
                .ok_or("a sampler has no values")? as usize,
        )?;
        let interpolation = sampler["interpolation"].as_str().unwrap_or("LINEAR");
        // A cubic spline keeps an in-tangent, the value and an out-tangent
        // for each key: the middle one is the value.
        let (stride, skip) = if interpolation == "CUBICSPLINE" {
            (3, 1)
        } else {
            (1, 0)
        };
        let mut values = Vec::with_capacity(times.len());
        for k in 0..times.len() {
            let at = (k * stride + skip) * per;
            let v = raw
                .get(at..at + per)
                .ok_or("an animation has fewer values than times")?;
            let mut value = [0.0, 0.0, 0.0, 1.0];
            value[..per.min(4)].copy_from_slice(&v[..per.min(4)]);
            values.push(match what {
                Animated::Translation => [-value[0], value[1], value[2], 0.0],
                Animated::Rotation => to_forge_rotation(value),
                Animated::Scale => value,
            });
        }
        if let Some(&last) = times.last() {
            seconds = seconds.max(last);
        }
        channels.push(Channel {
            node,
            what,
            times,
            values,
            step: interpolation == "STEP",
        });
    }
    Ok(GltfAnimation {
        name,
        seconds,
        channels,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A `.glb` file built from a JSON description and a block of numbers.
    pub(crate) fn glb(json: &str, bin: &[u8]) -> Vec<u8> {
        let mut json = json.as_bytes().to_vec();
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bin = bin.to_vec();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    /// A 2 x 2 PNG: red, green / blue, white.
    fn png_bytes() -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, 2, 2);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer
                .write_image_data(&[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255])
                .unwrap();
        }
        out
    }

    /// A textured square (a "body" node) with a red triangle on a child
    /// "flag" node, and an animation "slide": the body moves from x = 0
    /// to x = 2 (glTF's) over a second, and the flag turns a quarter round
    /// y at half a second (a step).
    pub(crate) fn test_model() -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let floats = |bin: &mut Vec<u8>, values: &[f32]| {
            let start = bin.len();
            for v in values {
                bin.extend_from_slice(&v.to_le_bytes());
            }
            (start, bin.len() - start)
        };
        // The square, facing -z (towards a camera looking along +z), seen
        // in glTF's directions (x mirrored).
        let square = floats(
            &mut bin,
            &[
                -0.5, -0.5, 0.0, -0.5, 0.5, 0.0, 0.5, 0.5, 0.0, 0.5, -0.5, 0.0,
            ],
        );
        let uvs = floats(&mut bin, &[1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
        let corners_start = bin.len();
        for i in [0u16, 1, 2, 0, 2, 3] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let corners = (corners_start, 12);
        let triangle = floats(&mut bin, &[0.0, 0.0, 0.0, 0.0, 0.3, 0.0, -0.3, 0.0, 0.0]);
        let times = floats(&mut bin, &[0.0, 1.0]);
        let slide = floats(&mut bin, &[0.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let step_times = floats(&mut bin, &[0.0, 0.5]);
        let half = std::f32::consts::FRAC_1_SQRT_2;
        let turn = floats(&mut bin, &[0.0, 0.0, 0.0, 1.0, 0.0, half, 0.0, half]);
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let image = (bin.len(), png_bytes().len());
        bin.extend_from_slice(&png_bytes());
        let views: Vec<String> = [
            square, uvs, corners, triangle, times, slide, step_times, turn, image,
        ]
        .iter()
        .map(|(o, l)| format!(r#"{{"buffer": 0, "byteOffset": {o}, "byteLength": {l}}}"#))
        .collect();
        let json = format!(
            r#"{{
            "asset": {{"version": "2.0"}},
            "scene": 0,
            "scenes": [{{"nodes": [0]}}],
            "nodes": [
                {{"name": "body", "mesh": 0, "children": [1]}},
                {{"name": "flag", "mesh": 1, "translation": [0, 1, 0]}}
            ],
            "meshes": [
                {{"primitives": [{{"attributes": {{"POSITION": 0, "TEXCOORD_0": 1}}, "indices": 2, "material": 0}}]}},
                {{"primitives": [{{"attributes": {{"POSITION": 3}}, "material": 1}}]}}
            ],
            "materials": [
                {{"pbrMetallicRoughness": {{"baseColorTexture": {{"index": 0}}}}}},
                {{"pbrMetallicRoughness": {{"baseColorFactor": [1, 0, 0, 1]}}}}
            ],
            "textures": [{{"source": 0}}],
            "images": [{{"bufferView": 8, "mimeType": "image/png"}}],
            "accessors": [
                {{"bufferView": 0, "componentType": 5126, "count": 4, "type": "VEC3"}},
                {{"bufferView": 1, "componentType": 5126, "count": 4, "type": "VEC2"}},
                {{"bufferView": 2, "componentType": 5123, "count": 6, "type": "SCALAR"}},
                {{"bufferView": 3, "componentType": 5126, "count": 3, "type": "VEC3"}},
                {{"bufferView": 4, "componentType": 5126, "count": 2, "type": "SCALAR"}},
                {{"bufferView": 5, "componentType": 5126, "count": 2, "type": "VEC3"}},
                {{"bufferView": 6, "componentType": 5126, "count": 2, "type": "SCALAR"}},
                {{"bufferView": 7, "componentType": 5126, "count": 2, "type": "VEC4"}}
            ],
            "bufferViews": [{}],
            "buffers": [{{"byteLength": {}}}],
            "animations": [{{
                "name": "slide",
                "samplers": [
                    {{"input": 4, "output": 5}},
                    {{"input": 6, "output": 7, "interpolation": "STEP"}}
                ],
                "channels": [
                    {{"sampler": 0, "target": {{"node": 0, "path": "translation"}}}},
                    {{"sampler": 1, "target": {{"node": 1, "path": "rotation"}}}}
                ]
            }}]
        }}"#,
            views.join(", "),
            bin.len()
        );
        glb(&json, &bin)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn a_model_keeps_its_texture_its_tree_and_its_animation() {
        let model = GltfModel::from_glb("test", &test_model(), None).unwrap();
        assert!(model.warnings.is_empty(), "{:?}", model.warnings);
        assert_eq!(model.node_names(), ["body", "flag"]);
        assert_eq!(model.animations()[0].name, "slide");
        assert_eq!(model.animations()[0].seconds, 1.0);
        let meshes = model.meshes();
        assert_eq!(meshes.len(), 2);
        let square = &meshes[0];
        let texture = square.texture.as_ref().expect("textured");
        assert_eq!((texture.width(), texture.height()), (2, 2));
        assert_eq!(square.triangles.len(), 2);
        assert_eq!(meshes[1].triangles[0].color, Color::rgb(255, 0, 0));
        // The flag hangs 1 above the body.
        let flag_at = model.node_position("flag", None, 0.0).unwrap();
        assert!(close(flag_at, vec3(0.0, 1.0, 0.0)), "{flag_at:?}");

        // Half way: the body has moved 1 along (glTF's +x is Duck Forge's
        // -x), and the flag with it; the flag has turned.
        let body = model.node_position("body", Some("slide"), 0.5).unwrap();
        assert!(close(body, vec3(-1.0, 0.0, 0.0)), "{body:?}");
        let moved = model.meshes_at("slide", 0.5);
        for (rest, now) in meshes[0].vertices.iter().zip(&moved[0].vertices) {
            assert!(close(*now, *rest + vec3(-1.0, 0.0, 0.0)));
        }
        let flag_before = model.meshes_at("slide", 0.49)[1].vertices[2];
        let flag_after = model.meshes_at("slide", 0.5)[1].vertices[2];
        assert!(
            close(flag_before, vec3(-0.49 * 2.0 + 0.3, 1.0, 0.0)),
            "{flag_before:?}"
        );
        // A quarter turn round y: the corner 0.3 to the side is now 0.3
        // ahead or behind.
        let offset = flag_after - vec3(-1.0, 1.0, 0.0);
        assert!(
            offset.x.abs() < 1e-4 && (offset.z.abs() - 0.3).abs() < 1e-4,
            "{offset:?}"
        );
        // It loops: 1.25 seconds is 0.25.
        let late = model.node_position("body", Some("slide"), 1.25).unwrap();
        assert!(close(late, vec3(-0.5, 0.0, 0.0)), "{late:?}");
    }

    #[test]
    fn a_textured_model_draws_its_texture_and_moves_when_animated() {
        let model = GltfModel::from_glb("test", &test_model(), None).unwrap();
        let camera = Camera3D::new(vec3(0.0, 0.0, -2.0));
        let draw = |seconds: Option<f32>| {
            let mut canvas = Canvas::new(80, 80);
            canvas.clear(Color::BLACK);
            canvas.set_lighting(crate::render3d::Lighting {
                ambient: 1.0,
                ..Default::default()
            });
            match seconds {
                None => model.draw(&mut canvas, &Transform::default(), &camera),
                Some(s) => {
                    model.draw_animated(&mut canvas, &Transform::default(), &camera, "slide", s)
                }
            }
            canvas.flush_3d();
            canvas
        };
        let still = draw(None);
        // The square's four texture colours are all on screen.
        let has = |c: &Canvas, color: Color| c.pixels().contains(&color.to_u32());
        for color in [
            Color::rgb(255, 0, 0),
            Color::rgb(0, 255, 0),
            Color::rgb(0, 0, 255),
        ] {
            assert!(has(&still, color), "{color:?}");
        }
        // Half a second in, it has slid left (out of the middle).
        let moved = draw(Some(0.5));
        let middle = |c: &Canvas| c.get_pixel(40, 40).unwrap();
        assert_ne!(middle(&still), Color::BLACK);
        assert_eq!(middle(&moved), Color::BLACK);
    }

    #[test]
    fn a_model_duck_mold_didnt_make_loads_with_gltf_model_and_load_glb_says_so() {
        let dir = crate::test_folder::TestFolder::new("gltf-load");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("slider.glb");
        std::fs::write(&path, test_model()).unwrap();
        let refused = Mesh::load_glb(&path).unwrap_err().to_string();
        assert!(refused.contains("GltfModel::load"), "{refused}");
        let model = GltfModel::load(&path).unwrap();
        let mesh = model.to_mesh().unwrap();
        assert_eq!(
            mesh.triangles.len(),
            3,
            "the square and the flag, as one mesh"
        );
        assert!(mesh.texture.is_some());
        assert_eq!(model.name, "slider");
        let missing = GltfModel::load(dir.join("nope.glb"))
            .unwrap_err()
            .to_string();
        assert!(
            missing.contains("couldn't find the model file"),
            "{missing}"
        );
    }

    /// Duck Mold's models are glTF too: the general reader reads every one
    /// to the same triangles, in the same colours and places, as Duck
    /// Mold's own reader does.
    #[test]
    fn a_duck_mold_model_reads_the_same_with_either_reader() {
        let fixtures = crate::test_folder::TestFolder::new("gltf-duck-mold");
        let folder = fixtures.fixture("duck_mold");
        let mut models = 0;
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "glb") {
                continue;
            }
            models += 1;
            let duck_mold = crate::glb::GlbModel::load(&path).unwrap().to_mesh();
            let general = GltfModel::load(&path).unwrap();
            assert!(
                general.warnings.is_empty(),
                "{path:?}: {:?}",
                general.warnings
            );
            let general = general.to_mesh().unwrap();
            // The same triangles, corner for corner (the general reader
            // doesn't join corners in the same place, so compare points).
            let corners = |mesh: &Mesh| -> Vec<([[i64; 3]; 3], Color)> {
                let mut list: Vec<_> = mesh
                    .triangles
                    .iter()
                    .map(|t| {
                        let points = t.corners.map(|i| {
                            let p = mesh.vertices[i];
                            [p.x, p.y, p.z].map(|v| (v * 1000.0).round() as i64)
                        });
                        (points, t.color)
                    })
                    .collect();
                list.sort_by_key(|(points, c)| (*points, c.r, c.g, c.b));
                list
            };
            assert_eq!(
                corners(&duck_mold),
                corners(&general),
                "{} reads differently",
                path.display()
            );
        }
        assert!(models >= 9, "{models} models");
    }

    #[test]
    fn what_isnt_read_is_said_and_broken_files_are_refused() {
        // A JPEG texture: the model loads, untextured, and says why.
        let mut bytes = test_model();
        let at = bytes
            .windows(4)
            .position(|w| w == [0x89, b'P', b'N', b'G'])
            .unwrap();
        bytes[at] = 0xFF;
        bytes[at + 1] = 0xD8;
        let model = GltfModel::from_glb("jpeg", &bytes, None).unwrap();
        assert!(model.warnings[0].contains("JPEG"), "{:?}", model.warnings);
        assert!(model.meshes()[0].texture.is_none());

        let error = GltfModel::from_glb("x", b"not a model at all, really", None).unwrap_err();
        assert!(error.contains("doesn't start with \"glTF\""), "{error}");
        let empty = glb(r#"{"asset": {"version": "2.0"}}"#, &[]);
        let error = GltfModel::from_glb("x", &empty, None).unwrap_err();
        assert!(error.contains("no triangles"), "{error}");
        assert_eq!(base64("aGk=").unwrap(), b"hi");
    }
}
