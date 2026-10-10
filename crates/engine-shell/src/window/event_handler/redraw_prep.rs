//! The redraw's pre-render half: the capture harness's per-frame seating,
//! the field view sync, the windowed movie, the battle render edge, the
//! cutscene camera and the frame-local draw inputs taken out of `self`
//! before the renderer borrow - split out of `handle_redraw`.

use super::super::*;
use super::redraw_passes::CutsceneCam;

/// The frame-local draw inputs [`PlayWindowApp::take_frame_prep`] takes out
/// of `self` (or builds) before the renderer borrow, in the order it took
/// them.
pub(super) struct FramePrep {
    /// VDF vertex-morph pack meshes to re-upload this frame.
    pub field_morph_rebuilds: Vec<(usize, legaia_tmd::mesh::VramMesh)>,
    /// Op-`0x4B` morphs on clip-less placed NPCs.
    pub npc_morph_rebuilds: Vec<(
        u8,
        Option<(legaia_tmd::mesh::VramMesh, legaia_tmd::mesh::ColorMesh)>,
    )>,
    /// The field-to-battle transition emitter, put back after the render.
    pub battle_intro: Option<legaia_engine_render::battle_intro::BattleIntro>,
    /// The transition's screen primitives.
    pub battle_intro_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
    /// The field fog sheets.
    pub field_fog_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
    /// The move-VM strip spans.
    pub move_strip_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
    /// The fishing line and the fishing HUD's sprites.
    pub fishing_line_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
    /// The slot machine's cabinet.
    pub slot_prims: Vec<legaia_engine_render::screen_overlay::ScreenPrim>,
}

impl PlayWindowApp {
    /// The capture harness's per-frame seating: on the frame a capture is
    /// due, the retail state's own fog sheets, sprite arms, glide alignment,
    /// HUD glides, clear colour, panel, object models, walkers and morphs;
    /// and the battle idle orbit's phase alignment.
    pub(super) fn seat_capture_frame(&mut self) {
        // Capture harness: on the frame the capture is taken, the fog pool
        // shows the retail state's own sheets (`LEGAIA_SEAT_FOG`), installed
        // before this frame's draw pass runs the pool's render step.
        if let Some(sc) = self.screenshot.as_ref() {
            let gated =
                sc.phase_gate.is_some() || sc.script_gate.is_some() || sc.battle_drive.is_some();
            let due = if gated {
                self.capture_phase_met()
            } else {
                self.tick_no >= sc.capture_tick
            };
            if due && let Some(fog) = sc.seat_fog.take() {
                self.session.host.world.fog.install_snapshot(&fog);
            }
            // ... and the ambient tree's sprite-arm sheets where retail's
            // emitters had put them (`LEGAIA_SEAT_SPRITE_ARMS`).
            if due && let Some(arms) = sc.seat_sprite_arms.take() {
                self.session.host.world.install_sprite_arm_snapshot(&arms);
            }
            // A capture taken mid-glide shows the shot as far along as
            // retail's mover had come (its frame-skip history, which no
            // replay reproduces): the gate carries the frames it had left on
            // the displayed frame, and this frame's view lands exactly there.
            if due
                && let Some(left) = sc.script_gate.as_ref().and_then(|g| g.glide_left)
                && left > 0
            {
                self.cutscene_glide.align_frames_left(left as u32);
                self.session.camera.align_glide_frames_left(left);
            }
            if due && sc.seat_hud_glides_landed {
                self.session.host.world.land_battle_hud_glides();
            }
            if due {
                for g in &sc.seat_hud_glides {
                    // The combo cluster's anchor (placement record 80) glides
                    // in from `x = 328` onto `x = 168`
                    // (`battle_melee_hit_spark`'s record reads `(168, 168)`);
                    // the host owns its age.
                    if g.target[0] == 168 && (160..=176).contains(&g.target[1]) {
                        if let Some(c) = self.battle_hud.combo.as_mut() {
                            c.age = u16::from(g.elapsed);
                        }
                    } else {
                        self.session
                            .host
                            .world
                            .seat_battle_hud_glide(g.target, g.elapsed);
                    }
                }
            }
            if due && let Some(rgb) = sc.seat_clear {
                self.session.host.world.presentation.clear_rgb = rgb;
                self.session.host.world.presentation.clear_ramp = None;
            }
            if due && let Some(f) = sc.seat_page_mark {
                self.session.host.world.seat_page_mark_frame(f);
            }
            if due && let Some(p) = sc.seat_panel {
                let world = &mut self.session.host.world;
                let mut fx = std::mem::take(&mut world.presentation.fx);
                fx.panel = Some(p);
                world.presentation.fx_frame = fx.tick(0, |i| world.system_flag_test(i));
                world.presentation.fx = fx;
            }
            if due {
                for c in &sc.seat_object_clips {
                    self.session.host.world.seed_object_prop_clip(
                        usize::from(c.record),
                        c.clip,
                        c.cursor,
                        c.flags,
                        c.rate,
                    );
                }
                for &(record, model) in &sc.seat_object_models {
                    self.session
                        .host
                        .world
                        .seed_object_live_model(record, model);
                }
                for w in &sc.seat_walkers {
                    self.session
                        .host
                        .world
                        .seed_ambient_walker(w.flat, w.x, w.z, w.heading);
                }
                for m in &sc.seat_morphs {
                    self.session.host.world.seed_field_morph(
                        m.flat,
                        &m.weights,
                        m.done_mask,
                        m.env,
                    );
                }
            }
        }
        // Capture harness: phase-align the battle idle orbit to the retail
        // state being compared (`LEGAIA_BATTLE_ORBIT_YAW`).
        if let Some(yaw) = self.screenshot.as_ref().and_then(|sc| sc.battle_orbit_yaw)
            && self.session.host.world.mode == SceneMode::Battle
            && let Some(cam) = self.session.host.world.battle.camera.as_mut()
        {
            cam.align_orbit_yaw(yaw);
        }
        // The backdrop's slot-1 angle is a clock too
        // (`LEGAIA_BATTLE_BACKDROP_YAW`): hold it on the capture's.
        if let Some(yaw) = self
            .screenshot
            .as_ref()
            .and_then(|sc| sc.battle_backdrop_yaw)
            && self.session.host.world.mode == SceneMode::Battle
        {
            self.session.host.world.seed_battle_backdrop_slot_1_yaw(yaw);
        }
        // ... and, on the captured phase itself, the glide's origin
        // (`LEGAIA_BATTLE_CAM_ALIGN`, `BattleCamera::align_glide_origin`).
        if let Some((live, end)) = self.screenshot.as_ref().and_then(|sc| sc.battle_cam_align)
            && self.session.host.world.mode == SceneMode::Battle
            && self.capture_phase_met()
            && let Some(cam) = self.session.host.world.battle.camera.as_mut()
        {
            cam.align_glide_origin(live, end);
        }
    }

