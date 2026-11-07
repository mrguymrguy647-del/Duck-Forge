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
use crate::sprite::Sprite;

/// Anything closer to the camera than this is cut off. Without it we'd
/// divide by zero (or by negative numbers, for things behind the camera).
const NEAR: f32 = 0.05;

/// The light 3D is drawn with: one sun, far away (all its rays are
/// parallel, like real sunlight), and a soft light from everywhere.
/// Set it with [`Canvas::set_lighting`]; it stays until it's changed.
///
/// ```
/// # use duckforge::prelude::*;
/// # let mut canvas = Canvas::new(32, 32);
/// canvas.set_lighting(Lighting {
///     sun_direction: vec3(0.0, 0.3, -1.0), // where the light comes from
///     sun_color: Color::from_hex(0xfff2d0),
///     sun_strength: 1.0,
///     ambient: 0.6, // a side view, lit from the front
/// });
/// ```
///
/// A triangle's brightness is `ambient` when it faces away from the sun,
/// rising to `ambient + (1 - ambient) * sun_strength` (in the sun's colour)
/// when it faces the sun head on. Each triangle is lit evenly (flat
/// shading), on the CPU and the GPU alike. [`Mesh::glowing`] meshes ignore
/// the light.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lighting {
    /// The direction the sunlight comes *from* (it needn't be 1 long).
    pub sun_direction: Vec3,
    /// The sun's colour: white leaves colours as they are.
    pub sun_color: Color,
    /// How strong the sun is: 1.0 normally, 0.0 for no sun at all.
    pub sun_strength: f32,
    /// How bright a triangle facing away from the sun is: 0.0 is black,
    /// 1.0 is no shading at all.
    pub ambient: f32,
}

impl Default for Lighting {
    /// The light every 3D picture had before `Lighting` existed (so they
    /// all stay the same): white, from above, a little left and in front,
    /// with ambient 0.35.
    fn default() -> Self {
        Lighting {
            sun_direction: vec3(-0.4, 1.0, -0.6),
            sun_color: Color::WHITE,
            sun_strength: 1.0,
            ambient: 0.35,
        }
    }
}

impl Lighting {
    /// The sun's direction, 1 long (straight up if it was given as zero).
    pub(crate) fn sun(&self) -> Vec3 {
        let d = self.sun_direction;
        if d.length() > 0.0 && d.length().is_finite() {
            d.normalized()
        } else {
            vec3(0.0, 1.0, 0.0)
        }
    }

