//! Tile-board tile-actor draw assembly for the play-window renderer.
//!
//! [`World`] rebuilds `tile_board_draw_list` each field tick (the retail
//! `overlay_0897_801e0f3c` deferred draw pass): one entry per drawable cell
//! in the active draw set, carrying the cell value, the value's tile-actor
//! pool slot, and the cell's world centre. This module turns that list into
//! the renderer-facing per-cell draw set - the positions each tile actor's
//! mesh instance draws at this frame - and answers the two ownership
//! questions the generic actor draw loop needs (which pool slots are
//! board-owned, which still need their template mesh uploaded).
//!
//! Kept in the shell lib (not the `legaia-engine` bin) so the assembly is
//! testable headlessly; the bin's redraw pass maps each draw's `slot` to its
//! uploaded GPU mesh and pushes one `SceneDraw` per entry.
//!
//! PORT: overlay_0897_801e0f3c (per-cell tile-actor draw pass; the select +
//! reposition halves live in `World::refresh_tile_board_draw_list`)

use legaia_engine_core::tile_board::{CELL_DRAW_FIRST, CELL_DRAW_LAST};
use legaia_engine_core::world::World;

/// One tile-actor mesh instance for this frame: the actor at `slot` draws
/// at `world` (a drawable cell's tile centre, floor-snapped like the field
/// NPC draws). A cell value repeated across cells yields multiple draws
/// sharing one `slot` - the per-cell instancing the shared actor can't
/// carry in its own transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileActorDraw {
    /// Actor-pool slot of the cell value's tile actor.
    pub slot: u8,
    /// The drawn cell's value (`2..=14`).
    pub cell_value: u8,
    /// World-space draw position `(x, y, z)` in the retail Y-down field
    /// frame (the same convention the field NPC / placement draws use).
    pub world: [f32; 3],
}

/// Assemble the per-cell tile-actor draw set from the world's per-frame
/// tile-board draw list. Empty when no board is installed. Slots whose
/// actor is gone (despawned mid-frame) are skipped - unresolved templates
/// degrade to "no draw", never a panic.
pub fn tile_board_actor_draws(world: &World) -> Vec<TileActorDraw> {
    world
        .tile_board_draw_list
        .iter()
        .filter(|d| world.actors.get(d.slot as usize).is_some_and(|a| a.active))
        .map(|d| {
            let y = world.sample_field_floor_height(d.world_x, d.world_z) as f32;
            TileActorDraw {
                slot: d.slot,
                cell_value: d.cell_value,
                world: [d.world_x as f32, y, d.world_z as f32],
            }
        })
        .collect()
}

/// The distinct tile-actor slots in the active draw set whose actor carries
/// a resolved template mesh (`tmd_ref`), in first-seen order - the set the
/// renderer must upload before the per-cell draws can land. Unresolved
/// templates (empty `tmd_ref`) are excluded: they allocated a slot but have
/// nothing to upload.
pub fn tile_actor_slots_needing_mesh(world: &World) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for d in &world.tile_board_draw_list {
        if out.contains(&d.slot) {
            continue;
        }
        if world
            .actors
            .get(d.slot as usize)
            .is_some_and(|a| a.active && a.tmd_ref.is_some())
        {
            out.push(d.slot);
        }
    }
    out
}

/// Whether actor-pool `slot` is a board-owned tile actor (a `2..=14` entry
/// of the tile-actor table). The generic per-actor draw loop skips these -
/// a tile actor draws once per cell through the deferred draw list, and its
/// own transform only holds the *last* repositioned cell. Table slot 0 (the
/// player) is not board-owned: the normal field path draws it.
pub fn is_tile_actor_slot(world: &World, slot: usize) -> bool {
    (CELL_DRAW_FIRST..=CELL_DRAW_LAST)
        .any(|v| world.tile_actor_slots[v as usize].is_some_and(|s| s as usize == slot))
}
