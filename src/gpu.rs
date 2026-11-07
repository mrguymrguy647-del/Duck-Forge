//! Drawing 3D on the graphics card (GPU) with wgpu.
//!
//! This whole file only exists when the `gpu` feature is on (it is by
//! default). It does the same job as the CPU path in `render3d.rs`, but the
//! graphics card does the heavy work:
//!
//! 1. `Canvas::draw_mesh` doesn't draw anything right away. It records
//!    *which* mesh to draw and *where* (a matrix), grouped by mesh.
//! 2. Each mesh is sent to the GPU only once and then kept there (the
//!    *mesh cache*).
//! 3. When the 3D picture is needed (the canvas is drawn on in 2D again, or
//!    the frame ends), [`GpuRenderer::flush`] draws every recorded object
//!    with one draw call per mesh (*instancing*), onto a copy of the
//!    canvas, and copies the result back into the canvas's pixels.
//!
//! The game loop and the window don't change at all: the canvas's pixels
//! simply contain the 3D picture afterwards, just as with the CPU renderer.

use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use bytemuck::{Pod, Zeroable};

use crate::render3d::{Camera3D, Fog, Lighting, Mesh, Transform, triangle_corners};
use crate::sprite::Sprite;

/// One corner of one triangle, laid out exactly as the shader reads it.
///
/// `#[repr(C)]` fixes the order and layout of the fields in memory, and
/// `Pod` ("plain old data") promises it's just bytes, so `bytemuck` can
/// hand a whole `Vec<Vertex>` to the GPU as a byte slice without copying.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    /// Red, green, blue, and how much the triangle glows (0 to 255 for 0 to 1).
    color: [u8; 4],
    /// The texture point this corner shows (unused without a texture).
    uv: [f32; 2],
    /// [`TEXTURED`] and [`GLOWING`], added together.
    flags: u32,
}

/// `Vertex::flags`: the triangle shows the mesh's texture.
const TEXTURED: u32 = 1;
/// `Vertex::flags`: the triangle ignores the light (it fully glows).
const GLOWING: u32 = 2;

/// Per-object data: the object's transform matrix, and its tint (red,
/// green, blue multiply its colours; alpha is how solid it is).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Instance {
    model: [[f32; 4]; 4],
    tint: [f32; 4],
}

/// How one object is drawn, besides where: the canvas's light and fog,
/// and the object's tint.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Look {
    pub lighting: Lighting,
    pub fog: Option<Fog>,
    pub tint: crate::color::Color,
}

/// Settings shared by one batch of drawing (see `Globals` in `gpu.wgsl`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    view_projection: [[f32; 4]; 4],
    camera_position: [f32; 4],
    sun: [f32; 4],
    sun_color: [f32; 4],
    fog_color: [f32; 4],
    fog_range: [f32; 4],
    camera_forward: [f32; 4],
}

/// Where a mesh's two lists live in memory, and how long they are. The same
/// `Mesh` drawn many times in a frame always has the same address.
type Address = (usize, usize, usize, usize);

/// How the cache recognizes a mesh: its address plus a fingerprint of its
/// contents. The fingerprint matters because meshes can change (their
/// fields are public), and because a temporary mesh can reuse the memory of
/// one that was just dropped.
type MeshKey = (Address, u64);

fn address_of(mesh: &Mesh) -> Address {
    (
        mesh.vertices.as_ptr() as usize,
        mesh.vertices.len(),
        mesh.triangles.as_ptr() as usize,
        mesh.triangles.len(),
    )
}

