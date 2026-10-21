//! PROT 0914 (Gola Gola), 0915 (Mushura) and 0917 (Barra): each module's
//! whole phase chain as a director ([`ModuleProfile::director_body`]),
//! read off its tick's disassembly. Each countdown is a word in the module's
//! own image drained by the scalar times the delta ([`P`] a tick).
//!
//! What a director here carries: every gate and phase store, the shots, the
//! per-pass writes into the camera globals, the captions, the fades, the
//! creature seat and the clips the arms stage on it. What it does not: the
//! part spawns (the stager seats the module's record set on its first tick),
//! the writes to the other seats (hiding, scattering and re-seating the
//! monster row), and every hit beyond the one the band's fold lands on the
//! module's hit arm.
//!
//! See `ghidra/scripts/funcs/overlay_summon_gola_gola_0914_801f69f0.txt`,
//! `overlay_summon_mushura_0915_801f69d8.txt` and
//! `overlay_summon_barra_0917_801f6a30.txt`.
//!
//! The arms' fades are not reported here: the presentation hook
//! (`World::fire_cast_presentation`) fires them off the image's own decoded
//! call sites, on the arm timing these directors pace.

use super::body_kit::*;
use super::*;

// ---------------------------------------------------------------------------
// PROT 0914 - Gola Gola
// ---------------------------------------------------------------------------

/// PROT 0914's hit arm: its pass sweeps the living monster row through
/// `FUN_801DD0AC(0x12, 7, seat)` (`0x801F74A4..0x801F7890`).
pub const GOLA_GOLA_HIT_ARM: u8 = 0xD;

/// The cap of the creature's colour-word ramp in arm `0xE` (`0x801F790C..
/// 0x801F792C`).
pub const GOLA_GOLA_SHATTER_CAP: u16 = 0x7530;

