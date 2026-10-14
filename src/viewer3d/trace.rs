use super::gpu::{COLOUR, DEPTH, SAMPLES, columns, normal_matrix};
use super::{Cut, Image, Look, Placement, Shading, Style, Trace, V3, View, rgb, to_linear};
use bytemuck::{Pod, Zeroable};

const LEAF: usize = 4;
pub(super) const WORKGROUP: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(super) struct Node {
    lo: [f32; 3],
    a: u32,
    hi: [f32; 3],
    count: u32,
}

struct Builder<'a> {
    triangles: &'a [[V3; 3]],
    centres: Vec<V3>,
    order: Vec<u32>,
    nodes: Vec<Node>,
    node_base: u32,
    prim_base: u32,
}

impl Builder<'_> {
    fn bounds(&self, start: usize, end: usize) -> (V3, V3, V3, V3) {
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        let (mut clo, mut chi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for &t in &self.order[start..end] {
            for p in &self.triangles[t as usize] {
                for k in 0..3 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
            let c = self.centres[t as usize];
            for k in 0..3 {
                clo[k] = clo[k].min(c[k]);
                chi[k] = chi[k].max(c[k]);
            }
        }
        (lo, hi, clo, chi)
    }

    fn split(&mut self, at: usize, start: usize, end: usize) {
        let (lo, hi, clo, chi) = self.bounds(start, end);
        let extent = [0, 1, 2].map(|k| chi[k] - clo[k]);
        let axis = (0..3).max_by(|a, b| extent[*a].total_cmp(&extent[*b])).unwrap_or(0);
        if end - start <= LEAF || extent[axis] <= 0.0 {
            let a = self.prim_base + start as u32;
            self.nodes[at] = Node { lo, hi, a, count: (end - start) as u32 };
            return;
        }
        let mid = start + (end - start) / 2;
        let centres = &self.centres;
        self.order[start..end].select_nth_unstable_by(mid - start, |a, b| {
            centres[*a as usize][axis].total_cmp(&centres[*b as usize][axis])
        });
        let left = self.nodes.len();
        self.nodes.extend([Node::zeroed(); 2]);
        self.nodes[at] = Node { lo, hi, a: self.node_base + left as u32, count: 0 };
        self.split(left, start, mid);
        self.split(left + 1, mid, end);
    }
}

pub(super) fn bvh(triangles: &[[V3; 3]], node_base: u32, prim_base: u32) -> (Vec<Node>, Vec<u32>) {
    if triangles.is_empty() {
        let empty = Node { lo: [f32::MAX; 3], a: 0, hi: [f32::MIN; 3], count: 0 };
        return (vec![empty], Vec::new());
    }
    let centres =
        triangles.iter().map(|[a, b, c]| [0, 1, 2].map(|k| (a[k] + b[k] + c[k]) / 3.0)).collect();
    let mut builder = Builder {
        triangles,
        centres,
        order: (0..triangles.len() as u32).collect(),
        nodes: vec![Node::zeroed()],
        node_base,
        prim_base,
    };
    builder.split(0, 0, triangles.len());
    (builder.nodes, builder.order)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Params {
    eye: [f32; 4],
    right: [f32; 4],
    up: [f32; 4],
    forward: [f32; 4],
    key: [f32; 4],
    key_radiance: [f32; 4],
    fill: [f32; 4],
    fill_radiance: [f32; 4],
    sky: [f32; 4],
    ground: [f32; 4],
    background: [f32; 4],
    cap: [f32; 4],
    clip: [f32; 4],
    counts: [u32; 4],
    size: [u32; 4],
}

pub(super) struct Shot<'a> {
    pub view: &'a View,
    pub ortho: bool,
    pub style: &'a Style,
    pub cut: Option<Cut>,
    pub trace: Trace,
    pub radius: f32,
    pub size: [u32; 2],
    pub instances: u32,
}

fn four(v: V3, w: f32) -> [f32; 4] {
    [v[0], v[1], v[2], w]
}

impl Params {
    pub(super) fn new(shot: &Shot, base: u32, count: u32, stamp: u32) -> Params {
        let (vw, l) = (shot.view, &shot.style.lighting);
        let background = rgb(shot.style.background).map(to_linear);
        let cap = rgb(shot.style.cap).map(to_linear);
        let clip = shot.cut.map_or([0.0, 0.0, 1.0, 0.0], |c| four(c.normal, c.offset));
        Params {
            eye: four(vw.eye, if shot.ortho { vw.dist } else { 0.0 }),
            right: four(vw.right, vw.focal),
            up: four(vw.up, 0.0),
            forward: four(vw.forward, shot.radius.max(1.0e-6) * 1.0e-4),
            key: four(l.direction(&l.key, vw), 0.0),
            key_radiance: four(l.key.radiance(), l.key.spread.max(0.0)),
            fill: four(l.direction(&l.fill, vw), 0.0),
            fill_radiance: four(l.fill.radiance(), l.fill.spread.max(0.0)),
            sky: four(l.sky(), 0.0),
            ground: four(l.ground(), 0.0),
            background: four(background, l.exposure),
            cap: four(cap, if shot.cut.is_some() { 1.0 } else { 0.0 }),
            clip,
            counts: [base, count, shot.trace.bounces.max(1), shot.instances],
            size: [shot.size[0], shot.size[1], stamp, 0],
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct Instance {
    world: [[f32; 4]; 4],
    inverse: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
    tint: [f32; 4],
    surface: [f32; 4],
    refs: [u32; 4],
}

const CAPPED: u32 = 1;
const VISIBLE: u32 = 2;
const TEXTURED: u32 = 4;

fn inverse(m: &Placement) -> [[f32; 4]; 4] {
    let n = normal_matrix(m);
    let r = |row: usize, col: usize| n[row][col];
    let t = [m[12], m[13], m[14]];
    let moved = [0, 1, 2].map(|row| -(r(row, 0) * t[0] + r(row, 1) * t[1] + r(row, 2) * t[2]));
    [
        [r(0, 0), r(1, 0), r(2, 0), 0.0],
        [r(0, 1), r(1, 1), r(2, 1), 0.0],
        [r(0, 2), r(1, 2), r(2, 2), 0.0],
        [moved[0], moved[1], moved[2], 1.0],
    ]
}

pub(super) struct Part {
    pub first: u32,
    pub root: u32,
    pub texture: Option<usize>,
    pub shading: Shading,
}

impl Instance {
    pub(super) fn new(part: &Part, look: &Look, style: &Style) -> Instance {
        let s = part.shading;
        let selected = rgb(style.selected);
        let tint = if look.selected && s.tinted {
            [selected[0], selected[1], selected[2], style.selected_tint]
        } else {
            [0.0; 4]
        };
        let roughness = (2.0 / (s.sharpness.max(0.0) + 2.0)).sqrt().clamp(0.03, 1.0);
        let f0 = 0.04 * (s.gloss / 0.12).clamp(0.0, 2.5);
        let flags = if s.capped { CAPPED } else { 0 }
            | if look.visible() { VISIBLE } else { 0 }
            | if part.texture.is_some() { TEXTURED } else { 0 };
        Instance {
            world: columns(&look.placement),
            inverse: inverse(&look.placement),
            normal: normal_matrix(&look.placement),
            tint,
            surface: [f0, roughness, s.metal.clamp(0.0, 1.0), look.opacity.min(1.0)],
            refs: [part.first, part.texture.unwrap_or(0) as u32, part.root, flags],
        }
    }
}

pub(super) fn transform(look: &Look) -> [f32; 12] {
    let m = &look.placement;
    std::array::from_fn(|i| {
        let (row, col) = (i / 4, i % 4);
        m[col * 4 + row]
    })
}

pub(super) fn pack_images(images: &[Image]) -> (Vec<u32>, Vec<[u32; 4]>) {
    let mut texels = Vec::new();
    let mut pictures = Vec::new();
    for image in images {
        let valid = image.width > 0
            && image.height > 0
            && image.rgba.len() >= image.width * image.height * 4;
        let (w, h) = if valid { (image.width, image.height) } else { (1, 1) };
        pictures.push([texels.len() as u32, w as u32, h as u32, 0]);
        if valid {
            texels.extend(
                image.rgba[..w * h * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]])),
            );
        } else {
            texels.push(u32::MAX);
        }
    }
    if texels.is_empty() {
        texels.push(u32::MAX);
    }
    if pictures.is_empty() {
        pictures.push([0, 1, 1, 0]);
    }
    (texels, pictures)
}

fn storage(
    binding: u32,
    read_only: bool,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn uniform(binding: u32, visibility: wgpu::ShaderStages) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

pub(super) struct Tracer {
    pub hardware: bool,
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::ComputePipeline,
    present_layout: wgpu::BindGroupLayout,
    pub present: wgpu::RenderPipeline,
}

pub(super) fn supported(device: &wgpu::Device, adapter: &wgpu::Adapter) -> bool {
    adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        && device.limits().max_storage_buffers_per_shader_stage >= 8
}

impl Tracer {
    pub(super) fn new(device: &wgpu::Device) -> Tracer {
        let hardware = device.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY);
        let compute = wgpu::ShaderStages::COMPUTE;
        let mut entries = vec![
            uniform(0, compute),
            storage(1, false, compute),
            storage(2, true, compute),
            storage(3, true, compute),
            storage(4, true, compute),
            storage(5, true, compute),
        ];
        if hardware {
            entries.push(wgpu::BindGroupLayoutEntry {
                binding: 6,
                visibility: compute,
                ty: wgpu::BindingType::AccelerationStructure { vertex_return: false },
                count: None,
            });
        } else {
            entries.extend([storage(6, true, compute), storage(7, true, compute)]);
        }
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d trace"),
            entries: &entries,
        });
        let source = if hardware {
            format!(
                "enable wgpu_ray_query;\n{}\n{}",
                include_str!("trace.wgsl"),
                include_str!("trace_query.wgsl")
            )
        } else {
            format!("{}\n{}", include_str!("trace.wgsl"), include_str!("trace_bvh.wgsl"))
        };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer3d trace"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewer3d trace"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("viewer3d trace"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("trace_main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let present_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("viewer3d present"),
            entries: &[uniform(0, fragment), storage(1, true, fragment)],
        });
        let present_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("viewer3d present"),
            source: wgpu::ShaderSource::Wgsl(include_str!("present.wgsl").into()),
        });
        let present_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("viewer3d present"),
                bind_group_layouts: &[Some(&present_layout)],
                immediate_size: 0,
            });
        let present = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewer3d present"),
            layout: Some(&present_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &present_module,
                entry_point: Some("present_vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &present_module,
                entry_point: Some("present_fragment"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: COLOUR,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState { count: SAMPLES, ..Default::default() },
            multiview_mask: None,
            cache: None,
        });
        Tracer { hardware, layout, pipeline, present_layout, present }
    }
}

