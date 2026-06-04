//! Town-merchant shop randomizer: reassign what each town store sells.
//!
//! ## Where a town shop lives
//!
//! Unlike the casino prize exchange (a static overlay table — see
//! [`crate::casino`]), a **gold town merchant's** stock is defined **inline in
//! the scene's field-VM script** (the MAN), the same place chests
//! ([op `0x39`](crate::chest)) and doors (op `0x3F`) live. Opening a shop is
//! field-VM **op `0x49` (`STATE_RESUME`)** — the multi-frame state machine that
//! drives the menu-request register `_DAT_8007B450` (the same register the
//! "Shop Modifier" cheat pokes). In its sub-op-`0` form it carries an inline
//! payload that, for a shop, is:
//!
//! ```text
//! 0x49 0x00 <length> <length args…>  [u8 count][count× u8 item_id][ASCII name\0]
//! ```
//!
//! (For an observed shop, `length == 0`, so the record starts at op `+3`.) The
//! item ids index the shared 256-entry item table; the name is the on-screen
//! shop title ("Variety Store", "Weapon Shop", …). After the record comes the
//! shop's `0x1F` dialogue ("Welcome!", "Thank you!"). Pinned from a live
//! PCSX-Redux capture standing in the Rim Elm Variety Store (its 10 ids match
//! the curated [shops table](../../docs/reference/gamedata.md)).
//!
//! ## Locating sites safely
//!
//! A raw `0x49` byte-scan would hit operand / dialogue bytes, so sites are found
//! by an **opcode-aware walk** of each MAN record's interaction script with the
//! Track-1 field-VM disassembler ([`legaia_asset::field_disasm`]) — identical to
//! the chest walk, including skipping `0x1F` dialogue segments (a shop's
//! "Welcome!" text precedes its `0x49`). Reaching op `0x49` *in real script
//! flow* is what distinguishes a shop record from coincidental bytes; the record
//! is then validated structurally (a small item count, every id non-zero, a
//! printable name terminated by `0x00`) so non-shop `0x49` sub-0 uses (inn /
//! save prompts, whose payload isn't an item list) are rejected.
//!
//! ## Randomization
//!
//! Only the `count` item-id bytes are rewritten — the count, name, price logic
//! (prices are looked up per item elsewhere) and surrounding script are
//! untouched, so the edit is same-size and the MAN recompresses + writes back
//! exactly like the [encounter](crate::encounter) / [chest](crate::chest) paths.
//! Global shuffle / random across all towns is orchestrated in [`crate::apply`].

use legaia_asset::field_disasm;
use legaia_asset::{man_section, scene_asset_table};

const MAN_TYPE: u8 = 0x03;
/// Field-VM opcode that opens a shop (`STATE_RESUME`).
const SHOP_OPCODE: u8 = 0x49;
/// Max item count a shop record is allowed to declare (a sanity bound that
/// rejects a non-shop `0x49` payload whose first byte happens to be large).
const MAX_SHOP_ITEMS: usize = 16;

/// One town-shop site located in a scene MAN: its declared item count, the
/// absolute offsets (within [`SceneShops::decoded`]) of each item-id byte, and
/// the shop's display name.
#[derive(Debug, Clone)]
pub struct ShopSite {
    /// Absolute offset of the `count` byte within `decoded`.
    pub count_off: usize,
    /// Absolute offsets of each item-id byte (length == count).
    pub id_offsets: Vec<usize>,
    /// The shop's on-screen name (decoded ASCII), for listings / audit.
    pub name: String,
}

/// A scene bundle's MAN located in a PROT entry, with its town-shop sites.
pub struct SceneShops {
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within.
    pub compressed_budget: usize,
    /// Decompressed MAN (mutate the item-id bytes in place, then [`Self::repack`]).
    pub decoded: Vec<u8>,
    /// The shop sites found in this scene.
    pub shops: Vec<ShopSite>,
}

impl SceneShops {
    /// Locate a scene bundle's MAN and its town-shop sites, or `None` if the
    /// entry isn't a scene bundle, has no MAN, or has no shop.
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        let table = scene_asset_table::detect(entry)?;
        let man = table
            .used()
            .iter()
            .find(|d| d.type_byte == MAN_TYPE)
            .copied()?;
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        let man_offset = man.data_offset as usize;
        let body = entry.get(man_offset..)?;
        let (decoded, consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        let shops = shop_sites(&decoded);
        if shops.is_empty() {
            return None;
        }
        Some(Self {
            entry_idx,
            man_offset,
            compressed_budget: consumed,
            decoded,
            shops,
        })
    }

