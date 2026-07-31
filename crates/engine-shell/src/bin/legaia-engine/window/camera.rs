//! Extracted from `window.rs` (mechanical split; behavior-preserving).

use super::*;

impl PlayWindowApp {
    pub(super) fn camera_mvp(&self, aspect: f32) -> Mat4 {
        // Frame the player's vicinity, not the whole scene. Loading the full
        // town environment-geometry pack makes `scene_aabb` (the union of every
        // mesh's local extent) span thousands of units, so fitting it pulls the
        // orbit camera far enough out that the actually-drawn terrain near the
        // player shrinks to a speck. Until per-mesh world placement lands, build
        // a fixed-size framing box around the player actor (the field draws
        // actor-bound meshes at actor positions) so the view stays close.
        const FIELD_VIEW_HALF: f32 = 700.0;
        // The camera-distance preset (T cycles) widens the framing box, and
        // a mouse drag-orbit swings the vantage - same knobs as the follow
        // camera, so the debug view composes with both.
        let view_half = FIELD_VIEW_HALF * self.session.camera.distance.scale();
        let (lo, hi) = self
            .session
            .host
            .world
            .actors
            .first()
            .filter(|p| p.active || p.tmd_binding.is_some())
            .map(|p| {
                let (cx, cy, cz) = (
                    p.move_state.world_x as f32,
                    p.move_state.world_y as f32,
                    p.move_state.world_z as f32,
                );
                (
                    [cx - view_half, cy - view_half, cz - view_half],
                    [cx + view_half, cy + view_half, cz + view_half],
                )
            })
            .unwrap_or((self.scene_aabb.0, self.scene_aabb.1));
        // Retail's field camera is a *fixed* per-scene 3/4 vantage that follows
        // the player, NOT a spinning orbit. Passing `elapsed_secs` here made the
        // camera rotate continuously (after ~15 s it points up at the sky with
        // the town splayed at the edges). Freeze the orbit angle to a fixed
        // diagonal and steepen the eye height to a town-like overhead pitch. The
        // AABB is still the player-centred box, so the view tracks the player.
        //
        // `orbit_camera_mvp` derives its azimuth from `elapsed_secs *
        // orbit_speed`; feed a constant "time" so the azimuth is fixed at
        // `FIELD_ORBIT_ANGLE`. Height ratio `FIELD_EYE_HEIGHT` sets the pitch
        // (`atan(height) ≈ 40deg`), matching Rim Elm's overhead framing.
        const FIELD_ORBIT_SPEED: f32 = 0.25;
        const FIELD_ORBIT_ANGLE: f32 = 0.75;
        const FIELD_EYE_HEIGHT: f32 = 0.85;
        let angle = FIELD_ORBIT_ANGLE + self.session.camera.manual_orbit;
        let fixed_time = angle / FIELD_ORBIT_SPEED;
        orbit_camera_mvp(
            lo,
            hi,
            FIELD_ORBIT_SPEED,
            FIELD_EYE_HEIGHT,
            fixed_time,
            aspect,
        )
    }

    /// The retail **field follow camera**, parametrized from the town01
    /// anchor savestate's camera globals (see docs/subsystems/cutscene.md for
    /// the global map): pitch `_DAT_8007B790 = 450` (~39.6 deg down-tilt),
    /// yaw `_DAT_8007B792 = -160`, roll 0, GTE `H = _DAT_8007B6F4 = 512`.
    /// The look-at target is the player anchor - retail's follow-cam
    /// (`FUN_801DBE9C`) folds `-(anchor X/Z)` into the focus globals each
    /// frame. The eye-space depth is an engine calibration (retail's exact
    /// field TR composition isn't pinned yet - the offset trio in the
    /// savestate doesn't project to the observed framing); `FIELD_CAM_DEPTH`
    /// is fitted so the player's on-screen height matches the retail frame
    /// (~55 px of 240 for the ~130-unit mesh at H = 512).
    ///
    /// Falls back to the fixed orbit vantage (`camera_mvp`) when no player
    /// actor exists to follow.
    ///
    /// Two user camera knobs compose onto the pinned base: the
    /// camera-distance preset (`T` cycles retail / far / farther) scales the
    /// eye-back depth, and a left-mouse horizontal drag orbits the yaw
    /// around the player (`Camera::manual_orbit`, compass sense - the PSX
    /// render yaw is its negation, so the movement compass fed through
    /// `Camera::compass_azimuth_units` tracks the on-screen view exactly).
    /// Both default to the retail-identical values.
    pub(super) fn field_follow_camera_mvp(&self, aspect: f32) -> Option<Mat4> {
        const PITCH_UNITS: f32 = 450.0;
        const FIELD_H: f32 = 512.0;
        const FIELD_CAM_DEPTH: f32 = 1200.0;
        let cam = &self.session.camera;
        let depth = FIELD_CAM_DEPTH * cam.distance.scale();
        let world = &self.session.host.world;
        let p = world
            .actors
            .first()
            .filter(|p| p.active || p.tmd_binding.is_some())?;
        let (wx, wz) = (p.move_state.world_x, p.move_state.world_z);
        // Anchor the look-at to the floor under the player, not the actor's
        // raw Y: `follow_terrain_height` is opt-in, so `world_y` is usually 0
        // while the town ground sits at a LUT-elevated tier - targeting y=0
        // there points the camera under the ground. The sampler returns the
        // retail-convention tier (up = negative, matching the placement world
        // Y); the caller composes `FIELD_WORLD_FLIP` onto this camera, which
        // cancels `psx_camera_mvp`'s internal pre-flip, so the whole
        // composition (including this target) runs on RAW retail Y-down
        // world coordinates - exactly the retail GTE model.
        let floor_y = world.sample_field_floor_height(wx as i32, wz as i32);
        let target = Vec3::new(wx as f32, floor_y as f32, wz as f32);
        let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
        // PSX camera yaw is the compass negation (`alpha = -psi`): a
        // positive manual orbit (compass sense) subtracts from the render
        // yaw. Base yaw is the pinned FIELD_FOLLOW_YAW_UNITS.
        let yaw = to_rad(FIELD_FOLLOW_YAW_UNITS) - cam.manual_orbit;
        Some(Self::psx_camera_mvp(
            to_rad(PITCH_UNITS),
            yaw,
            // The field follow camera never rolls (`FUN_80025C24` seeds the
            // roll global to `0` on scene entry and only an op-`0x45` beat
            // writes it).
            0.0,
            FIELD_H,
            Vec3::new(0.0, 0.0, depth),
            target,
            aspect,
        ))
    }

