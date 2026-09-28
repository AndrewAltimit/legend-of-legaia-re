//! Headless GPU check of the VRAM-mesh shader's overworld flat depth: a
//! continent cell carrying its four corners draws at its ordering-table
//! bucket's depth, one value across the whole cell - retail's
//! `(max corner SZ >> 5) + 14` (`FUN_801F89B8`), mapped back through the
//! draw's own matrix (`legaia_engine_core::overworld_draw_order`, which
//! computes the same number on the CPU for the fog sheets). Skips (passes
//! vacuously) with no GPU adapter.

use super::screen_overlay_gpu::headless_device;
use crate::renderer::VRAM_VERTEX_STRIDE;
use crate::shaders::{VRAM_MESH_SHADER_SRC, compose_psx_shader};
use wgpu::util::DeviceExt;

const SIDE: u32 = 16;
const NEAR: f32 = 1.0;
const FAR: f32 = 10_000.0;

/// A perspective looking down `+z` with the retail projection's depth row:
/// `z = a * w + b`, `w = z_eye`.
fn mvp() -> [f32; 16] {
    let a = FAR / (FAR - NEAR);
    let b = -NEAR * FAR / (FAR - NEAR);
    let k = 3.0;
    [
        k, 0.0, 0.0, 0.0, //
        0.0, k, 0.0, 0.0, //
        0.0, 0.0, a, 1.0, //
        0.0, 0.0, b, 0.0,
    ]
}

