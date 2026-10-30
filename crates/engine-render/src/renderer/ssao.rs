//! Screen-space ambient occlusion (an enhancement, NOT retail) over a thin
//! G-buffer: contact shadows where geometry closes over a surface - a
//! wall's foot, the floor under a table, the ground at a character's feet.
//!
//! Shading stays forward. With the toggle on ([`Renderer::set_ssao`]) and a
//! scene camera staged ([`Renderer::set_ssao_camera`]), a
//! [`RenderTarget::Scene`] frame gains:
//!
//! 1. **pre-pass** - the opaque scene draws (textured and untextured, the
//!    same uniform slots and the same vertex entry as the main pass, so the
//!    same fragment wins every pixel) into one `Rgba32Float` target:
//!    `rgb` = the view-space face normal, `a` = the linear view depth
//!    (clip `w`). The fragment entries repeat the opaque entries' discards
//!    (effect clip, NCLIP, double-sided copies, the occlusion fade's screen
//!    door, transparent texels, the STP texels a semi prim defers), so the
//!    G-buffer covers exactly the opaque fragments the frame shows.
//!    Normals are flat face normals from the derivatives of the
//!    reconstructed view position - the TMDs author normals only on their
//!    lit rows, and the depth tricks that keep retail's draw order
//!    (`field_far_bucket_depth` and kin) move clip `z`, never `w`;
//! 2. **AO** - the Alchemy estimator over a golden-angle spiral at full
//!    resolution up to a 540-row output (the native 320 x 240 target), half
//!    above it;
//! 3. **blur** - one separable depth- and normal-aware pass pair;
//! 4. **apply** - inside the main pass, right after the opaque draws: a
//!    full-screen triangle that multiplies the framebuffer by the AO (a
//!    joint-bilateral upsample when AO ran at half resolution). The
//!    semi-transparent prims, the glow sprites, the fog, the bloom and every
//!    screen-space layer draw after it, so none of them darkens.
//!
//! Off, none of this runs: no pass is encoded, no pipeline is built, and
//! the frame is the one it always was. The numbers and the per-sample law
//! are [`legaia_render_kernels::ssao`], which the browser twin shares.

use super::*;

pub use legaia_render_kernels::ssao::SsaoParams;

/// The G-buffer's format: view normal + linear depth.
pub(crate) const GBUF_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
const AO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;

/// The frame block every SSAO stage reads, and its view-position
/// reconstruction. Shared by the pre-pass entries (at group 3) and the
/// AO / blur / apply passes (at group 0).
pub(crate) const SSAO_FRAME_WGSL: &str = r#"
struct SsaoFrame {
    // (kx, ky, kw, 0): `legaia_render_kernels::ssao::view_scale`.
    view_k: vec4<f32>,
    // The scene viewport in target pixels (x, y, w, h).
    viewport: vec4<f32>,
    // (radius, bias, intensity, power).
    p0: vec4<f32>,
    // (min_ao, max_radius_frac, min_radius_px, samples).
    p1: vec4<f32>,
    // (blur_radius, blur_depth_sigma, blur_normal_power, blur_sigma).
    p2: vec4<f32>,
    // xy = the blur axis (AO texels), z = the AO downscale, w = 1 for the
    // apply pass's AO-only debug view.
    pass_info: vec4<f32>,
};

// A target pixel's view-space position in world units from its linear
// depth `w` (`view_position` in the kernel).
fn ssao_view_pos(f: SsaoFrame, px: vec2<f32>, w: f32) -> vec3<f32> {
    let ndc = vec2<f32>(
        (px.x - f.viewport.x) / f.viewport.z * 2.0 - 1.0,
        1.0 - (px.y - f.viewport.y) / f.viewport.w * 2.0,
    );
    return vec3<f32>(ndc.x * w * f.view_k.x, ndc.y * w * f.view_k.y, w * f.view_k.z);
}
"#;

