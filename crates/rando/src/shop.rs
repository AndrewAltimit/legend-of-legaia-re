//! Shop-inventory randomizer: reassign what each town store sells.
//!
//! ## On-disc layout
//!
//! The buy lists live in the field/menu overlay's data segment as a flat table
//! of fixed-size shop blocks (`DAT_801e4518`, read by the buy/confirm handlers
//! `FUN_801d5de0` / `FUN_801dc1cc`). The active shop is selected by
//! `shop_index * 0x60`, and within a block items are 8-byte records:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u16 | item id (shared 256-entry id space; `0` ends the list) |
//! | `+2` | u16 | flag word (equip-comparison / sort hint — preserved as-is) |
//! | `+4` | u32 | buy price in gold |
//!
//! A block is `0x60` bytes = up to [`RECORDS_PER_BLOCK`] records; the retail
//! buy-list loop iterates while the record's item id is `> 0`, so the first
//! `id == 0` record terminates that shop's list.
//!
//! ## Randomization model
//!
//! The whole 8-byte record (id + flag + price) is the unit that moves, so an
//! item always carries its own real price wherever it lands — no external price
//! table is needed (the shop records *are* the price source). `Shuffle`
//! redistributes the existing multiset of shop entries across all active slots
//! (each shop keeps its item *count*); `Random` draws each active slot from that
//! same pool with replacement. Every block's active count and terminator stay
//! put, so the edit is strictly same-size and in place — no slot offset moves.

use crate::drops::DropMode;
use crate::rng::SplitMix64;

/// Bytes per shop item record (`[u16 id][u16 flag][u32 price]`).
pub const SHOP_RECORD_SIZE: usize = 8;
/// Bytes per shop block in the table.
pub const SHOP_BLOCK_SIZE: usize = 0x60;
/// Maximum item records a single shop block can hold (`0x60 / 8`).
pub const RECORDS_PER_BLOCK: usize = SHOP_BLOCK_SIZE / SHOP_RECORD_SIZE;

/// One shop buy-list entry — a verbatim 8-byte record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShopRecord {
    /// Item id (`0` is the list terminator and never appears in an active slot).
    pub item_id: u16,
    /// Flag word (equip-comparison / sort hint). Preserved when the record
    /// moves so the item keeps whatever UI behaviour it had.
    pub flag: u16,
    /// Buy price in gold.
    pub price: u32,
}

impl ShopRecord {
    fn read(buf: &[u8], off: usize) -> Self {
        ShopRecord {
            item_id: u16::from_le_bytes([buf[off], buf[off + 1]]),
            flag: u16::from_le_bytes([buf[off + 2], buf[off + 3]]),
            price: u32::from_le_bytes([buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7]]),
        }
    }

    fn write(self, buf: &mut [u8], off: usize) {
        buf[off..off + 2].copy_from_slice(&self.item_id.to_le_bytes());
        buf[off + 2..off + 4].copy_from_slice(&self.flag.to_le_bytes());
        buf[off + 4..off + 8].copy_from_slice(&self.price.to_le_bytes());
    }
}

/// The decoded shop table: one inner `Vec` of active records per shop block, in
/// table order. Only the leading `id > 0` records of each block are kept (the
/// terminator + trailing padding are implied by the count).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShopTable {
    /// Active records per shop block (block `i` starts at `base + i*0x60`).
    pub shops: Vec<Vec<ShopRecord>>,
    /// Byte offset of the table base within the buffer it was parsed from.
    pub base: usize,
    /// Number of shop blocks parsed.
    pub block_count: usize,
}

impl ShopTable {
    /// Parse `block_count` shop blocks starting at byte offset `base` in `buf`.
    /// Each block's active records are the leading run with `item_id > 0`.
    /// Returns `None` if the table would run past the end of `buf`.
    pub fn parse(buf: &[u8], base: usize, block_count: usize) -> Option<Self> {
        let end = base.checked_add(block_count.checked_mul(SHOP_BLOCK_SIZE)?)?;
        if end > buf.len() {
            return None;
        }
        let mut shops = Vec::with_capacity(block_count);
        for b in 0..block_count {
            let block_off = base + b * SHOP_BLOCK_SIZE;
            let mut records = Vec::new();
            for r in 0..RECORDS_PER_BLOCK {
                let rec = ShopRecord::read(buf, block_off + r * SHOP_RECORD_SIZE);
                if rec.item_id == 0 {
                    break;
                }
                records.push(rec);
            }
            shops.push(records);
        }
        Some(ShopTable {
            shops,
            base,
            block_count,
        })
    }

    /// Total active item slots across all shops (the randomizable population).
    pub fn active_slot_count(&self) -> usize {
        self.shops.iter().map(|s| s.len()).sum()
    }

    /// Plan a randomization in place: reassign every active slot from the global
    /// pool of existing shop records, preserving each shop's item *count*.
    /// Deterministic in `(self, seed, mode)`.
    ///
    /// - [`DropMode::Shuffle`] permutes the existing multiset of records across
    ///   all slots (so the same set of items is for sale, in new shops).
    /// - [`DropMode::Random`] draws each slot from that multiset with
    ///   replacement (a shop might stock anything that's sold somewhere).
    pub fn randomize(&mut self, seed: u64, mode: DropMode) {
        let pool: Vec<ShopRecord> = self.shops.iter().flatten().copied().collect();
        if pool.is_empty() {
            return;
        }
        let mut rng = SplitMix64::new(seed);
        match mode {
            DropMode::Shuffle => {
                let mut bag = pool;
                rng.shuffle(&mut bag);
                let mut it = bag.into_iter();
                for shop in &mut self.shops {
                    for slot in shop.iter_mut() {
                        *slot = it.next().expect("bag has one entry per active slot");
                    }
                }
            }
            DropMode::Random => {
                for shop in &mut self.shops {
                    for slot in shop.iter_mut() {
                        *slot = pool[rng.below(pool.len())];
                    }
                }
            }
        }
    }

