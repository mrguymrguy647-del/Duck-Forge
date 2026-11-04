//! A small physics engine: balls and boxes that fall, bounce, slide and stack.
//!
//! It works in **2D and 3D** with the same code. Everything is generic over
//! `V: Vector`, and `V` is either [`Vec2`](crate::math::Vec2) or
//! [`Vec3`](crate::math::Vec3). Rust works out which one you mean from the
//! gravity you pass in:
//!
//! ```
//! use duckforge::physics::{Body, PhysicsWorld};
//! use duckforge::prelude::*;
//!
//! // 2D, in pixels. Screen y points down, so gravity is +y.
//! let mut world = PhysicsWorld::new(vec2(0.0, 500.0));
//! let floor = world.add(Body::block(vec2(160.0, 230.0), vec2(320.0, 20.0)).fixed());
//! let ball = world.add(Body::ball(vec2(160.0, 20.0), 8.0).with_bounce(0.6));
//!
//! for _ in 0..300 {
//!     world.step(1.0 / 60.0); // five seconds
//! }
//! let ball = world.get(ball).unwrap();
//! assert!((ball.position.y - 212.0).abs() < 1.0); // resting on the floor
//! ```
//!
//! Each call to [`PhysicsWorld::step`] runs a few small *substeps*, and each
//! substep:
//!
//! 1. lets gravity speed up every body,
//! 2. finds every pair of bodies that overlap (a *contact*),
//! 3. changes their velocities so they stop moving into each other, bouncing
//!    and applying friction (the *solver*),
//! 4. moves every body by its velocity,
//! 5. pushes apart bodies that still overlap.
//!
//! To keep the math simple, boxes are *axis-aligned*: they never rotate.
//!
//! # Units
//!
//! In 2D, one unit is one canvas **pixel**, and y points down. A 16-pixel
//! player, speeds of a few hundred pixels per second, and gravity of about
//! `vec2(0.0, 500.0)` to `vec2(0.0, 1000.0)` feel right on a 320 x 240
//! canvas. (Real gravity at 32 pixels per metre would be 9.8 x 32 = 314
//! px/s², which feels floaty in a game.)
//!
//! In 3D, one unit is one **metre**, and y points up. A person is about
//! 1.8 tall, a crate 1.0, and gravity is `vec3(0.0, -9.8, 0.0)`. The 3D
//! camera's controls and the meshes' default sizes assume this scale.
//!
//! The physics code works the same in any units; these are the scales the
//! examples and defaults are tuned for.

use std::collections::HashSet;
use std::f32::consts::PI;

use crate::math::Vector;

/// Each step is split into this many smaller steps. Smaller steps are more
/// accurate: fast objects are less likely to pass through thin walls.
const SUBSTEPS: usize = 4;

/// How many times per substep the solver goes over all the contacts. Each
/// pass fixes one contact but can disturb its neighbors, so it repeats until
/// things settle. More passes make stacks steadier.
const SOLVER_ITERATIONS: usize = 8;

/// How much of an overlap to undo per substep. Undoing all of it at once
/// makes resting objects jitter.
const POSITION_CORRECTION: f32 = 0.8;

/// The shape of a body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape<V> {
    /// A circle in 2D, a sphere in 3D.
    Ball { radius: f32 },
    /// A rectangle in 2D, a box in 3D. `half_size` is half the width and
    /// height (and depth): the distance from the center to each side.
    Block { half_size: V },
}

impl<V: Vector> Shape<V> {
    /// The same shape with every size made positive. A negative size can only
    /// be a mistake, and treating it as positive is the kindest guess.
    fn absolute(self) -> Self {
        match self {
            Shape::Ball { radius } => Shape::Ball {
                radius: radius.abs(),
            },
            Shape::Block { mut half_size } => {
                for axis in 0..V::DIMENSIONS {
                    half_size = half_size.with(axis, half_size.get(axis).abs());
                }
                Shape::Block { half_size }
            }
        }
    }

    /// The area (in 2D) or volume (in 3D). Used for a body's default mass.
    fn volume(&self) -> f32 {
        match *self {
            Shape::Ball { radius } => {
                if V::DIMENSIONS == 2 {
                    PI * radius * radius
                } else {
                    4.0 / 3.0 * PI * radius * radius * radius
                }
            }
            Shape::Block { half_size } => (0..V::DIMENSIONS)
                .map(|axis| 2.0 * half_size.get(axis))
                .product(),
        }
    }
}

