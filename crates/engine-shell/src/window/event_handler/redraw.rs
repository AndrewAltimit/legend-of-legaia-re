//! `RedrawRequested` window-event handler: one redraw is the sim ticks it
//! drains, then one rendered frame. `handle_redraw` is the frame's phase
//! list; each phase lives in a sibling step file:
//!
//! | Phase | File |
//! |---|---|
//! | sim ticks (`run_frame_ticks`, the per-tick body `sim_tick`) | `redraw_tick.rs` |
//! | capture seating, field view, windowed movie, battle edge, cutscene camera, frame prep | `redraw_prep.rs` |
//! | the render block's order and borrows (`render_scene_frame`) | `redraw_render.rs` |
//! | renderer state, mesh-upload drains, per-frame skinning | `redraw_stage.rs` |
//! | the scene draw lists, by mode (`DrawCtx`) | `redraw_draws.rs` |
//! | HUD / sprite overlays, clear colour, effect meshes, screen prims | `redraw_overlay.rs` |
//! | draw census, per-draw marks, screenshot harness, present | `redraw_present.rs` |
//!
//! `scripts/ci/check-ui-host-drift.py` reads the frame as one body: it
//! splices the step files back in at their call sites.

use super::super::*;

impl PlayWindowApp {
    /// Whether a `LEGAIA_CAPTURE_GATE` capture's phase holds this frame.
    pub(super) fn capture_phase_met(&self) -> bool {
        // `LEGAIA_DIAG_CAPTURE_TICK=<tick>` takes the frame at that world
        // tick instead of on the gate, with every seed and drive still
        // running: the frames either side of a gated capture (whose tick the
        // child logs as `capture at tick`) say whether a draw the retail
        // frame shows is missing or only early / late.
        static FORCED: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
        if let Some(t) = *FORCED.get_or_init(|| {
            std::env::var("LEGAIA_DIAG_CAPTURE_TICK")
                .ok()
                .and_then(|v| v.parse().ok())
        }) {
            return self.screenshot.is_some() && self.tick_no >= t;
        }
        let world = &self.session.host.world;
        self.screenshot.as_ref().is_some_and(|sc| {
            sc.phase_gate.as_ref().is_some_and(|g| g.met(world))
                || sc.script_gate.as_ref().is_some_and(|g| g.met(world))
                || sc.battle_drive.as_ref().is_some_and(|d| {
                    d.reached(world) && sc.battle_drive_held.get() >= d.hold_ticks()
                })
        })
    }

    /// One redraw: drain the sim ticks, prepare the frame, render it (or the
    /// windowed movie), and ask for the next redraw.
    pub(super) fn handle_redraw(&mut self) {
        // Opt-in frame profiler (`LEGAIA_PROFILE=1`; see
        // `legaia_engine_render::profile`). Free when off - each call is a
        // cached-bool branch. The stage marks below carve the frame into
        // tick / pose / drawlist / acquire / uniforms / encode / submit /
        // present.
        legaia_engine_render::profile::begin_frame();
        self.run_frame_ticks();
        self.seat_capture_frame();
        self.sync_minigame_caption_title();
        legaia_engine_render::profile::mark("tick");
        self.apply_floor_wave();
        let view_cells = self.sync_field_view();
        if self.service_windowed_cutscene() {
            return;
        }
        self.sync_battle_frame();
        let cutscene_cam = self.resolve_cutscene_camera();
        let mut prep = self.take_frame_prep();
        // The field-to-battle transition emitter leaves `self` for the
        // render borrow (see `take_frame_prep`).
        let mut battle_intro = prep.battle_intro.take();
        if self.win.renderer.is_some() && self.uploaded_vram.is_some() && self.font_atlas.is_some()
        {
            self.render_scene_frame(prep, &mut battle_intro, view_cells, cutscene_cam);
        }
        // The transition emitter was taken out of `self` for the render
        // borrow; put it back so its working set survives to the next frame.
        self.battle_intro = battle_intro;
        legaia_engine_render::profile::end_frame();
        self.win.request_redraw();
    }

