//! The field overlay's **passive-ability indicator HUD** - the small column of
//! icons retail floats above the player's head while an accessory passive is
//! active.
//!
//! REF: FUN_800431D0, FUN_8002C488, FUN_8005BA68
//!
//! The port of `FUN_801d095c` is split across [`hud_anchor_offsets`] and
//! [`passive_hud_icons`], each carrying its own `PORT` tag; the tag is
//! deliberately not repeated at module level.
//!
//! # Provenance, and a correction
//!
//! `FUN_801d095c` lives in the field overlay (PROT entry `0897_xxx_dat`,
//! slot-A base `0x801CE818`, file offset `0x2144`). It is 141 instructions
//! ending `jr ra / addiu sp, sp, 0x60`, and the extracted image agrees
//! instruction-for-instruction with
//! `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d095c.txt`.
//!
//! `docs/subsystems/script-vm.md` used to file this address under "party-roster
//! panel renderers" and describe it as *"the money/counter variant: it clamps
//! each of three values to 9,999,999 and draws them from the save-scan base
//! `&DAT_80084140` (stride `0x414`)"*. **That is not this function.** The body
//! contains no clamp, no `0x80084140`, no stride-`0x414` walk and no number
//! drawer: every one of its thirteen calls is either the global ability
//! bit-test `FUN_800431D0`, the icon sprite `FUN_8002C488`, or the one GTE
//! three-point transform `FUN_8005BA68`. The instruction count in that entry
//! (141) is the only part of it that survives contact with the bytes.
//!
//! # What it does
//!
//! Three world points are built on the stack from the player context
//! `_DAT_8007C364`, all sharing the player's X (`+0x14`) and Z (`+0x18`) and
//! differing only in how far above the player's head they sit. The lift is
//! scaled by the player's `+0x72` half-word - the actor's height - through
//! three different fixed-point fractions, which is what keeps the badge
//! column glued to the head rather than to the feet.
//!
//! The trio goes through `FUN_8005BA68` (the GTE `RTPT` wrapper: three
//! vertices in, three `SXY` pairs out). Retail then takes the **X of the first
//! projected point and the Y of the third** - not a single point's pair - and
//! anchors every icon on that mixed pair.
//!
//! Two independent groups hang off that anchor:
//!
//! * an **encounter-rate badge** driven by ability bits `0x3B` / `0x3C` (High
//!   and Low Encounter, the same two bits the encounter-rate scaler reads) and
//!   `0x3D`, drawn left of the anchor;
//! * a **vertically centred stack** of up to three icons for bits `0x38` /
//!   `0x39` / `0x3A`, drawn right of the anchor.
//!
//! Neither group reads a table: every bit id and icon id is an immediate in
//! the instruction stream.

/// Ability bit ids the HUD tests, in the order retail tests them.
pub mod ability_bit {
    /// Stack slot 1.
    pub const STACK_A: u8 = 0x38;
    /// Stack slot 2.
    pub const STACK_B: u8 = 0x39;
    /// Stack slot 3.
    pub const STACK_C: u8 = 0x3A;
    /// High Encounter (the `rate << 2` passive).
    pub const ENCOUNTER_HIGH: u8 = 0x3B;
    /// Low Encounter (the `rate >> 1` passive).
    pub const ENCOUNTER_LOW: u8 = 0x3C;
    /// The third badge bit, drawn furthest left.
    pub const BADGE_LEFT: u8 = 0x3D;
}

/// Icon ids passed to `FUN_8002C488`, all immediates in the body.
pub mod icon {
    /// Stack slot 1 (`0x38`).
    pub const STACK_A: u8 = 0x47;
    /// Stack slot 2 (`0x39`).
    pub const STACK_B: u8 = 0x48;
    /// Stack slot 3 (`0x3A`).
    pub const STACK_C: u8 = 0x49;
    /// Badge plate, drawn whenever the badge group has anything to show.
    pub const BADGE_PLATE: u8 = 0x4A;
    /// High-encounter chevron, above the plate.
    pub const BADGE_HIGH: u8 = 0x4B;
    /// Low-encounter chevron, below the plate.
    pub const BADGE_LOW: u8 = 0x4C;
    /// The `0x3D` badge, left of the plate.
    pub const BADGE_LEFT: u8 = 0x4D;
}

