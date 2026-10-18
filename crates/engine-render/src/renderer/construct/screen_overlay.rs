//! The screen-space 2D overlay pass (see `crate::screen_overlay`) and the
//! scene viewport fill quad that shares its vertex format.

use super::*;

/// The overlay pipelines and their quad buffers.
pub(super) struct ScreenOverlayParts {
    pub(super) pipeline: wgpu::RenderPipeline,
    /// The opaque half of a semi-transparent run (`fs_semi_opaque`).
    pub(super) semi_opaque_pipeline: wgpu::RenderPipeline,
    pub(super) blend_pipelines: [wgpu::RenderPipeline; 4],
    pub(super) vbuf: wgpu::Buffer,
    pub(super) ibuf: wgpu::Buffer,
    /// Quads the buffers start sized for; grown on demand.
    pub(super) initial_quads: u32,
}

/// Screen-space 2D overlay pass (see `crate::screen_overlay`): PSX
/// POLY_FT4 textured quads + flat quads in NDC, sampling the shared
/// VRAM (group 0 = `vram_bgl`) through the same CBA/TSB CLUT decode as
/// the 3D VRAM-mesh path. Opaque pipeline (replace) + four per-ABR
/// blend pipelines. Depth: GreaterEqual, no write - clip z is fixed at
/// 1.0 (the reversed-Z near plane) so every overlay quad passes the
/// test and composites on top, without occluding later draws (matches
/// the text overlay's role).
pub(super) fn build_screen_overlay(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    vram_bgl: &wgpu::BindGroupLayout,
) -> ScreenOverlayParts {
    let screen_overlay_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia screen overlay shader"),
        source: wgpu::ShaderSource::Wgsl(SCREEN_OVERLAY_SHADER_SRC.into()),
    });
    let screen_overlay_pipeline_layout =
        device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia screen overlay pipeline layout"),
            bind_group_layouts: &[vram_bgl],
            push_constant_ranges: &[],
        });
    // pos(Float32x2)@0 + uv(Float32x2)@8 + cba_tsb(Uint32x2)@16 +
    // color(Float32x4)@24 + flags(Uint32)@40 + depth(Float32)@44 = 48
    // bytes (matches `screen_overlay::ScreenVertex`).
    let screen_overlay_attributes = [
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
            format: wgpu::VertexFormat::Uint32x2,
        },
        wgpu::VertexAttribute {
            offset: 24,
            shader_location: 3,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 40,
            shader_location: 4,
            format: wgpu::VertexFormat::Uint32,
        },
        wgpu::VertexAttribute {
            offset: legaia_render_kernels::screen_prim::SCREEN_VERTEX_OFF_DEPTH,
            shader_location: 5,
            format: wgpu::VertexFormat::Float32,
        },
    ];
    let screen_overlay_vertex_layout = wgpu::VertexBufferLayout {
        array_stride: legaia_render_kernels::screen_prim::SCREEN_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &screen_overlay_attributes,
    };
    let opaque_pipeline = |label: &str, entry: &str| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: Some(&screen_overlay_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &screen_overlay_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&screen_overlay_vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &screen_overlay_shader,
                entry_point: Some(entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
                depth_write_enabled: false,
                depth_compare: wgpu::CompareFunction::GreaterEqual,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };
    let screen_overlay_pipeline = opaque_pipeline("legaia screen overlay pipeline", "fs_opaque");
    let screen_overlay_semi_opaque_pipeline = opaque_pipeline(
        "legaia screen overlay semi opaque pipeline",
        "fs_semi_opaque",
    );
    let screen_overlay_blend_pipelines = blend::blend_pipelines(
        device,
        view_format,
        "legaia screen overlay blend pipeline",
        &screen_overlay_pipeline_layout,
        &screen_overlay_shader,
        &screen_overlay_vertex_layout,
    );
    let initial_overlay_quads: u32 = 32;
    let screen_overlay_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screen overlay vertex buffer"),
        size: (initial_overlay_quads as u64)
            * 4
            * legaia_render_kernels::screen_prim::SCREEN_VERTEX_STRIDE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let screen_overlay_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("screen overlay index buffer"),
        size: (initial_overlay_quads as u64) * 6 * 4,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    ScreenOverlayParts {
        pipeline: screen_overlay_pipeline,
        semi_opaque_pipeline: screen_overlay_semi_opaque_pipeline,
        blend_pipelines: screen_overlay_blend_pipelines,
        vbuf: screen_overlay_vbuf,
        ibuf: screen_overlay_ibuf,
        initial_quads: initial_overlay_quads,
    }
}

/// The one-quad vertex and index buffers of the scene viewport fill.
pub(super) fn build_viewport_fill(device: &wgpu::Device) -> (wgpu::Buffer, wgpu::Buffer) {
    let viewport_fill_vbuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene viewport fill vertex buffer"),
        size: 4 * legaia_render_kernels::screen_prim::SCREEN_VERTEX_STRIDE,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let viewport_fill_ibuf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene viewport fill index buffer"),
        size: 6 * 4,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (viewport_fill_vbuf, viewport_fill_ibuf)
}
