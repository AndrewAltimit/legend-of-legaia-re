//! The zone follow camera: the per-frame follow tick, shot application and global composition.

use super::*;

impl Camera {
    /// One frame of the **zone-driven follow camera** - the engine side of
    /// retail's `FUN_801DE3E0` (tile query + load), `FUN_801DAB90`
    /// (compose), `FUN_801DB510` (ease) and `FUN_801DB8EC` (snap), all in
    /// [`crate::camera_zone`].
    ///
    /// Retail's query is **script-driven**, and so is the port's: the four
    /// `0x4C` arms ([`CameraZoneRequest`], queued by the field
    /// VM and drained in [`Self::route_camera_events`]), the op-`0x45` LOAD
    /// record, and the player-seat path - which retail runs in code at
    /// `0x801D1FE8..0x801D2014` as exactly the `[4C 39]` sequence, and which
    /// the port arms as [`ZoneFollow::arm_arrival`]. On top of those there
    /// is retail's **per-frame** re-query at `0x801D17FC..0x801D1830`, gated
    /// on scratchpad flag bit `22` (`legaia_engine_core::world::ZONE_REQUERY_FLAG`);
    /// with the bit clear - its state on every mode entry, and in 109 of the
    /// disc's 124 CDNAME scenes - the block simply stays put while the
    /// player walks. The four op-`0x43` ramp registers are folded into the
    /// block as they change, exactly the cells retail's ramps write.
    ///
    /// The composer's floor sample reads the **static** elevation LUT
    /// ([`World::sample_field_floor_height_static`]): `FUN_801DAB90`
    /// swaps the MAN's own ladder (`*(_DAT_8007B898) + 2`, 16 negated
    /// `short`s) into scratchpad `0x1F80035C` around its `FUN_80019278`
    /// call and restores the live rungs after, so a scripted floor-tier bob
    /// never shakes the camera.
    ///
    /// PORT: FUN_801DE3E0
    /// REF: FUN_801DAB90, FUN_801DB510, FUN_801DB8EC, FUN_801DBE9C, FUN_801D1344
    pub(super) fn zone_follow_tick<W: CameraWorld + ?Sized>(&mut self, world: &W, dt: i32) {
        use crate::camera_zone::{ComposeInputs, compose, ease_step, ease_step_i16, snap};
        use crate::field_regions::{RegionTable, refresh_region_attributes, zone_query};
        use crate::register_ramp::{CameraRegisterFile, RampSlot};

        let Some((x, y, z)) = world.player_world_pos() else {
            return;
        };
        // The player's tile in the two conventions retail uses: the seat
        // path and every `0x4C` arm take `(coord - 0x40) >> 7` (the exact
        // inverse of the tile-centre seat `tile * 0x80 + 0x40`), while the
        // per-frame re-query at `0x801D1804..0x801D181C` takes
        // `(coord + 0x40) >> 7` - one tile further on. Both are reproduced
        // rather than unified, because they select different records along a
        // region edge.
        let tile = ((x - 0x40) >> 7, (z - 0x40) >> 7);
        let frame_tile = ((x + 0x40) >> 7, (z + 0x40) >> 7);
        let requery_per_frame = world.camera_zone_requery_per_frame();
        let pending = std::mem::take(&mut self.zone.pending);
        let zone = &mut self.zone;

        // 1. The attribute box. Retail latches it in `FUN_800180EC`, which
        //    runs from the sub-area rebuild sweep `FUN_80017DD4` (and from
        //    the `[4C 3D]` arm); the port refreshes it whenever the player
        //    changes tile, which is the same box on every library state.
        let table = RegionTable::parse(world.map_region_block());
        if zone.tile != Some(tile) || zone.reload_pending {
            let (_, attrs) = refresh_region_attributes(table.as_ref(), tile.0, tile.1, false);
            zone.attrs = attrs;
            zone.tile = Some(tile);
        }

        // 2. The zone query + block load (`FUN_801DE3E0`). Never on a bare
        //    tile crossing: only on the seat / arrival, on a queued script
        //    arm, or while the per-frame re-query flag is raised.
        let load_at = |zone: &mut ZoneFollow, tx: i32, tz: i32| {
            let attrs = zone.attrs;
            let hit = zone_query(world.zone_table(), table.as_ref(), &attrs, tx, tz)
                .and_then(|r| r.record)
                .and_then(|r| <[u8; crate::field_regions::ZONE_RECORD_STRIDE]>::try_from(r).ok());
            match hit {
                Some(rec) => zone.load_record(&rec),
                None => {
                    zone.config.load_zone_miss();
                    zone.loaded_record = None;
                    zone.ramp_seen = CameraRegisterFile::DEFAULTS;
                }
            }
        };
        if zone.reload_pending {
            load_at(zone, tile.0, tile.1);
            zone.reload_pending = false;
        } else if requery_per_frame {
            load_at(zone, frame_tile.0, frame_tile.1);
        }
        for req in pending {
            use CameraZoneRequest as R;
            match req {
                R::QueryAtPlayer => load_at(zone, tile.0, tile.1),
                R::QueryAtTile { x: tx, z: tz } => load_at(zone, i32::from(tx), i32::from(tz)),
                R::QueryConformAndSnap => {
                    load_at(zone, tile.0, tile.1);
                    zone.snap_pending = true;
                }
                R::SnapAndClamp => zone.snap_pending = true,
                R::RefreshAttributes => {
                    let (_, a) = refresh_region_attributes(table.as_ref(), tile.0, tile.1, false);
                    zone.attrs = a;
                }
            }
        }

        // 3. Ramp registers (`FUN_80037018` stores through `+0x94`) land in
        //    the block's own cells.
        if world.camera_registers().written() {
            for slot in RampSlot::ALL {
                let v = world.camera_registers().get(slot);
                if v != zone.ramp_seen[slot.index()] {
                    zone.ramp_seen[slot.index()] = v;
                    match slot {
                        RampSlot::Dat8007B60C => zone.config.pitch = v,
                        RampSlot::Dat8007B610 => zone.config.yaw = v,
                        RampSlot::Dat8007B614 => zone.config.depth = v,
                        RampSlot::Dat8007B618 => zone.config.h = v,
                    }
                }
            }
        }

        // The ease's two hold gates (`0x801DB550` / `0x801DB564`): the
        // follow switch off, or scratchpad `0x1F800394 & 0x400` (the
        // top-view freeze, also the settle's bind block), sends the frame to
        // the pin leg `0x801DB820` - focus = `-player`, no compose, no ease,
        // no mode-5 focus ease - which falls into the shake tail. The arrival
        // snap waits (retail's `FUN_801DB8EC` tests the switch too).
        let story = world.story_flags();
        // The player tick's own gate in front of the ease (`FUN_801D1344`,
        // `0x801D1634..0x801D16C0`, branch at `0x801D17DC`): a
        // movement-locked player (`+0x10 & 0x80000` - a conversation, a
        // scripted walk, a seat) gets only the shake call `FUN_801D9D30`, so
        // nothing composes, eases, pins the focus or clamps it until the lock
        // lifts, unless `0x1F800394 & 0x10000` lets the ease through. The
        // snap is not behind it (the arrival actor and the script arms call
        // `FUN_801DB8EC` themselves), and neither are the two callers that
        // run the ease on their own: the ledge hop's phase tick
        // (`FUN_801D2298`) and the arc release watcher (`FUN_801D5D60`).
        let locked = world.field_player_movement_locked()
            && !world.ledge_hop_active()
            && !world.player_script_arc_live();
        let gated =
            locked && story & CAMERA_LOCKED_EASE_FLAG == 0 && !zone.snap_pending && zone.active;
        let hold = !gated && (!zone.follow_enabled || story & CAMERA_HOLD_FLAG != 0);
        if hold {
            zone.prev_player = Some([x, y, z]);
            let g = &mut self.globals.0;
            g[6] = -x;
            g[8] = -z;
        }

        // 4. Compose (`FUN_801DAB90`).
        let inputs = ComposeInputs {
            player: [x, y, z],
            // The static ladder, not the live one: the composer swaps the
            // MAN's own rungs in around its floor sample.
            floor_y: world.sample_field_floor_height_static(x, z),
            attr_box: zone.attrs.box_bytes,
            live_pitch: self.globals.0[0],
            live_yaw: self.globals.0[1],
            half_eye_y: world.scene_save_allowed(),
        };
        if !hold && !gated {
            let composed = compose(&zone.config, &inputs);
            if let Some(ly) = composed.live_yaw {
                self.globals.0[1] = i32::from(ly);
            }
            zone.target = composed.target;
        }

        // 5. Snap on arrival (`FUN_801DB8EC`), else ease on a frame the
        //    player moved (`FUN_801DB510`'s settle test).
        let moved = zone.prev_player != Some([x, y, z]);
        zone.prev_player = Some([x, y, z]);
        let mode5 = zone.config.mode_nibble() == 5;
        let focus_anchor = [
            -(zone.config.anchor_x << 7) - 0x40,
            -(zone.config.anchor_z << 7) - 0x40,
        ];
        let t = zone.target;
        let g = &mut self.globals.0;
        // The free-roam writeback in [`Camera::tick_globals`] pins the focus
        // onto the player every frame. Retail's ease does that too
        // (`0x801DB820`: focus = `-player`), but only when the shot is not
        // mode 5: `0x801DB724..0x801DB734` sends a mode-5 frame to the focus
        // ease instead, which starts from the focus the previous frame left,
        // and a mode-5 frame the player did not move on skips both. So a
        // fixed shot gets its own focus back before it eases.
        if mode5
            && zone.active
            && !hold
            && !gated
            && let Some([fx, fz]) = zone.focus_held
        {
            g[6] = fx;
            g[8] = fz;
        }
        // The stationary test (`0x801DB578..0x801DB5A4`): an unmoved player
        // skips the compose-and-ease unless scratchpad `0x1F800394 &
        // 0x40000` forces it - field-VM `2E 12` raises the bit for the
        // scenes whose camera must keep settling on a standing player.
        let forced = story & CAMERA_FORCE_EASE_FLAG != 0;
        let mut snapped = false;
        if gated || hold {
            // Shake only / pin leg: nothing more until the shake tail.
        } else if zone.snap_pending {
            snapped = true;
            let (p, yw, eye, h) = snap(&t);
            g[0] = p;
            g[1] = yw;
            g[3..6].copy_from_slice(&eye);
            g[9] = h;
            if mode5 {
                g[6] = focus_anchor[0];
                g[8] = focus_anchor[1];
            } else {
                g[6] = -x;
                g[8] = -z;
            }
            zone.snap_pending = false;
        } else if moved || forced || !zone.active {
            let code = zone.config.ease_shift();
            for _ in 0..dt.clamp(1, 8) {
                g[0] = i32::from(ease_step_i16(g[0] as i16, t.pitch, code));
                g[1] = i32::from(ease_step_i16(g[1] as i16, t.yaw, code));
                g[9] = i32::from(ease_step_i16(g[9] as i16, t.h, code));
                for (axis, e) in t.eye.iter().enumerate() {
                    g[3 + axis] = ease_step(g[3 + axis], i32::from(*e), code);
                }
                if mode5 {
                    g[6] = ease_step(g[6], focus_anchor[0], code);
                    g[8] = ease_step(g[8], focus_anchor[1], code);
                }
            }
            // Any shot but a mode-5 fixed one falls into the pin leg
            // `0x801DB820` after the ease (`bne` at `0x801DB734`): the focus
            // goes onto the player on exactly the frames the ease ran.
            if !mode5 {
                g[6] = -x;
                g[8] = -z;
            }
        }

        // 5b. The ease's shake arm (`FUN_801DB510` head `0x801DB51C..0x801DB55C`
        //     and tail `0x801DB844..0x801DB8D4`): the previous jitter pair
        //     comes back out of eye X / Y, and with a non-zero amplitude
        //     `_DAT_8007B630` (field-VM `[4C 84 amp]`) two fresh draws go in
        //     - X centred on zero, Y upward only. Retail runs it on every
        //     call, moved or not, snapped or not, so it sits outside the
        //     ease branch; the arithmetic is `FUN_801D9D30`'s, which the
        //     ease tail duplicates verbatim.
        {
            let mut eye = [g[3], g[4]];
            legaia_engine_camera::battle_camera::apply_shake(
                &mut eye,
                &mut zone.shake_offset,
                u32::from(world.camera_shake_amplitude()),
                &mut zone.shake_seed,
            );
            g[3] = eye[0];
            g[4] = eye[1];
        }

        // 6. The focus edge clamp (`FUN_801DAA50`), which every retail
        //    caller of the ease and the snap runs immediately after them.
        //    Keeps the focus inside the latched walk region widened by the
        //    camera's visible-tile window, so the lens never pans past a
        //    room's edge. A non-zero script focus override (`_DAT_8007B628`
        //    / `_DAT_8007B62A`, op `4C CF`) then replaces its axis.
        if gated {
            zone.focus_held = Some([g[6], g[8]]);
            return;
        }
        let clamped = crate::camera_zone::clamp_focus(
            [g[6], g[8]],
            zone.config.mode_nibble(),
            zone.attrs.kind != 0,
            zone.attrs.box_bytes,
            zone.view_window,
            world.scene_save_allowed(),
            world.script_focus_override(),
        );
        g[6] = clamped[0];
        g[8] = clamped[1];
        if zone.seat_hold.is_some_and(|h| h.player != [x, y, z]) {
            zone.seat_hold = None;
        }
        if snapped {
            let seat = (
                zone.seat_focus.take(),
                zone.seat_eye.take(),
                zone.seat_angles.take(),
            );
            let landed = match seat {
                (None, None, None) => zone.seat_hold.filter(|h| h.target == t),
                (focus, eye, angles) => {
                    let h = super::state::SeatHold {
                        player: [x, y, z],
                        target: t,
                        focus,
                        eye,
                        angles,
                    };
                    zone.seat_hold = Some(h);
                    Some(h)
                }
            };
            if let Some(h) = landed {
                if let Some([fx, fz]) = h.focus {
                    g[6] = fx;
                    g[8] = fz;
                }
                if let Some(eye) = h.eye {
                    g[3..6].copy_from_slice(&eye);
                }
                if let Some([p, yw, hh]) = h.angles {
                    g[0] = p;
                    g[1] = yw;
                    g[9] = hh;
                }
            }
        }
        zone.focus_held = Some([g[6], g[8]]);
        zone.active = true;
    }

