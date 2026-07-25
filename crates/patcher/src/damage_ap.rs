//! **Enemy-damage AP tuning**: set how much AP a party member's battle gauge
//! gains when an enemy damages them (retail: a full-HP hit fills the whole
//! 100-point gauge), including a **negative** range where taking damage
//! *drains* AP instead.
//!
//! ## Where the retail value lives
//!
//! The battle damage finisher `FUN_801DDB30` (battle-action overlay, PROT
//! entry 898, base VA `0x801CE818`) ends by filling the defender's AP gauge
//! (`actor[+0x170]`) in proportion to the fraction of max HP the hit took.
//! The scale factor is synthesized as a shift/add chain rather than an
//! immediate - `d*2 + d = d*3`, `*8 = d*24`, `+d = d*25`, `<<2 = d*100`:
//!
//! ```text
//! 801de1c4  subu v1,v1,v0     ; v1 = damage
//! 801de1c8  sll  v0,v1,0x1    ; \
//! 801de1cc  addu v0,v0,v1     ; |
//! 801de1d0  sll  v0,v0,0x3    ; |  x100   <- the scale (sites 1..4 + 6)
//! 801de1d4  addu v0,v0,v1     ; |
//! 801de1d8  lhu  v1,0x14e(s1) ; |  max HP
//! 801de1dc  sll  v0,v0,0x2    ; /
//! 801de1e0  divu v0,v1        ; pct = damage * 100 / max_hp
//! 801de1f8  sltiu a1,v0,0x1   ; \  a1 = max(pct, 1)      <- the min-1 floor
//! 801de1fc  addu  a1,a1,v0    ; /
//! ```
//!
//! and then adds that percentage into the gauge, clamped at 100, after two
//! equipment-gated bonus arms (the "spirit gain up" accessory bits `0x200`
//! and `0x100` of the character record's ability word, worth `+pct/4` and
//! `+pct/10`):
//!
//! ```text
//! 801de2b8  lhu  v0,0x170(s1) ; gauge
//! 801de2c0  addu v0,v0,a1     ; += pct                  <- the accrual
//! 801de2c4  sh   v0,0x170(s1)
//! 801de2cc  sltiu v0,v0,0x65  ; clamp at 100
//! ```
//!
//! The kernel is mirrored port-side by
//! `legaia_engine_vm::battle_formulas::spirit_gauge_fill`.
//!
//! ## What the patch rewrites
//!
//! **Positive** values keep retail's shape and only restate the scale as an
//! explicit multiply, so any factor is expressible (not just the constants a
//! shift/add chain can synthesize):
//!
//! ```text
//! 801de1c8  ori   v0,zero,N
//! 801de1cc  multu v1,v0
//! 801de1d0  mflo  v0
//! 801de1d4  nop
//! 801de1d8  lhu   v1,0x14e(s1)   (unchanged)
//! 801de1dc  nop
//! ```
//!
//! `N = 100` restores the retail chain byte-for-byte, so the retail value is
//! a true no-op. `N = 0` additionally rewrites the min-1 floor to `move
//! a1,zero`, because retail's `max(pct, 1)` would otherwise still grant 1 AP
//! per hit.
//!
//! **Negative** values reuse the same scale for the magnitude and turn the
//! accrual into a subtract with a floor at zero, in place:
//!
//! ```text
//! 801de2c0  subu v0,v0,a1        ; gauge -= pct (may go negative)
//! 801de2c4  bgez v0,0x801de2d0
//! 801de2c8  nop
//! 801de2cc  move v0,zero         ; floor at 0
//! 801de2d0  sh   v0,0x170(s1)
//! ```
//!
//! The retail clamp-at-100 that occupied those words is not needed on a
//! draining site - the gauge cannot grow here - and the gauge's other growth
//! sites (the per-action accrual in `FUN_801E295C`, see [`crate::spirit_ap`])
//! keep their own clamps. The two "spirit gain up" bonus arms are neutralized
//! for a negative setting by making their guard branches unconditional, so an
//! AP-Boost accessory does not *deepen* the drain; the accessory is inert
//! while the setting is negative.
//!
//! Every edit is a same-size word rewrite of the raw PROT 898 entry. Stock
//! words are cited from the project's own disassembly reference; no Sony
//! bytes are embedded. An unrecognized build is refused, never corrupted;
//! re-planning with a different value re-targets a previously patched image
//! (in either direction).

use anyhow::{Result, bail};

