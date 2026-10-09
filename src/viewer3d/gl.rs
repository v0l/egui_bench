use super::{Look, Material, Scene, Shading, Style, Viewer, rgb};
use egui::{Rect, Ui};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use three_d::{
    AxisAlignedBoundingBox, Blend, ClearState, ColorTexture, Context, CpuMesh, CpuTexture, Cull,
    DepthTexture, Effect, EffectMaterialId, Geometry, GeometryId, Gm, Interpolation, Light, Mat4,
    MaterialType, Mesh, Mipmap, Object, Positions, Program, RenderStates, RenderTarget, ScissorBox,
    SquareMatrix, Srgba, Texture2D, TextureData, Vec3, Vec4, VertexBuffer, Viewport, Wrapping,
    WriteMask, radians, render_with_effect, render_with_material, vec2, vec3, vec4,
};

const LIFT: f32 = 2.0;

struct Shaded {
    texture: Option<Arc<Texture2D>>,
    shading: Shading,
    eye: Vec3,
    key: Vec3,
    fill: Vec3,
    lighting: Vec3,
    cap: Vec3,
    clip: Vec4,
    clipping: f32,
    opacity: f32,
    tint: Vec4,
}

impl three_d::Material for Shaded {
    fn id(&self) -> EffectMaterialId {
        EffectMaterialId(0x7c10 | self.texture.is_some() as u16)
    }

    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        let mut source = String::new();
        if self.texture.is_some() {
            source.push_str("#define USE_TEXTURE\nin vec2 uvs;\nuniform sampler2D tex;\n");
        }
        source.push_str(
            r#"
uniform vec3 eye;
uniform vec3 keyDir;
uniform vec3 fillDir;
uniform vec3 lighting;
uniform vec3 capColour;
uniform vec3 gloss;
uniform vec4 clipPlane;
uniform float clipping;
uniform float capped;
uniform float opacity;
uniform vec4 tint;
in vec3 pos;
in vec3 nor;
in vec4 col;
layout (location = 0) out vec4 outColor;

vec3 to_linear(vec3 c) {
    return mix(c / 12.92, pow((c + 0.055) / 1.055, vec3(2.4)), step(vec3(0.04045), c));
}

vec3 to_srgb(vec3 c) {
    c = clamp(c, 0.0, 1.0);
    return mix(c * 12.92, 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055, step(vec3(0.0031308), c));
}

void main() {
    if (clipping > 0.5 && dot(pos, clipPlane.xyz) > clipPlane.w) discard;
    vec3 albedo = col.rgb;
#ifdef USE_TEXTURE
    albedo = to_linear(texture(tex, uvs).rgb);
#endif
    albedo = mix(albedo, tint.rgb, tint.a);
    vec3 n = normalize(nor);
    vec3 v = normalize(eye - pos);
    if (clipping > 0.5 && capped > 0.5 && opacity > 0.999 && !gl_FrontFacing) {
        vec3 c = normalize(clipPlane.xyz);
        float d = lighting.x + lighting.y * abs(dot(c, keyDir)) + lighting.z * abs(dot(c, fillDir));
        outColor = vec4(to_srgb(to_linear(capColour) * d), 1.0);
        return;
    }
    if (dot(n, v) < 0.0) n = -n;
    float d = lighting.x + lighting.y * max(dot(n, keyDir), 0.0) + lighting.z * max(dot(n, fillDir), 0.0);
    vec3 h = normalize(keyDir + v);
    float sp = gloss.x * pow(max(dot(n, h), 0.0), gloss.y);
    vec3 shine = vec3(1.0) + (albedo * 1.6 - vec3(1.0)) * gloss.z;
    outColor = vec4(to_srgb(albedo * d + sp * shine), opacity);
}
"#,
        );
        source
    }

    fn use_uniforms(&self, program: &Program, _viewer: &dyn three_d::Viewer, _: &[&dyn Light]) {
        let s = self.shading;
        program.use_uniform_if_required("eye", self.eye);
        program.use_uniform_if_required("keyDir", self.key);
        program.use_uniform_if_required("fillDir", self.fill);
        program.use_uniform_if_required("lighting", self.lighting);
        program.use_uniform_if_required("capColour", self.cap);
        program.use_uniform_if_required("gloss", vec3(s.gloss, s.sharpness, s.metal));
        program.use_uniform_if_required("clipPlane", self.clip);
        program.use_uniform_if_required("clipping", self.clipping);
        program.use_uniform_if_required("capped", if s.capped { 1.0f32 } else { 0.0 });
        program.use_uniform_if_required("opacity", self.opacity);
        program.use_uniform_if_required("tint", self.tint);
        if let Some(texture) = &self.texture {
            program.use_texture("tex", texture);
        }
    }

    fn render_states(&self) -> RenderStates {
        see_through(self.opacity)
    }

    fn material_type(&self) -> MaterialType {
        kind(self.opacity)
    }
}

