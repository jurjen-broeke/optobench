//! The shaded 3D view on wgpu (M8b): the scene's triangles lit from the viewer, its edges
//! and the traced rays as lines, and lenses as translucent glass, drawn into an
//! offscreen texture (4× multisampled, with depth) that the view shows as an image.
//! Per item a style (colour, hidden, selected, hovered) sits in a small storage buffer,
//! so selecting or hiding changes a few bytes, not the meshes.

use eframe::egui_wgpu::RenderState;
use eframe::wgpu;
use eframe::wgpu::util::DeviceExt;

use crate::camera::{Camera, V3};
use crate::document::Scene;

const SAMPLES: u32 = 4;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The item index of a line that belongs to no item (a ray).
const NO_ITEM: u32 = u32::MAX;

const SHADER: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    // xyz: the eye (perspective) ; w: 1 in perspective.
    eye: vec4<f32>,
    // xyz: towards the viewer (orthographic).
    back: vec4<f32>,
    // xyz: normal, w: offset; parts with dot(p, n) > w are cut away.
    section: vec4<f32>,
    // x: 1 when the section is on; y: how far lines move towards the eye (mm).
    flags: vec4<f32>,
    accent: vec4<f32>,
};

struct Style {
    colour: vec4<f32>,
    // x: hidden, y: selected, z: hovered.
    state: vec4<f32>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var<storage, read> styles: array<Style>;

fn cut(p: vec3<f32>) -> bool {
    return u.flags.x > 0.5 && dot(p, u.section.xyz) > u.section.w;
}

fn to_eye(p: vec3<f32>) -> vec3<f32> {
    if (u.eye.w > 0.5) {
        return normalize(u.eye.xyz - p);
    }
    return u.back.xyz;
}

struct MeshOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) @interpolate(flat) item: u32,
};

@vertex
fn vs_mesh(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) item: u32) -> MeshOut {
    var o: MeshOut;
    o.clip = u.view_proj * vec4<f32>(p, 1.0);
    o.world = p;
    o.normal = n;
    o.item = item;
    return o;
}

@fragment
fn fs_mesh(i: MeshOut) -> @location(0) vec4<f32> {
    let s = styles[i.item];
    if (s.state.x > 0.5 || cut(i.world)) {
        discard;
    }
    let e = to_eye(i.world);
    var n = normalize(i.normal);
    // Both sides are lit: faces of open shells and cut solids show from inside too.
    if (dot(n, e) < 0.0) {
        n = -n;
    }
    // A headlight, a little sky from +z (the scene is Z up) and a soft highlight.
    let diffuse = max(dot(n, e), 0.0);
    let sky = 0.5 + 0.5 * n.z;
    let h = normalize(e + vec3<f32>(0.0, 0.0, 0.6));
    let highlight = pow(max(dot(n, h), 0.0), 40.0);
    var base = s.colour.rgb;
    if (s.state.y > 0.5) {
        base = mix(base, u.accent.rgb, 0.55);
    }
    if (s.state.z > 0.5) {
        base = mix(base, vec3<f32>(1.0, 1.0, 1.0), 0.18);
    }
    let lit = base * (0.32 + 0.56 * diffuse + 0.14 * sky) + vec3<f32>(0.18 * highlight);
    let a = s.colour.a;
    return vec4<f32>(lit * a, a);
}

struct LineOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) colour: vec4<f32>,
    @location(2) @interpolate(flat) item: u32,
};

@vertex
fn vs_line(@location(0) p: vec3<f32>, @location(1) colour: vec4<f32>, @location(2) item: u32) -> LineOut {
    var o: LineOut;
    // Lines sit a hair in front of the faces they bound: moved towards the eye by a
    // fixed small distance, so edges behind other faces stay hidden.
    o.clip = u.view_proj * vec4<f32>(p + to_eye(p) * u.flags.y, 1.0);
    o.world = p;
    o.colour = colour;
    o.item = item;
    return o;
}

@fragment
fn fs_line(i: LineOut) -> @location(0) vec4<f32> {
    if (cut(i.world)) {
        discard;
    }
    var c = i.colour;
    if (i.item != 0xffffffffu) {
        let s = styles[i.item];
        if (s.state.x > 0.5) {
            discard;
        }
        if (s.state.y > 0.5) {
            c = vec4<f32>(u.accent.rgb, 1.0);
        }
    }
    return vec4<f32>(c.rgb * c.a, c.a);
}
"#;

/// The scene's buffers on the GPU.
struct Uploaded {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Index ranges: opaque items, then glass.
    opaque: u32,
    glass: u32,
    edges: Option<(wgpu::Buffer, u32)>,
    items: usize,
}

