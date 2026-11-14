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
//! which triangles belong to which part, and to which assembly (a part
//! made of parts, like a robot's arm), to recolour one; and its
//! *sockets*, the named points where its recipe joined its parts, to put
//! something there at run time:
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # use duckforge::glb::GlbModel;
//! # fn main() -> Result<(), duckforge::LoadError> {
//! let model = GlbModel::load("assets/robot.glb")?;
//! let robot = model.to_mesh();
//! let place = Transform::at(vec3(2.0, 0.0, 0.0));
//! if let Some(top) = model.socket("head.top") {
//!     let hat_at = place.apply(top.position()); // on top of its head
//! }
//! # Ok(())
//! # }
//! ```
//!
//! `duck-mold inspect robot.toml --sockets` lists a model's sockets.
//!
//! **A model that moves.** A Duck Mold recipe can hold animations (a
//! walk, a wave) and poses. [`AnimatedModel`] loads the model and plays
//! them:
//!
//! ```no_run
//! # use duckforge::prelude::*;
//! # use duckforge::glb::AnimatedModel;
//! # fn main() -> Result<(), duckforge::LoadError> {
//! let mut robot = AnimatedModel::load("assets/robot.glb")?; // in new()
//! robot.play("idle");
//! # let (walking, dt) = (true, 1.0 / 60.0);
//! // in update():
//! if walking { robot.play("walk") } else { robot.play("idle") }
//! robot.update(dt); // ctx.dt()
//! // in draw(): robot.mesh() is a mesh like any other
//! # let (mut canvas, camera) = (Canvas::new(8, 8), Camera3D::new(Vec3::ZERO));
//! canvas.draw_mesh(robot.mesh(), &Transform::at(Vec3::ZERO), &camera);
//! # Ok(())
//! # }
//! ```
//!
//! `duck-mold inspect robot.toml` lists a model's animations and poses.
//!
//! From Duck Mold's Phase 5 a file is laid out as a tree: every part is
//! an object of its own, at the join it turns about, hanging from the
//! part it is joined to. [`GlbModel::joins`] is that tree (what each part
//! turns about and hangs from), and a socket says which part carries it.
//! A model is put together at rest when it is loaded, so it loads and
//! draws as it always did. Files from before still load too; they have
//! no joins.
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
//! [`GlbModel::part`], [`GlbModel::socket`], [`GlbPart::range`],
//! [`GlbSocket::position`], [`GlbSocket::direction`],
//! [`Mesh::load_glb`] and [`AnimatedModel`].
//! `docs/decisions.md` ("Models from Duck Mold") says why.
//!
//! The file format, and what is done to its numbers, is in Duck Mold's
//! `docs/forge-handoff.md`. In short: a `.glb` is a 12-byte header, a
//! block of JSON describing the model and a block of plain numbers. A
//! part's corners are measured from its pivot, and the pivot from the
//! pivot of the part it hangs from; the reader adds them up. glTF's
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
use crate::math::{Vec3, vec3};
use crate::render3d::Mesh;

// ==== Duck Mold's .glb reader, version 5 ====================================
//
// Version 5: the model's animations and named poses, in `animations` and
// `poses`, and what it takes to play them: where an animation has every
// join at a time ([`GlbAnimation::at`]), and the model's corners and
// sockets with its joins so placed ([`GlbModel::stands`],
// [`GlbModel::posed`]). Version 4: files laid out as a tree of parts (Duck Mold's Phase 5), each
// part an object at the join it turns about: `joins`, and each socket's
// `join`. Files from before still load, as they always did. Version 3: the
// model's named sockets, in `sockets`, so a game can put a
// hat on `head.top`. Version 2: a part made of parts (an "assembly") has a
// triangle range too, in `assemblies`; and sizes read from the file are
// checked before they are added or multiplied, so a damaged file is refused
// instead of overflowing. Version 1 was Duck Mold's Phase 0.

/// A model read from a Duck Mold `.glb`, in Duck Forge's directions:
/// x is right, y is up, z is forward, 1 unit is about a metre. The model
/// faces forward (+z) and stands on y = 0.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbModel {
    /// The model's name.
    pub name: String,
    /// The corner points. Corners of a part that are in the same place
    /// are shared, so a box has 8. In a file laid out as a tree, no two
    /// parts share a corner (each part's are next to each other: see
    /// [`GlbJoin`]), so that a part can move by itself.
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
    /// The model's sockets: named points, each facing a way, where things
    /// join. The model's own have bare names (`top`), a part's are
    /// `part.socket` (`head.top`), a part inside an assembly keeps its
    /// path (`left_arm/hand.bottom`). Parts without a name have none
    /// here. Empty for files from before version 3.
    pub sockets: Vec<GlbSocket>,
    /// The tree the model moves by: one join for every part and every
    /// assembly, each hanging from the one it is joined to (or inside),
    /// and always after it in this list, so that one pass from first to
    /// last meets every join after the one it hangs from. Empty for a
    /// file that isn't laid out as a tree (from before version 4): such
    /// a model can be drawn, not posed.
    pub joins: Vec<GlbJoin>,
    /// The model's animations, in the recipe's order. Empty for a model
    /// with none, and for files from before version 5.
    pub animations: Vec<GlbAnimation>,
    /// The model's named poses. `rest`, the model as it is built, isn't
    /// in the list: it is every join at [`GlbPlace::REST`].
    pub poses: Vec<GlbPose>,
}

/// One join of the model's tree: a part (or an assembly), the point it
/// turns about, and what it hangs from.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbJoin {
    /// The part's name, as in `parts` and `assemblies`: `head`,
    /// `left_arm`, `left_arm/hand`.
    pub name: String,
    /// The join it hangs from, as a place in `joins`: turning that one
    /// carries this one along. `None`: it hangs from the model itself.
    pub parent: Option<usize>,
    /// The point it turns about, in the model's own space, as the model
    /// stands at rest: the socket it is joined to, or its own middle.
    pub pivot: [f32; 3],
    /// The first of the part's own corners, as a place in `vertices`.
    pub first_vertex: usize,
    /// How many corners it has of its own: none for an assembly, whose
    /// corners are those of the parts inside it.
    pub vertices: usize,
    /// The three directions its turns are measured around: what x, y
    /// and z mean in a pose's `[x, y, z]` for this part. The model's own
    /// right, up and forward, unless the part is inside one that is
    /// turned or a mirror image.
    pub turns_in: [[f32; 3]; 3],
}

