//! Choose Your Door, in 3D: a one-minute vertical video (YouTube Shorts size)
//! on a game-show stage. The doors, the room behind each one and the prizes
//! are 3D models built from boxes, spheres, cylinders and cones; gold coins
//! burst out of the winning door with real physics; the text, countdown and
//! badges are 2D, drawn on top.
//!
//! Watch it live:   cargo run --release --example door_show_3d
//! Make the video:  OUT=video.mp4 cargo test --release --example door_show_3d -- --ignored render
//!                  (pipes every frame to ffmpeg, and writes cues.json next to the video:
//!                  when each sound plays. tools/door_sounds.py turns that into audio.)
//! Bigger frames, for smoother edges once scaled down: DOOR_K=4 (the canvas is 540*K x 960*K).

// Doors are numbered 0..3 and several arrays are indexed by door, so plain index loops read best.
#![allow(clippy::needless_range_loop)]

use std::f32::consts::{PI, TAU};

use duckforge::physics::{Body, PhysicsWorld};
use duckforge::prelude::*;
use duckforge::render3d::Triangle;

const BASE_W: f32 = 540.0;
const BASE_H: f32 = 960.0;
const FPS: u32 = 30;

// ---- The timeline, in seconds (the same as door_short) ----

const INTRO: f32 = 3.0;
const ROUND_LEN: f32 = 13.0;
const END_START: f32 = INTRO + 4.0 * ROUND_LEN; // 55
const TOTAL: f32 = 60.0;
const COUNT_START: f32 = 1.0;
const COUNT_END: f32 = 6.0;
const OPEN_AT: [f32; 3] = [6.8, 8.0, 9.2];
const OPEN_TIME: f32 = 0.55;
const WINNER_AT: f32 = 10.6;
/// The camera whips sideways to the next round from here.
const WHIP_AT: f32 = 12.55;

// ---- The stage, in world units (a door is 1 wide and 2 tall) ----

const DOOR_X: [f32; 3] = [-1.25, 0.0, 1.25];
const DOOR_W: f32 = 0.96;
const DOOR_H: f32 = 1.96;
const ROOM_DEPTH: f32 = 1.3;
/// Prizes stand on a pedestal this high, this far into the room.
const PEDESTAL_TOP: f32 = 0.38;
const PRIZE_Z: f32 = 0.75;

const DOOR_COLORS: [Color; 3] = [
    Color::from_hex(0xff5a5a),
    Color::from_hex(0x4fd47a),
    Color::from_hex(0x4f95ff),
];
const WALL: Color = Color::from_hex(0x7b4fd6);
const WALL_PILLAR: Color = Color::from_hex(0x9a72ee);
const TRIM: Color = Color::from_hex(0xffc04a);
const BASEBOARD: Color = Color::from_hex(0x3a2378);
const FLOOR_A: Color = Color::from_hex(0x3c2f78);
const FLOOR_B: Color = Color::from_hex(0x2c2260);
const ROOM: Color = Color::from_hex(0x2a2140);
const GOLD: Color = Color::from_hex(0xffd23a);
const GOLD_DARK: Color = Color::from_hex(0xd99a14);

const SKY_TOP: Color = Color::from_hex(0x1b0a3d);
const SKY_BOTTOM: Color = Color::from_hex(0x3b1a6e);
const SHADOW: Color = Color::from_hex(0x0a0614);
const TEXT_GOLD: Color = Color::from_hex(0xffcf3a);
const WIN_COLOR: Color = Color::from_hex(0x46d16b);
const OK_COLOR: Color = Color::from_hex(0xf2c94c);
const BAD_COLOR: Color = Color::from_hex(0xf0484a);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Coins,
    Pike,
    RubberDuck,
    Ghost,
    IceCream,
    Cake,
    Rocket,
    Rainbow,
    Bomb,
    Ducklings,
    Treasure,
    Storm,
}

const ITEMS: [Item; 12] = [
    Item::Coins,
    Item::Pike,
    Item::RubberDuck,
    Item::Ghost,
    Item::IceCream,
    Item::Cake,
    Item::Rocket,
    Item::Rainbow,
    Item::Bomb,
    Item::Ducklings,
    Item::Treasure,
    Item::Storm,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rating {
    Win,
    Ok,
    Bad,
}

impl Rating {
    fn color(self) -> Color {
        match self {
            Rating::Win => WIN_COLOR,
            Rating::Ok => OK_COLOR,
            Rating::Bad => BAD_COLOR,
        }
    }

    fn index(self) -> usize {
        match self {
            Rating::Win => 0,
            Rating::Ok => 1,
            Rating::Bad => 2,
        }
    }

    #[cfg_attr(not(test), allow(dead_code))] // used by the render test
    fn sound(self) -> &'static str {
        match self {
            Rating::Win => "win",
            Rating::Ok => "ok",
            Rating::Bad => "bad",
        }
    }
}

struct Round {
    question: &'static str,
    doors: [(Item, Rating, &'static str); 3],
}

const ROUNDS: [Round; 4] = [
    Round {
        question: "FIND THE\nTREASURE",
        doors: [
            (Item::Pike, Rating::Bad, "CHOMP!"),
            (Item::Coins, Rating::Win, "RICH!"),
            (Item::RubberDuck, Rating::Ok, "SQUEAK"),
        ],
    },
    Round {
        question: "PICK YOUR\nDESSERT",
        doors: [
            (Item::IceCream, Rating::Ok, "YUM"),
            (Item::Ghost, Rating::Bad, "BOO!"),
            (Item::Cake, Rating::Win, "CAKE!"),
        ],
    },
    Round {
        question: "WHERE ARE\nYOU GOING?",
        doors: [
            (Item::Rocket, Rating::Win, "SPACE!"),
            (Item::Bomb, Rating::Bad, "BOOM!"),
            (Item::Rainbow, Rating::Ok, "PRETTY"),
        ],
    },
    Round {
        question: "LAST ROUND!\nONE DOOR HAS\nA MILLION",
        doors: [
            (Item::Ducklings, Rating::Ok, "AWW"),
            (Item::Storm, Rating::Bad, "ZAP!"),
            (Item::Treasure, Rating::Win, "MILLION"),
        ],
    },
];

/// Every sound in the video and the second it starts, from the same timeline.
#[cfg_attr(not(test), allow(dead_code))] // used by the render test
fn cues() -> Vec<(f32, &'static str)> {
    let mut cues = vec![
        (0.0, "whoosh"),
        (1.1, "pop"),
        (1.4, "pop"),
        (1.7, "pop"),
        (1.9, "title"),
        (1.9, "music"),
    ];
    for (i, round) in ROUNDS.iter().enumerate() {
        let b = INTRO + i as f32 * ROUND_LEN;
        for k in 0..5 {
            cues.push((
                b + COUNT_START + k as f32,
                if k == 4 { "tick_last" } else { "tick" },
            ));
        }
        cues.push((b + COUNT_END, "drumroll"));
        for (d, &(_, rating, _)) in round.doors.iter().enumerate() {
            cues.push((b + OPEN_AT[d], "creak"));
            cues.push((b + OPEN_AT[d] + 0.25, rating.sound()));
            if rating == Rating::Win {
                cues.push((b + OPEN_AT[d] + 0.3, "coins"));
            }
        }
        cues.push((b + WINNER_AT, "sting"));
        cues.push((b + WHIP_AT, "whoosh"));
    }
    for k in 0..4 {
        cues.push((END_START + 0.6 + 0.45 * k as f32, "pop"));
    }
    cues.push((END_START + 2.6, "fanfare"));
    cues.push((TOTAL, "end"));
    cues.sort_by(|a, b| a.0.total_cmp(&b.0));
    cues
}

// ---- Building meshes out of simple parts ----

/// The unit shapes everything is made of: each 1 unit across, centered on the origin.
struct Parts {
    cube: Mesh,
    sphere: Mesh,
    small_sphere: Mesh,
    cylinder: Mesh,
    cone: Mesh,
    pyramid: Mesh,
}

impl Parts {
    fn new() -> Self {
        Self {
            cube: Mesh::cube(Color::WHITE),
            sphere: Mesh::sphere(Color::WHITE, 16),
            small_sphere: Mesh::sphere(Color::WHITE, 8),
            cylinder: unit_cylinder(14),
            cone: unit_cone(14),
            pyramid: Mesh::pyramid(Color::WHITE),
        }
    }
}

/// A cylinder 1 across and 1 tall. Its caps get the color white and its
/// sides gray, so a builder can paint them differently.
fn unit_cylinder(segments: usize) -> Mesh {
    let mut m = Mesh::new();
    let top = m.add_vertex(vec3(0.0, 0.5, 0.0));
    let bottom = m.add_vertex(vec3(0.0, -0.5, 0.0));
    let ring = |m: &mut Mesh, y: f32| -> Vec<usize> {
        (0..segments)
            .map(|i| {
                let a = i as f32 * TAU / segments as f32;
                m.add_vertex(vec3(0.5 * a.cos(), y, 0.5 * a.sin()))
            })
            .collect()
    };
    let up = ring(&mut m, 0.5);
    let down = ring(&mut m, -0.5);
    for i in 0..segments {
        let j = (i + 1) % segments;
        m.add_triangle(top, up[i], up[j], Color::WHITE);
        m.add_triangle(bottom, down[i], down[j], Color::WHITE);
        m.add_triangle(up[i], down[i], down[j], Color::GRAY);
        m.add_triangle(up[i], down[j], up[j], Color::GRAY);
    }
    face_outward(m)
}

/// A cone 1 across and 1 tall, point up.
fn unit_cone(segments: usize) -> Mesh {
    let mut m = Mesh::new();
    let tip = m.add_vertex(vec3(0.0, 0.5, 0.0));
    let bottom = m.add_vertex(vec3(0.0, -0.5, 0.0));
    let ring: Vec<usize> = (0..segments)
        .map(|i| {
            let a = i as f32 * TAU / segments as f32;
            m.add_vertex(vec3(0.5 * a.cos(), -0.5, 0.5 * a.sin()))
        })
        .collect();
    for i in 0..segments {
        let j = (i + 1) % segments;
        m.add_triangle(tip, ring[i], ring[j], Color::GRAY);
        m.add_triangle(bottom, ring[i], ring[j], Color::WHITE);
    }
    face_outward(m)
}

/// For a convex shape: turns every triangle so its front faces away from
/// the middle. (The renderer only draws fronts, so this saves working out
/// the clockwise order of every triangle by hand.)
fn face_outward(mut m: Mesh) -> Mesh {
    let center =
        m.vertices.iter().fold(Vec3::ZERO, |a, &b| a + b) * (1.0 / m.vertices.len() as f32);
    for tri in &mut m.triangles {
        let [a, b, c] = tri.corners.map(|i| m.vertices[i]);
        let normal = (b - a).cross(c - a);
        let middle = (a + b + c) * (1.0 / 3.0);
        if normal.dot(middle - center) < 0.0 {
            tri.corners.swap(1, 2);
        }
    }
    m
}

/// Builds one mesh out of many transformed parts.
struct Builder<'a> {
    parts: &'a Parts,
    mesh: Mesh,
}

impl<'a> Builder<'a> {
    fn new(parts: &'a Parts) -> Self {
        Self {
            parts,
            mesh: Mesh::new(),
        }
    }

