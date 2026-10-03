//! The volumetric ground-fog **enhancement** pass
//! (`legaia_engine_core::fog_volume`): the bank drawn as horizontal
//! alpha-blended sheets over the walk ground, depth-tested against the scene
//! already in the frame (no depth write), after every 3D draw and before the
//! HUD overlays.
//!
//! The pass owns its pipeline and buffers and is built lazily on the first
//! frame a host stages a bank, so a renderer that never sees one - every
//! parity oracle, every host with the toggle down - creates nothing and
//! draws exactly what it drew before the feature.
//!
//! The browser play page draws the same bank through
//! `site/js/webgl-fog-volume.js`, a GLSL transcription of [`FOG_SHADER_SRC`]
//! over the same frame data; both read the recipe's numbers from the frame
//! (`legaia_engine_core::fog_volume::FOG_SHADER_CONSTANTS`). This crate does
//! not depend on `engine-core`, so the host hands the frame over as a
//! [`FogVolumeDraw`] of plain fields.

use super::*;

/// One frame's bank, as the host hands it over - the fields of
/// `legaia_engine_core::fog_volume::FogVolumeFrame` plus the world -> clip
/// matrix of the bank's space.
pub struct FogVolumeDraw<'a> {
    /// Bank space (retail Y-down world units for a field scene, raw battle
    /// stage units in battle) to clip space, before the reversed-Z remap.
    pub world_to_clip: Mat4,
    /// World X / Z of the disturbance grid's corner, its cell size, and its
    /// cells per side.
    pub sim_origin: [f32; 2],
    pub sim_cell: f32,
    pub sim_dim: u32,
    /// `sim_dim`² density bytes, row-major `[z][x]`, `255` = undisturbed.
    pub density: &'a [u8],
    /// World X / Z of the sheet mesh's corner, its quad size, and its quads
    /// per side.
    pub mesh_origin: [f32; 2],
    pub mesh_cell: f32,
    pub mesh_dim: u32,
    /// `(mesh_dim + 1)²` vertices of `[x, floor_y, z]`, row-major - uploaded
    /// only when `ground_gen` changes.
    pub mesh_positions: &'a [f32],
    pub ground_gen: u32,
    /// Framebuffer-space colour and summed floor opacity.
    pub color: [f32; 3],
    pub opacity: f32,
    /// Bank depth above the floor and the sheet count.
    pub height: f32,
    pub layers: u32,
    /// Accumulated drift, world units.
    pub drift: [f32; 2],
    /// `FOG_SHADER_CONSTANTS`.
    pub shader_constants: [f32; 4],
}

