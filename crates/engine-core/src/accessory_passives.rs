//! Accessory ("Goods") passive-effect catalog: equipped-item-id → passive
//! index resolution and per-character ability-bit derivation.
//!
//! REF: FUN_80042558
//!
//! The retail per-frame stat aggregator (`FUN_80042558`,
//! `ghidra/scripts/funcs/80042558.txt`) walks each active party member's
//! eight equipment-slot bytes, resolves every equipped item to a 64-slot
//! passive-effect index (`legaia_asset::accessory_passive`: descriptor `+3`
//! byte for accessories, equip-record `+5` byte for equipment - both
//! sentinel-gated at `< 0x40`), and sets bit `index` in the character's
//! 4×`u32` ability bitfield at record `+0xF4`. The four words are then OR'd
//! across the party into the global mask at `DAT_80074358` (bit-tested by
//! `FUN_800431D0`), which is how party-wide-scoped passives (gold / AP /
//! encounter / escape modifiers) are consumed.
//!
//! This type is the engine's table side of that mechanism: it owns the
//! item-id → passive-index map plus the per-index party-wide scope flags,
//! both decoded from the user's `SCUS_942.54`
//! ([`legaia_asset::accessory_passive::AccessoryPassiveTable`]). The bit
//! *derivation* ([`AccessoryPassives::bits_for_equipment`]) mirrors the
//! retail loop exactly; the stat-percent rebuild lives in
//! [`crate::battle_stats::compute_battle_stats_with_passives`] and the
//! global mask in `World::party_ability_mask` /
//! `World::refresh_party_ability_bits`.

use std::collections::HashMap;

/// Number of `u32` words in the ability bitfield (record `+0xF4..+0x103`
/// and the global `DAT_80074358..` mask).
pub const ABILITY_WORDS: usize = 4;

/// Item-id → passive-index catalog plus per-index scope flags. Built once
/// from the executable at boot ([`AccessoryPassives::from_disc`]); empty by
/// default (no item grants a passive), which keeps disc-free / synthetic
/// worlds byte-identical to their pre-accessory behaviour.
#[derive(Debug, Clone, Default)]
pub struct AccessoryPassives {
    /// `item id → passive index` (already sentinel-filtered: every stored
    /// index is `< 0x40`).
    passive_by_item: HashMap<u8, u8>,
    /// Bit `index` set when passive `index` is party-wide scoped (the
    /// `0x8007625C` record `+0` scope word is `1`). Upper words are always
    /// zero (the index space is 64 slots) but the array keeps the runtime
    /// mask shape.
    party_wide: [u32; ABILITY_WORDS],
}

impl AccessoryPassives {
    /// Build from the parsed static tables. Indexes every item id that
    /// resolves to a live passive (`passive_index` is already `< 0x40`) and
    /// flags the party-wide-scoped indices from the record scope words.
    pub fn from_disc(table: &legaia_asset::accessory_passive::AccessoryPassiveTable) -> Self {
        let mut passive_by_item = HashMap::new();
        for id in 0u8..=u8::MAX {
            if let Some(idx) = table.passive_index(id) {
                passive_by_item.insert(id, idx);
            }
        }
        let mut party_wide = [0u32; ABILITY_WORDS];
        for idx in 0..legaia_asset::accessory_passive::PASSIVE_COUNT as u8 {
            if table.record(idx).is_some_and(|r| r.party_wide()) {
                let (w, mask) = legaia_asset::accessory_passive::bit_location(idx);
                party_wide[w] |= mask;
            }
        }
        Self {
            passive_by_item,
            party_wide,
        }
    }

