use super::{Image, Surface, V3, cross, dot, norm, scale, sub};
use std::collections::HashMap;

impl Image {
    pub fn normal_from_height(&self, strength: f32) -> Image {
        let (w, h) = (self.width.max(1), self.height.max(1));
        let height = |x: i64, y: i64| {
            let (x, y) = (x.rem_euclid(w as i64) as usize, y.rem_euclid(h as i64) as usize);
            let at = (y * w + x) * 4;
            self.rgba.get(at).map_or(0.0, |v| *v as f32 / 255.0)
        };
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let dx = (height(x + 1, y) - height(x - 1, y)) * 0.5 * strength;
                let dy = (height(x, y + 1) - height(x, y - 1)) * 0.5 * strength;
                let n = norm([-dx, -dy, 1.0]);
                rgba.extend(n.map(|c| ((c * 0.5 + 0.5) * 255.0).round() as u8));
                rgba.push(255);
            }
        }
        Image { width: w, height: h, rgba }
    }
}

fn key(p: V3, n: V3, uv: [f32; 2]) -> [u32; 8] {
    [p[0], p[1], p[2], n[0], n[1], n[2], uv[0], uv[1]].map(f32::to_bits)
}

pub(super) fn tangents<M>(s: &Surface<M>) -> Vec<[f32; 4]> {
    let count = s.positions.len() / 3 * 3;
    if s.uvs.len() < count {
        return vec![[0.0; 4]; count];
    }
    let normal = |i: usize| s.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]);
    let mut sums: HashMap<[u32; 8], (V3, V3)> = HashMap::new();
    let mut faces = Vec::with_capacity(count / 3);
    for t in (0..count).step_by(3) {
        let (p, uv) = (&s.positions[t..t + 3], &s.uvs[t..t + 3]);
        let (e1, e2) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let (du1, dv1) = (uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]);
        let (du2, dv2) = (uv[2][0] - uv[0][0], uv[2][1] - uv[0][1]);
        let det = du1 * dv2 - du2 * dv1;
        let r = if det.abs() > 1.0e-20 { 1.0 / det } else { 0.0 };
        let tangent = scale(sub(scale(e1, dv2), scale(e2, dv1)), r);
        let bitangent = scale(sub(scale(e2, du1), scale(e1, du2)), r);
        faces.push((tangent, bitangent));
        for k in 0..3 {
            let entry = sums.entry(key(p[k], normal(t + k), uv[k])).or_insert(([0.0; 3], [0.0; 3]));
            entry.0 = [0, 1, 2].map(|c| entry.0[c] + tangent[c]);
            entry.1 = [0, 1, 2].map(|c| entry.1[c] + bitangent[c]);
        }
    }
    (0..count)
        .map(|i| {
            let n = norm(normal(i));
            let (t, b) = sums[&key(s.positions[i], normal(i), s.uvs[i])];
            let (t, b) = if dot(t, t) > 1.0e-24 { (t, b) } else { faces[i / 3] };
            let mut t = sub(t, scale(n, dot(n, t)));
            if dot(t, t) < 1.0e-24 {
                let other = if n[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
                t = cross(other, n);
            }
            let t = norm(t);
            let w = if dot(cross(n, t), b) < 0.0 { -1.0 } else { 1.0 };
            [t[0], t[1], t[2], w]
        })
        .collect()
}

pub(super) fn perturb(n: V3, tangent: [f32; 4], sample: V3, strength: f32) -> V3 {
    let t = [tangent[0], tangent[1], tangent[2]];
    if dot(t, t) < 1.0e-12 {
        return n;
    }
    let b = scale(cross(n, t), tangent[3]);
    let m = sample.map(|c| c * 2.0 - 1.0);
    let (mx, my) = (m[0] * strength, m[1] * strength);
    norm([0, 1, 2].map(|k| t[k] * mx + b[k] * my + n[k] * m[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_quad_gets_tangents_along_u_and_bitangents_along_v() {
        let s: Surface = Surface {
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            normals: vec![[0.0, 0.0, 1.0]; 6],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            ..Default::default()
        };
        for t in tangents(&s) {
            assert!((t[0] - 1.0).abs() < 1.0e-5 && t[1].abs() < 1.0e-5, "{t:?}");
            assert_eq!(t[3], 1.0);
        }
        let flipped: Surface =
            Surface { uvs: s.uvs.iter().map(|uv| [uv[0], 1.0 - uv[1]]).collect(), ..s };
        assert!(tangents(&flipped).iter().all(|t| t[3] == -1.0));
    }

    #[test]
    fn a_ramp_tilts_the_normal_against_the_slope() {
        let rgba = (0..8 * 8).flat_map(|i| [(i % 8) as u8 * 30, 0, 0, 255]).collect();
        let normals = Image { width: 8, height: 8, rgba }.normal_from_height(4.0);
        let at = (3 * 8 + 3) * 4;
        assert!(
            normals.rgba[at] < 100 && normals.rgba[at + 2] > 128,
            "{:?}",
            &normals.rgba[at..at + 3]
        );
        let flat = [128.0 / 255.0, 128.0 / 255.0, 1.0];
        let n = perturb([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], flat, 1.0);
        assert!(n[2] > 0.9999);
        let n = perturb([0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0], [1.0, 0.5, 0.5], 1.0);
        assert!(n[0] > 0.99);
    }
}
