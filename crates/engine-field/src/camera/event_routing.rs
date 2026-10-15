//! `Camera::route_camera_events`: draining the field event queue into camera state.

use super::*;

impl Camera {
    /// REF: FUN_801DE084
    pub fn route_camera_events<W: CameraWorld + ?Sized>(&mut self, world: &mut W) -> usize {
        // The camera-zone arms of op `0x4C` (nibble-3 sub-8/9/D/E and
        // nibble-C sub-4) queue on the world because the field VM's host is
        // `World` while these globals live here. Both hosts call this right
        // before `tick`, so this is the one drain point.
        self.zone.pending.extend(world.take_camera_zone_requests());
        let mut applied = 0usize;
        let mut leftover = Vec::new();
        for ev in world.drain_field_events() {
            match ev {
                FieldEvent::CameraConfigure {
                    params,
                    apply_trigger,
                    mode,
                } => {
                    // An `apply == 0` beat is retail's immediate snap. Bank
                    // it for the host's cutscene interp before the merge
                    // below folds it into one `camera_state` - see
                    // [`Self::camera_snap_beats`].
                    if apply_trigger == 0 {
                        self.camera_snap_beats
                            .push(params.iter().map(|p| (p.slot, p.value)).collect());
                    }
                    // Op-0x45 slot layout, pinned from the Camera Configure
                    // apply handler `FUN_801DE084` (writes the camera globals)
                    // + the view build `FUN_800172C0` (Euler kernel
                    // FUN_80026988). The 10 slots are three Euler
                    // angles, an offset trio, a focus trio, and H:
                    //   0 = pitch  (`_DAT_8007B790`, RotX)   1 = yaw (RotY)
                    //   2 = roll   (`_DAT_8007B794`, RotZ)   3,4,5 = offset
                    //   6,7,8 = focus (negated translation)  9 = GTE H
                    // Angles are 12-bit (4096 = 360 deg). See
                    // docs/subsystems/cutscene.md.
                    let ang = |v: u16| (v as i16) as f32 * std::f32::consts::TAU / 4096.0;
                    let slot = |s: u8| params.iter().find(|p| p.slot == s).map(|p| p.value);
                    if let Some(v) = slot(0) {
                        self.pitch = ang(v);
                    }
                    if let Some(v) = slot(1) {
                        self.yaw = ang(v);
                    }
                    // Slot 2 = roll (`_DAT_8007B794`, the `RotMatrixZ` angle).
                    // Retail authors it: see [`Self::roll`] for the executing
                    // census that found the eight scenes staging one.
                    if let Some(v) = slot(2) {
                        self.roll = ang(v);
                    }
                    // Focus slots 6/7/8 re-target the cinematic look-at, each
                    // applied INDEPENDENTLY on its own presence - the retail
                    // apply handler `FUN_801DE084` writes each camera focus
                    // global only when its slot bit is set, leaving the others
                    // at their prior value. A beat that supplies only focus X/Z
                    // (opdeene's opening beats omit slot 7 entirely) must still
                    // pan the look-at horizontally rather than freeze it; the
                    // all-or-nothing gate used before never retargeted such
                    // beats, pinning the shot on one angle. The focus globals
                    // are the negated GTE translation, so X/Z are negated back
                    // to a world point (matching the shell's `cutscene_view`).
                    if let Some(fx) = slot(6) {
                        self.look_at[0] = -((fx as i16) as f32);
                    }
                    if let Some(fy) = slot(7) {
                        self.look_at[1] = (fy as i16) as f32;
                    }
                    if let Some(fz) = slot(8) {
                        self.look_at[2] = -((fz as i16) as f32);
                    }
                    // The ten retail globals. Every masked slot writes its
                    // axis; an absent slot holds its prior value, which is
                    // what `FUN_801DE084` does by writing only the slots the
                    // mask selects. `apply_trigger` then chooses between the
                    // two commit behaviours:
                    //
                    // - `apply == 0` - SNAP. Write straight through and mark
                    //   every live mover dead, cancelling a glide in flight.
                    // - `apply != 0` - GLIDE. Hand the one mover actor ten
                    //   `(start, end)` pairs, `start` from the LIVE globals,
                    //   and let it interpolate over `apply` display frames
                    //   with `mode` as the shared ease curve.
                    //
                    // This is the half the camera was missing entirely: the
                    // eye-space translation trio (slots 3/4/5) had no engine
                    // representation at all, so every scripted shot rendered
                    // and traced from the follow orbit's fixed height.
                    self.script_owns_focus = true;
                    // Retail's op-`0x45` arms compose the zone camera into
                    // the same staging struct the beat's slots overwrite
                    // (`FUN_801DAB90(player, 0x801C6EA8)` at `0x801DF228`),
                    // so an unmasked slot holds the scene's own shot, and a
                    // glide starts from it. Snap the zone pose in first if
                    // this scene has not had its arrival snap yet.
                    self.prime_zone_before_script(world);
                    let mut target = self.globals;
                    for p in &params {
                        if (p.slot as usize) < AXIS_COUNT {
                            target.0[p.slot as usize] = (p.value as i16) as i32;
                        }
                    }
                    if apply_trigger == 0 {
                        self.globals = target;
                        self.mover = None;
                    } else {
                        let mut mv = self.mover.take().unwrap_or_default();
                        mv.arm(self.globals.0, target.0, apply_trigger, mode);
                        self.mover = Some(mv);
                    }
                    if std::env::var_os("LEGAIA_DIAG_CAMERA").is_some() {
                        eprintln!(
                            "DIAG camera configure: params={params:?} -> pitch={:.3} yaw={:.3} look_at={:?}",
                            self.pitch, self.yaw, self.look_at
                        );
                    }
                    applied += 1;
                }
                FieldEvent::CameraSave => {
                    // Engine snapshots the current eye/look-at into world.camera_state
                    // already; we just record we saw the event.
                    applied += 1;
                }
                FieldEvent::CameraLoad { payload } => {
                    // Op-`0x45` LOAD: the 18 bytes after the op byte are one
                    // camera-region record, and retail hands them straight to
                    // the camera-config loader (`FUN_801DBC20(operand + 1)`
                    // at `0x801DF28C`). It is not an eye / look-at pair and
                    // it does not seize the camera: the follow camera keeps
                    // running and eases toward the shot the new block
                    // composes.
                    if let Ok(rec) = <[u8; crate::field_regions::ZONE_RECORD_STRIDE]>::try_from(
                        payload.as_slice(),
                    ) {
                        self.zone.load_record(&rec);
                    }
                    applied += 1;
                }
                FieldEvent::CameraApply {
                    apply_trigger,
                    mode,
                } => {
                    self.apply_follow_shot(world, apply_trigger, mode);
                    applied += 1;
                }
                // Op `4C D7`: the mover's dead bit. The globals keep the
                // pose its last tick wrote, so the shot freezes mid-glide.
                FieldEvent::CameraRetireMovers => {
                    self.mover = None;
                    applied += 1;
                }
                // Op-`0x46` `VIEW_WINDOW`, both forms. Retail writes the same
                // four scratchpad bytes `0x1F8003E8..EB` from either arm
                // (`0x801DF2AC..0x801DF350` in the field overlay), and they
                // are the camera's visible-tile window in `[E8, E9, EA, EB]`
                // order - see docs/formats/encounter.md. The scene-entry
                // primer seeds them, the script replaces them, and the focus
                // edge clamp below widens the walk region by whatever is
                // there, so the op has to land here rather than only on the
                // event queue.
                FieldEvent::ViewWindowLong { b1, b2, b3, b4 } => {
                    self.zone.view_window = [b1 as i8, b2 as i8, b3 as i8, b4 as i8];
                    applied += 1;
                }
                FieldEvent::ViewWindowShort { r, g, b, packed } => {
                    // The short form is the same four bytes, already built by
                    // the VM: a window of half-width `op0 >> 1` in X about
                    // tile offset `-1` and `op1 >> 1` in Z about `+2`. The
                    // field's own default window is this form's `(7, 8)` -
                    // X `[-8, 6]`, Z `[-6, 10]` - which is where
                    // `FIELD_DEFAULT_VIEW_WINDOW`'s asymmetry comes from.
                    self.zone.view_window = [r as i8, g as i8, b as i8, packed as i8];
                    applied += 1;
                }
                other => leftover.push(other),
            }
        }
        world.requeue_field_events(leftover);
        // The field actor visibility cull (`FUN_801D79E8`) reads the focus
        // pair, the region box and the visible tile window. They live here,
        // so this drain point - which both hosts reach every frame - is
        // where the world gets them. Published only while the zone follow
        // camera owns them; before its first compose the focus is not the
        // one retail's globals would hold, and a cull against it would hold
        // gliders the player can see. Published after this frame's op-`0x46`
        // events land: retail's op writes the scratchpad window outright, so
        // the frame that runs the op already culls and crops against it (a
        // record's `46 ..` beat one frame late drew its shot against the
        // previous region's window).
        world.set_npc_cull_view(self.zone.active.then(|| FieldCullView {
            focus_stored: [self.globals.0[6], self.globals.0[8]],
            attr_box: self.zone.attrs.box_bytes,
            window: self.zone.view_window,
        }));
        // Publish the live visible-tile window to the fog pool: the ambient
        // emitter samples its burst span from the same scratchpad bytes
        // `0x1F8003E8..EB` every frame (`lb` at `0x801D6158..`), and those
        // bytes live here, not on the world the emitter ticks in. A record
        // load in this frame's `tick` reaches the emitter one frame later.
        world.set_fog_view_window(self.zone.view_window);
        applied
    }
}
