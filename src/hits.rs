//! What was hit: a ray, a line segment or a ball tested against a Duck
//! Mold model's **hit shapes**, in whatever pose the model is in.
//!
//! A Duck Mold recipe can give its parts hit shapes (`colliders =
//! "parts"`): a box, a ball or a capsule (a cylinder with a ball on each
//! end) for each part, which moves with the part when it is posed. The
//! `.glb` holds them, and an [`AnimatedModel`] knows where they are in
//! the frame it last worked out. These ask it what is where:
//!
//! ```no_run
//! use duckforge::glb::AnimatedModel;
//! use duckforge::prelude::*;
//!
//! # fn demo(robot: &AnimatedModel, camera: &Camera3D, canvas: &Canvas, mouse: Vec2) {
//! let place = Transform::at(vec3(2.0, 0.0, 5.0)); // where the robot is drawn
//! let ray = camera.ray_through(mouse, canvas);     // from the eye, through the mouse
//! if let Some(hit) = robot.hit_ray(ray, &place) {
//!     println!("you clicked the {}", hit.part);    // "head", "left_arm/hand"
//! }
//! for hit in robot.hits_ball(vec3(2.3, 1.4, 5.0), 0.2, &place) {
//!     println!("the explosion reached the {}", hit.part);
//! }
//! # }
//! ```
//!
//! This is separate from [`physics`](crate::physics): that moves bodies
//! that fall and bounce; this answers questions about a model. A game
//! usually has both: a physics body for the character to stand and bump
//! with, and the model's hit shapes to tell which part a bullet found.
//!
//! The model is placed by the same [`Transform`] it is drawn with: its
//! position and its rotation are used exactly. A model scaled the same
//! in every direction is fine; one stretched more one way than another
//! is tested as if every size were scaled by the largest of the three.

use crate::glb::{AnimatedModel, GlbHitShape, GlbModel};
use crate::math::{Vec3, vec3};
use crate::physics::Body;
use crate::render3d::Transform;

/// A ray: a starting point and a direction, going on for ever.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where it starts.
    pub origin: Vec3,
    /// Which way it goes, 1 long.
    pub direction: Vec3,
}

impl Ray {
    /// A ray from `origin` going `direction` (any length but 0; it is
    /// made 1 long).
    pub fn new(origin: Vec3, direction: Vec3) -> Ray {
        Ray {
            origin,
            direction: direction.normalized(),
        }
    }

    /// The point `distance` along it.
    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }
}

/// What a hit test found: the part, and where.
#[derive(Debug, Clone, PartialEq)]
pub struct PartHit {
    /// The part, by its path in the recipe: `head`, `left_arm/hand`. Empty
    /// for a model with one hit shape round all of it.
    pub part: String,
    /// Where, in the world: on the part's hit shape. For a ball that
    /// reaches a part, the point of the part's hit shape nearest the
    /// ball's middle.
    pub point: Vec3,
    /// Which way the hit shape's surface faces there, 1 long.
    pub normal: Vec3,
    /// How far: along the ray or segment from its start; for a ball,
    /// from its middle to `point` (0 if the middle is inside the part).
    pub distance: f32,
}

/// A part's hit shape as it is now, in the world.
#[derive(Debug, Clone, PartialEq)]
pub struct HitShape {
    /// The part, by its path; empty for one round the whole model.
    pub part: String,
    /// The shape.
    pub volume: HitVolume,
}

/// A box, a ball or a capsule, in the world.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitVolume {
    /// A box: its middle, its three directions (each 1 long) and how far
    /// it reaches along each from the middle.
    Box {
        /// Its middle.
        center: Vec3,
        /// Its three directions.
        axes: [Vec3; 3],
        /// How far it reaches along each.
        half: Vec3,
    },
    /// A ball.
    Ball {
        /// Its middle.
        center: Vec3,
        /// Its radius.
        radius: f32,
    },
    /// Every point within `radius` of the line from `a` to `b`.
    Capsule {
        /// One end of its middle line.
        a: Vec3,
        /// The other end.
        b: Vec3,
        /// How far it reaches from that line.
        radius: f32,
    },
}

/// Turns a direction as `place` turns the model (without moving it).
fn turn(place: &Transform, direction: Vec3) -> Vec3 {
    direction
        .rotate_x(place.rotation.x)
        .rotate_y(place.rotation.y)
        .rotate_z(place.rotation.z)
}

