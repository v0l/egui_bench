use egui::{Color32, ColorImage, Pos2, Rect, Vec2};
use egui_bench::viewer3d::{
    self, Camera, Cut, Light, Lighting, Lines, Look, Material, Painted, Projector, RenderMode,
    Scene, Shading, Surface, Trace, V3, Viewer,
};
use egui_kittest::Harness;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use std::sync::{Arc, Mutex};

static DEVICE: Mutex<()> = Mutex::new(());

const SIZE: [usize; 2] = [240, 180];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Finish {
    #[default]
    Matte,
}

impl Material for Finish {
    fn shading(&self) -> Shading {
        match self {
            Finish::Matte => Shading { gloss: 0.0, capped: true, ..Shading::default() },
        }
    }
}

fn cuboid(lo: V3, hi: V3, group: usize, colour: V3) -> Surface<Finish> {
    let mut s = Surface { colour, group, ..Default::default() };
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
                [0, 1, 2].map(|k| lo[k] + (p[k] * 0.5 + 0.5) * (hi[k] - lo[k]))
            };
            let quad = [point(-1.0, -1.0), point(1.0, -1.0), point(1.0, 1.0), point(-1.0, 1.0)];
            for t in [[0, 1, 2], [0, 2, 3]] {
                s.positions.extend(t.map(|i| quad[i]));
                s.normals.extend([n; 3]);
            }
        }
    }
    s
}

fn serial() -> std::sync::MutexGuard<'static, ()> {
    DEVICE.lock().unwrap_or_else(|e| e.into_inner())
}

fn device(hardware: bool) -> Option<egui_wgpu::RenderState> {
    let setup = if hardware { viewer3d::wgpu_setup() } else { default_wgpu_setup() };
    let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        create_render_state(setup, egui_wgpu::RendererOptions::PREDICTABLE)
    }));
    let state = made.ok()?;
    viewer3d::install(&state);
    if hardware && !viewer3d::hardware_tracing() {
        eprintln!("skipped: no ray query on this adapter");
        return None;
    }
    Some(state)
}

fn top() -> Camera {
    Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() }
}

fn rect() -> Rect {
    Rect::from_min_size(Pos2::ZERO, Vec2::new(SIZE[0] as f32, SIZE[1] as f32))
}

fn pixel(image: &ColorImage, at: Pos2) -> Color32 {
    image.pixels[at.y as usize * image.size[0] + at.x as usize]
}

fn bench() -> Arc<Scene<Finish>> {
    Arc::new(Scene {
        surfaces: vec![
            cuboid([-2.0, -1.5, -0.1], [2.0, 1.5, 0.0], 0, [0.6, 0.6, 0.6]),
            cuboid([-0.2, -0.2, 0.0], [0.2, 0.2, 0.4], 1, [0.8, 0.3, 0.2]),
        ],
        radius: 2.2,
        ..Default::default()
    })
}

fn sun() -> Lighting {
    Lighting {
        key: Light { azimuth: 0.0, elevation: 0.6, intensity: 1.0, spread: 0.0, ..Light::KEY },
        fill: Light { intensity: 0.0, ..Light::FILL },
        ambient: 0.0,
        follow: false,
        ..Lighting::STUDIO
    }
}

fn direct() -> Option<Trace> {
    Some(Trace { samples: 16, bounces: 1, per_frame: 4 })
}

fn traced(hardware: bool) -> Option<ColorImage> {
    device(hardware)?;
    let viewer = Viewer::new(bench(), top()).lighting(sun()).trace(direct());
    viewer.render_gpu(SIZE[0], SIZE[1])
}

#[test]
fn a_traced_plate_matches_the_raster_in_the_open_and_darkens_in_the_shadow() {
    let _gpu = serial();
    let Some(image) = traced(false) else {
        eprintln!("skipped: no wgpu adapter");
        return;
    };
    let viewer = Viewer::new(bench(), top()).lighting(sun());
    let raster = viewer.render_soft(SIZE[0], SIZE[1]);
    let at = Projector::new(viewer.scene(), &top(), rect());
    let open = at.project([1.3, 0.8, 0.0]).unwrap();
    let (r, t) = (pixel(&raster, open), pixel(&image, open));
    for k in 0..3 {
        assert!((r[k] as i32 - t[k] as i32).abs() <= 3, "open plate: raster {r:?}, traced {t:?}");
    }
    let shade = at.project([-0.5, 0.0, 0.0]).unwrap();
    let (r, t) = (pixel(&raster, shade), pixel(&image, shade));
    assert!(t.r() < r.r() / 4, "shadow: raster {r:?}, traced {t:?}");
    let block = at.project([0.0, 0.0, 0.4]).unwrap();
    let b = pixel(&image, block);
    assert!(b.r() > b.g() + 40, "block {b:?}");
}