/// Identifies one body in a [`PhysicsWorld`]. You get one from
/// [`PhysicsWorld::add`] and use it to look the body up later.
///
/// Inside, it's the body's slot in the world's list plus a *generation*.
/// When a body is removed, its slot is reused for the next body added, and
/// the slot's generation goes up by one, so an id for the old body never
/// finds the new one. Because the fields are private, an id can't be mixed
/// up with any other number, or made up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BodyId {
    slot: usize,
    generation: u32,
}

/// One object in the physics world.
///
/// Build one with [`Body::ball`] or [`Body::block`], then adjust it with the
/// `with_...` methods:
///
/// ```
/// # use duckforge::physics::Body;
/// # use duckforge::prelude::*;
/// let bouncy = Body::ball(vec2(10.0, 10.0), 5.0).with_bounce(0.9).with_friction(0.1);
/// let wall = Body::block(vec2(0.0, 100.0), vec2(10.0, 200.0)).fixed();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Body<V> {
    /// The center of the body.
    pub position: V,
    /// How fast it's moving, in units per second.
    pub velocity: V,
    pub shape: Shape<V>,
    /// Bounciness, from 0.0 (a thud) to 1.0 (bounces back at full speed).
    pub bounce: f32,
    /// Grip, from 0.0 (slippery like ice) to 1.0 (grippy like rubber).
    pub friction: f32,
    /// `1 / mass`, or 0.0 for fixed bodies. Physics engines store this
    /// instead of the mass because "infinitely heavy" becomes a simple 0.0.
    inverse_mass: f32,
}

impl<V: Vector> Body<V> {
    fn new(position: V, shape: Shape<V>) -> Self {
        Self {
            position,
            velocity: V::default(),
            shape,
            bounce: 0.2,
            friction: 0.4,
            inverse_mass: 1.0 / shape.volume().max(f32::MIN_POSITIVE),
        }
    }

    /// A ball (circle or sphere) centered on `position`. A negative radius
    /// counts as positive.
    pub fn ball(position: V, radius: f32) -> Self {
        Self::new(position, Shape::Ball { radius }.absolute())
    }

    /// A box centered on `position`. `size` is its full width and height
    /// (and depth, in 3D). A negative size counts as positive: a box can't
    /// be inside out.
    pub fn block(position: V, size: V) -> Self {
        let half_size = size * 0.5;
        Self::new(position, Shape::Block { half_size }.absolute())
    }

    /// Makes the body *fixed*: gravity and collisions never move it, like a
    /// floor or a wall. You can still move it yourself, by changing its
    /// `position` or giving it a `velocity` (for a moving platform).
    pub fn fixed(mut self) -> Self {
        self.inverse_mass = 0.0;
        self
    }

    pub fn with_velocity(mut self, velocity: V) -> Self {
        self.velocity = velocity;
        self
    }

    pub fn with_bounce(mut self, bounce: f32) -> Self {
        self.bounce = bounce;
        self
    }

    pub fn with_friction(mut self, friction: f32) -> Self {
        self.friction = friction;
        self
    }

    /// Sets the mass. By default a body weighs as much as its area (2D) or
    /// volume (3D), so bigger things are heavier.
    ///
    /// # Panics
    /// If `mass` isn't positive. Use [`Body::fixed`] for immovable bodies.
    pub fn with_mass(mut self, mass: f32) -> Self {
        assert!(
            mass > 0.0,
            "mass must be positive; use .fixed() for immovable bodies"
        );
        self.inverse_mass = 1.0 / mass;
        self
    }

    /// Is this a fixed body?
    pub fn is_fixed(&self) -> bool {
        self.inverse_mass == 0.0
    }

    /// The body's mass (infinite for fixed bodies).
    pub fn mass(&self) -> f32 {
        if self.is_fixed() {
            f32::INFINITY
        } else {
            1.0 / self.inverse_mass
        }
    }

    /// Gives the body a sudden push, like a kick or an explosion. Heavy
    /// bodies speed up less than light ones. Fixed bodies ignore it.
    pub fn apply_impulse(&mut self, impulse: V) {
        self.velocity += impulse * self.inverse_mass;
    }
}

/// Two bodies that touched during the last [`PhysicsWorld::step`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Contact<V> {
    pub a: BodyId,
    pub b: BodyId,
    /// The direction from `a` towards `b` (length 1).
    pub normal: V,
    /// How far they overlapped.
    pub depth: f32,
}

