mod gl;
mod soft;

use egui::{Color32, ColorImage, Pos2, Rect, Response, Ui, Vec2};
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use gl::draw_time;

pub type V3 = [f32; 3];
pub type Placement = [f32; 16];

pub const IDENTITY: Placement =
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];
pub const BACKGROUND: Color32 = Color32::from_rgb(20, 22, 26);
const DISTANCE: f32 = 2.6;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shading {
    pub gloss: f32,
    pub sharpness: f32,
    pub metal: f32,
    pub capped: bool,
    pub tinted: bool,
}

impl Default for Shading {
    fn default() -> Self {
        Shading { gloss: 0.12, sharpness: 40.0, metal: 0.0, capped: false, tinted: true }
    }
}

pub trait Material: Copy + Eq + Hash + Default + Send + Sync + 'static {
    fn shading(&self) -> Shading {
        Shading::default()
    }
}

impl Material for () {}

#[derive(Clone, Default)]
pub struct Surface<M = ()> {
    pub positions: Vec<V3>,
    pub normals: Vec<V3>,
    pub uvs: Vec<[f32; 2]>,
    pub colour: V3,
    pub texture: Option<usize>,
    pub material: M,
    pub group: usize,
}

#[derive(Clone, Default)]
pub struct Lines<M = ()> {
    pub segments: Vec<[V3; 2]>,
    pub colour: V3,
    pub material: M,
    pub group: usize,
    pub width: f32,
}

#[derive(Clone, Default)]
pub struct Image {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

pub struct Scene<M = ()> {
    pub id: u64,
    pub surfaces: Vec<Surface<M>>,
    pub lines: Vec<Lines<M>>,
    pub images: Vec<Image>,
    pub centre: V3,
    pub radius: f32,
}

impl<M> Default for Scene<M> {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Scene {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            surfaces: Vec::new(),
            lines: Vec::new(),
            images: Vec::new(),
            centre: [0.0; 3],
            radius: 1.0,
        }
    }
}

impl<M> Scene<M> {
    pub fn triangles(&self) -> usize {
        self.surfaces.iter().map(|s| s.positions.len() / 3).sum()
    }

    pub fn segments(&self) -> usize {
        self.lines.iter().map(|l| l.segments.len()).sum()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Look {
    pub placement: Placement,
    pub opacity: f32,
    pub selected: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look { placement: IDENTITY, opacity: 1.0, selected: false }
    }
}

impl Look {
    pub const HIDDEN: Look = Look { placement: IDENTITY, opacity: 0.0, selected: false };

    pub fn visible(&self) -> bool {
        self.opacity > 0.0
    }

    pub fn place(&self, p: V3) -> V3 {
        let m = &self.placement;
        [
            m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
            m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
            m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
        ]
    }

    pub fn turn(&self, v: V3) -> V3 {
        let m = &self.placement;
        [
            m[0] * v[0] + m[4] * v[1] + m[8] * v[2],
            m[1] * v[0] + m[5] * v[1] + m[9] * v[2],
            m[2] * v[0] + m[6] * v[1] + m[10] * v[2],
        ]
    }
}

#[derive(Clone, Copy, PartialEq)]
pub struct Style {
    pub background: Color32,
    pub ambient: f32,
    pub key: f32,
    pub fill: f32,
    pub cap: Color32,
    pub selected: Color32,
    pub selected_tint: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            background: BACKGROUND,
            ambient: 0.32,
            key: 0.78,
            fill: 0.25,
            cap: Color32::from_rgb(237, 140, 46),
            selected: Color32::from_rgb(245, 166, 59),
            selected_tint: 0.35,
        }
    }
}

pub fn rgb(c: Color32) -> V3 {
    [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0)
}

pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: V3, k: f32) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}

pub fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

pub fn norm(a: V3) -> V3 {
    scale(a, 1.0 / dot(a, a).sqrt().max(1.0e-12))
}

