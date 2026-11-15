//! The lit renderer: lamps, shadows, smooth shading, glow and smooth
//! edges, drawn on every core of the CPU. (Its settings are in
//! `lights.rs`.)
//!
//! The plain renderer (`render3d.rs`) draws each mesh the moment it's
//! given. This one only collects them (the *queue*), and draws the whole
//! frame's 3D at once when 2D is drawn over it or the frame ends
//! ([`Canvas::flush_3d`](crate::canvas::Canvas::flush_3d)). Having
//! everything at once is what makes shadows possible: before any pixel
//! is lit, every mesh is drawn once more from the sun (and from each lamp
//! with shadows) into a *shadow map*, a picture of how far the light gets.
//! A point is in shadow if something nearer the light is in that picture.
//!
//! Then the picture is drawn in bands of 16 rows, several bands at once on
//! different threads. Each band is worked out on its own, from the same
//! triangles in the same order, so it comes out the same whichever thread
//! draws it: the pictures are the same with 1 thread or 16. For each band:
//!
//! 1. **Which triangle is nearest at each point** (the depth test), for
//!    every solid triangle, without lighting anything yet.
//! 2. **Lighting each point once**, with that triangle: the sun (unless the
//!    shadow map says something's in the way), the ambient light, every
//!    lamp, glow and fog. Doing the depth test first means a point hidden
//!    behind ten walls is still lit only once.
//! 3. **See-through triangles** are mixed over it, in the order they came.
//! 4. **Smooth edges**: with supersampling, each pixel was worked out at
//!    several points (2 x 2 for a factor of 2), which are averaged.
//!
//! Last, glowing pixels are blurred and added over the picture (bloom).
//!
//! With no lamps, no shadows and flat shading, a triangle's light is the
//! same everywhere on it, and it's worked out exactly as the plain
//! renderer does: a test checks the two give the same pixels.

use std::ops::Range;
use std::sync::Mutex;

use crate::color::{self, Color};
use crate::lights::{Bloom, CUBE_FACES, Effects, Lamp, Shadows};
use crate::math::{Vec2, Vec3, vec2, vec3};
use crate::render3d::{
    Camera3D, Fog, GLOW_LEVELS, GlowMap, Lighting, Mesh, NEAR, Transform, texture_shade,
    triangle_corners,
};
use crate::sprite::Sprite;

/// Output rows in each band.
const BAND_ROWS: usize = 16;
/// "No triangle here".
const NONE: u32 = u32::MAX;
/// Edges sharper than this stay sharp in smooth shading (a cube's 90°).
const CREASE_DEGREES: f32 = 50.0;

/// The 3D of a frame, waiting to be drawn.
#[derive(Default)]
pub(crate) struct Queue {
    draws: Vec<Draw>,
    tris: Vec<WorldTri>,
    /// The next mesh starts in front of everything so far (a hand).
    reset_next: bool,
}

/// One mesh drawn: how, and which of the queue's triangles are its.
struct Draw {
    camera: Camera3D,
    lighting: Lighting,
    fog: Option<Fog>,
    tint: Color,
    texture: Option<Sprite>,
    /// Which pixels of the texture glow (see `GlowMap`).
    glow_map: Option<GlowMap>,
    /// A billboard: drawn as painted, and not glowing.
    unlit: bool,
    /// Drawn with `draw_mesh_in_view`: casts no shadow.
    in_view: bool,
    /// Starts in front of everything drawn before it.
    reset_depth: bool,
    smooth: bool,
    tris: Range<usize>,
}

/// One triangle, in the world.
struct WorldTri {
    p: [Vec3; 3],
    /// Smooth shading's normal at each corner.
    n: Option<[Vec3; 3]>,
    /// The way its front faces (1 long).
    face: Vec3,
    uv: Option<[Vec2; 3]>,
    color: Color,
    glow: f32,
    /// It glows where its texture's glow map says.
    texel_glow: bool,
    /// Its front faces the camera (only fronts are drawn; both sides
    /// cast shadows).
    front: bool,
}

/// How a mesh joins the queue.
pub(crate) struct Adding<'a> {
    pub(crate) mesh: &'a Mesh,
    pub(crate) transform: &'a Transform,
    pub(crate) camera: &'a Camera3D,
    pub(crate) lighting: Lighting,
    pub(crate) fog: Option<Fog>,
    pub(crate) tint: Color,
    pub(crate) smooth: bool,
    pub(crate) unlit: bool,
    /// For `draw_mesh_in_view`: the mesh is placed relative to the camera.
    pub(crate) in_view: bool,
}

impl Queue {
    pub(crate) fn is_empty(&self) -> bool {
        self.draws.is_empty()
    }

    /// Forgets everything queued (the canvas was cleared).
    pub(crate) fn clear(&mut self) {
        self.draws.clear();
        self.tris.clear();
        self.reset_next = false;
    }

    /// The next mesh starts in front of everything so far.
    pub(crate) fn forget_depth(&mut self) {
        self.reset_next = true;
    }

    /// Adds a mesh to the frame.
    pub(crate) fn add(&mut self, adding: Adding) {
        let Adding {
            mesh,
            transform,
            camera,
            ..
        } = adding;
        let camera_at = camera.position;
        let world: Vec<Vec3> = if adding.in_view {
            // Placed relative to the camera: back into the world, so the
            // lamps and the sun light it where it really is.
            mesh.vertices
                .iter()
                .map(|&v| {
                    transform
                        .apply(v)
                        .rotate_x(camera.pitch)
                        .rotate_y(camera.yaw)
                        + camera_at
                })
                .collect()
        } else {
            mesh.vertices.iter().map(|&v| transform.apply(v)).collect()
        };
        let first = self.tris.len();
        for (index, triangle) in mesh.triangles.iter().enumerate() {
            let p = triangle_corners(&world, index, triangle);
            let face = (p[1] - p[0]).cross(p[2] - p[0]).normalized();
            self.tris.push(WorldTri {
                p,
                n: None,
                face,
                uv: mesh.uvs_of(index),
                color: triangle.color,
                glow: mesh.glow_of(index),
                texel_glow: mesh.uses_glow_map(index),
                front: face.dot(p[0] - camera_at) < 0.0,
            });
        }
        let tris = first..self.tris.len();
        if adding.smooth {
            smooth_normals(&mut self.tris[tris.clone()]);
        }
        self.draws.push(Draw {
            camera: *camera,
            lighting: adding.lighting,
            fog: adding.fog,
            tint: adding.tint,
            texture: mesh.texture.clone(),
            glow_map: mesh.glow_map.clone(),
            unlit: adding.unlit,
            in_view: adding.in_view,
            reset_depth: std::mem::take(&mut self.reset_next),
            smooth: adding.smooth,
            tris,
        });
    }
}

/// Gives every corner the average of the faces round it (bigger faces
/// count for more), leaving out faces turned more than
/// [`CREASE_DEGREES`] from its own: round things go round, edges stay.
/// Corners at the same point are found by their position, so meshes made
/// of separate triangles are smoothed too.
fn smooth_normals(tris: &mut [WorldTri]) {
    let cos_crease = CREASE_DEGREES.to_radians().cos();
    let weighted: Vec<Vec3> = tris
        .iter()
        .map(|t| (t.p[1] - t.p[0]).cross(t.p[2] - t.p[0]))
        .collect();
    // Every corner, sorted by where it is (adding 0.0 makes -0.0 into 0.0).
    let key = |p: Vec3| {
        [
            (p.x + 0.0).to_bits(),
            (p.y + 0.0).to_bits(),
            (p.z + 0.0).to_bits(),
        ]
    };
    let mut corners: Vec<([u32; 3], u32)> = Vec::with_capacity(tris.len() * 3);
    for (t, tri) in tris.iter().enumerate() {
        for c in 0..3 {
            corners.push((key(tri.p[c]), (t * 3 + c) as u32));
        }
    }
    corners.sort_unstable();
    let mut normals = vec![[Vec3::ZERO; 3]; tris.len()];
    let mut start = 0;
    while start < corners.len() {
        let mut end = start + 1;
        while end < corners.len() && corners[end].0 == corners[start].0 {
            end += 1;
        }
        let group = &corners[start..end];
        for &(_, corner) in group {
            let t = corner as usize / 3;
            let own = tris[t].face;
            let mut sum = Vec3::ZERO;
            for &(_, other) in group {
                let o = other as usize / 3;
                if tris[o].face.dot(own) >= cos_crease {
                    sum += weighted[o];
                }
            }
            let n = sum.normalized();
            normals[t][corner as usize % 3] = if n == Vec3::ZERO { own } else { n };
        }
        start = end;
    }
    for (tri, n) in tris.iter_mut().zip(normals) {
        tri.n = Some(n);
    }
}

/// The light of one draw, worked out once.
struct DrawLight {
    sun: Vec3,
    sun_rgb: [f32; 3],
    ambient: f32,
    /// The sun's shadow map is for this draw's sun.
    shadowed: bool,
}

/// A lamp, ready to light points.
struct LampLight {
    position: Vec3,
    rgb: [f32; 3],
    reach2: f32,
    /// The way it points, the cosine of its cone's half, and 1 / the
    /// cosines its soft edge covers.
    spot: Option<(Vec3, f32, f32)>,
    shadow: Option<usize>,
}

impl LampLight {
    fn new(lamp: &Lamp) -> Option<LampLight> {
        let power = if lamp.power.is_finite() {
            lamp.power.max(0.0)
        } else {
            0.0
        };
        let reach = if lamp.reach.is_finite() {
            lamp.reach.max(0.0)
        } else {
            0.0
        };
        if power == 0.0 || reach == 0.0 {
            return None;
        }
        let c = lamp.color;
        let spot = lamp.spot.map(|spot| {
            let half = spot.cone.clamp(0.02, 3.0) / 2.0;
            let inner = half * (1.0 - spot.softness.clamp(0.0, 1.0));
            let (outer_cos, inner_cos) = (half.cos(), inner.cos());
            let direction = match spot.direction.normalized() {
                d if d == Vec3::ZERO => vec3(0.0, 0.0, 1.0),
                d => d,
            };
            (
                direction,
                outer_cos,
                1.0 / (inner_cos - outer_cos).max(1e-6),
            )
        });
        Some(LampLight {
            position: lamp.position,
            rgb: [c.r, c.g, c.b].map(|v| v as f32 / 255.0 * power),
            reach2: reach * reach,
            spot,
            shadow: None,
        })
    }
}

