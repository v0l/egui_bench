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
    env: vec4<f32>,
    env_size: vec4<u32>,
}

struct Instance {
    world: mat4x4<f32>,
    inverse: mat4x4<f32>,
    normal: mat4x4<f32>,
    tint: vec4<f32>,
    surface: vec4<f32>,
    refs: vec4<u32>,
    light: vec4<f32>,
    glass: vec4<f32>,
    maps: vec4<u32>,
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
const NORMAL_MAPPED: u32 = 8u;
const METAL_ROUGH: u32 = 16u;
const STRIDE: u32 = 16u;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> accum: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> vertices: array<f32>;
@group(0) @binding(3) var<storage, read> instances: array<Instance>;
@group(0) @binding(4) var<storage, read> texels: array<u32>;
@group(0) @binding(5) var<storage, read> pictures: array<vec4<u32>>;
@group(0) @binding(8) var<storage, read> sky: array<f32>;

var<private> visits: u32;
var<private> filtered: vec3<f32>;

const GLASS_F0: f32 = 0.04;
const GLASS_PASSES: u32 = 16u;

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

fn accept(o: vec3<f32>, d: vec3<f32>, t: f32, index: u32, prim: u32, shadow: bool) -> bool {
    if cutting() && dot(o + d * t, params.clip.xyz) > params.clip.w {
        return false;
    }
    let inst = instances[index];
    let transmission = inst.glass.x;
    if !shadow || (inst.surface.w >= 1.0 && transmission <= 0.0) {
        return true;
    }
    let albedo = fetch(inst.refs.x + prim * 3u, 6u);
    if inst.surface.w < 1.0 {
        filtered *= (1.0 - GLASS_F0) * glass_tint(albedo, inst.surface.w);
    }
    if transmission > 0.0 {
        filtered *= (1.0 - GLASS_F0) * transmission * (1.0 - inst.surface.z) * albedo;
    }
    return false;
}

fn glass_tint(albedo: vec3<f32>, opacity: f32) -> vec3<f32> {
    return mix(vec3<f32>(1.0), albedo, opacity);
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

fn tangent(index: u32) -> vec4<f32> {
    let at = index * STRIDE + 12u;
    return vec4<f32>(vertices[at], vertices[at + 1u], vertices[at + 2u], vertices[at + 3u]);
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
    let p = at * size - 0.5;
    let base = floor(p);
    let f = p - base;
    let wrap = vec2<i32>(i32(picture.y), i32(picture.z));
    let x0 = u32(((i32(base.x) % wrap.x) + wrap.x) % wrap.x);
    let y0 = u32(((i32(base.y) % wrap.y) + wrap.y) % wrap.y);
    let x1 = (x0 + 1u) % picture.y;
    let y1 = (y0 + 1u) % picture.z;
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
    opacity: f32,
    emission: vec3<f32>,
    nb: vec3<f32>,
    coat: f32,
    coat_alpha: f32,
    transmission: f32,
    ior: f32,
    entering: bool,
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
        s.opacity = 1.0;
        s.emission = vec3<f32>(0.0);
        s.nb = s.n;
        s.coat = 0.0;
        s.coat_alpha = 1.0;
        s.transmission = 0.0;
        s.ior = 1.5;
        s.entering = true;
        return s;
    }
    let shading = fetch(base, 3u) * w.x + fetch(base + 1u, 3u) * w.y
        + fetch(base + 2u, 3u) * w.z;
    var n = (inst.normal * vec4<f32>(shading, 0.0)).xyz;
    n = select(ng, normalize(n), dot(n, n) > 1.0e-12);
    let at = uv(base) * w.x + uv(base + 1u) * w.y + uv(base + 2u) * w.z;
    var nb = n;
    if (inst.refs.w & NORMAL_MAPPED) != 0u {
        let tw = tangent(base) * w.x + tangent(base + 1u) * w.y + tangent(base + 2u) * w.z;
        let tx = (inst.world * vec4<f32>(tw.xyz, 0.0)).xyz;
        if dot(tx, tx) > 1.0e-12 {
            let t = normalize(tx - n * dot(n, tx));
            let b = cross(n, t) * select(-1.0, 1.0, tw.w >= 0.0);
            let m = sample_picture(inst.maps.x, at) * 2.0 - 1.0;
            let k = inst.light.w;
            n = normalize(t * m.x * k + b * m.y * k + n * m.z);
        }
    }
    if dot(nb, d) > 0.0 {
        nb = -nb;
    }
    if dot(n, d) > 0.0 {
        n = -n;
    }
    var albedo = fetch(base, 6u) * w.x + fetch(base + 1u, 6u) * w.y
        + fetch(base + 2u, 6u) * w.z;
    if (inst.refs.w & TEXTURED) != 0u {
        albedo = to_linear(sample_picture(inst.refs.y, at));
    }
    albedo = mix(albedo, inst.tint.rgb, inst.tint.a);
    var metal = inst.surface.z;
    var rough = inst.surface.y;
    if (inst.refs.w & METAL_ROUGH) != 0u {
        let mr = sample_picture(inst.maps.y, at);
        rough *= mr.g;
        metal *= mr.b;
    }
    rough = max(rough, 0.02);
    s.n = n;
    s.ng = ng;
    s.albedo = albedo;
    s.f0 = mix(vec3<f32>(inst.surface.x), albedo, metal);
    s.diffuse = 1.0 - metal;
    s.alpha = rough * rough;
    s.opacity = inst.surface.w;
    s.emission = inst.light.rgb;
    s.nb = nb;
    s.coat = inst.glass.z;
    s.coat_alpha = max(inst.glass.w * inst.glass.w, 4.0e-4);
    s.transmission = inst.glass.x * (1.0 - metal);
    s.ior = max(inst.glass.y, 1.0001);
    s.entering = !back;
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
    let kd = s.diffuse * (1.0 - s.transmission) * (vec3<f32>(1.0) - fresnel(s.f0, nv));
    let base = s.albedo * kd / PI + spec * (1.0 - s.transmission);
    if s.coat <= 0.0 {
        return base;
    }
    let bl = max(dot(s.nb, l), 0.0);
    let bv = max(dot(s.nb, v), 1.0e-4);
    let bh = max(dot(s.nb, h), 0.0);
    let ca2 = s.coat_alpha * s.coat_alpha;
    let cq = bh * bh * (ca2 - 1.0) + 1.0;
    let cf = coat_fresnel(dot(v, h));
    let coat = s.coat * cf * ca2 / (PI * cq * cq) * smith(s.coat_alpha, bv) * smith(s.coat_alpha, bl)
        / (4.0 * bv * max(bl, 1.0e-4));
    return base * (1.0 - s.coat * coat_fresnel(bv)) + vec3<f32>(coat * select(0.0, 1.0, bl > 0.0));
}

fn coat_fresnel(c: f32) -> f32 {
    return GLASS_F0 + (1.0 - GLASS_F0) * pow(1.0 - clamp(c, 0.0, 1.0), 5.0);
}

fn ggx_pdf(n: vec3<f32>, alpha: f32, v: vec3<f32>, l: vec3<f32>) -> f32 {
    let h = normalize(v + l);
    let nh = max(dot(n, h), 0.0);
    let vh = max(dot(v, h), 1.0e-4);
    let a2 = alpha * alpha;
    let q = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * q * q) * nh / (4.0 * vh);
}

