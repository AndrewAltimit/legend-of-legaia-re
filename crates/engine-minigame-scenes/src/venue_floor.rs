//! Which placed records a minigame venue stands: the retail floor walks'
//! selection, applied to the venue's static draw list.
//!
//! A venue's placed props are not the field's. Neither minigame runs the field
//! init sweep `FUN_8003A55C`; each overlay spawns its own floor actors at setup
//! from the scene floor buffer, and only those draw.
//!
//! * **Fishing** runs one floor pass, `FUN_801D6BBC` (`jal` at `0x801CF664` in
//!   the setup state, after the region box is latched at the venue's anchor
//!   tile by `jal 0x800180EC` at `0x801CF65C`). It frees the floor-actor list
//!   (`*0x8007C36C`) and spawns one actor per placed record inside the box
//!   whose anchor cell lacks `0x400`
//!   ([`legaia_engine_field::minigame_floor::floor_pass_spawns`]). Both venues
//!   share one `.MAP` (`other1`); the box keeps Buma's mountains and bank (rows
//!   `102..110`) out of the Vidna venue.
//! * **Dance** runs two. The step-marker walk `FUN_801D2A10` (`jal` at
//!   `0x801CF1B8`, rect `(0x28, 0x50)` by `(0x14, 0x1E)`) spawns, into list
//!   `*0x8007C354`, every drawn cell of the rect whose anchor tile carries a
//!   kind-1 trigger record (`FUN_801D3EC0(1, ..)` non-zero at `0x801D2BFC`):
//!   the marker template for clips `6..=9`, the plain template otherwise. The
//!   floor pass `FUN_801D3A2C` (`jal` at `0x801CF1CC`) is gated off by a
//!   non-zero field-entry word `_DAT_8007B8B8` (`0x801D3A70..0x801D3A7C`),
//!   which the field's door warp into the minigame mode sets to `1`
//!   ([`legaia_engine_mode::mode_transition::REENTRY_FLAG`]) - so on the
//!   retail entry it spawns nothing. The retail dance states
//!   (`minigame_dance_retail_ready` / `_dancing` / `_judged`) hold `1` there,
//!   an empty `*0x8007C36C` list and exactly the 35 rect cells on
//!   `*0x8007C354`.

use std::collections::HashSet;

use legaia_engine_field::field_regions::{
    RegionTable, TileTrigger, lookup_tile_trigger, refresh_region_attributes,
};
use legaia_engine_field::minigame_floor::{
    FloorGrid, floor_pass_spawns, floor_tile_spawns, height_ramp,
};
use legaia_scene_formats::field_objects::Placement;

/// The region box `[x0, z0, x1, z1]` (end-exclusive, `0x1F800384..87`) the
/// setup latches at `tile` from the `.MAP` region block (`0x800180EC`).
pub fn latched_region_box(region_block: Option<&[u8]>, tile: (i32, i32)) -> [u8; 4] {
    let table = region_block.and_then(RegionTable::parse);
    let (_, attrs) = refresh_region_attributes(table.as_ref(), tile.0, tile.1, false);
    attrs.box_bytes
}

/// The cells the floor pass (`FUN_801D6BBC` / `FUN_801D3A2C`) spawns over
/// `region_box` of the floor buffer `map`.
pub fn floor_pass_cells(map: &[u8], region_box: [u8; 4]) -> HashSet<(i32, i32)> {
    let [x0, z0, x1, z1] = region_box.map(i32::from);
    floor_pass_spawns(FloorGrid::new(map), &height_ramp(), x0, z0, x1, z1)
        .into_iter()
        .map(|s| (s.cell_x, s.cell_z))
        .collect()
}

