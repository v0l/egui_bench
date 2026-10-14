@group(0) @binding(0) var<uniform> present: vec4<u32>;
@group(0) @binding(1) var<storage, read> accum: array<vec4<f32>>;

fn to_srgb(raw: vec3<f32>) -> vec3<f32> {
    let c = clamp(raw, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c < vec3<f32>(0.0031308));
}

@vertex
fn present_vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let x = f32((index << 1u) & 2u);
    let y = f32(index & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

@fragment
fn present_fragment(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let sum = accum[u32(at.y) * present.x + u32(at.x)];
    return vec4<f32>(to_srgb(sum.rgb / max(sum.w, 1.0)), 1.0);
}