fn ggx_half(n: vec3<f32>, alpha: f32, seed: ptr<function, u32>) -> vec3<f32> {
    let u1 = rand(seed);
    let u2 = rand(seed);
    let a2 = alpha * alpha;
    let ct = sqrt((1.0 - u2) / (1.0 + (a2 - 1.0) * u2));
    let st = sqrt(max(0.0, 1.0 - ct * ct));
    let phi = 2.0 * PI * u1;
    return basis(n) * vec3<f32>(st * cos(phi), st * sin(phi), ct);
}

fn lobes(s: Surface, v: vec3<f32>) -> vec4<f32> {
    let nv = max(dot(s.n, v), 1.0e-4);
    let coat = s.coat * coat_fresnel(max(dot(s.nb, v), 1.0e-4));
    let under = 1.0 - coat;
    let spec = luminance(fresnel(s.f0, nv)) * (1.0 - s.transmission) * under;
    let body = luminance(s.albedo) * s.diffuse * (1.0 - s.transmission) * under;
    let glass = s.transmission * under;
    let total = coat + spec + body + glass;
    if total <= 0.0 {
        return vec4<f32>(0.0);
    }
    var p = vec4<f32>(coat, spec, body, glass) / total;
    if spec > 0.0 && body > 0.0 {
        let both = p.y + p.z;
        let sp = clamp(p.y / both, 0.1, 0.9) * both;
        p = vec4<f32>(p.x, sp, both - sp, p.w);
    }
    return p;
}

fn reflection_pdf(s: Surface, p: vec4<f32>, v: vec3<f32>, l: vec3<f32>) -> f32 {
    var pdf = 0.0;
    if p.x > 0.0 {
        pdf += p.x * ggx_pdf(s.nb, s.coat_alpha, v, l);
    }
    if p.y > 0.0 {
        pdf += p.y * ggx_pdf(s.n, s.alpha, v, l);
    }
    if p.z > 0.0 {
        pdf += p.z * max(dot(s.n, l), 0.0) / PI;
    }
    return pdf;
}

