use egui_bench::prelude::*;
use egui_bench::viewer3d::{
    Camera, Cut, Lines, Look, Material, Orbit, Scene, Shading, Surface, V3, Viewer,
};
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
enum Finish {
    #[default]
    Paint,
    Chrome,
    Edge,
    Outline,
}

impl Material for Finish {
    fn shading(&self) -> Shading {
        match self {
            Finish::Paint => Shading { capped: true, ..Shading::default() },
            Finish::Chrome => {
                Shading { gloss: 0.5, sharpness: 90.0, metal: 0.8, capped: true, tinted: true }
            }
            Finish::Edge => Shading::default(),
            Finish::Outline => Shading { tinted: false, ..Shading::default() },
        }
    }
}

fn push_triangle(s: &mut Surface<Finish>, corners: [V3; 3], normals: [V3; 3]) {
    s.positions.extend(corners);
    s.normals.extend(normals);
}

fn cube(at: V3, half: f32, group: usize) -> (Surface<Finish>, Lines<Finish>) {
    let mut s = Surface { colour: [0.62, 0.66, 0.72], group, ..Default::default() };
    let corner = |x: f32, y: f32, z: f32| [at[0] + x * half, at[1] + y * half, at[2] + z * half];
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
                corner(p[0], p[1], p[2])
            };
            let quad = [point(-1.0, -1.0), point(1.0, -1.0), point(1.0, 1.0), point(-1.0, 1.0)];
            push_triangle(&mut s, [quad[0], quad[1], quad[2]], [n; 3]);
            push_triangle(&mut s, [quad[0], quad[2], quad[3]], [n; 3]);
        }
    }
    let mut lines = Lines {
        colour: [0.05, 0.06, 0.07],
        material: Finish::Edge,
        group,
        width: 1.5,
        ..Default::default()
    };
    let at_bits = |k: usize| {
        corner(
            if k & 1 == 0 { -1.0 } else { 1.0 },
            if k & 2 == 0 { -1.0 } else { 1.0 },
            if k & 4 == 0 { -1.0 } else { 1.0 },
        )
    };
    for k in 0..8 {
        for bit in [1, 2, 4].into_iter().filter(|bit| k & bit == 0) {
            lines.segments.push([at_bits(k), at_bits(k | bit)]);
        }
    }
    (s, lines)
}

fn sphere(at: V3, r: f32, group: usize) -> Surface<Finish> {
    let mut s = Surface {
        colour: [0.83, 0.68, 0.34],
        material: Finish::Chrome,
        group,
        ..Default::default()
    };
    let (rings, slices) = (24, 48);
    let point = |i: usize, j: usize| {
        let (t, p) = (PI * i as f32 / rings as f32, TAU * j as f32 / slices as f32);
        let n = [t.sin() * p.cos(), t.sin() * p.sin(), t.cos()];
        ([at[0] + r * n[0], at[1] + r * n[1], at[2] + r * n[2]], n)
    };
    for i in 0..rings {
        for j in 0..slices {
            let (a, b, c, d) = (point(i, j), point(i + 1, j), point(i + 1, j + 1), point(i, j + 1));
            push_triangle(&mut s, [a.0, b.0, c.0], [a.1, b.1, c.1]);
            push_triangle(&mut s, [a.0, c.0, d.0], [a.1, c.1, d.1]);
        }
    }
    s
}

fn ring(r: f32, z: f32, group: usize) -> Lines<Finish> {
    let n = 96;
    let at = |k: usize| {
        let t = TAU * k as f32 / n as f32;
        [r * t.cos(), r * t.sin(), z]
    };
    Lines {
        segments: (0..n).map(|k| [at(k), at(k + 1)]).collect(),
        colour: [0.31, 0.78, 0.86],
        material: Finish::Outline,
        group,
        width: 3.0,
    }
}

fn scene() -> Scene<Finish> {
    let mut scene = Scene { centre: [0.0, 0.0, 0.0], radius: 2.2, ..Default::default() };
    let (box_surface, box_lines) = cube([-0.9, 0.0, 0.0], 0.8, 0);
    scene.surfaces.push(box_surface);
    scene.lines.push(box_lines);
    scene.surfaces.push(sphere([0.9, 0.0, 0.0], 0.8, 1));
    scene.lines.push(ring(1.0, -0.8, 1));
    scene
}

struct Demo {
    scene: Arc<Scene<Finish>>,
    camera: Camera,
    orbit: Orbit,
    ghost: bool,
    cut: Option<f32>,
    selected: Option<usize>,
    soft: bool,
}

impl eframe::App for Demo {
    fn ui(&mut self, ui: &mut egui::Ui, _f: &mut eframe::Frame) {
        egui::Panel::top("controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                if toggle(ui, "ortho", self.camera.ortho).clicked() {
                    self.camera.ortho = !self.camera.ortho;
                }
                if toggle(ui, "ghost box", self.ghost).clicked() {
                    self.ghost = !self.ghost;
                }
                if toggle(ui, "section", self.cut.is_some()).clicked() {
                    self.cut = if self.cut.is_some() { None } else { Some(0.0) };
                }
                if let Some(at) = &mut self.cut {
                    ui.add(egui::Slider::new(at, -1.0..=1.0).show_value(false));
                }
                if toggle(ui, "software", self.soft).clicked() {
                    self.soft = !self.soft;
                }
            });
        });
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let rect = ui.max_rect();
            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            let looks: Vec<Look> = (0..2)
                .map(|group| Look {
                    opacity: if self.ghost && group == 0 { 0.35 } else { 1.0 },
                    selected: self.selected == Some(group),
                    ..Look::default()
                })
                .collect();
            let cut = self.cut.map(|offset| Cut { normal: [0.0, -1.0, 0.0], offset });
            let mut viewer = Viewer::new(self.scene.clone(), self.camera).looks(looks).cut(cut);
            viewer.navigate(ui, &response, &mut self.orbit);
            if response.double_clicked() {
                self.camera = Camera { ortho: self.camera.ortho, ..Camera::default() };
            } else {
                self.camera = viewer.camera();
            }
            if response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                self.selected =
                    viewer.pick(rect, at).map(|hit| self.scene.surfaces[hit.surface].group);
            }
            if self.soft {
                let ppp = ui.ctx().pixels_per_point();
                let image = viewer.render_soft(
                    (rect.width() * ppp) as usize / 2,
                    (rect.height() * ppp) as usize / 2,
                );
                let texture = ui.ctx().load_texture("soft", image, egui::TextureOptions::LINEAR);
                ui.painter().image(
                    texture.id(),
                    rect,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            } else {
                viewer.paint(ui, rect);
            }
            ui.painter().text(
                rect.left_bottom() + egui::vec2(10.0, -10.0),
                egui::Align2::LEFT_BOTTOM,
                "drag to orbit, shift-drag to pan, scroll to zoom, click to select, double-click to reset",
                egui::FontId::proportional(11.0),
                LEGEND,
            );
        });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 760.0]),
        depth_buffer: 24,
        multisampling: 4,
        ..Default::default()
    };
    eframe::run_native(
        "egui_bench viewer3d",
        options,
        Box::new(|cc| {
            egui_bench::install(&cc.egui_ctx);
            Ok(Box::new(Demo {
                scene: Arc::new(scene()),
                camera: Camera::default(),
                orbit: Orbit::default(),
                ghost: false,
                cut: None,
                selected: None,
                soft: false,
            }))
        }),
    )
}