/// Draw one sloped cell and read the depth attachment back; `sz_scale` is
/// the frame's `clip.w`-to-`SZ` factor (`MeshUniforms.flags.w`), `refs`
/// whether the vertices carry the cell's corners.
fn draw_cell(device: &wgpu::Device, queue: &wgpu::Queue, sz_scale: f32, refs: bool) -> Vec<f32> {
    // The cell: x in -100..100, z in 400..600, rising from y -80 at the near
    // edge to +80 at the far one, so it faces the eye.
    let (x0, z0, x1, z1) = (-100.0f32, 400.0f32, 100.0f32, 600.0f32);
    let ys = [-80.0f32, -80.0, 80.0, 80.0];
    let pos = [
        [x0, ys[0], z0],
        [x1, ys[1], z0],
        [x0, ys[2], z1],
        [x1, ys[3], z1],
    ];
    let flat = if refs {
        [x0, z0, x1, z1, ys[0], ys[1], ys[2], ys[3]]
    } else {
        [0.0; 8]
    };
    let mut bytes = Vec::new();
    for p in pos {
        bytes.extend_from_slice(bytemuck::cast_slice(&p));
        bytes.extend_from_slice(&[0, 0, 0, 0]); // uv
        bytes.extend_from_slice(&0u16.to_le_bytes()); // cba
        bytes.extend_from_slice(&0x100u16.to_le_bytes()); // tsb: 15bpp
        bytes.extend_from_slice(bytemuck::cast_slice(&[0.0f32; 3])); // normal
        bytes.extend_from_slice(&[0x80, 0x80, 0x80, 0]); // prim colour
        bytes.extend_from_slice(bytemuck::cast_slice(&flat));
    }
    assert_eq!(bytes.len(), 4 * VRAM_VERTEX_STRIDE as usize);
    let indices: [u32; 6] = [0, 1, 2, 1, 3, 2];

    // MeshUniforms: mvp, then twelve vec4s - depth_cue, psx_params,
    // tex_window, grade, flags, ...; `flags.w` is the curvature scale.
    let mut uniforms = [0f32; 64];
    uniforms[..16].copy_from_slice(&mvp());
    uniforms[16 + 4 * 4 + 3] = sz_scale;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("flat depth test shader"),
        source: wgpu::ShaderSource::Wgsl(compose_psx_shader(VRAM_MESH_SHADER_SRC).into()),
    });
    let ubgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let vbgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
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
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&ubgl, &vbgl],
        push_constant_ranges: &[],
    });
    let attrs = [
        (0, 0, wgpu::VertexFormat::Float32x3),
        (12, 1, wgpu::VertexFormat::Uint8x4),
        (16, 2, wgpu::VertexFormat::Uint16x2),
        (20, 3, wgpu::VertexFormat::Float32x3),
        (32, 4, wgpu::VertexFormat::Uint8x4),
        (36, 5, wgpu::VertexFormat::Float32x4),
        (52, 6, wgpu::VertexFormat::Float32x4),
    ]
    .map(|(offset, shader_location, format)| wgpu::VertexAttribute {
        offset,
        shader_location,
        format,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: VRAM_VERTEX_STRIDE,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attrs,
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: wgpu::TextureFormat::Rgba8Unorm,
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
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    let ubuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    // A white 15bpp VRAM, so every fragment is opaque and writes depth.
    let vram = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1024,
            height: 512,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &vram,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[0xFFu8, 0x7F].repeat(1024 * 512),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(2048),
            rows_per_image: Some(512),
        },
        wgpu::Extent3d {
            width: 1024,
            height: 512,
            depth_or_array_layers: 1,
        },
    );
    let ubg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &ubgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: ubuf.as_entire_binding(),
        }],
    });
    let vview = vram.create_view(&wgpu::TextureViewDescriptor::default());
    let vbg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &vbgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&vview),
        }],
    });
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &bytes,
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    let extent = wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    };
    let color = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let cview = color.create_view(&wgpu::TextureViewDescriptor::default());
    let dview = depth.create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &cview,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &dview,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        rp.set_pipeline(&pipeline);
        rp.set_bind_group(0, &ubg, &[]);
        rp.set_bind_group(1, &vbg, &[]);
        rp.set_vertex_buffer(0, vbuf.slice(..));
        rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.draw_indexed(0..6, 0, 0..1);
    }
    let row = 256u32; // SIDE * 4 bytes, padded to the copy alignment
    let rb = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * SIDE),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &depth,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::DepthOnly,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &rb,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(SIDE),
            },
        },
        extent,
    );
    queue.submit(std::iter::once(enc.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    rb.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    device.poll(wgpu::PollType::wait()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = rb.slice(..).get_mapped_range();
    let mut out = Vec::new();
    for y in 0..SIDE as usize {
        let r = &data[y * row as usize..y * row as usize + SIDE as usize * 4];
        out.extend(bytemuck::cast_slice::<u8, f32>(r).iter().copied());
    }
    out
}

#[test]
fn a_continent_cell_draws_flat_at_its_bucket_depth() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("[skip] no GPU adapter");
        return;
    };
    let covered = |d: &[f32]| d.iter().copied().filter(|&z| z < 1.0).collect::<Vec<_>>();

    // Overworld frame, corners carried: one depth, the bucket's.
    let flat = covered(&draw_cell(&device, &queue, 1.0, true));
    assert!(flat.len() > 20, "the cell covers the target");
    // The farthest corner is 600 deep: SZ 600, bucket (600 >> 5) + 14 = 32,
    // drawn at SZ 32 * 32 + 32.
    let a = FAR / (FAR - NEAR);
    let b = -NEAR * FAR / (FAR - NEAR);
    let want = a + b / 1056.0;
    for z in &flat {
        assert!(
            (z - want).abs() < 1e-5,
            "flat cell depth {z}, bucket depth {want}"
        );
    }

    // A 1x frame (SZ = 6 w): SZ 3600, bucket 126, drawn at w = 4064 / 6.
    let six = covered(&draw_cell(&device, &queue, 6.0, true));
    let want6 = a + b / (4064.0 / 6.0);
    for z in &six {
        assert!((z - want6).abs() < 1e-5, "1x frame: {z} vs {want6}");
    }

    // Off the overworld, or without the corners, the cell keeps its
    // per-pixel depth.
    for d in [
        covered(&draw_cell(&device, &queue, 0.0, true)),
        covered(&draw_cell(&device, &queue, 1.0, false)),
    ] {
        let (lo, hi) = d
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), &z| (l.min(z), h.max(z)));
        assert!(hi - lo > 1e-5, "per-pixel depth varies across the slope");
        assert!(hi < want, "and sits in front of the bucket depth");
    }
}