/// A picture of how far the light gets, seen from a light: the distance
/// to the nearest thing at each point (infinity: nothing).
struct DepthMap {
    size: usize,
    depth: Vec<f32>,
}

/// A triangle seen from a light, ready to draw into a depth map.
struct MapTri {
    xy: [Vec2; 3],
    /// Distance from the light at each corner (orthographic), or 1 / it
    /// (perspective).
    z: [f32; 3],
    perspective: bool,
    /// Texture coordinates (over distance, in perspective) and the draw
    /// whose texture they're for, so a fence's holes let light through.
    uv: Option<([Vec2; 3], u32)>,
    area: f32,
    bounds: [usize; 4],
}

/// The sun's shadow map: what's in the way of the sun, over what the
/// camera can see.
struct SunMap {
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    sun: Vec3,
    origin: Vec2,
    scale: Vec2,
    /// The size of one of its pixels in the world.
    texel: f32,
    map: DepthMap,
}

/// One of a lamp's shadow maps (a spot lamp has one, a point lamp six).
struct FaceMap {
    right: Vec3,
    up: Vec3,
    forward: Vec3,
    tan_half: f32,
    map: DepthMap,
}

/// A lamp's shadow maps.
struct LampMap {
    position: Vec3,
    faces: Vec<FaceMap>,
    point: bool,
}

/// How soft shadows are and how dark.
#[derive(Clone, Copy)]
struct ShadowLook {
    taps: i32,
    darkness: f32,
}

/// A triangle on the screen (in sample pixels), ready to draw.
struct ScreenTri {
    xy: [Vec2; 3],
    inv_z: [f32; 3],
    uv_over_z: [Vec2; 3],
    world: [Vec3; 3],
    normal: [Vec3; 3],
    look: u32,
    draw: u32,
    /// Which run between depth resets it's in.
    seg: u32,
    see_through: bool,
    textured: bool,
    area: f32,
    /// Its box: x from `x0` to `x1`, y from `y0` to `y1` (not included).
    x0: usize,
    x1: usize,
    y0: usize,
    y1: usize,
}

/// What a triangle looks like, worked out once.
struct TriLook {
    /// Its colour times the tint, 0 to 1.
    albedo: [f32; 3],
    glow: f32,
    /// Its texture glows in places: the glow map, and what a glowing
    /// pixel's colour is worked out from (the exact light, its colour and
    /// the tint).
    texel_glow: bool,
    brightness: [f32; 3],
    own: Color,
    tint: Color,
    face: Vec3,
    smooth: bool,
    unlit: bool,
    /// The same light all over it (no lamps, no shadows, flat shading):
    /// its colour exactly as the plain renderer works it out.
    exact: Option<Exact>,
}

#[derive(Clone, Copy)]
enum Exact {
    /// A plain triangle's colour.
    Color(u32),
    /// What a textured triangle's texture is multiplied by, out of 65,536.
    Shade([u32; 3]),
}

/// Everything the bands read while they draw.
struct Frame<'a> {
    draws: &'a [Draw],
    tris: Vec<ScreenTri>,
    looks: Vec<TriLook>,
    lights: Vec<DrawLight>,
    lamps: Vec<LampLight>,
    sun_map: Option<SunMap>,
    lamp_maps: Vec<LampMap>,
    shadow_look: ShadowLook,
    bloom: bool,
    /// Output size, the sample factor, and the last run between resets.
    width: usize,
    ss: usize,
    last_seg: u32,
}

/// Draws the queue into `pixels` (and `depth`, both the canvas's size),
/// then empties it.
pub(crate) fn render(
    queue: &mut Queue,
    effects: &Effects,
    width: usize,
    height: usize,
    pixels: &mut [u32],
    depth: &mut [f32],
) {
    if queue.draws.is_empty() || width == 0 || height == 0 {
        queue.clear();
        return;
    }
    let ss = effects.samples();
    let threads = effects.thread_count();
    let draws = &queue.draws;
    let tris = &queue.tris;

    // The world's camera and light: the first mesh not drawn in view.
    let main = draws.iter().find(|d| !d.in_view).unwrap_or(&draws[0]);
    let (sun_map, lamp_maps, lamps) = match effects.shadows {
        Some(shadows) => shadow_maps(
            draws,
            tris,
            &effects.lamps,
            main,
            &shadows,
            width as f32 / height as f32,
            threads,
        ),
        None => (
            None,
            Vec::new(),
            effects.lamps.iter().filter_map(LampLight::new).collect(),
        ),
    };
    let shadow_look = effects.shadows.map_or(
        ShadowLook {
            taps: 0,
            darkness: 0.0,
        },
        |s| ShadowLook {
            taps: if s.softness.is_finite() {
                s.softness.round().clamp(0.0, 4.0) as i32
            } else {
                1
            },
            darkness: if s.darkness.is_finite() {
                s.darkness.clamp(0.0, 1.0)
            } else {
                1.0
            },
        },
    );
    let lights: Vec<DrawLight> = draws
        .iter()
        .map(|d| {
            let sun = d.lighting.sun();
            DrawLight {
                sun,
                sun_rgb: d.lighting.sun_rgb(),
                ambient: d.lighting.ambient_level(),
                shadowed: sun_map.as_ref().is_some_and(|m| m.sun == sun),
            }
        })
        .collect();

    // Onto the screen, at the samples' size.
    let (sw, sh) = (width * ss, height * ss);
    let mut screen = Vec::new();
    let mut looks = Vec::new();
    let mut seg = 0u32;
    for (d, draw) in draws.iter().enumerate() {
        if draw.reset_depth && d > 0 {
            seg += 1;
        }
        let projection = draw.camera.projection_for(sw as f32, sh as f32);
        let light = &lights[d];
        let flat = lamps.is_empty() && !light.shadowed && !draw.smooth;
        for tri in &tris[draw.tris.clone()] {
            if !tri.front {
                continue;
            }
            let look = tri_look(tri, draw, light, flat);
            let look_index = looks.len() as u32;
            looks.push(look);
            project(tri, draw, &projection, |corners| {
                let corners: [Corner; 3] = corners;
                let xy = corners.map(|c| c.screen);
                let area = edge(xy[0], xy[1], xy[2]);
                if area == 0.0 || !area.is_finite() {
                    return;
                }
                let x0 = (xy[0].x.min(xy[1].x).min(xy[2].x).floor().max(0.0)) as usize;
                let y0 = (xy[0].y.min(xy[1].y).min(xy[2].y).floor().max(0.0)) as usize;
                let x1 = (xy[0].x.max(xy[1].x).max(xy[2].x).ceil().max(0.0) as usize).min(sw);
                let y1 = (xy[0].y.max(xy[1].y).max(xy[2].y).ceil().max(0.0) as usize).min(sh);
                if x0 >= x1 || y0 >= y1 {
                    return;
                }
                screen.push(ScreenTri {
                    xy,
                    inv_z: corners.map(|c| 1.0 / c.camera.z),
                    uv_over_z: corners.map(|c| c.uv * (1.0 / c.camera.z)),
                    world: corners.map(|c| c.world),
                    normal: corners.map(|c| c.normal),
                    look: look_index,
                    draw: d as u32,
                    seg,
                    see_through: draw.tint.a < 255,
                    textured: tri.uv.is_some() && draw.texture.is_some(),
                    area,
                    x0,
                    x1,
                    y0,
                    y1,
                });
            });
        }
    }

    // Which triangles touch which band.
    let band_samples = BAND_ROWS * ss;
    let bands = height.div_ceil(BAND_ROWS);
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); bands];
    for (i, t) in screen.iter().enumerate() {
        for bin in &mut bins[t.y0 / band_samples..=(t.y1 - 1) / band_samples] {
            bin.push(i as u32);
        }
    }

    let frame = Frame {
        draws,
        tris: screen,
        looks,
        lights,
        lamps,
        sun_map,
        lamp_maps,
        shadow_look,
        bloom: effects.bloom.is_some(),
        width,
        ss,
        last_seg: seg,
    };
    let mut glow = if frame.bloom {
        vec![[0.0f32; 3]; width * height]
    } else {
        Vec::new()
    };
    {
        let mut jobs = Vec::with_capacity(bands);
        let mut glow_rows = glow.chunks_mut(BAND_ROWS * width);
        for (band, (out, out_depth)) in pixels
            .chunks_mut(BAND_ROWS * width)
            .zip(depth.chunks_mut(BAND_ROWS * width))
            .enumerate()
        {
            jobs.push((band, out, out_depth, glow_rows.next()));
        }
        let bins = &bins;
        let frame = &frame;
        run_jobs(
            threads,
            jobs,
            Scratch::default,
            |scratch, (band, out, d, g)| {
                frame.band(band, &bins[band], out, d, g, scratch);
            },
        );
    }
    drop(frame);
    if let Some(bloom) = effects.bloom {
        add_bloom(pixels, &glow, width, height, bloom, threads);
    }
    queue.clear();
}

/// The colour of a triangle with the same light all over it, exactly as
/// the plain renderer works it out.
fn tri_look(tri: &WorldTri, draw: &Draw, light: &DrawLight, flat: bool) -> TriLook {
    let tint = draw.tint;
    let c = tri.color;
    let albedo = [
        c.r as f32 / 255.0 * tint.r as f32 / 255.0,
        c.g as f32 / 255.0 * tint.g as f32 / 255.0,
        c.b as f32 / 255.0 * tint.b as f32 / 255.0,
    ];
    let glow = tri.glow;
    let constant = flat || draw.unlit || glow >= 1.0;
    let mut kept = [1.0; 3];
    let exact = constant.then(|| {
        let brightness = if glow >= 1.0 || draw.unlit {
            [1.0; 3]
        } else {
            let b = sun_brightness(light, tri.face);
            if glow > 0.0 {
                b.map(|b| b.max(glow))
            } else {
                b
            }
        };
        kept = brightness;
        if tri.uv.is_some() && draw.texture.is_some() {
            Exact::Shade(texture_shade(c, brightness, tint))
        } else {
            let lit = if brightness[0] == brightness[1] && brightness[1] == brightness[2] {
                Color::BLACK.lerp(c, brightness[0])
            } else {
                let mix = |c: u8, b: f32| (c as f32 * b.clamp(0.0, 1.0)).round() as u8;
                Color::rgb(
                    mix(c.r, brightness[0]),
                    mix(c.g, brightness[1]),
                    mix(c.b, brightness[2]),
                )
            };
            Exact::Color(tinted(lit, tint).to_u32())
        }
    });
    TriLook {
        albedo,
        glow,
        texel_glow: tri.texel_glow && !draw.unlit && draw.glow_map.is_some(),
        brightness: kept,
        own: c,
        tint,
        face: tri.face,
        smooth: tri.n.is_some(),
        unlit: draw.unlit,
        exact,
    }
}

