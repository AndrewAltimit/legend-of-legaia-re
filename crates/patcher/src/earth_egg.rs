//! **Earth Egg coin-threshold edit** (the Sol Tower "Prize Counter").
//!
//! The Earth Ra-Seru Egg is *not* a row in the four-block casino
//! prize-exchange table ([`crate::casino`]) - it is a **bespoke scripted
//! exchange** in the `koin1` scene's field-VM script (the MAN, asset type
//! `0x03` of that scene's [`legaia_asset::scene_asset_table`] bundle; retail
//! extraction entry 543). Talking to the prize girl runs a partition-1
//! interaction script that, when the casino-coin bank exceeds a threshold,
//! offers "exchange 100,000 tokens for the Earth Ra-Seru Egg?"; on *Yes* it
//! gives item [`EARTH_EGG_ITEM_ID`] (`GIVE_ITEM` op `0x39 0x6E`), debits the
//! coins, and latches a "already redeemed" SYSTEM flag.
//!
//! ## The two literals (both verified from the disc, not guessed)
//!
//! - **Threshold gate** - field-VM op `0x4E` INVENTORY_CMP **sub-op 11** (the
//!   9-byte *coin* u32 compare; sub-op `>> 4` of the mode byte selects the coin
//!   bank `_DAT_800845A4`). Encoding
//!   `[4E, 00, mode, lo1, hi1, skip_lo, skip_hi, lo2, hi2]`; the compared value
//!   is `LE16(lo1,hi1) | (LE16(lo2,hi2) << 16)` - so the u32 is split into two
//!   u16 halves **straddling** the 16-bit skip-delta, at op-relative offsets
//!   `+3` and `+7`. Retail value is **99999**: the compare is `cmp == 1`
//!   (`stored < coins`), so the Earth Egg is offered only when
//!   `coins > 99999`, i.e. `coins >= 100000`. (Decoder
//!   [`legaia_asset::field_disasm`] `decode_inventory_cmp`; VM semantics
//!   `legaia_engine_vm` `flow::op_4e`.)
//! - **Coin debit** - field-VM op `0x4C` MENU_CTRL **nibble-E sub-5** (the
//!   add-coins hook onto `_DAT_800845A4`; the menuctrl doc's "XP add" label is
//!   the stale one - the engine port `menu_ctrl::nibble_e` adds coins).
//!   Encoding `[4C, E5, u24]` where the u24 is a **sign-extended 24-bit** delta:
//!   retail `60 79 FE` = `0xFE7960` = **-100000**.
//!
//! Retail keeps the invariant **gate = price - 1** and **debit = price**
//! (99999 / 100000). This module edits *both* together so a repriced Earth Egg
//! stays coherent: requiring `N` coins and removing exactly `N`. The "price" a
//! caller supplies is the **coins required** (retail = [`RETAIL_PRICE`] =
//! 100000).
//!
//! ## Same-size in place
//!
//! Both edits overwrite value bytes only (no length change), so the decompressed
//! MAN stays byte-for-byte the same size; the MAN is then LZS-recompressed and
//! written back exactly like the [chest](crate::chest) / [shop](crate::shop)
//! paths. The `koin1` MAN is sector-aligned with **zero** compressed slack, but
//! our LZS re-packer is a touch tighter than the retail one (it fits with room
//! to spare), so [`EarthEggExchange::repack`] returns `None` only if it ever
//! would overflow - never silently corrupting the neighbouring asset. No Sony
//! bytes are embedded: the module only rewrites integer fields the user's own
//! disc already holds.

use anyhow::{Result, bail};

use legaia_asset::scene_asset_table;

/// Item id of the Earth Ra-Seru Egg in the shared 256-entry item-name space.
pub const EARTH_EGG_ITEM_ID: u8 = 0x6E;

/// Retail coins required to redeem the Earth Egg (gate 99999 + 1).
pub const RETAIL_PRICE: u32 = 100_000;

/// Upper bound on a new price. The debit is a **signed 24-bit** field, so
/// `-price` must fit `i24` (`>= -0x80_0000`); prices above this can't be
/// represented as a coin debit. (The coin bank itself caps at 9,999,999, but
/// the debit field is the binding constraint.)
pub const MAX_PRICE: u32 = 0x80_0000; // 8,388,608

/// The MAN asset-type byte in a [`scene_asset_table`] bundle.
const MAN_TYPE: u8 = 0x03;

/// Sign-extend a 24-bit value to `i32`.
fn sext24(v: u32) -> i32 {
    let v = v & 0x00FF_FFFF;
    if v & 0x0080_0000 != 0 {
        (v | 0xFF00_0000) as i32
    } else {
        v as i32
    }
}

