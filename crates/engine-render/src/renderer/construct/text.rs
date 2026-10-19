//! The text overlay: 2D textured quads in NDC over the font atlas, opaque
//! and through each PSX semi-transparency mode, with their quad buffers.

use super::*;

/// The text pipelines, the atlas layout + sampler, and the quad buffers.
pub(super) struct TextParts {
    /// The alpha pipeline, indexed by `UI_QUANTIZE` (`[smooth, retail]`).
    pub(super) pipeline: [wgpu::RenderPipeline; 2],
    /// The per-ABR pipelines, indexed the same way.
    pub(super) blend_pipelines: [[wgpu::RenderPipeline; 4]; 2],
    pub(super) atlas_bgl: wgpu::BindGroupLayout,
    pub(super) sampler: wgpu::Sampler,
    pub(super) vbuf: wgpu::Buffer,
    pub(super) ibuf: wgpu::Buffer,
    /// Quads the buffers start sized for; grown on demand.
    pub(super) initial_quads: u32,
}

/// Text pipeline: 2D textured quads in NDC, alpha blended, no depth.
/// Vertex layout = 8 (pos: Float32x2) + 8 (uv: Float32x2) +
/// 16 (color: Float32x4) = 32 bytes.
pub(super) fn build_text_overlay(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
) -> TextParts {
    let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia text shader"),
        source: wgpu::ShaderSource::Wgsl(text_shader_src().into()),
    });
    let text_atlas_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("text atlas bgl"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    multisampled: false,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
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
    let text_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("text atlas sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let text_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia text pipeline layout"),
        bind_group_layouts: &[&text_atlas_bgl],
        push_constant_ranges: &[],
    });
    let text_vertex_layout = wgpu::VertexBufferLayout {
        array_stride: 32,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    };
    // `UI_QUANTIZE` per variant: index 0 = the plain 8-bit multiply, index
    // 1 = retail's 5-bit modulation (`legaia_render_kernels::ui_tint`).
    let quant_consts: [[(&str, f64); 1]; 2] = [[("UI_QUANTIZE", 0.0)], [("UI_QUANTIZE", 1.0)]];
    let pipeline_for = |q: usize, entry: &str, blend: wgpu::BlendState, label: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&text_vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some(entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(blend),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &quant_consts[q],
                    ..Default::default()
                },
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            // Scene render pass binds a depth attachment; every pipeline used
            // in that pass must declare a matching depth-stencil format.
            // Text never reads or writes depth - `Always` + write disabled
            // keeps it a pure overlay pass.
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::Always,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };
    let text_pipeline: [wgpu::RenderPipeline; 2] = std::array::from_fn(|q| {
        pipeline_for(
            q,
            "fs_main",
            wgpu::BlendState::ALPHA_BLENDING,
            "legaia text pipeline",
        )
    });
    // The same quads through a PSX semi-transparency equation, one
    // pipeline per ABR mode (`OverlayBlendSpan`). Mode 3's `F/4` is
    // pre-scaled in `fs_blend_quarter`, mode 0's halves come from the
    // blend constant the draw binds.
    let text_blend_pipelines: [[wgpu::RenderPipeline; 4]; 2] = std::array::from_fn(|q| {
        std::array::from_fn(|m| {
            let mode = m as u8;
            let entry = if psx_blend::src_shader_scale(mode) == 1.0 {
                "fs_blend"
            } else {
                "fs_blend_quarter"
            };
            pipeline_for(
                q,
                entry,
                psx_blend::blend_state(mode),
                "legaia text blend pipeline",
            )
        })
    });
    let initial_text_quads: u32 = 64;
    let text_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("text vertex buffer"),
        size: (initial_text_quads as u64) * 4 * 32,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let text_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("text index buffer"),
        size: (initial_text_quads as u64) * 6 * 4,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    TextParts {
        pipeline: text_pipeline,
        blend_pipelines: text_blend_pipelines,
        atlas_bgl: text_atlas_bgl,
        sampler: text_sampler,
        vbuf: text_vbuf,
        ibuf: text_ibuf,
        initial_quads: initial_text_quads,
    }
}