/// PROT 0914 (Gola Gola): every framing aims at the caster offset by
/// `(dx, -0x267)`.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0, 1 | - | cuts (`0x801F6B2C`, `0x801F6BA4`) |
/// | 2, 3 | - (the load) | TR z `+= 4 delta` a pass; arm 3 seats the creature `(0x400, -0x267)` off the caster, facing `0`, raises its floor clip, stages clip `1` and frames it over `0x40` frames |
/// | 4..6 | the creature's clip cursor `+0x68` past `0x100` / `0x200` / `0x300` | three `0x40` / `0x70`-frame framings |
/// | 7 | creature clip `0` | `0x40`-frame rise; clip `2` staged |
/// | 8 | the cursor past `0x200` | `0x40`-frame framing, floor clip dropped, the spell name |
/// | 9 | until clip `2` plays | word `= scalar * 180` |
/// | 0xA | word | a `0xC0`-frame tilt onto the caster, the attack name, clip `3` |
/// | 0xB | the clip's park (`+0x21B` off `0xFF`) | yaw `+= 4 p` a pass; word `= 0`; a red fade in |
/// | 0xC | word `+= p` until `word >> 4 + 2` reaches `0x20` | yaw and TR z `+= 4 p`; the park re-armed and the rate ramped to that byte; word `= scalar * 0x70` |
/// | 0xD | word | yaw / TR z `+= 4 p`, pitch `-= p`; on its pass the hit, a `0x40`-frame framing of the target (a group by its centroid, TR z four extents; a seat at two thirds of the depth), two fades; word `+= scalar << 7` |
/// | 0xE | the row settled and the word spent | the creature breaks up (`+= delta << 7` a pass, capped at `0x7530`); yaw `+= p / 2`, TR z `+= 4 p`; then `0xFF` |
/// | 0xFF | - | `ctx[+0x6DA] = 0`; the creature's loop counter cleared; done |
///
/// The frame and park waits are bounded ([`HOST_WAIT_CAP`]) so a creature
/// the host stops animating cannot hold the band.
///
/// PORT: FUN_801F69F0 (PROT 0914; the whole phase chain's gates, camera writes, captions, creature seat, clips, rate, loop window and colour word - the row's reaction writes not carried)
pub fn gola_gola_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let c = seats.caster;
    let at = |dx: i32| [-i32::from(c.x) - dx, 0, -i32::from(c.z) + 0x267];
    let tr_z = Some(drift(0, 0, 0, 0, 4 * D));
    match phase {
        0 => cut([0, 0x800], [0, 0x600, 0x600], at(0x400), 1),
        1 => cut([0x200, 0x400], [0, 0, 0x600], at(0x18E), 1),
        2 => with_drift(ArmDirection::PASS, tr_z),
        3 => {
            st.creature = Some(ModuleSeat {
                x: (i32::from(c.x) + 0x400) as i16,
                y: 0,
                z: (i32::from(c.z) - 0x267) as i16,
                facing: 0,
            });
            st.creature_floor_clip = true;
            st.creature_stage = Some(1);
            with_drift(
                cut([-0x180, 0x400], [0, 0x600, 0x100], at(0x400), 0x40),
                tr_z,
            )
        }
        // `lh v0,0x68(node); slti v0,v0,0x100 / 0x200 / 0x300; bne` - the
        // arms hold until the creature's clip cursor reaches the frame
        // (`0x801F6D88`, `0x801F6E30`, `0x801F6EC4`).
        4 if frame_wait(st, phase, 0x100) => ArmDirection::HOLD,
        5 if frame_wait(st, phase, 0x200) => ArmDirection::HOLD,
        6 if frame_wait(st, phase, 0x300) => ArmDirection::HOLD,
        4 => cut([-0x180, 0xC00], [0, 0x600, 0x100], at(0x400), 0x40),
        5 => cut([0x200, 0xC00], [0, 0, 0x800], at(0x672), 0x40),
        6 => cut([0, 0x800], [0, 0x600, 0x1000], at(0x400), 0x70),
        7 => {
            if clip_wait(st, |k| k == 0) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(2);
            cut([-0x100, 0x800], [0, 0x2000, 0x1000], at(0x400), 0x40)
        }
        8 => {
            // `slti v0,v0,0x200` on the cursor at `0x801F7004`.
            if frame_wait(st, phase, 0x200) {
                return ArmDirection::HOLD;
            }
            st.creature_floor_clip = false;
            caption(
                cut([-0x80, 0x900], [0x200, 0x600, 0x800], at(0x400), 0x40),
                ModuleCaption::SpellName,
            )
        }
        9 => {
            if clip_wait(st, |k| k != 2) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = S * 180;
            ArmDirection::PASS
        }
        0xA => {
            if counted(st, P) {
                return ArmDirection::HOLD;
            }
            st.creature_stage = Some(3);
            caption(
                cut([0x400, 0x900], [0, 0, 0x800], on(c), 0xC0),
                ModuleCaption::AttackName,
            )
        }
        0xB => {
            let d = Some(drift(0, 4 * P, 0, 0, 0));
            // `lbu v0,0x21b(s5); beq v0,0xff` at `0x801F72D0`: the attack
            // clip arm `0xA` cut to has not reached its park frame.
            if park_wait(st, phase) {
                return held(d);
            }
            st.countdown.0 = 0;
            with_drift(ArmDirection::PASS, d)
        }
        0xC => {
            let d = Some(drift(0, 4 * P, 0, 0, 4 * P));
            // Every pass re-arms the creature's park (`0x801F73E0`) and
            // ramps its rate `+0x21D = word >> 4 + 2` (`0x801F7418`).
            st.creature_loop_arm = true;
            st.countdown.0 += P;
            let rate = (st.countdown.0 >> 4) + 2;
            if rate < 0x20 {
                st.creature_rate = Some(rate as u8);
                return held(d);
            }
            st.creature_rate = Some(0x20);
            st.countdown.0 = S * 0x70;
            with_drift(ArmDirection::PASS, d)
        }
        GOLA_GOLA_HIT_ARM => {
            let d = Some(drift(-P, 4 * P, 0, 0, 4 * P));
            if counted(st, P) {
                return held(d);
            }
            st.countdown.0 += S << 7;
            let yaw = st.live_yaw.map_or(0, i32::from);
            let shot = match (st.target_code, st.target_aim) {
                (Some(code), Some(a)) if code >= 8 => cut(
                    [0x80, yaw],
                    [0, 0x500, 4 * i32::from(a.extent)],
                    [i32::from(a.centroid_x), 0, i32::from(a.centroid_z)],
                    0x40,
                ),
                _ => cut(
                    [0x40, yaw],
                    [0, 0x600, 2 * i32::from(seats.depth_raw as i16) / 3],
                    on(seats.victim),
                    0x40,
                ),
            };
            with_drift(shot, d)
        }
        0xE => {
            let d = Some(drift(0, P / 2, 0, 0, 4 * P));
            // Every pass re-arms the park (`0x801F78B0`).
            st.creature_loop_arm = true;
            let w = st.creature_shatter.unwrap_or(0);
            st.creature_shatter =
                Some(w.saturating_add((D << 7) as u16).min(GOLA_GOLA_SHATTER_CAP));
            if st.countdown.0 > 0 {
                st.countdown.0 -= P;
            }
            if !st.row_settled || st.countdown.0 > 0 {
                return held(d);
            }
            finish(caption(
                with_drift(ArmDirection::PASS, d),
                ModuleCaption::Cleared,
            ))
        }
        0xFF => {
            st.yaw_base = 0;
            st.creature_loop_release = true;
            ArmDirection::HOLD
        }
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0915 - Mushura
// ---------------------------------------------------------------------------

/// PROT 0915's hit arm: the rampage, whose passes hit every monster the
/// creature reaches (`0x801F77E4`). Arm 7's quarter hit on the latched
/// victim, which cannot kill, is not carried.
pub const MUSHURA_HIT_ARM: u8 = 0xC;

/// PROT 0915 (Mushura): the victim is latched at arm 0.
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0, 1 | - | a cut on the victim, the same pitched to `0x200` over `0x80` frames |
/// | 2, 3 | - (the load) | word `= scalar << 6` |
/// | 4 | word | a cut half a turn of `0x800` past the victim; `+= scalar << 6` |
/// | 5 | word | the creature seated `(v.x, v.z - 0xA00)`, facing `0`; `+= scalar << 6` |
/// | 6 | word | a `0x40`-frame framing on the victim; `+= scalar << 6` |
/// | 7 | word above `scalar << 5` | (the victim's quarter hit) |
/// | 8 | word | a `0x20`-frame framing of the creature, facing `0xB00`; the spell name; `+= scalar * 180` |
/// | 9 | word | the creature turned onto the victim and framed; the attack name; clip `1`; `+= scalar * 120` |
/// | 0xA | word, TR z `+= 4 p` a pass | clip `2`; a cut on the creature; `+= scalar * 120` |
/// | 0xB | word, pitch `-= p / 2` a pass | `+= scalar << 8` |
/// | 0xC | word | the rampage: a `0xC`-frame framing of the creature every pass |
/// | 0xD | the creature's tint word (not fed: pass) | a `0xC`-frame framing every pass; the caption cleared; `0xFF` |
///
/// The creature's walk through arms 6, 7 and the rampage is not carried: it
/// stays where arm 5 seats it, turned as arms 8 and 9 turn it. The arm 0xD
/// framing aims at the world anchor `0x80089118` / `0x80089120`, which the
/// port keeps no twin of; it aims at the origin.
///
/// PORT: FUN_801F69D8 (PROT 0915; the whole phase chain's gates, camera writes, captions, creature seat and clips - the seat scatter, the creature's walk and steering, part spawns and the arm-7 hit not carried)
pub fn mushura_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let creature = st.creature.unwrap_or_default();
    let f = i32::from(creature.facing);
    let counted_rearm = |st: &mut ModuleCamState, add: i32| -> bool {
        if counted(st, P) {
            return true;
        }
        st.countdown.0 += add;
        false
    };
    match phase {
        0 => cut([0, 0x800], [0, 0x400, 0x800], on(v), 1),
        1 => cut([0x200, 0x800], [0, 0x400, 0x800], on(v), 0x80),
        2 => ArmDirection::PASS,
        3 => {
            st.countdown.0 = S << 6;
            ArmDirection::PASS
        }
        4 => {
            if counted_rearm(st, S << 6) {
                return ArmDirection::HOLD;
            }
            cut(
                [-0x40, 0xA00],
                [0, 0x500, 0x1000],
                [-i32::from(v.x), 0, 0x800 - i32::from(v.z)],
                1,
            )
        }
        5 => {
            if counted_rearm(st, S << 6) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat {
                x: v.x,
                y: 0,
                z: (i32::from(v.z) - 0xA00) as i16,
                facing: 0,
            });
            ArmDirection::PASS
        }
        6 => {
            if counted_rearm(st, S << 6) {
                return ArmDirection::HOLD;
            }
            cut([-0x40, 0xE00], [0, 0x500, 0xC00], on(v), 0x40)
        }
        7 => {
            st.countdown.0 -= P;
            gate(st.countdown.0 > S << 5)
        }
        8 => {
            if counted_rearm(st, S * 180) {
                return ArmDirection::HOLD;
            }
            let c = ModuleSeat {
                facing: 0xB00,
                ..creature
            };
            st.creature = Some(c);
            caption(
                cut([0, 0xE00], [0, 0x500, 0xC00], on(c), 0x20),
                ModuleCaption::SpellName,
            )
        }
        9 => {
            if counted_rearm(st, S * 120) {
                return ArmDirection::HOLD;
            }
            let c = ModuleSeat {
                facing: heading(creature, v),
                ..creature
            };
            st.creature = Some(c);
            st.creature_stage = Some(1);
            caption(
                cut(
                    [0x40, 0x800 - i32::from(c.facing)],
                    [0, 0x500, 0x800],
                    on(c),
                    1,
                ),
                ModuleCaption::AttackName,
            )
        }
        0xA => {
            let d = Some(drift(0, 0, 0, 0, 4 * P));
            st.creature_stage = Some(2);
            if counted_rearm(st, S * 120) {
                return held(d);
            }
            with_drift(cut([0x2E0, -f], [0, 0x400, 0x1000], on(creature), 1), d)
        }
        0xB => {
            let d = Some(drift(-(P / 2), 0, 0, 0, 0));
            if counted_rearm(st, S << 8) {
                return held(d);
            }
            with_drift(ArmDirection::PASS, d)
        }
        MUSHURA_HIT_ARM => {
            let shot = cut([0x100, -f], [0, 0x400, 0x1000], on(creature), 0xC);
            st.countdown.0 -= P;
            ArmDirection {
                hold: st.countdown.0 > 0,
                ..shot
            }
        }
        0xD => finish(caption(
            cut([0, -f], [0, 0x400, 0], [0; 3], 0xC),
            ModuleCaption::Cleared,
        )),
        _ => ArmDirection::HOLD,
    }
}

