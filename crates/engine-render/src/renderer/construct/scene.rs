//! The scene pass: the dynamic-offset uniform buffer holding one
//! `MeshUniforms` slot per draw, and the pipelines that draw under it
//! (VRAM mesh, wireframe, vertex-colour mesh).

use super::*;

/// The scene uniform buffer and its dynamic-offset bind group.
pub(super) struct SceneUniformParts {
    /// Byte distance between two slots of a dynamic-offset uniform buffer.
    pub(super) offset_alignment: u32,
    pub(super) bgl: wgpu::BindGroupLayout,
    pub(super) buf: wgpu::Buffer,
    pub(super) bg: wgpu::BindGroup,
    /// Slots the buffer starts with; `render_scene` grows it on demand.
    pub(super) initial_capacity: usize,
}

/// Scene-uniforms layout: a single dynamic-offset uniform buffer
/// holding N `MeshUniforms` slots, each `uniform_offset_alignment`
/// bytes apart. Reused for the multi-actor VRAM-mesh and lines
/// pipelines below.
pub(super) fn build_scene_uniforms(device: &wgpu::Device) -> SceneUniformParts {
    let uniform_offset_alignment = device.limits().min_uniform_buffer_offset_alignment.max(256);
    let scene_uniforms_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("scene mesh uniforms bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: true,
                min_binding_size: std::num::NonZeroU64::new(
                    std::mem::size_of::<MeshUniforms>() as u64
                ),
            },
            count: None,
        }],
    });
    // Initial capacity: one slot. Grown on demand by render_scene.
    let initial_scene_capacity: usize = 1;
    let scene_uniforms_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("scene mesh uniforms"),
        size: (initial_scene_capacity * uniform_offset_alignment as usize) as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let scene_uniforms_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("scene mesh uniforms bg"),
        layout: &scene_uniforms_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &scene_uniforms_buf,
                offset: 0,
                size: std::num::NonZeroU64::new(std::mem::size_of::<MeshUniforms>() as u64),
            }),
        }],
    });
    SceneUniformParts {
        offset_alignment: uniform_offset_alignment,
        bgl: scene_uniforms_bgl,
        buf: scene_uniforms_buf,
        bg: scene_uniforms_bg,
        initial_capacity: initial_scene_capacity,
    }
}

/// The scene VRAM-mesh pipeline, with the pieces its blend pipelines build
/// on.
pub(super) struct SceneVramParts {
    pub(super) shader: wgpu::ShaderModule,
    pub(super) layout: wgpu::PipelineLayout,
    pub(super) pipeline: wgpu::RenderPipeline,
}

/// Scene shader variants: same WGSL bodies as the single-mesh
/// pipelines but with the REAL point-light layer (group 2 bindings)
/// in the prelude instead of the zero stub.
pub(super) fn build_scene_vram_mesh(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    scene_uniforms_bgl: &wgpu::BindGroupLayout,
    vram_bgl: &wgpu::BindGroupLayout,
    scene_lights_bgl: &wgpu::BindGroupLayout,
    vertex_layout: &wgpu::VertexBufferLayout<'_>,
) -> SceneVramParts {
    let scene_vram_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia scene vram mesh shader"),
        source: wgpu::ShaderSource::Wgsl(compose_scene_lit_shader(VRAM_MESH_SHADER_SRC).into()),
    });

    let scene_vram_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia scene vram mesh pipeline layout"),
        bind_group_layouts: &[scene_uniforms_bgl, vram_bgl, scene_lights_bgl],
        push_constant_ranges: &[],
    });
    let scene_vram_mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia scene vram mesh pipeline"),
        layout: Some(&scene_vram_mesh_layout),
        vertex: wgpu::VertexState {
            module: &scene_vram_mesh_shader,
            entry_point: Some("vs_main"),
            // Same vertex layout as the non-scene VRAM pipeline - share
            // it rather than restating it, so the two cannot drift (an
            // earlier hand-rolled copy silently omitted the prim-colour
            // attribute and failed pipeline validation).
            buffers: std::slice::from_ref(vertex_layout),
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &scene_vram_mesh_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: view_format,
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
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Greater,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    SceneVramParts {
        shader: scene_vram_mesh_shader,
        layout: scene_vram_mesh_layout,
        pipeline: scene_vram_mesh_pipeline,
    }
}

