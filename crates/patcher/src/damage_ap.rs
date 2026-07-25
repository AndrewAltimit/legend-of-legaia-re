//! **Enemy-damage AP tuning**: set how much AP a party member's battle gauge
//! gains when an enemy damages them (retail: a full-HP hit fills the whole
//! 100-point gauge), including a **negative** range where taking damage
//! *drains* AP instead.
//!
//! ## Two copies of one kernel
//!
//! The battle-action overlay (PROT entry 898, base VA `0x801CE818`) carries
//! **two independent, structurally identical copies** of the spirit-gauge
//! fill, and a hit reaches one or the other depending on how it was resolved:
//!
//! | Copy | Host | Reached by |
//! |---|---|---|
//! | A | `FUN_801DDB30` - the closed-form damage finisher | magic / summon / special-attack hits |
//! | B | `FUN_801EC3E4` - the arms execution resolver | ordinary physical hits |
//!
//! Patching only copy A leaves the common case - a regular enemy swing -
//! running stock, which reads as "the slider does nothing". Both copies are
//! therefore always written together, and a build where they disagree is
//! refused as partially patched.
//!
//! A structural sweep of the whole entry finds exactly these two: the
//! kernel's `andi v0,v0,0x200` / `andi v0,v0,0x100` "spirit gain up" tests
//! and its `sltiu rX,v0,0x1` min-one floor co-occur at `0x801DE1F8` and
//! `0x801EDBB0` and nowhere else.
//!
//! ## Where the retail value lives
//!
//! Each copy fills the defender's AP gauge (`actor[+0x170]`) in proportion to
//! the fraction of max HP the hit took. The scale factor is synthesized as a
//! shift/add chain rather than an immediate - `d*2 + d = d*3`, `*8 = d*24`,
//! `+d = d*25`, `<<2 = d*100` (copy A shown; copy B is the same chain around
//! its own interleaved loads):
//!
//! ```text
//! 801de1c4  subu v1,v1,v0     ; v1 = damage
//! 801de1c8  sll  v0,v1,0x1    ; \
//! 801de1cc  addu v0,v0,v1     ; |
//! 801de1d0  sll  v0,v0,0x3    ; |  x100   <- the scale
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
//! `N = 100` restores each retail chain byte-for-byte, so the retail value is
//! a true no-op. `N = 0` additionally rewrites the min-1 floor to a `move`,
//! because retail's `max(pct, 1)` would otherwise still grant 1 AP per hit.
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

/// PROT entry index of the battle-action overlay hosting both kernel copies.
pub const BATTLE_ACTION_OVERLAY_PROT_INDEX: usize =
    legaia_asset::move_power::BATTLE_ACTION_OVERLAY_PROT_INDEX;

/// Load base VA of the battle-action overlay (raw entry: file offset =
/// `va - OVERLAY_BASE_VA`).
pub const OVERLAY_BASE_VA: u32 = legaia_asset::move_power::BATTLE_OVERLAY_BASE;

/// The retail scale: a hit costing 100% of max HP grants 100 AP (a full gauge).
pub const RETAIL_DAMAGE_AP: i16 = 100;

/// Largest magnitude the slider accepts, in either direction.
pub const MAX_DAMAGE_AP: i16 = 200;

const NOP: u32 = 0x0000_0000;
/// `ori v0,zero,imm` - the configurable scale factor, first word of the
/// multiply form in both copies.
const SCALE_ORI_PREFIX: u32 = 0x3402_0000;
const MFLO_V0: u32 = 0x0000_1012;

/// One copy of the spirit-gauge-fill kernel, as a set of same-size rewrite
/// sites. Both copies take the same shape; only the registers and the
/// interleaved loads differ.
struct Kernel {
    /// The `x100` shift/add chain, in evaluation order. Words interleaved
    /// between them (the max-HP load) are context, not sites.
    scale_vas: [u32; 5],
    scale_retail: [u32; 5],
    /// `multu` for this copy's damage register, times `v0`.
    scale_multu: u32,
    /// `sltiu rX,v0,0x1`, the `a = max(pct, 1)` floor, and the `move rX,zero`
    /// that removes it when the scale is zero.
    floor_va: u32,
    floor_retail: u32,
    floor_none: u32,
    /// Guard branches of the two "spirit gain up" bonus arms, and the
    /// `beq zero,zero` that makes each unconditional.
    boost_br_vas: [u32; 2],
    boost_br_retail: [u32; 2],
    boost_br_skip: [u32; 2],
    /// The accrual + clamp tail. Copy A's is seven words, copy B's ten.
    tail_vas: &'static [u32],
    tail_retail: &'static [u32],
    tail_drain: &'static [u32],
    /// Words around the sites that are never rewritten.
    context: &'static [(u32, u32)],
}

