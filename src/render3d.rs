//! 3D: cameras, meshes, transforms, and the CPU renderer.
//!
//! Meshes are drawn by the graphics card when there is one (see `gpu.rs`),
//! and otherwise by the CPU renderer at the bottom of this file. Both follow
//! the same steps to turn a 3D triangle into pixels on the screen:
//!
//! 1. **Model to world.** A [`Mesh`] is built around its own center. Its
//!    [`Transform`] scales it, rotates it and moves it to its place in the world.
//! 2. **Culling and lighting.** Triangles facing away from the camera are
//!    skipped. The rest are shaded by how directly they face the sun.
//! 3. **World to camera.** Everything is moved and rotated so the camera
//!    sits at (0, 0, 0) looking straight down the +z axis.
//! 4. **Projection.** Dividing x and y by the distance z makes far things
//!    smaller. That's all perspective is.
//! 5. **Rasterization.** The canvas fills the triangle's pixels, using the
//!    depth buffer so that near things hide far things.
//!
//! Directions: `x` is right, `y` is up, `z` is forward.

use std::f32::consts::TAU;

use crate::canvas::Canvas;
use crate::color::Color;
use crate::engine::Context;
use crate::input::{Key, MouseButton};
use crate::math::{Mat4, Vec2, Vec3, vec2, vec3};

/// Anything closer to the camera than this is cut off. Without it we'd
/// divide by zero (or by negative numbers, for things behind the camera).
const NEAR: f32 = 0.05;

/// How bright a triangle facing completely away from the sun is (0 to 1).
pub(crate) const AMBIENT_LIGHT: f32 = 0.35;

/// The direction the sunlight comes *from*: above, a little left, a little in front.
pub(crate) fn sun_direction() -> Vec3 {
    vec3(-0.4, 1.0, -0.6).normalized()
}

/// A camera: where you're looking from and in which direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera3D {
    pub position: Vec3,
    /// Turning left/right, in radians. 0 looks along +z, and positive turns right.
    pub yaw: f32,
    /// Looking up/down, in radians. 0 is level, and positive looks up.
    pub pitch: f32,
    /// How much you can see from top to bottom of the screen, in radians.
    /// Smaller zooms in.
    pub fov: f32,
}