    /// Flat list of every item-id byte offset across all this scene's shops,
    /// in shop-then-slot order — the population a randomizer rewrites.
    pub fn id_offsets(&self) -> Vec<usize> {
        self.shops
            .iter()
            .flat_map(|s| s.id_offsets.iter().copied())
            .collect()
    }

    /// The current item id at each offset returned by [`Self::id_offsets`].
    pub fn current_items(&self) -> Vec<u8> {
        self.id_offsets().iter().map(|&o| self.decoded[o]).collect()
    }

    /// Set the item id at decoded offset `off` (one of [`Self::id_offsets`]).
    pub fn set_id(&mut self, off: usize, new_id: u8) {
        if let Some(b) = self.decoded.get_mut(off) {
            *b = new_id;
        }
    }

    /// Recompress the (mutated) MAN; `None` if it would overflow the footprint.
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

/// Walk a decompressed MAN's record scripts and return every town-shop site
/// (op `0x49` sub-op `0` whose inline payload validates as `[count][ids][name]`).
pub fn shop_sites(man: &[u8]) -> Vec<ShopSite> {
    let Ok(mf) = man_section::parse(man) else {
        return Vec::new();
    };
    // Record-start bounds across all partitions, to bound each walk to its own
    // record (mirrors the chest walk).
    let mut bounds: Vec<usize> = Vec::new();
    for part in &mf.partitions {
        for ri in 0..part.len() {
            if let Some(o) = mf.actor_placement_record_offset(ri, man.len()) {
                bounds.push(o);
            }
        }
    }
    bounds.sort_unstable();
    bounds.dedup();

    let mut out: Vec<ShopSite> = Vec::new();
    let mut seen_count_off: Vec<usize> = Vec::new();
    for part in &mf.partitions {
        for ri in 0..part.len() {
            let Some(rec) = mf.actor_placement_record_offset(ri, man.len()) else {
                continue;
            };
            let Some(&n) = man.get(rec) else { continue };
            let pc0 = 1 + n as usize * 2 + 4;
            if rec + pc0 >= man.len() {
                continue;
            }
            let end = bounds
                .iter()
                .copied()
                .find(|&o| o > rec)
                .unwrap_or(man.len());
            for site in walk_record_shops(man, rec, pc0, end) {
                // Dedup by count offset (a record reachable from two partitions).
                if !seen_count_off.contains(&site.count_off) {
                    seen_count_off.push(site.count_off);
                    out.push(site);
                }
            }
        }
    }
    out.sort_by_key(|s| s.count_off);
    out
}

/// Walk one record's script from `pc0` to `end` (relative to `rec`), returning
/// the shop sites reached. Skips `0x1F` dialogue like the chest walk; any other
/// decode error stops the walk.
fn walk_record_shops(man: &[u8], rec: usize, pc0: usize, end: usize) -> Vec<ShopSite> {
    let script = &man[rec..end.min(man.len())];
    let mut found = Vec::new();
    let mut pc = pc0;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 100_000 || pc >= script.len() {
            break;
        }
        match field_disasm::decode(script, pc) {
            Ok(insn) => {
                if insn.size == 0 {
                    break;
                }
                if insn.opcode == SHOP_OPCODE
                    && insn.extended.is_none()
                    && let Some(site) = parse_shop_record(man, rec + pc)
                {
                    found.push(site);
                }
                pc += insn.size;
            }
            Err(_) if script.get(pc) == Some(&0x1F) => {
                pc = skip_dialogue_segment(script, pc);
            }
            Err(_) => break,
        }
    }
    found
}

/// Parse + validate a shop record at the op-`0x49` byte `op_abs` (absolute in
/// `man`). The sub-op-`0` layout is `0x49 0x00 <length> <length args> [count]
/// [ids] [name\0]`; returns `None` unless the payload validates as a shop
/// (small non-zero count, all ids non-zero, a printable name terminated by
/// `0x00`).
fn parse_shop_record(man: &[u8], op_abs: usize) -> Option<ShopSite> {
    // sub_op at +1; only sub-op 0 carries the inline MES-shape payload.
    if *man.get(op_abs + 1)? != 0 {
        return None;
    }
    let length = *man.get(op_abs + 2)? as usize;
    let count_off = op_abs + 3 + length;
    let count = *man.get(count_off)? as usize;
    if count == 0 || count > MAX_SHOP_ITEMS {
        return None;
    }
    let ids_start = count_off + 1;
    let ids_end = ids_start + count;
    let ids = man.get(ids_start..ids_end)?;
    if ids.contains(&0) {
        return None;
    }
    // Name: a printable ASCII run terminated by 0x00, first char a letter.
    let name = read_shop_name(man, ids_end)?;
    Some(ShopSite {
        count_off,
        id_offsets: (ids_start..ids_end).collect(),
        name,
    })
}

