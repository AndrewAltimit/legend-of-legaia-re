//! PROT 0932 (Meta), 0933 (Terra) and 0934 (Ozma): each module's whole
//! phase chain as a director ([`ModuleProfile::director_body`]), read off
//! its tick's disassembly. All three drain their word by the frame delta
//! alone and re-arm it by literals; none stores `0xFF` - its terminal arm
//! returns `0` and the port stands at `0xFF` for it.
//!
//! Not carried, as in [`super::gola_mushura_barra`]: the stage, lighting and
//! camera-block words, and every hit beyond the one the band's fold lands on
//! the hit arm. The stores on other seats are `World::apply_meta_writes` and
//! `World::apply_ozma_writes`.
//!
//! See `ghidra/scripts/funcs/overlay_summon_meta_0932_801f6a34.txt`,
//! `overlay_summon_terra_0933_801f6a30.txt` and
//! `overlay_summon_ozma_0934_801f6a40.txt`.
//!
//! The arms' fades are not reported here: the presentation hook
//! (`World::fire_cast_presentation`) fires them off the image's own decoded
//! call sites, on the arm timing these directors pace.

use super::body_kit::*;
use super::*;

/// The delta-drained gate with a literal re-arm on exit.
fn gated(st: &mut ModuleCamState, add: i32) -> bool {
    if counted(st, D) {
        return true;
    }
    st.countdown.0 += add;
    false
}

/// A gated arm whose per-pass drift is `d` and whose exit is `exit`.
fn arm(
    st: &mut ModuleCamState,
    add: i32,
    d: Option<CaptureDrift>,
    exit: ArmDirection,
) -> ArmDirection {
    if gated(st, add) {
        return held(d);
    }
    with_drift(exit, d)
}

// ---------------------------------------------------------------------------
// PROT 0932 - Meta
// ---------------------------------------------------------------------------

/// PROT 0932's hit arm: its pass sweeps the monsters with a clamp that can
/// kill (`0x801F7E2C`). Arm `0x12`'s earlier sweep cannot kill and is not
/// carried.
pub const META_HIT_ARM: u8 = 0x15;

/// PROT 0932's terminal arm (`0x801F8140`).
pub const META_LAST_ARM: u8 = 0x16;

