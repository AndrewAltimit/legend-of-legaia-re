//! The bloom enhancement (NOT retail): the 3D frame's bright pixels - lamp
//! halos, emissive props, lit windows, sunlit sky - bleed a soft glow into
//! their surroundings.
//!
//! With the toggle on ([`Renderer::set_bloom`]) the scene half of a
//! [`RenderTarget::Scene`] frame (the 3D draws, the glow sprites and the
//! volumetric fog) renders into an offscreen colour target instead of the
//! presented one. An extract and four blur passes then run at quarter
//! resolution, and a composite at full resolution:
//!
//! 1. **extract** - four bilinear taps cover each 4x4 block of the scene,
//!    each weighted by the soft-knee brightness law (`bloom_weight`), so
//!    only what is already bright contributes;
//! 2. **blur** - a separable 9-tap Gaussian (five linear taps), horizontal
//!    then vertical, run twice;
//! 3. **composite** - the scene texel plus the blurred glow times the
//!    response's intensity, saturating, written to the presented target.
//!
//! The response - threshold, knee, intensity - is
//! [`legaia_render_kernels::bloom::BloomParams`], derived from the staged
//! enhanced-lighting mood (darker moods bloom lower and stronger) so the
//! play page, which asks the engine for the same value, blooms alike.
//!
//! The screen-space layers (the attached-light pools, sprites, text, the
//! composited ordering-table tail) draw after the composite, straight onto
//! the presented target, so the HUD never blooms.
//!
//! Off, none of this runs and the frame is the single pass it always was:
//! the faithful render is untouched, and nothing here reads or writes the
//! simulation. The browser play page draws the same passes through
//! `site/js/webgl-bloom.js`, a GLSL transcription held to this one by the
//! `bloom_parity` test.

use super::*;

pub use legaia_render_kernels::bloom::{BloomParams, bloom_weight};

/// Bloom targets are the surface divided by this on each axis.
pub const BLOOM_DOWNSCALE: u32 = 4;
/// Horizontal + vertical blur rounds.
pub const BLOOM_BLUR_ROUNDS: usize = 2;

pub(crate) const BLOOM_SHADER_SRC: &str = r#"
struct BloomU {
    // xy = the sampled texture's texel size (1 / width, 1 / height),
    // z = the brightness threshold, w = the soft knee (extract only).
    texel: vec4<f32>,
    // xy = the blur step (texel size along the blur axis), z = the
    // composite's glow gain.
    step: vec4<f32>,
};
@group(0) @binding(0) var<uniform> bu: BloomU;
@group(0) @binding(1) var t_src: texture_2d<f32>;
@group(0) @binding(2) var s_src: sampler;
@group(0) @binding(3) var t_scene: texture_2d<f32>;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle over the whole target; uv runs 0..1 over the visible part.
@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VOut {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var out: VOut;
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    out.uv = vec2<f32>(x, y);
    return out;
}

// Soft-knee brightness law: nothing below threshold - knee, a quadratic
// ease across the knee, linear above the threshold; returned as the share
// of the colour that glows.
fn bloom_weight(c: vec3<f32>, threshold: f32, knee: f32) -> f32 {
    let br = max(c.r, max(c.g, c.b));
    var soft = clamp(br - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 1e-5);
    return max(soft, br - threshold) / max(br, 1e-5);
}

fn bloom_tap(uv: vec2<f32>) -> vec3<f32> {
    let c = textureSampleLevel(t_src, s_src, uv, 0.0).rgb;
    return c * bloom_weight(c, bu.texel.z, bu.texel.w);
}

// Extract: four bilinear taps one source texel off the centre cover the
// 4x4 source block under one quarter-resolution texel. Each tap is weighted
// on its own, so one bright pixel does not lift its dark neighbours.
@fragment
fn fs_extract(in: VOut) -> @location(0) vec4<f32> {
    let o = bu.texel.xy;
    let c = bloom_tap(in.uv + vec2<f32>(-o.x, -o.y))
        + bloom_tap(in.uv + vec2<f32>(o.x, -o.y))
        + bloom_tap(in.uv + vec2<f32>(-o.x, o.y))
        + bloom_tap(in.uv + vec2<f32>(o.x, o.y));
    return vec4<f32>(c * 0.25, 1.0);
}