/// A contact plus the extra numbers the solver keeps while working on it.
struct SolverContact<V> {
    a: usize,
    b: usize,
    normal: V,
    depth: f32,
    /// The speed the bodies should end up separating at (from bouncing).
    target_speed: f32,
    friction: f32,
    /// The total push applied so far along the normal, and along the surface.
    normal_impulse: f32,
    friction_impulse: V,
}

/// A place in the world's list of bodies. See [`BodyId`].
struct Slot<V> {
    generation: u32,
    body: Option<Body<V>>,
}

/// A collection of bodies that move and collide together.
pub struct PhysicsWorld<V> {
    /// The acceleration applied to every non-fixed body, in units per second per second.
    pub gravity: V,
    /// Every body, in a slot. A removed body leaves an empty slot, which the
    /// next `add` reuses, so the list never grows beyond the most bodies
    /// there have ever been at once.
    bodies: Vec<Slot<V>>,
    /// The numbers of the empty slots, ready for reuse.
    free_slots: Vec<usize>,
    /// How many slots hold a body.
    live: usize,
    contacts: Vec<Contact<V>>,
}

impl<V: Vector> PhysicsWorld<V> {
    /// An empty world. Try `vec2(0.0, 500.0)` for a 2D game in pixels (y
    /// points down), or `vec3(0.0, -9.8, 0.0)` for 3D in meters (y points up).
    pub fn new(gravity: V) -> Self {
        Self {
            gravity,
            bodies: Vec::new(),
            free_slots: Vec::new(),
            live: 0,
            contacts: Vec::new(),
        }
    }

    /// The id of whatever is in slot `slot` now.
    fn id_of(&self, slot: usize) -> BodyId {
        BodyId {
            slot,
            generation: self.bodies[slot].generation,
        }
    }

    /// The slot for `id`, if it still holds that body.
    fn slot(&self, id: BodyId) -> Option<&Slot<V>> {
        self.bodies
            .get(id.slot)
            .filter(|slot| slot.generation == id.generation)
    }

    /// Adds a body and returns its id.
    pub fn add(&mut self, body: Body<V>) -> BodyId {
        self.live += 1;
        if let Some(slot) = self.free_slots.pop() {
            self.bodies[slot].body = Some(body);
            self.id_of(slot)
        } else {
            self.bodies.push(Slot {
                generation: 0,
                body: Some(body),
            });
            self.id_of(self.bodies.len() - 1)
        }
    }

    /// Removes a body, handing it back. Returns `None` if it was already gone.
    pub fn remove(&mut self, id: BodyId) -> Option<Body<V>> {
        self.slot(id)?;
        let body = self.empty_slot(id.slot)?;
        self.contacts.retain(|c| c.a != id && c.b != id);
        Some(body)
    }

    /// Takes the body out of a slot and makes the slot reusable.
    fn empty_slot(&mut self, slot: usize) -> Option<Body<V>> {
        let body = self.bodies[slot].body.take()?;
        // Old ids for this slot must never find the next body put here.
        self.bodies[slot].generation = self.bodies[slot].generation.wrapping_add(1);
        self.free_slots.push(slot);
        self.live -= 1;
        Some(body)
    }

    /// Removes every body for which `keep` returns `false`.
    pub fn retain(&mut self, mut keep: impl FnMut(BodyId, &Body<V>) -> bool) {
        for slot in 0..self.bodies.len() {
            let id = self.id_of(slot);
            if self.bodies[slot]
                .body
                .as_ref()
                .is_some_and(|body| !keep(id, body))
            {
                self.empty_slot(slot);
            }
        }
        // Forget contacts involving bodies that are gone.
        let bodies = &self.bodies;
        let alive = |id: BodyId| {
            bodies
                .get(id.slot)
                .is_some_and(|slot| slot.generation == id.generation && slot.body.is_some())
        };
        self.contacts.retain(|c| alive(c.a) && alive(c.b));
    }

    /// Looks up a body to read it.
    pub fn get(&self, id: BodyId) -> Option<&Body<V>> {
        self.slot(id)?.body.as_ref()
    }

    /// Looks up a body to change it.
    pub fn get_mut(&mut self, id: BodyId) -> Option<&mut Body<V>> {
        self.bodies
            .get_mut(id.slot)
            .filter(|slot| slot.generation == id.generation)?
            .body
            .as_mut()
    }

    /// Every body in the world, with its id.
    pub fn bodies(&self) -> impl Iterator<Item = (BodyId, &Body<V>)> {
        self.bodies.iter().enumerate().filter_map(|(slot, s)| {
            let id = BodyId {
                slot,
                generation: s.generation,
            };
            s.body.as_ref().map(|body| (id, body))
        })
    }