/// Read-only view of the Earth Egg exchange for listing / UX.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarthEggInfo {
    /// PROT entry index of the scene bundle holding the exchange.
    pub entry_idx: usize,
    /// Coins required to redeem the Earth Egg (= threshold + 1).
    pub price: u32,
    /// Raw threshold value in the op-`0x4E` sub-11 compare (retail 99999).
    pub threshold: u32,
    /// Coin-debit magnitude in the op-`0x4C` nibble-E sub-5 op (retail 100000).
    pub debit: u32,
    /// The granted item id (always [`EARTH_EGG_ITEM_ID`]).
    pub item_id: u8,
}

/// A located Earth Egg exchange: the scene bundle's decoded MAN plus the byte
/// offsets of the coin-threshold compare and the coin debit inside it. Mutate
/// via [`Self::set_price`], then [`Self::repack`] to a same-footprint LZS stream.
#[derive(Debug, Clone)]
pub struct EarthEggExchange {
    /// PROT entry index of the scene bundle.
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Bytes the recompressed MAN must fit within (the descriptor boundary).
    pub compressed_budget: usize,
    /// Decompressed MAN.
    pub decoded: Vec<u8>,
    /// Offset of the op-`0x4E` INVENTORY_CMP sub-11 coin compare in `decoded`.
    pub compare_off: usize,
    /// Current threshold value (`coins > threshold` unlocks the offer).
    pub threshold: u32,
    /// Offset of the op-`0x4C` nibble-E sub-5 coin-debit op in `decoded`.
    pub debit_off: usize,
    /// Current coin-debit magnitude.
    pub debit: u32,
}

impl EarthEggExchange {
    /// Locate the Earth Egg exchange inside a single PROT entry, or `None` if
    /// the entry isn't the scene bundle that carries it.
    ///
    /// Positive identification triangulates three independent signals in the one
    /// decoded MAN, so no coincidental byte run can match:
    /// 1. a `GIVE_ITEM 0x39 0x6E` (Earth Egg) is present;
    /// 2. an op-`0x4E` sub-11 **coin** compare (`4E 00 <mode|0xB0> …`, `cmp==1`)
    ///    exists, giving the threshold `T`;
    /// 3. a `4C E5 <u24>` coin debit exists whose magnitude is exactly `T + 1`
    ///    (the retail gate = price - 1 / debit = price invariant).
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
        let (decoded, _consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        // (1) The Earth Egg give must be present.
        if !decoded.windows(2).any(|w| w == [0x39, EARTH_EGG_ITEM_ID]) {
            return None;
        }
        // (2) + (3): find a coin compare whose threshold has a matching debit.
        for compare_off in find_coin_compares(&decoded) {
            let threshold = read_threshold(&decoded, compare_off);
            let want = threshold.checked_add(1)?;
            if let Some(debit_off) = find_coin_debit(&decoded, want) {
                return Some(Self {
                    entry_idx,
                    man_offset,
                    compressed_budget: crate::man_compressed_budget(
                        &table,
                        man_offset,
                        entry.len(),
                    ),
                    decoded,
                    compare_off,
                    threshold,
                    debit_off,
                    debit: want,
                });
            }
        }
        None
    }

    /// Coins currently required to redeem the Earth Egg (`threshold + 1`).
    pub fn price(&self) -> u32 {
        self.threshold + 1
    }

    /// A read-only info view.
    pub fn info(&self) -> EarthEggInfo {
        EarthEggInfo {
            entry_idx: self.entry_idx,
            price: self.price(),
            threshold: self.threshold,
            debit: self.debit,
            item_id: EARTH_EGG_ITEM_ID,
        }
    }

    /// Rewrite both literals to require `new_price` coins: threshold =
    /// `new_price - 1` (the `coins > threshold` gate), debit = `new_price`.
    /// Caller must have validated the range via [`validate_price`].
    pub fn set_price(&mut self, new_price: u32) {
        let threshold = new_price - 1;
        // Threshold u32 split: low half at compare+3, high half at compare+7.
        self.decoded[self.compare_off + 3] = (threshold & 0xFF) as u8;
        self.decoded[self.compare_off + 4] = ((threshold >> 8) & 0xFF) as u8;
        self.decoded[self.compare_off + 7] = ((threshold >> 16) & 0xFF) as u8;
        self.decoded[self.compare_off + 8] = ((threshold >> 24) & 0xFF) as u8;
        self.threshold = threshold;
        // Debit u24 = -new_price (sign-extended 24-bit).
        let d = ((-(new_price as i64)) as u32) & 0x00FF_FFFF;
        self.decoded[self.debit_off + 2] = (d & 0xFF) as u8;
        self.decoded[self.debit_off + 3] = ((d >> 8) & 0xFF) as u8;
        self.decoded[self.debit_off + 4] = ((d >> 16) & 0xFF) as u8;
        self.debit = new_price;
    }