    /// Battle camera: frame the **monster** actors (the ones carrying a bound
    /// mesh + idle animation) rather than the player vicinity. The live-loop
    /// seats battle actors at the retail stage seats around the world origin
    /// (`enter_battle` - party at negative Z, monsters at positive Z, from
    /// the `battle_seats` tables), far from the field player's world coords,
    /// so `camera_mvp`'s player-centred box leaves the enemies entirely
    /// off-screen. Framing the enemy cluster (gently orbiting) puts the
    /// animated monsters centre-frame and at a useful size.
    pub(super) fn battle_camera_mvp(&self, aspect: f32) -> Mat4 {
        let world = &self.session.host.world;
        let pc = world.party_count as usize;
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        let mut any = false;
        for (i, a) in world.actors.iter().enumerate() {
            // Monster slots only (party occupies slots 0..party_count and isn't
            // mesh-bound in the play-window battle path anyway).
            if i < pc || a.tmd_binding.is_none() {
                continue;
            }
            let p = [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ];
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            any = true;
        }
        if !any {
            // No bound monsters yet - fall back to the field framing.
            return self.camera_mvp(aspect);
        }
        // The bare position box collapses to a point/line; expand it to enclose
        // the monster mesh bodies (a few hundred units tall/wide).
        const M: f32 = 450.0;
        for k in 0..3 {
            lo[k] -= M;
            hi[k] += M;
        }
        // Gentle orbit (slower than the field's 0.25) so the animated enemies
        // read in 3D from several angles without spinning fast.
        orbit_camera_mvp(lo, hi, 0.12, 0.35, self.win.elapsed_secs(), aspect)
    }

    /// Advance the phase-scripted battle camera for this frame. Creates the
    /// state on stage-dome battle entry (snapped to the entry phase's
    /// framing), derives the retail phase from the live battle state -
    /// **Dialogue** while an in-battle box is up, **Submenu** while a
    /// command / arts / spell / item menu owns the pad, **Menu** otherwise -
    /// and steps the pose on the retail display-frame clock
    /// (`World::field_frames`; one camera step per 2 vsyncs). Dropped
    /// outside stage-dome battles so the next battle re-snaps.
    /// See [`super::battle_cam`] for the measured phase law + provenance.
    pub(super) fn tick_battle_camera(&mut self) {
        let world = &self.session.host.world;
        let active = world.mode == SceneMode::Battle && self.battle_stage_mesh.is_some();
        let inputs = battle_cam_inputs(world);
        // The create / retarget / phase-change / step ordering is the shared
        // `drive` - the browser play page runs the identical call, so the two
        // hosts cannot diverge on the phase script.
        legaia_engine_vm::battle_cam_script::drive(
            &mut self.battle_camera,
            active,
            inputs,
            world.field_frames,
        );
    }

    /// The X/Z bounding box of the actors the far battle framing encloses -
    /// retail's `min`/`max` walk over `actor[+0x34]` / `actor[+0x38]` for
    /// every **present** actor (`FUN_801D5854` case 9 gates on the presence
    /// halfword `actor[+0x14c]`; this host's equivalent is an actor that is
    /// either a party row or a mesh-bound monster). `None` when no actor
    /// qualifies, which is retail's un-entered accumulator case.
    fn battle_formation_box(
        world: &legaia_engine_core::world::World,
    ) -> Option<super::battle_cam::FormationBox> {
        let pc = world.party_count as usize;
        let mut bbox: Option<super::battle_cam::FormationBox> = None;
        for (i, a) in world.actors.iter().enumerate() {
            if !(i < pc || a.tmd_binding.is_some()) {
                continue;
            }
            super::battle_cam::FormationBox::extend(
                &mut bbox,
                a.move_state.world_x as f32,
                a.move_state.world_z as f32,
            );
        }
        bbox
    }

    /// The **exact** retail overworld-battle camera (game mode `0x15`), pinned
    /// from the four fingerprinted `overworld_battle_bg_angle_*` saves and
    /// `FUN_80026988`/`FUN_80026f50`. For a PSX (Y-down) world vertex `v` retail
    /// computes `screen = H * (R*v + TR) / Ze` with
    ///   `R  = Rx(pitch=32u) * Ry(yaw)`         (12-bit angles, 4096 = 360°),
    ///   `TR = (0, 1280, 7680)`                 (eye-space: depth 7680, height 1280),
    ///   `H  = 256`                             (GTE projection focal length),
    /// the look-at target is the world origin, and PSX screen `+Y` is **down**
    /// with screen-centre `(160, 120)` over the 320x240 frame.
    ///
    /// The engine draws its meshes Y-flipped (`scale(1,-1,1)` = `F`, PSX Y-down
    /// -> renderer Y-up), so this builds `cam = Proj_H * T(TR) * R * F`: every
    /// battle draw is `cam * model` where `model` already carries an `F` (the
    /// dome's plain flip, the actors' `Translate * F`), and `F*F = I` recovers
    /// the raw PSX vertex the retail transform expects. Verified by projecting
    /// PROT 88's dome through this matrix and matching the savestate framebuffer
    /// (sky / mountain-ring / horizon). See `project_battle_camera_re`.
    /// The exact retail dome projection (`tr = (0,1280,7680)`), kept as the
    /// camera-RE reference and the regression-test target. The live battle uses
    /// the unified [`battle_dome_camera_mvp`] (closer depth) for a coherent
    /// single-camera scene; this stays as the pinned ground truth.
    #[allow(dead_code)]
    pub(super) fn retail_battle_mvp(yaw_rad: f32, aspect: f32) -> Mat4 {
        Self::battle_mvp_with_tr(yaw_rad, Vec3::new(0.0, 1280.0, 7680.0), aspect)
    }

