//! Parser + classifier for GameShark / Mednafen cheat databases targeting
//! the NTSC-U build of *Legend of Legaia*. See [`crate`] for an overview.

#![deny(unsafe_code)]

pub mod classify;
pub mod gs_text;
pub mod mednafen_cht;

pub use classify::{
    BATTLE_ACTOR_BASE, BATTLE_ACTOR_STRIDE, CHAR_RECORD_BASES, Category, ClassifiedAddress,
    INVENTORY_BASE, INVENTORY_SLOTS, classify_address,
};
pub use gs_text::parse_gs_text;
pub use mednafen_cht::parse_mednafen_cht;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One write in a cheat code.
///
/// `addr` is masked to 24 bits (the PSX KSEG0 RAM range
/// `0x80000000..0x80200000` masks the high byte off; conditional codes
/// also live in this space).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CheatCode {
    /// Operation kind (write, conditional, …).
    pub op: CheatOp,
    /// Target PSX RAM address. Always normalised to the `0x80xxxxxx`
    /// canonical form regardless of the encoded prefix byte.
    pub addr: u32,
    /// Operand. For 8-bit writes, only the low byte is meaningful;
    /// for 16-bit writes, the whole `u16`.
    pub value: u16,
    /// Width of this write in bytes (1 or 2). Conditional codes are
    /// always 16-bit comparisons.
    pub width: u8,
}

/// What a single GameShark code line does.
///
/// The encoding is:
///
/// ```text
/// 30xxxxxx 00YY  =>  Write::U8           ; mem[80xxxxxx] = YY
/// 80xxxxxx YYYY  =>  Write::U16          ; mem[80xxxxxx] = YYYY (LE)
/// D0xxxxxx YYYY  =>  IfEqU16             ; if mem16[80xxxxxx] == YYYY then next code
/// E0xxxxxx YYYY  =>  IfNotEqU16          ; if mem16[80xxxxxx] != YYYY then next code
/// ```
///
/// Other prefixes exist in the wild (`10`, `50`, `C0`, `F0`) but the
/// Legaia corpus we ship under `data/cheats/` doesn't use them
/// productively, so they fall through as [`CheatOp::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CheatOp {
    /// 8-bit write (`30xxxxxx 00YY`).
    WriteU8,
    /// 16-bit write (`80xxxxxx YYYY`).
    WriteU16,
    /// Conditional: execute the next code only if the 16-bit memory
    /// at `addr` equals `value` (`D0xxxxxx YYYY`).
    IfEqU16,
    /// Conditional: execute the next code only if the 16-bit memory
    /// at `addr` does NOT equal `value` (`E0xxxxxx YYYY`).
    IfNotEqU16,
    /// Anything else - kept verbatim so round-trips don't lose data.
    Unknown {
        /// Raw prefix byte that wasn't recognised.
        prefix: u8,
    },
}

impl CheatCode {
    /// Decode the standard GameShark `[8-hex-digit-address] [4-hex-digit-value]`
    /// pair. The high byte of the address picks the [`CheatOp`].
    pub fn from_packed(addr_hex: u32, value: u16) -> Self {
        let prefix = (addr_hex >> 24) as u8;
        let addr = 0x80000000 | (addr_hex & 0x00FF_FFFF);
        let (op, width) = match prefix {
            0x30 => (CheatOp::WriteU8, 1),
            0x80 => (CheatOp::WriteU16, 2),
            0xD0 => (CheatOp::IfEqU16, 2),
            0xE0 => (CheatOp::IfNotEqU16, 2),
            other => (CheatOp::Unknown { prefix: other }, 0),
        };
        Self {
            op,
            addr,
            value,
            width,
        }
    }

    /// True if this code unconditionally writes to RAM (i.e. it's a
    /// `WriteU8` or `WriteU16`).
    pub fn is_write(&self) -> bool {
        matches!(self.op, CheatOp::WriteU8 | CheatOp::WriteU16)
    }

    /// True if this code is a conditional gate (`D0` / `E0`).
    pub fn is_conditional(&self) -> bool {
        matches!(self.op, CheatOp::IfEqU16 | CheatOp::IfNotEqU16)
    }
}

/// One named cheat effect. Holds an ordered list of [`CheatCode`]s
/// (multi-write effects keep their order; conditionals gate the
/// following code).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheatEntry {
    /// Human-written description (e.g. "Max HP (Vahn)").
    pub description: String,
    /// One or more codes that compose this effect.
    pub codes: Vec<CheatCode>,
}

impl CheatEntry {
    /// Iterate over the unconditional writes in this entry, dropping
    /// conditional gates. The applier resolves conditionals separately.
    pub fn writes(&self) -> impl Iterator<Item = &CheatCode> {
        self.codes.iter().filter(|c| c.is_write())
    }

