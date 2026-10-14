//! Per-frame scene staging: glow sprites, the point lights and their shadow maps, and the scene uniforms.

use super::*;
use bytemuck::Zeroable;

impl Renderer {
    /// Expand the staged glow sprites against the registered scene camera
    /// into a one-frame vertex buffer and stage the glow VP uniform.
    /// `None` unless dynamic lighting is on, a scene camera is registered
    /// and sprites are staged.
    pub(super) fn stage_glow_sprites(&self) -> Option<(wgpu::Buffer, u32)> {
        if !self.dyn_lighting.get() {
            return None;
        }
        let vp = self.scene_view_proj.get()?;
        let sprites = self.glow_sprites.borrow();
        if sprites.is_empty() {
            return None;
        }
        // Camera basis in world space, recovered from the VP itself so every
        // reflection the field frame folds into it is honoured.
        let inv = vp.inverse();
        let at = |x: f32, y: f32| inv.project_point3(glam::Vec3::new(x, y, 0.5));
        let o = at(0.0, 0.0);
        let right = (at(0.1, 0.0) - o).normalize_or_zero();
        let up = (at(0.0, 0.1) - o).normalize_or_zero();
        let verts = legaia_render_kernels::scene_lighting::glow_vertices(
            &sprites,
            right.to_array(),
            up.to_array(),
        );
        if verts.is_empty() {
            return None;
        }
        let mut bytes: Vec<f32> = Vec::with_capacity(verts.len() * 9);
        for v in &verts {
            bytes.extend_from_slice(&v.pos);
            bytes.extend_from_slice(&v.uv);
            bytes.extend_from_slice(&v.color);
        }
        self.queue.write_buffer(
            &self.glow_uniforms_buf,
            0,
            bytemuck::cast_slice(&[reverse_z(vp).to_cols_array_2d()]),
        );
        let buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("glow sprites"),
                contents: bytemuck::cast_slice(&bytes),
                usage: wgpu::BufferUsages::VERTEX,
            });
        Some((buf, verts.len() as u32))
    }

    /// True when the per-scene point-light layer should shade this frame:
    /// dynamic lighting on, lights staged, and a scene camera registered.
    /// The shadow sub-toggle is NOT part of this gate: with it off the lamps
    /// still light the scene, they just cast no shadow (each light's `color.w = 0`, no
    /// shadow pass) - the same meaning as the play page's "Lamp shadows" box.
    pub(in crate::renderer) fn scene_lights_active(&self) -> bool {
        self.dyn_lighting.get()
            && self.scene_view_proj.get().is_some()
            && !self.scene_lights.borrow().is_empty()
    }

    /// Stage the scene-lights uniform for the current frame and, when the
    /// layer is live, render one shadow-map layer per light over the
    /// scene's geometry (its own encoder, submitted ahead of the frame
    /// encoder). With the layer inert this writes a zero light count and
    /// encodes nothing - the faithful path's whole cost.
    pub(super) fn stage_scene_lights_and_shadows(&self, scene: &Scene<'_>) {
        use crate::scene_lights::light_view_proj;
        use legaia_render_kernels::scene_lighting::MAX_SCENE_LIGHTS;
        let active = self.scene_lights_active();
        let lights = self.scene_lights.borrow();
        let n = if active {
            lights.len().min(MAX_SCENE_LIGHTS)
        } else {
            0
        };
        let mut u = SceneLightsUniform::zeroed();
        // The enhanced-lighting mood's ambient floor + emissive gain - read
        // by `dyn_light` only while the enhancement is on - and its window
        // glow (`params.w`, read by `dyn_window` under the same gate).
        let mood = self.lighting_mood.get();
        u.params = [
            n as f32,
            1.0 / SHADOW_MAP_DIM as f32,
            SHADOW_COMPARE_BIAS,
            mood.window_word()[0],
        ];
        u.ambient = mood.uniforms(true)[2];
        u.prim_near = self.prim_near.get();
        // Camera-occlusion fade focus: project the host-staged player clip
        // position to framebuffer pixels + view depth, carrying the host's
        // eased fade strength in `.w`. The zeroed default (strength 0) is
        // the identity, so the faithful path pays one uniform read.
        // `w <= 0` = the player is behind the camera plane - no meaningful
        // projection, leave the fade inert for the frame.
        if self.occl_fade.get()
            && let Some(f) = self.occl_focus.get()
            && f.clip[3] > 1e-3
            && f.strength > 0.004
        {
            use crate::occlusion_fade as of;
            let c = f.clip;
            let vw = self.config.width as f32;
            let vh = self.config.height as f32;
            let px = (c[0] / c[3] * 0.5 + 0.5) * vw;
            // WebGPU NDC y is up, framebuffer y is down.
            let py = (0.5 - c[1] / c[3] * 0.5) * vh;
            u.occl_focus = [px, py, c[3], f.strength];
            // Radius is authored in world units and projected here, at the
            // focus depth, so the hole tracks the character through zoom.
            let radius = of::radius_px(c[3], f.proj_scale_y, vh);
            u.occl_params = [
                radius,
                of::OCCL_MIN_KEEP,
                of::OCCL_DEPTH_MARGIN,
                of::feather_px(radius),
            ];
            // Feet-line rule: the feet pixel + the feet -> centre lift axis.
            // Feet behind the lens leave the rule off (zero axis).
            let fc = f.feet_clip;
            if fc[3] > 1e-3 {
                let feet = [
                    (fc[0] / fc[3] * 0.5 + 0.5) * vw,
                    (0.5 - fc[1] / fc[3] * 0.5) * vh,
                ];
                let axis = of::lift_axis(feet, [px, py]);
                u.occl_lift = [feet[0], feet[1], axis[0], axis[1]];
            }
        }
        let light_vps: Vec<Mat4> = lights[..n].iter().map(light_view_proj).collect();
        for (i, l) in lights[..n].iter().enumerate() {
            u.lights[i] = ScenePointLightUniform {
                pos_radius: [l.pos[0], l.pos[1], l.pos[2], l.radius],
                // The mood scales every lamp (stronger at night than noon).
                color: [
                    l.color[0] * mood.point_scale,
                    l.color[1] * mood.point_scale,
                    l.color[2] * mood.point_scale,
                    // Shadows on (1) / off (0): the `Y` toggle. Off, the
                    // lamp still shades, unshadowed.
                    if self.dyn_shadows.get() { 1.0 } else { 0.0 },
                ],
                viewproj: light_vps[i].to_cols_array_2d(),
            };
        }
        self.queue
            .write_buffer(&self.scene_lights_buf, 0, bytemuck::bytes_of(&u));
        // Shadows off: the lamps shade unshadowed (`scene_light_shadow`
        // answers 1.0 on a light's `color.w == 0`), so no shadow map is rendered.
        if n == 0 || !self.dyn_shadows.get() {
            return;
        }
        // Per-(light, draw) light-space MVPs. `model = view_proj^-1 * mvp`
        // (same recovery as the main pass), so
        // `light_mvp = light_vp * model`.
        let Some(inv_vp) = self.scene_view_proj.get().map(|vp| vp.inverse()) else {
            return;
        };
        let stride = self.uniform_offset_alignment as usize;
        let n_draws = scene.draws.len() + scene.color_draws.len();
        if n_draws == 0 {
            return;
        }
        let needed = n * n_draws;
        let mut cap = self.shadow_uniforms_capacity.get();
        if cap < needed {
            cap = needed.next_power_of_two().max(needed);
            let new_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("shadow uniforms (resized)"),
                size: (cap * stride) as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let new_bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("shadow uniforms bg (resized)"),
                layout: &self.shadow_uniforms_bgl,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &new_buf,
                        offset: 0,
                        size: std::num::NonZeroU64::new(
                            std::mem::size_of::<ShadowUniforms>() as u64
                        ),
                    }),
                }],
            });
            *self.shadow_uniforms_buf.borrow_mut() = new_buf;
            *self.shadow_uniforms_bg.borrow_mut() = new_bg;
            self.shadow_uniforms_capacity.set(cap);
        }
        let mut bytes = vec![0u8; needed * stride];
        for (li, lvp) in light_vps.iter().enumerate() {
            for (di, mvp) in scene
                .draws
                .iter()
                .map(|d| d.mvp)
                .chain(scene.color_draws.iter().map(|d| d.mvp))
                .enumerate()
            {
                let smvp = *lvp * inv_vp * mvp;
                let su = ShadowUniforms {
                    mvp: smvp.to_cols_array_2d(),
                };
                let off = (li * n_draws + di) * stride;
                bytes[off..off + std::mem::size_of::<ShadowUniforms>()]
                    .copy_from_slice(bytemuck::bytes_of(&su));
            }
        }
        {
            let buf = self.shadow_uniforms_buf.borrow();
            self.queue.write_buffer(&buf, 0, &bytes);
        }
        // Depth-only passes, one per light layer. Full opaque index range
        // of every draw; the light's near plane keeps the emitting flame
        // quads themselves from occluding (see `scene_lights`).
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("shadow encoder"),
            });
        let bg_borrow = self.shadow_uniforms_bg.borrow();
        for li in 0..n {
            let mut rp = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("scene light shadow pass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.shadow_layer_views[li],
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            rp.set_pipeline(&self.shadow_vram_pipeline);
            for (di, draw) in scene.draws.iter().enumerate() {
                let off = ((li * n_draws + di) * stride) as u32;
                rp.set_bind_group(0, &*bg_borrow, &[off]);
                rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                rp.set_index_buffer(draw.mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
            }
            rp.set_pipeline(&self.shadow_color_pipeline);
            for (ci, draw) in scene.color_draws.iter().enumerate() {
                let di = scene.draws.len() + ci;
                let off = ((li * n_draws + di) * stride) as u32;
                rp.set_bind_group(0, &*bg_borrow, &[off]);
                rp.set_vertex_buffer(0, draw.mesh.vertex_buf.slice(..));
                rp.set_index_buffer(draw.mesh.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                rp.draw_indexed(0..draw.mesh.index_count, 0, 0..1);
            }
            drop(rp);
        }
        drop(bg_borrow);
        self.queue.submit(std::iter::once(enc.finish()));
        crate::profile::mark("shadow");
    }

    /// Resize the scene-uniforms buffer (and its bind group) to hold at
    /// least `slots` `MeshUniforms` entries, then write each entry.
    pub(super) fn stage_scene_uniforms(&self, scene: &Scene<'_>) {
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
        let tex_window_packed = pack_tex_window(self.tex_window.get());
        let (clips_textured, clips_color) = self.draw_clips.borrow().clone();
        let nclip_textured = self.draw_nclip.borrow().clone();
        let grade = self.color_grade.get();
        // flags[2] is set per draw below: 1.0 = environment geometry the
        // occlusion fade may dissolve, 0.0 = actor draws (player / NPCs)
        // that keep every fragment (see `set_occlusion_env_draws`).
        let flags_base = [
            self.backface_cull.get(),
            if self.semi_blend.get() { 1.0 } else { 0.0 },
            0.0,
            self.overworld_curve.get(),
        ];
        let curve_bias = self.overworld_curve_bias.get();
        let (occl_env_textured, occl_env_color) = self.occl_env_counts.get();
        let light_dir = self.dyn_light_dir_uniform();
        let light_color = self.dyn_light_color_uniform();
        let cue_ramp = self.cue_ramp.get();
        let palette = self.palette_grade.get();
        // Per-draw model matrix for the point-light layer: recovered from
        // the draw's MVP through the registered scene camera
        // (`model = view_proj^-1 * mvp`). Identity when no camera is
        // registered or the layer is inert - the shader then shades the
        // lights in mesh space, but with a zero light count it never reads
        // the matrix at all.
        let inv_vp = if self.scene_lights_active() {
            self.scene_view_proj.get().map(|vp| vp.inverse())
        } else {
            None
        };
        let push = |bytes: &mut [u8],
                    slot: usize,
                    mvp: Mat4,
                    cue: Option<crate::DrawCue>,
                    occl_allowed: bool,
                    clip: Option<DrawClip>,
                    nclip: Option<u32>| {
            let [c0, c1, c2] = pack_draw_clip(clip);
            let tex_window = [tex_window_packed, c0, c1, c2];
            let mut flags = flags_base;
            flags[2] = if occl_allowed { 1.0 } else { 0.0 };
            if let Some(mode) = nclip {
                flags[0] = mode.min(2) as f32;
            }
            let model = match inv_vp {
                Some(inv) => model_rows(&(inv * mvp)),
                None => MODEL_ROWS_IDENTITY,
            };
            // A per-draw cue overrides both the far colour and the
            // view-depth ramp for this slot only (retail stages the DPCS
            // far colour + IR0 per drawn object, so one draw fogging says
            // nothing about the others - the battle ground grid's SZ >> 2
            // cue is the worked case). `max_ir0` is deliberately NOT
            // clamped to 1: retail's manual `mtc2` IR0 load never
            // saturates, the DPCS output clamp does (see `psx_depth_cue`).
            let (depth_cue, cue_ramp) = match cue {
                Some(c) => {
                    let inv_range = if c.far_z > c.near_z {
                        1.0 / (c.far_z - c.near_z)
                    } else {
                        0.0
                    };
                    let global = self.depth_cue.get();
                    (
                        [
                            c.far[0].clamp(0.0, 1.0),
                            c.far[1].clamp(0.0, 1.0),
                            c.far[2].clamp(0.0, 1.0),
                            global[3],
                        ],
                        [c.near_z, inv_range, c.max_ir0.max(0.0), 1.0],
                    )
                }
                None => (self.depth_cue.get(), cue_ramp),
            };
            let u = MeshUniforms {
                // Reversed-Z applied centrally (see `reverse_z`).
                // The model rows above were recovered from the unbiased
                // matrix; only the staged clip matrix carries the offset.
                mvp: reverse_z(fold_screen_rows(mvp, curve_bias)).to_cols_array_2d(),
                depth_cue,
                psx_params,
                tex_window,
                grade,
                flags,
                light_dir,
                light_color,
                cue_ramp,
                palette,
                model_rows: model,
            };
            let off = slot * stride;
            let n = std::mem::size_of::<MeshUniforms>();
            bytes[off..off + n].copy_from_slice(bytemuck::bytes_of(&u));
        };
        for (i, draw) in scene.draws.iter().enumerate() {
            let clip = clips_textured.get(i).copied().flatten();
            push(
                &mut bytes,
                i,
                draw.mvp,
                draw.cue,
                i < occl_env_textured,
                clip,
                nclip_textured.get(i).copied().flatten(),
            );
        }
        if let Some((_, mvp)) = scene.overlay_lines {
            push(&mut bytes, scene.draws.len(), mvp, None, false, None, None);
        }
        // Colour-mesh slots follow the draws + the optional overlay-lines slot.
        let color_base = scene.draws.len() + scene.overlay_lines.is_some() as usize;
        for (i, draw) in scene.color_draws.iter().enumerate() {
            push(
                &mut bytes,
                color_base + i,
                draw.mvp,
                draw.cue,
                i < occl_env_color,
                clips_color.get(i).copied().flatten(),
                None,
            );
        }
        let buf_borrow = self.scene_uniforms_buf.borrow();
        let buf: &wgpu::Buffer = &buf_borrow;
        self.queue.write_buffer(buf, 0, &bytes);
    }
}