fn see_through(opacity: f32) -> RenderStates {
    if opacity < 1.0 {
        RenderStates {
            cull: Cull::None,
            blend: Blend::TRANSPARENCY,
            write_mask: WriteMask::COLOR,
            ..Default::default()
        }
    } else {
        RenderStates { cull: Cull::None, ..Default::default() }
    }
}

fn kind(opacity: f32) -> MaterialType {
    if opacity < 1.0 { MaterialType::Transparent } else { MaterialType::Opaque }
}

const LINE_VERTEX: &str = r#"
uniform mat4 viewMatrix;
uniform mat4 projectionMatrix;
uniform mat4 modelMatrix;
uniform vec2 viewportSize;
uniform float lineWidth;
uniform float lift;
in vec3 start;
in vec3 end;
in vec2 corner;
out vec3 pos;

vec4 lifted(vec3 p) {
    vec4 v = viewMatrix * modelMatrix * vec4(p, 1.0);
    bool perspective = projectionMatrix[2][3] != 0.0;
    float pixel = 2.0 / (projectionMatrix[1][1] * viewportSize.y);
    if (perspective) {
        float range = length(v.xyz);
        v.xyz *= max(range - lift * pixel * -v.z, 0.0) / range;
    } else {
        v.z += lift * pixel;
    }
    return projectionMatrix * v;
}

void main() {
    vec4 a = lifted(start);
    vec4 b = lifted(end);
    bool first = corner.x < 0.5;
    vec4 here = first ? a : b;
    vec2 along = (b.xy / b.w - a.xy / a.w) * viewportSize;
    float length_px = length(along);
    vec2 dir = length_px > 1.0e-6 ? along / length_px : vec2(1.0, 0.0);
    vec2 across = vec2(-dir.y, dir.x);
    vec2 offset = (across * corner.y + dir * (first ? -0.5 : 0.5)) * lineWidth / viewportSize;
    gl_Position = here + vec4(offset * here.w, 0.0, 0.0);
    vec4 world = modelMatrix * vec4(first ? start : end, 1.0);
    pos = world.xyz / world.w;
}
"#;

struct EdgeLines {
    context: Context,
    start: VertexBuffer<Vec3>,
    end: VertexBuffer<Vec3>,
    corner: VertexBuffer<three_d::Vec2>,
    count: u32,
    pixels: f32,
    transformation: Mat4,
    aabb: AxisAlignedBoundingBox,
}

impl EdgeLines {
    fn new(context: &Context, segments: &[[[f32; 3]; 2]]) -> EdgeLines {
        let corners = [(0.0, -1.0), (0.0, 1.0), (1.0, 1.0), (0.0, -1.0), (1.0, 1.0), (1.0, -1.0)];
        let mut start = Vec::with_capacity(segments.len() * 6);
        let mut end = Vec::with_capacity(segments.len() * 6);
        let mut corner = Vec::with_capacity(segments.len() * 6);
        for [a, b] in segments {
            for (t, side) in corners {
                start.push(v3(*a));
                end.push(v3(*b));
                corner.push(vec2(t, side));
            }
        }
        let aabb = AxisAlignedBoundingBox::new_with_positions(&start);
        EdgeLines {
            context: context.clone(),
            start: VertexBuffer::new_with_data(context, &start),
            end: VertexBuffer::new_with_data(context, &end),
            corner: VertexBuffer::new_with_data(context, &corner),
            count: start.len() as u32,
            pixels: 1.0,
            transformation: Mat4::identity(),
            aabb,
        }
    }
}