    /// The redraw's steps after its ticks that move world state, for the
    /// headless driver (`window::headless`): the battle frame edge, the
    /// cutscene camera glide and the frame prep (the fog pool's render
    /// step), in [`Self::handle_redraw`]'s order. The renderer-bound steps
    /// gate themselves on the renderer being absent.
    pub(in crate::window) fn headless_frame_prep(&mut self) {
        self.apply_floor_wave();
        let _ = self.sync_field_view();
        self.sync_battle_frame();
        let _ = self.resolve_cutscene_camera();
        let mut prep = self.take_frame_prep();
        self.battle_intro = prep.battle_intro.take();
    }
}

impl PlayWindowApp {
    /// One sim tick of ANIMATE-cue handling through the shared kernel
    /// (`World::drain_field_anim_cues`): the player's scripted gestures land
    /// on the world's own clip player, each NPC re-target swaps this
    /// window's clip player for the slot, and every player then takes the
    /// tick's step (`step_npc_clip_players`).
    ///
    /// The incoming clip restarts at frame 0 and reuses the same low frame
    /// indices, so the outgoing clip's pose-cache entries for the slot would
    /// alias it; they are dropped.
    pub(super) fn drain_anim_cues(&mut self) {
        // The clip targets are the scene binding's catalogued placements
        // (`scene_actors::bind_scene_actors`), the set the browser page
        // drains against: which placements this window uploaded a mesh for
        // is a draw fact, not one a cue may branch on.
        let special = &self.npc_bundle_special;
        let retargets = self.session.host.world.drain_field_anim_cues(
            self.npc_anim_bundles.0.as_ref(),
            self.npc_anim_bundles.1.as_ref(),
            |slot| special.get(&slot).copied(),
        );
        for r in retargets {
            self.npc_clip_players.insert(r.slot, r.player);
            self.npc_pose_cache.retain(|(s, _), _| *s != r.slot);
            self.npc_pose_verify.retain(|(s, _), _| *s != r.slot);
        }
        // ...then this tick's step of every player, the session's kernel
        // and the page's: gated per tick on the field owning the frame.
        legaia_engine_session::session::step_npc_clip_players(
            &self.session.host.world,
            self.npc_clip_players
                .iter_mut()
                .map(|(slot, player)| (*slot, player)),
        );
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

    /// `LEGAIA_DIAG_CAPTURE_STATE=1` instrument: on the screenshot frame,
    /// print the battle state a retail-compare residual is read against -
    /// the action state, the `ctx[+0x243]` marker, the stage ambient and
    /// backdrop cue, each roster member's equipment, each actor's seat,
    /// heading, mode word and tint, and every live effect sprite.
    /// `LEGAIA_DIAG_CAPTURE_VRAM=<path>` beside it writes the battle VRAM
    /// (1 MiB, BGR555) for a byte comparison with `mednafen-state vram-dump
    /// --out-bin`.
    pub(super) fn diag_capture_state(&self) {
        if std::env::var_os("LEGAIA_DIAG_CAPTURE_STATE").is_none() {
            return;
        }
        let w = &self.session.host.world;
        eprintln!(
            "capture state: action 0x{:02X} ctx243 {} ambient {:?} backdrop_cue {:?} sprites {}",
            w.battle_ctx.action_state,
            w.battle_ctx.gauge_rearm_latch,
            w.battle_ambient_base(),
            w.battle_backdrop_cue(),
            w.active_effect_sprites().len(),
        );
        if let (Some(path), Some(v)) = (
            std::env::var_os("LEGAIA_DIAG_CAPTURE_VRAM"),
            self.battle_vram.as_ref(),
        ) {
            let _ = std::fs::write(path, v.as_bytes());
        }
        for (i, m) in w.party.roster.members.iter().enumerate() {
            eprintln!("capture roster {i}: equip {:?}", m.equipment().slots);
        }
        eprintln!(
            "capture clock: tick {} frame step {} close-up accum {:?} fade age {:?}",
            w.clock.display_frames,
            w.battle_frame_step(),
            w.battle.camera.as_ref().map(|c| c.close_up_accum()),
            w.presentation.fade.as_ref().map(|f| f.age_vsyncs()),
        );
        eprintln!("capture camera: {:?}", w.battle_cam_pose());
        // Every battle body's draw inputs: where it stands, which clip and
        // keyframe it shows, and its tint state / colour word.
        for (i, a) in w
            .actors
            .iter()
            .enumerate()
            .take(8)
            .filter(|(_, a)| a.active)
        {
            eprintln!(
                "capture body {i}: pos ({}, {}) body {:?} hp {} anim {} reaction {:?} entry {:?} \
                 frame {:?} tint {} colour {:08X} blend {:04X}",
                a.move_state.world_x,
                a.move_state.world_z,
                a.battle.seat,
                a.battle.hp,
                a.battle.current_anim,
                a.battle_reaction,
                a.battle_reaction_entry,
                a.battle_animation.as_ref().map(|p| p.current_frame()),
                a.battle.render_flag,
                a.battle.render_color,
                a.battle.render_blend,
            );
        }
        for (rgb, abr, ot) in w.screen_fade_draws() {
            eprintln!("capture fade: rgb {rgb:06X} abr {abr} ot {ot}");
        }
        // Every live cast-scene part's draw state, in the retail actor
        // layout's offsets, to read beside a state's own `FUN_80021DF4`
        // nodes.
        let scenes = [
            ("summon", w.casting.active_summon.as_ref()),
            ("move_fx", w.casting.active_move_fx.as_ref()),
            ("mystic_shield", w.casting.mystic_shield_fx.as_ref()),
        ]
        .into_iter()
        .map(|(n, s)| (n, s.into_iter().collect::<Vec<_>>()))
        .chain(std::iter::once((
            "action_fx",
            w.casting.active_action_fx.iter().collect(),
        )));
        for (name, scene) in scenes {
            for p in scene
                .iter()
                .flat_map(|s| s.parts.iter())
                .filter(|p| !p.finished)
            {
                let s = &p.state;
                eprintln!(
                    "capture part {name}: tag {:?} sel {} pos [{}, {}, {}] rot [{}, {}, {}] \
                     52 {:#x} 56 {} 72 {:#x} 74 {:08X} 78 {:#x} 9e {:#x} 9c {} b4.. {:?} c0.. {:?} wait {} \
                     src {}",
                    p.tag.map(|t| format!("{t:08X}")),
                    p.model_sel,
                    s.world_x,
                    s.world_y,
                    s.world_z,
                    s.render_24,
                    s.render_26,
                    s.render_28,
                    s.field_52,
                    s.actor_u16(0x56),
                    s.field_72,
                    s.field_74,
                    s.field_78,
                    s.actor_u16(0x9E),
                    s.actor_u16(0x9C) as i16,
                    (0xB4..0xC0)
                        .step_by(2)
                        .map(|o| s.actor_u16(o) as i16)
                        .collect::<Vec<_>>(),
                    (0xC0..0xCC)
                        .step_by(2)
                        .map(|o| s.actor_u16(o) as i16)
                        .collect::<Vec<_>>(),
                    s.wait_timer,
                    (0x9C..0xCC)
                        .step_by(2)
                        .map(|o| format!("{:04x}", s.actor_u16(o)))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
        for (i, a) in w.actors.iter().enumerate().take(8) {
            eprintln!(
                "capture actor {i}: clip {:#04x} reaction {:?} entry {:?} cursor {:?} body {:?} pose {}",
                a.battle.current_anim,
                a.battle_reaction,
                a.battle_reaction_entry,
                a.battle_animation.as_ref().map(|p| p.cursor_sixteenths()),
                a.battle.seat,
                a.pose_frame.is_some(),
            );
            eprintln!(
                "capture actor {i}: xz ({},{}) y {} facing {} flag {:#010x} colour {:#010x} blend {:#x}",
                a.move_state.world_x,
                a.move_state.world_z,
                a.move_state.world_y,
                a.battle.facing_angle,
                a.battle.flag_word,
                a.battle.render_color,
                a.battle.render_blend,
            );
        }
        for s in w.active_effect_sprites() {
            eprintln!(
                "capture sprite: pos {:?} size {:?} page {:#04x} clut {:#06x} uv {:?}+{:?} bright {:#04x}",
                s.world_pos, s.size, s.page, s.clut, s.uv, s.uv_size, s.brightness
            );
        }
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
    pub(super) fn diag_effect_billboards(&self, cam: Mat4, view_scale: f32) {
        if std::env::var_os("LEGAIA_DIAG_FX_ALL").is_some() {
            // The quads the billboard builder draws, on the same basis, as
            // screen bounds in retail's 320 x 240 space: the side-by-side
            // against a display list's `0x2E` packets (`mednafen-state
            // display-list`).
            let inv = cam.inverse();
            let right = inv.transform_vector3(Vec3::X).normalize_or_zero();
            let up = inv.transform_vector3(Vec3::Y).normalize_or_zero();
            let screen = |c: [f32; 3]| {
                let p = cam * glam::Vec4::new(c[0], c[1], c[2], 1.0);
                (p.w > 0.01).then(|| {
                    (
                        (p.x / p.w * 0.5 + 0.5) * 320.0,
                        (0.5 - p.y / p.w * 0.5) * 240.0,
                        p.w,
                    )
                })
            };
            for b in legaia_engine_world_views::effect_draws::effect_billboards(
                &self.session.host.world,
                view_scale,
            ) {
                let s = &b.sprite;
                let Some((cx, cy, w)) = screen(s.world_pos) else {
                    continue;
                };
                let corners: Vec<(f32, f32)> = b
                    .corners(right.to_array(), up.to_array())
                    .iter()
                    .filter_map(|&c| screen(c).map(|(x, y, _)| (x, y)))
                    .collect();
                let (x0, x1) = corners
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, z), p| (a.min(p.0), z.max(p.0)));
                let (y0, y1) = corners
                    .iter()
                    .fold((f32::MAX, f32::MIN), |(a, z), p| (a.min(p.1), z.max(p.1)));
                eprintln!(
                    "DIAG fx-all: tick {} page {:#04x} centre ({cx:.0},{cy:.0}) w {w:.0} size {:?} \
                     quad x[{x0:.0},{x1:.0}] y[{y0:.0},{y1:.0}] bright {} uv {:?}",
                    self.tick_no, s.page, s.size, s.brightness, b.uv
                );
            }
        }
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
                legaia_render_kernels::effect_billboard::battle_billboard_centre(s.world_pos)
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

    /// The frame's floating value readout, as screen-space PSX primitives in
    /// stage coordinates.
    ///
    /// One run of digit cells per live popup, seated over the struck actor
    /// (`actor_stage_point`) and laid out by
    /// `legaia_engine_battle_vm::battle_value_readout::value_cells`, plus the combo
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
    ) -> Vec<legaia_render_kernels::screen_prim::ScreenPrim> {
        // The retail-art half: only while the sheet the quads sample is
        // resident. Before that the frame takes the font fallback instead
        // ([`Self::battle_value_readout_draws`]); the two are mutually
        // exclusive on this host exactly as they are on the play page,
        // because a frame that ran both would print every number twice.
        if self.battle_vram.is_none() {
            return Vec::new();
        }
        // `cam` is the FX camera, the battle camera with the stage scale
        // composed; the layout takes the camera alone.
        let cam = cam * Mat4::from_scale(Vec3::splat(1.0 / self.battle_stage_scale()));
        self.battle_value_readout_layout(cam)
            .map(|l| legaia_engine_screens::battle_value_readout::value_readout_prims(&l))
            .unwrap_or_default()
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
        if self.battle_vram.is_some() || w == 0 || h == 0 {
            return Vec::new();
        }
        let Some(layout) = self.battle_value_readout_layout(cam) else {
            return Vec::new();
        };
        let (origin, scale) = self.save_select_stage(w, h);
        legaia_engine_screens::battle_value_readout::value_readout_fallback_draws(
            &self.font, &layout, origin, scale,
        )
    }

    /// The world scale a battle's actors and effects are drawn at: the
    /// battle mode's base matrix, stage or no stage
    /// (`legaia_engine_session::battle_fx::battle_world_scale`).
    pub(super) fn battle_stage_scale(&self) -> f32 {
        legaia_engine_session::battle_fx::battle_world_scale(&self.session.host.world)
    }

    /// The readout's **layout**, shared by the retail-art emit above and the
    /// font fallback above it, seated through this host's camera
    /// (`legaia_engine_screens::battle_value_readout`, the layout the browser
    /// play page runs too).
    ///
    /// `cam` is the battle camera without the world scale the actor stage is
    /// drawn at; the layout scales the anchor itself.
    pub(super) fn battle_value_readout_layout(
        &self,
        cam: Mat4,
    ) -> Option<legaia_engine_screens::battle_value_readout::ValueReadoutLayout> {
        let world = &self.session.host.world;
        // A capture's seated popup rises over the trio of the frame its age
        // was taken back to (`ScreenshotConfig::popup_trio_rewind`).
        let rewind = self.screenshot.as_ref().and_then(|sc| {
            let (slot, back) = sc.popup_trio_rewind.get()?;
            let history = sc.popup_trio_history.borrow();
            let at = history.len().checked_sub(1 + usize::from(back))?;
            Some((slot, *history.get(at)?.get(usize::from(slot))?))
        });
        let retail_cells = self.screenshot.as_ref().and_then(|sc| {
            (sc.popup_cells_due.get() && !sc.seat_popup_cells.is_empty()).then(|| {
                use legaia_engine_battle_vm::battle_value_readout as vr;
                sc.seat_popup_cells
                    .iter()
                    .map(|&(x, y, w, digit)| vr::ValueCell {
                        digit,
                        x,
                        y,
                        w,
                        h: w,
                        u: vr::digit_cell_u(digit),
                        v: vr::DIGIT_ROW_V,
                        cell: vr::DIGIT_CELL,
                    })
                    .collect::<Vec<_>>()
            })
        });
        if let Some(cells) = retail_cells {
            let mut layout =
                legaia_engine_screens::battle_value_readout::battle_value_readout_layout(
                    world,
                    &self.battle_hud,
                    &cam.to_cols_array(),
                    self.battle_stage_scale(),
                )
                .unwrap_or((None, Vec::new()));
            layout.1 = vec![cells];
            return Some(layout);
        }
        legaia_engine_screens::battle_value_readout::battle_value_readout_layout_with(
            world,
            &self.battle_hud,
            &cam.to_cols_array(),
            self.battle_stage_scale(),
            |slot| match rewind {
                Some((s, trio)) if s == slot => trio,
                _ => world.battle_readout_trio(usize::from(slot)),
            },
        )
    }
}

/// Drive an open menu-runtime session (shop, prize exchange, inn) one tick on
/// this tick's pad edges, then unpark the field script a closed shop or
/// counter left suspended. A free function over the two fields it touches, so
/// the frozen-field arm and the frame tail share it.
pub(super) fn tick_menu_runtime_session(
    menu: &mut legaia_engine_menu_runtime::menu_runtime::MenuRuntime,
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