/// How one join stands: how it is turned about its pivot, and how far
/// it is moved from where it is at rest. Both are measured in the space
/// of the join it hangs from, so turning an arm carries the hand.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlbPlace {
    /// The turn, as four numbers (a "quaternion": x, y, z, w), 1 long.
    /// `[0, 0, 0, 1]` is no turn. [`GlbModel::turn_by`] makes one from
    /// degrees.
    pub turn: [f32; 4],
    /// The move, in the model's units.
    pub moved: [f32; 3],
}

impl GlbPlace {
    /// As the part stands when the model is at rest.
    pub const REST: GlbPlace = GlbPlace {
        turn: [0.0, 0.0, 0.0, 1.0],
        moved: [0.0; 3],
    };

    /// True if it is neither turned nor moved.
    pub fn is_rest(&self) -> bool {
        self.turn[0] == 0.0 && self.turn[1] == 0.0 && self.turn[2] == 0.0 && self.moved == [0.0; 3]
    }

    /// A place between this one and `other`: `part` of the way there,
    /// from 0 (this one) to 1 (the other). The turn goes the short way
    /// round.
    pub fn towards(&self, other: &GlbPlace, part: f32) -> GlbPlace {
        if part <= 0.0 {
            return *self;
        }
        if part >= 1.0 {
            return *other;
        }
        let (a, b) = (self.turn, other.turn);
        // The same turn can be written two ways, one the other with
        // every sign changed. Written alike, the two are close together.
        let alike = a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + a[3] * b[3];
        let sign = if alike < 0.0 { -1.0 } else { 1.0 };
        let mut turn = [0.0f32; 4];
        for (at, number) in turn.iter_mut().enumerate() {
            *number = a[at] * (1.0 - part) + b[at] * sign * part;
        }
        let long =
            (turn[0] * turn[0] + turn[1] * turn[1] + turn[2] * turn[2] + turn[3] * turn[3]).sqrt();
        let turn = if long > 0.0 {
            turn.map(|number| number / long + 0.0)
        } else {
            a
        };
        let mut moved = [0.0f32; 3];
        for (at, number) in moved.iter_mut().enumerate() {
            *number = self.moved[at] + (other.moved[at] - self.moved[at]) * part;
        }
        GlbPlace { turn, moved }
    }
}

/// One animation: for every join it moves, how the join stands at each
/// of a list of times. Between two times it goes straight from the one
/// to the other.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbAnimation {
    /// Its name in the recipe.
    pub name: String,
    /// True if the recipe says it starts again after its end.
    pub looping: bool,
    /// How long it lasts, in seconds.
    pub length: f32,
    /// The times of its samples, in seconds from its start, each later
    /// than the one before. Empty if it moves nothing.
    pub times: Vec<f32>,
    /// What it moves.
    pub tracks: Vec<GlbTrack>,
}

/// One join's way through an animation.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbTrack {
    /// The join, as a place in `joins`.
    pub join: usize,
    /// Its turn at each of the animation's times. Empty if the
    /// animation doesn't turn it.
    pub turns: Vec<[f32; 4]>,
    /// How far it is moved at each of the animation's times. Empty if
    /// the animation doesn't move it.
    pub moves: Vec<[f32; 3]>,
}

impl GlbAnimation {
    /// How the animation has each join it moves, `seconds` from its
    /// start: the join (a place in `joins`) and how it stands. Before
    /// the start it is as at the start, and after the end as at the end.
    pub fn at(&self, seconds: f32) -> Vec<(usize, GlbPlace)> {
        let Some(last) = self.times.len().checked_sub(1) else {
            return Vec::new();
        };
        // The sample at or before that time, and how far on to the next.
        let (before, part) = if seconds <= self.times[0] || last == 0 {
            (0, 0.0)
        } else if seconds >= self.times[last] {
            (last, 0.0)
        } else {
            let after = self.times.partition_point(|&time| time <= seconds);
            let before = after - 1;
            let step = self.times[after] - self.times[before];
            (before, (seconds - self.times[before]) / step)
        };
        self.tracks
            .iter()
            .map(|track| {
                let sample = |at: usize| GlbPlace {
                    turn: track.turns.get(at).copied().unwrap_or(GlbPlace::REST.turn),
                    moved: track.moves.get(at).copied().unwrap_or([0.0; 3]),
                };
                let place = if part > 0.0 {
                    sample(before).towards(&sample(before + 1), part)
                } else {
                    sample(before)
                };
                (track.join, place)
            })
            .collect()
    }
}

/// A named pose: the joins that aren't as they are at rest.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbPose {
    /// Its name in the recipe.
    pub name: String,
    /// Each join it turns or moves (a place in `joins`), and how.
    pub joints: Vec<(usize, GlbPlace)>,
}

/// Where a posed model has one of its joins: every point of the part
/// goes from where it is at rest to where [`GlbStand::point`] says.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlbStand {
    /// The part's whole turn, with those of everything it hangs from:
    /// three rows of three.
    turn: [[f64; 3]; 3],
    /// Where its pivot was, and where it is now.
    from: [f64; 3],
    to: [f64; 3],
}

impl GlbStand {
    /// Where a point of the part is now, given where it is at rest.
    pub fn point(&self, at_rest: [f32; 3]) -> [f32; 3] {
        let own = [0, 1, 2].map(|axis| f64::from(at_rest[axis]) - self.from[axis]);
        let turned = turned(&self.turn, own);
        [0, 1, 2].map(|axis| (self.to[axis] + turned[axis]) as f32 + 0.0)
    }

    /// The way a direction of the part points now, given the way it
    /// points at rest.
    pub fn direction(&self, at_rest: [f32; 3]) -> [f32; 3] {
        turned(&self.turn, at_rest.map(f64::from)).map(|number| number as f32 + 0.0)
    }
}

/// A turn applied to a point.
fn turned(turn: &[[f64; 3]; 3], point: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|row| turn[row][0] * point[0] + turn[row][1] * point[1] + turn[row][2] * point[2])
}

/// Two turns, one after the other: `second` after `first`.
fn turns_together(second: &[[f64; 3]; 3], first: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    [0, 1, 2].map(|row| {
        [0, 1, 2].map(|column| {
            second[row][0] * first[0][column]
                + second[row][1] * first[1][column]
                + second[row][2] * first[2][column]
        })
    })
}

/// A turn written as four numbers, as three rows of three.
fn turn_rows(turn: [f32; 4]) -> [[f64; 3]; 3] {
    let [x, y, z, w] = turn.map(f64::from);
    [
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - z * w),
            2.0 * (x * z + y * w),
        ],
        [
            2.0 * (x * y + z * w),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - x * w),
        ],
        [
            2.0 * (x * z - y * w),
            2.0 * (y * z + x * w),
            1.0 - 2.0 * (x * x + y * y),
        ],
    ]
}

