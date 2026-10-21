//! A `.MAP` placed object's actor words that a script tweens and the draw
//! reads back: the height law (op `4C 42`'s `+0x8E` slot and the actor tick's
//! `+0x10 & 0x20000000` override) and the draw tint (op `4C 81`'s `+0x74`
//! colour / `+0x78` blend).
//!
//! The field VM's op `0x4C` nibble-4 sub-2 writes `+0x8E` outright (and, on
//! that immediate path only, mirrors `world_y = -value` while `+0x10 &
//! 0x20000000` is up) or schedules a `FUN_8003C5F0` tween of the slot over
//! `ticks` frames. The tween touches `+0x8E` alone; what carries it into the
//! actor's Y is the per-actor tick's height arm (`FUN_8003BC08`), which for an
//! actor carrying `0x20000000` writes `+0x16 = -(+0x8E)` every frame and skips
//! the ground sample (`docs/subsystems/motion-vm.md`, "Height arm").
//!
//! `chitei2`'s collapse beat drops its boulder this way: partition-0 records
//! 28..30 raise the bit (`31 1D`) and park `+0x8E = 700` at spawn, and P2[17]
//! seats them at the foot of the escape stairs and tweens the slot to `0`
//! over 21..27 frames. The placed-object draw follows the actor
//! ([`World::object_draw_displacements`]).
//!
//! Nibble-8 sub-1 (`0x801E1FC4..0x801E2068`) writes or tweens the tint pair
//! the actor draw stages as the GTE far colour and `IR0` (`FUN_8001ADA4` ->
//! `FUN_80043390`, `0x8001B46C..0x8001B474`): `chitei2`'s hologram panels
//! (partition-0 records 19..27) run `4C 81 00 00 00 00 10 00 00` in their
//! spawn prologue once flag `0x4C5` (the generator destroyed) is up - colour
//! black at full blend, so the panels go dark
//! ([`World::object_draw_tints`]).
//!
//! REF: FUN_8003C5F0 (the tween scheduler), FUN_8003BC08 (the height arm)
//! REF: FUN_8001ADA4 (the tint staging)

use super::*;

/// The actor-tick flag that pins a field actor's Y to `-(+0x8E)`.
const ACTOR_Y_FROM_8E: u32 = 0x2000_0000;

impl World {
    /// Schedule a tween on object-bind actor `record`'s `slot` from `start`
    /// to `end` over `ticks` frames. A later tween of the same word replaces
    /// the earlier one.
    pub(crate) fn schedule_object_ramp(
        &mut self,
        record: u16,
        slot: ObjectRampSlot,
        start: i32,
        end: i32,
        ticks: u16,
    ) {
        let ramps = &mut self.field_vm.object_slot_ramps;
        ramps.retain(|r| !(r.record == record && r.slot == slot));
        ramps.push(ObjectSlotRamp {
            record,
            slot,
            start,
            end,
            total: ticks,
            elapsed: 0,
        });
    }

    /// Schedule an op-`4C 42` tween of object-bind actor `record`'s `+0x8E`.
    pub(crate) fn schedule_object_slot_ramp(
        &mut self,
        record: u16,
        start: i16,
        end: i16,
        ticks: u16,
    ) {
        self.schedule_object_ramp(
            record,
            ObjectRampSlot::Height8E,
            i32::from(start),
            i32::from(end),
            ticks,
        );
    }

