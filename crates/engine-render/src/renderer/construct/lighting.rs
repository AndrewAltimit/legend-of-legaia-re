//! The enhanced-lighting layer: the per-scene point lights with their
//! shadow maps, the depth-only shadow pass, and the additive glow sprites.

use super::*;

/// The point-light layer bound at group 2 of the scene pipelines.
pub(super) struct LightLayerParts {
    pub(super) bgl: wgpu::BindGroupLayout,
    pub(super) buf: wgpu::Buffer,
    pub(super) bg: wgpu::BindGroup,
    /// One render-attachment view per shadow-map layer.
    pub(super) shadow_layer_views: Vec<wgpu::TextureView>,
}

/// Per-scene point-light layer (dynamic-lighting enhancement).
///
/// Shadow-map depth array (one layer per possible light), the lights
/// uniform, and the comparison sampler, bound at group 2 of the SCENE
/// pipelines. The single-mesh pipelines keep the stub shader variant
/// and never see these. All of it is inert (light count staged to 0,
/// no shadow pass encoded) unless dynamic lighting + the shadow
/// sub-toggle are on and the host staged lights.
pub(super) fn build_light_layer(device: &wgpu::Device) -> LightLayerParts {
    let shadow_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scene light shadow maps"),
        size: wgpu::Extent3d {
            width: SHADOW_MAP_DIM,
            height: SHADOW_MAP_DIM,
            depth_or_array_layers: legaia_render_kernels::scene_lighting::MAX_SCENE_LIGHTS as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let shadow_array_view = shadow_tex.create_view(&wgpu::TextureViewDescriptor {
        label: Some("scene light shadow array view"),
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let shadow_layer_views: Vec<wgpu::TextureView> = (0
        ..legaia_render_kernels::scene_lighting::MAX_SCENE_LIGHTS)
        .map(|i| {
            shadow_tex.create_view(&wgpu::TextureViewDescriptor {
                label: Some("scene light shadow layer view"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("scene light shadow sampler"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    let scene_lights_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene lights bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                // The vertex stage reads the per-primitive near reject.
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
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2Array,
                    sample_type: wgpu::TextureSampleType::Depth,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                count: None,
            },
        ],
    });
    let scene_lights_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("scene lights uniform"),
        contents: bytemuck::bytes_of(&<SceneLightsUniform as bytemuck::Zeroable>::zeroed()),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let scene_lights_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene lights bg"),
        layout: &scene_lights_bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: scene_lights_buf.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&shadow_array_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&shadow_sampler),
            },
        ],
    });
    LightLayerParts {
        bgl: scene_lights_bgl,
        buf: scene_lights_buf,
        bg: scene_lights_bg,
        shadow_layer_views,
    }
}

/// The shadow pass: its dynamic-offset uniforms and the two depth-only
/// pipelines.
pub(super) struct ShadowPassParts {
    pub(super) uniforms_bgl: wgpu::BindGroupLayout,
    pub(super) uniforms_buf: wgpu::Buffer,
    pub(super) uniforms_bg: wgpu::BindGroup,
    /// Slots the uniform buffer starts with; grown on demand.
    pub(super) initial_capacity: usize,
    pub(super) vram_pipeline: wgpu::RenderPipeline,
    pub(super) color_pipeline: wgpu::RenderPipeline,
}

