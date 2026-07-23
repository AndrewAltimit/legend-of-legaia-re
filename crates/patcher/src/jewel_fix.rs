//! **Jewel fix**: make the boss cinematic casts respect elemental guards.
//!
//! ## The retail behaviour (pinned)
//!
//! The boss signature casts are *capture-class* spells (spell-table class byte
//! `'c'`), each shipping its own streamed code module
//! (`FUN_8003EC70(record[+1] + 0x28)` -> PROT entries 935..966) with baked-in
//! per-hit power constants. Each module's damage calls pick one of two SCUS
//! wrappers around the shared scale + finish kernels:
//!
//! - `FUN_801DD4B0` ([`RESPECT_WRAPPER_VA`]) - calls the finisher
//!   `FUN_801DDB30` with `param_5 = 0`: the party-defender resist ladder runs
//!   (elemental jewels / guards / All Guard apply).
//! - `FUN_801DD6B4` ([`BYPASS_WRAPPER_VA`]) - same roll shape but finisher
//!   `param_5 = 1`: the **entire** resist block is skipped.
//!
//! Both wrappers pass the caster's seat, so the affinity scale reads the
//! caster's true record element either way - the bypass is purely the
//! finisher's `param_5 == 0` gate. The full wrapper census over every
//! capture-class module (`docs/subsystems/battle-formulas.md`) finds the
//! bypass wrapper in exactly six modules - the boss signature-move set:
//!
//! | Module | Spell(s) | Caster |
//! |---|---|---|
//! | PROT 944 | Guilty Cross `0x37`, Curse All `0x53` | Cort (humanoid phases) |
//! | PROT 952 | Bloody Horns `0x5C` (Astral Slash `0xB8` shares the module; its dispatched tick has no damage call and it **respects** guards in playtests) | Xain; Gaza (first fight) |
//! | PROT 953 | Terio Punch `0x5D`, Bull Charge `0x5E` | Xain |
//! | PROT 958 | Blazing Slash `0x79` | Gi Delilas |
//! | PROT 959 | Megaton Press `0x7A` | Che Delilas |
//! | PROT 960 | Plasma Strike `0x7B` (Neo Star Slash `0xA6` shares the module and **respects** - its tick is untouched) | Lu Delilas |
//!
//! Every other capture-class damage module (incl. every Songi cast, Neo Star
//! Slash, and the enemy-side Evil Seru Magic) already calls the respecting
//! wrapper; plain-class casts, player summons, and move-power specials all
//! reach the finisher with `param_5 = 0`.
//!
//! ## The fix
//!
//! Retarget all thirteen `jal FUN_801DD6B4` words across the six modules to
//! `jal FUN_801DD4B0` - a same-size in-place PROT edit. The wrappers share
//! the argument contract (`a0` = power, `a1` = attacker slot, `a2` = defender
//! slot; both roll internally and return the damage margin), so the retarget
//! changes nothing but the finisher's `param_5`, and the resist ladder
//! engages exactly as it does for every ordinary monster special. Shared
//! modules dispatch per spell at a module-head id switch, so
//! respecting-spell paths (Neo Star Slash) are untouched, and PROT 952's
//! unreachable template respect call (`+0x15B0`) is left alone.
//!
//! NB the neighbouring `09xx` PROT extents **overlap on disc** (e.g. entry
//! 953's window starts [`OVERLAP_953_IN_952`] bytes into entry 952's, so the
//! Terio Punch word also appears in the Bloody Horns entry window at
//! `0x1800 + 0xA38 = 0x2238`). Every [`SITES`] offset lies inside its
//! module's **own** extent (bounded by the next entry's head-overlap), so
//! each physical word is written exactly once.
//!
//! Each site's stock word is verified before it is replaced - a
//! differently-laid-out image is refused, not corrupted. No Sony bytes are
//! embedded: the patch words are `jal` encodings of documented SCUS entry
//! points.

use std::collections::BTreeMap;

use anyhow::{Result, bail};

use crate::mips::jal;

