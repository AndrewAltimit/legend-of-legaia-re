//! PROT 0951's **Chaos Flare** body (`0x801F6A20`, action `0x36`): the camera
//! arms, the flare's two record runs and the seat hides.
//!
//! See `ghidra/scripts/funcs/overlay_cast_chaos_flare_0951_801f6a20.txt`.

use super::v_windhash::SHOWN;
use super::{
    CaptureCamArm, CaptureDrift, CaptureSeatWrite, MODULE_DRAIN_PER_TICK, ModuleCamSeats,
    ModuleCamState, ModuleShot, ModuleSpawn, SPEED_SCALAR, SpawnAnchor, SpawnRecord,
    bearing_12bit_approx, focus_on, yaw_from,
};
use legaia_engine_system::trig::trig12;

/// PROT 0951's Chaos Flare tick body.
pub const CHAOS_FLARE_BODY: u32 = crate::cast_module_ticks::CHAOS_FLARE_TICK;

/// The seven records arm 2 seats on the caster seat through the pool
/// wrapper `FUN_80050ED4` (`a0 = caster + 0x34`, `a1 = caster + 0x44`,
/// scale `0x1000`, `0x801F6D44..0x801F6E28`): the charge.
pub const CHAOS_FLARE_CHARGE_RECORDS: [u32; 7] = [
    0x801F_83FC,
    0x801F_8448,
    0x801F_8494,
    0x801F_8530,
    0x801F_859C,
    0x801F_85E8,
    0x801F_8978,
];

/// The seven records arm 6 seats on the caster after its pool flush
/// (`0x801F7110..0x801F71C4`): the flare.
pub const CHAOS_FLARE_BURST_RECORDS: [u32; 7] = [
    0x801F_8644,
    0x801F_869C,
    0x801F_8704,
    0x801F_8774,
    0x801F_8830,
    0x801F_889C,
    0x801F_895C,
];

/// The clip arm 2 stages on the caster with the `+0x1DC` bump
/// (`li v0,0x8` / `sb v0,0x1da(s4)` at `0x801F6D60`).
const CHARGE_CLIP: u8 = 8;

fn on_caster(records: [u32; 7]) -> [Option<ModuleSpawn>; 8] {
    let mut out = [None; 8];
    for (o, r) in out.iter_mut().zip(records) {
        *o = Some(ModuleSpawn {
            record: SpawnRecord::Module(r),
            anchor: SpawnAnchor::CasterShifted { dy: 0, dz: 0 },
        });
    }
    out
}

/// `sin / 8` as `bgez` / `addiu 7` / `sra 3` take it: toward zero.
fn eighth(v: i16) -> i16 {
    v / 8
}

/// A drift of `n` units of TR z a vsync (`0x800840C0 += delta * n`).
fn push(n: i16) -> CaptureDrift {
    CaptureDrift {
        tr_z: n,
        ..CaptureDrift::default()
    }
}

/// The victim's rate arms 9 and 10 store on every pass, ahead of their gates
/// (`0x801F7520..0x801F753C`, `0x801F75C0..0x801F75DC`): `2` while it still
/// plays its knockdown (`+0x1D9 == +0x1F1`), else `0`, which holds the clip it
/// went on to where it stands. The knockdown's cues ride the slower rate:
/// `battle_noaru_songi_chaos_flare_mid_cast`'s two `0x17` steps land forty
/// vsyncs apart on retail, twenty at the default rate.
fn knockdown_rate(st: &ModuleCamState) -> u8 {
    if st.victim_on_knockdown_clip { 2 } else { 0 }
}