    /// Fold the script-animated floor-height ladder into the four baked
    /// field draw lists.
    pub(super) fn apply_floor_wave(&mut self) {
        // The scene floor-height ladder is script-animated (op `0x4C`
        // nibble-9): fold whatever the ticks above moved it by into the four
        // baked field draw lists, so the drawn ground undulates with the walk
        // heightfield instead of staying at the disc-static tier. A no-op -
        // and not even an iteration - on a scene whose script leaves the
        // ladder alone.
        {
            let Self {
                field_floor_wave,
                field_terrain_draws,
                field_terrain_color_draws,
                field_placement_draws,
                field_placement_color_draws,
                field_placement_window_keys,
                field_placement_color_window_keys,
                session,
                ..
            } = self;
            field_floor_wave.apply(
                &session.host.world,
                [
                    field_terrain_draws,
                    field_terrain_color_draws,
                    field_placement_draws,
                    field_placement_color_draws,
                ],
                [
                    field_placement_window_keys,
                    field_placement_color_window_keys,
                ],
            );
        }
    }

    /// The visible-tile crop for this frame, with the ground wave, the
    /// cropped ground and the lit env meshes synced to it.
    pub(super) fn sync_field_view(
        &mut self,
    ) -> Option<legaia_engine_core::field_view_window::ViewCells> {
        // Retail's visible-tile crop: the cell rectangle the field render
        // library walks this frame (`field_view_window`, the shared kernel the
        // browser play page asks too). The terrain draws are gated per draw
        // below; the ground re-uploads a cropped index list whenever the
        // rectangle moves.
        let view_cells = legaia_engine_core::field_view_window::field_view_cells(
            &self.session.host.world,
            legaia_engine_core::field_view_window::framing_is_retail(&self.session.camera)
                && !self.field_debug_camera,
        );
        self.sync_ground_wave();
        self.sync_ground_crop(view_cells.as_ref());
        // The env draws' light-source rows follow the live field light
        // (op `4C 8A` mid-scene).
        self.sync_field_lit_meshes();
        view_cells
    }