/// PROT entry index of the battle-action overlay hosting `FUN_801DDB30`.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay (raw entry: file offset =
/// `va - OVERLAY_BASE_VA`).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// The retail scale: a hit costing 100% of max HP grants 100 AP (a full gauge).
pub const RETAIL_DAMAGE_AP: i16 = 100;

/// Largest magnitude the slider accepts, in either direction.
pub const MAX_DAMAGE_AP: i16 = 200;

// ---------------------------------------------------------------------------
// Patch sites
// ---------------------------------------------------------------------------

/// The `x100` shift/add chain (`0x801DE1C8..=0x801DE1D4` and `0x801DE1DC`;
/// `0x801DE1D8` in the middle is the max-HP load and is left alone).
const SCALE_VAS: [u32; 5] = [
    0x801D_E1C8,
    0x801D_E1CC,
    0x801D_E1D0,
    0x801D_E1D4,
    0x801D_E1DC,
];

/// Retail's shift/add chain, in `SCALE_VAS` order.
const SCALE_RETAIL: [u32; 5] = [
    0x0003_1040, // sll  v0,v1,0x1
    0x0043_1021, // addu v0,v0,v1
    0x0002_10C0, // sll  v0,v0,0x3
    0x0043_1021, // addu v0,v0,v1
    0x0002_1080, // sll  v0,v0,0x2
];

/// `ori v0,zero,imm` - the configurable scale factor (site 1 of the multiply
/// form). The remaining four words are fixed.
const SCALE_ORI_PREFIX: u32 = 0x3402_0000;
const SCALE_MULTU: u32 = 0x0062_0019; // multu v1,v0
const SCALE_MFLO: u32 = 0x0000_1012; // mflo  v0
const NOP: u32 = 0x0000_0000;

/// `sltiu a1,v0,0x1` + `addu a1,a1,v0` = `a1 = max(pct, 1)`.
const FLOOR_VA: u32 = 0x801D_E1F8;
const FLOOR_RETAIL: u32 = 0x2C45_0001; // sltiu a1,v0,0x1
const FLOOR_NONE: u32 = 0x0000_2821; // move  a1,zero  (a1 = pct, no floor)

/// Guard branches of the two "spirit gain up" bonus arms. Retail tests the
/// ability bit; a negative setting makes them unconditional so the bonus arms
/// never run.
const BOOST2_BR_VA: u32 = 0x801D_E248;
const BOOST2_BR_RETAIL: u32 = 0x1040_0005; // beq  v0,zero,0x801de260
const BOOST2_BR_SKIP: u32 = 0x1000_0005; // beq  zero,zero,0x801de260
const BOOST1_BR_VA: u32 = 0x801D_E290;
const BOOST1_BR_RETAIL: u32 = 0x1040_0009; // beq  v0,zero,0x801de2b8
const BOOST1_BR_SKIP: u32 = 0x1000_0009; // beq  zero,zero,0x801de2b8

/// The accrual + clamp tail (`0x801DE2C0..=0x801DE2D8`). `0x801DE2B8` (the
/// gauge load) and `0x801DE2BC` are branch targets / context and stay put.
const TAIL_VAS: [u32; 7] = [
    0x801D_E2C0,
    0x801D_E2C4,
    0x801D_E2C8,
    0x801D_E2CC,
    0x801D_E2D0,
    0x801D_E2D4,
    0x801D_E2D8,
];

/// Retail: `gauge += pct`, then clamp at 100.
const TAIL_RETAIL: [u32; 7] = [
    0x0045_1021, // addu  v0,v0,a1
    0xA622_0170, // sh    v0,0x170(s1)
    0x3042_FFFF, // andi  v0,v0,0xffff
    0x2C42_0065, // sltiu v0,v0,0x65
    0x1440_0002, // bne   v0,zero,0x801de2dc
    0x2402_0064, // li    v0,0x64
    0xA622_0170, // sh    v0,0x170(s1)
];

/// Draining: `gauge -= pct`, floored at 0.
const TAIL_DRAIN: [u32; 7] = [
    0x0045_1023, // subu  v0,v0,a1
    0x0441_0002, // bgez  v0,0x801de2d0
    0x0000_0000, // nop
    0x0000_1021, // move  v0,zero
    0xA622_0170, // sh    v0,0x170(s1)
    0x0000_0000, // nop
    0x0000_0000, // nop
];