/// PROT entry (extraction index) of the Guilty Cross / Curse All cast module
/// (capture sub-index `0x09` -> loader arg `0x31`).
pub const GUILTY_CROSS_PROT_INDEX: usize = 944;
/// PROT entry (extraction index) of the streamed Bloody Horns cast module
/// (spell `0x5C`, capture sub-index `0x11` -> loader arg `0x39`).
pub const BLOODY_HORNS_PROT_INDEX: usize = 952;
/// PROT entry (extraction index) of the streamed Terio Punch / Bull Charge cast
/// module (spells `0x5D`/`0x5E`, capture sub-index `0x12` -> loader arg `0x3A`).
pub const TERIO_PUNCH_PROT_INDEX: usize = 953;
/// PROT entry (extraction index) of the Blazing Slash cast module (Gi Delilas,
/// capture sub-index `0x17`).
pub const BLAZING_SLASH_PROT_INDEX: usize = 958;
/// PROT entry (extraction index) of the Megaton Press cast module (Che
/// Delilas, capture sub-index `0x18`).
pub const MEGATON_PRESS_PROT_INDEX: usize = 959;
/// PROT entry (extraction index) of the Plasma Strike / Neo Star Slash cast
/// module (Lu Delilas / Sim-Seru Gaza, capture sub-index `0x19`). Only the
/// Plasma Strike tick carries the bypass call.
pub const PLASMA_STRIKE_PROT_INDEX: usize = 960;

/// Every PROT entry the fix touches, ascending.
pub const MODULE_INDICES: [usize; 6] = [
    GUILTY_CROSS_PROT_INDEX,
    BLOODY_HORNS_PROT_INDEX,
    TERIO_PUNCH_PROT_INDEX,
    BLAZING_SLASH_PROT_INDEX,
    MEGATON_PRESS_PROT_INDEX,
    PLASMA_STRIKE_PROT_INDEX,
];

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
    /// Byte offset of the `jal` word within the entry (always inside the
    /// module's own extent).
    pub offset: usize,
    /// The hit's baked power constant where it is a nearby `li a0, imm`
    /// (documentation only; `0` = register-computed at the call site).
    pub power: u16,
}

const fn site(prot_index: usize, offset: usize, power: u16) -> Site {
    Site {
        prot_index,
        offset,
        power,
    }
}

/// The thirteen physically-distinct bypass-wrapper call sites across the six
/// modules, each inside its module's own extent.
pub const SITES: [Site; 13] = [
    site(GUILTY_CROSS_PROT_INDEX, 0x0100, 0x38E),
    site(BLOODY_HORNS_PROT_INDEX, 0x0F70, 0x1D0),
    site(TERIO_PUNCH_PROT_INDEX, 0x0A38, 0x274),
    site(BLAZING_SLASH_PROT_INDEX, 0x0B14, 0),
    site(BLAZING_SLASH_PROT_INDEX, 0x0E7C, 0x38),
    site(BLAZING_SLASH_PROT_INDEX, 0x12CC, 0x38),
    site(BLAZING_SLASH_PROT_INDEX, 0x170C, 0x38),
    site(BLAZING_SLASH_PROT_INDEX, 0x1D7C, 0x40),
    site(BLAZING_SLASH_PROT_INDEX, 0x1F00, 0),
    site(MEGATON_PRESS_PROT_INDEX, 0x0810, 0x80),
    site(MEGATON_PRESS_PROT_INDEX, 0x10B8, 0x80),
    site(MEGATON_PRESS_PROT_INDEX, 0x14E4, 0x30),
    site(PLASMA_STRIKE_PROT_INDEX, 0x1790, 0x1C0),
];

/// The stock word at every site: `jal FUN_801DD6B4` (`0x0C0775AD`).
pub const fn bypass_word() -> u32 {
    jal(BYPASS_WRAPPER_VA)
}

/// The replacement word: `jal FUN_801DD4B0` (`0x0C07752C`).
pub const fn respect_word() -> u32 {
    jal(RESPECT_WRAPPER_VA)
}

/// A planned jewel fix: the thirteen same-size word writes, verified against
/// the stock bytes first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JewelFix {
    /// `(prot_index, offset, word)` writes, one per [`SITES`] entry.
    pub writes: Vec<(usize, usize, u32)>,
}