    fn add(&mut self, part: &Mesh, tf: Transform, color: impl Fn(Color) -> Color) -> &mut Self {
        let base = self.mesh.vertices.len();
        self.mesh
            .vertices
            .extend(part.vertices.iter().map(|&v| tf.apply(v)));
        self.mesh
            .triangles
            .extend(part.triangles.iter().map(|t| Triangle {
                corners: t.corners.map(|i| i + base),
                color: color(t.color),
            }));
        self
    }

    fn cube(&mut self, center: Vec3, size: Vec3, color: Color) -> &mut Self {
        self.add(
            &self.parts.cube.clone(),
            Transform::at(center).scaled(size),
            |_| color,
        )
    }

    fn cube_turned(&mut self, center: Vec3, size: Vec3, rotation: Vec3, color: Color) -> &mut Self {
        self.add(
            &self.parts.cube.clone(),
            Transform::at(center).scaled(size).rotated(rotation),
            |_| color,
        )
    }

    fn ball(&mut self, center: Vec3, radius: f32, color: Color) -> &mut Self {
        self.blob(center, Vec3::ONE * radius, color)
    }

    /// A stretched sphere: `radii` along x, y and z.
    fn blob(&mut self, center: Vec3, radii: Vec3, color: Color) -> &mut Self {
        let part = if radii.x.max(radii.y).max(radii.z) < 0.08 {
            self.parts.small_sphere.clone()
        } else {
            self.parts.sphere.clone()
        };
        self.add(&part, Transform::at(center).scaled(radii * 2.0), |_| color)
    }

    fn blob_turned(
        &mut self,
        center: Vec3,
        radii: Vec3,
        rotation: Vec3,
        color: Color,
    ) -> &mut Self {
        let part = self.parts.sphere.clone();
        self.add(
            &part,
            Transform::at(center).scaled(radii * 2.0).rotated(rotation),
            |_| color,
        )
    }

    /// An upright cylinder (rotate it with `rotation`), sides and caps in their own colors.
    fn cylinder(
        &mut self,
        center: Vec3,
        radius: f32,
        height: f32,
        rotation: Vec3,
        side: Color,
        cap: Color,
    ) -> &mut Self {
        let part = self.parts.cylinder.clone();
        let tf = Transform::at(center)
            .scaled(vec3(radius * 2.0, height, radius * 2.0))
            .rotated(rotation);
        self.add(&part, tf, |c| if c == Color::WHITE { cap } else { side })
    }

    fn cone(
        &mut self,
        center: Vec3,
        radius: f32,
        height: f32,
        rotation: Vec3,
        color: Color,
    ) -> &mut Self {
        let part = self.parts.cone.clone();
        let tf = Transform::at(center)
            .scaled(vec3(radius * 2.0, height, radius * 2.0))
            .rotated(rotation);
        self.add(&part, tf, |_| color)
    }

    fn pyramid(&mut self, center: Vec3, size: Vec3, rotation: Vec3, color: Color) -> &mut Self {
        let part = self.parts.pyramid.clone();
        self.add(
            &part,
            Transform::at(center).scaled(size).rotated(rotation),
            |_| color,
        )
    }

    fn coin(&mut self, center: Vec3, rotation: Vec3) -> &mut Self {
        self.cylinder(center, 0.1, 0.03, rotation, GOLD_DARK, GOLD)
    }

    fn duck(&mut self, at: Vec3, size: f32, body: Color) -> &mut Self {
        let s = size;
        let p = |x: f32, y: f32, z: f32| at + vec3(x, y, z) * s;
        self.blob(p(0.0, 0.3, 0.0), vec3(0.42, 0.3, 0.32) * s, body)
            .blob(p(0.32, 0.45, 0.0), vec3(0.14, 0.2, 0.16) * s, body)
            .ball(p(-0.25, 0.75, 0.0), 0.22 * s, body)
            .blob(
                p(-0.5, 0.7, 0.0),
                vec3(0.14, 0.05, 0.1) * s,
                Color::from_hex(0xff8a2a),
            )
            .ball(p(-0.36, 0.82, -0.15), 0.04 * s, Color::from_hex(0x1a1a1a))
            .ball(p(-0.36, 0.82, 0.15), 0.04 * s, Color::from_hex(0x1a1a1a))
            .blob(
                p(0.05, 0.38, -0.27),
                vec3(0.22, 0.12, 0.06) * s,
                body.lerp(Color::from_hex(0xd08a10), 0.4),
            )
            .blob(
                p(0.05, 0.38, 0.27),
                vec3(0.22, 0.12, 0.06) * s,
                body.lerp(Color::from_hex(0xd08a10), 0.4),
            )
    }

