//! The per-frame draw path: [`Renderer::render`], scene/text uniform
//! staging, and the internal text-quad helpers.

use super::*;

mod capture;
mod overlay_staging;
mod scene_staging;

pub use capture::*;

/// Panic message for the two arms that `encode_frame`'s normalisation makes
/// unreachable. Kept as one constant so the invariant is stated once.
const NORMALISED: &str =
    "RenderTarget::SceneWithScreenPrims is rebound to Scene before these matches";

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
        // `acquire` is the swapchain wait: under vsync (and under a full frame
        // queue with vsync off) this is where the CPU blocks, so keeping it as
        // its own profiler stage stops it from being misread as GPU-submit cost.
        crate::profile::mark("acquire");
        self.encode_frame(target, &view)?;
        frame.present();
        crate::profile::mark("present");
        Ok(())
    }

    /// Render one frame into `color_view` and submit it - the shared body of
    /// the on-screen [`Self::render`] (swapchain view) and the offscreen
    /// [`Self::capture_rgba`] (readback texture view). Does not present.
    fn encode_frame(&self, target: RenderTarget<'_>, color_view: &wgpu::TextureView) -> Result<()> {
        // `SceneWithScreenPrims` is a `Scene` plus a composited ordering-table
        // pass; unwrap it here so the scene arms below stay single-purpose and
        // the overlay is drawn as a tail on top of whatever the scene emitted.
        let empty: &[legaia_render_kernels::screen_prim::ScreenPrim] = &[];
        let (target, composited_prims, under_prims) = match target {
            RenderTarget::SceneWithScreenPrims {
                scene,
                prims,
                under_overlay,
            } => (RenderTarget::Scene(scene), prims, under_overlay),
            other => (other, empty, empty),
        };
        let composited_vram = match &target {
            RenderTarget::Scene(s) => Some(s.vram),
            _ => None,
        };
        let composite_overlay =
            composited_vram.is_some() && !(composited_prims.is_empty() && under_prims.is_empty());
        if composited_vram.is_some() {
            self.stage_screen_overlay_split(under_prims, composited_prims);
        }
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
                        // Reversed-Z applied centrally (see `reverse_z`).
                        mvp: reverse_z(fold_screen_rows(*mvp, self.overworld_curve_bias.get()))
                            .to_cols_array_2d(),
                        // Light coming from upper-back-left in world space.
                        depth_cue: self.depth_cue.get(),
                        psx_params: [
                            self.config.width as f32,
                            self.config.height as f32,
                            snap,
                            snap, // .w = dither_enable (shares the psx_mode flag)
                        ],
                        tex_window: [pack_tex_window(self.tex_window.get()), 0, 0, 0],
                        grade: self.color_grade.get(),
                        flags: [
                            self.backface_cull.get(),
                            if self.semi_blend.get() { 1.0 } else { 0.0 },
                            0.0,
                            self.overworld_curve.get(),
                        ],
                        light_dir: self.dyn_light_dir_uniform(),
                        light_color: self.dyn_light_color_uniform(),
                        cue_ramp: self.cue_ramp.get(),
                        palette: self.palette_grade.get(),
                        model_rows: MODEL_ROWS_IDENTITY,
                    }]),
                );
            }
            RenderTarget::Scene(scene) => {
                self.stage_scene_uniforms(scene);
                self.stage_scene_lights_and_shadows(scene);
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
            // Rebound to `Scene` at the top of this function.
            RenderTarget::SceneWithScreenPrims { .. } => unreachable!("{NORMALISED}"),
        }
        crate::profile::mark("uniforms");
        // Enhanced lighting's glow sprites: expanded to camera-facing quads
        // against this frame's scene camera. `None` (the faithful path, any
        // non-scene target, no staged sprites) draws nothing.
        let glow = match &target {
            RenderTarget::Scene(_) => self.stage_glow_sprites(),
            _ => None,
        };

        let view = color_view;
        // The bloom enhancement: the scene half of the frame renders into
        // an offscreen target the bloom passes read, and their composite
        // lands on `view`. `None` (bloom off, a non-scene target) is the
        // single pass straight onto `view` the frame always was.
        let bloom_target = match &target {
            RenderTarget::Scene(_) => self.bloom_scene_target(),
            _ => None,
        };
        let scene_view: &wgpu::TextureView = bloom_target.as_ref().unwrap_or(view);
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame encoder"),
            });
        {
            // Mesh paths use the depth attachment; texture/clear paths skip it
            // (it would just sit unused, but keeping the depth-stencil-attachment
            // optional avoids needing wgpu to validate it for 2D-only frames).
            // The volumetric ground fog splits the scene pass in two around a
            // fog pass of its own (it samples the scene depth, which an open
            // pass that writes depth cannot). With no bank staged the frame
            // is the single pass it always was.
            let fog_split =
                self.fog_volume_active.get() && matches!(target, RenderTarget::Scene(_));
            // Either enhancement ends the 3D half in a pass of its own.
            let scene_split = fog_split || bloom_target.is_some();
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
                    // Reversed-Z: 0.0 is the far plane (see `reverse_z`).
                    load: wgpu::LoadOp::Clear(DEPTH_CLEAR),
                    // Kept when the ground-fog pass will sample it after the
                    // 3D draws; discarded otherwise, as it always was.
                    store: if scene_split {
                        wgpu::StoreOp::Store
                    } else {
                        wgpu::StoreOp::Discard
                    },
                }),
                stencil_ops: None,
            });
            // With a scene viewport set, the clear colour belongs to the
            // stage rect only: the letterbox around it is black (retail has
            // no letterbox - its clear colour covers exactly the 320x240
            // display), and the stage rect is filled with the clear colour
            // by `viewport_fill` as the scene pass's first draw.
            let viewport_fill = self.stage_viewport_fill(&target);
            let clear_rgba = match &target {
                RenderTarget::Scene(_) if viewport_fill.is_some() => wgpu::Color::BLACK,
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
            let viewport_fill = viewport_fill.is_some();
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("legaia frame pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: scene_view,
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
            // The stage rect the 3D pass and the screen-primitive overlay are
            // confined to (see `set_scene_viewport`); the text / sprite
            // overlays are in surface pixels and take the whole target.
            let (full_w, full_h) = (self.config.width, self.config.height);
            let scene_vp = self
                .scene_viewport
                .get()
                .filter(|&(x, y, w, h)| w > 0 && h > 0 && x + w <= full_w && y + h <= full_h);
            let set_scene_vp = |rp: &mut wgpu::RenderPass<'_>| {
                if let Some((x, y, w, h)) = scene_vp {
                    rp.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
                }
            };
            let set_full_vp = |rp: &mut wgpu::RenderPass<'_>| {
                if scene_vp.is_some() {
                    rp.set_viewport(0.0, 0.0, full_w as f32, full_h as f32, 0.0, 1.0);
                }
            };
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
                    if self.semi_blend.get() && mesh.has_semi_prims() {
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
                    set_scene_vp(&mut rp);
                    if viewport_fill {
                        // Depth test passes against the cleared far plane and
                        // the pipeline writes no depth, so every 3D draw
                        // below still lands over the fill.
                        rp.set_pipeline(&self.screen_overlay_pipeline);
                        rp.set_bind_group(0, &scene.vram.bind_group, &[]);
                        rp.set_vertex_buffer(0, self.viewport_fill_vbuf.slice(..));
                        rp.set_index_buffer(
                            self.viewport_fill_ibuf.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        rp.draw_indexed(0..6, 0, 0..1);
                    }
                    // The composited backdrops (the colour wash, the
                    // overworld sky band): retail's farthest bucket, drawn
                    // before every 3D primitive of the frame.
                    if composite_overlay {
                        self.draw_screen_overlay_backdrop_runs(&mut rp, scene.vram);
                    }
                    let bg_borrow = self.scene_uniforms_bg.borrow();
                    let bg: &wgpu::BindGroup = &bg_borrow;
                    rp.set_pipeline(&self.scene_vram_mesh_pipeline);
                    rp.set_bind_group(1, &scene.vram.bind_group, &[]);
                    // Point-light layer (lights uniform + shadow array +
                    // comparison sampler). Always bound - the uniform's
                    // light count is zero whenever the layer is inert, and
                    // every scene pipeline (VRAM + colour, opaque + blend)
                    // shares the same three-group layout so the binding
                    // stays valid across the whole pass.
                    rp.set_bind_group(2, &self.scene_lights_bg, &[]);
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
                    if self.semi_blend.get() && any_semi {
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
                    // Enhanced lighting's glow sprites (halos + light shafts):
                    // additive, depth-tested, after every opaque and blended
                    // 3D draw so they bloom over the lamps they surround.
                    if let Some((buf, count)) = &glow {
                        rp.set_pipeline(&self.glow_pipeline);
                        rp.set_bind_group(0, &self.glow_uniforms_bg, &[]);
                        rp.set_vertex_buffer(0, buf.slice(..));
                        rp.draw(0..*count, 0..1);
                    }
                    // The volumetric ground-fog enhancement: after every 3D
                    // draw (depth-tested against them, writing none), before
                    // every screen-space layer and the HUD.
                    // Bloom then reads that finished 3D half and composites
                    // it onto the presented target, under every screen layer.
                    if scene_split {
                        drop(rp);
                        if fog_split {
                            self.encode_fog_volume_pass(&mut enc, scene_view, scene_vp);
                        }
                        if bloom_target.is_some() {
                            self.encode_bloom_passes(&mut enc, view);
                        }
                        rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("legaia frame pass (after fog / bloom)"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view,
                                depth_slice: None,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Load,
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: Some(
                                wgpu::RenderPassDepthStencilAttachment {
                                    view: &self.depth_view,
                                    depth_ops: Some(wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Discard,
                                    }),
                                    stencil_ops: None,
                                },
                            ),
                            occlusion_query_set: None,
                            timestamp_writes: None,
                        });
                        set_scene_vp(&mut rp);
                    }
                    // The composited prims that sit UNDER the 2D overlays
                    // (`SceneWithScreenPrims::under_overlay`): the field
                    // attached-light pools, which retail draws beneath the
                    // party HUD.
                    if composite_overlay {
                        set_scene_vp(&mut rp);
                        self.draw_screen_overlay_runs(&mut rp, scene.vram, true);
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
                        set_full_vp(&mut rp);
                        let ranges = self.scene_quad_ranges.borrow();
                        if !ranges.iter().all(|(_, n)| *n == 0) {
                            let vbuf_borrow = self.text_vbuf.borrow();
                            let ibuf_borrow = self.text_ibuf.borrow();
                            rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                            rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                            for (overlay, (base_quad, count)) in overlays.iter().zip(ranges.iter())
                            {
                                self.draw_quad_overlay(&mut rp, overlay, *base_quad, *count);
                            }
                        }
                    }
                }
                RenderTarget::TextOnly(text) => {
                    let ranges = self.scene_quad_ranges.borrow();
                    if let Some(&(base_quad, count)) = ranges.first()
                        && count > 0
                    {
                        let vbuf_borrow = self.text_vbuf.borrow();
                        let ibuf_borrow = self.text_ibuf.borrow();
                        rp.set_vertex_buffer(0, vbuf_borrow.slice(..));
                        rp.set_index_buffer(ibuf_borrow.slice(..), wgpu::IndexFormat::Uint32);
                        self.draw_quad_overlay(&mut rp, text, base_quad, count);
                    }
                }
                RenderTarget::ScreenOverlay { vram, .. } => {
                    set_scene_vp(&mut rp);
                    self.draw_screen_overlay(&mut rp, vram);
                }
                // Rebound to `Scene` at the top of this function.
                RenderTarget::SceneWithScreenPrims { .. } => unreachable!("{NORMALISED}"),
            }
            // The composited ordering-table tail of `SceneWithScreenPrims`:
            // drawn last so the quads sit over the scene's meshes, sprites and
            // text, at the reversed-Z near plane.
            if let (true, Some(vram)) = (composite_overlay, composited_vram) {
                set_scene_vp(&mut rp);
                self.draw_screen_overlay_runs(&mut rp, vram, false);
            }
        }
        crate::profile::mark("encode");
        self.queue.submit(std::iter::once(enc.finish()));
        crate::profile::mark("submit");
        Ok(())
    }
}
