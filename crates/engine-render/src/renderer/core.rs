//! The [`Renderer`] struct definition, its constructor (`new`/`new_async`
//! pipeline+resource setup), and the small state accessors. Split out of
//! `renderer.rs`.

use super::*;

/// Builds a static-offset uniform bind group: a one-entry `BindGroupLayout`,
/// a `create_buffer_init` buffer, and the matching one-entry bind group.
/// Shared by the quad (`uniforms_*`) and mesh (`mesh_uniforms_*`) setups,
/// which are identical apart from the label, shader `visibility`, and the
/// buffer `contents`. Labels reproduce the original `"{base} bgl"` /
/// `"{base}"` / `"{base} bg"` naming exactly. The dynamic-offset
/// `scene_uniforms` bind group is deliberately NOT built here (different BGL:
/// `has_dynamic_offset: true` + a non-None `min_binding_size`).
fn make_uniform_bind_group(
    device: &wgpu::Device,
    label: &str,
    contents: &[u8],
    visibility: wgpu::ShaderStages,
) -> (wgpu::BindGroupLayout, wgpu::Buffer, wgpu::BindGroup) {
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(&format!("{label} bgl")),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(&format!("{label} bg")),
        layout: &bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buf.as_entire_binding(),
        }],
    });
    (bgl, buf, bg)
}

impl Renderer {
    /// Constructs a renderer attached to a winit-style window. Caller passes
    /// an `Arc<Window>` so the Surface can outlive the borrow.
    pub fn new<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: wgpu::WindowHandle + 'static,
    {
        pollster::block_on(Self::new_async(window, width, height))
    }