/// **Chaos Flare** (`sltiu 0x0C` through the table at `0x801F69D8`; caster
/// `actor_table[ctx + 0x13]`, victim `actor_table[caster + 0x1DD]`). Every
/// gate drains the word `0x801F89CC` by `scalar * delta` and holds while it
/// stays positive:
///
/// | arm | camera and seats | countdown as it passes |
/// |---|---|---|
/// | 0, 1 (`0x801F6AE0`, `0x801F6BE0`) | the walk into reach, framed by the action SM's case 6 (the host's approach leg) | - |
/// | 2 (`0x801F6C60`) | the caster turns on the victim's display pair `+0x3C` / `+0x40` and steps back an eighth of its heading's `(sin, cos)`; a cut on it: pitch `0x70`, yaw `0x800 - facing`, TR `(0, 0x800, 0x800)`; clip `8` staged with the bump, rate `2`; the charge's seven records on it; every party seat but the victim hidden | `= scalar * 0xE0` |
/// | 3 (`0x801F6EC4`) | the caster's rate `2`; every pass pitch `-delta`, TR z `+product` | `+= scalar << 7` |
/// | 4 (`0x801F6F44`) | a cut on the caster: pitch `0x100`, yaw `-facing`, TR `(0, 0x600, 0xC00)` | `+= scalar << 7` |
/// | 5 (`0x801F6FDC`) | every pass TR y and TR z `+8 * delta` | `+= scalar << 6` |
/// | 6 (`0x801F7050`) | the pool flushed; the flare's seven records on the caster; the stage byte `3`, the backdrop off, the caster's rate `5` | `+= scalar * 0x60` |
/// | 7 (`0x801F7288`) | a cut on the caster: pitch `0x100`, yaw `0xA00 - facing`, TR `(0x300, 0x600, 0x800)` | `+= scalar * 0x60` |
/// | 8 (`0x801F735C`) | every pass TR z `+24 * delta`; a cut on the victim: pitch `0x80`, yaw `-facing`, TR `(0, 0x600, 0x800)`; the strike, the victim turned on the caster, its knockdown at rate `2` | `+= scalar * 0xC0` |
/// | 9 (`0x801F74D0`) | every pass TR z `+12 * delta`; the victim's rate held ([`knockdown_rate`]) | `+= scalar << 7` |
/// | 10 (`0x801F75C0`) | every pass the victim's rate held; the pool flushed; a cut on the caster: pitch `-0x80`, yaw `0x800 - facing`, TR `(0, 0x600, 0x600)`; the caster's clip released and the stage byte `0`; the victim's rate `2` | `+= scalar << 7` |
/// | 11 (`0x801F76F4`) | every pass TR z `+4 * delta`; then every seat with HP shown, rate `8` | - |
///
/// Every cut is one frame. The phase chain, the strike and the victim's
/// settle are [`crate::cast_module_ticks::CHAOS_FLARE_TICK`]'s and the band's,
/// so `next` stays `None`. Not carried: arms 2 and 6's stores into the first
/// monster record's node (`+0x85` / `+0x86`), arm 6's and 7's light words
/// behind `0x800845C8`.
///
/// PORT: overlay_cast_chaos_flare_0951_801f6a20 (PROT 0951 `0x36`; the camera arms, the record runs and the seat hides)
pub fn chaos_flare_camera(
    st: &mut ModuleCamState,
    phase: u8,
    seats: ModuleCamSeats,
) -> CaptureCamArm {
    let (c, v) = (seats.caster, seats.victim);
    let mut arm = CaptureCamArm::default();
    let cut = |angles: [i16; 3], tr: [i16; 3], on: super::ModuleSeat| {
        Some(ModuleShot {
            angles,
            tr,
            focus: focus_on(on),
            frames: 1,
        })
    };
    // A gate that holds returns the arm as it stands.
    macro_rules! gate {
        () => {
            if st.countdown.drain_above(0) {
                arm.hold = true;
                return arm;
            }
        };
    }
    match phase {
        2 => {
            // The turn reads the victim's display pair `+0x3C` / `+0x40`
            // (`lh a0,0x40(s1)` / `lh a1,0x3c(s1)` at `0x801F6C68`).
            let vb = st.victim_body.unwrap_or(v);
            let facing = bearing_12bit_approx(vb.z, vb.x, c.z, c.x).wrapping_add(0x800) & 0xFFF;
            let (s, co) = trig12(facing);
            let at = super::ModuleSeat {
                x: c.x.wrapping_sub(eighth(s)),
                z: c.z.wrapping_sub(eighth(co)),
                facing,
                ..c
            };
            arm.caster_write = CaptureSeatWrite {
                x: Some(at.x),
                z: Some(at.z),
                facing: Some(facing),
                stage: Some(CHARGE_CLIP),
                restage: true,
                rate: Some(2),
                ..Default::default()
            };
            arm.shot = cut([0x70, yaw_from(0x800, facing), 0], [0, 0x800, 0x800], at);
            arm.charge_spawns = on_caster(CHAOS_FLARE_CHARGE_RECORDS);
            arm.party_write = CaptureSeatWrite {
                colour: Some(0),
                render_flag: Some(0xFF),
                ..Default::default()
            };
            arm.party_skip_victim = true;
            st.countdown.0 = SPEED_SCALAR * 0xE0;
        }
        3 => {
            arm.caster_write.rate = Some(2);
            arm.drift = Some(CaptureDrift {
                pitch: -1,
                tr_z: MODULE_DRAIN_PER_TICK as i16,
                ..CaptureDrift::default()
            });
            gate!();
            st.countdown.add(1 << 7);
        }
        4 => {
            gate!();
            arm.shot = cut([0x100, yaw_from(0, c.facing), 0], [0, 0x600, 0xC00], c);
            st.countdown.add(1 << 7);
        }
        5 => {
            arm.drift = Some(CaptureDrift {
                tr_y: 8,
                tr_z: 8,
                ..CaptureDrift::default()
            });
            gate!();
            st.countdown.add(1 << 6);
        }
        6 => {
            gate!();
            arm.flushes_pool = true;
            arm.charge_spawns = on_caster(CHAOS_FLARE_BURST_RECORDS);
            arm.ctx_278 = Some(3);
            arm.backdrop_off = true;
            arm.caster_write.rate = Some(5);
            st.countdown.add(0x60);
        }
        7 => {
            gate!();
            arm.shot = cut(
                [0x100, yaw_from(0xA00, c.facing), 0],
                [0x300, 0x600, 0x800],
                c,
            );
            st.countdown.add(0x60);
        }
        8 => {
            arm.drift = Some(push(24));
            gate!();
            arm.shot = cut([0x80, yaw_from(0, v.facing), 0], [0, 0x600, 0x800], v);
            let facing = bearing_12bit_approx(c.z, c.x, v.z, v.x).wrapping_add(0x800) & 0xFFF;
            arm.victim_write.facing = Some(facing);
            // The knockdown the strike stages plays at rate `2`
            // (`li v1,0x2` / `sb v1,0x21d(s1)` at `0x801F7494`).
            arm.victim_write.rate = Some(2);
            st.countdown.add(0xC0);
        }
        9 => {
            arm.drift = Some(push(12));
            arm.victim_write.rate = Some(knockdown_rate(st));
            gate!();
            st.countdown.add(1 << 7);
        }
        10 => {
            arm.victim_write.rate = Some(knockdown_rate(st));
            gate!();
            // The arm's last store, after the cut (`0x801F76DC`).
            arm.victim_write.rate = Some(2);
            arm.flushes_pool = true;
            arm.shot = cut([-0x80, yaw_from(0x800, c.facing), 0], [0, 0x600, 0x600], c);
            arm.caster_write = CaptureSeatWrite {
                stage: Some(0),
                loop_release: true,
                ..Default::default()
            };
            arm.ctx_278 = Some(0);
            st.countdown.add(1 << 7);
        }
        11 => {
            arm.drift = Some(push(4));
            gate!();
            arm.others_write = CaptureSeatWrite {
                rate: Some(8),
                ..SHOWN
            };
            arm.others_live_only = true;
        }
        _ => {}
    }
    arm
}