/// Read a shop name string at `start`: printable ASCII (2..=18 chars), first
/// char alphabetic, terminated by `0x00`. `None` if it isn't shop-name-shaped
/// (this is the key rejector for non-shop `0x49` sub-0 payloads).
fn read_shop_name(man: &[u8], start: usize) -> Option<String> {
    let mut s = String::new();
    let mut i = start;
    loop {
        let &b = man.get(i)?;
        if b == 0 {
            break;
        }
        if !(0x20..0x7F).contains(&b) {
            return None;
        }
        s.push(b as char);
        i += 1;
        if s.len() > 18 {
            return None;
        }
    }
    let first_alpha = s.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    (s.len() >= 2 && first_alpha).then_some(s)
}

/// Skip a `0x1F` inline-dialogue segment beginning at `pc`, returning the offset
/// just past its terminating `0x00` (`0xC?` bytes are 2-byte escapes).
fn skip_dialogue_segment(script: &[u8], mut pc: usize) -> usize {
    pc += 1;
    while pc < script.len() {
        let b = script[pc];
        if b == 0 {
            return pc + 1;
        }
        if b & 0xF0 == 0xC0 {
            pc += 2;
        } else {
            pc += 1;
        }
    }
    pc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tiny script buffer with one op-0x49 shop record and parse it.
    #[test]
    fn parse_shop_record_reads_ids_and_name() {
        // 0x49 0x00 0x00 [count=3] 0x22 0x34 0x59 "Shop\0"
        let mut man = vec![0u8; 4];
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x03, 0x22, 0x34, 0x59]);
        man.extend_from_slice(b"Shop\0");
        let site = parse_shop_record(&man, 4).expect("valid shop record");
        assert_eq!(site.name, "Shop");
        assert_eq!(site.id_offsets, vec![4 + 4, 4 + 5, 4 + 6]);
        assert_eq!(
            site.id_offsets.iter().map(|&o| man[o]).collect::<Vec<_>>(),
            vec![0x22, 0x34, 0x59]
        );
    }

    #[test]
    fn rejects_non_shop_0x49_payloads() {
        // sub-op != 0 -> not the inline form.
        let mut m = vec![0x49, 0x01, 0x00, 0x03, 0x22, 0x34, 0x59];
        m.extend_from_slice(b"Shop\0");
        assert!(parse_shop_record(&m, 0).is_none());

        // count 0 -> rejected.
        let m = vec![0x49, 0x00, 0x00, 0x00, b'X', b'Y', 0x00];
        assert!(parse_shop_record(&m, 0).is_none());

        // An id byte is 0 -> rejected.
        let mut m = vec![0x49, 0x00, 0x00, 0x02, 0x22, 0x00];
        m.extend_from_slice(b"Shop\0");
        assert!(parse_shop_record(&m, 0).is_none());

        // Name not name-shaped (starts with a digit) -> rejected.
        let mut m = vec![0x49, 0x00, 0x00, 0x01, 0x22];
        m.extend_from_slice(b"3X\0");
        assert!(parse_shop_record(&m, 0).is_none());

        // Name not terminated / not printable -> rejected.
        let m = vec![0x49, 0x00, 0x00, 0x01, 0x22, 0x1F, 0x40];
        assert!(parse_shop_record(&m, 0).is_none());
    }

    #[test]
    fn honours_length_arg_offset() {
        // length=2 shifts the record start by 2 (op+3+2 = op+5).
        let mut man = vec![0x49, 0x00, 0x02, 0xAA, 0xBB, 0x02, 0x77, 0x7e];
        man.extend_from_slice(b"Item\0");
        let site = parse_shop_record(&man, 0).expect("length-shifted record");
        assert_eq!(site.id_offsets, vec![6, 7]);
        assert_eq!(site.name, "Item");
    }
}
