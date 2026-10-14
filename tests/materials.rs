use egui::{Color32, ColorImage, Pos2, Rect, Vec2};
use egui_bench::viewer3d::{
    self, Camera, Environment, EnvironmentLight, Image, Light, Lighting, Material, Pbr, Projector,
    Scene, Shading, Surface, Trace, V3, Viewer,
};
use egui_kittest::Harness;
use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};
use std::sync::{Arc, Mutex};

static DEVICE: Mutex<()> = Mutex::new(());

const SIZE: [usize; 2] = [200, 150];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Kind {
    #[default]
    Matte,
    Lamp,
    Glass,
    Lacquer,
    Mirror,
    Brushed,
}

impl Material for Kind {
    fn shading(&self) -> Shading {
        Shading { gloss: 0.0, capped: true, ..Shading::default() }
    }

    fn pbr(&self) -> Pbr {
        match self {
            Kind::Matte => Pbr { specular: 0.0, roughness: 1.0, ..Pbr::default() },
            Kind::Lamp => Pbr { emission: [6.0, 6.0, 6.0], ..Pbr::default() },
            Kind::Glass => Pbr { transmission: 1.0, roughness: 0.02, ior: 1.5, ..Pbr::default() },
            Kind::Lacquer => Pbr { clearcoat: 1.0, clearcoat_roughness: 0.1, ..Pbr::default() },
            Kind::Mirror => Pbr { metallic: 1.0, roughness: 0.05, ..Pbr::default() },
            Kind::Brushed => Pbr { metallic: 0.6, roughness: 0.35, ..Pbr::default() },
        }
    }
}

