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
//! record validation ([`legaia_asset::shop_stock::parse_record`]): the byte after the opcode must be
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

/// One town-shop site located in a scene MAN: its declared item count, the
/// absolute offsets (within [`SceneShops::decoded`]) of each item-id byte, and
/// the shop's display name. The pure scanner lives in [`legaia_asset::shop_stock`]
/// (shared with the engine's read side); this is its alias.
pub use legaia_asset::shop_stock::ShopRecord as ShopSite;

/// Scan a decompressed MAN for every town-shop site (op `0x49` sub-op `0`). A
/// re-export of the shared scanner [`legaia_asset::shop_stock::scan`]; `valid`
/// optionally restricts shop ids to named items (the SCUS item mask).
pub use legaia_asset::shop_stock::scan as shop_sites;

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

    /// Like [`Self::locate`], but `valid` restricts shop ids to **sellable items**
    /// (a `256`-entry "id is priced `> 0`" mask from the SCUS item table), so a
    /// stray `0x49`-prefixed byte run can't be mistaken for a shop AND the
    /// trailing unsellable template-id padding is trimmed out of the stock (see
    /// [`legaia_asset::shop_stock`]). The apply layer always uses this form.
    pub fn locate_with_items(entry: &[u8], entry_idx: usize, valid: &[bool; 256]) -> Option<Self> {
        Self::locate_inner(entry, entry_idx, Some(valid))
    }

    fn locate_inner(entry: &[u8], entry_idx: usize, valid: Option<&[bool; 256]>) -> Option<Self> {
        // The decode + shop-record scan is the shared read side; the randomizer
        // adds only the recompress budget so a rewritten MAN can be bounded.
        let located = legaia_asset::shop_stock::locate(entry, valid)?;
        let table = scene_asset_table::detect(entry)?;
        Some(Self {
            entry_idx,
            man_offset: located.man_offset,
            compressed_budget: crate::man_compressed_budget(
                &table,
                located.man_offset,
                entry.len(),
            ),
            decoded: located.decoded,
            shops: located.records,
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

// The shop-record scanner (`scan` / `parse_record` / name reader) and its unit
// tests live in [`legaia_asset::shop_stock`]; `shop_sites` / `ShopSite` above
// re-export them. This module keeps only the randomizer-specific MAN
// locate-with-budget + repack wrapper.

#[cfg(test)]
mod tests {
    use super::*;

    /// The re-exported scanner finds an embedded shop even past arbitrary bytes
    /// (the randomizer relies on this for `SceneShops`). Full record-validation
    /// coverage lives in `legaia_asset::shop_stock`.
    #[test]
    fn shop_sites_reexport_scans() {
        let mut man = vec![0x2A, 0x0E, 0x00, 0x46, 0xFF, 0x1F, b'Y', 0x00];
        man.extend_from_slice(&[0x49, 0x00, 0x00, 0x02, 0x77, 0x7e]);
        man.extend_from_slice(b"Corey\0");
        let sites = shop_sites(&man, None);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].name, "Corey");
    }
}
