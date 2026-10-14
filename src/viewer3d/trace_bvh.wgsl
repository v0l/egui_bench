struct Node {
    lo: vec3<f32>,
    a: u32,
    hi: vec3<f32>,
    count: u32,
}

@group(0) @binding(6) var<storage, read> nodes: array<Node>;
@group(0) @binding(7) var<storage, read> prims: array<u32>;

fn entry(node: Node, o: vec3<f32>, inv: vec3<f32>, tmin: f32, tmax: f32) -> f32 {
    let a = (node.lo - o) * inv;
    let b = (node.hi - o) * inv;
    let near = max(max(min(a.x, b.x), min(a.y, b.y)), max(min(a.z, b.z), tmin));
    let far = min(min(max(a.x, b.x), max(a.y, b.y)), min(max(a.z, b.z), tmax));
    return select(1.0e30, near, near <= far);
}

fn trace_ray(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32, first_hit: bool, seed: ptr<function, u32>) -> Hit {
    var best: Hit;
    best.found = false;
    best.t = tmax;
    var stack: array<u32, 40>;
    for (var i = 0u; i < params.counts.w; i++) {
        let inst = instances[i];
        if (inst.refs.w & VISIBLE) == 0u {
            continue;
        }
        let lo = (inst.inverse * vec4<f32>(o, 1.0)).xyz;
        let ld = (inst.inverse * vec4<f32>(d, 0.0)).xyz;
        let inv = 1.0 / select(ld, vec3<f32>(1.0e-30), abs(ld) < vec3<f32>(1.0e-30));
        var top = 0u;
        var current = inst.refs.z;
        if entry(nodes[current], lo, inv, tmin, best.t) >= 1.0e30 {
            continue;
        }
        loop {
            let node = nodes[current];
            if node.count == 0u {
                let left = nodes[node.a];
                let right = nodes[node.a + 1u];
                let tl = entry(left, lo, inv, tmin, best.t);
                let tr = entry(right, lo, inv, tmin, best.t);
                if tl < 1.0e30 || tr < 1.0e30 {
                    let near_left = tl <= tr;
                    current = select(node.a + 1u, node.a, near_left);
                    if max(tl, tr) < 1.0e30 && top < 40u {
                        stack[top] = select(node.a, node.a + 1u, near_left);
                        top += 1u;
                    }
                    continue;
                }
            } else {
                for (var k = 0u; k < node.count; k++) {
                    let prim = prims[node.a + k];
                    let base = inst.refs.x + prim * 3u;
                    let a = position(base);
                    let e1 = position(base + 1u) - a;
                    let e2 = position(base + 2u) - a;
                    let p = cross(ld, e2);
                    let det = dot(e1, p);
                    if abs(det) < 1.0e-20 {
                        continue;
                    }
                    let inv_det = 1.0 / det;
                    let s = lo - a;
                    let u = dot(s, p) * inv_det;
                    let q = cross(s, e1);
                    let v = dot(ld, q) * inv_det;
                    let t = dot(e2, q) * inv_det;
                    if u < 0.0 || v < 0.0 || u + v > 1.0 || t <= tmin || t >= best.t {
                        continue;
                    }
                    if !accept(o, d, t, i, seed) {
                        continue;
                    }
                    best.found = true;
                    best.t = t;
                    best.instance = i;
                    best.prim = prim;
                    best.bary = vec2<f32>(u, v);
                    if first_hit {
                        return best;
                    }
                }
            }
            var next = false;
            while top > 0u {
                top -= 1u;
                let candidate = stack[top];
                if entry(nodes[candidate], lo, inv, tmin, best.t) < 1.0e30 {
                    current = candidate;
                    next = true;
                    break;
                }
            }
            if !next {
                break;
            }
        }
    }
    return best;
}
