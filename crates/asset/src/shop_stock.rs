//! Town gold-shop **stock records** embedded in a scene MAN.
//!
//! Unlike the casino prize exchange (a static overlay table), a gold town
//! merchant's stock is defined **inline in the scene's field-VM script** (the
//! MAN, asset type `0x03`), the same place chests (op `0x39`) and doors
//! (op `0x3F`) live. Opening a shop is field-VM **op `0x49` (`STATE_RESUME`)** —
//! the multi-frame state machine that drives the menu-request register
//! `_DAT_8007B450`. In its sub-op-`0` form it carries an inline payload that,
//! for a shop, is:
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
//! the curated shops table).
//!
//! ## `count` includes unsellable padding
//!
//! The `count` byte counts the purchasable stock **plus** a trailing run of
//! unsellable, price-`0` *template* ids — commonly the "Ra-Seru Meta $N"
//! placeholders `0x01/0x02/0x03`, or a lone `0x03`. The on-screen shop stops at
//! the sellable run, so this tail is structural padding, not stock. The Rim Elm
//! Variety Store that pinned the format happens to have a tail-less ten-item
//! list, which is why the padding wasn't in the original capture. Across the
//! whole disc every shop partitions cleanly — a leading run of price-`> 0` items
//! then an unsellable tail (≤ 3 ids), never interleaved — and the priced prefix
//! matches the curated walkthrough stock (e.g. "Market" decodes to 10 ids but
//! sells 7). [`ShopRecord::sellable_count`] returns the priced-prefix length
//! given the SCUS price table; the asset-`tests` cross-table sweep
//! (`legaia_rando::cross_table_integrity_real`) pins the partition disc-wide.
//!
//! ## Locating sites safely
//!
//! Sites are found by **scanning** the decompressed MAN for the op-`0x49`
//! sub-op-`0` shop signature, *not* by an opcode walk. A shop's `0x49` is often
//! gated behind a dialogue confirm-picker ("Buy them?") whose option-jump table
//! desyncs a linear disassembler before it reaches the op (Biron Monastery's
//! Corey vendor is the case that exposed this), so a walk silently misses those
//! shops. The scan doesn't care how the script reaches the op. False positives
//! are ruled out by a strict record validation ([`parse_record`]): the byte
//! after the opcode must be `0x00` (sub-op 0 — this alone rejects almost every
//! stray `0x49`), the count is small and non-zero, every id is non-zero (and,
//! with the SCUS item mask, names a real item), and the trailing shop name is a
//! printable, letter-initial, `0x00`-terminated string.
//!
//! This is the shared *read* side: the randomizer
//! ([`legaia_rando::shop`](https://docs.rs)) wraps [`locate`] with its
//! recompress/write-back machinery, and the engine builds its shop UI stock from
//! the same records (`legaia_engine_core::shop_catalog`).

use crate::scene_asset_table;

/// Scene-MAN asset type byte.
const MAN_TYPE: u8 = 0x03;
/// Field-VM opcode that opens a shop (`STATE_RESUME`).
const SHOP_OPCODE: u8 = 0x49;
/// Max item count a shop record is allowed to declare (a sanity bound that
/// rejects a non-shop `0x49` payload whose first byte happens to be large).
const MAX_SHOP_ITEMS: usize = 16;
/// Max trailing unsellable template-id padding a shop record's `count` may carry
/// past the purchasable stock (see the module docs). Observed values are 0, 1, or
/// 3 disc-wide; the bound rejects a non-shop payload whose only valid-looking
/// byte is its first.
const MAX_SHOP_PAD: usize = 3;

/// One town-shop site located in a scene MAN: its declared item count, the
/// absolute offsets (within the decoded MAN) of each item-id byte, and the
/// shop's display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopRecord {
    /// Absolute offset of the `count` byte within the decoded MAN.
    pub count_off: usize,
    /// Absolute offsets of each item-id byte (length == count).
    pub id_offsets: Vec<usize>,
    /// The shop's on-screen name (decoded ASCII), for listings / audit.
    pub name: String,
}

impl ShopRecord {
    /// The number of leading **purchasable** item ids — the real shop stock,
    /// excluding the trailing unsellable template-id padding the record `count`
    /// over-counts (see the module docs). `man` is the decoded MAN the offsets
    /// index into; `is_priced` reports whether an id has a `> 0` `SCUS_942.54`
    /// price (e.g. `|id| legaia_asset::item_names::price_slot(scus, id)
    /// .is_some_and(|(_, p)| p > 0)`).
    ///
    /// The stock is the leading priced run: every shop on the disc partitions
    /// cleanly into a priced prefix then an unsellable tail (verified disc-wide),
    /// so the prefix length is the count the shop UI actually shows.
    pub fn sellable_count(&self, man: &[u8], is_priced: impl Fn(u8) -> bool) -> usize {
        self.id_offsets
            .iter()
            .take_while(|&&o| man.get(o).copied().is_some_and(&is_priced))
            .count()
    }
}

