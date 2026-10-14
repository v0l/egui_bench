struct Frame {
    view_proj: mat4x4<f32>,
    eye: vec4<f32>,
    key: vec4<f32>,
    fill: vec4<f32>,
    key_radiance: vec4<f32>,
    fill_radiance: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    cap: vec4<f32>,
    clip: vec4<f32>,
    viewport: vec4<f32>,
    forward: vec4<f32>,
    debug: vec4<f32>,
}

struct Draw {
    model: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    gloss: vec4<f32>,
    params: vec4<f32>,
    colour: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(1) @binding(0) var<uniform> draw: Draw;
@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var samp: sampler;

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c < vec3<f32>(0.04045));
}

fn to_srgb(raw: vec3<f32>) -> vec3<f32> {
    let c = clamp(raw, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c < vec3<f32>(0.0031308));
}

fn clipped(pos: vec3<f32>) -> bool {
    return frame.viewport.z > 0.5 && dot(pos, frame.clip.xyz) > frame.clip.w;
}

struct SurfaceIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) colour: vec3<f32>,
    @location(3) uv: vec2<f32>,
}

struct SurfaceOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec3<f32>,
    @location(1) nor: vec3<f32>,
    @location(2) col: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) bary: vec3<f32>,
}

const SHADED: u32 = 0u;
const WIREFRAME: u32 = 1u;
const TRIANGLES: u32 = 2u;
const NORMALS: u32 = 3u;
const FACETS: u32 = 4u;
const DEPTH: u32 = 5u;
const UV: u32 = 6u;
const GROUPS: u32 = 7u;
const BATCHES: u32 = 8u;
const BACKFACES: u32 = 9u;
const OVERDRAW: u32 = 10u;

@vertex
fn surface_vertex(v: SurfaceIn, @builtin(vertex_index) index: u32) -> SurfaceOut {
    let world = draw.model * vec4<f32>(v.position, 1.0);
    var out: SurfaceOut;
    out.clip = frame.view_proj * world;
    out.pos = world.xyz / world.w;
    out.nor = (draw.normal * vec4<f32>(v.normal, 0.0)).xyz;
    out.col = v.colour;
    out.uv = v.uv;
    let corner = index % 3u;
    out.bary = vec3<f32>(f32(corner == 0u), f32(corner == 1u), f32(corner == 2u));
    return out;
}

fn wire(b: vec3<f32>) -> f32 {
    let dx = dpdx(b);
    let dy = dpdy(b);
    let pixels = b / max(sqrt(dx * dx + dy * dy), vec3<f32>(1.0e-8));
    let near = min(min(pixels.x, pixels.y), pixels.z);
    return 1.0 - smoothstep(0.0, frame.debug.w, near);
}

fn lit(albedo: vec3<f32>, normal: vec3<f32>, pos: vec3<f32>) -> vec3<f32> {
    let key = frame.key.xyz;
    let fill = frame.fill.xyz;
    var n = normalize(normal);
    let v = normalize(frame.eye.xyz - pos);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let d = environment(n) + frame.key_radiance.rgb * max(dot(n, key), 0.0)
        + frame.fill_radiance.rgb * max(dot(n, fill), 0.0);
    let h = normalize(key + v);
    let sp = draw.gloss.x * pow(max(dot(n, h), 1.0e-6), draw.gloss.y);
    let shine = vec3<f32>(1.0) + (albedo * 1.6 - vec3<f32>(1.0)) * draw.gloss.z;
    return to_srgb((albedo * d + sp * shine * frame.key_radiance.rgb) * frame.key.w);
}