impl Geometry for EdgeLines {
    fn draw(&self, viewer: &dyn three_d::Viewer, program: &Program, render_states: RenderStates) {
        let viewport = viewer.viewport();
        program.use_uniform("viewMatrix", viewer.view());
        program.use_uniform("projectionMatrix", viewer.projection());
        program.use_uniform("lift", self.pixels * LIFT);
        program.use_uniform("modelMatrix", self.transformation);
        program.use_uniform("viewportSize", vec2(viewport.width as f32, viewport.height as f32));
        program.use_uniform("lineWidth", self.pixels);
        program.use_vertex_attribute("start", &self.start);
        program.use_vertex_attribute("end", &self.end);
        program.use_vertex_attribute("corner", &self.corner);
        program.draw_arrays(render_states, viewport, self.count);
    }

    fn vertex_shader_source(&self) -> String {
        LINE_VERTEX.to_string()
    }

    fn id(&self) -> GeometryId {
        GeometryId(0x7d01)
    }

    fn render_with_material(
        &self,
        material: &dyn three_d::Material,
        viewer: &dyn three_d::Viewer,
        lights: &[&dyn Light],
    ) {
        if let Err(error) = render_with_material(&self.context, viewer, self, material, lights) {
            panic!("{error}");
        }
    }

    fn render_with_effect(
        &self,
        effect: &dyn Effect,
        viewer: &dyn three_d::Viewer,
        lights: &[&dyn Light],
        color_texture: Option<ColorTexture>,
        depth_texture: Option<DepthTexture>,
    ) {
        if let Err(error) = render_with_effect(
            &self.context,
            viewer,
            self,
            effect,
            lights,
            color_texture,
            depth_texture,
        ) {
            panic!("{error}");
        }
    }

    fn aabb(&self) -> AxisAlignedBoundingBox {
        self.aabb.transformed(self.transformation)
    }
}

struct Flat {
    colour: Vec3,
    clip: Vec4,
    clipping: f32,
    opacity: f32,
}

impl three_d::Material for Flat {
    fn id(&self) -> EffectMaterialId {
        EffectMaterialId(0x7c20)
    }

    fn fragment_shader_source(&self, _lights: &[&dyn Light]) -> String {
        r#"
uniform vec3 lineColour;
uniform vec4 clipPlane;
uniform float clipping;
uniform float opacity;
in vec3 pos;
layout (location = 0) out vec4 outColor;

void main() {
    if (clipping > 0.5 && dot(pos, clipPlane.xyz) > clipPlane.w) discard;
    outColor = vec4(lineColour, opacity);
}
"#
        .to_string()
    }

    fn use_uniforms(&self, program: &Program, _viewer: &dyn three_d::Viewer, _: &[&dyn Light]) {
        program.use_uniform_if_required("lineColour", self.colour);
        program.use_uniform_if_required("clipPlane", self.clip);
        program.use_uniform_if_required("clipping", self.clipping);
        program.use_uniform_if_required("opacity", self.opacity);
    }

    fn render_states(&self) -> RenderStates {
        see_through(self.opacity)
    }

    fn material_type(&self) -> MaterialType {
        kind(self.opacity)
    }
}

struct Batch {
    group: usize,
    gm: Gm<Mesh, Shaded>,
}

struct Stroke {
    group: usize,
    colour: Vec3,
    width: f32,
    tinted: bool,
    gm: Gm<EdgeLines, Flat>,
}

struct Uploaded {
    pass: u64,
    batches: Vec<Batch>,
    strokes: Vec<Stroke>,
}

struct Gpu {
    context: Context,
    scenes: HashMap<u64, Uploaded>,
}

fn v3(v: [f32; 3]) -> Vec3 {
    vec3(v[0], v[1], v[2])
}

fn texture(context: &Context, image: &super::Image) -> Arc<Texture2D> {
    let cpu = CpuTexture {
        data: TextureData::RgbaU8(
            image.rgba.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        ),
        width: image.width as u32,
        height: image.height as u32,
        min_filter: Interpolation::Linear,
        mag_filter: Interpolation::Linear,
        mipmap: Some(Mipmap { filter: Interpolation::Linear, max_ratio: 8, max_levels: 12 }),
        wrap_s: Wrapping::ClampToEdge,
        wrap_t: Wrapping::ClampToEdge,
        ..Default::default()
    };
    Arc::new(Texture2D::new(context, &cpu))
}