    /// The shared battle projection-times-view for a given eye-space translation
    /// `tr`. Retail keeps a single rotation `R = Rx(32u)·Ry(yaw)` (stored
    /// rotation-only in `DAT_8007bf10`) and applies the translation per draw
    /// class: the backdrop gets `tr = (0, 1280, 7680)` (pushed far), the actors
    /// get their own (closer) translation off the rotation-only matrix
    /// ([`FUN_80048A08`] composes each actor's world transform onto `8007bf10`,
    /// NOT onto the backdrop's `7680`-deep matrix). Sharing `R` keeps the
    /// foreground and backdrop orbiting in lock-step; the differing `tr.z`
    /// is what lets the party read large while the dome sits on the horizon.
    pub(super) fn battle_mvp_with_tr(yaw_rad: f32, tr: Vec3, aspect: f32) -> Mat4 {
        const PITCH_UNITS: f32 = 32.0;
        let pitch = PITCH_UNITS / 4096.0 * std::f32::consts::TAU;
        Self::psx_camera_mvp(pitch, yaw_rad, 0.0, 256.0, tr, Vec3::ZERO, aspect)
    }

    /// Shared PSX-projection camera: `screen = H * (R*(v - target) + tr) / Ze`
    /// with `R = Rx(pitch)·Ry(yaw)·Rz(roll)` (the retail GTE camera-rotation
    /// build `FUN_8001CF50`), `tr` the post-rotation eye-space translation, `target`
    /// the world-space look-at (retail folds it into the GTE translation as
    /// the negated focus trio `_DAT_80089118/1C/20`), and `H` the GTE
    /// projection register (`_DAT_8007B6F4`). The battle camera is this with
    /// `target = origin`, `H = 256`; the field camera drives `target` from
    /// the player anchor with the savestate-pinned angle globals.
    ///
    /// The engine draws its meshes Y-flipped (`scale(1,-1,1)` = `F`, PSX
    /// Y-down -> renderer Y-up); every draw's `model` carries an `F`, and
    /// `F*F = I` recovers the raw PSX vertex the retail transform expects.
    pub(super) fn psx_camera_mvp(
        pitch_rad: f32,
        yaw_rad: f32,
        roll_rad: f32,
        h: f32,
        tr: Vec3,
        target: Vec3,
        aspect: f32,
    ) -> Mat4 {
        // Retail's own composition order, `Rx * Ry * Rz` with pitch outermost
        // (`FUN_8001CF50` post-multiplies each axis factor onto the resident
        // rotation; `RotMatrixX` at `0x800461A4`, `RotMatrixY` at `0x8004629C`,
        // `RotMatrixZ` at `0x8004638C`). `glam`'s `from_rotation_*` build the
        // same right-handed factors as the crate's own q3.12 rendition
        // `legaia_engine_render::gte::math::camera_view_rotation`, so this is
        // that product in float. Roll (op-`0x45` slot 2, `_DAT_8007B794`) is
        // authored by retail - eight scenes stage a non-zero one; see
        // `crates/engine-core/tests/thread_camera_roll_execution.rs`.
        let r = Mat4::from_rotation_x(pitch_rad)
            * Mat4::from_rotation_y(yaw_rad)
            * Mat4::from_rotation_z(roll_rad);
        let t = Mat4::from_translation(tr);
        let f = Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0));
        // PSX perspective onto a 320x240 frame: ndc.x = H*Ex/(160*Ez),
        // ndc.y = -H*Ey/(120*Ez) (PSX +Y down -> NDC up), clip.w = Ez, depth
        // mapped to wgpu [0,1]. Correct X for non-4:3 viewports so the 4:3
        // retail framing holds at any window size.
        // Near stays at the retail-ish 4 units (it is what governs depth
        // precision); the far plane is the engine-wide `SCENE_FAR`. The old
        // 60 000 looked generous in the 1x field frame but the **overworld
        // walk** composes a 6x world scale onto this camera, so 60 000 eye
        // units is only ~8.5 k world units of draw distance: continent
        // terrain past that was silently far-clipped and popped in as the
        // player walked toward it. Nothing is distance-culled now.
        let (near, far) = (4.0f32, legaia_engine_render::window::SCENE_FAR);
        let a = far / (far - near);
        let b = -near * far / (far - near);
        let aspect_fix = (4.0 / 3.0) / aspect.max(0.01);
        let proj = Mat4::from_cols(
            Vec4::new(h / 160.0 * aspect_fix, 0.0, 0.0, 0.0),
            Vec4::new(0.0, -h / 120.0, 0.0, 0.0),
            Vec4::new(0.0, 0.0, a, 1.0),
            Vec4::new(0.0, 0.0, b, 0.0),
        );
        proj * t * r * Mat4::from_translation(-target) * f
    }

    /// The single battle camera, used for **everything** in a stage-dome
    /// battle: the RETAIL phase-scripted camera. Rotation trio `0x8007B790`
    /// = `(pitch, yaw, 0)`, GTE `H = 256`, translation trio `0x800840B8` -
    /// all four driven per battle phase by [`super::battle_cam`] (dialogue
    /// close-up / far menu framing with the idle orbit / per-character
    /// submenu close-up, with the measured glides between), stepped by
    /// [`Self::tick_battle_camera`]. The static-projection composition was
    /// verified to 0.0002 px against the savestate framebuffer
    /// (`retail_battle_mvp_matches_psx_projection`).
    ///
    /// The ACTORS ride the same rotation but with the **4.0x uniform world
    /// scale** base matrix `0x8007BF10` (`16384 * I`; GTE `4096` = 1.0)
    /// composed under it (`FUN_80048A08` multiplies the camera matrix per
    /// actor) - that scale is what makes the small battle meshes (130-370
    /// units) read at retail size against the far 7680-deep translation
    /// (`256 * 4*370 / 7680` = ~49 px). The draw branch composes
    /// [`BATTLE_WORLD_SCALE`] onto this camera for the actor + battle-FX
    /// draws only, superseding the old DEPTH=1500 single-camera compromise
    /// (and its "separate close actor matrix" reading).
    pub(super) fn battle_dome_camera_mvp(&self, aspect: f32) -> Mat4 {
        let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
        // First frame before the tick armed the state: the shared BOOT_POSE
        // (far framing at its minimum depth, on the origin) - the same
        // fallback the browser host renders.
        let p = self
            .battle_camera
            .as_ref()
            .map(|c| c.pose())
            .unwrap_or(legaia_engine_vm::battle_cam_script::BOOT_POSE);
        let (pitch, yaw, tr, focus) = (p.pitch, p.yaw, Vec3::from(p.tr), Vec3::from(p.focus));
        // The focus trio is the world point the camera orbits (retail stores
        // it negated at `0x80089118/1C/20`). Retail folds the battle world
        // scale into the camera rotation, so the focus is scaled with the
        // geometry it targets; the engine composes [`BATTLE_WORLD_SCALE`] on
        // the actor draws *after* this camera, so the focus has to be
        // pre-scaled by the same factor to land on the same eye-space point.
        let focus = focus * super::BATTLE_WORLD_SCALE;
        // The battle camera's rotation trio is `(pitch, yaw, 0)` - the battle
        // phase script never stages a roll.
        Self::psx_camera_mvp(to_rad(pitch), to_rad(yaw), 0.0, 256.0, tr, focus, aspect)
    }

    /// Camera parameters for the cutscene shot, decoded from the cutscene
    /// timeline's executed op-`0x45` Camera Configure params (read from
    /// `World::camera_state`, committed by `FUN_801DE084`). Returns
    /// `(focus, pitch_radians, yaw_radians, h, tr_eye)` - the inputs to the
    /// retail PSX GTE camera `screen = H * (R*(v - focus) + tr_eye) / Ze`
    /// (see [`Self::psx_camera_mvp`]), the SAME model the field follow camera
    /// uses. Provenance (`FUN_800172c0` view builder + `FUN_801DE084` apply):
    ///
    /// - **focus**: the world look target. Retail stores the *negated* focus
    ///   X / Z in params 6 / 8 (`_DAT_80089118` / `_DAT_80089120`; the
    ///   follow-cam `FUN_801DBE9C` writes `-(anchor X/Z)`), so X / Z are
    ///   negated back to world space; Y (param 7) is un-negated and defaults
    ///   to `0` (retail never writes `_DAT_8008911C`, and the eye-space Y
    ///   offset below - not the focus Y - is what frames the shot vertically).
    /// - **pitch / yaw**: params 0 / 1 (`_DAT_8007B790/92`), PSX `4096` = turn.
    /// - **h**: param 9 (`_DAT_8007B6F4`), written straight to the GTE H
    ///   projection register - the focal length. Passed through unchanged.
    /// - **tr_eye**: the eye-space translation trio, params 3 / 4 / 5
    ///   (`0x800840B8/BC/C0`; the analog of the battle camera's
    ///   `(0, 1280, 7680)`). Retail builds `R` with a 6x world scale folded in
    ///   (base matrix `DAT_8007BF10` = `24576*I`); the engine renders the
    ///   scene geometry at native 1x, so the trio is divided by that scale to
    ///   frame identically (the perspective divide makes 6x-geometry-at-`z`
    ///   and 1x-geometry-at-`z/6` project to the same pixels - the same trick
    ///   `field_follow_camera_mvp`'s `FIELD_CAM_DEPTH = 1200 = 7200/6` uses).
    ///   opdeene supplies all three per beat; the Z component is the eye-back
    ///   depth (raw ~16k-21k across beats).
    pub(super) fn cutscene_view(&self) -> ([f32; 3], f32, f32, f32, f32, [f32; 3]) {
        use std::f32::consts::TAU;
        // Retail folds a 6x uniform world scale into the camera rotation
        // (base matrix `DAT_8007BF10` = `24576*I`); the engine renders at 1x.
        const CUTSCENE_WORLD_SCALE: f32 = 6.0;
        let world = &self.session.host.world;
        let params = &world.camera_state.params;
        let param = |slot: u8| {
            params
                .iter()
                .find(|p| p.slot == slot)
                .map(|p| p.value as i16 as f32)
        };
        let scene_center = [
            (self.scene_aabb.0[0] + self.scene_aabb.1[0]) * 0.5,
            (self.scene_aabb.0[2] + self.scene_aabb.1[2]) * 0.5,
        ];
        let (px, pz) = world
            .actors
            .first()
            .filter(|a| a.active || a.tmd_binding.is_some())
            .map(|a| (a.move_state.world_x as f32, a.move_state.world_z as f32))
            .unwrap_or((scene_center[0], scene_center[1]));
        // Focus X/Z fall back to the lead actor (the cutscene anchor) if a beat
        // hasn't staged them; focus Y follows retail's `0` (the vertical framing
        // rides the eye-space Y offset in `tr_eye`, not the focus).
        let focus = [
            param(6).map(|v| -v).unwrap_or(px),
            param(7).unwrap_or(0.0),
            param(8).map(|v| -v).unwrap_or(pz),
        ];
        let yaw = param(1).map(|v| v / 4096.0 * TAU).unwrap_or(0.0);
        // Slot 0 = op-0x45 camera pitch (`_DAT_8007B790`, GTE RotMatrixX angle,
        // 12-bit / 4096 = 360 deg). Beats that omit it default to the prior
        // fixed ~24 deg downward framing so absent-pitch shots are unchanged.
        let pitch = param(0)
            .map(|v| v / 4096.0 * TAU)
            .unwrap_or_else(|| 0.45f32.atan());
        // Slot 2 = roll (`_DAT_8007B794`, the GTE `RotMatrixZ` angle). Retail
        // authors it: an executing census of the whole MAN corpus finds
        // reachable beats staging a non-zero roll in eight scenes, from a
        // 0.9 deg tilt to -58 deg. A beat that omits the slot holds the
        // scene-entry reset `0` (`FUN_80025C24`).
        let roll = param(2).map(|v| v / 4096.0 * TAU).unwrap_or(0.0);
        // H (focal length) passed straight through; fall back to the field H.
        let h = param(9).filter(|&h| h > 1.0).unwrap_or(512.0);
        // Eye-space translation trio (offset slots 3/4/5), reduced into the
        // engine's 1x frame. Fall back to a mid cutscene depth if a beat omits
        // it (opdeene always supplies all three).
        let s = CUTSCENE_WORLD_SCALE;
        let tr_eye = [
            param(3).unwrap_or(0.0) / s,
            param(4).unwrap_or(1200.0) / s,
            param(5).filter(|&z| z.abs() > 1.0).unwrap_or(17000.0) / s,
        ];
        (focus, pitch, yaw, roll, h, tr_eye)
    }

    /// Replay this frame's drained `apply == 0` Camera Configure beats as
    /// snaps onto the cutscene camera interp (see `pending_camera_snaps`).
    /// Slot decode mirrors [`Self::cutscene_view`] exactly - same negations,
    /// same angle scale, same `CUTSCENE_WORLD_SCALE` reduction, same
    /// degenerate-value filters - so a snapped component's value equals the
    /// glide target `cutscene_view` computes for it (no spurious re-arm).
    pub(super) fn replay_camera_snap_beats(&mut self) {
        use std::f32::consts::TAU;
        const CUTSCENE_WORLD_SCALE: f32 = 6.0;
        let beats = std::mem::take(&mut self.pending_camera_snaps);
        for params in beats {
            let mut comps: Vec<(usize, f32)> = Vec::with_capacity(params.len());
            for p in &params {
                let v = p.value as i16 as f32;
                match p.slot {
                    0 => comps.push((3, v / 4096.0 * TAU)),
                    1 => comps.push((4, v / 4096.0 * TAU)),
                    // Roll rides packed component 9 (see
                    // `CutsceneCameraInterp::ROLL`).
                    2 => comps.push((9, v / 4096.0 * TAU)),
                    3 => comps.push((6, v / CUTSCENE_WORLD_SCALE)),
                    4 => comps.push((7, v / CUTSCENE_WORLD_SCALE)),
                    5 if v.abs() > 1.0 => comps.push((8, v / CUTSCENE_WORLD_SCALE)),
                    6 => comps.push((0, -v)),
                    7 => comps.push((1, v)),
                    8 => comps.push((2, -v)),
                    9 if v > 1.0 => comps.push((5, v)),
                    _ => {}
                }
            }
            self.cutscene_cam_interp.snap_components(&comps);
        }
    }

    pub(super) fn actor_model(&self, slot: usize) -> Mat4 {
        let a = &self.session.host.world.actors[slot];
        let pos = Vec3::new(
            a.move_state.world_x as f32,
            a.move_state.world_y as f32,
            a.move_state.world_z as f32,
        );
        // Field actors carry their heading in `render_26` (PSX 12-bit angle,
        // maintained by the locomotion controller); retail builds the actor
        // matrix from the rotation trio before the per-bone pose is composed
        // onto it (`FUN_8001B964` -> `FUN_80026988`).
        //
        // The engine heading convention is `0` = travel Z+ (atan2), while the
        // rest-pose mesh faces Z- (retail's facing byte stores `0` = Z-: a Z+
        // walk writes `0x800` to `+0x26`, and retail feeds that byte straight
        // into the rotation trio). Compose the half-turn so the model faces
        // its travel direction instead of rendering 180 deg opposite.
        //
        // Y handling is per render frame: BATTLE keeps the per-model
        // `scale(1,-1,1)` (its cameras carry no world negation), while the
        // field frame draws the raw PSX Y-down vertices and lets the camera's
        // `FIELD_WORLD_FLIP` provide the single net negation - so field
        // actor world Y (the retail-convention floor tier) renders at the
        // correct elevation. Yaw is unaffected either way (a Y negation
        // leaves X/Z, and thus the heading, untouched).
        if self.session.host.world.mode == SceneMode::Battle {
            // Monster battle actors face the party (-Z from the +Z seats):
            // the retail Tetsu dialogue close-up (camera at yaw 0 on the
            // party side, looking +Z) shows the monster's FACE, while the
            // archive meshes rest facing +Z - so the enemy side carries the
            // half-turn. Party actors keep their rest orientation (already
            // toward the enemy seats).
            let rot = if a.battle_monster_id.is_some() {
                Mat4::from_rotation_y(std::f32::consts::PI)
            } else {
                Mat4::IDENTITY
            };
            Mat4::from_translation(pos) * rot * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0))
        } else {
            let yaw = std::f32::consts::PI
                + (a.move_state.render_26 as f32) / 4096.0 * std::f32::consts::TAU;
            Mat4::from_translation(pos) * Mat4::from_rotation_y(yaw)
        }
    }
}