/// The sheet mesh's triangle list for `dim` quads a side.
fn mesh_indices(dim: u32) -> Vec<u32> {
    let n = dim + 1;
    let mut out = Vec::with_capacity((dim * dim * 6) as usize);
    for z in 0..dim {
        for x in 0..dim {
            let a = z * n + x;
            let b = a + 1;
            let c = a + n;
            let d = c + 1;
            out.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    out
}

/// WGSL for the fog sheets. The GLSL twin lives in
/// `site/js/webgl-fog-volume.js`; keep the two in step.
pub(super) const FOG_SHADER_SRC: &str = r#"
struct FogU {
    m: mat4x4<f32>,
    sim: vec4<f32>,
    mesh: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    consts: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: FogU;
@group(0) @binding(1) var dens: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) xz: vec2<f32>,
    @location(1) layer: f32,
};

@vertex
fn vs_main(@location(0) p: vec3<f32>, @builtin(instance_index) inst: u32) -> VOut {
    let t = (f32(inst) + 0.5) / u.params.y;
    // Retail Y-down: the sheets stack upward from just above the floor.
    let y = p.y - 4.0 - t * u.params.x;
    var o: VOut;
    o.pos = u.m * vec4<f32>(p.x, y, p.z, 1.0);
    o.xz = p.xz;
    o.layer = t;
    return o;
}

fn fog_hash(i: vec2<i32>) -> f32 {
    let x = u32(i.x & 0xffff);
    let y = u32(i.y & 0xffff);
    var h = x * 0x8da6b343u ^ y * 0xd8163841u;
    h = (h ^ (h >> 13u)) * 0x85ebca6bu;
    h = h ^ (h >> 16u);
    return f32(h & 0xffffu) / 65535.0;
}

fn fog_noise(p: vec2<f32>) -> f32 {
    let fl = floor(p);
    let i = vec2<i32>(fl);
    let f = p - fl;
    let s = f * f * (vec2<f32>(3.0) - 2.0 * f);
    let a = fog_hash(i);
    let b = fog_hash(i + vec2<i32>(1, 0));
    let c = fog_hash(i + vec2<i32>(0, 1));
    let d = fog_hash(i + vec2<i32>(1, 1));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let drift = u.params.zw;
    let t = in.layer;
    let fine = u.consts.x;
    let bank = u.consts.y;
    let n1 = fog_noise((in.xz - drift) * fine + vec2<f32>(t * 7.31, t * 3.17));
    let n2 = fog_noise((in.xz - drift * 0.6) * (fine * 2.3) + vec2<f32>(11.7, 5.3));
    let nb = fog_noise((in.xz - drift * 0.35) * bank + vec2<f32>(3.1, 8.9));
    // The bank's top undulates with the slow noise; the profile thins it
    // toward that top.
    let top = 0.55 + 0.65 * nb;
    let profile = pow(clamp(1.0 - t / top, 0.0, 1.0), u.consts.w);
    let shape = clamp(0.2 + 1.0 * nb, 0.0, 1.0) * (0.5 + 0.5 * (0.65 * n1 + 0.35 * n2));
    // Disturbance: the sim grid's density, undisturbed outside it.
    let uv = (in.xz - u.sim.xy) / (u.sim.z * u.sim.w);
    var d = 1.0;
    if (all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0))) {
        d = textureSampleLevel(dens, samp, uv, 0.0).r;
    }
    // Fade out toward the sheet mesh's rim so its square edge never shows.
    let half_extent = 0.5 * u.mesh.z * u.mesh.w;
    let centre = u.mesh.xy + vec2<f32>(half_extent);
    let r = length(in.xz - centre) / half_extent;
    let edge = 1.0 - smoothstep(0.55, 0.95, r);
    let a = u.color.a * d * profile * shape * edge * u.consts.z / u.params.y;
    let rgb = u.color.rgb * (0.9 + 0.2 * n1);
    return vec4<f32>(rgb, clamp(a, 0.0, 1.0));
}
"#;

/// Uniform block of [`FOG_SHADER_SRC`].
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FogUniforms {
    m: [[f32; 4]; 4],
    sim: [f32; 4],
    mesh: [f32; 4],
    color: [f32; 4],
    params: [f32; 4],
    consts: [f32; 4],
}

/// The pass's GPU resources.
pub(crate) struct FogVolumePass {
    pipeline: wgpu::RenderPipeline,
    ubuf: wgpu::Buffer,
    tex: wgpu::Texture,
    bg: wgpu::BindGroup,
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    ground_gen: Option<u32>,
    layers: u32,
    sim_dim: u32,
    mesh_dim: u32,
}