fn cuboid(lo: V3, hi: V3, group: usize, colour: V3, material: Kind) -> Surface<Kind> {
    let mut s = Surface { colour, group, material, ..Default::default() };
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
            let uv = [[0.0, 0.0], [2.0, 0.0], [2.0, 2.0], [0.0, 2.0]];
            for t in [[0, 1, 2], [0, 2, 3]] {
                s.positions.extend(t.map(|i| quad[i]));
                s.normals.extend([n; 3]);
                s.uvs.extend(t.map(|i| uv[i]));
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

fn dark() -> Lighting {
    Lighting {
        key: Light { intensity: 0.0, ..Light::KEY },
        fill: Light { intensity: 0.0, ..Light::FILL },
        ambient: 0.0,
        follow: false,
        ..Lighting::STUDIO
    }
}

fn to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn bumps() -> Image {
    let rgba = (0..64 * 64)
        .flat_map(|i| {
            let (x, y) = ((i % 64) as f32, (i / 64) as f32);
            let h = ((x / 64.0 * std::f32::consts::TAU * 4.0).sin()
                * (y / 64.0 * std::f32::consts::TAU * 4.0).sin()
                * 0.5
                + 0.5)
                * 255.0;
            [h as u8, 0, 0, 255]
        })
        .collect();
    Image { width: 64, height: 64, rgba }.normal_from_height(6.0)
}

fn metal_rough() -> Image {
    let rgba = (0..32 * 32)
        .flat_map(|i| {
            if (i % 32 / 8 + i / 32 / 8) % 2 == 0 { [0, 40, 255, 255] } else { [0, 255, 0, 255] }
        })
        .collect();
    Image { width: 32, height: 32, rgba }
}

fn showcase() -> Arc<Scene<Kind>> {
    showcase_with(true)
}

fn showcase_with(bumped: bool) -> Arc<Scene<Kind>> {
    let mut plate = cuboid([-2.0, -1.5, -0.1], [2.0, 1.5, 0.0], 0, [0.7, 0.7, 0.7], Kind::Brushed);
    plate.normal_map = bumped.then_some(0);
    plate.metal_rough_map = Some(1);
    Arc::new(Scene {
        surfaces: vec![
            plate,
            cuboid([-1.4, -0.4, 0.0], [-0.6, 0.4, 0.8], 1, [0.8, 0.1, 0.1], Kind::Lacquer),
            cuboid([0.6, -0.4, 0.0], [1.4, 0.4, 0.1], 2, [1.0, 0.9, 0.7], Kind::Lamp),
        ],
        images: vec![bumps(), metal_rough()],
        radius: 2.2,
        ..Default::default()
    })
}

fn studio() -> Option<EnvironmentLight> {
    Some(EnvironmentLight {
        strength: 0.6,
        ..EnvironmentLight::new(Arc::new(Environment::studio()))
    })
}

fn rendered(viewer: Viewer<Kind>, state: egui_wgpu::RenderState) -> ColorImage {
    let size = rect().size();
    let mut harness = Harness::builder()
        .with_size(size)
        .with_pixels_per_point(1.0)
        .renderer(WgpuTestRenderer::from_render_state(state))
        .build_ui(move |ui| {
            viewer.paint(ui, rect());
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

#[test]
fn the_raster_view_draws_maps_emission_coats_and_backdrops_like_the_software_renderer() {
    let _gpu = serial();
    if device(false).is_none() {
        return;
    }
    let turned = Camera { yaw: 0.4, pitch: 0.7, ortho: false, ..Camera::default() };
    for (name, viewer) in [
        ("plain", Viewer::new(showcase(), turned)),
        ("studio", Viewer::new(showcase(), turned).environment(studio())),
    ] {
        let gpu = rendered(viewer.clone(), device(false).expect("adapter"));
        let soft = viewer.render_soft(SIZE[0], SIZE[1]);
        let (mean, far) = difference(&gpu, &soft);
        eprintln!("{name}: mean {mean:.2}, {:.2}% far", far * 100.0);
        assert!(mean < 3.0 && far < 0.03, "{name}: mean {mean:.2}, far {far:.4}");
    }
    let flat = Viewer::new(showcase(), top());
    let at = Projector::new(flat.scene(), &top(), rect()).project([1.0, 0.0, 0.1]).unwrap();
    let lamp = pixel(&flat.render_soft(SIZE[0], SIZE[1]), at);
    assert!(lamp.r() == 255 && lamp.g() == 255, "{lamp:?}");
}

#[test]
fn a_normal_map_changes_the_shading_it_sits_under() {
    let plain = Viewer::new(showcase_with(false), top()).render_soft(SIZE[0], SIZE[1]);
    let bumped = Viewer::new(showcase(), top()).render_soft(SIZE[0], SIZE[1]);
    let (mean, _) = difference(&plain, &bumped);
    assert!(mean > 2.0, "the bumps barely show: {mean}");
}

fn traced(viewer: &Viewer<Kind>, samples: u32, bounces: u32) -> ColorImage {
    viewer
        .clone()
        .trace(Some(Trace { samples, bounces, per_frame: 32 }))
        .render_gpu(SIZE[0], SIZE[1])
        .expect("traced")
}

#[test]
fn a_white_sky_lights_an_open_matte_plate_to_its_own_colour() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let plate = cuboid([-2.0, -1.5, -0.1], [2.0, 1.5, 0.0], 0, [0.5, 0.5, 0.5], Kind::Matte);
        let scene = Arc::new(Scene { surfaces: vec![plate], radius: 2.2, ..Default::default() });
        let sky = Environment::new(32, 16, vec![[1.0; 3]; 32 * 16]);
        let light = EnvironmentLight { backdrop: false, ..EnvironmentLight::new(Arc::new(sky)) };
        let viewer = Viewer::new(scene, top()).lighting(dark()).environment(Some(light));
        let image = traced(&viewer, 256, 2);
        let centre = pixel(&image, rect().center());
        let want = 128;
        assert!(
            (centre.r() as i32 - want).abs() <= 4,
            "hardware {hardware}: {centre:?}, want {want}"
        );
    }
}

#[test]
fn a_lamp_lights_the_plate_beside_it_in_the_dark() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let viewer = Viewer::new(showcase(), top()).lighting(dark());
        let image = traced(&viewer, 128, 3);
        let at = Projector::new(viewer.scene(), &top(), rect());
        let near = pixel(&image, at.project([1.0, 0.7, 0.0]).unwrap());
        let far = pixel(&image, at.project([-1.8, -1.3, 0.0]).unwrap());
        let lamp = pixel(&image, at.project([1.0, 0.0, 0.1]).unwrap());
        assert!(lamp.r() > 240, "hardware {hardware}: lamp {lamp:?}");
        assert!(
            near.r() as i32 > far.r() as i32 + 30,
            "hardware {hardware}: near {near:?} far {far:?}"
        );
    }
}

#[test]
fn glass_refracts_the_red_block_behind_it() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let scene = Arc::new(Scene {
            surfaces: vec![
                cuboid([-2.0, -1.5, -0.1], [2.0, 1.5, 0.0], 0, [0.9, 0.1, 0.1], Kind::Matte),
                cuboid([-0.6, -0.6, 0.3], [0.6, 0.6, 1.2], 1, [1.0, 1.0, 1.0], Kind::Glass),
            ],
            radius: 2.2,
            ..Default::default()
        });
        let viewer = Viewer::new(scene, top()).environment(studio());
        let image = traced(&viewer, 128, 6);
        let through = pixel(&image, rect().center());
        assert!(
            through.r() as i32 > through.g() as i32 + 40,
            "hardware {hardware}: the plate does not show through the glass: {through:?}"
        );
    }
}

