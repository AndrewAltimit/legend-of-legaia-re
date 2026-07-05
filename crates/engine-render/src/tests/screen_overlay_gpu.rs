//! Headless GPU smoke test for the screen-space overlay pass. Builds the
//! *real* `SCREEN_OVERLAY_SHADER_SRC` + vertex layout + `build_geometry`
//! output and renders to an offscreen target on whatever adapter is present,
//! reading pixels back. Skips (passes vacuously) when no GPU adapter is
//! available - CI machines without a render node just get the CPU-side
//! `screen_overlay::tests` + naga validation coverage.

use super::*;
use crate::screen_overlay::{
    BlendClass, FlatQuad, SCREEN_VERTEX_STRIDE, ScreenPrim, ScreenQuad, build_geometry,
};
use wgpu::util::DeviceExt;

const TARGET: usize = 4; // 4x4 offscreen render target

/// Attempt to get a headless device+queue; `None` when no adapter exists.
fn headless_device() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("screen overlay test device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::downlevel_defaults(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .ok()?;
    Some((device, queue))
}

fn vram_attributes() -> [wgpu::VertexAttribute; 5] {
    [
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
    ]
}

/// Build the full test harness: opaque + per-mode blend pipelines, a small
/// VRAM texture (one red 15bpp texel at (5,5)) and its bind group.
struct Harness {
    device: wgpu::Device,
    queue: wgpu::Queue,
    opaque: wgpu::RenderPipeline,
    blend: [wgpu::RenderPipeline; 4],
    vram_bg: wgpu::BindGroup,
    color_tex: wgpu::Texture,
    depth_view: wgpu::TextureView,
}

fn build_harness(device: wgpu::Device, queue: wgpu::Queue) -> Harness {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("test screen overlay shader"),
        source: wgpu::ShaderSource::Wgsl(SCREEN_OVERLAY_SHADER_SRC.into()),
    });
    let vram_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("test vram bgl"),
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
        label: Some("test overlay layout"),
        bind_group_layouts: &[&vram_bgl],
        push_constant_ranges: &[],
    });
    let attrs = vram_attributes();
    let vlayout = wgpu::VertexBufferLayout {
        array_stride: SCREEN_VERTEX_STRIDE,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &attrs,
    };
    let depth = wgpu::DepthStencilState {
        format: wgpu::TextureFormat::Depth32Float,
        depth_write_enabled: false,
        depth_compare: wgpu::CompareFunction::LessEqual,
        stencil: wgpu::StencilState::default(),
        bias: wgpu::DepthBiasState::default(),
    };
    let mk = |entry: &str, blend: Option<wgpu::BlendState>| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("test overlay pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: std::slice::from_ref(&vlayout),
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(entry),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(depth.clone()),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    };
    let opaque = mk("fs_opaque", Some(wgpu::BlendState::ALPHA_BLENDING));
    let blend: [wgpu::RenderPipeline; 4] = std::array::from_fn(|m| {
        let entry = if crate::psx_blend::src_shader_scale(m as u8) == 1.0 {
            "fs_blend"
        } else {
            "fs_blend_quarter"
        };
        mk(entry, Some(crate::psx_blend::blend_state(m as u8)))
    });

    // 64x64 R16Uint VRAM with a red 15bpp texel (0x001F) at (5,5).
    let vram = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test vram"),
        size: wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R16Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut words = vec![0u16; 64 * 64];
    words[5 * 64 + 5] = 0x001F; // red, STP implied by non-zero
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &vram,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytemuck::cast_slice(&words),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(64 * 2),
            rows_per_image: Some(64),
        },
        wgpu::Extent3d {
            width: 64,
            height: 64,
            depth_or_array_layers: 1,
        },
    );
    let vram_view = vram.create_view(&wgpu::TextureViewDescriptor::default());
    let vram_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("test vram bg"),
        layout: &vram_bgl,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(&vram_view),
        }],
    });

    let color_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test color target"),
        size: wgpu::Extent3d {
            width: TARGET as u32,
            height: TARGET as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let depth_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test depth"),
        size: wgpu::Extent3d {
            width: TARGET as u32,
            height: TARGET as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth_view = depth_tex.create_view(&wgpu::TextureViewDescriptor::default());
    Harness {
        device,
        queue,
        opaque,
        blend,
        vram_bg,
        color_tex,
        depth_view,
    }
}

/// Render one primitive list over a clear colour and read back the centre
/// pixel as RGBA8.
fn render_center_pixel(h: &Harness, prims: &[ScreenPrim], clear: [f64; 4]) -> [u8; 4] {
    let geo = build_geometry(prims, TARGET as u32, TARGET as u32);
    // Empty geometry -> clear only (an empty buffer slice is invalid).
    let buffers = (!geo.is_empty()).then(|| {
        let vbuf = h
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("test overlay vbuf"),
                contents: bytemuck::cast_slice(&geo.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
        let ibuf = h
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("test overlay ibuf"),
                contents: bytemuck::cast_slice(&geo.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
        (vbuf, ibuf)
    });
    let color_view = h
        .color_tex
        .create_view(&wgpu::TextureViewDescriptor::default());
    let mut enc = h
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("test encoder"),
        });
    {
        let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("test pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear[0],
                        g: clear[1],
                        b: clear[2],
                        a: clear[3],
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &h.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
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
        rp.set_bind_group(0, &h.vram_bg, &[]);
        if let Some((vbuf, ibuf)) = &buffers {
            rp.set_vertex_buffer(0, vbuf.slice(..));
            rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
            for run in &geo.runs {
                match run.class {
                    BlendClass::Opaque => rp.set_pipeline(&h.opaque),
                    BlendClass::Semi(m) => rp.set_pipeline(&h.blend[(m & 3) as usize]),
                }
                rp.draw_indexed(run.index_start..run.index_start + run.index_count, 0, 0..1);
            }
        }
    }
    // Read back the whole 4x4 (bytes_per_row must be 256-aligned).
    let padded = 256u32;
    let buf = h.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test readback"),
        size: (padded * TARGET as u32) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &h.color_tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(TARGET as u32),
            },
        },
        wgpu::Extent3d {
            width: TARGET as u32,
            height: TARGET as u32,
            depth_or_array_layers: 1,
        },
    );
    h.queue.submit(std::iter::once(enc.finish()));
    let (tx, rx) = std::sync::mpsc::channel();
    buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    h.device.poll(wgpu::PollType::wait()).unwrap();
    rx.recv().unwrap().unwrap();
    let data = buf.slice(..).get_mapped_range();
    // Centre pixel (2,2).
    let row = 2usize;
    let col = 2usize;
    let off = row * padded as usize + col * 4;
    let px = [data[off], data[off + 1], data[off + 2], data[off + 3]];
    drop(data);
    buf.unmap();
    px
}

