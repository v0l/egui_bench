use egui::{Color32, ColorImage, Pos2, Rect, Vec2};
use egui_bench::viewer3d::{
    self, BACKGROUND, Camera, Cut, Image, Lines, Look, Material, Scene, Shading, Surface, V3,
    Viewer,
};
use egui_kittest::Harness;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use std::sync::{Arc, Mutex};

static DEVICE: Mutex<()> = Mutex::new(());

const SIZE: [usize; 2] = [320, 240];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Finish {
    #[default]
    Paint,
    Chrome,
}

impl Material for Finish {
    fn shading(&self) -> Shading {
        match self {
            Finish::Paint => Shading { capped: true, ..Shading::default() },
            Finish::Chrome => {
                Shading { gloss: 0.5, sharpness: 90.0, metal: 0.8, capped: true, tinted: true }
            }
        }
    }
}

fn cube(at: V3, half: f32, group: usize, material: Finish) -> Surface<Finish> {
    let mut s = Surface { colour: [0.62, 0.66, 0.72], group, material, ..Default::default() };
    for axis in 0..3 {
        for sign in [-1.0f32, 1.0] {
            let mut n = [0.0; 3];
            n[axis] = sign;
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            let point = |a: f32, b: f32| {
                let mut p = [0.0; 3];
                p[axis] = sign;
                p[u] = a;
                p[v] = b * sign;
                [at[0] + p[0] * half, at[1] + p[1] * half, at[2] + p[2] * half]
            };
            let quad = [point(-1.0, -1.0), point(1.0, -1.0), point(1.0, 1.0), point(-1.0, 1.0)];
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            for t in [[0, 1, 2], [0, 2, 3]] {
                s.positions.extend(t.map(|i| quad[i]));
                s.normals.extend([n; 3]);
                s.uvs.extend(t.map(|i| uv[i]));
            }
        }
    }
    s
}

fn checker() -> Image {
    let rgba = (0..64 * 64)
        .flat_map(|i| {
            let (x, y) = (i % 64, i / 64);
            if (x / 8 + y / 8) % 2 == 0 { [220, 40, 40, 255] } else { [30, 30, 200, 255] }
        })
        .collect();
    Image { width: 64, height: 64, rgba }
}

fn scene() -> Scene<Finish> {
    let mut textured = cube([1.4, 0.0, 0.0], 0.6, 1, Finish::Paint);
    textured.texture = Some(0);
    Scene {
        surfaces: vec![cube([-0.8, 0.0, 0.0], 0.8, 0, Finish::Chrome), textured],
        images: vec![checker()],
        radius: 2.4,
        ..Default::default()
    }
}

fn gpu() -> Option<egui_wgpu::RenderState> {
    let _one = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    let made = std::panic::catch_unwind(|| {
        create_render_state(default_wgpu_setup(), egui_wgpu::RendererOptions::PREDICTABLE)
    });
    if made.is_err() {
        eprintln!("skipped: no wgpu adapter");
    }
    made.ok()
}

fn available() -> bool {
    gpu().is_some()
}

fn rendered(viewer: Viewer<Finish>) -> ColorImage {
    let state = gpu().expect("adapter");
    let _one = DEVICE.lock().unwrap_or_else(|e| e.into_inner());
    viewer3d::install(&state);
    assert!(viewer3d::installed());
    let size = Vec2::new(SIZE[0] as f32, SIZE[1] as f32);
    let rect = Rect::from_min_size(Pos2::ZERO, size);
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(1.0)
        .renderer(WgpuTestRenderer::from_render_state(state))
        .build_ui(move |ui| {
            viewer.paint(ui, rect);
        });
    harness.run();
    let image = harness.render().expect("rendered");
    let pixels =
        image.pixels().map(|p| Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3])).collect();
    ColorImage::new([image.width() as usize, image.height() as usize], pixels)
}

fn difference(a: &ColorImage, b: &ColorImage) -> (f32, f32) {
    let (mut sum, mut far) = (0u64, 0usize);
    for (p, q) in a.pixels.iter().zip(&b.pixels) {
        let d = (0..3).map(|k| (p[k] as i32 - q[k] as i32).unsigned_abs()).max().unwrap_or(0);
        sum += d as u64;
        if d > 40 {
            far += 1;
        }
    }
    let n = a.pixels.len() as f32;
    (sum as f32 / n, far as f32 / n)
}