    fn done(&mut self) -> Mesh {
        std::mem::take(&mut self.mesh)
    }
}

/// The stage: the wall with three doorways, frames, pillars and the floor.
fn build_stage(parts: &Parts) -> Mesh {
    let mut b = Builder::new(parts);
    let half_gap = DOOR_W / 2.0 + 0.02;
    let wall_z = 0.0;
    let thick = 0.2;
    let height = 3.6;
    let left_edge = -6.0;
    let right_edge = 6.0;
    // Wall pieces between and around the doorways, and one long piece above.
    let mut x = left_edge;
    for &dx in &DOOR_X {
        let to = dx - half_gap;
        b.cube(
            vec3((x + to) / 2.0, DOOR_H / 2.0, wall_z + thick / 2.0),
            vec3(to - x, DOOR_H + 0.02, thick),
            WALL,
        );
        x = dx + half_gap;
    }
    b.cube(
        vec3((x + right_edge) / 2.0, DOOR_H / 2.0, wall_z + thick / 2.0),
        vec3(right_edge - x, DOOR_H + 0.02, thick),
        WALL,
    );
    let top = DOOR_H + 0.02;
    b.cube(
        vec3(0.0, (top + height) / 2.0, wall_z + thick / 2.0),
        vec3(right_edge - left_edge, height - top, thick),
        WALL,
    );

    // Pillars and a marquee band.
    for x in [-1.875, -0.625, 0.625, 1.875, -3.2, 3.2] {
        b.cube(vec3(x, 1.45, -0.04), vec3(0.16, 2.9, 0.1), WALL_PILLAR);
    }
    b.cube(vec3(0.0, 2.62, -0.06), vec3(12.0, 0.16, 0.14), WALL_PILLAR);
    b.cube(vec3(0.0, 3.25, -0.06), vec3(12.0, 0.08, 0.14), TRIM);
    b.cube(vec3(0.0, 0.08, -0.05), vec3(12.0, 0.16, 0.12), BASEBOARD);

    // A gold frame around each doorway.
    for &dx in &DOOR_X {
        let f = 0.09;
        b.cube(
            vec3(dx - half_gap - f / 2.0, DOOR_H / 2.0 + 0.04, -0.06),
            vec3(f, DOOR_H + 0.1, 0.14),
            TRIM,
        );
        b.cube(
            vec3(dx + half_gap + f / 2.0, DOOR_H / 2.0 + 0.04, -0.06),
            vec3(f, DOOR_H + 0.1, 0.14),
            TRIM,
        );
        b.cube(
            vec3(dx, DOOR_H + 0.07, -0.06),
            vec3(DOOR_W + 0.04 + 2.0 * f, f, 0.14),
            TRIM,
        );
        // A little plaque for the number.
        b.cube(
            vec3(dx, DOOR_H + 0.3, -0.05),
            vec3(0.36, 0.3, 0.06),
            Color::from_hex(0x2a1a55),
        );
        b.cube(vec3(dx, DOOR_H + 0.3, -0.03), vec3(0.4, 0.34, 0.04), TRIM);
    }

    let mut stage = b.done();
    // The floor: a big checkerboard in front of the wall.
    let floor = Mesh::checkerboard(24, 0.5, FLOOR_A, FLOOR_B);
    let mut b = Builder::new(parts);
    b.add(&floor, Transform::at(vec3(0.0, 0.0, -6.0)), |c| c);
    let floor = b.done();
    let base = stage.vertices.len();
    stage.vertices.extend(floor.vertices);
    stage
        .triangles
        .extend(floor.triangles.into_iter().map(|t| Triangle {
            corners: t.corners.map(|i| i + base),
            color: t.color,
        }));
    stage
}

/// The little room behind a doorway, lit in the color of what's inside.
fn build_room(parts: &Parts, glow: Color) -> Mesh {
    let mut b = Builder::new(parts);
    let w = DOOR_W + 0.04;
    let back = ROOM.lerp(glow, 0.55);
    let sides = ROOM.lerp(glow, 0.3);
    b.cube(
        vec3(0.0, DOOR_H / 2.0, ROOM_DEPTH + 0.05),
        vec3(w + 0.2, DOOR_H + 0.2, 0.1),
        back,
    )
    .cube(
        vec3(-w / 2.0 - 0.05, DOOR_H / 2.0, ROOM_DEPTH / 2.0 + 0.1),
        vec3(0.1, DOOR_H + 0.2, ROOM_DEPTH),
        sides,
    )
    .cube(
        vec3(w / 2.0 + 0.05, DOOR_H / 2.0, ROOM_DEPTH / 2.0 + 0.1),
        vec3(0.1, DOOR_H + 0.2, ROOM_DEPTH),
        sides,
    )
    .cube(
        vec3(0.0, DOOR_H + 0.05, ROOM_DEPTH / 2.0 + 0.1),
        vec3(w + 0.2, 0.1, ROOM_DEPTH),
        ROOM,
    )
    .cube(
        vec3(0.0, -0.04, ROOM_DEPTH / 2.0 + 0.1),
        vec3(w + 0.2, 0.1, ROOM_DEPTH),
        ROOM.lerp(glow, 0.2),
    )
    // A light strip on the ceiling and rays on the back wall.
    .cube(
        vec3(0.0, DOOR_H - 0.02, PRIZE_Z),
        vec3(0.5, 0.04, 0.1),
        Color::WHITE.lerp(glow, 0.3),
    );
    for k in 0..5 {
        let a = -0.9 + k as f32 * 0.45;
        b.cube_turned(
            vec3(0.0, 1.2, ROOM_DEPTH - 0.01),
            vec3(0.07, 2.4, 0.02),
            vec3(0.0, 0.0, a),
            back.lerp(Color::WHITE, 0.25),
        );
    }
    // The pedestal.
    b.cylinder(
        vec3(0.0, PEDESTAL_TOP / 2.0, PRIZE_Z),
        0.3,
        PEDESTAL_TOP,
        Vec3::ZERO,
        Color::from_hex(0xc9c2e0),
        Color::from_hex(0xf2eefc),
    )
    .cylinder(
        vec3(0.0, PEDESTAL_TOP + 0.01, PRIZE_Z),
        0.33,
        0.04,
        Vec3::ZERO,
        TRIM,
        GOLD,
    );
    b.done()
}

/// A door, hinged on its left edge at the origin, swinging around y.
fn build_door(parts: &Parts, color: Color) -> Mesh {
    let mut b = Builder::new(parts);
    let w = DOOR_W;
    let dark = color.lerp(Color::BLACK, 0.25);
    b.cube(
        vec3(w / 2.0, DOOR_H / 2.0, 0.0),
        vec3(w, DOOR_H, 0.08),
        color,
    );
    for (y, h) in [(1.42, 0.7), (0.55, 0.8)] {
        b.cube(vec3(w / 2.0, y, -0.045), vec3(w - 0.24, h, 0.02), dark)
            .cube(
                vec3(w / 2.0, y, -0.06),
                vec3(w - 0.34, h - 0.1, 0.02),
                color.lerp(Color::WHITE, 0.12),
            );
    }
    b.ball(vec3(w - 0.13, 0.98, -0.1), 0.06, GOLD).cube(
        vec3(w - 0.13, 0.98, -0.06),
        vec3(0.06, 0.18, 0.03),
        GOLD_DARK,
    );
    b.done()
}

fn build_prize(parts: &Parts, item: Item) -> Mesh {
    let mut b = Builder::new(parts);
    let flat = Vec3::ZERO;
    match item {
        Item::Coins => {
            let mut rng = Rng::new(3);
            for layer in 0..4 {
                let count = [9, 6, 3, 1][layer];
                let radius = [0.22, 0.15, 0.08, 0.0][layer];
                for i in 0..count {
                    let a = i as f32 * TAU / count as f32 + layer as f32;
                    let pos = vec3(
                        a.cos() * radius,
                        0.02 + layer as f32 * 0.04,
                        a.sin() * radius,
                    );
                    let tilt = vec3(rng.range(-0.3, 0.3), 0.0, rng.range(-0.3, 0.3));
                    b.coin(pos, tilt);
                }
            }
            for i in 0..4 {
                let a = i as f32 * 1.7;
                b.coin(
                    vec3(a.cos() * 0.28, 0.12, a.sin() * 0.18),
                    vec3(1.2, a, 0.0),
                );
            }
            b.blob(vec3(0.0, 0.17, 0.0), vec3(0.12, 0.09, 0.12), GOLD);
        }
        Item::Treasure => {
            let wood = Color::from_hex(0xa0602a);
            b.cube(vec3(0.0, 0.17, 0.0), vec3(0.62, 0.34, 0.4), wood)
                .cube(vec3(0.0, 0.17, 0.0), vec3(0.64, 0.05, 0.42), GOLD_DARK)
                .cube(vec3(-0.2, 0.17, 0.0), vec3(0.06, 0.36, 0.42), GOLD_DARK)
                .cube(vec3(0.2, 0.17, 0.0), vec3(0.06, 0.36, 0.42), GOLD_DARK)
                .cube(vec3(0.0, 0.2, -0.21), vec3(0.1, 0.12, 0.03), GOLD)
                // The lid, flipped open behind.
                .cube_turned(
                    vec3(0.0, 0.48, 0.25),
                    vec3(0.62, 0.36, 0.06),
                    vec3(-0.35, 0.0, 0.0),
                    wood,
                )
                .cube_turned(
                    vec3(0.0, 0.48, 0.23),
                    vec3(0.64, 0.05, 0.08),
                    vec3(-0.35, 0.0, 0.0),
                    GOLD_DARK,
                );
            // Gold heaped inside.
            b.blob(vec3(0.0, 0.34, 0.0), vec3(0.28, 0.1, 0.17), GOLD);
            let mut rng = Rng::new(9);
            for _ in 0..12 {
                let pos = vec3(
                    rng.range(-0.25, 0.25),
                    rng.range(0.36, 0.46),
                    rng.range(-0.14, 0.12),
                );
                b.coin(pos, vec3(rng.range(-0.6, 0.6), 0.0, rng.range(-0.6, 0.6)));
            }
            b.ball(vec3(0.12, 0.47, -0.05), 0.06, Color::from_hex(0x5ae0ff))
                .ball(vec3(-0.13, 0.45, -0.02), 0.05, Color::from_hex(0xff4f8a));
        }
        Item::Pike => {
            let body = Color::from_hex(0x6f9a52);
            let belly = Color::from_hex(0xd8e6a2);
            b.blob(vec3(0.0, 0.32, 0.0), vec3(0.4, 0.15, 0.13), body)
                .blob(vec3(0.02, 0.27, 0.0), vec3(0.34, 0.09, 0.115), belly)
                .blob(vec3(-0.33, 0.33, 0.0), vec3(0.16, 0.08, 0.09), body)
                .pyramid(
                    vec3(0.45, 0.32, 0.0),
                    vec3(0.22, 0.25, 0.04),
                    vec3(0.0, 0.0, -PI / 2.0),
                    body.lerp(Color::BLACK, 0.2),
                )
                .pyramid(
                    vec3(0.02, 0.48, 0.0),
                    vec3(0.18, 0.1, 0.03),
                    flat,
                    body.lerp(Color::BLACK, 0.2),
                )
                .ball(vec3(-0.3, 0.38, -0.08), 0.035, Color::from_hex(0xfff07a))
                .ball(vec3(-0.31, 0.38, -0.105), 0.018, Color::BLACK)
                // The lower jaw hangs open, full of teeth.
                .blob_turned(
                    vec3(-0.35, 0.24, 0.0),
                    vec3(0.15, 0.04, 0.08),
                    vec3(0.0, 0.0, -0.35),
                    body,
                )
                .blob(
                    vec3(-0.43, 0.29, 0.0),
                    vec3(0.06, 0.04, 0.06),
                    Color::from_hex(0x7a1d2a),
                );
            for k in 0..4 {
                let x = -0.46 + k as f32 * 0.05;
                b.pyramid(
                    vec3(x, 0.29, -0.06),
                    vec3(0.025, 0.05, 0.025),
                    vec3(PI, 0.0, 0.0),
                    Color::WHITE,
                );
                b.pyramid(
                    vec3(x, 0.27, -0.06),
                    vec3(0.025, 0.05, 0.025),
                    flat,
                    Color::WHITE,
                );
            }
            for k in 0..5 {
                b.ball(
                    vec3(
                        -0.15 + k as f32 * 0.09,
                        0.36 + (k % 2) as f32 * 0.04,
                        -0.125,
                    ),
                    0.018,
                    belly,
                );
            }
        }
        Item::RubberDuck => {
            b.duck(vec3(0.0, 0.0, 0.0), 0.75, Color::from_hex(0xffd84a));
        }
        Item::Ghost => {
            let white = Color::from_hex(0xf4f6ff);
            b.ball(vec3(0.0, 0.62, 0.0), 0.25, white).cylinder(
                vec3(0.0, 0.42, 0.0),
                0.25,
                0.4,
                flat,
                white,
                white,
            );
            for k in 0..6 {
                let a = k as f32 * TAU / 6.0;
                b.ball(vec3(a.cos() * 0.19, 0.22, a.sin() * 0.19), 0.08, white);
            }
            b.blob(
                vec3(-0.09, 0.66, -0.22),
                vec3(0.045, 0.07, 0.03),
                Color::BLACK,
            )
            .blob(
                vec3(0.09, 0.66, -0.22),
                vec3(0.045, 0.07, 0.03),
                Color::BLACK,
            )
            .blob(vec3(0.0, 0.52, -0.23), vec3(0.05, 0.06, 0.03), Color::BLACK)
            .blob_turned(
                vec3(-0.3, 0.45, 0.0),
                vec3(0.12, 0.05, 0.05),
                vec3(0.0, 0.0, 0.6),
                white,
            )
            .blob_turned(
                vec3(0.3, 0.45, 0.0),
                vec3(0.12, 0.05, 0.05),
                vec3(0.0, 0.0, -0.6),
                white,
            );
        }
        Item::IceCream => {
            b.cone(
                vec3(0.0, 0.25, 0.0),
                0.14,
                0.5,
                vec3(PI, 0.0, 0.0),
                Color::from_hex(0xe0a865),
            )
            .ball(vec3(0.0, 0.55, 0.0), 0.15, Color::from_hex(0xff9ccb))
            .ball(vec3(0.0, 0.76, 0.0), 0.13, Color::from_hex(0xa7f0d2))
            .ball(vec3(0.0, 0.93, 0.0), 0.05, Color::from_hex(0xff2f4a));
            for k in 0..6 {
                let a = k as f32 * TAU / 6.0;
                b.blob(
                    vec3(a.cos() * 0.14, 0.47, a.sin() * 0.14),
                    vec3(0.05, 0.06, 0.05),
                    Color::from_hex(0xff9ccb),
                );
            }
        }
        Item::Cake => {
            let sponge = Color::from_hex(0xb06a3a);
            let icing = Color::from_hex(0xffb0d2);
            b.cylinder(
                vec3(0.0, 0.02, 0.0),
                0.36,
                0.03,
                flat,
                Color::from_hex(0xd8dbe8),
                Color::WHITE,
            )
            .cylinder(vec3(0.0, 0.14, 0.0), 0.3, 0.2, flat, sponge, icing)
            .cylinder(vec3(0.0, 0.25, 0.0), 0.31, 0.04, flat, icing, icing)
            .cylinder(vec3(0.0, 0.36, 0.0), 0.2, 0.18, flat, sponge, icing)
            .cylinder(vec3(0.0, 0.45, 0.0), 0.21, 0.04, flat, icing, icing)
            .cylinder(
                vec3(0.0, 0.54, 0.0),
                0.025,
                0.14,
                flat,
                Color::from_hex(0x7ec8ff),
                Color::WHITE,
            )
            .ball(vec3(0.0, 0.64, 0.0), 0.04, Color::from_hex(0xffb22a))
            .ball(vec3(0.0, 0.66, -0.01), 0.022, Color::from_hex(0xfff3a0));
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                b.ball(vec3(a.cos() * 0.3, 0.25, a.sin() * 0.3), 0.035, icing);
                if k % 2 == 0 {
                    b.ball(
                        vec3(a.cos() * 0.15, 0.48, a.sin() * 0.15),
                        0.035,
                        Color::from_hex(0xff2f4a),
                    );
                }
            }
        }
        Item::Rocket => {
            let red = Color::from_hex(0xff4040);
            b.cylinder(
                vec3(0.0, 0.5, 0.0),
                0.13,
                0.5,
                flat,
                Color::from_hex(0xf2f4f8),
                Color::WHITE,
            )
            .cone(vec3(0.0, 0.88, 0.0), 0.13, 0.26, flat, red)
            .cylinder(
                vec3(0.0, 0.22, 0.0),
                0.09,
                0.08,
                flat,
                Color::from_hex(0x7a8090),
                Color::from_hex(0x7a8090),
            )
            .ball(vec3(0.0, 0.58, -0.11), 0.06, Color::from_hex(0x5a6a7a))
            .ball(vec3(0.0, 0.58, -0.125), 0.045, Color::from_hex(0x6fd0ff));
            for k in 0..3 {
                let a = k as f32 * TAU / 3.0 + PI / 2.0;
                b.cube_turned(
                    vec3(a.cos() * 0.15, 0.32, a.sin() * 0.15),
                    vec3(0.14, 0.2, 0.025),
                    vec3(0.0, -a, 0.0),
                    red,
                );
            }
        }
        Item::Rainbow => {
            let colors = [0xff3f4a, 0xff9a2a, 0xffe03a, 0x46d16b, 0x3d9bff, 0x8a5cff];
            for (k, &hex) in colors.iter().enumerate() {
                let radius = 0.42 - k as f32 * 0.055;
                for step in 0..=20 {
                    let a = step as f32 * PI / 20.0;
                    b.cube_turned(
                        vec3(a.cos() * radius, 0.05 + a.sin() * radius, 0.0),
                        vec3(0.06, 0.06, 0.08),
                        vec3(0.0, 0.0, a),
                        Color::from_hex(hex),
                    );
                }
            }
            for x in [-0.36, 0.36] {
                b.ball(vec3(x - 0.08, 0.07, -0.02), 0.09, Color::WHITE)
                    .ball(vec3(x + 0.07, 0.07, -0.02), 0.1, Color::WHITE)
                    .ball(vec3(x, 0.14, -0.04), 0.1, Color::WHITE);
            }
        }
        Item::Bomb => {
            b.ball(vec3(0.0, 0.28, 0.0), 0.27, Color::from_hex(0x3a3a48))
                .ball(vec3(-0.1, 0.38, -0.2), 0.06, Color::from_hex(0x6a6a80))
                .cylinder(
                    vec3(0.0, 0.57, 0.0),
                    0.08,
                    0.08,
                    flat,
                    Color::from_hex(0x8a8aa0),
                    Color::from_hex(0x6a6a80),
                )
                .cylinder(
                    vec3(0.05, 0.67, 0.0),
                    0.015,
                    0.16,
                    vec3(0.0, 0.0, -0.5),
                    Color::from_hex(0xd9b36a),
                    Color::from_hex(0xd9b36a),
                );
        }
        Item::Ducklings => {
            for (k, x) in [-0.3, 0.0, 0.3].into_iter().enumerate() {
                let z = if k == 1 { -0.08 } else { 0.05 };
                b.duck(vec3(x, 0.0, z), 0.42, Color::from_hex(0xffe06a));
            }
        }
        Item::Storm => {
            let cloud = Color::from_hex(0x8a92a8);
            for &(x, y, r) in &[
                (-0.2, 0.9, 0.17),
                (0.0, 0.98, 0.21),
                (0.2, 0.9, 0.16),
                (0.08, 0.84, 0.17),
                (-0.1, 0.84, 0.15),
            ] {
                b.ball(vec3(x, y, 0.0), r, cloud);
            }
        }
    }
    b.done()
}