fn v(point: [f32; 3]) -> Vec3 {
    vec3(point[0], point[1], point[2])
}

impl HitVolume {
    /// A model's hit shape, placed in the world.
    fn placed(shape: &GlbHitShape, place: &Transform) -> HitVolume {
        let grow = place.largest_scale();
        match *shape {
            GlbHitShape::Box { center, axes, half } => HitVolume::Box {
                center: place.apply(v(center)),
                axes: axes.map(|axis| turn(place, v(axis)).normalized()),
                half: v(half) * grow,
            },
            GlbHitShape::Sphere { center, radius } => HitVolume::Ball {
                center: place.apply(v(center)),
                radius: radius * grow,
            },
            GlbHitShape::Capsule { a, b, radius } => HitVolume::Capsule {
                a: place.apply(v(a)),
                b: place.apply(v(b)),
                radius: radius * grow,
            },
        }
    }

    /// Where a ray first meets it, as how far along the ray, and which
    /// way the surface faces there. A ray that starts inside meets it at
    /// once (0), facing back along the ray.
    pub fn ray(&self, ray: &Ray) -> Option<(f32, Vec3)> {
        match *self {
            HitVolume::Ball { center, radius } => ray_ball(ray, center, radius),
            HitVolume::Box { center, axes, half } => ray_box(ray, center, axes, half),
            HitVolume::Capsule { a, b, radius } => ray_capsule(ray, a, b, radius),
        }
    }

    /// The point of it nearest `point` (the point itself, if it is
    /// inside), and which way its surface faces there.
    pub fn nearest(&self, point: Vec3) -> (Vec3, Vec3) {
        match *self {
            HitVolume::Ball { center, radius } => {
                let from = point - center;
                let length = from.length();
                if length <= radius {
                    return (point, normal_or_up(from));
                }
                let out = from * (1.0 / length);
                (center + out * radius, out)
            }
            HitVolume::Box { center, axes, half } => {
                let from = point - center;
                let reach = [half.x, half.y, half.z];
                let mut nearest = center;
                let mut inside = true;
                let mut best = (f32::INFINITY, axes[1]);
                for (at, axis) in axes.iter().enumerate() {
                    let along = from.dot(*axis);
                    let kept = along.clamp(-reach[at], reach[at]);
                    if kept != along {
                        inside = false;
                    }
                    nearest += *axis * kept;
                    // How far to this axis's nearer face, from inside.
                    let to_face = reach[at] - along.abs();
                    if to_face < best.0 {
                        best = (to_face, *axis * along.signum());
                    }
                }
                if inside {
                    return (point, normal_or_up(best.1));
                }
                (nearest, normal_or_up(point - nearest))
            }
            HitVolume::Capsule { a, b, radius } => {
                let on_line = nearest_on_line(point, a, b);
                let from = point - on_line;
                let length = from.length();
                if length <= radius {
                    return (point, normal_or_up(from));
                }
                let out = from * (1.0 / length);
                (on_line + out * radius, out)
            }
        }
    }

    /// True if `point` is inside it.
    pub fn holds(&self, point: Vec3) -> bool {
        self.nearest(point).0 == point
    }
}

/// `direction` made 1 long, or straight up if it has no length.
fn normal_or_up(direction: Vec3) -> Vec3 {
    if direction.length() > 1e-12 {
        direction.normalized()
    } else {
        vec3(0.0, 1.0, 0.0)
    }
}

/// The point of the straight line from `a` to `b` nearest `point`.
fn nearest_on_line(point: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let line = b - a;
    let length = line.dot(line);
    if length <= 0.0 {
        return a;
    }
    a + line * ((point - a).dot(line) / length).clamp(0.0, 1.0)
}

fn ray_ball(ray: &Ray, center: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    let from = ray.origin - center;
    if from.length() <= radius {
        return Some((0.0, -ray.direction));
    }
    let along = from.dot(ray.direction);
    let rest = from.dot(from) - radius * radius;
    let room = along * along - rest;
    if room < 0.0 {
        return None;
    }
    let distance = -along - room.sqrt();
    (distance >= 0.0).then(|| (distance, (ray.at(distance) - center).normalized()))
}