impl GlbModel {
    /// Where the model has every join when each stands as `places`
    /// says: one place for each of `joins`, in the same order (a join
    /// with none is at rest). `None` for a join that is where it is at
    /// rest, with everything it hangs from.
    pub fn stands(&self, places: &[GlbPlace]) -> Vec<Option<GlbStand>> {
        const STILL: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let mut stands: Vec<Option<GlbStand>> = Vec::with_capacity(self.joins.len());
        for (at, join) in self.joins.iter().enumerate() {
            let place = places.get(at).copied().unwrap_or(GlbPlace::REST);
            // What it hangs from: its turn, and where its pivot was and
            // is. (A join always comes after the one it hangs from.)
            let above = join.parent.filter(|parent| *parent < at);
            let carried = above.and_then(|parent| stands[parent]);
            if place.is_rest() && carried.is_none() {
                stands.push(None);
                continue;
            }
            let rest_above =
                above.map_or([0.0; 3], |parent| self.joins[parent].pivot.map(f64::from));
            let (turn_above, from_above, to_above) = match carried {
                Some(stand) => (stand.turn, stand.from, stand.to),
                None => (STILL, rest_above, rest_above),
            };
            let pivot = join.pivot.map(f64::from);
            // Its pivot, measured from the one above, and moved; then
            // carried by the one above.
            let own =
                [0, 1, 2].map(|axis| pivot[axis] - from_above[axis] + f64::from(place.moved[axis]));
            let carried_to = turned(&turn_above, own);
            stands.push(Some(GlbStand {
                turn: turns_together(&turn_above, &turn_rows(place.turn)),
                from: pivot,
                to: [0, 1, 2].map(|axis| to_above[axis] + carried_to[axis]),
            }));
        }
        stands
    }

    /// The model's corner points with every join standing as `places`
    /// says ([`GlbModel::stands`]): the same list as `vertices`, each
    /// point where its part now has it. A part that hasn't moved keeps
    /// its points exactly.
    pub fn posed(&self, places: &[GlbPlace]) -> Vec<[f32; 3]> {
        let mut vertices = self.vertices.clone();
        for (join, stand) in self.joins.iter().zip(self.stands(places)) {
            let Some(stand) = stand else {
                continue;
            };
            let own = join.first_vertex..join.first_vertex + join.vertices;
            for vertex in &mut vertices[own] {
                *vertex = stand.point(*vertex);
            }
        }
        vertices
    }

    /// A join's turn from degrees: around x, then y, then z, measured
    /// in the join's own directions (`turns_in`), as a pose in a recipe
    /// writes it. For [`GlbPlace::turn`].
    pub fn turn_by(&self, join: usize, degrees: [f32; 3]) -> [f32; 4] {
        let [x, y, z] = degrees.map(|angle| f64::from(angle).to_radians());
        let ((sx, cx), (sy, cy), (sz, cz)) = (x.sin_cos(), y.sin_cos(), z.sin_cos());
        // Duck Forge's directions: a turn around x takes forward
        // towards up, around y forward towards right, around z right
        // towards up.
        let around_x = [[1.0, 0.0, 0.0], [0.0, cx, sx], [0.0, -sx, cx]];
        let around_y = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
        let around_z = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
        let turn = turns_together(&around_z, &turns_together(&around_y, &around_x));
        // Seen from the model: the join's three directions are the
        // columns of `into`, and its rows take a direction back.
        let directions = self.joins.get(join).map_or(
            [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            |join| join.turns_in.map(|direction| direction.map(f64::from)),
        );
        let into = [0, 1, 2].map(|row| [0, 1, 2].map(|column| directions[column][row]));
        let m = turns_together(&into, &turns_together(&turn, &directions));
        // As four numbers: the biggest of them from the diagonal, the
        // others from it, so that nothing small is divided by.
        let trace = m[0][0] + m[1][1] + m[2][2];
        let [qx, qy, qz, qw] = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            [
                (m[2][1] - m[1][2]) / s,
                (m[0][2] - m[2][0]) / s,
                (m[1][0] - m[0][1]) / s,
                s / 4.0,
            ]
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            [
                s / 4.0,
                (m[0][1] + m[1][0]) / s,
                (m[0][2] + m[2][0]) / s,
                (m[2][1] - m[1][2]) / s,
            ]
        } else if m[1][1] > m[2][2] {
            let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            [
                (m[0][1] + m[1][0]) / s,
                s / 4.0,
                (m[1][2] + m[2][1]) / s,
                (m[0][2] - m[2][0]) / s,
            ]
        } else {
            let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            [
                (m[0][2] + m[2][0]) / s,
                (m[1][2] + m[2][1]) / s,
                s / 4.0,
                (m[1][0] - m[0][1]) / s,
            ]
        };
        let long = (qx * qx + qy * qy + qz * qz + qw * qw).sqrt();
        // Written with w not below zero, so that no turn is `[0, 0, 0, 1]`.
        let sign = if qw < 0.0 { -long } else { long };
        [qx, qy, qz, qw].map(|number| (number / sign) as f32 + 0.0)
    }
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

/// One named socket: a point on the model and the way it faces, in the
/// model's own space (before the model is moved or turned), like the
/// vertices.
#[derive(Debug, Clone, PartialEq)]
pub struct GlbSocket {
    /// Its name: `top`, `head.top`, `left_arm/hand.bottom`.
    pub name: String,
    /// Where it is.
    pub at: [f32; 3],
    /// The way it faces, 1 long: something joined there faces the other
    /// way, the two pressed together.
    pub facing: [f32; 3],
    /// The join that carries it, as a place in `joins`: when that part
    /// moves, the socket moves with it. `None` for a socket of the whole
    /// model that no part carries, and in files from before version 4.
    pub join: Option<usize>,
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

    // ---- The description. Version 0 of it (Duck Mold's Phases 0 to 4)
    // is one object with one list of triangles. Version 1 (Phase 5) is a
    // tree of objects, one for every part, each with its own triangles,
    // measured from the point the part turns about.
    let extras = &gltf["extras"]["duck_mold"];
    let tree = match extras["version"].as_i64() {
        Some(0) => false,
        Some(1) => true,
        Some(_) => {
            return Err(
                "this .glb was written by a newer Duck Mold than this reader knows: update Duck Forge"
                    .into(),
            );
        }
        None => {
            return Err(
                "this .glb wasn't written by Duck Mold (no extras.duck_mold version)".into(),
            );
        }
    };

