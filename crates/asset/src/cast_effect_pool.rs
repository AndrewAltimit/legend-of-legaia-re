//! The slot-B cast-module band (`PROT 0903..0966`) as a **cast-effect pool**:
//! spell id -> module -> the module's spawn records.
//!
//! A cast in retail does not run its choreography out of the battle overlay.
//! The action SM pages one PROT entry of the band into the shared slot-B
//! window at `0x801F69D8` and re-enters it every frame, and two dispatchers in
//! PROT 0898 decide *which* entry - keyed differently, over the same 64-entry
//! band (see `docs/subsystems/cast-module.md`):
//!
//! | dispatcher | key | table | row `i` |
//! |---|---|---|---|
//! | `FUN_801F1ED4` | queued action id `actor[+0x1DF]`, `0x81..=0xA0` | `0x801CF4EC` | PROT `903 + (id - 0x81)` |
//! | `FUN_801F2160` | spell record's `+0x01` byte, `0x00..=0x1F` | `0x801CF56C` | PROT `935 + sub_id` |
//! | move-VM op `0x20` (the spawn stager) | either row above | `0x801F6734` | PROT `903 + i` |
//!
//! Both indexings land on the same band, so **one** pool serves both, keyed by
//! the PROT entry the dispatcher names.
//!
//! ## What this pool holds: the DATA half of a module
//!
//! `docs/subsystems/cast-module.md` classifies every routine PROT 0898 names in
//! this band as **DATA** (an arm switch whose arms only hand a module-resident
//! record pointer and a scale literal to `FUN_80050ED4` / `FUN_80021B04`) or
//! **PORT** (a routine that reads or writes simulation state - a damage roll,
//! the staged-clip bytes, the module phase `ctx+0x279`). This pool is the DATA
//! half: the spawn records those `jal` sites pass as `a2`.
//!
//! The records are byte-for-byte the shape the whole spawn stack shares -
//! `[i16 model_sel][u16 flags][move-VM bytecode]` - so recovering them needs no
//! new reader: [`crate::summon_overlay::parse`] already scans both spawn-call
//! forms and follows `a2`. This module is the band's *index* over that parser,
//! not a second copy of it.
//!
//! The module's **code** half - lift, camera, the `ctx+0x279` phase machine and
//! the damage shape - is not expressible as a record and is not here; a
//! consumer that stages this pool's records is staging a cast's particle layer,
//! not its choreography.
//!
//! Provenance: disassembly of `FUN_801F2160` (`0x801F2160..0x801F21D8`: caster
//! `actor_table[ctx+0x13]`, `lbu a0,0x1df`, record at `0x800754C8 + id*0xC`,
//! `lbu v1,0x1(v0)`, `sltiu v0,v1,0x20`, `jr` through `0x801CF56C`) - see
//! `ghidra/scripts/funcs/overlay_battle_action_0898_801f2160.txt` - and of the
//! single call site `jal 0x801f2160` at `0x801E50C8` in
//! `overlay_0898_801e295c.txt`, whose `bne v0,zero` is battle phase `0x70`'s
//! hold.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::summon_overlay::{SUMMON_OVERLAY_LINK_BASE, SummonPart};

/// First extraction PROT entry of the band (`FUN_801F1ED4` row 0 = action id
/// `0x81`).
pub const CAST_MODULE_PROT_FIRST: u32 = 903;

/// Last extraction PROT entry of the band (`FUN_801F2160` row `0x1F`).
pub const CAST_MODULE_PROT_LAST: u32 = 966;

/// Entries in the band: `0903..=0966`.
pub const CAST_MODULE_COUNT: usize = (CAST_MODULE_PROT_LAST - CAST_MODULE_PROT_FIRST) as usize + 1;

/// Lowest action id `FUN_801F1ED4` dispatches (`sltiu ..., 0x20` on
/// `id - 0x81`).
pub const SERU_ACTION_ID_MIN: u8 = 0x81;

/// Highest action id `FUN_801F1ED4` dispatches.
pub const SERU_ACTION_ID_MAX: u8 = 0xA0;