fn buffer_with(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    bytes: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (bytes.len() as u64).next_multiple_of(16).max(16),
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buffer, 0, bytes);
    buffer
}

struct Accel {
    blas: Vec<wgpu::Blas>,
    tlas: wgpu::Tlas,
}

pub(super) struct Traced {
    parts: Vec<Part>,
    instances: wgpu::Buffer,
    texels: wgpu::Buffer,
    pictures: wgpu::Buffer,
    nodes: Option<(wgpu::Buffer, wgpu::Buffer)>,
    accel: Option<Accel>,
}

pub(super) struct Piece<'a> {
    pub first: u32,
    pub count: u32,
    pub texture: Option<usize>,
    pub shading: Shading,
    pub triangles: &'a [[V3; 3]],
}

impl Traced {
    pub(super) fn new(
        tracer: &Tracer,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vertices: &wgpu::Buffer,
        pieces: &[Piece],
        images: &[Image],
    ) -> Traced {
        let (texels, pictures) = pack_images(images);
        let read = wgpu::BufferUsages::STORAGE;
        let texels =
            buffer_with(device, queue, "viewer3d texels", bytemuck::cast_slice(&texels), read);
        let pictures =
            buffer_with(device, queue, "viewer3d pictures", bytemuck::cast_slice(&pictures), read);
        let mut parts = Vec::with_capacity(pieces.len());
        let mut all_nodes: Vec<Node> = Vec::new();
        let mut all_prims: Vec<u32> = Vec::new();
        for piece in pieces {
            let root = all_nodes.len() as u32;
            if !tracer.hardware {
                let (nodes, prims) = bvh(piece.triangles, root, all_prims.len() as u32);
                all_nodes.extend(nodes);
                all_prims.extend(prims);
            }
            parts.push(Part {
                first: piece.first,
                root,
                texture: piece.texture,
                shading: piece.shading,
            });
        }
        let nodes = (!tracer.hardware).then(|| {
            if all_prims.is_empty() {
                all_prims.push(0);
            }
            if all_nodes.is_empty() {
                all_nodes.push(Node::zeroed());
            }
            (
                buffer_with(
                    device,
                    queue,
                    "viewer3d nodes",
                    bytemuck::cast_slice(&all_nodes),
                    read,
                ),
                buffer_with(
                    device,
                    queue,
                    "viewer3d prims",
                    bytemuck::cast_slice(&all_prims),
                    read,
                ),
            )
        });
        let accel = tracer.hardware.then(|| {
            let sizes: Vec<wgpu::BlasTriangleGeometrySizeDescriptor> = pieces
                .iter()
                .map(|p| wgpu::BlasTriangleGeometrySizeDescriptor {
                    vertex_format: wgpu::VertexFormat::Float32x3,
                    vertex_count: p.count,
                    index_format: None,
                    index_count: None,
                    flags: wgpu::AccelerationStructureGeometryFlags::empty(),
                })
                .collect();
            let blas: Vec<wgpu::Blas> = sizes
                .iter()
                .map(|size| {
                    device.create_blas(
                        &wgpu::CreateBlasDescriptor {
                            label: Some("viewer3d blas"),
                            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
                        },
                        wgpu::BlasGeometrySizeDescriptors::Triangles {
                            descriptors: vec![size.clone()],
                        },
                    )
                })
                .collect();
            let entries: Vec<wgpu::BlasBuildEntry> = blas
                .iter()
                .zip(&sizes)
                .zip(pieces)
                .map(|((blas, size), piece)| wgpu::BlasBuildEntry {
                    blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                        wgpu::BlasTriangleGeometry {
                            size,
                            vertex_buffer: vertices,
                            first_vertex: piece.first,
                            vertex_stride: 48,
                            index_buffer: None,
                            first_index: None,
                            transform_buffer: None,
                            transform_buffer_offset: None,
                        },
                    ]),
                })
                .collect();
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.build_acceleration_structures(entries.iter(), std::iter::empty());
            queue.submit([encoder.finish()]);
            let tlas = device.create_tlas(&wgpu::CreateTlasDescriptor {
                label: Some("viewer3d tlas"),
                max_instances: pieces.len().max(1) as u32,
                flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                update_mode: wgpu::AccelerationStructureUpdateMode::Build,
            });
            Accel { blas, tlas }
        });
        let instances = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("viewer3d instances"),
            size: (std::mem::size_of::<Instance>() * pieces.len().max(1)) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Traced { parts, instances, texels, pictures, nodes, accel }
    }
}