/// PROT 0932 (Meta): the opening (arms 0..5) frames the stage origin and
/// seats the creature at `(-0x96, -0x1C0)`; arms 6..0x16 alternate drifts
/// written into the camera globals with one-frame cuts.
///
/// | arm | re-arm | per-pass drift | exit |
/// |---|---|---|---|
/// | 0 | word `= 0xC0` | - | a cut |
/// | 1 | passes below `0x41` | pitch `-4`, TR y `+12`, TR z `-12` | - |
/// | 2, 3 | word `= 0x60` (3) | - | - |
/// | 4, 5 | `0x2E`, `8` | - | the creature seated, clip `1`; cuts |
/// | 6 | `0x60` | TR y `-140`, TR z `-128` | - |
/// | 7 | `0x100` | - | a cut |
/// | 8 | one drain | - | a `0x100`-frame pan |
/// | 9, 0xA | `0xB4`, `0xC0` | - | the spell name; the attack name, a cut, clip `2` |
/// | 0xB..0xF | `0x80`, `0x100`, `0x140`, `0x140`, `0x100` | pitch / yaw / TR (see the arms) | cuts on `(0, 0, 0x1C0)`, clips `3..5` |
/// | 0x10, 0x11 | `0x78`, `0x10` | pitch `+1`, TR z `+4` | a cut (0x11) |
/// | 0x12, 0x13 | `0x40`, `0x20` | - | an `0x18`-frame shot re-armed each pass; a fade (0x12) |
/// | 0x14 | `0x40` | - | a cut |
/// | 0x15 | `0x120` | TR z `+1` | the hit, three fades |
/// | 0x16 | - | TR z `+1` | the row settled and the word spent: a fade out, the caption cleared, `ctx[+0x6DA] = 0`, the loop counter cleared; done |
///
/// The absolute TR y / z stores of arm 6's exit (`0x3A0`, `0x720`) and arm
/// 4's camera block `0x80083FF8` are not carried.
///
/// PORT: overlay_summon_meta_0932_801f6a34 (PROT 0932; the whole phase chain's gates, camera writes, captions, creature seat and clips - the other seats are `World::apply_meta_writes`; the lighting and camera-block words and the per-call tail not carried)
pub fn meta_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let at = [0, 0, 0x1C0];
    let shot12 = || cut([0x40, 0xA64], [0, 0x500, 0x1288], [-0xB0, 0, 0x80], 0x18);
    let d = |pitch: i32, yaw: i32, tr_x: i32, tr_y: i32, tr_z: i32| {
        Some(drift(pitch, yaw, tr_x, tr_y, tr_z))
    };
    match phase {
        0 => {
            st.countdown.0 = 0xC0;
            cut([0, 0x800], [0, 0x400, 0x800], [0; 3], 1)
        }
        1 => {
            st.countdown.0 -= D;
            let swing = d(-4 * D, 0, 0, 12 * D, -12 * D);
            if st.countdown.0 < 0x41 {
                st.countdown.drain_rest(D);
                with_drift(ArmDirection::PASS, swing)
            } else {
                held(swing)
            }
        }
        2 => ArmDirection::PASS,
        3 => {
            st.countdown.0 = 0x60;
            ArmDirection::PASS
        }
        4 => {
            if gated(st, 0x2E) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat {
                x: -0x96,
                y: 0,
                z: -0x1C0,
                facing: 0,
            });
            st.creature_stage = Some(1);
            cut([-0x3C0, 0x80], [-0x5C8, 0x600, -0x1000], [0; 3], 1)
        }
        5 => arm(
            st,
            8,
            None,
            cut([0xA0, 0xCD4], [-0x200, 0x800, 0x6F4], [0; 3], 1),
        ),
        6 => arm(st, 0x60, d(0, 0, 0, -140 * D, -128 * D), ArmDirection::PASS),
        7 => arm(
            st,
            0x100,
            None,
            cut([0x100, 0x800], [-0x23C, -0x100, 0x800], [0; 3], 1),
        ),
        8 => {
            st.countdown.pass_drain(D);
            cut([0, 0x800], [-0x23C, 0xC00, 0], [0; 3], 0x100)
        }
        9 => arm(
            st,
            0xB4,
            None,
            caption(ArmDirection::PASS, ModuleCaption::SpellName),
        ),
        0xA => {
            if gated(st, 0xC0) {
                return ArmDirection::HOLD;
            }
            // `+0x1DA` and `+0x1DC` together (`0x801F750C`).
            st.creature_stage = Some(2);
            st.creature_restage = true;
            caption(
                cut([0x100, 0xA00], [0, 0x880, 0xD48], [0x96, 0, 0x1C0], 1),
                ModuleCaption::AttackName,
            )
        }
        0xB => arm(
            st,
            0x80,
            d(-2 * D, -5 * D, 0, 2 * D, -12 * D),
            cut([0, -0x12C], [0x20, 0x320, 0x618], [0; 3], 1),
        ),
        0xC => {
            if gated(st, 0x100) {
                return held(d(0, 0, D, 0, 0));
            }
            // `+0x1DA` and `+0x1DC` together (`0x801F7708`).
            st.creature_stage = Some(3);
            st.creature_restage = true;
            with_drift(
                cut([0, 0x800], [-0x23C, 0xA00, 0x800], at, 1),
                d(0, 0, D, 0, 0),
            )
        }
        0xD => {
            st.creature_stage = Some(4);
            arm(
                st,
                0x140,
                d(0, 0, 0, 0, 4 * D),
                cut([0xDC, 0x198], [-0x23C, 0x968, 0x870], at, 1),
            )
        }
        0xE => {
            let dir = arm(
                st,
                0x140,
                d(D, -D, D, 0, 2 * D),
                cut([-0x100, 0xA30], [0x260, 0xA08, 0x200], at, 1),
            );
            // The pass past its countdown cuts to clip `4`, which arm `0xD`
            // staged bare (`0x801F78C8..0x801F78E0`).
            if !dir.hold {
                st.creature_stage = Some(4);
                st.creature_restage = true;
            }
            dir
        }
        0xF => {
            st.creature_stage = Some(5);
            arm(
                st,
                0x100,
                d(0, 0, 0, 0, 4 * D),
                cut([-0x100, 0x800], [0x1C, 0xA30, 0x600], at, 1),
            )
        }
        0x10 => arm(st, 0x78, d(D, 0, 0, 0, 4 * D), ArmDirection::PASS),
        0x11 => arm(
            st,
            0x10,
            d(0, 0, 0, 0, 4 * D),
            cut([0x40, 0xA64], [0, 0x500, 0xA88], [-0xB0, 0, 0x80], 1),
        ),
        0x12 => {
            if gated(st, 0x40) {
                return ArmDirection {
                    hold: true,
                    ..shot12()
                };
            }
            shot12()
        }
        0x13 => {
            let hold = gated(st, 0x20);
            ArmDirection { hold, ..shot12() }
        }
        0x14 => arm(
            st,
            0x40,
            None,
            cut([0x100, 0x864], [0, 0x200, 0x1200], [0; 3], 1),
        ),
        META_HIT_ARM => arm(st, 0x120, d(0, 0, 0, 0, D), ArmDirection::PASS),
        META_LAST_ARM => {
            let dz = d(0, 0, 0, 0, D);
            let drained = st.countdown.0 > 0;
            if drained {
                st.countdown.0 -= D;
            }
            if !st.row_settled || st.countdown.0 > 0 {
                return held(dz);
            }
            if drained {
                st.countdown.drain_rest(D);
            }
            st.yaw_base = 0;
            st.creature_loop_release = true;
            finish(caption(
                with_drift(ArmDirection::PASS, dz),
                ModuleCaption::Cleared,
            ))
        }
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0933 - Terra
// ---------------------------------------------------------------------------