fn blur_at(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(t_src, s_src, uv, 0.0).rgb;
}

// Separable 9-tap Gaussian as five linear taps along `bu.step.xy`.
@fragment
fn fs_blur(in: VOut) -> @location(0) vec4<f32> {
    let s = bu.step.xy;
    var c = blur_at(in.uv) * 0.2270270270;
    c = c + (blur_at(in.uv + s * 1.3846153846) + blur_at(in.uv - s * 1.3846153846)) * 0.3162162162;
    c = c + (blur_at(in.uv + s * 3.2307692308) + blur_at(in.uv - s * 3.2307692308)) * 0.0702702703;
    return vec4<f32>(c, 1.0);
}

// The glow one composite fragment adds.
fn bloom_term(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(t_src, s_src, uv, 0.0).rgb * bu.step.z;
}

// Composite: the scene texel unchanged plus the glow, saturating - the
// same sum the page's additive blend makes.
@fragment
fn fs_composite(in: VOut) -> @location(0) vec4<f32> {
    let scene = textureLoad(t_scene, vec2<i32>(in.pos.xy), 0).rgb;
    return vec4<f32>(min(scene + bloom_term(in.uv), vec3<f32>(1.0)), 1.0);
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct BloomUniforms {
    texel: [f32; 4],
    step: [f32; 4],
}

/// The size-dependent half: the offscreen scene target, the two
/// quarter-resolution ping-pong targets and the bind groups over them.
struct BloomTargets {
    size: (u32, u32),
    scene_view: wgpu::TextureView,
    a_view: wgpu::TextureView,
    b_view: wgpu::TextureView,
    /// Extract (reads the scene), blur into B (reads A), blur into A (reads
    /// B), composite (reads A).
    bg_extract: wgpu::BindGroup,
    bg_from_a_h: wgpu::BindGroup,
    bg_from_b_v: wgpu::BindGroup,
    bg_composite: wgpu::BindGroup,
}

/// The pass's GPU resources, built on the first bloomed frame.
pub(crate) struct BloomPass {
    extract: wgpu::RenderPipeline,
    blur: wgpu::RenderPipeline,
    composite: wgpu::RenderPipeline,
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Extract, horizontal blur, vertical blur, composite.
    ubufs: [wgpu::Buffer; 4],
    scene_format: wgpu::TextureFormat,
    targets: Option<BloomTargets>,
}

const BLOOM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

impl BloomPass {
    fn new(device: &wgpu::Device, view_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("bloom shader"),
            source: wgpu::ShaderSource::Wgsl(BLOOM_SHADER_SRC.into()),
        });
        let tex_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                tex_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex_entry(3),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("bloom layout"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = |entry: &str, format: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("bloom pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let extract = pipeline("fs_extract", BLOOM_FORMAT);
        let blur = pipeline("fs_blur", BLOOM_FORMAT);
        let composite = pipeline("fs_composite", view_format);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("bloom sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ubufs = std::array::from_fn(|_| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("bloom uniforms"),
                size: std::mem::size_of::<BloomUniforms>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        });
        Self {
            extract,
            blur,
            composite,
            bgl,
            sampler,
            ubufs,
            scene_format: view_format,
            targets: None,
        }
    }

    /// (Re)build the size-dependent targets for a `w x h` surface.
    fn ensure_targets(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, w: u32, h: u32) {
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
        let (qw, qh) = (
            w.div_ceil(BLOOM_DOWNSCALE).max(1),
            h.div_ceil(BLOOM_DOWNSCALE).max(1),
        );
        let scene_view = tex("bloom scene target", w, h, self.scene_format);
        let a_view = tex("bloom target a", qw, qh, BLOOM_FORMAT);
        let b_view = tex("bloom target b", qw, qh, BLOOM_FORMAT);
        let bg = |ubuf: &wgpu::Buffer, src: &wgpu::TextureView| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bloom bg"),
                layout: &self.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: ubuf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(src),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&scene_view),
                    },
                ],
            })
        };
        let (qtx, qty) = (1.0 / qw as f32, 1.0 / qh as f32);
        let blur = [
            BloomUniforms {
                texel: [qtx, qty, 0.0, 0.0],
                step: [qtx, 0.0, 0.0, 0.0],
            },
            BloomUniforms {
                texel: [qtx, qty, 0.0, 0.0],
                step: [0.0, qty, 0.0, 0.0],
            },
        ];
        for (buf, u) in self.ubufs[1..3].iter().zip(blur) {
            queue.write_buffer(buf, 0, bytemuck::cast_slice(&[u]));
        }
        self.targets = Some(BloomTargets {
            size: (w, h),
            bg_extract: bg(&self.ubufs[0], &scene_view),
            bg_from_a_h: bg(&self.ubufs[1], &a_view),
            bg_from_b_v: bg(&self.ubufs[2], &b_view),
            bg_composite: bg(&self.ubufs[3], &a_view),
            scene_view,
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
            label: Some("bloom pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, bg, &[]);
        rp.draw(0..3, 0..1);
    }
}

impl Renderer {
    /// Toggle the **bloom enhancement** (see the module docs). Off by
    /// default, and off draws the frame exactly as without the feature. The
    /// play hosts mirror the persisted `OptionsState::bloom` into it.
    pub fn set_bloom(&self, enable: bool) {
        self.bloom_on.set(enable);
    }

    /// Whether bloom is on.
    pub fn bloom(&self) -> bool {
        self.bloom_on.get()
    }

    /// The offscreen target the scene half of a bloomed frame renders into,
    /// building or resizing the pass's targets first. `None` while bloom is
    /// off.
    pub(super) fn bloom_scene_target(&self) -> Option<wgpu::TextureView> {
        if !self.bloom_on.get() {
            return None;
        }
        let mut slot = self.bloom_pass.borrow_mut();
        let pass = slot.get_or_insert_with(|| BloomPass::new(&self.device, self.view_format));
        let (w, h) = (self.config.width, self.config.height);
        pass.ensure_targets(&self.device, &self.queue, w, h);
        let p = BloomParams::for_frame(&self.lighting_mood.get(), self.dyn_lighting.get());
        let extract = BloomUniforms {
            texel: [1.0 / w as f32, 1.0 / h as f32, p.threshold, p.knee],
            step: [0.0; 4],
        };
        let composite = BloomUniforms {
            texel: [0.0; 4],
            step: [0.0, 0.0, p.intensity, 0.0],
        };
        self.queue
            .write_buffer(&pass.ubufs[0], 0, bytemuck::cast_slice(&[extract]));
        self.queue
            .write_buffer(&pass.ubufs[3], 0, bytemuck::cast_slice(&[composite]));
        pass.targets.as_ref().map(|t| t.scene_view.clone())
    }

    /// Encode extract, blur and composite: from the offscreen scene target
    /// staged by [`Self::bloom_scene_target`] onto `present`.
    pub(super) fn encode_bloom_passes(
        &self,
        enc: &mut wgpu::CommandEncoder,
        present: &wgpu::TextureView,
    ) {
        let slot = self.bloom_pass.borrow();
        let Some(pass) = slot.as_ref() else {
            return;
        };
        let Some(t) = pass.targets.as_ref() else {
            return;
        };
        BloomPass::fullscreen(enc, &pass.extract, &t.bg_extract, &t.a_view);
        for _ in 0..BLOOM_BLUR_ROUNDS {
            BloomPass::fullscreen(enc, &pass.blur, &t.bg_from_a_h, &t.b_view);
            BloomPass::fullscreen(enc, &pass.blur, &t.bg_from_b_v, &t.a_view);
        }
        BloomPass::fullscreen(enc, &pass.composite, &t.bg_composite, present);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloom_shader_parses_and_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(BLOOM_SHADER_SRC)
            .unwrap_or_else(|e| panic!("bloom shader failed to parse: {e:?}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("bloom shader failed to validate: {e:?}"));
    }

    #[test]
    fn uniform_block_matches_the_wgsl_struct() {
        assert_eq!(std::mem::size_of::<BloomUniforms>(), 2 * 16);
    }
}