/// The scene wireframe pipeline: the single-mesh wireframe shader under the
/// scene uniforms.
pub(super) fn build_scene_wireframe(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    scene_uniforms_bgl: &wgpu::BindGroupLayout,
    lines_shader: &wgpu::ShaderModule,
) -> wgpu::RenderPipeline {
    let scene_lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia scene lines pipeline layout"),
        bind_group_layouts: &[scene_uniforms_bgl],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia scene lines pipeline"),
        layout: Some(&scene_lines_layout),
        vertex: wgpu::VertexState {
            module: lines_shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 16,
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
                        format: wgpu::VertexFormat::Unorm8x4,
                    },
                ],
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: lines_shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: view_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Greater,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// The vertex-colour mesh pipeline and its four per-ABR blend pipelines.
pub(super) struct ColorMeshParts {
    pub(super) pipeline: wgpu::RenderPipeline,
    pub(super) blend_pipelines: [wgpu::RenderPipeline; 4],
}

/// Vertex-colour mesh pipeline (untextured F*/G* props): scene-
/// uniforms dynamic-offset layout, TriangleList, position(12) +
/// Unorm8x4 colour(4) + Uint32 blend word(4) = 20 bytes. The blend
/// word carries the prim's ABE/ABR state in the low 16 bits
/// ([`psx_blend::pack_blend_word`]). The layout mirrors the scene
/// VRAM path's three groups (the shader ignores the VRAM group) so
/// the blend pass can interleave textured and untextured prims
/// without invalidating sticky bind groups, and the scene-lit
/// shader variant reaches the group-2 point-light layer.
pub(super) fn build_scene_color_mesh(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    scene_uniforms_bgl: &wgpu::BindGroupLayout,
    vram_bgl: &wgpu::BindGroupLayout,
    scene_lights_bgl: &wgpu::BindGroupLayout,
) -> ColorMeshParts {
    let color_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia color mesh shader"),
        source: wgpu::ShaderSource::Wgsl(compose_scene_lit_shader(COLOR_MESH_SHADER_SRC).into()),
    });
    let scene_color_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia scene color mesh pipeline layout"),
        bind_group_layouts: &[scene_uniforms_bgl, vram_bgl, scene_lights_bgl],
        push_constant_ranges: &[],
    });
    let color_mesh_attributes = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: 12,
            shader_location: 1,
            format: wgpu::VertexFormat::Unorm8x4,
        },
        wgpu::VertexAttribute {
            offset: 16,
            shader_location: 2,
            format: wgpu::VertexFormat::Uint32,
        },
        prim_ref_attributes(20)[0],
        prim_ref_attributes(20)[1],
        prim_ref_attributes(20)[2],
        prim_ref_attributes(20)[3],
    ];
    let color_mesh_vertex_layout = wgpu::VertexBufferLayout {
        array_stride: crate::renderer::COLOR_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &color_mesh_attributes,
    };
    let scene_color_mesh_pipeline =
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia scene color mesh pipeline"),
            layout: Some(&scene_color_mesh_layout),
            vertex: wgpu::VertexState {
                module: &color_mesh_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&color_mesh_vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &color_mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: view_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Greater,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
    // PSX semi-transparency blend pipelines for the colour-mesh path,
    // one per ABR mode. Same shader module + vertex layout as the opaque
    // colour pipeline; the blend-pass fragment entries emit the prim
    // colour (mode 3 pre-scales by 0.25) and the per-mode fixed-function
    // [`psx_blend::blend_state`] applies the PSX equation. Unlike the
    // VRAM-mesh blend pass there is no STP discard - an untextured ABE
    // prim blends every pixel. Depth: LessEqual without writing, like
    // the textured blend pass.
    let scene_color_mesh_blend_pipelines = blend::blend_pipelines(
        device,
        view_format,
        "legaia scene color mesh blend pipeline",
        &scene_color_mesh_layout,
        &color_mesh_shader,
        &color_mesh_vertex_layout,
    );
    ColorMeshParts {
        pipeline: scene_color_mesh_pipeline,
        blend_pipelines: scene_color_mesh_blend_pipelines,
    }
}
