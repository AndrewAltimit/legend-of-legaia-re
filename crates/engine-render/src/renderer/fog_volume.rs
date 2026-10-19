//! The volumetric ground-fog **enhancement** pass
//! (`legaia_render_kernels::fog_volume`): the bank drawn as horizontal
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
//! (`legaia_render_kernels::fog_volume::FOG_SHADER_CONSTANTS`). This crate does
//! not depend on `engine-core`, so the host hands the frame over as a
//! [`FogVolumeDraw`] of plain fields.

use super::*;

/// One frame's bank, as the host hands it over - the fields of
/// `legaia_render_kernels::fog_volume::FogVolumeFrame` plus the world -> clip
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
    /// `(mesh_dim + 1)²` vertices of `[x, floor_y, z, floor_weight]`,
    /// row-major (a negative weight marks the distant haze layer) - uploaded
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
    /// `FogSpace::shader_constants`.
    pub shader_constants: [f32; 4],
    /// View-depth span (clip `w` units of `world_to_clip`) over which a
    /// sheet fades into the scene surface behind it - the soft
    /// intersection.
    pub soft_distance: f32,
    /// The spill streaks' flow phase
    /// (`legaia_render_kernels::fog_volume::FogVolumeFrame::spill_phase`).
    pub spill_phase: f32,
    /// The spill and distant-haze shading constants
    /// (`legaia_render_kernels::fog_volume::FOG_SPILL_CONSTANTS` /
    /// `FOG_HAZE_CONSTANTS`).
    pub spill_constants: [f32; 4],
    pub haze_constants: [f32; 4],
    /// The scene's ground coverage field, `None` for full coverage (battle,
    /// or a field scene without one).
    pub ground: Option<FogGroundTexture<'a>>,
}

/// The ground coverage field's texture
/// (`legaia_render_kernels::fog_volume::FogGround::texels`), uploaded only
/// when `ground_gen` changes.
pub struct FogGroundTexture<'a> {
    /// `[origin_x, origin_z, 1 / extent_x, 1 / extent_z]` - world X / Z to
    /// texture UV.
    pub uv_rect: [f32; 4],
    pub w: u32,
    pub h: u32,
    /// `w * h` RGBA8 texels: coverage, spill, flow, unused.
    pub texels: &'a [u8],
    pub ground_gen: u32,
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
pub(crate) const FOG_SHADER_SRC: &str = r#"
struct FogU {
    m: mat4x4<f32>,
    sim: vec4<f32>,
    mesh: vec4<f32>,
    color: vec4<f32>,
    params: vec4<f32>,
    consts: vec4<f32>,
    depth: vec4<f32>,
    ground: vec4<f32>,
    spill: vec4<f32>,
    haze: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: FogU;
@group(0) @binding(1) var dens: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;
@group(0) @binding(3) var gtex: texture_2d<f32>;
@group(1) @binding(0) var scene_depth: texture_depth_2d;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) xz: vec2<f32>,
    @location(1) layer: f32,
    @location(2) clip_w: f32,
    @location(3) world: vec3<f32>,
    @location(4) floor_w: f32,
    @location(5) haze: f32,
    @location(6) view: vec3<f32>,
};

// The spill's outward direction at ground-texture UV `guv`: the gradient of
// the flow channel (distance past the rim), +X where it is flat.
fn spill_dir(guv: vec2<f32>) -> vec2<f32> {
    let e = u.ground.zw * 64.0;
    let fx = textureSampleLevel(gtex, samp, guv + vec2<f32>(e.x, 0.0), 0.0).b
        - textureSampleLevel(gtex, samp, guv - vec2<f32>(e.x, 0.0), 0.0).b;
    let fz = textureSampleLevel(gtex, samp, guv + vec2<f32>(0.0, e.y), 0.0).b
        - textureSampleLevel(gtex, samp, guv - vec2<f32>(0.0, e.y), 0.0).b;
    let glen = length(vec2<f32>(fx, fz));
    if (glen > 1.0e-4) {
        return vec2<f32>(fx, fz) / glen;
    }
    return vec2<f32>(1.0, 0.0);
}