/// The pre-pass's G-buffer word for one fragment: the face normal from the
/// derivatives of the reconstructed view position, turned to face the
/// camera, and the linear depth. Called ahead of any discard so the
/// derivatives are taken in uniform control flow.
const SSAO_PREPASS_COMMON_WGSL: &str = r#"
@group(3) @binding(0) var<uniform> ssao_frame: SsaoFrame;

fn ssao_gbuffer_word(frag: vec4<f32>) -> vec4<f32> {
    let w = 1.0 / max(frag.w, 1e-8);
    let p = ssao_view_pos(ssao_frame, frag.xy, w);
    var n = cross(dpdx(p), dpdy(p));
    let len = length(n);
    if (len < 1e-12) {
        n = -normalize(p);
    } else {
        n = n / len;
        if (dot(n, p) > 0.0) {
            n = -n;
        }
    }
    return vec4<f32>(n, w);
}
"#;

/// The textured pre-pass entry: `fs_main`'s discards, then the G-buffer
/// word. Keep the discard chain in step with `fs_main` in
/// `VRAM_MESH_SHADER_SRC`.
const SSAO_PREPASS_VRAM_WGSL: &str = r#"
@fragment
fn fs_ssao_prepass(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let uv_dy = dpdy(in.uv_affine);
    let bary_dy = dpdy(in.prim_bary);
    let uv_dx = dpdx(in.uv_affine);
    let bary_dx = dpdx(in.prim_bary);
    let g = ssao_gbuffer_word(in.clip_pos);
    if effect_clipped(in.world_pos) {
        discard;
    }
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
        discard;
    }
    if (in.cba_tsb.x & 0x8000u) != 0u && front_facing {
        discard;
    }
    if (u.flags.z > 0.5
        && occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)) {
        discard;
    }
    let tsb = in.cba_tsb.y;
    let cba = in.cba_tsb.x;
    var word = fetch_vram_word(psx_texel_uv(in.uv_affine, uv_dx, uv_dy, in.prim_bary, bary_dx, bary_dy), cba, tsb);
    if u.palette.w > 0.5 && palette_law_reaches(cba, tsb) {
        word = palette_law_word(word);
    }
    if bgr555_to_rgba(word).a <= 0.0 {
        discard;
    }
    if u.flags.y >= 0.5 && (tsb & 0x8000u) != 0u && ((word >> 15u) & 1u) == 1u {
        discard;
    }
    return g;
}
"#;

/// The untextured pre-pass entry: the colour mesh's `fs_main` discards.
const SSAO_PREPASS_COLOR_WGSL: &str = r#"
@fragment
fn fs_ssao_prepass(in: VsOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let g = ssao_gbuffer_word(in.clip_pos);
    if effect_clipped(in.world_pos) {
        discard;
    }
    if (u.flags.x >= 0.5 && u.flags.x < 1.5 && !front_facing)
        || (u.flags.x >= 1.5 && front_facing) {
        discard;
    }
    if ((in.blend & 0x4000u) != 0u && front_facing) {
        discard;
    }
    if (u.flags.y >= 0.5 && (in.blend & 0x8000u) != 0u) {
        discard;
    }
    if (u.flags.z > 0.5
        && occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)) {
        discard;
    }
    return g;
}
"#;

/// The AO, blur and apply passes. Each helper mirrors the function of the
/// same name in `legaia_render_kernels::ssao`.
pub(crate) const SSAO_PASS_WGSL: &str = r#"
@group(0) @binding(0) var<uniform> f: SsaoFrame;
@group(0) @binding(1) var t_gbuf: texture_2d<f32>;
@group(0) @binding(2) var t_ao: texture_2d<f32>;