/// Copy A - `FUN_801DDB30`, the closed-form damage finisher (magic / summon /
/// special-attack hits). Damage is in `v1`, the defender pointer in `s1`, the
/// percentage in `a1`.
const KERNEL_A: Kernel = Kernel {
    scale_vas: [
        0x801D_E1C8,
        0x801D_E1CC,
        0x801D_E1D0,
        0x801D_E1D4,
        0x801D_E1DC,
    ],
    scale_retail: [
        0x0003_1040, // sll  v0,v1,0x1
        0x0043_1021, // addu v0,v0,v1
        0x0002_10C0, // sll  v0,v0,0x3
        0x0043_1021, // addu v0,v0,v1
        0x0002_1080, // sll  v0,v0,0x2
    ],
    scale_multu: 0x0062_0019, // multu v1,v0
    floor_va: 0x801D_E1F8,
    floor_retail: 0x2C45_0001, // sltiu a1,v0,0x1
    floor_none: 0x0000_2821,   // move  a1,zero
    boost_br_vas: [0x801D_E248, 0x801D_E290],
    boost_br_retail: [0x1040_0005, 0x1040_0009],
    boost_br_skip: [0x1000_0005, 0x1000_0009],
    tail_vas: &[
        0x801D_E2C0,
        0x801D_E2C4,
        0x801D_E2C8,
        0x801D_E2CC,
        0x801D_E2D0,
        0x801D_E2D4,
        0x801D_E2D8,
    ],
    tail_retail: &[
        0x0045_1021, // addu  v0,v0,a1
        0xA622_0170, // sh    v0,0x170(s1)
        0x3042_FFFF, // andi  v0,v0,0xffff
        0x2C42_0065, // sltiu v0,v0,0x65
        0x1440_0002, // bne   v0,zero,0x801de2dc
        0x2402_0064, // li    v0,0x64
        0xA622_0170, // sh    v0,0x170(s1)
    ],
    tail_drain: &[
        0x0045_1023, // subu  v0,v0,a1
        0x0441_0002, // bgez  v0,0x801de2d0
        NOP,
        0x0000_1021, // move  v0,zero
        0xA622_0170, // sh    v0,0x170(s1)
        NOP,
        NOP,
    ],
    context: &[
        (0x801D_E1C4, 0x0062_1823), // subu  v1,v1,v0     (damage)
        (0x801D_E1D8, 0x9623_014E), // lhu   v1,0x14e(s1) (max HP)
        (0x801D_E1E0, 0x0043_001B), // divu  v0,v1
        (0x801D_E1F0, 0x0000_1012), // mflo  v0
        (0x801D_E1FC, 0x00A2_2821), // addu  a1,a1,v0
        (0x801D_E200, 0x2C62_0003), // sltiu v0,v1,0x3    (party-only gate)
        (0x801D_E244, 0x3042_0200), // andi  v0,v0,0x200  (spirit gain up 2)
        (0x801D_E28C, 0x3042_0100), // andi  v0,v0,0x100  (spirit gain up 1)
        (0x801D_E2B8, 0x9622_0170), // lhu   v0,0x170(s1) (gauge load)
        (0x801D_E2BC, 0x0000_0000), // nop                (load delay)
    ],
};