fn ray_box(ray: &Ray, center: Vec3, axes: [Vec3; 3], half: Vec3) -> Option<(f32, Vec3)> {
    let from = ray.origin - center;
    let reach = [half.x, half.y, half.z];
    let (mut enter, mut leave) = (0.0f32, f32::INFINITY);
    let mut normal = -ray.direction;
    for (at, axis) in axes.iter().enumerate() {
        let start = from.dot(*axis);
        let speed = ray.direction.dot(*axis);
        if speed.abs() < 1e-9 {
            if start.abs() > reach[at] {
                return None; // going alongside, outside
            }
            continue;
        }
        let mut near = (-reach[at] - start) / speed;
        let mut far = (reach[at] - start) / speed;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        if near > enter {
            enter = near;
            normal = *axis * -speed.signum();
        }
        leave = leave.min(far);
        if enter > leave {
            return None;
        }
    }
    Some((enter, normal))
}

fn ray_capsule(ray: &Ray, a: Vec3, b: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    let on_line = nearest_on_line(ray.origin, a, b);
    if (ray.origin - on_line).length() <= radius {
        return Some((0.0, -ray.direction));
    }
    // The two round ends...
    let mut best = [a, b]
        .into_iter()
        .filter_map(|end| ray_ball(ray, end, radius))
        .min_by(|one, other| one.0.total_cmp(&other.0));
    // ...and the side, between them.
    let line = b - a;
    let length = line.length();
    if length > 1e-9 {
        let along_line = line * (1.0 / length);
        let from = ray.origin - a;
        // The ray and the start, with the part along the line taken away.
        let d = ray.direction - along_line * ray.direction.dot(along_line);
        let o = from - along_line * from.dot(along_line);
        let (qa, qb, qc) = (d.dot(d), 2.0 * o.dot(d), o.dot(o) - radius * radius);
        if qa > 1e-12 {
            let room = qb * qb - 4.0 * qa * qc;
            if room >= 0.0 {
                let distance = (-qb - room.sqrt()) / (2.0 * qa);
                if distance >= 0.0 {
                    let point = ray.at(distance);
                    let t = (point - a).dot(along_line);
                    if (0.0..=length).contains(&t) {
                        let axis_point = a + along_line * t;
                        let side = (distance, (point - axis_point).normalized());
                        if best.is_none_or(|known| side.0 < known.0) {
                            best = Some(side);
                        }
                    }
                }
            }
        }
    }
    best
}

impl AnimatedModel {
    /// The model's hit shapes as they are now (in the frame the last
    /// [`update`](AnimatedModel::update) worked out), placed in the world
    /// by `place`: to draw them while making a game, say. Empty for a
    /// model without any.
    pub fn hit_shapes(&self, place: &Transform) -> Vec<HitShape> {
        let stands = self.stands_now();
        self.model()
            .colliders
            .iter()
            .map(|collider| {
                let shape = match collider.join.and_then(|join| stands[join]) {
                    Some(stand) => collider.shape.moved(&stand),
                    None => collider.shape,
                };
                HitShape {
                    part: collider.part.clone(),
                    volume: HitVolume::placed(&shape, place),
                }
            })
            .collect()
    }

    /// The first part a ray hits, on the model drawn at `place`, as it
    /// stands now: for a click (with [`Camera3D::ray_through`]), or a
    /// shot. `None` if it misses every hit shape, or the model has none.
    ///
    /// [`Camera3D::ray_through`]: crate::render3d::Camera3D::ray_through
    pub fn hit_ray(&self, ray: Ray, place: &Transform) -> Option<PartHit> {
        self.hit_shapes(place)
            .into_iter()
            .filter_map(|shape| {
                let (distance, normal) = shape.volume.ray(&ray)?;
                Some(PartHit {
                    part: shape.part,
                    point: ray.at(distance),
                    normal,
                    distance,
                })
            })
            .min_by(|one, other| one.distance.total_cmp(&other.distance))
    }

    /// The first part hit on the straight way from `from` to `to`: a
    /// bullet between two frames, a sword's blade. `None` if the way is
    /// clear.
    pub fn hit_segment(&self, from: Vec3, to: Vec3, place: &Transform) -> Option<PartHit> {
        let length = (to - from).length();
        if length <= 0.0 {
            return self
                .hits_ball(from, 0.0, place)
                .into_iter()
                .next()
                .map(|hit| PartHit {
                    distance: 0.0,
                    ..hit
                });
        }
        self.hit_ray(Ray::new(from, to - from), place)
            .filter(|hit| hit.distance <= length)
    }