struct VOut {
    @builtin(position) pos: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var out: VOut;
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

const TAU: f32 = 6.2831853;
const GOLDEN_ANGLE: f32 = 2.3999632;

fn spin(px: vec2<f32>) -> f32 {
    return fract(52.982918 * fract(0.06711056 * px.x + 0.00583715 * px.y));
}

fn sample_offset(i: u32, n: u32, s: f32) -> vec2<f32> {
    let t = (f32(i) + 0.5) / f32(max(n, 1u));
    let r = sqrt(t);
    let a = f32(i) * GOLDEN_ANGLE + s * TAU;
    return vec2<f32>(r * cos(a), r * sin(a));
}

fn radius_px(view_z: f32) -> f32 {
    if (view_z <= 0.0 || f.view_k.y <= 0.0) {
        return 0.0;
    }
    let px = f.p0.x * 0.5 * f.viewport.w * f.view_k.z / (f.view_k.y * view_z);
    return min(px, f.p1.y * f.viewport.w);
}

fn occlusion_term(v: vec3<f32>, n: vec3<f32>, view_z: f32) -> f32 {
    let vv = dot(v, v);
    let r2 = f.p0.x * f.p0.x;
    let falloff = max(1.0 - vv / r2, 0.0);
    let num = max(dot(v, n) - f.p0.y * view_z, 0.0);
    return f.p0.x * num / (vv + 0.01 * r2) * falloff;
}

fn ao_from_sum(sum: f32, n: u32) -> f32 {
    let a = clamp(1.0 - f.p0.z * sum / f32(max(n, 1u)), 0.0, 1.0);
    return max(pow(a, f.p0.w), f.p1.x);
}

fn blur_weight(t: f32, z0: f32, z: f32, n0: vec3<f32>, n: vec3<f32>) -> f32 {
    if (z <= 0.0) {
        return 0.0;
    }
    let g = exp(-(t * t) / (2.0 * f.p2.w * f.p2.w));
    let dz = abs(z - z0) / (max(z0, 1e-3) * f.p2.y);
    let dw = exp(-(dz * dz));
    let nw = pow(max(dot(n0, n), 0.0), f.p2.z);
    return g * dw * nw;
}

fn gbuf_dims() -> vec2<i32> {
    return vec2<i32>(textureDimensions(t_gbuf));
}

fn ao_dims() -> vec2<i32> {
    return vec2<i32>(textureDimensions(t_ao));
}

// The full-resolution pixel an AO texel stands for.
fn texel_pixel(t: vec2<i32>) -> vec2<i32> {
    let d = i32(f.pass_info.z);
    return clamp(t * d + vec2<i32>(d / 2), vec2<i32>(0), gbuf_dims() - vec2<i32>(1));
}

@fragment
fn fs_ao(in: VOut) -> @location(0) vec4<f32> {
    let c = texel_pixel(vec2<i32>(in.pos.xy));
    let g = textureLoad(t_gbuf, c, 0);
    if (g.w <= 0.0) {
        return vec4<f32>(1.0);
    }
    let pc = vec2<f32>(c) + vec2<f32>(0.5);
    let p = ssao_view_pos(f, pc, g.w);
    let r = radius_px(p.z);
    if (r < f.p1.z) {
        return vec4<f32>(1.0);
    }
    let n = g.xyz;
    let ns = u32(f.p1.w);
    let s = spin(floor(in.pos.xy));
    let hi = gbuf_dims() - vec2<i32>(1);
    var sum = 0.0;
    for (var i = 0u; i < ns; i = i + 1u) {
        let q = clamp(vec2<i32>(round(pc + sample_offset(i, ns, s) * r)), vec2<i32>(0), hi);
        let gq = textureLoad(t_gbuf, q, 0);
        if (gq.w <= 0.0) {
            continue;
        }
        let v = ssao_view_pos(f, vec2<f32>(q) + vec2<f32>(0.5), gq.w) - p;
        sum = sum + occlusion_term(v, n, p.z);
    }
    return vec4<f32>(ao_from_sum(sum, ns));
}

@fragment
fn fs_blur(in: VOut) -> @location(0) vec4<f32> {
    let t = vec2<i32>(in.pos.xy);
    let g0 = textureLoad(t_gbuf, texel_pixel(t), 0);
    let a0 = textureLoad(t_ao, t, 0).r;
    if (g0.w <= 0.0) {
        return vec4<f32>(a0);
    }
    let axis = vec2<i32>(f.pass_info.xy);
    let hi = ao_dims() - vec2<i32>(1);
    let taps = i32(f.p2.x);
    var sum = a0;
    var wsum = 1.0;
    for (var k = -taps; k <= taps; k = k + 1) {
        if (k == 0) {
            continue;
        }
        let tt = clamp(t + axis * k, vec2<i32>(0), hi);
        let g = textureLoad(t_gbuf, texel_pixel(tt), 0);
        let w = blur_weight(f32(k), g0.w, g.w, g0.xyz, g.xyz);
        sum = sum + textureLoad(t_ao, tt, 0).r * w;
        wsum = wsum + w;
    }
    return vec4<f32>(sum / wsum);
}

// The AO at a full-resolution pixel: the texel itself at full resolution,
// a joint-bilateral blend of the four nearest texels at half (bilinear
// weights times the blur's depth and normal terms against this pixel).
fn ao_at(px: vec2<i32>, g: vec4<f32>) -> f32 {
    let d = f.pass_info.z;
    if (d < 1.5) {
        return textureLoad(t_ao, px, 0).r;
    }
    let base = (vec2<f32>(px) + vec2<f32>(0.5)) / d - vec2<f32>(0.5);
    let i0 = vec2<i32>(floor(base));
    let fr = base - floor(base);
    let hi = ao_dims() - vec2<i32>(1);
    var sum = 0.0;
    var wsum = 0.0;
    for (var j = 0; j < 4; j = j + 1) {
        let o = vec2<i32>(j & 1, j >> 1u);
        let t = clamp(i0 + o, vec2<i32>(0), hi);
        let bx = select(1.0 - fr.x, fr.x, o.x == 1);
        let by = select(1.0 - fr.y, fr.y, o.y == 1);
        let gt = textureLoad(t_gbuf, texel_pixel(t), 0);
        let w = bx * by * blur_weight(0.0, g.w, gt.w, g.xyz, gt.xyz) + 1e-5 * bx * by;
        sum = sum + textureLoad(t_ao, t, 0).r * w;
        wsum = wsum + w;
    }
    return sum / max(wsum, 1e-8);
}

// Apply: the multiplier the blend state multiplies the framebuffer by
// (`dst * src`). Pixels the pre-pass left empty (sky, backdrops, the
// letterbox) take 1.
@fragment
fn fs_apply(in: VOut) -> @location(0) vec4<f32> {
    let px = vec2<i32>(in.pos.xy);
    let g = textureLoad(t_gbuf, px, 0);
    var a = 1.0;
    if (g.w > 0.0) {
        a = ao_at(px, g);
    }
    return vec4<f32>(a, a, a, 1.0);
}
"#;

/// The pre-pass shader sources: the scene shader with the frame block and
/// the pre-pass entry appended (WGSL declarations are order-free).
pub(crate) fn prepass_vram_source() -> String {
    format!(
        "{}\n{SSAO_FRAME_WGSL}\n{SSAO_PREPASS_COMMON_WGSL}\n{SSAO_PREPASS_VRAM_WGSL}",
        compose_scene_lit_shader(VRAM_MESH_SHADER_SRC)
    )
}

pub(crate) fn prepass_color_source() -> String {
    format!(
        "{}\n{SSAO_FRAME_WGSL}\n{SSAO_PREPASS_COMMON_WGSL}\n{SSAO_PREPASS_COLOR_WGSL}",
        compose_scene_lit_shader(COLOR_MESH_SHADER_SRC)
    )
}

pub(crate) fn pass_source() -> String {
    format!("{SSAO_FRAME_WGSL}\n{SSAO_PASS_WGSL}")
}

#[repr(C)]
#[derive(Clone, Copy, Default, bytemuck::Pod, bytemuck::Zeroable)]
struct SsaoFrameUniform {
    view_k: [f32; 4],
    viewport: [f32; 4],
    p0: [f32; 4],
    p1: [f32; 4],
    p2: [f32; 4],
    pass_info: [f32; 4],
}

/// The size-dependent half: the G-buffer, the two AO ping-pong targets and
/// the bind groups over them.
struct SsaoTargets {
    size: (u32, u32),
    downscale: u32,
    gbuf_view: wgpu::TextureView,
    a_view: wgpu::TextureView,
    b_view: wgpu::TextureView,
    /// AO (reads the G-buffer), blur into B (reads A), blur into A (reads
    /// B), apply (reads A).
    bg_ao: wgpu::BindGroup,
    bg_h: wgpu::BindGroup,
    bg_v: wgpu::BindGroup,
    bg_apply: wgpu::BindGroup,
}

/// The pass's GPU resources, built on the first frame SSAO runs.
pub(crate) struct SsaoPass {
    prepass_vram: wgpu::RenderPipeline,
    prepass_color: wgpu::RenderPipeline,
    ao: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    apply: wgpu::RenderPipeline,
    apply_debug: wgpu::RenderPipeline,
    pass_bgl: wgpu::BindGroupLayout,
    /// AO, horizontal blur, vertical blur, apply (the pre-pass reads the
    /// AO block's frame words through `prepass_bg`).
    ubufs: [wgpu::Buffer; 4],
    prepass_bg: wgpu::BindGroup,
    targets: Option<SsaoTargets>,
}

impl SsaoPass {
    fn new(r: &Renderer) -> Self {
        let device = &r.device;
        let uniform_entry = |visibility| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let prepass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao prepass frame bgl"),
            entries: &[uniform_entry(wgpu::ShaderStages::FRAGMENT)],
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let pass_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao pass bgl"),
            entries: &[
                uniform_entry(wgpu::ShaderStages::FRAGMENT),
                tex_entry(1),
                tex_entry(2),
            ],
        });
        let ubufs: [wgpu::Buffer; 4] = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ssao frame uniforms"),
                size: std::mem::size_of::<SsaoFrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        let prepass_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ssao prepass frame bg"),
            layout: &prepass_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: ubufs[0].as_entire_binding(),
            }],
        });

        // Pre-pass: the scene layouts plus the frame block at group 3.
        let prepass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssao prepass layout"),
            bind_group_layouts: &[
                Some(&r.scene_uniforms_bgl),
                Some(&r.vram_bgl),
                Some(&r.scene_lights_bgl),
                Some(&prepass_bgl),
            ],
            immediate_size: 0,
        });
        let prepass = |label, src: String, layout: wgpu::VertexBufferLayout<'_>| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&prepass_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[Some(layout)],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_ssao_prepass"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: GBUF_FORMAT,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(true),
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let vram_attrs = construct::vram_vertex_attributes();
        let color_attrs = construct::color_mesh_vertex_attributes();
        let prepass_vram = prepass(
            "ssao prepass (vram mesh)",
            prepass_vram_source(),
            construct::vram_vertex_layout(&vram_attrs),
        );
        let prepass_color = prepass(
            "ssao prepass (colour mesh)",
            prepass_color_source(),
            construct::color_mesh_vertex_layout(&color_attrs),
        );

        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ssao passes"),
            source: wgpu::ShaderSource::Wgsl(pass_source().into()),
        });
        let pass_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ssao pass layout"),
            bind_group_layouts: &[Some(&pass_bgl)],
            immediate_size: 0,
        });
        let full = |entry: &str,
                    format: wgpu::TextureFormat,
                    blend: Option<wgpu::BlendState>,
                    depth: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ssao pass"),
                layout: Some(&pass_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                // The apply draw runs inside the main pass, which carries the
                // depth attachment: test nothing, write nothing.
                depth_stencil: depth.then_some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(wgpu::CompareFunction::Always),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        // `framebuffer * ao`, alpha kept.
        let multiply = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Dst,
                dst_factor: wgpu::BlendFactor::Zero,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        Self {
            prepass_vram,
            prepass_color,
            ao: full("fs_ao", AO_FORMAT, None, false),
            blur: full("fs_blur", AO_FORMAT, None, false),
            apply: full("fs_apply", r.view_format, Some(multiply), true),
            apply_debug: full("fs_apply", r.view_format, None, true),
            pass_bgl,
            ubufs,
            prepass_bg,
            targets: None,
        }
    }

    /// (Re)build the size-dependent targets for a `w x h` target.
    fn ensure_targets(&mut self, device: &wgpu::Device, w: u32, h: u32) {
        if self.targets.as_ref().is_some_and(|t| t.size == (w, h)) {
            return;
        }
        let tex = |label, tw: u32, th: u32, format| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: tw.max(1),
                        height: th.max(1),
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };
        let d = legaia_render_kernels::ssao::ao_downscale(w, h);
        let (aw, ah) = (w.div_ceil(d), h.div_ceil(d));
        let gbuf_view = tex("ssao g-buffer", w, h, GBUF_FORMAT);
        let a_view = tex("ssao ao a", aw, ah, AO_FORMAT);
        let b_view = tex("ssao ao b", aw, ah, AO_FORMAT);
        let bg = |ubuf: &wgpu::Buffer, ao: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ssao pass bg"),
                layout: &self.pass_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ubuf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&gbuf_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(ao),
                    },
                ],
            })
        };
        self.targets = Some(SsaoTargets {
            size: (w, h),
            downscale: d,
            // The AO pass reads no AO; B is bound only to fill the slot.
            bg_ao: bg(&self.ubufs[0], &b_view),
            bg_h: bg(&self.ubufs[1], &a_view),
            bg_v: bg(&self.ubufs[2], &b_view),
            bg_apply: bg(&self.ubufs[3], &a_view),
            gbuf_view,
            a_view,
            b_view,
        });
    }

    fn fullscreen(
        enc: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::RenderPipeline,
        bg: &wgpu::BindGroup,
        target: &wgpu::TextureView,
    ) {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ssao pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            multiview_mask: None,
            timestamp_writes: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, bg, &[]);
        rp.draw(0..3, 0..1);
    }
}

