// The programs that run on the graphics card, written in WGSL (the WebGPU
// Shading Language). The GPU runs `vs_main` once for every corner of every
// triangle, then `fs_main` once for every pixel the triangle covers.

// Settings shared by everything drawn in one batch.
struct Globals {
    // World space to the screen: camera.projection_matrix() * camera.view_matrix().
    view_projection: mat4x4<f32>,
    // Where the camera is (w is unused).
    camera_position: vec4<f32>,
    // xyz: the direction sunlight comes from. w: the ambient light level.
    sun: vec4<f32>,
    // rgb: the sun's colour times its strength (1, 1, 1 by default). w is unused.
    sun_color: vec4<f32>,
    // Distance fog. rgb: its colour. w: 1 with fog, 0 without.
    fog_color: vec4<f32>,
    // x: where fog starts, y: where it's complete (distances ahead of the camera).
    fog_range: vec4<f32>,
    // The way the camera looks (w is unused), to measure "ahead of the camera".
    camera_forward: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

// The mesh's texture, and how it's read: the nearest pixel, repeating.
@group(1) @binding(0) var picture: texture_2d<f32>;
@group(1) @binding(1) var picture_sampler: sampler;

// Vertex flags (see gpu.rs): the triangle is textured; the mesh glows.
const TEXTURED: u32 = 1u;
const GLOWING: u32 = 2u;

struct VertexIn {
    // From the mesh: one corner of one triangle.
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(6) uv: vec2<f32>,
    @location(7) flags: u32,
    // From the object being drawn: its transform matrix, one column at a time.
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    // From the object: its tint (rgb multiply the colour; a is how solid).
    @location(8) tint: vec4<f32>,
};

struct VertexOut {
    // Where the corner lands on screen. The GPU divides x, y and z by w.
    @builtin(position) clip_position: vec4<f32>,
    // Passed on to fs_main, blended across the triangle.
    @location(0) world_position: vec3<f32>,
    @location(1) color: vec3<f32>,
    // The texture point, blended across the triangle. The GPU does that
    // perspective-correctly by itself.
    @location(2) uv: vec2<f32>,
    // The same for the whole triangle, so not blended at all.
    @location(3) @interpolate(flat) flags: u32,
    @location(4) @interpolate(flat) tint: vec4<f32>,
    // How much the triangle glows: it's at least this bright.
    @location(5) @interpolate(flat) glow: f32,
};

@vertex
fn vs_main(v: VertexIn) -> VertexOut {
    let model = mat4x4<f32>(v.model_0, v.model_1, v.model_2, v.model_3);
    let world = model * vec4<f32>(v.position, 1.0);

    var out: VertexOut;
    out.clip_position = globals.view_projection * world;
    out.world_position = world.xyz;
    out.color = v.color.rgb;
    out.glow = v.color.a;
    out.uv = v.uv;
    out.flags = v.flags;
    out.tint = v.tint;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    // The triangle's normal. `dpdx` and `dpdy` say how the world position
    // changes from this pixel to its right and lower neighbors: two
    // directions lying flat in the triangle. Their cross product sticks
    // straight out of it, just like `(b - a).cross(c - a)` on the CPU.
    let to_camera = globals.camera_position.xyz - in.world_position;
    var normal = cross(dpdx(in.world_position), dpdy(in.world_position));
    // A zero-length normal can't be normalized (it would give NaN, and a
    // black pixel). It only happens for triangles too thin to see, so any
    // direction will do: face the camera.
    if dot(normal, normal) == 0.0 {
        normal = to_camera;
    }
    normal = normalize(normal);
    // Make sure it points out of the front, towards the camera.
    if dot(normal, to_camera) < 0.0 {
        normal = -normal;
    }

    // The texture's colour times the triangle's, as on the CPU. See-through
    // texture pixels aren't drawn at all (nor written to the depth buffer).
    // (`textureSampleLevel`, unlike `textureSample`, may be used inside an `if`.)
    var color = in.color;
    if (in.flags & TEXTURED) != 0u {
        let texel = textureSampleLevel(picture, picture_sampler, in.uv, 0.0);
        if texel.a < 0.5 {
            discard;
        }
        color = color * texel.rgb;
    }
    color = color * in.tint.rgb;
    var lit = color;
    if (in.flags & GLOWING) == 0u {
        // The same flat shading as the CPU renderer.
        let sunlight = max(dot(normal, globals.sun.xyz), 0.0);
        let ambient = globals.sun.w;
        let brightness = max(
            ambient + (1.0 - ambient) * (sunlight * globals.sun_color.rgb),
            vec3<f32>(in.glow),
        );
        lit = color * brightness;
    }
    // Distance fog, by how far ahead of the camera this pixel is (as on the CPU).
    if globals.fog_color.w > 0.5 {
        let ahead = dot(in.world_position - globals.camera_position.xyz, globals.camera_forward.xyz);
        let near = globals.fog_range.x;
        let far = globals.fog_range.y;
        var amount = 0.0;
        if far > near {
            amount = clamp((ahead - near) / (far - near), 0.0, 1.0);
        } else if ahead >= far {
            amount = 1.0;
        }
        lit = mix(lit, globals.fog_color.rgb, amount);
    }
    return vec4<f32>(lit, in.tint.a);
}