#[test]
fn ray_query_and_the_compute_bvh_trace_the_same_picture() {
    let _gpu = serial();
    let Some(hardware) = traced(true) else {
        return;
    };
    let compute = traced(false).expect("compute tracer");
    let mut sum = 0u64;
    for (a, b) in hardware.pixels.iter().zip(&compute.pixels) {
        sum += (0..3).map(|k| (a[k] as i32 - b[k] as i32).unsigned_abs() as u64).sum::<u64>();
    }
    let mean = sum as f32 / (hardware.pixels.len() * 3) as f32;
    assert!(mean < 1.0, "mean difference {mean}");
}

fn cut_cube() -> Viewer<Finish> {
    let scene = Arc::new(Scene {
        surfaces: vec![cuboid([-0.6; 3], [0.6; 3], 0, [0.6, 0.6, 0.6])],
        radius: 1.2,
        ..Default::default()
    });
    let cut = Some(Cut { normal: [0.0, -1.0, 0.0], offset: 0.0 });
    Viewer::new(scene, Camera::default()).cut(cut).trace(direct())
}

#[test]
fn a_traced_section_is_capped() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let image = cut_cube().render_gpu(SIZE[0], SIZE[1]).expect("traced");
        let orange =
            image.pixels.iter().filter(|p| p.r() > 120 && p.g() > 50 && p.b() < 50).count();
        assert!(orange > 1000, "hardware {hardware}: {orange} cap pixels");
    }
}

#[test]
fn a_traced_ghost_shows_what_is_behind_it_and_a_hidden_part_is_gone() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let scene = Arc::new(Scene {
            surfaces: vec![
                cuboid([-0.5, -0.5, 0.0], [0.5, 0.5, 1.0], 0, [0.6, 0.6, 0.6]),
                cuboid([-0.5, -0.5, -2.0], [0.5, 0.5, -1.0], 1, [0.9, 0.1, 0.1]),
            ],
            radius: 2.0,
            ..Default::default()
        });
        let centre = rect().center();
        let base = Viewer::new(scene, top()).trace(direct());
        let solid = pixel(&base.render_gpu(SIZE[0], SIZE[1]).unwrap(), centre);
        let ghost = [Look { opacity: 0.3, ..Look::default() }, Look::default()];
        let seen = pixel(&base.clone().looks(ghost).render_gpu(SIZE[0], SIZE[1]).unwrap(), centre);
        assert!(
            seen.r() as i32 - seen.b() as i32 > solid.r() as i32 - solid.b() as i32 + 40,
            "hardware {hardware}: {solid:?} then {seen:?}"
        );
        let hidden = [Look::HIDDEN, Look::HIDDEN];
        let gone = base.looks(hidden).render_gpu(SIZE[0], SIZE[1]).unwrap();
        let bg = viewer3d::BACKGROUND;
        let near = |p: &Color32| (0..3).all(|k| (p[k] as i32 - bg[k] as i32).abs() <= 1);
        assert!(gone.pixels.iter().all(near), "hardware {hardware}");
    }
}

#[test]
fn lines_are_drawn_over_a_traced_picture() {
    let _gpu = serial();
    if device(false).is_none() {
        return;
    }
    let edge = Lines {
        segments: vec![[[-0.6, 0.0, 0.6], [0.6, 0.0, 0.6]]],
        colour: [0.0, 1.0, 0.0],
        width: 3.0,
        ..Default::default()
    };
    let scene = Arc::new(Scene {
        surfaces: vec![cuboid([-0.6; 3], [0.6; 3], 0, [0.6, 0.6, 0.6])],
        lines: vec![edge],
        radius: 1.2,
        ..Default::default()
    });
    let image = Viewer::new(scene, top()).trace(direct()).render_gpu(SIZE[0], SIZE[1]).unwrap();
    let row = SIZE[1] / 2;
    let green = (0..SIZE[0])
        .filter(|x| {
            let p = image.pixels[row * SIZE[0] + x];
            p.g() > 200 && p.r() < 60 && p.b() < 60
        })
        .count();
    assert!(green > SIZE[0] / 4, "{green} green pixels");
}