/// Whether a frame runs SSAO, and the view-space reconstruction factors it
/// runs with: the toggle on and a scene camera staged whose rows are not
/// degenerate. `None` - the renderer default, every retail-pinned host -
/// encodes nothing.
pub(crate) fn ssao_gate(on: bool, camera: Option<Mat4>) -> Option<[f32; 3]> {
    if !on {
        return None;
    }
    let k = legaia_render_kernels::ssao::view_scale(&camera?.to_cols_array());
    (!k.contains(&0.0)).then_some(k)
}

impl Renderer {
    /// Toggle the **SSAO enhancement** (see the module docs). Off by default,
    /// and off draws the frame exactly as without the feature. The play
    /// window mirrors the persisted `OptionsState::ssao` into it.
    pub fn set_ssao(&self, enable: bool) {
        self.ssao_on.set(enable);
    }

    /// Whether SSAO is on.
    pub fn ssao(&self) -> bool {
        self.ssao_on.get()
    }

    /// Stage this frame's scene camera (world to clip, before the reversed-Z
    /// remap) for SSAO's view-space reconstruction; `None` leaves the pass
    /// inert for the frame. Held until the next call.
    pub fn set_ssao_camera(&self, world_to_clip: Option<Mat4>) {
        self.ssao_camera.set(world_to_clip);
    }

