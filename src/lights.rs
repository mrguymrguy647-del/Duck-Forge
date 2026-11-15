//! Lamps, shadows, glow and smooth edges for 3D: the settings of the CPU's
//! *lit renderer* (`frame3d.rs`).
//!
//! The plain 3D renderer (`render3d.rs`) lights each triangle evenly by one
//! sun. These settings ask for more, and turn on the lit renderer, which
//! works out the light at every pixel:
//!
//! - [`Lamp`]s: point and spot lights, as many as you like
//!   ([`Canvas::set_lamps`]).
//! - Smooth shading: round things look round, with no facets
//!   ([`Canvas::set_smooth_shading`]).
//! - [`Shadows`]: the sun's, and the lamps' that ask for them
//!   ([`Canvas::set_shadows`]).
//! - [`Bloom`]: glowing triangles glow softly into the picture round them
//!   ([`Canvas::set_bloom`]).
//! - Smooth edges: every pixel is worked out from several points inside it
//!   (supersampling, [`Canvas::set_supersample`] or
//!   [`Config::supersample`](crate::engine::Config::supersample)).
//!
//! The lit renderer collects the 3D of a frame and draws it all at once
//! (when 2D is drawn over it, or the frame ends), split into bands of rows
//! drawn side by side on every core of the computer. Each band is worked
//! out the same way whichever core draws it, so the pictures are the same
//! to the pixel on every run, with any number of cores.
//!
//! It always draws on the CPU, in a window too (the graphics card draws
//! only the plain look). With none of these settings, 3D is drawn exactly
//! as before.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::{Vec3, vec3};

/// A light in the world: a bare bulb (a *point* lamp, lighting every way)
/// or a torch (a *spot* lamp, lighting a cone). Give the canvas a list of
/// them with [`Canvas::set_lamps`].
///
/// ```
/// # use duckforge::prelude::*;
/// # let mut canvas = Canvas::new(32, 24);
/// let bulb = Lamp::point(vec3(0.0, 2.5, 0.0), Color::from_hex(0xffc880), 1.5, 8.0);
/// let torch = Lamp::spot(vec3(1.0, 1.6, -3.0), vec3(0.0, -0.2, 1.0), 0.6, Color::WHITE, 3.0, 15.0)
///     .with_shadows();
/// canvas.set_lamps(&[bulb, torch]);
/// ```
///
/// How bright it lights a surface: with `power` 1, a surface 1 metre
/// away, facing it, is as bright as in full sun (a lamp's light adds to
/// the sun's and the ambient light). Further away it fades: 40% as bright
/// at 2 metres, 20% at 3, 6% at 5 (like `2 / (1 + distance²)`), and from
/// `reach` on it lights nothing at all, fading out smoothly before it
/// rather than stopping at a hard edge. Faces turned away from it aren't
/// lit by it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lamp {
    /// Where it is, in the world.
    pub position: Vec3,
    /// Its colour: white, a warm `0xffc880` bulb, a cold blue screen.
    pub color: Color,
    /// How bright it is: 1.0 lights a surface 1 metre away fully. 0 is off.
    pub power: f32,
    /// How far its light goes, in metres: nothing is lit by it from there on.
    pub reach: f32,
    /// `None` for a point lamp; a spot lamp's cone.
    pub spot: Option<Spot>,
    /// Whether things block its light (see [`Shadows`]). Shadows from a
    /// point lamp cost six times a spot lamp's (it looks every way).
    pub shadows: bool,
}

/// A spot lamp's cone (see [`Lamp::spot`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spot {
    /// The way it points (needn't be 1 long).
    pub direction: Vec3,
    /// How wide the cone is, edge to edge, in radians: 0.6 is a torch,
    /// 1.5 a wide floodlight. Kept between 0.02 and 3.
    pub cone: f32,
    /// How much of the cone fades at its edge, from 0 (a hard edge) to 1
    /// (fading all the way from the middle). 0.3 by default.
    pub softness: f32,
}

impl Lamp {
    /// A point lamp, lighting every way: a bulb, a candle, a fire.
    pub fn point(position: Vec3, color: Color, power: f32, reach: f32) -> Lamp {
        Lamp {
            position,
            color,
            power,
            reach,
            spot: None,
            shadows: false,
        }
    }

    /// A spot lamp, pointing along `direction`, lighting a cone `cone`
    /// radians wide: a torch, a headlight, a stage light.
    pub fn spot(
        position: Vec3,
        direction: Vec3,
        cone: f32,
        color: Color,
        power: f32,
        reach: f32,
    ) -> Lamp {
        Lamp {
            spot: Some(Spot {
                direction,
                cone,
                softness: 0.3,
            }),
            ..Lamp::point(position, color, power, reach)
        }
    }