    /// One actor tick of the object words a script tweens: step every live
    /// tween, then pin each object actor carrying `0x20000000` to
    /// `-(+0x8E)` (the height arm's first branch).
    pub(crate) fn tick_object_actor_heights(&mut self) {
        self.tick_object_glide_legs();
        let mut ramps = std::mem::take(&mut self.field_vm.object_slot_ramps);
        for r in &mut ramps {
            r.elapsed = r.elapsed.saturating_add(1);
            let v = r.value();
            if let Some(c) = self
                .field_vm
                .channels
                .iter_mut()
                .find(|c| c.object_bind && c.ctx.script_id == r.record)
            {
                match r.slot {
                    ObjectRampSlot::Height8E => c.ctx.field_8e = v as i16,
                    ObjectRampSlot::TintColour => {
                        c.ctx.field_74 = (c.ctx.field_74 & 0xFF00_0000) | (v as u32 & 0x00FF_FFFF)
                    }
                    ObjectRampSlot::TintBlend => c.ctx.field_78 = v as u16,
                }
            }
        }
        ramps.retain(|r| r.elapsed < r.total);
        self.field_vm.object_slot_ramps = ramps;
        for c in self.field_vm.channels.iter_mut().filter(|c| c.object_bind) {
            if c.ctx.flags & ACTOR_Y_FROM_8E != 0 {
                c.ctx.world_y = c.ctx.field_8e.wrapping_neg() as u16;
            }
        }
        self.tick_actor_tints();
    }

    /// Arm a compass walk (`(body0, body1, rate)`, the parked `37` / `41`
    /// op's operands) on object-bind actor `record`.
    pub(crate) fn arm_object_glide_leg(
        &mut self,
        record: u16,
        (body0, body1, rate): (u8, u8, i32),
    ) {
        self.field_vm.object_glide_legs.insert(
            record,
            ObjectGlideLeg {
                body0,
                body1,
                rate,
                spent: 0,
                frames: 0,
            },
        );
    }

    /// One walk-kernel frame of every object-bind compass walk: the actor's
    /// `+0x14` / `+0x18` move by the leg's step, and on landing the shared
    /// epilogue clears the context's halt bit `0x400` so the record runs
    /// on. The draw follows through
    /// [`Self::object_draw_displacements`], which measures the context's
    /// position against its `.MAP` seat.
    // REF: FUN_8003BC08 (the 0x400 kernel visit), FUN_8003774C (the 0x37 / 0x41 arm)
    fn tick_object_glide_legs(&mut self) {
        if self.field_vm.object_glide_legs.is_empty() {
            return;
        }
        let speed = self.clock.display_frame_step.max(1);
        for (record, mut leg) in std::mem::take(&mut self.field_vm.object_glide_legs) {
            let Some(c) = self
                .field_vm
                .channels
                .iter_mut()
                .find(|c| c.object_bind && c.ctx.script_id == record)
            else {
                continue;
            };
            let mut state = legaia_engine_motion_vm::motion_vm::MotionState {
                world_x: c.ctx.world_x as i16,
                world_z: c.ctx.world_z as i16,
                speed,
                op_accum: leg.spent,
                ..Default::default()
            };
            let done = legaia_engine_motion_vm::motion_vm::compass_walk(
                &mut state, leg.body0, leg.body1, leg.rate,
            );
            c.ctx.world_x = state.world_x as u16;
            c.ctx.world_z = state.world_z as u16;
            leg.spent = state.op_accum;
            leg.frames += 1;
            if done || leg.frames >= WALK_PARK_TIMEOUT {
                c.ctx.flags &= !0x400;
            } else {
                self.field_vm.object_glide_legs.insert(record, leg);
            }
        }
    }

