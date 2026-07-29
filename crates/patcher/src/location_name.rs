//! **Location / landmark name renaming.**
//!
//! A place name is shown in three places, and each reads its **own** copy of
//! the string off the disc. Renaming a town therefore means editing three
//! carriers, not one:
//!
//! | Site | Display | Carrier |
//! |---|---|---|
//! | 1 | quick-travel / Door-of-Wind destination list | `SCUS_942.54` `0x80073B18`, 16 fixed `0x20`-byte cells |
//! | 2 | the labels drawn over the world map at each place's map position | the **world-map location table** trailing every kingdom MAN (`map01` / `map02` / `map03`) |
//! | 3 | the banner on entering the scene (and the save-screen location row) | each scene MAN's **section 2** display name |
//!
//! Sites 2 and 3 are parsed by [`legaia_asset::place_names`], which carries
//! the byte layout and the provenance for both.
//!
//! ## What an edit costs
//!
//! - **Site 1** is a same-size overwrite of a 32-byte slot, zero-padded so no
//!   stale tail survives - the same mechanism the item / spell name tables use.
//! - **Site 2** is a same-size overwrite of a record's fixed 24-byte name
//!   field, then an LZS re-pack of the kingdom MAN.
//! - **Site 3** is *not* padded: the section body is exactly `strlen + 1`, so a
//!   longer name resizes the section. That is safe because every later section
//!   is reached by walking the chain, but it changes the MAN's decompressed
//!   size, so the scene bundle's descriptor size word is rewritten alongside
//!   the re-packed stream.
//!
//! The shared cap is therefore the tightest of the three, **23 characters**
//! ([`MAX_NAME_LEN`]) - the world-map record's 24-byte name field minus its
//! NUL. Retail's longest name ("Zora's Floating Castle") is 22.
//!
//! No Sony bytes are embedded; only the user's own disc strings are rewritten.
//!
//! The default table (element caves at idx 3/4, "Vidna" at 6, "Conkram" at 14)
//! is what an element-swap hack renames - e.g. rename "Ancient Wind Cave" to
//! "Ancient Fire Cave" to match a re-elemented party.

use anyhow::{Result, bail};

use legaia_asset::item_names::file_offset_for_va;
use legaia_asset::place_names::{
    self, PlaceNameError, SceneName, WORLD_MAP_NAME_CAPACITY, WorldMapTable,
};
use legaia_asset::scene_asset_table;
use legaia_asset::worldmap_menu::{NAME_COUNT, NAME_STRIDE, NAME_TABLE_ADDR};

/// Max bytes a renamed name can use across **all three** carriers: the
/// world-map record's 24-byte name field is the tightest of them, so 23
/// characters plus a terminating NUL. (The SCUS cell would hold 31, but a name
/// that only fits there would rename site 1 and desync the other two.)
pub const MAX_NAME_LEN: usize = WORLD_MAP_NAME_CAPACITY - 1;

/// The MAN asset-type byte in a [`scene_asset_table`] bundle.
const MAN_TYPE: u8 = 0x03;

/// One planned rename: the SCUS file offset of a name slot and the new 32-byte
/// slot contents (ASCII + NUL padding).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameEdit {
    /// Landmark index (0..16).
    pub index: usize,
    /// SCUS file offset of the slot.
    pub offset: usize,
    /// The prior name (decoded).
    pub old_name: String,
    /// The new name.
    pub new_name: String,
    /// The full 32-byte replacement slot (ASCII + zero padding).
    pub slot: [u8; NAME_STRIDE],
}

/// SCUS file offset of landmark slot `index`.
pub fn slot_offset(scus: &[u8], index: usize) -> Option<usize> {
    let va = NAME_TABLE_ADDR + (index * NAME_STRIDE) as u32;
    file_offset_for_va(scus, va)
}