/// First extraction PROT entry `FUN_801F2160` reaches (`935 + 0`).
pub const CAPTURE_MODULE_PROT_FIRST: u32 = 935;

/// Slots in either dispatcher's jump table.
pub const CAST_TABLE_SLOTS: usize = 0x20;

/// Which of PROT 0898's two tick tables named a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastKey {
    /// `FUN_801F1ED4`, keyed on the queued action id `actor[+0x1DF]`.
    SeruAction(u8),
    /// `FUN_801F2160`, keyed on the spell record's `+0x01` byte.
    CaptureClass(u8),
}

impl CastKey {
    /// The band entry this key names, or `None` when the key falls outside its
    /// dispatcher's `sltiu ..., 0x20` bound.
    pub fn prot_entry(self) -> Option<u32> {
        match self {
            Self::SeruAction(id) => seru_module_prot(id),
            Self::CaptureClass(sub) => capture_module_prot(sub),
        }
    }
}

/// Extraction PROT entry `FUN_801F1ED4` dispatches action `id` to: row
/// `id - 0x81` of `0x801CF4EC`, i.e. `903 + (id - 0x81)`. `None` outside
/// `0x81..=0xA0` (retail forms `id - 0x81` unsigned and bounds it with
/// `sltiu ..., 0x20`, so a low id wraps and fails the same test).
///
/// Id `0x98` is included: its **tick** slot points at the shared epilogue and
/// ticks nothing, but its stager row in `0x801F6734` is a real entry, and this
/// function answers the band question, not the tick question. (PROT 0926, that
/// row's module, is the band's 1-sector null stub and carries no record.)
pub fn seru_module_prot(id: u8) -> Option<u32> {
    let row = id.wrapping_sub(SERU_ACTION_ID_MIN) as usize;
    (row < CAST_TABLE_SLOTS).then(|| CAST_MODULE_PROT_FIRST + row as u32)
}

/// Extraction PROT entry `FUN_801F2160` dispatches a capture-class cast to:
/// row `sub_id` of `0x801CF56C`, i.e. `935 + sub_id`. `None` at or above
/// `0x20` (retail's `sltiu v0,v1,0x20`).
///
/// `sub_id` is the **spell record's `+0x01` byte**
/// ([`crate::spell_names::SpellEntry::sub_class`]), and the same byte the
/// pager `FUN_8003EC70(record[+1] + 0x28)` resolves - two readers, one byte.
pub fn capture_module_prot(sub_id: u8) -> Option<u32> {
    ((sub_id as usize) < CAST_TABLE_SLOTS).then(|| CAPTURE_MODULE_PROT_FIRST + sub_id as u32)
}

/// `true` when `entry` is one of the 64 band entries.
pub fn is_cast_module_prot(entry: u32) -> bool {
    (CAST_MODULE_PROT_FIRST..=CAST_MODULE_PROT_LAST).contains(&entry)
}

/// One module's DATA layer: the spawn records its `FUN_80021B04` /
/// `FUN_80050ED4` sites pass, plus the image bytes their move-VM bytecode
/// lives in (a record's program is bounded by the next record, so the buffer
/// has to travel with the parts).
#[derive(Debug, Clone)]
pub struct CastModuleRecords {
    /// Extraction PROT entry of the module.
    pub prot_entry: u32,
    /// The image the records were parsed from, retained so a consumer can seed
    /// each part's move buffer.
    pub bytes: Arc<[u8]>,
    /// Spawn-call sites found in the image. Records recovered may be fewer: a
    /// site whose `a2` is loaded from a saved register the static window cannot
    /// see resolves to nothing.
    pub spawn_sites: usize,
    /// The recovered records, sorted by file offset.
    pub parts: Vec<SummonPart>,
}

impl CastModuleRecords {
    /// Parse one band image. `bytes` must be exactly one PROT entry
    /// (`legaia_prot::archive::Archive::read_entry`) - an over-read window
    /// resolves record pointers that belong to the *next* module's own load at
    /// the shared link base.
    pub fn parse(prot_entry: u32, bytes: &[u8]) -> Self {
        let overlay = crate::summon_overlay::parse(bytes, SUMMON_OVERLAY_LINK_BASE);
        Self {
            prot_entry,
            bytes: Arc::from(bytes),
            spawn_sites: overlay.spawn_sites,
            parts: overlay.parts,
        }
    }