    /// Op `4C 81`: the three arms of `0x801E1FC4..0x801E2068`, landed on
    /// the actor the op names.
    ///
    /// Retail's dispatcher resolves an `0x80`-prefix target byte through
    /// `FUN_8003C83C` and runs the arm against **that** actor's `+0x74` /
    /// `+0x78` (`0xF8` = the live player), so the tint belongs to whichever
    /// actor the op addresses, never to the calling script's own record:
    ///
    /// - a placed object's actor (the executing object, or a target
    ///   resolving to an object-bind channel) - [`Self::object_draw_tints`];
    /// - the player (`0xF8`, or an unprefixed op on the player's context) and
    ///   a field NPC (a target resolving to a placement channel, or an
    ///   unprefixed op stepped as that NPC) keep it in
    ///   [`super::FieldVmState::actor_tints`] ([`Self::player_draw_tint`],
    ///   [`Self::field_npc_draw_tint`]).
    ///
    /// The arm: `ticks == 0` writes both words; otherwise a zero current
    /// blend takes the colour at once, a non-zero current *and* new blend
    /// tweens the colour per channel, and the blend always tweens (the two
    /// `FUN_8003C5F0` schedules, types 3 and 2).
    // REF: FUN_8003C83C, FUN_8003C5F0
    pub(crate) fn set_actor_tint(
        &mut self,
        ctx: &mut legaia_engine_vm::field::FieldCtx,
        target: Option<u8>,
        ctx_is_player: bool,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        let colour = colour & 0x00FF_FFFF;
        let record = self.field_vm.executing_object;
        let key = match target {
            Some(legaia_engine_scene::field_env::PLAYER_ANCHOR_TARGET) => ActorTintKey::Player,
            Some(t) => {
                let view = self.channel_view();
                let Some(ci) = legaia_engine_field::field_channels::resolve_target(view, t) else {
                    // No actor answers the id: retail's resolve miss skips
                    // the arm.
                    return;
                };
                let ch = &view[ci];
                if ch.object_bind {
                    let r = ch.ctx.script_id;
                    if record == Some(r) {
                        return self.set_object_tint(ctx, r, colour, blend, ticks);
                    }
                    ActorTintKey::Object(r)
                } else {
                    ActorTintKey::Npc(ch.placement_index)
                }
            }
            None => {
                if let Some(r) = record {
                    return self.set_object_tint(ctx, r, colour, blend, ticks);
                }
                if ctx_is_player {
                    ActorTintKey::Player
                } else if let Some(slot) = self
                    .field_vm
                    .executing_channel
                    .or(self.dialog.stepping_inline_npc)
                {
                    ActorTintKey::Npc(usize::from(slot))
                } else if let Some(ch) = self
                    .channel_view()
                    .iter()
                    .find(|c| !c.object_bind && c.ctx.script_id == ctx.script_id)
                {
                    ActorTintKey::Npc(ch.placement_index)
                } else {
                    // A context no draw reads (the system script): keep the
                    // words on it, landed at once.
                    ctx.field_74 = colour;
                    ctx.field_78 = blend;
                    return;
                }
            }
        };
        self.field_vm
            .actor_tints
            .entry(key)
            .or_default()
            .set(colour, blend, ticks);
    }

    /// The placed-object arm of [`Self::set_actor_tint`]: the pair lives on
    /// the object channel's context, its tweens on the object ramp list.
    fn set_object_tint(
        &mut self,
        ctx: &mut legaia_engine_vm::field::FieldCtx,
        record: u16,
        colour: u32,
        blend: u16,
        ticks: u16,
    ) {
        if ticks == 0 {
            ctx.field_74 = colour;
            ctx.field_78 = blend;
            return;
        }
        let tween_colour = ctx.field_78 != 0 && blend != 0;
        if ctx.field_78 == 0 {
            ctx.field_74 = colour;
        }
        if tween_colour {
            self.schedule_object_ramp(
                record,
                ObjectRampSlot::TintColour,
                (ctx.field_74 & 0x00FF_FFFF) as i32,
                colour as i32,
                ticks,
            );
        }
        self.schedule_object_ramp(
            record,
            ObjectRampSlot::TintBlend,
            i32::from(ctx.field_78),
            i32::from(blend),
            ticks,
        );
    }

    /// One actor tick of the character tints' tweens.
    pub(crate) fn tick_actor_tints(&mut self) {
        for t in self.field_vm.actor_tints.values_mut() {
            t.tick();
        }
    }