impl Camera3D {
    /// A camera at `position` looking along +z.
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: 0.0,
            fov: 60f32.to_radians(),
        }
    }

    /// A camera at `position`, turned to look at `target`.
    pub fn looking_at(position: Vec3, target: Vec3) -> Self {
        let mut camera = Self::new(position);
        camera.look_at(target);
        camera
    }

    /// Turns the camera to look at `target`.
    pub fn look_at(&mut self, target: Vec3) {
        let d = target - self.position;
        self.yaw = d.x.atan2(d.z);
        self.pitch = d.y.atan2((d.x * d.x + d.z * d.z).sqrt());
    }

    /// The direction the camera is looking (length 1).
    pub fn forward(&self) -> Vec3 {
        vec3(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }

    /// The direction to the camera's right, level with the ground (length 1).
    pub fn right(&self) -> Vec3 {
        vec3(self.yaw.cos(), 0.0, -self.yaw.sin())
    }

    /// Ready-made controls for flying around a scene:
    ///
    /// - **W/A/S/D** move, **Q/E** move down/up, hold **Shift** to go faster
    /// - **arrow keys** turn and look up/down
    /// - **drag with the right mouse button** to look around
    ///
    /// `speed` is in units per second.
    pub fn fly(&mut self, ctx: &Context, speed: f32) {
        const TURN_SPEED: f32 = 2.0; // radians per second
        const MOUSE_SENSITIVITY: f32 = 0.01; // radians per canvas pixel

        let input = &ctx.input;
        let dt = ctx.dt();

        self.yaw += input.axis(Key::Left, Key::Right) * TURN_SPEED * dt;
        self.pitch += input.axis(Key::Down, Key::Up) * TURN_SPEED * dt;
        if input.is_mouse_down(MouseButton::Right) {
            let delta = input.mouse_delta();
            self.yaw += delta.x * MOUSE_SENSITIVITY;
            self.pitch -= delta.y * MOUSE_SENSITIVITY; // screen y points down
        }
        // Stop just short of straight up or down, where the view would flip over.
        let limit = 89f32.to_radians();
        self.pitch = self.pitch.clamp(-limit, limit);

        // W/S move along the ground in the direction we're facing, ignoring
        // pitch, so looking down doesn't make you fly into the floor.
        let level_forward = vec3(self.yaw.sin(), 0.0, self.yaw.cos());
        let direction = level_forward * input.axis(Key::S, Key::W)
            + self.right() * input.axis(Key::A, Key::D)
            + Vec3::UP * input.axis(Key::Q, Key::E);
        let boost = if input.is_down(Key::LeftShift) {
            3.0
        } else {
            1.0
        };
        self.position += direction.normalized() * speed * boost * dt;
    }

    /// Moves a point from world space into camera space, where the camera is
    /// at (0, 0, 0) looking along +z. This undoes the camera's position, then
    /// its yaw, then its pitch.
    pub fn to_camera_space(&self, point: Vec3) -> Vec3 {
        (point - self.position)
            .rotate_y(-self.yaw)
            .rotate_x(-self.pitch)
    }

    /// Where a point in the world appears on the canvas, or `None` if it's
    /// behind the camera. Handy for drawing 2D labels over 3D objects.
    pub fn world_to_screen(&self, point: Vec3, canvas: &Canvas) -> Option<Vec2> {
        let p = self.to_camera_space(point);
        if p.z < NEAR {
            return None;
        }
        Some(self.projection(canvas).project(p))
    }

    /// The field of view actually used: `fov` kept between 1 and 179 degrees
    /// (or 60 degrees if it isn't a number). At 0 or 180 degrees the
    /// projection would divide by zero, and beyond 180 the picture would turn
    /// upside down.
    fn usable_fov(&self) -> f32 {
        if self.fov.is_finite() {
            self.fov.clamp(1f32.to_radians(), 179f32.to_radians())
        } else {
            60f32.to_radians()
        }
    }

    /// The numbers needed to project points onto `canvas`. They're worked
    /// out once and then used for every corner, since `tan` isn't cheap.
    fn projection(&self, canvas: &Canvas) -> Projection {
        let (width, height) = (canvas.width() as f32, canvas.height() as f32);
        Projection {
            center: vec2(width / 2.0, height / 2.0),
            // How many pixels one unit covers at distance 1. A narrow field of
            // view gives a bigger number: that's zooming in.
            focal_length: (height / 2.0) / (self.usable_fov() / 2.0).tan(),
        }
    }

    /// [`Camera3D::to_camera_space`] as a matrix: undo the position, then
    /// the yaw, then the pitch. (Read the multiplication right to left.)
    pub fn view_matrix(&self) -> Mat4 {
        Mat4::rotation_x(-self.pitch)
            * Mat4::rotation_y(-self.yaw)
            * Mat4::translation(-self.position)
    }

    /// The perspective projection as a matrix, for a canvas `aspect` times
    /// as wide as it is tall. It maps the same points to the same pixels as
    /// the CPU renderer does.
    pub fn projection_matrix(&self, aspect: f32) -> Mat4 {
        Mat4::perspective(self.usable_fov(), aspect, NEAR)
    }
}

/// Turns camera-space points into canvas pixels (see [`Camera3D::projection`]).
struct Projection {
    center: Vec2,
    focal_length: f32,
}

impl Projection {
    /// The perspective divide: further away (bigger `z`) means closer to the middle.
    fn project(&self, p: Vec3) -> Vec2 {
        vec2(
            self.center.x + p.x / p.z * self.focal_length,
            self.center.y - p.y / p.z * self.focal_length, // minus: screen y points down
        )
    }
}