/// A summary of *everything* in the mesh. Two meshes with different contents
/// get different fingerprints (with overwhelming probability).
fn fingerprint(mesh: &Mesh) -> u64 {
    let mut hasher = DefaultHasher::new();
    for &v in &mesh.vertices {
        hash_vertex(v, &mut hasher);
    }
    for &t in &mesh.triangles {
        hash_triangle(t, &mut hasher);
    }
    for uvs in &mesh.uvs {
        match uvs {
            Some(corners) => {
                for uv in corners {
                    [uv.x.to_bits(), uv.y.to_bits()].hash(&mut hasher);
                }
            }
            None => 0u8.hash(&mut hasher),
        }
    }
    mesh.texture
        .as_ref()
        .map(Sprite::identity)
        .hash(&mut hasher);
    mesh.glowing.hash(&mut hasher);
    for glow in &mesh.glow {
        glow.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// A few parts of a mesh: its first, middle and last corner and triangle.
/// Much quicker to compare than working out a full fingerprint, and enough
/// to notice that a different mesh now lives at the same address.
#[derive(Clone, Copy, PartialEq)]
struct Sample {
    vertices: [Option<crate::math::Vec3>; 3],
    triangles: [Option<crate::render3d::Triangle>; 3],
    uvs: [Option<Option<[crate::math::Vec2; 3]>>; 3],
    texture: Option<(usize, [usize; 4])>,
    glowing: bool,
    glow: [Option<u32>; 3],
}

fn sample(mesh: &Mesh) -> Sample {
    let picks = |len: usize| [0, len / 2, len.saturating_sub(1)];
    Sample {
        vertices: picks(mesh.vertices.len()).map(|i| mesh.vertices.get(i).copied()),
        triangles: picks(mesh.triangles.len()).map(|i| mesh.triangles.get(i).copied()),
        uvs: picks(mesh.uvs.len()).map(|i| mesh.uvs.get(i).copied()),
        texture: mesh.texture.as_ref().map(Sprite::identity),
        glowing: mesh.glowing,
        glow: picks(mesh.glow.len()).map(|i| mesh.glow.get(i).map(|g| g.to_bits())),
    }
}

fn hash_vertex(v: crate::math::Vec3, hasher: &mut DefaultHasher) {
    // Floats can't be hashed directly (NaN isn't equal to itself), so hash their bits.
    [v.x.to_bits(), v.y.to_bits(), v.z.to_bits()].hash(hasher);
}

fn hash_triangle(t: crate::render3d::Triangle, hasher: &mut DefaultHasher) {
    t.corners.hash(hasher);
    [t.color.r, t.color.g, t.color.b].hash(hasher);
}

/// What we learned about a mesh address during the current batch.
struct Seen {
    sample: Sample,
    fingerprint: u64,
}

/// A very fast hash function for the lookup tables below.
///
/// The standard library's default hasher is built to resist deliberate
/// attacks from untrusted data, which makes it slow for small keys like
/// ours (it showed up as most of the time spent per object). This one is
/// the simple design the Rust compiler itself uses ("FxHash"): mix in each
/// number with a rotate, an XOR and a multiply.
#[derive(Default)]
struct FastHasher(u64);

impl Hasher for FastHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(byte as u64);
        }
    }

    fn write_u64(&mut self, n: u64) {
        self.0 = (self.0.rotate_left(5) ^ n).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }

    fn write_usize(&mut self, n: usize) {
        self.write_u64(n as u64);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// A `HashMap` that uses [`FastHasher`].
type FastMap<K, V> = HashMap<K, V, BuildHasherDefault<FastHasher>>;

/// A mesh that lives on the GPU.
struct GpuMesh {
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    /// Its texture (see [`TextureKey`]), if it has one.
    texture: Option<TextureKey>,
    /// A ball around it (center and radius, in its own coordinates), to
    /// skip copies of it that are off screen.
    ball: (crate::math::Vec3, f32),
    /// The flush during which it was last drawn.
    last_used: u64,
}

/// How the texture cache recognizes a picture: which picture, and which
/// part of it (see `Sprite::identity`).
type TextureKey = (usize, [usize; 4]);

/// A texture that lives on the GPU.
struct GpuTexture {
    bind_group: wgpu::BindGroup,
    /// Kept so the picture can't be freed (and its address reused by
    /// another picture) while the GPU has a copy under its key.
    _sprite: Sprite,
    last_used: u64,
}

/// Meshes not drawn for this many flushes are removed from the GPU.
const EVICT_AFTER_FLUSHES: u64 = 120;

/// The most objects in one batch. With more, the batch is drawn early, so the
/// buffer of object matrices never grows past what the GPU allows.
const MAX_BATCH_INSTANCES: usize = 1 << 20;

/// Everything needed to draw 3D on the GPU for one canvas size.
pub(crate) struct GpuRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// "GPU: " plus the graphics card's name and the API used to talk to it.
    description: String,
    pipeline: wgpu::RenderPipeline,
    /// The same, for see-through objects: mixed over what's there, and not
    /// written to the depth buffer.
    see_through_pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    /// The picture being drawn. Same size and byte layout as the canvas.
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    depth_view: wgpu::TextureView,
    /// Where the finished picture is copied, so the CPU can read it.
    readback_buffer: wgpu::Buffer,
    /// Rows in `readback_buffer` are padded to a multiple of 256 bytes.
    padded_bytes_per_row: u32,
    width: u32,
    height: u32,
    instance_buffer: wgpu::Buffer,
    /// How textures are bound: the picture and how it's read (sampled).
    texture_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// A 1 x 1 white texture for meshes without one (the shader always
    /// wants a texture, even when it doesn't read it).
    plain: wgpu::BindGroup,
    textures: FastMap<TextureKey, GpuTexture>,
    /// The light for the pending objects.
    lighting: Lighting,
    /// The fog for the pending objects.
    fog: Option<Fog>,

    meshes: FastMap<MeshKey, GpuMesh>,
    /// Fingerprints worked out during the current batch, so each mesh is
    /// fully fingerprinted only once per batch, however often it's drawn.
    seen: FastMap<Address, Seen>,
    /// The objects recorded since the last flush, grouped by mesh.
    pending: FastMap<MeshKey, Vec<Instance>>,
    /// See-through objects recorded since the last flush, in the order
    /// they were drawn (drawn after the solid ones, one at a time).
    pending_see_through: Vec<(MeshKey, Instance)>,
    /// The camera for the pending objects.
    camera: Option<Camera3D>,
    /// Whether the next flush starts with an empty depth buffer.
    clear_depth: bool,
    flushes: u64,
    /// How many objects are waiting in `pending`, and how many may.
    pending_instances: usize,
    max_batch_instances: usize,
}

impl GpuRenderer {
    /// Starts up the GPU. This can fail, for example on a computer with no
    /// usable graphics driver; the canvas then falls back to the CPU.
    pub(crate) fn new(width: usize, height: usize) -> Result<Self, String> {
        if width == 0 || height == 0 {
            return Err(String::from("the canvas has no pixels"));
        }
        let (width, height) = (width as u32, height as u32);

        // 1. An *instance* is the entry point to wgpu. `new_without_display_handle_from_env`
        //    means we won't draw into a window (just into a texture), and lets
        //    the WGPU_BACKEND environment variable pick Vulkan, Metal, DX12 or GL.
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        // 2. An *adapter* is one graphics card (or a software imitation of one).
        //    wgpu's setup functions are `async`; `pollster::block_on` simply
        //    waits for them to finish.
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|err| format!("no graphics adapter found: {err}"))?;
        let info = adapter.get_info();
        let description = format!("GPU: {} ({:?})", info.name, info.backend);

        // Every graphics card has limits, like how big a picture it can draw.
        // Check the canvas fits, rather than letting wgpu fail later.
        let supported = adapter.limits();
        let padded_bytes_per_row = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let readback_size = padded_bytes_per_row as u64 * height as u64;
        let biggest = supported.max_texture_dimension_2d;
        if width > biggest || height > biggest || readback_size > supported.max_buffer_size {
            return Err(format!(
                "the canvas is {width} x {height} pixels, but this graphics card can only \
                 draw pictures up to {biggest} x {biggest}"
            ));
        }

