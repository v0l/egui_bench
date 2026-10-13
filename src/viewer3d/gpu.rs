use super::{Image, Look, Material, Scene, Style, V3, View, Viewer, dot, rgb, sub};
use bytemuck::{Pod, Zeroable};
use egui::{Color32, Rect, TextureHandle, TextureOptions, Ui};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use web_time::{Duration, Instant};

const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const SAMPLES: u32 = 4;
const STRIDE: u64 = 256;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    key: [f32; 4],
    fill: [f32; 4],
    lighting: [f32; 4],
    cap: [f32; 4],
    clip: [f32; 4],
    viewport: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DrawUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    tint: [f32; 4],
    gloss: [f32; 4],
    params: [f32; 4],
    colour: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    colour: [f32; 3],
    uv: [f32; 2],
}

type Aabb = (V3, V3);

struct Batch {
    group: usize,
    shading: super::Shading,
    texture: Option<usize>,
    vertices: wgpu::Buffer,
    count: u32,
    aabb: Aabb,
}

struct Stroke {
    group: usize,
    colour: V3,
    width: f32,
    tinted: bool,
    segments: wgpu::Buffer,
    count: u32,
    aabb: Aabb,
}

struct Uploaded {
    used: u64,
    textures: Vec<wgpu::BindGroup>,
    batches: Vec<Batch>,
    strokes: Vec<Stroke>,
}

struct Slot {
    used: u64,
    size: [u32; 2],
    resolved: wgpu::TextureView,
    multisampled: wgpu::TextureView,
    depth: wgpu::TextureView,
    texture: egui::TextureId,
    frame: wgpu::Buffer,
    frame_group: wgpu::BindGroup,
    draws: wgpu::Buffer,
    draw_group: wgpu::BindGroup,
    capacity: u64,
}

struct Pipelines {
    frame_layout: wgpu::BindGroupLayout,
    draw_layout: wgpu::BindGroupLayout,
    texture_layout: wgpu::BindGroupLayout,
    surface_layout: wgpu::PipelineLayout,
    line_layout: wgpu::PipelineLayout,
    module: wgpu::ShaderModule,
    surfaces: HashMap<(bool, i32), wgpu::RenderPipeline>,
    lines: HashMap<bool, wgpu::RenderPipeline>,
}

struct State {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    anisotropic: bool,
    pipelines: Pipelines,
    blank: wgpu::BindGroup,
    scenes: HashMap<u64, Uploaded>,
    slots: Vec<Slot>,
    pass: u64,
    painted: u64,
    uploads: u64,
    next: usize,
}

struct Soft {
    pass: u64,
    next: usize,
    handles: Vec<TextureHandle>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static SOFT: RefCell<Soft> = const { RefCell::new(Soft { pass: 0, next: 0, handles: Vec::new() }) };
}

static DRAW_MICROS: AtomicU64 = AtomicU64::new(0);

pub fn draw_time() -> Duration {
    Duration::from_micros(DRAW_MICROS.load(Ordering::Relaxed))
}

pub fn install(render_state: &egui_wgpu::RenderState) {
    let device = render_state.device.clone();
    let anisotropic = render_state
        .adapter
        .get_downlevel_capabilities()
        .flags
        .contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING);
    let pipelines = Pipelines::new(&device);
    let blank = texture_group(
        &device,
        &render_state.queue,
        &pipelines.texture_layout,
        &Image { width: 1, height: 1, rgba: vec![255; 4] },
        anisotropic,
    );
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            queue: render_state.queue.clone(),
            renderer: render_state.renderer.clone(),
            anisotropic,
            pipelines,
            blank,
            scenes: HashMap::new(),
            slots: Vec::new(),
            pass: 0,
            painted: 0,
            uploads: 0,
            next: 0,
            device,
        });
    });
}

pub fn installed() -> bool {
    STATE.with(|cell| cell.borrow().is_some())
}

