//! The scene's **windowed static-object list**: the actors the sub-area window
//! sweep `FUN_801D7B50` keeps, re-planned on every camera re-centre.
//!
//! Retail places a field scene's `.MAP` objects through two sweeps. The
//! scene-init sweep `FUN_8003A55C` creates the *bound* placements once, on the
//! actor list at the scene control block's `+0x0C`. The window sweep creates the
//! rest - the placements whose footprint-anchor tile lacks
//! [`crate::field_regions::CELL_BIND_OWNED`] - on its own list at `+0x24`
//! (`0x8007C36C`), and only those inside the region box the camera re-centre
//! latched. It runs from exactly two places, the re-centre routines
//! `FUN_80017DD4` (the scene-entry window install in `FUN_801D6704`) and
//! `FUN_80017EC8` (every mid-scene re-centre: a player `MOVE_TO`, the player
//! arm of `4C 51`, the kind-0 warp landing, the leader swap). Both call
//! `FUN_800180EC(tile_x, tile_z)` to latch the box at the re-centre tile first
//! (`0x80017E0C` / `0x80017F00`) and then `jal 0x801D7B50`. A player walking
//! across a region boundary without a re-centre keeps the old list.
//!
//! [`World::recentre_field_window`] is that pair. It keeps what the sweep needs
//! resident on [`crate::world::FieldTerrain::static_window`]: the `.MAP`
//! `+0x0000..0x4000` object-descriptor region (the one region the engine
//! otherwise drops after scene load), next to the live walk grid and object
//! cells the terrain already holds.
//!
//! What a host draws from it is decided by one kernel,
//! [`crate::field_env::placed_draw_live`]. By default the port keeps drawing
//! every placement for the whole map (the list still runs; the draw simply does
//! not gate on it); with [`StaticObjectWindow::retail_windowing`] set, a
//! window-owned placement draws only while this list holds a drawn actor for
//! it, which is retail's sub-area pop-in.
//!
//! REF: FUN_80017DD4, FUN_80017EC8 (the re-centre pair; their per-cell tile
//! emission is scope-ignored), FUN_800180EC, FUN_801D6704

use super::*;
use crate::field_env::PlacedWindowKey;
use crate::field_regions::{self, RegionTable, WindowSpawn};
use std::collections::HashSet;

/// The windowed static-object list and the resident bytes it is planned from.
#[derive(Debug, Clone, Default)]
pub struct StaticObjectWindow {
    /// The `.MAP` `+0x0000..0x4000` object-descriptor region (`0x200` records
    /// of `0x20` bytes), resident for the life of the scene - retail indexes it
    /// through `*(_DAT_1F8003EC)` on every rebuild. Empty outside a field scene.
    pub descriptors: Vec<u8>,
    /// The region box the list was last planned for, in the scratchpad's byte
    /// order (`0x1F800384..0x1F800387`: `[x0, z0, x1, z1]`, half-open). `None`
    /// until the first re-centre of the scene.
    pub window: Option<[u8; 4]>,
    /// The list itself - one [`WindowSpawn`] per actor on the `+0x24` head, in
    /// the sweep's own order.
    pub spawns: Vec<WindowSpawn>,
    /// The sweep's spawn counter `_DAT_8007B924`: zeroed on entry to every
    /// rebuild, `+1` per actor spawned.
    pub spawn_count: u32,
    /// Bumped on every rebuild, so a host can re-derive per-draw visibility
    /// only when the list actually changed.
    pub generation: u32,
    /// Retail windowing: gate window-owned placements on this list (the
    /// sub-area pop-in). Off by default - the port draws the whole map.
    pub retail_windowing: bool,
    /// The drawn actors' identities, the set [`Self::draws`] answers from.
    drawn: HashSet<PlacedWindowKey>,
}

impl StaticObjectWindow {
    /// Whether the list holds a *drawn* actor (draw kind non-zero - see
    /// [`WindowSpawn::drawn`]) for the placement `key` names.
    pub fn draws(&self, key: &PlacedWindowKey) -> bool {
        self.drawn.contains(key)
    }

    /// Replace the list with a fresh plan (the free + respawn of one rebuild).
    fn replace(&mut self, window: [u8; 4], spawns: Vec<WindowSpawn>) {
        self.drawn = spawns
            .iter()
            .filter(|s| s.drawn())
            .map(PlacedWindowKey::of_spawn)
            .collect();
        self.spawn_count = spawns.len() as u32;
        self.spawns = spawns;
        self.window = Some(window);
        self.generation = self.generation.wrapping_add(1);
    }