#[cfg(test)]
mod tests {
    use super::super::ModuleSeat;
    use super::*;

    fn seats() -> ModuleCamSeats {
        ModuleCamSeats {
            caster: ModuleSeat {
                x: 0,
                y: 0,
                z: 800,
                facing: 0,
            },
            victim: ModuleSeat {
                x: 0,
                y: 0,
                z: -800,
                facing: 0x800,
            },
            action: 0x36,
            ..Default::default()
        }
    }

    /// Arm 2 seats the charge's seven records, hides the party but the
    /// victim, stages the charge clip and cuts on the stepped-back caster.
    #[test]
    fn arm_2_seats_the_charge_and_hides_the_party() {
        let mut st = ModuleCamState::default();
        let a = chaos_flare_camera(&mut st, 2, seats());
        let recs: Vec<_> = a.charge_spawns.iter().flatten().map(|s| s.record).collect();
        assert_eq!(
            recs,
            CHAOS_FLARE_CHARGE_RECORDS.map(SpawnRecord::Module).to_vec()
        );
        assert!(a.party_skip_victim);
        assert_eq!(a.party_write.render_flag, Some(0xFF));
        assert_eq!(a.caster_write.stage, Some(CHARGE_CLIP));
        assert!(a.caster_write.restage);
        let s = a.shot.unwrap();
        assert_eq!(s.tr, [0, 0x800, 0x800]);
        assert_eq!(s.frames, 1);
        assert_eq!(st.countdown.0, SPEED_SCALAR * 0xE0);
    }