/// Derive the shared battle-camera drive inputs from the live world state -
/// phase, acting actor, formation box. Kept as a free function so the
/// cross-host invariant test can feed it a constructed `World`; the browser
/// play page's `derive_battle_cam` (`web-viewer::play_battle_render`) is the
/// same derivation over the same `World` type, pinned to the same expected
/// pose by its mirror test.
///
/// Retail rebuilds the submenu close-up from the acting actor's own record
/// on every open (`FUN_801D5854` case 0), so the framing follows whoever
/// owns the menu rather than staying pinned to the measured solo-Vahn pose:
/// facing drives the yaw, the character id keys the per-model height table,
/// and the actor's world position is the focus the camera orbits.
pub(super) fn battle_cam_inputs(
    world: &legaia_engine_core::world::World,
) -> legaia_engine_vm::battle_cam_script::BattleCamInputs {
    use legaia_engine_vm::battle_cam_script as script;
    let acting_slot = world
        .battle_command
        .as_ref()
        .map(|c| c.actor)
        .unwrap_or(world.battle_ctx.active_actor);
    let phase = script::phase_for(
        world.current_dialog.is_some() || world.inline_dialogue.is_some(),
        world.battle_command.is_some()
            || world.battle_arts_menu.is_some()
            || world.battle_spell_menu.is_some()
            || world.battle_item_menu.is_some(),
        script::action_state_frames_the_action(world.battle_ctx.action_state),
    );
    // The submenu close-up frames whoever owns the menu; the action framing
    // frames whoever is acting. Both are the same `BattleCamActor`.
    let actor_at = |slot: u8, party_slot: Option<u8>| {
        let a = world.actors.get(slot as usize)?;
        // Retail's height key is `DAT_8007BD10[slot]`, the 1-based
        // party-record selector; the engine's party rows are that record
        // order, so the row index + 1 is the same id.
        let height = party_slot.and_then(|p| {
            world
                .battle_camera_heights
                .as_ref()
                .and_then(|t| t.height_for_char_id(p + 1))
                .map(|h| h as f32)
        });
        Some(script::BattleCamActor {
            // The port's actors carry their heading in `render_26` (the
            // retail `+0x26` 12-bit angle); it stands in for the battle
            // actor record's `+0x46` the framing formula subtracts.
            facing: (a.move_state.render_26 as u16 & 0xFFF) as i32,
            world: [
                a.move_state.world_x as f32,
                a.move_state.world_y as f32,
                a.move_state.world_z as f32,
            ],
            height,
        })
    };
    let acting = match world.battle_command.as_ref() {
        Some(c) => actor_at(c.actor, Some(c.party_slot)),
        None => actor_at(acting_slot, None),
    };
    legaia_engine_vm::battle_cam_script::BattleCamInputs {
        phase,
        acting,
        // The far menu framing sizes its depth to - and centres on - the
        // live formation's X/Z bounding box (`FUN_801D5854` case 9).
        formation: PlayWindowApp::battle_formation_box(world),
        action: battle_action_framing(world, acting_slot),
        shake_amplitude: world.camera_shake_amplitude,
    }
}