/// Context fingerprint: words around the sites that are never rewritten, so a
/// build that does not match retail's damage finisher is refused outright.
/// `(va, stock_word)`.
const CONTEXT: [(u32, u32); 10] = [
    (0x801D_E1C4, 0x0062_1823), // subu  v1,v1,v0     (damage)
    (0x801D_E1D8, 0x9623_014E), // lhu   v1,0x14e(s1) (max HP)
    (0x801D_E1E0, 0x0043_001B), // divu  v0,v1
    (0x801D_E1F0, 0x0000_1012), // mflo  v0
    (0x801D_E1FC, 0x00A2_2821), // addu  a1,a1,v0
    (0x801D_E200, 0x2C62_0003), // sltiu v0,v1,0x3    (party-only gate)
    (0x801D_E244, 0x3042_0200), // andi  v0,v0,0x200  (spirit gain up 2)
    (0x801D_E28C, 0x3042_0100), // andi  v0,v0,0x100  (spirit gain up 1)
    (0x801D_E2B8, 0x9622_0170), // lhu   v0,0x170(s1) (gauge load)
    (0x801D_E2BC, 0x0000_0000), // nop
];

fn word_at(overlay: &[u8], va: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + 4).ok_or_else(|| {
        anyhow::anyhow!("overlay entry too short for word at {va:#x} (+{off:#x})")
    })?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

/// The complete set of `(va, word)` the image should hold for `value`.
fn desired(value: i16) -> Vec<(u32, u32)> {
    let magnitude = value.unsigned_abs();
    let draining = value < 0 && magnitude != 0;
    let mut out = Vec::with_capacity(15);

    // Scale. The retail factor keeps retail's own shift/add chain so that
    // `--damage-ap 100` is a genuine no-op on a stock image.
    if !draining && magnitude == RETAIL_DAMAGE_AP as u16 {
        for (va, word) in SCALE_VAS.iter().zip(SCALE_RETAIL) {
            out.push((*va, word));
        }
    } else {
        let multiply = [
            SCALE_ORI_PREFIX | magnitude as u32,
            SCALE_MULTU,
            SCALE_MFLO,
            NOP,
            NOP,
        ];
        for (va, word) in SCALE_VAS.iter().zip(multiply) {
            out.push((*va, word));
        }
    }

    // Min-1 floor: retail grants at least 1 AP per hit. Only a scale of zero
    // needs it removed - otherwise "0 AP from damage" would still be 1.
    out.push((
        FLOOR_VA,
        if magnitude == 0 {
            FLOOR_NONE
        } else {
            FLOOR_RETAIL
        },
    ));

    // Bonus arms + accrual tail.
    out.push((
        BOOST2_BR_VA,
        if draining {
            BOOST2_BR_SKIP
        } else {
            BOOST2_BR_RETAIL
        },
    ));
    out.push((
        BOOST1_BR_VA,
        if draining {
            BOOST1_BR_SKIP
        } else {
            BOOST1_BR_RETAIL
        },
    ));
    let tail = if draining { TAIL_DRAIN } else { TAIL_RETAIL };
    for (va, word) in TAIL_VAS.iter().zip(tail) {
        out.push((*va, word));
    }
    out
}

/// Verify the context fingerprint and that every site currently holds either
/// its retail word or a word this patch writes. Returns the configured value.
fn recognize(overlay: &[u8]) -> Result<i16> {
    for (va, want) in CONTEXT {
        let got = word_at(overlay, va)?;
        if got != want {
            bail!(
                "context word {va:#x} = {got:#010x}, expected {want:#010x} \
                 (battle damage-finisher AP fill) - unrecognized build, refusing to patch"
            );
        }
    }

    // Sign comes from the accrual tail; magnitude from the scale.
    let tail: Vec<u32> = TAIL_VAS
        .iter()
        .map(|va| word_at(overlay, *va))
        .collect::<Result<_>>()?;
    let draining = if tail == TAIL_RETAIL {
        false
    } else if tail == TAIL_DRAIN {
        true
    } else {
        bail!(
            "damage-AP accrual tail at {:#x} is neither the stock accrual nor a \
             previously configured drain - unrecognized build, refusing to patch",
            TAIL_VAS[0]
        );
    };

    let scale: Vec<u32> = SCALE_VAS
        .iter()
        .map(|va| word_at(overlay, *va))
        .collect::<Result<_>>()?;
    let magnitude = if scale == SCALE_RETAIL {
        RETAIL_DAMAGE_AP as u16
    } else if scale[1] == SCALE_MULTU
        && scale[2] == SCALE_MFLO
        && scale[3] == NOP
        && scale[4] == NOP
        && scale[0] & 0xFFFF_0000 == SCALE_ORI_PREFIX
        && scale[0] & 0xFFFF <= MAX_DAMAGE_AP as u32
    {
        (scale[0] & 0xFFFF) as u16
    } else {
        bail!(
            "damage-AP scale at {:#x} is neither the stock x100 chain nor a \
             previously configured multiply - unrecognized build, refusing to patch",
            SCALE_VAS[0]
        );
    };

    let value = magnitude as i16;
    Ok(if draining { -value } else { value })
}