#[test]
fn the_viewport_rasters_while_moving_then_refines_to_the_target() {
    let _gpu = serial();
    let Some(state) = device(false) else {
        return;
    };
    let camera = Arc::new(Mutex::new(Camera::default()));
    let painted = Arc::new(Mutex::new(Vec::<Painted>::new()));
    let (cam, log) = (camera.clone(), painted.clone());
    let trace = Some(Trace { samples: 32, bounces: 2, per_frame: 8 });
    let scene = bench();
    let mut harness = Harness::builder()
        .with_size(rect().size())
        .with_pixels_per_point(1.0)
        .renderer(WgpuTestRenderer::from_render_state(state))
        .build_ui(move |ui| {
            let viewer = Viewer::new(scene.clone(), *cam.lock().unwrap()).trace(trace);
            log.lock().unwrap().push(viewer.paint(ui, rect()));
        });
    for _ in 0..8 {
        harness.step();
    }
    let seen = painted.lock().unwrap().clone();
    assert_eq!(seen[0].samples, 0, "{seen:?}");
    let last = seen.last().unwrap();
    assert!(last.settled() && last.samples == 32, "{seen:?}");
    camera.lock().unwrap().yaw += 0.3;
    harness.step();
    assert_eq!(painted.lock().unwrap().last().unwrap().samples, 0);
}

#[test]
fn the_traversal_heatmap_lights_up_geometry_and_leaves_empty_space_alone() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let viewer = Viewer::new(bench(), top()).mode(RenderMode::Traversal);
        let image = viewer.render_gpu(SIZE[0], SIZE[1]).expect("traced");
        let at = Projector::new(viewer.scene(), &top(), rect());
        let block = pixel(&image, at.project([0.0, 0.0, 0.4]).unwrap());
        let bg = viewer3d::BACKGROUND;
        assert!(
            (0..3).any(|k| (block[k] as i32 - bg[k] as i32).abs() > 20),
            "hardware {hardware}: {block:?}"
        );
        assert_eq!(image.pixels[0], bg, "hardware {hardware}");
    }
}

#[test]
fn render_gpu_without_a_trace_is_the_raster_view() {
    let _gpu = serial();
    if device(false).is_none() {
        return;
    }
    let viewer = Viewer::new(bench(), top()).lighting(sun());
    let gpu = viewer.render_gpu(SIZE[0], SIZE[1]).unwrap();
    let soft = viewer.render_soft(SIZE[0], SIZE[1]);
    let far = gpu
        .pixels
        .iter()
        .zip(&soft.pixels)
        .filter(|(a, b)| (0..3).any(|k| (a[k] as i32 - b[k] as i32).abs() > 40))
        .count();
    assert!(far < gpu.pixels.len() / 50, "{far} pixels differ");
}

#[test]
fn glass_lets_the_plate_and_its_sunlight_through() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let scene = Arc::new(Scene {
            surfaces: vec![
                cuboid([-2.0, -1.5, -0.1], [2.0, 1.5, 0.0], 0, [0.8, 0.2, 0.2]),
                cuboid([-1.0, -1.0, 0.5], [1.0, 1.0, 0.52], 1, [0.6, 0.8, 0.85]),
            ],
            radius: 2.2,
            ..Default::default()
        });
        let glass = [Look::default(), Look { opacity: 0.25, ..Look::default() }];
        let viewer = Viewer::new(scene, top()).lighting(sun()).looks(glass).trace(Some(Trace {
            samples: 64,
            bounces: 2,
            per_frame: 16,
        }));
        let image = viewer.render_gpu(SIZE[0], SIZE[1]).unwrap();
        let at = Projector::new(viewer.scene(), &top(), rect());
        let open = pixel(&image, at.project([1.8, 1.3, 0.0]).unwrap());
        let behind = pixel(&image, at.project([0.4, 0.0, 0.0]).unwrap());
        assert!(
            behind.r() as f32 > open.r() as f32 * 0.8,
            "hardware {hardware}: open {open:?}, under the glass {behind:?}"
        );
        assert!(
            behind.r() as i32 - behind.g() as i32 > (open.r() as i32 - open.g() as i32) / 2,
            "hardware {hardware}: open {open:?}, under the glass {behind:?}"
        );
    }
}