#[test]
fn a_mirror_shows_the_sky_and_a_lacquer_shines_over_its_colour() {
    let _gpu = serial();
    for hardware in [false, true] {
        if device(hardware).is_none() {
            continue;
        }
        let surfaces = vec![
            cuboid([-2.0, -1.0, -0.1], [-0.1, 1.0, 0.0], 0, [0.95, 0.95, 0.95], Kind::Mirror),
            cuboid([0.1, -1.0, -0.1], [2.0, 1.0, 0.0], 1, [0.8, 0.1, 0.1], Kind::Lacquer),
        ];
        let scene = Arc::new(Scene { surfaces, radius: 2.2, ..Default::default() });
        let sky = Environment::new(32, 16, vec![[0.2, 0.4, 0.9]; 32 * 16]);
        let light = EnvironmentLight { backdrop: false, ..EnvironmentLight::new(Arc::new(sky)) };
        let viewer = Viewer::new(scene, top()).lighting(dark()).environment(Some(light));
        let image = traced(&viewer, 128, 3);
        let at = Projector::new(viewer.scene(), &top(), rect());
        let mirror = pixel(&image, at.project([-1.0, 0.0, 0.0]).unwrap());
        let want = [0.2f32, 0.4, 0.9].map(|c| (to_srgb(c * 0.888) * 255.0) as i32);
        for k in 0..3 {
            assert!(
                (mirror[k] as i32 - want[k]).abs() < 8,
                "hardware {hardware}: mirror {mirror:?} want {want:?}"
            );
        }
        let lacquer = pixel(&image, at.project([1.0, 0.0, 0.0]).unwrap());
        assert!(
            lacquer.b() > 15 && lacquer.r() > lacquer.g() + 30,
            "hardware {hardware}: {lacquer:?}"
        );
    }
}

#[test]
fn both_tracers_agree_on_maps_coats_lamps_and_the_studio() {
    let _gpu = serial();
    if device(true).is_none() {
        return;
    }
    let turned = Camera { yaw: 0.4, pitch: 0.7, ortho: false, ..Camera::default() };
    let viewer = Viewer::new(showcase(), turned).environment(studio());
    let hardware = traced(&viewer, 64, 3);
    device(false).expect("compute");
    let compute = traced(&viewer, 64, 3);
    let (mean, _) = difference(&hardware, &compute);
    assert!(mean < 1.5, "mean difference {mean}");
}