/// The enemy-damage AP scale currently on the image, after verifying the
/// build is recognized. Retail reads 100.
pub fn current(overlay: &[u8]) -> Result<i16> {
    recognize(overlay)
}

/// A planned enemy-damage AP edit: same-size word rewrites in the
/// battle-action overlay PROT entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DamageAp {
    /// Per site: `(file offset within the overlay entry, replacement word)`.
    pub writes: Vec<(usize, u32)>,
    /// The scale the image held before this plan.
    pub previous: i16,
}

/// Plan setting the enemy-damage AP scale to `value`
/// (`-MAX_DAMAGE_AP..=MAX_DAMAGE_AP`, retail 100) against the raw
/// battle-action overlay entry. Returns `Ok(None)` when the image already
/// holds `value` at every site (idempotent no-op). Refuses an unrecognized
/// build rather than corrupting it.
pub fn plan(overlay: &[u8], value: i16) -> Result<Option<DamageAp>> {
    if !(-MAX_DAMAGE_AP..=MAX_DAMAGE_AP).contains(&value) {
        bail!(
            "damage AP must be {}..={MAX_DAMAGE_AP}, got {value}",
            -MAX_DAMAGE_AP
        );
    }
    let previous = recognize(overlay)?;
    let mut writes = Vec::new();
    for (va, word) in desired(value) {
        if word_at(overlay, va)? == word {
            continue;
        }
        writes.push(((va - OVERLAY_BASE_VA) as usize, word));
    }
    if writes.is_empty() {
        return Ok(None);
    }
    Ok(Some(DamageAp { writes, previous }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic overlay holding the stock context + site words.
    fn synth_overlay() -> Vec<u8> {
        let mut ov = vec![0u8; 0x18000];
        for (va, w) in CONTEXT {
            put(&mut ov, va, w);
        }
        for (va, w) in desired(RETAIL_DAMAGE_AP) {
            put(&mut ov, va, w);
        }
        ov
    }

    fn put(ov: &mut [u8], va: u32, w: u32) {
        let off = (va - OVERLAY_BASE_VA) as usize;
        ov[off..off + 4].copy_from_slice(&w.to_le_bytes());
    }

    fn apply(ov: &mut [u8], value: i16) {
        if let Some(p) = plan(ov, value).unwrap() {
            for (off, w) in &p.writes {
                ov[*off..*off + 4].copy_from_slice(&w.to_le_bytes());
            }
        }
    }

    #[test]
    fn retail_form_is_the_stock_shift_add_chain() {
        // The x100 chain the disc actually holds is what `desired(100)` emits,
        // so a stock image reads 100 and plans nothing.
        let ov = synth_overlay();
        assert_eq!(current(&ov).unwrap(), RETAIL_DAMAGE_AP);
        assert_eq!(plan(&ov, RETAIL_DAMAGE_AP).unwrap(), None);
    }

    #[test]
    fn positive_value_rewrites_the_scale_only() {
        let ov = synth_overlay();
        let p = plan(&ov, 50).unwrap().unwrap();
        assert_eq!(p.previous, RETAIL_DAMAGE_AP);
        let by_off: std::collections::BTreeMap<usize, u32> = p.writes.iter().copied().collect();
        // Scale becomes the multiply form; nothing else moves.
        assert_eq!(by_off.len(), 5);
        assert_eq!(
            by_off[&((SCALE_VAS[0] - OVERLAY_BASE_VA) as usize)],
            SCALE_ORI_PREFIX | 50
        );
        assert_eq!(
            by_off[&((SCALE_VAS[1] - OVERLAY_BASE_VA) as usize)],
            SCALE_MULTU
        );
        assert!(!by_off.contains_key(&((TAIL_VAS[0] - OVERLAY_BASE_VA) as usize)));
    }

    #[test]
    fn zero_also_removes_the_min_one_floor() {
        let mut ov = synth_overlay();
        apply(&mut ov, 0);
        assert_eq!(current(&ov).unwrap(), 0);
        assert_eq!(word_at(&ov, FLOOR_VA).unwrap(), FLOOR_NONE);
        // Any non-zero magnitude keeps retail's max(pct, 1).
        apply(&mut ov, 5);
        assert_eq!(word_at(&ov, FLOOR_VA).unwrap(), FLOOR_RETAIL);
    }

    #[test]
    fn negative_value_flips_the_tail_and_kills_the_bonus_arms() {
        let mut ov = synth_overlay();
        apply(&mut ov, -40);
        assert_eq!(current(&ov).unwrap(), -40);
        assert_eq!(
            word_at(&ov, SCALE_VAS[0]).unwrap(),
            SCALE_ORI_PREFIX | 40,
            "magnitude drives the scale"
        );
        assert_eq!(word_at(&ov, TAIL_VAS[0]).unwrap(), TAIL_DRAIN[0]);
        assert_eq!(word_at(&ov, BOOST2_BR_VA).unwrap(), BOOST2_BR_SKIP);
        assert_eq!(word_at(&ov, BOOST1_BR_VA).unwrap(), BOOST1_BR_SKIP);
        // The min-1 floor stays: a scratch still drains 1 AP.
        assert_eq!(word_at(&ov, FLOOR_VA).unwrap(), FLOOR_RETAIL);
    }

    #[test]
    fn round_trips_through_negative_back_to_retail() {
        let stock = synth_overlay();
        let mut ov = stock.clone();
        apply(&mut ov, -200);
        assert_eq!(current(&ov).unwrap(), -200);
        apply(&mut ov, 175);
        assert_eq!(current(&ov).unwrap(), 175);
        apply(&mut ov, RETAIL_DAMAGE_AP);
        assert_eq!(current(&ov).unwrap(), RETAIL_DAMAGE_AP);
        assert_eq!(ov, stock, "restoring retail restores every stock word");
        assert_eq!(plan(&ov, RETAIL_DAMAGE_AP).unwrap(), None);
    }

    #[test]
    fn negative_zero_normalizes_to_zero() {
        let mut a = synth_overlay();
        let mut b = synth_overlay();
        apply(&mut a, 0);
        apply(&mut b, -0);
        assert_eq!(a, b);
        assert_eq!(current(&a).unwrap(), 0);
    }

    #[test]
    fn refuses_out_of_range_and_unrecognized_builds() {
        let ov = synth_overlay();
        assert!(plan(&ov, MAX_DAMAGE_AP + 1).is_err());
        assert!(plan(&ov, -MAX_DAMAGE_AP - 1).is_err());
        // Perturbed context word.
        let mut bad = ov.clone();
        put(&mut bad, 0x801D_E1E0, 0xDEAD_BEEF);
        assert!(plan(&bad, 10).is_err());
        // Perturbed scale (neither retail chain nor the multiply form).
        let mut bad2 = ov.clone();
        put(&mut bad2, SCALE_VAS[2], 0x1234_5678);
        assert!(plan(&bad2, 10).is_err());
        // Perturbed tail.
        let mut bad3 = ov.clone();
        put(&mut bad3, TAIL_VAS[3], 0x1234_5678);
        assert!(plan(&bad3, 10).is_err());
        // Truncated overlay.
        assert!(plan(&ov[..0x100], 10).is_err());
    }

    #[test]
    fn site_offsets_are_linear_from_base() {
        assert_eq!((SCALE_VAS[0] - OVERLAY_BASE_VA) as usize, 0xF9B0);
        assert_eq!((FLOOR_VA - OVERLAY_BASE_VA) as usize, 0xF9E0);
        assert_eq!((TAIL_VAS[0] - OVERLAY_BASE_VA) as usize, 0xFAA8);
    }

    /// The drain tail's branch must land on the store, and the bonus-arm
    /// skips must keep retail's targets - encode them here so a typo in a
    /// hand-assembled word is caught without a disc.
    #[test]
    fn hand_assembled_branch_targets_resolve() {
        // bgez v0,+2 at 0x801de2c4 -> 0x801de2d0 (the `sh`).
        let bgez = TAIL_DRAIN[1];
        let off = (bgez & 0xFFFF) as i16 as i32;
        assert_eq!(
            (TAIL_VAS[1] as i32 + 4 + off * 4) as u32,
            TAIL_VAS[4],
            "floor branch skips the `move v0,zero`"
        );
        // The neutralized guards keep the retail branch displacement, so they
        // still land on the arm's exit.
        assert_eq!(BOOST2_BR_SKIP & 0xFFFF, BOOST2_BR_RETAIL & 0xFFFF);
        assert_eq!(BOOST1_BR_SKIP & 0xFFFF, BOOST1_BR_RETAIL & 0xFFFF);
        assert_eq!(BOOST2_BR_SKIP >> 16, 0x1000, "beq zero,zero");
        assert_eq!(BOOST1_BR_SKIP >> 16, 0x1000, "beq zero,zero");
    }
}