impl FogVolumePass {
    fn new(
        device: &wgpu::Device,
        view_format: wgpu::TextureFormat,
        sim_dim: u32,
        mesh_dim: u32,
    ) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("fog volume shader"),
            source: wgpu::ShaderSource::Wgsl(FOG_SHADER_SRC.into()),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog volume bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
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
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fog volume uniforms"),
            size: std::mem::size_of::<FogUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fog volume density"),
            size: wgpu::Extent3d {
                width: sim_dim,
                height: sim_dim,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fog volume sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog volume bg"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: ubuf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog volume layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let attrs = [wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        }];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fog volume pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 12,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &attrs,
                }],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(wgpu::BlendState {
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
                    }),
                    write_mask: wgpu::ColorWrites::COLOR,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            // Reversed-Z: nearer is greater. Tested against the scene, never
            // written, so the sheets sit behind every wall and leg in front
            // of them and never hide one another.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        let n = ((mesh_dim + 1) * (mesh_dim + 1)) as usize;
        let vbuf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("fog volume mesh"),
            size: (n * 12) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let indices = mesh_indices(mesh_dim);
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fog volume indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self {
            pipeline,
            ubuf,
            tex,
            bg,
            vbuf,
            ibuf,
            index_count: indices.len() as u32,
            ground_gen: None,
            layers: 0,
            sim_dim,
            mesh_dim,
        }
    }

    fn stage(&mut self, queue: &wgpu::Queue, d: &FogVolumeDraw<'_>) {
        if self.ground_gen != Some(d.ground_gen) {
            queue.write_buffer(&self.vbuf, 0, bytemuck::cast_slice(d.mesh_positions));
            self.ground_gen = Some(d.ground_gen);
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            d.density,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(self.sim_dim),
                rows_per_image: Some(self.sim_dim),
            },
            wgpu::Extent3d {
                width: self.sim_dim,
                height: self.sim_dim,
                depth_or_array_layers: 1,
            },
        );
        let u = FogUniforms {
            m: reverse_z(d.world_to_clip).to_cols_array_2d(),
            sim: [
                d.sim_origin[0],
                d.sim_origin[1],
                d.sim_cell,
                d.sim_dim as f32,
            ],
            mesh: [
                d.mesh_origin[0],
                d.mesh_origin[1],
                d.mesh_cell,
                d.mesh_dim as f32,
            ],
            color: [d.color[0], d.color[1], d.color[2], d.opacity],
            params: [d.height, d.layers as f32, d.drift[0], d.drift[1]],
            consts: d.shader_constants,
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::cast_slice(&[u]));
        self.layers = d.layers;
    }

    pub(super) fn draw(&self, rp: &mut wgpu::RenderPass<'_>) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.draw_indexed(0..self.index_count, 0, 0..self.layers);
    }
}

impl Renderer {
    /// Stage (or clear) this frame's volumetric ground-fog bank - the
    /// enhancement layer `legaia_engine_core::fog_volume` simulates.
    ///
    /// [`FogVolumeDraw::world_to_clip`] maps the bank's space to clip space,
    /// before the reversed-Z remap the renderer applies itself - the host's
    /// scene camera for the field, the camera times the stage model in
    /// battle.
    ///
    /// Sticky until the next call: hosts call it every frame, `None` when
    /// `World::fog_volume_frame` has no bank.
    /// With `None` the pass draws nothing and, if it was never built, costs
    /// nothing.
    pub fn set_fog_volume(&self, bank: Option<&FogVolumeDraw<'_>>) {
        let Some(d) = bank else {
            self.fog_volume_active.set(false);
            return;
        };
        let n_mesh = ((d.mesh_dim + 1) * (d.mesh_dim + 1) * 3) as usize;
        if d.density.len() != (d.sim_dim * d.sim_dim) as usize || d.mesh_positions.len() != n_mesh {
            self.fog_volume_active.set(false);
            return;
        }
        let mut slot = self.fog_volume_pass.borrow_mut();
        if slot
            .as_ref()
            .is_some_and(|p| p.sim_dim != d.sim_dim || p.mesh_dim != d.mesh_dim)
        {
            *slot = None;
        }
        let pass = slot.get_or_insert_with(|| {
            FogVolumePass::new(&self.device, self.view_format, d.sim_dim, d.mesh_dim)
        });
        pass.stage(&self.queue, d);
        self.fog_volume_active.set(true);
    }

    /// Draw the staged bank into the open scene pass, if one is staged.
    pub(super) fn draw_fog_volume(&self, rp: &mut wgpu::RenderPass<'_>) {
        if !self.fog_volume_active.get() {
            return;
        }
        if let Some(pass) = self.fog_volume_pass.borrow().as_ref() {
            pass.draw(rp);
        }
    }
}