/// Decode the current name of landmark `index` from its SCUS slot.
pub fn current_name(scus: &[u8], index: usize) -> Option<String> {
    let off = slot_offset(scus, index)?;
    let slot = scus.get(off..off + NAME_STRIDE)?;
    let end = slot.iter().position(|&b| b == 0).unwrap_or(NAME_STRIDE);
    Some(String::from_utf8_lossy(&slot[..end]).into_owned())
}

/// List all 16 landmark names (index, name), for UX.
pub fn list_names(scus: &[u8]) -> Result<Vec<(usize, String)>> {
    let mut out = Vec::with_capacity(NAME_COUNT);
    for i in 0..NAME_COUNT {
        let name = current_name(scus, i)
            .ok_or_else(|| anyhow::anyhow!("landmark name table not resolvable in SCUS"))?;
        out.push((i, name));
    }
    Ok(out)
}

/// Guard a replacement name against the shared three-carrier budget. Public so
/// the world-map / scene-banner paths refuse exactly what the SCUS path does.
pub fn validate_name(new_name: &str) -> Result<()> {
    if new_name.is_empty() {
        bail!("a location name may not be empty");
    }
    if new_name.len() > MAX_NAME_LEN {
        bail!(
            "name {new_name:?} is {} bytes; a location name holds at most {MAX_NAME_LEN} \
             (the world-map record's 24-byte name field, minus its NUL)",
            new_name.len()
        );
    }
    if !new_name.is_ascii() || new_name.bytes().any(|b| !(0x20..0x7F).contains(&b)) {
        bail!("name {new_name:?} has non-ASCII bytes (the menu font renders ASCII only here)");
    }
    Ok(())
}

/// Plan a rename of landmark `index` to `new_name`. Fails on an out-of-range
/// index, a name past [`MAX_NAME_LEN`], a non-ASCII name (the dialog font only
/// renders the ASCII set here), or an unresolvable table.
/// Returns `Ok(None)` when the name already matches (idempotent no-op).
pub fn plan_rename(scus: &[u8], index: usize, new_name: &str) -> Result<Option<RenameEdit>> {
    if index >= NAME_COUNT {
        bail!("landmark index {index} out of range (0..{NAME_COUNT})");
    }
    validate_name(new_name)?;
    let off = slot_offset(scus, index)
        .ok_or_else(|| anyhow::anyhow!("landmark slot {index} unresolvable"))?;
    let old_name = current_name(scus, index).unwrap_or_default();
    if old_name == new_name {
        return Ok(None);
    }
    let mut slot = [0u8; NAME_STRIDE];
    slot[..new_name.len()].copy_from_slice(new_name.as_bytes());
    Ok(Some(RenameEdit {
        index,
        offset: off,
        old_name,
        new_name: new_name.to_string(),
        slot,
    }))
}

/// A scene bundle's MAN together with whichever of the two MAN-resident place
/// name carriers it holds: the scene's own display name (section 2) and, for
/// the three kingdom bundles, the world-map location table (the section-5
/// trailer). Mutate through [`Self::rename`], then [`Self::repack`].
#[derive(Debug, Clone)]
pub struct ManPlaceNames {
    /// PROT entry index of the scene bundle.
    pub entry_idx: usize,
    /// Byte offset of the compressed MAN stream within the entry.
    pub man_offset: usize,
    /// Byte offset, within the entry, of the MAN descriptor's
    /// `(type<<24)|size` word. Rewritten whenever the decompressed size moves.
    pub man_descriptor_off: usize,
    /// Bytes the recompressed MAN may occupy: the gap to the next asset's
    /// data, clamped to the entry's **true on-disc footprint** so a growing
    /// MAN can use the entry's sector padding but never a neighbour's bytes.
    pub compressed_budget: usize,
    /// Decompressed MAN.
    pub decoded: Vec<u8>,
    /// Section-2 display name, when the MAN carries a printable one.
    pub scene_name: Option<SceneName>,
    /// The world-map location table, when this is a kingdom MAN.
    pub world_map: Option<WorldMapTable>,
}

/// What one [`ManPlaceNames::rename`] changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ManRenameCounts {
    /// `1` when the scene's own banner name was rewritten.
    pub scene_banners: usize,
    /// World-map location records rewritten.
    pub world_map_records: usize,
}