#[test]
fn screen_overlay_pipeline_draws_on_gpu() {
    let Some((device, queue)) = headless_device() else {
        eprintln!("no GPU adapter; skipping screen_overlay_pipeline_draws_on_gpu");
        return;
    };
    let h = build_harness(device, queue);

    // 1) Opaque flat red quad covering the whole target.
    let flat = ScreenPrim::Flat(FlatQuad {
        xy: [
            (0, 0),
            (TARGET as i16, 0),
            (0, TARGET as i16),
            (TARGET as i16, TARGET as i16),
        ],
        color: [255, 0, 0, 255],
        semi_transparent: false,
        abr_mode: 0,
        ot_index: 10,
    });
    let px = render_center_pixel(&h, &[flat], [0.0, 0.0, 0.0, 1.0]);
    assert!(
        px[0] > 200 && px[1] < 40 && px[2] < 40,
        "opaque flat red: {px:?}"
    );

    // 2) Textured quad sampling the red 15bpp VRAM texel (uv (5,5), 15bpp
    //    depth => tpage bit; neutral 0x808080 modulation passes it through).
    let textured = ScreenPrim::Textured(ScreenQuad {
        xy: [
            (0, 0),
            (TARGET as i16, 0),
            (0, TARGET as i16),
            (TARGET as i16, TARGET as i16),
        ],
        uv: [(5, 5); 4],
        clut: 0,
        tpage: 2 << 7, // depth = 2 (15bpp)
        color: 0x0080_8080,
        semi_transparent: false,
        ot_index: 10,
    });
    let px = render_center_pixel(&h, &[textured], [0.0, 0.0, 0.0, 1.0]);
    assert!(
        px[0] > 200 && px[1] < 40 && px[2] < 40,
        "textured 15bpp red decode: {px:?}"
    );

    // 3) Additive (ABR mode 1) flat quad over a grey clear brightens red.
    let add = ScreenPrim::Flat(FlatQuad {
        xy: [
            (0, 0),
            (TARGET as i16, 0),
            (0, TARGET as i16),
            (TARGET as i16, TARGET as i16),
        ],
        color: [128, 0, 0, 255],
        semi_transparent: true,
        abr_mode: 1,
        ot_index: 10,
    });
    let bg = render_center_pixel(&h, &[], [0.25, 0.25, 0.25, 1.0]);
    let blended = render_center_pixel(&h, &[add], [0.25, 0.25, 0.25, 1.0]);
    assert!(
        blended[0] > bg[0] + 40,
        "additive blend should brighten red: bg={bg:?} blended={blended:?}"
    );
}