fn agrees(viewer: Viewer<Finish>, what: &str) -> ColorImage {
    let gpu = rendered(viewer.clone());
    let soft = viewer.render_soft(SIZE[0], SIZE[1]);
    assert_eq!(gpu.size, soft.size);
    let (mean, far) = difference(&gpu, &soft);
    eprintln!("{what}: mean {mean:.2}, {:.2}% far", far * 100.0);
    assert!(mean < 3.0 && far < 0.02, "{what}: mean {mean:.2}, far {far:.4}");
    gpu
}

#[test]
fn the_gpu_draws_what_the_software_renderer_draws() {
    if !available() {
        return;
    }
    let scene = Arc::new(scene());
    let base = Viewer::new(scene.clone(), Camera::default());
    let lit = agrees(base.clone(), "lit");
    assert_ne!(lit.pixels[SIZE[0] * SIZE[1] / 2 + SIZE[0] / 2 - 40], BACKGROUND);
    assert_eq!(lit.pixels[0], BACKGROUND);

    let perspective = Camera { ortho: false, ..Camera::default() };
    agrees(Viewer::new(scene.clone(), perspective), "perspective");

    let cut = Some(Cut { normal: [0.0, -1.0, 0.0], offset: 0.0 });
    let capped = agrees(base.clone().cut(cut), "cut");
    let orange = capped.pixels.iter().filter(|p| p.r() > 150 && p.g() > 70 && p.b() < 60).count();
    assert!(orange > 500, "{orange} cap pixels");

    let selected = [Look { selected: true, ..Look::default() }, Look::default()];
    agrees(base.clone().looks(selected), "selected");

    let hidden = [Look::HIDDEN, Look::default()];
    agrees(base.clone().looks(hidden), "hidden");

    let turned = Camera { yaw: 2.2, pitch: -0.4, zoom: 1.6, ..Camera::default() };
    agrees(Viewer::new(scene, turned), "turned");
}

#[test]
fn a_ghost_lets_the_part_behind_show_through() {
    if !available() {
        return;
    }
    let front = cube([0.0, 0.0, 0.0], 0.5, 0, Finish::Paint);
    let mut back = cube([0.0, 0.0, -1.5], 0.5, 1, Finish::Paint);
    back.colour = [0.9, 0.2, 0.1];
    let scene = Arc::new(Scene { surfaces: vec![front, back], radius: 2.0, ..Default::default() });
    let top = Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() };
    let solid = rendered(Viewer::new(scene.clone(), top));
    let ghost = [Look { opacity: 0.3, ..Look::default() }, Look::default()];
    let seen = rendered(Viewer::new(scene, top).looks(ghost));
    let centre = SIZE[1] / 2 * SIZE[0] + SIZE[0] / 2;
    let (a, b) = (solid.pixels[centre], seen.pixels[centre]);
    assert!(b.r() as i32 - b.b() as i32 > a.r() as i32 - a.b() as i32 + 40, "{a:?} then {b:?}");
}

#[test]
fn lines_draw_over_the_faces_they_edge() {
    if !available() {
        return;
    }
    let face = cube([0.0, 0.0, 0.0], 0.8, 0, Finish::Paint);
    let edge = Lines {
        segments: vec![[[-0.8, 0.0, 0.8], [0.8, 0.0, 0.8]]],
        colour: [0.0, 1.0, 0.0],
        width: 3.0,
        ..Default::default()
    };
    let scene = Arc::new(Scene {
        surfaces: vec![face],
        lines: vec![edge],
        radius: 1.5,
        ..Default::default()
    });
    let top = Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() };
    let image = rendered(Viewer::new(scene.clone(), top));
    let green = |p: &Color32| p.g() > 200 && p.r() < 60 && p.b() < 60;
    let row = SIZE[1] / 2;
    let hits = (0..SIZE[0]).filter(|x| green(&image.pixels[row * SIZE[0] + x])).count();
    assert!(hits > SIZE[0] / 4, "{hits} green pixels on the middle row");
    let cut = Some(Cut { normal: [1.0, 0.0, 0.0], offset: 0.0 });
    let half = rendered(Viewer::new(scene, top).cut(cut));
    let left = (0..SIZE[0] / 2).filter(|x| green(&half.pixels[row * SIZE[0] + x])).count();
    let right = (SIZE[0] / 2..SIZE[0]).filter(|x| green(&half.pixels[row * SIZE[0] + x])).count();
    assert!(left > 20 && right == 0, "{left} left, {right} right");
}
