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
        // `LEGAIA_DIAG_CAST_HIDE=<va>,..`: the cast recorder's per-part draw
        // toggle - parts seated from these records stay out of the draw
        // lists (their simulation runs on), to name the part a frame shows.
        if let Some(hide) = diag_cast_hide()
            && let Some(scene) = self.session.host.world.casting.active_summon.as_mut()
            && scene.diag_hidden != *hide
        {
            scene.diag_hidden = hide.clone();
        }
        // Capture harness: on the frame the capture is taken, the fog pool
        // shows the retail state's own sheets (`LEGAIA_SEAT_FOG`). The tick
        // has already stepped the engine's own pool, so the frame's fog draw
        // installs the records and re-runs the frame's step over them
        // (`take_field_fog_prims`, `BootSession::seat_field_fog`).
        if let Some(sc) = self.screenshot.as_ref() {
            let gated = sc.phase_gate.is_some()
                || sc.arrival_gate.is_some()
                || sc.script_gate.is_some()
                || sc.battle_drive.is_some();
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
            // ... and the turning ambient parts' banks as the displayed frame
            // drew them: their angle is time since the entry
            // (`LEGAIA_SEAT_TURNING_PARTS`).
            if due && let Some(parts) = sc.seat_turning_parts.take() {
                self.session.host.world.seat_ambient_part_banks(&parts);
            }
            // ... and the dance's marker tiles on the steps the displayed frame
            // drew (`LEGAIA_SEAT_DANCE_MARKERS`).
            if due
                && let Some(rows) = sc.seat_dance_markers.take()
                && let Some(g) = self.session.host.world.minigames.dance.as_mut()
            {
                for (class, [cursor, timer, mesh]) in rows {
                    g.seat_step_marker(class, cursor as u16, timer, mesh);
                }
            }
            // ... and the hall's animated props on the clip cursors the
            // displayed frame drew (`LEGAIA_SEAT_DANCE_PROPS`).
            if due
                && let Some(clips) = sc.seat_dance_props.take()
                && let Some(g) = self.session.host.world.minigames.dance.as_mut()
            {
                for (clip, cursor, passes) in clips {
                    g.seat_hall_prop_displayed(clip, cursor, passes);
                }
            }
            // ... and the text balloon the displayed frame drew, or none
            // (`LEGAIA_SEAT_BALLOON`).
            if due && let Some(b) = sc.seat_balloon.take() {
                self.session.host.world.cutscene.text_balloon = b;
                // A seated balloon arrives unmeasured, as a spawned one
                // does, and the tick's own measure pass has already run
                // this frame: retail's spawner measures before the first
                // draw (`FUN_8003C764` -> `FUN_80035F04`), so measure now
                // or the captured frame shows the window without its line.
                legaia_engine_screens::field_messages::commit_text_balloon_measure(
                    &mut self.session.host.world,
                    &self.font,
                );
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
            // ... and the arrival's columns off a frame whose RAM holds none
            // (`LEGAIA_SEAT_ARRIVAL_GONE`): a replay reaches its first strike
            // inside their life, a fight rounds further on does not.
            if due && sc.seat_arrival_gone.take() {
                self.session.host.world.retire_arrival_spawns();
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
                if let Some(pairs) = &sc.seat_glide_pairs {
                    self.session.camera.seat_glide_pairs(pairs);
                }
                self.session.camera.align_glide_frames_left(left);
            }
            // A capture no pose reaches stands the camera on this frame's
            // seats, and the tick's ghost bits read the pose before them: the
            // pass walks a table, and a seat's jump is none of retail's.
            //
            // Not on a sweep: a sweep films the play path on a pad script and
            // seats nothing, and re-running the pass after the tick there
            // ran it after the anim pass too - a defeat fade's first frame
            // drew additive one frame before the play path (and the page)
            // draws it, retail's order being ghost pass, presentation tick,
            // then the walker that raises the fade.
            if due && sc.sweep.is_none() && sc.battle_ghost_pose.is_none() {
                self.session.host.world.refresh_battle_camera_ghost();
            }
            if due && !sc.seat_battle_clips.is_empty() {
                legaia_engine_capture::clip_cursors::apply_clip_cursors(
                    &mut self.session.host.world,
                    &sc.seat_battle_clips,
                );
            }
            // A round prompt caught with its far framing still walking
            // stands the engine's arm as many frames short of its end
            // (`LEGAIA_SEAT_MENU_GLIDE_LEFT`).
            if due
                && let Some(left) = sc.seat_menu_glide_left
                && let Some(cam) = self.session.host.world.battle.camera.as_mut()
            {
                cam.align_menu_glide_frames_left(left);
                if let Some(shown) = sc.seat_menu_glide_pose {
                    cam.align_menu_glide_pose(shown, left);
                }
            }
            if sc.seat_sideband_seek.is_some() {
                self.session.host.world.casting.sideband_first_seek_seat = sc.seat_sideband_seek;
            }
            // Ahead of the cast it forks: battle init clears the word.
            if let Some(n) = sc.seat_ai_counter
                && self.session.host.world.casting.active_summon.is_none()
            {
                self.session
                    .host
                    .world
                    .battle
                    .monster_ai_state
                    .set_counter(n);
            }
            if due && sc.seat_hud_glides_landed {
                self.session.host.world.battle.land_battle_hud_glides();
            }
            if due && sc.seat_move_label_unseen {
                self.session.host.world.battle.move_label_closed = true;
                self.session.host.world.battle.monster_move_label = None;
            }
            if due && let Some((actor, member)) = sc.seat_hud_close {
                let targets: Vec<[i16; 2]> = sc.seat_hud_glides.iter().map(|g| g.target).collect();
                self.session
                    .host
                    .world
                    .seat_battle_hud_close(actor, member, &targets);
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
            // retail's displayed frame drew (`LEGAIA_SEAT_POPUP_AGE`). Only
            // the age is seated: the renderer `FUN_801DF6B8` reads the struck
            // body's display trio `+0x3C..+0x40` on every draw
            // (`0x801DF758..0x801DF780`), so the anchor is the frame's own
            // readout trio, not the one the popup was pushed over.
            if let Some((slot, age)) = sc.seat_popup_age
                && due
                && !sc.popup_age_seated.get()
                && let Some(p) = self
                    .battle_hud
                    .popups
                    .iter_mut()
                    .filter(|p| p.slot == slot && p.status.is_none())
                    .min_by_key(|p| p.frames_total.saturating_sub(p.frames_remaining))
            {
                p.frames_remaining = p.frames_total.saturating_sub(age);
                sc.popup_age_seated.set(true);
            }
            if due && let Some((page, slide)) = sc.seat_arts_list {
                self.session.host.world.battle.seat_arts_list(page, slide);
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
                if let Some(c) = sc.seat_player_clip {
                    self.session
                        .host
                        .world
                        .seed_player_clip(c.clip, c.cursor, c.flags, c.rate);
                }
                for &(record, byte) in &sc.seat_object_flags {
                    self.session.host.world.seed_object_draw_flags(record, byte);
                }
                for &(record, model) in &sc.seat_object_models {
                    self.session
                        .host
                        .world
                        .seed_object_live_model(record, model);
                }
                for w in &sc.seat_placements {
                    self.session
                        .host
                        .world
                        .seed_placement_seat(w.flat, w.x, w.z, w.heading);
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
                if !sc.seat_floor_bobs.is_empty() {
                    let bobs: Vec<_> = sc
                        .seat_floor_bobs
                        .iter()
                        .map(|b| (b.rung, b.pos, b.vel, b.accel, b.target))
                        .collect();
                    self.session.host.world.seed_floor_ladder(&bobs);
                }
                self.session.host.world.seed_morph_weight_actors(
                    sc.seat_morph_weights
                        .iter()
                        .map(|m| (m.up_rate, m.down_rate, m.weight, m.descending)),
                );
            }
        }
        // Capture harness: phase-align the battle idle orbit to the retail
        // state being compared (`LEGAIA_BATTLE_ORBIT_YAW`), on the captured
        // frame alone - the gate's, or the fixed tick's - as the headless
        // seed aligns it only where it samples. Aligned on every redraw, the
        // child's camera stood elsewhere than the seed's between ticks, and
        // a menu drive's glide-landed hold (`BattleDrive::reached`) parted
        // from it.
        let capture_frame = self.screenshot.as_ref().is_some_and(|sc| {
            if sc.battle_drive.is_some()
                || sc.phase_gate.is_some()
                || sc.arrival_gate.is_some()
                || sc.script_gate.is_some()
            {
                self.capture_phase_met()
            } else {
                self.tick_no >= sc.capture_tick
            }
        });
        // Capture harness: a field view off retail's framing
        // (`LEGAIA_CAPTURE_FIELD_KNOBS`), held on every field frame.
        if let Some([orbit, tilt, zoom]) = self.screenshot.as_ref().and_then(|sc| sc.field_knobs)
            && self.session.host.world.mode == SceneMode::Field
        {
            let cam = &mut self.session.camera;
            (cam.manual_orbit, cam.manual_tilt, cam.manual_zoom) = (orbit, tilt, zoom);
        }
        if let Some(yaw) = self.screenshot.as_ref().and_then(|sc| sc.battle_orbit_yaw)
            && self.session.host.world.mode == SceneMode::Battle
            && capture_frame
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
            // A film that runs on from the capture's gate
            // (`LEGAIA_DIAG_CAST_SEQ_BEGIN=gate`) lets go of the angle there:
            // retail's frame driver winds it on every frame after
            // (`World::tick_battle_backdrop_spin`), and held it stood the sky
            // still behind the whole strike.
            // The gate itself, not the capture frame: a film forces its
            // capture tick past the run (`LEGAIA_DIAG_CAPTURE_TICK`).
            if std::env::var("LEGAIA_DIAG_CAST_SEQ_BEGIN").as_deref() == Ok("gate")
                && self.capture_gate_met()
                && let Some(sc) = self.screenshot.as_mut()
            {
                sc.battle_backdrop_yaw = None;
            }
        }
        // ... and, once a cast capture's gate holds, the summon close-up's
        // accumulator (`PhaseGate::accum_align`).
        if let Some(gate) = self.screenshot.as_ref().and_then(|sc| sc.phase_gate)
            && gate.met(&self.session.host.world)
        {
            gate.align_camera(&mut self.session.host.world);
            // ... then the screen fades as far on as the displayed frame's
            // own table had them (`LEGAIA_SEAT_FADE_LEAD`), past the flash
            // the alignment just re-ran to the gate's age.
            if let Some(n) = self
                .screenshot
                .as_ref()
                .and_then(|sc| sc.seat_fade_lead.take())
            {
                self.session.host.world.lead_screen_fades(n);
            }
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
        // ... and a summon close-up onto the body pair its displayed frame
        // was framed on (`LEGAIA_BATTLE_SUMMON_FOCUS`,
        // `BattleCamera::align_summon_focus`).
        if let Some(body) = self
            .screenshot
            .as_ref()
            .and_then(|sc| sc.battle_summon_focus)
            && self.session.host.world.mode == SceneMode::Battle
            && self.capture_phase_met()
            && let Some(cam) = self.session.host.world.battle.camera.as_mut()
        {
            cam.align_summon_focus(body);
        }
        // The capture band's stage cue on the captured phase
        // (`LEGAIA_BATTLE_BACKDROP_CUE`, `RetailBattle::capture_band_backdrop_cue`).
        if let Some(w) = self
            .screenshot
            .as_ref()
            .and_then(|sc| sc.battle_backdrop_cue)
            && self.session.host.world.mode == SceneMode::Battle
            && self.capture_phase_met()
        {
            self.session.host.world.seat_battle_backdrop_cue(w);
        }
        if let Some(b) = self
            .screenshot
            .as_ref()
            .and_then(|sc| sc.battle_ambient_base)
            && self.session.host.world.mode == SceneMode::Battle
            && self.capture_phase_met()
        {
            self.session.host.world.battle.ambient_stored = [b; 3];
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
            let armed = legaia_engine_capture::ground_snap::snap_battle_ground(world, &snaps)
                .map(|t| legaia_engine_capture::ground_snap::strike_heading_target(world, t, end));
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
                    let (shown, held) = match displayed {
                        // Judged on the components the walk back moved: one
                        // retail holds still reads the same either way
                        // (`BattleCamera::glide_endpoint_matches_walked`).
                        // A component both framings hold takes the displayed
                        // value whatever the two endpoints read: it is the
                        // strike walk's leftover, not a framing
                        // (`BattleCamera::framing_held_components`).
                        Some((shown, _))
                            if cam.glide_endpoint_matches_walked(end, target, live, shown) =>
                        {
                            (shown, cam.framing_held_components(end, target, live))
                        }
                        _ => (live, 0),
                    };
                    // A capture-band module's own shot still walking (`0x70`
                    // arms no framing; a slot-B arm built the table once)
                    // whose endpoints are retail's where the glide's are
                    // not: its walk takes retail's pose on the shot's
                    // increments, as a summon module's does
                    // (`BattleCamera::align_module_shot`). Otherwise the
                    // glide's origin.
                    let mut glide = cam.clone();
                    let by_glide = glide.align_glide_origin_to(shown, end, target)
                        | glide.seat_pose_components(shown, held);
                    // A yaw the ease band placed short of the engine's own
                    // endpoint draws retail's (`BattleCamera::seat_eased_yaw`).
                    glide.seat_eased_yaw(shown, end, target, by_glide);
                    let mut shot = cam.clone();
                    let by_shot = shot.align_module_shot(live, end);
                    if by_shot.count_ones() > by_glide.count_ones() {
                        *cam = shot;
                        by_shot
                    } else {
                        *cam = glide;
                        by_glide
                    }
                }
                _ => 0,
            };
            let mask = if self
                .screenshot
                .as_ref()
                .is_some_and(|sc| sc.seat_cam_behind_commit)
                && let Some(cam) = world.battle.camera.as_mut()
            {
                cam.align_commit_pass(live);
                0xFF
            } else {
                mask
            };
            // A monster's strike: case 7 re-arms every pass, so the RAM's pose
            // walked back is the displayed frame's on every component, whatever
            // the replay's body pairs put the framing on
            // (`RetailBattle::stand_on_displayed_pose`).
            let mask = if self
                .screenshot
                .as_ref()
                .is_some_and(|sc| sc.battle_cam_stand_displayed)
                && let Some(cam) = self.session.host.world.battle.camera.as_mut()
            {
                cam.stand_on_pose(live);
                0xFF
            } else {
                mask
            };
            // A monster's magic-band retake on a shot still walking: the
            // displayed frame is the framing the shot replaced, and `live`
            // is retail's pose walked back onto it along the RAM's table.
            // The replay's own case 6 carries the camera history of its own
            // rounds (`RetailBattle::monster_magic_band_age`).
            // A capture-band retake across the band's entry reads the same
            // pose (`RetailBattle::retake_across_band_entry`).
            if live != end
                && std::env::var_os("LEGAIA_DIAG_CAPTURE_TICK").is_some()
                && self.screenshot.as_ref().is_some_and(|sc| {
                    sc.retake_shot_only
                        || (sc.retake_across_entry
                            && sc
                                .battle_drive
                                .as_ref()
                                .is_some_and(|d| !d.in_action_state(&self.session.host.world)))
                })
                && let Some(cam) = self.session.host.world.battle.camera.as_mut()
            {
                cam.stand_on_pose(live);
            }
            // A turning shot's retake: the yaw it took over.
            if let Some(yaw) = self.screenshot.as_ref().and_then(|sc| sc.retake_yaw)
                && std::env::var_os("LEGAIA_DIAG_CAPTURE_TICK").is_some()
                && let Some(cam) = self.session.host.world.battle.camera.as_mut()
            {
                let mut pose = cam.framing_pose();
                pose.yaw = yaw;
                cam.stand_on_pose(pose);
            }
            // ... or the whole pose it stood on.
            if let Some(pose) = self.screenshot.as_ref().and_then(|sc| sc.retake_pose)
                && std::env::var_os("LEGAIA_DIAG_CAPTURE_TICK").is_some()
                && let Some(cam) = self.session.host.world.battle.camera.as_mut()
            {
                cam.stand_on_pose(pose);
            }
            let ghost_pose = self.screenshot.as_ref().and_then(|sc| sc.battle_ghost_pose);
            let world = &mut self.session.host.world;
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
                match ghost_pose {
                    Some(pose) => world.refresh_battle_camera_ghost_on(pose),
                    None => world.refresh_battle_camera_ghost(),
                }
            } else if let Some(pose) = ghost_pose
                && (0x6E..=0x71).contains(&world.battle_ctx.action_state)
            {
                // In the capture band only the near-camera test sets a bit
                // (the target-selection row arm is `0x28..=0x2E`'s), and it
                // reads the camera it is handed: retail's pose names the bits
                // whatever the alignment took (`battle_uru2_nurga_glare_mid_cast`,
                // the second Nurga faint as retail draws it). A magic-band
                // frame keeps the replay's bits, which its row arm set.
                world.refresh_battle_camera_ghost_on(pose);
            } else if let Some((shown, _)) =
                self.screenshot.as_ref().and_then(|sc| sc.battle_cam_align)
                && (0x1E..=0x20).contains(&world.battle_ctx.action_state)
            {
                // The strike band has no row arm either, so its bits are the
                // near-camera test's on retail's camera rather than on the
                // replay's own glide, which a partial alignment leaves
                // standing (`battle_garmel_party_noa_lizard_tail_mid`: the
                // replay's pose ghosted Vahn, `1624` units from the test
                // point against a radius of `1228` on retail's; the
                // displayed pose and the pass-older ghost pose agree there).
                // The displayed pose is the one the alignment aimed at, and
                // the one `battle_ropeway_party_noa_sonic_javelin_open`'s
                // frame confirms where the two part.
                world.refresh_battle_camera_ghost_on(shown);
            }
            // A seat the displayed frame drew elsewhere than the RAM's ground
            // (`LEGAIA_BATTLE_DISPLAYED_SEAT`): moved after the alignment, so
            // the camera keeps the framing the RAM's ground armed.
            let seats = self
                .screenshot
                .as_ref()
                .map(|sc| sc.battle_displayed_seat.clone())
                .unwrap_or_default();
            // ... and posed on the clip that frame shows
            // (`LEGAIA_SEAT_DISPLAYED_CLIPS`).
            let clips = self
                .screenshot
                .as_ref()
                .map(|sc| sc.seat_displayed_clips.clone())
                .unwrap_or_default();
            if !seats.is_empty() || !clips.is_empty() {
                let world = &mut self.session.host.world;
                let _ = legaia_engine_capture::ground_snap::snap_battle_ground(world, &seats);
                legaia_engine_capture::clip_cursors::apply_displayed_clips(world, &clips);
                if mask == 0xFF {
                    match ghost_pose {
                        Some(pose) => world.refresh_battle_camera_ghost_on(pose),
                        None => world.refresh_battle_camera_ghost(),
                    }
                }
            }
            // The frame on screen is the last actor's Done tail, whose ghost
            // bits the capture's setup state has since cleared.
            if let Some(legaia_engine_capture::battle_drive::BattleDrive::Action {
                steer:
                    legaia_engine_capture::battle_drive::ActionSteer {
                        prior: Some(prior), ..
                    },
                ..
            }) = self.screenshot.as_ref().and_then(|sc| sc.battle_drive)
            {
                let world = &mut self.session.host.world;
                let (seat, target) = prior.engine_seats(world.party.party_count.clamp(1, 3));
                world.refresh_prior_action_ghost(seat, prior.category, target);
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
            legaia_engine_world_views::field_view_window::crop_framing(
                &self.session.host.world,
                &self.session.camera,
                self.field_debug_camera,
            ),
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
        // A cast recording's opening: the pose retail's recording stood on,
        // on the cast begin, or for a film that opens on the capture's own
        // gate (`LEGAIA_DIAG_CAST_SEQ_BEGIN=gate`) on that gate's tick.
        // A gate film's drive plays the round through its own `0x28` first,
        // so it seats on the gate alone.
        let on_gate = std::env::var("LEGAIA_DIAG_CAST_SEQ_BEGIN").as_deref() == Ok("gate");
        let seat_now = if on_gate {
            self.capture_gate_met()
        } else {
            matches!(self.session.host.world.battle_ctx.action_state, 0x28 | 0x3C)
        };
        if let Some(sc) = self.screenshot.as_ref() {
            let world = &mut self.session.host.world;
            if seat_now
                && let Some(pose) = sc.cast_seq_seat_pose.take()
                && let Some(cam) = world.battle.camera.as_mut()
            {
                cam.stand_on_pose(pose);
            }
            if matches!(world.battle_ctx.action_state, 0x28 | 0x3C)
                && let Some((clip, cursor)) = sc.cast_seq_seat_cursor.take()
            {
                let slot = usize::from(world.battle_ctx.active_actor);
                world.seat_battle_clip_cursor(slot, clip, cursor);
            }
        }
        // Colour the ground grid from this frame's battle ambient (a summon
        // close-up dims it); re-uploads the grid only when it moved.
        self.sync_battle_ground_ambient();
    }

    /// This frame's cutscene camera (`frame_step::scripted_view`); `None`
    /// while no cutscene timeline owns the camera.
    pub(super) fn resolve_cutscene_camera(&mut self) -> Option<CutsceneCam> {
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
            // The shot is the live camera globals, which the op-`0x45` beats
            // and the mover (`FUN_801DC0BC`) wrote this tick
            // (`frame_step::scripted_view`).
            let target = legaia_engine_camera::psx_camera::FieldCameraView {
                focus,
                pitch,
                yaw,
                roll,
                h,
                tr_eye,
            };
            let v = legaia_engine_field::frame_step::scripted_view(&self.session.camera, target);
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

/// The record VAs `LEGAIA_DIAG_CAST_HIDE` names (comma list, hex with or
/// without `0x`), parsed once; `None` when it is unset.
fn diag_cast_hide() -> Option<&'static Vec<u32>> {
    static HIDE: std::sync::OnceLock<Option<Vec<u32>>> = std::sync::OnceLock::new();
    HIDE.get_or_init(|| {
        let v = std::env::var("LEGAIA_DIAG_CAST_HIDE").ok()?;
        Some(
            v.split(',')
                .filter_map(|t| {
                    let t = t.trim();
                    let t = t.strip_prefix("0x").or(t.strip_prefix("0X")).unwrap_or(t);
                    u32::from_str_radix(t, 16).ok()
                })
                .collect(),
        )
    })
    .as_ref()
}