fn uniform_entry(binding: u32, dynamic: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: None,
        },
        count: None,
    }
}

impl Pipelines {
    fn new(device: &wgpu::Device) -> Pipelines {
        let frame_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d frame"),
            entries: &[uniform_entry(0, false)],
        });
        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d draw"),
            entries: &[uniform_entry(0, true)],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let surface_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer3d surfaces"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer3d lines"),
            bind_group_layouts: &[Some(&frame_layout), Some(&draw_layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer3d"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        Pipelines {
            frame_layout,
            draw_layout,
            texture_layout,
            surface_layout,
            line_layout,
            module,
            surfaces: HashMap::new(),
            lines: HashMap::new(),
        }
    }

    fn surface(
        &mut self,
        device: &wgpu::Device,
        see_through: bool,
        bias: i32,
    ) -> &wgpu::RenderPipeline {
        let (layout, module) = (&self.surface_layout, &self.module);
        self.surfaces.entry((see_through, bias)).or_insert_with(|| {
            let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x2];
            pipeline(
                device,
                layout,
                module,
                ("surface_vertex", "surface_fragment"),
                wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attributes,
                },
                see_through,
                wgpu::DepthBiasState { constant: 1, slope_scale: bias as f32 / 100.0, clamp: 0.0 },
            )
        })
    }

    fn line(&mut self, device: &wgpu::Device, see_through: bool) -> &wgpu::RenderPipeline {
        let (layout, module) = (&self.line_layout, &self.module);
        self.lines.entry(see_through).or_insert_with(|| {
            let attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
            pipeline(
                device,
                layout,
                module,
                ("line_vertex", "line_fragment"),
                wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Instance,
                    attributes: &attributes,
                },
                see_through,
                wgpu::DepthBiasState::default(),
            )
        })
    }
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    (vertex, fragment): (&str, &str),
    buffer: wgpu::VertexBufferLayout,
    see_through: bool,
    bias: wgpu::DepthBiasState,
) -> wgpu::RenderPipeline {
    let blend = see_through.then_some(wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        },
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(vertex),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[buffer],
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOUR,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(!see_through),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias,
        }),
        multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
        multiview_mask: None,
        cache: None,
    })
}

fn mip_levels(image: &Image) -> Vec<(u32, u32, Vec<u8>)> {
    let mut levels = vec![(image.width as u32, image.height as u32, image.rgba.clone())];
    while levels.len() < 12 {
        let (w, h, data) = levels.last().expect("first level");
        if *w == 1 && *h == 1 {
            break;
        }
        let (nw, nh) = ((*w / 2).max(1), (*h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let sx = (x * 2 + dx).min(w - 1);
                        let sy = (y * 2 + dy).min(h - 1);
                        sum += data[((sy * w + sx) * 4 + k) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + k) as usize] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push((nw, nh, next));
    }
    levels
}

fn texture_group(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    image: &Image,
    anisotropic: bool,
) -> wgpu::BindGroup {
    let image = if image.width == 0
        || image.height == 0
        || image.rgba.len() < image.width * image.height * 4
    {
        &Image { width: 1, height: 1, rgba: vec![255; 4] }
    } else {
        image
    };
    let levels = mip_levels(image);
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("viewer3d image"),
        size: wgpu::Extent3d {
            width: image.width as u32,
            height: image.height as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOUR,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, (w, h, data)) in levels.iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(*h),
            },
            wgpu::Extent3d { width: *w, height: *h, depth_or_array_layers: 1 },
        );
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("viewer3d image"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: if anisotropic { 8 } else { 1 },
        ..Default::default()
    });
    let view = texture.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("viewer3d image"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) },
        ],
    })
}

fn to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn grow(aabb: &mut Aabb, p: V3) {
    aabb.0 = std::array::from_fn(|k| aabb.0[k].min(p[k]));
    aabb.1 = std::array::from_fn(|k| aabb.1[k].max(p[k]));
}

