//! Vector math: [`Vec2`] and [`Rect`] for 2D, [`Vec3`] for 3D, and the
//! [`Vector`] trait that lets the same code (like the physics engine) work
//! with both.

use std::fmt::Debug;
use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

/// A 2D vector. Used for positions, velocities, directions and sizes.
///
/// `Vec2` is `Copy`, so it is passed around by value just like an `f32`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// Shorthand for [`Vec2::new`]: `vec2(1.0, 2.0)`.
pub const fn vec2(x: f32, y: f32) -> Vec2 {
    Vec2 { x, y }
}

impl Vec2 {
    pub const ZERO: Vec2 = vec2(0.0, 0.0);

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The length (magnitude) of the vector, by Pythagoras.
    pub fn length(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// A vector pointing the same way with length 1.
    /// Returns [`Vec2::ZERO`] for the zero vector instead of dividing by zero.
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len == 0.0 {
            Self::ZERO
        } else {
            self * (1.0 / len)
        }
    }

    /// The distance between two points.
    pub fn distance(self, other: Vec2) -> f32 {
        (other - self).length()
    }

    /// The dot product: `x1 * x2 + y1 * y2`. Positive when the vectors point
    /// roughly the same way, zero when they are at right angles.
    pub fn dot(self, other: Vec2) -> f32 {
        self.x * other.x + self.y * other.y
    }
}