/// One `FUN_8002C488(x, y, id)` call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudIcon {
    /// Screen X.
    pub x: i32,
    /// Screen Y.
    pub y: i32,
    /// Icon id - one of [`icon`].
    pub id: u8,
}

/// The three head-relative lifts, in world units, for a player whose `+0x72`
/// height half-word is `height`.
///
/// PORT: FUN_801d095c (`0x801D098C..0x801D09F4`)
///
/// Each is subtracted from the player's `+0x16` (world Y grows downward, so
/// subtracting raises the point), and each uses a different fixed-point
/// fraction of the height:
///
/// | Point | Retail | Fraction |
/// |---|---|---|
/// | `0` | `(h * 13) >> 9`, `srl` | `13/512` |
/// | `1` | `h >> 6`, `srl` | `8/512` |
/// | `2` | `(h * 43) >> 10`, `sra` | `21.5/512` |
///
/// The first two shifts are **logical** (`srl` on the zero-extended `lhu`) and
/// the third is **arithmetic** (`sra`), which only diverges for a height past
/// `0x8000 / 43`; retail heights are far below that, so the port keeps the
/// widths honest rather than pretending the three agree.
// NOT WIRED: same blocker as [`passive_hud_icons`] - the lifts feed a GTE
// three-point transform that only a field HUD host would issue, and no such
// host exists.
pub fn hud_anchor_offsets(height: u16) -> [i32; 3] {
    let h = i64::from(height);
    [
        ((h * 13) as u64 >> 9) as i32,
        (height >> 6) as i32,
        ((h * 43) >> 10) as i32,
    ]
}