fn dielectric(cos_i: f32, eta: f32) -> f32 {
    let sin_t2 = eta * eta * (1.0 - cos_i * cos_i);
    if sin_t2 >= 1.0 {
        return 1.0;
    }
    let cos_t = sqrt(1.0 - sin_t2);
    let rs = (eta * cos_i - cos_t) / (eta * cos_i + cos_t);
    let rp = (cos_i - eta * cos_t) / (cos_i + eta * cos_t);
    return 0.5 * (rs * rs + rp * rp);
}

fn has_sky() -> bool {
    return params.env.w > 0.5;
}

fn sky_texel(d: vec3<f32>) -> vec2<u32> {
    let w = params.env_size.x;
    let h = params.env_size.y;
    let phi = atan2(d.y, d.x) - params.env.x;
    let u = phi / (2.0 * PI) - floor(phi / (2.0 * PI));
    let v = acos(clamp(d.z, -1.0, 1.0)) / PI;
    return vec2<u32>(min(u32(u * f32(w)), w - 1u), min(u32(v * f32(h)), h - 1u));
}

fn sky_radiance(d: vec3<f32>) -> vec3<f32> {
    let at = sky_texel(d);
    let i = (at.y * params.env_size.x + at.x) * 3u;
    return vec3<f32>(sky[i], sky[i + 1u], sky[i + 2u]) * params.env.y;
}

fn sky_probability(at: vec2<u32>) -> f32 {
    let w = params.env_size.x;
    let h = params.env_size.y;
    let marginal = 3u * w * h;
    let conditional = marginal + h;
    let row = sky[marginal + at.y] - select(0.0, sky[marginal + at.y - 1u], at.y > 0u);
    let ci = conditional + at.y * w + at.x;
    let col = sky[ci] - select(0.0, sky[ci - 1u], at.x > 0u);
    return row * col;
}

fn sky_pdf(d: vec3<f32>) -> f32 {
    let at = sky_texel(d);
    let st = sqrt(max(1.0 - d.z * d.z, 1.0e-8));
    let texels = f32(params.env_size.x * params.env_size.y);
    return sky_probability(at) * texels / (2.0 * PI * PI * st);
}