const EMPTY: Aabb = ([f32::MAX; 3], [f32::MIN; 3]);

fn vertex_buffer(state: &State, label: &str, bytes: &[u8]) -> wgpu::Buffer {
    let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (bytes.len() as u64).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT).max(4),
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    state.queue.write_buffer(&buffer, 0, bytes);
    buffer
}

fn upload<M: Material>(state: &State, scene: &Scene<M>) -> Uploaded {
    let device = &state.device;
    let textures: Vec<wgpu::BindGroup> = scene
        .images
        .iter()
        .map(|image| {
            texture_group(
                device,
                &state.queue,
                &state.pipelines.texture_layout,
                image,
                state.anisotropic,
            )
        })
        .collect();
    let mut keys: Vec<(usize, M, Option<usize>)> = Vec::new();
    let mut meshes: Vec<(Vec<Vertex>, Aabb)> = Vec::new();
    for s in scene.surfaces.iter().filter(|s| !s.positions.is_empty()) {
        let key = (s.group, s.material, s.texture.filter(|t| *t < textures.len()));
        let at = keys.iter().position(|k| *k == key).unwrap_or_else(|| {
            keys.push(key);
            meshes.push((Vec::new(), EMPTY));
            keys.len() - 1
        });
        let colour = s.colour.map(|c| to_linear((c.clamp(0.0, 1.0) * 255.0).round() / 255.0));
        let (vertices, aabb) = &mut meshes[at];
        for (i, p) in s.positions.iter().enumerate() {
            grow(aabb, *p);
            vertices.push(Vertex {
                position: *p,
                normal: s.normals.get(i).copied().unwrap_or([0.0, 0.0, 1.0]),
                colour,
                uv: s.uvs.get(i).copied().unwrap_or_default(),
            });
        }
    }
    let batches = keys
        .into_iter()
        .zip(meshes)
        .map(|((group, material, texture), (vertices, aabb))| Batch {
            group,
            shading: material.shading(),
            texture,
            count: vertices.len() as u32,
            vertices: vertex_buffer(state, "viewer3d surface", bytemuck::cast_slice(&vertices)),
            aabb,
        })
        .collect();
    let strokes = scene
        .lines
        .iter()
        .filter(|l| !l.segments.is_empty())
        .map(|l| {
            let mut aabb = EMPTY;
            for [a, b] in &l.segments {
                grow(&mut aabb, *a);
                grow(&mut aabb, *b);
            }
            Stroke {
                group: l.group,
                colour: l.colour,
                width: l.width,
                tinted: l.material.shading().tinted,
                count: l.segments.len() as u32,
                segments: vertex_buffer(state, "viewer3d lines", bytemuck::cast_slice(&l.segments)),
                aabb,
            }
        })
        .collect();
    Uploaded { used: 0, textures, batches, strokes }
}

fn columns(m: &[f32; 16]) -> [[f32; 4]; 4] {
    [
        [m[0], m[1], m[2], m[3]],
        [m[4], m[5], m[6], m[7]],
        [m[8], m[9], m[10], m[11]],
        [m[12], m[13], m[14], m[15]],
    ]
}