/// Reads the `DUCKFORGE_RENDERER` environment variable, if it's set:
/// `cpu`, `gpu` or `auto`. It lets you switch renderers without changing
/// code: in a window, and also in a headless run (which otherwise always
/// draws on the CPU, so it's the same on every computer), to compare the
/// two renderers' pictures.
pub(crate) fn renderer_from_env() -> Option<Renderer> {
    let value = std::env::var("DUCKFORGE_RENDERER").ok()?;
    match value.to_lowercase().as_str() {
        "cpu" => Some(Renderer::Cpu),
        "gpu" => Some(Renderer::Gpu),
        "auto" => Some(Renderer::Auto),
        other => {
            eprintln!("duckforge: ignoring DUCKFORGE_RENDERER={other:?} (use cpu, gpu or auto)");
            None
        }
    }
}

/// What draws 3D meshes. Set it with [`Config::renderer`](crate::engine::Config::renderer)
/// or [`Canvas::set_renderer`], or override it for any game with the
/// `DUCKFORGE_RENDERER` environment variable (`cpu`, `gpu` or `auto`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Renderer {
    /// The graphics card if there is a usable one, otherwise the CPU.
    #[default]
    Auto,
    /// The graphics card. (Still falls back to the CPU, with a message, if
    /// there isn't one.)
    Gpu,
    /// The CPU renderer in this file: slower, but you can read every step.
    Cpu,
}

/// Where an object is, how it's turned and how big it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    pub position: Vec3,
    /// Rotation around the x, y and z axes, in radians, applied in that order.
    pub rotation: Vec3,
    /// Size multiplier along each axis. The built-in meshes are 1 unit big,
    /// so `scale` is simply the object's size.
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        }
    }
}

impl Transform {
    /// An unrotated, 1-unit-big object at `position`.
    pub fn at(position: Vec3) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }

    /// The same transform with a different rotation.
    pub fn rotated(self, rotation: Vec3) -> Self {
        Self { rotation, ..self }
    }

    /// The same transform with a different size along each axis.
    pub fn scaled(self, scale: Vec3) -> Self {
        Self { scale, ..self }
    }

    /// The same transform with the same size along every axis.
    pub fn sized(self, size: f32) -> Self {
        self.scaled(Vec3::ONE * size)
    }

    /// Moves a point from the mesh's own space into the world: scale, then
    /// rotate, then move.
    pub fn apply(&self, point: Vec3) -> Vec3 {
        let scaled = vec3(
            point.x * self.scale.x,
            point.y * self.scale.y,
            point.z * self.scale.z,
        );
        scaled
            .rotate_x(self.rotation.x)
            .rotate_y(self.rotation.y)
            .rotate_z(self.rotation.z)
            + self.position
    }

    /// The same as [`Transform::apply`], packed into one matrix: scale, then
    /// rotate around x, y and z, then move. (Read right to left.)
    pub fn matrix(&self) -> Mat4 {
        let mut m = Mat4::scaling(self.scale);
        // Skipping rotations of zero saves time: most objects only turn one way.
        if self.rotation.x != 0.0 {
            m = Mat4::rotation_x(self.rotation.x) * m;
        }
        if self.rotation.y != 0.0 {
            m = Mat4::rotation_y(self.rotation.y) * m;
        }
        if self.rotation.z != 0.0 {
            m = Mat4::rotation_z(self.rotation.z) * m;
        }
        // Moving just means filling in the last column.
        m.columns[3] = [self.position.x, self.position.y, self.position.z, 1.0];
        m
    }
}

/// One triangle of a mesh: three indexes into the mesh's `vertices`, plus a color.
///
/// The corners must go **clockwise when you look at the front** of the
/// triangle. That's how the renderer knows which side is the front: it
/// only draws the front, so it can skip the insides of solid objects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Triangle {
    pub corners: [usize; 3],
    pub color: Color,
}

