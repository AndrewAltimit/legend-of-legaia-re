//! Item-name table parser (`PTR_DAT_8007436C` in `SCUS_942.54`).
//!
//! This is the static table the MES interpreter's `0xC2` / `0xC4` substitution
//! codes read to print an item name on screen (see `docs/formats/mes.md`). It
//! is the executable's ground-truth name for every item id - weapons, armor,
//! accessories, consumables and key items all share one 256-entry id space.
//! The same id space is what a monster record's `drop_item` byte
//! ([`crate::monster_archive`]) indexes, so this table is how a raw drop id
//! becomes a readable name (e.g. `0x79` -> `Healing Berry`).
//!
//! ## Record layout (12 bytes, stride `0xC`)
//!
//! The MES dispatch indexes the table as a `u32` array `PTR_DAT_8007436C[id*3]`,
//! i.e. three words per id. The first word is the name pointer; the rest carry
//! per-item metadata (price / type byte) the name decode doesn't need.
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u32 | `name_ptr` - pointer to the NUL-terminated display name |
//! | `+4` | u32 | secondary pointer (shared "type" string for some classes) |
//! | `+8` | u32 | packed price / id / type metadata |
//!
//! Ids run `0x00..=0xFF`; the pointers leave the data segment past `0xFF`,
//! which is how [`ItemNameTable::from_scus`] finds the table's extent. A
//! handful of ids (`0x00`, `0x12`, `0x1A`, `0x52`, `0xB9`, `0xFD`) have empty
//! name strings (reserved / gap slots) and decode to `None`.
//!
//! The display strings carry the same MES control prefixes as every other
//! in-game string (a leading `0x01` icon escape, `0xCE XX` colour controls);
//! [`ItemNameTable::from_scus`] strips them, keeping the printable ASCII.

/// RAM address of the item-name pointer table (`PTR_DAT_8007436C`).
pub const TABLE_VA: u32 = 0x8007_436C;
/// Per-id stride in bytes (three `u32` words).
pub const RECORD_STRIDE: usize = 0x0C;
/// Number of item ids the table covers (`0x00..=0xFF`).
pub const ITEM_COUNT: usize = 256;

/// PSX-EXE `t_addr` -> file-offset resolver. `SCUS_942.54` loads its data
/// segment at `t_addr` from file offset `0x800`. (Same shape as the resolver
/// in `legaia_art::arts_table`; kept local so this crate has no art dep.)
struct ExeMap {
    t_addr: u32,
    t_size: u32,
}

impl ExeMap {
    fn parse(scus: &[u8]) -> Option<Self> {
        if scus.len() < 0x800 || &scus[0..8] != b"PS-X EXE" {
            return None;
        }
        let t_addr = u32::from_le_bytes(scus[0x18..0x1C].try_into().ok()?);
        let t_size = u32::from_le_bytes(scus[0x1C..0x20].try_into().ok()?);
        Some(Self { t_addr, t_size })
    }

    /// File offset for a virtual address, or `None` if outside the data
    /// segment.
    fn off(&self, va: u32) -> Option<usize> {
        if va < self.t_addr || va >= self.t_addr.checked_add(self.t_size)? {
            return None;
        }
        Some((va - self.t_addr) as usize + 0x800)
    }
}