fn normal_matrix(m: &[f32; 16]) -> [[f32; 4]; 4] {
    let a = [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]];
    let cof = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]
    };
    let det = a[0][0] * cof(0, 0) + a[0][1] * cof(0, 1) + a[0][2] * cof(0, 2);
    let inv = if det.abs() > 1.0e-20 { 1.0 / det } else { 1.0 };
    let n = |r: usize, c: usize| cof(r, c) * inv;
    [
        [n(0, 0), n(1, 0), n(2, 0), 0.0],
        [n(0, 1), n(1, 1), n(2, 1), 0.0],
        [n(0, 2), n(1, 2), n(2, 2), 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn placed(aabb: &Aabb, look: &Look) -> [V3; 8] {
    let (lo, hi) = aabb;
    std::array::from_fn(|k| {
        look.place([
            if k & 1 == 0 { lo[0] } else { hi[0] },
            if k & 2 == 0 { lo[1] } else { hi[1] },
            if k & 4 == 0 { lo[2] } else { hi[2] },
        ])
    })
}

fn depth_range(corners: &[[V3; 8]], vw: &View, ortho: bool) -> (f32, f32) {
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for c in corners.iter().flatten() {
        let depth = dot(sub(*c, vw.eye), vw.forward);
        lo = lo.min(depth);
        hi = hi.max(depth);
    }
    if hi < lo || (!ortho && hi <= 0.0) {
        return (vw.near, vw.far);
    }
    let pad = (hi - lo).max(vw.dist * 1.0e-3) * 0.01;
    let (near, far) = (lo - pad, hi + pad);
    if ortho { (near, far) } else { (near.max(far * 1.0e-3), far) }
}

fn view_projection(vw: &View, size: [u32; 2], ortho: bool, near: f32, far: f32) -> [[f32; 4]; 4] {
    let (w, h) = (size[0] as f32, size[1] as f32);
    let row = |axis: V3| [axis[0], axis[1], axis[2], -dot(axis, vw.eye)];
    let (x, y, d) = (row(vw.right), row(vw.up), row(vw.forward));
    let one = [0.0, 0.0, 0.0, 1.0];
    let scaled = |r: [f32; 4], k: f32| r.map(|v| v * k);
    let span = (far - near).max(1.0e-12);
    let rows = if ortho {
        let half_h = h * 0.5 * vw.dist / vw.focal;
        let half_w = half_h * w / h;
        let z = std::array::from_fn(|i| (d[i] - near * one[i]) / span);
        [scaled(x, 1.0 / half_w), scaled(y, 1.0 / half_h), z, one]
    } else {
        let t = (vw.fov_y * 0.5).tan();
        let z = std::array::from_fn(|i| far / span * d[i] - far * near / span * one[i]);
        [scaled(x, 1.0 / (t * w / h)), scaled(y, 1.0 / t), z, d]
    };
    std::array::from_fn(|c| std::array::from_fn(|r| rows[r][c]))
}

impl State {
    fn begin(&mut self, pass: u64) {
        if pass == self.pass {
            return;
        }
        self.pass = pass;
        self.painted += 1;
        self.next = 0;
        let painted = self.painted;
        self.scenes.retain(|_, u| u.used + 1 >= painted);
        let mut renderer = self.renderer.write();
        self.slots.retain(|s| {
            let keep = s.used + 1 >= painted;
            if !keep {
                renderer.free_texture(&s.texture);
            }
            keep
        });
    }

    fn slot(&mut self, size: [u32; 2], draws: u64) -> usize {
        let index = self.next;
        self.next += 1;
        let fits = self.slots.get(index).is_some_and(|s| s.size == size);
        if !fits {
            let (resolved, multisampled, depth) = self.targets(size);
            let mut renderer = self.renderer.write();
            match self.slots.get_mut(index) {
                Some(slot) => {
                    renderer.update_egui_texture_from_wgpu_texture(
                        &self.device,
                        &resolved,
                        wgpu::FilterMode::Nearest,
                        slot.texture,
                    );
                    slot.size = size;
                    slot.resolved = resolved;
                    slot.multisampled = multisampled;
                    slot.depth = depth;
                }
                None => {
                    let texture = renderer.register_native_texture(
                        &self.device,
                        &resolved,
                        wgpu::FilterMode::Nearest,
                    );
                    drop(renderer);
                    let frame = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("viewer3d frame"),
                        size: std::mem::size_of::<FrameUniform>() as u64,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    let frame_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("viewer3d frame"),
                        layout: &self.pipelines.frame_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: frame.as_entire_binding(),
                        }],
                    });
                    let (draws, draw_group) = self.draw_buffer(16);
                    self.slots.push(Slot {
                        used: self.painted,
                        size,
                        resolved,
                        multisampled,
                        depth,
                        texture,
                        frame,
                        frame_group,
                        draws,
                        draw_group,
                        capacity: 16,
                    });
                }
            }
        }
        if self.slots[index].capacity < draws {
            let capacity = draws.next_power_of_two();
            let (buffer, group) = self.draw_buffer(capacity);
            let slot = &mut self.slots[index];
            slot.draws = buffer;
            slot.draw_group = group;
            slot.capacity = capacity;
        }
        self.slots[index].used = self.painted;
        index
    }

    fn draw_buffer(&self, capacity: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer3d draws"),
            size: capacity * STRIDE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer3d draws"),
            layout: &self.pipelines.draw_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniform>() as u64),
                }),
            }],
        });
        (buffer, group)
    }

    fn targets(&self, size: [u32; 2]) -> (wgpu::TextureView, wgpu::TextureView, wgpu::TextureView) {
        let extent = wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 };
        let make = |label, format, samples, usage| {
            self.device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: extent,
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        (
            make(
                "viewer3d resolved",
                COLOUR,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            make("viewer3d multisampled", COLOUR, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT),
            make("viewer3d depth", DEPTH, SAMPLES, wgpu::TextureUsages::RENDER_ATTACHMENT),
        )
    }
}