impl JewelFix {
    /// Plan the fix given each touched module's raw PROT entry bytes, keyed by
    /// extraction index (every index in [`MODULE_INDICES`] must be present).
    /// Fails (rather than corrupts) if any site does not hold the stock
    /// `jal FUN_801DD6B4` word - an unrecognized build, or an already-patched
    /// image.
    pub fn plan(modules: &BTreeMap<usize, Vec<u8>>) -> Result<Self> {
        let mut writes = Vec::with_capacity(SITES.len());
        for s in SITES {
            let Some(entry) = modules.get(&s.prot_index) else {
                bail!("cast module PROT {} bytes not supplied", s.prot_index);
            };
            let Some(bytes) = entry.get(s.offset..s.offset + 4) else {
                bail!(
                    "cast module PROT {} is only {} bytes; call site +{:#x} out of range",
                    s.prot_index,
                    entry.len(),
                    s.offset
                );
            };
            let word = u32::from_le_bytes(bytes.try_into().unwrap());
            if word != bypass_word() {
                bail!(
                    "cast module PROT {} +{:#x} = {word:#010x}, expected {:#010x} \
                     (`jal FUN_801DD6B4`; unrecognized or already-patched build) - refusing to patch",
                    s.prot_index,
                    s.offset,
                    bypass_word(),
                );
            }
            writes.push((s.prot_index, s.offset, respect_word()));
        }
        Ok(Self { writes })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_modules() -> BTreeMap<usize, Vec<u8>> {
        let mut m: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
        for idx in MODULE_INDICES {
            m.insert(idx, vec![0u8; 0x3800]);
        }
        for s in SITES {
            m.get_mut(&s.prot_index).unwrap()[s.offset..s.offset + 4]
                .copy_from_slice(&bypass_word().to_le_bytes());
        }
        m
    }

    #[test]
    fn wrapper_jal_words_match_the_disc_bytes() {
        // The raw little-endian words observed at the call sites on the USA
        // disc: `ad 75 07 0c` (bypass) and `2c 75 07 0c` (respect).
        assert_eq!(bypass_word(), 0x0C07_75AD);
        assert_eq!(respect_word(), 0x0C07_752C);
    }

    #[test]
    fn sites_cover_exactly_the_module_set() {
        let mut idxs: Vec<usize> = SITES.iter().map(|s| s.prot_index).collect();
        idxs.sort_unstable();
        idxs.dedup();
        assert_eq!(idxs, MODULE_INDICES.to_vec());
        // No duplicate (entry, offset) pairs - each physical word once.
        let mut pairs: Vec<(usize, usize)> =
            SITES.iter().map(|s| (s.prot_index, s.offset)).collect();
        pairs.sort_unstable();
        let n = pairs.len();
        pairs.dedup();
        assert_eq!(pairs.len(), n);
    }

    #[test]
    fn plan_refuses_an_unrecognized_build() {
        let mut m = stock_modules();
        // Corrupt one site.
        m.get_mut(&BLAZING_SLASH_PROT_INDEX).unwrap()[0x12CC] ^= 0xFF;
        assert!(JewelFix::plan(&m).is_err());
    }

    #[test]
    fn plan_refuses_a_missing_or_truncated_module() {
        let mut m = stock_modules();
        m.remove(&MEGATON_PRESS_PROT_INDEX);
        assert!(JewelFix::plan(&m).is_err());
        let mut m = stock_modules();
        m.get_mut(&PLASMA_STRIKE_PROT_INDEX)
            .unwrap()
            .truncate(0x100);
        assert!(JewelFix::plan(&m).is_err());
    }

    #[test]
    fn plan_accepts_stock_words_and_retargets_every_site() {
        let plan = JewelFix::plan(&stock_modules()).expect("stock build accepted");
        assert_eq!(plan.writes.len(), SITES.len());
        assert!(plan.writes.iter().all(|&(_, _, w)| w == respect_word()));
        assert_eq!(
            plan.writes[1],
            (BLOODY_HORNS_PROT_INDEX, 0x0F70, respect_word())
        );
        assert_eq!(
            plan.writes[12],
            (PLASMA_STRIKE_PROT_INDEX, 0x1790, respect_word())
        );
    }

    #[test]
    fn planning_is_idempotence_guarded() {
        // A second plan over an already-patched image must refuse (the sites
        // now hold the respect word, not the stock bypass word).
        let mut m = stock_modules();
        for s in SITES {
            m.get_mut(&s.prot_index).unwrap()[s.offset..s.offset + 4]
                .copy_from_slice(&respect_word().to_le_bytes());
        }
        assert!(JewelFix::plan(&m).is_err());
    }
}