// The camera's position in the bank's space: the one point the matrix
// sends to clip x = y = w = 0 (every view ray passes through it).
fn eye_of(m: mat4x4<f32>) -> vec3<f32> {
    let r0 = vec4<f32>(m[0].x, m[1].x, m[2].x, m[3].x);
    let r1 = vec4<f32>(m[0].y, m[1].y, m[2].y, m[3].y);
    let r3 = vec4<f32>(m[0].w, m[1].w, m[2].w, m[3].w);
    let c01 = cross(r0.xyz, r1.xyz);
    let c13 = cross(r1.xyz, r3.xyz);
    let c30 = cross(r3.xyz, r0.xyz);
    let det = dot(r0.xyz, c13);
    if (abs(det) < 1.0e-12) {
        return vec3<f32>(0.0);
    }
    return -(r0.w * c13 + r1.w * c30 + r3.w * c01) / det;
}

@vertex
fn vs_main(@location(0) p: vec4<f32>, @builtin(instance_index) inst: u32) -> VOut {
    let s = (f32(inst) + 0.5) / u.params.y;
    // Sheets packed toward the floor (quadratic spacing); retail Y-down,
    // so they stack upward from just above it. A negative weight marks the
    // distant haze layer, whose stack is deeper.
    let t = s * s;
    let haze = select(0.0, 1.0, p.w < 0.0);
    let y = p.y - 2.0 - t * u.params.x * mix(1.0, u.haze.x, haze);
    // On the spill's near-vertical curtain a vertical stack slides the
    // sheets along themselves; spread them outward instead, so the fall
    // is a slab with depth rather than one bright plane seen edge-on.
    var xz = p.xz;
    let guv = (p.xz - u.ground.xy) * u.ground.zw;
    let gv = textureSampleLevel(gtex, samp, guv, 0.0);
    if (gv.g > 0.0 && haze == 0.0) {
        xz = xz + spill_dir(guv) * (s * u.spill.w * min(gv.g * 4.0, 1.0));
    }
    var o: VOut;
    o.pos = u.m * vec4<f32>(xz.x, y, xz.y, 1.0);
    o.xz = p.xz;
    o.layer = t;
    o.clip_w = o.pos.w;
    o.world = vec3<f32>(xz.x, y, xz.y);
    o.view = o.world - eye_of(u.m);
    o.floor_w = abs(p.w);
    o.haze = haze;
    return o;
}

