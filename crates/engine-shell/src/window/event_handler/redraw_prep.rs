//! The redraw's pre-render half: the capture harness's per-frame seating,
//! the field view sync, the windowed movie, the battle render edge, the
//! cutscene camera and the frame-local draw inputs taken out of `self`
//! before the renderer borrow.

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
    pub battle_intro: Option<legaia_render_kernels::battle_intro::BattleIntro>,
    /// The transition's screen primitives.
    pub battle_intro_prims: Vec<legaia_render_kernels::screen_prim::ScreenPrim>,
    /// The field fog sheets.
    pub field_fog_prims: Vec<legaia_render_kernels::screen_prim::ScreenPrim>,
    /// The move-VM strip spans.
    pub move_strip_prims: Vec<legaia_render_kernels::screen_prim::ScreenPrim>,
    /// The fishing line and the fishing HUD's sprites.
    pub fishing_line_prims: Vec<legaia_render_kernels::screen_prim::ScreenPrim>,
    /// The slot machine's cabinet.
    pub slot_prims: Vec<legaia_render_kernels::screen_prim::ScreenPrim>,
}

impl PlayWindowApp {
    /// The capture harness's per-frame seating: on the frame a capture is
    /// due, the retail state's own fog sheets, sprite arms, glide alignment,
    /// HUD glides, clear colour, panel, object models, walkers and morphs;
    /// and the battle idle orbit's phase alignment.
    pub(super) fn seat_capture_frame(&mut self) {
        // Capture harness: on the frame the capture is taken, the fog pool
        // shows the retail state's own sheets (`LEGAIA_SEAT_FOG`). The tick
        // has already stepped the engine's own pool, so the frame's fog draw
        // installs the records and re-runs the frame's step over them
        // (`take_field_fog_prims`, `BootSession::seat_field_fog`).
        if let Some(sc) = self.screenshot.as_ref() {
            let gated =
                sc.phase_gate.is_some() || sc.script_gate.is_some() || sc.battle_drive.is_some();
            let due = if gated {
                self.capture_phase_met()
            } else {
                self.tick_no >= sc.capture_tick
            };
            if due {
                sc.seat_fog_due.set(true);
                sc.popup_cells_due.set(true);
            }
            // ... and the ambient tree's sprite-arm sheets where retail's
            // emitters had put them (`LEGAIA_SEAT_SPRITE_ARMS`).
            if due && let Some(arms) = sc.seat_sprite_arms.take() {
                self.session.host.world.install_sprite_arm_snapshot(&arms);
            }
            // ... and a captured cast's parts where retail's passes had put
            // them (`LEGAIA_SEAT_CAST_PARTS`).
            if due
                && let Some(parts) = sc.seat_cast_parts.take()
                && let Some(scene) = self.session.host.world.casting.active_summon.as_mut()
            {
                scene.install_part_snapshot(&parts);
            }
            // ... and the effect-script table parts' rotation clocks
            // (`LEGAIA_SEAT_ACTION_FX_PARTS`).
            if due && let Some(parts) = sc.seat_action_fx_parts.take() {
                self.session
                    .host
                    .world
                    .install_action_fx_part_snapshot(&parts);
            }
            // ... and the after-image ghosts where the capture's history
            // ring stood them (`LEGAIA_SEAT_GHOST_RING`).
            if due && let Some(ghosts) = sc.seat_ghost_ring.take() {
                legaia_engine_capture::ghost_ring::apply_ghost_ring(
                    &mut self.session.host.world,
                    &ghosts,
                );
            }
            // ... and the effect pool's billboards where retail's walker had
            // put them (`LEGAIA_SEAT_EFFECT_CHILDREN`).
            if due && let Some(children) = sc.seat_effect_children.take() {
                self.session
                    .host
                    .world
                    .install_effect_child_snapshot(&children);
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
            if due && !sc.seat_battle_clips.is_empty() {
                legaia_engine_capture::clip_cursors::apply_clip_cursors(
                    &mut self.session.host.world,
                    &sc.seat_battle_clips,
                );
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
            // The newest numeral on the struck slot, aged to the rise
            // retail's displayed frame drew (`LEGAIA_SEAT_POPUP_AGE`).
            // The popup and its anchor are both rewound: the trio it rose
            // over that many frames ago (`popup_trio_history`).
            if let Some((slot, age)) = sc.seat_popup_age {
                let world = &self.session.host.world;
                let trios: Vec<Option<[f32; 3]>> = (0..world.actors.len())
                    .map(|i| world.battle_readout_trio(i))
                    .collect();
                let mut history = sc.popup_trio_history.borrow_mut();
                history.push_back(trios);
                if history.len() > 16 {
                    history.pop_front();
                }
                if due
                    && sc.popup_trio_rewind.get().is_none()
                    && let Some(p) = self
                        .battle_hud
                        .popups
                        .iter_mut()
                        .filter(|p| p.slot == slot && p.status.is_none())
                        .min_by_key(|p| p.frames_total.saturating_sub(p.frames_remaining))
                {
                    let now = p.frames_total.saturating_sub(p.frames_remaining);
                    p.frames_remaining = p.frames_total.saturating_sub(age);
                    sc.popup_trio_rewind
                        .set(Some((slot, now.saturating_sub(age))));
                }
            }
            if due && let Some((width, plate)) = sc.seat_spirit_gauge {
                let ctx = &mut self.session.host.world.battle_ctx;
                ctx.spirit_bar_width = width;
                ctx.spirit_plate_value = plate;
            }
            if due && let Some(rgb) = sc.seat_clear {
                self.session.host.world.presentation.clear_rgb = rgb;
                self.session.host.world.presentation.clear_ramp = None;
            }
            if due && let Some(bias) = sc.seat_curve_bias {
                self.session.host.world.world_map.curve_bias = bias;
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
        // ... and, once a cast capture's gate holds, the summon close-up's
        // accumulator (`PhaseGate::accum_align`).
        if let Some(gate) = self.screenshot.as_ref().and_then(|sc| sc.phase_gate)
            && gate.met(&self.session.host.world)
        {
            gate.align_camera(&mut self.session.host.world);
        }
        // ... and a module shot still walking, onto the displayed frame's
        // pose (`LEGAIA_BATTLE_MODULE_CAM_ALIGN`,
        // `BattleCamera::align_module_shot`).
        if let Some((live, end)) = self
            .screenshot
            .as_ref()
            .and_then(|sc| sc.battle_module_cam_align)
            && self.session.host.world.mode == SceneMode::Battle
            && self.capture_phase_met()
            && let Some(cam) = self.session.host.world.battle.camera.as_mut()
        {
            cam.align_module_shot(live, end);
        }
        // ... and, on the captured phase itself, the glide's origin
        // (`LEGAIA_BATTLE_CAM_ALIGN`, `BattleCamera::align_glide_origin`).
        if let Some((live, end)) = self.screenshot.as_ref().and_then(|sc| sc.battle_cam_align)
            && self.session.host.world.mode == SceneMode::Battle
            && self.capture_phase_met()
            && self.session.host.world.battle.camera.is_some()
        {
            // Stand the combatants on the capture's own ground first, and
            // align against the framing the camera's law arms there
            // (`LEGAIA_BATTLE_GROUND_SNAP`, `ground_snap`).
            let snaps = self
                .screenshot
                .as_ref()
                .map(|sc| sc.battle_ground_snap.clone())
                .unwrap_or_default();
            let world = &mut self.session.host.world;
            let armed = legaia_engine_capture::ground_snap::snap_battle_ground(world, &snaps);
            // On a display-lag retake the frame is two game frames older than
            // the RAM the endpoints were read from, and the framing armed
            // here may stand a step of yaw drift or depth ramp short of them;
            // a component the gate tick's framing agrees on (the first run
            // carried it over, `LEGAIA_BATTLE_CAM_GATE_TARGET`) takes the
            // gate's (`retake_align_target`). The gate tick keeps its own for
            // the retake it names.
            let gate_target = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.battle_cam_gate_target);
            if let Some(sc) = self.screenshot.as_ref()
                && gate_target.is_none()
            {
                sc.gate_cam_target.set(armed);
            }
            let target = match (armed, gate_target) {
                (Some(a), Some(g)) => {
                    Some(legaia_engine_battle_vm::battle_cam_script::retake_align_target(a, g, end))
                }
                (a, _) => a,
            };
            // A Done-band capture's displayed pose, where the glide walks
            // retail's own table (`LEGAIA_BATTLE_CAM_ALIGN_DISPLAYED`).
            let displayed = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.battle_cam_align_displayed);
            let world = &mut self.session.host.world;
            let mask = match (world.battle.camera.as_mut(), target) {
                (Some(cam), Some(target)) => {
                    let live = match displayed {
                        Some((shown, _)) if cam.glide_endpoint_matches_to(end, target) => shown,
                        _ => live,
                    };
                    cam.align_glide_origin_to(live, end, target)
                }
                _ => 0,
            };
            // The near-camera ghost bits are derived from the pose, and the
            // tick computed them on the unaligned one. Re-derive them only
            // when every component took retail's value: a partly aligned
            // pose is neither retail's camera nor the engine's own.
            if mask == 0xFF {
                // Every component took retail's pose, so the framing no
                // longer reads the engine's bodies: the body pairs a seat
                // stood a pose-mismatched body on served the framing law
                // only, and the frame draws each body where retail's live
                // pair puts it - its pose drawn from the live pair, as
                // `FUN_80048A08` draws it - not shifted by the gap between
                // the two poses' centroids (`battle_vahn_tri_somersault_super`:
                // Vahn `62` and the reacting monster `76` units off retail's
                // pairs on their body snaps).
                if snaps.iter().any(|s| s.body.is_some()) {
                    let live: Vec<_> = snaps
                        .iter()
                        .copied()
                        .map(|mut s| {
                            s.body = None;
                            s
                        })
                        .collect();
                    let _ = legaia_engine_capture::ground_snap::snap_battle_ground(world, &live);
                }
                world.refresh_battle_camera_ghost();
            }
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
    ) -> Option<legaia_engine_scene::field_view_window::ViewCells> {
        // Diag: `LEGAIA_DIAG_FOLLOW_KNOBS=orbit,tilt,zoom` pins the user's
        // follow knobs (radians, radians, factor) every frame - the mouse
        // drag / wheel a screenshot run cannot send.
        if let Some([orbit, tilt, zoom]) =
            std::env::var("LEGAIA_DIAG_FOLLOW_KNOBS")
                .ok()
                .and_then(|s| {
                    let v: Vec<f32> = s.split(',').filter_map(|t| t.trim().parse().ok()).collect();
                    <[f32; 3]>::try_from(v).ok()
                })
        {
            let cam = &mut self.session.camera;
            cam.manual_orbit = orbit.rem_euclid(std::f32::consts::TAU);
            cam.manual_tilt = tilt;
            cam.manual_zoom = zoom;
        }
        // Retail's visible-tile crop: the cell rectangle the field render
        // library walks this frame (`field_view_window`, the shared kernel the
        // browser play page asks too). The terrain draws are gated per draw
        // below; the ground re-uploads a cropped index list whenever the
        // rectangle moves.
        let view_cells = legaia_engine_world_views::field_view_window::field_view_cells(
            &self.session.host.world,
            legaia_engine_scene::field_view_window::framing_is_retail(&self.session.camera)
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
        if legaia_engine_world_views::camera_view::cutscene_owns_camera(&self.session.host.world) {
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
            let target = legaia_engine_camera::psx_camera::FieldCameraView {
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
        // Field fog sheets: the quads the last sim tick's render step left
        // (`FUN_8003F348` runs once per retail frame, inside the field render
        // pass, so both hosts step it per tick - `BootSession::step_field_fog`
        // - and the draw only wraps them). Empty outside a gated field scene.
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