    /// The player's draw tint `(colour, blend)` while it draws tinted
    /// (`+0x78 != 0`), in [`Self::object_draw_tints`]' units. Both hosts
    /// stage it on the player's mesh as a constant per-draw cue
    /// ([`tint_cue`]).
    pub fn player_draw_tint(&self) -> Option<(u32, u16)> {
        self.field_vm
            .actor_tints
            .get(&ActorTintKey::Player)
            .and_then(ActorTint::drawn)
    }

    /// Field NPC `placement_index`'s draw tint, as [`Self::player_draw_tint`].
    pub fn field_npc_draw_tint(&self, placement_index: usize) -> Option<(u32, u16)> {
        self.field_vm
            .actor_tints
            .get(&ActorTintKey::Npc(placement_index))
            .and_then(ActorTint::drawn)
    }

    /// Flat partition-0 record index -> `(colour, blend)` for every
    /// **object-bind channel** whose actor draws tinted (`+0x78 != 0`): the
    /// low 24 bits of `+0x74` (`0xBBGGRR`) and `+0x78` (`0x1000` = full),
    /// which the actor draw stages as the GTE far colour and `IR0`
    /// (`FUN_8001ADA4` -> `FUN_80043390`). Both hosts hand each listed
    /// record's placed draws a constant per-draw depth cue from it.
    // REF: FUN_8001ADA4 (0x8001B46C..0x8001B474)
    pub fn object_draw_tints(&self) -> std::collections::HashMap<usize, (u32, u16)> {
        let mut out: std::collections::HashMap<usize, (u32, u16)> = self
            .field_vm
            .channels
            .iter()
            .filter(|c| c.object_bind && c.ctx.field_78 != 0)
            .map(|c| {
                (
                    c.placement_index,
                    (c.ctx.field_74 & 0x00FF_FFFF, c.ctx.field_78),
                )
            })
            .collect();
        // A tint another script's prefixed op landed on the object.
        for (k, t) in &self.field_vm.actor_tints {
            if let ActorTintKey::Object(r) = *k {
                match t.drawn() {
                    Some(pair) => {
                        out.insert(usize::from(r), pair);
                    }
                    None => {
                        out.remove(&usize::from(r));
                    }
                }
            }
        }
        out
    }

    /// The object-effect clip ([`legaia_engine_effects::object_effect`]) of every field
    /// context whose `+0x42` is raised (field-VM `4C C2`), keyed the way the
    /// hosts key their draws: [`ActorTintKey::Object`] for an object-bind
    /// channel (its flat record), [`ActorTintKey::Npc`] for a placement. The
    /// `f32` is the actor's render scale `+0x72` (`1.0 = 0x1000`; `0` reads
    /// as `1.0`, the allocator's seed). Empty while no context raised it,
    /// which is every scene but nine.
    // REF: FUN_8001C204, FUN_80027F00
    pub fn object_effect_clips(
        &self,
    ) -> Vec<(
        ActorTintKey,
        legaia_engine_effects::object_effect::EffectClip,
        f32,
    )> {
        // The player (`CC F8 C2`), at its own render scale.
        let player = self
            .object_effect
            .clip_for(self.field_vm.player_field_42, &self.sin_lut, &self.cos_lut)
            .map(|clip| {
                let scale = self
                    .player_actor_slot
                    .and_then(|s| self.actors.get(usize::from(s)))
                    .map_or(1.0, |a| match a.move_state.field_72 {
                        0 => 1.0,
                        s => f32::from(s) / 4096.0,
                    });
                (ActorTintKey::Player, clip, scale)
            });
        player
            .into_iter()
            .chain(
                self.field_vm
                    .channels
                    .iter()
                    .filter(|c| c.ctx.field_42 != 0)
                    .filter_map(|c| {
                        let clip = self.object_effect.clip_for(
                            c.ctx.field_42,
                            &self.sin_lut,
                            &self.cos_lut,
                        )?;
                        let key = if c.object_bind {
                            ActorTintKey::Object(c.placement_index as u16)
                        } else {
                            ActorTintKey::Npc(c.placement_index)
                        };
                        let scale = match c.ctx.field_72 {
                            0 => 1.0,
                            s => f32::from(s) / 4096.0,
                        };
                        Some((key, clip, scale))
                    }),
            )
            .collect()
    }