/// The cells the dance's step-marker walk (`FUN_801D2A10`) spawns over the
/// rect `(x0, z0, width, height)`: the drawn cells whose anchor tile resolves
/// a kind-1 trigger record, primary block first.
pub fn marker_walk_cells(
    map: &[u8],
    rect: (i32, i32, i32, i32),
    triggers_primary: &[TileTrigger],
    triggers_fallback: &[TileTrigger],
) -> HashSet<(i32, i32)> {
    let grid = FloorGrid::new(map);
    let (x0, z0, w, h) = rect;
    floor_tile_spawns(grid, &height_ramp(), x0, z0, w, h, false)
        .into_iter()
        .filter(|s| {
            let rec = grid.tile(s.tile_id);
            let (Ok(tx), Ok(tz)) = (
                u8::try_from(s.cell_x + i32::from(rec.nbr_dx)),
                u8::try_from(s.cell_z + i32::from(rec.nbr_dz)),
            ) else {
                return false;
            };
            lookup_tile_trigger(triggers_primary, triggers_fallback, tx, tz).is_some()
        })
        .map(|s| (s.cell_x, s.cell_z))
        .collect()
}

/// `FUN_801D3A2C`'s two enable gates (`0x801D3A64..0x801D3A7C`): the dev
/// word `_DAT_8007B868` bit `2` and a non-zero field-entry word
/// `_DAT_8007B8B8` each skip the pass.
pub const fn dance_floor_pass_runs(dev_word: u32, entry_word: u32) -> bool {
    dev_word & 2 == 0 && entry_word == 0
}

/// Keep the placed records standing on a spawned cell.
pub fn keep_spawned(placements: Vec<Placement>, cells: &HashSet<(i32, i32)>) -> Vec<Placement> {
    placements
        .into_iter()
        .filter(|p| cells.contains(&(i32::from(p.col), i32::from(p.row))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use legaia_scene_formats::field_objects::parse_placements;

    /// A floor buffer with one tile record (id 1, placed) and the given cells
    /// pointing at it; `blocked` cells carry `0x400`.
    fn map_with(cells: &[(usize, usize)], blocked: &[(usize, usize)]) -> Vec<u8> {
        let mut m = vec![0u8; 0x10000];
        // Record 1: placed (+0x12 bit 4), zero anchor delta.
        m[0x20 + 0x12] = 0x04;
        for &(x, z) in cells {
            m[0x8000 + z * 0x100 + x * 2] = 1;
        }
        for &(x, z) in blocked {
            m[0x8000 + z * 0x100 + x * 2 + 1] |= 0x04;
        }
        m
    }

    fn cells_of(p: &[Placement]) -> Vec<(u8, u8)> {
        p.iter().map(|p| (p.col, p.row)).collect()
    }

    #[test]
    fn the_floor_pass_keeps_unbound_cells_inside_the_box() {
        let map = map_with(&[(5, 5), (6, 6), (40, 40)], &[(6, 6)]);
        let kept = keep_spawned(
            parse_placements(&map),
            &floor_pass_cells(&map, [0, 0, 10, 10]),
        );
        assert_eq!(cells_of(&kept), vec![(5, 5)]);
    }

    #[test]
    fn the_marker_walk_keeps_triggered_cells_inside_its_rect() {
        let map = map_with(&[(5, 5), (6, 6), (40, 40)], &[(6, 6)]);
        let trig = |x, z| TileTrigger {
            tile_x: x,
            tile_z: z,
            record: 0,
            gate: 0,
        };
        let primary = [trig(6, 6), trig(40, 40)];
        let cells = marker_walk_cells(&map, (0, 0, 10, 10), &primary, &[]);
        let kept = keep_spawned(parse_placements(&map), &cells);
        // (5, 5) has no trigger; (40, 40) stands past the rect; the walk does
        // not test the `0x400` bit.
        assert_eq!(cells_of(&kept), vec![(6, 6)]);
    }

    #[test]
    fn the_door_warp_entry_gates_the_dance_floor_pass_off() {
        use legaia_engine_mode::mode_transition::REENTRY_FLAG;
        assert!(!dance_floor_pass_runs(0, REENTRY_FLAG));
        assert!(!dance_floor_pass_runs(2, 0));
        assert!(dance_floor_pass_runs(0, 0));
    }
}