/// Build the icon list for one frame.
///
/// `anchor` is the mixed screen pair retail assembles after the transform:
/// `(sxy[0].x, sxy[2].y)`. `has_bit` is the global ability bit-test
/// `FUN_800431D0`.
///
/// PORT: FUN_801d095c
// NOT WIRED: the field HUD's icon-atlas draw list is built by `engine-ui`, and
// nothing there yet emits above-head widgets - `engine-core`'s field overlay
// draws the party HUD and the dialog panel only. The bit source exists
// (`engine-core::accessory_passives`), the projection host does not.
pub fn passive_hud_icons<F: Fn(u8) -> bool>(anchor: (i32, i32), has_bit: F) -> Vec<HudIcon> {
    let (ax, ay) = anchor;
    let mut out = Vec::new();

    // Retail: `s0 = (bit(0x3B) != 0); if bit(0x3C) { s0 += 1 } s0 &= 1`, i.e.
    // the **parity** of the two encounter bits, not "either". Both set is the
    // same as neither - the two accessories cancel, and the chevrons vanish.
    let high = has_bit(ability_bit::ENCOUNTER_HIGH);
    let low = has_bit(ability_bit::ENCOUNTER_LOW);
    let parity = high ^ low;

    if parity || has_bit(ability_bit::BADGE_LEFT) {
        out.push(HudIcon {
            x: ax - 0x14,
            y: ay,
            id: icon::BADGE_PLATE,
        });
    }
    if parity {
        if high {
            out.push(HudIcon {
                x: ax - 0x10,
                y: ay - 8,
                id: icon::BADGE_HIGH,
            });
        }
        if low {
            out.push(HudIcon {
                x: ax - 0x10,
                y: ay + 0xa,
                id: icon::BADGE_LOW,
            });
        }
    }
    if has_bit(ability_bit::BADGE_LEFT) {
        out.push(HudIcon {
            x: ax - 0x1e,
            y: ay,
            id: icon::BADGE_LEFT,
        });
    }

    // The stack's start offset is `5 - 5 * n` for `n` set bits, built by
    // retail as `s0 = 5`, then `if bit(0x38) { s0 = 0 }` (a `move`, not a
    // subtract), `if bit(0x39) { s0 -= 5 }`, `if bit(0x3A) { s0 -= 5 }`. That
    // gives 0 / -5 / -10 for one / two / three icons at a 10px pitch: the
    // column is centred on the anchor.
    let stack = [
        (ability_bit::STACK_A, icon::STACK_A),
        (ability_bit::STACK_B, icon::STACK_B),
        (ability_bit::STACK_C, icon::STACK_C),
    ];
    let mut cursor: i32 = 5;
    if has_bit(ability_bit::STACK_A) {
        cursor = 0;
    }
    if has_bit(ability_bit::STACK_B) {
        cursor -= 5;
    }
    if has_bit(ability_bit::STACK_C) {
        cursor -= 5;
    }
    for (bit, id) in stack {
        if has_bit(bit) {
            out.push(HudIcon {
                x: ax + 2,
                y: ay + cursor,
                id,
            });
            cursor += 10;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hud_for(bits: &[u8]) -> Vec<HudIcon> {
        passive_hud_icons((100, 50), |b| bits.contains(&b))
    }

    #[test]
    fn nothing_active_draws_nothing() {
        assert!(hud_for(&[]).is_empty());
    }

    #[test]
    fn both_encounter_bits_cancel() {
        // Parity, not "either": High + Low together draw no plate and no
        // chevrons at all.
        assert!(hud_for(&[ability_bit::ENCOUNTER_HIGH, ability_bit::ENCOUNTER_LOW]).is_empty());
    }

    #[test]
    fn one_encounter_bit_draws_plate_plus_its_chevron() {
        let v = hud_for(&[ability_bit::ENCOUNTER_HIGH]);
        assert_eq!(
            v,
            vec![
                HudIcon {
                    x: 100 - 0x14,
                    y: 50,
                    id: icon::BADGE_PLATE
                },
                HudIcon {
                    x: 100 - 0x10,
                    y: 50 - 8,
                    id: icon::BADGE_HIGH
                },
            ]
        );
        let v = hud_for(&[ability_bit::ENCOUNTER_LOW]);
        assert_eq!(v[1].id, icon::BADGE_LOW);
        assert_eq!(v[1].y, 50 + 0xa);
    }

    #[test]
    fn badge_left_alone_still_draws_the_plate() {
        let v = hud_for(&[ability_bit::BADGE_LEFT]);
        assert_eq!(v[0].id, icon::BADGE_PLATE);
        assert_eq!(v[1].id, icon::BADGE_LEFT);
        assert_eq!(v[1].x, 100 - 0x1e);
    }

    #[test]
    fn stack_is_centred_on_the_anchor() {
        for (bits, want) in [
            (vec![ability_bit::STACK_A], vec![50]),
            (vec![ability_bit::STACK_B], vec![50]),
            (
                vec![ability_bit::STACK_A, ability_bit::STACK_B],
                vec![45, 55],
            ),
            (
                vec![
                    ability_bit::STACK_A,
                    ability_bit::STACK_B,
                    ability_bit::STACK_C,
                ],
                vec![40, 50, 60],
            ),
        ] {
            let ys: Vec<i32> = hud_for(&bits).iter().map(|i| i.y).collect();
            assert_eq!(ys, want, "bits {bits:?}");
        }
    }

    #[test]
    fn stack_x_is_two_right_of_the_anchor() {
        for i in hud_for(&[ability_bit::STACK_C]) {
            assert_eq!(i.x, 102);
        }
    }

    #[test]
    fn anchor_offsets_are_ordered_by_lift() {
        // Point 2 sits highest, point 1 lowest - the ordering the badge column
        // depends on, since retail takes X from point 0 and Y from point 2.
        let o = hud_anchor_offsets(0x200);
        assert!(o[2] > o[0] && o[0] > o[1]);
        assert_eq!(o, [13, 8, 21]);
    }

    #[test]
    fn zero_height_collapses_the_trio() {
        assert_eq!(hud_anchor_offsets(0), [0, 0, 0]);
    }
}