    /// How many bodies are in the world.
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every pair of bodies that touched during the last step.
    pub fn contacts(&self) -> &[Contact<V>] {
        &self.contacts
    }

    /// Did these two bodies touch during the last step?
    pub fn touching(&self, a: BodyId, b: BodyId) -> bool {
        self.contacts
            .iter()
            .any(|c| (c.a == a && c.b == b) || (c.a == b && c.b == a))
    }

    /// Moves the simulation forward by `dt` seconds. Call it once per frame
    /// with `ctx.dt()`.
    ///
    /// A `dt` that is zero, negative, infinite or not a number is ignored:
    /// there's no sensible way to step by it, and a NaN would spread into
    /// every body's position for good.
    pub fn step(&mut self, dt: f32) {
        self.contacts.clear();
        if !(dt.is_finite() && dt > 0.0) {
            return;
        }
        let h = dt / SUBSTEPS as f32;
        let mut touched = Vec::new();
        for _ in 0..SUBSTEPS {
            self.substep(h, &mut touched);
        }

        // A pair may have touched in several substeps; report it once.
        let mut seen = HashSet::new();
        for contact in touched {
            if seen.insert((contact.a, contact.b)) {
                self.contacts.push(contact);
            }
        }
    }

    fn substep(&mut self, h: f32, touched: &mut Vec<Contact<V>>) {
        // 1. Gravity changes velocities. (`filter_map` skips empty slots.)
        for body in self.bodies.iter_mut().filter_map(|slot| slot.body.as_mut()) {
            if !body.is_fixed() {
                body.velocity += self.gravity * h;
            }
        }

        // 2. Find overlapping pairs.
        let mut contacts = self.find_contacts(h);

        // 3. Fix the velocities, a few passes over all contacts.
        for _ in 0..SOLVER_ITERATIONS {
            for contact in &mut contacts {
                let (a, b) = pair_mut(&mut self.bodies, contact.a, contact.b);
                solve_velocity(a, b, contact);
            }
        }

        // 4. Move.
        for body in self.bodies.iter_mut().filter_map(|slot| slot.body.as_mut()) {
            body.position += body.velocity * h;
        }

        // 5. Push apart whatever still overlaps after moving.
        for contact in &contacts {
            let (a, b) = pair_mut(&mut self.bodies, contact.a, contact.b);
            separate(a, b);
        }

        touched.extend(contacts.iter().map(|c| Contact {
            a: self.id_of(c.a),
            b: self.id_of(c.b),
            normal: c.normal,
            depth: c.depth,
        }));
    }

    /// Checks every pair of bodies (this is called the *narrow phase*; big
    /// engines first skip far-apart pairs cheaply, in a *broad phase*).
    fn find_contacts(&self, h: f32) -> Vec<SolverContact<V>> {
        // Slower than this counts as resting, not hitting: no bounce. It's
        // about the speed gravity adds in one substep, so resting objects
        // don't jitter.
        let resting_speed = 3.0 * self.gravity.dot(self.gravity).sqrt() * h;

        let mut contacts = Vec::new();
        for (i, slot_a) in self.bodies.iter().enumerate() {
            let Some(a) = &slot_a.body else { continue };
            for (j, slot_b) in self.bodies.iter().enumerate().skip(i + 1) {
                let Some(b) = &slot_b.body else { continue };
                if a.is_fixed() && b.is_fixed() {
                    continue; // walls never need to collide with walls
                }
                let Some((normal, depth)) = collide(a, b) else {
                    continue;
                };

                // How fast are they coming together? (Negative = approaching.)
                let approach = (b.velocity - a.velocity).dot(normal);
                let bounce = a.bounce.max(b.bounce);
                let target_speed = if approach < -resting_speed {
                    -approach * bounce
                } else {
                    0.0
                };

                contacts.push(SolverContact {
                    a: i,
                    b: j,
                    normal,
                    depth,
                    target_speed,
                    friction: (a.friction * b.friction).sqrt(),
                    normal_impulse: 0.0,
                    friction_impulse: V::default(),
                });
            }
        }
        contacts
    }
}

