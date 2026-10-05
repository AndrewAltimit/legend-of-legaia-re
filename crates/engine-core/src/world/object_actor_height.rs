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

/// Which actor word an [`ObjectSlotRamp`] tweens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectRampSlot {
    /// `+0x8E` - the height slot (`4C 42`).
    Height8E,
    /// `+0x74` - the tint colour, per channel (`4C 81`, scheduler type 3).
    TintColour,
    /// `+0x78` - the tint blend (`4C 81`, scheduler type 2).
    TintBlend,
}

/// One live tween on an object-bind actor, keyed by the actor's flat record
/// index (`+0x50`) and the word it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectSlotRamp {
    /// The object channel's script id (flat partition-0 record).
    pub record: u16,
    /// The word tweened.
    pub slot: ObjectRampSlot,
    /// The word when the tween was scheduled.
    pub start: i32,
    /// The tween's end value.
    pub end: i32,
    /// Length in frames.
    pub total: u16,
    /// Frames stepped so far.
    pub elapsed: u16,
}

/// A straight line from `start` to `end` after `elapsed` of `total` frames,
/// landing exactly on `end`.
fn lerp(start: i32, end: i32, elapsed: u16, total: u16) -> i32 {
    if elapsed >= total || total == 0 {
        return end;
    }
    start + (end - start) * i32::from(elapsed) / i32::from(total)
}

impl ObjectSlotRamp {
    /// The word's value after [`Self::elapsed`] frames. A colour tween moves
    /// each 8-bit channel on its own line.
    fn value(&self) -> i32 {
        match self.slot {
            ObjectRampSlot::TintColour => {
                let mut out = 0u32;
                for sh in [0u32, 8, 16] {
                    let a = (self.start as u32 >> sh) & 0xFF;
                    let b = (self.end as u32 >> sh) & 0xFF;
                    let c = lerp(a as i32, b as i32, self.elapsed, self.total) as u32 & 0xFF;
                    out |= c << sh;
                }
                out as i32
            }
            _ => lerp(self.start, self.end, self.elapsed, self.total),
        }
    }
}

/// Whose draw tint an [`ActorTint`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActorTintKey {
    /// The live player (`0xF8`).
    Player,
    /// A field NPC, by placement index.
    Npc(usize),
    /// A placed object's actor (flat MAN record) tinted by another
    /// script's prefixed op.
    Object(u16),
}

/// One straight-line tween of a tint word.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TintRamp {
    pub start: i32,
    pub end: i32,
    pub total: u16,
    pub elapsed: u16,
}

/// A character's draw tint: `+0x74` colour (`0xBBGGRR`) and `+0x78` blend
/// (`0x1000` = full), with the op-`4C 81` tweens still running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActorTint {
    pub colour: u32,
    pub blend: u16,
    pub colour_ramp: Option<TintRamp>,
    pub blend_ramp: Option<TintRamp>,
}

impl ActorTint {
    /// Apply one op-`4C 81` (see [`World::set_actor_tint`]).
    pub fn set(&mut self, colour: u32, blend: u16, ticks: u16) {
        if ticks == 0 {
            *self = Self {
                colour,
                blend,
                ..Self::default()
            };
            return;
        }
        let tween_colour = self.blend != 0 && blend != 0;
        if self.blend == 0 {
            self.colour = colour;
            self.colour_ramp = None;
        }
        if tween_colour {
            self.colour_ramp = Some(TintRamp {
                start: self.colour as i32,
                end: colour as i32,
                total: ticks,
                elapsed: 0,
            });
        }
        self.blend_ramp = Some(TintRamp {
            start: i32::from(self.blend),
            end: i32::from(blend),
            total: ticks,
            elapsed: 0,
        });
    }

    /// Step the running tweens one frame.
    pub fn tick(&mut self) {
        if let Some(r) = self.colour_ramp.as_mut() {
            r.elapsed = r.elapsed.saturating_add(1);
            let v = ObjectSlotRamp {
                record: 0,
                slot: ObjectRampSlot::TintColour,
                start: r.start,
                end: r.end,
                total: r.total,
                elapsed: r.elapsed,
            }
            .value();
            self.colour = v as u32 & 0x00FF_FFFF;
            if r.elapsed >= r.total {
                self.colour_ramp = None;
            }
        }
        if let Some(r) = self.blend_ramp.as_mut() {
            r.elapsed = r.elapsed.saturating_add(1);
            self.blend = lerp(r.start, r.end, r.elapsed, r.total) as u16;
            if r.elapsed >= r.total {
                self.blend_ramp = None;
            }
        }
    }

