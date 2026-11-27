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
    /// `+0x24` - the X Euler angle (`4C 43`).
    Pitch24,
    /// `+0x28` - the Z Euler angle (`4C 44`).
    Roll28,
    /// `+0x72` - the render scale (`4C 40`).
    Scale72,
    /// `+0x6A` - the clip rate (`4C 41`).
    Rate6A,
    /// `+0x74` - the tint colour, per channel (`4C 81`, scheduler type 3).
    TintColour,
    /// `+0x78` - the tint blend (`4C 81`, scheduler type 2).
    TintBlend,
}

/// One live tween on an object-bind actor, keyed by the actor's flat record
/// index (`+0x50`) and the word it moves: a slot of the property-ramp pool
/// `FUN_8003C5F0` installs and `FUN_80036D80` ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectSlotRamp {
    /// The object channel's script id (flat partition-0 record).
    pub record: u16,
    /// The word tweened.
    pub slot: ObjectRampSlot,
    /// The word when the tween was scheduled (`slot+0x08`).
    pub start: i32,
    /// The tween's end value (`slot+0x0C`).
    pub end: i32,
    /// Length in frames (`slot+0x10`), never rewritten.
    pub total: i32,
    /// The countdown (`slot+0x14`).
    pub remaining: i32,
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

/// One visit of the ramp ticker `FUN_80036D80` to a slot: the countdown
/// drops by `step` (`_DAT_1F800393`), and at or below zero the slot stores
/// its end value and retires; otherwise it stores
/// `end + (start - end) * remaining / total`, truncated toward zero
/// (`0x80036DD4..0x80036E50`), or the packed-colour lanes of kind 3
/// (`0x80036E54..0x80036F60`). Returns `(value, finished)`.
///
/// The countdown is the script's ticks word read signed (`FUN_8003CE9C`), so a
/// `0x8000..` count lands on the end value at the first visit.
// REF: FUN_80036D80
pub fn ramp_visit(
    start: i32,
    end: i32,
    total: i32,
    remaining: &mut i32,
    step: i32,
    colour: bool,
) -> (i32, bool) {
    use legaia_engine_motion_vm::ambient_motion::{lerp_remaining, rgb_lerp};
    *remaining = remaining.wrapping_sub(step);
    if *remaining <= 0 {
        (end, true)
    } else if colour {
        (rgb_lerp(start, end, *remaining, total, false), false)
    } else {
        (lerp_remaining(start, end, *remaining, total), false)
    }
}

impl ObjectSlotRamp {
    /// A tween of `ticks` frames (the script word, read signed).
    pub fn new(record: u16, slot: ObjectRampSlot, start: i32, end: i32, ticks: u16) -> Self {
        let total = legaia_engine_motion_vm::ambient_motion::script_ramp_ticks(ticks);
        Self {
            record,
            slot,
            start,
            end,
            total,
            remaining: total,
        }
    }

    /// Step the tween `step` frames: the word's new value and whether the
    /// tween has landed ([`ramp_visit`]). A colour tween moves the packed
    /// `0xBBGGRR` lanes and stores a word whose top byte is zero.
    pub fn visit(&mut self, step: i32) -> (i32, bool) {
        ramp_visit(
            self.start,
            self.end,
            self.total,
            &mut self.remaining,
            step,
            self.slot == ObjectRampSlot::TintColour,
        )
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

/// One tween of a tint word: a ramp-pool slot ([`ramp_visit`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TintRamp {
    pub start: i32,
    pub end: i32,
    pub total: i32,
    pub remaining: i32,
}

impl TintRamp {
    fn new(start: i32, end: i32, ticks: u16) -> Self {
        let total = legaia_engine_motion_vm::ambient_motion::script_ramp_ticks(ticks);
        Self {
            start,
            end,
            total,
            remaining: total,
        }
    }
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
    /// Apply one op-`4C 81` (see `World::set_actor_tint`): `ticks == 0`
    /// writes both words; otherwise a zero current blend takes the colour at
    /// once, a non-zero current and new blend tween the colour (pool kind 3),
    /// and the blend always tweens (kind 2, from the `lhu` of `+0x78` to the
    /// operand's signed halfword) - `0x801E1FE0..0x801E2060`.
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
            self.colour_ramp = Some(TintRamp::new(self.colour as i32, colour as i32, ticks));
        }
        self.blend_ramp = Some(TintRamp::new(
            i32::from(self.blend),
            i32::from(blend as i16),
            ticks,
        ));
    }

    /// Step the running tweens one frame (one ticker visit at frame step 1).
    pub fn tick(&mut self) {
        if let Some(r) = self.colour_ramp.as_mut() {
            let (v, done) = ramp_visit(r.start, r.end, r.total, &mut r.remaining, 1, true);
            self.colour = v as u32 & 0x00FF_FFFF;
            if done {
                self.colour_ramp = None;
            }
        }
        if let Some(r) = self.blend_ramp.as_mut() {
            let (v, done) = ramp_visit(r.start, r.end, r.total, &mut r.remaining, 1, false);
            // Kind 2: the `sh` keeps the low halfword.
            self.blend = v as u16;
            if done {
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
        let mut r = ObjectSlotRamp::new(28, ObjectRampSlot::Height8E, 700, 0, 23);
        let (v, done) = r.visit(11);
        assert!(v < 700 && v > 0 && !done);
        // `end + (start - end) * remaining / total`, truncated: 12/23 of 700.
        assert_eq!(v, 700 * 12 / 23);
        assert_eq!(r.visit(12), (0, true));
    }

    #[test]
    fn a_negative_ticks_word_lands_at_the_first_visit() {
        let mut r = ObjectSlotRamp::new(1, ObjectRampSlot::Scale72, 0, 0x1000, 0xFFF0);
        assert_eq!(r.visit(1), (0x1000, true));
        let mut t = ActorTint {
            colour: 0x10,
            blend: 0x800,
            ..Default::default()
        };
        t.set(0x80, 0x1000, 0x8000);
        t.tick();
        assert_eq!((t.colour, t.blend), (0x80, 0x1000));
        assert!(t.colour_ramp.is_none() && t.blend_ramp.is_none());
    }

    #[test]
    fn colour_tween_moves_each_lane_as_the_ticker_does() {
        let mut r = ObjectSlotRamp::new(1, ObjectRampSlot::TintColour, 0x00_00_FF, 0xFF_00_00, 2);
        // Red runs on the plain byte, blue on the byte scaled by 256 and
        // masked: one half remaining gives red 0x7F and blue 0x7F.
        assert_eq!(r.visit(1), (0x7F_00_7F, false));
        assert_eq!(r.visit(1), (0xFF_00_00, true));
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