    /// Drop the list and the resident descriptors (a scene with no field map).
    fn clear(&mut self) {
        self.descriptors.clear();
        self.window = None;
        self.spawns.clear();
        self.drawn.clear();
        self.spawn_count = 0;
        self.generation = self.generation.wrapping_add(1);
    }
}

impl World {
    /// Install the scene's `.MAP` object-descriptor region (`+0x0000..0x4000`)
    /// as resident field state and drop the previous scene's list. An empty
    /// slice clears both.
    pub fn load_field_object_descriptors(&mut self, descriptors: &[u8]) {
        let w = &mut self.terrain.static_window;
        w.clear();
        let n = descriptors.len().min(field_regions::MAP_WALK_GRID_OFFSET);
        w.descriptors.extend_from_slice(&descriptors[..n]);
    }

    /// The logic half of retail's camera re-centre (`FUN_80017DD4` /
    /// `FUN_80017EC8`): latch the region box at `(tile_x, tile_z)` and re-plan
    /// the windowed static-object list inside it.
    ///
    /// The latch is `FUN_800180EC` at the re-centre tile - the same
    /// [`field_regions::refresh_region_attributes`] the per-tile refresh runs,
    /// writing the region-type mask and the scratch attribute block - and the
    /// re-plan is [`Self::rebuild_static_object_window`] over the box it just
    /// latched. The routines' remaining work (the scroll-origin words and the
    /// `32 x 32` per-cell emit through the empty stub `FUN_8002B96C`) is the
    /// render pass the engine's own field background replaces.
    ///
    /// Callers pass the tile the retail call site passes: the scene-entry
    /// install and the warp landing / leader swap use `world >> 7`, the
    /// `MOVE_TO` / `4C 51` player arms the operand bytes `& 0x7F`.
    pub fn recentre_field_window(&mut self, tile_x: i32, tile_z: i32) {
        let table = RegionTable::parse(&self.terrain.map_region_block);
        let world_map_mode = self.mode == SceneMode::WorldMap;
        let (mask, attrs) = field_regions::refresh_region_attributes(
            table.as_ref(),
            tile_x,
            tile_z,
            world_map_mode,
        );
        if table.is_some() {
            self.flags.extra_flags = mask;
        }
        self.terrain.region_attributes = attrs;
        self.rebuild_static_object_window(attrs.box_bytes);
    }

    /// One window rebuild over `window` (`[x0, z0, x1, z1]`, the scratchpad box
    /// bytes): free the list and respawn it through
    /// [`field_regions::window_rebuild_spawns_resident`] against the resident
    /// descriptors, the live walk grid, the live object cells and the live
    /// floor-height ladder (`0x1F80035C`).
    ///
    /// Retail's two skip gates are not reachable at any engine call site:
    /// `_DAT_8007B8B8 != 0` is the warp-entry word, which the field initialiser
    /// clears before any mid-scene re-centre and which the engine's field entry
    /// never sets (it always enters cold), and `_DAT_8007B868 & 2` is the
    /// dev menu's `CLOSED` word, which a retail session holds at `0`. A scene
    /// with no resident descriptors keeps an empty list.
    pub fn rebuild_static_object_window(&mut self, window: [u8; 4]) {
        let t = &mut self.terrain;
        let spawns = if t.static_window.descriptors.is_empty() {
            Vec::new()
        } else {
            field_regions::window_rebuild_spawns_resident(
                &t.static_window.descriptors,
                &t.collision_grid,
                &t.object_cells,
                (window[0], window[1], window[2], window[3]),
                t.floor_height_lut,
            )
        };
        t.static_window.replace(window, spawns);
    }