/// Looks up the three corner points of a mesh's triangle number `index`.
///
/// # Panics
/// If the triangle refers to a vertex the mesh doesn't have, with a message
/// that says which triangle. (Both renderers use this, so a broken mesh
/// fails the same clear way on the CPU and on the GPU.)
#[inline]
pub(crate) fn triangle_corners(points: &[Vec3], index: usize, triangle: &Triangle) -> [Vec3; 3] {
    let [a, b, c] = triangle.corners;
    match (points.get(a), points.get(b), points.get(c)) {
        (Some(&a), Some(&b), Some(&c)) => [a, b, c],
        _ => bad_triangle(points.len(), index, triangle),
    }
}

/// The panic for [`triangle_corners`], kept in its own function. `#[cold]`
/// tells the compiler this almost never runs, so it keeps the message-building
/// code out of the way of the fast path. (Measured: with the message inline,
/// the CPU renderer's stress test ran about 13% slower.)
#[cold]
#[inline(never)]
fn bad_triangle(vertex_count: usize, index: usize, triangle: &Triangle) -> ! {
    let missing = triangle
        .corners
        .into_iter()
        .find(|&corner| corner >= vertex_count);
    panic!(
        "triangle {index} uses vertex {}, but the mesh only has {vertex_count} vertices",
        missing.unwrap_or_default()
    )
}

/// A 3D shape made of triangles.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<Triangle>,
}

impl Mesh {
    /// An empty mesh, to build your own shape with `add_vertex` and `add_triangle`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a corner point and returns its index.
    pub fn add_vertex(&mut self, position: Vec3) -> usize {
        self.vertices.push(position);
        self.vertices.len() - 1
    }

    /// Adds a triangle. Its corners go clockwise when seen from the front.
    pub fn add_triangle(&mut self, a: usize, b: usize, c: usize, color: Color) {
        self.triangles.push(Triangle {
            corners: [a, b, c],
            color,
        });
    }

    /// Adds a four-sided face as two triangles. Its corners go clockwise when
    /// seen from the front.
    pub fn add_quad(&mut self, a: usize, b: usize, c: usize, d: usize, color: Color) {
        self.add_triangle(a, b, c, color);
        self.add_triangle(a, c, d, color);
    }

    /// Changes the color of every triangle.
    pub fn with_color(mut self, color: Color) -> Self {
        for triangle in &mut self.triangles {
            triangle.color = color;
        }
        self
    }

    /// A cube, 1 unit on each side, centered on (0, 0, 0).
    pub fn cube(color: Color) -> Mesh {
        let mut mesh = Mesh::new();
        let h = 0.5;
        // The eight corners. `v0..v3` are the face nearest a camera looking
        // along +z; `v4..v7` are the far face.
        let v: Vec<usize> = [
            vec3(-h, -h, -h),
            vec3(h, -h, -h),
            vec3(h, h, -h),
            vec3(-h, h, -h),
            vec3(-h, -h, h),
            vec3(h, -h, h),
            vec3(h, h, h),
            vec3(-h, h, h),
        ]
        .into_iter()
        .map(|corner| mesh.add_vertex(corner))
        .collect();

        mesh.add_quad(v[0], v[3], v[2], v[1], color); // front (-z)
        mesh.add_quad(v[5], v[6], v[7], v[4], color); // back (+z)
        mesh.add_quad(v[4], v[7], v[3], v[0], color); // left (-x)
        mesh.add_quad(v[1], v[2], v[6], v[5], color); // right (+x)
        mesh.add_quad(v[3], v[7], v[6], v[2], color); // top (+y)
        mesh.add_quad(v[0], v[1], v[5], v[4], color); // bottom (-y)
        mesh
    }