    /// All addresses this entry touches (writes + conditional reads).
    pub fn addresses(&self) -> impl Iterator<Item = u32> + '_ {
        self.codes.iter().map(|c| c.addr)
    }
}

/// A whole parsed cheat file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Database {
    /// Entries in source order.
    pub entries: Vec<CheatEntry>,
}

impl Database {
    /// Build an empty database.
    pub fn new() -> Self {
        Self::default()
    }

    /// Total count of (entry, write) pairs across the database.
    pub fn write_count(&self) -> usize {
        self.entries.iter().map(|e| e.writes().count()).sum()
    }

    /// Group entries by their first write address, producing an
    /// `(addr -> Vec<&CheatEntry>)` map. Stable address order.
    pub fn group_by_first_address(&self) -> BTreeMap<u32, Vec<&CheatEntry>> {
        let mut out: BTreeMap<u32, Vec<&CheatEntry>> = BTreeMap::new();
        for e in &self.entries {
            if let Some(addr) = e.codes.first().map(|c| c.addr) {
                out.entry(addr).or_default().push(e);
            }
        }
        out
    }

    /// All distinct write addresses across the database, sorted ascending.
    pub fn distinct_write_addresses(&self) -> Vec<u32> {
        let mut s: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        for e in &self.entries {
            for w in e.writes() {
                s.insert(w.addr);
            }
        }
        s.into_iter().collect()
    }

    /// Drop entries whose description is identical to one already
    /// present **and** whose code list is identical. Returns the count
    /// of removed entries. Used to collapse the GameShark-format
    /// "Have 99 Items" duplicate sprawl.
    pub fn dedupe_identical(&mut self) -> usize {
        let mut seen: std::collections::HashSet<(String, Vec<CheatCode>)> =
            std::collections::HashSet::new();
        let before = self.entries.len();
        self.entries.retain(|e| {
            let key = (e.description.clone(), e.codes.clone());
            seen.insert(key)
        });
        before - self.entries.len()
    }

    /// Group entries by [`Category`] for the classify CLI.
    pub fn classify(&self) -> BTreeMap<Category, Vec<&CheatEntry>> {
        let mut out: BTreeMap<Category, Vec<&CheatEntry>> = BTreeMap::new();
        for e in &self.entries {
            let cat = e
                .codes
                .iter()
                .find(|c| c.is_write())
                .map(|c| classify_address(c.addr).category)
                .unwrap_or(Category::Unknown);
            out.entry(cat).or_default().push(e);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_packed_decodes_write_u8() {
        let c = CheatCode::from_packed(0x300848A3, 0x0042);
        assert_eq!(c.op, CheatOp::WriteU8);
        assert_eq!(c.addr, 0x800848A3);
        assert_eq!(c.value, 0x0042);
        assert_eq!(c.width, 1);
        assert!(c.is_write());
    }

    #[test]
    fn from_packed_decodes_write_u16() {
        let c = CheatCode::from_packed(0x80084708, 0xFFFF);
        assert_eq!(c.op, CheatOp::WriteU16);
        assert_eq!(c.addr, 0x80084708);
        assert_eq!(c.value, 0xFFFF);
        assert_eq!(c.width, 2);
    }

    #[test]
    fn from_packed_decodes_conditionals() {
        let eq = CheatCode::from_packed(0xD007B7C0, 0x0100);
        assert_eq!(eq.op, CheatOp::IfEqU16);
        assert!(eq.is_conditional());
        let neq = CheatCode::from_packed(0xE007B83C, 0x0003);
        assert_eq!(neq.op, CheatOp::IfNotEqU16);
        assert!(neq.is_conditional());
    }

    #[test]
    fn from_packed_keeps_unknown_prefixes() {
        let c = CheatCode::from_packed(0x10000000, 0x0001);
        assert!(matches!(c.op, CheatOp::Unknown { prefix: 0x10 }));
        assert!(!c.is_write());
        assert!(!c.is_conditional());
    }

    #[test]
    fn dedupe_collapses_identical_entries() {
        let mut db = Database::new();
        let entry = CheatEntry {
            description: "Have 99 Items".into(),
            codes: vec![CheatCode::from_packed(0x30085959, 0x0063)],
        };
        db.entries.push(entry.clone());
        db.entries.push(entry.clone());
        db.entries.push(entry);
        assert_eq!(db.entries.len(), 3);
        let removed = db.dedupe_identical();
        assert_eq!(removed, 2);
        assert_eq!(db.entries.len(), 1);
    }
}
