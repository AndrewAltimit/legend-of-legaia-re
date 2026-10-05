//! PROT 0966 (Cort's Evil Seru Magic) tick body `0x801F6A74`: the half of
//! each arm that writes the seats. The camera, the countdown and the phase
//! chain are [`crate::cast_module_camera::evil_seru_magic_camera`], which
//! decides whether an arm passes; this is what the arm then does to the
//! party row `actor_table[0 .. ctx[+0]]` and the caster `actor_table[ctx+0x13]`.

use super::{
    CastActorState, CastDamageShape, CastWrapper, SweepHit, aoe_seat_is_hittable,
    apply_hit_floor_zero, stage_clip,
};

/// The neutral tint word the body shows a seat with (`lui 0x2008; ori 0x200`).
pub const ESM_NEUTRAL_TINT: u32 = 0x2008_0200;

/// The `+0x21C` value that hides a seat (`li 0xFF; sb 0x21c`).
pub const ESM_HIDDEN: u8 = 0xFF;

/// The arm-26 party hit's damage shape: `FUN_801DD4B0(0x327, ctx[+0x13],
/// seat)` at `0x801F8610`, the unsigned clamp to HP at `0x801F863C` - shape A,
/// so it **kills**. It is a separate hit from the module's move-VM stager
/// (`0x801F8D64`, power `0x100`, the never-kill clamp), which the band's fold
/// still applies.
pub const EVIL_SERU_MAGIC_SWEEP_SHAPE: CastDamageShape = CastDamageShape {
    prot_entry: 966,
    routine: 0x801F_6A74,
    wrapper: CastWrapper::Respect,
    never_kills: false,
    powers: &[0x327],
};

/// The seat writes of one pass of PROT 0966's arm `phase`. `passed` is
/// whether the arm's countdown gate let it through this pass (the camera
/// director's answer); the writes an arm makes before its gate run either
/// way.
///
/// | arm | every pass | on pass |
/// |---|---|---|
/// | 0 | party `+0x04 = 0`, `+0x21C = 0xFF` (`0x801F6C80` / `0x801F6C8C`) | - |
/// | 1 | the same, again (`0x801F6D8C` / `0x801F6D98`) | caster stages clip `6`, rate `2` (`0x801F6DE4` / `0x801F6DF0`) |
/// | 10 | - | caster hidden (`0x801F70E0` / `0x801F70E8`); party shown, neutral tint (`0x801F7310` / `0x801F731C`) |
/// | 18 | - | party hidden again (`0x801F7C34` / `0x801F7C40`) |
/// | 21 | - | caster stages clip `0`, shown, neutral tint (`0x801F7EA8..0x801F7EC8`); party shown (`0x801F7EF8` / `0x801F7F04`) |
/// | 23 | - | caster `+0x21C = 2`, the defeat-fade tint state (`0x801F8280`) |
/// | 26 | - | the party hit, see [`EVIL_SERU_MAGIC_SWEEP_SHAPE`] |
/// | 27 | party rate `0` (`0x801F8738`, the `beq` delay slot - it lands either way) | caster shown, neutral tint (`0x801F890C` / `0x801F8910`) |
/// | 28 | party rate `2` (`0x801F89E8`) | - |
///
/// The hit (`0x801F85D4..0x801F86C8`) walks the party row, skips a dead or
/// non-targetable seat, takes `roll(seat)` as the wrapper's signed net, clamps
/// it to the live HP, and stages the seat's own knockdown `+0x1F1` at rate
/// `2` - the stager's writes with the other clamp.
///
/// Not carried: the creature seat 7's stages and rate bytes (the engine seats
/// no creature for a capture cast), the seat poses and model scales arms 0 /
/// 27 / 28 save and restore, the effect records, the screen fades and the
/// arm-20 banner.
///
/// PORT: FUN_801F6A74, overlay_cast_evil_seru_magic_0966_801f6a74 (PROT 0966; the party and caster writes and the arm-26 hit)
pub fn evil_seru_magic_seat_writes(
    phase: u8,
    passed: bool,
    party_count: u8,
    caster: u8,
    seats: &mut [CastActorState],
    mut roll: impl FnMut(u8) -> i32,
) -> Vec<SweepHit> {
    let mut hits = Vec::new();
    let party = party_count as usize;
    let each_party = |seats: &mut [CastActorState], f: &mut dyn FnMut(&mut CastActorState)| {
        for s in seats.iter_mut().take(party) {
            f(s);
        }
    };
    let hide = |s: &mut CastActorState| {
        s.present_04 = 0;
        s.render_flag = ESM_HIDDEN;
    };
    let show = |s: &mut CastActorState| {
        s.present_04 = ESM_NEUTRAL_TINT;
        s.render_flag = 0;
    };
    match phase {
        0 | 1 => each_party(seats, &mut |s| hide(s)),
        27 => each_party(seats, &mut |s| s.anim_rate = 0),
        28 => each_party(seats, &mut |s| s.anim_rate = 2),
        _ => {}
    }
    if !passed {
        return hits;
    }
    let caster_seat = |seats: &mut [CastActorState], f: &dyn Fn(&mut CastActorState)| {
        if let Some(c) = seats.get_mut(caster as usize) {
            f(c);
        }
    };
    match phase {
        1 => caster_seat(seats, &|c| {
            stage_clip(c, 6);
            c.anim_rate = 2;
        }),
        10 => {
            caster_seat(seats, &|c| hide(c));
            each_party(seats, &mut |s| show(s));
        }
        18 => each_party(seats, &mut |s| hide(s)),
        21 => {
            caster_seat(seats, &|c| {
                stage_clip(c, 0);
                show(c);
            });
            each_party(seats, &mut |s| show(s));
        }
        23 => caster_seat(seats, &|c| c.render_flag = 2),
        26 => {
            for seat in 0..party_count {
                let Some(v) = seats.get_mut(seat as usize) else {
                    break;
                };
                if !aoe_seat_is_hittable(v) {
                    continue;
                }
                let applied = apply_hit_floor_zero(v, roll(seat));
                let knockdown = v.knockdown_anim;
                stage_clip(v, knockdown);
                v.anim_rate = 2;
                hits.push(SweepHit { seat, applied });
            }
        }
        27 => caster_seat(seats, &|c| show(c)),
        _ => {}
    }
    hits
}

