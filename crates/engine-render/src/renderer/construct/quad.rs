//! The textured-quad pipeline (a fullscreen texture blit) and the
//! texture + sampler bind group layout the textured-mesh pipeline reuses.

use super::*;

/// What the textured-quad setup leaves on the renderer.
pub(super) struct QuadParts {
    /// The texture + sampler layout, group 0 here and group 1 of the
    /// textured-mesh pipeline.
    pub(super) bind_group_layout: wgpu::BindGroupLayout,
    pub(super) uniforms_buf: wgpu::Buffer,
    pub(super) uniforms_bg: wgpu::BindGroup,
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) sampler: wgpu::Sampler,
}

/// Build the textured-quad shader, layouts, pipeline and sampler.
pub(super) fn build_quad(device: &wgpu::Device, view_format: wgpu::TextureFormat) -> QuadParts {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia textured quad shader"),
        source: wgpu::ShaderSource::Wgsl(SHADER_SRC.into()),
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("texture bgl"),
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

    let (uniforms_bgl, uniforms_buf, uniforms_bg) = make_uniform_bind_group(
        device,
        "uniforms",
        bytemuck::cast_slice(&[Uniforms {
            scale: [1.0, 1.0, 0.0, 0.0],
        }]),
        wgpu::ShaderStages::VERTEX,
    );

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia pipeline layout"),
        bind_group_layouts: &[&bind_group_layout, &uniforms_bgl],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia textured quad pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: view_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("texture sampler"),
        mag_filter: wgpu::FilterMode::Nearest,
        min_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    QuadParts {
        bind_group_layout,
        uniforms_buf,
        uniforms_bg,
        pipeline,
        sampler,
    }
}