    async fn new_async<W>(window: Arc<W>, width: u32, height: u32) -> Result<Self>
    where
        W: wgpu::WindowHandle + 'static,
    {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });
        let surface = instance
            .create_surface(window)
            .context("create wgpu surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("legaia engine device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults()
                    .using_resolution(adapter.limits()),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
            })
            .await
            .context("request device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
        };
        surface.configure(&device, &config);

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
            &device,
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
                    format: config.format,
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

        // Mesh pipeline: 3D triangle list, depth-tested, single directional light.
        let mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(MESH_SHADER_SRC).into()),
        });
        let (mesh_uniforms_bgl, mesh_uniforms_buf, mesh_uniforms_bg) = make_uniform_bind_group(
            &device,
            "mesh uniforms",
            bytemuck::cast_slice(&[MeshUniforms {
                mvp: Mat4::IDENTITY.to_cols_array_2d(),
                light_dir: [0.4, -0.8, 0.4, 0.0],
                psx_params: [width as f32, height as f32, 0.0, 0.0],
                tex_window: [0; 4],
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
                    format: config.format,
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
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Textured-mesh pipeline: same depth + MVP path as the flat mesh
        // pipeline, but with a per-vertex UV attribute and a fragment shader
        // that samples a bound texture. Reuses `bind_group_layout` (the
        // texture+sampler layout from the quad pipeline) at group 1.
        let textured_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia textured mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(TEXTURED_MESH_SHADER_SRC).into()),
        });
        let textured_mesh_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia textured mesh pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl, &bind_group_layout],
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
        let textured_mesh_pipeline =
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
                        format: config.format,
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
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // VRAM-mesh pipeline: per-vertex (UV, CBA, TSB) + a 1024×512 R16Uint
        // texture holding the whole PSX VRAM. The fragment shader does its
        // own page+CLUT lookup so a single mesh can sample multiple texture
        // pages and palettes correctly.
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
            bind_group_layouts: &[&mesh_uniforms_bgl, &vram_bgl],
            push_constant_ranges: &[],
        });
        // 12 (pos) + 4 (uv as Uint8x4) + 4 (cba/tsb as Uint16x2) + 12
        // (normal as Float32x3) = 32 bytes
        let vram_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 32,
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
            ],
        };
        let vram_mesh_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia vram mesh pipeline"),
            layout: Some(&vram_mesh_layout),
            vertex: wgpu::VertexState {
                module: &vram_mesh_shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&vram_vertex_layout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &vram_mesh_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
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
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Wireframe lines pipeline: LineList topology, per-vertex color,
        // depth-tested. Reuses `mesh_uniforms_bgl` for the MVP. Per-vertex
        // layout = 12 (position) + 4 (color as Uint8x4) = 16 bytes.
        let lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia lines shader"),
            source: wgpu::ShaderSource::Wgsl(LINES_SHADER_SRC.into()),
        });
        let lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia lines pipeline layout"),
            bind_group_layouts: &[&mesh_uniforms_bgl],
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
                    format: config.format,
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
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Scene-uniforms layout: a single dynamic-offset uniform buffer
        // holding N `MeshUniforms` slots, each `uniform_offset_alignment`
        // bytes apart. Reused for the multi-actor VRAM-mesh and lines
        // pipelines below.
        let uniform_offset_alignment = device.limits().min_uniform_buffer_offset_alignment.max(256);
        let scene_uniforms_bgl =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("scene mesh uniforms bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZeroU64::new(
                            std::mem::size_of::<MeshUniforms>() as u64,
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

        let scene_vram_mesh_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("legaia scene vram mesh pipeline layout"),
                bind_group_layouts: &[&scene_uniforms_bgl, &vram_bgl],
                push_constant_ranges: &[],
            });
        let scene_vram_mesh_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("legaia scene vram mesh pipeline"),
                layout: Some(&scene_vram_mesh_layout),
                vertex: wgpu::VertexState {
                    module: &vram_mesh_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[wgpu::VertexBufferLayout {
                        array_stride: 32,
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
                        ],
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &vram_mesh_shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
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
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            });

        // PSX semi-transparency blend pipelines: one per ABR mode, for both
        // the single-mesh layout and the scene (dynamic-offset) layout. Same
        // shader module + vertex state as the opaque VRAM-mesh pipelines;
        // the blend-pass fragment entry keeps only STP texels and the
        // per-mode fixed-function [`psx_blend::blend_state`] applies the PSX
        // equation (mode 3 pre-scales F by 0.25 via its own entry point).
        // Depth: test against the opaque pass but don't write (the PSX has
        // no depth buffer and blended fragments must not occlude later
        // draws); LessEqual so decal prims coplanar with already-drawn
        // geometry aren't z-rejected.
        let make_blend_pipeline = |label: &'static str,
                                   layout: &wgpu::PipelineLayout,
                                   module: &wgpu::ShaderModule,
                                   vertex_layout: &wgpu::VertexBufferLayout,
                                   mode: u8|
         -> wgpu::RenderPipeline {
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
                        format: config.format,
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
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };
        let vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
            make_blend_pipeline(
                "legaia vram mesh blend pipeline",
                &vram_mesh_layout,
                &vram_mesh_shader,
                &vram_vertex_layout,
                m as u8,
            )
        });
        let scene_vram_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
            make_blend_pipeline(
                "legaia scene vram mesh blend pipeline",
                &scene_vram_mesh_layout,
                &vram_mesh_shader,
                &vram_vertex_layout,
                m as u8,
            )
        });

        let scene_lines_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("legaia scene lines pipeline layout"),
            bind_group_layouts: &[&scene_uniforms_bgl],
            push_constant_ranges: &[],
        });
        let scene_lines_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia scene lines pipeline"),
            layout: Some(&scene_lines_layout),
            vertex: wgpu::VertexState {
                module: &lines_shader,
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
                module: &lines_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
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
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // Vertex-colour mesh pipeline (untextured F*/G* props): same scene-
        // uniforms dynamic-offset layout as the lines pipeline (group 0 only,
        // no VRAM), TriangleList, position(12) + Unorm8x4 colour(4) +
        // Uint32 blend word(4) = 20 bytes. The blend word carries the prim's
        // ABE/ABR state in the low 16 bits ([`psx_blend::pack_blend_word`]).
        let color_mesh_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia color mesh shader"),
            source: wgpu::ShaderSource::Wgsl(compose_psx_shader(COLOR_MESH_SHADER_SRC).into()),
        });
        let scene_color_mesh_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("legaia scene color mesh pipeline layout"),
                bind_group_layouts: &[&scene_uniforms_bgl],
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
        ];
        let color_mesh_vertex_layout = wgpu::VertexBufferLayout {
            array_stride: 20,
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
                        format: config.format,
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
                    depth_compare: wgpu::CompareFunction::Less,
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
        let scene_color_mesh_blend_pipelines: [wgpu::RenderPipeline; 4] =
            std::array::from_fn(|m| {
                make_blend_pipeline(
                    "legaia scene color mesh blend pipeline",
                    &scene_color_mesh_layout,
                    &color_mesh_shader,
                    &color_mesh_vertex_layout,
                    m as u8,
                )
            });

        // Text pipeline: 2D textured quads in NDC, alpha blended, no depth.
        // Vertex layout = 8 (pos: Float32x2) + 8 (uv: Float32x2) +
        // 16 (color: Float32x4) = 32 bytes.
        let text_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("legaia text shader"),
            source: wgpu::ShaderSource::Wgsl(TEXT_SHADER_SRC.into()),
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
        let text_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("legaia text pipeline"),
            layout: Some(&text_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &text_shader,
                entry_point: Some("vs_main"),
                buffers: &[text_vertex_layout],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &text_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
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

        let depth_view = create_depth_view(&device, config.width, config.height);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            sampler,
            bind_group_layout,
            uniforms_buf,
            uniforms_bg,
            mesh_pipeline,
            mesh_uniforms_buf,
            mesh_uniforms_bg,
            textured_mesh_pipeline,
            vram_mesh_pipeline,
            vram_mesh_blend_pipelines,
            vram_bgl,
            scene_vram_mesh_pipeline,
            scene_vram_mesh_blend_pipelines,
            scene_lines_pipeline,
            scene_color_mesh_pipeline,
            scene_color_mesh_blend_pipelines,
            scene_uniforms_bgl,
            scene_uniforms_bg: std::cell::RefCell::new(scene_uniforms_bg),
            scene_uniforms_buf: std::cell::RefCell::new(scene_uniforms_buf),
            scene_uniforms_capacity: std::cell::Cell::new(initial_scene_capacity),
            uniform_offset_alignment,
            lines_pipeline,
            text_pipeline,
            text_atlas_bgl,
            text_sampler,
            text_vbuf: std::cell::RefCell::new(text_vbuf),
            text_ibuf: std::cell::RefCell::new(text_ibuf),
            text_vertex_capacity: std::cell::Cell::new(initial_text_quads * 4),
            text_index_capacity: std::cell::Cell::new(initial_text_quads * 6),
            scene_quad_ranges: std::cell::RefCell::new(Vec::new()),
            blend_list: std::cell::RefCell::new(Vec::new()),
            depth_view,
            psx_mode: std::cell::Cell::new(false),
            vram_upload_counter: std::cell::Cell::new(0),
            tex_window: std::cell::Cell::new([0; 4]),
        })
    }
}
