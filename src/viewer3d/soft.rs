use super::{
    Image, Material, Shading, Style, V3, View, Viewer, add, dot, norm, rgb, scale, sub, to_linear,
};
use egui::{Color32, ColorImage, Vec2};

const SUPERSAMPLE: usize = 2;

struct Lights {
    key: V3,
    fill: V3,
    key_radiance: V3,
    fill_radiance: V3,
    exposure: f32,
}

impl Lights {
    fn new(style: &Style, vw: &View) -> Lights {
        let l = &style.lighting;
        Lights {
            key: l.direction(&l.key, vw),
            fill: l.direction(&l.fill, vw),
            key_radiance: l.key.radiance(),
            fill_radiance: l.fill.radiance(),
            exposure: l.exposure,
        }
    }
}

fn to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn shade(albedo: V3, n: V3, v: V3, shading: Shading, style: &Style, lights: &Lights) -> V3 {
    let n = if dot(n, v) < 0.0 { scale(n, -1.0) } else { n };
    let ambient = style.lighting.environment(n);
    let (k, f) = (dot(n, lights.key).max(0.0), dot(n, lights.fill).max(0.0));
    let h = norm(add(lights.key, v));
    let sp = shading.gloss * dot(n, h).max(0.0).powf(shading.sharpness);
    [0, 1, 2].map(|c| {
        let d = ambient[c] + lights.key_radiance[c] * k + lights.fill_radiance[c] * f;
        let shine = 1.0 + (albedo[c] * 1.6 - 1.0) * shading.metal;
        to_srgb((albedo[c] * d + sp * shine * lights.key_radiance[c]) * lights.exposure)
    })
}

fn cap(style: &Style, normal: V3, lights: &Lights) -> V3 {
    let n = norm(normal);
    let ambient = style.lighting.environment(n);
    let (k, f) = (dot(n, lights.key).abs(), dot(n, lights.fill).abs());
    let colour = rgb(style.cap);
    [0, 1, 2].map(|c| {
        let d = ambient[c] + lights.key_radiance[c] * k + lights.fill_radiance[c] * f;
        to_srgb(to_linear(colour[c]) * d * lights.exposure)
    })
}

fn sample(image: &Image, u: f32, v: f32) -> V3 {
    let (w, h) = (image.width, image.height);
    let x = (u * w as f32 - 0.5).clamp(0.0, w as f32 - 1.0);
    let y = (v * h as f32 - 0.5).clamp(0.0, h as f32 - 1.0);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x: usize, y: usize, k: usize| image.rgba[(y * w + x) * 4 + k] as f32 / 255.0;
    [0, 1, 2].map(|k| {
        let top = px(x0, y0, k) + (px(x1, y0, k) - px(x0, y0, k)) * fx;
        let bottom = px(x0, y1, k) + (px(x1, y1, k) - px(x0, y1, k)) * fx;
        top + (bottom - top) * fy
    })
}

