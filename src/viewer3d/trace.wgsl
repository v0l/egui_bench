struct Params {
    eye: vec4<f32>,
    right: vec4<f32>,
    up: vec4<f32>,
    forward: vec4<f32>,
    key: vec4<f32>,
    key_radiance: vec4<f32>,
    fill: vec4<f32>,
    fill_radiance: vec4<f32>,
    sky: vec4<f32>,
    ground: vec4<f32>,
    background: vec4<f32>,
    cap: vec4<f32>,
    clip: vec4<f32>,
    counts: vec4<u32>,
    size: vec4<u32>,
}

struct Instance {
    world: mat4x4<f32>,
    inverse: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    surface: vec4<f32>,
    refs: vec4<u32>,
}

struct Hit {
    found: bool,
    t: f32,
    instance: u32,
    prim: u32,
    bary: vec2<f32>,
}

const PI: f32 = 3.14159265;
const CAPPED: u32 = 1u;
const VISIBLE: u32 = 2u;
const TEXTURED: u32 = 4u;
const STRIDE: u32 = 12u;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> vertices: array<f32>;
@group(0) @binding(3) var<storage, read> instances: array<Instance>;
@group(0) @binding(4) var<storage, read> texels: array<u32>;
@group(0) @binding(5) var<storage, read> pictures: array<vec4<u32>>;

var<private> visits: u32;

fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rand(seed: ptr<function, u32>) -> f32 {
    *seed = pcg(*seed);
    return f32(*seed >> 8u) / 16777216.0;
}

fn cutting() -> bool {
    return params.cap.w > 0.5;
}

fn accept(o: vec3<f32>, d: vec3<f32>, t: f32, index: u32, seed: ptr<function, u32>) -> bool {
    if cutting() && dot(o + d * t, params.clip.xyz) > params.clip.w {
        return false;
    }
    let opacity = instances[index].surface.w;
    return opacity >= 1.0 || rand(seed) < opacity;
}

fn position(index: u32) -> vec3<f32> {
    let at = index * STRIDE;
    return vec3<f32>(vertices[at], vertices[at + 1u], vertices[at + 2u]);
}

fn fetch(index: u32, offset: u32) -> vec3<f32> {
    let at = index * STRIDE + offset;
    return vec3<f32>(vertices[at], vertices[at + 1u], vertices[at + 2u]);
}

fn uv(index: u32) -> vec2<f32> {
    let at = index * STRIDE + 9u;
    return vec2<f32>(vertices[at], vertices[at + 1u]);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c < vec3<f32>(0.04045));
}

fn texel(picture: vec4<u32>, x: u32, y: u32) -> vec3<f32> {
    let packed = texels[picture.x + y * picture.y + x];
    return vec3<f32>(
        f32(packed & 255u),
        f32((packed >> 8u) & 255u),
        f32((packed >> 16u) & 255u),
    ) / 255.0;
}

fn sample_picture(index: u32, at: vec2<f32>) -> vec3<f32> {
    let picture = pictures[index];
    let size = vec2<f32>(f32(picture.y), f32(picture.z));
    let p = clamp(at * size - 0.5, vec2<f32>(0.0), size - 1.0);
    let x0 = u32(p.x);
    let y0 = u32(p.y);
    let x1 = min(x0 + 1u, picture.y - 1u);
    let y1 = min(y0 + 1u, picture.z - 1u);
    let f = p - vec2<f32>(f32(x0), f32(y0));
    let top = mix(texel(picture, x0, y0), texel(picture, x1, y0), f.x);
    let bottom = mix(texel(picture, x0, y1), texel(picture, x1, y1), f.x);
    return mix(top, bottom, f.y);
}

struct Surface {
    pos: vec3<f32>,
    n: vec3<f32>,
    ng: vec3<f32>,
    albedo: vec3<f32>,
    f0: vec3<f32>,
    diffuse: f32,
    alpha: f32,
}