fn fog_hash(i: vec2<i32>) -> f32 {
    let x = u32(i.x & 0xffff);
    let y = u32(i.y & 0xffff);
    var h = (x * 0x8da6b343u) ^ (y * 0xd8163841u);
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
    let t = in.layer;
    let q = in.xz * u.consts.x;
    let dq = u.params.zw * u.consts.x;
    // Domain-warped three-octave wisps, each octave drifting at its own rate.
    let w = vec2<f32>(
        fog_noise(q * 0.45 + vec2<f32>(1.7, 9.2) - dq * 0.3),
        fog_noise(q * 0.45 + vec2<f32>(8.3, 2.8) + dq * 0.25),
    ) - vec2<f32>(0.5);
    let p = q + w * 1.8;
    let f = 0.55 * fog_noise(p - dq)
        + 0.30 * fog_noise(p * 2.07 - dq * 1.7 + vec2<f32>(t * 3.1, 0.0))
        + 0.15 * fog_noise(p * 4.31 - dq * 2.6 + vec2<f32>(0.0, t * 5.7));
    let nb = fog_noise((in.xz - u.params.zw * 0.4) * u.consts.y + vec2<f32>(3.1, 8.9));
    // Banks: bright crests and clear gaps, not a uniform wash.
    let crest = smoothstep(0.30, 0.72, f * 0.65 + nb * 0.55);
    // Each crest has its own soft top; the profile thins hard toward it.
    let top = 0.35 + 0.65 * crest;
    let h = clamp(1.0 - t / top, 0.0, 1.0);
    let profile = pow(h, u.consts.w);
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
    // Soft intersection: fade a sheet as it nears the scene surface behind
    // it, in view depth (the scene's NDC depth back through the camera's
    // own depth mapping `ndc = A + B / w`).
    let ds = textureLoad(scene_depth, vec2<i32>(in.pos.xy), 0);
    let den = ds - u.depth.x;
    var ws = 1.0e9;
    if (abs(den) > 1.0e-7) {
        let cand = u.depth.y / den;
        if (cand > 0.0) {
            ws = cand;
        }
    }
    let soft = clamp((ws - in.clip_w) / u.depth.z, 0.0, 1.0);
    // A sheet draped down a cliff between two floor tiers is not a bank: fade
    // it by the surface's steepness (its screen-space normal).
    let nrm = cross(dpdx(in.world), dpdy(in.world));
    let flat_ = abs(nrm.y) / max(length(nrm), 1.0e-6);
    var level = smoothstep(0.55, 0.85, flat_);
    // Ground field: full bank on the floor, a soft fade over its edge, and
    // past a rim the spill - slow tendrils pouring down the falling sheet,
    // parting and thinning with the fall.
    let guv = (in.xz - u.ground.xy) * u.ground.zw;
    let g = textureSampleLevel(gtex, samp, guv, 0.0);
    var ground_a = g.r;
    if (g.g > 0.0) {
        let dir = spill_dir(guv);
        // World units along the rim, and of drop below it (the
        // `spill_drop` curve over the flow coordinate).
        let along = dot(in.xz, vec2<f32>(-dir.y, dir.x));
        let fall01 = g.b * sqrt(g.b);
        let fall = fall01 * u.spill.y;
        let ph = u.depth.w;
        // Pour mask: long stretches of rim pour, others barely do; it
        // evolves in place rather than scrolling.
        let m = fog_noise(vec2<f32>(along * u.spill.z + 4.7, ph * 0.05 + 1.3));
        let pour = 0.12 + 0.88 * smoothstep(0.32, 0.62, m);
        // Tendrils: narrow along the rim, long down the fall, swaying
        // slowly; the second octave flows at its own rate so the pattern
        // changes as it falls.
        let sway = (fog_noise(vec2<f32>(fall * 0.6 - ph * 0.4, along * u.spill.z * 2.0 + 9.1)) - 0.5) * 1.8;
        // Each sheet of the slab a little apart, so the layers do not
        // repeat one pattern.
        let ta = along * u.spill.x + sway + sqrt(t) * 0.9;
        let tn = 0.65 * fog_noise(vec2<f32>(ta, fall - ph))
            + 0.35 * fog_noise(vec2<f32>(ta * 2.1 + 3.3, fall * 1.7 - ph * 1.35 + 5.2));
        // High-frequency erosion: only matters near the threshold, so it
        // rags each tendril's edges and leaves its core.
        let er = fog_noise(vec2<f32>(along * u.spill.x * 4.3 + 1.9, fall * 5.0 - ph * 2.3)) - 0.5;
        let th = 0.34 + 0.24 * fall01;
        let tendril = smoothstep(th, th + 0.3, tn + er * 0.3);
        // Puffs: the tendrils fall as clumps with thin gaps between them,
        // not as unbroken lines.
        let puff = smoothstep(0.3, 0.7, fog_noise(vec2<f32>(along * u.spill.z * 3.7 + 2.0, fall * 1.3 - ph * 0.8 + 6.1)));
        let spill = 1.4 * g.g * pour * tendril * (0.45 + 0.55 * puff) * (0.35 + 0.65 * f);
        ground_a = max(ground_a * pour * (0.4 + 0.6 * tendril), spill);
        // The spill sheet falls steeply on purpose: exempt it from the
        // cliff-face fade, but thin a sheet seen edge-on, where each layer
        // of the slab would draw as one bright line.
        let facing = abs(dot(nrm, in.view)) / max(length(nrm) * length(in.view), 1.0e-6);
        level = mix(level, smoothstep(0.1, 0.45, facing), clamp(g.g * 4.0, 0.0, 1.0) * (1.0 - g.r));
    }
    let a = u.color.a * d * crest * profile * edge * soft * level * ground_a * in.floor_w * u.consts.z / u.params.y;
    // The distant haze: broad, soft banks drifting slower than the bank,
    // thickest mid-stack, faint.
    var haze_a = 0.0;
    if (in.haze > 0.0) {
        let hq = (in.xz - u.params.zw * u.haze.w) * u.haze.z;
        let hn = 0.6 * fog_noise(hq + vec2<f32>(t * 0.9, 2.3))
            + 0.4 * fog_noise(hq * 2.3 + vec2<f32>(7.7, t * 1.7));
        let bank = smoothstep(0.25, 0.8, hn);
        let hp = 4.0 * t * (1.0 - t);
        haze_a = u.color.a * u.haze.y * bank * hp * edge * soft * in.floor_w * in.haze * u.consts.z / u.params.y;
    }
    let rgb = u.color.rgb * mix(0.75 + 0.5 * f, 0.9, in.haze);
    return vec4<f32>(rgb, clamp(a * (1.0 - in.haze) + haze_a, 0.0, 1.0));
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
    depth: [f32; 4],
    ground: [f32; 4],
    spill: [f32; 4],
    haze: [f32; 4],
}

