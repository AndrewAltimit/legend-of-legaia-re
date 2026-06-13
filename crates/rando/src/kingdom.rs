//! Map each field scene (a PROT entry) to the macro world-area — the in-game
//! **kingdom** — it belongs to: Drake Kingdom, Sebucus Islands, or Karisto
//! Kingdom. This is the grouping the scoped encounter randomizer uses to keep a
//! "within a region" shuffle from leaking late-game monsters into the opening
//! kingdom (and, inversely, to let a "across regions" shuffle do exactly that).
//!
//! **Source of truth: the disc's own `CDNAME.TXT`.** The scene namespace is
//! ordered by kingdom, and each kingdom's overworld (`map01` / `map02` /
//! `map03`) is a pinned world-map bundle (see
//! `docs/formats/world-map-overlay.md`). The CDNAME block order is, in raw-TOC
//! index space:
//!
//! ```text
//!   town01(3) .. suimon(77)    Drake field scenes
//!   map01(85)                  Drake overworld          <- pinned anchor
//!   garmel(94) .. tunnela(235) Sebucus field scenes
//!   map02(244)                 Sebucus overworld        <- pinned anchor
//!   tower(254) .. deene(382)   Karisto field scenes
//!   map03(391)                 Karisto overworld
//!   doman(399) .. bubu2(416)   Karisto late dungeons (after the overworld)
//! ```
//!
//! So the kingdom boundaries are anchored on `map01` / `map02` (never on a
//! hardcoded field-scene name): **Sebucus begins at the first CDNAME block
//! after `map01`; Karisto begins at the first block after `map02`** and absorbs
//! everything past it (including the post-`map03` dungeons, which have no
//! following overworld marker). No game bytes are embedded — every threshold is
//! read from the user's disc at runtime.

use legaia_prot::cdname::{self, IndexMap, RAW_TOC_INDEX_OFFSET};

/// The three macro world-areas a field scene can belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kingdom {
    /// Drake Kingdom — the opening region (Rim Elm, the first caves, `map01`).
    Drake,
    /// Sebucus Islands — the middle region (begins at the block after `map01`).
    Sebucus,
    /// Karisto Kingdom — the final region (begins at the block after `map02`;
    /// includes the late dungeons listed after the `map03` overworld marker).
    Karisto,
}

impl Kingdom {
    /// Stable lowercase identifier, for manifests / CLI reporting.
    pub fn as_str(self) -> &'static str {
        match self {
            Kingdom::Drake => "drake",
            Kingdom::Sebucus => "sebucus",
            Kingdom::Karisto => "karisto",
        }
    }

    /// A small distinct tag used to perturb a per-kingdom RNG seed so each
    /// kingdom's scoped shuffle is independent yet reproducible.
    pub(crate) fn seed_tag(self) -> u64 {
        match self {
            Kingdom::Drake => 1,
            Kingdom::Sebucus => 2,
            Kingdom::Karisto => 3,
        }
    }

    /// All three kingdoms, in world order.
    pub fn all() -> [Kingdom; 3] {
        [Kingdom::Drake, Kingdom::Sebucus, Kingdom::Karisto]
    }
}

/// Raw-TOC index thresholds that partition the PROT scene namespace into the
/// three kingdoms. Built once from a parsed `CDNAME.TXT`.
#[derive(Debug, Clone, Copy)]
pub struct KingdomMap {
    /// First raw-TOC index belonging to Sebucus (= the block right after
    /// `map01`).
    sebucus_start_raw: u32,
    /// First raw-TOC index belonging to Karisto (= the block right after
    /// `map02`).
    karisto_start_raw: u32,
}

impl KingdomMap {
    /// Build the partition from a parsed CDNAME map. Returns `None` if the disc
    /// doesn't declare both `map01` and `map02` (so the anchors are missing) or
    /// if their block order is inverted — in either case the caller should fall
    /// back to world-scope (one global pool), which never needs the partition.
    pub fn from_cdname(map: &IndexMap) -> Option<Self> {
        let next_block_after = |anchor: u32| -> Option<u32> {
            map.range(anchor.saturating_add(1)..)
                .next()
                .map(|(k, _)| *k)
        };
        let map01 = index_of(map, "map01")?;
        let map02 = index_of(map, "map02")?;
        let sebucus_start_raw = next_block_after(map01)?;
        let karisto_start_raw = next_block_after(map02)?;
        if !(map01 < sebucus_start_raw && sebucus_start_raw <= map02 && map02 < karisto_start_raw) {
            return None;
        }
        Some(Self {
            sebucus_start_raw,
            karisto_start_raw,
        })
    }