    /// Start a windowed movie a tick this frame triggered, and draw the
    /// current one. Returns `true` when a movie owns the frame (the scene
    /// render is skipped).
    pub(super) fn service_windowed_cutscene(&mut self) -> bool {
        // A tick this frame may have flipped the world into
        // SceneMode::Cutscene (field-VM FMV-trigger op). Start
        // windowed STR playback if so; a cut/missing slot drains the
        // trigger as a no-op (mirrors the headless `play` loop).
        if self.cutscene.is_none() {
            self.try_start_windowed_cutscene();
            // Its two skip arms (cut slot, undecodable STR) call
            // `finish_cutscene` themselves, and retail transfers control
            // whether or not the movie played - so the hand-off has to drain
            // here too. Safe beside the drain at the top of this handler:
            // both go through `World::take_finished_fmv`, so whichever runs
            // first is the only one that transfers.
            self.apply_fmv_handoff();
        }
        // While a cutscene plays, the window shows the video and the
        // scene render is skipped entirely.
        if self.cutscene.is_some() {
            self.render_windowed_cutscene();
            self.win.request_redraw();
            return true;
        }
        false
    }

    /// The battle render edge, the phase-scripted battle camera and the
    /// battle ground's ambient colour.
    pub(super) fn sync_battle_frame(&mut self) {
        // On a Field<->Battle transition, upload/drop monster meshes
        // and swap the VRAM. Must run before the render borrows
        // `uploaded_vram` below (this method may re-upload it).
        self.sync_battle_render();
        // Step the phase-scripted battle camera (dialogue close-up / far
        // menu framing + idle orbit / submenu close-up, with the measured
        // glides between). After `sync_battle_render` so battle entry sees
        // `battle_stage_mesh`; before the render borrow reads the pose.
        self.tick_battle_camera();
        // Colour the ground grid from this frame's battle ambient (a summon
        // close-up dims it); re-uploads the grid only when it moved.
        self.sync_battle_ground_ambient();
    }

    /// This frame's cutscene camera, eased through the shared glide kernel;
    /// `None` while no cutscene timeline owns the camera.
    pub(super) fn resolve_cutscene_camera(&mut self) -> Option<CutsceneCam> {
        // Ease the in-engine cutscene camera between Camera Configure
        // beats. Done here (outside the renderer borrow below) so the
        // interpolator can take `&mut self`; while no cutscene timeline
        // owns the scene the interp is reset so the next opening shot
        // snaps in rather than sweeping from a stale pose.
        // The cutscene camera also owns the WORLD-MAP frame while the opening
        // chain's map01 leg runs its timeline: retail's Rim Elm fly-in is
        // three op-0x45 beats in map01's opening record (snap to the high
        // aerial shot, then the `45 0B .. apply 900` ease-out descent), driven
        // through the same camera globals as the field cutscenes. Gate on a
        // staged param (`camera_view::cutscene_owns_camera`) so a beat record
        // WITHOUT camera beats - a field taunt, the Drake mist-wall
        // force-walk bands - keeps the ordinary field / walk camera.
        if legaia_engine_core::camera_view::cutscene_owns_camera(&self.session.host.world) {
            let (focus, pitch, yaw, roll, h, tr_eye) = self.cutscene_view();
            // Glide pacing from the op-`0x45` `apply_trigger` (retail
            // `FUN_801DE084` → `FUN_801DB510`): a Configure with `apply == 0`
            // commits its camera targets IMMEDIATELY (snap cut), while
            // `apply > 0` stages them and the per-frame mover glides the live
            // globals there over exactly `apply` frames - the mover law
            // (curve per mode nibble, 1 apply unit = 1 sim tick) is
            // capture-pinned; see `CutsceneCameraInterp`. opdeene's beats mix
            // both: the entry shot snaps (`apply 0`), the mid-prologue grove
            // drift glides (`apply 840`, paired with a 760-frame WaitFrames),
            // and the crater-rim tableau dolly glides (`apply 480`) WHILE the
            // narration text scrolls - the "3D keeps playing under the
            // crawl" retail behaviour. The interp arms glides PER COMPONENT
            // on target change (see `CutsceneCameraInterp::glide`), so the
            // H-only re-poke one frame after the tableau beat cannot snap
            // the in-flight dolly (the earlier whole-tuple ease-rate model
            // did exactly that, tele-porting the eye into the crater-rim
            // geometry - the "opening shot buried in a gold wall" report).
            //
            // Advanced in RETAIL DISPLAY-FRAME time, not render-frame or
            // sim-tick time, through the shared kernel
            // (`frame_step::CutsceneGlide`): retail's mover (`FUN_801DC0BC`)
            // credits one unit per display frame, so `apply` is a duration in
            // display frames and a redraw on which no tick ran advances the
            // glide by nothing. The kernel also replays this frame's snap
            // beats first (retail order: the mover snaps to an `apply 0` beat,
            // then glides from there when a same-tick follow-up beat
            // re-stages - the map01 fly-in pair).
            let target = legaia_engine_vm::psx_camera::FieldCameraView {
                focus,
                pitch,
                yaw,
                roll,
                h,
                tr_eye,
            };
            let v = self.cutscene_glide.advance(
                &self.session.host.world,
                &mut self.session.camera,
                target,
            );
            let out = (v.focus, v.pitch, v.yaw, v.roll, v.h, v.tr_eye);
            let apply = self.session.host.world.camera.state.apply_trigger;
            if std::env::var_os("LEGAIA_DIAG_CUTCAM").is_some() {
                let w = &self.session.host.world;
                eprintln!(
                    "DIAG cutcam: frame {} apply {} target focus={focus:?} pitch={pitch:.3} \
                 yaw={yaw:.3} roll={roll:.3} h={h} tr_eye={tr_eye:?} | eased focus={:?} \
                 pitch={:.3} yaw={:.3} roll={:.3} h={} tr_eye={:?} | params={:?}",
                    w.frame, apply, out.0, out.1, out.2, out.3, out.4, out.5, w.camera.state.params
                );
            }
            Some(out)
        } else {
            // Nothing is interpolating this frame, so a banked snap would
            // move a pose no draw reads - drop them rather than let them
            // land on the next shot.
            self.cutscene_glide.idle(&mut self.session.camera);
            None
        }
    }