enum Kind {
    Batch(usize),
    Stroke(usize),
}

struct Item {
    kind: Kind,
    see_through: bool,
    distance: f32,
}

fn render<M: Material>(
    state: &mut State,
    viewer: &Viewer<M>,
    size: [u32; 2],
    ppp: f32,
) -> egui::TextureId {
    let scene = &viewer.scene;
    if !state.scenes.contains_key(&scene.id) {
        let uploaded = upload(state, scene);
        state.scenes.insert(scene.id, uploaded);
        state.uploads += 1;
    }
    let used = state.painted;
    let style: Style = viewer.style;
    let vw = viewer.view(egui::Vec2::new(size[0] as f32, size[1] as f32));
    let selected = rgb(style.selected);
    let mut draws: Vec<DrawUniform> = Vec::new();
    let mut items: Vec<Item> = Vec::new();
    let mut corners: Vec<[V3; 8]> = Vec::new();
    let mut widest: f32 = 0.0;
    let uploaded = state.scenes.get_mut(&scene.id).expect("uploaded above");
    uploaded.used = used;
    let distance = |box_: &[V3; 8]| {
        let centre = box_
            .iter()
            .fold([0.0; 3], |a, p| [a[0] + p[0] / 8.0, a[1] + p[1] / 8.0, a[2] + p[2] / 8.0]);
        let d = sub(centre, vw.eye);
        dot(d, d)
    };
    for (i, batch) in uploaded.batches.iter().enumerate() {
        let look = viewer.look(batch.group);
        if !look.visible() {
            continue;
        }
        let opacity = look.opacity.min(1.0);
        let tint = if look.selected && batch.shading.tinted {
            [selected[0], selected[1], selected[2], style.selected_tint]
        } else {
            [0.0; 4]
        };
        let s = batch.shading;
        draws.push(DrawUniform {
            model: columns(&look.placement),
            normal: normal_matrix(&look.placement),
            tint,
            gloss: [s.gloss, s.sharpness, s.metal, if s.capped { 1.0 } else { 0.0 }],
            params: [opacity, if batch.texture.is_some() { 1.0 } else { 0.0 }, 0.0, 0.0],
            colour: [0.0; 4],
        });
        let box_ = placed(&batch.aabb, &look);
        items.push(Item {
            kind: Kind::Batch(i),
            see_through: opacity < 1.0,
            distance: distance(&box_),
        });
        corners.push(box_);
    }
    for (i, stroke) in uploaded.strokes.iter().enumerate() {
        let look = viewer.look(stroke.group);
        if !look.visible() {
            continue;
        }
        let opacity = (look.opacity * 1.5).min(1.0);
        let colour = if look.selected && stroke.tinted { selected } else { stroke.colour };
        let pixels = stroke.width * ppp;
        widest = widest.max(pixels);
        draws.push(DrawUniform {
            model: columns(&look.placement),
            normal: normal_matrix(&look.placement),
            tint: [0.0; 4],
            gloss: [0.0; 4],
            params: [opacity, 0.0, pixels, 0.0],
            colour: [colour[0], colour[1], colour[2], 1.0],
        });
        let box_ = placed(&stroke.aabb, &look);
        items.push(Item {
            kind: Kind::Stroke(i),
            see_through: opacity < 1.0,
            distance: distance(&box_),
        });
        corners.push(box_);
    }
    let (near, far) = depth_range(&corners, &vw, viewer.camera.ortho);
    let clip = viewer.cut.map(|c| [c.normal[0], c.normal[1], c.normal[2], c.offset]);
    let frame = FrameUniform {
        view_proj: view_projection(&vw, size, viewer.camera.ortho, near, far),
        eye: [vw.eye[0], vw.eye[1], vw.eye[2], 1.0],
        key: [vw.key[0], vw.key[1], vw.key[2], 0.0],
        fill: [vw.fill[0], vw.fill[1], vw.fill[2], 0.0],
        lighting: [style.ambient, style.key, style.fill, 0.0],
        cap: {
            let c = rgb(style.cap);
            [c[0], c[1], c[2], 1.0]
        },
        clip: clip.unwrap_or([0.0, 0.0, 1.0, 0.0]),
        viewport: [size[0] as f32, size[1] as f32, if clip.is_some() { 1.0 } else { 0.0 }, 0.0],
    };
    let bias = ((widest * 0.5 + 1.0) * 100.0).round() as i32;
    let index = state.slot(size, draws.len() as u64);
    let mut order: Vec<usize> = (0..items.len()).filter(|i| !items[*i].see_through).collect();
    let mut behind: Vec<usize> = (0..items.len()).filter(|i| items[*i].see_through).collect();
    behind.sort_by(|a, b| items[*b].distance.total_cmp(&items[*a].distance));
    order.extend(behind);
    let device = state.device.clone();
    for item in &items {
        match item.kind {
            Kind::Batch(_) => {
                state.pipelines.surface(&device, item.see_through, bias);
            }
            Kind::Stroke(_) => {
                state.pipelines.line(&device, item.see_through);
            }
        }
    }
    let slot = &state.slots[index];
    state.queue.write_buffer(&slot.frame, 0, bytemuck::bytes_of(&frame));
    let mut packed = vec![0u8; draws.len() * STRIDE as usize];
    for (k, d) in draws.iter().enumerate() {
        let at = k * STRIDE as usize;
        packed[at..at + std::mem::size_of::<DrawUniform>()].copy_from_slice(bytemuck::bytes_of(d));
    }
    if !packed.is_empty() {
        state.queue.write_buffer(&slot.draws, 0, &packed);
    }
    let [r, g, b] = rgb(style.background);
    let uploaded = &state.scenes[&scene.id];
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("viewer3d"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &slot.multisampled,
                depth_slice: None,
                resolve_target: Some(&slot.resolved),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: r as f64,
                        g: g as f64,
                        b: b as f64,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &slot.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, &slot.frame_group, &[]);
        for i in order {
            let item = &items[i];
            let offset = (i as u64 * STRIDE) as u32;
            pass.set_bind_group(1, &slot.draw_group, &[offset]);
            match item.kind {
                Kind::Batch(b) => {
                    let batch = &uploaded.batches[b];
                    let group = batch
                        .texture
                        .and_then(|t| uploaded.textures.get(t))
                        .unwrap_or(&state.blank);
                    pass.set_pipeline(&state.pipelines.surfaces[&(item.see_through, bias)]);
                    pass.set_bind_group(2, group, &[]);
                    pass.set_vertex_buffer(0, batch.vertices.slice(..));
                    pass.draw(0..batch.count, 0..1);
                }
                Kind::Stroke(s) => {
                    let stroke = &uploaded.strokes[s];
                    pass.set_pipeline(&state.pipelines.lines[&item.see_through]);
                    pass.set_vertex_buffer(0, stroke.segments.slice(..));
                    pass.draw(0..6, 0..stroke.count);
                }
            }
        }
    }
    state.queue.submit([encoder.finish()]);
    slot.texture
}