/// Copy B - `FUN_801EC3E4`, the arms execution resolver (ordinary physical
/// hits). Damage is in `a0`, the defender pointer in `a1`, the percentage in
/// `a2`. Its chain's first word is a **branch delay slot** (the `beq` at
/// `0x801EDB74` joins at `0x801EDB80`), so the scale factor is loaded there
/// and the multiply lands on the join - both paths see the same `v0`.
const KERNEL_B: Kernel = Kernel {
    scale_vas: [
        0x801E_DB78,
        0x801E_DB80,
        0x801E_DB84,
        0x801E_DB8C,
        0x801E_DB94,
    ],
    scale_retail: [
        0x0004_1040, // sll  v0,a0,0x1   (branch delay slot)
        0x0044_1021, // addu v0,v0,a0    (branch join)
        0x0002_10C0, // sll  v0,v0,0x3
        0x0044_1021, // addu v0,v0,a0
        0x0002_1080, // sll  v0,v0,0x2
    ],
    scale_multu: 0x0082_0019, // multu a0,v0
    floor_va: 0x801E_DBB0,
    floor_retail: 0x2C43_0001, // sltiu v1,v0,0x1
    floor_none: 0x0000_1821,   // move  v1,zero
    boost_br_vas: [0x801E_DC00, 0x801E_DC48],
    boost_br_retail: [0x1040_0005, 0x1040_000C],
    boost_br_skip: [0x1000_0005, 0x1000_000C],
    tail_vas: &[
        0x801E_DCA0,
        0x801E_DCA4,
        0x801E_DCA8,
        0x801E_DCAC,
        0x801E_DCB0,
        0x801E_DCB4,
        0x801E_DCB8,
        0x801E_DCBC,
        0x801E_DCC4,
        0x801E_DCC8,
    ],
    tail_retail: &[
        0x0043_1021, // addu  v0,v0,v1
        0xA4A2_0170, // sh    v0,0x170(a1)
        0x8C84_0000, // lw    a0,0x0(a0)
        NOP,
        0x9482_0170, // lhu   v0,0x170(a0)
        NOP,
        0x2C42_0065, // sltiu v0,v0,0x65
        0x1440_0004, // bne   v0,zero,0x801edcd0
        0x2402_0064, // li    v0,0x64
        0xA482_0170, // sh    v0,0x170(a0)
    ],
    tail_drain: &[
        0x0043_1023, // subu  v0,v0,v1
        0x0441_0002, // bgez  v0,0x801edcb0
        NOP,
        0x0000_1021, // move  v0,zero
        0xA4A2_0170, // sh    v0,0x170(a1)
        NOP,
        NOP,
        NOP,
        NOP,
        NOP,
    ],
    context: &[
        (0x801E_DB74, 0x1040_0002), // beq   v0,zero,0x801edb80 (chain join)
        (0x801E_DB88, 0x8D05_0000), // lw    a1,0x0(t0)
        (0x801E_DB90, 0x94A3_014E), // lhu   v1,0x14e(a1)       (max HP)
        (0x801E_DB98, 0x0043_001B), // divu  v0,v1
        (0x801E_DBA8, 0x0000_1012), // mflo  v0
        (0x801E_DBB4, 0x0062_3021), // addu  a2,v1,v0
        (0x801E_DBB8, 0x2CE2_0003), // sltiu v0,a3,0x3          (party-only gate)
        (0x801E_DBFC, 0x3042_0200), // andi  v0,v0,0x200        (spirit gain up 2)
        (0x801E_DC44, 0x3042_0100), // andi  v0,v0,0x100        (spirit gain up 1)
        (0x801E_DC90, 0x8C85_0000), // lw    a1,0x0(a0)
        (0x801E_DC98, 0x94A2_0170), // lhu   v0,0x170(a1)       (gauge load)
        (0x801E_DC9C, 0x30C3_00FF), // andi  v1,a2,0xff         (the delta)
        (0x801E_DCC0, 0x3C02_8008), // lui   v0,0x8008          (live after the tail)
        (0x801E_DCCC, 0x3C02_8008), // lui   v0,0x8008
        (0x801E_DCD0, 0x8C42_45C8), // lw    v0,0x45c8(v0)      (tail join)
    ],
};

const KERNELS: [&Kernel; 2] = [&KERNEL_A, &KERNEL_B];

fn word_at(overlay: &[u8], va: u32) -> Result<u32> {
    let off = (va - OVERLAY_BASE_VA) as usize;
    let b = overlay.get(off..off + 4).ok_or_else(|| {
        anyhow::anyhow!("overlay entry too short for word at {va:#x} (+{off:#x})")
    })?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()))
}

