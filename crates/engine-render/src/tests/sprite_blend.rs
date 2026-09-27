//! The 2D sprite pass's PSX semi-transparency: [`OverlayBlendSpan`] run
//! splitting on the CPU, and the text shader's blend entries through the
//! per-ABR fixed-function state on whatever GPU adapter is present (the GPU
//! half skips when none is).

use super::screen_overlay_gpu::headless_device;
use super::*;
use wgpu::util::DeviceExt;

#[test]
fn blend_spans_split_the_draw_list_in_draw_order() {
    let span = |start, count, abr| OverlayBlendSpan { start, count, abr };
    // Tiles, a two-row shade, then the screens over it.
    assert_eq!(
        OverlayBlendSpan::segments(&[span(3, 2, 2)], 7),
        vec![(0, 3, None), (3, 2, Some(2)), (5, 2, None)]
    );
    // A span at either end, and one past the list is clipped.
    assert_eq!(
        OverlayBlendSpan::segments(&[span(0, 1, 1), span(4, 9, 3)], 6),
        vec![(0, 1, Some(1)), (1, 3, None), (4, 2, Some(3))]
    );
    // No spans is one alpha run; an empty list is nothing.
    assert_eq!(OverlayBlendSpan::segments(&[], 4), vec![(0, 4, None)]);
    assert!(OverlayBlendSpan::segments(&[span(0, 2, 2)], 0).is_empty());
    // Overlap keeps the first claim.
    assert_eq!(
        OverlayBlendSpan::segments(&[span(0, 3, 2), span(1, 3, 1)], 5),
        vec![(0, 3, Some(2)), (3, 1, Some(1)), (4, 1, None)]
    );
}

/// Draw one full-target quad through the text shader's `mode` blend
/// pipeline over `clear`, sampling a 1x1 atlas texel `texel`, tinted
/// `tint`; return the centre pixel.
fn blend_quad(mode: u8, clear: f64, texel: [u8; 4], tint: [f32; 4]) -> Option<[u8; 4]> {
    let (device, queue) = headless_device()?;
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("test text shader"),
        source: wgpu::ShaderSource::Wgsl(TEXT_SHADER_SRC.into()),
    });
    let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
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
    let atlas = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &texel,
    );
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor::default());
    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &bgl,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(
                    &atlas.create_view(&wgpu::TextureViewDescriptor::default()),
                ),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&bgl],
        push_constant_ranges: &[],
    });
    let attrs = [
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
    ];
    let entry = if crate::psx_blend::src_shader_scale(mode) == 1.0 {
        "fs_blend"
    } else {
        "fs_blend_quarter"
    };
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 32,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &attrs,
            }],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some(entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(crate::psx_blend::blend_state(mode)),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });
    let mut verts: Vec<f32> = Vec::new();
    for (x, y) in [(-1.0, 1.0), (1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        verts.extend_from_slice(&[x, y, 0.5, 0.5]);
        verts.extend_from_slice(&tint);
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[0u32, 2, 1, 1, 2, 3]),
        usage: wgpu::BufferUsages::INDEX,
    });
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear,
                        g: clear,
                        b: clear,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        let c = crate::psx_blend::MODE0_BLEND_CONSTANT;
        rp.set_blend_constant(wgpu::Color {
            r: c,
            g: c,
            b: c,
            a: c,
        });
        rp.set_pipeline(&pipeline);
        rp.set_bind_group(0, &bg, &[]);
        rp.set_vertex_buffer(0, vbuf.slice(..));
        rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
        rp.draw_indexed(0..6, 0, 0..1);
    }
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(4),
            },
        },
        wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(enc.finish()));
    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait).ok()?;
    let data = slice.get_mapped_range();
    let off = 2 * 256 + 2 * 4;
    Some([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

#[test]
fn sprite_blend_pipelines_apply_each_psx_equation() {
    // B = 0.6 (153), F = white texel * 0.2 tint (51).
    let white = [255, 255, 255, 255];
    let f = [0.2, 0.2, 0.2, 1.0];
    let Some(sub) = blend_quad(2, 0.6, white, f) else {
        eprintln!("[skip] no GPU adapter");
        return;
    };
    let near = |got: u8, want: i32| (i32::from(got) - want).abs() <= 2;
    assert!(near(sub[0], 102), "ABR 2 = B - F: {sub:?}");
    let add = blend_quad(1, 0.6, white, f).unwrap();
    assert!(near(add[0], 204), "ABR 1 = B + F: {add:?}");
    let quarter = blend_quad(3, 0.6, white, f).unwrap();
    assert!(near(quarter[0], 166), "ABR 3 = B + F/4: {quarter:?}");
    let half = blend_quad(0, 0.6, white, f).unwrap();
    assert!(near(half[0], 102), "ABR 0 = B/2 + F/2: {half:?}");
    // The tint's alpha is not part of the equation.
    let faint = blend_quad(2, 0.6, white, [0.2, 0.2, 0.2, 0.1]).unwrap();
    assert!(near(faint[0], 102), "alpha ignored: {faint:?}");
    // A transparent atlas texel (PSX 0x0000) draws nothing.
    let hole = blend_quad(2, 0.6, [0, 0, 0, 0], f).unwrap();
    assert!(near(hole[0], 153), "0x0000 texel discards: {hole:?}");
}