    /// Build from explicit `(item id, passive index)` pairs + a list of
    /// party-wide-scoped indices. For engines that source the data elsewhere,
    /// and for tests. Indices `>= 0x40` are dropped (the sentinel rule).
    pub fn from_entries(
        items: impl IntoIterator<Item = (u8, u8)>,
        party_wide_indices: impl IntoIterator<Item = u8>,
    ) -> Self {
        let passive_by_item = items
            .into_iter()
            .filter(|&(_, idx)| idx < legaia_asset::accessory_passive::NO_PASSIVE)
            .collect();
        let mut party_wide = [0u32; ABILITY_WORDS];
        for idx in party_wide_indices {
            if idx < legaia_asset::accessory_passive::NO_PASSIVE {
                let (w, mask) = legaia_asset::accessory_passive::bit_location(idx);
                party_wide[w] |= mask;
            }
        }
        Self {
            passive_by_item,
            party_wide,
        }
    }

    /// The passive index `item_id` grants while equipped, or `None`.
    pub fn passive_index(&self, item_id: u8) -> Option<u8> {
        self.passive_by_item.get(&item_id).copied()
    }

    /// The 4×`u32` ability bitfield a character's eight equipment slots
    /// derive - the bit-resolution arm of `FUN_80042558`: every equipped
    /// item's passive index becomes bit `index & 0x1F` of word `index >> 5`.
    /// Empty slots (id `0`) and items without a passive contribute nothing.
    pub fn bits_for_equipment(&self, equip: &[u8; 8]) -> [u32; ABILITY_WORDS] {
        let mut words = [0u32; ABILITY_WORDS];
        for &id in equip {
            if id == 0 {
                continue;
            }
            if let Some(idx) = self.passive_index(id) {
                let (w, mask) = legaia_asset::accessory_passive::bit_location(idx);
                words[w] |= mask;
            }
        }
        words
    }

    /// Mask of the party-wide-scoped passive indices (one wearer benefits
    /// the whole party), in the same word/bit layout as the ability field.
    pub fn party_wide_mask(&self) -> [u32; ABILITY_WORDS] {
        self.party_wide
    }

    /// `true` when no item grants a passive (the disc-free default). The
    /// world's ability-bit refresh no-ops on an empty catalog so synthetic
    /// setups that write `character_ability_bits` directly are not clobbered.
    pub fn is_empty(&self) -> bool {
        self.passive_by_item.is_empty()
    }

    /// Number of item ids that grant a passive.
    pub fn len(&self) -> usize {
        self.passive_by_item.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_for_equipment_sets_word_and_bit_per_item() {
        let p = AccessoryPassives::from_entries([(0xC5, 0x05), (0xF0, 0x30)], [0x30]);
        // Slot position is irrelevant - retail walks all 8 bytes.
        let words = p.bits_for_equipment(&[0, 0, 0, 0, 0, 0xF0, 0, 0xC5]);
        assert_eq!(words[0], 0x20, "index 0x05 -> word 0 bit 5");
        assert_eq!(words[1], 0x1_0000, "index 0x30 -> word 1 bit 16");
        assert_eq!(words[2], 0);
        assert_eq!(words[3], 0);
    }

    #[test]
    fn empty_slots_and_passive_less_items_contribute_nothing() {
        let p = AccessoryPassives::from_entries([(0xC0, 0x00)], []);
        assert_eq!(p.bits_for_equipment(&[0; 8]), [0; ABILITY_WORDS]);
        // Item 0x22 grants no passive.
        assert_eq!(p.bits_for_equipment(&[0x22; 8]), [0; ABILITY_WORDS]);
        assert_eq!(
            p.bits_for_equipment(&[0xC0, 0, 0, 0, 0, 0, 0, 0]),
            [1, 0, 0, 0]
        );
    }

    #[test]
    fn sentinel_indices_are_dropped_at_build() {
        let p = AccessoryPassives::from_entries([(0x10, 0x40), (0x11, 0x41)], [0x40]);
        assert!(p.is_empty());
        assert_eq!(p.party_wide_mask(), [0; ABILITY_WORDS]);
    }

    #[test]
    fn party_wide_mask_round_trips() {
        let p = AccessoryPassives::from_entries([], [0x30, 0x3F, 0x04]);
        let m = p.party_wide_mask();
        assert_eq!(m[0], 0x10, "index 0x04");
        assert_eq!(m[1], 0x8001_0000, "indices 0x30 + 0x3F");
    }
}