    /// The same lamp, casting shadows (when the canvas has
    /// [`Shadows`] turned on).
    pub fn with_shadows(self) -> Lamp {
        Lamp {
            shadows: true,
            ..self
        }
    }
}

/// Things blocking the light: the sun's shadows, and those of the
/// [`Lamp`]s made [`with_shadows`](Lamp::with_shadows). Turn them on with
/// [`Canvas::set_shadows`]: `canvas.set_shadows(Some(Shadows::default()))`.
///
/// Every solid mesh casts a shadow (see-through and fully glowing ones
/// don't: a bulb doesn't shadow its own light), and
/// see-through pixels of a texture let the light through, so a fence's
/// shadow has holes. Meshes drawn with `draw_mesh_in_view` (a hand) are
/// shaded but cast nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadows {
    /// How sharp the sun's shadows are: the size of its shadow picture,
    /// which covers what the camera sees. 2048 by default; 1024 is quicker
    /// and blurrier.
    pub resolution: usize,
    /// How far from the camera the sun's shadows reach, in metres (30 by
    /// default). Shorter is sharper.
    pub distance: f32,
    /// How soft their edges are: 0 is hard, 1 (the default) a little
    /// soft, 2 or 3 softer (and slower).
    pub softness: f32,
    /// How dark a shadow is: 1 (the default) blocks all of the light, 0.6
    /// lets 40% through (a bright, hazy day).
    pub darkness: f32,
    /// The size of each lamp's shadow picture (each of six, for a point
    /// lamp): 512 by default.
    pub lamp_resolution: usize,
}

impl Default for Shadows {
    fn default() -> Shadows {
        Shadows {
            resolution: 2048,
            distance: 30.0,
            softness: 1.0,
            darkness: 1.0,
            lamp_resolution: 512,
        }
    }
}

/// Glowing triangles ([`Mesh::glowing`](crate::render3d::Mesh::glowing),
/// [`Mesh::set_glow`](crate::render3d::Mesh::set_glow)) glowing softly into
/// the picture round them: lava lights the air above it, a screen glows
/// in the dark. Turn it on with [`Canvas::set_bloom`]:
/// `canvas.set_bloom(Some(Bloom::default()))`. Only glowing triangles
/// bloom (a white wall in the sun doesn't), each by how much it glows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bloom {
    /// How much glow is added: 1.0 (the default) adds as much again as the
    /// glowing pixels' own light, spread round them.
    pub strength: f32,
    /// How far it spreads, as a share of the picture's height: 0.03 (the
    /// default) is 3%, about 58 pixels of a 1920-tall picture.
    pub radius: f32,
}

impl Default for Bloom {
    fn default() -> Bloom {
        Bloom {
            strength: 1.0,
            radius: 0.03,
        }
    }
}

/// The lit renderer, reached only through these two: the setters below
/// put them in a canvas's [`Effects`], so a game that never asks for
/// lamps, shadows, glow, smooth shading or supersampling doesn't carry the
/// lit renderer at all (it's most of 60,000 bytes).
#[derive(Clone, Copy)]
pub(crate) struct LitRenderer {
    pub(crate) add: fn(&mut crate::frame3d::Queue, crate::frame3d::Adding<'_>),
    pub(crate) render:
        fn(&mut crate::frame3d::Queue, &Effects, usize, usize, &mut [u32], &mut [f32]),
}

impl std::fmt::Debug for LitRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LitRenderer")
    }
}

/// The lit renderer's two ways in.
pub(crate) const LIT: LitRenderer = LitRenderer {
    add: crate::frame3d::Queue::add,
    render: crate::frame3d::render,
};

/// The lit renderer's settings, kept by the canvas.
#[derive(Debug, Clone, Default)]
pub(crate) struct Effects {
    /// The lit renderer, once something has asked for it.
    pub(crate) lit: Option<LitRenderer>,
    pub(crate) lamps: Vec<Lamp>,
    pub(crate) shadows: Option<Shadows>,
    pub(crate) bloom: Option<Bloom>,
    /// For meshes drawn from now on.
    pub(crate) smooth: bool,
    /// Points worked out across and down each pixel: 1 is just its middle.
    pub(crate) supersample: u32,
    /// How many threads draw (0: as many as the computer has cores).
    pub(crate) threads: usize,
    /// The 360° sky behind the 3D, if there is one (see `sky.rs`).
    pub(crate) sky: Option<Box<crate::sky::Sky>>,
    /// For tests: the lit renderer even with nothing asking for it.
    #[cfg(test)]
    pub(crate) always: bool,
}

