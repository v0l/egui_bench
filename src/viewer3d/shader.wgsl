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
}

@vertex
fn surface_vertex(v: SurfaceIn) -> SurfaceOut {
    let world = draw.model * vec4<f32>(v.position, 1.0);
    var out: SurfaceOut;
    out.clip = frame.view_proj * world;
    out.pos = world.xyz / world.w;
    out.nor = (draw.normal * vec4<f32>(v.normal, 0.0)).xyz;
    out.col = v.colour;
    out.uv = v.uv;
    return out;
}

@fragment
fn surface_fragment(in: SurfaceOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let sampled = textureSample(tex, samp, in.uv).rgb;
    if clipped(in.pos) {
        discard;
    }
    var albedo = in.col;
    if draw.params.y > 0.5 {
        albedo = to_linear(sampled);
    }
    albedo = mix(albedo, draw.tint.rgb, draw.tint.a);
    let key = frame.key.xyz;
    let fill = frame.fill.xyz;
    let exposure = frame.key.w;
    if frame.viewport.z > 0.5 && draw.gloss.w > 0.5 && draw.params.x > 0.999 && !front {
        let c = normalize(frame.clip.xyz);
        let d = environment(c) + frame.key_radiance.rgb * abs(dot(c, key))
            + frame.fill_radiance.rgb * abs(dot(c, fill));
        return vec4<f32>(to_srgb(to_linear(frame.cap.rgb) * d * exposure), 1.0);
    }
    var n = normalize(in.nor);
    let v = normalize(frame.eye.xyz - in.pos);
    if dot(n, v) < 0.0 {
        n = -n;
    }
    let d = environment(n) + frame.key_radiance.rgb * max(dot(n, key), 0.0)
        + frame.fill_radiance.rgb * max(dot(n, fill), 0.0);
    let h = normalize(key + v);
    let sp = draw.gloss.x * pow(max(dot(n, h), 1.0e-6), draw.gloss.y);
    let shine = vec3<f32>(1.0) + (albedo * 1.6 - vec3<f32>(1.0)) * draw.gloss.z;
    let lit = (albedo * d + sp * shine * frame.key_radiance.rgb) * exposure;
    return vec4<f32>(to_srgb(lit), draw.params.x);
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