    /// [`Self::object_effect_clips`] for one key, in mesh space for a draw
    /// whose model matrix rows are `model_rows` - the one call both hosts
    /// make per placed / NPC draw.
    pub fn object_effect_mesh_clip(
        &self,
        key: ActorTintKey,
        model_rows: [[f32; 4]; 3],
    ) -> Option<legaia_engine_effects::object_effect::MeshClip> {
        self.object_effect_clips()
            .into_iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, clip, scale)| clip.in_mesh_space(model_rows, scale))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immediate_tint_writes_both_words() {
        let mut w = World::default();
        let mut ctx = legaia_engine_vm::field::FieldCtx::default();
        w.field_vm.executing_object = Some(19);
        w.set_actor_tint(&mut ctx, None, false, 0, 0x1000, 0);
        assert_eq!((ctx.field_74, ctx.field_78), (0, 0x1000));
    }

    fn channel(
        placement: usize,
        script_id: u16,
        object_bind: bool,
    ) -> legaia_engine_field::field_channels::FieldChannel {
        legaia_engine_field::field_channels::FieldChannel {
            placement_index: placement,
            ctx: legaia_engine_vm::field::FieldCtx {
                script_id,
                ..Default::default()
            },
            record_offset: 0,
            pc: 0,
            done: false,
            object_bind,
        }
    }

    /// `CC F8 81 ..` tints the player, whoever's script carries it, and a
    /// prefixed op naming an NPC lands on that NPC - never on the caller.
    #[test]
    fn a_prefixed_tint_lands_on_the_named_actor() {
        let mut w = World::default();
        w.field_vm.channels = vec![channel(3, 0x46, false), channel(9, 0x12, true)];
        let mut caller = legaia_engine_vm::field::FieldCtx::default();
        w.set_actor_tint(&mut caller, Some(0xF8), false, 0x4040FF, 0x100, 0);
        assert_eq!(w.player_draw_tint(), Some((0x4040FF, 0x100)));
        w.set_actor_tint(&mut caller, Some(0x46), false, 0, 0x1000, 0);
        assert_eq!(w.field_npc_draw_tint(3), Some((0, 0x1000)));
        assert_eq!(caller.field_78, 0, "the caller's own words stay untouched");
        // An object named by another script's prefixed op.
        w.set_actor_tint(&mut caller, Some(0x12), false, 0x123456, 0x800, 0);
        assert_eq!(w.object_draw_tints().get(&0x12), Some(&(0x123456, 0x800)));
        // A miss tints nobody.
        w.set_actor_tint(&mut caller, Some(0x77), false, 0, 0x1000, 0);
        assert_eq!(w.field_vm.actor_tints.len(), 3);
    }

    /// `conc`'s tube floor (`P0[17]`): a prologue `37 02 01` parked on the
    /// object's own actor walks it one tile along `-X` over four speed
    /// units, then lifts the halt bit.
    #[test]
    fn a_parked_compass_walk_moves_the_object_and_unparks_it() {
        let mut w = World::default();
        let mut c = channel(17, 17, true);
        c.ctx.world_x = 4000;
        c.ctx.world_z = 4032;
        c.ctx.flags = 0x401;
        w.field_vm.channels = vec![c];
        w.arm_object_glide_leg(17, (0x02, 0x01, 0x80));
        for _ in 0..8 {
            w.tick_object_actor_heights();
        }
        let c = &w.field_vm.channels[0];
        assert_eq!((c.ctx.world_x, c.ctx.world_z), (3872, 4032));
        assert_eq!(c.ctx.flags & 0x400, 0, "the landed leg releases the halt");
        assert!(w.field_vm.object_glide_legs.is_empty());
    }
}