    /// Op-`0x45` APPLY: hand the camera back to the **follow shot**.
    ///
    /// Retail's arm (`0x801DF210` in the field overlay) composes the zone
    /// camera for the player into the staging block (`FUN_801DAB90(player,
    /// 0x801C6EA8)`), runs the focus edge clamp over the staged focus
    /// (`FUN_801DAA50`), and hands the block to `FUN_801DE084` - the same
    /// commit the CONFIGURE arm makes: with a zero trigger every staged axis
    /// is copied straight into the camera globals and the live movers are
    /// cancelled; otherwise a mover glides the globals there over `trigger`
    /// frames with `mode` as its curve. It changes no camera mode: the
    /// follow ease keeps running from the player's handler afterwards.
    ///
    /// So APPLY is where a script **releases** a shot it staged, and several
    /// scenes run it from a looping record every frame to keep the camera
    /// pinned on the composed follow pose. Reading it as "commit the staged
    /// shot and go cinematic" froze the camera on the terrain-less fallback
    /// pose (`H = 512`, the zone-miss depth) in every such scene.
    ///
    /// A zero trigger arms the follow camera's snap, which composes, clamps
    /// and copies on this frame's [`Camera::tick`]; a non-zero trigger
    /// composes now and arms the mover toward that pose.
    ///
    /// PORT: FUN_801DE084
    /// REF: FUN_801DAB90, FUN_801DAA50
    pub(super) fn apply_follow_shot<W: CameraWorld + ?Sized>(
        &mut self,
        world: &W,
        apply_trigger: i16,
        mode: u8,
    ) {
        self.mode = CameraMode::Follow;
        self.script_owns_focus = false;
        if apply_trigger == 0 || !zone_camera_scene(world) {
            self.mover = None;
            self.zone.snap_pending = true;
            return;
        }
        // Bring the block up to date (an arrival still pending re-queries the
        // tile) before composing from it.
        self.prime_zone_before_script(world);
        let Some((target, live_yaw)) = self.compose_follow_globals(world) else {
            self.mover = None;
            self.zone.snap_pending = true;
            return;
        };
        if let Some(ly) = live_yaw {
            self.globals.0[1] = i32::from(ly);
        }
        let mut mv = self.mover.take().unwrap_or_default();
        mv.arm(self.globals.0, target, apply_trigger as u16, mode);
        self.mover = Some(mv);
    }

