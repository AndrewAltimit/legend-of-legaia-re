//! The op-`0x49` tile board. Its rules (the cell codes, the header, the walk
//! state machine, the fades, the prompt) live in
//! `legaia_engine_minigames::tile_board` and are re-exported here; this file
//! keeps the per-frame draw assembly, which reads the `World`.

pub use legaia_engine_minigames::tile_board::*;

// ---------------------------------------------------------------------
// Per-frame draw assembly
// ---------------------------------------------------------------------
//
// These three lived in `legaia-engine-shell`, which pulls winit and cpal and
// therefore does not build for wasm32 - so the browser play page had no way
// to reach them and drew no board at all. `World` maintains
// `tile_board_draw_list` for both hosts (the field VM's op `0x49` installs
// the board regardless of who is rendering), so a scene that installs one on
// the play page produced an invisible board: the walk SM still refuses the
// wall cells, which is a walk into nothing rather than a cosmetic gap.
//
// They are pure `&World -> Vec<..>` with no GPU, glam or serde in sight, and
// they sit here rather than in `engine-ui` because `engine-ui` deliberately
// does not depend on `engine-core` - its contract is view-struct in,
// `TextDraw` out, and these take a `World`.

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
    /// Uniform scale, the tile actor's `+0x72` over `0x1000`: the board's
    /// fade grows the tiles in at install and shrinks them away at exit.
    pub scale: f32,
}

/// Assemble the per-cell tile-actor draw set from the world's per-frame
/// tile-board draw list. Empty when no board is installed. Slots whose
/// actor is gone (despawned mid-frame) are skipped - unresolved templates
/// degrade to "no draw", never a panic.
///
/// This is the port site for the board's draw pass, and the only one: both
/// hosts call it (`play-window`'s redraw and the browser play page's
/// `play_tile_board`). The `engine-shell` module of the same name is a bare
/// re-export kept for the native bin's import path.
///
/// Provenance caveat - the dump this cites is a **wrong-base print**. The
/// board renderer is not a function: it is the tail block of the walk SM
/// `FUN_801EF2B0` at `0x801EFEA0`, entered by nineteen in-body branches and
/// running to that routine's epilogue, so there is no render entry to name
/// (`overlay_0897_801efea0.txt`, and
/// [`docs/subsystems/tile-board.md`](../../../docs/subsystems/tile-board.md)
/// "The render tail"). `overlay_0897_801e0f3c.txt` is a second print of the
/// same instructions under a different load base; it is cited here because
/// it is the dump file the corpus carries for this pass, not because a
/// function begins at that address.
///
/// PORT: overlay_0897_801e0f3c (per-cell tile-actor draw pass; the select +
/// reposition halves live in `World::refresh_tile_board_draw_list`)
pub fn tile_board_actor_draws(world: &crate::world::World) -> Vec<TileActorDraw> {
    world
        .board
        .draw_list
        .iter()
        .filter(|d| world.actors.get(d.slot as usize).is_some_and(|a| a.active))
        .map(|d| {
            let y = world.sample_field_floor_height(d.world_x, d.world_z) as f32;
            TileActorDraw {
                slot: d.slot,
                cell_value: d.cell_value,
                world: [d.world_x as f32, y, d.world_z as f32],
                scale: world.tile_board_cell_scale(d.cell_value),
            }
        })
        .collect()
}

/// The distinct tile-actor slots in the active draw set whose actor carries
/// a resolved template mesh (`tmd_ref`), in first-seen order - the set the
/// renderer must upload before the per-cell draws can land. Unresolved
/// templates (empty `tmd_ref`) are excluded: they allocated a slot but have
/// nothing to upload.
pub fn tile_actor_slots_needing_mesh(world: &crate::world::World) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for d in &world.board.draw_list {
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
pub fn is_tile_actor_slot(world: &crate::world::World, slot: usize) -> bool {
    (CELL_DRAW_FIRST..=CELL_DRAW_LAST)
        .any(|v| world.board.actor_slots[v as usize].is_some_and(|s| s as usize == slot))
}
