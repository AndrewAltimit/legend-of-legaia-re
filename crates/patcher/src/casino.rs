//! Casino prize-exchange randomizer: reassign the prizes you redeem coins for.
//!
//! ## On-disc layout
//!
//! Unlike the town gold merchants (which are inline in each scene's field-VM
//! script - see [`crate::shop`]), the **casino prize exchange** is a static
//! table in the menu/save/shop overlay's data segment (`DAT_801e4518`, read by
//! the buy/confirm handlers `FUN_801d5de0` / `FUN_801dc1cc`). It debits the
//! **casino coin** bank (`_DAT_800845A4`, the "Infinite Coins" cheat target),
//! not gold - which is how it's distinguished from a gold shop.
//!
//! The overlay's data segment is **PROT entry 0899** (`0899_xxx_dat.BIN`),
//! stored **raw** (no LZS). The table base `DAT_801e4518` maps to file offset
//! **0x15D00** under the overlay data-segment load base VA `0x801CE818` (pinned
//! via the save-UI function-pointer table `0x801E4F40` at file offset
//! `0x16728`). The active prize block is selected by `block_index * 0x60`, and
//! within a block items are 8-byte records:
//!
//! | Offset | Type | Field |
//! |---|---|---|
//! | `+0` | u16 | item id (shared 256-entry id space; `0` ends the list) |
//! | `+2` | u16 | story-flag gate (`0` = always available; `0x36..0x3c` gate the high-value prizes behind casino progression) |
//! | `+4` | u32 | price in casino coins |
//!
//! A block is `0x60` bytes = up to [`RECORDS_PER_BLOCK`] records; the buy-list
//! loop iterates while the record's item id is `> 0`, so the first `id == 0`
//! record terminates that block. The retail US disc has [`CASINO_BLOCK_COUNT`]
//! prize blocks.
//!
//! ## Randomization model
//!
//! The whole 8-byte record (id + gate + price) is the unit that moves, so a
//! prize always carries its own coin price and progression gate wherever it
//! lands - no external price table is needed. `Shuffle` redistributes the
//! existing multiset of prize entries across all active slots (each block keeps
//! its prize *count*); `Random` draws each active slot from that same pool with
//! replacement. Every block's active count and terminator stay put, so the edit
//! is strictly same-size and in place - and since 0899 is raw, no recompression
//! is involved.

use crate::drops::DropMode;
use crate::rng::SplitMix64;

/// Bytes per prize record (`[u16 id][u16 gate][u32 price]`).
pub const RECORD_SIZE: usize = 8;
/// Bytes per prize block in the table.
pub const BLOCK_SIZE: usize = 0x60;
/// Maximum records a single prize block can hold (`0x60 / 8`).
pub const RECORDS_PER_BLOCK: usize = BLOCK_SIZE / RECORD_SIZE;

/// File offset of the prize table (`DAT_801e4518`) within PROT entry 0899's raw
/// bytes (`0x801E4518 − 0x801CE818`).
pub const CASINO_TABLE_OFFSET: usize = 0x15D00;
/// PROT entry index of the menu/save/shop overlay data segment (`0899_xxx_dat`;
/// the `0899` is a decimal index).
pub const CASINO_ENTRY: usize = 899;
/// Number of prize blocks the retail US disc's casino table holds.
pub const CASINO_BLOCK_COUNT: usize = 4;

/// One casino prize entry - a verbatim 8-byte record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrizeRecord {
    /// Item id (`0` is the list terminator and never appears in an active slot).
    pub item_id: u16,
    /// Story-flag gate (`0` = always available). Preserved when the record moves
    /// so a prize keeps its progression requirement.
    pub gate: u16,
    /// Price in casino coins.
    pub price: u32,
}

impl PrizeRecord {
    fn read(buf: &[u8], off: usize) -> Self {
        PrizeRecord {
            item_id: u16::from_le_bytes([buf[off], buf[off + 1]]),
            gate: u16::from_le_bytes([buf[off + 2], buf[off + 3]]),
            price: u32::from_le_bytes([buf[off + 4], buf[off + 5], buf[off + 6], buf[off + 7]]),
        }
    }

    fn write(self, buf: &mut [u8], off: usize) {
        buf[off..off + 2].copy_from_slice(&self.item_id.to_le_bytes());
        buf[off + 2..off + 4].copy_from_slice(&self.gate.to_le_bytes());
        buf[off + 4..off + 8].copy_from_slice(&self.price.to_le_bytes());
    }
}

/// The decoded casino prize table: one inner `Vec` of active records per block,
/// in table order. Only the leading `id > 0` records of each block are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CasinoExchange {
    /// Active records per prize block (block `i` starts at `base + i*0x60`).
    pub blocks: Vec<Vec<PrizeRecord>>,
    /// Byte offset of the table base within the buffer it was parsed from.
    pub base: usize,
    /// Number of prize blocks parsed.
    pub block_count: usize,
}

impl CasinoExchange {
    /// Parse `block_count` prize blocks starting at byte offset `base` in `buf`
    /// (typically [`CASINO_TABLE_OFFSET`] of PROT entry 0899). Each block's
    /// active records are the leading run with `item_id > 0`. Returns `None` if
    /// the table would run past the end of `buf`.
    pub fn parse(buf: &[u8], base: usize, block_count: usize) -> Option<Self> {
        let end = base.checked_add(block_count.checked_mul(BLOCK_SIZE)?)?;
        if end > buf.len() {
            return None;
        }
        let mut blocks = Vec::with_capacity(block_count);
        for b in 0..block_count {
            let block_off = base + b * BLOCK_SIZE;
            let mut records = Vec::new();
            for r in 0..RECORDS_PER_BLOCK {
                let rec = PrizeRecord::read(buf, block_off + r * RECORD_SIZE);
                if rec.item_id == 0 {
                    break;
                }
                records.push(rec);
            }
            blocks.push(records);
        }
        Some(CasinoExchange {
            blocks,
            base,
            block_count,
        })
    }