impl Kernel {
    /// The `(va, word)` this copy should hold for `value`.
    fn desired(&self, value: i16) -> Vec<(u32, u32)> {
        let magnitude = value.unsigned_abs();
        let draining = value < 0 && magnitude != 0;
        let mut out = Vec::with_capacity(18);

        // Scale. The retail factor keeps retail's own shift/add chain so that
        // `--damage-ap 100` is a genuine no-op on a stock image.
        let scale = if !draining && magnitude == RETAIL_DAMAGE_AP as u16 {
            self.scale_retail
        } else {
            [
                SCALE_ORI_PREFIX | magnitude as u32,
                self.scale_multu,
                MFLO_V0,
                NOP,
                NOP,
            ]
        };
        for (va, word) in self.scale_vas.iter().zip(scale) {
            out.push((*va, word));
        }

        // Min-1 floor: retail grants at least 1 AP per hit. Only a scale of
        // zero needs it removed - otherwise "0 AP from damage" would be 1.
        out.push((
            self.floor_va,
            if magnitude == 0 {
                self.floor_none
            } else {
                self.floor_retail
            },
        ));

        // Bonus arms + accrual tail.
        for i in 0..2 {
            out.push((
                self.boost_br_vas[i],
                if draining {
                    self.boost_br_skip[i]
                } else {
                    self.boost_br_retail[i]
                },
            ));
        }
        let tail = if draining {
            self.tail_drain
        } else {
            self.tail_retail
        };
        for (va, word) in self.tail_vas.iter().zip(tail.iter()) {
            out.push((*va, *word));
        }
        out
    }

    /// Verify this copy's context fingerprint, then decode its configured
    /// value. Refuses anything that is neither retail nor a form this patch
    /// writes.
    fn decode(&self, overlay: &[u8]) -> Result<i16> {
        for (va, want) in self.context {
            let got = word_at(overlay, *va)?;
            if got != *want {
                bail!(
                    "context word {va:#x} = {got:#010x}, expected {want:#010x} \
                     (battle damage-finisher AP fill) - unrecognized build, refusing to patch"
                );
            }
        }

        // Sign comes from the accrual tail; magnitude from the scale.
        let tail: Vec<u32> = self
            .tail_vas
            .iter()
            .map(|va| word_at(overlay, *va))
            .collect::<Result<_>>()?;
        let draining = if tail == self.tail_retail {
            false
        } else if tail == self.tail_drain {
            true
        } else {
            bail!(
                "damage-AP accrual tail at {:#x} is neither the stock accrual nor a \
                 previously configured drain - unrecognized build, refusing to patch",
                self.tail_vas[0]
            );
        };

        let scale: Vec<u32> = self
            .scale_vas
            .iter()
            .map(|va| word_at(overlay, *va))
            .collect::<Result<_>>()?;
        let magnitude = if scale == self.scale_retail {
            RETAIL_DAMAGE_AP as u16
        } else if scale[1] == self.scale_multu
            && scale[2] == MFLO_V0
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
                self.scale_vas[0]
            );
        };

        let value = magnitude as i16;
        Ok(if draining { -value } else { value })
    }
}

/// The complete set of `(va, word)` the image should hold for `value`, across
/// both kernel copies.
fn desired(value: i16) -> Vec<(u32, u32)> {
    KERNELS.iter().flat_map(|k| k.desired(value)).collect()
}

