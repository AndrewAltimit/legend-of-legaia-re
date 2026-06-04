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
//! Sites are found by **scanning** the decompressed MAN for the op-`0x49`
//! sub-op-`0` shop signature, *not* by an opcode walk. A shop's `0x49` is often
//! gated behind a dialogue confirm-picker ("Buy them?") whose option-jump table
//! desyncs a linear disassembler before it reaches the op (Biron Monastery's
//! Corey vendor is the case that exposed this — its op is reached only past a
//! Yes/No picker), so a walk silently misses those shops. The scan doesn't care
//! how the script reaches the op. False positives are ruled out by a strict
//! record validation ([`parse_shop_record`]): the byte after the opcode must be
//! `0x00` (sub-op 0 — this alone rejects almost every stray `0x49`, since ids,
//! operands and `0x49`-lead names like "Items Shop" are followed by non-zero),
//! the count is small and non-zero, every id is non-zero (and, with the SCUS
//! mask the apply layer supplies, names a real item), and the trailing shop name
//! is a printable, letter-initial, `0x00`-terminated string. Non-shop `0x49`
//! sub-0 uses (inn / save prompts, whose payload is MES text not an item list)
//! fail those checks.
//!
//! ## Randomization
//!
//! Only the `count` item-id bytes are rewritten — the count, name, price logic
//! (prices are looked up per item elsewhere) and surrounding script are
//! untouched, so the edit is same-size and the MAN recompresses + writes back
//! exactly like the [encounter](crate::encounter) / [chest](crate::chest) paths.
//! Global shuffle / random across all towns is orchestrated in [`crate::apply`].