/// The `FUN_801D5854` case-6 context inputs for the acting slot.
///
/// `party_slot` is retail's `ctx[+0x13] < 3` over the engine's own party
/// band, `char_id` its `DAT_8007BD10[slot]` (the party row + 1), and
/// `depth_raw` is `ctx[+0x6D0]` - the value `camera_height_for_frame`
/// recomputed at the last action seed. `flow_active` is retail's
/// `_DAT_8007BD71 == 0xFE`, the battle-flow SM's in-battle state: the engine
/// has no flow-state byte and only ever runs the camera while a battle is
/// live, so it is constant here. `style` (`ctx[+0xD]`) is unmodelled.
pub(super) fn battle_action_framing(
    world: &legaia_engine_core::world::World,
    acting_slot: u8,
) -> legaia_engine_vm::battle_cam_script::ActionFraming {
    let party = usize::from(acting_slot) < world.party_count as usize;
    legaia_engine_vm::battle_cam_script::ActionFraming {
        party_slot: party,
        flow_active: true,
        depth_raw: world.battle_camera_frame_height as i32,
        yaw_base: 0,
        style: 0,
        char_id: if party { acting_slot + 1 } else { 0 },
    }
}

#[cfg(test)]
mod follow_compass_tests {
    use super::*;
    use std::f32::consts::TAU;