/// Read an item name string at `va`, stripping MES control prefixes (`0xCE XX`
/// colour controls, the leading `0x01` icon escape, any other control byte)
/// and trimming surrounding whitespace. Returns `None` if the pointer is out
/// of range or the decoded name is empty.
fn read_name(scus: &[u8], map: &ExeMap, va: u32) -> Option<String> {
    let start = map.off(va)?;
    let mut out = String::new();
    let mut i = start;
    while i < scus.len() {
        let b = scus[i];
        if b == 0 {
            break;
        }
        if b == 0xCE {
            // 0xCE + control byte (+ an optional trailing space).
            i += 2;
            if scus.get(i) == Some(&0x20) {
                i += 1;
            }
            continue;
        }
        if (0x20..0x7F).contains(&b) {
            out.push(b as char);
        }
        i += 1;
    }
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// File offset of the `name_ptr` word for item `id` within a `SCUS_942.54`
/// image, plus its current value. `None` if `scus` isn't a PS-X EXE or the slot
/// falls outside the loaded data segment.
///
/// This is the word a *name-injection* patch repoints: the unnamed accessory
/// (`0xFD`) ships pointing at the shared empty-string slot, so repointing only
/// its word gives it a name without touching the other ids that share that
/// slot. The string itself goes in reclaimable space found by
/// [`data_segment_free_tail`].
pub fn name_ptr_slot(scus: &[u8], id: u8) -> Option<(usize, u32)> {
    let map = ExeMap::parse(scus)?;
    let rec = map.off(TABLE_VA + (id as u32) * RECORD_STRIDE as u32)?;
    let val = u32::from_le_bytes(scus.get(rec..rec + 4)?.try_into().ok()?);
    Some((rec, val))
}

/// Real base of the item record table. [`TABLE_VA`] is the *name pointer* field,
/// which sits at `+4` of each record; the record itself starts 4 bytes earlier.
/// The record layout is `[+0 u8 kind][+1 u8][+2 u16 shop_price][+4 u32 name_ptr]
/// [+8 u32 type_ptr]`. The shop buy/sell UI reads the **u16 at `+2`** as the
/// price (verified against a live shop: e.g. War God Band = 21000), and a price
/// of `0` marks a quest / found-only item the shop never prices.
pub const TABLE_BASE_VA: u32 = TABLE_VA - 4;

/// File offset of the `u16` shop-price field for item `id`, plus its current
/// value (`price` at record `+2`, i.e. VA `TABLE_BASE_VA + id*0xC + 2`). `None`
/// if `scus` isn't a PS-X EXE or the slot is out of range. This is the field a
/// price patch rewrites to give a normally-free (chest-found) equipment a shop
/// value; see `legaia_rando::item_price`.
pub fn price_slot(scus: &[u8], id: u8) -> Option<(usize, u16)> {
    let map = ExeMap::parse(scus)?;
    let va = TABLE_BASE_VA + (id as u32) * RECORD_STRIDE as u32 + 2;
    let o = map.off(va)?;
    let val = u16::from_le_bytes(scus.get(o..o + 2)?.try_into().ok()?);
    Some((o, val))
}

/// The `u16` shop price for item `id` (`0` = quest / found-only / not priced).
pub fn item_price(scus: &[u8], id: u8) -> Option<u16> {
    price_slot(scus, id).map(|(_, v)| v)
}

/// File offset for a virtual address within the loaded data segment, or `None`
/// if `scus` isn't a PS-X EXE or `va` falls outside the segment. The inverse of
/// the table's pointer math, exposed so a name-injection patch can resolve where
/// to write a string it stashes at a known-constant VA.
///
/// NB: do **not** stash a string in the *trailing* zero-fill of the data
/// segment - that span is zero in the file but is `.sbss`/`.bss`-class scratch
/// the game overwrites with variables at runtime (a string put there renders as
/// changing garbage). A safe target is a region verified constant across diverse
/// runtime states; see `legaia_rando::item_name`.
pub fn file_offset_for_va(scus: &[u8], va: u32) -> Option<usize> {
    ExeMap::parse(scus)?.off(va)
}

/// The decoded item-name table: one entry per item id (`0x00..=0xFF`). Empty /
/// reserved slots are `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ItemNameTable {
    names: Vec<Option<String>>,
}

impl ItemNameTable {
    /// Parse the item-name table out of a `SCUS_942.54` image. Returns `None`
    /// if the image isn't a PSX-EXE or the table address is out of range.
    pub fn from_scus(scus: &[u8]) -> Option<Self> {
        let map = ExeMap::parse(scus)?;
        let mut names = Vec::with_capacity(ITEM_COUNT);
        for id in 0..ITEM_COUNT {
            let rec = map.off(TABLE_VA + (id * RECORD_STRIDE) as u32)?;
            let name_ptr = u32::from_le_bytes(scus.get(rec..rec + 4)?.try_into().ok()?);
            names.push(read_name(scus, &map, name_ptr));
        }
        Some(Self { names })
    }

    /// Build directly from a name list (tests / non-SCUS callers).
    pub fn from_names(names: Vec<Option<String>>) -> Self {
        Self { names }
    }

    /// Display name for item `id`, or `None` for a reserved / empty slot (and
    /// for `id == 0`, which the game uses as "no item").
    pub fn name(&self, id: u8) -> Option<&str> {
        self.names.get(id as usize)?.as_deref()
    }

    /// Number of id slots the table covers.
    pub fn len(&self) -> usize {
        self.names.len()
    }