impl ManRenameCounts {
    /// `true` when nothing matched.
    pub fn is_empty(&self) -> bool {
        self.scene_banners == 0 && self.world_map_records == 0
    }
}

impl ManPlaceNames {
    /// Locate the place-name carriers in one PROT entry. `footprint` is the
    /// entry's true on-disc size in bytes (`DiscPatcher::entry_true_footprint_sectors`
    /// x 2048) - `entry` itself may be the over-reading indexed read, so the
    /// growth budget is clamped to the footprint rather than to `entry.len()`.
    ///
    /// `None` when the entry isn't a scene bundle, has no MAN, the MAN doesn't
    /// decode, or it carries neither place-name field.
    pub fn locate(entry: &[u8], entry_idx: usize, footprint: usize) -> Option<Self> {
        // The strict detector's count allow-list (6/7) excludes two count-5
        // bundles that do carry a MAN - one of them `bubu1`, half of Buma - so
        // fall back to the lenient walk the runtime itself uses. Everything
        // below re-confirms the find far more strongly than the header could:
        // the stream must LZS-decode to exactly its declared size, walk as a
        // MAN, and yield a printable place name.
        let descriptors = match scene_asset_table::detect(entry) {
            Some(table) => table.used().to_vec(),
            None => scene_asset_table::lenient_descriptor_walk(entry)?,
        };
        let man_idx = descriptors.iter().position(|d| d.type_byte == MAN_TYPE)?;
        let man = descriptors[man_idx];
        if man.size == 0 || man.data_offset == 0 {
            return None;
        }
        let man_offset = man.data_offset as usize;
        if man_offset >= footprint {
            return None;
        }
        let body = entry.get(man_offset..)?;
        let (decoded, consumed) = legaia_lzs::decompress_tracked(body, man.size as usize).ok()?;
        if decoded.len() != man.size as usize {
            return None;
        }
        let parsed = legaia_asset::man_section::parse(&decoded).ok()?;
        let scene_name = place_names::scene_name_of(&parsed, &decoded);
        let world_map = place_names::world_map_table_of(&parsed, &decoded);
        if scene_name.is_none() && world_map.is_none() {
            return None;
        }
        // Growth room: up to the next asset's data, else to the end of the
        // entry's own footprint (the tail is sector padding). Never below the
        // stream we actually consumed.
        let limit = descriptors
            .iter()
            .map(|d| d.data_offset as usize)
            .filter(|&o| o > man_offset)
            .min()
            .unwrap_or(footprint)
            .min(footprint);
        Some(Self {
            entry_idx,
            man_offset,
            man_descriptor_off: scene_asset_table::SceneAssetTable::size_word_offset(man_idx),
            compressed_budget: limit.saturating_sub(man_offset).max(consumed),
            decoded,
            scene_name,
            world_map,
        })
    }

    /// Every place name this MAN carries, for listing.
    pub fn names(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(s) = &self.scene_name {
            out.push(s.name.clone());
        }
        if let Some(t) = &self.world_map {
            out.extend(t.locations.iter().map(|l| l.name.clone()));
        }
        out
    }

    /// Rewrite every carrier whose current name is exactly `old_name`.
    /// Returns what changed; the buffer is left untouched when nothing matched.
    pub fn rename(
        &mut self,
        old_name: &str,
        new_name: &str,
    ) -> Result<ManRenameCounts, PlaceNameError> {
        let mut counts = ManRenameCounts::default();
        if let Some(table) = &self.world_map {
            let hits: Vec<_> = table
                .locations
                .iter()
                .filter(|l| l.name == old_name)
                .cloned()
                .collect();
            for loc in &hits {
                place_names::set_world_map_name(&mut self.decoded, loc, new_name)?;
                counts.world_map_records += 1;
            }
        }
        // The scene name resizes the buffer, so it goes last and every cached
        // offset is re-derived from the rebuilt bytes.
        if self.scene_name.as_ref().is_some_and(|s| s.name == old_name) {
            self.decoded = place_names::rewrite_scene_name(&self.decoded, new_name)?;
            counts.scene_banners += 1;
        }
        if !counts.is_empty() {
            self.rescan();
        }
        Ok(counts)
    }

