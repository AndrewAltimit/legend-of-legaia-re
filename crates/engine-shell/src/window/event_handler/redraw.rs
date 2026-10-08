//! `RedrawRequested` window-event handler (per-frame tick + render),
//! extracted from `event_handler.rs` (mechanical split; behavior-preserving).

use super::super::*;
use super::redraw_draws::{DrawCtx, DrawLists, MeshStore};
use super::redraw_overlay::OverlayDraws;
use super::redraw_prep::FramePrep;
use super::redraw_present;
use super::redraw_stage::FrameView;

/// What the frame presents: the scene alone, or the scene with the
/// field-to-battle transition's screen primitives composited over it.
///
/// A function rather than an inline expression because the screenshot
/// harness has to capture the *same* target it presents - capturing a bare
/// `Scene` there drops every transition style from the PNGs.
pub(super) fn present_target<'a>(
    scene: &'a RenderScene<'a>,
    prims: &'a [legaia_engine_render::screen_overlay::ScreenPrim],
    under_overlay: &'a [legaia_engine_render::screen_overlay::ScreenPrim],
) -> RenderTarget<'a> {
    if prims.is_empty() && under_overlay.is_empty() {
        RenderTarget::Scene(scene)
    } else {
        RenderTarget::SceneWithScreenPrims {
            scene,
            prims,
            under_overlay,
        }
    }
}

impl PlayWindowApp {
    /// Whether a `LEGAIA_CAPTURE_GATE` capture's phase holds this frame.
    pub(super) fn capture_phase_met(&self) -> bool {
        let world = &self.session.host.world;
        self.screenshot.as_ref().is_some_and(|sc| {
            sc.phase_gate.as_ref().is_some_and(|g| g.met(world))
                || sc.script_gate.as_ref().is_some_and(|g| g.met(world))
                || sc.battle_drive.as_ref().is_some_and(|d| {
                    d.reached(world) && sc.battle_drive_held.get() >= d.hold_ticks()
                })
        })
    }