    /// Replace the SSAO response (the shared defaults otherwise).
    pub fn set_ssao_params(&self, params: SsaoParams) {
        self.ssao_params.set(params);
    }

    /// Draw the AO term alone (white unoccluded, darker occluded) in place
    /// of the scene's opaque colour - a tuning view, not a player mode.
    pub fn set_ssao_debug_view(&self, enable: bool) {
        self.ssao_debug.set(enable);
    }

    /// Whether this frame runs SSAO: on, a camera staged and a usable
    /// projection. Builds the pass and sizes its targets, and writes the
    /// frame blocks.
    pub(super) fn ssao_stage(&self, scene_vp: Option<(u32, u32, u32, u32)>) -> bool {
        let Some(k) = ssao_gate(self.ssao_on.get(), self.ssao_camera.get()) else {
            return false;
        };
        let mut slot = self.ssao_pass.borrow_mut();
        let pass = slot.get_or_insert_with(|| SsaoPass::new(self));
        let (w, h) = self.target_size();
        pass.ensure_targets(&self.device, w, h);
        let d = pass.targets.as_ref().map_or(1, |t| t.downscale) as f32;
        let (vx, vy, vw, vh) = scene_vp.unwrap_or((0, 0, w, h));
        let [p0, p1, p2] = legaia_render_kernels::ssao::shader_words(&self.ssao_params.get());
        let base = SsaoFrameUniform {
            view_k: [k[0], k[1], k[2], 0.0],
            viewport: [vx as f32, vy as f32, vw as f32, vh as f32],
            p0,
            p1,
            p2,
            pass_info: [0.0, 0.0, d, 0.0],
        };
        let debug = if self.ssao_debug.get() { 1.0 } else { 0.0 };
        let blocks = [
            base,
            SsaoFrameUniform {
                pass_info: [1.0, 0.0, d, 0.0],
                ..base
            },
            SsaoFrameUniform {
                pass_info: [0.0, 1.0, d, 0.0],
                ..base
            },
            SsaoFrameUniform {
                pass_info: [0.0, 0.0, d, debug],
                ..base
            },
        ];
        for (buf, b) in pass.ubufs.iter().zip(blocks) {
            self.queue.write_buffer(buf, 0, bytemuck::cast_slice(&[b]));
        }
        true
    }

