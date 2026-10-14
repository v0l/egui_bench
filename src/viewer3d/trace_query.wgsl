@group(0) @binding(6) var tlas: acceleration_structure;

fn trace_ray(o: vec3<f32>, d: vec3<f32>, tmin: f32, tmax: f32, first_hit: bool, seed: ptr<function, u32>) -> Hit {
    var rq: ray_query;
    rayQueryInitialize(&rq, tlas, RayDesc(RAY_FLAG_FORCE_NO_OPAQUE, 0xFFu, tmin, tmax, o, d));
    while rayQueryProceed(&rq) {
        let c = rayQueryGetCandidateIntersection(&rq);
        if c.kind == RAY_QUERY_INTERSECTION_TRIANGLE && accept(o, d, c.t, c.instance_custom_data, seed) {
            rayQueryConfirmIntersection(&rq);
            if first_hit {
                rayQueryTerminate(&rq);
            }
        }
    }
    let h = rayQueryGetCommittedIntersection(&rq);
    var hit: Hit;
    hit.found = h.kind != RAY_QUERY_INTERSECTION_NONE;
    hit.t = h.t;
    hit.instance = h.instance_custom_data;
    hit.prim = h.primitive_index;
    hit.bary = h.barycentrics;
    return hit;
}