    /// Which kingdom the PROT **extraction** entry `entry_idx` falls in. The
    /// CDNAME `#define` numbers are raw-TOC indices, so the extraction index is
    /// converted to raw space (`+ RAW_TOC_INDEX_OFFSET`) before comparing
    /// against the thresholds (see [`cdname::block_for_extraction_index`]).
    ///
    /// Every entry below the Sebucus threshold is Drake — including the handful
    /// of non-field entries before the first town, which simply never carry an
    /// encounter table and so never enter a pool.
    pub fn kingdom_for_extraction_index(&self, entry_idx: usize) -> Kingdom {
        let raw = (entry_idx as u32).saturating_add(RAW_TOC_INDEX_OFFSET);
        if raw >= self.karisto_start_raw {
            Kingdom::Karisto
        } else if raw >= self.sebucus_start_raw {
            Kingdom::Sebucus
        } else {
            Kingdom::Drake
        }
    }
}

/// First raw index whose declared block name is exactly `name`.
fn index_of(map: &IndexMap, name: &str) -> Option<u32> {
    cdname::block_range_for_name(map, name).map(|(start, _)| start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_prot::cdname::parse_str;

    /// A trimmed CDNAME that reproduces the real disc's kingdom layout: field
    /// scenes, then each kingdom's overworld marker, with Karisto continuing
    /// past `map03`.
    fn sample_map() -> IndexMap {
        parse_str(
            "#define town01 3\n\
             #define suimon 77\n\
             #define map01 85\n\
             #define garmel 94\n\
             #define tunnela 235\n\
             #define map02 244\n\
             #define tower 254\n\
             #define deene 382\n\
             #define map03 391\n\
             #define doman 399\n\
             #define bubu2 416\n",
        )
        .unwrap()
    }

    /// Convert a scene NAME to its extraction-entry index (raw − offset) so the
    /// tests can assert kingdom membership by the names a human recognises.
    fn ext_index_of(map: &IndexMap, name: &str) -> usize {
        (index_of(map, name).unwrap() - RAW_TOC_INDEX_OFFSET) as usize
    }

    #[test]
    fn anchors_partition_the_three_kingdoms() {
        let map = sample_map();
        let km = KingdomMap::from_cdname(&map).unwrap();
        let kingdom = |name: &str| km.kingdom_for_extraction_index(ext_index_of(&map, name));

        // Drake: opening field scenes and the Drake overworld itself.
        assert_eq!(kingdom("town01"), Kingdom::Drake);
        assert_eq!(kingdom("suimon"), Kingdom::Drake);
        assert_eq!(kingdom("map01"), Kingdom::Drake);
        // Sebucus: starts at the first block after map01 (garmel), through map02.
        assert_eq!(kingdom("garmel"), Kingdom::Sebucus);
        assert_eq!(kingdom("tunnela"), Kingdom::Sebucus);
        assert_eq!(kingdom("map02"), Kingdom::Sebucus);
        // Karisto: starts at the first block after map02 (tower) and absorbs
        // everything past map03, including the late dungeons.
        assert_eq!(kingdom("tower"), Kingdom::Karisto);
        assert_eq!(kingdom("deene"), Kingdom::Karisto);
        assert_eq!(kingdom("map03"), Kingdom::Karisto);
        assert_eq!(kingdom("doman"), Kingdom::Karisto);
        assert_eq!(kingdom("bubu2"), Kingdom::Karisto);
    }

    #[test]
    fn missing_anchors_yield_no_partition() {
        // No map01/map02 -> caller must fall back to world scope.
        let map = parse_str("#define town01 3\n#define cave01 38\n").unwrap();
        assert!(KingdomMap::from_cdname(&map).is_none());
    }

    #[test]
    fn entries_before_first_town_are_drake() {
        let map = sample_map();
        let km = KingdomMap::from_cdname(&map).unwrap();
        // Extraction index 0/1 (raw 2/3) are below the Sebucus threshold.
        assert_eq!(km.kingdom_for_extraction_index(0), Kingdom::Drake);
    }
}