fn surface_at(hit: Hit, o: vec3<f32>, d: vec3<f32>) -> Surface {
    let inst = instances[hit.instance];
    let base = inst.refs.x + hit.prim * 3u;
    let w = vec3<f32>(1.0 - hit.bary.x - hit.bary.y, hit.bary.x, hit.bary.y);
    let a = (inst.world * vec4<f32>(position(base), 1.0)).xyz;
    let b = (inst.world * vec4<f32>(position(base + 1u), 1.0)).xyz;
    let c = (inst.world * vec4<f32>(position(base + 2u), 1.0)).xyz;
    var ng = normalize(cross(b - a, c - a));
    let back = dot(ng, d) > 0.0;
    var s: Surface;
    s.pos = o + d * hit.t;
    if back {
        ng = -ng;
    }
    if back && cutting() && (inst.refs.w & CAPPED) != 0u && inst.surface.w >= 1.0 {
        let cn = normalize(params.clip.xyz);
        let along = dot(d, cn);
        if abs(along) > 1.0e-6 {
            let t = (params.clip.w - dot(o, cn)) / along;
            if t > 0.0 && t < hit.t {
                s.pos = o + d * t;
            }
        }
        s.ng = select(cn, -cn, dot(cn, d) > 0.0);
        s.n = s.ng;
        s.albedo = params.cap.rgb;
        s.f0 = vec3<f32>(0.0);
        s.diffuse = 1.0;
        s.alpha = 1.0;
        return s;
    }
    let shading = fetch(base, 3u) * w.x + fetch(base + 1u, 3u) * w.y
        + fetch(base + 2u, 3u) * w.z;
    var n = (inst.normal * vec4<f32>(shading, 0.0)).xyz;
    n = select(ng, normalize(n), dot(n, n) > 1.0e-12);
    if dot(n, d) > 0.0 {
        n = -n;
    }
    var albedo = fetch(base, 6u) * w.x + fetch(base + 1u, 6u) * w.y
        + fetch(base + 2u, 6u) * w.z;
    if (inst.refs.w & TEXTURED) != 0u {
        let at = uv(base) * w.x + uv(base + 1u) * w.y + uv(base + 2u) * w.z;
        albedo = to_linear(sample_picture(inst.refs.y, at));
    }
    albedo = mix(albedo, inst.tint.rgb, inst.tint.a);
    let metal = inst.surface.z;
    s.n = n;
    s.ng = ng;
    s.albedo = albedo;
    s.f0 = mix(vec3<f32>(inst.surface.x), albedo, metal);
    s.diffuse = 1.0 - metal;
    s.alpha = inst.surface.y * inst.surface.y;
    return s;
}

fn basis(n: vec3<f32>) -> mat3x3<f32> {
    let sign = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (sign + n.z);
    let b = n.x * n.y * a;
    let t = vec3<f32>(1.0 + sign * n.x * n.x * a, sign * b, -sign * n.x);
    let u = vec3<f32>(b, sign + n.y * n.y * a, -n.y);
    return mat3x3<f32>(t, u, n);
}

fn cone(dir: vec3<f32>, spread: f32, seed: ptr<function, u32>) -> vec3<f32> {
    if spread <= 0.0 {
        return dir;
    }
    let cos_max = cos(spread);
    let z = 1.0 - rand(seed) * (1.0 - cos_max);
    let r = sqrt(max(0.0, 1.0 - z * z));
    let phi = 2.0 * PI * rand(seed);
    return basis(dir) * vec3<f32>(r * cos(phi), r * sin(phi), z);
}

fn fresnel(f0: vec3<f32>, c: f32) -> vec3<f32> {
    if max(f0.r, max(f0.g, f0.b)) <= 0.0 {
        return vec3<f32>(0.0);
    }
    return f0 + (vec3<f32>(1.0) - f0) * pow(1.0 - clamp(c, 0.0, 1.0), 5.0);
}

fn smith(alpha: f32, c: f32) -> f32 {
    let a2 = alpha * alpha;
    return 2.0 * c / (c + sqrt(a2 + (1.0 - a2) * c * c));
}

fn brdf(s: Surface, v: vec3<f32>, l: vec3<f32>) -> vec3<f32> {
    let nl = dot(s.n, l);
    let nv = max(dot(s.n, v), 1.0e-4);
    if nl <= 0.0 {
        return vec3<f32>(0.0);
    }
    let h = normalize(v + l);
    let nh = max(dot(s.n, h), 0.0);
    let a2 = s.alpha * s.alpha;
    let q = nh * nh * (a2 - 1.0) + 1.0;
    let distribution = a2 / (PI * q * q);
    let f = fresnel(s.f0, dot(v, h));
    let spec = f * distribution * smith(s.alpha, nv) * smith(s.alpha, nl) / (4.0 * nv * nl);
    let kd = s.diffuse * (vec3<f32>(1.0) - fresnel(s.f0, nv));
    return s.albedo * kd / PI + spec;
}

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn environment(d: vec3<f32>) -> vec3<f32> {
    return select(params.ground.rgb, params.sky.rgb, d.z >= 0.0);
}

fn direct(s: Surface, v: vec3<f32>, light: vec4<f32>, radiance: vec4<f32>, eps: f32, seed: ptr<function, u32>) -> vec3<f32> {
    if max(radiance.r, max(radiance.g, radiance.b)) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let l = cone(light.xyz, radiance.w, seed);
    let nl = dot(s.n, l);
    if nl <= 0.0 || dot(s.ng, l) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let shadow = trace_ray(s.pos + s.ng * eps, l, eps, 1.0e30, true, seed);
    if shadow.found {
        return vec3<f32>(0.0);
    }
    return brdf(s, v, l) * radiance.rgb * PI * nl;
}

fn camera_ray(px: vec2<f32>) -> array<vec3<f32>, 2> {
    let focal = params.right.w;
    let dx = (px.x - f32(params.size.x) * 0.5) / focal;
    let dy = (px.y - f32(params.size.y) * 0.5) / focal;
    let dist = params.eye.w;
    if dist > 0.0 {
        let o = params.eye.xyz + params.right.xyz * dx * dist - params.up.xyz * dy * dist
            - params.forward.xyz * dist * 4.0;
        return array<vec3<f32>, 2>(o, params.forward.xyz);
    }
    let d = normalize(params.forward.xyz + params.right.xyz * dx - params.up.xyz * dy);
    return array<vec3<f32>, 2>(params.eye.xyz, d);
}