impl Effects {
    /// Is anything asking for the lit renderer?
    pub(crate) fn on(&self) -> bool {
        if self.lit.is_none() {
            return false;
        }
        #[cfg(test)]
        if self.always {
            return true;
        }
        !self.lamps.is_empty()
            || self.shadows.is_some()
            || self.bloom.is_some()
            || self.smooth
            || self.supersample > 1
    }

    /// The supersample factor, kept between 1 and 4.
    pub(crate) fn samples(&self) -> usize {
        self.supersample.clamp(1, 4) as usize
    }

    /// How many threads to draw with.
    pub(crate) fn thread_count(&self) -> usize {
        if self.threads > 0 {
            return self.threads.min(64);
        }
        if let Some(n) = std::env::var("DUCKFORGE_THREADS")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&n| n > 0)
        {
            return n.min(64);
        }
        std::thread::available_parallelism().map_or(1, |n| n.get().min(16))
    }
}

impl Canvas {
    /// Lights 3D with these lamps from now on, as well as the sun (see
    /// [`Lamp`]). It stays until changed; `&[]` turns them all off. Set
    /// them before drawing 3D (at the start of `draw`).
    ///
    /// Lamps turn on the lit renderer (see [`lights`](crate::lights)),
    /// which works out the light at every pixel: a lamp lights a pool of
    /// floor under it, not whole triangles.
    pub fn set_lamps(&mut self, lamps: &[Lamp]) {
        self.effects.lamps = lamps.to_vec();
        self.effects.lit = Some(LIT);
    }

    /// The lamps 3D is lit with now.
    pub fn lamps(&self) -> &[Lamp] {
        &self.effects.lamps
    }

    /// Turns shadows on (`Some`) or off (`None`, the start). See
    /// [`Shadows`]. It stays until changed.
    pub fn set_shadows(&mut self, shadows: Option<Shadows>) {
        self.effects.shadows = shadows;
        self.effects.lit = Some(LIT);
    }

    /// Turns glow round glowing triangles on (`Some`) or off (`None`, the
    /// start). See [`Bloom`]. It stays until changed.
    pub fn set_bloom(&mut self, bloom: Option<Bloom>) {
        self.effects.bloom = bloom;
        self.effects.lit = Some(LIT);
    }

    /// With `true`, meshes drawn from now on are shaded smoothly: the light
    /// changes gradually across each triangle, following the curve of the
    /// shape, so a sphere looks round rather than cut from flat faces.
    /// Edges sharper than 50 degrees (a cube's) stay sharp. `false` (the
    /// start) shades each triangle evenly. It can change between meshes:
    /// smooth a statue, keep the walls flat.
    pub fn set_smooth_shading(&mut self, on: bool) {
        self.effects.smooth = on;
        self.effects.lit = Some(LIT);
    }

    /// Smooth edges for 3D: each pixel is worked out from `factor` x
    /// `factor` points inside it and their colours averaged, so edges are
    /// soft instead of jagged. 1 (the start) is off; 2 is good (four times
    /// the work); 3 and 4 are smoother still. 2D is drawn as it always is,
    /// with sharp pixels. [`Config::supersample`](crate::engine::Config::supersample)
    /// sets it for a game's canvas.
    pub fn set_supersample(&mut self, factor: u32) {
        self.effects.supersample = factor.clamp(1, 4);
        self.effects.lit = Some(LIT);
    }

    /// The supersample factor (1: off).
    pub fn supersample(&self) -> u32 {
        self.effects.supersample.max(1)
    }

    /// How many threads the lit renderer draws with: 0 (the start) means
    /// one per core of the computer (or `DUCKFORGE_THREADS`, if it's set).
    /// The pictures are the same with any number.
    pub fn set_threads(&mut self, threads: usize) {
        self.effects.threads = threads;
    }
}

/// One of the six ways a point lamp looks to find its shadows: along +x,
/// -x, +y, -y, +z or -z, with a right and an up for each.
pub(crate) const CUBE_FACES: [(Vec3, Vec3); 6] = [
    (vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)),
    (vec3(-1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0)),
    (vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, -1.0)),
    (vec3(0.0, -1.0, 0.0), vec3(0.0, 0.0, 1.0)),
    (vec3(0.0, 0.0, 1.0), vec3(0.0, 1.0, 0.0)),
    (vec3(0.0, 0.0, -1.0), vec3(0.0, 1.0, 0.0)),
];