/// The plain renderer's light for a face: `ambient`, plus the sun.
fn sun_brightness(light: &DrawLight, normal: Vec3) -> [f32; 3] {
    let sunlight = normal.dot(light.sun).max(0.0);
    let ambient = light.ambient;
    light
        .sun_rgb
        .map(|share| ambient + (1.0 - ambient) * (sunlight * share))
}

/// `color` times `tint`, as the plain renderer does it.
fn tinted(color: Color, tint: Color) -> Color {
    let times = |c: u8, t: u8| ((c as u32 * t as u32 + 127) / 255) as u8;
    Color::rgb(
        times(color.r, tint.r),
        times(color.g, tint.g),
        times(color.b, tint.b),
    )
}

/// A corner of a triangle on its way to the screen.
#[derive(Clone, Copy)]
struct Corner {
    camera: Vec3,
    world: Vec3,
    normal: Vec3,
    uv: Vec2,
    screen: Vec2,
}

/// Puts a triangle on the screen: into camera space, cut where it's too
/// close, and projected. Calls `emit` with each of the (0 to 2) triangles.
fn project(
    tri: &WorldTri,
    draw: &Draw,
    projection: &crate::render3d::Projection,
    mut emit: impl FnMut([Corner; 3]),
) {
    let normals = tri.n.unwrap_or([tri.face; 3]);
    let uvs = if draw.texture.is_some() {
        tri.uv.unwrap_or([Vec2::ZERO; 3])
    } else {
        [Vec2::ZERO; 3]
    };
    let corners: [Corner; 3] = [0, 1, 2].map(|i| Corner {
        camera: draw.camera.to_camera_space(tri.p[i]),
        world: tri.p[i],
        normal: normals[i],
        uv: uvs[i],
        screen: Vec2::ZERO,
    });
    // Cut against the near plane, as the plain renderer does (the same
    // sums, so the same points).
    let mut out = [corners[0]; 4];
    let mut count = 0;
    for i in 0..3 {
        let current = corners[i];
        let next = corners[(i + 1) % 3];
        let current_inside = current.camera.z >= NEAR;
        if current_inside {
            out[count] = current;
            count += 1;
        }
        if current_inside != (next.camera.z >= NEAR) {
            let t = (NEAR - current.camera.z) / (next.camera.z - current.camera.z);
            out[count] = Corner {
                camera: current.camera + (next.camera - current.camera) * t,
                world: current.world + (next.world - current.world) * t,
                normal: current.normal + (next.normal - current.normal) * t,
                uv: current.uv + (next.uv - current.uv) * t,
                screen: Vec2::ZERO,
            };
            count += 1;
        }
    }
    for corner in &mut out[..count] {
        corner.screen = projection.project(corner.camera);
    }
    for i in 1..count.saturating_sub(1) {
        emit([out[0], out[i], out[i + 1]]);
    }
}

/// Which side of the line from `a` to `b` is `p` on? (As in `canvas.rs`.)
#[inline]
fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// How much each corner owns the point `p`, if it's inside the triangle.
/// (Each weight is its edge value over the area; a point is inside when
/// all three are at least 0, which the signs of the edge values say
/// without dividing, so points outside cost no divisions.)
#[inline]
fn weights_at(xy: &[Vec2; 3], area: f32, p: Vec2) -> Option<[f32; 3]> {
    let [a, b, c] = *xy;
    let e = [edge(b, c, p), edge(c, a, p), edge(a, b, p)];
    let inside = if area > 0.0 {
        e.iter().all(|&v| v >= 0.0)
    } else {
        e.iter().all(|&v| v <= 0.0)
    };
    inside.then(|| [e[0] / area, e[1] / area, e[2] / area])
}

/// `x.floor()`, as a whole number, without calling the maths library
/// (a big share of the time on computers without a rounding instruction).
#[inline]
fn fast_floor(x: f32) -> f32 {
    let whole = x as i64 as f32;
    if whole > x { whole - 1.0 } else { whole }
}

/// A texture pixel number, wrapped round into `0..size`.
#[inline]
fn wrap(position: f32, size: usize) -> usize {
    if position >= 0.0 && position < size as f32 {
        position as usize
    } else {
        (position as i64).rem_euclid(size as i64) as usize
    }
}

/// Each thread's own working space, kept from band to band.
#[derive(Default)]
struct Scratch {
    colors: Vec<u32>,
    depth: Vec<f32>,
    ids: Vec<u32>,
    marks: Vec<u32>,
    glow: Vec<[f32; 3]>,
}