    /// `true` when the image staged no record at all.
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// The band, indexed by PROT entry. Built once (the images are static disc
/// data) and read per cast.
#[derive(Debug, Clone, Default)]
pub struct CastEffectPool {
    modules: BTreeMap<u32, CastModuleRecords>,
}

impl CastEffectPool {
    /// An empty pool - what a host without a disc image holds.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse and insert one band entry. Entries outside `0903..=0966` are
    /// rejected (`false`), so a mis-numbered read cannot seed the pool with an
    /// unrelated image.
    pub fn insert(&mut self, prot_entry: u32, bytes: &[u8]) -> bool {
        if !is_cast_module_prot(prot_entry) {
            return false;
        }
        self.modules
            .insert(prot_entry, CastModuleRecords::parse(prot_entry, bytes));
        true
    }

    /// The module at `prot_entry`, if loaded.
    pub fn module(&self, prot_entry: u32) -> Option<&CastModuleRecords> {
        self.modules.get(&prot_entry)
    }

    /// The module a [`CastKey`] resolves to, if the key is in bounds and the
    /// module is loaded.
    pub fn resolve(&self, key: CastKey) -> Option<&CastModuleRecords> {
        self.module(key.prot_entry()?)
    }

    /// Modules loaded.
    pub fn len(&self) -> usize {
        self.modules.len()
    }

    /// `true` when nothing is loaded.
    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Every loaded entry, ascending.
    pub fn entries(&self) -> impl Iterator<Item = &CastModuleRecords> {
        self.modules.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn band_bounds() {
        assert_eq!(CAST_MODULE_COUNT, 64);
        assert!(is_cast_module_prot(903));
        assert!(is_cast_module_prot(966));
        assert!(!is_cast_module_prot(902));
        assert!(!is_cast_module_prot(967));
    }

    #[test]
    fn seru_rows_cover_the_action_id_span() {
        assert_eq!(seru_module_prot(0x81), Some(903));
        assert_eq!(seru_module_prot(SERU_ACTION_ID_MAX), Some(934));
        // Below the base wraps unsigned and fails the same bound retail's
        // `sltiu` applies.
        assert_eq!(seru_module_prot(0x80), None);
        assert_eq!(seru_module_prot(0xA1), None);
        // The two bands abut with no gap: the last seru row is one below the
        // first capture row.
        assert_eq!(
            seru_module_prot(SERU_ACTION_ID_MAX).unwrap() + 1,
            CAPTURE_MODULE_PROT_FIRST
        );
    }

    #[test]
    fn capture_rows_cover_the_sub_id_span() {
        assert_eq!(capture_module_prot(0), Some(935));
        assert_eq!(capture_module_prot(0x1F), Some(966));
        assert_eq!(capture_module_prot(0x20), None);
        assert_eq!(capture_module_prot(0xFF), None);
    }

    #[test]
    fn pool_rejects_entries_outside_the_band() {
        let mut pool = CastEffectPool::new();
        assert!(!pool.insert(898, &[0u8; 16]));
        assert!(pool.is_empty());
        assert!(pool.insert(958, &[0u8; 16]));
        assert_eq!(pool.len(), 1);
        assert!(pool.module(958).unwrap().is_empty());
    }

    #[test]
    fn resolve_follows_the_key() {
        let mut pool = CastEffectPool::new();
        pool.insert(903, &[0u8; 16]);
        pool.insert(935, &[0u8; 16]);
        assert_eq!(
            pool.resolve(CastKey::SeruAction(0x81))
                .map(|m| m.prot_entry),
            Some(903)
        );
        assert_eq!(
            pool.resolve(CastKey::CaptureClass(0)).map(|m| m.prot_entry),
            Some(935)
        );
        // In bounds but not loaded.
        assert!(pool.resolve(CastKey::SeruAction(0x82)).is_none());
        // Out of bounds.
        assert!(pool.resolve(CastKey::CaptureClass(0x20)).is_none());
    }
}