    pub(super) fn handle_redraw(&mut self) {
        // Opt-in frame profiler (`LEGAIA_PROFILE=1`; see
        // `legaia_engine_render::profile`). Free when off - each call is a
        // cached-bool branch. The stage marks below carve the frame into
        // tick / pose / drawlist / acquire / uniforms / encode / submit /
        // present.
        legaia_engine_render::profile::begin_frame();
        let field_tail_ticks = self.run_frame_ticks();
        self.seat_capture_frame();
        legaia_engine_render::profile::mark("tick");
        self.apply_floor_wave();
        let view_cells = self.sync_field_view();
        if self.service_windowed_cutscene() {
            return;
        }
        self.sync_battle_frame();
        let cutscene_cam = self.resolve_cutscene_camera();
        let FramePrep {
            field_morph_rebuilds,
            npc_morph_rebuilds,
            mut battle_intro,
            battle_intro_prims,
            field_fog_prims,
            move_strip_prims,
            fishing_line_prims,
            slot_prims,
        } = self.take_frame_prep();
        if self.win.renderer.is_some() && self.uploaded_vram.is_some() && self.font_atlas.is_some()
        {
            let FrameView {
                w,
                h,
                cam,
                in_world_map,
            } = self.stage_frame_render_state(cutscene_cam);
            self.drain_frame_mesh_uploads();
            let posed =
                self.pose_frame_meshes(field_morph_rebuilds, npc_morph_rebuilds, field_tail_ticks);
            let (Some(r), Some(vram), Some(atlas)) = (
                self.win.renderer.as_ref(),
                self.uploaded_vram.as_ref(),
                self.font_atlas.as_ref(),
            ) else {
                unreachable!("render gate checked above");
            };
            // A live Baka duel draws against its own VRAM.
            // So does the dance venue - the hall's own upload, never the
            // walked-in scene's.
            let duel_gpu = self
                .baka_gpu
                .as_ref()
                .or(self.muscle_gpu.as_ref())
                .or(self.fishing_gpu.as_ref());
            // And the slot machine: every quad it draws samples its art pack.
            let vram = match (
                duel_gpu,
                self.dance_venue_gpu.as_ref(),
                self.slot_gpu.as_ref(),
            ) {
                (Some(g), _, _) => &g.vram,
                (None, Some(d), _) => &d.vram,
                (None, None, Some(v)) => v,
                (None, None, None) => vram,
            };
            // Re-borrow the memo immutably: `slot -> this frame's posed halves`.
            let npc_posed: std::collections::HashMap<u8, &NpcPosedHalves> = posed
                .npc_frames
                .iter()
                .filter_map(|k| self.npc_pose_cache.get(k).map(|m| (k.0, m)))
                .collect();
            // Everything above this mark is per-frame skinning: CPU mesh
            // re-pose + GPU re-upload for the player, the animated props and
            // every placed NPC.
            legaia_engine_render::profile::mark("pose");
            // Iterate every actor that has a `tmd_binding`. Scene-init
            // actors (slots 0..N from `init_scene_animations`) have
            // their bindings set but aren't necessarily `.active` -
            // the original draws iteration walked meshes directly,
            // so we preserve that behaviour by not gating on
            // `.active` here. Dynamically spawned actors set both
            // `.active` and a binding to their freshly uploaded
            // mesh slot (beyond `scene_tmd_data.len()`) via the
            // spawn pass above.
            //
            // Suppress 3D draws while the boot UI is active so the
            // last-loaded scene (e.g. a town) doesn't show through
            // behind publisher logos / title / save-select. The one
            // exception is the party-wipe hand-off: retail holds the
            // final battle frame while mode 22 CARD INIT streams the
            // menu overlay, so the GameOver hold keeps drawing the
            // (frozen, untick'd) battle scene underneath.
            let game_over_hold = matches!(self.boot_ui, BootUiState::GameOver(_));
            let draw_cx = DrawCtx {
                app: &*self,
                store: MeshStore {
                    meshes: &self.meshes,
                    color_meshes: &self.color_meshes,
                    field_lit: &self.field_lit,
                    field_morph_live: &self.field_morph_live,
                    ground_heightfield: &self.ground_heightfield,
                    ground_crop: &self.ground_crop,
                    baka_gpu: &self.baka_gpu,
                    muscle_gpu: &self.muscle_gpu,
                    fishing_gpu: &self.fishing_gpu,
                    dance_venue_gpu: &self.dance_venue_gpu,
                    dance_cast_gpu: &self.dance_cast_gpu,
                    npc_morph_static: &self.npc_morph_static,
                    npc_posed: &npc_posed,
                    posed: &posed,
                },
                r,
                cam,
                cutscene_cam,
                view_cells: &view_cells,
                in_world_map,
                game_over_hold,
            };
            let DrawLists {
                mut draws,
                color_draws,
                clip_marks,
                nclip_marks,
                color_clip_marks,
            } = draw_cx.build_scene_draws();
            let OverlayDraws {
                hud,
                logo_draw_vec,
                title_draw_vec,
                menu_glyph_draw_vec,
                muscle_hub_draw_vec,
                muscle_hub_blend,
                save_chrome_draw_vec,
                save_chrome_blend,
                caption_draw_vec,
            } = self.build_overlay_draws(w, h, cam);
            let overlay = TextOverlay {
                atlas,
                draws: &hud,
                blend: &[],
            };
            let logo_overlay = self.publisher_logos.as_ref().map(|p| TextOverlay {
                atlas: &p.atlas,
                draws: &logo_draw_vec,
                blend: &[],
            });
            let title_overlay = self.title_screen.as_ref().map(|t| TextOverlay {
                atlas: &t.atlas,
                draws: &title_draw_vec,
                blend: &[],
            });
            let menu_glyph_overlay = self.menu_glyphs.as_ref().map(|m| TextOverlay {
                atlas: &m.atlas,
                draws: &menu_glyph_draw_vec,
                blend: &[],
            });
            let save_chrome_overlay = self.save_menu.as_ref().map(|sm| TextOverlay {
                atlas: &sm.atlas,
                draws: &save_chrome_draw_vec,
                blend: &save_chrome_blend,
            });
            let muscle_hub_overlay = self.muscle_hub.as_ref().map(|m| TextOverlay {
                atlas: &m.atlas,
                draws: &muscle_hub_draw_vec,
                blend: &muscle_hub_blend,
            });
            let caption_overlay = self
                .caption_atlas
                .as_ref()
                .map(|(atlas, _, _)| TextOverlay {
                    atlas,
                    draws: &caption_draw_vec,
                    blend: &[],
                });
            let scene_clear = self.frame_clear_color(game_over_hold);

            // Slot 1: logos OR title-art bands (title still
            // emits during SaveSelect, dimmed). Slot 2: either
            // the save-menu chrome (panel + slot pills) when
            // SaveSelect is active, or the menu-glyph atlas
            // (deprecated no-disc title-menu fallback) otherwise.
            // The opdeene caption takes slot 1 when active: during the opening
            // cutscene the boot-UI logo / title overlays are inactive (their
            // draw vecs empty), so there is no contention.
            let sprites_slot_1 = if !caption_draw_vec.is_empty() {
                caption_overlay.as_ref()
            } else if !logo_draw_vec.is_empty() {
                logo_overlay.as_ref()
            } else if !title_draw_vec.is_empty() {
                title_overlay.as_ref()
            } else if !muscle_hub_draw_vec.is_empty() {
                muscle_hub_overlay.as_ref()
            } else {
                None
            };
            let sprites_slot_2 = if !save_chrome_draw_vec.is_empty() {
                save_chrome_overlay.as_ref()
            } else if !menu_glyph_draw_vec.is_empty() {
                menu_glyph_overlay.as_ref()
            } else {
                None
            };
            let fx = self.build_fx_frame(r, cam, in_world_map, cutscene_cam);
            let fx_cam = fx.fx_cam;
            fx.push_draws(&mut draws);
            // The floating value readout is a screen-space primitive run, not
            // a scene mesh: see `screen_prims` below, where both hosts build
            // it from `legaia_engine_ui::battle_numerals`.
            // The scripted screen fade (op 0x4C 0x12) is NOT drawn as a wash
            // mesh here: it is a multiply tint staged into the colour grade +
            // depth-cue far colour (see the grade staging above), matching
            // the retail mechanism - the 3D scene darkens while the narration
            // overlay keeps scrolling bright.
            legaia_engine_render::profile::draw_counts(draws.len(), color_draws.len());
            self.write_draw_census(&draws);
            redraw_present::stage_draw_marks(
                r,
                draws.len(),
                color_draws.len(),
                clip_marks,
                color_clip_marks,
                nclip_marks,
            );
            let scene = RenderScene {
                vram,
                draws: &draws,
                color_draws: &color_draws,
                // Effect outlines share the billboards' `fx_cam` (the two
                // sources are mutually exclusive, and off the battle stage
                // `fx_cam == cam`, so the slot-4 inspection lines are unaffected).
                overlay_lines: fx
                    .world_map_slot4_lines
                    .as_ref()
                    .or(fx.effect_lines.as_ref())
                    .map(|m| (m, fx_cam)),
                overlay_sprites: sprites_slot_1,
                overlay_sprites_2: sprites_slot_2,
                overlay_text: Some(&overlay),
                clear_color: scene_clear,
            };
            legaia_engine_render::profile::mark("drawlist");
            // On the frame the transition arms, land the field frame in the
            // software VRAM the intro strips texture themselves with, and draw
            // the rest of this frame against that page. Retail gets it for
            // free - on the console the framebuffer *is* VRAM - so the port
            // re-renders this scene offscreen and blits the readback in.
            if let Some(v) = Self::capture_battle_intro_frame(
                battle_intro.as_mut(),
                r,
                &scene,
                self.cpu_vram_base.as_ref(),
            ) {
                // Keep the captured page GPU-resident for the whole
                // transition: the capture is a one-shot, but every
                // transition frame's primitives sample it (the curtain
                // strips, and the tile shatter's pages + shade page).
                self.battle_intro_vram = Some(v);
            }
            let scene = match (battle_intro.as_ref(), self.battle_intro_vram.as_ref()) {
                (Some(_), Some(v)) => RenderScene { vram: v, ..scene },
                _ => scene,
            };
            // A field-VM `43 12` copy that reads the display framebuffer
            // (the ending vignettes' photo grab into `(512, 0)`) waits for
            // this frame: land it in the CPU VRAM's display page, and the
            // next drain (`apply_world_clut_fx`) runs the copy. Shared
            // handshake `World::framebuffer_grab_pending`; the browser page
            // lands its own frame through `play_land_frame_grab`.
            if self.session.host.world.framebuffer_grab_pending()
                && let Some(base) = self.cpu_vram_base.as_mut()
            {
                match r.capture_scene_rgba(legaia_engine_render::RenderTarget::Scene(&scene)) {
                    Ok(img) => {
                        legaia_engine_render::vram_capture::land_display_frame(
                            &img.rgba, img.width, img.height, base,
                        );
                        self.session.host.world.land_framebuffer();
                    }
                    Err(e) => log::error!("play-window: framebuffer grab: {e:#}"),
                }
            }
            let (screen_prims, light_prims) = self.build_screen_prims(
                battle_intro_prims,
                field_fog_prims,
                move_strip_prims,
                slot_prims,
                fishing_line_prims,
                fx_cam,
            );
            let target = |scene| present_target(scene, &screen_prims, &light_prims);
            redraw_present::capture_sweep_frame(
                r,
                target(&scene),
                self.screenshot.as_ref(),
                self.tick_no,
                &mut self.sweep_next_tick,
            );
            self.present_frame(r, &scene, &screen_prims, &light_prims);
        }
        // The transition emitter was taken out of `self` for the render
        // borrow; put it back so its working set survives to the next frame.
        self.battle_intro = battle_intro;
        legaia_engine_render::profile::end_frame();
        self.win.request_redraw();
    }
}