/// PROT 0933's hit arm: its pass sweeps the monsters with a clamp that can
/// kill. Arm `0xC`'s sweep cannot kill and is not carried.
pub const TERRA_HIT_ARM: u8 = 0x13;

/// PROT 0933's terminal arm (`0x801F8628`).
pub const TERRA_LAST_ARM: u8 = 0x15;

/// PROT 0933 (Terra): the creature stands on the stage origin, every framing
/// a literal.
///
/// | arm | re-arm | per-pass drift | exit |
/// |---|---|---|---|
/// | 0 | - | - | a cut |
/// | 1, 2 | word `= 0x100` (2) | - | a `0x140`-frame rise, the creature seated, clip `1` (2) |
/// | 3, 4 | `0x20`, `0x60` | - | a white hold; a cut and the white faded out |
/// | 5 | one drain | - | a `0x60`-frame push in |
/// | 6, 7 | `0xB4`, `0x120` | - | the spell name; the attack name, a cut, clip `2` |
/// | 8..0xC | `0xA0`, `0x100`, `0x120`, `0x40`, `0x120` | yaw / TR / pitch (see the arms) | cuts; clip `3` (0xA) |
/// | 0xD | - | - | - |
/// | 0xE | `0x100` | TR z `+2` | a cut, clip `1` |
/// | 0xF | one drain | TR z `+8`, TR y `-3` | - |
/// | 0x10, 0x11 | `0x80` | TR z `+8`, TR y `-3` / `-14`, pitch `+2` (0x11) | clips `4`, `5` |
/// | 0x12 | `0x20` | TR z `+48`, TR y `-12`, pitch `+1` | - |
/// | 0x13 | `0x100` | - | the hit, three fades |
/// | 0x14 | `0x80` | TR z `+1` | a fade out |
/// | 0x15 | - | - | the word spent: the caption cleared, `ctx[+0x6DA] = 0`; done |
///
/// PORT: overlay_summon_terra_0933_801f6a30 (PROT 0933; the whole phase chain's gates, camera writes, captions, creature seat and clips - the monster orbit and its seats, part spawns and their pool loop, the colour pulses (`0x8004695C` washes) and the arm-0xC sweep not carried)
pub fn terra_direct(st: &mut ModuleCamState, phase: u8, _seats: ModuleCamSeats) -> ArmDirection {
    let d = |pitch: i32, yaw: i32, tr_x: i32, tr_y: i32, tr_z: i32| {
        Some(drift(pitch, yaw, tr_x, tr_y, tr_z))
    };
    match phase {
        0 => cut([-0xA0, 0x800], [0, 0x700, 0x1800], [0; 3], 1),
        1 => ArmDirection::PASS,
        2 => {
            st.countdown.0 = 0x100;
            st.creature = Some(ModuleSeat::default());
            st.creature_stage = Some(1);
            cut([0xA0, 0x800], [0, 0x700, 0], [0; 3], 0x140)
        }
        3 => arm(st, 0x20, None, ArmDirection::PASS),
        4 => arm(st, 0x60, None, cut([0, 0x800], [0, 0x700, 0], [0; 3], 1)),
        5 => {
            st.countdown.pass_drain(D);
            cut([-0xC0, 0x800], [0, 0x700, 0x118F], [0; 3], 0x60)
        }
        6 => arm(
            st,
            0xB4,
            None,
            caption(ArmDirection::PASS, ModuleCaption::SpellName),
        ),
        7 => {
            if gated(st, 0x120) {
                return ArmDirection::HOLD;
            }
            // `+0x1DA` and `+0x1DC` together (`0x801F7398`, `0x801F73A0`).
            st.creature_stage = Some(2);
            st.creature_restage = true;
            caption(
                cut([0, 0x700], [0, 0xA00, 0x400], [0; 3], 1),
                ModuleCaption::AttackName,
            )
        }
        8 => arm(
            st,
            0xA0,
            d(0, -4 * D, 0, 2 * D, D),
            cut([-0x40, 0x780], [0, 0x600, 0x1200], [0; 3], 1),
        ),
        9 => arm(
            st,
            0x100,
            d(0, 0, 0, 0, 12 * D),
            cut([0x100, 0x800], [0, -0x5C0, 0x1500], [0; 3], 1),
        ),
        0xA => {
            if gated(st, 0x120) {
                return held(d(-D, 0, 0, 12 * D, 0));
            }
            // Together (`0x801F7834`, `0x801F783C`).
            st.creature_stage = Some(3);
            st.creature_restage = true;
            with_drift(
                cut([-0x40, 0x800], [0, 0xA3C, 0x240], [0; 3], 1),
                d(-D, 0, 0, 12 * D, 0),
            )
        }
        0xB => arm(
            st,
            0x40,
            d(0, 0, 2 * D, 4 * D, 2 * D),
            cut([-0x80, 0x800], [0, 0x1480, 0x1400], [0; 3], 1),
        ),
        0xC => arm(
            st,
            0x120,
            d(0, 0, 0, D, D),
            // TR y is the row's middle: `-4 * +0x3E` over the living monsters
            // (`subu s3,s3,v0` at `0x801F7CD0`, `div` at `0x801F7D54`).
            cut(
                [0, 0],
                [0, i32::from(st.row_tr_y), 0x800],
                [0, 0, -0x613],
                1,
            ),
        ),
        0xD => ArmDirection::PASS,
        0xE => {
            if gated(st, 0x100) {
                return held(d(0, 0, 0, 0, 2 * D));
            }
            // Together (`0x801F7E24`, `0x801F7E2C`).
            st.creature_stage = Some(1);
            st.creature_restage = true;
            with_drift(
                cut([-0x80, 0x800], [0, 0xA00, 0x240], [0; 3], 1),
                d(0, 0, 0, 0, 2 * D),
            )
        }
        0xF => {
            st.countdown.pass_drain(D);
            with_drift(ArmDirection::PASS, d(0, 0, 0, -3 * D, 8 * D))
        }
        0x10 => {
            st.creature_stage = Some(4);
            arm(st, 0x80, d(0, 0, 0, -3 * D, 8 * D), ArmDirection::PASS)
        }
        0x11 => {
            st.creature_stage = Some(5);
            arm(st, 0x80, d(2 * D, 0, 0, -14 * D, 8 * D), ArmDirection::PASS)
        }
        0x12 => arm(st, 0x20, d(D, 0, 0, -12 * D, 48 * D), ArmDirection::PASS),
        TERRA_HIT_ARM => arm(st, 0x100, None, ArmDirection::PASS),
        0x14 => arm(st, 0x80, d(0, 0, 0, 0, D), ArmDirection::PASS),
        TERRA_LAST_ARM => {
            if counted(st, D) {
                return ArmDirection::HOLD;
            }
            st.yaw_base = 0;
            finish(caption(ArmDirection::PASS, ModuleCaption::Cleared))
        }
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0934 - Ozma
// ---------------------------------------------------------------------------

/// PROT 0934's hit arm: its exit sweeps the living, targetable monsters
/// through `FUN_801DD0AC(0x10, 7, seat)` (the hit ladder, `0x801F8D70..
/// 0x801F9108`). The ladder's three earlier hits are not carried.
pub const OZMA_HIT_ARM: u8 = 0x18;

/// PROT 0934's terminal arm (`0x801F9124`).
pub const OZMA_LAST_ARM: u8 = 0x19;

/// The trail id PROT 0934's lightning passes the ribbon (`li a1,0xC` at
/// `0x801F6EE8`).
pub const OZMA_BOLT_TRAIL: u8 = 0xC;

/// The eight ground points PROT 0934's arms 5..9 hand `FUN_801E1D98` on
/// every pass that holds (`0x801F6E80..0x801F6EF4`): the table angle `i *
/// 0x200`, `x = sin / 4`, `z = cos / 8 - 0x500`, each toward zero, at height
/// `-0xC0`.
pub fn ozma_bolt_ring() -> capture::CastRibbonRing {
    let mut points = [[0i16; 3]; 16];
    for (i, p) in points.iter_mut().take(8).enumerate() {
        let (sin, cos) = legaia_engine_system::trig::trig12((i as u16) << 9);
        *p = [sin / 4, -0xC0, cos / 8 - 0x500];
    }
    capture::CastRibbonRing {
        points,
        trail_id: OZMA_BOLT_TRAIL,
        count: 8,
    }
}

fn with_ring(dir: ArmDirection) -> ArmDirection {
    ArmDirection {
        ribbons: Some(ozma_bolt_ring()),
        ..dir
    }
}

/// The vsyncs one retail frame takes through PROT 0934's arms 5..9: the frame
/// step reads `4` on every capture of them. The arms' words count frames, and
/// the engine's tick is a vsync, so the shake's low bit is read per this many
/// ticks: the camera does not jump `0x200` on alternate vsyncs.
const OZMA_SHAKE_TICKS: u32 = 4;

/// One pass of the camera shake PROT 0934's arms 5..9 store straight into
/// TR y: the pass word `0x801FA408` read for its low bit and bumped, the
/// rise word `0x801FA40C` moved by `step` (`0x801F6FBC..0x801F7018`). The
/// low bit and the moved word.
fn ozma_shake_step(st: &mut ModuleCamState, step: i32) -> (i32, i32) {
    let odd = ((st.ozma_shake.0 / OZMA_SHAKE_TICKS) & 1) as i32;
    st.ozma_shake.0 = st.ozma_shake.0.wrapping_add(1);
    st.ozma_shake.1 += step;
    (odd, st.ozma_shake.1)
}

/// The live TR y the host fed, so an absolute store can travel as a drift.
fn live_tr_y(st: &ModuleCamState) -> i32 {
    st.live_pitch_tr.map_or(0, |(_, tr)| tr[1])
}

/// PROT 0934 (Ozma): every framing aims at the raw focus `(0, 0, 0x400)`.
///
/// | arm | re-arm | per-pass drift | exit |
/// |---|---|---|---|
/// | 0, 1 | - | - | a cut, an `0x80`-frame swing |
/// | 2, 3 | - | - | the stream request skips arm 3 when the band timer is spent; arm 3 waits for it |
/// | 4 | word `= 0x20` | - | - |
/// | 5 | `0xC0` | yaw `+1`, pitch `-1`, TR z `+8` | the creature seated at `(0, -0x600)`, its floor clip raised, clip `1` |
/// | 6..9 | `0x50`, `0x50`, `0x30`, `0x30` | pitch `-1`, TR z `+8` / `+24` / `+48` / `+4` (6: yaw `+1`) | cuts (6, 7, 9); the floor clip dropped (9) |
/// | 0xA..0xD | `0x60`, `0x10`, `0x18`, `0x38` | TR y `+24` (0xB) | a cut and clip `2` (0xC) |
/// | 0xE, 0xF | `0xA0`, `0xF0` | TR z `+64`, TR y `-16` (0xE) | fades; the spell name (0xF) |
/// | 0x10 | `0x200` | - | a cut, the attack name, clip `3` |
/// | 0x11, 0x13, 0x16 | one drain | TR z `+8` pitch `+1`; TR z `+128` TR y `-16` TR x `+12`; TR z `+128` TR y `-48` pitch `-1` | - |
/// | 0x12, 0x14, 0x15, 0x17 | `0x80`, `0x100`, `0x80`, `0x80` | as the drain arm before each | cuts (0x12, 0x15) and dim fades |
/// | 0x18 | `0x100` | - | the hit |
/// | 0x19 | - | - | the word spent: the caption cleared, `ctx[+0x6DA] = 0`, the loop counter cleared; done |
///
/// Arms 5..9's absolute TR y stores (a rise word with a one-frame shake) are
/// [`ozma_shake_step`], their lightning [`ozma_bolt_ring`]. Arm 0x15's focus,
/// the last random spawn's point, aims at `(0, 0, 0x400)`.
///
/// PORT: overlay_summon_ozma_0934_801f6a40 (PROT 0934; the whole phase chain's gates, camera writes, captions, creature seat and clips - the other seats are `World::apply_ozma_writes`; the hit ladder's earlier hits not carried)
pub fn ozma_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let at = [0, 0, 0x400];
    let d = |pitch: i32, yaw: i32, tr_x: i32, tr_y: i32, tr_z: i32| {
        Some(drift(pitch, yaw, tr_x, tr_y, tr_z))
    };
    let flare = d(0, 0, 12 * D, -16 * D, 128 * D);
    let sink = d(-D, 0, 0, -48 * D, 128 * D);
    match phase {
        0 => cut([0x180, 0x700], [0, 0, 0x1000], at, 1),
        1 => cut([0x180, 0x6E8], [0, 0, 0x1800], at, 0x80),
        2 if seats.band_timer == 0 => goto(ArmDirection::PASS, 4),
        2 => ArmDirection::PASS,
        3 => gate(seats.band_timer != 0),
        4 => {
            st.countdown.0 = 0x20;
            st.ozma_shake = (0, 0);
            ArmDirection::PASS
        }
        5 => {
            let (odd, rise) = ozma_shake_step(st, D);
            let dd = d(-D, D, 0, odd * 0x80 + rise - live_tr_y(st), 8 * D);
            if gated(st, 0xC0) {
                return with_ring(held(dd));
            }
            st.creature = Some(ModuleSeat {
                x: 0,
                y: 0,
                z: -0x600,
                facing: 0,
            });
            st.creature_floor_clip = true;
            st.creature_stage = Some(1);
            with_ring(with_drift(ArmDirection::PASS, d(-D, D, 0, 0, 8 * D)))
        }
        6 => {
            let (odd, rise) = ozma_shake_step(st, 4 * D);
            let dd = d(-D, D, 0, odd * 0x200 + rise - live_tr_y(st), 8 * D);
            if gated(st, 0x50) {
                return with_ring(held(dd));
            }
            with_ring(with_drift(
                cut([0x180, 0x800], [0, 0xC00, 0xC00], at, 1),
                d(-D, D, 0, 0, 8 * D),
            ))
        }
        7 => {
            let (odd, _) = ozma_shake_step(st, 0);
            let dd = d(-D, 0, 0, odd * 0xC0 + 0xC00 - live_tr_y(st), 24 * D);
            if gated(st, 0x50) {
                return with_ring(held(dd));
            }
            st.ozma_shake.1 = 0x80;
            with_ring(with_drift(
                cut([-0x80, 0x800], [0, 0xE00, 0x600], at, 1),
                d(-D, 0, 0, 0, 24 * D),
            ))
        }
        8 => {
            let (odd, rise) = ozma_shake_step(st, -D);
            let dd = d(-D, 0, 0, odd * rise + 0xE00 - live_tr_y(st), 48 * D);
            with_ring(arm(st, 0x30, dd, ArmDirection::PASS))
        }
        9 => {
            let (odd, rise) = ozma_shake_step(st, -D);
            let dd = d(-D, 0, 0, odd * rise + 0xE00 - live_tr_y(st), 4 * D);
            if gated(st, 0x30) {
                return with_ring(held(dd));
            }
            st.creature_floor_clip = false;
            with_drift(
                cut([0x180, 0x900], [-0x300, 0x600, 0x1000], at, 1),
                d(-D, 0, 0, 0, 4 * D),
            )
        }
        0xA => arm(st, 0x60, None, ArmDirection::PASS),
        0xB => arm(st, 0x10, d(0, 0, 0, 24 * D, 0), ArmDirection::PASS),
        0xC => {
            if gated(st, 0x18) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(2);
            cut([0, 0x800], [0, 0x1600, 0x400], at, 1)
        }
        0xD => arm(st, 0x38, None, ArmDirection::PASS),
        0xE => arm(st, 0xA0, d(0, 0, 0, -16 * D, 64 * D), ArmDirection::PASS),
        0xF => {
            let dir = arm(
                st,
                0xF0,
                None,
                caption(ArmDirection::PASS, ModuleCaption::SpellName),
            );
            // The bump alone (`0x801F7DAC`) cuts to the clip arm `0xC` staged
            // bare.
            if !dir.hold {
                st.creature_stage = Some(2);
                st.creature_restage = true;
            }
            dir
        }
        0x10 => {
            if gated(st, 0x200) {
                return ArmDirection::HOLD;
            }
            // `+0x1DA` and `+0x1DC` together (`0x801F7E7C..0x801F7E94`).
            st.creature_stage = Some(3);
            st.creature_restage = true;
            caption(
                cut([0, 0x800], [0, 0xC00, 0x1000], at, 1),
                ModuleCaption::AttackName,
            )
        }
        0x11 => {
            st.countdown.pass_drain(D);
            with_drift(ArmDirection::PASS, d(D, 0, 0, 0, 8 * D))
        }
        0x12 => arm(
            st,
            0x80,
            d(D, 0, 0, 2 * D, 8 * D),
            cut([0x180, 0x900], [0, 0x1000, 0x1000], at, 1),
        ),
        0x13 | 0x16 => {
            st.countdown.pass_drain(D);
            with_drift(ArmDirection::PASS, if phase == 0x13 { flare } else { sink })
        }
        0x14 => arm(st, 0x100, flare, ArmDirection::PASS),
        0x15 => arm(
            st,
            0x80,
            None,
            cut([0x200, 0x800], [0, 0x1400, 0x1400], at, 1),
        ),
        0x17 => arm(st, 0x80, sink, ArmDirection::PASS),
        OZMA_HIT_ARM => arm(st, 0x100, None, ArmDirection::PASS),
        OZMA_LAST_ARM => {
            if counted(st, D) {
                return ArmDirection::HOLD;
            }
            st.yaw_base = 0;
            st.creature_loop_release = true;
            finish(caption(ArmDirection::PASS, ModuleCaption::Cleared))
        }
        _ => ArmDirection::HOLD,
    }
}