    /// Recompress the (possibly edited) MAN; `None` if it would overflow the
    /// zero-slack footprint.
    pub fn repack(&self) -> Option<Vec<u8>> {
        let stream = legaia_lzs::compress(&self.decoded);
        (stream.len() <= self.compressed_budget).then_some(stream)
    }
}

/// Op-relative read of the split threshold u32 in an op-`0x4E` sub-11 compare.
fn read_threshold(man: &[u8], off: usize) -> u32 {
    let lo = u16::from_le_bytes([man[off + 3], man[off + 4]]) as u32;
    let hi = u16::from_le_bytes([man[off + 7], man[off + 8]]) as u32;
    lo | (hi << 16)
}

/// Offsets of every op-`0x4E` INVENTORY_CMP **sub-op 11** (coin u32) compare
/// with compare mode `1` (`stored < coins`). Structural, not a raw byte hunt:
/// `4E 00` lead, `mode >> 4 == 0x0B`, `mode & 0x0F == 0x01`, 9 bytes in bounds.
fn find_coin_compares(man: &[u8]) -> Vec<usize> {
    let mut out = Vec::new();
    if man.len() < 9 {
        return out;
    }
    for i in 0..=man.len() - 9 {
        if man[i] == 0x4E && man[i + 1] == 0x00 {
            let mode = man[i + 2];
            if mode >> 4 == 0x0B && mode & 0x0F == 0x01 {
                out.push(i);
            }
        }
    }
    out
}

/// Offset of the first `4C E5 <u24>` coin-debit op whose signed-24 magnitude is
/// exactly `-want` (i.e. debits `want` coins).
fn find_coin_debit(man: &[u8], want: u32) -> Option<usize> {
    if man.len() < 5 {
        return None;
    }
    (0..=man.len() - 5).find(|&i| {
        man[i] == 0x4C && man[i + 1] == 0xE5 && {
            let u24 = man[i + 2] as u32 | ((man[i + 3] as u32) << 8) | ((man[i + 4] as u32) << 16);
            sext24(u24) == -(want as i32)
        }
    })
}

/// Guard a requested price: must be `1..=MAX_PRICE`. `0` is rejected (a
/// threshold of `-1` is meaningless) and anything above [`MAX_PRICE`] can't be
/// represented as the signed-24-bit coin debit.
pub fn validate_price(new_price: u32) -> Result<()> {
    if new_price == 0 {
        bail!("earth-egg price must be at least 1 coin");
    }
    if new_price > MAX_PRICE {
        bail!("earth-egg price {new_price} exceeds the max representable debit ({MAX_PRICE})");
    }
    Ok(())
}

/// One planned Earth Egg price edit: the located exchange with its price already
/// set to the new value (ready to [`EarthEggExchange::repack`] + write back), the
/// prior price, and the new price. Distinct offsets are inside the exchange.
#[derive(Debug, Clone)]
pub struct EarthEggEdit {
    /// The exchange with `set_price(new_price)` already applied.
    pub exchange: EarthEggExchange,
    /// Prior coins-required.
    pub old_price: u32,
    /// New coins-required.
    pub new_price: u32,
}

/// Plan an Earth Egg price change for one PROT entry.
///
/// Refuses **absent** (the entry doesn't carry the exchange) and **out of
/// range** (`0` or `> MAX_PRICE`) with `Err`; returns `Ok(None)` when the price
/// already matches (idempotent no-op).
pub fn plan_set_price(
    entry: &[u8],
    entry_idx: usize,
    new_price: u32,
) -> Result<Option<EarthEggEdit>> {
    validate_price(new_price)?;
    let mut exchange = EarthEggExchange::locate(entry, entry_idx)
        .ok_or_else(|| anyhow::anyhow!("Earth Egg exchange not found in PROT entry {entry_idx}"))?;
    let old_price = exchange.price();
    if old_price == new_price {
        return Ok(None);
    }
    exchange.set_price(new_price);
    Ok(Some(EarthEggEdit {
        exchange,
        old_price,
        new_price,
    }))
}