    /// The gates hold, then pass and re-arm by the table's amounts; arm 6
    /// flushes the pool and seats the flare; arm 11 shows the field.
    #[test]
    fn the_gates_pace_the_arms() {
        let mut st = ModuleCamState::default();
        chaos_flare_camera(&mut st, 2, seats());
        let mut held = 0;
        while chaos_flare_camera(&mut st, 3, seats()).hold {
            held += 1;
        }
        assert_eq!(held, 0xDF);
        assert_eq!(st.countdown.0, SPEED_SCALAR << 7);
        st.countdown.0 = SPEED_SCALAR;
        let a = chaos_flare_camera(&mut st, 6, seats());
        assert!(a.flushes_pool && a.backdrop_off);
        assert_eq!(a.ctx_278, Some(3));
        assert_eq!(a.charge_spawns.iter().flatten().count(), 7);
        st.countdown.0 = SPEED_SCALAR;
        let a = chaos_flare_camera(&mut st, 11, seats());
        assert_eq!(a.others_write.rate, Some(8));
        assert_eq!(a.others_write.render_flag, Some(0));
    }

    /// Arms 9 and 10 hold the victim at rate `2` on its knockdown and at `0`
    /// off it, on held passes too; arm 10's pass ends on `2`.
    #[test]
    fn arms_9_and_10_hold_the_victims_rate_on_its_knockdown() {
        let mut st = ModuleCamState {
            victim_on_knockdown_clip: true,
            ..ModuleCamState::default()
        };
        st.countdown.0 = SPEED_SCALAR << 7;
        let a = chaos_flare_camera(&mut st, 9, seats());
        assert!(a.hold);
        assert_eq!(a.victim_write.rate, Some(2));
        st.victim_on_knockdown_clip = false;
        let a = chaos_flare_camera(&mut st, 9, seats());
        assert!(a.hold);
        assert_eq!(a.victim_write.rate, Some(0));
        let a = chaos_flare_camera(&mut st, 10, seats());
        assert!(a.hold);
        assert_eq!(a.victim_write.rate, Some(0));
        st.countdown.0 = 1;
        let a = chaos_flare_camera(&mut st, 10, seats());
        assert!(!a.hold);
        assert_eq!(a.victim_write.rate, Some(2));
        st.countdown.0 = 1;
        let a = chaos_flare_camera(&mut st, 8, seats());
        assert!(!a.hold);
        assert_eq!(a.victim_write.rate, Some(2));
    }
}