impl Frame<'_> {
    /// Draws one band: `out` and `out_depth` are its rows of the canvas.
    fn band(
        &self,
        band: usize,
        list: &[u32],
        out: &mut [u32],
        out_depth: &mut [f32],
        out_glow: Option<&mut [[f32; 3]]>,
        s: &mut Scratch,
    ) {
        let ss = self.ss;
        let width = self.width;
        let sw = width * ss;
        let rows = out.len() / width;
        let srow0 = band * BAND_ROWS * ss;
        let srows = rows * ss;
        let samples = sw * srows;
        if list.is_empty() {
            if self.last_seg > 0 {
                out_depth.fill(0.0);
            }
            return;
        }
        s.colors.clear();
        s.depth.clear();
        for sy in 0..srows {
            let row = (sy / ss) * width;
            for sx in 0..sw {
                s.colors.push(out[row + sx / ss]);
                s.depth.push(out_depth[row + sx / ss]);
            }
        }
        s.ids.clear();
        s.ids.resize(samples, NONE);
        s.marks.clear();
        s.marks.resize(samples, NONE);
        if self.bloom {
            s.glow.clear();
            s.glow.resize(samples, [0.0; 3]);
        }
        let band_rows = srow0..srow0 + srows;
        let mut depth_seg = 0;
        let mut i = 0;
        while i < list.len() {
            let seg = self.tris[list[i] as usize].seg;
            let mut end = i + 1;
            while end < list.len() && self.tris[list[end] as usize].seg == seg {
                end += 1;
            }
            let run = &list[i..end];
            if seg != depth_seg {
                s.depth.fill(0.0);
                depth_seg = seg;
            }
            s.ids.fill(NONE);
            // 1. The nearest solid triangle at each point.
            for &index in run {
                let t = &self.tris[index as usize];
                if t.see_through {
                    continue;
                }
                self.each_sample(t, band_rows.clone(), |x, y, w| {
                    let at = (y - srow0) * sw + x;
                    let d = w[0] * t.inv_z[0] + w[1] * t.inv_z[1] + w[2] * t.inv_z[2];
                    if d > s.depth[at] && (!t.textured || (d > 0.0 && self.solid_texel(t, w, d))) {
                        s.depth[at] = d;
                        s.ids[at] = index;
                    }
                });
            }
            // 2. Lit once each: once per triangle per pixel (with
            // supersampling, a pixel's samples on the same triangle share
            // its light, and only the edges get the extra work).
            let mut lit: Vec<(u32, u32, [f32; 3])> = Vec::with_capacity(ss * ss);
            for oy in 0..rows {
                for ox in 0..width {
                    lit.clear();
                    for sy in 0..ss {
                        let row = (oy * ss + sy) * sw + ox * ss;
                        for at in row..row + ss {
                            let index = s.ids[at];
                            if index == NONE {
                                continue;
                            }
                            let (color, glow) = match lit.iter().find(|l| l.0 == index) {
                                Some(&(_, color, glow)) => (color, glow),
                                None => {
                                    let t = &self.tris[index as usize];
                                    let (x, y) = (at % sw, srow0 + at / sw);
                                    let p = vec2(x as f32 + 0.5, y as f32 + 0.5);
                                    let w = weights_at(&t.xy, t.area, p).unwrap_or([1.0 / 3.0; 3]);
                                    let (color, glow) = self.shade(t, w, s.depth[at]);
                                    lit.push((index, color, glow));
                                    (color, glow)
                                }
                            };
                            s.colors[at] = color;
                            if self.bloom {
                                s.glow[at] = glow;
                            }
                        }
                    }
                }
            }
            // 3. See-through triangles, mixed in, each point once per mesh.
            for &index in run {
                let t = &self.tris[index as usize];
                if !t.see_through {
                    continue;
                }
                let opacity = self.draws[t.draw as usize].tint.a;
                self.each_sample(t, band_rows.clone(), |x, y, w| {
                    let at = (y - srow0) * sw + x;
                    let d = w[0] * t.inv_z[0] + w[1] * t.inv_z[1] + w[2] * t.inv_z[2];
                    if d <= s.depth[at] || s.marks[at] == t.draw {
                        return;
                    }
                    if t.textured && (d <= 0.0 || !self.solid_texel(t, w, d)) {
                        return;
                    }
                    s.marks[at] = t.draw;
                    let (color, glow) = self.shade(t, w, d);
                    s.colors[at] = color::blend(s.colors[at], color, opacity);
                    if self.bloom {
                        let k = opacity as f32 / 255.0;
                        for (g, add) in s.glow[at].iter_mut().zip(glow) {
                            *g += add * k;
                        }
                    }
                });
            }
            i = end;
        }
        if self.last_seg != depth_seg {
            s.depth.fill(0.0);
        }
        // 4. Each pixel: the average of its samples.
        let n = (ss * ss) as u32;
        for oy in 0..rows {
            for ox in 0..width {
                let o = oy * width + ox;
                if ss == 1 {
                    out[o] = s.colors[o];
                    out_depth[o] = s.depth[o];
                    continue;
                }
                let (mut r, mut g, mut b, mut near) = (0u32, 0u32, 0u32, 0.0f32);
                for sy in 0..ss {
                    let row = (oy * ss + sy) * sw + ox * ss;
                    for sx in 0..ss {
                        let c = s.colors[row + sx];
                        r += (c >> 16) & 0xff;
                        g += (c >> 8) & 0xff;
                        b += c & 0xff;
                        near = near.max(s.depth[row + sx]);
                    }
                }
                let avg = |v: u32| (v + n / 2) / n;
                out[o] = (avg(r) << 16) | (avg(g) << 8) | avg(b);
                out_depth[o] = near;
            }
        }
        if let Some(out_glow) = out_glow {
            let k = 1.0 / n as f32;
            for oy in 0..rows {
                for ox in 0..width {
                    let mut sum = [0.0f32; 3];
                    for sy in 0..ss {
                        let row = (oy * ss + sy) * sw + ox * ss;
                        for sx in 0..ss {
                            let g = s.glow[row + sx];
                            sum[0] += g[0];
                            sum[1] += g[1];
                            sum[2] += g[2];
                        }
                    }
                    out_glow[oy * width + ox] = sum.map(|v| v * k);
                }
            }
        }
    }

    /// Calls `plot(x, y, weights)` for every sample of `t` in `rows`, as
    /// the plain renderer's `for_each_pixel_in_triangle` does.
    #[inline]
    fn each_sample(
        &self,
        t: &ScreenTri,
        rows: Range<usize>,
        mut plot: impl FnMut(usize, usize, [f32; 3]),
    ) {
        let y0 = t.y0.max(rows.start);
        let y1 = t.y1.min(rows.end);
        for y in y0..y1 {
            for x in t.x0..t.x1 {
                let p = vec2(x as f32 + 0.5, y as f32 + 0.5);
                if let Some(w) = weights_at(&t.xy, t.area, p) {
                    plot(x, y, w);
                }
            }
        }
    }

    /// Where on its texture a point of `t` is: (pixel x, pixel y).
    #[inline]
    fn texel_at(&self, t: &ScreenTri, w: [f32; 3], d: f32, texture: &Sprite) -> usize {
        self.texel_xy(t, w, d, texture).0
    }

    /// [`texel_at`](Self::texel_at), with the pixel's x and y too.
    #[inline]
    fn texel_xy(
        &self,
        t: &ScreenTri,
        w: [f32; 3],
        d: f32,
        texture: &Sprite,
    ) -> (usize, usize, usize) {
        let (tw, th) = (texture.width(), texture.height());
        let to_texture = 1.0 / d;
        let u = (w[0] * t.uv_over_z[0].x + w[1] * t.uv_over_z[1].x + w[2] * t.uv_over_z[2].x)
            * to_texture;
        let v = (w[0] * t.uv_over_z[0].y + w[1] * t.uv_over_z[1].y + w[2] * t.uv_over_z[2].y)
            * to_texture;
        let x = wrap(fast_floor(u * tw as f32), tw);
        let y = wrap(fast_floor(v * th as f32), th);
        let (_, _, row, start) = texture.texels();
        (start + y * row + x, x, y)
    }

    /// Is the texture solid at this point (not a see-through pixel)?
    fn solid_texel(&self, t: &ScreenTri, w: [f32; 3], d: f32) -> bool {
        match &self.draws[t.draw as usize].texture {
            Some(texture) if texture.width() > 0 && texture.height() > 0 => {
                let at = self.texel_at(t, w, d, texture);
                texture.texels().1[at] >= 128
            }
            _ => false,
        }
    }

    /// The colour of a point of `t`, and its glow (for bloom), 0 to 255.
    fn shade(&self, t: &ScreenTri, w: [f32; 3], d: f32) -> (u32, [f32; 3]) {
        let look = &self.looks[t.look as usize];
        let draw = &self.draws[t.draw as usize];
        let mut texel_glow = 0.0;
        let texel = if t.textured {
            draw.texture.as_ref().map(|texture| {
                let (at, x, y) = self.texel_xy(t, w, d, texture);
                if look.texel_glow {
                    if let Some(map) = &draw.glow_map {
                        texel_glow = f32::from(map.raw(x, y)) / f32::from(GLOW_LEVELS);
                    }
                }
                texture.texels().0[at]
            })
        } else {
            None
        };
        let color = match (look.exact, texel) {
            (Some(Exact::Color(c)), None) => c,
            (Some(Exact::Shade(k)), Some(rgb)) => {
                // A glowing pixel of the texture: at least as bright as it
                // glows (worked out as the plain renderer does).
                let k = if texel_glow > 0.0 && look.brightness.iter().any(|&b| b < texel_glow) {
                    texture_shade(
                        look.own,
                        look.brightness.map(|b| b.max(texel_glow)),
                        look.tint,
                    )
                } else {
                    k
                };
                let channel = |shift: u32, k: u32| {
                    let c = ((rgb >> shift) & 0xff) as u64 * k as u64;
                    (((c + 32768) >> 16).min(255) as u32) << shift
                };
                channel(16, k[0]) | channel(8, k[1]) | channel(0, k[2])
            }
            _ => self.light_point(t, look, w, d, texel, texel_glow),
        };
        let fog = draw.fog.map_or(0, |f| f.amount(1.0 / d));
        let fogged = match (draw.fog, fog) {
            (Some(f), amount) if amount > 0 => color::blend(color, f.color.to_u32(), amount),
            _ => color,
        };
        let glows = look.glow.max(texel_glow);
        let glow = if self.bloom && glows > 0.0 && !look.unlit {
            let k = glows * (1.0 - fog as f32 / 255.0);
            [
                ((color >> 16) & 0xff) as f32 * k,
                ((color >> 8) & 0xff) as f32 * k,
                (color & 0xff) as f32 * k,
            ]
        } else {
            [0.0; 3]
        };
        (fogged, glow)
    }

    /// The colour of a point lit by everything, worked out at the point.
    fn light_point(
        &self,
        t: &ScreenTri,
        look: &TriLook,
        w: [f32; 3],
        d: f32,
        texel: Option<u32>,
        texel_glow: f32,
    ) -> u32 {
        let glow = look.glow.max(texel_glow);
        // Perspective-correct weights for the point's place and normal.
        let k = [
            w[0] * t.inv_z[0] / d,
            w[1] * t.inv_z[1] / d,
            w[2] * t.inv_z[2] / d,
        ];
        let position = t.world[0] * k[0] + t.world[1] * k[1] + t.world[2] * k[2];
        let normal = if look.smooth {
            match (t.normal[0] * k[0] + t.normal[1] * k[1] + t.normal[2] * k[2]).normalized() {
                n if n == Vec3::ZERO => look.face,
                n => n,
            }
        } else {
            look.face
        };
        let brightness = if look.unlit || glow >= 1.0 {
            [1.0; 3]
        } else {
            let light = &self.lights[t.draw as usize];
            let mut sunlight = normal.dot(light.sun).max(0.0);
            if sunlight > 0.0 && light.shadowed {
                if let Some(map) = &self.sun_map {
                    let lit = map.light(position, normal, self.shadow_look.taps);
                    sunlight *= 1.0 - self.shadow_look.darkness * (1.0 - lit);
                }
            }
            let ambient = light.ambient;
            let mut b = light
                .sun_rgb
                .map(|share| ambient + (1.0 - ambient) * (sunlight * share));
            for lamp in &self.lamps {
                self.add_lamp(lamp, position, normal, &mut b);
            }
            if glow > 0.0 {
                b = b.map(|b| b.max(glow));
            }
            b
        };
        let base = match texel {
            Some(rgb) => [
                look.albedo[0] * ((rgb >> 16) & 0xff) as f32 / 255.0,
                look.albedo[1] * ((rgb >> 8) & 0xff) as f32 / 255.0,
                look.albedo[2] * (rgb & 0xff) as f32 / 255.0,
            ],
            None => look.albedo,
        };
        let c = |i: usize| (base[i] * brightness[i] * 255.0).round().clamp(0.0, 255.0) as u32;
        (c(0) << 16) | (c(1) << 8) | c(2)
    }

    /// Adds a lamp's light at a point.
    #[inline]
    fn add_lamp(&self, lamp: &LampLight, position: Vec3, normal: Vec3, b: &mut [f32; 3]) {
        let to_lamp = lamp.position - position;
        let d2 = to_lamp.dot(to_lamp);
        if d2 >= lamp.reach2 || d2 == 0.0 {
            return;
        }
        let distance = d2.sqrt();
        let l = to_lamp * (1.0 / distance);
        let facing = normal.dot(l);
        if facing <= 0.0 {
            return;
        }
        // Fading with distance, and to nothing at its reach.
        let x = d2 / lamp.reach2;
        let window = (1.0 - x * x).max(0.0);
        let mut amount = facing * 2.0 * window * window / (1.0 + d2);
        if let Some((direction, outer, inverse)) = lamp.spot {
            let along = -l.dot(direction);
            if along <= outer {
                return;
            }
            let f = ((along - outer) * inverse).min(1.0);
            amount *= f * f * (3.0 - 2.0 * f);
        }
        if let Some(map) = lamp.shadow {
            let lit = self.lamp_maps[map].light(position, normal, self.shadow_look.taps);
            amount *= 1.0 - self.shadow_look.darkness * (1.0 - lit);
        }
        if amount <= 0.0 {
            return;
        }
        for (b, rgb) in b.iter_mut().zip(lamp.rgb) {
            *b += rgb * amount;
        }
    }
}

/// Runs `work` on every job, on up to `threads` threads at once, each with
/// its own scratch space. Jobs are independent, so it doesn't matter which
/// thread does which.
fn run_jobs<J: Send, S>(
    threads: usize,
    jobs: Vec<J>,
    scratch: impl Fn() -> S + Sync,
    work: impl Fn(&mut S, J) + Sync,
) {
    if threads <= 1 || jobs.len() <= 1 {
        let mut space = scratch();
        for job in jobs {
            work(&mut space, job);
        }
        return;
    }
    let count = threads.min(jobs.len());
    let queue = Mutex::new(jobs);
    std::thread::scope(|scope| {
        for _ in 0..count {
            scope.spawn(|| {
                let mut space = scratch();
                loop {
                    let job = queue.lock().map(|mut jobs| jobs.pop());
                    match job {
                        Ok(Some(job)) => work(&mut space, job),
                        _ => break,
                    }
                }
            });
        }
    });
}