/// Borrows two different items of a slice mutably at the same time.
///
/// Writing `(&mut items[i], &mut items[j])` doesn't compile: that's two
/// mutable borrows of `items`, and the compiler can't tell that `i != j`.
/// `split_at_mut` cuts the slice into two halves that don't overlap, which
/// the compiler *can* hand out separately.
fn pair_mut<V>(bodies: &mut [Slot<V>], i: usize, j: usize) -> (&mut Body<V>, &mut Body<V>) {
    assert!(i < j);
    let (left, right) = bodies.split_at_mut(j);
    let a = left[i]
        .body
        .as_mut()
        .expect("contact refers to a removed body");
    let b = right[0]
        .body
        .as_mut()
        .expect("contact refers to a removed body");
    (a, b)
}

/// Changes the velocities of two touching bodies so they stop moving into
/// each other (and bounce), and so friction slows any sliding.
///
/// It works with *impulses*: instant changes in momentum. An impulse `j`
/// changes a body's velocity by `j / mass`, so light bodies react more.
fn solve_velocity<V: Vector>(a: &mut Body<V>, b: &mut Body<V>, contact: &mut SolverContact<V>) {
    let total_inverse_mass = a.inverse_mass + b.inverse_mass;
    if total_inverse_mass == 0.0 {
        return;
    }
    let n = contact.normal;

    // --- Along the normal: stop them pushing into each other. ---
    let speed = (b.velocity - a.velocity).dot(n);
    let impulse = (contact.target_speed - speed) / total_inverse_mass;
    // Contacts can push but never pull, so the *total* impulse can't go below 0.
    let new_total = (contact.normal_impulse + impulse).max(0.0);
    let impulse = new_total - contact.normal_impulse;
    contact.normal_impulse = new_total;
    a.velocity -= n * (impulse * a.inverse_mass);
    b.velocity += n * (impulse * b.inverse_mass);

    // --- Along the surface: friction. ---
    let relative = b.velocity - a.velocity;
    let sliding = relative - n * relative.dot(n);
    // The impulse that would stop the sliding completely...
    let mut total = contact.friction_impulse - sliding * (1.0 / total_inverse_mass);
    // ...but friction can't be stronger than `friction x how hard they press together`.
    let limit = contact.friction * contact.normal_impulse;
    let length = total.dot(total).sqrt();
    if length > limit {
        total = total * (limit / length);
    }
    let impulse = total - contact.friction_impulse;
    contact.friction_impulse = total;
    a.velocity -= impulse * a.inverse_mass;
    b.velocity += impulse * b.inverse_mass;
}

/// If two bodies overlap, moves them apart (most of the way). Heavier
/// bodies move less, and fixed bodies don't move at all.
fn separate<V: Vector>(a: &mut Body<V>, b: &mut Body<V>) {
    let total_inverse_mass = a.inverse_mass + b.inverse_mass;
    if total_inverse_mass == 0.0 {
        return;
    }
    if let Some((normal, depth)) = collide(a, b) {
        let correction = normal * (depth * POSITION_CORRECTION / total_inverse_mass);
        a.position -= correction * a.inverse_mass;
        b.position += correction * b.inverse_mass;
    }
}

/// Do two bodies overlap? If so, returns the direction from `a` to `b` and
/// how deep the overlap is.
fn collide<V: Vector>(a: &Body<V>, b: &Body<V>) -> Option<(V, f32)> {
    // `shape` is a public field, so a game could have set a negative size.
    match (a.shape.absolute(), b.shape.absolute()) {
        (Shape::Ball { radius: ra }, Shape::Ball { radius: rb }) => {
            ball_vs_ball(a.position, ra, b.position, rb)
        }
        (Shape::Block { half_size: ha }, Shape::Block { half_size: hb }) => {
            block_vs_block(a.position, ha, b.position, hb)
        }
        (Shape::Block { half_size }, Shape::Ball { radius }) => {
            ball_vs_block(b.position, radius, a.position, half_size)
        }
        (Shape::Ball { radius }, Shape::Block { half_size }) => {
            // Same test with the roles swapped, so flip the direction.
            ball_vs_block(a.position, radius, b.position, half_size)
                .map(|(normal, depth)| (-normal, depth))
        }
    }
}

/// Two balls overlap when their centers are closer than their radii added together.
fn ball_vs_ball<V: Vector>(a: V, radius_a: f32, b: V, radius_b: f32) -> Option<(V, f32)> {
    let offset = b - a;
    let distance_squared = offset.dot(offset);
    let reach = radius_a + radius_b;
    if distance_squared >= reach * reach {
        return None;
    }
    let distance = distance_squared.sqrt();
    let normal = if distance > 1e-6 {
        offset * (1.0 / distance)
    } else {
        V::unit(1) // exactly on top of each other: pick any direction
    };
    Some((normal, reach - distance))
}