    /// The movement compass must agree with the rendered follow camera at
    /// ANY manual orbit: walking the compass-forward direction
    /// (`alpha = base_bias + manual_orbit`, forward = `(sin a, 0, cos a)`)
    /// must move the player UP the screen (away from the camera), and the
    /// compass-right direction must move screen-right. This is the
    /// engine-shell ground-truth check for `Camera::compass_azimuth_units`'s
    /// sign convention against `psx_camera_mvp` (the same job
    /// `world_map_camera_remap.rs` does for the overworld remap).
    #[test]
    fn compass_forward_walks_up_screen_for_any_manual_orbit() {
        const PITCH_UNITS: f32 = 450.0;
        let to_rad = |units: f32| units / 4096.0 * TAU;
        for manual_orbit in [0.0f32, 0.4, 1.2, 2.5, -0.7, 3.9] {
            // The exact composition `field_follow_camera_mvp` builds
            // (with the caller's FIELD_WORLD_FLIP post-multiply).
            let yaw = to_rad(FIELD_FOLLOW_YAW_UNITS) - manual_orbit;
            let target = Vec3::new(500.0, 0.0, 800.0);
            let mvp = PlayWindowApp::psx_camera_mvp(
                to_rad(PITCH_UNITS),
                yaw,
                0.0,
                512.0,
                Vec3::new(0.0, 0.0, 1200.0),
                target,
                4.0 / 3.0,
            ) * FIELD_WORLD_FLIP;
            // Compass azimuth = render bias + manual orbit (what
            // BootSession feeds World::field_camera_azimuth).
            let alpha = -to_rad(FIELD_FOLLOW_YAW_UNITS) + manual_orbit;
            let forward = Vec3::new(alpha.sin(), 0.0, alpha.cos());
            let right = Vec3::new(alpha.cos(), 0.0, -alpha.sin());
            let project = |p: Vec3| {
                let c = mvp * p.extend(1.0);
                (c.x / c.w, c.y / c.w)
            };
            let (x0, y0) = project(target);
            let (xf, yf) = project(target + forward * 100.0);
            let (xr, yr) = project(target + right * 100.0);
            // NDC +y is up: forward must increase y and stay ~centred in x.
            assert!(
                yf > y0 + 1e-4,
                "orbit {manual_orbit}: forward walks up-screen (dy={})",
                yf - y0
            );
            assert!(
                (xf - x0).abs() < 0.35 * (yf - y0).abs().max(1e-3),
                "orbit {manual_orbit}: forward stays near-vertical (dx={} dy={})",
                xf - x0,
                yf - y0
            );
            // Right must increase x and stay ~level in y.
            assert!(
                xr > x0 + 1e-4,
                "orbit {manual_orbit}: right walks screen-right (dx={})",
                xr - x0
            );
            let _ = yr;
        }
    }
}

#[cfg(test)]
mod battle_cam_shared_tests {
    use super::*;
    use legaia_engine_core::world::{SceneMode, World};
    use legaia_engine_vm::battle_cam_script as script;

    /// The shared projection's far plane must stay paired with the native
    /// renderer's `SCENE_FAR` (the script crate cannot take the wgpu link,
    /// so the constant is duplicated there by design - this is the pin).
    #[test]
    fn scene_far_paired_with_renderer() {
        assert_eq!(script::SCENE_FAR, legaia_engine_render::window::SCENE_FAR);
    }

    /// The shared `battle_vp` must reproduce the native glam composition
    /// (`battle_dome_camera_mvp` = `psx_camera_mvp(pitch, yaw, 0, H=256,
    /// tr, focus * BATTLE_WORLD_SCALE)`) element-for-element: the browser
    /// projects through `battle_vp`, the native window through glam, and
    /// this is what keeps them the same camera.
    #[test]
    fn battle_vp_matches_the_native_glam_composition() {
        let to_rad = |units: f32| units / 4096.0 * std::f32::consts::TAU;
        let poses = [
            script::BOOT_POSE,
            script::BattleCamPose {
                pitch: 32.0,
                yaw: 224.0,
                tr: [0.0, 1280.0, 7680.0],
                focus: [100.0, 0.0, -50.0],
            },
            script::BattleCamPose {
                pitch: 32.0,
                yaw: 2288.0,
                tr: [-512.0, 1152.0, 2457.0],
                focus: [0.0, 0.0, -800.0],
            },
            script::BattleCamPose {
                pitch: 256.0,
                yaw: 4064.0,
                tr: [0.0, 1536.0, 3276.0],
                focus: [-700.0, 0.0, -900.0],
            },
        ];
        for aspect in [4.0f32 / 3.0, 16.0 / 9.0, 1.0] {
            for p in &poses {
                let native = PlayWindowApp::psx_camera_mvp(
                    to_rad(p.pitch),
                    to_rad(p.yaw),
                    0.0,
                    256.0,
                    Vec3::from(p.tr),
                    Vec3::from(p.focus) * BATTLE_WORLD_SCALE,
                    aspect,
                )
                .to_cols_array();
                let shared = script::battle_vp(p, BATTLE_WORLD_SCALE, aspect);
                for (i, (a, b)) in shared.iter().zip(native.iter()).enumerate() {
                    assert!(
                        (a - b).abs() <= 1e-3 * b.abs().max(1.0),
                        "pose {p:?} aspect {aspect} elem {i}: shared {a} vs native {b}"
                    );
                }
            }
        }
    }

