//! The per-frame draw path: [`Renderer::render`], scene/text uniform
//! staging, and the internal text-quad helpers. Split out of
//! `renderer.rs`.

use super::*;

/// A CPU-side RGBA8 readback of one rendered frame, produced by
/// [`Renderer::capture_rgba`]. `rgba` is exactly `width * height * 4`
/// bytes, row-major, top-to-bottom, no row padding.
#[derive(Debug, Clone)]
pub struct CaptureImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Renderer {
    /// Render the scene. Dispatches by [`RenderTarget`]:
    /// * `Clear` - clear to dark gray, no draws.
    /// * `Texture(t)` - letterboxed quad (Phase 1 TIM viewer).
    /// * `Mesh { mesh, mvp }` - depth-tested 3D mesh draw (Phase 1 TMD viewer).
    pub fn render(&self, target: RenderTarget<'_>) -> Result<()> {
        let frame = self
            .surface
            .get_current_texture()
            .context("get current swapchain texture")?;
        // Viewed as UNORM even when the surface itself is sRGB: the shaders
        // write PSX framebuffer bytes, which must reach the display unencoded
        // (see `choose_surface_format`).
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor {
            format: Some(self.view_format),
            ..Default::default()
        });
        self.encode_frame(target, &view)?;
        frame.present();
        Ok(())
    }

    /// Render one frame into `color_view` and submit it - the shared body of
    /// the on-screen [`Self::render`] (swapchain view) and the offscreen
    /// [`Self::capture_rgba`] (readback texture view). Does not present.
    fn encode_frame(&self, target: RenderTarget<'_>, color_view: &wgpu::TextureView) -> Result<()> {
        // Stage uniform writes before begin_render_pass.
        match &target {
            RenderTarget::Texture(t) => {
                let (sx, sy) =
                    letterbox_scale(self.config.width, self.config.height, t.width, t.height);
                self.queue.write_buffer(
                    &self.uniforms_buf,
                    0,
                    bytemuck::cast_slice(&[Uniforms {
                        scale: [sx, sy, 0.0, 0.0],
                    }]),
                );
            }
            RenderTarget::Mesh { mvp, .. }
            | RenderTarget::TexturedMesh { mvp, .. }
            | RenderTarget::VramMesh { mvp, .. }
            | RenderTarget::Lines { mvp, .. } => {
                let snap = if self.psx_mode.get() { 1.0f32 } else { 0.0 };
                self.queue.write_buffer(
                    &self.mesh_uniforms_buf,
                    0,
                    bytemuck::cast_slice(&[MeshUniforms {
                        mvp: mvp.to_cols_array_2d(),
                        // Light coming from upper-back-left in world space.
                        depth_cue: self.depth_cue.get(),
                        psx_params: [
                            self.config.width as f32,
                            self.config.height as f32,
                            snap,
                            snap, // .w = dither_enable (shares the psx_mode flag)
                        ],
                        tex_window: self.tex_window.get(),
                        grade: self.color_grade.get(),
                    }]),
                );
            }
            RenderTarget::Scene(scene) => {
                self.stage_scene_uniforms(scene);
                let mut overlays: Vec<&TextOverlay<'_>> = Vec::with_capacity(3);
                if let Some(s) = scene.overlay_sprites {
                    overlays.push(s);
                }
                if let Some(s) = scene.overlay_sprites_2 {
                    overlays.push(s);
                }
                if let Some(t) = scene.overlay_text {
                    overlays.push(t);
                }
                if !overlays.is_empty() {
                    self.scene_quad_ranges
                        .borrow_mut()
                        .clone_from(&self.stage_quad_overlays(&overlays));
                } else {
                    self.scene_quad_ranges.borrow_mut().clear();
                }
            }
            RenderTarget::TextOnly(overlay) => {
                self.scene_quad_ranges
                    .borrow_mut()
                    .clone_from(&self.stage_quad_overlays(&[overlay]));
            }
            RenderTarget::ScreenOverlay { prims, .. } => {
                self.stage_screen_overlay(prims);
            }
            RenderTarget::Clear => {}
        }

        let view = color_view;
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        {
            // Mesh paths use the depth attachment; texture/clear paths skip it
            // (it would just sit unused, but keeping the depth-stencil-attachment
            // optional avoids needing wgpu to validate it for 2D-only frames).
            let depth_attachment = matches!(
                target,
                RenderTarget::Mesh { .. }
                    | RenderTarget::TexturedMesh { .. }
                    | RenderTarget::VramMesh { .. }
                    | RenderTarget::Lines { .. }
                    | RenderTarget::Scene(_)
                    | RenderTarget::TextOnly(_)
                    | RenderTarget::ScreenOverlay { .. }
            )
            .then(|| wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            });
            let clear_rgba = match &target {
                RenderTarget::Scene(s) => s
                    .clear_color
                    .map(|c| wgpu::Color {
                        r: c[0] as f64,
                        g: c[1] as f64,
                        b: c[2] as f64,
                        a: c[3] as f64,
                    })
                    .unwrap_or(wgpu::Color {
                        r: 0.05,
                        g: 0.05,
                        b: 0.07,
                        a: 1.0,
                    }),
                _ => wgpu::Color {
                    r: 0.05,
                    g: 0.05,
                    b: 0.07,
                    a: 1.0,
                },
            };
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("legaia frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear_rgba),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: depth_attachment,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            match target {
                RenderTarget::Clear => {}
                RenderTarget::Texture(t) => {
                    rp.set_pipeline(&self.pipeline);
                    rp.set_bind_group(0, &t.bind_group, &[]);
                    rp.set_bind_group(1, &self.uniforms_bg, &[]);
                    rp.draw(0..4, 0..1);
                }
                RenderTarget::Mesh { mesh, .. } => {
                    rp.set_pipeline(&self.mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::TexturedMesh { mesh, texture, .. } => {
                    rp.set_pipeline(&self.textured_mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_bind_group(1, &texture.bind_group, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::VramMesh { mesh, vram, mvp } => {
                    rp.set_pipeline(&self.vram_mesh_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_bind_group(1, &vram.bind_group, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                    // PSX-faithful semi-transparency blend pass (see
                    // [`psx_blend`]): re-draw the semi prims back-to-front
                    // by per-prim depth (the retail ordering-table walk),
                    // selecting the matching ABR blend pipeline per run.
                    // Gated like the rest of the faithful extras.
                    if self.psx_mode.get() && mesh.has_semi_prims() {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        let mut list = self.blend_list.borrow_mut();
                        list.clear();
                        psx_blend::push_draw_prims(&mut list, false, 0, &mvp, mesh.semi_prims());
                        psx_blend::sort_blend_list(&mut list);
                        let mut bound_mode: Option<u8> = None;
                        psx_blend::coalesce_sorted(&list, |head, start, count| {
                            if bound_mode != Some(head.mode) {
                                rp.set_pipeline(
                                    &self.vram_mesh_blend_pipelines[head.mode as usize],
                                );
                                bound_mode = Some(head.mode);
                            }
                            rp.draw_indexed(start..start + count, 0, 0..1);
                        });
                    }
                }
                RenderTarget::Lines { mesh, .. } => {
                    rp.set_pipeline(&self.lines_pipeline);
                    rp.set_bind_group(0, &self.mesh_uniforms_bg, &[]);
                    rp.set_vertex_buffer(0, mesh.vertex_buf.slice(..));
                    rp.set_index_buffer(mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    rp.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
                RenderTarget::Scene(scene) => {
                    let bg_borrow = self.scene_uniforms_bg.borrow();
                    let bg: &wgpu::BindGroup = &bg_borrow;
                    rp.set_pipeline(&self.scene_vram_mesh_pipeline);
                    rp.set_bind_group(1, &scene.vram.bind_group, &[]);
                    let stride = self.uniform_offset_alignment;
                    for (i, draw) in scene.draws.iter().enumerate() {
                        let off = (i as u32) * stride;
                        rp.set_bind_group(0, bg, &[off]);
                        rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                        rp.set_index_buffer(
                            draw.mesh.index_buf.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                    }
                    if let Some((lines, _mvp)) = scene.overlay_lines {
                        // Overlay-lines uniforms live at slot N (one past
                        // the last actor), staged by `stage_scene_uniforms`.
                        let off = (scene.draws.len() as u32) * stride;
                        rp.set_pipeline(&self.scene_lines_pipeline);
                        rp.set_bind_group(0, bg, &[off]);
                        rp.set_vertex_buffer(0, lines.vertex_buf.slice(..));
                        rp.set_index_buffer(lines.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        rp.draw_indexed(0..lines.index_count, 0, 0..1);
                    }
                    if !scene.color_draws.is_empty() {
                        // Untextured F*/G* props: slots follow the draws + the
                        // optional overlay-lines slot (see stage_scene_uniforms).
                        let color_base =
                            scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                        rp.set_pipeline(&self.scene_color_mesh_pipeline);
                        for (i, draw) in scene.color_draws.iter().enumerate() {
                            let off = (color_base + i as u32) * stride;
                            rp.set_bind_group(0, bg, &[off]);
                            rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                            rp.set_index_buffer(
                                draw.mesh.index_buf.slice(..),
                                wgpu::IndexFormat::Uint32,
                            );
                            rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
                        }
                    }
                    // PSX-faithful semi-transparency blend pass (see
                    // [`psx_blend`]): after every opaque draw, re-draw the
                    // semi-transparent prims with the matching blend
                    // pipelines. Runs last among the 3D draws so blended
                    // fragments (which don't write depth) can't be
                    // overwritten by later opaque geometry.
                    //
                    // Ordering: retail inserts each semi prim into the
                    // depth-bucketed ordering table and blends back-to-front,
                    // interleaved across actors. The engine mirrors that at
                    // per-PRIMITIVE granularity - every semi prim of every
                    // semi-carrying draw (textured + untextured alike) is
                    // keyed by its centroid's clip-space `w` under the
                    // draw's MVP (= the average of its vertices' clip `w`,
                    // the GTE avg-Z the OT bins on) and the whole list is
                    // blended far-to-near, regardless of draw boundaries.
                    // Equal keys (one OT bucket) draw later-submitted-first,
                    // the retail LIFO bucket order (`AddPrim` prepends).
                    let any_semi = scene.draws.iter().any(|d| d.mesh.has_semi_prims())
                        || scene.color_draws.iter().any(|d| d.mesh.has_semi_prims());
                    if self.psx_mode.get() && any_semi {
                        let c = psx_blend::MODE0_BLEND_CONSTANT;
                        rp.set_blend_constant(wgpu::Color {
                            r: c,
                            g: c,
                            b: c,
                            a: c,
                        });
                        let color_base =
                            scene.draws.len() as u32 + scene.overlay_lines.is_some() as u32;
                        let mut list = self.blend_list.borrow_mut();
                        list.clear();
                        for (i, draw) in scene.draws.iter().enumerate() {
                            psx_blend::push_draw_prims(
                                &mut list,
                                false,
                                i as u32,
                                &draw.mvp,
                                draw.mesh.semi_prims(),
                            );
                        }
                        for (i, draw) in scene.color_draws.iter().enumerate() {
                            psx_blend::push_draw_prims(
                                &mut list,
                                true,
                                i as u32,
                                &draw.mvp,
                                draw.mesh.semi_prims(),
                            );
                        }
                        psx_blend::sort_blend_list(&mut list);
                        // Emit with state caching: rebind buffers + uniform
                        // offset only when the owning draw changes, switch
                        // pipelines only when the (path, ABR mode) changes;
                        // contiguous tail runs merge into one draw call
                        // (`coalesce_sorted`).
                        let mut bound_draw: Option<(bool, u32)> = None;
                        let mut bound_pipe: Option<(bool, u8)> = None;
                        psx_blend::coalesce_sorted(&list, |head, start, count| {
                            let draw_key = (head.untextured, head.draw_index);
                            if bound_draw != Some(draw_key) {
                                let (vbuf, ibuf, off) = if head.untextured {
                                    let m = scene.color_draws[head.draw_index as usize].mesh;
                                    (
                                        &m.vertex_buf,
                                        &m.index_buf,
                                        (color_base + head.draw_index) * stride,
                                    )
                                } else {
                                    let m = scene.draws[head.draw_index as usize].mesh;
                                    (&m.vertex_buf, &m.index_buf, head.draw_index * stride)
                                };
                                rp.set_vertex_buffer(0, vbuf.slice(..));
                                rp.set_index_buffer(ibuf.slice(..), wgpu::IndexFormat::Uint32);
                                rp.set_bind_group(0, bg, &[off]);
                                bound_draw = Some(draw_key);
                            }
                            let pipe_key = (head.untextured, head.mode);
                            if bound_pipe != Some(pipe_key) {
                                let pipelines = if head.untextured {
                                    &self.scene_color_mesh_blend_pipelines
                                } else {
                                    &self.scene_vram_mesh_blend_pipelines
                                };
                                rp.set_pipeline(&pipelines[head.mode as usize]);
                                bound_pipe = Some(pipe_key);
                            }
                            rp.draw_indexed(start..start + count, 0, 0..1);
                        });
                    }
                    let mut overlays: Vec<&TextOverlay<'_>> = Vec::with_capacity(3);
                    if let Some(s) = scene.overlay_sprites {
                        overlays.push(s);
                    }
                    if let Some(s) = scene.overlay_sprites_2 {
                        overlays.push(s);
                    }
                    if let Some(t) = scene.overlay_text {
                        overlays.push(t);
                    }
                    if !overlays.is_empty() {
                        let ranges = self.scene_quad_ranges.borrow();
                        if !ranges.iter().all(|(_, n)| *n == 0) {
                            rp.set_pipeline(&self.text_pipeline);
                            let vbuf_borrow = self.text_vbuf.borrow();
                            let ibuf_borrow = self.text_ibuf.borrow();
                            rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                            rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                            for (overlay, (base_quad, count)) in overlays.iter().zip(ranges.iter())
                            {
                                if *count == 0 {
                                    continue;
                                }
                                rp.set_bind_group(0, &overlay.atlas.bind_group, &[]);
                                let start = base_quad * 6;
                                let end = (base_quad + count) * 6;
                                rp.draw_indexed(start..end, 0, 0..1);
                            }
                        }
                    }
                }
                RenderTarget::TextOnly(text) => {
                    let ranges = self.scene_quad_ranges.borrow();
                    if let Some(&(base_quad, count)) = ranges.first()
                        && count > 0
                    {
                        rp.set_pipeline(&self.text_pipeline);
                        rp.set_bind_group(0, &text.atlas.bind_group, &[]);
                        let vbuf_borrow = self.text_vbuf.borrow();
                        let ibuf_borrow = self.text_ibuf.borrow();
                        rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                        rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                        let start = base_quad * 6;
                        let end = (base_quad + count) * 6;
                        rp.draw_indexed(start..end, 0, 0..1);
                    }
                }
                RenderTarget::ScreenOverlay { vram, .. } => {
                    self.draw_screen_overlay(&mut rp, vram);
                }
            }
        }
        self.queue.submit(std::iter::once(enc.finish()));
        Ok(())
    }

    /// Render one frame into an offscreen texture at the current surface
    /// size and read it back to a CPU RGBA8 buffer (no window / no present).
    /// This is the headless screenshot path - the deterministic replacement
    /// for scrapping the on-screen window with `scrot`. The returned image
    /// is exactly `width * height * 4` bytes, row-major, no padding.
    pub fn capture_rgba(&self, target: RenderTarget<'_>) -> Result<CaptureImage> {
        let (width, height) = (self.config.width, self.config.height);
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("capture target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // UNORM, like the presented frame: a screenshot must read back the
            // exact bytes the window shows (see `choose_surface_format`).
            format: self.view_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        self.encode_frame(target, &view)?;

        // copy_texture_to_buffer requires bytes_per_row aligned to 256.
        let bpp = 4u32;
        let unpadded = width * bpp;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded = unpadded.div_ceil(align) * align;
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture readback"),
            size: (padded as u64) * (height as u64),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("capture encoder"),
            });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buf,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(std::iter::once(enc.finish()));

        let (tx, rx) = std::sync::mpsc::channel();
        buf.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait())
            .context("poll device for capture readback")?;
        rx.recv()
            .context("capture readback channel closed")?
            .context("map capture readback buffer")?;

        let data = buf.slice(..).get_mapped_range();
        // Swizzle BGRA->RGBA when the surface uses a Bgra format (typical on
        // desktop swapchains); Rgba formats pass through unchanged.
        let bgra = matches!(
            self.view_format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut rgba = Vec::with_capacity((unpadded as usize) * (height as usize));
        for row in 0..height as usize {
            let start = row * padded as usize;
            let line = &data[start..start + unpadded as usize];
            if bgra {
                for px in line.chunks_exact(4) {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
                }
            } else {
                rgba.extend_from_slice(line);
            }
        }
        drop(data);
        buf.unmap();
        Ok(CaptureImage {
            rgba,
            width,
            height,
        })
    }

    /// Resize the scene-uniforms buffer (and its bind group) to hold at
    /// least `slots` `MeshUniforms` entries, then write each entry.
    fn stage_scene_uniforms(&self, scene: &Scene<'_>) {
        let stride = self.uniform_offset_alignment as usize;
        let needed =
            scene.draws.len() + scene.overlay_lines.is_some() as usize + scene.color_draws.len();
        if needed == 0 {
            return;
        }
        let mut cap = self.scene_uniforms_capacity.get();
        if cap < needed {
            // Grow geometrically so we don't churn on small N.
            cap = needed.next_power_of_two().max(needed);
            let new_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("scene mesh uniforms (resized)"),
                size: (cap * stride) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let new_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene mesh uniforms bg (resized)"),
                layout: &self.scene_uniforms_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &new_buf,
                        offset: 0,
                        size: std::num::NonZeroU64::new(std::mem::size_of::<MeshUniforms>() as u64),
                    }),
                }],
            });
            *self.scene_uniforms_buf.borrow_mut() = new_buf;
            *self.scene_uniforms_bg.borrow_mut() = new_bg;
            self.scene_uniforms_capacity.set(cap);
        }
        // Build a flat byte buffer with one MeshUniforms entry per slot,
        // padded to `stride`. wgpu rejects overlapping writes, so we hand
        // the queue a single contiguous range.
        let total = needed * stride;
        let mut bytes = vec![0u8; total];
        let snap = if self.psx_mode.get() { 1.0f32 } else { 0.0 };
        let psx_params = [
            self.config.width as f32,
            self.config.height as f32,
            snap,
            snap, // .w = dither_enable (shares the psx_mode flag)
        ];
        let tex_window = self.tex_window.get();
        let grade = self.color_grade.get();
        let push = |bytes: &mut [u8], slot: usize, mvp: Mat4| {
            let u = MeshUniforms {
                mvp: mvp.to_cols_array_2d(),
                depth_cue: self.depth_cue.get(),
                psx_params,
                tex_window,
                grade,
            };
            let off = slot * stride;
            let n = std::mem::size_of::<MeshUniforms>();
            bytes[off..off + n].copy_from_slice(bytemuck::bytes_of(&u));
        };
        for (i, draw) in scene.draws.iter().enumerate() {
            push(&mut bytes, i, draw.mvp);
        }
        if let Some((_, mvp)) = scene.overlay_lines {
            push(&mut bytes, scene.draws.len(), mvp);
        }
        // Colour-mesh slots follow the draws + the optional overlay-lines slot.
        let color_base = scene.draws.len() + scene.overlay_lines.is_some() as usize;
        for (i, draw) in scene.color_draws.iter().enumerate() {
            push(&mut bytes, color_base + i, draw.mvp);
        }
        let buf_borrow = self.scene_uniforms_buf.borrow();
        let buf: &wgpu::Buffer = &buf_borrow;
        self.queue.write_buffer(buf, 0, &bytes);
    }
}

/// Number of quads in `text` capped at u32::MAX/6, or `None` if there's
/// nothing to draw. Pulled out so the pre-pass staging and the in-pass draw
/// agree on what counts as renderable.
fn text_quad_count(text: &TextOverlay<'_>) -> Option<u32> {
    let n = text.draws.len();
    if n == 0 {
        return None;
    }
    Some(n as u32)
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct TextVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

impl Renderer {
    /// Build the per-frame text vertex/index buffers from one or more 2D
    /// quad overlays (sprite batches and text batches share the same
    /// pipeline; the only per-batch difference is the bound atlas). Quads
    /// are concatenated in input order; the returned `[(base_quad, count)]`
    /// pairs let the render pass issue one `draw_indexed` per overlay with
    /// the matching atlas bind group.
    ///
    /// Pixel coords are converted to NDC using the current surface size;
    /// atlas pixel coords are converted to `[0, 1]` UVs using each
    /// overlay's atlas size.
    fn stage_quad_overlays(&self, overlays: &[&TextOverlay<'_>]) -> Vec<(u32, u32)> {
        let mut total_quads: u32 = 0;
        let mut ranges: Vec<(u32, u32)> = Vec::with_capacity(overlays.len());
        for o in overlays {
            let n = text_quad_count(o).unwrap_or(0);
            ranges.push((total_quads, n));
            total_quads = total_quads.saturating_add(n);
        }
        if total_quads == 0 {
            return ranges;
        }
        let needed_v = total_quads * 4;
        let needed_i = total_quads * 6;
        if needed_v > self.text_vertex_capacity.get() {
            let cap = needed_v.next_power_of_two().max(needed_v);
            *self.text_vbuf.borrow_mut() = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad2d vertex buffer (resized)"),
                size: (cap as u64) * 32,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_vertex_capacity.set(cap);
        }
        if needed_i > self.text_index_capacity.get() {
            let cap = needed_i.next_power_of_two().max(needed_i);
            *self.text_ibuf.borrow_mut() = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("quad2d index buffer (resized)"),
                size: (cap as u64) * 4,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.text_index_capacity.set(cap);
        }

        let surf_w = self.config.width.max(1) as f32;
        let surf_h = self.config.height.max(1) as f32;

        let mut verts: Vec<TextVertex> = Vec::with_capacity(needed_v as usize);
        let mut idxs: Vec<u32> = Vec::with_capacity(needed_i as usize);
        let mut quad_idx: u32 = 0;
        for overlay in overlays {
            let atlas_w = overlay.atlas.width.max(1) as f32;
            let atlas_h = overlay.atlas.height.max(1) as f32;
            for draw in overlay.draws {
                let (dx, dy, dw, dh) = draw.dst;
                let (sx, sy, sw, sh) = draw.src;
                let nx0 = (dx as f32 / surf_w) * 2.0 - 1.0;
                let ny0 = 1.0 - (dy as f32 / surf_h) * 2.0;
                let nx1 = ((dx + dw as i32) as f32 / surf_w) * 2.0 - 1.0;
                let ny1 = 1.0 - ((dy + dh as i32) as f32 / surf_h) * 2.0;
                let u0 = sx as f32 / atlas_w;
                let v0 = sy as f32 / atlas_h;
                let u1 = (sx + sw) as f32 / atlas_w;
                let v1 = (sy + sh) as f32 / atlas_h;
                let color = draw.color;
                let base = quad_idx * 4;
                verts.push(TextVertex {
                    pos: [nx0, ny0],
                    uv: [u0, v0],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx1, ny0],
                    uv: [u1, v0],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx0, ny1],
                    uv: [u0, v1],
                    color,
                });
                verts.push(TextVertex {
                    pos: [nx1, ny1],
                    uv: [u1, v1],
                    color,
                });
                idxs.extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
                quad_idx += 1;
            }
        }
        let vbuf_borrow = self.text_vbuf.borrow();
        let ibuf_borrow = self.text_ibuf.borrow();
        self.queue
            .write_buffer(&vbuf_borrow, 0, bytemuck::cast_slice(&verts));
        self.queue
            .write_buffer(&ibuf_borrow, 0, bytemuck::cast_slice(&idxs));
        ranges
    }

    /// Build one frame's screen-overlay geometry from `prims` (see
    /// [`crate::screen_overlay::build_geometry`]), grow the dynamic
    /// vertex/index buffers if needed, upload the geometry, and cache the
    /// per-run draw list for [`Self::draw_screen_overlay`]. The staging pass
    /// of [`RenderTarget::ScreenOverlay`].
    fn stage_screen_overlay(&self, prims: &[crate::screen_overlay::ScreenPrim]) {
        let (w, h) = (self.config.width, self.config.height);
        let geo = crate::screen_overlay::build_geometry(prims, w, h);
        self.screen_overlay_runs.borrow_mut().clone_from(&geo.runs);
        if geo.is_empty() {
            return;
        }
        let needed_v = geo.vertices.len() as u32;
        let needed_i = geo.indices.len() as u32;
        if needed_v > self.screen_overlay_vcap.get() {
            let cap = needed_v.next_power_of_two().max(needed_v);
            *self.screen_overlay_vbuf.borrow_mut() =
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("screen overlay vertex buffer (resized)"),
                    size: (cap as u64) * crate::screen_overlay::SCREEN_VERTEX_STRIDE,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            self.screen_overlay_vcap.set(cap);
        }
        if needed_i > self.screen_overlay_icap.get() {
            let cap = needed_i.next_power_of_two().max(needed_i);
            *self.screen_overlay_ibuf.borrow_mut() =
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("screen overlay index buffer (resized)"),
                    size: (cap as u64) * 4,
                    usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
            self.screen_overlay_icap.set(cap);
        }
        let vbuf_borrow = self.screen_overlay_vbuf.borrow();
        let ibuf_borrow = self.screen_overlay_ibuf.borrow();
        self.queue
            .write_buffer(&vbuf_borrow, 0, bytemuck::cast_slice(&geo.vertices));
        self.queue
            .write_buffer(&ibuf_borrow, 0, bytemuck::cast_slice(&geo.indices));
    }

    /// Draw the runs staged by [`Self::stage_screen_overlay`]: one indexed
    /// draw per [`crate::screen_overlay::DrawRun`], binding the opaque
    /// pipeline or the matching per-ABR blend pipeline. Groups 0 = the shared
    /// PSX VRAM texture.
    fn draw_screen_overlay(&self, rp: &mut wgpu::RenderPass<'_>, vram: &UploadedVram) {
        use crate::screen_overlay::BlendClass;
        let runs = self.screen_overlay_runs.borrow();
        if runs.is_empty() {
            return;
        }
        // Mode-0 (`0.5*B + 0.5*F`) uses BlendFactor::Constant on both sides.
        let c = psx_blend::MODE0_BLEND_CONSTANT;
        rp.set_blend_constant(wgpu::Color {
            r: c,
            g: c,
            b: c,
            a: c,
        });
        rp.set_bind_group(0, &vram.bind_group, &[]);
        let vbuf_borrow = self.screen_overlay_vbuf.borrow();
        let ibuf_borrow = self.screen_overlay_ibuf.borrow();
        rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
        rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
        let mut bound: Option<BlendClass> = None;
        for run in runs.iter() {
            if bound != Some(run.class) {
                match run.class {
                    BlendClass::Opaque => rp.set_pipeline(&self.screen_overlay_pipeline),
                    BlendClass::Semi(mode) => {
                        rp.set_pipeline(&self.screen_overlay_blend_pipelines[(mode & 0x3) as usize])
                    }
                }
                bound = Some(run.class);
            }
            rp.draw_indexed(run.index_start..run.index_start + run.index_count, 0, 0..1);
        }
    }
}