// ---------------------------------------------------------------------------
// PROT 0917 - Barra
// ---------------------------------------------------------------------------

/// PROT 0917's hit arm: its pass, once the dropped victim lands, rolls
/// `FUN_801DD0AC(0x12, 7, target)` (`0x801F7C48`).
pub const BARRA_HIT_ARM: u8 = 0xD;

/// PROT 0917's arm whose pass puts the creature into the capture / defeat
/// fade (`+0x21C = 2`, `+0x225 = 2` at `0x801F7FE4..0x801F7FE8`): it fades
/// out additive before arm 0xF frames the struck row on the ground.
pub const BARRA_CREATURE_FADE_ARM: u8 = 0xE;

/// Arms 4 and 5 pull the TR z global in by `p << 7` a pass from the `0xFFFF`
/// arm 3 / 4 store, to below `0x801` and `0xC01`.
const BARRA_PULL: i32 = P << 7;

/// PROT 0917 (Barra): the module stores the camera globals straight in arms
/// 3..6 and 0xC; the port lands each store as a one-frame cut on the origin,
/// where arm 4 has moved the victim. Its two module ramps (the TR z pull of
/// arms 4 / 5 and the victim's fall in arm 0xD) are modelled on
/// [`ModuleCamState::ramp`].
///
/// | arm | gate | camera / writes |
/// |---|---|---|
/// | 0 | - | a cut a quarter unit back from the victim |
/// | 1 | TR y below `0x800` | TR y `+= p`, pitch `-= p / 2` a pass |
/// | 2 | - (the CD poll) | word `= scalar << 6` |
/// | 3 | word | TR `(·, 0x400, 0xFFFF)`, yaw `0x800`, pitch `0`; the creature seated on the origin |
/// | 4 | TR z `-= p << 7` while at or above `0x801` | TR `(·, 0x400, 0xFFFF)`, yaw `0`, pitch `0x60` |
/// | 5 | the same to `0xC01` | TR `(·, 0x800, 0x2000)`, yaw `0x700`, pitch `-0x40`; a white fade in after `0xC0`; clip `1`; word `= 0` |
/// | 6 | word `+= p / 8` until `0x100` | pitch `-= p / 8` / TR z `-= 12 p` to `0xA0`, then TR z `+= 32 p` / pitch `+= p / 2`; yaw `= word + 0x700` (`+ 0xF00` from `0x51`); on its pass a cut, the fade out, word `= scalar << 8` |
/// | 7 | word, TR y `-= p` a pass | the spell name; `+= scalar * 180` |
/// | 8 | word | a cut; the attack name; word `= 0` |
/// | 9 | word `+= p / 8` until `0x81` | word `= scalar << 5` |
/// | 0xA | word | `+= scalar << 6` |
/// | 0xB | word | a cut on `(0, 0, 0)`; `+= scalar << 8` |
/// | 0xC | word, TR z `+= 3 p` a pass | TR `(0x1000, 0x1000, 0x42D3)`, yaw `0x8C4`, pitch `0xFE0`; clip `2`; word `= 0` |
/// | 0xD | the victim's fall (`+0x36 += 16 p` from `-0x2000`); word `+= p / 8` a pass | the hit |
/// | 0xE | word `+= p / 8` until `0x56`, on from arm 0xD's | word `= scalar << 7` |
/// | 0xF | word, then the victim settled | every pass a cut on the victim at a third of the depth, TR z easing from half the depth; the caption cleared; `0xFF` |
///
/// PORT: FUN_801F6A30, overlay_summon_barra_0917_801f6a30 (PROT 0917; the whole phase chain's gates, camera writes and stores, captions, creature seat and clips - the seat swaps, the victim and creature arcs, part spawns and the epilogue's ctx bytes not carried)
pub fn barra_direct(st: &mut ModuleCamState, phase: u8, seats: ModuleCamSeats) -> ArmDirection {
    let v = seats.victim;
    let store = |pitch: i32, yaw: i32, tr: [i32; 3]| {
        cut([pitch, yaw], [tr[0], tr[1], raw_tr_z(tr[2])], [0; 3], 1)
    };
    let depth = i32::from(seats.depth_raw as i16);
    match phase {
        0 => {
            let h = heading(v, seats.caster).wrapping_add(0x800) & 0xFFF;
            let (sin, cos) = trig12(h);
            st.tr_y = 0x400;
            cut(
                [0, i32::from(yaw_from(0x800, h))],
                [0, 0x400, 0x400],
                [
                    -i32::from(v.x) + i32::from(sin) / 4,
                    0,
                    -i32::from(v.z) + i32::from(cos) / 4,
                ],
                1,
            )
        }
        1 => {
            if st.tr_y < 0x800 {
                st.tr_y += P;
                return ArmDirection::HOLD.nudged(ModuleNudge {
                    pitch: -(P >> 1) as i16,
                    tr_x: 0,
                    tr_y: P as i16,
                    tr_z: 0,
                });
            }
            ArmDirection::PASS
        }
        2 => {
            st.countdown.0 = S << 6;
            ArmDirection::PASS
        }
        3 => {
            if counted(st, P) {
                return ArmDirection::HOLD;
            }
            st.creature = Some(ModuleSeat::default());
            st.ramp = 0xFFFF;
            store(0, 0x800, [0, 0x400, 0xFFFF])
        }
        4 | 5 => {
            st.ramp -= BARRA_PULL;
            let floor = if phase == 4 { 0x801 } else { 0xC01 };
            if st.ramp >= floor {
                return held(Some(drift(0, 0, 0, 0, -BARRA_PULL)));
            }
            if phase == 4 {
                st.ramp = 0xFFFF;
                return store(0x60, 0, [0, 0x400, 0xFFFF]);
            }
            st.countdown.0 = 0;
            st.creature_stage = Some(1);
            store(-0x40, 0x700, [0, 0x800, 0x2000])
        }
        6 => {
            st.countdown.0 += P / 8;
            let cd = st.countdown.0;
            if cd < 0x100 {
                let (pitch, tr_z) = if cd <= 0xA0 {
                    (-(P / 8), -12 * P)
                } else {
                    (P / 2, 32 * P)
                };
                let base = if cd >= 0x51 { 0xF00 } else { 0x700 };
                let yaw = yaw_to(st, cd + base);
                return held(Some(drift(pitch, yaw, 0, 0, tr_z)));
            }
            st.countdown.0 = S << 8;
            cut([0xC0, 0x880], [0, 0xC00, 0x700], [0; 3], 1)
        }
        7 => {
            let d = Some(drift(0, 0, 0, -P, 0));
            if counted(st, P) {
                return held(d);
            }
            st.countdown.0 += S * 180;
            caption(with_drift(ArmDirection::PASS, d), ModuleCaption::SpellName)
        }
        8 => {
            if counted(st, P) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = 0;
            caption(
                cut([0, 0], [0, 0x300, 0], [0; 3], 1),
                ModuleCaption::AttackName,
            )
        }
        9 | 0xE => {
            st.countdown.0 += P / 8;
            let (end, rearm) = if phase == 9 {
                (0x81, S << 5)
            } else {
                (0x56, S << 7)
            };
            if st.countdown.0 < end {
                return ArmDirection::HOLD;
            }
            st.countdown.0 = rearm;
            ArmDirection::PASS
        }
        0xA => {
            if counted(st, P) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 += S << 6;
            ArmDirection::PASS
        }
        0xB => {
            if counted(st, P) {
                return ArmDirection::HOLD;
            }
            st.countdown.0 += S << 8;
            cut([-4, 0x900], [0, 0x2000, 0x800], [0; 3], 1)
        }
        0xC => {
            let d = Some(drift(0, 0, 0, 0, 3 * P));
            if counted(st, P) {
                return held(d);
            }
            st.countdown.0 = 0;
            st.creature_stage = Some(2);
            // The victim and the creature hang at `+0x36 = -0x2000`.
            st.ramp = -0x2000;
            store(0xFE0, 0x8C4, [0x1000, 0x1000, 0x42D3])
        }
        BARRA_HIT_ARM => {
            st.ramp += 16 * P;
            // The arm also steps the word by `p / 8` every pass (the
            // creature's arc, `0x801F7CB0..0x801F7CE0`), and arm 0xE climbs
            // on from where it leaves it rather than from zero.
            st.countdown.0 += P / 8;
            gate(st.ramp < 0)
        }
        0xF => {
            let full = S << 7;
            let ease = ((full - st.countdown.0.max(0)) * (depth / 2)) / full;
            let shot = cut([0, 0x80], [0, depth / 3, depth / 2 + ease], on(v), 1);
            if st.countdown.0 > 0 {
                st.countdown.0 -= P;
                if st.countdown.0 > 0 {
                    return ArmDirection { hold: true, ..shot };
                }
            }
            if !st.victim_settled {
                return ArmDirection { hold: true, ..shot };
            }
            finish(caption(shot, ModuleCaption::Cleared))
        }
        _ => ArmDirection::HOLD,
    }
}