    /// `ambient`, kept between 0 and 1.
    pub(crate) fn ambient_level(&self) -> f32 {
        if self.ambient.is_finite() {
            self.ambient.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The sun's colour times its strength, for red, green and blue: how
    /// much of the sunlight each colour gets (1.0 for each, by default).
    pub(crate) fn sun_rgb(&self) -> [f32; 3] {
        let strength = if self.sun_strength.is_finite() {
            self.sun_strength.max(0.0)
        } else {
            0.0
        };
        let c = self.sun_color;
        [c.r, c.g, c.b].map(|v| v as f32 / 255.0 * strength)
    }

    /// How bright each colour of a triangle with this `normal` is.
    ///
    /// Written so the default light gives exactly the numbers the engine
    /// always used (`0.35 + 0.65 * sunlight`): multiplying by 1.0 changes
    /// nothing, so every existing picture stays the same to the pixel.
    pub(crate) fn brightness(&self, normal: Vec3, sun: Vec3, sun_rgb: [f32; 3]) -> [f32; 3] {
        let ambient = self.ambient_level();
        let sunlight = normal.dot(sun).max(0.0);
        sun_rgb.map(|share| ambient + (1.0 - ambient) * (sunlight * share))
    }
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

    /// Where a point in the world appears on the canvas, and how far it is
    /// from the camera along the view (its depth), or `None` if it's behind
    /// the camera. For a health bar above a 3D character, or to sort
    /// things yourself (bigger depth is further away).
    pub fn project(&self, point: Vec3, canvas: &Canvas) -> Option<(Vec2, f32)> {
        let p = self.to_camera_space(point);
        if p.z < NEAR {
            return None;
        }
        Some((self.projection(canvas).project(p), p.z))
    }

    /// Whether any of a ball (`center` in the world, `radius`) could be on
    /// a canvas `width` x `height`. False only when it's all outside the
    /// view, so skipping such a mesh never changes the picture.
    pub(crate) fn might_see(&self, center: Vec3, radius: f32, width: f32, height: f32) -> bool {
        if !(radius.is_finite()
            && center.x.is_finite()
            && center.y.is_finite()
            && center.z.is_finite())
        {
            return true; // can't tell: draw it
        }
        let p = self.to_camera_space(center);
        if p.z + radius < NEAR {
            return false; // all of it behind the camera
        }
        // The four sides of the view, as planes through the camera. A
        // point is inside when it's on the inner side of all four.
        let up = (self.usable_fov() / 2.0).tan();
        let across = up * width / height;
        let outside = |slope: f32, sideways: f32| {
            // How far the center is past the plane x (or y) = slope * z.
            (sideways - slope * p.z) / (1.0 + slope * slope).sqrt() > radius
        };
        !(outside(across, p.x) || outside(across, -p.x) || outside(up, p.y) || outside(up, -p.y))
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

    /// The biggest of the three scales (ignoring signs): how much longer
    /// any distance can get.
    pub(crate) fn largest_scale(&self) -> f32 {
        self.scale
            .x
            .abs()
            .max(self.scale.y.abs())
            .max(self.scale.z.abs())
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

/// Texture coordinates showing a whole picture upright on a four-sided
/// face whose corners go bottom-left, top-left, top-right, bottom-right.
const UPRIGHT: [Vec2; 4] = [
    vec2(0.0, 1.0),
    vec2(0.0, 0.0),
    vec2(1.0, 0.0),
    vec2(1.0, 1.0),
];

/// A 3D shape made of triangles, each with a colour, and optionally a
/// picture (a *texture*) painted on it.
///
/// A texture is one [`Sprite`], shared, not copied. Where it goes on each
/// triangle is given by *texture coordinates* (`uvs`): for each corner, a
/// point on the picture, from (0, 0) at its top-left to (1, 1) at its
/// bottom-right (beyond 1 it repeats). The picture's colours are
/// multiplied by the triangle's colour (white shows it as painted) and by
/// the light. Its see-through pixels aren't drawn at all, so leaves and
/// fences have holes. The built-in shapes come with texture coordinates,
/// so `Mesh::cube(Color::WHITE).with_texture(crate_picture)` is a crate.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub triangles: Vec<Triangle>,
    /// Texture coordinates for each triangle's three corners, in the same
    /// order as `triangles` (`None`, or a list shorter than `triangles`,
    /// leaves a triangle plain).
    pub uvs: Vec<Option<[Vec2; 3]>>,
    /// The picture painted on it, if any.
    pub texture: Option<Sprite>,
    /// Drawn at full brightness, whatever the light (lava, lamps, a screen).
    pub glowing: bool,
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

    /// Adds a triangle with texture coordinates for its corners (see
    /// [`Mesh`]): which point of the texture each corner shows.
    pub fn add_triangle_uv(&mut self, a: usize, b: usize, c: usize, uvs: [Vec2; 3], color: Color) {
        self.add_triangle(a, b, c, color);
        self.set_uvs(self.triangles.len() - 1, uvs);
    }

    /// Adds a four-sided face with texture coordinates for its corners,
    /// in the same order: `[vec2(0.0, 1.0), vec2(0.0, 0.0), vec2(1.0, 0.0),
    /// vec2(1.0, 1.0)]` shows the whole picture upright on a face whose
    /// corners go bottom-left, top-left, top-right, bottom-right.
    pub fn add_quad_uv(
        &mut self,
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        uvs: [Vec2; 4],
        color: Color,
    ) {
        self.add_triangle_uv(a, b, c, [uvs[0], uvs[1], uvs[2]], color);
        self.add_triangle_uv(a, c, d, [uvs[0], uvs[2], uvs[3]], color);
    }

    /// Gives triangle `index` texture coordinates.
    fn set_uvs(&mut self, index: usize, uvs: [Vec2; 3]) {
        if self.uvs.len() <= index {
            self.uvs.resize(index + 1, None);
        }
        self.uvs[index] = Some(uvs);
    }

    /// Triangle `index`'s texture coordinates, if it's textured.
    pub(crate) fn uvs_of(&self, index: usize) -> Option<[Vec2; 3]> {
        self.texture.as_ref()?;
        self.uvs.get(index).copied().flatten()
    }

    /// Paints `texture` on the mesh, where its texture coordinates say (the
    /// built-in shapes have them). Its colours are multiplied by the
    /// triangles' colours: make them white to show it as painted.
    pub fn with_texture(mut self, texture: Sprite) -> Self {
        self.texture = Some(texture);
        self
    }

    /// Makes it glow: drawn at full brightness whatever the light, like
    /// lava, a lamp or a screen.
    pub fn glowing(mut self) -> Self {
        self.glowing = true;
        self
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

        // Each face shows the whole texture, upright on the sides. Every
        // face's corners go bottom-left, top-left, top-right, bottom-right,
        // seen from outside.
        let upright = UPRIGHT;
        mesh.add_quad_uv(v[0], v[3], v[2], v[1], upright, color); // front (-z)
        mesh.add_quad_uv(v[5], v[6], v[7], v[4], upright, color); // back (+z)
        mesh.add_quad_uv(v[4], v[7], v[3], v[0], upright, color); // left (-x)
        mesh.add_quad_uv(v[1], v[2], v[6], v[5], upright, color); // right (+x)
        mesh.add_quad_uv(v[3], v[7], v[6], v[2], upright, color); // top (+y)
        mesh.add_quad_uv(v[0], v[1], v[5], v[4], upright, color); // bottom (-y)
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

        mesh.add_quad_uv(base[0], base[1], base[2], base[3], UPRIGHT, color); // bottom
        for i in 0..4 {
            let next = (i + 1) % 4;
            let uvs = [vec2(0.0, 1.0), vec2(0.5, 0.0), vec2(1.0, 1.0)];
            mesh.add_triangle_uv(base[i], top, base[next], uvs, color); // the four sides
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

        // The texture wraps round it like a map of the world: across is
        // around, down is from the top to the bottom.
        let uv = |ring: usize, segment: usize| {
            vec2(segment as f32 / segments as f32, ring as f32 / rings as f32)
        };
        for s in 0..segments {
            let next = (s + 1) % segments;
            let (u0, u1) = (s, s + 1); // s + 1, not `next`: the last slice ends at u = 1
            let mid = |ring| {
                vec2(
                    (s as f32 + 0.5) / segments as f32,
                    ring as f32 / rings as f32,
                )
            };
            mesh.add_triangle_uv(
                top,
                grid[0][next],
                grid[0][s],
                [mid(0), uv(1, u1), uv(1, u0)],
                color,
            );
            for r in 0..grid.len() - 1 {
                let (row, below) = (&grid[r], &grid[r + 1]);
                let uvs = [uv(r + 1, u0), uv(r + 1, u1), uv(r + 2, u1), uv(r + 2, u0)];
                mesh.add_quad_uv(row[s], row[next], below[next], below[s], uvs, color);
            }
            let last = &grid[grid.len() - 1];
            let uvs = [uv(rings - 1, u0), uv(rings - 1, u1), mid(rings)];
            mesh.add_triangle_uv(last[s], last[next], bottom, uvs, color);
        }
        mesh
    }

    /// A flat rectangle `width` x `height`, standing up (in the x-y plane,
    /// centred on (0, 0, 0)) and facing a camera that looks along +z: a
    /// sign, a picture, a wall. One side only: from behind it's invisible.
    /// Its texture coordinates show a whole texture upright.
    pub fn quad(width: f32, height: f32) -> Mesh {
        let mut mesh = Mesh::new();
        let (w, h) = (width / 2.0, height / 2.0);
        let corners = [
            vec3(-w, -h, 0.0),
            vec3(-w, h, 0.0),
            vec3(w, h, 0.0),
            vec3(w, -h, 0.0),
        ];
        let [a, b, c, d] = corners.map(|p| mesh.add_vertex(p));
        mesh.add_quad_uv(a, b, c, d, UPRIGHT, Color::WHITE);
        mesh
    }

    /// A cylinder 1 unit across and 1 tall, standing on its end, centred
    /// on (0, 0, 0), with `segments` sides (12 looks round). A texture
    /// wraps round its side; the ends show it as a disc.
    pub fn cylinder(color: Color, segments: usize) -> Mesh {
        let segments = segments.max(3);
        let mut mesh = Mesh::new();
        let ring = |y: f32, mesh: &mut Mesh| -> Vec<usize> {
            (0..segments)
                .map(|s| {
                    let around = TAU * s as f32 / segments as f32;
                    mesh.add_vertex(vec3(around.cos() * 0.5, y, around.sin() * 0.5))
                })
                .collect()
        };
        let bottom = ring(-0.5, &mut mesh);
        let top = ring(0.5, &mut mesh);
        let (top_middle, bottom_middle) = (
            mesh.add_vertex(vec3(0.0, 0.5, 0.0)),
            mesh.add_vertex(vec3(0.0, -0.5, 0.0)),
        );
        let disc = |s: usize| {
            let around = TAU * s as f32 / segments as f32;
            vec2(0.5 + 0.5 * around.cos(), 0.5 + 0.5 * around.sin())
        };
        for s in 0..segments {
            let next = (s + 1) % segments;
            let (u0, u1) = (s as f32 / segments as f32, (s + 1) as f32 / segments as f32);
            let side = [vec2(u0, 1.0), vec2(u0, 0.0), vec2(u1, 0.0), vec2(u1, 1.0)];
            mesh.add_quad_uv(bottom[s], top[s], top[next], bottom[next], side, color);
            let middle = vec2(0.5, 0.5);
            mesh.add_triangle_uv(
                top_middle,
                top[next],
                top[s],
                [middle, disc(next), disc(s)],
                color,
            );
            mesh.add_triangle_uv(
                bottom_middle,
                bottom[s],
                bottom[next],
                [middle, disc(s), disc(next)],
                color,
            );
        }
        mesh
    }

    /// A cone 1 unit across and 1 tall, point up, centred on (0, 0, 0),
    /// with `segments` sides (a tree, a spike, a party hat).
    pub fn cone(color: Color, segments: usize) -> Mesh {
        let segments = segments.max(3);
        let mut mesh = Mesh::new();
        let base: Vec<usize> = (0..segments)
            .map(|s| {
                let around = TAU * s as f32 / segments as f32;
                mesh.add_vertex(vec3(around.cos() * 0.5, -0.5, around.sin() * 0.5))
            })
            .collect();
        let tip = mesh.add_vertex(vec3(0.0, 0.5, 0.0));
        let middle = mesh.add_vertex(vec3(0.0, -0.5, 0.0));
        for s in 0..segments {
            let next = (s + 1) % segments;
            let (u0, u1) = (s as f32 / segments as f32, (s + 1) as f32 / segments as f32);
            let side = [vec2(u0, 1.0), vec2((u0 + u1) / 2.0, 0.0), vec2(u1, 1.0)];
            mesh.add_triangle_uv(base[s], tip, base[next], side, color);
            let floor = [vec2(0.5, 0.5), vec2(u0, 1.0), vec2(u1, 1.0)];
            mesh.add_triangle_uv(middle, base[s], base[next], floor, color);
        }
        mesh
    }

    /// A 1 x 1 x 1 block (like [`Mesh::cube`]) with a tile of `atlas` on
    /// each side: `faces` is `[top, sides, bottom]`, by tile name, like
    /// `["grass_top", "grass_side", "dirt"]` (`["stone"; 3]` for the same
    /// all round). The tiles must all come from the same picture (one Duck
    /// Quill sheet), so blocks can be [merged](Mesh::merge) into one mesh.
    ///
    /// # Panics
    ///
    /// If `atlas` has no tile by one of those names, or the tiles come
    /// from different pictures.
    #[track_caller]
    pub fn block(atlas: &crate::tileset::Tileset, faces: [&str; 3]) -> Mesh {
        let tiles = faces.map(|name| atlas.frame(name, 0.0).clone());
        let (sheet, _) = tiles[0].in_sheet();
        for (tile, name) in tiles.iter().zip(faces) {
            assert!(
                tile.in_sheet().0.same_pixels(&sheet),
                "Mesh::block: the tile {name:?} is from a different picture than {:?}; \
                 put every block's tiles in one atlas (one sprite sheet) so they can share a texture",
                faces[0]
            );
        }
        let (sheet_w, sheet_h) = (sheet.width() as f32, sheet.height() as f32);
        // Each tile's place in the atlas, as texture points, a hundredth
        // of a pixel inside its edges so the next tile never shows.
        let region = |tile: &Sprite| {
            let (_, [left, top, w, h]) = tile.in_sheet();
            let inset = 0.01;
            move |uv: Vec2| {
                vec2(
                    (left as f32 + inset + uv.x * (w as f32 - 2.0 * inset)) / sheet_w,
                    (top as f32 + inset + uv.y * (h as f32 - 2.0 * inset)) / sheet_h,
                )
            }
        };
        let mut mesh = Mesh::cube(Color::WHITE);
        // Mesh::cube's faces, two triangles each: front, back, left, right,
        // top, bottom.
        let face_tile = [1, 1, 1, 1, 0, 2];
        for (triangle, uvs) in mesh.uvs.iter_mut().enumerate() {
            let place = region(&tiles[face_tile[triangle / 2]]);
            *uvs = uvs.map(|corners| corners.map(place));
        }
        mesh.texture = Some(sheet);
        mesh
    }

    /// Adds `other`, placed by `transform`, to this mesh: many parts drawn
    /// as one mesh, in one go. For a world of blocks, merge the blocks of
    /// each chunk (say 16 x 16) into one mesh per chunk: far fewer draws,
    /// and chunks off screen are still skipped whole.
    ///
    /// Texture coordinates come along, so parts merged before the texture
    /// is given (`head.merge(&beak, ..)`, then `.with_texture(skin)`) are
    /// all painted. The one exception: a part with no texture merged into
    /// a mesh that has one stays plain (its colour), as it was drawn before.
    ///
    /// # Panics
    ///
    /// If both meshes have textures and they're different pictures (a mesh
    /// has one texture: put the pictures in one atlas, see
    /// [`Mesh::block`]), or one glows and the other doesn't.
    #[track_caller]
    pub fn merge(&mut self, other: &Mesh, transform: &Transform) {
        // A plain part joining a textured mesh: don't let it pick up the
        // texture through texture coordinates it was never drawn with.
        let stays_plain = self.texture.is_some() && other.texture.is_none();
        match (&self.texture, &other.texture) {
            (Some(mine), Some(theirs)) => assert!(
                mine.same_pixels(theirs),
                "Mesh::merge: these meshes have different textures, and a mesh has only one; \
                 put their pictures in one atlas (one sprite sheet, with Mesh::block or texture \
                 coordinates into it) and give both meshes that texture"
            ),
            (None, Some(theirs)) => {
                // This mesh's triangles had no texture: keep them plain.
                self.uvs = vec![None; self.triangles.len()];
                self.texture = Some(theirs.clone());
            }
            _ => {}
        }
        let empty = self.triangles.is_empty() && self.vertices.is_empty();
        assert!(
            empty || self.glowing == other.glowing,
            "Mesh::merge: one mesh glows and the other doesn't; draw them as two meshes"
        );
        if empty {
            self.glowing = other.glowing;
        }
        let base = self.vertices.len();
        let first = self.triangles.len();
        self.vertices
            .extend(other.vertices.iter().map(|&v| transform.apply(v)));
        self.triangles
            .extend(other.triangles.iter().map(|t| Triangle {
                corners: t.corners.map(|i| i + base),
                color: t.color,
            }));
        self.uvs.resize(first, None);
        for index in 0..other.triangles.len() {
            let points = other.uvs.get(index).copied().flatten();
            self.uvs.push(if stays_plain { None } else { points });
        }
    }

    /// A ball around every corner: the one around its
    /// [`bounds`](Mesh::bounds) box (a little bigger than the tightest,
    /// but found in one quick pass). Used to skip meshes that are off
    /// screen.
    pub(crate) fn bounding_ball(&self) -> (Vec3, f32) {
        let (low, high) = self.bounds();
        ((low + high) * 0.5, (high - low).length() * 0.5)
    }

    /// The smallest box around every corner: its lowest and highest x, y
    /// and z. Both are (0, 0, 0) for an empty mesh.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut corners = self.vertices.iter();
        let Some(&first) = corners.next() else {
            return (Vec3::ZERO, Vec3::ZERO);
        };
        corners.fold((first, first), |(low, high), &p| {
            (
                vec3(low.x.min(p.x), low.y.min(p.y), low.z.min(p.z)),
                vec3(high.x.max(p.x), high.y.max(p.y), high.z.max(p.z)),
            )
        })
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
                // Each square shows the whole texture.
                mesh.add_quad_uv(
                    index(x, z),
                    index(x, z + 1),
                    index(x + 1, z + 1),
                    index(x + 1, z),
                    UPRIGHT,
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
        self.drawing_in_view = false;
        self.draw_mesh_from(mesh, transform, camera);
    }

    /// Draws `mesh` placed by `transform` *relative to the camera*: x to
    /// the right of the view, y up it, z straight ahead (so `vec3(0.3,
    /// -0.3, 0.6)` is a little right, below the middle, 0.6 ahead). It
    /// moves and turns with the camera, so it stays put on the screen: for
    /// a first-person hand, a held tool, a gun. It's lit by the scene's
    /// light as the world is (turn round, and the sun moves across it).
    ///
    /// It's drawn in front of all the 3D drawn before it, so a hand never
    /// sinks into a wall you stand against: draw these after the world.
    /// Several in a row (a hand and the block it holds) still hide each
    /// other properly.
    pub fn draw_mesh_in_view(&mut self, mesh: &Mesh, transform: &Transform, camera: &Camera3D) {
        if !self.drawing_in_view {
            self.forget_depth();
            self.drawing_in_view = true;
        }
        // A camera at the middle of the world, looking along +z: view
        // space is its world.
        let view = Camera3D {
            position: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            fov: camera.fov,
        };
        // The sun turned the same way, so the light still comes from the
        // same place in the world.
        let world_light = self.lighting();
        let sun = world_light.sun_direction;
        let in_view = Lighting {
            sun_direction: camera.to_camera_space(camera.position + sun),
            ..world_light
        };
        self.set_lighting(in_view);
        self.draw_mesh_from(mesh, transform, &view);
        self.set_lighting(world_light);
    }

    /// Draws a mesh on whichever renderer this canvas uses.
    fn draw_mesh_from(&mut self, mesh: &Mesh, transform: &Transform, camera: &Camera3D) {
        #[cfg(feature = "gpu")]
        if self.draw_mesh_on_gpu(mesh, transform, camera) {
            return; // the graphics card will draw it (see gpu.rs)
        }

        // Otherwise, draw it right here on the CPU. First, a quick look:
        // if a ball around the whole mesh is off screen, so is every
        // triangle (frustum culling).
        let (center, radius) = mesh.bounding_ball();
        let (width, height) = (self.width() as f32, self.height() as f32);
        if !camera.might_see(
            transform.apply(center),
            radius * transform.largest_scale(),
            width,
            height,
        ) {
            return;
        }
        let world: Vec<Vec3> = mesh.vertices.iter().map(|&v| transform.apply(v)).collect();
        let lighting = self.lighting();
        let sun = lighting.sun();
        let sun_rgb = lighting.sun_rgb();
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
            let brightness = if mesh.glowing {
                [1.0; 3]
            } else {
                lighting.brightness(normal, sun, sun_rgb)
            };

            // Into camera space, then cut off anything too close or behind us.
            let in_camera = [a, b, c].map(|p| camera.to_camera_space(p));

            if let (Some(uvs), Some(texture)) = (mesh.uvs_of(index), &mesh.texture) {
                // Textured: each colour is the texture's, times the
                // triangle's, times the light.
                let tint = [triangle.color.r, triangle.color.g, triangle.color.b];
                let shade = [0, 1, 2].map(|i| tint[i] as f32 / 255.0 * brightness[i]);
                let (points, count) = clip_near_uv(in_camera, uvs);
                for i in 1..count.saturating_sub(1) {
                    let corners = [points[0], points[i], points[i + 1]];
                    let on_screen = corners.map(|(p, _)| projection.project(p));
                    let inverse_depths = corners.map(|(p, _)| 1.0 / p.z);
                    // The texture point divided by the distance: these blend
                    // in a straight line across the screen, and dividing by
                    // the blended 1 / distance per pixel gives the true point
                    // (perspective-correct texturing).
                    let uv_over_z = corners.map(|(p, uv)| uv * (1.0 / p.z));
                    self.fill_triangle_3d_textured(
                        on_screen,
                        inverse_depths,
                        uv_over_z,
                        texture,
                        shade,
                    );
                }
                continue;
            }
            let color = if brightness[0] == brightness[1] && brightness[1] == brightness[2] {
                // White light: exactly the calculation the engine always made.
                Color::BLACK.lerp(triangle.color, brightness[0])
            } else {
                let mix = |c: u8, b: f32| (c as f32 * b.clamp(0.0, 1.0)).round() as u8;
                let t = triangle.color;
                Color::rgb(
                    mix(t.r, brightness[0]),
                    mix(t.g, brightness[1]),
                    mix(t.b, brightness[2]),
                )
            };
            let (points, count) = clip_near(in_camera);

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

/// How a [billboard](Canvas::draw_billboard_with) is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Billboard {
    /// The point of the sprite (in its pixels) that goes at the position
    /// given: `None` for the middle of its bottom edge (its feet). A
    /// [`SpriteSheet`](crate::spritesheet::SpriteSheet)'s is
    /// `Some(sheet.pivot())`.
    pub pivot: Option<Vec2>,
    /// Mirrored left to right (a character facing the other way).
    pub flip_x: bool,
    /// Turned to face the camera fully, tilting back when the camera looks
    /// down (for things that float: sparks, coins, clouds). Normally a
    /// billboard stays upright and only turns round, so characters stand
    /// straight.
    pub face_camera: bool,
}

impl Canvas {
    /// Draws a sprite standing in the 3D world, turned towards the camera:
    /// a *billboard*. Its feet (the middle of its bottom edge) are at `at`,
    /// and it's `height` world units tall (as wide as its picture says).
    /// It stays upright, hides behind meshes in front of it and hides those
    /// behind it, and its see-through pixels show what's behind (they
    /// don't hide anything). Its pixels stay sharp squares, and it isn't
    /// shaded: it looks as painted.
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(64, 48);
    /// # let camera = Camera3D::new(vec3(0.0, 1.0, -4.0));
    /// # let duck = Sprite::from_pixels(16, 16, vec![Color::YELLOW; 256]);
    /// canvas.draw_billboard(&duck, vec3(0.0, 0.0, 0.0), 1.0, &camera);
    /// ```
    pub fn draw_billboard(&mut self, sprite: &Sprite, at: Vec3, height: f32, camera: &Camera3D) {
        self.draw_billboard_with(sprite, at, height, camera, Billboard::default());
    }

    /// [`draw_billboard`](Canvas::draw_billboard), with a pivot, mirroring
    /// or full facing (see [`Billboard`]):
    ///
    /// ```
    /// # use duckforge::prelude::*;
    /// # let mut canvas = Canvas::new(64, 48);
    /// # let camera = Camera3D::new(vec3(0.0, 1.0, -4.0));
    /// # let frame = Sprite::from_pixels(16, 16, vec![Color::YELLOW; 256]);
    /// let look = Billboard { pivot: Some(vec2(8.0, 16.0)), flip_x: true, ..Default::default() };
    /// canvas.draw_billboard_with(&frame, vec3(1.0, 0.0, 2.0), 1.0, &camera, look);
    /// ```
    pub fn draw_billboard_with(
        &mut self,
        sprite: &Sprite,
        at: Vec3,
        height: f32,
        camera: &Camera3D,
        look: Billboard,
    ) {
        let (w, h) = (sprite.width() as f32, sprite.height() as f32);
        if w == 0.0 || h == 0.0 || !(height.is_finite() && height > 0.0) {
            return;
        }
        let size = vec3(height * w / h, height, 1.0);
        // Turned like the camera: round (yaw), and back (pitch) if asked.
        let pitch = if look.face_camera { camera.pitch } else { 0.0 };
        let rotation = vec3(pitch, camera.yaw, 0.0);
        // The pivot, in the quad's own units (it's 1 x 1, centred).
        let pivot = look.pivot.unwrap_or(vec2(w / 2.0, h));
        let mut x = pivot.x / w - 0.5;
        if look.flip_x {
            x = -x;
        }
        let local = vec3(x * size.x, (0.5 - pivot.y / h) * size.y, 0.0);
        let turned = local.rotate_x(rotation.x).rotate_y(rotation.y);
        let transform = Transform {
            position: at - turned,
            rotation,
            scale: size,
        };
        let mesh = self.billboard_mesh(sprite, look.flip_x);
        self.draw_mesh(&mesh, &transform, camera);
    }
}

/// A 1 x 1 quad showing `sprite`, glowing (sprites look as painted). The
/// texture is the whole picture the sprite was cut from, so every frame of
/// a sprite sheet shares one texture on the GPU.
pub(crate) fn billboard_quad(sprite: &Sprite, flip_x: bool) -> Mesh {
    let (sheet, [left, top, w, h]) = sprite.in_sheet();
    let (sheet_w, sheet_h) = (sheet.width() as f32, sheet.height() as f32);
    let (mut u0, mut u1) = (left as f32 / sheet_w, (left + w) as f32 / sheet_w);
    if flip_x {
        std::mem::swap(&mut u0, &mut u1);
    }
    // A hundredth of a pixel inside the edges, so the next frame never shows.
    let nudge = |u: f32, towards: f32| u + (towards - u).signum() * 0.01 / sheet_w;
    let (u0, u1) = (nudge(u0, u1), nudge(u1, u0));
    let (v0, v1) = (
        (top as f32 + 0.01) / sheet_h,
        ((top + h) as f32 - 0.01) / sheet_h,
    );
    let mut mesh = Mesh::new();
    let corners = [
        vec3(-0.5, -0.5, 0.0),
        vec3(-0.5, 0.5, 0.0),
        vec3(0.5, 0.5, 0.0),
        vec3(0.5, -0.5, 0.0),
    ];
    let [a, b, c, d] = corners.map(|p| mesh.add_vertex(p));
    let uvs = [vec2(u0, v1), vec2(u0, v0), vec2(u1, v0), vec2(u1, v1)];
    mesh.add_quad_uv(a, b, c, d, uvs, Color::WHITE);
    mesh.with_texture(sheet).glowing()
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

/// [`clip_near`] for a textured triangle: the texture coordinates are cut
/// at the same places as the corners.
fn clip_near_uv(triangle: [Vec3; 3], uvs: [Vec2; 3]) -> ([(Vec3, Vec2); 4], usize) {
    let mut out = [(Vec3::ZERO, Vec2::ZERO); 4];
    let mut count = 0;
    for i in 0..3 {
        let (current, next) = (triangle[i], triangle[(i + 1) % 3]);
        let (current_uv, next_uv) = (uvs[i], uvs[(i + 1) % 3]);
        let current_inside = current.z >= NEAR;
        if current_inside {
            out[count] = (current, current_uv);
            count += 1;
        }
        if current_inside != (next.z >= NEAR) {
            let t = (NEAR - current.z) / (next.z - current.z);
            out[count] = (
                current + (next - current) * t,
                current_uv + (next_uv - current_uv) * t,
            );
            count += 1;
        }
    }
    (out, count)
}

/// A wall right in front of a camera turned to `yaw`, and a green hand
/// (`glowing` or lit) held out in view space, behind the wall's depth.
#[cfg(test)]
pub(crate) fn draw_hand_in_front_of_wall(
    canvas: &mut crate::canvas::Canvas,
    yaw: f32,
    glowing: bool,
) {
    let mut camera = Camera3D::new(vec3(5.0, 1.0, 5.0));
    camera.yaw = yaw;
    canvas.clear(Color::BLACK);
    let wall = Transform::at(camera.position + camera.forward() * 0.5).sized(0.5);
    canvas.draw_mesh(&Mesh::cube(Color::RED), &wall, &camera);
    let hand = Mesh::cube(Color::GREEN);
    let hand = if glowing { hand.glowing() } else { hand };
    let held = Transform::at(vec3(0.0, 0.0, 0.9)).sized(0.2);
    canvas.draw_mesh_in_view(&hand, &held, &camera);
}

/// A floor 6 x 6 blocks wide painted with the "red" tile of a two-tile
/// atlas (red, then blue), repeated once a block: texture points 0 to 6.
#[cfg(test)]
pub(crate) fn draw_repeated_tile_floor(canvas: &mut crate::canvas::Canvas) {
    use crate::math::vec2;
    let mut pixels = Vec::new();
    for _ in 0..4 {
        pixels.extend([Color::RED; 4]);
        pixels.extend([Color::BLUE; 4]);
    }
    let atlas =
        crate::tileset::Tileset::grid(&Sprite::from_pixels(8, 4, pixels), 4, 4, &["red", "blue"]);
    let mut floor = Mesh::new().with_texture(atlas.tile("red"));
    let corners = [
        vec3(-3.0, 0.0, 0.0),
        vec3(-3.0, 0.0, 6.0),
        vec3(3.0, 0.0, 6.0),
        vec3(3.0, 0.0, 0.0),
    ];
    let [a, b, c, d] = corners.map(|corner| floor.add_vertex(corner));
    let uv = [
        vec2(0.0, 0.0),
        vec2(0.0, 6.0),
        vec2(6.0, 6.0),
        vec2(6.0, 0.0),
    ];
    floor.add_quad_uv(a, b, c, d, uv, Color::WHITE);
    let floor = floor.glowing(); // exact colours, whatever the light
    let mut camera = Camera3D::new(vec3(0.0, 2.0, -1.0));
    camera.look_at(vec3(0.0, 0.0, 3.0));
    canvas.clear(Color::BLACK);
    canvas.draw_mesh(&floor, &Transform::default(), &camera);
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
    fn new_shapes_face_outward_and_keep_their_sizes() {
        assert_faces_point_outward(&Mesh::cylinder(Color::RED, 12));
        assert_faces_point_outward(&Mesh::cone(Color::RED, 12));
        assert_eq!(Mesh::cylinder(Color::RED, 12).triangles.len(), 48);
        assert_eq!(Mesh::cone(Color::RED, 12).triangles.len(), 24);
        assert_eq!(
            Mesh::quad(2.0, 1.0).bounds(),
            (vec3(-1.0, -0.5, 0.0), vec3(1.0, 0.5, 0.0))
        );
        assert_eq!(Mesh::cylinder(Color::RED, 8).bounds().1.y, 0.5);
        // The built-in shapes keep their sizes, and now have texture
        // coordinates for every triangle.
        for (mesh, triangles, vertices) in [
            (Mesh::cube(Color::RED), 12, 8),
            (Mesh::pyramid(Color::RED), 6, 5),
            (Mesh::sphere(Color::RED, 12), 120, 62),
            (Mesh::checkerboard(4, 1.0, Color::RED, Color::RED), 32, 25),
        ] {
            assert_eq!(
                (mesh.triangles.len(), mesh.vertices.len()),
                (triangles, vertices)
            );
            assert_eq!(mesh.uvs.len(), triangles);
            assert!(
                mesh.uvs
                    .iter()
                    .flatten()
                    .flatten()
                    .all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y))
            );
        }
    }

    fn atlas() -> crate::tileset::Tileset {
        // A 4 x 2 picture: two 2 x 2 tiles, grass and dirt.
        let sheet = Sprite::from_pixels(
            4,
            2,
            vec![
                Color::GREEN,
                Color::GREEN,
                Color::ORANGE,
                Color::ORANGE,
                Color::GREEN,
                Color::GREEN,
                Color::ORANGE,
                Color::ORANGE,
            ],
        );
        crate::tileset::Tileset::grid(&sheet, 2, 2, &["grass", "dirt"])
    }

    #[test]
    fn a_block_shows_its_tiles_from_one_atlas() {
        let block = Mesh::block(&atlas(), ["grass", "dirt", "dirt"]);
        assert_eq!(block.triangles.len(), 12);
        assert_eq!(
            block.texture.as_ref().unwrap().width(),
            4,
            "the whole atlas"
        );
        // The top's texture points are in the grass tile (the left half),
        // the sides' in the dirt tile (the right half).
        let us = |triangle: usize| block.uvs[triangle].unwrap().map(|uv| uv.x);
        assert!(us(8).iter().all(|&u| u > 0.0 && u < 0.5), "{:?}", us(8));
        assert!(us(0).iter().all(|&u| u > 0.5 && u < 1.0), "{:?}", us(0));
        let camera = Camera3D::looking_at(vec3(0.0, 3.0, -0.01), Vec3::ZERO);
        let mut canvas = Canvas::new(16, 16);
        canvas.set_lighting(Lighting {
            ambient: 1.0,
            ..Lighting::default()
        });
        canvas.draw_mesh(&block, &Transform::default(), &camera);
        assert_eq!(
            canvas.get_pixel(8, 8),
            Some(Color::GREEN),
            "the top, from above"
        );
    }

    #[test]
    #[should_panic(expected = "put every block's tiles in one atlas")]
    fn a_block_from_two_pictures_is_refused() {
        let mut tiles = atlas();
        tiles.insert(
            "stone",
            crate::sprite::Animation::new(vec![Sprite::from_pixels(1, 1, vec![Color::GRAY])], 1.0),
        );
        Mesh::block(&tiles, ["grass", "stone", "dirt"]);
    }

    #[test]
    fn merged_meshes_are_one_mesh() {
        let tiles = atlas();
        let mut chunk = Mesh::new();
        for x in 0..3 {
            chunk.merge(
                &Mesh::block(&tiles, ["grass", "dirt", "dirt"]),
                &Transform::at(vec3(x as f32, 0.0, 0.0)),
            );
        }
        // A plain coloured cube can join a textured mesh: it stays plain.
        chunk.merge(&Mesh::cube(Color::RED), &Transform::at(vec3(0.0, 1.0, 0.0)));
        assert_eq!((chunk.triangles.len(), chunk.vertices.len()), (48, 32));
        assert!(chunk.uvs_of(0).is_some() && chunk.uvs_of(47).is_none());
        assert_eq!(
            chunk.bounds(),
            (vec3(-0.5, -0.5, -0.5), vec3(2.5, 1.5, 0.5))
        );
        // And the other way round: textured parts added to a plain mesh.
        let mut plain = Mesh::cube(Color::RED);
        plain.merge(
            &Mesh::block(&tiles, ["dirt"; 3]),
            &Transform::at(vec3(2.0, 0.0, 0.0)),
        );
        assert!(plain.uvs_of(0).is_none() && plain.uvs_of(12).is_some());
    }

    #[test]
    fn a_mesh_in_view_stays_on_the_screen_in_front_of_the_world() {
        let green_pixels = |yaw: f32| {
            let mut canvas = crate::canvas::Canvas::new(80, 60);
            draw_hand_in_front_of_wall(&mut canvas, yaw, true);
            let middle = canvas.get_pixel(40, 30).unwrap();
            assert_eq!(middle, Color::GREEN, "the hand, though the wall is nearer");
            canvas
                .pixels()
                .iter()
                .map(|&p| p == Color::GREEN.to_u32())
                .collect::<Vec<bool>>()
        };
        // Turning round doesn't move it on the screen.
        assert_eq!(green_pixels(0.0), green_pixels(2.5));
    }

    #[test]
    fn a_mesh_in_view_is_lit_by_the_worlds_sun() {
        let middle = |yaw: f32| {
            let mut canvas = crate::canvas::Canvas::new(80, 60);
            canvas.set_lighting(Lighting {
                // From behind a camera with yaw 0: on the hand's face.
                sun_direction: vec3(0.0, 0.0, -1.0),
                sun_strength: 1.0,
                ambient: 0.3,
                ..Lighting::default()
            });
            draw_hand_in_front_of_wall(&mut canvas, yaw, false);
            assert_eq!(
                canvas.lighting().sun_direction,
                vec3(0.0, 0.0, -1.0),
                "put back"
            );
            canvas.get_pixel(40, 30).unwrap().g
        };
        // Turned round, the sun is ahead: the face we see is in shadow.
        assert!(middle(0.0) > middle(std::f32::consts::PI) + 60);
    }

    /// One tile of an atlas repeats over a big floor, and the tile next to
    /// it in the atlas never shows.
    #[test]
    fn an_atlas_tile_repeats_on_its_own() {
        let mut canvas = crate::canvas::Canvas::new(80, 60);
        draw_repeated_tile_floor(&mut canvas);
        let count = |color: Color| {
            let wanted = color.to_u32();
            canvas.pixels().iter().filter(|&&p| p == wanted).count()
        };
        assert!(count(Color::RED) > 1500, "the floor is red");
        assert_eq!(count(Color::BLUE), 0, "the blue tile never shows");
    }

    /// Duck Craft's beak: parts merged before the texture is given keep
    /// their texture points, so the texture paints all of them.
    #[test]
    fn merging_before_the_texture_keeps_the_texture_points() {
        let skin = Sprite::from_pixels(2, 1, vec![Color::RED, Color::BLUE]);
        let mut head = Mesh::cube(Color::WHITE);
        let mut beak = Mesh::cube(Color::WHITE);
        beak.set_uvs(0, [vec2(0.5, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0)]);
        head.merge(&beak, &Transform::at(vec3(0.0, 0.0, 0.6)));
        let head = head.with_texture(skin);
        assert!(
            (0..24).all(|i| head.uvs_of(i).is_some()),
            "every triangle is painted"
        );
        assert_eq!(
            head.uvs_of(12),
            Some([vec2(0.5, 0.0), vec2(1.0, 0.0), vec2(1.0, 1.0)]),
            "the beak's own points"
        );
    }

    #[test]
    #[should_panic(
        expected = "these meshes have different textures, and a mesh has only one; put their pictures in one atlas"
    )]
    fn merging_two_textures_is_refused() {
        let mut mesh =
            Mesh::cube(Color::WHITE).with_texture(Sprite::from_pixels(1, 1, vec![Color::RED]));
        let other =
            Mesh::cube(Color::WHITE).with_texture(Sprite::from_pixels(1, 1, vec![Color::RED]));
        mesh.merge(&other, &Transform::default());
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

    /// A 2 x 1 texture: red on the left, blue on the right.
    fn red_blue() -> Sprite {
        Sprite::from_pixels(2, 1, vec![Color::RED, Color::BLUE])
    }

    #[test]
    fn textures_are_perspective_correct() {
        // A long floor, textured red then blue (turned so the blue half is
        // nearer), seen from just above its near end: its far half looks
        // much smaller than its near half. The colours must swap exactly
        // where the floor's middle line is on screen (blending the texture
        // straight across the screen would put it far too low).
        let floor = Mesh::checkerboard(1, 1.0, Color::WHITE, Color::WHITE).with_texture(red_blue());
        let transform = Transform::at(vec3(0.0, 0.0, 6.0))
            .rotated(vec3(0.0, std::f32::consts::FRAC_PI_2, 0.0))
            .scaled(vec3(10.0, 1.0, 2.0));
        let camera = Camera3D::looking_at(vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 6.0));
        let mut canvas = Canvas::new(64, 96);
        canvas.set_lighting(Lighting {
            ambient: 1.0,
            ..Lighting::default()
        });
        canvas.draw_mesh(&floor, &transform, &camera);
        let middle = camera
            .world_to_screen(vec3(0.0, 0.0, 6.0), &canvas)
            .unwrap();
        let row = |y: f32| canvas.get_pixel(32, y as i32).unwrap();
        let (near, far) = (row(middle.y + 1.5), row(middle.y - 1.5));
        assert_eq!(near, Color::BLUE, "just nearer than the middle");
        assert_eq!(far, Color::RED, "just further");
    }

    #[test]
    fn see_through_texels_show_what_is_behind() {
        let holes = Sprite::from_pixels(2, 1, vec![Color::GREEN, Color::rgba(0, 0, 0, 0)]);
        let fence = Mesh::quad(2.0, 1.0).with_texture(holes);
        let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
        let mut canvas = Canvas::new(64, 48);
        canvas.clear(Color::BLACK);
        canvas.draw_mesh(&fence, &Transform::default(), &camera);
        canvas.draw_mesh(
            &Mesh::cube(Color::RED).glowing(),
            &Transform::at(vec3(0.0, 0.0, 3.0)).sized(3.0),
            &camera,
        );
        let left = canvas.get_pixel(24, 24).unwrap();
        let right = canvas.get_pixel(40, 24).unwrap();
        assert!(
            left.g > left.r && left.g > left.b,
            "the fence's solid half: {left:?}"
        );
        assert_eq!(
            right,
            Color::RED,
            "through the hole, the cube behind, drawn after it"
        );
    }

    #[test]
    fn lighting_changes_colour_and_glowing_ignores_it() {
        let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
        let shot = |lighting: Lighting, mesh: Mesh| {
            let mut canvas = Canvas::new(16, 16);
            canvas.set_lighting(lighting);
            canvas.draw_mesh(&mesh, &Transform::default(), &camera);
            canvas.get_pixel(8, 8).unwrap()
        };
        let grey = Color::rgb(200, 200, 200);
        // The face towards the camera (-z), lit from straight in front.
        let front = Lighting {
            sun_direction: vec3(0.0, 0.0, -1.0),
            ambient: 0.0,
            ..Lighting::default()
        };
        assert_eq!(shot(front, Mesh::cube(grey)), grey, "full sun");
        let behind = Lighting {
            sun_direction: vec3(0.0, 0.0, 1.0),
            ambient: 0.5,
            ..front
        };
        assert_eq!(
            shot(behind, Mesh::cube(grey)),
            Color::rgb(100, 100, 100),
            "only the ambient half"
        );
        let red_sun = Lighting {
            sun_color: Color::rgb(255, 0, 0),
            ..front
        };
        assert_eq!(shot(red_sun, Mesh::cube(grey)), Color::rgb(200, 0, 0));
        let dark = Lighting {
            sun_strength: 0.0,
            ambient: 0.0,
            ..front
        };
        assert_eq!(shot(dark, Mesh::cube(grey)), Color::BLACK);
        assert_eq!(
            shot(dark, Mesh::cube(grey).glowing()),
            grey,
            "glowing ignores the light"
        );
        // The default is the light the engine always had.
        assert_eq!(Canvas::new(1, 1).lighting(), Lighting::default());
        assert_eq!(
            Lighting::default().sun(),
            vec3(-0.4, 1.0, -0.6).normalized()
        );
    }

    /// A 4 x 8 sprite: yellow, with a see-through top-left corner pixel
    /// and a red bottom row (its feet).
    fn duck() -> Sprite {
        let pixels = (0..32)
            .map(|i| match (i % 4, i / 4) {
                (0, 0) => Color::rgba(0, 0, 0, 0),
                (_, 7) => Color::RED,
                _ => Color::YELLOW,
            })
            .collect();
        Sprite::from_pixels(4, 8, pixels)
    }

    #[test]
    fn billboards_stand_on_their_feet_and_hide_behind_things() {
        let camera = Camera3D::new(vec3(0.0, 1.0, -5.0));
        let mut canvas = Canvas::new(64, 64);
        canvas.clear(Color::BLACK);
        canvas.draw_billboard(&duck(), vec3(0.0, 0.0, 0.0), 2.0, &camera);
        // Its feet are at the ground's height on screen, its middle above.
        let (feet, depth) = camera.project(vec3(0.0, 0.0, 0.0), &canvas).unwrap();
        assert_eq!(depth, 5.0, "depth is the distance along the view");
        let (middle, _) = camera.project(vec3(0.0, 1.0, 0.0), &canvas).unwrap();
        assert_eq!(
            canvas.get_pixel(feet.x as i32, feet.y as i32 - 1),
            Some(Color::RED)
        );
        assert_eq!(
            canvas.get_pixel(middle.x as i32, middle.y as i32),
            Some(Color::YELLOW)
        );
        assert_eq!(
            canvas.get_pixel(feet.x as i32, feet.y as i32 + 2),
            Some(Color::BLACK),
            "nothing under its feet"
        );
        // A cube in front hides its lower half; one behind is hidden by it.
        let wall = Mesh::cube(Color::BLUE).glowing();
        canvas.draw_mesh(
            &wall,
            &Transform::at(vec3(0.0, 0.25, -1.0)).scaled(vec3(4.0, 0.5, 0.1)),
            &camera,
        );
        canvas.draw_mesh(
            &wall,
            &Transform::at(vec3(0.0, 1.0, 3.0)).scaled(vec3(4.0, 4.0, 0.1)),
            &camera,
        );
        assert_eq!(
            canvas.get_pixel(feet.x as i32, feet.y as i32 - 1),
            Some(Color::BLUE),
            "behind the low wall"
        );
        assert_eq!(
            canvas.get_pixel(middle.x as i32, middle.y as i32),
            Some(Color::YELLOW),
            "in front of the far wall"
        );
        // Its see-through corner shows the far wall.
        let corner = camera.project(vec3(-0.45, 1.95, 0.0), &canvas).unwrap().0;
        assert_eq!(
            canvas.get_pixel(corner.x as i32, corner.y as i32),
            Some(Color::BLUE)
        );
    }

    #[test]
    fn billboards_turn_with_the_camera_and_flip() {
        // Seen from the side (the camera at +x looking back along -x), it
        // still faces the camera, as wide as from the front.
        let side = Camera3D::looking_at(vec3(5.0, 1.0, 0.0), vec3(0.0, 1.0, 0.0));
        let mut canvas = Canvas::new(64, 64);
        canvas.draw_billboard(&duck(), Vec3::ZERO, 2.0, &side);
        let width = (0..64)
            .filter(|&x| canvas.get_pixel(x, 32) == Some(Color::YELLOW))
            .count();
        assert!((10..=14).contains(&width), "{width} pixels wide");
        // Mirrored, the see-through pixel is on the other side.
        let front = Camera3D::new(vec3(0.0, 1.0, -5.0));
        let look = Billboard {
            flip_x: true,
            ..Default::default()
        };
        let mut flipped = Canvas::new(64, 64);
        flipped.draw_billboard_with(&duck(), Vec3::ZERO, 2.0, &front, look);
        let corner = |x: f32| {
            let p = front.project(vec3(x, 1.95, 0.0), &flipped).unwrap().0;
            flipped.get_pixel(p.x as i32, p.y as i32).unwrap()
        };
        assert_eq!((corner(-0.45), corner(0.45)), (Color::YELLOW, Color::BLACK));
        // Upright, a camera looking down sees it lean back; facing the
        // camera, it stays square to the view (so it looks taller).
        let above = Camera3D::looking_at(vec3(0.0, 4.0, -3.0), vec3(0.0, 1.0, 0.0));
        let height = |face_camera: bool| {
            let mut c = Canvas::new(64, 64);
            let look = Billboard {
                face_camera,
                ..Default::default()
            };
            c.draw_billboard_with(&duck(), Vec3::ZERO, 2.0, &above, look);
            (0..64)
                .filter(|&y| c.get_pixel(32, y) != Some(Color::BLACK))
                .count()
        };
        assert!(
            height(true) > height(false),
            "{} vs {}",
            height(true),
            height(false)
        );
    }

    #[test]
    fn meshes_off_screen_are_skipped_but_never_ones_partly_on_it() {
        let camera = Camera3D::new(Vec3::ZERO);
        let sees =
            |x: f32, z: f32, radius: f32| camera.might_see(vec3(x, 0.0, z), radius, 320.0, 240.0);
        assert!(sees(0.0, 10.0, 1.0), "straight ahead");
        assert!(!sees(0.0, -10.0, 1.0), "behind");
        assert!(!sees(30.0, 10.0, 1.0), "far to the right");
        // The middle is off screen, but it's big enough to reach onto it.
        assert!(sees(30.0, 10.0, 25.0));
        // At the edge of the view (60 degrees high, 4:3 wide): the side is
        // slanted, so a ball 1 across is fully outside only once its middle
        // is about 1.26 past the edge sideways.
        let slope = 30f32.to_radians().tan() * 320.0 / 240.0;
        let edge = 10.0 * slope;
        let reach = (1.0 + slope * slope).sqrt();
        assert!(sees(edge + reach - 0.05, 10.0, 1.0) && !sees(edge + reach + 0.05, 10.0, 1.0));
        // A big mesh whose middle is behind the camera, reaching in front.
        assert!(sees(0.0, -1.0, 5.0));
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