pub fn hit_triangle(origin: V3, direction: V3, [a, b, c]: [V3; 3]) -> Option<f32> {
    let (e1, e2) = (sub(b, a), sub(c, a));
    let p = cross(direction, e2);
    let det = dot(e1, p);
    if det.abs() < 1.0e-12 {
        return None;
    }
    let s = sub(origin, a);
    let u = dot(s, p) / det;
    let q = cross(s, e1);
    let v = dot(direction, q) / det;
    let t = dot(e2, q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Camera {
    pub yaw: f32,
    pub pitch: f32,
    pub zoom: f32,
    pub shift: V3,
    pub ortho: bool,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { yaw: 0.6, pitch: 0.55, zoom: 1.0, shift: [0.0; 3], ortho: true }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Cut {
    pub normal: V3,
    pub offset: f32,
}

impl Cut {
    pub fn keeps(&self, p: V3) -> bool {
        dot(p, self.normal) <= self.offset
    }
}

pub struct View {
    pub dist: f32,
    pub focal: f32,
    pub eye: V3,
    pub target: V3,
    pub right: V3,
    pub up: V3,
    pub forward: V3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    pub key: V3,
    pub fill: V3,
}

pub fn view<M>(scene: &Scene<M>, cam: &Camera, size: Vec2) -> View {
    let (cy, sy, cp, sp) = (cam.yaw.cos(), cam.yaw.sin(), cam.pitch.cos(), cam.pitch.sin());
    let eye_dir = [sy * cp, -cy * cp, sp];
    let dist = scene.radius * DISTANCE;
    let forward = norm(scale(eye_dir, -1.0));
    let right = norm(cross(forward, [0.0, 0.0, 1.0]));
    let right = if dot(right, right) < 0.5 { [1.0, 0.0, 0.0] } else { right };
    let up = cross(right, forward);
    let focal = size.x.min(size.y) * 1.25 * cam.zoom;
    let target = add(scene.centre, cam.shift);
    let eye = add(target, scale(eye_dir, dist));
    let back = scale(forward, -1.0);
    View {
        dist,
        focal,
        eye,
        target,
        right,
        up,
        forward,
        fov_y: 2.0 * (size.y * 0.5 / focal).atan(),
        near: dist * 0.02,
        far: dist * 10.0,
        key: norm(add(add(scale(right, -0.35), scale(up, 0.55)), scale(back, 0.75))),
        fill: norm(add(add(scale(right, 0.55), scale(up, -0.25)), scale(back, 0.8))),
    }
}

impl Camera {
    pub fn panned<M>(self, scene: &Scene<M>, size: Vec2, delta: Vec2) -> Camera {
        let f = view(scene, &self, size);
        let per_pixel = f.dist / f.focal;
        let moved = add(scale(f.right, -delta.x * per_pixel), scale(f.up, delta.y * per_pixel));
        Camera { shift: add(self.shift, moved), ..self }
    }

    pub fn zoomed<M>(
        self,
        scene: &Scene<M>,
        rect: Rect,
        at: Pos2,
        factor: f32,
        anchor: Option<V3>,
    ) -> Camera {
        let zoom = (self.zoom * factor).clamp(0.1, 400.0);
        let before = view(scene, &self, rect.size());
        let after = Camera { zoom, ..self };
        let focal_after = view(scene, &after, rect.size()).focal;
        let depth = match (self.ortho, anchor) {
            (false, Some(p)) => dot(sub(p, before.eye), before.forward).max(before.dist * 0.02),
            _ => before.dist,
        };
        let change = depth / before.focal - depth / focal_after;
        let (dx, dy) = (at.x - rect.center().x, at.y - rect.center().y);
        let moved = add(scale(before.right, dx * change), scale(before.up, -dy * change));
        Camera { shift: add(self.shift, moved), ..after }
    }

    pub fn orbited<M>(
        self,
        scene: &Scene<M>,
        size: Vec2,
        yaw: f32,
        pitch: f32,
        pivot: V3,
    ) -> Camera {
        let before = view(scene, &self, size);
        let turned = Camera {
            yaw: self.yaw + yaw,
            pitch: (self.pitch + pitch).clamp(-1.5699, 1.5699),
            ..self
        };
        let after = view(scene, &turned, size);
        let offset = sub(before.target, pivot);
        let local =
            [dot(offset, before.right), dot(offset, before.up), dot(offset, before.forward)];
        let rotated = add(
            add(scale(after.right, local[0]), scale(after.up, local[1])),
            scale(after.forward, local[2]),
        );
        let target = add(pivot, rotated);
        Camera { shift: add(turned.shift, sub(target, after.target)), ..turned }
    }

    pub fn framed<M>(self, scene: &Scene<M>, centre: V3, span: f32) -> Camera {
        let dist = scene.radius * DISTANCE;
        Camera {
            shift: sub(centre, scene.centre),
            zoom: dist / (1.25 * span.max(1.0e-6) * 1.15),
            ..self
        }
    }
}

pub struct Projector {
    pub ortho: Option<f32>,
    pub eye: V3,
    pub right: V3,
    pub up: V3,
    pub forward: V3,
    pub focal: f32,
    pub centre: Pos2,
}

impl Projector {
    pub fn new<M>(scene: &Scene<M>, cam: &Camera, rect: Rect) -> Projector {
        let vw = view(scene, cam, rect.size());
        Projector {
            ortho: cam.ortho.then_some(vw.dist),
            eye: vw.eye,
            right: vw.right,
            up: vw.up,
            forward: vw.forward,
            focal: vw.focal,
            centre: rect.center(),
        }
    }

    pub fn project(&self, p: V3) -> Option<Pos2> {
        let d = sub(p, self.eye);
        let z = dot(d, self.forward);
        if let Some(dist) = self.ortho {
            return Some(Pos2::new(
                self.centre.x + dot(d, self.right) / dist * self.focal,
                self.centre.y - dot(d, self.up) / dist * self.focal,
            ));
        }
        (z > 1.0e-6).then(|| {
            Pos2::new(
                self.centre.x + dot(d, self.right) / z * self.focal,
                self.centre.y - dot(d, self.up) / z * self.focal,
            )
        })
    }

    pub fn ray(&self, at: Pos2) -> (V3, V3) {
        let (dx, dy) = ((at.x - self.centre.x) / self.focal, (at.y - self.centre.y) / self.focal);
        if let Some(dist) = self.ortho {
            let origin =
                add(add(self.eye, scale(self.right, dx * dist)), scale(self.up, -dy * dist));
            return (origin, self.forward);
        }
        let direction = norm(add(add(self.forward, scale(self.right, dx)), scale(self.up, -dy)));
        (self.eye, direction)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Hit {
    pub surface: usize,
    pub point: V3,
    pub distance: f32,
}

#[derive(Clone, Copy, Default)]
pub struct Orbit {
    pivot: Option<V3>,
}

#[derive(Clone)]
pub struct Viewer<M: Material = ()> {
    scene: Arc<Scene<M>>,
    camera: Camera,
    looks: Arc<[Look]>,
    cut: Option<Cut>,
    style: Style,
}

impl<M: Material> Viewer<M> {
    pub fn new(scene: Arc<Scene<M>>, camera: Camera) -> Viewer<M> {
        Viewer { scene, camera, looks: Arc::new([]), cut: None, style: Style::default() }
    }

    pub fn looks(mut self, looks: impl Into<Arc<[Look]>>) -> Viewer<M> {
        self.looks = looks.into();
        self
    }

    pub fn cut(mut self, cut: Option<Cut>) -> Viewer<M> {
        self.cut = cut;
        self
    }

    pub fn style(mut self, style: Style) -> Viewer<M> {
        self.style = style;
        self
    }

    pub fn camera(&self) -> Camera {
        self.camera
    }

    pub fn scene(&self) -> &Scene<M> {
        &self.scene
    }

    pub fn look(&self, group: usize) -> Look {
        self.looks.get(group).copied().unwrap_or_default()
    }

    pub fn projector(&self, rect: Rect) -> Projector {
        Projector::new(&self.scene, &self.camera, rect)
    }

    pub fn view(&self, size: Vec2) -> View {
        view(&self.scene, &self.camera, size)
    }

    pub fn pick(&self, rect: Rect, at: Pos2) -> Option<Hit> {
        let (origin, direction) = self.projector(rect).ray(at);
        let along = |d: f32| add(origin, scale(direction, d));
        let kept = |p: V3| self.cut.is_none_or(|c| c.keeps(p));
        self.scene
            .surfaces
            .iter()
            .enumerate()
            .filter(|(_, s)| self.look(s.group).visible())
            .flat_map(|(index, s)| {
                let look = self.look(s.group);
                let ghost = look.opacity < 1.0;
                s.positions.chunks_exact(3).filter_map(move |t| {
                    let corners = [look.place(t[0]), look.place(t[1]), look.place(t[2])];
                    hit_triangle(origin, direction, corners)
                        .filter(|d| kept(along(*d)))
                        .map(|d| (ghost, d, index))
                })
            })
            .min_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)))
            .map(|(_, distance, surface)| Hit { surface, point: along(distance), distance })
    }

    pub fn navigate(&mut self, ui: &Ui, response: &Response, orbit: &mut Orbit) -> bool {
        let before = self.camera;
        let rect = response.rect;
        let panning = ui.input(|i| i.modifiers.shift);
        if response.drag_started_by(egui::PointerButton::Primary) && !panning {
            orbit.pivot =
                response.interact_pointer_pos().and_then(|at| self.pick(rect, at)).map(|h| h.point);
        }
        if response.dragged_by(egui::PointerButton::Primary) && !panning {
            let d = response.drag_delta();
            let pivot = orbit.pivot.unwrap_or_else(|| self.view(rect.size()).target);
            self.camera =
                self.camera.orbited(&self.scene, rect.size(), -d.x * 0.01, d.y * 0.01, pivot);
        } else if response.dragged() {
            self.camera = self.camera.panned(&self.scene, rect.size(), response.drag_delta());
        }
        if response.hovered()
            && let Some(at) = response.hover_pos()
        {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (1.0 + scroll * 0.002);
            if (factor - 1.0).abs() > 1.0e-4 {
                let anchor =
                    if self.camera.ortho { None } else { self.pick(rect, at).map(|h| h.point) };
                self.camera = self.camera.zoomed(&self.scene, rect, at, factor, anchor);
            }
        }
        self.camera != before
    }

    pub fn paint(&self, ui: &Ui, rect: Rect) {
        gl::paint(ui, rect, self.clone());
    }

    pub fn render_soft(&self, width: usize, height: usize) -> ColorImage {
        soft::render(self, width, height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> Scene {
        Scene { centre: [1.0, 2.0, 3.0], radius: 10.0, ..Default::default() }
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(800.0, 600.0))
    }

    fn cameras() -> [Camera; 2] {
        [false, true].map(|ortho| Camera { ortho, ..Camera::default() })
    }

    #[test]
    fn picking_rays_land_where_points_project() {
        for cam in cameras() {
            let projector = Projector::new(&scene(), &cam, rect());
            let at = Pos2::new(250.0, 410.0);
            let (origin, direction) = projector.ray(at);
            let back = projector.project(add(origin, scale(direction, 23.0))).expect("in front");
            assert!(back.distance(at) < 1.0e-2, "ortho {}: {back:?}", cam.ortho);
        }
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        for cam in cameras() {
            let point = [4.0, -3.0, 6.0];
            let at = Projector::new(&scene(), &cam, rect()).project(point).expect("in front");
            for factor in [1.8, 0.6] {
                let zoomed = cam.zoomed(&scene(), rect(), at, factor, Some(point));
                let after =
                    Projector::new(&scene(), &zoomed, rect()).project(point).expect("in front");
                assert!(after.distance(at) < 1.0e-2, "ortho {}: {at:?} -> {after:?}", cam.ortho);
                assert!((zoomed.zoom - factor).abs() < 1.0e-6);
            }
        }
    }

    #[test]
    fn orbit_keeps_the_pivot_in_place() {
        for cam in cameras() {
            let pivot = [6.0, 1.0, -2.0];
            let at = Projector::new(&scene(), &cam, rect()).project(pivot).expect("in front");
            let turned = cam.orbited(&scene(), rect().size(), 0.4, -0.3, pivot);
            let after = Projector::new(&scene(), &turned, rect()).project(pivot).expect("in front");
            assert!(after.distance(at) < 1.0e-2, "ortho {}: {at:?} -> {after:?}", cam.ortho);
            assert!((turned.yaw - cam.yaw - 0.4).abs() < 1.0e-6);
        }
    }

    #[test]
    fn pan_moves_the_model_with_the_pointer() {
        for cam in cameras() {
            let point = scene().centre;
            let at = Projector::new(&scene(), &cam, rect()).project(point).expect("in front");
            let delta = Vec2::new(30.0, -12.0);
            let panned = cam.panned(&scene(), rect().size(), delta);
            let after = Projector::new(&scene(), &panned, rect()).project(point).expect("in front");
            assert!(after.distance(at + delta) < 1.0e-2, "ortho {}: {after:?}", cam.ortho);
        }
    }

    #[test]
    fn framing_fits_the_span_across_the_short_side() {
        for cam in cameras() {
            let centre = [5.0, -4.0, 0.0];
            let framed = cam.framed(&scene(), centre, 8.0);
            let projector = Projector::new(&scene(), &framed, rect());
            let a = projector.project(add(centre, scale(projector.right, -4.0))).unwrap();
            let b = projector.project(add(centre, scale(projector.right, 4.0))).unwrap();
            assert!(projector.project(centre).unwrap().distance(rect().center()) < 1.0e-2);
            if cam.ortho {
                assert!((b.x - a.x - 600.0 / 1.15).abs() < 1.0e-2, "{}", b.x - a.x);
            }
        }
    }

    #[test]
    fn a_cut_keeps_one_side() {
        let cut = Cut { normal: [0.0, -1.0, 0.0], offset: -2.0 };
        assert!(cut.keeps([0.0, 3.0, 0.0]) && !cut.keeps([0.0, 1.0, 0.0]));
    }

    fn square(group: usize, z: f32) -> Surface {
        let (a, b, c, d) = ([-1.0, -1.0, z], [1.0, -1.0, z], [1.0, 1.0, z], [-1.0, 1.0, z]);
        Surface {
            positions: vec![a, b, c, a, c, d],
            normals: vec![[0.0, 0.0, 1.0]; 6],
            colour: [0.5; 3],
            group,
            ..Default::default()
        }
    }

    #[test]
    fn picking_prefers_solid_parts_over_ghosts_in_front() {
        let scene = Scene {
            surfaces: vec![square(0, 1.0), square(1, 0.0)],
            radius: 2.0,
            ..Default::default()
        };
        let top = Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() };
        let viewer = Viewer::new(Arc::new(scene), top);
        assert_eq!(viewer.pick(rect(), rect().center()).map(|h| h.surface), Some(0));
        let ghost = Look { opacity: 0.4, ..Look::default() };
        let viewer = viewer.looks(vec![ghost, Look::default()]);
        let hit = viewer.pick(rect(), rect().center()).unwrap();
        assert_eq!(hit.surface, 1);
        assert!(hit.point[2].abs() < 1.0e-4, "{hit:?}");
        let viewer = viewer.looks(vec![Look::HIDDEN, Look::HIDDEN]);
        assert!(viewer.pick(rect(), rect().center()).is_none());
    }

    #[test]
    fn soft_render_draws_a_visible_surface_and_skips_a_hidden_one() {
        let scene =
            Arc::new(Scene { surfaces: vec![square(0, 0.0)], radius: 2.0, ..Default::default() });
        let top = Camera { pitch: 1.5699, yaw: 0.0, ..Camera::default() };
        let lit = Viewer::new(scene.clone(), top).render_soft(64, 48);
        assert_ne!(lit.pixels[24 * 64 + 32], BACKGROUND);
        assert_eq!(lit.pixels[0], BACKGROUND);
        let hidden = Viewer::new(scene, top).looks(vec![Look::HIDDEN]).render_soft(64, 48);
        assert!(hidden.pixels.iter().all(|p| *p == BACKGROUND));
    }
}