use legaia_asset::scene_asset_table;

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
    /// entry isn't a scene bundle, has no MAN, or has no shop. Structural-only
    /// validation (no item-name check); prefer [`Self::locate_with_items`] from a
    /// caller that has the SCUS item table.
    pub fn locate(entry: &[u8], entry_idx: usize) -> Option<Self> {
        Self::locate_inner(entry, entry_idx, None)
    }

    /// Like [`Self::locate`], but `valid` restricts shop ids to **named items**
    /// (a `256`-entry "id names a real item" mask from the SCUS item table), so a
    /// stray `0x49`-prefixed byte run can't be mistaken for a shop. The apply
    /// layer always uses this form.
    pub fn locate_with_items(entry: &[u8], entry_idx: usize, valid: &[bool; 256]) -> Option<Self> {
        Self::locate_inner(entry, entry_idx, Some(valid))
    }

    fn locate_inner(entry: &[u8], entry_idx: usize, valid: Option<&[bool; 256]>) -> Option<Self> {
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
        let shops = shop_sites(&decoded, valid);
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

/// Scan a decompressed MAN for every town-shop site: op `0x49` sub-op `0` whose
/// inline payload validates as a `[count][ids][name]` shop record.
///
/// This is a **byte scan**, not an opcode walk: a shop's op `0x49` is often
/// gated behind a dialogue confirm-picker ("Buy them?") whose option-jump table
/// desyncs a linear disassembler before it reaches the shop op (Biron's Corey
/// vendor is the canonical case the walk missed). The scan finds the record
/// regardless of how the script reaches it. The op-`0x49` + sub-op-`0` prefix
/// (the byte after the opcode must be `0x00`) already filters out almost every
/// stray `0x49` byte — those inside item-id lists, operands, or `0x49`-lead
/// names like "Items Shop" are followed by a non-zero byte — and the record
/// validation ([`parse_shop_record`]) does the rest.
///
/// `valid` optionally restricts shop ids to **named items** (the SCUS item
/// table): when supplied, every item id in the record must name a real item.
/// This is the strongest guard against a false positive corrupting non-shop
/// bytes; the apply layer always supplies it. `None` validates structurally
/// only (used by tests).
pub fn shop_sites(man: &[u8], valid: Option<&[bool; 256]>) -> Vec<ShopSite> {
    let mut out: Vec<ShopSite> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for op in 0..man.len() {
        if man[op] != SHOP_OPCODE {
            continue;
        }
        if let Some(site) = parse_shop_record(man, op, valid)
            && !seen.contains(&site.count_off)
        {
            seen.push(site.count_off);
            out.push(site);
        }
    }
    out.sort_by_key(|s| s.count_off);
    out
}

/// Parse + validate a shop record at the op-`0x49` byte `op_abs` (absolute in
/// `man`). The sub-op-`0` layout is `0x49 0x00 <length> <length args> [count]
/// [ids] [name\0]`; returns `None` unless the payload validates as a shop: a
/// small non-zero count, all ids non-zero (and, when `valid` is supplied, all
/// naming a real item), and a printable name terminated by `0x00`.
fn parse_shop_record(man: &[u8], op_abs: usize, valid: Option<&[bool; 256]>) -> Option<ShopSite> {
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
    if let Some(v) = valid
        && !ids.iter().all(|&id| v[id as usize])
    {
        // An id that names no real item ⇒ not a (sellable) shop record.
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
        let site = parse_shop_record(&man, 4, None).expect("valid shop record");
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
        assert!(parse_shop_record(&m, 0, None).is_none());

        // count 0 -> rejected.
        let m = vec![0x49, 0x00, 0x00, 0x00, b'X', b'Y', 0x00];
        assert!(parse_shop_record(&m, 0, None).is_none());

        // An id byte is 0 -> rejected.
        let mut m = vec![0x49, 0x00, 0x00, 0x02, 0x22, 0x00];
        m.extend_from_slice(b"Shop\0");
        assert!(parse_shop_record(&m, 0, None).is_none());

        // Name not name-shaped (starts with a digit) -> rejected.
        let mut m = vec![0x49, 0x00, 0x00, 0x01, 0x22];
        m.extend_from_slice(b"3X\0");
        assert!(parse_shop_record(&m, 0, None).is_none());

        // Name not terminated / not printable -> rejected.
        let m = vec![0x49, 0x00, 0x00, 0x01, 0x22, 0x1F, 0x40];
        assert!(parse_shop_record(&m, 0, None).is_none());
    }

    #[test]
    fn honours_length_arg_offset() {
        // length=2 shifts the record start by 2 (op+3+2 = op+5).
        let mut man = vec![0x49, 0x00, 0x02, 0xAA, 0xBB, 0x02, 0x77, 0x7e];
        man.extend_from_slice(b"Item\0");
        let site = parse_shop_record(&man, 0, None).expect("length-shifted record");
        assert_eq!(site.id_offsets, vec![6, 7]);
        assert_eq!(site.name, "Item");
    }

    #[test]
    fn scan_finds_a_record_past_arbitrary_bytes() {
        // The scan must find a shop op-0x49 even when it isn't reachable by a
        // clean linear opcode walk (the Corey-behind-a-picker case). Embed the
        // record after some arbitrary "script" bytes that a walk would desync on.
        let mut man = vec![0x2A, 0x0E, 0x00, 0x46, 0xFF, 0x1F, b'Y', 0x00];
        let rec_at = man.len();
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x02, 0x77, 0x7e]); // shop: 2 ids
        man.extend_from_slice(b"Corey\0");
        let sites = shop_sites(&man, None);
        assert_eq!(sites.len(), 1, "scan finds the embedded shop");
        assert_eq!(sites[0].name, "Corey");
        assert_eq!(sites[0].count_off, rec_at + 3);
    }

    #[test]
    fn valid_mask_rejects_unnamed_ids() {
        // id 0x77 named, 0xFE not named -> with the mask, the record is rejected.
        let mut man = vec![0x49, 0x00, 0x00, 0x02, 0x77, 0xFE];
        man.extend_from_slice(b"Shop\0");
        let mut mask = [true; 256];
        mask[0xFE] = false;
        assert!(
            parse_shop_record(&man, 0, Some(&mask)).is_none(),
            "an unnamed id fails the SCUS mask check"
        );
        // Without the mask (structural only) it parses.
        assert!(parse_shop_record(&man, 0, None).is_some());
    }
}