    /// Total active prize slots across all blocks (the randomizable population).
    pub fn active_slot_count(&self) -> usize {
        self.blocks.iter().map(|b| b.len()).sum()
    }

    /// Plan a randomization in place: reassign every active slot from the global
    /// pool of existing prize records, preserving each block's prize *count*.
    /// Deterministic in `(self, seed, mode)`.
    pub fn randomize(&mut self, seed: u64, mode: DropMode) {
        let pool: Vec<PrizeRecord> = self.blocks.iter().flatten().copied().collect();
        if pool.is_empty() {
            return;
        }
        let mut rng = SplitMix64::new(seed);
        match mode {
            DropMode::Shuffle => {
                let mut bag = pool;
                rng.shuffle(&mut bag);
                let mut it = bag.into_iter();
                for block in &mut self.blocks {
                    for slot in block.iter_mut() {
                        *slot = it.next().expect("bag has one entry per active slot");
                    }
                }
            }
            DropMode::Random => {
                for block in &mut self.blocks {
                    for slot in block.iter_mut() {
                        *slot = pool[rng.below(pool.len())];
                    }
                }
            }
        }
    }

    /// Write the (possibly randomized) records back over the table region of
    /// `buf` in place. The slot count per block is unchanged, so the write is
    /// strictly same-size.
    pub fn write_back(&self, buf: &mut [u8]) {
        for (b, block) in self.blocks.iter().enumerate() {
            let block_off = self.base + b * BLOCK_SIZE;
            for (r, rec) in block.iter().enumerate() {
                rec.write(buf, block_off + r * RECORD_SIZE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic table buffer of `blocks` prize blocks, each with the
    /// given active item ids (price = id*10, gate = id) + a zero terminator.
    fn synth(blocks: &[&[u16]]) -> (Vec<u8>, usize) {
        let base = 16;
        let mut buf = vec![0u8; base + blocks.len() * BLOCK_SIZE + 16];
        for (b, ids) in blocks.iter().enumerate() {
            let off = base + b * BLOCK_SIZE;
            for (r, &id) in ids.iter().enumerate() {
                PrizeRecord {
                    item_id: id,
                    gate: id,
                    price: id as u32 * 10,
                }
                .write(&mut buf, off + r * RECORD_SIZE);
            }
        }
        (buf, base)
    }

    #[test]
    fn parse_keeps_active_records_only() {
        let (buf, base) = synth(&[&[0xD0, 0xE7, 0xC1], &[0xEC, 0xC4]]);
        let t = CasinoExchange::parse(&buf, base, 2).unwrap();
        assert_eq!(t.blocks[0].len(), 3);
        assert_eq!(t.blocks[1].len(), 2);
        assert_eq!(t.blocks[0][0].item_id, 0xD0);
        assert_eq!(t.active_slot_count(), 5);
    }

    #[test]
    fn shuffle_preserves_multiset_and_counts() {
        let (buf, base) = synth(&[&[0xD0, 0xE7, 0xC1], &[0xEC, 0xC4], &[0x80]]);
        let mut t = CasinoExchange::parse(&buf, base, 3).unwrap();
        let counts: Vec<usize> = t.blocks.iter().map(|b| b.len()).collect();
        let mut before: Vec<PrizeRecord> = t.blocks.iter().flatten().copied().collect();
        t.randomize(0x1234, DropMode::Shuffle);
        let after_counts: Vec<usize> = t.blocks.iter().map(|b| b.len()).collect();
        assert_eq!(counts, after_counts, "each block keeps its prize count");
        let mut after: Vec<PrizeRecord> = t.blocks.iter().flatten().copied().collect();
        before.sort_by_key(|r| (r.item_id, r.price));
        after.sort_by_key(|r| (r.item_id, r.price));
        assert_eq!(before, after, "shuffle preserves the full record multiset");
    }

    #[test]
    fn random_draws_only_from_pool_and_keeps_price() {
        let (buf, base) = synth(&[&[0xD0, 0xE7], &[0xEC, 0xC4, 0x80]]);
        let mut t = CasinoExchange::parse(&buf, base, 2).unwrap();
        let pool: std::collections::HashSet<u16> =
            t.blocks.iter().flatten().map(|r| r.item_id).collect();
        t.randomize(0x9, DropMode::Random);
        for rec in t.blocks.iter().flatten() {
            assert!(pool.contains(&rec.item_id));
            assert_eq!(
                rec.price,
                rec.item_id as u32 * 10,
                "price travels with prize"
            );
        }
    }

    #[test]
    fn write_back_same_size_round_trips() {
        let (buf, base) = synth(&[&[0xD0, 0xE7, 0xC1], &[0xEC, 0xC4]]);
        let mut t = CasinoExchange::parse(&buf, base, 2).unwrap();
        t.randomize(0x5, DropMode::Shuffle);
        let mut out = buf.clone();
        t.write_back(&mut out);
        assert_eq!(out.len(), buf.len());
        let reparsed = CasinoExchange::parse(&out, base, 2).unwrap();
        assert_eq!(reparsed.blocks, t.blocks);
    }
}