// ---- The show ----

struct Coin {
    body: duckforge::physics::BodyId,
    spin: f32,
    tilt: f32,
}

struct Show {
    t: f32,
    parts: Parts,
    stage: Mesh,
    rooms: [Mesh; 3],
    doors: [Mesh; 3],
    prizes: Vec<Mesh>,
    bulbs: [Mesh; 2],
    bolt: Mesh,
    flame: Mesh,
    coin: Mesh,
    shadow: Mesh,
    world: PhysicsWorld<Vec3>,
    coins: Vec<Coin>,
    rng: Rng,
}

impl Show {
    fn new() -> Self {
        let parts = Parts::new();
        let stage = build_stage(&parts);
        let rooms = [WIN_COLOR, OK_COLOR, BAD_COLOR].map(|c| build_room(&parts, c));
        let doors = DOOR_COLORS.map(|c| build_door(&parts, c));
        let prizes = ITEMS.iter().map(|&i| build_prize(&parts, i)).collect();
        let bulbs = [0, 1].map(|odd| {
            let mut b = Builder::new(&parts);
            for k in 0..40 {
                if k % 2 == odd {
                    let x = -4.875 + k as f32 * 0.25;
                    b.ball(vec3(x, 3.0, -0.12), 0.055, Color::WHITE);
                }
            }
            b.done()
        });
        let bolt = {
            let mut b = Builder::new(&parts);
            b.cube_turned(
                vec3(0.02, 0.62, 0.0),
                vec3(0.06, 0.26, 0.04),
                vec3(0.0, 0.0, -0.5),
                Color::WHITE,
            )
            .cube_turned(
                vec3(0.0, 0.47, 0.0),
                vec3(0.14, 0.05, 0.04),
                vec3(0.0, 0.0, 0.0),
                Color::WHITE,
            )
            .cube_turned(
                vec3(-0.02, 0.32, 0.0),
                vec3(0.06, 0.26, 0.04),
                vec3(0.0, 0.0, -0.5),
                Color::WHITE,
            );
            b.done()
        };
        let flame = {
            let mut b = Builder::new(&parts);
            b.cone(
                vec3(0.0, 0.0, 0.0),
                0.08,
                0.25,
                vec3(PI, 0.0, 0.0),
                Color::from_hex(0xff8a2a),
            )
            .cone(
                vec3(0.0, 0.03, -0.01),
                0.05,
                0.16,
                vec3(PI, 0.0, 0.0),
                Color::from_hex(0xfff07a),
            );
            b.done()
        };
        let coin = {
            let mut b = Builder::new(&parts);
            b.coin(Vec3::ZERO, Vec3::ZERO);
            b.done()
        };
        let shadow = {
            let mut b = Builder::new(&parts);
            b.cylinder(
                Vec3::ZERO,
                0.5,
                0.002,
                Vec3::ZERO,
                Color::from_hex(0x1a1238),
                Color::from_hex(0x1a1238),
            );
            b.done()
        };
        let mut world = PhysicsWorld::new(vec3(0.0, -9.0, 0.0));
        world.add(
            Body::block(vec3(0.0, -0.5, -4.0), vec3(20.0, 1.0, 20.0))
                .fixed()
                .with_friction(0.6),
        );
        // The wall, so coins bounce back toward the camera instead of into it.
        world.add(Body::block(vec3(0.0, 1.0, 0.6), vec3(20.0, 4.0, 0.2)).fixed());
        Self {
            t: 0.0,
            parts,
            stage,
            rooms,
            doors,
            prizes,
            bulbs,
            bolt,
            flame,
            coin,
            shadow,
            world,
            coins: Vec::new(),
            rng: Rng::new(42),
        }
    }