fn upload<M: Material>(context: &Context, scene: &Scene<M>) -> Uploaded {
    let textures: Vec<Arc<Texture2D>> =
        scene.images.iter().map(|image| texture(context, image)).collect();
    let mut keys: Vec<(usize, M, Option<usize>)> = Vec::new();
    let mut meshes: Vec<CpuMesh> = Vec::new();
    for s in scene.surfaces.iter().filter(|s| !s.positions.is_empty()) {
        let key = (s.group, s.material, s.texture.filter(|t| *t < textures.len()));
        let at = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
            keys.push(key);
            meshes.push(CpuMesh {
                positions: Positions::F32(Vec::new()),
                normals: Some(Vec::new()),
                colors: Some(Vec::new()),
                uvs: key.2.map(|_| Vec::new()),
                ..Default::default()
            });
            keys.len() - 1
        });
        let cpu = &mut meshes[at];
        if let Positions::F32(positions) = &mut cpu.positions {
            positions.extend(s.positions.iter().copied().map(v3));
        }
        if let Some(normals) = &mut cpu.normals {
            normals.extend(s.normals.iter().copied().map(v3));
        }
        let [r, g, b] = s.colour.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        if let Some(colors) = &mut cpu.colors {
            colors.extend(std::iter::repeat_n(Srgba::new(r, g, b, 255), s.positions.len()));
        }
        if let Some(uvs) = &mut cpu.uvs {
            uvs.extend((0..s.positions.len()).map(|i| {
                let [u, v] = s.uvs.get(i).copied().unwrap_or_default();
                vec2(u, v)
            }));
        }
    }
    let batches = keys
        .into_iter()
        .zip(meshes)
        .map(|((group, material, tex), cpu)| {
            let shaded = Shaded {
                texture: tex.map(|t| textures[t].clone()),
                shading: material.shading(),
                eye: vec3(0.0, 0.0, 1.0),
                key: vec3(0.0, 0.0, 1.0),
                fill: vec3(0.0, 0.0, 1.0),
                lighting: vec3(0.3, 0.8, 0.25),
                cap: vec3(1.0, 0.5, 0.0),
                clip: vec4(0.0, 0.0, 1.0, 0.0),
                clipping: 0.0,
                opacity: 1.0,
                tint: vec4(0.0, 0.0, 0.0, 0.0),
            };
            Batch { group, gm: Gm::new(Mesh::new(context, &cpu), shaded) }
        })
        .collect();
    let strokes = scene
        .lines
        .iter()
        .filter(|l| !l.segments.is_empty())
        .map(|l| Stroke {
            group: l.group,
            colour: v3(l.colour),
            width: l.width,
            tinted: l.material.shading().tinted,
            gm: Gm::new(
                EdgeLines::new(context, &l.segments),
                Flat {
                    colour: v3(l.colour),
                    clip: vec4(0.0, 0.0, 1.0, 0.0),
                    clipping: 0.0,
                    opacity: 1.0,
                },
            ),
        })
        .collect();
    Uploaded { pass: 0, batches, strokes }
}

fn matrix(look: &Look) -> Mat4 {
    let m = look.placement;
    Mat4::new(
        m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8], m[9], m[10], m[11], m[12], m[13],
        m[14], m[15],
    )
}