/// The pass's GPU resources.
pub(crate) struct FogVolumePass {
    pipeline: wgpu::RenderPipeline,
    depth_bgl: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    tex: wgpu::Texture,
    /// Kept to rebuild [`Self::bg`] when the ground texture is replaced.
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    ground_tex: wgpu::Texture,
    ground_dim: (u32, u32),
    ground_tex_gen: Option<Option<u32>>,
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
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
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
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("fog volume sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ground_tex = Self::ground_texture(device, 1, 1);
        let bg = Self::bind_group(device, &bgl, &ubuf, &tex, &sampler, &ground_tex);
        let depth_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("fog volume scene depth bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("fog volume layout"),
            bind_group_layouts: &[&bgl, &depth_bgl],
            push_constant_ranges: &[],
        });
        let attrs = [wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x4,
        }];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("fog volume pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 16,
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
            size: (n * 16) as u64,
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
            depth_bgl,
            ubuf,
            tex,
            bgl,
            sampler,
            ground_tex,
            ground_dim: (1, 1),
            ground_tex_gen: None,
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

    /// An RGBA8 ground-field texture of `w x h`.
    fn ground_texture(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fog volume ground field"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    }

    fn bind_group(
        device: &wgpu::Device,
        bgl: &wgpu::BindGroupLayout,
        ubuf: &wgpu::Buffer,
        tex: &wgpu::Texture,
        sampler: &wgpu::Sampler,
        ground: &wgpu::Texture,
    ) -> wgpu::BindGroup {
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        let gview = ground.create_view(&wgpu::TextureViewDescriptor::default());
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog volume bg"),
            layout: bgl,
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
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&gview),
                },
            ],
        })
    }

    /// Upload the frame's ground field (or the one full-coverage texel when
    /// it has none), only when it changed.
    fn stage_ground(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, d: &FogVolumeDraw<'_>) {
        let want = d.ground.as_ref().map(|g| g.ground_gen);
        if self.ground_tex_gen == Some(want) {
            return;
        }
        let none = legaia_render_kernels::fog_volume::FOG_GROUND_NONE_TEXEL;
        let (w, h, texels): (u32, u32, &[u8]) = match &d.ground {
            Some(g) if g.texels.len() == (g.w * g.h * 4) as usize && g.w > 0 && g.h > 0 => {
                (g.w, g.h, g.texels)
            }
            _ => (1, 1, &none),
        };
        if self.ground_dim != (w, h) {
            self.ground_tex = Self::ground_texture(device, w, h);
            self.ground_dim = (w, h);
            self.bg = Self::bind_group(
                device,
                &self.bgl,
                &self.ubuf,
                &self.tex,
                &self.sampler,
                &self.ground_tex,
            );
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.ground_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(w * 4),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.ground_tex_gen = Some(want);
    }

    fn stage(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, d: &FogVolumeDraw<'_>) {
        self.stage_ground(device, queue, d);
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
        let m = reverse_z(d.world_to_clip);
        let (a, b) = depth_mapping(m);
        let u = FogUniforms {
            m: m.to_cols_array_2d(),
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
            depth: [a, b, d.soft_distance.max(1.0e-3), d.spill_phase],
            ground: d.ground.as_ref().map_or([0.0; 4], |g| g.uv_rect),
            spill: d.spill_constants,
            haze: d.haze_constants,
        };
        queue.write_buffer(&self.ubuf, 0, bytemuck::cast_slice(&[u]));
        self.layers = d.layers;
    }

    pub(super) fn draw(&self, rp: &mut wgpu::RenderPass<'_>, depth_bg: &wgpu::BindGroup) {
        rp.set_pipeline(&self.pipeline);
        rp.set_bind_group(0, &self.bg, &[]);
        rp.set_bind_group(1, depth_bg, &[]);
        rp.set_vertex_buffer(0, self.vbuf.slice(..));
        rp.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.draw_indexed(0..self.index_count, 0, 0..self.layers);
    }
}