/// Right and up for a light looking along `forward`.
fn basis(forward: Vec3) -> (Vec3, Vec3) {
    let hint = if forward.y.abs() < 0.99 {
        Vec3::UP
    } else {
        vec3(0.0, 0.0, 1.0)
    };
    let right = hint.cross(forward).normalized();
    (right, forward.cross(right))
}

/// The sun's shadow map and the lamps' (and the lamps, ready to light).
fn shadow_maps(
    draws: &[Draw],
    tris: &[WorldTri],
    lamps: &[Lamp],
    main: &Draw,
    shadows: &Shadows,
    aspect: f32,
    threads: usize,
) -> (Option<SunMap>, Vec<LampMap>, Vec<LampLight>) {
    // What casts shadows: every solid triangle not drawn in view, except
    // glowing ones (a bulb doesn't shadow its own light).
    let casters: Vec<(usize, usize)> = draws
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.in_view && d.tint.a == 255)
        .flat_map(|(i, d)| d.tris.clone().map(move |t| (i, t)))
        .filter(|&(_, t)| tris[t].glow < 1.0)
        .collect();
    let sun_map = sun_map(draws, tris, &casters, main, shadows, aspect, threads);
    let mut lamp_maps = Vec::new();
    let mut lights = Vec::new();
    for lamp in lamps {
        let Some(mut light) = LampLight::new(lamp) else {
            continue;
        };
        if lamp.shadows {
            if let Some(map) = lamp_map(draws, tris, &casters, lamp, shadows, threads) {
                light.shadow = Some(lamp_maps.len());
                lamp_maps.push(map);
            }
        }
        lights.push(light);
    }
    (sun_map, lamp_maps, lights)
}

fn sun_map(
    draws: &[Draw],
    tris: &[WorldTri],
    casters: &[(usize, usize)],
    main: &Draw,
    shadows: &Shadows,
    aspect: f32,
    threads: usize,
) -> Option<SunMap> {
    let lighting = main.lighting;
    if lighting.sun_rgb().iter().all(|&v| v == 0.0) || casters.is_empty() {
        return None;
    }
    let sun = lighting.sun();
    let forward = -sun;
    let (right, up) = basis(forward);
    let to_light = |p: Vec3| vec2(p.dot(right), p.dot(up));
    // What the camera sees, as far as the shadows reach.
    let camera = &main.camera;
    let far = if shadows.distance.is_finite() {
        shadows.distance.max(NEAR * 2.0)
    } else {
        30.0
    };
    let tan_v = (camera.usable_fov() / 2.0).tan();
    let tan_h = tan_v * aspect;
    let (mut low, mut high) = (vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN));
    for z in [NEAR, far] {
        for (sx, sy) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
            let c = vec3(sx * tan_h * z, sy * tan_v * z, z);
            let world = c.rotate_x(camera.pitch).rotate_y(camera.yaw) + camera.position;
            let q = to_light(world);
            low = vec2(low.x.min(q.x), low.y.min(q.y));
            high = vec2(high.x.max(q.x), high.y.max(q.y));
        }
    }
    // No bigger than the things that cast shadows.
    let (mut c_low, mut c_high) = (vec2(f32::MAX, f32::MAX), vec2(f32::MIN, f32::MIN));
    for &(_, t) in casters {
        for p in tris[t].p {
            let q = to_light(p);
            c_low = vec2(c_low.x.min(q.x), c_low.y.min(q.y));
            c_high = vec2(c_high.x.max(q.x), c_high.y.max(q.y));
        }
    }
    let low = vec2(low.x.max(c_low.x), low.y.max(c_low.y));
    let high = vec2(high.x.min(c_high.x), high.y.min(c_high.y));
    if !(low.x < high.x && low.y < high.y) {
        return None;
    }
    let size = shadows.resolution.clamp(64, 8192);
    let span = (high.x - low.x).max(high.y - low.y);
    let margin = span * 2.0 / size as f32;
    let origin = vec2(low.x - margin, low.y - margin);
    let span = span + margin * 2.0;
    let texel = span / size as f32;
    let scale = vec2(size as f32 / span, size as f32 / span);
    let map_tris: Vec<MapTri> = casters
        .iter()
        .filter_map(|&(d, t)| {
            let tri = &tris[t];
            let xy = tri.p.map(|p| {
                let q = to_light(p);
                vec2((q.x - origin.x) * scale.x, (q.y - origin.y) * scale.y)
            });
            let uv = caster_uv(&draws[d], tri, d, None);
            MapTri::new(xy, tri.p.map(|p| p.dot(forward)), false, uv, size)
        })
        .collect();
    let map = DepthMap::draw(size, &map_tris, draws, threads);
    Some(SunMap {
        right,
        up,
        forward,
        sun,
        origin,
        scale,
        texel,
        map,
    })
}

/// A caster's texture coordinates (divided by `z` in perspective), if its
/// texture has see-through pixels to let light through.
fn caster_uv(
    draw: &Draw,
    tri: &WorldTri,
    d: usize,
    inv_z: Option<[f32; 3]>,
) -> Option<([Vec2; 3], u32)> {
    let uv = tri.uv?;
    draw.texture.as_ref()?;
    let uv = match inv_z {
        Some(k) => [uv[0] * k[0], uv[1] * k[1], uv[2] * k[2]],
        None => uv,
    };
    Some((uv, d as u32))
}

impl MapTri {
    fn new(
        xy: [Vec2; 3],
        z: [f32; 3],
        perspective: bool,
        uv: Option<([Vec2; 3], u32)>,
        size: usize,
    ) -> Option<MapTri> {
        let area = edge(xy[0], xy[1], xy[2]);
        if area == 0.0 || !area.is_finite() {
            return None;
        }
        let x0 = (xy[0].x.min(xy[1].x).min(xy[2].x).floor().max(0.0)) as usize;
        let y0 = (xy[0].y.min(xy[1].y).min(xy[2].y).floor().max(0.0)) as usize;
        let x1 = (xy[0].x.max(xy[1].x).max(xy[2].x).ceil().max(0.0) as usize).min(size);
        let y1 = (xy[0].y.max(xy[1].y).max(xy[2].y).ceil().max(0.0) as usize).min(size);
        (x0 < x1 && y0 < y1).then_some(MapTri {
            xy,
            z,
            perspective,
            uv,
            area,
            bounds: [x0, x1, y0, y1],
        })
    }
}

impl DepthMap {
    /// Draws the triangles' distances, keeping the nearest, in bands of
    /// rows on several threads.
    fn draw(size: usize, tris: &[MapTri], draws: &[Draw], threads: usize) -> DepthMap {
        let mut depth = vec![f32::INFINITY; size * size];
        const ROWS: usize = 32;
        let mut bins: Vec<Vec<u32>> = vec![Vec::new(); size.div_ceil(ROWS)];
        for (i, t) in tris.iter().enumerate() {
            for bin in &mut bins[t.bounds[2] / ROWS..=(t.bounds[3] - 1) / ROWS] {
                bin.push(i as u32);
            }
        }
        let jobs: Vec<(usize, &mut [f32])> = depth.chunks_mut(ROWS * size).enumerate().collect();
        let bins = &bins;
        run_jobs(
            threads,
            jobs,
            || (),
            |_, (band, rows)| {
                let y_start = band * ROWS;
                let y_end = y_start + rows.len() / size;
                for &i in &bins[band] {
                    let t = &tris[i as usize];
                    let [x0, x1, y0, y1] = t.bounds;
                    for y in y0.max(y_start)..y1.min(y_end) {
                        for x in x0..x1 {
                            let p = vec2(x as f32 + 0.5, y as f32 + 0.5);
                            let Some(w) = weights_at(&t.xy, t.area, p) else {
                                continue;
                            };
                            let mixed = w[0] * t.z[0] + w[1] * t.z[1] + w[2] * t.z[2];
                            let z = if t.perspective {
                                if mixed <= 0.0 {
                                    continue;
                                }
                                1.0 / mixed
                            } else {
                                mixed
                            };
                            let at = (y - y_start) * size + x;
                            if z >= rows[at] {
                                continue;
                            }
                            if let Some((uv, d)) = &t.uv {
                                let mut point = uv[0] * w[0] + uv[1] * w[1] + uv[2] * w[2];
                                if t.perspective {
                                    point = point * z;
                                }
                                if !solid_at(&draws[*d as usize], point) {
                                    continue;
                                }
                            }
                            rows[at] = z;
                        }
                    }
                }
            },
        );
        DepthMap { size, depth }
    }

    /// How much of a point at `x`, `y` (in map pixels) and `z` is lit:
    /// 0 in shadow, 1 not, in between at a soft edge.
    fn light(&self, x: f32, y: f32, z: f32, taps: i32) -> f32 {
        let (cx, cy) = (fast_floor(x) as i64, fast_floor(y) as i64);
        let size = self.size as i64;
        let mut lit = 0;
        let mut total = 0;
        for dy in -taps..=taps {
            for dx in -taps..=taps {
                let (px, py) = (cx + dx as i64, cy + dy as i64);
                total += 1;
                if px < 0 || py < 0 || px >= size || py >= size {
                    lit += 1;
                    continue;
                }
                if z <= self.depth[(py * size + px) as usize] {
                    lit += 1;
                }
            }
        }
        lit as f32 / total as f32
    }
}

/// Is a draw's texture solid at texture point `uv`?
fn solid_at(draw: &Draw, uv: Vec2) -> bool {
    let Some(texture) = &draw.texture else {
        return true;
    };
    let (tw, th) = (texture.width(), texture.height());
    if tw == 0 || th == 0 {
        return true;
    }
    let x = wrap(fast_floor(uv.x * tw as f32), tw);
    let y = wrap(fast_floor(uv.y * th as f32), th);
    let (_, alpha, row, start) = texture.texels();
    alpha[start + y * row + x] >= 128
}