    /// **Cross-host single-model pin, native half.** One battle world state
    /// (solo Vahn at the tutorial seat, command menu open, one bound
    /// monster) driven through THIS host's derivation + the shared drive
    /// must land on the literal measured close-up. The browser host's
    /// mirror test (`web_pose_matches_the_native_recipe` in
    /// `web-viewer::play_battle_render`) runs the identical world recipe to
    /// the identical literals, so the two hosts' poses are equal to each
    /// other by transitivity.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn native_pose_matches_the_shared_recipe() {
        let mut world = World::default();
        world.mode = SceneMode::Battle;
        world.party_count = 1;
        let mut vahn = legaia_engine_core::world::Actor::default();
        vahn.active = true;
        vahn.move_state.world_x = 0;
        vahn.move_state.world_y = 0;
        vahn.move_state.world_z = -800;
        vahn.move_state.render_26 = 0;
        let mut tetsu = legaia_engine_core::world::Actor::default();
        tetsu.active = true;
        tetsu.move_state.world_z = 800;
        tetsu.battle_monster_id = Some(1);
        tetsu.tmd_binding = Some(0);
        world.actors = vec![vahn, tetsu];
        world.battle_command = Some(legaia_engine_core::battle_input::BattleCommandSession::new(
            0, 0,
        ));

        let inputs = battle_cam_inputs(&world);
        assert_eq!(inputs.phase, script::BattleCamPhase::Submenu);
        assert_eq!(
            inputs.acting,
            Some(script::BattleCamActor {
                facing: 0,
                world: [0.0, 0.0, -800.0],
                height: None,
            })
        );
        assert_eq!(
            inputs.formation,
            Some(script::FormationBox {
                min: [0.0, -800.0],
                max: [0.0, 800.0],
            })
        );
        let mut slot = None;
        for f in 0..=6u64 {
            script::drive(&mut slot, true, inputs, f * 2);
        }
        let pose = slot.as_ref().unwrap().pose();
        // The measured solo-Vahn submenu close-up, arrived after the 6-step
        // glide - the SAME literals the web mirror test asserts.
        assert_eq!(pose.pitch, 32.0);
        assert_eq!(pose.yaw.rem_euclid(4096.0), 2288.0);
        assert_eq!(pose.tr, [-512.0, 1152.0, 2457.0]);
        assert_eq!(pose.focus, [0.0, 0.0, -800.0]);
    }

    /// Closing the command menu and letting the action SM run must move the
    /// camera onto the acting actor - the whole point of the Action phase.
    /// Driven through THIS host's derivation, so a phase that the shared
    /// script supports but the host never selects would fail here.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn host_derivation_selects_the_action_phase_and_frames_the_actor() {
        let mut world = World::default();
        world.mode = SceneMode::Battle;
        world.party_count = 1;
        let mut vahn = legaia_engine_core::world::Actor::default();
        vahn.active = true;
        vahn.move_state.world_z = -800;
        let mut tetsu = legaia_engine_core::world::Actor::default();
        tetsu.active = true;
        tetsu.move_state.world_z = 800;
        tetsu.battle_monster_id = Some(1);
        tetsu.tmd_binding = Some(0);
        world.actors = vec![vahn, tetsu];

        // Idle between actions: the far framing, not the action framing.
        world.battle_ctx.action_state =
            legaia_engine_vm::battle_action::ActionState::Begin.as_byte();
        assert_eq!(
            battle_cam_inputs(&world).phase,
            script::BattleCamPhase::Menu
        );

        // The strike is executing on party slot 0.
        world.battle_ctx.action_state =
            legaia_engine_vm::battle_action::ActionState::AttackStrike.as_byte();
        world.battle_ctx.active_actor = 0;
        let inputs = battle_cam_inputs(&world);
        assert_eq!(inputs.phase, script::BattleCamPhase::Action);
        assert!(inputs.action.party_slot, "slot 0 is a party seat");
        assert_eq!(inputs.action.char_id, 1, "DAT_8007BD10[0]");

        let mut slot = None;
        for f in 0..=6u64 {
            script::drive(&mut slot, true, inputs, f * 2);
        }
        let pose = slot.as_ref().unwrap().pose();
        let want = script::action_framing(inputs.acting.unwrap(), inputs.action);
        assert_eq!(pose.tr, want.tr);
        assert_eq!(pose.focus, [0.0, 0.0, -800.0], "framed on the attacker");
        // Closer than the far framing this battle idles at.
        let idle = script::menu_framing(inputs.formation, 0.0);
        assert!(pose.tr[2] < idle.tr[2], "{:?} vs {:?}", pose.tr, idle.tr);

        // A monster's turn frames on the computed size-class depth instead.
        world.battle_ctx.active_actor = 1;
        world.battle_camera_frame_height = legaia_engine_vm::battle_formulas::CAMERA_HEIGHT_MAX;
        let mob = battle_cam_inputs(&world);
        assert!(!mob.action.party_slot);
        assert_eq!(
            mob.action.depth_raw,
            legaia_engine_vm::battle_formulas::CAMERA_HEIGHT_MAX as i32,
            "the fallback arm reads ctx[+0x6D0]"
        );
        assert_eq!(mob.action.raw_z(), mob.action.depth_raw);
    }

    /// The field VM's shake opcode reaches the camera through this host.
    #[test]
    #[allow(clippy::field_reassign_with_default)]
    fn host_derivation_carries_the_field_vm_shake_amplitude() {
        let mut world = World::default();
        world.mode = SceneMode::Battle;
        world.party_count = 1;
        let mut vahn = legaia_engine_core::world::Actor::default();
        vahn.active = true;
        world.actors = vec![vahn];
        assert_eq!(battle_cam_inputs(&world).shake_amplitude, 0);
        world.camera_shake_amplitude = 9;
        let inputs = battle_cam_inputs(&world);
        assert_eq!(inputs.shake_amplitude, 9);
        let mut slot = None;
        for f in 0..4u64 {
            script::drive(&mut slot, true, inputs, f * 2);
        }
        let cam = slot.as_ref().unwrap();
        assert_ne!(cam.shake_offset(), [0, 0], "the jitter is live");
        assert_ne!(cam.pose().tr, cam.framing_pose().tr);
    }
}