/// Verify both copies and return the configured value. A build where the two
/// copies disagree is refused - it is half-patched, which is exactly the
/// failure mode of only knowing about one of them.
fn recognize(overlay: &[u8]) -> Result<i16> {
    let a = KERNEL_A.decode(overlay)?;
    let b = KERNEL_B.decode(overlay)?;
    if a != b {
        bail!(
            "the two damage-AP kernel copies disagree ({a} at {:#x}, {b} at {:#x}) - \
             partially patched image, refusing to patch",
            KERNEL_A.scale_vas[0],
            KERNEL_B.scale_vas[0]
        );
    }
    Ok(a)
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
        let mut ov = vec![0u8; 0x20000];
        for k in KERNELS {
            for (va, w) in k.context {
                put(&mut ov, *va, *w);
            }
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
    fn both_kernel_copies_are_written() {
        let ov = synth_overlay();
        let p = plan(&ov, 50).unwrap().unwrap();
        assert_eq!(p.previous, RETAIL_DAMAGE_AP);
        let by_off: std::collections::BTreeMap<usize, u32> = p.writes.iter().copied().collect();
        // Scale becomes the multiply form in each copy; nothing else moves.
        assert_eq!(by_off.len(), 10, "five scale words per copy");
        for k in KERNELS {
            assert_eq!(
                by_off[&((k.scale_vas[0] - OVERLAY_BASE_VA) as usize)],
                SCALE_ORI_PREFIX | 50
            );
            assert_eq!(
                by_off[&((k.scale_vas[1] - OVERLAY_BASE_VA) as usize)],
                k.scale_multu
            );
            assert!(!by_off.contains_key(&((k.tail_vas[0] - OVERLAY_BASE_VA) as usize)));
        }
    }

    #[test]
    fn zero_also_removes_the_min_one_floor_in_both_copies() {
        let mut ov = synth_overlay();
        apply(&mut ov, 0);
        assert_eq!(current(&ov).unwrap(), 0);
        for k in KERNELS {
            assert_eq!(word_at(&ov, k.floor_va).unwrap(), k.floor_none);
        }
        // Any non-zero magnitude keeps retail's max(pct, 1).
        apply(&mut ov, 5);
        for k in KERNELS {
            assert_eq!(word_at(&ov, k.floor_va).unwrap(), k.floor_retail);
        }
    }

    #[test]
    fn negative_value_flips_both_tails_and_kills_the_bonus_arms() {
        let mut ov = synth_overlay();
        apply(&mut ov, -40);
        assert_eq!(current(&ov).unwrap(), -40);
        for k in KERNELS {
            assert_eq!(
                word_at(&ov, k.scale_vas[0]).unwrap(),
                SCALE_ORI_PREFIX | 40,
                "magnitude drives the scale"
            );
            assert_eq!(word_at(&ov, k.tail_vas[0]).unwrap(), k.tail_drain[0]);
            assert_eq!(word_at(&ov, k.boost_br_vas[0]).unwrap(), k.boost_br_skip[0]);
            assert_eq!(word_at(&ov, k.boost_br_vas[1]).unwrap(), k.boost_br_skip[1]);
            // The min-1 floor stays: a scratch still drains 1 AP.
            assert_eq!(word_at(&ov, k.floor_va).unwrap(), k.floor_retail);
        }
    }

    #[test]
    fn a_half_patched_image_is_refused() {
        // Patching only one copy is the exact bug that made the slider look
        // inert on ordinary physical hits - it must not be accepted silently.
        let mut ov = synth_overlay();
        for (va, w) in KERNEL_A.desired(-40) {
            put(&mut ov, va, w);
        }
        let err = current(&ov).unwrap_err().to_string();
        assert!(err.contains("disagree"), "got: {err}");
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
        // Perturbed context word, in either copy.
        for va in [0x801D_E1E0, 0x801E_DB98] {
            let mut bad = ov.clone();
            put(&mut bad, va, 0xDEAD_BEEF);
            assert!(plan(&bad, 10).is_err(), "context {va:#x}");
        }
        // Perturbed scale / tail, in either copy.
        for k in KERNELS {
            let mut bad = ov.clone();
            put(&mut bad, k.scale_vas[2], 0x1234_5678);
            assert!(plan(&bad, 10).is_err());
            let mut bad = ov.clone();
            put(&mut bad, k.tail_vas[0], 0x1234_5678);
            assert!(plan(&bad, 10).is_err());
        }
        // Truncated overlay.
        assert!(plan(&ov[..0x100], 10).is_err());
    }

    #[test]
    fn site_offsets_are_linear_from_base() {
        assert_eq!((KERNEL_A.scale_vas[0] - OVERLAY_BASE_VA) as usize, 0xF9B0);
        assert_eq!((KERNEL_A.floor_va - OVERLAY_BASE_VA) as usize, 0xF9E0);
        assert_eq!((KERNEL_A.tail_vas[0] - OVERLAY_BASE_VA) as usize, 0xFAA8);
        assert_eq!((KERNEL_B.scale_vas[0] - OVERLAY_BASE_VA) as usize, 0x1F360);
        assert_eq!((KERNEL_B.floor_va - OVERLAY_BASE_VA) as usize, 0x1F398);
        assert_eq!((KERNEL_B.tail_vas[0] - OVERLAY_BASE_VA) as usize, 0x1F488);
    }

    /// The drain tails are hand-assembled; check every branch each one adds
    /// resolves to the store, and that the neutralized guards keep retail's
    /// displacement - without needing a disc.
    #[test]
    fn hand_assembled_branch_targets_resolve() {
        for k in KERNELS {
            // bgez v0,+2 -> the `sh`, skipping the `move v0,zero`.
            let off = (k.tail_drain[1] & 0xFFFF) as i16 as i32;
            assert_eq!(
                (k.tail_vas[1] as i32 + 4 + off * 4) as u32,
                k.tail_vas[4],
                "floor branch skips the `move v0,zero`"
            );
            for i in 0..2 {
                assert_eq!(k.boost_br_skip[i] & 0xFFFF, k.boost_br_retail[i] & 0xFFFF);
                assert_eq!(k.boost_br_skip[i] >> 16, 0x1000, "beq zero,zero");
            }
        }
    }
}
