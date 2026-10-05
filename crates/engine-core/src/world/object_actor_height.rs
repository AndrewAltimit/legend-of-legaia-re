//! The height law of a `.MAP` placed object's actor: op `4C 42`'s `+0x8E`
//! slot and the actor tick's `+0x10 & 0x20000000` override.
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
//! REF: FUN_8003C5F0 (the tween scheduler), FUN_8003BC08 (the height arm)

use super::*;

/// The actor-tick flag that pins a field actor's Y to `-(+0x8E)`.
const ACTOR_Y_FROM_8E: u32 = 0x2000_0000;

/// One live `+0x8E` tween on an object-bind actor (`4C 42 <val> <ticks>`
/// with `ticks != 0`), keyed by the actor's flat record index (`+0x50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectSlotRamp {
    /// The object channel's script id (flat partition-0 record).
    pub record: u16,
    /// `+0x8E` when the tween was scheduled.
    pub start: i16,
    /// The tween's end value.
    pub end: i16,
    /// Length in frames.
    pub total: u16,
    /// Frames stepped so far.
    pub elapsed: u16,
}

impl ObjectSlotRamp {
    /// The slot value after `elapsed` frames - a straight line from `start`
    /// to `end`, landing exactly on `end`.
    fn value(&self) -> i16 {
        if self.elapsed >= self.total || self.total == 0 {
            return self.end;
        }
        let span = i32::from(self.end) - i32::from(self.start);
        (i32::from(self.start) + span * i32::from(self.elapsed) / i32::from(self.total)) as i16
    }
}

impl World {
    /// Schedule an op-`4C 42` tween on object-bind actor `record` from its
    /// current `+0x8E` (`start`) to `end` over `ticks` frames. A later tween
    /// on the same actor replaces the earlier one.
    pub(crate) fn schedule_object_slot_ramp(
        &mut self,
        record: u16,
        start: i16,
        end: i16,
        ticks: u16,
    ) {
        let ramps = &mut self.field_vm.object_slot_ramps;
        ramps.retain(|r| r.record != record);
        ramps.push(ObjectSlotRamp {
            record,
            start,
            end,
            total: ticks,
            elapsed: 0,
        });
    }

    /// One actor tick of the object height law: step every live `+0x8E`
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
                c.ctx.field_8e = v;
            }
        }
        ramps.retain(|r| r.elapsed < r.total);
        self.field_vm.object_slot_ramps = ramps;
        for c in self.field_vm.channels.iter_mut().filter(|c| c.object_bind) {
            if c.ctx.flags & ACTOR_Y_FROM_8E != 0 {
                c.ctx.world_y = c.ctx.field_8e.wrapping_neg() as u16;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tween_lands_on_its_end_value() {
        let mut r = ObjectSlotRamp {
            record: 28,
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
}