#[cfg(test)]
mod cutscene_framing_tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Project a raw retail (Y-down) world point through the full cutscene
    /// camera composition (`psx_camera_mvp(..) * FIELD_WORLD_FLIP`, exactly as
    /// `compute_scene_camera`'s cutscene branch builds it) to 320x240 screen
    /// pixels (PSX frame; wgpu NDC, framebuffer origin top-left).
    fn project_cutscene(v: Vec3, tr_eye: Vec3, focus: Vec3) -> (f32, f32) {
        // Intro-beat globals, pinned live from the `new_game_cutscene_intro_a`
        // save state (`0x8007B790`/`0x8007B6F4`/`0x800840B8`/`0x80089118`).
        let pitch = 180.0 / 4096.0 * TAU;
        let yaw = -2967.0 / 4096.0 * TAU;
        let h = 792.0;
        // The captured intro beat's roll global reads `0`.
        let mvp = PlayWindowApp::psx_camera_mvp(pitch, yaw, 0.0, h, tr_eye, focus, 4.0 / 3.0)
            * FIELD_WORLD_FLIP;
        let clip = mvp * v.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        let px = (ndc.x * 0.5 + 0.5) * 320.0;
        let py = (0.5 - ndc.y * 0.5) * 240.0;
        (px, py)
    }

    /// The shell composes the camera rotation in `glam`; retail composes it
    /// through the GTE. This pins the two to the same product, roll included,
    /// so a sign or ordering slip in the third factor cannot pass unnoticed:
    /// `psx_camera_mvp`'s `R` must equal
    /// [`legaia_engine_render::gte::camera_view_rotation`], the q3.12
    /// port of `FUN_8001CF50` (`Rx * Ry * Rz`, pitch outermost).
    #[test]
    fn the_shell_rotation_matches_the_ported_gte_composition_including_roll() {
        use legaia_engine_render::gte::camera_view_rotation;
        // `juui2`'s beat: pitch -643, yaw -1480, roll -660 (12-bit units).
        let to_rad = |u: f32| u / 4096.0 * TAU;
        let (pitch, yaw, roll) = (to_rad(-643.0), to_rad(-1480.0), to_rad(-660.0));
        let want = camera_view_rotation(0, pitch, yaw, roll).expect("no saved-matrix flag");
        let got =
            Mat4::from_rotation_x(pitch) * Mat4::from_rotation_y(yaw) * Mat4::from_rotation_z(roll);
        // `camera_view_rotation` is row-major q3.12; `glam` is column-major
        // f32. Compare element by element through both conventions.
        for r in 0..3 {
            for c in 0..3 {
                let a = f32::from(want.m[r][c]) / 4096.0;
                let b = got.col(c)[r];
                assert!(
                    (a - b).abs() < 2e-3,
                    "R[{r}][{c}]: gte {a} vs glam {b}\nwant {:?}",
                    want.m
                );
            }
        }
        // Dropping the third factor is visibly different, which is what makes
        // this a test of the roll rather than of the pitch/yaw pair.
        let without = Mat4::from_rotation_x(pitch) * Mat4::from_rotation_y(yaw);
        assert_ne!(got.to_cols_array(), without.to_cols_array());
    }

    /// The retail intro shot ("It was the Seru.") frames the party at screen
    /// ~(172, 180) in the 320x240 PSX frame (measured from the intro save
    /// state's framebuffer). The camera's focus tracks the party anchor, so the
    /// focus point itself must land there - regardless of the 6x world scale
    /// (the perspective divide cancels it) - which is exactly what pins the
    /// eye-back depth. This guards against the old heuristic that framed the
    /// shot too close.
    #[test]
    fn intro_focus_projects_to_retail_party_position() {
        // Eye-space translation trio `0x800840B8 = (260, 1293, 17145)`, reduced
        // into the engine's native-1x frame by the 6x world scale.
        let tr_eye = Vec3::new(260.0, 1293.0, 17145.0) / 6.0;
        let focus = Vec3::new(8640.0, 0.0, 10304.0);
        let (px, py) = project_cutscene(focus, tr_eye, focus);
        assert!(
            (px - 172.0).abs() < 3.0,
            "focus X projects to retail ~172, got {px}"
        );
        assert!(
            (py - 180.0).abs() < 3.0,
            "focus Y projects to retail ~180 (lower-centre), got {py}"
        );
    }

    /// A ~133-unit-tall field character standing at the focus (retail-Y-down,
    /// up = negative Y) subtends the retail ~1/6-frame height and renders
    /// upright (head above feet). Confirms the world scale + orientation, not
    /// just the focus position.
    #[test]
    fn intro_character_subtends_retail_height_upright() {
        let tr_eye = Vec3::new(260.0, 1293.0, 17145.0) / 6.0;
        let focus = Vec3::new(8640.0, 0.0, 10304.0);
        let (_, feet_y) = project_cutscene(focus, tr_eye, focus);
        // Head is 133 units UP = negative Y in the retail convention.
        let head = focus + Vec3::new(0.0, -133.0, 0.0);
        let (_, head_y) = project_cutscene(head, tr_eye, focus);
        let height_px = feet_y - head_y;
        assert!(
            head_y < feet_y,
            "head renders above feet (upright): head {head_y} < feet {feet_y}"
        );
        assert!(
            (20.0..48.0).contains(&height_px),
            "character subtends ~retail height (~37 px of 240), got {height_px}"
        );
    }
}