impl PlayWindowApp {
    /// One sim tick of ANIMATE-cue handling through the shared kernel
    /// (`World::drain_field_anim_cues`): the player's scripted gestures land
    /// on the world's own clip player, and each NPC re-target swaps this
    /// window's clip player for the slot.
    ///
    /// The incoming clip restarts at frame 0 and reuses the same low frame
    /// indices, so the outgoing clip's pose-cache entries for the slot would
    /// alias it; they are dropped.
    pub(super) fn drain_anim_cues(&mut self) {
        let srcs = &self.npc_anim_srcs;
        let special = &self.npc_bundle_special;
        let retargets = self.session.host.world.drain_field_anim_cues(
            self.npc_anim_bundles.0.as_ref(),
            self.npc_anim_bundles.1.as_ref(),
            |slot| {
                srcs.contains_key(&slot)
                    .then(|| special.get(&slot).copied().unwrap_or(false))
            },
        );
        for r in retargets {
            self.npc_clip_players.insert(r.slot, r.player);
            self.npc_pose_cache.retain(|(s, _), _| *s != r.slot);
            self.npc_pose_verify.retain(|(s, _), _| *s != r.slot);
        }
    }

    /// Screen-space stage position (retail 320x240) an actor's origin
    /// projects to under `cam`, or `None` when it is behind the camera.
    pub(super) fn actor_stage_point(&self, slot: usize, cam: Mat4) -> Option<(i32, i32)> {
        let a = self.session.host.world.actors.get(slot)?;
        let w = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        let clip = cam * w.extend(1.0);
        if clip.w <= 0.01 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some((
            ((ndc.x * 0.5 + 0.5) * 320.0) as i32,
            ((0.5 - ndc.y * 0.5) * 240.0) as i32,
        ))
    }

