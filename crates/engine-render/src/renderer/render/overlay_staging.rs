//! Text / sprite quad overlays and the screen-space primitive overlay: staging and draw.

use super::*;

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
    pub(super) fn stage_quad_overlays(&self, overlays: &[&TextOverlay<'_>]) -> Vec<(u32, u32)> {
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
    /// [`legaia_render_kernels::screen_prim::build_geometry`]), grow the dynamic
    /// vertex/index buffers if needed, upload the geometry, and cache the
    /// per-run draw list for [`Self::draw_screen_overlay`]. The staging pass
    /// of [`RenderTarget::ScreenOverlay`].
    /// The geometry space is the **PSX display** (320x240), not the window:
    /// every retail screen-space emitter authors there and clamps against it,
    /// so handing `build_geometry` the surface size would pin the overlay into
    /// the top-left corner of a larger frame. See
    /// [`legaia_render_kernels::screen_prim::build_geometry`].
    pub(super) fn stage_screen_overlay(
        &self,
        prims: &[legaia_render_kernels::screen_prim::ScreenPrim],
    ) {
        self.stage_screen_overlay_split(&[], prims);
    }

    /// Stage two ordering-table lists into the one overlay buffer: `under`
    /// (drawn before the 2D sprite / text overlays) first, then `over`. Each
    /// list is ordered on its own by the shared builder; the split point is
    /// the number of runs `under` produced. `under`'s backdrops
    /// ([`legaia_render_kernels::screen_prim::split_backdrop`]) lead the
    /// buffer as a list of their own, which the scene pass draws first.
    pub(super) fn stage_screen_overlay_split(
        &self,
        under: &[legaia_render_kernels::screen_prim::ScreenPrim],
        over: &[legaia_render_kernels::screen_prim::ScreenPrim],
    ) {
        let (w, h) = (
            legaia_render_kernels::vram_capture::PSX_SCREEN_WIDTH as u32,
            legaia_render_kernels::vram_capture::PSX_SCREEN_HEIGHT as u32,
        );
        use legaia_render_kernels::screen_prim::{OverlayGeometry, build_geometry, split_backdrop};
        let append =
            |geo: &mut OverlayGeometry, list: &[legaia_render_kernels::screen_prim::ScreenPrim]| {
                if list.is_empty() {
                    return;
                }
                let tail = build_geometry(list, w, h);
                let (vbase, ibase) = (geo.vertices.len() as u32, geo.indices.len() as u32);
                geo.vertices.extend(tail.vertices);
                geo.indices.extend(tail.indices.iter().map(|i| i + vbase));
                geo.runs.extend(tail.runs.into_iter().map(|mut r| {
                    r.index_start += ibase;
                    r
                }));
            };
        let (backdrop, under) = split_backdrop(under);
        let mut geo = build_geometry(&backdrop, w, h);
        self.screen_overlay_backdrop_runs.set(geo.runs.len());
        append(&mut geo, &under);
        self.screen_overlay_under_runs.set(geo.runs.len());
        append(&mut geo, over);
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
                    size: (cap as u64) * legaia_render_kernels::screen_prim::SCREEN_VERTEX_STRIDE,
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
    /// draw per [`legaia_render_kernels::screen_prim::DrawRun`], binding the opaque
    /// pipeline or the matching per-ABR blend pipeline. Groups 0 = the shared
    /// PSX VRAM texture.
    /// Stage the scene-viewport fill for this frame: when the target is a
    /// scene with a non-black clear colour and a
    /// [`scene viewport`](Self::set_scene_viewport) narrower than the target,
    /// write one flat quad in that colour over the viewport and return the
    /// colour. The render pass then clears the whole target black and draws
    /// the quad first, so only the stage rect reads as the clear colour -
    /// before this, a stage battle's sky clear filled the letterbox bars too.
    pub(super) fn stage_viewport_fill(&self, target: &RenderTarget<'_>) -> Option<[f32; 4]> {
        let RenderTarget::Scene(scene) = target else {
            return None;
        };
        let c = scene.clear_color?;
        if c[0] == 0.0 && c[1] == 0.0 && c[2] == 0.0 {
            return None;
        }
        let (x, y, w, h) = self.scene_viewport.get()?;
        let (full_w, full_h) = (self.config.width, self.config.height);
        if w == 0 || h == 0 || x + w > full_w || y + h > full_h || (w, h) == (full_w, full_h) {
            return None;
        }
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        let (sw, sh) = (
            legaia_render_kernels::vram_capture::PSX_SCREEN_WIDTH as i16,
            legaia_render_kernels::vram_capture::PSX_SCREEN_HEIGHT as i16,
        );
        let quad = legaia_render_kernels::screen_prim::ScreenPrim::Flat(
            legaia_render_kernels::screen_prim::FlatQuad {
                xy: [(0, 0), (sw, 0), (0, sh), (sw, sh)],
                color: [byte(c[0]), byte(c[1]), byte(c[2]), 255],
                gouraud: None,
                semi_transparent: false,
                abr_mode: 0,
                ot_index: 0,
                depth: None,
            },
        );
        let geo = legaia_render_kernels::screen_prim::build_geometry(&[quad], sw as u32, sh as u32);
        self.queue.write_buffer(
            &self.viewport_fill_vbuf,
            0,
            bytemuck::cast_slice(&geo.vertices),
        );
        self.queue.write_buffer(
            &self.viewport_fill_ibuf,
            0,
            bytemuck::cast_slice(&geo.indices),
        );
        Some(c)
    }

    /// Draw one staged quad overlay (`count` quads from `base_quad` of the
    /// buffers [`Self::stage_quad_overlays`] filled, already bound): the
    /// alpha pipeline outside the overlay's [`OverlayBlendSpan`]s, the
    /// span's ABR pipeline inside one, in draw order.
    pub(super) fn draw_quad_overlay(
        &self,
        rp: &mut wgpu::RenderPass<'_>,
        overlay: &TextOverlay<'_>,
        base_quad: u32,
        count: u32,
    ) {
        if count == 0 {
            return;
        }
        rp.set_bind_group(0, &overlay.atlas.bind_group, &[]);
        for (start, n, abr) in OverlayBlendSpan::segments(overlay.blend, count) {
            match abr {
                None => rp.set_pipeline(&self.text_pipeline[usize::from(self.ui_psx_tint.get())]),
                Some(mode) => {
                    let c = psx_blend::MODE0_BLEND_CONSTANT;
                    rp.set_blend_constant(wgpu::Color {
                        r: c,
                        g: c,
                        b: c,
                        a: c,
                    });
                    rp.set_pipeline(
                        &self.text_blend_pipelines[usize::from(self.ui_psx_tint.get())]
                            [mode as usize],
                    );
                }
            }
            let first = (base_quad + start) * 6;
            rp.draw_indexed(first..first + n * 6, 0, 0..1);
        }
    }

    pub(super) fn draw_screen_overlay(&self, rp: &mut wgpu::RenderPass<'_>, vram: &UploadedVram) {
        self.draw_screen_overlay_runs(rp, vram, false);
    }

    /// Draw one half of the staged runs: `under == true` draws the runs of
    /// the `under` list [`Self::stage_screen_overlay_split`] staged, `false`
    /// the rest.
    pub(super) fn draw_screen_overlay_runs(
        &self,
        rp: &mut wgpu::RenderPass<'_>,
        vram: &UploadedVram,
        under: bool,
    ) {
        let all = self.screen_overlay_runs.borrow();
        let split = self.screen_overlay_under_runs.get().min(all.len());
        let back = self.screen_overlay_backdrop_runs.get().min(split);
        let runs = if under {
            &all[back..split]
        } else {
            &all[split..]
        };
        self.draw_runs(rp, vram, runs);
    }

    /// Draw the staged backdrops - the screen primitives at the far plane,
    /// retail's farthest ordering-table bucket - ahead of the scene's 3D
    /// draws, so blended scene primitives (which write no depth) land over
    /// them as they do over retail's first-drawn packets.
    pub(super) fn draw_screen_overlay_backdrop_runs(
        &self,
        rp: &mut wgpu::RenderPass<'_>,
        vram: &UploadedVram,
    ) {
        let all = self.screen_overlay_runs.borrow();
        let back = self.screen_overlay_backdrop_runs.get().min(all.len());
        self.draw_runs(rp, vram, &all[..back]);
    }

    fn draw_runs(
        &self,
        rp: &mut wgpu::RenderPass<'_>,
        vram: &UploadedVram,
        runs: &[legaia_render_kernels::screen_prim::DrawRun],
    ) {
        use legaia_render_kernels::screen_prim::BlendClass;
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
        // A semi-transparent run draws twice: its textured prims' STP-clear
        // texels solid first (`fs_semi_opaque`), then the blend pipeline for
        // the STP-set texels and the untextured prims - the PSX per-texel
        // rule, which a single fixed-function blend state cannot express.
        let mut bound_opaque = false;
        for run in runs.iter() {
            let range = run.index_start..run.index_start + run.index_count;
            match run.class {
                BlendClass::Opaque => {
                    if !bound_opaque {
                        rp.set_pipeline(&self.screen_overlay_pipeline);
                        bound_opaque = true;
                    }
                }
                BlendClass::Semi(mode) => {
                    rp.set_pipeline(&self.screen_overlay_semi_opaque_pipeline);
                    rp.draw_indexed(range.clone(), 0, 0..1);
                    rp.set_pipeline(&self.screen_overlay_blend_pipelines[(mode & 0x3) as usize]);
                    bound_opaque = false;
                }
            }
            rp.draw_indexed(range, 0, 0..1);
        }
    }
}