    fn prize(&self, item: Item) -> &Mesh {
        let i = ITEMS.iter().position(|&x| x == item).unwrap_or(0);
        &self.prizes[i]
    }

    /// Gold coins bursting out of door `d` toward the camera.
    fn burst(&mut self, d: usize, count: usize) {
        for _ in 0..count {
            let pos = vec3(
                DOOR_X[d] + self.rng.range(-0.25, 0.25),
                self.rng.range(0.6, 1.2),
                -0.15,
            );
            let vel = vec3(
                self.rng.range(-1.8, 1.8),
                self.rng.range(2.5, 4.6),
                self.rng.range(-2.2, -0.7),
            );
            let id = self.world.add(
                Body::ball(pos, 0.05)
                    .with_velocity(vel)
                    .with_bounce(0.45)
                    .with_friction(0.5),
            );
            self.coins.push(Coin {
                body: id,
                spin: self.rng.range(6.0, 14.0),
                tilt: self.rng.range(0.0, TAU),
            });
        }
    }

    fn clear_coins(&mut self) {
        for coin in self.coins.drain(..) {
            self.world.remove(coin.body);
        }
    }
}

impl Game for Show {
    fn update(&mut self, ctx: &mut Context) {
        let before = self.t;
        self.t += ctx.dt();
        let now = self.t;
        let crossed = |at: f32| before < at && now >= at;
        for (i, round) in ROUNDS.iter().enumerate() {
            let b = INTRO + i as f32 * ROUND_LEN;
            if crossed(b) {
                self.clear_coins();
            }
            for (d, door) in round.doors.iter().enumerate() {
                if door.1 == Rating::Win && crossed(b + OPEN_AT[d] + 0.3) {
                    let count = if i == 3 { 55 } else { 28 };
                    self.burst(d, count);
                }
            }
        }
        if crossed(END_START) {
            self.clear_coins();
        }
        self.world.step(ctx.dt());
        if self.t > TOTAL + 1.0 || ctx.input.was_pressed(Key::Escape) {
            ctx.quit();
        }
    }

    fn draw(&self, canvas: &mut Canvas) {
        let k = canvas.width() as f32 / BASE_W;
        let t = self.t;
        draw_sky(canvas, t, k);

        let (round, rt) = if (INTRO..END_START).contains(&t) {
            let i = ((t - INTRO) / ROUND_LEN) as usize;
            (Some(i), t - INTRO - i as f32 * ROUND_LEN)
        } else {
            (None, 0.0)
        };
        let camera = camera_at(t, round, rt);
        self.draw_world(canvas, &camera, round, rt);

        match round {
            Some(i) => draw_round_overlay(canvas, &camera, i, rt, t, k),
            None if t < INTRO => draw_intro_overlay(canvas, t, k),
            None => draw_end_overlay(canvas, t - END_START, t, k),
        }

        // A wipe between rounds: stripes sweep in at the end of a round and out at the start of the next.
        if round.is_some() {
            if rt > WHIP_AT {
                wipe(canvas, (rt - WHIP_AT) / (ROUND_LEN - WHIP_AT), true, k);
            } else if rt < 0.35 && t > INTRO + 0.1 {
                wipe(canvas, rt / 0.35, false, k);
            }
        } else if t > INTRO - 0.45 && t < INTRO {
            wipe(canvas, (t - INTRO + 0.45) / 0.45, true, k);
        } else if (END_START..END_START + 0.35).contains(&t) {
            wipe(canvas, (t - END_START) / 0.35, false, k);
        }
    }
}