    /// `LEGAIA_DIAG_FX=1` instrument: report where each live effect billboard
    /// actually lands, in the frame's own FX camera.
    ///
    /// "The spawn fires and nothing appears" has three distinguishable causes
    /// and this separates them in one line per sprite: a clip `w <= 0` (the
    /// quad is behind the eye), NDC outside `[-1, 1]` (projected off-screen),
    /// or an in-frame NDC with texel coordinates that name a page nothing
    /// uploaded (the quad draws, but every texel discards). Off by default -
    /// the log is per-frame per-sprite.
    pub(super) fn diag_effect_billboards(&self, cam: Mat4) {
        if std::env::var_os("LEGAIA_DIAG_FX").is_none() {
            return;
        }
        let sprites = self.session.host.world.active_effect_sprites();
        if sprites.is_empty() {
            return;
        }
        for slot in 0..4usize {
            if let Some(a) = self.session.host.world.actors.get(slot) {
                log::info!(
                    "DIAG fx actor {slot}: world ({},{},{}) screen {:?}",
                    a.move_state.world_x,
                    a.move_state.world_y,
                    a.move_state.world_z,
                    self.actor_stage_point(slot, cam)
                );
            }
        }
        let battle = self.session.host.world.mode == SceneMode::Battle;
        for s in sprites.iter().take(4) {
            // The same centre the billboard builder draws around.
            let c = if battle {
                legaia_engine_render::effect_billboard::battle_billboard_centre(s.world_pos)
            } else {
                s.world_pos
            };
            let p = cam * glam::Vec4::new(c[0], c[1], c[2], 1.0);
            let ndc = if p.w.abs() > 1e-6 {
                [p.x / p.w, p.y / p.w, p.z / p.w]
            } else {
                [f32::NAN; 3]
            };
            let u0 = s.uv[0] as u8;
            let v0 = s.uv[1] as u8;
            let u1 = u0.saturating_add(s.uv_size[0].saturating_sub(1) as u8);
            let v1 = v0.saturating_add(s.uv_size[1].saturating_sub(1) as u8);
            let texels = self
                .battle_vram
                .as_ref()
                .or(self.cpu_vram_base.as_ref())
                .map(|v| {
                    v.prim_texture_status(s.clut, s.page, &[(u0, v0), (u1, v0), (u0, v1), (u1, v1)])
                });
            // Resolve the sprite's own texel rect through its CLUT, exactly
            // as the fragment shader would: 4bpp indices out of the texture
            // page, index 0 discarded, everything else a BGR555 word.
            let mut opaque = 0usize;
            let mut total = 0usize;
            let mut sample = 0u16;
            if let Some(v) = self.battle_vram.as_ref().or(self.cpu_vram_base.as_ref()) {
                let px = ((s.page & 0xF) * 64) as usize;
                let py = (((s.page >> 4) & 1) * 256) as usize;
                let cx = ((s.clut & 0x3F) * 16) as usize;
                let cy = ((s.clut >> 6) & 0x1FF) as usize;
                for dv in 0..s.uv_size[1] as usize {
                    for du in 0..s.uv_size[0] as usize {
                        let u = u0 as usize + du;
                        let word = v.pixel(px + (u >> 2), py + v0 as usize + dv);
                        let idx = ((word >> (4 * (u & 3))) & 0xF) as usize;
                        total += 1;
                        if idx != 0 {
                            opaque += 1;
                            if sample == 0 {
                                sample = v.pixel(cx + idx, cy);
                            }
                        }
                    }
                }
            }
            log::info!(
                "DIAG fx: n={} pos {:?} size {:?} page {:#04x} clut {:#06x} uv {:?}+{:?} \
                 bright {:#04x} clip.w {:.1} ndc [{:.3} {:.3} {:.3}] texels {:?} \
                 opaque {opaque}/{total} sample {sample:#06x}",
                sprites.len(),
                s.world_pos,
                s.size,
                s.page,
                s.clut,
                s.uv,
                s.uv_size,
                s.brightness,
                p.w,
                ndc[0],
                ndc[1],
                ndc[2],
                texels,
            );
        }
    }

