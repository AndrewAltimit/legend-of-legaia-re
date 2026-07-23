//! **Jewel fix**: make Xain's signature casts respect elemental guards.
//!
//! ## The retail behaviour (pinned)
//!
//! Xain is element 0 (Earth), yet his two signature casts - **Bloody Horns**
//! (spell id `0x5C`) and **Terio Punch** (`0x5D`) - are unaffected by Earth
//! Jewels (and every other elemental guard, and All Guard). The element is not
//! dropped anywhere: those spells are *capture-class* boss cinematic casts
//! (spell-table class byte `'c'`), each shipping its own streamed code module
//! (`FUN_8003EC70(record[+1] + 0x28)` -> PROT entries [`BLOODY_HORNS_PROT_INDEX`]
//! / [`TERIO_PUNCH_PROT_INDEX`]) with baked-in per-hit power constants. Each
//! module's damage calls pick one of two SCUS wrappers around the shared
//! scale + finish kernels:
//!
//! - `FUN_801DD4B0` ([`RESPECT_WRAPPER_VA`]) - calls the finisher
//!   `FUN_801DDB30` with `param_5 = 0`: the party-defender resist ladder runs
//!   (elemental jewels / guards / All Guard apply).
//! - `FUN_801DD6B4` ([`BYPASS_WRAPPER_VA`]) - same roll shape but finisher
//!   `param_5 = 1`: the **entire** resist block is skipped.
//!
//! Both wrappers pass the caster's seat, so the affinity scale reads Xain's
//! true Earth element either way - the bypass is purely the finisher's
//! `param_5 == 0` gate. Bloody Horns' big hit (power `0x1D0`) and Terio
//! Punch's hit (`0x274`) call the bypass wrapper; the enemy-side Evil Seru
//! Magic module calls the respecting one (which is why Cort's ESM behaves as
//! Dark). See `docs/subsystems/battle-formulas.md` and
//! `docs/formats/spell-table.md` (cast classes).
//!
//! ## The fix
//!
//! Retarget the two `jal FUN_801DD6B4` words - one per module - to
//! `jal FUN_801DD4B0`: a same-size, two-word in-place PROT edit. The
//! wrappers share the argument contract (`a0` = power constant, `a1` =
//! attacker slot, `a2` = defender slot; both roll internally and return the
//! damage margin), so the retarget changes nothing but the finisher's
//! `param_5`, and the resist ladder engages exactly as it does for every
//! ordinary monster special. Bloody Horns' small second component (power
//! `0x80`, entry offset `0x15B0`) already uses the respecting wrapper in
//! retail and is untouched.
//!
//! Terio Punch shares its module (spell sub-index `0x12`) with **Bull Charge**
//! (`0x5E`), so the fix covers that cast too.
//!
//! NB the neighbouring `09xx` PROT extents **overlap on disc**: entry 953's
//! window starts [`OVERLAP_953_IN_952`] bytes into entry 952's, so the Terio
//! Punch word also appears in the Bloody Horns entry window at
//! `0x1800 + 0xA38 = 0x2238`. That is the same physical word, not a third
//! site - the fix writes each physical word exactly once.
//!
//! Each site's stock word is verified before it is replaced - a
//! differently-laid-out image is refused, not corrupted. No Sony bytes are
//! embedded: the patch words are `jal` encodings of documented SCUS entry
//! points.

use anyhow::{Result, bail};

use crate::mips::jal;

/// PROT entry (extraction index) of the streamed Bloody Horns cast module
/// (spell `0x5C`, capture sub-index `0x11` -> loader arg `0x39`).
pub const BLOODY_HORNS_PROT_INDEX: usize = 952;
/// PROT entry (extraction index) of the streamed Terio Punch / Bull Charge cast
/// module (spells `0x5D`/`0x5E`, capture sub-index `0x12` -> loader arg `0x3A`).
pub const TERIO_PUNCH_PROT_INDEX: usize = 953;

/// `FUN_801DD4B0` - damage wrapper whose finisher call passes `param_5 = 0`
/// (party-defender resist ladder runs).
pub const RESPECT_WRAPPER_VA: u32 = 0x801D_D4B0;
/// `FUN_801DD6B4` - damage wrapper whose finisher call passes `param_5 = 1`
/// (party-defender resist ladder skipped).
pub const BYPASS_WRAPPER_VA: u32 = 0x801D_D6B4;

/// Byte offset of PROT entry 953's disc extent inside entry 952's window (the
/// neighbouring `09xx` extents overlap; measured against the USA disc).
pub const OVERLAP_953_IN_952: usize = 0x1800;

/// One retargeted damage call: `jal FUN_801DD6B4` -> `jal FUN_801DD4B0` at a
/// fixed offset inside a cast module's raw PROT entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Site {
    /// PROT entry (extraction index) holding the module.
    pub prot_index: usize,
    /// Byte offset of the `jal` word within the entry.
    pub offset: usize,
    /// The hit's baked power constant (documentation only; not patched).
    pub power: u16,
}