    /// Encode the pre-pass and the AO + blur passes for `scene`, ahead of
    /// the main pass. The pre-pass borrows the main depth target, which the
    /// main pass clears again.
    pub(super) fn encode_ssao_passes(
        &self,
        enc: &mut wgpu::CommandEncoder,
        scene: &Scene<'_>,
        scene_vp: Option<(u32, u32, u32, u32)>,
    ) {
        let slot = self.ssao_pass.borrow();
        let Some(pass) = slot.as_ref() else {
            return;
        };
        let Some(t) = pass.targets.as_ref() else {
            return;
        };
        {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ssao prepass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.gbuf_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(DEPTH_CLEAR),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                multiview_mask: None,
                timestamp_writes: None,
            });
            if let Some((x, y, w, h)) = scene_vp {
                rp.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
            }
            let bg_borrow = self.scene_uniforms_bg.borrow();
            let bg: &wgpu::BindGroup = &bg_borrow;
            let stride = self.uniform_offset_alignment;
            rp.set_bind_group(1, &scene.vram.bind_group, &[]);
            rp.set_bind_group(2, &self.scene_lights_bg, &[]);
            rp.set_bind_group(3, &pass.prepass_bg, &[]);
            rp.set_pipeline(&pass.prepass_vram);
            for (i, draw) in scene.draws.iter().enumerate() {
                rp.set_bind_group(0, bg, &[(i as u32) * stride]);
                rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                rp.set_index_buffer(draw.mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
            }
            if !scene.color_draws.is_empty() {
                let color_base = scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                rp.set_pipeline(&pass.prepass_color);
                for (i, draw) in scene.color_draws.iter().enumerate() {
                    rp.set_bind_group(0, bg, &[(color_base + i as u32) * stride]);
                    rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(draw.mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                }
            }
        }
        SsaoPass::fullscreen(enc, &pass.ao, &t.bg_ao, &t.a_view);
        SsaoPass::fullscreen(enc, &pass.blur, &t.bg_h, &t.b_view);
        SsaoPass::fullscreen(enc, &pass.blur, &t.bg_v, &t.a_view);
    }