fn soft<M: Material>(ui: &Ui, rect: Rect, viewer: &Viewer<M>, size: [u32; 2]) -> egui::TextureId {
    let image = viewer.render_soft(size[0] as usize, size[1] as usize);
    let pass = ui.ctx().cumulative_pass_nr();
    SOFT.with(|cell| {
        let mut soft = cell.borrow_mut();
        if soft.pass != pass {
            soft.pass = pass;
            soft.next = 0;
        }
        let index = soft.next;
        soft.next += 1;
        if let Some(handle) = soft.handles.get_mut(index) {
            handle.set(image, TextureOptions::NEAREST);
        } else {
            let handle = ui.ctx().load_texture(
                format!("viewer3d {index} {rect:?}"),
                image,
                TextureOptions::NEAREST,
            );
            soft.handles.push(handle);
        }
        soft.handles[index].id()
    })
}

pub(super) fn paint<M: Material>(ui: &Ui, rect: Rect, viewer: &Viewer<M>) {
    let started = Instant::now();
    let ppp = ui.ctx().pixels_per_point();
    let size = [
        (rect.width() * ppp).round().max(1.0) as u32,
        (rect.height() * ppp).round().max(1.0) as u32,
    ];
    let pass = ui.ctx().cumulative_pass_nr();
    let texture = STATE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let state = slot.as_mut()?;
        state.begin(pass);
        Some(render(state, viewer, size, ppp))
    });
    let texture = texture.unwrap_or_else(|| soft(ui, rect, viewer, size));
    let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter_at(rect).image(texture, rect, uv, Color32::WHITE);
    DRAW_MICROS.store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
}