/// The two physically-distinct bypass-wrapper call sites, one per module. (The
/// Terio Punch word also aliases into entry 952's window at
/// `OVERLAP_953_IN_952 + 0xA38 = 0x2238` - same disc bytes, not a third site.)
pub const SITES: [Site; 2] = [
    Site {
        prot_index: BLOODY_HORNS_PROT_INDEX,
        offset: 0x0F70,
        power: 0x1D0,
    },
    Site {
        prot_index: TERIO_PUNCH_PROT_INDEX,
        offset: 0x0A38,
        power: 0x274,
    },
];

/// The stock word at every site: `jal FUN_801DD6B4` (`0x0C0775AD`).
pub const fn bypass_word() -> u32 {
    jal(BYPASS_WRAPPER_VA)
}

/// The replacement word: `jal FUN_801DD4B0` (`0x0C07752C`).
pub const fn respect_word() -> u32 {
    jal(RESPECT_WRAPPER_VA)
}

/// A planned jewel fix: the two same-size word writes, verified against the
/// stock bytes first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JewelFix {
    /// `(prot_index, offset, word)` writes, one per [`SITES`] entry.
    pub writes: Vec<(usize, usize, u32)>,
}

impl JewelFix {
    /// Plan the fix given the two modules' raw PROT entry bytes. Fails (rather
    /// than corrupts) if any site does not hold the stock
    /// `jal FUN_801DD6B4` word - an unrecognized build, or an already-patched
    /// image.
    pub fn plan(bloody_horns: &[u8], terio_punch: &[u8]) -> Result<Self> {
        let mut writes = Vec::with_capacity(SITES.len());
        for site in SITES {
            let entry = match site.prot_index {
                BLOODY_HORNS_PROT_INDEX => bloody_horns,
                _ => terio_punch,
            };
            let Some(bytes) = entry.get(site.offset..site.offset + 4) else {
                bail!(
                    "cast module PROT {} is only {} bytes; call site +{:#x} out of range",
                    site.prot_index,
                    entry.len(),
                    site.offset
                );
            };
            let word = u32::from_le_bytes(bytes.try_into().unwrap());
            if word != bypass_word() {
                bail!(
                    "cast module PROT {} +{:#x} = {word:#010x}, expected {:#010x} \
                     (`jal FUN_801DD6B4`; unrecognized or already-patched build) - refusing to patch",
                    site.prot_index,
                    site.offset,
                    bypass_word(),
                );
            }
            writes.push((site.prot_index, site.offset, respect_word()));
        }
        Ok(Self { writes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrapper_jal_words_match_the_disc_bytes() {
        // The raw little-endian words observed at the call sites on the USA
        // disc: `ad 75 07 0c` (bypass) and `2c 75 07 0c` (respect).
        assert_eq!(bypass_word(), 0x0C07_75AD);
        assert_eq!(respect_word(), 0x0C07_752C);
    }

    #[test]
    fn plan_refuses_an_unrecognized_build() {
        let zeroed = vec![0u8; 0x3000];
        assert!(JewelFix::plan(&zeroed, &zeroed).is_err());
    }

    #[test]
    fn plan_refuses_a_truncated_entry() {
        let short = vec![0u8; 0x100];
        assert!(JewelFix::plan(&short, &short).is_err());
    }

    #[test]
    fn plan_accepts_stock_words_and_retargets_both_sites() {
        let mut bh = vec![0u8; 0x3000];
        let mut tp = vec![0u8; 0x3000];
        for site in SITES {
            let entry = if site.prot_index == BLOODY_HORNS_PROT_INDEX {
                &mut bh
            } else {
                &mut tp
            };
            entry[site.offset..site.offset + 4].copy_from_slice(&bypass_word().to_le_bytes());
        }
        let plan = JewelFix::plan(&bh, &tp).expect("stock build accepted");
        assert_eq!(plan.writes.len(), 2);
        assert!(plan.writes.iter().all(|&(_, _, w)| w == respect_word()));
        // Site offsets survive into the writes verbatim.
        assert_eq!(
            plan.writes[0],
            (BLOODY_HORNS_PROT_INDEX, 0x0F70, respect_word())
        );
        assert_eq!(
            plan.writes[1],
            (TERIO_PUNCH_PROT_INDEX, 0x0A38, respect_word())
        );
    }

    #[test]
    fn planning_is_idempotence_guarded() {
        // A second plan over an already-patched image must refuse (the sites
        // now hold the respect word, not the stock bypass word).
        let mut bh = vec![0u8; 0x3000];
        let mut tp = vec![0u8; 0x3000];
        for site in SITES {
            let entry = if site.prot_index == BLOODY_HORNS_PROT_INDEX {
                &mut bh
            } else {
                &mut tp
            };
            entry[site.offset..site.offset + 4].copy_from_slice(&respect_word().to_le_bytes());
        }
        assert!(JewelFix::plan(&bh, &tp).is_err());
    }
}