impl Show {
    fn draw_world(&self, canvas: &mut Canvas, cam: &Camera3D, round: Option<usize>, rt: f32) {
        let t = self.t;
        canvas.draw_mesh(&self.stage, &Transform::default(), cam);

        // The marquee bulbs chase along the top of the wall.
        let phase = ((t * 4.0) as usize) % 2;
        let on = Color::from_hex(0xffe14a);
        let off = Color::from_hex(0x8a5a1a);
        for (i, bulbs) in self.bulbs.iter().enumerate() {
            let color = if i == phase { on } else { off };
            canvas.draw_mesh(&bulbs.clone().with_color(color), &Transform::default(), cam);
        }

        // What each door hides this round (at the end, every door shows its last prize).
        let doors: Option<&[(Item, Rating, &str); 3]> = match round {
            Some(i) => Some(&ROUNDS[i].doors),
            None if t >= END_START => Some(&ROUNDS[3].doors),
            None => None,
        };
        for d in 0..3 {
            let open = match (round, doors) {
                (Some(_), _) => ease_in_out((rt - OPEN_AT[d]) / OPEN_TIME),
                (None, Some(_)) => 1.0,
                _ => 0.0,
            };
            let door_pos = vec3(DOOR_X[d] - DOOR_W / 2.0, 0.0, 0.0);
            if let Some(doors) = doors {
                let (item, rating, _) = doors[d];
                if open > 0.0 {
                    let room = &self.rooms[rating.index()];
                    canvas.draw_mesh(room, &Transform::at(vec3(DOOR_X[d], 0.0, 0.0)), cam);
                    let since = if round.is_some() {
                        rt - OPEN_AT[d]
                    } else {
                        5.0
                    };
                    let s = ease_out_back((since - 0.15) / 0.45).max(0.0);
                    if s > 0.0 {
                        self.draw_prize(canvas, cam, item, d, s, t);
                    }
                }
            }
            // A door shakes a little just before it opens.
            let mut shake = 0.0;
            if round.is_some() {
                let before = OPEN_AT[d] - rt;
                if (0.0..0.3).contains(&before) || (COUNT_END - 0.8..COUNT_END).contains(&rt) {
                    shake = (t * 70.0 + d as f32).sin() * 0.02;
                }
            }
            let tf = Transform::at(door_pos + vec3(0.0, 0.0, -0.01)).rotated(vec3(
                0.0,
                -open * 1.95 + shake,
                0.0,
            ));
            canvas.draw_mesh(&self.doors[d], &tf, cam);
        }

        // Flying coins, each with a soft shadow under it.
        for coin in &self.coins {
            if let Some(body) = self.world.get(coin.body) {
                let p = body.position;
                let shadow = Transform::at(vec3(p.x, 0.003, p.z)).scaled(vec3(0.12, 1.0, 0.12));
                canvas.draw_mesh(&self.shadow, &shadow, cam);
                let rotation = vec3(coin.tilt + t * coin.spin, t * coin.spin * 0.5, 0.4);
                canvas.draw_mesh(
                    &self.coin,
                    &Transform::at(p).rotated(rotation).sized(0.7),
                    cam,
                );
            }
        }
    }

    fn draw_prize(
        &self,
        canvas: &mut Canvas,
        cam: &Camera3D,
        item: Item,
        d: usize,
        s: f32,
        t: f32,
    ) {
        let base = vec3(DOOR_X[d], PEDESTAL_TOP + 0.03, PRIZE_Z);
        // Each prize turns gently on its pedestal, and some move on their own.
        let mut turn = (t * 0.9 + d as f32).sin() * 0.45;
        let mut lift = 0.0;
        let mut squash = 1.0;
        match item {
            Item::Ghost | Item::RubberDuck | Item::Ducklings => {
                lift = ((t * 3.0).sin() * 0.5 + 0.5) * 0.06
            }
            Item::Rocket => lift = ((t * 3.0).sin() * 0.5 + 0.5) * 0.05,
            Item::Pike => {
                turn = -0.35 + (t * 8.0).sin() * 0.12;
            }
            Item::Bomb => squash = 1.0 + (t * 9.0).sin() * 0.04,
            Item::Coins | Item::Treasure => turn = t * 0.8,
            _ => {}
        }
        let tf = Transform::at(base + vec3(0.0, lift, 0.0))
            .rotated(vec3(0.0, turn, 0.0))
            .scaled(vec3(s, s * squash, s));
        canvas.draw_mesh(self.prize(item), &tf, cam);

        if item == Item::Rocket {
            let flicker = 0.85 + (t * 30.0).sin() * 0.2;
            let flame =
                Transform::at(base + vec3(0.0, lift + 0.08, 0.0)).scaled(vec3(s, s * flicker, s));
            canvas.draw_mesh(&self.flame, &flame, cam);
        }
        if item == Item::Storm && (t * 6.0).sin() > 0.1 {
            let bolt = Transform::at(base).sized(s).rotated(vec3(0.0, 0.0, 0.0));
            canvas.draw_mesh(
                &self.bolt.clone().with_color(Color::from_hex(0xfff27a)),
                &bolt,
                cam,
            );
        }
        let _ = &self.parts;
    }
}

// ---- The camera ----

fn camera_at(t: f32, round: Option<usize>, rt: f32) -> Camera3D {
    let front = vec3(0.0, 1.45, -5.4);
    let look = vec3(0.0, 1.25, 0.0);
    let (pos, target) = if t < INTRO {
        // Swoop down from high above the stage.
        let k = ease_in_out(t / 2.2);
        (
            vec3(0.0, 7.0, -10.0).lerp_to(front, k),
            vec3(0.0, 0.0, 0.0).lerp_to(look, k),
        )
    } else if let Some(i) = round {
        let side = if i % 2 == 0 { 1.0 } else { -1.0 };
        // Arrive from the side the last round whipped toward.
        let arrive = ease_out(rt / 1.0);
        let mut pos = vec3(side * 3.0, 2.6, -4.2).lerp_to(front, arrive);
        let mut target = vec3(-side, 1.0, 0.0).lerp_to(look, arrive);
        // A slow push in during the countdown.
        let push = ease_in_out((rt - COUNT_START) / (COUNT_END - COUNT_START));
        pos.z += 0.5 * push;
        pos.x += (rt * 0.7).sin() * 0.15;
        // Lean toward each door as it opens.
        for d in 0..3 {
            let k = bump(rt, OPEN_AT[d] - 0.35, OPEN_AT[d] + 0.9, 0.35);
            pos += vec3(DOOR_X[d] * 0.3, -0.1, 0.55) * k;
            target += vec3(DOOR_X[d] * 0.55, -0.15, 0.0) * k;
        }
        // Push into the winner.
        let winner = ROUNDS[i]
            .doors
            .iter()
            .position(|d| d.1 == Rating::Win)
            .unwrap_or(0);
        let k = ease_in_out((rt - WINNER_AT) / 0.8);
        pos = pos.lerp_to(vec3(DOOR_X[winner] * 0.6, 1.3, -3.6), k);
        target = target.lerp_to(vec3(DOOR_X[winner], 0.95, 0.5), k);
        // Whip away to the side for the next round.
        let whip = ease_in((rt - WHIP_AT) / (ROUND_LEN - WHIP_AT));
        target += vec3(-side * 6.0, 0.0, 0.0) * whip;
        // A jolt when a bad door opens.
        for (d, door) in ROUNDS[i].doors.iter().enumerate() {
            if door.1 == Rating::Bad {
                let since = rt - OPEN_AT[d] - 0.25;
                if (0.0..0.5).contains(&since) {
                    let a = (1.0 - since / 0.5) * 0.06;
                    pos += vec3((rt * 80.0).sin() * a, (rt * 67.0).cos() * a, 0.0);
                }
            }
        }
        (pos, target)
    } else {
        // The end: slowly circle the stage.
        let et = t - END_START;
        let a = -0.5 + et * 0.18;
        (
            vec3(a.sin() * 5.6, 2.2, -a.cos() * 5.6),
            vec3(0.0, 1.0, 0.0),
        )
    };
    let mut cam = Camera3D::looking_at(pos, target);
    cam.fov = 68f32.to_radians();
    cam
}

/// 0 outside `from..to`, rising to 1 over `ramp` seconds at each end.
fn bump(x: f32, from: f32, to: f32, ramp: f32) -> f32 {
    ease_in_out(((x - from) / ramp).min((to - x) / ramp))
}

// ---- 2D, drawn over the 3D ----

fn draw_sky(canvas: &mut Canvas, t: f32, k: f32) {
    let h = canvas.height();
    let w = canvas.width() as f32;
    canvas.clear(SKY_TOP); // also resets the depth buffer for this frame's 3D
    for y in 0..h {
        let f = y as f32 / h as f32;
        canvas.fill_rect(
            Rect::new(0.0, y as f32, w, 1.0),
            SKY_TOP.lerp(SKY_BOTTOM, f),
        );
    }
    let mut rng = Rng::new(99);
    for i in 0..70 {
        let p = vec2(rng.range(0.0, w), rng.range(0.0, h as f32 * 0.5));
        let twinkle = (t * 2.0 + i as f32 * 1.7).sin();
        if twinkle > 0.0 {
            let c = SKY_TOP.lerp(Color::WHITE, 0.3 + twinkle * 0.6);
            canvas.fill_circle(p, k * (0.6 + twinkle * 0.8), c);
        }
    }
}

fn draw_intro_overlay(canvas: &mut Canvas, t: f32, k: f32) {
    let words = [
        ("CHOOSE", Color::WHITE),
        ("YOUR", TEXT_GOLD),
        ("DOOR", Color::WHITE),
    ];
    for (i, (word, color)) in words.iter().enumerate() {
        let p = pop(t - 1.1 - 0.3 * i as f32);
        if p > 0.0 {
            let scale = (13.0 * p * k).round().max(1.0) as u32;
            let y = (90.0 + i as f32 * 92.0) * k + (13.0 * k - scale as f32) * 2.5;
            big_text(canvas, word, y, scale, *color, k);
        }
    }
    if t > 2.1 {
        big_text(
            canvas,
            "4 ROUNDS - KEEP SCORE!",
            800.0 * k,
            (4.0 * k) as u32,
            TEXT_GOLD,
            k,
        );
    }
}