fn search(start: u32, count: u32, u: f32) -> u32 {
    var lo = 0u;
    var hi = count - 1u;
    while lo < hi {
        let mid = (lo + hi) / 2u;
        if sky[start + mid] < u {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    return lo;
}

fn sky_sample(seed: ptr<function, u32>) -> vec4<f32> {
    let w = params.env_size.x;
    let h = params.env_size.y;
    let marginal = 3u * w * h;
    let y = search(marginal, h, rand(seed));
    let x = search(marginal + h + y * w, w, rand(seed));
    let u = (f32(x) + rand(seed)) / f32(w);
    let v = (f32(y) + rand(seed)) / f32(h);
    let phi = u * 2.0 * PI + params.env.x;
    let theta = v * PI;
    let st = sin(theta);
    let d = vec3<f32>(st * cos(phi), st * sin(phi), cos(theta));
    let pdf = sky_probability(vec2<u32>(x, y)) * f32(w * h) / (2.0 * PI * PI * max(st, 1.0e-4));
    return vec4<f32>(d, pdf);
}

fn direct_sky(s: Surface, v: vec3<f32>, eps: f32, seed: ptr<function, u32>) -> vec3<f32> {
    if !has_sky() {
        return vec3<f32>(0.0);
    }
    let smp = sky_sample(seed);
    let l = smp.xyz;
    let nl = dot(s.n, l);
    if smp.w <= 0.0 || nl <= 0.0 || dot(s.ng, l) <= 0.0 {
        return vec3<f32>(0.0);
    }
    let shadow = trace_ray(s.pos + s.ng * eps, l, eps, 1.0e30, true, seed);
    if shadow.found {
        return vec3<f32>(0.0);
    }
    let bp = reflection_pdf(s, lobes(s, v), v, l);
    let weight = smp.w * smp.w / (smp.w * smp.w + bp * bp);
    return brdf(s, v, l) * sky_radiance(l) * filtered * nl / smp.w * weight;
}

fn backdrop(d: vec3<f32>) -> vec3<f32> {
    if has_sky() && params.env.z > 0.5 {
        return min(sky_radiance(d) * params.background.w, vec3<f32>(1.0));
    }
    return params.background.rgb;
}

fn surroundings(d: vec3<f32>, pdf: f32) -> vec3<f32> {
    if !has_sky() {
        return environment(d);
    }
    if pdf <= 0.0 {
        return sky_radiance(d);
    }
    let sp = sky_pdf(d);
    return sky_radiance(d) * pdf * pdf / (pdf * pdf + sp * sp);
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
    return brdf(s, v, l) * radiance.rgb * filtered * PI * nl;
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
    var depth = 0u;
    var passes = 0u;
    var straight = true;
    var last_pdf = 0.0;
    loop {
        let hit = trace_ray(o, d, tmin, 1.0e30, false, seed);
        if !hit.found {
            if straight {
                return min(sum * exposure, vec3<f32>(1.0)) + through * backdrop(d);
            }
            sum += through * surroundings(d, last_pdf);
            break;
        }
        var s = surface_at(hit, o, d);
        let v = -d;
        sum += through * s.emission;
        if s.opacity < 1.0 {
            let tint = glass_tint(s.albedo, s.opacity);
            s.albedo = tint;
            s.f0 = vec3<f32>(GLASS_F0);
            s.diffuse = 0.0;
            s.alpha = min(s.alpha, 0.01);
            sum += through * direct(s, v, params.key, params.key_radiance, eps, seed);
            sum += through * direct(s, v, params.fill, params.fill_radiance, eps, seed);
            let reflected = fresnel(s.f0, max(dot(s.n, v), 0.0)).x;
            if passes < GLASS_PASSES && rand(seed) >= reflected {
                through *= tint;
                o = s.pos - s.ng * eps;
                tmin = eps;
                passes += 1u;
                continue;
            }
            if depth == bounces {
                break;
            }
            o = s.pos + s.ng * eps;
            d = reflect(d, s.n);
            tmin = eps;
            depth += 1u;
            straight = false;
            last_pdf = 0.0;
            continue;
        }
        sum += through * direct(s, v, params.key, params.key_radiance, eps, seed);
        sum += through * direct(s, v, params.fill, params.fill_radiance, eps, seed);
        sum += through * direct_sky(s, v, eps, seed);
        let p = lobes(s, v);
        if p.x + p.y + p.z + p.w <= 0.0 {
            break;
        }
        let pick = rand(seed);
        var l: vec3<f32>;
        var side = 1.0;
        if pick < p.w {
            let coat = s.coat * coat_fresnel(max(dot(s.nb, v), 1.0e-4));
            let h = ggx_half(s.n, s.alpha, seed);
            let eta = select(s.ior, 1.0 / s.ior, s.entering);
            let cos_i = abs(dot(v, h));
            let reflected = dielectric(cos_i, eta);
            let bent = refract(d, h, eta);
            if rand(seed) < reflected || dot(bent, bent) < 1.0e-12 {
                if depth == bounces {
                    break;
                }
                l = reflect(d, h);
                straight = false;
            } else {
                l = normalize(bent);
                side = -1.0;
                through *= s.albedo;
            }
            let nl = abs(dot(s.n, l));
            let nv = max(dot(s.n, v), 1.0e-4);
            let nh = max(dot(s.n, h), 1.0e-4);
            through *= s.transmission * (1.0 - coat) * smith(s.alpha, nv) * smith(s.alpha, nl)
                * cos_i / (nv * nh) / p.w;
            last_pdf = 0.0;
            if dot(s.ng, l) * side <= 0.0 {
                break;
            }
            if side < 0.0 {
                passes += 1u;
                if passes > GLASS_PASSES {
                    break;
                }
                o = s.pos - s.ng * eps;
                d = l;
                tmin = eps;
                continue;
            }
        } else {
            if depth == bounces {
                break;
            }
            if pick < p.w + p.x {
                l = reflect(d, ggx_half(s.nb, s.coat_alpha, seed));
            } else if pick < p.w + p.x + p.y {
                l = reflect(d, ggx_half(s.n, s.alpha, seed));
            } else {
                let u1 = rand(seed);
                let u2 = rand(seed);
                let r = sqrt(u1);
                let phi = 2.0 * PI * u2;
                l = basis(s.n) * vec3<f32>(r * cos(phi), r * sin(phi), sqrt(max(0.0, 1.0 - u1)));
            }
            let nl = dot(s.n, l);
            let pdf = reflection_pdf(s, p, v, l);
            if nl <= 0.0 || pdf <= 0.0 || dot(s.ng, l) <= 0.0 {
                break;
            }
            through *= brdf(s, v, l) * nl / pdf;
            last_pdf = pdf;
            straight = false;
        }
        if depth >= 2u {
            let q = clamp(max(through.r, max(through.g, through.b)), 0.05, 1.0);
            if rand(seed) > q {
                break;
            }
            through /= q;
        }
        o = s.pos + s.ng * eps * side;
        d = l;
        tmin = eps;
        depth += 1u;
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