    /// Re-walk the (possibly resized) buffer so the cached carrier offsets stay
    /// truthful after an edit.
    fn rescan(&mut self) {
        if let Ok(parsed) = legaia_asset::man_section::parse(&self.decoded) {
            self.scene_name = place_names::scene_name_of(&parsed, &self.decoded);
            self.world_map = place_names::world_map_table_of(&parsed, &self.decoded);
        }
    }

    /// Recompress the edited MAN. Returns `(stream, decompressed_size)`, or
    /// `None` when the stream would overflow the footprint - never a silent
    /// write into the neighbouring asset.
    pub fn repack(&self) -> Option<(Vec<u8>, u32)> {
        let stream = legaia_lzs::compress(&self.decoded);
        let size = self.decoded.len() as u32;
        (stream.len() <= self.compressed_budget).then_some((stream, size))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_asset::worldmap_menu::SCUS_LOAD_ADDR;

    /// Build a synthetic SCUS-EXE-shaped buffer with the name table populated.
    fn synthetic(names: &[&str]) -> Vec<u8> {
        // Enough buffer to cover the table's file offset.
        let table_off = 0x800 + (NAME_TABLE_ADDR - SCUS_LOAD_ADDR) as usize;
        let mut scus = vec![0u8; table_off + NAME_COUNT * NAME_STRIDE + 0x20];
        // Minimal PS-X EXE header so file_offset_for_va resolves: magic + t_addr
        // + t_size are what the resolver reads. Mirror worldmap_menu's mapping
        // (file = va - t_addr + 0x800). We fake it by writing the header fields.
        scus[0..8].copy_from_slice(b"PS-X EXE");
        scus[0x18..0x1C].copy_from_slice(&SCUS_LOAD_ADDR.to_le_bytes()); // t_addr
        // t_size must cover the table's file offset (file = va - t_addr + 0x800).
        let t_size = (scus.len() - 0x800) as u32;
        scus[0x1C..0x20].copy_from_slice(&t_size.to_le_bytes());
        for (i, n) in names.iter().enumerate() {
            let b = table_off + i * NAME_STRIDE;
            scus[b..b + n.len()].copy_from_slice(n.as_bytes());
        }
        scus
    }

    #[test]
    fn plan_rewrites_the_slot_and_zero_pads() {
        let scus = synthetic(&["Ancient Wind Cave", "Vidna"]);
        let edit = plan_rename(&scus, 0, "Ancient Fire Cave")
            .expect("valid")
            .expect("changed");
        assert_eq!(edit.old_name, "Ancient Wind Cave");
        assert_eq!(&edit.slot[..17], b"Ancient Fire Cave");
        // Everything past the name is zero (no stale tail).
        assert!(edit.slot[17..].iter().all(|&b| b == 0));
    }

    #[test]
    fn same_name_is_a_noop() {
        let scus = synthetic(&["Vidna"]);
        assert!(plan_rename(&scus, 0, "Vidna").unwrap().is_none());
    }

    #[test]
    fn refuses_too_long_non_ascii_and_oob() {
        let scus = synthetic(&["Vidna"]);
        assert!(plan_rename(&scus, 0, &"x".repeat(32)).is_err());
        assert!(plan_rename(&scus, 0, "Vïdna").is_err());
        assert!(plan_rename(&scus, NAME_COUNT, "X").is_err());
    }

    #[test]
    fn list_reads_all_slots() {
        let scus = synthetic(&["A", "B", "C"]);
        let names = list_names(&scus).unwrap();
        assert_eq!(names.len(), NAME_COUNT);
        assert_eq!(names[0].1, "A");
        assert_eq!(names[2].1, "C");
        assert_eq!(names[3].1, ""); // unpopulated slot decodes empty
    }
}