fn draw_round_overlay(canvas: &mut Canvas, cam: &Camera3D, i: usize, rt: f32, t: f32, k: f32) {
    let round = &ROUNDS[i];
    let winner = round
        .doors
        .iter()
        .position(|d| d.1 == Rating::Win)
        .unwrap_or(0);

    // Header.
    let drop = ease_out(rt / 0.5);
    let top = (-60.0 + 120.0 * drop) * k;
    big_text(
        canvas,
        &format!("ROUND {} OF 4", i + 1),
        top,
        (4.0 * k) as u32,
        TEXT_GOLD,
        k,
    );
    let lines = round.question.lines().count() as f32;
    let scale = if lines > 2.0 { 7.0 } else { 9.0 };
    let line_h = 7.0 * scale;
    let q_top = 108.0 + (3.0 - lines) * line_h * 0.25 - (1.0 - drop) * 300.0;
    for (n, line) in round.question.lines().enumerate() {
        big_text(
            canvas,
            line,
            (q_top + n as f32 * line_h) * k,
            (scale * k) as u32,
            Color::WHITE,
            k,
        );
    }

    // Door numbers on their plaques.
    for d in 0..3 {
        if let Some(p) = cam.world_to_screen(vec3(DOOR_X[d], DOOR_H + 0.3, -0.1), canvas) {
            let number = (d + 1).to_string();
            let s = (4.0 * k) as u32;
            let w = Canvas::text_width(&number, s);
            outlined_text(
                canvas,
                &number,
                vec2(p.x - w / 2.0, p.y - 2.5 * s as f32),
                s,
                Color::WHITE,
                k,
            );
        }
    }

    // A badge and a label for each opened door, on the floor in front of it.
    for d in 0..3 {
        let shown = rt - OPEN_AT[d] - 0.25;
        if shown <= 0.0 || rt > WINNER_AT + 0.6 {
            continue;
        }
        let (_, rating, label) = round.doors[d];
        if let Some(p) = cam.world_to_screen(vec3(DOOR_X[d], 0.0, -0.45), canvas) {
            // Doors the camera has turned away from keep their badge off screen.
            if p.x < 20.0 * k || p.x > canvas.width() as f32 - 20.0 * k {
                continue;
            }
            let pk = pop(shown);
            badge(canvas, vec2(p.x, p.y + 18.0 * k), 17.0 * k * pk, rating, k);
            if pk > 0.6 {
                let s = (3.0 * k) as u32;
                let w = Canvas::text_width(label, s);
                outlined_text(
                    canvas,
                    label,
                    vec2(p.x - w / 2.0, p.y + 44.0 * k),
                    s,
                    rating.color(),
                    k,
                );
            }
        }
    }

    // The countdown.
    if (COUNT_START..COUNT_END).contains(&rt) {
        let left = COUNT_END - rt;
        let number = left.ceil() as u32;
        let within = left - left.floor();
        let center = vec2(270.0, 790.0) * k;
        let ring = if number <= 2 { BAD_COLOR } else { TEXT_GOLD };
        let beat = 1.0 + (1.0 - (1.0 - within) * 6.0).max(0.0) * 0.12;
        canvas.fill_circle(center, 66.0 * k * beat, SHADOW);
        canvas.fill_circle(center, 61.0 * k * beat, Color::from_hex(0x2b2155));
        arc(canvas, center, 56.0 * k * beat, within, ring, 9.0 * k);
        let s = (11.0 * k * beat) as u32;
        let text = number.to_string();
        let w = Canvas::text_width(&text, s);
        outlined_text(
            canvas,
            &text,
            vec2(center.x - w / 2.0, center.y - 2.5 * s as f32),
            s,
            Color::WHITE,
            k,
        );
        if (t * 3.0).sin() > -0.6 {
            big_text(
                canvas,
                "PICK A DOOR!",
                880.0 * k,
                (5.0 * k) as u32,
                Color::WHITE,
                k,
            );
        }
    } else if (COUNT_END..OPEN_AT[0]).contains(&rt) {
        let s = (12.0 * pop(rt - COUNT_END) * k).round().max(1.0) as u32;
        big_text(canvas, "OPEN!", 760.0 * k, s, TEXT_GOLD, k);
    }

    // The winner.
    if rt >= WINNER_AT {
        let pw = pop(rt - WINNER_AT);
        let s = (7.0 * pw * k).round().max(1.0) as u32;
        let text = format!("DOOR {} WINS!", winner + 1);
        let panel = Rect::from_center(
            vec2(270.0, 842.0) * k,
            470.0 * k * pw.min(1.0),
            132.0 * k * pw.min(1.0),
        );
        canvas.fill_rect(
            Rect::new(panel.x + 5.0 * k, panel.y + 5.0 * k, panel.w, panel.h),
            SHADOW,
        );
        canvas.fill_rect(panel, Color::from_hex(0x2b2155));
        canvas.draw_rect(panel, TEXT_GOLD);
        big_text(canvas, &text, 800.0 * k, s, TEXT_GOLD, k);
        let (_, _, label) = round.doors[winner];
        if rt > WINNER_AT + 0.3 {
            big_text(canvas, label, 870.0 * k, (5.0 * k) as u32, WIN_COLOR, k);
        }
        confetti(canvas, rt - WINNER_AT, t, k);
    }
}

fn draw_end_overlay(canvas: &mut Canvas, et: f32, t: f32, k: f32) {
    let slide = (1.0 - ease_out(et / 0.4)) * -200.0;
    big_text(
        canvas,
        "HOW DID",
        (70.0 + slide) * k,
        (10.0 * k) as u32,
        Color::WHITE,
        k,
    );
    big_text(
        canvas,
        "YOU DO?",
        (150.0 + slide) * k,
        (10.0 * k) as u32,
        TEXT_GOLD,
        k,
    );
    let rows = [
        ("4 WINS", "LEGEND", WIN_COLOR),
        ("3 WINS", "LUCKY", Color::from_hex(0x8be08f)),
        ("2 WINS", "NOT BAD", OK_COLOR),
        ("0-1 WINS", "UNLUCKY", BAD_COLOR),
    ];
    for (i, (wins, title, color)) in rows.iter().enumerate() {
        let p = pop(et - 0.6 - 0.45 * i as f32);
        if p <= 0.0 {
            continue;
        }
        let y = (560.0 + i as f32 * 66.0) * k;
        let x = (40.0 + (1.0 - p) * 200.0) * k;
        let panel = Rect::new(x, y, (BASE_W - 80.0) * k, 54.0 * k);
        canvas.fill_rect(
            Rect::new(panel.x + 4.0 * k, panel.y + 4.0 * k, panel.w, panel.h),
            SHADOW,
        );
        canvas.fill_rect(panel, Color::from_hex(0x2b2155));
        canvas.fill_rect(Rect::new(panel.x, panel.y, 8.0 * k, panel.h), *color);
        let s = (4.0 * k) as u32;
        outlined_text(
            canvas,
            wins,
            vec2(panel.x + 24.0 * k, y + 17.0 * k),
            s,
            Color::WHITE,
            k,
        );
        let w = Canvas::text_width(title, s);
        outlined_text(
            canvas,
            title,
            vec2(panel.right() - 20.0 * k - w, y + 17.0 * k),
            s,
            *color,
            k,
        );
    }
    if et > 2.6 {
        let s = (6.0 * pop(et - 2.6) * k).round().max(1.0) as u32;
        big_text(canvas, "COMMENT YOUR SCORE!", 860.0 * k, s, TEXT_GOLD, k);
        confetti(canvas, et - 2.6, t, k);
    }
}

fn confetti(canvas: &mut Canvas, since: f32, t: f32, k: f32) {
    let mut rng = Rng::new(5);
    let colors = [
        TEXT_GOLD,
        WIN_COLOR,
        BAD_COLOR,
        Color::from_hex(0x6fb3ff),
        Color::WHITE,
        Color::from_hex(0xff7ad9),
    ];
    let h = canvas.height() as f32;
    for i in 0..90 {
        let x = rng.range(0.0, BASE_W) * k;
        let speed = rng.range(160.0, 320.0) * k;
        let start = rng.range(-h * 0.6, 0.0);
        let y = start + since * speed;
        if y > h {
            continue;
        }
        let wob = (t * 5.0 + i as f32).sin() * 6.0 * k;
        let flip = (t * 9.0 + i as f32 * 0.7).sin().abs();
        let c = colors[i % colors.len()];
        canvas.fill_rect(Rect::new(x + wob, y, 5.0 * k * (0.3 + flip), 7.0 * k), c);
    }
}