    /// Re-centre on the player's own position (`world >> 7`), the tile form
    /// the scene-entry install, the warp landing and the leader swap pass.
    pub fn recentre_field_window_on_player(&mut self) {
        let Some((x, z)) = self
            .player_actor_slot
            .and_then(|s| self.actors.get(s as usize))
            .map(|a| (a.move_state.world_x, a.move_state.world_z))
        else {
            return;
        };
        self.recentre_field_window(i32::from(x) >> 7, i32::from(z) >> 7);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field_regions::{CELL_BIND_OWNED, MAP_OBJECT_DESCRIPTOR_STRIDE};

    /// A world with one field scene's worth of resident state: descriptor 5
    /// (spawnable, drawn) on tile `(3, 4)` and descriptor 6 (spawnable, drawn)
    /// on tile `(40, 40)`, plus a region table whose one type-0 box covers only
    /// the first.
    fn world() -> World {
        let mut w = World::new();
        w.mode = SceneMode::Field;
        w.install_field_player(0);
        let mut desc = vec![0u8; 0x4000];
        for (d, flags) in [(5usize, 0x4u16 | 0x2), (6, 0x4 | 0x2), (7, 0x4)] {
            let b = d * MAP_OBJECT_DESCRIPTOR_STRIDE;
            desc[b + 0x12..b + 0x14].copy_from_slice(&flags.to_le_bytes());
        }
        w.load_field_object_descriptors(&desc);
        w.terrain.collision_grid = vec![0u8; 0x4000];
        w.terrain.object_cells = vec![0u16; 0x4000];
        w.terrain.object_cells[3 + 4 * 0x80] = 5;
        w.terrain.object_cells[40 + 40 * 0x80] = 6;
        w.terrain.object_cells[5 + 5 * 0x80] = 7;
        // Region block: body offset 0x20, one 8-byte record, type 0, box
        // [0, 10) x [0, 10) (stored `[x0, z1, x1, z0]`, latched `[+0, +3, +2,
        // +1]`).
        let mut block = vec![0u8; 0x40];
        block[0xE..0x10].copy_from_slice(&0x20i16.to_le_bytes());
        block[0x10..0x12].copy_from_slice(&1i16.to_le_bytes());
        block[0x20..0x25].copy_from_slice(&[0, 10, 10, 0, 0]);
        w.terrain.map_region_block = block;
        w
    }

    #[test]
    fn recentre_latches_the_box_and_plans_only_its_placements() {
        let mut w = world();
        w.recentre_field_window(3, 4);
        let sw = &w.terrain.static_window;
        assert_eq!(sw.window, Some([0, 0, 10, 10]));
        let tiles: Vec<_> = sw.spawns.iter().map(|s| s.tile).collect();
        assert_eq!(tiles, vec![(3, 4), (5, 5)]);
        assert_eq!(sw.spawn_count, 2);
        assert_eq!(w.terrain.region_attributes.box_bytes, [0, 0, 10, 10]);
        // Outside every type-0/1 box the latch falls back to the full-map
        // default fill, and the list re-plans over the whole grid.
        let g = w.terrain.static_window.generation;
        w.recentre_field_window(60, 60);
        let sw = &w.terrain.static_window;
        assert_eq!(sw.window, Some([0, 0, 0x7F, 0x7F]));
        assert_eq!(sw.spawns.len(), 3);
        assert_ne!(sw.generation, g, "every rebuild bumps the generation");
    }

    #[test]
    fn a_draw_kind_zero_actor_is_listed_but_never_drawn() {
        let mut w = world();
        w.recentre_field_window(3, 4);
        let sw = &w.terrain.static_window;
        let kind0 = sw.spawns.iter().find(|s| s.tile == (5, 5)).unwrap();
        assert_eq!(kind0.template_kind, 0);
        assert!(!sw.draws(&PlacedWindowKey::of_spawn(kind0)));
        let kind5 = sw.spawns.iter().find(|s| s.tile == (3, 4)).unwrap();
        assert!(sw.draws(&PlacedWindowKey::of_spawn(kind5)));
    }

    #[test]
    fn the_rebuild_reads_the_live_cells_not_the_disc_bytes() {
        let mut w = world();
        // A live stamp of the ownership bit on the anchor tile moves the
        // placement to the init sweep.
        w.terrain.object_cells[3 + 4 * 0x80] |= CELL_BIND_OWNED;
        w.recentre_field_window(3, 4);
        assert!(
            w.terrain
                .static_window
                .spawns
                .iter()
                .all(|s| s.tile != (3, 4))
        );
    }

    #[test]
    fn a_warp_landing_replans_the_list_at_the_destination() {
        let mut w = world();
        w.recentre_field_window(60, 60);
        assert_eq!(w.terrain.static_window.window, Some([0, 0, 0x7F, 0x7F]));
        // Destination half-tiles (10, 12) seat the player at (704, 832), tile
        // (5, 6) - inside the one type-0 box.
        w.arm_field_warp((10, 12));
        while !matches!(
            w.tick_field_warp(),
            crate::world::FieldWarpTick::Landed { .. }
        ) {}
        let sw = &w.terrain.static_window;
        assert_eq!(sw.window, Some([0, 0, 10, 10]));
        assert!(sw.spawns.iter().all(|s| s.tile != (40, 40)));
    }

    #[test]
    fn loading_a_new_scene_drops_the_list() {
        let mut w = world();
        w.recentre_field_window(3, 4);
        w.load_field_object_descriptors(&[]);
        let sw = &w.terrain.static_window;
        assert!(sw.spawns.is_empty() && sw.window.is_none());
        w.recentre_field_window(3, 4);
        assert!(w.terrain.static_window.spawns.is_empty());
    }
}