impl SunMap {
    /// How much sun reaches `position` (on a face turned to `normal`).
    fn light(&self, position: Vec3, normal: Vec3, taps: i32) -> f32 {
        // Moved a little off the surface, so it doesn't shadow itself.
        let p = position + normal * (self.texel * 1.5);
        let x = (p.dot(self.right) - self.origin.x) * self.scale.x;
        let y = (p.dot(self.up) - self.origin.y) * self.scale.y;
        let z = p.dot(self.forward) - self.texel * 0.5;
        self.map.light(x, y, z, taps)
    }
}

fn lamp_map(
    draws: &[Draw],
    tris: &[WorldTri],
    casters: &[(usize, usize)],
    lamp: &Lamp,
    shadows: &Shadows,
    threads: usize,
) -> Option<LampMap> {
    let reach = lamp.reach;
    if !(reach.is_finite() && reach > 0.0) {
        return None;
    }
    let size = shadows.lamp_resolution.clamp(16, 4096);
    let position = lamp.position;
    // Only what's within its reach.
    let near: Vec<(usize, usize)> = casters
        .iter()
        .copied()
        .filter(|&(_, t)| {
            let p = tris[t].p;
            let low = vec3(
                p[0].x.min(p[1].x).min(p[2].x),
                p[0].y.min(p[1].y).min(p[2].y),
                p[0].z.min(p[1].z).min(p[2].z),
            );
            let high = vec3(
                p[0].x.max(p[1].x).max(p[2].x),
                p[0].y.max(p[1].y).max(p[2].y),
                p[0].z.max(p[1].z).max(p[2].z),
            );
            let closest = vec3(
                position.x.clamp(low.x, high.x),
                position.y.clamp(low.y, high.y),
                position.z.clamp(low.z, high.z),
            );
            (closest - position).length() < reach
        })
        .collect();
    let looks: Vec<(Vec3, Vec3, f32)> = match lamp.spot {
        Some(spot) => {
            let forward = match spot.direction.normalized() {
                d if d == Vec3::ZERO => vec3(0.0, 0.0, 1.0),
                d => d,
            };
            let half = (spot.cone.clamp(0.02, 3.0) / 2.0 + 0.1).min(1.45);
            let (_, up) = basis(forward);
            vec![(forward, up, half.tan())]
        }
        None => CUBE_FACES.iter().map(|&(f, u)| (f, u, 1.0)).collect(),
    };
    let faces = looks
        .into_iter()
        .map(|(forward, up, tan_half)| {
            let right = up.cross(forward).normalized();
            let up = forward.cross(right);
            let mut map_tris = Vec::new();
            for &(d, t) in &near {
                let tri = &tris[t];
                let q = tri.p.map(|p| {
                    let v = p - position;
                    vec3(v.dot(right), v.dot(up), v.dot(forward))
                });
                let uv = tri.uv.filter(|_| draws[d].texture.is_some());
                clip_near_light(q, uv, |corners, uvs| {
                    let xy = corners.map(|c| {
                        vec2(
                            (c.x / c.z / tan_half * 0.5 + 0.5) * size as f32,
                            (0.5 - c.y / c.z / tan_half * 0.5) * size as f32,
                        )
                    });
                    let inv = corners.map(|c| 1.0 / c.z);
                    let uv = uvs.map(|uvs| {
                        (
                            [uvs[0] * inv[0], uvs[1] * inv[1], uvs[2] * inv[2]],
                            d as u32,
                        )
                    });
                    if let Some(t) = MapTri::new(xy, inv, true, uv, size) {
                        map_tris.push(t);
                    }
                });
            }
            FaceMap {
                right,
                up,
                forward,
                tan_half,
                map: DepthMap::draw(size, &map_tris, draws, threads),
            }
        })
        .collect();
    Some(LampMap {
        position,
        faces,
        point: lamp.spot.is_none(),
    })
}

/// Cuts a triangle seen from a lamp where it's too close, as for the
/// camera, and calls `emit` with each piece.
fn clip_near_light(
    q: [Vec3; 3],
    uv: Option<[Vec2; 3]>,
    mut emit: impl FnMut([Vec3; 3], Option<[Vec2; 3]>),
) {
    let uvs = uv.unwrap_or([Vec2::ZERO; 3]);
    let mut out = [(Vec3::ZERO, Vec2::ZERO); 4];
    let mut count = 0;
    for i in 0..3 {
        let (current, next) = (q[i], q[(i + 1) % 3]);
        let inside = current.z >= NEAR;
        if inside {
            out[count] = (current, uvs[i]);
            count += 1;
        }
        if inside != (next.z >= NEAR) {
            let t = (NEAR - current.z) / (next.z - current.z);
            out[count] = (
                current + (next - current) * t,
                uvs[i] + (uvs[(i + 1) % 3] - uvs[i]) * t,
            );
            count += 1;
        }
    }
    for i in 1..count.saturating_sub(1) {
        let corners = [out[0].0, out[i].0, out[i + 1].0];
        let uvs = uv.map(|_| [out[0].1, out[i].1, out[i + 1].1]);
        emit(corners, uvs);
    }
}

impl LampMap {
    /// How much of the lamp's light reaches `position`.
    fn light(&self, position: Vec3, normal: Vec3, taps: i32) -> f32 {
        let from_lamp = position - self.position;
        let face = if self.point {
            let a = vec3(from_lamp.x.abs(), from_lamp.y.abs(), from_lamp.z.abs());
            if a.x >= a.y && a.x >= a.z {
                if from_lamp.x >= 0.0 { 0 } else { 1 }
            } else if a.y >= a.z {
                if from_lamp.y >= 0.0 { 2 } else { 3 }
            } else if from_lamp.z >= 0.0 {
                4
            } else {
                5
            }
        } else {
            0
        };
        let f = &self.faces[face];
        let size = f.map.size as f32;
        let distance = from_lamp.length();
        let texel = 2.0 * distance * f.tan_half / size;
        let v = from_lamp + normal * (texel * 1.5);
        let q = vec3(v.dot(f.right), v.dot(f.up), v.dot(f.forward));
        if q.z <= NEAR {
            return 1.0;
        }
        let x = (q.x / q.z / f.tan_half * 0.5 + 0.5) * size;
        let y = (0.5 - q.y / q.z / f.tan_half * 0.5) * size;
        f.map.light(x, y, q.z - texel * 0.5, taps)
    }
}

/// Bloom: glowing pixels' light, blurred, added over the picture. Two
/// blurs are added: a tight one (a bright halo round small lights) and a
/// wide one (the soft glow filling the air round them).
fn add_bloom(
    pixels: &mut [u32],
    glow: &[[f32; 3]],
    width: usize,
    height: usize,
    bloom: Bloom,
    threads: usize,
) {
    let strength = if bloom.strength.is_finite() {
        bloom.strength.max(0.0)
    } else {
        0.0
    };
    if strength == 0.0 || !glow.iter().any(|g| g[0] + g[1] + g[2] > 0.0) {
        return;
    }
    let radius = if bloom.radius.is_finite() {
        bloom.radius.max(0.0)
    } else {
        0.03
    };
    let short = width.min(height);
    // Each blur at a smaller size: much quicker, and it's blurry anyway.
    let tight = Blurred::new(
        glow,
        width,
        height,
        (short / 480).clamp(1, 4),
        radius * 0.2,
        threads,
    );
    let wide = Blurred::new(
        glow,
        width,
        height,
        (short / 240).clamp(1, 8),
        radius,
        threads,
    );
    let jobs: Vec<(usize, &mut [u32])> = pixels.chunks_mut(width).enumerate().collect();
    let (tight, wide) = (&tight, &wide);
    run_jobs(
        threads,
        jobs,
        || (),
        |_, (y, row)| {
            for (x, pixel) in row.iter_mut().enumerate() {
                let (a, b) = (tight.at(x, y), wide.at(x, y));
                let add = [0, 1, 2].map(|c| (a[c] * 0.5 + b[c] * 0.5) * strength);
                if add.iter().all(|&v| v < 0.5) {
                    continue;
                }
                let channel = |shift: u32, extra: f32| {
                    let c = ((*pixel >> shift) & 0xff) as f32 + extra;
                    (c.round().clamp(0.0, 255.0) as u32) << shift
                };
                *pixel = channel(16, add[0]) | channel(8, add[1]) | channel(0, add[2]);
            }
        },
    );
}

/// The glow, made `q` times smaller and blurred, `radius` being a share
/// of the picture's height.
struct Blurred {
    values: Vec<[f32; 3]>,
    width: usize,
    height: usize,
    q: usize,
}