#[cfg(test)]
pub(super) fn uploads() -> u64 {
    STATE.with(|cell| cell.borrow().as_ref().map_or(0, |s| s.uploads))
}

#[cfg(test)]
mod tests {
    use super::super::{Camera, Scene, Surface, Viewer};
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::wgpu::{WgpuTestRenderer, create_render_state, default_wgpu_setup};

    #[test]
    fn a_scene_stays_on_the_gpu_while_passes_go_by_unpainted() {
        let made = std::panic::catch_unwind(|| {
            create_render_state(default_wgpu_setup(), egui_wgpu::RendererOptions::PREDICTABLE)
        });
        let Ok(state) = made else {
            eprintln!("skipped: no wgpu adapter");
            return;
        };
        install(&state);
        let triangle: Surface = Surface {
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            normals: vec![[0.0, 0.0, 1.0]; 3],
            ..Default::default()
        };
        let scene = Arc::new(Scene { surfaces: vec![triangle], ..Default::default() });
        let viewer = Viewer::new(scene, Camera::default());
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(64.0, 48.0));
        let mut pass = 0;
        let mut h = Harness::builder()
            .with_size(rect.size())
            .renderer(WgpuTestRenderer::from_render_state(state))
            .build_ui(move |ui| {
                pass += 1;
                if pass % 3 == 0 {
                    viewer.paint(ui, rect);
                }
            });
        for _ in 0..12 {
            h.step();
        }
        assert_eq!(uploads(), 1);
    }
}