/// The offscreen target, at one size.
struct Target {
    size: (u32, u32),
    multisampled: wgpu::TextureView,
    depth: wgpu::TextureView,
    resolved: wgpu::TextureView,
    texture: egui::TextureId,
}

/// What a frame draws, besides the scene.
pub struct Frame<'a> {
    /// The camera.
    pub camera: &'a Camera,
    /// Item index → hidden.
    pub hidden: &'a [bool],
    /// The selected item.
    pub selected: Option<usize>,
    /// The item under the pointer.
    pub hovered: Option<usize>,
    /// The section plane (unit normal, offset), if on.
    pub section: Option<(V3, f64)>,
    /// The workspace's accent.
    pub accent: egui::Color32,
    /// Ray segments (from, to, colour), drawn when given.
    pub rays: &'a [(V3, V3, egui::Color32)],
    /// Changes when `rays` changes.
    pub rays_version: u64,
}

/// The renderer.
pub struct Renderer {
    state: RenderState,
    mesh_pipeline: wgpu::RenderPipeline,
    glass_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    styles: wgpu::Buffer,
    styles_len: usize,
    bind_group: wgpu::BindGroup,
    scene: Option<Uploaded>,
    scene_version: u64,
    rays: Option<(wgpu::Buffer, u32)>,
    rays_version: u64,
    target: Option<Target>,
}

/// Item colours: (fill, alpha, edge).
pub fn style_of(role: &str) -> ([f32; 3], f32, [u8; 4]) {
    match role {
        "lens" => ([0.55, 0.82, 0.95], 0.38, [120, 205, 240, 255]),
        "mirror" => ([0.86, 0.88, 0.92], 1.0, [70, 76, 88, 255]),
        "detector" => ([0.93, 0.78, 0.31], 1.0, [120, 96, 30, 255]),
        // A sample's layers, see-through so the focus inside shows.
        "sample" => ([0.42, 0.62, 0.86], 0.22, [90, 140, 200, 160]),
        // An ideal objective's pupil disc.
        "objective" => ([0.30, 0.33, 0.40], 1.0, [214, 128, 36, 255]),
        _ => ([0.60, 0.63, 0.68], 1.0, [36, 40, 48, 200]),
    }
}

