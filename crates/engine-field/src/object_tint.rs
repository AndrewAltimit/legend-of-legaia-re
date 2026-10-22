//! The tweened actor words a field script drives on a placed object or a
//! character: the per-word ramp (op `4C 42`'s `+0x8E` height slot, op `4C
//! 81`'s `+0x74` colour / `+0x78` blend), the parked compass-walk leg, the
//! draw tint and its depth-cue form.
//!
//! These are the `World`-free halves. The scheduling - which actor a
//! prefixed op names, the per-frame step, the height arm carrying `+0x8E`
//! into the actor's Y - reads the field channels and lives with `World` in
//! `engine-core` (`world::object_actor_height`).
//!
//! REF: FUN_8003C5F0 (the tween scheduler), FUN_8001ADA4 (the tint staging)

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

/// A compass walk (`37` / `41`) parked on an object-bind actor's own
/// context: the operands of the parked op and the walk kernel's progress
/// cursor (`+0x54`).
///
/// `FUN_8003A55C` runs a bound object's spawn prologue through the field VM
/// at scene load; a `37 b0 b1` there parks the context under the halt bit and
/// seats the op on the actor's `+0x94`, and the object actor's tick
/// (`FUN_8003BC08`) hands it to the walk kernel `FUN_8003774C` until it
/// lands. `conc`'s glass-tube floor disc (`P0[17]`, `.MAP` record 365 at cell
/// `(31, 31)`, `x_off` `-32`) opens `37 02 01`: one tile along `-X`, so a
/// retail `conc` state holds its actor at `X = 3872`, centred in the tube,
/// not at the `4000` the `.MAP` seat alone gives.
// REF: FUN_8003A55C (the prologue run), FUN_8003BC08, FUN_8003774C (0x8003789C..0x800379F8)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectGlideLeg {
    /// Direction / divisor-selector byte.
    pub body0: u8,
    /// Length / divisor-selector byte.
    pub body1: u8,
    /// `0x80` for op `0x37`, `0x40` for op `0x41`.
    pub rate: i32,
    /// Speed units spent so far (`+0x54`).
    pub spent: u16,
    /// Ticks the leg has played, bounded by the walk park timeout.
    pub frames: u32,
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
    pub fn value(&self) -> i32 {
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
    /// Apply one op-`4C 81` (see `World::set_actor_tint`).
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