    /// Write the (possibly randomized) records back over the table region of
    /// `buf` in place. Each block's active records are written followed by a
    /// zeroed terminator record (so a list that didn't fill its block still
    /// ends correctly); the slot count per block is unchanged, so the write is
    /// strictly same-size.
    pub fn write_back(&self, buf: &mut [u8]) {
        for (b, shop) in self.shops.iter().enumerate() {
            let block_off = self.base + b * SHOP_BLOCK_SIZE;
            for (r, rec) in shop.iter().enumerate() {
                rec.write(buf, block_off + r * SHOP_RECORD_SIZE);
            }
            // Re-zero the terminator slot (and it alone) in case a future model
            // ever shortened a list; with count preserved this is a no-op for
            // the slot right after the active run, bounded to the block.
            if shop.len() < RECORDS_PER_BLOCK {
                let term = block_off + shop.len() * SHOP_RECORD_SIZE;
                for byte in &mut buf[term..term + SHOP_RECORD_SIZE] {
                    *byte = 0;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic table buffer of `blocks` shops, each with the given
    /// active item ids (price = id*10, flag = id) followed by a zero terminator.
    fn synth(blocks: &[&[u16]]) -> (Vec<u8>, usize) {
        let base = 16; // non-zero base to exercise the offset math
        let mut buf = vec![0u8; base + blocks.len() * SHOP_BLOCK_SIZE + 16];
        for (b, ids) in blocks.iter().enumerate() {
            let off = base + b * SHOP_BLOCK_SIZE;
            for (r, &id) in ids.iter().enumerate() {
                ShopRecord {
                    item_id: id,
                    flag: id,
                    price: id as u32 * 10,
                }
                .write(&mut buf, off + r * SHOP_RECORD_SIZE);
            }
        }
        (buf, base)
    }

    #[test]
    fn parse_keeps_active_records_only() {
        let (buf, base) = synth(&[&[0x43, 0x77, 0x7e], &[0x22, 0x34]]);
        let t = ShopTable::parse(&buf, base, 2).unwrap();
        assert_eq!(t.shops[0].len(), 3);
        assert_eq!(t.shops[1].len(), 2);
        assert_eq!(t.shops[0][0].item_id, 0x43);
        assert_eq!(t.shops[0][0].price, 0x43 * 10);
        assert_eq!(t.active_slot_count(), 5);
    }

    #[test]
    fn parse_rejects_out_of_bounds() {
        let buf = vec![0u8; 10];
        assert!(ShopTable::parse(&buf, 0, 2).is_none());
    }

    #[test]
    fn shuffle_preserves_multiset_and_per_shop_counts() {
        let (buf, base) = synth(&[&[0x43, 0x77, 0x7e], &[0x22, 0x34], &[0x88]]);
        let mut t = ShopTable::parse(&buf, base, 3).unwrap();
        let counts: Vec<usize> = t.shops.iter().map(|s| s.len()).collect();
        let mut before: Vec<ShopRecord> = t.shops.iter().flatten().copied().collect();
        t.randomize(0x1234, DropMode::Shuffle);
        let after_counts: Vec<usize> = t.shops.iter().map(|s| s.len()).collect();
        assert_eq!(counts, after_counts, "each shop keeps its item count");
        let mut after: Vec<ShopRecord> = t.shops.iter().flatten().copied().collect();
        before.sort_by_key(|r| (r.item_id, r.price));
        after.sort_by_key(|r| (r.item_id, r.price));
        assert_eq!(before, after, "shuffle preserves the full record multiset");
    }

    #[test]
    fn random_draws_only_from_the_existing_pool() {
        let (buf, base) = synth(&[&[0x43, 0x77], &[0x22, 0x34, 0x88]]);
        let mut t = ShopTable::parse(&buf, base, 2).unwrap();
        let pool: std::collections::HashSet<u16> =
            t.shops.iter().flatten().map(|r| r.item_id).collect();
        t.randomize(0x9, DropMode::Random);
        for rec in t.shops.iter().flatten() {
            assert!(pool.contains(&rec.item_id), "random item from pool");
            assert_eq!(
                rec.price,
                rec.item_id as u32 * 10,
                "price travels with item"
            );
        }
    }

    #[test]
    fn deterministic_for_seed() {
        let (buf, base) = synth(&[&[0x43, 0x77, 0x7e], &[0x22, 0x34]]);
        for mode in [DropMode::Shuffle, DropMode::Random] {
            let mut a = ShopTable::parse(&buf, base, 2).unwrap();
            let mut b = ShopTable::parse(&buf, base, 2).unwrap();
            a.randomize(0xABCD, mode);
            b.randomize(0xABCD, mode);
            assert_eq!(a, b, "same seed reproduces the plan ({mode:?})");
        }
    }

    #[test]
    fn write_back_is_same_size_and_round_trips() {
        let (buf, base) = synth(&[&[0x43, 0x77, 0x7e], &[0x22, 0x34]]);
        let mut t = ShopTable::parse(&buf, base, 2).unwrap();
        t.randomize(0x5, DropMode::Shuffle);
        let mut out = buf.clone();
        t.write_back(&mut out);
        assert_eq!(out.len(), buf.len(), "write-back never resizes");
        // Re-parse the written buffer and confirm it matches the planned table.
        let reparsed = ShopTable::parse(&out, base, 2).unwrap();
        assert_eq!(reparsed.shops, t.shops, "records round-trip through bytes");
    }
}