    /// A pyramid with a 1x1 square base, 1 unit tall, centered on (0, 0, 0).
    pub fn pyramid(color: Color) -> Mesh {
        let mut mesh = Mesh::new();
        let h = 0.5;
        let base = [
            mesh.add_vertex(vec3(-h, -h, -h)),
            mesh.add_vertex(vec3(h, -h, -h)),
            mesh.add_vertex(vec3(h, -h, h)),
            mesh.add_vertex(vec3(-h, -h, h)),
        ];
        let top = mesh.add_vertex(vec3(0.0, h, 0.0));

        mesh.add_quad(base[0], base[1], base[2], base[3], color); // bottom
        for i in 0..4 {
            let next = (i + 1) % 4;
            mesh.add_triangle(base[i], top, base[next], color); // the four sides
        }
        mesh
    }

    /// A sphere 1 unit across, centered on (0, 0, 0), made of `segments`
    /// slices around (like an orange) and half as many rings from top to
    /// bottom. More segments look rounder but take longer to draw. 12 is a
    /// good start.
    pub fn sphere(color: Color, segments: usize) -> Mesh {
        let segments = segments.max(3);
        let rings = (segments / 2).max(2);
        let mut mesh = Mesh::new();

        // A point on the sphere. `ring` goes from 0 (top) to `rings` (bottom).
        let point = |ring: usize, segment: usize| {
            let down = std::f32::consts::PI * ring as f32 / rings as f32;
            let around = TAU * segment as f32 / segments as f32;
            vec3(
                down.sin() * around.cos(),
                down.cos(),
                down.sin() * around.sin(),
            ) * 0.5
        };

        let top = mesh.add_vertex(point(0, 0));
        let bottom = mesh.add_vertex(point(rings, 0));
        // `grid[r][s]` is the vertex index for ring `r`, segment `s` (not counting the poles).
        let grid: Vec<Vec<usize>> = (1..rings)
            .map(|r| {
                (0..segments)
                    .map(|s| mesh.add_vertex(point(r, s)))
                    .collect()
            })
            .collect();

        for s in 0..segments {
            let next = (s + 1) % segments;
            mesh.add_triangle(top, grid[0][next], grid[0][s], color);
            for r in 0..grid.len() - 1 {
                let (row, below) = (&grid[r], &grid[r + 1]);
                mesh.add_quad(row[s], row[next], below[next], below[s], color);
            }
            let last = &grid[grid.len() - 1];
            mesh.add_triangle(last[s], last[next], bottom, color);
        }
        mesh
    }

    /// A flat checkerboard lying on the ground (y = 0), facing up, centered on
    /// (0, 0, 0). It is `tiles` x `tiles` squares, each `tile_size` wide.
    pub fn checkerboard(tiles: usize, tile_size: f32, a: Color, b: Color) -> Mesh {
        let mut mesh = Mesh::new();
        let half = tiles as f32 * tile_size / 2.0;
        let corner = |i: usize| i as f32 * tile_size - half;

        // One vertex per grid point, shared by the tiles around it.
        let row_len = tiles + 1;
        for z in 0..row_len {
            for x in 0..row_len {
                mesh.add_vertex(vec3(corner(x), 0.0, corner(z)));
            }
        }
        let index = |x: usize, z: usize| z * row_len + x;

        for z in 0..tiles {
            for x in 0..tiles {
                let color = if (x + z) % 2 == 0 { a } else { b };
                mesh.add_quad(
                    index(x, z),
                    index(x, z + 1),
                    index(x + 1, z + 1),
                    index(x + 1, z),
                    color,
                );
            }
        }
        mesh
    }
}

