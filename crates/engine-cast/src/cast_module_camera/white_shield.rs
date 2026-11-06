//! PROT 0955's **White Shield** body (`0x801F8F0C`, action `0x60`): the
//! camera arms and the module countdown they gate on.
//!
//! See `ghidra/scripts/funcs/overlay_cast_white_shield_0955_801f8f0c.txt`.

use super::{
    CaptureCamArm, ModuleCamSeats, ModuleCamState, ModuleShot, ModuleSpawn, SpawnAnchor,
    SpawnRecord, focus_on, yaw_from,
};

/// The record arm 0 seats on the caster seat (`a0 = caster + 0x34`,
/// `a1 = caster + 0x44`, scale `0x1000`, `0x801F8FB4..0x801F8FC8`): the
/// shield that rises over the caster through arms 1 and 2.
pub const WHITE_SHIELD_RECORD: u32 = 0x801F_93D4;

/// PROT 0955's White Shield tick body.
pub const WHITE_SHIELD_BODY: u32 = crate::cast_module_ticks::WHITE_SHIELD_TICK;

/// The countdown word the six-spell cell shares (`lui 0x8020` / `-0x62D8`).
pub const WHITE_SHIELD_COUNTDOWN: u32 = super::KISS_OF_DEATH_COUNTDOWN;

/// **White Shield** (a `beq` chain over `0..=3`, `0x801F8F6C..0x801F8FA8`;
/// caster `actor_table[ctx + 0x13]`). Every shot is behind the caster, yaw
/// `0x800 - caster[+0x46]`, focus its live pair, TR y `0x400`; `depth` is
/// `ctx[+0x6D0]`:
///
/// | arm | camera | countdown as it passes |
/// |---|---|---|
/// | 0 (`0x801F8FAC`) | pitch `-0x20`, TR z `depth / 2`, `0xC` frames; the shield record on the caster | `= scalar * 0xC` |
/// | 1 (`0x801F9080`) | held, then pitch `0x80`, TR z `depth`, `0x100` frames | `+= scalar << 8` |
/// | 2 (`0x801F9124`) | held, then pitch `0`, TR z `depth`, `0x40` frames | `+= scalar * 0x60` |
/// | 3 (`0x801F91D8`) | held; the buff, and the body returns `0` | - |
///
/// Arms 1 to 3 drain the word by `scalar * delta` and hold while it stays
/// positive (`bgtz` to the epilogue with the busy `1`). Arm 0's depth halves
/// toward zero (`srl 0x1f` / `addu` / `sra 1` at `0x801F9010..0x801F9018`).
/// The body's own seat writes (the render flag of arms 1 and 2, the buff)
/// are [`crate::cast_module_ticks::white_shield_tick`]'s; this director owns
/// the gate that holds it and the one record the body seats, so nothing else
/// of the cell's image goes in
/// ([`super::capture_director_seats_all`]).
///
/// PORT: overlay_cast_white_shield_0955_801f8f0c (PROT 0955 `0x60`; the camera arms and the countdown)
pub fn white_shield_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let c = seats.caster;
    let depth = seats.depth_raw as i16;
    let shot = |pitch: i16, z: i16, frames: u16| {
        Some(ModuleShot {
            angles: [pitch, yaw_from(0x800, c.facing), 0],
            tr: [0, 0x400, z],
            focus: focus_on(c),
            frames,
        })
    };
    match phase {
        0 => {
            st.countdown.0 = super::SPEED_SCALAR * 0xC;
            let mut pass_spawns = [None; 5];
            pass_spawns[0] = Some(ModuleSpawn {
                record: SpawnRecord::Module(WHITE_SHIELD_RECORD),
                anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
            });
            CaptureCamArm {
                shot: shot(-0x20, depth / 2, 0xC),
                next: Some(1),
                pass_spawns,
                ..Default::default()
            }
        }
        1..=3 => {
            if st.countdown.drain_above(0) {
                return CaptureCamArm {
                    hold: true,
                    next: Some(phase),
                    ..Default::default()
                };
            }
            let shot = match phase {
                1 => {
                    st.countdown.add(1 << 8);
                    shot(0x80, depth, 0x100)
                }
                2 => {
                    st.countdown.add(0x60);
                    shot(0, depth, 0x40)
                }
                _ => None,
            };
            CaptureCamArm {
                shot,
                next: (phase < 3).then_some(phase + 1),
                ..Default::default()
            }
        }
        _ => CaptureCamArm::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 207,
                y: 0,
                z: -476,
                facing: 2297,
            },
            depth_raw: 3008,
            ..Default::default()
        }
    }

    /// `battle_bubu2_spoon_white_shield_mid_cast`: the pass after arm 0, the
    /// step table at `ctx[+0x118C]` holds pitch `4064`, yaw `3847`,
    /// `TR (0, 1024, 1504)` (raw; `2406` prescaled) on Spoon's live pair
    /// `(207, -476)`, and the countdown word reads `96`.
    #[test]
    fn arm_0_is_the_capture_s_shot() {
        let mut st = ModuleCamState::default();
        let a = white_shield_camera(&mut st, 0, seats());
        let s = a.shot.expect("arm 0 cuts");
        assert_eq!(s.angles, [-0x20, 3847, 0]);
        assert_eq!(s.tr, [0, 0x400, 1504]);
        assert_eq!(s.focus, [-207, 0, 476]);
        assert_eq!(s.frames, 0xC);
        assert_eq!(st.countdown.0, 96);
        assert_eq!(
            a.pass_spawns[0].map(|s| s.record),
            Some(SpawnRecord::Module(WHITE_SHIELD_RECORD))
        );
    }

    /// Arms 1 and 2 hold on the word, then cut and re-arm it; arm 3 finishes.
    #[test]
    fn the_arms_hold_on_the_word() {
        let mut st = ModuleCamState::default();
        white_shield_camera(&mut st, 0, seats());
        let mut held = 0;
        while white_shield_camera(&mut st, 1, seats()).hold {
            held += 1;
        }
        assert_eq!(held, 11, "scalar * 0xC drains in twelve passes");
        let mut held = 0;
        loop {
            let a = white_shield_camera(&mut st, 2, seats());
            if !a.hold {
                assert_eq!(a.shot.unwrap().angles[0], 0);
                break;
            }
            held += 1;
        }
        assert_eq!(held, 255);
        let mut n = 0;
        loop {
            let a = white_shield_camera(&mut st, 3, seats());
            if !a.hold {
                assert!(a.shot.is_none());
                assert_eq!(a.next, None);
                break;
            }
            n += 1;
        }
        assert_eq!(n, 0x5F);
    }
}