/// Read-only: the current Earth Egg exchange info for a PROT entry, or `None`
/// if the entry doesn't carry it.
pub fn list_price(entry: &[u8], entry_idx: usize) -> Option<EarthEggInfo> {
    EarthEggExchange::locate(entry, entry_idx).map(|e| e.info())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A synthetic MAN carrying the exact op shapes: a coin compare (threshold
    /// 99999), a matching -100000 debit, and a `39 6E` give. No scene-bundle
    /// wrapper (those paths are covered by the disc-gated `earth_egg_real`
    /// oracle); this exercises the locate/edit arithmetic on the decoded MAN.
    fn synth_man(threshold: u32, debit: u32) -> Vec<u8> {
        let mut m = Vec::new();
        // give Earth Egg
        m.extend_from_slice(&[0x39, EARTH_EGG_ITEM_ID]);
        // some filler
        m.extend_from_slice(&[0x21, 0x21, 0x21]);
        // coin compare: 4E 00 B1 lo1 hi1 skip_lo skip_hi lo2 hi2
        let lo1 = (threshold & 0xFFFF) as u16;
        let hi2 = ((threshold >> 16) & 0xFFFF) as u16;
        m.extend_from_slice(&[0x4E, 0x00, 0xB1]);
        m.extend_from_slice(&lo1.to_le_bytes());
        m.extend_from_slice(&[0xE2, 0x00]); // skip delta (must survive edits)
        m.extend_from_slice(&hi2.to_le_bytes());
        // debit: 4C E5 u24(-debit)
        let d = ((-(debit as i64)) as u32) & 0x00FF_FFFF;
        m.extend_from_slice(&[
            0x4C,
            0xE5,
            (d & 0xFF) as u8,
            ((d >> 8) & 0xFF) as u8,
            ((d >> 16) & 0xFF) as u8,
        ]);
        m
    }

    fn locate_synth(man: Vec<u8>) -> EarthEggExchange {
        // Build an EarthEggExchange bypassing the scene wrapper by hand.
        let compare_off = man
            .windows(3)
            .position(|w| w == [0x4E, 0x00, 0xB1])
            .unwrap();
        let threshold = read_threshold(&man, compare_off);
        let debit_off = find_coin_debit(&man, threshold + 1).unwrap();
        EarthEggExchange {
            entry_idx: 0,
            man_offset: 0x40,
            compressed_budget: usize::MAX,
            decoded: man,
            compare_off,
            threshold,
            debit_off,
            debit: threshold + 1,
        }
    }

    #[test]
    fn reads_retail_shape() {
        let e = locate_synth(synth_man(99999, 100000));
        assert_eq!(e.threshold, 99999);
        assert_eq!(e.debit, 100000);
        assert_eq!(e.price(), 100000);
    }

    #[test]
    fn coin_compare_and_debit_are_found_and_paired() {
        let man = synth_man(99999, 100000);
        let compares = find_coin_compares(&man);
        assert_eq!(compares.len(), 1);
        assert_eq!(read_threshold(&man, compares[0]), 99999);
        assert_eq!(find_coin_debit(&man, 100000).unwrap(), man.len() - 5);
        // A debit whose magnitude isn't the paired value is not matched.
        assert!(find_coin_debit(&man, 12345).is_none());
    }

    #[test]
    fn set_price_rewrites_both_literals_and_preserves_skip_delta() {
        let mut e = locate_synth(synth_man(99999, 100000));
        let skip_before = [e.decoded[e.compare_off + 5], e.decoded[e.compare_off + 6]];
        e.set_price(250_000);
        assert_eq!(e.threshold, 249_999);
        assert_eq!(e.debit, 250_000);
        assert_eq!(e.price(), 250_000);
        // Skip-delta (op+5,+6) untouched.
        assert_eq!(
            [e.decoded[e.compare_off + 5], e.decoded[e.compare_off + 6]],
            skip_before
        );
        // Re-read the threshold from the bytes and the debit u24.
        assert_eq!(read_threshold(&e.decoded, e.compare_off), 249_999);
        let u24 = e.decoded[e.debit_off + 2] as u32
            | ((e.decoded[e.debit_off + 3] as u32) << 8)
            | ((e.decoded[e.debit_off + 4] as u32) << 16);
        assert_eq!(sext24(u24), -250_000);
    }

    #[test]
    fn validate_rejects_zero_and_overflow() {
        assert!(validate_price(0).is_err());
        assert!(validate_price(MAX_PRICE + 1).is_err());
        assert!(validate_price(1).is_ok());
        assert!(validate_price(RETAIL_PRICE).is_ok());
        assert!(validate_price(MAX_PRICE).is_ok());
    }

    #[test]
    fn sext24_round_trips_negative() {
        assert_eq!(sext24(0xFE7960), -100000);
        assert_eq!(sext24(0xFFFFFF), -1);
        assert_eq!(sext24(0x000000), 0);
        assert_eq!(sext24(0x7FFFFF), 0x7FFFFF);
        assert_eq!(sext24(0x800000), -8_388_608);
    }
}