pub(super) struct Target {
    size: [u32; 2],
    pub accum: wgpu::Buffer,
    params: wgpu::Buffer,
    present: wgpu::Buffer,
    pub samples: u32,
    pub stamp: u32,
}

impl Target {
    pub(super) fn new(device: &wgpu::Device, size: [u32; 2]) -> Target {
        let make = |label, bytes: u64, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        Target {
            size,
            accum: make(
                "viewer3d accum",
                size[0] as u64 * size[1] as u64 * 16,
                wgpu::BufferUsages::STORAGE,
            ),
            params: make(
                "viewer3d trace params",
                std::mem::size_of::<Params>() as u64,
                wgpu::BufferUsages::UNIFORM,
            ),
            present: make("viewer3d present", 16, wgpu::BufferUsages::UNIFORM),
            samples: 0,
            stamp: 0,
        }
    }

    pub(super) fn size(&self) -> [u32; 2] {
        self.size
    }
}

pub(super) struct Frame<'a> {
    pub looks: Vec<Look>,
    pub style: &'a Style,
    pub vertices: &'a wgpu::Buffer,
}

impl Tracer {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn dispatch(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        traced: &mut Traced,
        frame: &Frame,
        target: &mut Target,
        shot: &Shot,
        count: u32,
    ) {
        let instances: Vec<Instance> = traced
            .parts
            .iter()
            .zip(&frame.looks)
            .map(|(part, look)| Instance::new(part, look, frame.style))
            .collect();
        if !instances.is_empty() {
            queue.write_buffer(&traced.instances, 0, bytemuck::cast_slice(&instances));
        }
        let params = Params::new(shot, target.samples, count, target.stamp);
        queue.write_buffer(&target.params, 0, bytemuck::bytes_of(&params));
        if let Some(accel) = &mut traced.accel {
            for (i, (blas, look)) in accel.blas.iter().zip(&frame.looks).enumerate() {
                let mask = if look.visible() { 0xff } else { 0 };
                accel.tlas[i] =
                    Some(wgpu::TlasInstance::new(blas, transform(look), i as u32, mask));
            }
            encoder.build_acceleration_structures(std::iter::empty(), [&accel.tlas]);
        }
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: target.params.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: target.accum.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: frame.vertices.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: traced.instances.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 4, resource: traced.texels.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 5, resource: traced.pictures.as_entire_binding() },
        ];
        if let Some(accel) = &traced.accel {
            entries.push(wgpu::BindGroupEntry { binding: 6, resource: accel.tlas.as_binding() });
        }
        if let Some((nodes, prims)) = &traced.nodes {
            entries.push(wgpu::BindGroupEntry { binding: 6, resource: nodes.as_entire_binding() });
            entries.push(wgpu::BindGroupEntry { binding: 7, resource: prims.as_entire_binding() });
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer3d trace"),
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("viewer3d trace"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        let [w, h] = target.size;
        pass.dispatch_workgroups(w.div_ceil(WORKGROUP), h.div_ceil(WORKGROUP), 1);
        drop(pass);
        target.samples += count;
    }

    pub(super) fn present_group(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &Target,
    ) -> wgpu::BindGroup {
        queue.write_buffer(&target.present, 0, bytemuck::cast_slice(&[target.size[0], 0, 0, 0]));
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("viewer3d present"),
            layout: &self.present_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: target.present.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: target.accum.as_entire_binding() },
            ],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer3d::{IDENTITY, hit_triangle};

    fn closest(
        nodes: &[Node],
        prims: &[u32],
        tris: &[[V3; 3]],
        o: V3,
        d: V3,
    ) -> Option<(f32, u32)> {
        let mut best: Option<(f32, u32)> = None;
        let mut stack = vec![0u32];
        while let Some(i) = stack.pop() {
            let n = nodes[i as usize];
            let inv = d.map(|v| 1.0 / v);
            let (mut near, mut far) = (0.0f32, best.map_or(f32::MAX, |b| b.0));
            for k in 0..3 {
                let (a, b) = ((n.lo[k] - o[k]) * inv[k], (n.hi[k] - o[k]) * inv[k]);
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            if near > far {
                continue;
            }
            if n.count == 0 {
                stack.extend([n.a, n.a + 1]);
                continue;
            }
            for &p in &prims[n.a as usize..(n.a + n.count) as usize] {
                if let Some(t) = hit_triangle(o, d, tris[p as usize])
                    && best.is_none_or(|b| t < b.0)
                {
                    best = Some((t, p));
                }
            }
        }
        best
    }

    fn noise(seed: &mut u32) -> f32 {
        *seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (*seed >> 8) as f32 / 16_777_216.0
    }

    #[test]
    fn the_bvh_finds_the_same_hit_as_every_triangle() {
        let mut seed = 7;
        let tris: Vec<[V3; 3]> = (0..500)
            .map(|_| {
                let c = [0, 1, 2].map(|_| noise(&mut seed) * 10.0 - 5.0);
                std::array::from_fn(|_| [0, 1, 2].map(|k| c[k] + noise(&mut seed) - 0.5))
            })
            .collect();
        let (nodes, prims) = bvh(&tris, 0, 0);
        let mut sorted = prims.clone();
        sorted.sort();
        assert_eq!(sorted, (0..500).collect::<Vec<u32>>());
        for _ in 0..400 {
            let o = [0, 1, 2].map(|_| noise(&mut seed) * 30.0 - 15.0);
            let to = [0, 1, 2].map(|_| noise(&mut seed) * 8.0 - 4.0);
            let d = super::super::norm(super::super::sub(to, o));
            let brute = tris
                .iter()
                .enumerate()
                .filter_map(|(i, t)| hit_triangle(o, d, *t).map(|h| (h, i as u32)))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            assert_eq!(closest(&nodes, &prims, &tris, o, d), brute);
        }
    }

    #[test]
    fn the_inverse_undoes_the_placement() {
        let (s, c) = 0.7f32.sin_cos();
        let m: Placement =
            [c * 2.0, s * 2.0, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 3.0, -1.0, 2.0, 1.0];
        let look = Look { placement: m, ..Look::default() };
        let inv = inverse(&m);
        let p = [0.3, -2.0, 5.0];
        let w = look.place(p);
        let back =
            [0, 1, 2].map(|r| inv[0][r] * w[0] + inv[1][r] * w[1] + inv[2][r] * w[2] + inv[3][r]);
        for k in 0..3 {
            assert!((back[k] - p[k]).abs() < 1.0e-4, "{back:?}");
        }
        let t = transform(&Look { placement: IDENTITY, ..Look::default() });
        assert_eq!(t, [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]);
        let t = transform(&look);
        assert_eq!([t[3], t[7], t[11]], [3.0, -1.0, 2.0]);
    }
}