// Operator overloading: implementing these traits is what lets us write
// `a + b`, `a - b`, `v * 2.0`, `-v` and `pos += vel` with vectors.

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, rhs: Vec2) -> Vec2 {
        vec2(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, rhs: Vec2) -> Vec2 {
        vec2(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f32> for Vec2 {
    type Output = Vec2;
    fn mul(self, rhs: f32) -> Vec2 {
        vec2(self.x * rhs, self.y * rhs)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        vec2(-self.x, -self.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, rhs: Vec2) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, rhs: Vec2) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

/// An axis-aligned rectangle: `(x, y)` is the top-left corner.
///
/// Screen coordinates grow to the right (x) and *down* (y).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Builds a rectangle of the given size centered on `center`.
    pub fn from_center(center: Vec2, w: f32, h: f32) -> Self {
        Self::new(center.x - w / 2.0, center.y - h / 2.0, w, h)
    }

    pub fn left(&self) -> f32 {
        self.x
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn top(&self) -> f32 {
        self.y
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Vec2 {
        vec2(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Axis-aligned bounding box (AABB) collision test: do the two rectangles overlap?
    ///
    /// Two boxes overlap unless one is completely to the left, right,
    /// above or below the other. Touching edges do not count as overlapping.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.left() < other.right()
            && self.right() > other.left()
            && self.top() < other.bottom()
            && self.bottom() > other.top()
    }

    /// Is the point inside the rectangle?
    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.left()
            && point.x < self.right()
            && point.y >= self.top()
            && point.y < self.bottom()
    }

    /// The rectangle shrunk by `by` on every side, keeping its center.
    /// A negative `by` grows it instead. It never gets smaller than 0 x 0.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let screen = Rect::new(0.0, 0.0, 320.0, 240.0);
    /// let inside_the_border = screen.inset(8.0);
    /// assert_eq!(inside_the_border, Rect::new(8.0, 8.0, 304.0, 224.0));
    /// ```
    pub fn inset(&self, by: f32) -> Rect {
        let w = (self.w - 2.0 * by).max(0.0);
        let h = (self.h - 2.0 * by).max(0.0);
        Rect::from_center(self.center(), w, h)
    }

    /// The rectangle grown by `by` on every side, keeping its center.
    /// Handy for a forgiving hitbox, or a border around a panel.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let coin = Rect::new(100.0, 50.0, 8.0, 8.0);
    /// let easy_to_grab = coin.expand(4.0);
    /// assert_eq!(easy_to_grab, Rect::new(96.0, 46.0, 16.0, 16.0));
    /// ```
    pub fn expand(&self, by: f32) -> Rect {
        self.inset(-by)
    }

    /// Where two rectangles overlap, or `None` if they don't (touching
    /// edges don't count, the same as [`Rect::overlaps`]).
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// let a = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// let b = Rect::new(6.0, 4.0, 10.0, 10.0);
    /// assert_eq!(a.intersection(&b), Some(Rect::new(6.0, 4.0, 4.0, 6.0)));
    /// assert_eq!(a.intersection(&Rect::new(10.0, 0.0, 5.0, 5.0)), None);
    /// ```
    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        if !self.overlaps(other) {
            return None;
        }
        let left = self.left().max(other.left());
        let top = self.top().max(other.top());
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Some(Rect::new(left, top, right - left, bottom - top))
    }
}

/// Do a circle and a rectangle overlap? (Touching doesn't count.)
///
/// It finds the point of the rectangle closest to the circle's center, and
/// checks whether that point is inside the circle.
///
/// ```
/// # use duckforge::prelude::*;
/// let paddle = Rect::new(100.0, 200.0, 40.0, 8.0);
/// assert!(circle_vs_rect(vec2(120.0, 196.0), 5.0, paddle)); // ball dipping into the top
/// assert!(!circle_vs_rect(vec2(95.0, 195.0), 5.0, paddle)); // near the corner, but not touching
/// ```
pub fn circle_vs_rect(center: Vec2, radius: f32, rect: Rect) -> bool {
    let closest = vec2(
        center.x.clamp(rect.left(), rect.right().max(rect.left())),
        center.y.clamp(rect.top(), rect.bottom().max(rect.top())),
    );
    let gap = center - closest;
    gap.dot(gap) < radius * radius
}

/// Do two circles overlap? (Touching doesn't count.)
///
/// ```
/// # use duckforge::prelude::*;
/// assert!(circle_vs_circle(vec2(0.0, 0.0), 5.0, vec2(8.0, 0.0), 4.0)); // 8 apart, radii add to 9
/// assert!(!circle_vs_circle(vec2(0.0, 0.0), 5.0, vec2(9.0, 0.0), 4.0)); // just touching
/// ```
pub fn circle_vs_circle(a: Vec2, radius_a: f32, b: Vec2, radius_b: f32) -> bool {
    let gap = b - a;
    let reach = radius_a + radius_b;
    gap.dot(gap) < reach * reach
}

/// Where a moving rectangle first hits another: see [`swept_aabb`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// How far through the move the hit happens: 0.0 at the start, 1.0 at
    /// the end. Move by `velocity * time` to stop right at the wall.
    pub time: f32,
    /// Which way the surface that was hit faces: `vec2(0.0, -1.0)` is the
    /// top of the other rectangle (it pushes up), `vec2(-1.0, 0.0)` its
    /// left side, and so on.
    pub normal: Vec2,
}

/// Does `rect`, moving by `velocity` (this frame's movement: speed times
/// `ctx.dt()`), hit `other` on the way? If so, when and on which side.
///
/// This catches fast objects that would jump straight through a thin wall
/// between two frames, which a plain [`Rect::overlaps`] check at the end
/// of the move misses. ("AABB" means an axis-aligned bounding box: a
/// rectangle that doesn't rotate.)
///
/// ```
/// # use duckforge::prelude::*;
/// let bullet = Rect::new(0.0, 10.0, 4.0, 2.0);
/// let wall = Rect::new(50.0, 0.0, 2.0, 40.0);
/// let movement = vec2(100.0, 0.0); // fast: it would end up past the wall
///
/// let hit = swept_aabb(bullet, movement, wall).unwrap();
/// assert_eq!(hit.time, 0.46); // it reaches the wall 46% of the way
/// assert_eq!(hit.normal, vec2(-1.0, 0.0)); // it hit the wall's left side
/// let stopped_at = bullet.x + movement.x * hit.time; // 46.0: touching the wall
/// # assert_eq!(stopped_at, 46.0);
/// ```
///
/// If the rectangles already overlap at the start, the hit is at time 0,
/// with the normal pointing the shortest way out. Rectangles that only
/// slide along each other's edges don't hit.
pub fn swept_aabb(rect: Rect, velocity: Vec2, other: Rect) -> Option<Hit> {
    if rect.overlaps(&other) {
        // Already inside: the way out is through the nearest side.
        let pushes = [
            (other.right() - rect.left(), vec2(1.0, 0.0)),
            (rect.right() - other.left(), vec2(-1.0, 0.0)),
            (other.bottom() - rect.top(), vec2(0.0, 1.0)),
            (rect.bottom() - other.top(), vec2(0.0, -1.0)),
        ];
        let (_, normal) = pushes
            .into_iter()
            .fold((f32::INFINITY, Vec2::ZERO), |best, push| {
                if push.0 < best.0 { push } else { best }
            });
        return Some(Hit { time: 0.0, normal });
    }
    // For each axis: when (as a fraction of the move) the rectangles start
    // and stop overlapping along that axis.
    let (enter_x, leave_x) = overlap_times(
        (rect.left(), rect.right()),
        (other.left(), other.right()),
        velocity.x,
    );
    let (enter_y, leave_y) = overlap_times(
        (rect.top(), rect.bottom()),
        (other.top(), other.bottom()),
        velocity.y,
    );
    let enter = enter_x.max(enter_y);
    let leave = leave_x.min(leave_y);
    // A hit needs both axes to overlap at once, during this move.
    if enter >= leave || !(0.0..=1.0).contains(&enter) {
        return None;
    }
    let normal = if enter_x > enter_y {
        vec2(-velocity.x.signum(), 0.0)
    } else {
        vec2(0.0, -velocity.y.signum())
    };
    Some(Hit {
        time: enter,
        normal,
    })
}

/// For [`swept_aabb`], along one axis: when a span `(start, end)` moving
/// by `speed` starts and stops overlapping the span `other`, as fractions
/// of the move. Not moving means overlapping all the time, or never.
fn overlap_times(span: (f32, f32), other: (f32, f32), speed: f32) -> (f32, f32) {
    if speed > 0.0 {
        ((other.0 - span.1) / speed, (other.1 - span.0) / speed)
    } else if speed < 0.0 {
        ((other.1 - span.0) / speed, (other.0 - span.1) / speed)
    } else if span.1 <= other.0 || span.0 >= other.1 {
        (f32::INFINITY, f32::NEG_INFINITY) // never
    } else {
        (f32::NEG_INFINITY, f32::INFINITY) // always
    }
}

/// Moves `current` towards `target` by at most `step`, without going past
/// it. Great for smooth speeds and timers:
///
/// ```
/// # use duckforge::prelude::*;
/// let mut speed = 0.0;
/// speed = approach(speed, 100.0, 30.0); // speed up by 30 per call...
/// assert_eq!(speed, 30.0);
/// speed = approach(speed, 40.0, 30.0); // ...but never past the target
/// assert_eq!(speed, 40.0);
/// ```
///
/// In a game, scale the step by time: `approach(speed, top_speed, 400.0 * ctx.dt())`.
pub fn approach(current: f32, target: f32, step: f32) -> f32 {
    let step = step.abs();
    if current < target {
        (current + step).min(target)
    } else {
        (current - step).max(target)
    }
}

/// Turns `value` from one range into another: where it sits between
/// `from.0` and `from.1` becomes the same place between `to.0` and `to.1`.
/// Values outside `from` carry on past `to` (wrap it in `.clamp` if you
/// don't want that).
///
/// ```
/// # use duckforge::prelude::*;
/// // Health 0 to 100 as a bar 0 to 60 pixels wide:
/// assert_eq!(remap(25.0, (0.0, 100.0), (0.0, 60.0)), 15.0);
/// // Height 200 (ground) to 0 (sky) as volume 0.0 to 1.0, reversed:
/// assert_eq!(remap(50.0, (200.0, 0.0), (0.0, 1.0)), 0.75);
/// ```
///
/// If `from` is empty (both ends the same), the result is `to.0`.
pub fn remap(value: f32, from: (f32, f32), to: (f32, f32)) -> f32 {
    let span = from.1 - from.0;
    if span == 0.0 {
        return to.0;
    }
    to.0 + (value - from.0) / span * (to.1 - to.0)
}

/// A 3D vector.
///
/// In 3D the engine uses these directions: `x` is right, `y` is **up**
/// and `z` is forward (into the screen). Note that `y` points the other way
/// from 2D screen coordinates, where it points down.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// Shorthand for [`Vec3::new`]: `vec3(1.0, 2.0, 3.0)`.
pub const fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3 { x, y, z }
}

impl Vec3 {
    pub const ZERO: Vec3 = vec3(0.0, 0.0, 0.0);
    pub const ONE: Vec3 = vec3(1.0, 1.0, 1.0);
    pub const UP: Vec3 = vec3(0.0, 1.0, 0.0);

    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }

    /// A vector pointing the same way with length 1 (or zero for the zero vector).
    pub fn normalized(self) -> Self {
        let len = self.length();
        if len == 0.0 {
            Self::ZERO
        } else {
            self * (1.0 / len)
        }
    }

    pub fn distance(self, other: Vec3) -> f32 {
        (other - self).length()
    }

    pub fn dot(self, other: Vec3) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// The cross product: a vector at right angles to both inputs. The 3D
    /// renderer uses it to find which way a triangle is facing.
    pub fn cross(self, other: Vec3) -> Vec3 {
        vec3(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Rotates around the x axis (tilting forward/back). A positive angle
    /// turns forward (+z) towards up (+y).
    pub fn rotate_x(self, angle: f32) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vec3(
            self.x,
            self.y * cos + self.z * sin,
            -self.y * sin + self.z * cos,
        )
    }

    /// Rotates around the y axis (turning left/right). A positive angle
    /// turns forward (+z) towards right (+x).
    pub fn rotate_y(self, angle: f32) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vec3(
            self.x * cos + self.z * sin,
            self.y,
            -self.x * sin + self.z * cos,
        )
    }

    /// Rotates around the z axis (rolling). A positive angle turns right (+x)
    /// towards up (+y).
    pub fn rotate_z(self, angle: f32) -> Vec3 {
        let (sin, cos) = angle.sin_cos();
        vec3(
            self.x * cos - self.y * sin,
            self.x * sin + self.y * cos,
            self.z,
        )
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, rhs: Vec3) -> Vec3 {
        vec3(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, rhs: Vec3) -> Vec3 {
        vec3(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Mul<f32> for Vec3 {
    type Output = Vec3;
    fn mul(self, rhs: f32) -> Vec3 {
        vec3(self.x * rhs, self.y * rhs, self.z * rhs)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        vec3(-self.x, -self.y, -self.z)
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Vec3) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl SubAssign for Vec3 {
    fn sub_assign(&mut self, rhs: Vec3) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

/// Anything that behaves like a list of numbers you can add, subtract and
/// scale: [`Vec2`] and [`Vec3`].
///
/// Code written for "any `V: Vector`" works in 2D *and* 3D. The physics
/// engine is written this way, so one implementation handles both.
///
/// The long list after the `:` are *supertraits*: to be a `Vector`, a type
/// must also support `+`, `-`, `* f32` and so on.
pub trait Vector:
    Copy
    + Debug
    + Default
    + PartialEq
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<f32, Output = Self>
    + Neg<Output = Self>
    + AddAssign
    + SubAssign
{
    /// How many numbers the vector holds: 2 for [`Vec2`], 3 for [`Vec3`].
    const DIMENSIONS: usize;

    /// One component: axis 0 is x, 1 is y, 2 is z.
    fn get(self, axis: usize) -> f32;

    /// A copy of this vector with one component replaced.
    fn with(self, axis: usize, value: f32) -> Self;

    fn dot(self, other: Self) -> f32;

    /// A vector of length 1 along one axis, like `(0, 1)` or `(0, 0, 1)`.
    ///
    /// This one has a *default implementation*, so `Vec2` and `Vec3` get it for free.
    fn unit(axis: usize) -> Self {
        Self::default().with(axis, 1.0)
    }
}

impl Vector for Vec2 {
    const DIMENSIONS: usize = 2;

    fn get(self, axis: usize) -> f32 {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => panic!("Vec2 has no axis {axis}"),
        }
    }

    fn with(mut self, axis: usize, value: f32) -> Self {
        match axis {
            0 => self.x = value,
            1 => self.y = value,
            _ => panic!("Vec2 has no axis {axis}"),
        }
        self
    }

    fn dot(self, other: Self) -> f32 {
        Vec2::dot(self, other)
    }
}

impl Vector for Vec3 {
    const DIMENSIONS: usize = 3;

    fn get(self, axis: usize) -> f32 {
        match axis {
            0 => self.x,
            1 => self.y,
            2 => self.z,
            _ => panic!("Vec3 has no axis {axis}"),
        }
    }

    fn with(mut self, axis: usize, value: f32) -> Self {
        match axis {
            0 => self.x = value,
            1 => self.y = value,
            2 => self.z = value,
            _ => panic!("Vec3 has no axis {axis}"),
        }
        self
    }

    fn dot(self, other: Self) -> f32 {
        Vec3::dot(self, other)
    }
}

/// A 4x4 matrix: one package holding a whole chain of moves, rotations,
/// scalings, and even the perspective projection.
///
/// The GPU renderer uses these: instead of rotating and moving every
/// corner step by step like the CPU renderer, it multiplies each corner by
/// one matrix. Multiplying two matrices gives a matrix that does both jobs,
/// so `translation * rotation * scaling` scales first, then rotates, then
/// moves (read right to left).
///
/// The numbers are stored column by column, the layout GPUs expect:
/// `columns[c][r]` is row `r` of column `c`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub columns: [[f32; 4]; 4],
}

impl Mat4 {
    /// The "do nothing" matrix.
    pub const IDENTITY: Mat4 = Mat4 {
        columns: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };

    /// Moves points by `offset`.
    pub fn translation(offset: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.columns[3] = [offset.x, offset.y, offset.z, 1.0];
        m
    }

    /// Stretches points along each axis.
    pub fn scaling(scale: Vec3) -> Mat4 {
        let mut m = Mat4::IDENTITY;
        m.columns[0][0] = scale.x;
        m.columns[1][1] = scale.y;
        m.columns[2][2] = scale.z;
        m
    }

    /// The same rotation as [`Vec3::rotate_x`].
    pub fn rotation_x(angle: f32) -> Mat4 {
        let (sin, cos) = angle.sin_cos();
        Mat4 {
            columns: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, cos, -sin, 0.0],
                [0.0, sin, cos, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// The same rotation as [`Vec3::rotate_y`].
    pub fn rotation_y(angle: f32) -> Mat4 {
        let (sin, cos) = angle.sin_cos();
        Mat4 {
            columns: [
                [cos, 0.0, -sin, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [sin, 0.0, cos, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// The same rotation as [`Vec3::rotate_z`].
    pub fn rotation_z(angle: f32) -> Mat4 {
        let (sin, cos) = angle.sin_cos();
        Mat4 {
            columns: [
                [cos, sin, 0.0, 0.0],
                [-sin, cos, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    /// A perspective projection for a camera looking along +z, in the form
    /// the GPU wants: x and y from -1 to 1 across the screen, and a *depth*
    /// that is 1.0 at `near` and shrinks towards 0.0 far away.
    ///
    /// That depth is `near / distance`: the same "bigger means closer" idea
    /// as the CPU renderer's `1 / distance` depth buffer. (GPU programmers
    /// call it *reversed Z*: it keeps far-away depths precise.)
    pub fn perspective(fov_y: f32, aspect: f32, near: f32) -> Mat4 {
        let y = 1.0 / (fov_y / 2.0).tan();
        let x = y / aspect;
        Mat4 {
            columns: [
                [x, 0.0, 0.0, 0.0],
                [0.0, y, 0.0, 0.0],
                [0.0, 0.0, 0.0, 1.0], // w = z: the GPU divides by the distance
                [0.0, 0.0, near, 0.0],
            ],
        }
    }

    /// Multiplies a 4-number column `[x, y, z, w]` by this matrix.
    ///
    /// Each result row adds up "column times amount" over the four columns.
    /// (Written out in full because the GPU renderer does this thousands of
    /// times per frame; the compiler turns it into very fast code.)
    pub fn transform(&self, v: [f32; 4]) -> [f32; 4] {
        let c = &self.columns;
        [0, 1, 2, 3]
            .map(|row| c[0][row] * v[0] + c[1][row] * v[1] + c[2][row] * v[2] + c[3][row] * v[3])
    }

    /// Transforms a point (a position, so moves apply to it).
    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let [x, y, z, _] = self.transform([p.x, p.y, p.z, 1.0]);
        vec3(x, y, z)
    }
}

impl Mul for Mat4 {
    type Output = Mat4;

    /// `a * b` does `b` first, then `a`.
    fn mul(self, rhs: Mat4) -> Mat4 {
        Mat4 {
            columns: rhs.columns.map(|column| self.transform(column)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inset_and_expand_keep_the_center() {
        let r = Rect::new(10.0, 20.0, 30.0, 40.0);
        assert_eq!(r.inset(5.0), Rect::new(15.0, 25.0, 20.0, 30.0));
        assert_eq!(r.expand(5.0), Rect::new(5.0, 15.0, 40.0, 50.0));
        assert_eq!(r.inset(-5.0), r.expand(5.0));
        // Shrunk too far: a 0 x 0 rectangle at the center, never negative.
        let gone = r.inset(100.0);
        assert_eq!((gone.w, gone.h), (0.0, 0.0));
        assert_eq!(gone.center(), r.center());
    }

    #[test]
    fn intersections() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(a.intersection(&a), Some(a));
        let inside = Rect::new(2.0, 3.0, 4.0, 5.0);
        assert_eq!(a.intersection(&inside), Some(inside));
        assert_eq!(inside.intersection(&a), Some(inside));
        assert_eq!(
            a.intersection(&Rect::new(0.0, 10.0, 10.0, 10.0)),
            None,
            "touching"
        );
        assert_eq!(a.intersection(&Rect::new(20.0, 20.0, 1.0, 1.0)), None);
    }

    #[test]
    fn circles_against_rects_and_circles() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(circle_vs_rect(vec2(5.0, 5.0), 1.0, r), "center inside");
        assert!(
            circle_vs_rect(vec2(12.0, 5.0), 2.5, r),
            "overlapping a side"
        );
        assert!(
            !circle_vs_rect(vec2(12.0, 5.0), 2.0, r),
            "just touching a side"
        );
        // Diagonal from the corner (10, 10): distance 5 * sqrt(2), about 7.07.
        assert!(circle_vs_rect(vec2(15.0, 15.0), 7.1, r));
        assert!(!circle_vs_rect(vec2(15.0, 15.0), 7.0, r));
        assert!(circle_vs_circle(vec2(0.0, 0.0), 1.0, vec2(0.0, 0.0), 1.0));
        assert!(circle_vs_circle(vec2(0.0, 0.0), 3.0, vec2(3.0, 4.0), 2.01));
        assert!(!circle_vs_circle(vec2(0.0, 0.0), 3.0, vec2(3.0, 4.0), 2.0));
    }

    #[test]
    fn swept_hits_from_every_side() {
        let wall = Rect::new(10.0, 10.0, 10.0, 10.0);
        let block = |x: f32, y: f32| Rect::new(x, y, 2.0, 2.0);
        // From the left, right, top and bottom, each 4 pixels away, moving 8.
        let cases = [
            (block(4.0, 14.0), vec2(8.0, 0.0), vec2(-1.0, 0.0)),
            (block(24.0, 14.0), vec2(-8.0, 0.0), vec2(1.0, 0.0)),
            (block(14.0, 4.0), vec2(0.0, 8.0), vec2(0.0, -1.0)),
            (block(14.0, 24.0), vec2(0.0, -8.0), vec2(0.0, 1.0)),
        ];
        for (rect, velocity, normal) in cases {
            let hit = swept_aabb(rect, velocity, wall).unwrap();
            assert_eq!(
                hit,
                Hit { time: 0.5, normal },
                "{rect:?} moving {velocity:?}"
            );
        }
    }

    #[test]
    fn swept_misses() {
        let wall = Rect::new(10.0, 10.0, 10.0, 10.0);
        let block = Rect::new(0.0, 0.0, 2.0, 2.0);
        assert_eq!(swept_aabb(block, vec2(4.0, 4.0), wall), None, "stops short");
        assert_eq!(
            swept_aabb(block, vec2(-50.0, 0.0), wall),
            None,
            "moving away"
        );
        assert_eq!(
            swept_aabb(block, vec2(50.0, 0.0), wall),
            None,
            "passes above"
        );
        assert_eq!(swept_aabb(block, Vec2::ZERO, wall), None, "not moving");
        // Sliding along the wall's top edge, touching it, isn't a hit...
        let on_top = Rect::new(0.0, 8.0, 2.0, 2.0);
        assert_eq!(swept_aabb(on_top, vec2(40.0, 0.0), wall), None);
        // ...but pressing down into it from on top is, straight away.
        let pressing = Rect::new(12.0, 8.0, 2.0, 2.0);
        let hit = swept_aabb(pressing, vec2(0.0, 3.0), wall).unwrap();
        assert_eq!(
            hit,
            Hit {
                time: 0.0,
                normal: vec2(0.0, -1.0)
            }
        );
    }

    #[test]
    fn swept_catches_a_fast_bullet_and_corners() {
        // A bullet so fast it jumps from one side of a thin wall to the other.
        let bullet = Rect::new(0.0, 5.0, 1.0, 1.0);
        let thin = Rect::new(100.0, 0.0, 1.0, 20.0);
        let after = Rect::new(bullet.x + 1000.0, bullet.y, 1.0, 1.0);
        assert!(!after.overlaps(&thin), "checking only the end misses it");
        let hit = swept_aabb(bullet, vec2(1000.0, 0.0), thin).unwrap();
        assert_eq!(hit.normal, vec2(-1.0, 0.0));
        assert!((hit.time - 0.099).abs() < 1e-6);
        // Diagonal into a corner: the side reached last decides the normal.
        let wall = Rect::new(10.0, 10.0, 10.0, 10.0);
        let hit = swept_aabb(Rect::new(0.0, 4.0, 2.0, 2.0), vec2(10.0, 10.0), wall).unwrap();
        assert_eq!(hit.normal, vec2(-1.0, 0.0), "x lines up at 0.8, y at 0.4");
        assert!((hit.time - 0.8).abs() < 1e-6);
    }

    #[test]
    fn swept_from_inside_points_the_short_way_out() {
        let wall = Rect::new(0.0, 0.0, 100.0, 10.0);
        let stuck = Rect::new(40.0, 7.0, 4.0, 4.0); // overlaps the wall's bottom edge by 3 pixels
        let hit = swept_aabb(stuck, vec2(1.0, 0.0), wall).unwrap();
        assert_eq!(
            hit,
            Hit {
                time: 0.0,
                normal: vec2(0.0, 1.0)
            },
            "out through the bottom"
        );
    }

    #[test]
    fn approach_and_remap() {
        assert_eq!(approach(0.0, 10.0, 3.0), 3.0);
        assert_eq!(approach(9.0, 10.0, 3.0), 10.0, "no overshoot");
        assert_eq!(approach(10.0, 0.0, 3.0), 7.0, "downwards too");
        assert_eq!(approach(1.0, 0.0, 3.0), 0.0);
        assert_eq!(approach(5.0, 5.0, 3.0), 5.0);
        assert_eq!(
            approach(0.0, 10.0, -3.0),
            3.0,
            "a negative step counts as positive"
        );
        assert_eq!(remap(0.5, (0.0, 1.0), (10.0, 20.0)), 15.0);
        assert_eq!(remap(2.0, (0.0, 1.0), (10.0, 20.0)), 30.0, "not clamped");
        assert_eq!(remap(3.0, (3.0, 3.0), (7.0, 9.0)), 7.0, "empty range");
        assert_eq!(remap(-1.0, (-1.0, 1.0), (1.0, -1.0)), 1.0);
    }

    #[test]
    fn vector_arithmetic() {
        let a = vec2(1.0, 2.0);
        let b = vec2(3.0, 5.0);
        assert_eq!(a + b, vec2(4.0, 7.0));
        assert_eq!(b - a, vec2(2.0, 3.0));
        assert_eq!(a * 2.0, vec2(2.0, 4.0));
        assert_eq!(-a, vec2(-1.0, -2.0));

        let mut c = a;
        c += b;
        c -= vec2(1.0, 1.0);
        assert_eq!(c, vec2(3.0, 6.0));
    }

    #[test]
    fn length_and_normalize() {
        assert_eq!(vec2(3.0, 4.0).length(), 5.0);
        assert_eq!(vec2(0.0, 10.0).normalized(), vec2(0.0, 1.0));
        assert_eq!(Vec2::ZERO.normalized(), Vec2::ZERO);
        assert_eq!(vec2(1.0, 1.0).distance(vec2(4.0, 5.0)), 5.0);
    }

    #[test]
    fn rect_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(a.overlaps(&Rect::new(5.0, 5.0, 10.0, 10.0)));
        assert!(a.overlaps(&Rect::new(2.0, 2.0, 2.0, 2.0))); // fully inside
        assert!(!a.overlaps(&Rect::new(10.0, 0.0, 5.0, 5.0))); // touching edge
        assert!(!a.overlaps(&Rect::new(0.0, 20.0, 5.0, 5.0))); // below
    }

    #[test]
    fn rect_helpers() {
        let r = Rect::from_center(vec2(10.0, 10.0), 4.0, 6.0);
        assert_eq!(r, Rect::new(8.0, 7.0, 4.0, 6.0));
        assert_eq!(r.center(), vec2(10.0, 10.0));
        assert!(r.contains(vec2(8.0, 7.0)));
        assert!(!r.contains(vec2(12.0, 10.0)));
    }

    /// Floats are rarely exactly equal after trigonometry, so compare with a tolerance.
    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-5
    }

    #[test]
    fn vec3_arithmetic() {
        let a = vec3(1.0, 2.0, 3.0);
        assert_eq!(a + Vec3::ONE, vec3(2.0, 3.0, 4.0));
        assert_eq!(a - Vec3::ONE, vec3(0.0, 1.0, 2.0));
        assert_eq!(a * 2.0, vec3(2.0, 4.0, 6.0));
        assert_eq!(-a, vec3(-1.0, -2.0, -3.0));
        assert_eq!(a.dot(vec3(1.0, 0.0, 1.0)), 4.0);
        assert_eq!(vec3(0.0, 3.0, 4.0).length(), 5.0);
        assert_eq!(vec3(0.0, 0.0, 9.0).normalized(), vec3(0.0, 0.0, 1.0));
    }

    #[test]
    fn cross_product_is_perpendicular() {
        let x = vec3(1.0, 0.0, 0.0);
        let y = vec3(0.0, 1.0, 0.0);
        assert_eq!(x.cross(y), vec3(0.0, 0.0, 1.0));
        let c = vec3(1.0, 2.0, 3.0).cross(vec3(-4.0, 0.5, 2.0));
        assert!(c.dot(vec3(1.0, 2.0, 3.0)).abs() < 1e-5);
    }

    #[test]
    fn rotations_turn_the_documented_way() {
        let forward = vec3(0.0, 0.0, 1.0);
        let quarter = std::f32::consts::FRAC_PI_2;
        assert!(close(forward.rotate_y(quarter), vec3(1.0, 0.0, 0.0))); // turn right
        assert!(close(forward.rotate_x(quarter), vec3(0.0, 1.0, 0.0))); // tilt up
        assert!(close(vec3(1.0, 0.0, 0.0).rotate_z(quarter), Vec3::UP));
        // Rotating back undoes a rotation.
        let v = vec3(1.0, 2.0, 3.0);
        assert!(close(v.rotate_y(0.7).rotate_y(-0.7), v));
    }

    #[test]
    fn vector_trait_works_for_both_sizes() {
        assert_eq!(Vec2::unit(1), vec2(0.0, 1.0));
        assert_eq!(Vec3::unit(2), vec3(0.0, 0.0, 1.0));
        assert_eq!(vec3(1.0, 2.0, 3.0).with(1, 9.0), vec3(1.0, 9.0, 3.0));
        assert_eq!(vec2(4.0, 5.0).get(0), 4.0);
        assert_eq!(<Vec3 as Vector>::DIMENSIONS, 3);
    }

    #[test]
    fn matrix_rotations_match_vector_rotations() {
        let p = vec3(1.0, -2.0, 3.0);
        for angle in [0.3, -1.2, 2.5] {
            assert!(close(
                Mat4::rotation_x(angle).transform_point(p),
                p.rotate_x(angle)
            ));
            assert!(close(
                Mat4::rotation_y(angle).transform_point(p),
                p.rotate_y(angle)
            ));
            assert!(close(
                Mat4::rotation_z(angle).transform_point(p),
                p.rotate_z(angle)
            ));
        }
    }

    #[test]
    fn matrix_multiplication_applies_right_to_left() {
        let p = vec3(1.0, 1.0, 1.0);
        let m = Mat4::translation(vec3(10.0, 0.0, 0.0)) * Mat4::scaling(vec3(2.0, 3.0, 4.0));
        // Scale first (to 2, 3, 4), then move.
        assert!(close(m.transform_point(p), vec3(12.0, 3.0, 4.0)));
        assert_eq!(Mat4::IDENTITY * m, m);
    }

    #[test]
    fn perspective_depth_is_near_over_distance() {
        let m = Mat4::perspective(std::f32::consts::FRAC_PI_2, 2.0, 0.1);
        let [x, y, z, w] = m.transform([4.0, 3.0, 10.0, 1.0]);
        // The GPU divides everything by w (the distance).
        assert!((x / w - 0.2).abs() < 1e-6); // 4 / 10, halved by the 2:1 aspect
        assert!((y / w - 0.3).abs() < 1e-6);
        assert!((z / w - 0.01).abs() < 1e-6); // near / distance
    }
}