/// A scene MAN located + decompressed from a PROT entry, with its shop records.
#[derive(Debug, Clone)]
pub struct LocatedShops {
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Decompressed MAN (the [`ShopRecord`] offsets index into this).
    pub decoded: Vec<u8>,
    /// The shop records found in this scene's MAN.
    pub records: Vec<ShopRecord>,
}

/// Locate + decompress a scene-bundle's MAN and scan it for town-shop sites.
///
/// `entry` is the raw PROT entry bytes; `valid`, when supplied, restricts shop
/// ids to **named items** (a `256`-entry "id names a real item" mask from the
/// SCUS item table) so a stray `0x49`-prefixed byte run can't be mistaken for a
/// shop. Returns `None` when the entry isn't a scene bundle, has no MAN, the MAN
/// fails to decompress to its declared size, or has no shop record.
pub fn locate(entry: &[u8], valid: Option<&[bool; 256]>) -> Option<LocatedShops> {
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
    let (decoded, _consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
    if decoded.len() != man.size as usize {
        return None;
    }
    let records = scan(&decoded, valid);
    if records.is_empty() {
        return None;
    }
    Some(LocatedShops {
        man_offset,
        decoded,
        records,
    })
}

/// Scan a decompressed MAN for every town-shop site: op `0x49` sub-op `0` whose
/// inline payload validates as a `[count][ids][name]` shop record.
///
/// This is a **byte scan**, not an opcode walk (see the module docs for why).
/// `valid` optionally restricts shop ids to named items.
pub fn scan(man: &[u8], valid: Option<&[bool; 256]>) -> Vec<ShopRecord> {
    let mut out: Vec<ShopRecord> = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    for op in 0..man.len() {
        if man[op] != SHOP_OPCODE {
            continue;
        }
        if let Some(site) = parse_record(man, op, valid)
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
/// [ids] [name\0]`; returns `None` unless the payload validates as a shop.
pub fn parse_record(man: &[u8], op_abs: usize, valid: Option<&[bool; 256]>) -> Option<ShopRecord> {
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
    // The declared `count` covers the purchasable stock PLUS a trailing run of
    // unsellable template-id padding (see the module docs). When a `valid` mask
    // is supplied, the stock is the leading run of valid ids; the record holds
    // only if it sells at least one valid item, the padding tail is entirely
    // invalid (no interleaving — verified disc-wide), and the padding stays
    // within the observed bound. This both rejects non-shop `0x49` payloads and
    // trims the padding out of the returned stock. With no mask the whole
    // declared list is kept verbatim.
    let stock = match valid {
        Some(v) => {
            let stock = ids.iter().take_while(|&&id| v[id as usize]).count();
            if stock == 0 {
                // Doesn't lead with a real sellable item ⇒ not a shop record.
                return None;
            }
            let pad = &ids[stock..];
            if pad.len() > MAX_SHOP_PAD || pad.iter().any(|&id| v[id as usize]) {
                return None;
            }
            stock
        }
        None => count,
    };
    // Name: a printable ASCII run terminated by 0x00, first char a letter. It
    // sits after the FULL declared list (count includes the padding), so the
    // name position is unchanged whether or not the padding is trimmed.
    let name = read_name(man, ids_end)?;
    Some(ShopRecord {
        count_off,
        id_offsets: (ids_start..ids_start + stock).collect(),
        name,
    })
}

/// Read a shop name string at `start`: printable ASCII (2..=18 chars), first
/// char alphabetic, terminated by `0x00`. `None` if it isn't shop-name-shaped
/// (this is the key rejector for non-shop `0x49` sub-0 payloads).
fn read_name(man: &[u8], start: usize) -> Option<String> {
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
    fn parse_record_reads_ids_and_name() {
        // 0x49 0x00 0x00 [count=3] 0x22 0x34 0x59 "Shop\0"
        let mut man = vec![0u8; 4];
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x03, 0x22, 0x34, 0x59]);
        man.extend_from_slice(b"Shop\0");
        let site = parse_record(&man, 4, None).expect("valid shop record");
        assert_eq!(site.name, "Shop");
        assert_eq!(site.id_offsets, vec![4 + 4, 4 + 5, 4 + 6]);
        assert_eq!(
            site.id_offsets.iter().map(|&o| man[o]).collect::<Vec<_>>(),
            vec![0x22, 0x34, 0x59]
        );
    }

    /// `sellable_count` counts the leading priced run and stops at the first
    /// unsellable padding id (here `0x01/0x02/0x03`, like the real "Market").
    #[test]
    fn sellable_count_trims_unsellable_padding() {
        // 7 priced ids then the 01 02 03 template padding, "Market\0".
        let mut man = vec![0u8; 4];
        let stock = [0xD3u8, 0xD4, 0x77, 0x78, 0x7C, 0x7F, 0x88];
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x0A]);
        man.extend_from_slice(&stock);
        man.extend_from_slice(&[0x01, 0x02, 0x03]);
        man.extend_from_slice(b"Market\0");
        let site = parse_record(&man, 4, None).expect("valid shop record");
        assert_eq!(site.id_offsets.len(), 10, "count includes the padding");
        // Priced predicate: the seven real ids, not 0x01/0x02/0x03.
        let priced = |id: u8| stock.contains(&id);
        assert_eq!(site.sellable_count(&man, priced), 7);
    }

    #[test]
    fn rejects_non_shop_0x49_payloads() {
        // sub-op != 0 -> not the inline form.
        let mut m = vec![0x49, 0x01, 0x00, 0x03, 0x22, 0x34, 0x59];
        m.extend_from_slice(b"Shop\0");
        assert!(parse_record(&m, 0, None).is_none());

        // count 0 -> rejected.
        let m = vec![0x49, 0x00, 0x00, 0x00, b'X', b'Y', 0x00];
        assert!(parse_record(&m, 0, None).is_none());

        // An id byte is 0 -> rejected.
        let mut m = vec![0x49, 0x00, 0x00, 0x02, 0x22, 0x00];
        m.extend_from_slice(b"Shop\0");
        assert!(parse_record(&m, 0, None).is_none());

        // Name not name-shaped (starts with a digit) -> rejected.
        let mut m = vec![0x49, 0x00, 0x00, 0x01, 0x22];
        m.extend_from_slice(b"3X\0");
        assert!(parse_record(&m, 0, None).is_none());

        // Name not terminated / not printable -> rejected.
        let m = vec![0x49, 0x00, 0x00, 0x01, 0x22, 0x1F, 0x40];
        assert!(parse_record(&m, 0, None).is_none());
    }

    #[test]
    fn honours_length_arg_offset() {
        // length=2 shifts the record start by 2 (op+3+2 = op+5).
        let mut man = vec![0x49, 0x00, 0x02, 0xAA, 0xBB, 0x02, 0x77, 0x7e];
        man.extend_from_slice(b"Item\0");
        let site = parse_record(&man, 0, None).expect("length-shifted record");
        assert_eq!(site.id_offsets, vec![6, 7]);
        assert_eq!(site.name, "Item");
    }

    #[test]
    fn scan_finds_a_record_past_arbitrary_bytes() {
        // The scan must find a shop op-0x49 even when it isn't reachable by a
        // clean linear opcode walk (the Corey-behind-a-picker case).
        let mut man = vec![0x2A, 0x0E, 0x00, 0x46, 0xFF, 0x1F, b'Y', 0x00];
        let rec_at = man.len();
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x02, 0x77, 0x7e]); // shop: 2 ids
        man.extend_from_slice(b"Corey\0");
        let sites = scan(&man, None);
        assert_eq!(sites.len(), 1, "scan finds the embedded shop");
        assert_eq!(sites[0].name, "Corey");
        assert_eq!(sites[0].count_off, rec_at + 3);
    }

    /// With a mask, the stock is the leading valid run; a bounded trailing run
    /// of invalid (unsellable) ids is treated as padding and trimmed, but a shop
    /// that doesn't *lead* with a valid item — or interleaves valid past the
    /// padding, or pads too far — is rejected.
    #[test]
    fn valid_mask_splits_stock_from_padding() {
        let mut mask = [true; 256];
        for id in [0x01u8, 0x02, 0x03, 0xFE] {
            mask[id as usize] = false;
        }
        let rec = |ids: &[u8]| {
            let mut man = vec![0x49, 0x00, 0x00, ids.len() as u8];
            man.extend_from_slice(ids);
            man.extend_from_slice(b"Shop\0");
            man
        };

        // Trailing invalid run = padding: stock trimmed to the leading valid run.
        let man = rec(&[0x77, 0x88, 0x01, 0x02, 0x03]);
        let site = parse_record(&man, 0, Some(&mask)).expect("padding-tail shop");
        assert_eq!(
            site.id_offsets.iter().map(|&o| man[o]).collect::<Vec<_>>(),
            vec![0x77, 0x88],
            "padding 01 02 03 is trimmed from the stock"
        );

        // Leads with an invalid id -> not a shop.
        assert!(parse_record(&rec(&[0xFE, 0x77]), 0, Some(&mask)).is_none());

        // Valid id *after* the padding starts (interleaved) -> rejected.
        assert!(parse_record(&rec(&[0x77, 0xFE, 0x88]), 0, Some(&mask)).is_none());

        // Padding longer than the observed bound -> rejected.
        assert!(parse_record(&rec(&[0x77, 0x01, 0x02, 0x03, 0xFE]), 0, Some(&mask)).is_none());

        // Without the mask (structural only) the whole declared list is kept.
        let man = rec(&[0x77, 0x88, 0x01, 0x02, 0x03]);
        assert_eq!(
            parse_record(&man, 0, None).unwrap().id_offsets.len(),
            5,
            "structural scan keeps the full count"
        );
    }
}