@fragment
fn surface_fragment(in: SurfaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let sampled = textureSample(tex, samp, in.uv).rgb;
    let edge = wire(in.bary);
    let facet = cross(dpdx(in.pos), dpdy(in.pos));
    if clipped(in.pos) {
        discard;
    }
    let mode = u32(frame.debug.x);
    let alpha = draw.params.x;
    var albedo = in.col;
    if draw.params.y > 0.5 {
        albedo = to_linear(sampled);
    }
    albedo = mix(albedo, draw.tint.rgb, draw.tint.a);
    switch mode {
        case WIREFRAME: {
            if edge < 0.5 {
                discard;
            }
            return vec4<f32>(to_srgb(albedo * 0.6 + vec3<f32>(0.3)), alpha);
        }
        case NORMALS: {
            return vec4<f32>(normalize(in.nor) * 0.5 + 0.5, alpha);
        }
        case DEPTH: {
            let depth = dot(in.pos - frame.eye.xyz, frame.forward.xyz);
            let t = clamp((depth - frame.debug.y) / max(frame.debug.z - frame.debug.y, 1.0e-12), 0.0, 1.0);
            return vec4<f32>(vec3<f32>(1.0 - t * 0.85), alpha);
        }
        case UV: {
            let cells = floor(in.uv * 8.0);
            let even = (i32(cells.x) + i32(cells.y)) % 2 == 0;
            let cell = vec3<f32>(fract(in.uv), 0.5) * select(0.55, 1.0, even);
            return vec4<f32>(cell, alpha);
        }
        case OVERDRAW: {
            return vec4<f32>(0.1, 0.05, 0.02, 1.0);
        }
        default: {}
    }
    let capped_mode = mode == SHADED || mode == TRIANGLES || mode == GROUPS || mode == BATCHES;
    if capped_mode && frame.viewport.z > 0.5 && draw.gloss.w > 0.5 && alpha > 0.999 && !front {
        let c = normalize(frame.clip.xyz);
        let d = environment(c) + frame.key_radiance.rgb * abs(dot(c, frame.key.xyz))
            + frame.fill_radiance.rgb * abs(dot(c, frame.fill.xyz));
        return vec4<f32>(to_srgb(to_linear(frame.cap.rgb) * d * frame.key.w), 1.0);
    }
    var normal = in.nor;
    if mode == GROUPS || mode == BATCHES {
        albedo = draw.colour.rgb;
    } else if mode == BACKFACES {
        albedo = select(vec3<f32>(0.85, 0.2, 0.15), vec3<f32>(0.2, 0.4, 0.85), front);
    } else if mode == FACETS {
        normal = facet;
    }
    var colour = lit(albedo, normal, in.pos);
    if mode == TRIANGLES {
        colour = mix(colour, vec3<f32>(0.06, 0.07, 0.08), edge * 0.85);
    }
    return vec4<f32>(colour, alpha);
}

fn environment(n: vec3<f32>) -> vec3<f32> {
    return mix(frame.ground.rgb, frame.sky.rgb, clamp(n.z * 0.5 + 0.5, 0.0, 1.0));
}

struct LineIn {
    @location(0) start: vec3<f32>,
    @location(1) end: vec3<f32>,
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) pos: vec3<f32>,
}

@vertex
fn line_vertex(@builtin(vertex_index) index: u32, l: LineIn) -> LineOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, -1.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, -1.0),
    );
    let corner = corners[index];
    let a = frame.view_proj * draw.model * vec4<f32>(l.start, 1.0);
    let b = frame.view_proj * draw.model * vec4<f32>(l.end, 1.0);
    let first = corner.x < 0.5;
    let here = select(b, a, first);
    let size = frame.viewport.xy;
    let along = (b.xy / b.w - a.xy / a.w) * size;
    let length_px = length(along);
    let dir = select(vec2<f32>(1.0, 0.0), along / length_px, length_px > 1.0e-6);
    let across = vec2<f32>(-dir.y, dir.x);
    let offset = (across * corner.y + dir * select(0.5, -0.5, first)) * draw.params.z / size;
    var out: LineOut;
    out.clip = here + vec4<f32>(offset * here.w, 0.0, 0.0);
    let world = draw.model * vec4<f32>(select(l.end, l.start, first), 1.0);
    out.pos = world.xyz / world.w;
    return out;
}

@fragment
fn line_fragment(in: LineOut) -> @location(0) vec4<f32> {
    if clipped(in.pos) {
        discard;
    }
    return vec4<f32>(draw.colour.rgb, draw.params.x);
}