    /// `true` when the table holds no slots.
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    /// Count of slots that resolved to a non-empty name.
    pub fn named_count(&self) -> usize {
        self.names.iter().filter(|n| n.is_some()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal PSX-EXE image whose item table holds the given name
    /// strings, so the parser can be exercised without any Sony bytes.
    fn synth_scus(names: &[&str]) -> Vec<u8> {
        const T_ADDR: u32 = 0x8001_0000;
        // Lay strings out in a string pool after the table; the table itself
        // sits at TABLE_VA. Compute the file big enough to cover both.
        let table_off = (TABLE_VA - T_ADDR) as usize + 0x800;
        let table_bytes = ITEM_COUNT * RECORD_STRIDE;
        let pool_va = TABLE_VA + table_bytes as u32;
        let pool_off = (pool_va - T_ADDR) as usize + 0x800;

        // First pass: place each string in the pool, record its VA.
        let mut pool = Vec::new();
        let mut str_va = Vec::new();
        for s in names {
            str_va.push(pool_va + pool.len() as u32);
            pool.extend_from_slice(s.as_bytes());
            pool.push(0);
        }

        // Trailing zero padding stands in for the real executable's
        // data-segment zero-fill tail (where a name injection is stashed).
        let total = pool_off + pool.len() + 0x40;
        let mut buf = vec![0u8; total];
        buf[0..8].copy_from_slice(b"PS-X EXE");
        buf[0x18..0x1C].copy_from_slice(&T_ADDR.to_le_bytes());
        // t_size must cover everything past the load address.
        let t_size = (total - 0x800) as u32;
        buf[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());

        // Write the pointer table: word 0 = name_ptr (0 for unfilled slots).
        for (id, va) in str_va.iter().enumerate() {
            let rec = table_off + id * RECORD_STRIDE;
            buf[rec..rec + 4].copy_from_slice(&va.to_le_bytes());
        }
        buf[pool_off..pool_off + pool.len()].copy_from_slice(&pool);
        buf
    }

    #[test]
    fn parses_names_and_handles_gaps() {
        // id 0 empty ("no item"), id 1/2 named, id 3 empty.
        let scus = synth_scus(&["", "Healing Berry", "Survival Knife", ""]);
        let table = ItemNameTable::from_scus(&scus).expect("parse");
        assert_eq!(table.len(), ITEM_COUNT);
        assert_eq!(table.name(0), None);
        assert_eq!(table.name(1), Some("Healing Berry"));
        assert_eq!(table.name(2), Some("Survival Knife"));
        assert_eq!(table.name(3), None);
        assert_eq!(table.named_count(), 2);
    }

    #[test]
    fn strips_control_prefixes_and_trims() {
        // A leading 0x01 icon escape + trailing space, like several retail rows:
        // the reader must drop the control byte and trim the surrounding space.
        let scus = synth_scus(&["", "\u{1}Mace "]);
        let table = ItemNameTable::from_scus(&scus).unwrap();
        assert_eq!(table.name(1), Some("Mace"));
    }

    #[test]
    fn non_psx_exe_returns_none() {
        assert!(ItemNameTable::from_scus(b"not an exe").is_none());
        assert!(ItemNameTable::from_scus(&[0u8; 0x900]).is_none());
    }

    #[test]
    fn from_names_round_trips() {
        let t = ItemNameTable::from_names(vec![None, Some("X".into())]);
        assert_eq!(t.name(0), None);
        assert_eq!(t.name(1), Some("X"));
        assert_eq!(t.name(2), None);
    }

    #[test]
    fn name_ptr_slot_locates_the_word_and_reads_its_value() {
        // id 1 points at a string; id 3 is empty (name_ptr == 0 in the synth).
        let scus = synth_scus(&["", "Healing Berry", "", ""]);
        let (off1, ptr1) = name_ptr_slot(&scus, 1).expect("slot 1");
        // The slot offset is table_off + id*stride and holds the string VA.
        let table_off = (TABLE_VA - 0x8001_0000) as usize + 0x800;
        assert_eq!(off1, table_off + RECORD_STRIDE);
        // Reading the name at that pointer reproduces the table value.
        assert_eq!(
            read_name(&scus, &ExeMap::parse(&scus).unwrap(), ptr1).as_deref(),
            Some("Healing Berry")
        );
        // A non-EXE input yields None.
        assert!(name_ptr_slot(b"nope", 1).is_none());
    }

    #[test]
    fn file_offset_for_va_inverts_the_load_map() {
        let scus = synth_scus(&["", "Sword"]);
        // The table VA maps back to its file offset (table_off).
        let table_off = (TABLE_VA - 0x8001_0000) as usize + 0x800;
        assert_eq!(file_offset_for_va(&scus, TABLE_VA), Some(table_off));
        // A VA below the load address / past the segment is rejected.
        assert!(file_offset_for_va(&scus, 0x8000_0000).is_none());
        assert!(file_offset_for_va(&scus, 0x8FFF_FFFF).is_none());
        assert!(file_offset_for_va(b"nope", TABLE_VA).is_none());
    }
}