    /// `(colour, blend)` while the actor draws tinted (`blend != 0`).
    pub fn drawn(&self) -> Option<(u32, u16)> {
        (self.blend != 0).then_some((self.colour, self.blend))
    }
}

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
            Some(crate::field_env::PLAYER_ANCHOR_TARGET) => ActorTintKey::Player,
            Some(t) => {
                let view = self.channel_view();
                let Some(ci) = crate::field_channels::resolve_target(view, t) else {
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

    /// The object-effect clip ([`crate::object_effect`]) of every field
    /// context whose `+0x42` is raised (field-VM `4C C2`), keyed the way the
    /// hosts key their draws: [`ActorTintKey::Object`] for an object-bind
    /// channel (its flat record), [`ActorTintKey::Npc`] for a placement. The
    /// `f32` is the actor's render scale `+0x72` (`1.0 = 0x1000`; `0` reads
    /// as `1.0`, the allocator's seed). Empty while no context raised it,
    /// which is every scene but nine.
    // REF: FUN_8001C204, FUN_80027F00
    pub fn object_effect_clips(
        &self,
    ) -> Vec<(ActorTintKey, crate::object_effect::EffectClip, f32)> {
        self.field_vm
            .channels
            .iter()
            .filter(|c| c.ctx.field_42 != 0)
            .filter_map(|c| {
                let clip =
                    self.object_effect
                        .clip_for(c.ctx.field_42, &self.sin_lut, &self.cos_lut)?;
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
            })
            .collect()
    }

    /// [`Self::object_effect_clips`] for one key, in mesh space for a draw
    /// whose model matrix rows are `model_rows` - the one call both hosts
    /// make per placed / NPC draw.
    pub fn object_effect_mesh_clip(
        &self,
        key: ActorTintKey,
        model_rows: [[f32; 4]; 3],
    ) -> Option<crate::object_effect::MeshClip> {
        self.object_effect_clips()
            .into_iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, clip, scale)| clip.in_mesh_space(model_rows, scale))
    }
}

/// A tint pair as a constant per-draw depth cue: `far` in display `0..1`
/// units (`0xBBGGRR`) and `IR0` in `1.0 = 0x1000` units - the
/// `legaia_engine_render::DrawCue` / page `cue` shape with a flat ramp.
pub fn tint_cue(colour: u32, blend: u16) -> ([f32; 3], f32) {
    (
        [
            f32::from(colour as u8) / 255.0,
            f32::from((colour >> 8) as u8) / 255.0,
            f32::from((colour >> 16) as u8) / 255.0,
        ],
        f32::from(blend) / 4096.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_lands_on_its_end_value() {
        let mut r = ObjectSlotRamp {
            record: 28,
            slot: ObjectRampSlot::Height8E,
            start: 700,
            end: 0,
            total: 23,
            elapsed: 0,
        };
        assert_eq!(r.value(), 700);
        r.elapsed = 11;
        assert!(r.value() < 700 && r.value() > 0);
        r.elapsed = 23;
        assert_eq!(r.value(), 0);
    }

    #[test]
    fn colour_tween_moves_each_channel_alone() {
        let r = ObjectSlotRamp {
            record: 1,
            slot: ObjectRampSlot::TintColour,
            start: 0x00_00_FF,
            end: 0xFF_00_00,
            total: 2,
            elapsed: 1,
        };
        assert_eq!(r.value(), 0x7F_00_80);
    }

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
    ) -> crate::field_channels::FieldChannel {
        crate::field_channels::FieldChannel {
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

    /// The fade-in idiom (`4C 81 .. 00 10 00 00` then `.. 00 00 24 00`):
    /// black at once, then the blend tweens back to zero over 0x24 frames.
    #[test]
    fn a_character_tint_tweens_off_over_its_ticks() {
        let mut t = ActorTint::default();
        t.set(0, 0x1000, 0);
        assert_eq!(t.drawn(), Some((0, 0x1000)));
        t.set(0, 0, 0x24);
        for _ in 0..0x12 {
            t.tick();
        }
        let (_, mid) = t.drawn().expect("still tinted half way");
        assert!(mid > 0 && mid < 0x1000);
        for _ in 0..0x12 {
            t.tick();
        }
        assert_eq!(t.drawn(), None, "the tween lands on blend 0");
        // A tint from zero takes its colour at once and tweens the blend up.
        t.set(0x4040FF, 0x100, 0x20);
        assert_eq!(t.colour, 0x4040FF);
        t.tick();
        assert!(t.blend > 0 && t.blend < 0x100);
    }
}