#[cfg(test)]
mod esm_tests {
    use super::*;

    fn seat(hp: u16) -> CastActorState {
        CastActorState {
            hp,
            knockdown_anim: 0x0B,
            anim_rate: 8,
            present_04: ESM_NEUTRAL_TINT,
            ..Default::default()
        }
    }

    #[test]
    fn arm_26_hits_the_party_and_can_kill() {
        let mut seats = vec![seat(500), seat(0), seat(40), seat(9000)];
        let hits = evil_seru_magic_seat_writes(26, true, 3, 3, &mut seats, |_| 100);
        assert_eq!(hits.len(), 2, "the dead seat draws nothing");
        assert_eq!(seats[0].hp, 400);
        assert_eq!(seats[2].hp, 0, "shape A clamps to HP and kills");
        assert_eq!(seats[0].staged_anim, 0x0B);
        assert_eq!(seats[0].anim_rate, 2);
        assert_eq!(seats[3].hp, 9000, "the caster is outside the party row");
    }

    #[test]
    fn a_held_arm_makes_only_its_pre_gate_writes() {
        let mut seats = vec![seat(500), seat(500), seat(500), seat(9000)];
        assert!(evil_seru_magic_seat_writes(26, false, 3, 3, &mut seats, |_| 100).is_empty());
        assert_eq!(seats[0].hp, 500);
        evil_seru_magic_seat_writes(1, false, 3, 3, &mut seats, |_| 0);
        assert_eq!(seats[0].render_flag, ESM_HIDDEN);
        assert_eq!(
            seats[3].staged_anim, 0,
            "the caster stage waits for the gate"
        );
        evil_seru_magic_seat_writes(1, true, 3, 3, &mut seats, |_| 0);
        assert_eq!(seats[3].staged_anim, 6);
        assert_eq!(seats[3].anim_rate, 2);
    }

    #[test]
    fn the_party_is_shown_again_before_the_body_ends() {
        let mut seats = vec![seat(500), seat(500), seat(500), seat(9000)];
        for (p, passed) in [(0, true), (10, true), (18, true), (21, true)] {
            evil_seru_magic_seat_writes(p, passed, 3, 3, &mut seats, |_| 0);
        }
        assert!(seats[..3].iter().all(|s| s.render_flag == 0));
        assert!(seats[..3].iter().all(|s| s.present_04 == ESM_NEUTRAL_TINT));
        assert_eq!(seats[3].render_flag, 0);
    }
}