thread_local! {
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

static DRAW_MICROS: AtomicU64 = AtomicU64::new(0);

pub fn draw_time() -> Duration {
    Duration::from_micros(DRAW_MICROS.load(Ordering::Relaxed))
}

fn update(uploaded: &mut Uploaded, viewer: &Viewer<impl Material>, vw: &super::View, ppp: f32) {
    let style: Style = viewer.style;
    let clip = viewer.cut.map(|c| vec4(c.normal[0], c.normal[1], c.normal[2], c.offset));
    let selected = v3(rgb(style.selected));
    for batch in &mut uploaded.batches {
        let look = viewer.look(batch.group);
        let m = &mut batch.gm.material;
        m.eye = v3(vw.eye);
        m.key = v3(vw.key);
        m.fill = v3(vw.fill);
        m.lighting = vec3(style.ambient, style.key, style.fill);
        m.cap = v3(rgb(style.cap));
        m.clipping = if clip.is_some() { 1.0 } else { 0.0 };
        m.clip = clip.unwrap_or(m.clip);
        m.opacity = look.opacity.min(1.0);
        m.tint = if look.selected && m.shading.tinted {
            selected.extend(style.selected_tint)
        } else {
            vec4(0.0, 0.0, 0.0, 0.0)
        };
        batch.gm.set_transformation(matrix(&look));
    }
    for stroke in &mut uploaded.strokes {
        let look = viewer.look(stroke.group);
        let m = &mut stroke.gm.material;
        m.clipping = if clip.is_some() { 1.0 } else { 0.0 };
        m.clip = clip.unwrap_or(m.clip);
        m.opacity = (look.opacity * 1.5).min(1.0);
        m.colour = if look.selected && stroke.tinted { selected } else { stroke.colour };
        stroke.gm.geometry.pixels = stroke.width * ppp;
        stroke.gm.geometry.transformation = matrix(&look);
    }
}

pub(super) fn paint<M: Material>(ui: &Ui, rect: Rect, viewer: Viewer<M>) {
    let pass = ui.ctx().cumulative_pass_nr();
    let callback = egui_glow::CallbackFn::new(move |info, painter| {
        let started = Instant::now();
        GPU.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                let Ok(context) = three_d::Context::from_gl_context(painter.gl().clone()) else {
                    return;
                };
                *slot = Some(Gpu { context, scenes: HashMap::new() });
            }
            let Some(gpu) = slot.as_mut() else { return };
            let scene = &viewer.scene;
            gpu.scenes.retain(|id, u| *id == scene.id || u.pass + 1 >= pass);
            let context = gpu.context.clone();
            let uploaded = gpu.scenes.entry(scene.id).or_insert_with(|| upload(&context, scene));
            uploaded.pass = pass;
            let vp = info.viewport_in_pixels();
            let clip = info.clip_rect_in_pixels();
            let vw = viewer.view(egui::Vec2::new(vp.width_px as f32, vp.height_px as f32));
            update(uploaded, &viewer, &vw, info.pixels_per_point);
            let viewport = Viewport {
                x: vp.left_px,
                y: vp.from_bottom_px,
                width: vp.width_px.max(1) as u32,
                height: vp.height_px.max(1) as u32,
            };
            let camera = if viewer.camera.ortho {
                three_d::Camera::new_orthographic(
                    viewport,
                    v3(vw.eye),
                    v3(vw.target),
                    v3(vw.up),
                    vp.height_px as f32 / vw.focal,
                    vw.near,
                    vw.far,
                )
            } else {
                three_d::Camera::new_perspective(
                    viewport,
                    v3(vw.eye),
                    v3(vw.target),
                    v3(vw.up),
                    radians(vw.fov_y),
                    vw.near,
                    vw.far,
                )
            };
            let x0 = clip.left_px.max(vp.left_px);
            let y0 = clip.from_bottom_px.max(vp.from_bottom_px);
            let x1 = (clip.left_px + clip.width_px).min(vp.left_px + vp.width_px);
            let y1 = (clip.from_bottom_px + clip.height_px).min(vp.from_bottom_px + vp.height_px);
            if x1 <= x0 || y1 <= y0 {
                return;
            }
            let scissor =
                ScissorBox { x: x0, y: y0, width: (x1 - x0) as u32, height: (y1 - y0) as u32 };
            let [w, h] = info.screen_size_px;
            let target = match painter.intermediate_fbo() {
                Some(fbo) => RenderTarget::from_framebuffer(&context, w, h, fbo),
                None => RenderTarget::screen(&context, w, h),
            };
            let [r, g, b] = rgb(viewer.style.background);
            target.clear_partially(scissor, ClearState::color_and_depth(r, g, b, 1.0, 1.0));
            let visible = |group: usize| viewer.look(group).visible();
            let shown: Vec<&dyn Object> = uploaded
                .batches
                .iter()
                .filter(|b| visible(b.group))
                .map(|b| &b.gm as &dyn Object)
                .chain(
                    uploaded
                        .strokes
                        .iter()
                        .filter(|s| visible(s.group))
                        .map(|s| &s.gm as &dyn Object),
                )
                .collect();
            target.render_partially(scissor, &camera, shown, &[]);
            let _ = target.into_framebuffer();
        });
        DRAW_MICROS.store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    });
    ui.painter_at(rect).add(egui::PaintCallback { rect, callback: Arc::new(callback) });
}
