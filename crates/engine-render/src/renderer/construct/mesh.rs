//! The single-mesh 3D pipelines, each drawing one mesh under the static
//! `MeshUniforms` bind group: flat, textured, VRAM-sampled and wireframe.

use super::*;

/// The flat-mesh pipeline and the mesh uniforms every single-mesh pipeline
/// binds at group 0.
pub(super) struct FlatMeshParts {
    pub(super) uniforms_bgl: wgpu::BindGroupLayout,
    pub(super) uniforms_buf: wgpu::Buffer,
    pub(super) uniforms_bg: wgpu::BindGroup,
    pub(super) pipeline: wgpu::RenderPipeline,
}

/// Mesh pipeline: 3D triangle list, depth-tested, single directional light.
pub(super) fn build_flat_mesh(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
) -> FlatMeshParts {
    let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia mesh shader"),
        source: wgpu::ShaderSource::Wgsl(compose_psx_shader(MESH_SHADER_SRC).into()),
    });
    let (mesh_uniforms_bgl, mesh_uniforms_buf, mesh_uniforms_bg) = make_uniform_bind_group(
        device,
        "mesh uniforms",
        bytemuck::cast_slice(&[MeshUniforms {
            mvp: Mat4::IDENTITY.to_cols_array_2d(),
            depth_cue: [0.0, 0.0, 0.0, 0.0],
            psx_params: [width as f32, height as f32, 0.0, 0.0],
            tex_window: [0; 4],
            grade: [1.0, 1.0, 1.0, 0.0],
            flags: [0.0; 4],
            // Dynamic lighting off (w = 0) = the retail-identical path.
            light_dir: [0.0; 4],
            light_color: legaia_render_kernels::scene_lighting::LightingMood::DAY.uniforms(false)
                [1],
            cue_ramp: [0.0; 4],
            palette: [0.0; 4],
            model_rows: MODEL_ROWS_IDENTITY,
        }]),
        wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
    );
    let mesh_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia mesh pipeline layout"),
        bind_group_layouts: &[&mesh_uniforms_bgl],
        push_constant_ranges: &[],
    });
    // Vertex layout: 3 floats position. Normals are computed in the shader
    // from screen-space derivatives - no per-vertex normal needed, which
    // keeps the upload format dead-simple for the source TMDs (which only
    // store per-object normals, not per-vertex).
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: 12,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        }],
    };
    let mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia mesh pipeline"),
        layout: Some(&mesh_pipeline_layout),
        vertex: wgpu::VertexState {
            module: &mesh_shader,
            entry_point: Some("vs_main"),
            buffers: &[vertex_layout],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &mesh_shader,
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
    FlatMeshParts {
        uniforms_bgl: mesh_uniforms_bgl,
        uniforms_buf: mesh_uniforms_buf,
        uniforms_bg: mesh_uniforms_bg,
        pipeline: mesh_pipeline,
    }
}

/// Textured-mesh pipeline: same depth + MVP path as the flat mesh
/// pipeline, but with a per-vertex UV attribute and a fragment shader
/// that samples a bound texture. Reuses `bind_group_layout` (the
/// texture+sampler layout from the quad pipeline) at group 1.
pub(super) fn build_textured_mesh(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    mesh_uniforms_bgl: &wgpu::BindGroupLayout,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let textured_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia textured mesh shader"),
        source: wgpu::ShaderSource::Wgsl(compose_psx_shader(TEXTURED_MESH_SHADER_SRC).into()),
    });
    let textured_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia textured mesh pipeline layout"),
        bind_group_layouts: &[mesh_uniforms_bgl, bind_group_layout],
        push_constant_ranges: &[],
    });
    let textured_vertex_layout = wgpu::VertexBufferLayout {
        // 3 floats position + 2 floats UV = 20 bytes.
        array_stride: 20,
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
        ],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia textured mesh pipeline"),
        layout: Some(&textured_mesh_layout),
        vertex: wgpu::VertexState {
            module: &textured_mesh_shader,
            entry_point: Some("vs_main"),
            buffers: &[textured_vertex_layout],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &textured_mesh_shader,
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
    })
}