impl Renderer {
    /// Stage (or clear) this frame's volumetric ground-fog bank - the
    /// enhancement layer `legaia_render_kernels::fog_volume` simulates.
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
        let n_mesh = ((d.mesh_dim + 1) * (d.mesh_dim + 1) * 4) as usize;
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
        pass.stage(&self.device, &self.queue, d);
        self.fog_volume_active.set(true);
    }

    /// Encode the staged bank as a pass of its own between the two halves
    /// of the scene pass: the colour target loaded, the scene depth attached
    /// **read-only** (the sheets still depth-test against it) and bound as a
    /// texture at the same time, which is what the soft intersection reads.
    pub(super) fn encode_fog_volume_pass(
        &self,
        enc: &mut wgpu::CommandEncoder,
        color: &wgpu::TextureView,
        scene_vp: Option<(u32, u32, u32, u32)>,
    ) {
        let slot = self.fog_volume_pass.borrow();
        let Some(pass) = slot.as_ref() else {
            return;
        };
        let depth_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("fog volume scene depth bg"),
            layout: &pass.depth_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&self.depth_view),
            }],
        });
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("fog volume pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: None,
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        if let Some((x, y, w, h)) = scene_vp {
            rp.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
        }
        pass.draw(&mut rp, &depth_bg);
    }
}

/// The camera's depth mapping, `ndc_z = A + B / clip_w`, read off the
/// matrix's third and fourth rows (`z = A * w + B` for a perspective
/// projection: the z row is the w row scaled, plus a constant). The soft
/// intersection inverts it to turn a stored scene depth back into view
/// depth. A degenerate (non-perspective) matrix maps to `(A, 0)`, which the
/// shader reads as "infinitely far" - no fade.
fn depth_mapping(m: Mat4) -> (f32, f32) {
    let r = m.transpose();
    let z = r.z_axis;
    let w = r.w_axis;
    let ww = w.x * w.x + w.y * w.y + w.z * w.z;
    if ww <= 1.0e-12 {
        return (0.0, 0.0);
    }
    let a = (z.x * w.x + z.y * w.y + z.z * w.z) / ww;
    (a, z.w - a * w.w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fog WGSL parses and validates - the GPU-free guard, since the
    /// pipeline itself only builds on a device.
    #[test]
    fn fog_shader_parses_and_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(FOG_SHADER_SRC)
            .unwrap_or_else(|e| panic!("fog shader failed to parse: {e:?}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("fog shader failed to validate: {e:?}"));
    }

    /// The uniform block is the size the WGSL struct declares
    /// (a mat4 and nine vec4s).
    #[test]
    fn uniform_block_matches_the_wgsl_struct() {
        assert_eq!(std::mem::size_of::<FogUniforms>(), 64 + 9 * 16);
    }

    #[test]
    fn mesh_indices_cover_every_quad() {
        let idx = mesh_indices(3);
        assert_eq!(idx.len(), 3 * 3 * 6);
        assert_eq!(*idx.iter().max().unwrap(), 15);
    }
}

#[cfg(test)]
mod depth_mapping_tests {
    use super::*;

    #[test]
    fn inverts_a_reversed_perspective() {
        let proj = reverse_z(Mat4::perspective_rh(1.0, 1.3, 8.0, 50_000.0));
        let view = Mat4::look_at_rh(
            glam::Vec3::new(100.0, -400.0, 900.0),
            glam::Vec3::ZERO,
            glam::Vec3::Y,
        );
        let m = proj * view;
        let (a, b) = depth_mapping(m);
        for p in [
            glam::Vec4::new(0.0, 0.0, 0.0, 1.0),
            glam::Vec4::new(300.0, 50.0, -2000.0, 1.0),
        ] {
            let c = m * p;
            let ndc = c.z / c.w;
            let w = b / (ndc - a);
            assert!((w - c.w).abs() / c.w < 1.0e-3, "{w} vs {}", c.w);
        }
    }
}