    // ---- The tree: which object hangs from which, and where each is.
    // An object's place is its own shift added to those of everything it
    // hangs from. At rest nothing is turned or stretched: only shifted.
    let mut pivots: Vec<[f64; 3]> = Vec::new();
    // Each object's own shift, as the file writes it.
    let mut shifts: Vec<[f32; 3]> = Vec::new();
    let mut parents: Vec<Option<usize>> = Vec::new();
    // The objects in the order of a walk down the tree from the model
    // (object 0), so each comes after the one it hangs from.
    let mut walk: Vec<usize> = Vec::new();
    if tree {
        let nodes = gltf["nodes"]
            .as_array()
            .ok_or("a .glb laid out as a tree has no objects")?;
        pivots = vec![[0.0; 3]; nodes.len()];
        shifts = vec![[0.0; 3]; nodes.len()];
        parents = vec![None; nodes.len()];
        let mut reached = vec![false; nodes.len()];
        let mut waiting = vec![0usize];
        while let Some(node) = waiting.pop() {
            let object = nodes
                .get(node)
                .ok_or("an object names one that isn't there")?;
            if std::mem::replace(&mut reached[node], true) {
                return Err("an object hangs from two others, or from itself".into());
            }
            if ["rotation", "scale", "matrix"]
                .iter()
                .any(|field| !object[*field].is_null())
            {
                return Err(
                    "an object is turned or stretched at rest, which Duck Mold never writes".into(),
                );
            }
            let mut place = match parents[node] {
                Some(parent) => pivots[parent],
                None => [0.0; 3],
            };
            match object["translation"].as_array().map(Vec::as_slice) {
                None => {}
                Some([x, y, z]) => match (x.as_f64(), y.as_f64(), z.as_f64()) {
                    (Some(x), Some(y), Some(z)) => {
                        // x is negated, like the points.
                        place = [place[0] - x, place[1] + y, place[2] + z];
                        shifts[node] = [x as f32, y as f32, z as f32];
                    }
                    _ => return Err("an object's place isn't three numbers".into()),
                },
                Some(_) => return Err("an object's place isn't three numbers".into()),
            }
            // Kept as 32-bit numbers, like every number of the file: the
            // corners of its part were measured from exactly there.
            pivots[node] = place.map(|number| f64::from(number as f32));
            walk.push(node);
            // Pushed last to first, so they are taken first to last.
            let children = object["children"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            for child in children.iter().rev() {
                let child = child
                    .as_u64()
                    .and_then(|child| usize::try_from(child).ok())
                    .filter(|child| *child < nodes.len())
                    .ok_or("an object names one that isn't there")?;
                parents[child] = Some(node);
                waiting.push(child);
            }
        }
    }

    // ---- The lumps of triangles: the whole model's in a file from
    // before; in a tree, each part's own, in the order of `parts`. Each
    // lump is read the same way.
    //
    // Points: glTF's "right" is -x and Duck Forge's is +x, so x is
    // negated. Corners of a lump in the same place become one vertex.
    // Triangles: negating x turns every triangle inside out, so the
    // second and third corners swap: that gives Duck Forge's order,
    // clockwise seen from the front. All three corners of a triangle
    // carry the same colour; it is read from the first.
    let mut vertices: Vec<[f32; 3]> = Vec::new();
    let mut triangles: Vec<GlbTriangle> = Vec::new();
    // In a tree: for each object, the name of its part and its corners.
    let mut named: Vec<Option<(String, usize, usize)>> = vec![None; pivots.len()];
    // And the directions its turns are measured around, where the file
    // says them: x negated, like the points.
    const OWN: [[f32; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut turns_in: Vec<[[f32; 3]; 3]> = vec![OWN; pivots.len()];
    if tree {
        for (list, with_shape) in [(&extras["parts"], true), (&extras["assemblies"], false)] {
            for entry in list.as_array().map(Vec::as_slice).unwrap_or_default() {
                let name = entry["name"].as_str().ok_or("a part has no name")?;
                let node = entry["node"]
                    .as_u64()
                    .and_then(|node| usize::try_from(node).ok())
                    .filter(|node| *node > 0 && *node < pivots.len() && parents[*node].is_some())
                    .ok_or_else(|| format!("the part \"{name}\" has no object in the tree"))?;
                if named[node].is_some() {
                    return Err(format!(
                        "the part \"{name}\" shares its object with another"
                    ));
                }
                let first_vertex = vertices.len();
                if with_shape {
                    // The parts are in the order of their triangles.
                    if entry["first_triangle"].as_u64() != Some(triangles.len() as u64) {
                        return Err(format!("the part \"{name}\" isn't where its triangles are"));
                    }
                    // (A part with no triangles has no mesh.)
                    let before = triangles.len();
                    if let Some(mesh) = gltf["nodes"][node]["mesh"].as_u64() {
                        add_lump(
                            &gltf,
                            bin,
                            &gltf["meshes"][mesh as usize]["primitives"][0],
                            pivots[node],
                            &mut vertices,
                            &mut triangles,
                        )?;
                    }
                    if entry["triangles"].as_u64() != Some((triangles.len() - before) as u64) {
                        return Err(format!(
                            "the part \"{name}\" names triangles that aren't there"
                        ));
                    }
                }
                named[node] = Some((name.to_owned(), first_vertex, vertices.len() - first_vertex));
                if let Some(directions) = entry["turns_in"].as_array() {
                    let mut read = OWN;
                    for (at, direction) in read.iter_mut().enumerate() {
                        let number = |axis: usize| {
                            directions
                                .get(at)
                                .and_then(|direction| direction[axis].as_f64())
                                .ok_or_else(|| {
                                    format!("the part \"{name}\" has broken directions to turn in")
                                })
                        };
                        *direction = [
                            -(number(0)? as f32) + 0.0,
                            number(1)? as f32 + 0.0,
                            number(2)? as f32 + 0.0,
                        ];
                    }
                    turns_in[node] = read;
                }
            }
        }
    } else {
        add_lump(
            &gltf,
            bin,
            &gltf["meshes"][0]["primitives"][0],
            [0.0; 3],
            &mut vertices,
            &mut triangles,
        )?;
    }

    // ---- The joins: every object but the model itself, in the order of
    // the walk.
    let mut join_of_node: Vec<Option<usize>> = vec![None; pivots.len()];
    let mut joins: Vec<GlbJoin> = Vec::new();
    for &node in walk.iter().skip(1) {
        let (name, first_vertex, count) = named[node]
            .take()
            .ok_or("an object of the tree is no part of the model")?;
        join_of_node[node] = Some(joins.len());
        joins.push(GlbJoin {
            name,
            parent: parents[node].and_then(|parent| join_of_node[parent]),
            pivot: pivots[node].map(|number| number as f32 + 0.0),
            first_vertex,
            vertices: count,
            turns_in: turns_in[node],
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

    // The sockets, with x negated like the points.
    let mut sockets = Vec::new();
    for entry in extras["sockets"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = entry["name"].as_str().ok_or("a socket has no name")?;
        let three = |field: &str| match entry[field].as_array().map(Vec::as_slice) {
            Some([x, y, z]) => match (x.as_f64(), y.as_f64(), z.as_f64()) {
                (Some(x), Some(y), Some(z)) => {
                    Ok([-(x as f32) + 0.0, y as f32 + 0.0, z as f32 + 0.0])
                }
                _ => Err(format!("the socket \"{name}\" has a broken {field}")),
            },
            _ => Err(format!("the socket \"{name}\" has no {field}")),
        };
        // The object that carries it; the model itself carries the rest.
        let join = match entry["node"].as_u64() {
            None | Some(0) => None,
            Some(node) => Some(
                usize::try_from(node)
                    .ok()
                    .and_then(|node| join_of_node.get(node).copied().flatten())
                    .ok_or_else(|| {
                        format!("the socket \"{name}\" is on an object that isn't there")
                    })?,
            ),
        };
        sockets.push(GlbSocket {
            name: name.to_owned(),
            at: three("at")?,
            facing: three("facing")?,
            join,
        });
    }

    // ---- What moves it: the animations and the named poses. An
    // object's turn is four numbers (x, y, z, w); with x negated like
    // the points, its y and z change sign. An object's place is written
    // whole (its shift at rest and its move together): the move is what
    // is left when the shift at rest is taken away.
    let join_at = |node: &serde_json::Value, what: &str| {
        node.as_u64()
            .and_then(|node| usize::try_from(node).ok())
            .and_then(|node| Some((node, join_of_node.get(node).copied().flatten()?)))
            .ok_or_else(|| format!("{what} moves an object that isn't a part of the model"))
    };
    let turn_read = |x: f32, y: f32, z: f32, w: f32| [x + 0.0, -y + 0.0, -z + 0.0, w + 0.0];
    let move_read = |node: usize, x: f32, y: f32, z: f32| {
        let rest = shifts[node];
        [-(x - rest[0]) + 0.0, y - rest[1] + 0.0, z - rest[2] + 0.0]
    };
    let mut animations = Vec::new();
    for entry in extras["animations"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = entry["name"].as_str().ok_or("an animation has no name")?;
        let what = format!("the animation \"{name}\"");
        let mut times: Vec<f32> = Vec::new();
        let mut tracks: Vec<GlbTrack> = Vec::new();
        // (One that moves nothing has no place in the file's own list.)
        if let Some(index) = entry["animation"].as_u64() {
            let animation = &gltf["animations"][index as usize];
            let mut input: Option<u64> = None;
            for channel in animation["channels"]
                .as_array()
                .ok_or_else(|| format!("{what} isn't in the file"))?
            {
                let sampler = &animation["samplers"]
                    [channel["sampler"].as_u64().unwrap_or(u64::MAX) as usize];
                if sampler["interpolation"].as_str().unwrap_or("LINEAR") != "LINEAR" {
                    return Err(format!(
                        "{what} isn't made of straight steps, which is all Duck Mold writes"
                    ));
                }
                let its_times = sampler["input"]
                    .as_u64()
                    .ok_or_else(|| format!("{what} has no times"))?;
                match input {
                    None => {
                        let list = numbers(&gltf, bin, &sampler["input"], "SCALAR", 5126)?;
                        times = (0..list.len() / 4).map(|at| f32_at(list, at)).collect();
                        if times.is_empty() || times.windows(2).any(|pair| pair[1] <= pair[0]) {
                            return Err(format!("{what} has times that don't go forward"));
                        }
                        input = Some(its_times);
                    }
                    Some(first) if first != its_times => {
                        return Err(format!(
                            "{what} has more than one list of times, which Duck Mold never writes"
                        ));
                    }
                    Some(_) => {}
                }
                let (node, join) = join_at(&channel["target"]["node"], &what)?;
                let track = match tracks.iter().position(|track| track.join == join) {
                    Some(at) => at,
                    None => {
                        tracks.push(GlbTrack {
                            join,
                            turns: Vec::new(),
                            moves: Vec::new(),
                        });
                        tracks.len() - 1
                    }
                };
                match channel["target"]["path"].as_str() {
                    Some("rotation") => {
                        let list = numbers(&gltf, bin, &sampler["output"], "VEC4", 5126)?;
                        if list.len() != times.len() * 16 {
                            return Err(format!("{what} has turns that don't match its times"));
                        }
                        tracks[track].turns = (0..times.len())
                            .map(|at| {
                                let number = |which: usize| f32_at(list, at * 4 + which);
                                turn_read(number(0), number(1), number(2), number(3))
                            })
                            .collect();
                    }
                    Some("translation") => {
                        let list = numbers(&gltf, bin, &sampler["output"], "VEC3", 5126)?;
                        if list.len() != times.len() * 12 {
                            return Err(format!("{what} has moves that don't match its times"));
                        }
                        tracks[track].moves = (0..times.len())
                            .map(|at| {
                                let number = |which: usize| f32_at(list, at * 3 + which);
                                move_read(node, number(0), number(1), number(2))
                            })
                            .collect();
                    }
                    _ => {
                        return Err(format!(
                            "{what} changes something other than a turn or a place"
                        ));
                    }
                }
            }
        }
        animations.push(GlbAnimation {
            name: name.to_owned(),
            looping: entry["loop"].as_bool().unwrap_or(false),
            length: entry["length"].as_f64().unwrap_or(0.0) as f32,
            times,
            tracks,
        });
    }
    let mut poses = Vec::new();
    for entry in extras["poses"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        let name = entry["name"].as_str().ok_or("a pose has no name")?;
        let what = format!("the pose \"{name}\"");
        let mut joints = Vec::new();
        for joint in entry["joints"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let (node, join) = join_at(&joint["node"], &what)?;
            let number = |field: &str, at: usize| {
                joint[field][at]
                    .as_f64()
                    .map(|number| number as f32)
                    .ok_or_else(|| format!("{what} has a broken {field}"))
            };
            let mut place = GlbPlace::REST;
            if !joint["rotation"].is_null() {
                place.turn = turn_read(
                    number("rotation", 0)?,
                    number("rotation", 1)?,
                    number("rotation", 2)?,
                    number("rotation", 3)?,
                );
            }
            if !joint["translation"].is_null() {
                place.moved = move_read(
                    node,
                    number("translation", 0)?,
                    number("translation", 1)?,
                    number("translation", 2)?,
                );
            }
            joints.push((join, place));
        }
        poses.push(GlbPose {
            name: name.to_owned(),
            joints,
        });
    }

    Ok(GlbModel {
        name: extras["name"].as_str().unwrap_or("model").to_owned(),
        vertices,
        triangles,
        palette,
        parts,
        assemblies,
        sockets,
        joins,
        animations,
        poses,
    })
}

/// Reads one lump of triangles (the whole model's in a file from before
/// version 4, one part's in a tree) and adds its points and triangles to
/// the model's. `from` is where its points are measured from.
fn add_lump(
    gltf: &serde_json::Value,
    bin: &[u8],
    primitive: &serde_json::Value,
    from: [f64; 3],
    vertices: &mut Vec<[f32; 3]>,
    triangles: &mut Vec<GlbTriangle>,
) -> Result<(), String> {
    if primitive["mode"].as_u64().unwrap_or(4) != 4 {
        return Err("the mesh isn't a list of triangles".into());
    }
    // The numbers 5126 and 5125 are glTF's names for "32-bit decimal"
    // and "32-bit whole number".
    let positions = numbers(
        gltf,
        bin,
        &primitive["attributes"]["POSITION"],
        "VEC3",
        5126,
    )?;
    let colors = numbers(gltf, bin, &primitive["attributes"]["COLOR_0"], "VEC3", 5126)?;
    let indices = numbers(gltf, bin, &primitive["indices"], "SCALAR", 5125)?;
    let corner_count = positions.len() / 12;
    if colors.len() != positions.len() || indices.len() % 12 != 0 {
        return Err("the lists of positions, colours and triangles don't match".into());
    }
    let mut vertex_of_corner: Vec<usize> = Vec::with_capacity(corner_count);
    let mut seen: HashMap<[u32; 3], usize> = HashMap::new();
    for corner in 0..corner_count {
        // Measured from `from` (zero, in a file from before), and
        // added up with more digits than are kept. `+ 0.0` turns -0
        // into 0, so that the two count as one place.
        let point = [
            (from[0] - f64::from(f32_at(positions, corner * 3))) as f32 + 0.0,
            (from[1] + f64::from(f32_at(positions, corner * 3 + 1))) as f32 + 0.0,
            (from[2] + f64::from(f32_at(positions, corner * 3 + 2))) as f32 + 0.0,
        ];
        let vertex = *seen.entry(point.map(f32::to_bits)).or_insert_with(|| {
            vertices.push(point);
            vertices.len() - 1
        });
        vertex_of_corner.push(vertex);
    }
    for triangle in 0..indices.len() / 12 {
        let corner =
            |which: usize| u32_at(indices, (triangle * 3 + which) * 4).map(|at| at as usize);
        let (a, b, c) = (corner(0)?, corner(1)?, corner(2)?);
        if a >= corner_count || b >= corner_count || c >= corner_count {
            return Err(format!(
                "triangle {} names a corner that isn't there",
                triangles.len()
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
    Ok(())
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
    let per: usize = match kind {
        "VEC4" => 16,
        "VEC3" => 12,
        _ => 4,
    };
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

// ==== end of Duck Mold's .glb reader, version 5 =============================

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

    /// The socket with this name, if the model has one: `"head.top"` (a
    /// part's), `"left_arm/hand.bottom"` (a part inside another),
    /// `"left_arm.grip"` (an assembly's) or `"top"` (the whole model's).
    /// `duck-mold inspect recipe.toml --sockets` lists them all.
    pub fn socket(&self, name: &str) -> Option<&GlbSocket> {
        self.sockets.iter().find(|socket| socket.name == name)
    }
}

impl GlbSocket {
    /// Where the socket is, in the model's own space. To put something
    /// there, move it with the model: `place.apply(socket.position())`,
    /// where `place` is the [`Transform`](crate::render3d::Transform) the
    /// model is drawn with.
    pub fn position(&self) -> Vec3 {
        let [x, y, z] = self.at;
        vec3(x, y, z)
    }

    /// The way the socket faces, 1 long, in the model's own space: out of
    /// the part, the way something put there points (up for `head.top`).
    pub fn direction(&self) -> Vec3 {
        let [x, y, z] = self.facing;
        vec3(x, y, z)
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

// ---- a model that moves ----------------------------------------------------------

/// How long changing from one animation or pose to another takes, in
/// seconds, unless [`AnimatedModel::set_blend`] says otherwise.
const BLEND: f32 = 0.15;

/// A model made with Duck Mold that plays its animations.
///
/// Load it once, tell it what to play, let time pass in `update`, and
/// draw its mesh:
///
/// ```no_run
/// # use duckforge::prelude::*;
/// # use duckforge::glb::AnimatedModel;
/// # fn main() -> Result<(), duckforge::LoadError> {
/// let mut robot = AnimatedModel::load("assets/robot.glb")?;
/// robot.play("walk");
/// robot.update(1.0 / 60.0);
/// # let (mut canvas, camera) = (Canvas::new(8, 8), Camera3D::new(Vec3::ZERO));
/// canvas.draw_mesh(robot.mesh(), &Transform::at(vec3(2.0, 0.0, 0.0)), &camera);
/// # Ok(())
/// # }
/// ```
///
/// An animation only turns and moves the model's parts about their
/// joins: it doesn't move the model along the ground. A walk is played
/// in place, and the game moves the `Transform` it draws the model with.
///
/// Time only passes in [`update`](AnimatedModel::update), by the `dt`
/// it is given, so a recorded playthrough plays back the same.
#[derive(Debug, Clone)]
pub struct AnimatedModel {
    model: GlbModel,
    /// The model as it stands now.
    mesh: Mesh,
    /// How every join stands now, and where that puts it.
    places: Vec<GlbPlace>,
    stands: Vec<Option<GlbStand>>,
    /// What it is doing.
    doing: Doing,
    /// True between `stop` and `resume`.
    stopped: bool,
    speed: f32,
    /// How long a change takes, and the change that is under way: how
    /// every join stood when it began, and how long ago that was.
    blend: f32,
    changing: Option<(Vec<GlbPlace>, f32)>,
    /// The joins game code has turned itself.
    turned: Vec<Option<[f32; 4]>>,
    /// The names already complained about, so each is said once.
    complained: Vec<String>,
}

/// What an [`AnimatedModel`] is doing.
#[derive(Debug, Clone, PartialEq)]
enum Doing {
    /// Standing as it was built.
    Rest,
    /// Holding a pose: its place in the model's `poses`.
    Pose(usize),
    /// Playing an animation.
    Playing {
        /// Its place in the model's `animations`.
        animation: usize,
        /// Seconds from its start.
        time: f32,
        looping: bool,
        /// True once an animation that plays once has reached its end.
        finished: bool,
    },
}

impl AnimatedModel {
    /// Loads a `.glb` file made by Duck Mold (`duck-mold export`), to
    /// play its animations. A model without any loads too: it stands at
    /// rest, and game code can still turn its joins
    /// ([`turn`](AnimatedModel::turn)).
    ///
    /// A missing file, or one that isn't a Duck Mold model, is a
    /// [`LoadError`]: one sentence saying what's wrong and what to do.
    pub fn load(path: impl AsRef<Path>) -> Result<AnimatedModel, LoadError> {
        Ok(AnimatedModel::new(GlbModel::load(path)?))
    }

    /// The same, from a model that is already loaded.
    pub fn new(model: GlbModel) -> AnimatedModel {
        let joins = model.joins.len();
        AnimatedModel {
            mesh: model.to_mesh(),
            places: vec![GlbPlace::REST; joins],
            stands: vec![None; joins],
            doing: Doing::Rest,
            stopped: false,
            speed: 1.0,
            blend: BLEND,
            changing: None,
            turned: vec![None; joins],
            complained: Vec::new(),
            model,
        }
    }

    /// The model as it was loaded: its name, palette, parts and sockets
    /// at rest.
    pub fn model(&self) -> &GlbModel {
        &self.model
    }

    /// The names of the model's animations.
    pub fn animations(&self) -> Vec<&str> {
        self.model
            .animations
            .iter()
            .map(|animation| animation.name.as_str())
            .collect()
    }

    /// The names of the model's poses. `"rest"`, the model as it was
    /// built, is always one more ([`rest`](AnimatedModel::rest)).
    pub fn poses(&self) -> Vec<&str> {
        self.model
            .poses
            .iter()
            .map(|pose| pose.name.as_str())
            .collect()
    }

    /// Plays an animation from its start. It loops if its recipe says
    /// `loop = true`; otherwise it plays once and stops on its last
    /// pose.
    ///
    /// Asking for the animation that is already playing changes nothing,
    /// so this is safe to call every frame: `if walking {
    /// robot.play("walk") } else { robot.play("idle") }`. (To play it
    /// from its start again, [`restart`](AnimatedModel::restart).)
    ///
    /// The model doesn't snap to the new animation: it changes over to
    /// it in 0.15 seconds ([`set_blend`](AnimatedModel::set_blend)).
    ///
    /// A name the model doesn't have changes nothing, and is said once
    /// on the terminal, with the names it does have.
    pub fn play(&mut self, name: &str) {
        self.start(name, None);
    }

    /// [`play`](AnimatedModel::play), once: it stops on its last pose,
    /// whatever its recipe says.
    pub fn play_once(&mut self, name: &str) {
        self.start(name, Some(false));
    }

    /// [`play`](AnimatedModel::play), round and round, whatever its
    /// recipe says.
    pub fn play_looping(&mut self, name: &str) {
        self.start(name, Some(true));
    }

    fn start(&mut self, name: &str, looping: Option<bool>) {
        let Some(animation) = self
            .model
            .animations
            .iter()
            .position(|animation| animation.name == name)
        else {
            let has = self.animations().join(", ");
            self.complain(name, "animation", &has);
            return;
        };
        let looping = looping.unwrap_or(self.model.animations[animation].looping);
        if let Doing::Playing {
            animation: now,
            looping: now_looping,
            ..
        } = &mut self.doing
        {
            if *now == animation {
                // Already playing: only whether it loops can change.
                *now_looping = looping;
                return;
            }
        }
        self.change_to(Doing::Playing {
            animation,
            // Backwards, it starts from its end.
            time: if self.speed < 0.0 {
                self.model.animations[animation].length
            } else {
                0.0
            },
            looping,
            finished: false,
        });
    }

    /// Plays what is playing from its start again (from its end, if the
    /// speed is negative). Nothing, if no animation is playing.
    pub fn restart(&mut self) {
        let backwards = self.speed < 0.0;
        if let Doing::Playing {
            animation,
            time,
            finished,
            ..
        } = &mut self.doing
        {
            *time = if backwards {
                self.model.animations[*animation].length
            } else {
                0.0
            };
            *finished = false;
            self.stopped = false;
            self.refresh();
        }
    }

    /// Freezes the model as it stands. [`resume`](AnimatedModel::resume)
    /// carries on.
    pub fn stop(&mut self) {
        self.stopped = true;
    }

    /// Carries on after [`stop`](AnimatedModel::stop).
    pub fn resume(&mut self) {
        self.stopped = false;
    }

    /// Goes back to the model as it was built, playing nothing.
    pub fn rest(&mut self) {
        if self.doing != Doing::Rest {
            self.change_to(Doing::Rest);
        }
    }

    /// Holds one of the model's poses, playing nothing. `"rest"` is the
    /// model as it was built. A name the model doesn't have changes
    /// nothing, and is said once on the terminal.
    pub fn set_pose(&mut self, name: &str) {
        if name == "rest" {
            return self.rest();
        }
        let Some(pose) = self.model.poses.iter().position(|pose| pose.name == name) else {
            let has = std::iter::once("rest")
                .chain(self.poses())
                .collect::<Vec<_>>()
                .join(", ");
            self.complain(name, "pose", &has);
            return;
        };
        if self.doing != Doing::Pose(pose) {
            self.change_to(Doing::Pose(pose));
        }
    }

    /// How fast animations play: 1 is as they were made, 2 twice as
    /// fast, 0 still. A negative speed plays backwards: with `-1.0`, an
    /// `open` animation closes the door.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    /// How long changing from one animation or pose to another takes,
    /// in seconds: 0.15 unless this says otherwise, and 0 to snap.
    pub fn set_blend(&mut self, seconds: f32) {
        self.blend = seconds.max(0.0);
    }

    /// Turns one join from game code, in degrees: around x, then y, then
    /// z, about the point the part is joined at, as a pose in the recipe
    /// writes it. `name` is the part's: `"head"`, `"left_arm"`,
    /// `"left_arm/elbow"`. Everything else keeps playing; this join is
    /// the game's until [`release`](AnimatedModel::release) gives it
    /// back. For a head that looks at the player, or a wheel that turns
    /// as fast as the cart drives.
    ///
    /// A name the model doesn't have changes nothing, and is said once
    /// on the terminal.
    pub fn turn(&mut self, name: &str, degrees: Vec3) {
        let Some(join) = self.join(name) else {
            return;
        };
        self.turned[join] = Some(self.model.turn_by(join, [degrees.x, degrees.y, degrees.z]));
        self.refresh();
    }

    /// Gives a join turned with [`turn`](AnimatedModel::turn) back to
    /// what is playing.
    pub fn release(&mut self, name: &str) {
        let Some(join) = self.join(name) else {
            return;
        };
        self.turned[join] = None;
        self.refresh();
    }

    fn join(&mut self, name: &str) -> Option<usize> {
        let join = self.model.joins.iter().position(|join| join.name == name);
        if join.is_none() {
            let has = self
                .model
                .joins
                .iter()
                .map(|join| join.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            self.complain(name, "part", &has);
        }
        join
    }

    /// Moves time on by `dt` seconds. Call it once in your game's
    /// `update`, with `ctx.dt()`.
    pub fn update(&mut self, dt: f32) {
        if self.stopped {
            return;
        }
        if let Some((_, since)) = &mut self.changing {
            *since += dt.abs();
            if *since >= self.blend {
                self.changing = None;
            }
        }
        if let Doing::Playing {
            animation,
            time,
            looping,
            finished,
        } = &mut self.doing
        {
            let length = self.model.animations[*animation].length;
            *time += dt * self.speed;
            if *looping {
                *time = if length > 0.0 {
                    time.rem_euclid(length)
                } else {
                    0.0
                };
            } else if *time >= length && self.speed >= 0.0 {
                (*time, *finished) = (length, true);
            } else if *time <= 0.0 && self.speed < 0.0 {
                (*time, *finished) = (0.0, true);
            } else {
                *time = time.clamp(0.0, length);
                *finished = false;
            }
        }
        self.refresh();
    }

    /// The model as it stands now, to draw:
    /// `canvas.draw_mesh(robot.mesh(), &place, &camera)`. Its triangles
    /// are in the file's order, so a part's
    /// [`range`](GlbPart::range) still picks out its triangles.
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// The same, to change: recolour a part, make it glow. (Its corner
    /// points are set again whenever the model moves.)
    pub fn mesh_mut(&mut self) -> &mut Mesh {
        &mut self.mesh
    }

    /// The name of the animation that is playing, stopped or finished;
    /// `None` at rest or holding a pose.
    pub fn playing(&self) -> Option<&str> {
        match &self.doing {
            Doing::Playing { animation, .. } => {
                Some(self.model.animations[*animation].name.as_str())
            }
            _ => None,
        }
    }

    /// True while an animation is running: not stopped, and not at the
    /// end of one that plays once.
    pub fn is_playing(&self) -> bool {
        !self.stopped
            && matches!(
                self.doing,
                Doing::Playing {
                    finished: false,
                    ..
                }
            )
    }

    /// True once an animation that plays once has reached its end (its
    /// start, played backwards). It stays on that pose.
    pub fn finished(&self) -> bool {
        matches!(self.doing, Doing::Playing { finished: true, .. })
    }

    /// How far into the animation that is playing, in seconds; 0 if
    /// none is.
    pub fn time(&self) -> f32 {
        match self.doing {
            Doing::Playing { time, .. } => time,
            _ => 0.0,
        }
    }

    /// Where one of the model's sockets is **now**, and the way it
    /// faces: the socket, moved with the part that carries it. So a hat
    /// on `"head.top"` stays on a nodding head, and a sword in
    /// `"left_arm.grip"` in a waving hand. Like [`GlbModel::socket`], in
    /// the model's own space: `place.apply(socket.position())`.
    pub fn socket(&self, name: &str) -> Option<GlbSocket> {
        let socket = self.model.socket(name)?;
        let mut now = socket.clone();
        if let Some(stand) = socket.join.and_then(|join| self.stands[join]) {
            now.at = stand.point(socket.at);
            now.facing = stand.direction(socket.facing);
        }
        Some(now)
    }

    /// Starts doing something else, changing over to it from how the
    /// model stands now.
    fn change_to(&mut self, doing: Doing) {
        self.changing = (self.blend > 0.0).then(|| (self.places.clone(), 0.0));
        self.doing = doing;
        self.stopped = false;
        self.refresh();
    }

    /// Works out how every join stands now, and moves the mesh's corner
    /// points there.
    fn refresh(&mut self) {
        // What is playing or held...
        let mut places = vec![GlbPlace::REST; self.model.joins.len()];
        let named = match &self.doing {
            Doing::Rest => Vec::new(),
            Doing::Pose(pose) => self.model.poses[*pose].joints.clone(),
            Doing::Playing {
                animation, time, ..
            } => self.model.animations[*animation].at(*time),
        };
        for (join, place) in named {
            places[join] = place;
        }
        // ...changing over from what was...
        if let Some((from, since)) = &self.changing {
            let part = if self.blend > 0.0 {
                since / self.blend
            } else {
                1.0
            };
            for (place, from) in places.iter_mut().zip(from) {
                *place = from.towards(place, part);
            }
        }
        // ...and the joins the game turns itself.
        let mut stood = places.clone();
        for (place, turned) in stood.iter_mut().zip(&self.turned) {
            if let Some(turn) = turned {
                place.turn = *turn;
            }
        }
        if stood == self.places {
            // (The game's turns aren't part of what a change starts
            // from: they are put on top of it.)
            self.places = stood;
            return;
        }
        self.stands = self.model.stands(&stood);
        for (vertex, [x, y, z]) in self.mesh.vertices.iter_mut().zip(self.model.posed(&stood)) {
            *vertex = vec3(x, y, z);
        }
        self.places = stood;
    }

    /// Says once, on the terminal, that the model has no such thing.
    fn complain(&mut self, name: &str, what: &str, has: &str) {
        let key = format!("{what} {name}");
        if self.complained.contains(&key) {
            return;
        }
        self.complained.push(key);
        let has = if has.is_empty() {
            format!("it has no {what}s")
        } else {
            format!("it has: {has}")
        };
        eprintln!(
            "duckforge: the model \"{}\" has no {what} \"{name}\" ({has})",
            self.model.name
        );
    }
}