    /// The ten camera globals the follow camera's snap would write for the
    /// player's current position: the composed pitch / yaw / eye trio / `H`
    /// (`FUN_801DAB90`), the focus on the player (a mode-5 shot's on its
    /// anchor tile) with the edge clamp (`FUN_801DAA50`) applied, and focus
    /// Y at `0` (`FUN_801DE084` clears it), plus the live yaw the mode-4
    /// composer rewrites (see [`crate::camera_zone::Composed::live_yaw`]).
    /// `None` without a player actor.
    fn compose_follow_globals<W: CameraWorld + ?Sized>(
        &self,
        world: &W,
    ) -> Option<([i32; AXIS_COUNT], Option<i16>)> {
        use crate::camera_zone::{ComposeInputs, compose, snap};
        let (x, y, z) = world.player_world_pos()?;
        let zone = &self.zone;
        let composed = compose(
            &zone.config,
            &ComposeInputs {
                player: [x, y, z],
                floor_y: world.sample_field_floor_height_static(x, z),
                attr_box: zone.attrs.box_bytes,
                live_pitch: self.globals.0[0],
                live_yaw: self.globals.0[1],
                half_eye_y: world.scene_save_allowed(),
            },
        );
        let (p, yw, eye, h) = snap(&composed.target);
        let mut g = self.globals.0;
        g[0] = p;
        g[1] = yw;
        g[3..6].copy_from_slice(&eye);
        g[9] = h;
        let focus = if zone.config.mode_nibble() == 5 {
            [
                -(zone.config.anchor_x << 7) - 0x40,
                -(zone.config.anchor_z << 7) - 0x40,
            ]
        } else {
            [-x, -z]
        };
        let clamped = crate::camera_zone::clamp_focus(
            focus,
            zone.config.mode_nibble(),
            zone.attrs.kind != 0,
            zone.attrs.box_bytes,
            zone.view_window,
            world.scene_save_allowed(),
            world.script_focus_override(),
        );
        g[6] = clamped[0];
        g[7] = 0;
        g[8] = clamped[1];
        Some((g, composed.live_yaw))
    }