        // 3. A *device* is our connection to the adapter; the *queue* sends it work.
        //    We ask for modest limits that older graphics cards can meet too,
        //    except for picture and buffer sizes, where we take what the card has.
        let limits = wgpu::Limits {
            max_texture_dimension_2d: supported.max_texture_dimension_2d,
            max_buffer_size: supported.max_buffer_size,
            ..wgpu::Limits::downlevel_defaults()
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("duckforge"),
            required_limits: limits.clone(),
            ..Default::default()
        }))
        .map_err(|err| format!("could not open the graphics device: {err}"))?;
        let max_batch_instances =
            ((limits.max_buffer_size / std::mem::size_of::<Instance>() as u64) as usize)
                .clamp(1, MAX_BATCH_INSTANCES);

        // 4. The shader programs, and a *pipeline*: the full recipe for drawing.
        let shader = device.create_shader_module(wgpu::include_wgsl!("gpu.wgsl"));

        let globals_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("globals"),
            size: std::mem::size_of::<Globals>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("globals"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("globals"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        // Textures: the picture, and a *sampler* that says how to read it:
        // the nearest pixel (sharp pixel art), repeating past the edges.
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nearest"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let white = Sprite::from_pixels(1, 1, vec![crate::color::Color::WHITE]);
        let plain = texture_bind_group(&device, &queue, &texture_layout, &sampler, &white);

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("mesh"),
            bind_group_layouts: &[Some(&globals_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let vertex_attributes =
            wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4, 6 => Float32x2, 7 => Uint32];
        let instance_attributes = wgpu::vertex_attr_array![
            2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 8 => Float32x4
        ];

        // Two pipelines, the same but for how pixels are written: solid
        // objects replace the pixel and write the depth; see-through ones
        // mix over it and don't.
        let make_pipeline = |see_through: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("mesh"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[
                        // Buffer 0: the mesh's corners, one per shader run.
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Vertex>() as u64,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes: &vertex_attributes,
                        }),
                        // Buffer 1: one matrix per *object*. `Instance` step mode means
                        // the GPU moves to the next matrix only for the next copy
                        // of the whole mesh. That's instancing.
                        Some(wgpu::VertexBufferLayout {
                            array_stride: std::mem::size_of::<Instance>() as u64,
                            step_mode: wgpu::VertexStepMode::Instance,
                            attributes: &instance_attributes,
                        }),
                    ],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    // Our rule: corners go clockwise when seen from the front.
                    front_face: wgpu::FrontFace::Cw,
                    cull_mode: Some(wgpu::Face::Back), // back-face culling, in hardware
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: Some(!see_through),
                    // Depth is `near / distance`, so bigger means closer (like the CPU's 1 / z).
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOR_FORMAT,
                        // Solid: just replace the pixel. See-through: new * alpha
                        // + old * (1 - alpha).
                        blend: see_through.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipeline = make_pipeline(false);
        let see_through_pipeline = make_pipeline(true);

        // 5. The picture, the depth buffer, and a buffer to copy the picture into.
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("color"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: COLOR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let depth_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("depth"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });

        // Copies from a texture into a buffer need each row to take a
        // multiple of 256 bytes, hence `padded_bytes_per_row` above.
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: readback_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let instance_buffer = create_instance_buffer(&device, 1024);

        Ok(Self {
            color_view: color_texture.create_view(&Default::default()),
            depth_view: depth_texture.create_view(&Default::default()),
            color_texture,
            device,
            queue,
            description,
            pipeline,
            see_through_pipeline,
            globals_buffer,
            globals_bind_group,
            readback_buffer,
            padded_bytes_per_row,
            width,
            height,
            instance_buffer,
            texture_layout,
            sampler,
            plain,
            textures: FastMap::default(),
            lighting: Lighting::default(),
            fog: None,
            meshes: FastMap::default(),
            seen: FastMap::default(),
            pending: FastMap::default(),
            pending_see_through: Vec::new(),
            camera: None,
            clear_depth: true,
            flushes: 0,
            pending_instances: 0,
            max_batch_instances,
        })
    }

    /// "GPU: " plus the graphics card's name and which API wgpu uses to talk to it.
    pub(crate) fn description(&self) -> &str {
        &self.description
    }

    /// Called by `Canvas::clear`: forget pending drawing and start the next
    /// batch with an empty depth buffer.
    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        self.pending_see_through.clear();
        self.pending_instances = 0;
        self.seen.clear();
        self.camera = None;
        self.clear_depth = true;
    }

    /// The next batch starts with an empty depth buffer: it's drawn in
    /// front of everything before it (call `flush` first).
    pub(crate) fn forget_depth(&mut self) {
        self.clear_depth = true;
    }

    /// Records one object to draw at the next flush.
    pub(crate) fn draw(
        &mut self,
        mesh: &Mesh,
        transform: &Transform,
        camera: &Camera3D,
        look: Look,
        pixels: &mut [u32],
    ) {
        let Look {
            lighting,
            fog,
            tint,
        } = look;
        if mesh.triangles.is_empty() {
            return; // nothing to draw (and the GPU can't hold an empty buffer)
        }
        // Everything in one batch shares a camera and a light. A new one?
        // Draw what we have first.
        let changed = self.camera.is_some_and(|current| current != *camera)
            || (self.camera.is_some() && (self.lighting != lighting || self.fog != fog));
        // A solid object after see-through ones: draw those first, so the
        // picture follows the drawing order, as on the CPU.
        let solid_after_glass = tint.a == 255 && !self.pending_see_through.is_empty();
        if changed || solid_after_glass {
            self.flush(pixels);
        }
        self.camera = Some(*camera);
        self.lighting = lighting;
        self.fog = fog;

        let key = self.key_for(mesh);
        if !self.meshes.contains_key(&key) {
            self.upload(key, mesh);
        }
        let cached = self.meshes.get_mut(&key).expect("uploaded just above");
        cached.last_used = self.flushes;
        // Frustum culling: a copy whose ball is off screen is skipped.
        let (center, radius) = cached.ball;
        let (width, height) = (self.width as f32, self.height as f32);
        if !camera.might_see(
            transform.apply(center),
            radius * transform.largest_scale(),
            width,
            height,
        ) {
            return;
        }
        let instance = Instance {
            model: transform.matrix().columns,
            tint: [tint.r, tint.g, tint.b, tint.a].map(|c| c as f32 / 255.0),
        };
        if tint.a < 255 {
            self.pending_see_through.push((key, instance));
        } else {
            self.pending.entry(key).or_default().push(instance);
        }
        self.pending_instances += 1;
        if self.pending_instances >= self.max_batch_instances {
            self.flush(pixels); // a huge batch: draw this part now
        }
    }

    /// Works out the cache key for `mesh`, fingerprinting it only if this
    /// address hasn't been seen this batch, or seems to hold something else now.
    fn key_for(&mut self, mesh: &Mesh) -> MeshKey {
        let address = address_of(mesh);
        let quick = sample(mesh);
        match self.seen.get(&address) {
            Some(seen) if seen.sample == quick => (address, seen.fingerprint),
            _ => {
                let full = fingerprint(mesh);
                let seen = Seen {
                    sample: quick,
                    fingerprint: full,
                };
                self.seen.insert(address, seen);
                (address, full)
            }
        }
    }

    /// Sends a copy of `mesh` (and its texture, if the GPU hasn't got it
    /// yet) to the GPU.
    fn upload(&mut self, key: MeshKey, mesh: &Mesh) {
        // One GPU vertex per triangle corner, each carrying the triangle's
        // color and glow, and its texture point if it's textured.
        let vertices: Vec<Vertex> = mesh
            .triangles
            .iter()
            .enumerate()
            .flat_map(|(index, triangle)| {
                let level = mesh.glow_of(index);
                let glow = if level >= 1.0 { GLOWING } else { 0 };
                let partly = (level * 255.0).round() as u8;
                let color = [triangle.color.r, triangle.color.g, triangle.color.b, partly];
                let uvs = mesh.uvs_of(index);
                let flags = glow | if uvs.is_some() { TEXTURED } else { 0 };
                let corners = triangle_corners(&mesh.vertices, index, triangle);
                [0, 1, 2].map(|i| {
                    let p = corners[i];
                    let uv = uvs.map_or([0.0, 0.0], |uvs| [uvs[i].x, uvs[i].y]);
                    Vertex {
                        position: [p.x, p.y, p.z],
                        color,
                        uv,
                        flags,
                    }
                })
            })
            .collect();
        let texture = mesh.texture.as_ref().map(|sprite| {
            let texture_key = sprite.identity();
            if !self.textures.contains_key(&texture_key) {
                let bind_group = texture_bind_group(
                    &self.device,
                    &self.queue,
                    &self.texture_layout,
                    &self.sampler,
                    sprite,
                );
                let texture = GpuTexture {
                    bind_group,
                    _sprite: sprite.clone(),
                    last_used: self.flushes,
                };
                self.textures.insert(texture_key, texture);
            }
            texture_key
        });

        use wgpu::util::DeviceExt; // brings `create_buffer_init` into scope
        let vertex_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("mesh"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        self.meshes.insert(
            key,
            GpuMesh {
                vertex_buffer,
                vertex_count: vertices.len() as u32,
                texture,
                ball: mesh.bounding_ball(),
                last_used: self.flushes,
            },
        );
    }

    /// Draws everything recorded since the last flush on top of `pixels`
    /// (the canvas), then copies the result back into `pixels`.
    pub(crate) fn flush(&mut self, pixels: &mut [u32]) {
        let Some(camera) = self.camera else {
            return;
        };
        if self.pending.is_empty() && self.pending_see_through.is_empty() {
            return;
        }

        // --- Send this batch's data to the GPU. ---

        // The canvas as it is now, so the 3D is drawn over any 2D background.
        // A canvas pixel `0x00RRGGBB` is stored as the bytes B, G, R, 0: exactly
        // the `Bgra8Unorm` texture format, so no conversion is needed.
        self.queue.write_texture(
            self.color_texture.as_image_copy(),
            bytemuck::cast_slice(pixels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.width * 4),
                rows_per_image: Some(self.height),
            },
            self.color_texture.size(),
        );

        let aspect = self.width as f32 / self.height as f32;
        let view_projection = camera.projection_matrix(aspect) * camera.view_matrix();
        let sun = self.lighting.sun();
        let [r, g, b] = self.lighting.sun_rgb();
        let globals = Globals {
            view_projection: view_projection.columns,
            camera_position: [camera.position.x, camera.position.y, camera.position.z, 0.0],
            sun: [sun.x, sun.y, sun.z, self.lighting.ambient_level()],
            sun_color: [r, g, b, 0.0],
            fog_color: self.fog.map_or([0.0; 4], |fog| {
                let c = fog.color;
                [
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    1.0,
                ]
            }),
            fog_range: self
                .fog
                .map_or([0.0; 4], |fog| [fog.near, fog.far, 0.0, 0.0]),
            camera_forward: {
                let f = camera.forward();
                [f.x, f.y, f.z, 0.0]
            },
        };
        self.queue
            .write_buffer(&self.globals_buffer, 0, bytemuck::bytes_of(&globals));

        // All objects' matrices go into one buffer, mesh after mesh, then
        // the see-through objects one by one.
        let batches: Vec<(MeshKey, Vec<Instance>)> = self.pending.drain().collect();
        let see_through = std::mem::take(&mut self.pending_see_through);
        let total: usize = batches
            .iter()
            .map(|(_, instances)| instances.len())
            .sum::<usize>()
            + see_through.len();
        let needed = (total * std::mem::size_of::<Instance>()) as u64;
        if self.instance_buffer.size() < needed {
            let capacity = total.next_power_of_two().min(self.max_batch_instances);
            self.instance_buffer = create_instance_buffer(&self.device, capacity);
        }
        let all_instances: Vec<Instance> = batches
            .iter()
            .flat_map(|(_, list)| list.iter().copied())
            .chain(see_through.iter().map(|&(_, instance)| instance))
            .collect();
        self.queue.write_buffer(
            &self.instance_buffer,
            0,
            bytemuck::cast_slice(&all_instances),
        );

        // --- Record the GPU's work: one render pass, then a copy. ---

        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let depth_load = if self.clear_depth {
                wgpu::LoadOp::Clear(0.0) // 0.0 = infinitely far away
            } else {
                wgpu::LoadOp::Load // keep depths from earlier batches this frame
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("meshes"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.color_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load, // start from the uploaded canvas
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: depth_load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.globals_bind_group, &[]);

            // One draw call per mesh, however many copies of it there are;
            // then each see-through object, in the order it was drawn.
            let solid = batches.iter().map(|(key, list)| (key, list.len() as u32));
            let glassy = see_through.iter().map(|(key, _)| (key, 1));
            let mut first = 0u32;
            for (n, (key, count)) in solid.chain(glassy).enumerate() {
                if n == batches.len() {
                    pass.set_pipeline(&self.see_through_pipeline);
                }
                let mesh = &self.meshes[key];
                let texture = match mesh.texture.and_then(|t| self.textures.get_mut(&t)) {
                    Some(texture) => {
                        texture.last_used = self.flushes;
                        &texture.bind_group
                    }
                    None => &self.plain,
                };
                pass.set_bind_group(1, texture, &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, self.instance_buffer.slice(..));
                pass.draw(0..mesh.vertex_count, first..first + count);
                first += count;
            }
        }
        encoder.copy_texture_to_buffer(
            self.color_texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback_buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.padded_bytes_per_row),
                    rows_per_image: Some(self.height),
                },
            },
            self.color_texture.size(),
        );
        self.queue.submit([encoder.finish()]);

        // --- Wait for the GPU, then copy the picture back into the canvas. ---

        self.readback_buffer
            .map_async(wgpu::MapMode::Read, .., |result| {
                result.expect("could not read the picture back from the GPU");
            });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the GPU stopped responding");
        {
            let bytes = self
                .readback_buffer
                .get_mapped_range(..)
                .expect("the picture should be readable after mapping");
            let row_bytes = self.width as usize * 4;
            for (y, row) in pixels.chunks_exact_mut(self.width as usize).enumerate() {
                let start = y * self.padded_bytes_per_row as usize;
                let source = &bytes[start..start + row_bytes];
                for (pixel, bgra) in row.iter_mut().zip(source.chunks_exact(4)) {
                    // Bytes B, G, R, A back into 0x00RRGGBB (dropping A).
                    *pixel = u32::from_le_bytes([bgra[0], bgra[1], bgra[2], 0]);
                }
            }
        }
        self.readback_buffer.unmap();

        // Forget meshes that haven't been drawn for a while, to free GPU memory.
        self.flushes += 1;
        let now = self.flushes;
        self.meshes
            .retain(|_, mesh| now - mesh.last_used <= EVICT_AFTER_FLUSHES);
        self.textures
            .retain(|_, texture| now - texture.last_used <= EVICT_AFTER_FLUSHES);
        self.seen.clear(); // next batch: check every mesh again
        self.pending_instances = 0;
        self.clear_depth = false;
    }
}