    /// The dance count-in banner as screen-space PSX primitives.
    ///
    /// Empty outside the count-in and empty while the hall's HUD page is not
    /// resident - in which case `hud.rs` draws the placeholder letterforms
    /// instead. The art comes off the run's own widget table when it has one,
    /// so the cell, page and palette are the disc's rather than this host's.
    pub(super) fn dance_countin_prims(
        &self,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_render::ui_dance as ud;
        let mg = &self.session.host.world.minigames;
        if !mg.dance_hud_art_staged {
            return Vec::new();
        }
        // `GO!` after READY (`FUN_801cf470` states 4 / 5): widget `0x0C`
        // off the run's own table, through the one shared emitter.
        if let Some(go) = mg.dance_countin_go {
            return mg
                .dance
                .as_ref()
                .and_then(|g| g.widget(ud::COUNTIN_GO_WIDGET))
                .map(|(w, abr)| {
                    ud::dance_go_prims(
                        go,
                        ud::DanceCountInArt::from_widget(&w, abr),
                        ud::COUNTIN_OT,
                    )
                })
                .unwrap_or_default();
        }
        let Some(env) = mg.dance_countin_banner.as_ref() else {
            return Vec::new();
        };
        let art = mg
            .dance
            .as_ref()
            .and_then(|g| g.widget(0))
            .map(|(w, abr)| ud::DanceCountInArt::from_widget(&w, abr))
            .unwrap_or_default();
        ud::dance_countin_prims(
            ud::DanceCountInView {
                x_offset: env.x_offset,
                brightness: env.brightness,
                hold: env.hold,
            },
            art,
            ud::COUNTIN_OT,
        )
    }

