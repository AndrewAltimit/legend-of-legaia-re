//! The PSX semi-transparency blend pipelines: one per ABR mode over an
//! opaque pipeline's own layout, shader module and vertex layout.

use super::*;

/// PSX semi-transparency blend pipelines: one per ABR mode, for both
/// the single-mesh layout and the scene (dynamic-offset) layout. Same
/// shader module + vertex state as the opaque VRAM-mesh pipelines;
/// the blend-pass fragment entry keeps only STP texels and the
/// per-mode fixed-function [`psx_blend::blend_state`] applies the PSX
/// equation (mode 3 pre-scales F by 0.25 via its own entry point).
/// Depth: test against the opaque pass but don't write (the PSX has
/// no depth buffer and blended fragments must not occlude later
/// draws); GreaterEqual (reversed-Z LessEqual) so decal prims coplanar
/// with already-drawn geometry aren't z-rejected.
fn blend_pipeline(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    vertex_layout: &wgpu::VertexBufferLayout,
    mode: u8,
) -> wgpu::RenderPipeline {
    let entry = if psx_blend::src_shader_scale(mode) == 1.0 {
        "fs_blend"
    } else {
        "fs_blend_quarter"
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("vs_main"),
            buffers: std::slice::from_ref(vertex_layout),
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format: view_format,
                blend: Some(psx_blend::blend_state(mode)),
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
}

/// The four blend pipelines of one opaque pipeline, indexed by ABR mode and
/// created in mode order.
pub(super) fn blend_pipelines(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    label: &'static str,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    vertex_layout: &wgpu::VertexBufferLayout,
) -> [wgpu::RenderPipeline; 4] {
    std::array::from_fn(|m| {
        blend_pipeline(
            device,
            view_format,
            label,
            layout,
            module,
            vertex_layout,
            m as u8,
        )
    })
}