/// Two boxes overlap only if they overlap along *every* axis. They are pushed
/// apart along the axis where they overlap least, the shortest way out.
fn block_vs_block<V: Vector>(a: V, half_a: V, b: V, half_b: V) -> Option<(V, f32)> {
    let offset = b - a;
    let mut best_axis = 0;
    let mut best_overlap = f32::INFINITY;
    for axis in 0..V::DIMENSIONS {
        let overlap = half_a.get(axis) + half_b.get(axis) - offset.get(axis).abs();
        if overlap <= 0.0 {
            return None; // a gap along this axis: no collision
        }
        if overlap < best_overlap {
            best_axis = axis;
            best_overlap = overlap;
        }
    }
    let sign = if offset.get(best_axis) < 0.0 {
        -1.0
    } else {
        1.0
    };
    Some((V::unit(best_axis) * sign, best_overlap))
}

/// A ball and a box. Returns the direction from the **box to the ball**.
fn ball_vs_block<V: Vector>(ball: V, radius: f32, block: V, half: V) -> Option<(V, f32)> {
    // The ball's center, relative to the box's center.
    let offset = ball - block;

    // The point of the box closest to the ball's center: clamp each
    // coordinate to the box's extent. (`max` then `min`, not `clamp`, which
    // would panic if a size were NaN.)
    let mut closest = offset;
    for axis in 0..V::DIMENSIONS {
        let limit = half.get(axis);
        closest = closest.with(axis, offset.get(axis).max(-limit).min(limit));
    }

    if closest != offset {
        // The center is outside the box: compare the distance to the closest point.
        let gap = offset - closest;
        let distance_squared = gap.dot(gap);
        if distance_squared >= radius * radius {
            return None;
        }
        let distance = distance_squared.sqrt();
        return Some((gap * (1.0 / distance), radius - distance));
    }

    // The center is inside the box: push the ball out through the nearest side.
    let mut best_axis = 0;
    let mut best_room = f32::INFINITY;
    for axis in 0..V::DIMENSIONS {
        let room = half.get(axis) - offset.get(axis).abs();
        if room < best_room {
            best_axis = axis;
            best_room = room;
        }
    }
    let sign = if offset.get(best_axis) < 0.0 {
        -1.0
    } else {
        1.0
    };
    Some((V::unit(best_axis) * sign, radius + best_room))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::{Vec2, Vec3, vec2, vec3};

    const DT: f32 = 1.0 / 60.0;

    fn run(world: &mut PhysicsWorld<impl Vector>, seconds: f32) {
        for _ in 0..(seconds / DT) as usize {
            world.step(DT);
        }
    }

    /// A world with a fixed floor whose top edge is at y = 200 (2D, y down).
    fn world_with_floor() -> (PhysicsWorld<Vec2>, BodyId) {
        let mut world = PhysicsWorld::new(vec2(0.0, 500.0));
        let floor = world.add(Body::block(vec2(0.0, 210.0), vec2(1000.0, 20.0)).fixed());
        (world, floor)
    }

    #[test]
    fn ball_comes_to_rest_on_the_floor() {
        let (mut world, floor) = world_with_floor();
        let ball = world.add(Body::ball(vec2(0.0, 0.0), 10.0));
        run(&mut world, 4.0);

        let b = world.get(ball).unwrap();
        assert!((b.position.y - 190.0).abs() < 0.5, "y = {}", b.position.y);
        assert!(b.velocity.length() < 1.0, "still moving: {:?}", b.velocity);
        assert!(world.touching(ball, floor));
        assert!(world.touching(floor, ball));
        // The fixed floor didn't move.
        assert_eq!(world.get(floor).unwrap().position, vec2(0.0, 210.0));
    }

    #[test]
    fn bouncy_ball_bounces_high() {
        let (mut world, _) = world_with_floor();
        // Dropped from 100 pixels above its resting height.
        let ball = world.add(Body::ball(vec2(0.0, 90.0), 10.0).with_bounce(0.8));

        let mut hit_floor = false;
        let mut highest_after_bounce = f32::INFINITY;
        for _ in 0..120 {
            world.step(DT);
            let b = world.get(ball).unwrap();
            if b.velocity.y < 0.0 {
                hit_floor = true;
            }
            if hit_floor {
                highest_after_bounce = highest_after_bounce.min(b.position.y);
            }
        }
        // 0.8 bounce keeps 0.8 x 0.8 = 64% of the height in theory.
        let height = 190.0 - highest_after_bounce;
        assert!(height > 50.0 && height < 75.0, "bounced {height} px");
    }

    #[test]
    fn equal_balls_swap_speeds_in_a_perfect_bounce() {
        let mut world = PhysicsWorld::new(Vec2::ZERO);
        let a = world.add(
            Body::ball(vec2(0.0, 0.0), 5.0)
                .with_velocity(vec2(100.0, 0.0))
                .with_bounce(1.0),
        );
        let b = world.add(Body::ball(vec2(30.0, 0.0), 5.0).with_bounce(1.0));
        run(&mut world, 0.5);

        let (va, vb) = (
            world.get(a).unwrap().velocity,
            world.get(b).unwrap().velocity,
        );
        assert!(va.x.abs() < 1.0, "a should stop: {va:?}");
        assert!((vb.x - 100.0).abs() < 1.0, "b takes a's speed: {vb:?}");
    }

    #[test]
    fn a_stack_of_boxes_stays_up() {
        let (mut world, _) = world_with_floor();
        let boxes: Vec<BodyId> = (0..4)
            .map(|i| {
                world.add(Body::block(
                    vec2(0.0, 190.0 - 20.0 * i as f32),
                    vec2(20.0, 20.0),
                ))
            })
            .collect();
        run(&mut world, 5.0);

        for (i, id) in boxes.iter().enumerate() {
            let b = world.get(*id).unwrap();
            let expected_y = 190.0 - 20.0 * i as f32;
            assert!(
                b.position.x.abs() < 0.5,
                "box {i} slid to x = {}",
                b.position.x
            );
            assert!(
                (b.position.y - expected_y).abs() < 1.5,
                "box {i} at y = {}",
                b.position.y
            );
        }
    }

    #[test]
    fn friction_stops_sliding() {
        let (mut world, _) = world_with_floor();
        let grippy = world.add(
            Body::block(vec2(0.0, 190.0), vec2(20.0, 20.0))
                .with_velocity(vec2(200.0, 0.0))
                .with_friction(0.8),
        );
        let icy = world.add(
            Body::block(vec2(0.0, 150.0), vec2(20.0, 20.0))
                .with_velocity(vec2(200.0, 0.0))
                .with_friction(0.0),
        );
        // Move the icy one far away so they don't bump into each other.
        world.get_mut(icy).unwrap().position = vec2(-400.0, 190.0);
        run(&mut world, 1.0);

        assert!(world.get(grippy).unwrap().velocity.x.abs() < 1.0);
        assert!((world.get(icy).unwrap().velocity.x - 200.0).abs() < 1.0);
    }

    #[test]
    fn heavy_things_push_light_things() {
        let mut world = PhysicsWorld::new(Vec2::ZERO);
        let heavy = world.add(
            Body::ball(vec2(0.0, 0.0), 5.0)
                .with_mass(100.0)
                .with_velocity(vec2(50.0, 0.0)),
        );
        let light = world.add(Body::ball(vec2(20.0, 0.0), 5.0).with_mass(1.0));
        run(&mut world, 0.5);
        assert!(world.get(heavy).unwrap().velocity.x > 40.0);
        assert!(world.get(light).unwrap().velocity.x > 50.0);
    }

    #[test]
    fn works_in_3d_too() {
        // 3D, in meters, with y pointing up.
        let mut world = PhysicsWorld::new(vec3(0.0, -9.8, 0.0));
        world.add(Body::block(Vec3::ZERO, vec3(10.0, 1.0, 10.0)).fixed()); // top at y = 0.5
        let ball = world.add(Body::ball(vec3(0.0, 3.0, 0.0), 0.5));
        let crate_ = world.add(Body::block(vec3(2.0, 3.0, 1.0), Vec3::ONE));
        run(&mut world, 3.0);

        assert!((world.get(ball).unwrap().position.y - 1.0).abs() < 0.02);
        assert!((world.get(crate_).unwrap().position.y - 1.0).abs() < 0.02);
    }

    #[test]
    fn ball_deep_inside_a_box_gets_pushed_out() {
        let (mut world, _) = world_with_floor();
        let ball = world.add(Body::ball(vec2(0.0, 205.0), 10.0)); // center inside the floor
        run(&mut world, 1.0);
        assert!(world.get(ball).unwrap().position.y < 191.0);
    }

    #[test]
    fn add_remove_and_retain() {
        let mut world = PhysicsWorld::new(Vec2::ZERO);
        let a = world.add(Body::ball(Vec2::ZERO, 1.0));
        let b = world.add(Body::ball(vec2(10.0, 0.0), 1.0));
        let c = world.add(Body::ball(vec2(20.0, 0.0), 1.0));
        assert_eq!(world.len(), 3);

        assert!(world.remove(a).is_some());
        assert!(world.remove(a).is_none());
        assert!(world.get(a).is_none());
        assert!(world.get(b).is_some()); // other ids still work

        world.retain(|_, body| body.position.x < 15.0);
        assert!(world.get(c).is_none());
        assert_eq!(
            world.bodies().map(|(id, _)| id).collect::<Vec<_>>(),
            vec![b]
        );
        world.step(DT); // stepping with removed bodies is fine
    }

    #[test]
    fn mass_and_impulses() {
        let mut body = Body::ball(Vec2::ZERO, 1.0).with_mass(2.0);
        assert_eq!(body.mass(), 2.0);
        body.apply_impulse(vec2(10.0, 0.0));
        assert_eq!(body.velocity, vec2(5.0, 0.0));

        let mut wall = Body::block(Vec2::ZERO, vec2(1.0, 1.0)).fixed();
        wall.apply_impulse(vec2(10.0, 0.0));
        assert_eq!(wall.velocity, Vec2::ZERO);
        assert!(wall.mass().is_infinite());
    }

    #[test]
    fn removed_slots_are_reused() {
        let mut world = PhysicsWorld::new(Vec2::ZERO);
        for _ in 0..10_000 {
            let id = world.add(Body::ball(Vec2::ZERO, 1.0));
            world.remove(id);
        }
        // Each step walks every slot, so they must not pile up.
        assert!(
            world.bodies.len() <= 1,
            "{} slots for 0 bodies",
            world.bodies.len()
        );
        assert!(world.is_empty());
    }

    #[test]
    fn old_ids_never_reach_a_new_body_in_the_same_slot() {
        let mut world = PhysicsWorld::new(Vec2::ZERO);
        let old = world.add(Body::ball(Vec2::ZERO, 1.0));
        world.remove(old);
        let new = world.add(Body::ball(vec2(5.0, 0.0), 1.0));
        assert_ne!(old, new);
        assert!(world.get(old).is_none());
        assert!(world.remove(old).is_none()); // must not remove the new body
        assert_eq!(world.get(new).unwrap().position, vec2(5.0, 0.0));
        assert_eq!(world.len(), 1);
    }

    #[test]
    fn removed_bodies_stop_touching() {
        let (mut world, floor) = world_with_floor();
        let ball = world.add(Body::ball(vec2(0.0, 190.0), 10.0));
        world.step(DT);
        assert!(world.touching(ball, floor));
        world.remove(ball);
        assert!(!world.touching(ball, floor));
        assert!(world.contacts().iter().all(|c| c.a != ball && c.b != ball));
    }

    #[test]
    fn bad_time_steps_are_ignored() {
        let (mut world, _) = world_with_floor();
        let ball = world.add(Body::ball(vec2(0.0, 0.0), 10.0));
        for dt in [f32::NAN, f32::INFINITY, -1.0, 0.0] {
            world.step(dt);
        }
        let b = world.get(ball).unwrap();
        assert_eq!((b.position, b.velocity), (Vec2::ZERO, Vec2::ZERO));
        world.step(DT); // and it still works afterwards
        assert!(world.get(ball).unwrap().position.y > 0.0);
    }

    #[test]
    fn negative_sizes_act_like_positive_ones() {
        let (mut world, _) = world_with_floor();
        let block = world.add(Body::block(vec2(-100.0, 0.0), vec2(-20.0, -20.0)));
        let ball = world.add(Body::ball(vec2(100.0, 0.0), -10.0));
        run(&mut world, 3.0);
        // Both come to rest on the floor (top at y = 200) instead of falling through.
        assert!((world.get(block).unwrap().position.y - 190.0).abs() < 1.0);
        assert!((world.get(ball).unwrap().position.y - 190.0).abs() < 1.0);
    }

    #[test]
    fn negative_sizes_set_directly_on_the_shape_do_not_crash() {
        let (mut world, _) = world_with_floor();
        let ball = world.add(Body::ball(vec2(0.0, 0.0), 10.0));
        world.get_mut(ball).unwrap().shape = Shape::Ball { radius: -10.0 };
        run(&mut world, 3.0);
        assert!((world.get(ball).unwrap().position.y - 190.0).abs() < 1.0);
    }
}
