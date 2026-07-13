//! Per-tile **floor-elevation overrides** - the `.MAP` kind-2 table the floor
//! sampler `FUN_80019278` consults for slope / stair tiles.
//!
//! A field tile's floor height normally comes from the collision grid's
//! low-nibble elevation tier, bilinearly interpolated across the tile's four
//! corner tiles (see [`World::sample_field_floor_height`]
//! (crate::world::World::sample_field_floor_height)). **Ramps and staircases
//! do not use that surface at all.** Their tiles set bit
//! [`CELL_ELEVATION_OVERRIDE`] (`0x800`) in the object-grid cell word
//! (`.MAP` `+0x8000`), and for those tiles retail replaces the whole bilinear
//! branch with
//!
//! ```text
//! height = (lut[c00] + lut[c01] + lut[c10] + lut[c11]) >> 2   // flat tile mean
//!        + override.coarse * -32                              // whole-tile step
//!        + sub_cell_step * -16                                // 64-unit sub-cell step
//! ```
//!
//! where the override record comes from the `.MAP` trigger block's **kind-2**
//! sub-table (`FUN_801D5630(2, tile_x, tile_z)` -> `FUN_801D5AE0`, primary
//! `+0x10000` then fallback `+0x12000`), and `sub_cell_step` is the 2-bit field
//! of [`ElevationOverride::quads`] selected by the 64-unit sub-cell the entity
//! stands in. A tile with the bit but no record keeps just the flat mean.
//!
//! This is why a ramp's collision-grid nibbles are meaningless: Rim Elm's
//! shore ramps sit on nibble-`0` (sea-level) tiles and carry their whole
//! elevation in the kind-2 records, so a sampler that only bilinear-interpolates
//! the nibbles drops the player straight to sea level at the top of the ramp -
//! under the drawn stair mesh.
//!
//! Provenance: `ghidra/scripts/funcs/80019278.txt` (the `cell & 0x800` branch),
//! `ghidra/scripts/funcs/overlay_cutscene_mapview_801d5630.txt`,
//! `ghidra/scripts/funcs/overlay_0896_801d5ae0.txt` (the shared kind-table
//! lookup). See `docs/subsystems/field-locomotion.md`.

/// Object-grid (`.MAP` `+0x8000`) cell bit marking a tile whose floor height
/// comes from the elevation-override model instead of the corner-nibble
/// bilinear surface. Sibling of `legaia_asset::field_objects`'s
/// `CELL_VISIBLE` (`0x2000`) / `CELL_WALK_VISIBLE` (`0x1000`).
pub const CELL_ELEVATION_OVERRIDE: u16 = 0x0800;

/// Record stride of the `.MAP` trigger block's kind-0/1/2 sub-tables (retail
/// reads it from the per-kind byte table at `DAT_8007B318`; the disc corpus
/// pins kinds 0..2 at 4 bytes and kind 3 - the region table - at 8, and every
/// scene's sub-tables tile the block back-to-back at exactly those strides).
pub const ELEVATION_RECORD_STRIDE: usize = 4;

/// Whole-tile elevation step, in world units per [`ElevationOverride::coarse`]
/// count (retail `* -0x20`; up is negative in the PSX Y-down field frame).
pub const COARSE_STEP_UNITS: i32 = -32;

/// Sub-cell elevation step, in world units per 2-bit
/// [`ElevationOverride::quads`] count (retail `* -0x10`).
pub const SUB_CELL_STEP_UNITS: i32 = -16;

/// One kind-2 `.MAP` elevation-override record: `[tile_x, tile_z, coarse,
/// quads]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ElevationOverride {
    /// Tile X the record applies to (exact match, like the kind-1 triggers).
    pub tile_x: u8,
    /// Tile Z the record applies to.
    pub tile_z: u8,
    /// Whole-tile elevation step (signed), [`COARSE_STEP_UNITS`] each.
    pub coarse: i8,
    /// Four packed 2-bit sub-cell steps, [`SUB_CELL_STEP_UNITS`] each. The
    /// 64-unit sub-cell the entity stands in - `(x >> 6) & 1`, `(z >> 6) & 1` -
    /// selects the field at bit `2 * sx + 4 * sz`. This is what turns a
    /// one-tile record into a **staircase**: the four quadrants of a 128-unit
    /// tile can each sit at a different 16-unit step.
    pub quads: u8,
}

