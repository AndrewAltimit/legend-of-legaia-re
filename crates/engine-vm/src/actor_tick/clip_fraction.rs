//! The clip-fraction step.

use super::*;

/// The full-scale value of the 12-bit clip fraction at `actor + 0x78`.
/// Reaching it retires the actor; the stored value saturates one below.
pub const CLIP_FRACTION_FULL: u16 = 0x1000;

/// Advance the **12-bit fraction** clip timer of one field-overlay actor and
/// report whether this step retired it.
///
/// `actor[+0x78] += (i16)actor[+0x54] * dt` as a wrapping 16-bit add, where
/// `dt` is the adaptive frame-step byte `DAT_1F800393`; when the unsigned
/// result reaches [`CLIP_FRACTION_FULL`] the accumulator is pinned at `0xFFF`
/// and the retire bit `0x8` goes into the flag word `+0x10`. Below that the
/// flag word is not touched, so the caller can drive several of these against
/// one actor without one of them clearing another's retire.
///
/// It is the fourth member of the field overlay's frameless clip-timer family
/// (`FUN_801DD4C4` / `FUN_801DD784` / `FUN_801DA930` are the others). What
/// separates it is the **ceiling**: the other three clamp at a per-actor
/// duration halfword, this one at a fixed 12-bit fraction, so `+0x54` here is
/// a *rate* rather than a cursor and `+0x78` is a normalised progress value.
///
/// The multiply is done in `i32` and stored back through `u16`, matching
/// retail's `mult` / `mflo` / `addu` / `sh` chain: a rate large enough to
/// overshoot `0x10000` wraps first and is then tested, exactly as retail
/// tests `andi v0, v0, 0xffff` after the store.
///
/// Retail reaches it as the `+0x08` **tick** word of the static actor
/// template at SCUS `0x80070644` (`+0x02` id `0x15`, flag word `0x80`, state
/// word `1`) - the record's only reference on the whole disc, and the
/// template's only materialisation site is `FUN_801D835C` at `0x801D8370`,
/// the actor-clone helper behind field-VM op `0x4C` sub-1 sub-op `0x14`.
///
/// PORT: FUN_801D820C
///
/// Live on both hosts through that same chain: the field VM's `0x4C` sub-1
/// arm calls `FieldActorSpawnHost::clone_actor`, `World::spawn_actor_clone`
/// seats the clone on a pool slot carrying
/// `legaia_engine_field::actor_handler::ActorHandler::ClipFade`, and
/// `World::tick_handler_actors` runs this every frame until the accumulator
/// fills and the retire bit collects it. Six shipped scenes issue the
/// opcode (`vozz`, `retona`, `urudre3`, `kor5`, `nilboa`, `noaru`).
/// REF: FUN_801D835C
pub fn clip_fraction_step(p: &mut ActorPhysics, dt: u8) -> bool {
    let step = (p.timer as i32).wrapping_mul(dt as i32);
    let next = (p.focal_envelope as u16).wrapping_add(step as u16);
    p.focal_envelope = next as i16;
    if next < CLIP_FRACTION_FULL {
        return false;
    }
    p.focal_envelope = (CLIP_FRACTION_FULL - 1) as i16;
    p.status_flags |= 0x8;
    true
}

#[cfg(test)]
mod clip_fraction_tests {
    use super::*;

    #[test]
    fn accumulates_rate_times_frame_step() {
        let mut p = ActorPhysics {
            timer: 0x40,
            focal_envelope: 0x100,
            ..Default::default()
        };
        assert!(!clip_fraction_step(&mut p, 3));
        assert_eq!(p.focal_envelope as u16, 0x100 + 0x40 * 3);
        assert_eq!(p.status_flags, 0);
    }

    #[test]
    fn retires_at_the_12_bit_ceiling_and_pins_one_below() {
        let mut p = ActorPhysics {
            timer: 0x100,
            focal_envelope: 0x0F80,
            ..Default::default()
        };
        assert!(clip_fraction_step(&mut p, 1));
        assert_eq!(p.focal_envelope as u16, 0x0FFF);
        assert_eq!(p.status_flags & 0x8, 0x8);
    }

    #[test]
    fn exactly_full_scale_still_retires() {
        let mut p = ActorPhysics {
            timer: 0x800,
            focal_envelope: 0x800,
            ..Default::default()
        };
        assert!(clip_fraction_step(&mut p, 1));
        assert_eq!(p.focal_envelope as u16, 0x0FFF);
    }

    /// A negative rate walks the fraction back down and never retires: the
    /// compare is unsigned, so only a value that has actually reached full
    /// scale trips it.
    #[test]
    fn negative_rate_never_retires() {
        let mut p = ActorPhysics {
            timer: -0x20,
            focal_envelope: 0x200,
            ..Default::default()
        };
        assert!(!clip_fraction_step(&mut p, 2));
        assert_eq!(p.focal_envelope as u16, 0x200 - 0x40);
        assert_eq!(p.status_flags, 0);
    }

    /// The store is a halfword, so an overshoot past `0x10000` wraps before
    /// the test - retail's `andi 0xffff` after the `sh`.
    #[test]
    fn overshoot_wraps_through_the_halfword_store() {
        let mut p = ActorPhysics {
            timer: 0x4000,
            focal_envelope: 0,
            ..Default::default()
        };
        assert!(!clip_fraction_step(&mut p, 4));
        assert_eq!(p.focal_envelope as u16, 0);
        assert_eq!(p.status_flags, 0);
    }
}