/// Shadow-pass uniforms: one light-space MVP per (light, draw),
/// dynamic-offset like the scene uniforms.
pub(super) fn build_shadow_pass(
    device: &wgpu::Device,
    uniform_offset_alignment: u32,
) -> ShadowPassParts {
    let shadow_uniforms_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shadow uniforms bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: std::num::NonZeroU64::new(
                    std::mem::size_of::<ShadowUniforms>() as u64
                ),
            },
            count: None,
        }],
    });
    let initial_shadow_capacity: usize = 1;
    let shadow_uniforms_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shadow uniforms"),
        size: (initial_shadow_capacity * uniform_offset_alignment as usize) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let shadow_uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow uniforms bg"),
        layout: &shadow_uniforms_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &shadow_uniforms_buf,
                offset: 0,
                size: std::num::NonZeroU64::new(std::mem::size_of::<ShadowUniforms>() as u64),
            }),
        }],
    });
    let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia shadow shader"),
        source: wgpu::ShaderSource::Wgsl(SHADOW_MESH_SHADER_SRC.into()),
    });
    let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia shadow pipeline layout"),
        bind_group_layouts: &[&shadow_uniforms_bgl],
        push_constant_ranges: &[],
    });
    // Depth-only pipelines: position attribute only, over each mesh
    // path's native vertex stride. Rasterizer depth bias keeps the
    // casters' own surfaces from self-shadow acne (the shader adds a
    // small compare bias on top - SHADOW_COMPARE_BIAS).
    let make_shadow_pipeline = |label: &'static str, stride: u64| -> wgpu::RenderPipeline {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&shadow_layout),
            vertex: wgpu::VertexState {
                module: &shadow_shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: stride,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x3,
                    }],
                }],
                compilation_options: Default::default(),
            },
            fragment: None,
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };
    let shadow_vram_pipeline = make_shadow_pipeline(
        "legaia shadow pipeline (vram mesh)",
        crate::renderer::VRAM_VERTEX_STRIDE,
    );
    let shadow_color_pipeline = make_shadow_pipeline(
        "legaia shadow pipeline (color mesh)",
        crate::renderer::COLOR_VERTEX_STRIDE,
    );
    ShadowPassParts {
        uniforms_bgl: shadow_uniforms_bgl,
        uniforms_buf: shadow_uniforms_buf,
        uniforms_bg: shadow_uniforms_bg,
        initial_capacity: initial_shadow_capacity,
        vram_pipeline: shadow_vram_pipeline,
        color_pipeline: shadow_color_pipeline,
    }
}

/// The glow-sprite pipeline and its view-projection uniform.
pub(super) struct GlowParts {
    pub(super) uniforms_buf: wgpu::Buffer,
    pub(super) uniforms_bg: wgpu::BindGroup,
    pub(super) pipeline: wgpu::RenderPipeline,
}

/// Glow sprites (enhanced lighting's halos + light shafts): position
/// (12) + falloff uv (8) + additive colour/kind (16) = 36 bytes, one
/// VP uniform, additive blend, depth-tested against the scene pass
/// but never writing depth.
pub(super) fn build_glow_sprites(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
) -> GlowParts {
    let glow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia glow shader"),
        source: wgpu::ShaderSource::Wgsl(GLOW_SHADER_SRC.into()),
    });
    let (glow_uniforms_bgl, glow_uniforms_buf, glow_uniforms_bg) = make_uniform_bind_group(
        device,
        "glow uniforms",
        bytemuck::cast_slice(&[Mat4::IDENTITY.to_cols_array_2d()]),
        wgpu::ShaderStages::VERTEX,
    );
    let glow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia glow pipeline layout"),
        bind_group_layouts: &[&glow_uniforms_bgl],
        push_constant_ranges: &[],
    });
    let additive = wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::One,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
    };
    let glow_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia glow pipeline"),
        layout: Some(&glow_layout),
        vertex: wgpu::VertexState {
            module: &glow_shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: GLOW_VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x3,
                    },
                    wgpu::VertexAttribute {
                        offset: 12,
                        shader_location: 1,
                        format: wgpu::VertexFormat::Float32x2,
                    },
                    wgpu::VertexAttribute {
                        offset: 20,
                        shader_location: 2,
                        format: wgpu::VertexFormat::Float32x4,
                    },
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &glow_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: view_format,
                blend: Some(wgpu::BlendState {
                    color: additive,
                    alpha: additive,
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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Greater,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    GlowParts {
        uniforms_buf: glow_uniforms_buf,
        uniforms_bg: glow_uniforms_bg,
        pipeline: glow_pipeline,
    }
}