    /// The dance HUD's textured quads as screen-space PSX primitives, off the
    /// world's one predicate (`MinigameState::dance_hud_quads`: HUD up and
    /// page resident). Empty otherwise, when `hud.rs` draws the text rows.
    pub(super) fn dance_hud_prims(&self) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_render::ui_dance as ud;
        let views: Vec<ud::DanceHudQuadView> = self
            .session
            .host
            .world
            .minigames
            .dance_hud_quads()
            .iter()
            .map(|q| ud::DanceHudQuadView {
                poly_code: q.poly_code,
                rect: (q.x0, q.y0, q.x1, q.y1),
                uv: q.uv,
                rgb_top: q.rgb_top,
                rgb_bottom: q.rgb_bottom,
                clut: q.clut,
                tpage: q.tpage_attr,
            })
            .collect();
        ud::dance_hud_prims(&views, ud::COUNTIN_OT)
    }

    /// The frame's floating value readout, as screen-space PSX primitives in
    /// stage coordinates.
    ///
    /// One run of digit cells per live popup, seated over the struck actor
    /// (`actor_stage_point`) and laid out by
    /// `legaia_engine_vm::battle_value_readout::value_cells`, plus the combo
    /// counter cluster. The quads themselves come from the shared
    /// `legaia_engine_ui::battle_numerals` builder the browser play page also
    /// emits through - this host only supplies the camera-dependent seat.
    ///
    /// Empty outside battle, with no popups, or before the battle VRAM (which
    /// is what makes the effect atlas's digit page resident) has been
    /// uploaded.
    pub(super) fn battle_value_readout_prims(
        &self,
        cam: Mat4,
    ) -> Vec<legaia_engine_render::screen_overlay::ScreenPrim> {
        use legaia_engine_render::battle_numerals as bn;
        // The retail-art half: only while the sheet the quads sample is
        // resident. Before that the frame takes the font fallback instead
        // ([`Self::battle_value_readout_draws`]); the two are mutually
        // exclusive on this host exactly as they are on the play page,
        // because a frame that ran both would print every number twice.
        if self.battle_vram.is_none() {
            return Vec::new();
        }
        let Some((cluster, runs)) = self.battle_value_readout_layout(cam) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if let Some(c) = cluster.as_ref() {
            out.extend(bn::combo_cluster_prims(c, bn::VALUE_READOUT_OT));
        }
        for cells in &runs {
            out.extend(bn::digit_run_prims(cells, bn::VALUE_READOUT_OT));
        }
        out
    }

    /// The battle value readout as **font text**, for the frames before the
    /// battle VRAM exists.
    ///
    /// Empty whenever the retail cells are drawable
    /// ([`Self::battle_value_readout_prims`]) - the two must never both draw,
    /// or every number renders twice. The browser play page's
    /// `battle_value_readout_draws` is the same bargain on the same layout;
    /// this host had the prim half and no fallback, which is a gap only a
    /// side-by-side of the first frames of a fight shows.
    pub(super) fn battle_value_readout_draws(&self, cam: Mat4, w: u32, h: u32) -> Vec<TextDraw> {
        use legaia_engine_render as ui;
        if self.battle_vram.is_some() || w == 0 || h == 0 {
            return Vec::new();
        }
        let Some((cluster, runs)) = self.battle_value_readout_layout(cam) else {
            return Vec::new();
        };
        let (origin, scale) = self.save_select_stage(w, h);
        let view = |k: &legaia_engine_vm::battle_value_readout::ValueCell| ui::ValueCellView {
            digit: k.digit,
            x: k.x,
            y: k.y,
            w: k.w,
            h: k.h,
        };
        let mut out = Vec::new();
        if let Some(c) = cluster.as_ref() {
            let labels: Vec<ui::ComboLabelView<'_>> = c
                .labels
                .iter()
                .map(|l| ui::ComboLabelView {
                    word: l.word,
                    x: l.x,
                    y: l.y,
                })
                .collect();
            let cells: Vec<ui::ValueCellView> = c.cells.iter().map(view).collect();
            out.extend(ui::battle_combo_cluster_draws_for(
                &self.font, &labels, &cells, origin, scale,
            ));
        }
        for run in &runs {
            let cells: Vec<ui::ValueCellView> = run.iter().map(view).collect();
            out.extend(ui::battle_value_readout_draws_for(
                &self.font,
                &cells,
                ui::VALUE_READOUT_FALLBACK_COLOR,
                origin,
                scale,
            ));
        }
        out
    }

    /// The readout's **layout**, shared by the retail-art emit above and the
    /// font fallback above it: the combo cluster and one run of digit
    /// cells per struck actor, seated through this host's camera.
    ///
    /// `None` outside battle and with nothing to say. Splitting it out is
    /// what lets the window fall back the way the browser play page already
    /// did - the page had a font path for the frames before its VRAM existed
    /// and this host drew nothing at all, so the same fight opened with
    /// numbers in the tab and none in the window.
    pub(super) fn battle_value_readout_layout(
        &self,
        cam: Mat4,
    ) -> Option<(
        Option<legaia_engine_vm::battle_value_readout::ComboCluster>,
        Vec<Vec<legaia_engine_vm::battle_value_readout::ValueCell>>,
    )> {
        use legaia_engine_vm::battle_value_readout as vr;
        if self.session.host.world.mode != SceneMode::Battle {
            return None;
        }
        if self.battle_hud.popups.is_empty() && self.battle_hud.combo.is_none() {
            return None;
        }
        // The combo counter cluster: the `HIT` / `TOTAL` / `DAMAGE` word
        // cells and the value digits, off the same sheet, on the seats the
        // steal-banner and tail-fire display lists pin, sliding in with
        // placement record 80's glide (`vr::combo_slide`).
        let cluster = self
            .battle_hud
            .combo
            .as_ref()
            .map(|c| vr::combo_cluster(c.style, c.hits, c.total, c.slide()));
        // One numeral per actor, the newest. Retail's readout is a per-slot
        // **value window** (`_DAT_801F6980`, four halfwords, one per slot), so
        // a second hit on the same actor replaces the figure rather than
        // stacking beside it - and stacking is not cosmetic here: two runs
        // centred on the same point interleave into an unreadable third
        // number (a 9 landing inside a 10 reads as "190").
        let mut newest: Vec<&legaia_engine_core::battle_hud::DamagePopup> = Vec::new();
        for p in &self.battle_hud.popups {
            if p.status.is_some() {
                // Status applications have no numeral on the sheet.
                continue;
            }
            match newest.iter_mut().find(|q| q.slot == p.slot) {
                Some(q) if q.frames_remaining >= p.frames_remaining => {}
                Some(q) => *q = p,
                None => newest.push(p),
            }
        }
        // Seated the way retail's renderer `FUN_801DF6B8` seats them: a
        // view-space square over the struck actor's display trio, rising and
        // growing with the ring timer (`battle_numerals::popup_value_cells`,
        // the kernel the browser play page seats through too).
        let vp = cam.to_cols_array();
        let world = &self.session.host.world;
        let mut runs = Vec::new();
        for p in newest {
            let Some(trio) = world.battle_display_trio(usize::from(p.slot)) else {
                continue;
            };
            let age = p.frames_total.saturating_sub(p.frames_remaining);
            let cells = legaia_engine_render::battle_numerals::popup_value_cells(
                &vp, 1.0, trio, p.amount, age,
            );
            if !cells.is_empty() {
                runs.push(cells);
            }
        }
        Some((cluster, runs))
    }
}

/// Drive an open menu-runtime session (shop, prize exchange, inn) one tick on
/// this tick's pad edges, then unpark the field script a closed shop or
/// counter left suspended. A free function over the two fields it touches, so
/// the frozen-field arm and the frame tail share it.
pub(super) fn tick_menu_runtime_session(
    menu: &mut legaia_engine_core::menu_runtime::MenuRuntime,
    world: &mut legaia_engine_core::world::World,
    pressed_edge: u16,
) -> Option<u8> {
    // The one per-tick step both hosts run (the browser page's
    // `play_shop_input` calls it per sim tick too): edges, not the held word
    // - the runtime filters no repeats, so a held key used to step the shop
    // cursor / commit a screen every tick it stayed down - then the unpark of
    // a closed shop / prize counter's suspended op-0x49.
    menu.step_field_session(world, pressed_edge)
}