fn path(px: vec2<f32>, seed: ptr<function, u32>) -> vec3<f32> {
    let ray = camera_ray(px);
    var o = ray[0];
    var d = ray[1];
    let eps = params.forward.w;
    let exposure = params.background.w;
    var through = vec3<f32>(1.0);
    var sum = vec3<f32>(0.0);
    var tmin = 0.0;
    let bounces = params.counts.z;
    for (var depth = 0u; depth <= bounces; depth++) {
        let hit = trace_ray(o, d, tmin, 1.0e30, false, seed);
        if !hit.found {
            if depth == 0u {
                return params.background.rgb;
            }
            sum += through * environment(d);
            break;
        }
        let s = surface_at(hit, o, d);
        let v = -d;
        sum += through * direct(s, v, params.key, params.key_radiance, eps, seed);
        sum += through * direct(s, v, params.fill, params.fill_radiance, eps, seed);
        if depth == bounces {
            break;
        }
        let nv = max(dot(s.n, v), 1.0e-4);
        let spec = luminance(fresnel(s.f0, nv));
        let body = luminance(s.albedo) * s.diffuse;
        let p_spec = select(0.0, clamp(spec / max(spec + body, 1.0e-6), 0.1, 0.9), spec > 0.0);
        var l: vec3<f32>;
        if rand(seed) < p_spec {
            let u1 = rand(seed);
            let u2 = rand(seed);
            let a2 = s.alpha * s.alpha;
            let ct = sqrt((1.0 - u2) / (1.0 + (a2 - 1.0) * u2));
            let st = sqrt(max(0.0, 1.0 - ct * ct));
            let phi = 2.0 * PI * u1;
            let h = basis(s.n) * vec3<f32>(st * cos(phi), st * sin(phi), ct);
            l = reflect(d, h);
            let nl = dot(s.n, l);
            let vh = max(dot(v, h), 1.0e-4);
            let nh = max(dot(s.n, h), 1.0e-4);
            if nl <= 0.0 {
                break;
            }
            through *= fresnel(s.f0, vh) * smith(s.alpha, nv) * smith(s.alpha, nl) * vh / (nv * nh) / p_spec;
        } else {
            let u1 = rand(seed);
            let u2 = rand(seed);
            let r = sqrt(u1);
            let phi = 2.0 * PI * u2;
            l = basis(s.n) * vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(0.0, 1.0 - u1)));
            through *= s.albedo * s.diffuse * (vec3<f32>(1.0) - fresnel(s.f0, nv)) / (1.0 - p_spec);
        }
        if dot(s.ng, l) <= 0.0 {
            break;
        }
        if depth >= 2u {
            let q = clamp(max(through.r, max(through.g, through.b)), 0.05, 1.0);
            if rand(seed) > q {
                break;
            }
            through /= q;
        }
        o = s.pos + s.ng * eps;
        d = l;
        tmin = eps;
    }
    return min(sum * exposure, vec3<f32>(1.0));
}

fn heat(px: vec2<f32>, seed: ptr<function, u32>) -> vec3<f32> {
    let ray = camera_ray(px);
    visits = 0u;
    let hit = trace_ray(ray[0], ray[1], 0.0, 1.0e30, false, seed);
    if visits == 0u && !hit.found {
        return params.background.rgb;
    }
    let t = clamp(log2(1.0 + f32(visits)) / 8.0, 0.0, 1.0);
    let cold = vec3<f32>(0.02, 0.05, 0.35);
    let mid = vec3<f32>(0.05, 0.6, 0.55);
    let warm = vec3<f32>(0.95, 0.75, 0.05);
    let hot = vec3<f32>(0.9, 0.08, 0.05);
    if t < 0.33 {
        return mix(cold, mid, t / 0.33);
    }
    if t < 0.66 {
        return mix(mid, warm, (t - 0.33) / 0.33);
    }
    return mix(warm, hot, (t - 0.66) / 0.34);
}

@compute @workgroup_size(8, 8)
fn trace_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = params.size.xy;
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let index = id.y * size.x + id.x;
    let base = params.counts.x;
    let count = params.counts.y;
    var total = vec3<f32>(0.0);
    for (var k = 0u; k < count; k++) {
        var seed = pcg(index ^ pcg(base + k + params.size.z * 7919u));
        let jitter = vec2<f32>(rand(&seed), rand(&seed));
        if params.size.w == 1u {
            total += heat(vec2<f32>(id.xy) + jitter, &seed);
        } else {
            total += path(vec2<f32>(id.xy) + jitter, &seed);
        }
    }
    var previous = vec4<f32>(0.0);
    if base > 0u {
        previous = accum[index];
    }
    accum[index] = previous + vec4<f32>(total, f32(count));
}