/// The picture's format on the GPU. Blue, green, red, alpha: the same byte
/// order as the canvas's pixels.
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// Sends a sprite's pixels to the GPU as a texture, and makes the bind
/// group the shader reads it through.
fn texture_bind_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    sprite: &Sprite,
) -> wgpu::BindGroup {
    let (width, height) = (sprite.width().max(1) as u32, sprite.height().max(1) as u32);
    // Red, green, blue, alpha bytes, row by row.
    let mut bytes = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..sprite.height() {
        for x in 0..sprite.width() {
            let (rgb, alpha) = sprite.texel(x, y);
            bytes.extend_from_slice(&[(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, alpha]);
        }
    }
    if bytes.is_empty() {
        bytes = vec![255; 4]; // an empty sprite: one white pixel
    }
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("texture"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        // Plain numbers, not sRGB: the shader multiplies them exactly as
        // the CPU renderer does.
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        size,
    );
    let view = texture.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("texture"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn create_instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("instances"),
        size: (capacity.max(1) * std::mem::size_of::<Instance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[cfg(test)]
mod tests {
    use super::GpuRenderer;
    use crate::canvas::Canvas;
    use crate::color::Color;
    use crate::math::{Rect, Vec3, vec3};
    use crate::render3d::{Camera3D, Mesh, Renderer, Transform};

    /// A canvas that draws 3D on the GPU, or `None` (and the test is
    /// skipped) on a machine without a usable graphics adapter.
    fn gpu_canvas(width: usize, height: usize) -> Option<Canvas> {
        let mut canvas = Canvas::new(width, height);
        canvas.set_renderer(Renderer::Gpu);
        // Draw one mesh off-screen to start the GPU up.
        let camera = Camera3D::new(Vec3::ZERO);
        canvas.draw_mesh(
            &Mesh::cube(Color::RED),
            &Transform::at(vec3(0.0, 0.0, -5.0)),
            &camera,
        );
        canvas.clear(Color::BLACK);
        if canvas.renderer_name().starts_with("GPU:") {
            Some(canvas)
        } else {
            eprintln!("no GPU adapter: skipping this test");
            None
        }
    }

    /// 2D background, a floor, several shapes at odd angles, and 2D on top.
    fn draw_scene(canvas: &mut Canvas) {
        let camera = Camera3D::looking_at(vec3(-3.0, 4.0, -7.0), vec3(0.0, 0.5, 0.0));
        canvas.clear(Color::from_hex(0x20_30_40));
        canvas.fill_rect(Rect::new(0.0, 0.0, 40.0, 30.0), Color::GREEN); // under the 3D
        let floor = Mesh::checkerboard(8, 1.0, Color::GRAY, Color::WHITE);
        canvas.draw_mesh(&floor, &Transform::default(), &camera);
        let shapes = [
            (Mesh::cube(Color::RED), vec3(-1.5, 0.5, 0.0)),
            (Mesh::sphere(Color::BLUE, 16), vec3(1.2, 0.7, 0.5)),
            (Mesh::pyramid(Color::YELLOW), vec3(0.0, 0.5, 2.0)),
        ];
        for (i, (mesh, position)) in shapes.iter().enumerate() {
            let transform = Transform::at(*position)
                .rotated(vec3(0.3 * i as f32, 0.8 + i as f32, 0.1))
                .scaled(vec3(1.0, 1.3, 0.9));
            canvas.draw_mesh(mesh, &transform, &camera);
        }
        canvas.fill_rect(Rect::new(120.0, 90.0, 40.0, 30.0), Color::PURPLE); // over the 3D
    }

    #[test]
    fn gpu_picture_matches_the_cpu_picture() {
        let Some(mut gpu) = gpu_canvas(160, 120) else {
            return;
        };
        let mut cpu = Canvas::new(160, 120);
        draw_scene(&mut gpu);
        draw_scene(&mut cpu);
        gpu.flush_3d();

        // The two renderers may round a few edge pixels differently, and
        // their colors may differ by a tiny bit. Everything else must match.
        let mut different = 0;
        for (&g, &c) in gpu.pixels().iter().zip(cpu.pixels()) {
            let (g, c) = (Color::from_hex(g), Color::from_hex(c));
            let gap = (g.r.abs_diff(c.r))
                .max(g.g.abs_diff(c.g))
                .max(g.b.abs_diff(c.b));
            if gap > 3 {
                different += 1;
            }
        }
        let share = different as f32 / gpu.pixels().len() as f32;
        assert!(share < 0.02, "{:.1}% of pixels differ", share * 100.0);
    }

    /// How many pixels of two pictures differ by more than 3 in any colour,
    /// as a share of all of them.
    fn share_different(a: &Canvas, b: &Canvas) -> f32 {
        let different = a
            .pixels()
            .iter()
            .zip(b.pixels())
            .filter(|&(&x, &y)| {
                let (x, y) = (Color::from_hex(x), Color::from_hex(y));
                x.r.abs_diff(y.r)
                    .max(x.g.abs_diff(y.g))
                    .max(x.b.abs_diff(y.b))
                    > 3
            })
            .count();
        different as f32 / a.pixels().len() as f32
    }

    /// Textured shapes, see-through texels, a glowing cube and a coloured
    /// sun.
    fn draw_textured_scene(canvas: &mut Canvas) {
        let camera = Camera3D::looking_at(vec3(-2.0, 3.0, -6.0), vec3(0.0, 0.5, 0.0));
        canvas.clear(Color::from_hex(0x10_20_30));
        canvas.set_lighting(crate::render3d::Lighting {
            sun_direction: vec3(0.3, 1.0, -0.5),
            sun_color: Color::from_hex(0xffe0b0),
            sun_strength: 0.9,
            ambient: 0.45,
        });
        // An 8 x 8 picture: a checker of four colours, and a see-through corner.
        let pixels = (0..64)
            .map(|i| {
                let (x, y) = (i % 8, i / 8);
                if x < 2 && y < 2 {
                    Color::rgba(0, 0, 0, 0)
                } else {
                    [Color::RED, Color::YELLOW, Color::GREEN, Color::WHITE][(x / 2 + y / 2) % 4]
                }
            })
            .collect();
        let picture = crate::sprite::Sprite::from_pixels(8, 8, pixels);
        let floor =
            Mesh::checkerboard(4, 2.0, Color::WHITE, Color::GRAY).with_texture(picture.clone());
        canvas.draw_mesh(&floor, &Transform::default(), &camera);
        let crate_mesh = Mesh::cube(Color::WHITE).with_texture(picture.clone());
        canvas.draw_mesh(
            &crate_mesh,
            &Transform::at(vec3(-1.0, 0.6, 0.0))
                .rotated(vec3(0.0, 0.6, 0.0))
                .sized(1.2),
            &camera,
        );
        let ball = Mesh::sphere(Color::from_hex(0xc0_c0_ff), 16).with_texture(picture);
        canvas.draw_mesh(&ball, &Transform::at(vec3(1.2, 0.8, 0.5)), &camera);
        let lamp = Mesh::pyramid(Color::from_hex(0xff_80_20)).glowing();
        canvas.draw_mesh(&lamp, &Transform::at(vec3(0.0, 0.5, 2.0)), &camera);
        // A billboard: a 4 x 6 figure with see-through corners, behind the crate.
        let figure = (0..24)
            .map(|i| match (i % 4, i / 4) {
                (0 | 3, 0) => Color::rgba(0, 0, 0, 0),
                (_, 5) => Color::BLACK,
                _ => Color::from_hex(0xff_d0_40),
            })
            .collect();
        let figure = crate::sprite::Sprite::from_pixels(4, 6, figure);
        canvas.draw_billboard(&figure, vec3(-1.5, 0.0, 1.5), 1.5, &camera);
    }

    #[test]
    fn textured_and_lit_gpu_picture_matches_the_cpu_picture() {
        let Some(mut gpu) = gpu_canvas(160, 120) else {
            return;
        };
        let mut cpu = Canvas::new(160, 120);
        draw_textured_scene(&mut gpu);
        draw_textured_scene(&mut cpu);
        gpu.flush_3d();
        let share = share_different(&gpu, &cpu);
        assert!(share < 0.02, "{:.1}% of pixels differ", share * 100.0);
    }

    #[test]
    fn glow_levels_and_fog_are_drawn_like_the_cpu() {
        let Some(mut gpu) = gpu_canvas(160, 120) else {
            return;
        };
        let mut cpu = Canvas::new(160, 120);
        crate::render3d::draw_glow_and_fog_scene(&mut gpu);
        crate::render3d::draw_glow_and_fog_scene(&mut cpu);
        gpu.flush_3d();
        let share = share_different(&gpu, &cpu);
        assert!(share < 0.02, "{:.1}% of pixels differ", share * 100.0);
    }

    #[test]
    fn tinted_and_see_through_meshes_are_drawn_like_the_cpu() {
        for green in [false, true] {
            let Some(mut gpu) = gpu_canvas(80, 60) else {
                return;
            };
            let mut cpu = Canvas::new(80, 60);
            crate::render3d::draw_glass_scene(&mut gpu, green);
            crate::render3d::draw_glass_scene(&mut cpu, green);
            gpu.flush_3d();
            let share = share_different(&gpu, &cpu);
            assert!(share < 0.02, "{:.1}% of pixels differ", share * 100.0);
            let (g, c) = (
                gpu.get_pixel(40, 30).unwrap(),
                cpu.get_pixel(40, 30).unwrap(),
            );
            let near = |a: u8, b: u8| a.abs_diff(b) <= 2;
            assert!(
                near(g.r, c.r) && near(g.g, c.g) && near(g.b, c.b),
                "{g:?} {c:?}"
            );
        }
    }

    #[test]
    fn a_mesh_in_view_is_drawn_like_the_cpu() {
        for (yaw, glowing) in [(0.0, true), (2.5, false)] {
            let Some(mut gpu) = gpu_canvas(80, 60) else {
                return;
            };
            let mut cpu = Canvas::new(80, 60);
            crate::render3d::draw_hand_in_front_of_wall(&mut gpu, yaw, glowing);
            crate::render3d::draw_hand_in_front_of_wall(&mut cpu, yaw, glowing);
            gpu.flush_3d();
            let share = share_different(&gpu, &cpu);
            assert!(share < 0.02, "{:.1}% of pixels differ", share * 100.0);
            assert_eq!(gpu.get_pixel(40, 30), cpu.get_pixel(40, 30));
        }
    }

    /// A tile cut from an atlas repeats on its own on the GPU too.
    #[test]
    fn an_atlas_tile_repeats_on_its_own_like_the_cpu() {
        let Some(mut gpu) = gpu_canvas(80, 60) else {
            return;
        };
        let mut cpu = Canvas::new(80, 60);
        crate::render3d::draw_repeated_tile_floor(&mut gpu);
        crate::render3d::draw_repeated_tile_floor(&mut cpu);
        gpu.flush_3d();
        let blue = Color::BLUE.to_u32();
        assert!(!gpu.pixels().contains(&blue), "the blue tile never shows");
        let share = share_different(&gpu, &cpu);
        assert!(share < 0.02, "{:.1}% of pixels differ", share * 100.0);
    }

    /// Saves `out/cpu-vs-gpu.png`: the textured, lit test scene drawn by
    /// the CPU (left) and the GPU (right), and prints how many pixels
    /// differ. For checking a real graphics card by eye:
    /// `cargo test --release --lib cpu_and_gpu_side_by_side -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn cpu_and_gpu_side_by_side() {
        let Some(mut gpu) = gpu_canvas(320, 240) else {
            return;
        };
        let mut cpu = Canvas::new(320, 240);
        draw_textured_scene(&mut gpu);
        draw_textured_scene(&mut cpu);
        gpu.flush_3d();
        let mut both = Canvas::new(640, 240);
        for y in 0..240 {
            for x in 0..320 {
                both.set_pixel(x, y, cpu.get_pixel(x, y).unwrap());
                both.set_pixel(x + 320, y, gpu.get_pixel(x, y).unwrap());
            }
        }
        std::fs::create_dir_all("out").unwrap();
        both.save_png("out/cpu-vs-gpu.png").unwrap();
        println!(
            "{}: {:.2}% of pixels differ by more than 3; saved out/cpu-vs-gpu.png",
            gpu.renderer_name(),
            share_different(&gpu, &cpu) * 100.0
        );
    }

    #[test]
    fn two_dimensional_drawing_layers_under_and_over_3d() {
        let Some(mut canvas) = gpu_canvas(64, 48) else {
            return;
        };
        let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
        canvas.clear(Color::BLACK);
        canvas.fill_rect(Rect::new(0.0, 0.0, 64.0, 48.0), Color::GREEN); // background
        canvas.draw_mesh(&Mesh::cube(Color::RED), &Transform::default(), &camera);
        canvas.fill_rect(Rect::new(30.0, 22.0, 4.0, 4.0), Color::WHITE); // on top
        canvas.flush_3d();

        assert_eq!(canvas.get_pixel(1, 1), Some(Color::GREEN)); // background shows
        assert_eq!(canvas.get_pixel(32, 24), Some(Color::WHITE)); // 2D over 3D
        let cube = canvas.get_pixel(26, 24).unwrap();
        // Red, shaded to 66% brightness: about (153, 40, 40).
        assert!(
            cube.r > 100 && cube.r > 3 * cube.g,
            "the cube is drawn: {cube:?}"
        );
    }

    #[test]
    fn temporary_meshes_keep_their_own_colors() {
        // Meshes made on the spot are dropped right after drawing, so the
        // next one often lands at the same memory address.
        let Some(mut canvas) = gpu_canvas(64, 48) else {
            return;
        };
        let camera = Camera3D::new(vec3(0.0, 0.0, -4.0));
        canvas.clear(Color::BLACK);
        canvas.draw_mesh(
            &Mesh::cube(Color::RED),
            &Transform::at(vec3(-1.0, 0.0, 0.0)),
            &camera,
        );
        canvas.draw_mesh(
            &Mesh::cube(Color::BLUE),
            &Transform::at(vec3(1.0, 0.0, 0.0)),
            &camera,
        );
        canvas.flush_3d();

        let left = canvas.get_pixel(20, 24).unwrap();
        let right = canvas.get_pixel(44, 24).unwrap();
        assert!(left.r > left.b, "left cube should be red: {left:?}");
        assert!(right.b > right.r, "right cube should be blue: {right:?}");
    }

    #[test]
    fn edited_meshes_are_uploaded_again() {
        let Some(mut canvas) = gpu_canvas(32, 24) else {
            return;
        };
        let camera = Camera3D::new(vec3(0.0, 0.0, -2.0));
        let mut mesh = Mesh::cube(Color::RED);
        canvas.draw_mesh(&mesh, &Transform::default(), &camera);
        canvas.flush_3d();
        assert!(canvas.get_pixel(16, 12).unwrap().r > 100);

        mesh = mesh.with_color(Color::BLUE); // same sizes, likely the same memory
        canvas.clear(Color::BLACK);
        canvas.draw_mesh(&mesh, &Transform::default(), &camera);
        canvas.flush_3d();
        let pixel = canvas.get_pixel(16, 12).unwrap();
        // Blue, shaded to 66% brightness: about (46, 86, 152).
        assert!(
            pixel.b > 100 && pixel.b > 3 * pixel.r,
            "should be blue now: {pixel:?}"
        );
    }

    #[test]
    fn thousands_of_instances() {
        let Some(mut canvas) = gpu_canvas(100, 100) else {
            return;
        };
        let camera = Camera3D::looking_at(vec3(0.0, 60.0, -1.0), Vec3::ZERO);
        let cube = Mesh::cube(Color::ORANGE);
        canvas.clear(Color::BLACK);
        for x in -50..50 {
            for z in -50..50 {
                let at = vec3(x as f32 * 0.6, 0.0, z as f32 * 0.6);
                canvas.draw_mesh(&cube, &Transform::at(at).sized(0.4), &camera);
            }
        }
        canvas.flush_3d();
        let lit = canvas.pixels().iter().filter(|&&p| p != 0).count();
        assert!(
            lit > 3_000,
            "10,000 cubes should cover much of the view ({lit} pixels lit)"
        );
    }

    #[test]
    fn very_large_batches_are_split() {
        let Ok(mut gpu) = GpuRenderer::new(40, 40) else {
            return;
        };
        let camera = Camera3D::looking_at(vec3(0.0, 20.0, -0.1), Vec3::ZERO);
        let cube = Mesh::cube(Color::ORANGE);
        let draw_all = |gpu: &mut GpuRenderer, pixels: &mut [u32]| {
            for i in 0..25 {
                let at = vec3((i % 5) as f32 * 2.0 - 4.0, 0.0, (i / 5) as f32 * 2.0 - 4.0);
                gpu.draw(
                    &cube,
                    &Transform::at(at),
                    &camera,
                    super::Look {
                        lighting: crate::render3d::Lighting::default(),
                        fog: None,
                        tint: Color::WHITE,
                    },
                    pixels,
                );
            }
            gpu.flush(pixels);
        };

        let mut one_batch = vec![0; 40 * 40];
        draw_all(&mut gpu, &mut one_batch);

        gpu.clear();
        gpu.max_batch_instances = 10; // pretend the GPU only takes 10 at a time
        let before = gpu.flushes;
        let mut split = vec![0; 40 * 40];
        draw_all(&mut gpu, &mut split);

        assert_eq!(gpu.flushes - before, 3, "25 objects in batches of 10");
        assert_eq!(one_batch, split, "splitting must not change the picture");
    }

    #[test]
    fn empty_meshes_draw_nothing() {
        let Some(mut canvas) = gpu_canvas(32, 24) else {
            return;
        };
        let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
        canvas.clear(Color::BLACK);
        let nothing = Mesh::checkerboard(0, 1.0, Color::RED, Color::RED);
        canvas.draw_mesh(&Mesh::new(), &Transform::default(), &camera);
        canvas.draw_mesh(&nothing, &Transform::default(), &camera);
        canvas.draw_mesh(&Mesh::cube(Color::RED), &Transform::default(), &camera);
        canvas.flush_3d();
        assert!(
            canvas.get_pixel(16, 12).unwrap().r > 100,
            "the cube still draws"
        );
    }

    #[test]
    fn canvases_too_big_for_the_gpu_fall_back_to_the_cpu() {
        let mut canvas = Canvas::new(40_000, 2); // wider than any GPU allows
        canvas.set_renderer(Renderer::Gpu);
        let camera = Camera3D::new(vec3(0.0, 0.0, -3.0));
        canvas.draw_mesh(&Mesh::cube(Color::RED), &Transform::default(), &camera);
        canvas.flush_3d();
        assert_eq!(canvas.renderer_name(), "CPU");
    }
}