impl Canvas {
    /// Draws a 3D mesh, placed in the world by `transform` and seen through `camera`.
    ///
    /// Call `canvas.clear(...)` first each frame: it also resets the depth
    /// buffer that makes near objects hide far ones. 2D drawing afterwards
    /// (like a score) always appears on top.
    ///
    /// On the GPU, 3D is drawn in batches, and any 2D drawing first finishes
    /// the batch so far. So draw all your 3D first and your 2D after it:
    /// alternating between them (a 2D label after every object, say) sends
    /// many small batches to the graphics card, which is much slower.
    ///
    /// (This `impl Canvas` block lives in `render3d.rs`, not `canvas.rs`: Rust
    /// lets a type's methods be spread over several files in the same crate.)
    pub fn draw_mesh(&mut self, mesh: &Mesh, transform: &Transform, camera: &Camera3D) {
        #[cfg(feature = "gpu")]
        if self.draw_mesh_on_gpu(mesh, transform, camera) {
            return; // the graphics card will draw it (see gpu.rs)
        }

        // Otherwise, draw it right here on the CPU.
        let world: Vec<Vec3> = mesh.vertices.iter().map(|&v| transform.apply(v)).collect();
        let sun = sun_direction();
        let projection = camera.projection(self);

        for (index, triangle) in mesh.triangles.iter().enumerate() {
            let [a, b, c] = triangle_corners(&world, index, triangle);

            // Which way does the triangle face? The cross product of two edges
            // points straight out of its front.
            let normal = (b - a).cross(c - a).normalized();

            // Back-face culling: if the front faces away from the camera, skip it.
            if normal.dot(a - camera.position) >= 0.0 {
                continue;
            }

            // Flat shading: brightest when facing the sun head-on.
            let sunlight = normal.dot(sun).max(0.0);
            let brightness = AMBIENT_LIGHT + (1.0 - AMBIENT_LIGHT) * sunlight;
            let color = Color::BLACK.lerp(triangle.color, brightness);

            // Into camera space, then cut off anything too close or behind us.
            let (points, count) = clip_near([a, b, c].map(|p| camera.to_camera_space(p)));

            // Clipping can turn the triangle into a 4-sided shape; draw it as a
            // fan of triangles: (0, 1, 2), then (0, 2, 3).
            for i in 1..count.saturating_sub(1) {
                let corners = [points[0], points[i], points[i + 1]];
                let on_screen = corners.map(|p| projection.project(p));
                let inverse_depths = corners.map(|p| 1.0 / p.z);
                self.fill_triangle_3d(on_screen, inverse_depths, color);
            }
        }
    }
}