fn floats(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl Renderer {
    /// A renderer on eframe's wgpu device.
    pub fn new(state: &RenderState) -> Self {
        let device = &state.device;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("optobench view"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("optobench view"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("optobench view"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let pipeline = |label: &str,
                        vertex: &str,
                        fragment: &str,
                        stride: u64,
                        attributes: &[wgpu::VertexAttribute],
                        topology: wgpu::PrimitiveTopology,
                        depth_write: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vertex),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: stride,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState {
                    topology,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: None,
                    unclipped_depth: false,
                    polygon_mode: wgpu::PolygonMode::Fill,
                    conservative: false,
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::LessEqual),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: SAMPLES,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fragment),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: COLOUR,
                        blend: Some(blend),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let mesh_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
        let line_attributes = wgpu::vertex_attr_array![0 => Float32x3, 1 => Unorm8x4, 2 => Uint32];
        let mesh_pipeline = pipeline(
            "optobench faces",
            "vs_mesh",
            "fs_mesh",
            28,
            &mesh_attributes,
            wgpu::PrimitiveTopology::TriangleList,
            true,
        );
        let glass_pipeline = pipeline(
            "optobench glass",
            "vs_mesh",
            "fs_mesh",
            28,
            &mesh_attributes,
            wgpu::PrimitiveTopology::TriangleList,
            false,
        );
        let line_pipeline = pipeline(
            "optobench lines",
            "vs_line",
            "fs_line",
            20,
            &line_attributes,
            wgpu::PrimitiveTopology::LineList,
            true,
        );
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("optobench view uniforms"),
            size: 64 + 5 * 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let (styles, bind_group) = Self::styles_buffer(device, &layout, &uniforms, 1);
        Self {
            state: state.clone(),
            mesh_pipeline,
            glass_pipeline,
            line_pipeline,
            layout,
            uniforms,
            styles,
            styles_len: 1,
            bind_group,
            scene: None,
            scene_version: u64::MAX,
            rays: None,
            rays_version: u64::MAX,
            target: None,
        }
    }

    fn styles_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniforms: &wgpu::Buffer,
        items: usize,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let styles = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("optobench view styles"),
            size: (items.max(1) * 32) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("optobench view"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: styles.as_entire_binding(),
                },
            ],
        });
        (styles, bind_group)
    }

    /// Uploads a scene's meshes and edges (when its version changed).
    fn upload(&mut self, scene: &Scene, version: u64) {
        if self.scene_version == version && self.scene.is_some() {
            return;
        }
        let device = &self.state.device;
        let mut vertices: Vec<u8> = Vec::new();
        let mut opaque: Vec<u32> = Vec::new();
        let mut glass: Vec<u32> = Vec::new();
        let mut edges: Vec<u8> = Vec::new();
        let mut base = 0_u32;
        for (index, item) in scene.items.iter().enumerate() {
            let (_, alpha, edge) = style_of(&item.role);
            if let Some(mesh) = &item.mesh {
                for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
                    vertices.extend(floats(&[p[0], p[1], p[2], n[0], n[1], n[2]]));
                    vertices.extend((index as u32).to_le_bytes());
                }
                let list = if alpha < 1.0 { &mut glass } else { &mut opaque };
                list.extend(mesh.triangles.iter().map(|i| i + base));
                base += mesh.positions.len() as u32;
            }
            for line in &item.lines {
                for w in line.windows(2) {
                    for p in w {
                        edges.extend(floats(&[p[0] as f32, p[1] as f32, p[2] as f32]));
                        edges.extend(edge);
                        edges.extend((index as u32).to_le_bytes());
                    }
                }
            }
        }
        let opaque_count = opaque.len() as u32;
        let glass_count = glass.len() as u32;
        let indices: Vec<u8> = opaque
            .into_iter()
            .chain(glass)
            .flat_map(|i| i.to_le_bytes())
            .collect();
        let buffer = |label: &str, contents: &[u8], usage: wgpu::BufferUsages| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if contents.is_empty() {
                    &[0; 4]
                } else {
                    contents
                },
                usage,
            })
        };
        let edge_count = (edges.len() / 20) as u32;
        self.scene = Some(Uploaded {
            vertices: buffer("optobench vertices", &vertices, wgpu::BufferUsages::VERTEX),
            indices: buffer("optobench indices", &indices, wgpu::BufferUsages::INDEX),
            opaque: opaque_count,
            glass: glass_count,
            edges: (edge_count > 0).then(|| {
                (
                    buffer("optobench edges", &edges, wgpu::BufferUsages::VERTEX),
                    edge_count,
                )
            }),
            items: scene.items.len(),
        });
        if scene.items.len() > self.styles_len {
            let (styles, bind_group) =
                Self::styles_buffer(device, &self.layout, &self.uniforms, scene.items.len());
            self.styles = styles;
            self.bind_group = bind_group;
            self.styles_len = scene.items.len();
        }
        self.scene_version = version;
    }

    fn upload_rays(&mut self, frame: &Frame<'_>) {
        if self.rays_version == frame.rays_version && self.rays.is_some() {
            return;
        }
        let mut bytes = Vec::with_capacity(frame.rays.len() * 40);
        for (a, b, colour) in frame.rays {
            for p in [a, b] {
                bytes.extend(floats(&[p[0] as f32, p[1] as f32, p[2] as f32]));
                bytes.extend(colour.to_array());
                bytes.extend(NO_ITEM.to_le_bytes());
            }
        }
        let count = (bytes.len() / 20) as u32;
        self.rays = (count > 0).then(|| {
            (
                self.state
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("optobench rays"),
                        contents: &bytes,
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
                count,
            )
        });
        self.rays_version = frame.rays_version;
    }

    fn target(&mut self, size: (u32, u32)) -> &Target {
        let stale = self.target.as_ref().is_none_or(|t| t.size != size);
        if stale {
            let device = &self.state.device;
            let texture = |label: &str, format, samples, usage| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size.0,
                            height: size.1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: samples,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage,
                        view_formats: &[],
                    })
                    .create_view(&wgpu::TextureViewDescriptor::default())
            };
            let multisampled = texture(
                "optobench view samples",
                COLOUR,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let depth = texture(
                "optobench view depth",
                DEPTH,
                SAMPLES,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
            );
            let resolved = texture(
                "optobench view",
                COLOUR,
                1,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            );
            let mut renderer = self.state.renderer.write();
            let id = match &self.target {
                Some(old) => {
                    renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        &resolved,
                        wgpu::FilterMode::Linear,
                        old.texture,
                    );
                    old.texture
                }
                None => {
                    renderer.register_native_texture(device, &resolved, wgpu::FilterMode::Linear)
                }
            };
            drop(renderer);
            self.target = Some(Target {
                size,
                multisampled,
                depth,
                resolved,
                texture: id,
            });
        }
        self.target.as_ref().expect("just made")
    }

    /// Draws the scene for a view `size` pixels large; returns the texture to show.
    pub fn draw(
        &mut self,
        scene: &Scene,
        scene_version: u64,
        frame: &Frame<'_>,
        size: (u32, u32),
    ) -> egui::TextureId {
        let size = (size.0.clamp(1, 8192), size.1.clamp(1, 8192));
        self.upload(scene, scene_version);
        self.upload_rays(frame);

        // Uniforms.
        let (low, high) = scene.bounds.unwrap_or(([-100.0; 3], [100.0; 3]));
        let radius = 0.5
            * ((high[0] - low[0]).powi(2)
                + (high[1] - low[1]).powi(2)
                + (high[2] - low[2]).powi(2))
            .sqrt();
        let c = frame.camera;
        // Room for the scene around the target, wherever the target was panned to.
        let centre = [0, 1, 2].map(|i| 0.5 * (low[i] + high[i]));
        let offset = (0..3)
            .map(|i| (c.target[i] - centre[i]).powi(2))
            .sum::<f64>()
            .sqrt();
        let matrix = c.matrix(size.0 as f64 / size.1 as f64, radius + offset);
        let (back, _, _) = c.basis();
        let eye = [0, 1, 2].map(|i| c.target[i] + back[i] * c.distance());
        let (normal, offset) = frame.section.unwrap_or(([0.0, 0.0, 1.0], 0.0));
        let mut uniform: Vec<f32> = matrix.iter().flatten().copied().collect();
        uniform.extend([
            eye[0] as f32,
            eye[1] as f32,
            eye[2] as f32,
            if c.perspective { 1.0 } else { 0.0 },
            back[0] as f32,
            back[1] as f32,
            back[2] as f32,
            0.0,
            normal[0] as f32,
            normal[1] as f32,
            normal[2] as f32,
            offset as f32,
            if frame.section.is_some() { 1.0 } else { 0.0 },
            (c.height * 1.5e-3) as f32,
            0.0,
            0.0,
            // The target is shown as gamma-space colour, as egui's colours are.
            frame.accent.r() as f32 / 255.0,
            frame.accent.g() as f32 / 255.0,
            frame.accent.b() as f32 / 255.0,
            1.0,
        ]);
        let queue = self.state.queue.clone();
        queue.write_buffer(&self.uniforms, 0, &floats(&uniform));
        let mut styles: Vec<f32> = Vec::with_capacity(scene.items.len() * 8);
        for (i, item) in scene.items.iter().enumerate() {
            let (fill, alpha, _) = style_of(&item.role);
            styles.extend([fill[0], fill[1], fill[2], alpha]);
            styles.extend([
                if frame.hidden.get(i).copied().unwrap_or(false) {
                    1.0
                } else {
                    0.0
                },
                if frame.selected == Some(i) { 1.0 } else { 0.0 },
                if frame.hovered == Some(i) { 1.0 } else { 0.0 },
                0.0,
            ]);
        }
        if !styles.is_empty() {
            queue.write_buffer(&self.styles, 0, &floats(&styles));
        }

        let target = self.target(size);
        let (multisampled, depth, resolved, texture) = (
            target.multisampled.clone(),
            target.depth.clone(),
            target.resolved.clone(),
            target.texture,
        );
        let mut encoder =
            self.state
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("optobench view"),
                });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("optobench view"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &multisampled,
                    resolve_target: Some(&resolved),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Discard,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
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
            pass.set_bind_group(0, &self.bind_group, &[]);
            if let Some(up) = &self.scene {
                if up.items == scene.items.len() {
                    pass.set_vertex_buffer(0, up.vertices.slice(..));
                    pass.set_index_buffer(up.indices.slice(..), wgpu::IndexFormat::Uint32);
                    if up.opaque > 0 {
                        pass.set_pipeline(&self.mesh_pipeline);
                        pass.draw_indexed(0..up.opaque, 0, 0..1);
                    }
                    if let Some((edges, count)) = &up.edges {
                        pass.set_pipeline(&self.line_pipeline);
                        pass.set_vertex_buffer(0, edges.slice(..));
                        pass.draw(0..*count, 0..1);
                    }
                    if let Some((rays, count)) = &self.rays {
                        pass.set_pipeline(&self.line_pipeline);
                        pass.set_vertex_buffer(0, rays.slice(..));
                        pass.draw(0..*count, 0..1);
                    }
                    if up.glass > 0 {
                        pass.set_pipeline(&self.glass_pipeline);
                        pass.set_vertex_buffer(0, up.vertices.slice(..));
                        pass.draw_indexed(up.opaque..up.opaque + up.glass, 0, 0..1);
                    }
                }
            }
        }
        queue.submit([encoder.finish()]);
        texture
    }
}