/// 12 (pos) + 4 (uv as Uint8x4) + 4 (cba/tsb as Uint16x2) + 12
/// (normal as Float32x3) + 4 (prim colour as Uint8x4) + 32 (the flat
/// bucket-depth reference as two Float32x4) = 68 bytes.
///
/// The colour is the TMD prim's baked colour word, kept as raw bytes
/// (never converted): the GPU modulates the texel by it as
/// `texel * colour / 128`, which is retail's whole field lighting model.
///
/// The flat reference is zero on every mesh but the continent ground,
/// whose vertices carry their cell's four corners so the overworld
/// draws each cell at its ordering-table bucket's depth
/// (`legaia_engine_field::overworld_draw_order`).
pub(super) fn vram_vertex_attributes() -> [wgpu::VertexAttribute; 11] {
    [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: 12,
            shader_location: 1,
            format: wgpu::VertexFormat::Uint8x4,
        },
        wgpu::VertexAttribute {
            offset: 16,
            shader_location: 2,
            format: wgpu::VertexFormat::Uint16x2,
        },
        wgpu::VertexAttribute {
            offset: 20,
            shader_location: 3,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: 32,
            shader_location: 4,
            format: wgpu::VertexFormat::Uint8x4,
        },
        wgpu::VertexAttribute {
            offset: 36,
            shader_location: 5,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 52,
            shader_location: 6,
            format: wgpu::VertexFormat::Float32x4,
        },
        // Retail's per-primitive near reject: the primitive's corners.
        prim_ref_attributes(68)[0],
        prim_ref_attributes(68)[1],
        prim_ref_attributes(68)[2],
        prim_ref_attributes(68)[3],
    ]
}

/// The VRAM-mesh vertex buffer layout over [`vram_vertex_attributes`]. Every
/// pipeline that draws a VRAM mesh takes this one layout rather than
/// restating it, so they cannot drift (a hand-rolled copy once omitted the
/// prim-colour attribute and failed pipeline validation).
pub(super) fn vram_vertex_layout(
    attributes: &[wgpu::VertexAttribute],
) -> wgpu::VertexBufferLayout<'_> {
    wgpu::VertexBufferLayout {
        array_stride: crate::renderer::VRAM_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes,
    }
}

/// The single-mesh VRAM pipeline, with the pieces the blend and scene
/// pipelines build on.
pub(super) struct VramMeshParts {
    /// The whole-VRAM texture layout.
    pub(super) bgl: wgpu::BindGroupLayout,
    pub(super) shader: wgpu::ShaderModule,
    pub(super) layout: wgpu::PipelineLayout,
    pub(super) pipeline: wgpu::RenderPipeline,
}

/// VRAM-mesh pipeline: per-vertex (UV, CBA, TSB) + a 1024×512 R16Uint
/// texture holding the whole PSX VRAM. The fragment shader does its
/// own page+CLUT lookup so a single mesh can sample multiple texture
/// pages and palettes correctly.
pub(super) fn build_vram_mesh(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    mesh_uniforms_bgl: &wgpu::BindGroupLayout,
    vertex_layout: &wgpu::VertexBufferLayout<'_>,
) -> VramMeshParts {
    let vram_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("psx vram bgl"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                multisampled: false,
                view_dimension: wgpu::TextureViewDimension::D2,
                sample_type: wgpu::TextureSampleType::Uint,
            },
            count: None,
        }],
    });
    let vram_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia vram mesh shader"),
        source: wgpu::ShaderSource::Wgsl(compose_psx_shader(VRAM_MESH_SHADER_SRC).into()),
    });
    let vram_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia vram mesh pipeline layout"),
        bind_group_layouts: &[mesh_uniforms_bgl, &vram_bgl],
        push_constant_ranges: &[],
    });
    let vram_mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia vram mesh pipeline"),
        layout: Some(&vram_mesh_layout),
        vertex: wgpu::VertexState {
            module: &vram_mesh_shader,
            entry_point: Some("vs_main"),
            buffers: std::slice::from_ref(vertex_layout),
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &vram_mesh_shader,
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
    VramMeshParts {
        bgl: vram_bgl,
        shader: vram_mesh_shader,
        layout: vram_mesh_layout,
        pipeline: vram_mesh_pipeline,
    }
}

/// The wireframe pipeline and its shader, which the scene wireframe pipeline
/// shares.
pub(super) struct LinesParts {
    pub(super) shader: wgpu::ShaderModule,
    pub(super) pipeline: wgpu::RenderPipeline,
}

/// Wireframe lines pipeline: LineList topology, per-vertex color,
/// depth-tested. Reuses `mesh_uniforms_bgl` for the MVP. Per-vertex
/// layout = 12 (position) + 4 (color as Uint8x4) = 16 bytes.
pub(super) fn build_wireframe(
    device: &wgpu::Device,
    view_format: wgpu::TextureFormat,
    mesh_uniforms_bgl: &wgpu::BindGroupLayout,
) -> LinesParts {
    let lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("legaia lines shader"),
        source: wgpu::ShaderSource::Wgsl(LINES_SHADER_SRC.into()),
    });
    let lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("legaia lines pipeline layout"),
        bind_group_layouts: &[mesh_uniforms_bgl],
        push_constant_ranges: &[],
    });
    let lines_vertex_layout = wgpu::VertexBufferLayout {
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
    };
    let lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("legaia lines pipeline"),
        layout: Some(&lines_layout),
        vertex: wgpu::VertexState {
            module: &lines_shader,
            entry_point: Some("vs_main"),
            buffers: &[lines_vertex_layout],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &lines_shader,
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
    });
    LinesParts {
        shader: lines_shader,
        pipeline: lines_pipeline,
    }
}