    /// Draw the apply triangle into the open main pass: the framebuffer
    /// times the AO over the opaque draws (the AO alone in the debug view).
    pub(super) fn draw_ssao_apply(&self, rp: &mut wgpu::RenderPass<'_>) {
        let slot = self.ssao_pass.borrow();
        let (Some(pass), Some(t)) = (
            slot.as_ref(),
            slot.as_ref().and_then(|p| p.targets.as_ref()),
        ) else {
            return;
        };
        let pipeline = if self.ssao_debug.get() {
            &pass.apply_debug
        } else {
            &pass.apply
        };
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &t.bg_apply, &[]);
        rp.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(label: &str, src: &str) {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(src)
            .unwrap_or_else(|e| panic!("{label} failed to parse: {}", e.emit_to_string(src)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{label} failed to validate: {e:?}"));
    }

    #[test]
    fn ssao_shaders_parse_and_validate() {
        validate("ssao prepass (vram)", &prepass_vram_source());
        validate("ssao prepass (colour)", &prepass_color_source());
        validate("ssao passes", &pass_source());
    }

    /// Off is the frame without SSAO: the gate the encoder asks refuses
    /// every frame while the toggle is off (the renderer's default, which
    /// `--retail` and so every retail-compare child keeps), and a frame with
    /// no staged camera.
    #[test]
    fn the_gate_is_closed_off_and_without_a_camera() {
        let cam = glam::camera::rh::proj::directx::perspective(1.0, 4.0 / 3.0, 1.0, 1000.0)
            * glam::camera::rh::view::look_at_mat4(
                glam::Vec3::new(0.0, 300.0, 600.0),
                glam::Vec3::ZERO,
                glam::Vec3::Y,
            );
        assert_eq!(ssao_gate(false, Some(cam)), None);
        assert_eq!(ssao_gate(true, None), None);
        assert_eq!(ssao_gate(true, Some(Mat4::ZERO)), None);
        assert!(ssao_gate(true, Some(cam)).is_some());
    }

    /// A renderer starts with SSAO off: a host that never asks for it (the
    /// asset viewer, the parity oracles) renders without it.
    #[test]
    fn the_renderer_default_is_off() {
        let construct = include_str!("construct.rs");
        assert!(construct.contains("ssao_on: std::cell::Cell::new(false)"));
    }

    #[test]
    fn frame_block_matches_the_wgsl_struct() {
        assert_eq!(std::mem::size_of::<SsaoFrameUniform>(), 6 * 16);
    }

    /// The pre-pass shares the scene shaders' vertex entry and appends to
    /// their source: the main pipelines' own sources carry no SSAO text, so
    /// a renderer with SSAO off compiles exactly what it did without it.
    #[test]
    fn the_scene_shaders_carry_no_ssao_text() {
        for src in [
            compose_scene_lit_shader(VRAM_MESH_SHADER_SRC),
            compose_scene_lit_shader(COLOR_MESH_SHADER_SRC),
        ] {
            assert!(!src.contains("ssao"), "scene shader names ssao");
        }
        assert!(prepass_vram_source().starts_with(&compose_scene_lit_shader(VRAM_MESH_SHADER_SRC)));
        assert!(
            prepass_color_source().starts_with(&compose_scene_lit_shader(COLOR_MESH_SHADER_SRC))
        );
        {}
    }

    /// The pre-pass entries keep the opaque entries' discards: every
    /// `discard` condition of the textured and untextured opaque entry
    /// appears in the pre-pass entry that shadows it.
    #[test]
    fn the_prepass_keeps_the_opaque_discards() {
        let vram_tests = [
            "effect_clipped(in.world_pos)",
            "(in.cba_tsb.x & 0x8000u) != 0u && front_facing",
            "occl_bayer(in.clip_pos.xy) >= occl_keep(in.clip_pos.xy, in.clip_pos.w)",
            "(tsb & 0x8000u) != 0u && ((word >> 15u) & 1u) == 1u",
        ];
        for t in vram_tests {
            assert!(VRAM_MESH_SHADER_SRC.contains(t), "fs_main lost {t}");
            assert!(SSAO_PREPASS_VRAM_WGSL.contains(t), "prepass lacks {t}");
        }
        let color_tests = [
            "effect_clipped(in.world_pos)",
            "(in.blend & 0x4000u) != 0u && front_facing",
            "(in.blend & 0x8000u) != 0u",
        ];
        for t in color_tests {
            assert!(COLOR_MESH_SHADER_SRC.contains(t), "colour fs_main lost {t}");
            assert!(
                SSAO_PREPASS_COLOR_WGSL.contains(t),
                "colour prepass lacks {t}"
            );
        }
    }
}
