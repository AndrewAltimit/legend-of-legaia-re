//! Field-host bodies for screen tint, colour-intensity setup, effect-animation
//! triggers and camera configuration.

use super::*;

impl FieldHostImpl<'_> {
    // Op `0x4C` outer-nibble-4 subs `0xA..=0xD` - four scene globals written
    // or ramped from one script operand. Subs `0xA`/`0xB`/`0xC` are the three
    // the world-map frame pump `FUN_801D1344` forwards into the horizon
    // emitter gate, which is why the engine parks them on the world-map
    // controller: `_DAT_8007BCD0` (`sw v0,-0x4330(v1)` at `0x801E1648`),
    // `_DAT_8007BCD4` (`0x801E1688`) and `_DAT_8007BCD8` (`0x801E16C8`).
    // Sub `0xD` scales its operand by `_DAT_8008457C >> 12` before storing to
    // `_DAT_8007B910` (`0x801E1700..0x801E1720`); the engine has no consumer
    // for that slot, so it is dropped rather than parked somewhere a reader
    // would then have to be invented for.
    //
    // The ramp arms post the endpoint immediately for the same reason the
    // sub-9 ramp does - the gate reads a level, not a trajectory.
    // REF: FUN_801D1344 (the gate arm), FUN_801DE840 (these four arms)
    // Op `0x4C` nibble-4 sub-0 with a tick count, aimed at the player
    // ([`field_step_routed`] runs it on the player's stand-in context): the
    // arm hands `&ctx+0x72` to the generic ramp scheduler, kind 2, from the
    // live `+0x72` (`lhu a3,0x72(a0)`) to the operand over `ticks` frames
    // (`0x801E118C..0x801E11A8` -> `jal 0x8003C5F0` at `0x801E205C`). Other
    // contexts' `+0x72` ramps still drop: no port reader animates them.
    // REF: FUN_8003C5F0
    /// Op `4C 81` - the actor's draw tint (`+0x74` colour, `+0x78` blend).
    /// See `world::object_actor_height`.
    ///
    /// PORT: FUN_801DE840 (the nibble-8 sub-1 arm, `0x801E1FC4..0x801E2068`)
    pub(super) fn set_actor_tint(
        &mut self,
        ctx: &mut FieldCtx,
        target: Option<u8>,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        let player = self.ctx_is_player(ctx);
        self.world
            .set_actor_tint(ctx, target, player, colour, blend, ticks);
    }

    pub(super) fn camera_configure(
        &mut self,
        params: &[CameraParam],
        apply_trigger: u16,
        mode: u8,
    ) {
        // Retail op-0x45 (`FUN_801DE084`) writes each masked param into a
        // PERSISTENT camera struct slot (`0x801C6EA8 + 0x02 + i*4`); a beat that
        // omits a slot keeps its prior value. So MERGE per-slot rather than
        // replacing the set: an opdeene beat that sets only slot 9 (H) - one of
        // its nine beats is exactly `[(9, 792)]` - must keep the focus / pitch /
        // eye-depth staged by the previous beat, not reset the shot to the
        // `cutscene_view` fall-back framing (lead-actor focus + default depth).
        // The persistent set is cleared on scene entry so cutscene shots don't
        // leak across scenes (see `enter_field_scene`).
        for p in params {
            if let Some(existing) = self
                .world
                .camera
                .state
                .params
                .iter_mut()
                .find(|e| e.slot == p.slot)
            {
                existing.value = p.value;
            } else {
                self.world.camera.state.params.push(*p);
            }
        }
        self.world.camera.state.apply_trigger = apply_trigger;
        self.world.camera.state.mode = mode;
        // A glide arms the mover for `apply_trigger` display frames; a snap
        // marks every live mover dead.
        // REF: FUN_801DE084
        self.world.camera.state.glide_frames = i32::from(apply_trigger);
        // The event still carries only THIS beat's params (the per-beat delta):
        // the `Camera` controller's `route_camera_events` applies them per-axis
        // onto its own persistent eye/look-at, matching the same retail model.
        self.world
            .pending_field_events
            .push(FieldEvent::CameraConfigure {
                params: params.to_vec(),
                apply_trigger,
                mode,
            });
    }

    pub(super) fn set_effect_colour(&mut self, op0: u8, rgb: [u8; 3], intensity: i16) {
        // Op `0x34` sub-0 is the **screen-effect colour tween**, and it is a
        // walk-out / walk-in pair rather than a value ramp. Reading the arm
        // at `0x801DFCD4..0x801DFEF8` off the field overlay:
        //
        // 1. If `_DAT_8007B62C` names a live effect actor, retire it
        //    (`+0x10 |= 8`) and spawn a tween that runs from the *previous*
        //    target colour down to black with a **one**-frame hold, using
        //    the blend and kind selectors as they stood.
        // 2. Recompute both selectors from the sub-op byte and latch the new
        //    target RGB into `_DAT_8007BCCD/CE/CF`.
        // 3. An all-zero target **clears** the effect: retail stores zero
        //    into `_DAT_8007B62C` and leaves without spawning anything.
        // 4. Otherwise spawn the walk-in tween, black -> target, hold `-1`.
        //
        // REF: FUN_801DE2B0 (the spawner, ported at
        // `legaia_engine_field::actor_colour_tween::tween_from_fade_template`)
        //
        // Both spawns fork on scratchpad global `_DAT_1F800394` bit 23
        // (`lui v1,0x80; and` at `0x801DFD1C` / `0x801DFEB0`): set, retail
        // spawns through `FUN_80024E80` instead of `FUN_801DE2B0`. Only the
        // clear arm is modelled here because no script the disc carries
        // raises that bit - neither a field-VM `0x2E` (`asset
        // field-op-census --only 2E`) nor a motion-VM `0x10`/`0x11` (none is
        // authored in any bank), pinned by
        // `crates/asset/tests/scratch_global_bit_writers_real.rs`.
        use legaia_engine_field::actor_colour_tween::tween_from_fade_template;
        use legaia_engine_field::scene_transition_sweep::ACTOR_FLAG_YIELD;
        use legaia_engine_system::fade::FadeTemplate;

        if let Some(slot) = self.world.presentation.effect_tween_slot.take() {
            if let Some(a) = self.world.actors.get_mut(slot) {
                a.physics.status_flags |= ACTOR_FLAG_YIELD;
            }
            let walk_out = FadeTemplate {
                kind: self.world.presentation.effect_blend,
                duration: intensity,
                start_rgb: self.world.presentation.effect_target_rgb,
                end_rgb: [0; 3],
                mode: [0, 1, 0],
            };
            let kind = self.world.presentation.effect_kind;
            if let Some(out) = self
                .world
                .spawn_colour_tween(tween_from_fade_template(&walk_out, kind))
            {
                self.world.tick_spawned_colour_tween(out);
            }
        }

        let blend: i16 = if op0 & 1 != 0 { 2 } else { 1 };
        let kind: i16 = if op0 & 2 != 0 {
            8
        } else if op0 & 4 != 0 {
            0
        } else {
            2
        };
        let target = [i16::from(rgb[0]), i16::from(rgb[1]), i16::from(rgb[2])];
        self.world.presentation.effect_blend = blend;
        self.world.presentation.effect_kind = kind;
        self.world.presentation.effect_target_rgb = target;

        self.world
            .pending_field_events
            .push(FieldEvent::ColorFade { op0, rgb });

        if target == [0; 3] {
            return;
        }

        // The one conditional on the operand: a pure-white target under
        // blend `2` shortens the ramp by an eighth (`sra v0,s1,3` /
        // `subu s1,s1,v0` at `0x801DFE60`). The shipped `0x41`-frame
        // instruction is what a capture sees as a 57-frame template.
        let duration = if blend == 2 && target == [0xFF; 3] {
            intensity - (intensity >> 3)
        } else {
            intensity
        };
        let walk_in = FadeTemplate {
            kind: blend,
            duration,
            start_rgb: [0; 3],
            end_rgb: target,
            mode: [
                0,
                legaia_engine_field::actor_colour_tween::TWEEN_HOLD_FOREVER,
                0,
            ],
        };
        self.world.presentation.effect_tween_slot = self
            .world
            .spawn_colour_tween(tween_from_fade_template(&walk_in, kind));
        if let Some(slot) = self.world.presentation.effect_tween_slot {
            self.world.tick_spawned_colour_tween(slot);
        }
    }

    pub(super) fn effect_anim_trigger(&mut self, ctx: &mut FieldCtx, arg: u8) {
        // Op 0x34 sub-3 is the ambient-tree install, and retail has ONE of
        // them. The dispatcher arm at `0x801E00B0` is
        // `FUN_800252EC(bytecode[1] + 1, s5 + 0x14, s5 + 0x24)` where `s5` is
        // the executing script's context (`FUN_801DE840`'s third argument,
        // retargeted by the `0x80` cross-context prefix), and that call stages
        // the record through `FUN_80021B04` -> the move VM. The scene-entry
        // installer is not a second mechanism: `FUN_8003A1E4` runs this same
        // dispatcher for one frame slice per just-spawned placement, so a
        // load-slice install and a runtime install differ only in when the
        // arm is reached.
        //
        // The port therefore routes both to `spawn_ambient_record_at` - the
        // full `FUN_80021B04` port, with the op-`0x25` fan-out, the
        // self-modifying bytecode writes, the mode-3 CLUT-cell integrator,
        // the mode-4 VRAM-rect scroller and the VDF morph envelope. Routing
        // this arm at the older `SummonScene` pool instead left a runtime
        // install running a stripped copy of its own tree.
        //
        // Seat = the executing context, not the player: `s5 + 0x14` is the
        // ctx position and `s5 + 0x24` its render banks. A placement channel
        // seeds both from its MAN record, so a scripted install stages where
        // its actor stands.
        //
        // PORT: FUN_800252EC (op 0x34 sub-3 -> stager record `arg + 1`)
        let origin = [ctx.world_x as i16, ctx.world_y as i16, ctx.world_z as i16];
        let rot = [ctx.field_24, ctx.field_26 as i16, ctx.field_28];
        self.world
            .spawn_ambient_record_at(arg as usize + 1, origin, rot);
        self.world
            .pending_field_events
            .push(FieldEvent::EffectAnimTrigger { arg });
    }

    pub(super) fn set_scene_colour(&mut self, op0: u8, payload: &[u8; 5]) {
        // Sub-op 0x12: the global multiply screen tint `DAT_8007BCB8/B9/BA =
        // payload[0..3]` (neutral 0x80), optionally ramped there over
        // `LE_u16(payload[3..5])` frames by the slot-job spawner
        // `FUN_8003C5F0`. This is the retail scene-fade primitive - every
        // field scene `P1[0]`'s 0x52F arrival arm fades in from black with
        // `4C 12 00 00 00 00 00` (instant black) + `4C 12 80 80 80 44 00`
        // (ramp to neutral over 68 frames).
        // REF: FUN_8003C5F0
        if op0 == 0x12 {
            let target = [
                (payload[0] as f32 / 128.0).min(2.0),
                (payload[1] as f32 / 128.0).min(2.0),
                (payload[2] as f32 / 128.0).min(2.0),
            ];
            let frames = u16::from_le_bytes([payload[3], payload[4]]);
            let current = self.world.presentation.tint.as_ref().map(|t| t.factor());
            self.world.presentation.tint = Some(
                legaia_engine_system::fade::SceneTintRamp::to_target(current, target, frames),
            );
        }
        // Sub-op 0x13: the field clear colour (both draw environments'
        // `r0 / g0 / b0`), instant or ramped by `FUN_8003C5F0` over
        // `LE_u16(payload[3..5])` frames - see `ClearColourRamp`.
        // REF: FUN_8003C5F0
        if op0 == 0x13 {
            let end = [payload[0], payload[1], payload[2]];
            let frames = u16::from_le_bytes([payload[3], payload[4]]);
            let p = &mut self.world.presentation;
            if frames == 0 {
                p.clear_rgb = end;
                p.clear_ramp = None;
            } else {
                p.clear_ramp = Some(crate::world::ClearColourRamp {
                    start: p.clear_rgb,
                    end,
                    total: frames,
                    elapsed: 0,
                });
            }
        }
        self.world.pending_field_events.push(FieldEvent::MenuCtrl {
            op0,
            payload: *payload,
        });
    }
}