    /// The frame-local draw inputs taken out of `self` before the renderer
    /// borrow, and the minigame surfaces posed and uploaded ahead of it.
    pub(super) fn take_frame_prep(&mut self) -> FramePrep {
        // VDF vertex morphs (jou's flesh-ground pulse, rikuroa's generator
        // sacs): rebuild the pack meshes whose morph deltas moved this frame
        // (collected outside the renderer borrow; uploaded inside it below).
        let field_morph_rebuilds = self.take_field_morph_rebuilds();
        // Op-`0x4B` morphs on placed NPCs: the clip-less slots' re-staged
        // static meshes (the clip-driven ones re-skin in the pose pass).
        let npc_morph_rebuilds = self.take_npc_morph_rebuilds();
        // Field-to-battle intro: advance the transition emitter and take both
        // it and its screen-space primitives out of `self`, before the
        // renderer borrow below - the same borrow-window pattern as the morph
        // rebuilds above. Both are empty whenever no transition is running,
        // and the emitter is put back after the render. See `window::battle`.
        let (battle_intro, battle_intro_prims) = self.take_battle_intro_frame();
        // Field fog sheets: the pool's render step runs here, in the same
        // borrow window, through the camera this frame draws the field with
        // (`FUN_8003F348` runs inside retail's field render pass, so it is a
        // draw-path step on both hosts). Empty outside a gated field scene.
        let field_fog_prims = self.take_field_fog_prims();
        // Move-VM strip spans (`FUN_801D31B0`), through the same camera.
        let move_strip_prims = self.take_move_strip_prims();
        // The fishing line (`FUN_801D26CC`'s packet, clipped by
        // `FUN_801D56E4`): latched here, outside the renderer borrow, through
        // the same follow camera - the session's yaw feedback is a write.
        let mut fishing_line_prims = self.fishing_line_screen_prims();
        // The fishing HUD's sprites (`FUN_801D63B0`'s quads) over the pond.
        fishing_line_prims.extend(self.fishing_hud_screen_prims());
        // The Baka duel's 3D surface: posed and uploaded here, outside the
        // renderer borrow (`window::minigames`).
        self.refresh_baka_duel_gpu();
        // The dance floor's bodies: posed by the engine surface and uploaded
        // here, drawn over the venue below.
        self.refresh_dance_cast_gpu();
        // The Muscle Dome's 3D arena, posed by its engine surface.
        self.refresh_muscle_dome_gpu();
        // The fishing pond + seated party, posed by its engine surface.
        self.refresh_fishing_gpu();
        // The slot machine's own VRAM (its art pack), resident while the
        // machine is on screen.
        self.refresh_slot_cabinet_gpu();
        let slot_prims = self.slot_cabinet_screen_prims();
        // The hall, cut to what the GPU draws under this frame's camera.
        self.refresh_dance_venue_view();
        FramePrep {
            field_morph_rebuilds,
            npc_morph_rebuilds,
            battle_intro,
            battle_intro_prims,
            field_fog_prims,
            move_strip_prims,
            fishing_line_prims,
            slot_prims,
        }
    }
}