impl Blurred {
    fn new(
        glow: &[[f32; 3]],
        width: usize,
        height: usize,
        q: usize,
        radius: f32,
        threads: usize,
    ) -> Blurred {
        let (w, h) = (width.div_ceil(q), height.div_ceil(q));
        let mut small = vec![[0.0f32; 3]; w * h];
        for (y, row) in small.chunks_mut(w).enumerate() {
            for (x, cell) in row.iter_mut().enumerate() {
                let mut sum = [0.0f32; 3];
                let mut count = 0.0;
                for gy in y * q..((y + 1) * q).min(height) {
                    for gx in x * q..((x + 1) * q).min(width) {
                        let g = glow[gy * width + gx];
                        sum[0] += g[0];
                        sum[1] += g[1];
                        sum[2] += g[2];
                        count += 1.0;
                    }
                }
                *cell = sum.map(|v| v / count);
            }
        }
        let sigma = (radius * height as f32 / q as f32).max(0.5);
        let reach = (sigma * 3.0).ceil() as i64;
        let mut kernel: Vec<f32> = (-reach..=reach)
            .map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp())
            .collect();
        let total: f32 = kernel.iter().sum();
        for k in &mut kernel {
            *k /= total;
        }
        let blur = |from: &[[f32; 3]], to: &mut [[f32; 3]], across: bool| {
            let (lines, length) = if across { (h, w) } else { (w, h) };
            let at = |line: usize, i: usize| if across { line * w + i } else { i * w + line };
            // Each line on its own, so lines can go to threads; written
            // back in one go after.
            let done: Vec<Mutex<Vec<[f32; 3]>>> =
                (0..lines).map(|_| Mutex::new(Vec::new())).collect();
            run_jobs(
                threads,
                (0..lines).collect(),
                || (),
                |_, line| {
                    // Most lines of a dark picture have no glow at all.
                    if (0..length).all(|i| from[at(line, i)] == [0.0; 3]) {
                        return;
                    }
                    let mut out = Vec::with_capacity(length);
                    for i in 0..length as i64 {
                        let mut sum = [0.0f32; 3];
                        for (k, weight) in kernel.iter().enumerate() {
                            let j = (i + k as i64 - reach).clamp(0, length as i64 - 1) as usize;
                            let v = from[at(line, j)];
                            sum[0] += v[0] * weight;
                            sum[1] += v[1] * weight;
                            sum[2] += v[2] * weight;
                        }
                        out.push(sum);
                    }
                    if let Ok(mut slot) = done[line].lock() {
                        *slot = out;
                    }
                },
            );
            for (line, slot) in done.into_iter().enumerate() {
                let values = slot.into_inner().unwrap_or_default();
                if values.is_empty() {
                    for i in 0..length {
                        to[at(line, i)] = [0.0; 3];
                    }
                }
                for (i, v) in values.into_iter().enumerate() {
                    to[at(line, i)] = v;
                }
            }
        };
        let mut across = vec![[0.0f32; 3]; w * h];
        blur(&small, &mut across, true);
        blur(&across, &mut small, false);
        Blurred {
            values: small,
            width: w,
            height: h,
            q,
        }
    }

    /// The blurred glow at pixel `x`, `y` of the picture (mixed from the
    /// four nearest of its own).
    fn at(&self, x: usize, y: usize) -> [f32; 3] {
        let (w, h, q) = (self.width, self.height, self.q as f32);
        let fy = ((y as f32 + 0.5) / q - 0.5).clamp(0.0, (h - 1) as f32);
        let fx = ((x as f32 + 0.5) / q - 0.5).clamp(0.0, (w - 1) as f32);
        let (y0, ty) = (fy as usize, fy - fast_floor(fy));
        let (x0, tx) = (fx as usize, fx - fast_floor(fx));
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let v = &self.values;
        [0, 1, 2].map(|c| {
            let top = v[y0 * w + x0][c] * (1.0 - tx) + v[y0 * w + x1][c] * tx;
            let bottom = v[y1 * w + x0][c] * (1.0 - tx) + v[y1 * w + x1][c] * tx;
            top * (1.0 - ty) + bottom * ty
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Canvas;
    use crate::lights::{Bloom, Lamp, Shadows};
    use crate::render3d::{
        draw_glass_scene, draw_glow_and_fog_scene, draw_hand_in_front_of_wall,
        draw_repeated_tile_floor,
    };

    /// A canvas drawing with the lit renderer even with nothing asking.
    fn lit(width: usize, height: usize, threads: usize) -> Canvas {
        let mut canvas = Canvas::new(width, height);
        canvas.effects.always = true;
        canvas.effects.lit = Some(crate::lights::LIT);
        canvas.set_threads(threads);
        canvas
    }

    /// A mixed scene: lit cubes and a sphere in a coloured light, a
    /// textured crate with a tint, a cube cut by the near plane, fog.
    fn mixed_scene(canvas: &mut Canvas) {
        let mut camera = Camera3D::new(vec3(0.3, 1.2, -3.0));
        camera.look_at(vec3(0.0, 0.3, 2.0));
        canvas.clear(Color::rgb(20, 30, 40));
        canvas.set_lighting(Lighting {
            sun_direction: vec3(-0.6, 1.0, -0.2),
            sun_color: Color::from_hex(0xffd0a0),
            sun_strength: 0.9,
            ambient: 0.3,
        });
        canvas.set_fog(Some(Fog {
            color: Color::rgb(20, 30, 40),
            near: 4.0,
            far: 12.0,
        }));
        canvas.draw_mesh(
            &Mesh::checkerboard(8, 1.0, Color::GRAY, Color::rgb(60, 60, 70)),
            &Transform::at(vec3(0.0, -0.5, 3.0)),
            &camera,
        );
        canvas.draw_mesh(
            &Mesh::sphere(Color::ORANGE, 12),
            &Transform::at(vec3(-1.0, 0.2, 2.0)),
            &camera,
        );
        let picture = Sprite::from_pixels(
            2,
            2,
            vec![
                Color::RED,
                Color::WHITE,
                Color::BLUE,
                Color::rgba(0, 0, 0, 0),
            ],
        );
        let crate_mesh = Mesh::cube(Color::WHITE).with_texture(picture);
        canvas.draw_mesh(
            &crate_mesh,
            &Transform::at(vec3(1.0, 0.0, 2.5)).rotated(vec3(0.0, 0.6, 0.0)),
            &camera,
        );
        canvas.draw_mesh_tinted(
            &Mesh::cube(Color::WHITE),
            &Transform::at(vec3(0.0, 0.0, 6.0)).sized(1.5),
            &camera,
            Color::rgb(120, 255, 160),
        );
        // Half of it behind the camera: cut by the near plane.
        canvas.draw_mesh(
            &Mesh::cube(Color::GREEN),
            &Transform::at(vec3(0.3, 1.0, -3.0)).sized(0.6),
            &camera,
        );
    }

    fn drawn(mut canvas: Canvas, draw: impl Fn(&mut Canvas)) -> Vec<u32> {
        draw(&mut canvas);
        canvas.flush_3d();
        canvas.pixels().to_vec()
    }

    #[test]
    fn with_nothing_turned_on_the_lit_renderer_draws_what_the_plain_one_does() {
        type Scene = fn(&mut Canvas);
        let scenes: [(&str, Scene); 5] = [
            ("glow and fog", draw_glow_and_fog_scene),
            ("glowing ore veins", crate::blocks::draw_ore_scene),
            ("see-through", |c| draw_glass_scene(c, false)),
            ("repeated texture", draw_repeated_tile_floor),
            ("mixed", mixed_scene),
        ];
        for (name, scene) in scenes {
            for threads in [1, 3] {
                let plain = drawn(Canvas::new(160, 120), scene);
                let new = drawn(lit(160, 120, threads), scene);
                let differ = plain.iter().zip(&new).filter(|(a, b)| a != b).count();
                assert_eq!(differ, 0, "{name}: {differ} pixels differ");
            }
        }
        // A hand in view is put back into the world to be drawn, so its
        // edges can move by a hair.
        for glowing in [true, false] {
            let scene = |c: &mut Canvas| draw_hand_in_front_of_wall(c, 0.7, glowing);
            let plain = drawn(Canvas::new(160, 120), scene);
            let new = drawn(lit(160, 120, 2), scene);
            let differ = plain.iter().zip(&new).filter(|(a, b)| a != b).count();
            assert!(differ <= 8, "the hand: {differ} pixels differ");
        }
    }

    #[test]
    fn the_same_frame_is_the_same_on_any_number_of_threads() {
        let scene = |canvas: &mut Canvas| {
            canvas.set_lamps(&[
                Lamp::point(vec3(0.5, 1.5, 1.0), Color::from_hex(0xffc080), 2.0, 6.0)
                    .with_shadows(),
                Lamp::spot(
                    vec3(-1.0, 2.0, 0.0),
                    vec3(0.3, -1.0, 0.8),
                    0.8,
                    Color::from_hex(0x80a0ff),
                    3.0,
                    8.0,
                )
                .with_shadows(),
            ]);
            canvas.set_shadows(Some(Shadows {
                resolution: 512,
                lamp_resolution: 128,
                ..Shadows::default()
            }));
            canvas.set_bloom(Some(Bloom::default()));
            canvas.set_supersample(2);
            canvas.set_smooth_shading(true);
            mixed_scene(canvas);
            let lava = Mesh::cube(Color::rgb(255, 90, 20)).glowing();
            let camera = Camera3D::looking_at(vec3(0.3, 1.2, -3.0), vec3(0.0, 0.3, 2.0));
            canvas.draw_mesh(
                &lava,
                &Transform::at(vec3(-0.2, -0.3, 1.0)).sized(0.4),
                &camera,
            );
        };
        let one = drawn(Canvas::new(120, 90).threads(1), scene);
        let four = drawn(Canvas::new(120, 90).threads(4), scene);
        let again = drawn(Canvas::new(120, 90).threads(4), scene);
        assert!(one == four, "1 thread and 4 give the same picture");
        assert!(four == again, "and every run is the same");
    }

    impl Canvas {
        fn threads(mut self, threads: usize) -> Canvas {
            self.set_threads(threads);
            self
        }
    }

    /// A dark grey floor seen from above, lit only by `lamps`, with no sun
    /// and very little ambient light.
    fn floor_lit_by(lamps: &[Lamp], shadows: bool, smooth: bool) -> (Canvas, Camera3D) {
        let mut canvas = Canvas::new(160, 160).threads(2);
        let camera = Camera3D::looking_at(vec3(0.0, 8.0, -0.01), Vec3::ZERO);
        canvas.clear(Color::BLACK);
        canvas.set_lighting(Lighting {
            sun_strength: 0.0,
            ambient: 0.05,
            ..Lighting::default()
        });
        canvas.set_lamps(lamps);
        canvas.set_smooth_shading(smooth);
        if shadows {
            canvas.set_shadows(Some(Shadows {
                lamp_resolution: 256,
                ..Shadows::default()
            }));
        }
        canvas.draw_mesh(
            &Mesh::checkerboard(1, 8.0, Color::GRAY, Color::GRAY),
            &Transform::default(),
            &camera,
        );
        (canvas, camera)
    }

    fn brightness_at(canvas: &Canvas, camera: &Camera3D, point: Vec3) -> u32 {
        let at = camera.world_to_screen(point, canvas).unwrap();
        let c = canvas.get_pixel(at.x as i32, at.y as i32).unwrap();
        c.r as u32 + c.g as u32 + c.b as u32
    }

    #[test]
    fn a_point_lamp_lights_a_pool_round_it_that_fades_to_nothing_at_its_reach() {
        let lamp = Lamp::point(vec3(0.0, 1.0, 0.0), Color::WHITE, 1.0, 3.0);
        let (mut canvas, camera) = floor_lit_by(&[lamp], false, false);
        canvas.flush_3d();
        let under = brightness_at(&canvas, &camera, vec3(0.0, 0.0, 0.0));
        let near = brightness_at(&canvas, &camera, vec3(1.0, 0.0, 0.0));
        let far = brightness_at(&canvas, &camera, vec3(3.5, 0.0, 0.0));
        let (dark, _) = floor_lit_by(&[], false, false);
        let mut dark = dark;
        dark.flush_3d();
        let unlit = brightness_at(&dark, &camera, vec3(0.0, 0.0, 0.0));
        assert!(under > near && near > far, "{under} > {near} > {far}");
        assert_eq!(far, unlit, "beyond its reach, only the ambient light");
        // One flat floor, two triangles: the light still changes across it.
        assert!(under > unlit * 5, "{under} vs {unlit}");
    }

    #[test]
    fn a_spot_lamp_lights_only_its_cone_in_its_colour() {
        let torch = Lamp::spot(
            vec3(2.0, 3.0, 0.0),
            vec3(0.0, -1.0, 0.0),
            0.5,
            Color::rgb(255, 40, 40),
            6.0,
            10.0,
        );
        let (mut canvas, camera) = floor_lit_by(&[torch], false, false);
        canvas.flush_3d();
        let at = |x: f32| {
            let p = camera.world_to_screen(vec3(x, 0.0, 0.0), &canvas).unwrap();
            canvas.get_pixel(p.x as i32, p.y as i32).unwrap()
        };
        let inside = at(2.0);
        let outside = at(-2.0);
        assert!(
            inside.r > 150 && inside.r > inside.g * 3,
            "red in the cone: {inside:?}"
        );
        assert!(outside.r < 20, "dark outside it: {outside:?}");
    }

    #[test]
    fn smooth_shading_rounds_a_sphere_and_keeps_a_cube_sharp() {
        let distinct = |smooth: bool, mesh: &Mesh| {
            let mut canvas = Canvas::new(120, 120).threads(2);
            canvas.clear(Color::BLACK);
            canvas.set_lighting(Lighting {
                sun_direction: vec3(-1.0, 0.5, -0.5),
                ambient: 0.2,
                ..Lighting::default()
            });
            canvas.set_smooth_shading(smooth);
            if !smooth {
                canvas.effects.always = true;
                canvas.effects.lit = Some(crate::lights::LIT);
            }
            let camera = Camera3D::new(vec3(0.0, 0.0, -2.5));
            canvas.draw_mesh(mesh, &Transform::default(), &camera);
            canvas.flush_3d();
            let mut colors: Vec<u32> = canvas.pixels().to_vec();
            colors.sort_unstable();
            colors.dedup();
            (colors.len(), canvas.pixels().to_vec())
        };
        let ball = Mesh::sphere(Color::WHITE, 10);
        let (flat, _) = distinct(false, &ball);
        let (smooth, _) = distinct(true, &ball);
        assert!(smooth > flat * 4, "{smooth} shades smooth, {flat} flat");
        // A cube's edges are 90 degrees: each face stays one shade.
        let cube = Mesh::cube(Color::WHITE);
        let (_, flat) = distinct(false, &cube);
        let (_, smooth) = distinct(true, &cube);
        let worst = flat
            .iter()
            .zip(&smooth)
            .map(|(a, b)| {
                (0..3)
                    .map(|i| ((a >> (i * 8)) & 0xff).abs_diff((b >> (i * 8)) & 0xff))
                    .max()
                    .unwrap()
            })
            .max()
            .unwrap();
        assert!(worst <= 1, "a cube looks the same smooth: off by {worst}");
    }

    /// A floor in the sun with a cube floating over it; the sun from the
    /// right, so the shadow falls to the left of the cube.
    fn cube_over_floor(canvas: &mut Canvas, camera: &Camera3D, shadows: bool) {
        canvas.clear(Color::BLACK);
        canvas.set_lighting(Lighting {
            sun_direction: vec3(1.0, 1.0, 0.0),
            ambient: 0.3,
            ..Lighting::default()
        });
        canvas.set_shadows(shadows.then(Shadows::default));
        canvas.draw_mesh(
            &Mesh::checkerboard(1, 10.0, Color::WHITE, Color::WHITE),
            &Transform::default(),
            camera,
        );
        canvas.draw_mesh(
            &Mesh::cube(Color::WHITE),
            &Transform::at(vec3(0.0, 1.0, 0.0)),
            camera,
        );
    }

    #[test]
    fn the_sun_casts_shadows_when_asked() {
        let camera = Camera3D::looking_at(vec3(0.0, 6.0, -5.0), Vec3::ZERO);
        let shade = |shadows: bool| {
            let mut canvas = Canvas::new(160, 120).threads(2);
            canvas.effects.always = true;
            canvas.effects.lit = Some(crate::lights::LIT);
            cube_over_floor(&mut canvas, &camera, shadows);
            canvas.flush_3d();
            (
                brightness_at(&canvas, &camera, vec3(-1.0, 0.0, 0.0)),
                brightness_at(&canvas, &camera, vec3(2.5, 0.0, 0.0)),
            )
        };
        let (left, right) = shade(false);
        assert_eq!(left, right, "no shadows: the floor is lit evenly");
        let (shadowed, sunny) = shade(true);
        assert_eq!(sunny, right, "out of the shadow, as before");
        assert!(shadowed * 2 < sunny, "in the shadow: {shadowed} vs {sunny}");
    }

    #[test]
    fn a_lamp_with_shadows_is_blocked_by_what_stands_in_front_of_it() {
        let lamp = |shadows: bool| {
            let lamp = Lamp::point(vec3(0.0, 1.0, 0.0), Color::WHITE, 3.0, 8.0);
            if shadows { lamp.with_shadows() } else { lamp }
        };
        let behind = vec3(2.5, 0.0, 0.0);
        let shade = |shadows: bool| {
            let (mut canvas, camera) = floor_lit_by(&[lamp(shadows)], shadows, false);
            // A wall between the lamp and the floor to its right.
            canvas.draw_mesh(
                &Mesh::cube(Color::GRAY),
                &Transform::at(vec3(1.0, 0.75, 0.0)).scaled(vec3(0.1, 1.5, 3.0)),
                &camera,
            );
            canvas.flush_3d();
            (
                brightness_at(&canvas, &camera, behind),
                brightness_at(&canvas, &camera, vec3(-2.5, 0.0, 0.0)),
            )
        };
        let (open, other_side) = shade(false);
        assert!(open > 60, "without shadows the light gets through: {open}");
        let (blocked, still_lit) = shade(true);
        assert!(
            blocked * 3 < open,
            "the wall blocks it: {blocked} vs {open}"
        );
        assert_eq!(still_lit, other_side, "the open side is lit as before");
    }

    #[test]
    fn bloom_spreads_glowing_triangles_into_the_dark_round_them() {
        let draw = |bloom: bool| {
            let mut canvas = Canvas::new(200, 200).threads(2);
            canvas.effects.always = true;
            canvas.effects.lit = Some(crate::lights::LIT);
            canvas.clear(Color::BLACK);
            canvas.set_bloom(bloom.then(Bloom::default));
            let camera = Camera3D::new(vec3(0.0, 0.0, -4.0));
            canvas.draw_mesh(
                &Mesh::cube(Color::rgb(255, 120, 20)).glowing(),
                &Transform::at(Vec3::ZERO).sized(0.5),
                &camera,
            );
            // A lit white cube beside it doesn't bloom.
            canvas.draw_mesh(
                &Mesh::cube(Color::WHITE),
                &Transform::at(vec3(1.4, 0.0, 0.0)).sized(0.5),
                &camera,
            );
            canvas.flush_3d();
            canvas
        };
        let (plain, bloomed) = (draw(false), draw(true));
        let near = |c: &Canvas| c.get_pixel(100, 84).unwrap(); // just above the lava
        assert_eq!(near(&plain), Color::BLACK);
        assert!(
            near(&bloomed).r > 10,
            "it glows round: {:?}",
            near(&bloomed)
        );
        let corner = |c: &Canvas| c.get_pixel(2, 2).unwrap();
        assert_eq!(corner(&bloomed), Color::BLACK, "far away, nothing");
        // Round the white cube (not glowing), nothing either.
        let camera = Camera3D::new(vec3(0.0, 0.0, -4.0));
        let side = camera
            .world_to_screen(vec3(1.4, 0.45, 0.0), &bloomed)
            .unwrap();
        let above = bloomed.get_pixel(side.x as i32, side.y as i32 - 8).unwrap();
        assert!(above.g < 12, "a lit wall doesn't bloom: {above:?}");
    }

    #[test]
    fn supersampling_smooths_edges_and_leaves_2d_sharp() {
        let draw = |factor: u32| {
            let mut canvas = Canvas::new(64, 64).threads(2);
            canvas.set_supersample(factor);
            canvas.clear(Color::BLACK);
            let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
            canvas.draw_mesh(
                &Mesh::cube(Color::WHITE).glowing(),
                &Transform::default().rotated(vec3(0.0, 0.0, 0.5)),
                &camera,
            );
            canvas.fill_rect(
                crate::math::Rect::new(2.0, 2.0, 5.0, 5.0),
                Color::rgb(255, 0, 0),
            );
            canvas.flush_3d();
            canvas
        };
        let partial = |c: &Canvas| {
            c.pixels()
                .iter()
                .filter(|&&p| {
                    let g = (p >> 8) & 0xff;
                    g > 0 && g < 255
                })
                .count()
        };
        let (sharp, smooth) = (draw(1), draw(2));
        assert_eq!(partial(&sharp), 0, "without it, every pixel is in or out");
        assert!(
            partial(&smooth) > 40,
            "edges are soft: {}",
            partial(&smooth)
        );
        // Inside the cube, and the 2D square, exactly as without it.
        assert_eq!(smooth.get_pixel(32, 32), sharp.get_pixel(32, 32));
        assert_eq!(smooth.get_pixel(4, 4), Some(Color::rgb(255, 0, 0)));
        assert_eq!(smooth.get_pixel(1, 1), Some(Color::BLACK));
    }
}