pub(super) fn render<M: Material>(viewer: &Viewer<M>, width: usize, height: usize) -> ColorImage {
    let style = viewer.style;
    let background = rgb(style.background);
    let (w, h) = (width * SUPERSAMPLE, height * SUPERSAMPLE);
    let vw = viewer.view(Vec2::new(width as f32, height as f32));
    let lights = Lights::new(&style, &vw);
    let ortho = viewer.camera.ortho;
    let focal = vw.focal * SUPERSAMPLE as f32;
    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
    let mut depth = vec![f32::INFINITY; w * h];
    let mut colour = vec![background; w * h];
    let selected = rgb(style.selected);
    for surface in &viewer.scene.surfaces {
        let look = viewer.look(surface.group);
        if !look.visible() {
            continue;
        }
        let shading = surface.material.shading();
        let image = surface.texture.and_then(|i| viewer.scene.images.get(i));
        let tint = if look.selected && shading.tinted { style.selected_tint } else { 0.0 };
        let base = surface.colour.map(to_linear);
        let positions: Vec<V3> = surface.positions.iter().map(|p| look.place(*p)).collect();
        for t in (0..positions.len() / 3).map(|i| i * 3) {
            let corners = [positions[t], positions[t + 1], positions[t + 2]];
            let screen = corners.map(|p| {
                let d = sub(p, vw.eye);
                let z = dot(d, vw.forward);
                let w = if ortho { vw.dist } else { z };
                [cx + dot(d, vw.right) / w * focal, cy - dot(d, vw.up) / w * focal, z, w]
            });
            if screen.iter().any(|p| p[2] < vw.near) {
                continue;
            }
            let area = (screen[1][0] - screen[0][0]) * (screen[2][1] - screen[0][1])
                - (screen[1][1] - screen[0][1]) * (screen[2][0] - screen[0][0]);
            if area.abs() < 1.0e-9 {
                continue;
            }
            let back = area > 0.0;
            let lo = |k: usize| screen.iter().map(|p| p[k]).fold(f32::MAX, f32::min);
            let hi = |k: usize| screen.iter().map(|p| p[k]).fold(f32::MIN, f32::max);
            let (x0, y0) = (lo(0).floor().max(0.0) as usize, lo(1).floor().max(0.0) as usize);
            let (x1, y1) =
                ((hi(0).ceil() as i64).min(w as i64), (hi(1).ceil() as i64).min(h as i64));
            if x1 <= x0 as i64 || y1 <= y0 as i64 {
                continue;
            }
            let inv_w = screen.map(|p| 1.0 / p[3]);
            for y in y0..y1 as usize {
                let py = y as f32 + 0.5;
                for x in x0..x1 as usize {
                    let px = x as f32 + 0.5;
                    let edge = |a: [f32; 4], b: [f32; 4]| {
                        (b[0] - a[0]) * (py - a[1]) - (b[1] - a[1]) * (px - a[0])
                    };
                    let bary = [
                        edge(screen[1], screen[2]) / area,
                        edge(screen[2], screen[0]) / area,
                        edge(screen[0], screen[1]) / area,
                    ];
                    if bary.iter().any(|b| *b < 0.0) {
                        continue;
                    }
                    let total: f32 = (0..3).map(|k| bary[k] * inv_w[k]).sum();
                    let weight = [0, 1, 2].map(|k| bary[k] * inv_w[k] / total);
                    let z: f32 = (0..3).map(|k| weight[k] * screen[k][2]).sum();
                    let i = y * w + x;
                    if z >= depth[i] {
                        continue;
                    }
                    let lerp = |a: &[V3]| {
                        [0, 1, 2].map(|c| (0..3).map(|k| weight[k] * a[t + k][c]).sum::<f32>())
                    };
                    let pos = lerp(&positions);
                    if viewer.cut.is_some_and(|c| !c.keeps(pos)) {
                        continue;
                    }
                    depth[i] = z;
                    if back
                        && shading.capped
                        && look.opacity >= 1.0
                        && let Some(c) = viewer.cut
                    {
                        colour[i] = cap(&style, c.normal, &lights);
                        continue;
                    }
                    let n = norm(look.turn(lerp(&surface.normals)));
                    let albedo = match image {
                        Some(image) if surface.uvs.len() > t + 2 => {
                            let uv =
                                |c: usize| (0..3).map(|k| weight[k] * surface.uvs[t + k][c]).sum();
                            sample(image, uv(0), uv(1)).map(to_linear)
                        }
                        _ => base,
                    };
                    let albedo = [0, 1, 2].map(|k| albedo[k] + (selected[k] - albedo[k]) * tint);
                    let v = if ortho { scale(vw.forward, -1.0) } else { norm(sub(vw.eye, pos)) };
                    colour[i] = shade(albedo, n, v, shading, &style, &lights);
                }
            }
        }
    }
    let mut out = ColorImage::new([width, height], vec![Color32::BLACK; width * height]);
    let n = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    for y in 0..height {
        for x in 0..width {
            let mut sum = [0.0f32; 3];
            for dy in 0..SUPERSAMPLE {
                for dx in 0..SUPERSAMPLE {
                    let c = colour[(y * SUPERSAMPLE + dy) * w + x * SUPERSAMPLE + dx];
                    (0..3).for_each(|k| sum[k] += c[k]);
                }
            }
            let [r, g, b] = sum.map(|v| (v / n * 255.0 + 0.5) as u8);
            out.pixels[y * width + x] = Color32::from_rgb(r, g, b);
        }
    }
    out
}