impl ElevationOverride {
    /// The height delta this record contributes at world `(x, z)` - the
    /// `rec[3]`-selected sub-cell step plus the `rec[2]` whole-tile step.
    ///
    /// PORT: FUN_80019278 (the `cell & 0x800` branch's offset term)
    pub fn delta_at(&self, world_x: i32, world_z: i32) -> i32 {
        let shift = ((world_x >> 6) & 1) * 2 + ((world_z >> 6) & 1) * 4;
        let step = i32::from((self.quads >> shift) & 3);
        step * SUB_CELL_STEP_UNITS + i32::from(self.coarse) * COARSE_STEP_UNITS
    }
}

/// Parse the **kind-2** sub-table out of a `.MAP` trigger block (either the
/// `+0x10000` primary or the `+0x12000` fallback).
///
/// Same header shape every kind shares (`FUN_801D5AE0`): sub-table offset
/// `s16` at `+4k+2`, count `s16` at `+4k+4` - so kind 2 reads `+0xA` / `+0xC`.
/// Returns an empty vec on a short / negative header.
///
/// PORT: FUN_801D5AE0 (kind 2)
pub fn parse_elevation_overrides(block: &[u8]) -> Vec<ElevationOverride> {
    let read_s16 = |off: usize| -> Option<i16> {
        Some(i16::from_le_bytes([*block.get(off)?, *block.get(off + 1)?]))
    };
    let (Some(off), Some(count)) = (read_s16(0xA), read_s16(0xC)) else {
        return Vec::new();
    };
    if off <= 0 || count <= 0 {
        return Vec::new();
    }
    let (off, count) = (off as usize, count as usize);
    (0..count)
        .map_while(|i| {
            let base = off + i * ELEVATION_RECORD_STRIDE;
            let r = block.get(base..base + ELEVATION_RECORD_STRIDE)?;
            Some(ElevationOverride {
                tile_x: r[0],
                tile_z: r[1],
                coarse: r[2] as i8,
                quads: r[3],
            })
        })
        .collect()
}

/// Exact-match lookup of the elevation override at `(tile_x, tile_z)` - first
/// hit wins, mirroring `FUN_801D5630`'s primary-then-fallback scan order (the
/// caller concatenates the two tables in that order).
///
/// PORT: FUN_801D5630 (kind 2)
pub fn lookup_elevation_override(
    records: &[ElevationOverride],
    tile_x: u8,
    tile_z: u8,
) -> Option<ElevationOverride> {
    records
        .iter()
        .copied()
        .find(|r| r.tile_x == tile_x && r.tile_z == tile_z)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quads_select_the_sub_cell_step() {
        // rec[3] = 0x2a = 0b00_10_10_10: sub-cells (0,0), (1,0), (0,1) each
        // step 2 (-32); (1,1) steps 0.
        let r = ElevationOverride {
            tile_x: 17,
            tile_z: 16,
            coarse: 2,
            quads: 0x2a,
        };
        let tile = (17 * 128, 16 * 128);
        // Whole-tile term: 2 * -32 = -64.
        assert_eq!(r.delta_at(tile.0, tile.1), -64 - 32); // sub-cell (0,0)
        assert_eq!(r.delta_at(tile.0 + 64, tile.1), -64 - 32); // (1,0)
        assert_eq!(r.delta_at(tile.0, tile.1 + 64), -64 - 32); // (0,1)
        assert_eq!(r.delta_at(tile.0 + 64, tile.1 + 64), -64); // (1,1)
    }

    #[test]
    fn header_short_or_empty_parses_to_nothing() {
        assert!(parse_elevation_overrides(&[]).is_empty());
        assert!(parse_elevation_overrides(&[0; 0x10]).is_empty());
    }

    #[test]
    fn parses_kind2_records_at_the_kind_header_slot() {
        let mut block = vec![0u8; 0x20];
        // kind-2 header: offset at +0xA, count at +0xC.
        block[0xA..0xC].copy_from_slice(&16i16.to_le_bytes());
        block[0xC..0xE].copy_from_slice(&2i16.to_le_bytes());
        block[16..20].copy_from_slice(&[17, 17, 3, 0x00]);
        block[20..24].copy_from_slice(&[17, 16, 2, 0x2a]);
        let recs = parse_elevation_overrides(&block);
        assert_eq!(recs.len(), 2);
        assert_eq!(lookup_elevation_override(&recs, 17, 17).unwrap().coarse, 3);
        assert_eq!(
            lookup_elevation_override(&recs, 17, 16).unwrap().quads,
            0x2a
        );
        assert!(lookup_elevation_override(&recs, 1, 1).is_none());
    }
}