fn badge(canvas: &mut Canvas, c: Vec2, r: f32, rating: Rating, k: f32) {
    if r < 1.0 {
        return;
    }
    canvas.fill_circle(c + vec2(2.0, 3.0) * k, r + 3.0 * k, SHADOW);
    canvas.fill_circle(c, r + 2.0 * k, Color::WHITE);
    canvas.fill_circle(c, r, rating.color());
    let s = r / 17.0;
    let w = 3.0 * k * s;
    match rating {
        Rating::Win => {
            thick_line(
                canvas,
                c + vec2(-8.0, 0.0) * (k * s),
                c + vec2(-2.0, 7.0) * (k * s),
                w,
            );
            thick_line(
                canvas,
                c + vec2(-2.0, 7.0) * (k * s),
                c + vec2(9.0, -7.0) * (k * s),
                w,
            );
        }
        Rating::Bad => {
            thick_line(
                canvas,
                c + vec2(-7.0, -7.0) * (k * s),
                c + vec2(7.0, 7.0) * (k * s),
                w,
            );
            thick_line(
                canvas,
                c + vec2(7.0, -7.0) * (k * s),
                c + vec2(-7.0, 7.0) * (k * s),
                w,
            );
        }
        Rating::Ok => thick_line(
            canvas,
            c + vec2(-8.0, 0.0) * (k * s),
            c + vec2(8.0, 0.0) * (k * s),
            w,
        ),
    }
}

/// Diagonal stripes sweeping across the screen. `cover`: sweeping in (true) or out (false).
fn wipe(canvas: &mut Canvas, p: f32, cover: bool, k: f32) {
    let p = ease_in_out(p);
    let w = canvas.width() as f32;
    let h = canvas.height() as f32;
    let colors = [
        Color::from_hex(0xffcf3a),
        Color::from_hex(0xff5a8a),
        Color::from_hex(0x7b4fd6),
    ];
    let stripe = 120.0 * k;
    let slant = h * 0.35;
    let total = w + slant + stripe * 3.0;
    for (n, &color) in colors.iter().enumerate() {
        // Each stripe trails the one before, so they arrive as a wave.
        let q = (p * 1.4 - n as f32 * 0.2).clamp(0.0, 1.0);
        let (from, to) = if cover {
            (-slant - stripe * 3.0, -slant - stripe * 3.0 + total * q)
        } else {
            (-slant - stripe * 3.0 + total * q, w + stripe)
        };
        if to <= from {
            continue;
        }
        let a = vec2(from, h);
        let b = vec2(to, h);
        let c = vec2(to + slant, 0.0);
        let d = vec2(from + slant, 0.0);
        canvas.fill_triangle(a, b, c, color);
        canvas.fill_triangle(a, c, d, color);
    }
}

fn big_text(canvas: &mut Canvas, text: &str, y: f32, scale: u32, color: Color, k: f32) {
    let x = ((canvas.width() as f32 - Canvas::text_width(text, scale)) / 2.0).round();
    outlined_text(canvas, text, vec2(x, y.round()), scale, color, k);
}

/// Text with a thick dark outline and a drop shadow, so it reads on anything.
fn outlined_text(canvas: &mut Canvas, text: &str, pos: Vec2, scale: u32, color: Color, k: f32) {
    let o = (scale as f32 * 0.35).max(1.5 * k).round();
    let mut offsets = vec![(o * 1.4, o * 1.6)];
    for a in 0..12 {
        let ang = a as f32 * TAU / 12.0;
        offsets.push((ang.cos() * o, ang.sin() * o));
    }
    for (dx, dy) in offsets {
        canvas.draw_text(text, pos + vec2(dx.round(), dy.round()), scale, SHADOW);
    }
    canvas.draw_text(text, pos, scale, color);
}

fn arc(canvas: &mut Canvas, c: Vec2, r: f32, fraction: f32, color: Color, thickness: f32) {
    let steps = (120.0 * fraction).ceil() as i32;
    for i in 0..=steps {
        let a = -PI / 2.0 + i as f32 / 120.0 * TAU;
        canvas.fill_circle(c + vec2(a.cos(), a.sin()) * r, thickness / 2.0, color);
    }
}

fn thick_line(canvas: &mut Canvas, a: Vec2, b: Vec2, width: f32) {
    let steps = (a.distance(b) / (width * 0.3)).ceil().max(1.0) as i32;
    for i in 0..=steps {
        let p = a + (b - a) * (i as f32 / steps as f32);
        canvas.fill_circle(p, width / 2.0, Color::WHITE);
    }
}

fn ease_in(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * x
}

fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

fn ease_in_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

fn ease_out_back(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let c = 1.7;
    1.0 + (c + 1.0) * (x - 1.0).powi(3) + c * (x - 1.0).powi(2)
}

fn pop(x: f32) -> f32 {
    if x <= 0.0 {
        0.0
    } else {
        ease_out_back(x / 0.3)
    }
}

trait LerpTo {
    fn lerp_to(self, other: Self, t: f32) -> Self;
}

impl LerpTo for Vec3 {
    fn lerp_to(self, other: Vec3, t: f32) -> Vec3 {
        self + (other - self) * t
    }
}

fn scale_factor() -> usize {
    std::env::var("DOOR_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .clamp(1, 4)
}

fn config() -> Config {
    let k = scale_factor();
    Config {
        title: String::from("Choose Your Door 3D - duckforge"),
        width: BASE_W as usize * k,
        height: BASE_H as usize * k,
        scale: 1,
        timestep: Timestep::Fixed {
            updates_per_second: FPS,
        },
        seed: Some(1),
        ..Config::default()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    duckforge::run(config(), Show::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[test]
    fn every_round_has_one_win_one_ok_and_one_bad() {
        for round in &ROUNDS {
            for rating in [Rating::Win, Rating::Ok, Rating::Bad] {
                assert_eq!(round.doors.iter().filter(|d| d.1 == rating).count(), 1);
            }
        }
    }

    #[test]
    fn the_cues_fit_in_one_minute() {
        assert!(cues().iter().all(|c| (0.0..=TOTAL).contains(&c.0)));
    }

    #[test]
    fn every_prize_mesh_is_built() {
        let show = Show::new();
        for item in ITEMS {
            assert!(
                !show.prize(item).triangles.is_empty(),
                "{item:?} has no triangles"
            );
        }
    }

    /// Not a real test: saves PNGs of chosen frames. STILLS="90 300 450" DIR=folder
    #[test]
    #[ignore]
    fn stills() {
        let dir = std::env::var("DIR").unwrap_or_else(|_| String::from("target/door3d_stills"));
        std::fs::create_dir_all(&dir).unwrap();
        let wanted: Vec<u64> = std::env::var("STILLS")
            .unwrap_or_else(|_| String::from("60 150 300 430 1250"))
            .split_whitespace()
            .filter_map(|s| s.parse().ok())
            .collect();
        let last = wanted.iter().copied().max().unwrap_or(0);
        let mut sim = Simulation::new(config(), Show::new());
        for frame in 0..=last {
            if wanted.contains(&frame) {
                sim.picture()
                    .save_png(format!("{dir}/{frame:05}.png"))
                    .unwrap();
            }
            sim.step();
        }
    }

    /// Not a real test: renders the whole video straight into ffmpeg, plus cues.json.
    #[test]
    #[ignore]
    fn render() {
        let out = std::env::var("OUT").unwrap_or_else(|_| String::from("target/door_show_3d.mp4"));
        let config = config();
        let (w, h) = (config.width, config.height);
        let mut ffmpeg = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "bgr0",
            ])
            .args(["-s", &format!("{w}x{h}"), "-r", &FPS.to_string(), "-i", "-"])
            .args([
                "-vf",
                "scale=1080:1920:flags=area",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
            ])
            .args(["-crf", "16", "-pix_fmt", "yuv420p", &out])
            .stdin(Stdio::piped())
            .spawn()
            .expect("ffmpeg must be installed");
        let mut pipe = ffmpeg.stdin.take().unwrap();
        let mut sim = Simulation::new(config, Show::new());
        let mut bytes = Vec::with_capacity(w * h * 4);
        let started = std::time::Instant::now();
        for _ in 0..(TOTAL * FPS as f32) as u64 {
            bytes.clear();
            for &p in sim.picture().pixels() {
                bytes.extend_from_slice(&p.to_le_bytes());
            }
            pipe.write_all(&bytes).unwrap();
            sim.step();
        }
        drop(pipe);
        assert!(ffmpeg.wait().unwrap().success());
        println!("rendered in {:.1} s", started.elapsed().as_secs_f32());
        let json: Vec<String> = cues()
            .iter()
            .map(|(t, name)| format!("  {{\"t\": {t:.3}, \"sound\": \"{name}\"}}"))
            .collect();
        let cues_path = std::path::Path::new(&out).with_extension("cues.json");
        std::fs::write(cues_path, format!("[\n{}\n]\n", json.join(",\n"))).unwrap();
    }
}