    /// Every part a ball touches, nearest first: an explosion, a sword's
    /// tip, a hand reaching. For each, the point of its hit shape nearest
    /// the ball's middle.
    pub fn hits_ball(&self, center: Vec3, radius: f32, place: &Transform) -> Vec<PartHit> {
        let mut hits: Vec<PartHit> = self
            .hit_shapes(place)
            .into_iter()
            .filter_map(|shape| {
                let (point, normal) = shape.volume.nearest(center);
                let distance = (point - center).length();
                (distance <= radius).then_some(PartHit {
                    part: shape.part,
                    point,
                    normal,
                    distance,
                })
            })
            .collect();
        hits.sort_by(|one, other| one.distance.total_cmp(&other.distance));
        hits
    }
}

// ---- a body for physics ---------------------------------------------------------

impl GlbModel {
    /// The box round every hit shape with the model at rest, as its
    /// lowest corner and its highest; round the model's triangles if it
    /// has no hit shapes. `None` for a model with neither.
    pub fn hit_box(&self) -> Option<(Vec3, Vec3)> {
        let mut low = vec3(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut high = vec3(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut take = |at: Vec3, reach: Vec3| {
            low = vec3(
                low.x.min(at.x - reach.x),
                low.y.min(at.y - reach.y),
                low.z.min(at.z - reach.z),
            );
            high = vec3(
                high.x.max(at.x + reach.x),
                high.y.max(at.y + reach.y),
                high.z.max(at.z + reach.z),
            );
        };
        for collider in &self.colliders {
            match collider.shape {
                GlbHitShape::Box { center, axes, half } => {
                    // How far a turned box reaches along x, y and z.
                    let reach = |axis: usize| {
                        (0..3)
                            .map(|along| (axes[along][axis] * half[along]).abs())
                            .sum::<f32>()
                    };
                    take(v(center), vec3(reach(0), reach(1), reach(2)));
                }
                GlbHitShape::Sphere { center, radius } => {
                    take(v(center), vec3(radius, radius, radius));
                }
                GlbHitShape::Capsule { a, b, radius } => {
                    take(v(a), vec3(radius, radius, radius));
                    take(v(b), vec3(radius, radius, radius));
                }
            }
        }
        if self.colliders.is_empty() {
            for corner in &self.vertices {
                take(v(*corner), Vec3::ZERO);
            }
        }
        (low.x <= high.x).then_some((low, high))
    }

    /// A body for Duck Forge's physics: a block round the model's hit
    /// shapes as it stands at rest (round its triangles if it has none),
    /// standing with the middle of its bottom at `feet`. The block is the
    /// model at rest, not posed: a body that changed size with every step
    /// would bounce on the floor. A part with no hit shape (an antenna's
    /// tip) doesn't make it taller. Use [`GlbModel::stands_at`] to know
    /// where to draw the model on it.
    ///
    /// ```ignore
    /// let body = world.add(robot.model().physics_body(vec3(0.0, 0.0, 3.0)));
    /// world.step(dt);
    /// let place = Transform::at(robot.model().stands_at(world.get(body).unwrap()));
    /// ```
    ///
    /// It's a starting body: `.with_mass`, `.with_friction` and the rest
    /// work on it as on any.
    pub fn physics_body(&self, feet: Vec3) -> Body<Vec3> {
        let (low, high) = self
            .hit_box()
            .unwrap_or((vec3(-0.5, 0.0, -0.5), vec3(0.5, 1.0, 0.5)));
        let size = high - low;
        Body::block(feet + vec3(0.0, size.y * 0.5, 0.0), size)
    }

    /// Where to draw the model so that it stands in `body` (one made by
    /// [`GlbModel::physics_body`]): the point under the model's own zero,
    /// for [`Transform::at`]. The block's middle across x and z is the hit
    /// shapes' middle, so a character with one arm out isn't off-centre.
    pub fn stands_at(&self, body: &Body<Vec3>) -> Vec3 {
        let (low, high) = self
            .hit_box()
            .unwrap_or((vec3(-0.5, 0.0, -0.5), vec3(0.5, 1.0, 0.5)));
        let middle = (low + high) * 0.5;
        let half_height = (high.y - low.y) * 0.5;
        vec3(
            body.position.x - middle.x,
            body.position.y - half_height - low.y,
            body.position.z - middle.z,
        )
    }
}