    /// Run the arrival snap now if it is still pending in a terrain-bearing
    /// free-roam scene, so a scripted beat that is about to capture the live
    /// globals sees the zone camera's pose rather than the field reset.
    pub(super) fn prime_zone_before_script<W: CameraWorld + ?Sized>(&mut self, world: &W) {
        if self.zone.snap_pending && self.mode == CameraMode::Follow && zone_camera_scene(world) {
            self.zone_follow_tick(world, 1);
        }
    }

    /// The composed follow yaw, PSX 12-bit units (sign-extended from the
    /// live halfword), or `None` while the zone camera is not driving the
    /// frame.
    pub fn zone_follow_yaw_units(&self) -> Option<i32> {
        self.zone
            .active
            .then_some(i32::from(self.globals.0[1] as i16))
    }
}

/// Whether a world carries the per-scene field terrain the zone camera
/// queries - the walk-region table, the MAN section-3 zone table, or the
/// collision grid the floor sampler reads.
/// Whether the zone-driven follow camera owns this world's walk camera: a
/// walkable scene - the field or the kingdom overworld - with field terrain.
///
/// The overworld is not a separate camera. Retail runs it as an ordinary
/// mode-`0x03` field-run scene through the field overlay's own per-frame
/// chain, and its walk camera is the zone camera: on the three resident
/// overworld states (`keikoku_chest_preload` on `map01`,
/// `sebucus_overworld_resident` on `map02`, `karisto_overworld_resident` on
/// `map03`) the live pitch `0x8007B790`, yaw, eye trio `0x800840B8/BC/C0` and
/// GTE `H` `0x8007B6F4` equal the `FUN_801DAB90` staging descriptor at
/// `0x801F3580` exactly, the block at `0x8007B606` carries `H = 0x170`, and
/// the eye X is the composer's `-(depth >> 7)` on all three.
pub fn zone_camera_scene<W: CameraWorld + ?Sized>(world: &W) -> bool {
    matches!(
        world.scene_mode(),
        legaia_engine_mode::mode::SceneMode::Field | legaia_engine_mode::mode::SceneMode::WorldMap
    ) && has_field_terrain(world)
}

fn has_field_terrain<W: CameraWorld + ?Sized>(world: &W) -> bool {
    !world.zone_table().is_empty()
        || !world.map_region_block().is_empty()
        || !world.collision_grid().is_empty()
}