/// Cuts a camera-space triangle against the near plane (`z = NEAR`), keeping
/// the part in front of the camera. The result has 0 corners (all of it was
/// too close), 3 (a triangle) or 4 (a quad).
fn clip_near(triangle: [Vec3; 3]) -> ([Vec3; 4], usize) {
    let mut out = [Vec3::ZERO; 4];
    let mut count = 0;

    for i in 0..3 {
        let current = triangle[i];
        let next = triangle[(i + 1) % 3];
        let current_inside = current.z >= NEAR;
        let next_inside = next.z >= NEAR;

        if current_inside {
            out[count] = current;
            count += 1;
        }
        // If this edge crosses the plane, add the point where it crosses.
        if current_inside != next_inside {
            let t = (NEAR - current.z) / (next.z - current.z);
            out[count] = current + (next - current) * t;
            count += 1;
        }
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle's front should face away from the center of a solid shape.
    fn assert_faces_point_outward(mesh: &Mesh) {
        for triangle in &mesh.triangles {
            let [a, b, c] = triangle.corners.map(|i| mesh.vertices[i]);
            let normal = (b - a).cross(c - a);
            let middle = (a + b + c) * (1.0 / 3.0);
            assert!(
                normal.dot(middle) > 0.0,
                "triangle {triangle:?} faces inward"
            );
        }
    }

    #[test]
    fn built_in_meshes_face_outward() {
        assert_faces_point_outward(&Mesh::cube(Color::RED));
        assert_faces_point_outward(&Mesh::pyramid(Color::RED));
        assert_faces_point_outward(&Mesh::sphere(Color::RED, 12));
        assert_eq!(Mesh::cube(Color::RED).triangles.len(), 12);
    }

    #[test]
    fn checkerboard_faces_up() {
        let floor = Mesh::checkerboard(4, 1.0, Color::WHITE, Color::BLACK);
        assert_eq!(floor.triangles.len(), 4 * 4 * 2);
        for triangle in &floor.triangles {
            let [a, b, c] = triangle.corners.map(|i| floor.vertices[i]);
            assert!((b - a).cross(c - a).y > 0.0);
        }
    }

    #[test]
    fn camera_looks_where_it_is_told() {
        let target = vec3(3.0, 1.0, 4.0);
        let camera = Camera3D::looking_at(vec3(-2.0, 5.0, 0.0), target);
        let direction = (target - camera.position).normalized();
        assert!(camera.forward().distance(direction) < 1e-5);

        // In camera space the target is dead ahead.
        let p = camera.to_camera_space(target);
        assert!(p.x.abs() < 1e-4 && p.y.abs() < 1e-4 && p.z > 0.0);
        assert!(camera.right().dot(camera.forward()).abs() < 1e-5);
    }

    #[test]
    fn world_to_screen() {
        let canvas = Canvas::new(320, 240);
        let camera = Camera3D::new(Vec3::ZERO);
        let ahead = camera.world_to_screen(vec3(0.0, 0.0, 5.0), &canvas);
        assert_eq!(ahead, Some(vec2(160.0, 120.0)));
        // Up in the world is up on screen (smaller y); right is right.
        let p = camera
            .world_to_screen(vec3(1.0, 1.0, 5.0), &canvas)
            .unwrap();
        assert!(p.x > 160.0 && p.y < 120.0);
        assert_eq!(camera.world_to_screen(vec3(0.0, 0.0, -5.0), &canvas), None);
    }

    #[test]
    fn transform_scales_rotates_then_moves() {
        let t = Transform::at(vec3(10.0, 0.0, 0.0))
            .rotated(vec3(0.0, std::f32::consts::FRAC_PI_2, 0.0))
            .scaled(vec3(2.0, 1.0, 1.0));
        // (0.5, 0, 0) -> scaled to (1, 0, 0) -> turned right to (0, 0, -1) -> moved.
        assert!(t.apply(vec3(0.5, 0.0, 0.0)).distance(vec3(10.0, 0.0, -1.0)) < 1e-5);
    }

    #[test]
    fn matrices_match_the_cpu_math() {
        let transform = Transform::at(vec3(3.0, -1.0, 7.0))
            .rotated(vec3(0.4, -1.1, 0.25))
            .scaled(vec3(2.0, 0.5, 1.5));
        let camera = Camera3D::looking_at(vec3(-4.0, 3.0, -6.0), vec3(1.0, 0.0, 2.0));
        let canvas = Canvas::new(320, 240);
        let view_projection = camera.projection_matrix(320.0 / 240.0) * camera.view_matrix();

        for point in [vec3(0.5, 0.5, 0.5), vec3(-0.5, 0.2, -0.3), Vec3::ZERO] {
            let world = transform.apply(point);
            assert!(transform.matrix().transform_point(point).distance(world) < 1e-4);
            let in_camera = camera.to_camera_space(world);
            assert!(
                camera
                    .view_matrix()
                    .transform_point(world)
                    .distance(in_camera)
                    < 1e-4
            );

            // Project with the matrix, then turn the GPU's -1..1 range into pixels.
            let [x, y, _, w] = view_projection.transform([world.x, world.y, world.z, 1.0]);
            let pixel = vec2((x / w + 1.0) / 2.0 * 320.0, (1.0 - y / w) / 2.0 * 240.0);
            let expected = camera.world_to_screen(world, &canvas).unwrap();
            assert!(pixel.distance(expected) < 0.01, "{pixel:?} vs {expected:?}");
        }
    }

    #[test]
    fn near_clipping() {
        let behind = [
            vec3(0.0, 0.0, -1.0),
            vec3(1.0, 0.0, -1.0),
            vec3(0.0, 1.0, -2.0),
        ];
        assert_eq!(clip_near(behind).1, 0);

        let in_front = [
            vec3(0.0, 0.0, 1.0),
            vec3(1.0, 0.0, 1.0),
            vec3(0.0, 1.0, 2.0),
        ];
        assert_eq!(clip_near(in_front).1, 3);

        // One corner behind: the triangle becomes a quad, all in front of the plane.
        let straddling = [
            vec3(0.0, 0.0, -1.0),
            vec3(1.0, 0.0, 1.0),
            vec3(0.0, 1.0, 1.0),
        ];
        let (points, count) = clip_near(straddling);
        assert_eq!(count, 4);
        assert!(points.iter().all(|p| p.z >= NEAR - 1e-6));
    }

    #[test]
    fn draws_cubes_with_near_ones_in_front() {
        let camera = Camera3D::new(vec3(0.0, 0.0, -5.0));
        let mut canvas = Canvas::new(64, 48);
        canvas.clear(Color::BLACK);

        let red = Mesh::cube(Color::RED);
        let blue = Mesh::cube(Color::BLUE);
        // Draw the near red cube first, then the far blue one, which is
        // bigger so it would cover the red one without a depth buffer.
        canvas.draw_mesh(&red, &Transform::at(Vec3::ZERO), &camera);
        canvas.draw_mesh(
            &blue,
            &Transform::at(vec3(0.0, 0.0, 5.0)).sized(4.0),
            &camera,
        );

        let center = canvas.get_pixel(32, 24).unwrap();
        assert!(
            center.r > center.b,
            "the near red cube should be in front: {center:?}"
        );
        // The red cube covers x = 27..37 and the blue one x = 22..42, so at
        // x = 24 only blue is visible.
        let beside = canvas.get_pixel(24, 24).unwrap();
        assert!(
            beside.b > beside.r,
            "the big blue cube shows around it: {beside:?}"
        );

        // A cube behind the camera draws nothing.
        let mut empty = Canvas::new(64, 48);
        empty.draw_mesh(&red, &Transform::at(vec3(0.0, 0.0, -10.0)), &camera);
        assert!(empty.pixels().iter().all(|&p| p == 0));
    }

    #[test]
    #[should_panic(expected = "triangle 1 uses vertex 7, but the mesh only has 3 vertices")]
    fn bad_triangle_indices_get_a_clear_message() {
        let mut mesh = Mesh::new();
        for p in [Vec3::ZERO, vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)] {
            mesh.add_vertex(p);
        }
        mesh.add_triangle(0, 1, 2, Color::RED);
        mesh.add_triangle(0, 1, 7, Color::RED); // there is no vertex 7
        let mut canvas = Canvas::new(8, 8);
        let camera = Camera3D::new(Vec3::ZERO);
        canvas.draw_mesh(&mesh, &Transform::at(vec3(0.0, 0.0, 3.0)), &camera);
    }

    #[test]
    fn extreme_fields_of_view_stay_usable() {
        let canvas = Canvas::new(320, 240);
        for fov in [0.0, -1.0, std::f32::consts::PI, 10.0, f32::NAN] {
            let mut camera = Camera3D::new(Vec3::ZERO);
            camera.fov = fov;
            let p = camera
                .world_to_screen(vec3(1.0, 1.0, 5.0), &canvas)
                .unwrap();
            // Up and to the right of the middle, as always, and a real number.
            assert!(
                p.x.is_finite() && p.y.is_finite() && p.x > 160.0 && p.y < 120.0,
                "fov {fov}: {p:?}"
            );
            let m = camera.projection_matrix(4.0 / 3.0);
            assert!(
                m.columns.iter().flatten().all(|v| v.is_finite()),
                "fov {fov}"
            );
        }
    }
}
