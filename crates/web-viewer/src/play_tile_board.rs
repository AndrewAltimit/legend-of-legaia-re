//! Tile-board rendering for the browser play page.
//!
//! The field VM's op `0x49` installs a tile board on **either** host - the
//! opcode is in `engine-vm` and `World::refresh_tile_board_draw_list` rebuilds
//! `tile_board_draw_list` every field tick regardless of who is drawing. Only
//! the native window turned that list into draws, so a scene that installed a
//! board on this page produced an *invisible* board: the walk state machine
//! still refuses its `CELL_WALL` cells, so the player walks into nothing and
//! stops, which reads as a softlock rather than a missing decoration.
//!
//! The assembly itself is shared - [`legaia_engine_core::tile_board`]'s
//! `tile_board_actor_draws` / `tile_actor_slots_needing_mesh` /
//! `is_tile_actor_slot`, the same three the native `redraw` pass calls
//! through `legaia_engine_shell::tile_board_draws`. This module is only the
//! wasm boundary: the flat typed arrays the page's WebGL layer consumes,
//! shaped like the NPC path (`play_npc_transforms` and friends) because a
//! tile actor is drawn exactly like one - a scene mesh at a world position.
//!
//! Two native behaviours the page must keep, both easy to lose:
//!
//! * a cell whose mesh never uploaded is skipped, not drawn at the origin
//!   (`redraw.rs`'s `drained_spawn_slots` gate). Here that is the caller's
//!   check against [`LegaiaRuntime::play_tile_board_slots`];
//! * a board-owned actor must be skipped by the *generic* actor draw loop
//!   ([`LegaiaRuntime::play_tile_actor_slots`]), because its own transform
//!   only holds the last repositioned cell - draw it there as well and every
//!   tile actor ghosts once at whichever cell the refresh touched last.
//!
//! REF: overlay_0897_801e0f3c - the retail per-cell deferred draw pass this
//! serves; the port of it is in `engine-core`.

use crate::runtime::LegaiaRuntime;
use legaia_engine_core::tile_board;
use legaia_engine_core::world::SceneMode;
use wasm_bindgen::prelude::*;

/// A tile-actor mesh staged for upload: `(slot, mesh, per-vertex object ids,
/// flat RGBA + textured flag)`. One at a time, like the NPC path's `cur`.
pub(crate) type StagedTileMesh = (u8, legaia_tmd::mesh::VramMesh, Vec<u32>, Vec<u8>);

#[wasm_bindgen]
impl LegaiaRuntime {
    /// Actor-pool slots the live board draws that carry a resolved template
    /// mesh, in first-seen order. The page uploads one scene mesh per slot
    /// through [`Self::play_tile_actor_mesh`] before drawing any cell.
    ///
    /// Empty when no board is installed, which is every retail scene shipped
    /// so far - the op-`0x49` census found no scene MAN that installs one, so
    /// this path is reached today only through a demo trigger. The native
    /// window's is `LEGAIA_TILE_BOARD_DEMO=1`, which a browser cannot set;
    /// the page's is [`Self::play_install_demo_tile_board`], which installs
    /// the same 7x7 board through the same `try_install_tile_board` bytecode.
    /// This module claimed to share the native trigger for a while, and did
    /// not - `std::env::var_os` is unreachable in a browser, so the whole
    /// module was dead code wearing a "wired" comment.
    /// Install the demo tile board - the browser's twin of the native
    /// window's `LEGAIA_TILE_BOARD_DEMO=1` env trigger, which no browser can
    /// set. Same 7x7 board centred on the player, installed through the same
    /// op-`0x49` bytecode (`World::try_install_tile_board`), so the page
    /// exercises the identical install path rather than a second one.
    ///
    /// Returns `false` off the field, with a board already up, or when the
    /// install is refused. No retail scene installs a board, so this is the
    /// only way either host reaches the per-cell draw pass today.
    pub fn play_install_demo_tile_board(&mut self) -> bool {
        let Some(host) = self.scene_host.as_mut() else {
            return false;
        };
        let world = &mut host.world;
        if world.mode != SceneMode::Field || world.tile_board.is_some() || world.tile_board_armed {
            return false;
        }
        let Some(pslot) = world.player_actor_slot else {
            return false;
        };
        let (px, pz) = {
            let a = &world.actors[pslot as usize];
            (a.move_state.world_x as i32, a.move_state.world_z as i32)
        };
        // 7x7 board with the player's tile at its centre - byte-identical to
        // the native window's `maybe_install_demo_tile_board` instruction.
        let origin_x = ((px >> 7) - 3).clamp(0, 255) as u8;
        let origin_z = ((pz >> 7) - 3).clamp(0, 255) as u8;
        let instr: [u8; 14] = [
            0x49, 0x05, // op, sub-op
            origin_x, origin_z, // +1/+2 tile origin
            7, 7, // +3/+4 width x height
            5, // +5 draw radius
            0, // +6 mode flag (full-board draw)
            0, 0, 0, 0, // +7/+9 event-flag bases (unused by the demo)
            0, // +0xb player template (character-mesh head)
            3, // +0xc tile template base (effect-model library)
        ];
        world.try_install_tile_board(&instr)
    }

    pub fn play_tile_board_slots(&self) -> Vec<u32> {
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        tile_board::tile_actor_slots_needing_mesh(&h.world)
            .into_iter()
            .map(u32::from)
            .collect()
    }

    /// Every actor-pool slot the board owns (the `2..=14` tile-actor table),
    /// whether or not it has a mesh. The page's generic actor loop must skip
    /// these - see the module docs for what happens if it does not.
    pub fn play_tile_actor_slots(&self) -> Vec<u32> {
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        (0..h.world.actors.len())
            .filter(|&s| tile_board::is_tile_actor_slot(&h.world, s))
            .map(|s| s as u32)
            .collect()
    }

    /// This frame's per-cell draw set, flattened `[slot, x, y, z, ...]`.
    ///
    /// One entry per drawn cell, so a cell value repeated across the board
    /// yields several entries sharing one slot - that per-cell instancing is
    /// the whole reason the tile actor's own transform cannot carry it. `y`
    /// is the floor height under the tile centre, in the engine's Y-down
    /// field frame; the page negates it like every other world draw.
    pub fn play_tile_board_transforms(&self) -> Vec<f32> {
        let Some(h) = self.scene_host.as_ref() else {
            return Vec::new();
        };
        let draws = tile_board::tile_board_actor_draws(&h.world);
        let mut out = Vec::with_capacity(draws.len() * 4);
        for d in draws {
            out.push(f32::from(d.slot));
            out.extend_from_slice(&d.world);
        }
        out
    }

    /// Build the tile actor at `slot`'s mesh and stage it for the accessors
    /// below. Returns `slot`.
    ///
    /// The mesh source is the actor's own `tmd_ref`, which
    /// `try_install_tile_board` resolved from the board header's tile
    /// template - no scene resource table involved, so this works for a
    /// procedurally filled board as well as an authored one.
    pub fn play_tile_actor_mesh(&mut self, slot: u32) -> Result<u32, JsValue> {
        if self.tile_mesh.as_ref().map(|m| m.0) == Some(slot as u8) {
            return Ok(slot);
        }
        let global = self
            .scene_host
            .as_ref()
            .and_then(|h| h.world.actors.get(slot as usize))
            .and_then(|a| a.tmd_ref.clone())
            .ok_or_else(|| {
                JsValue::from_str(&format!(
                    "play_tile_actor_mesh: slot {slot} has no template"
                ))
            })?;
        let (mesh, object_ids, shading) =
            legaia_tmd::mesh::tmd_to_vram_mesh_field_hybrid(&global.tmd, &global.raw);
        let mut flat = Vec::with_capacity(shading.colors.len() * 4);
        for (c, &t) in shading.colors.iter().zip(shading.textured.iter()) {
            flat.extend_from_slice(&[c[0], c[1], c[2], if t != 0 { 255 } else { 0 }]);
        }
        self.tile_mesh = Some((slot as u8, mesh, object_ids, flat));
        Ok(slot)
    }

    pub fn play_tile_actor_mesh_positions(&self) -> Vec<f32> {
        self.tile_mesh
            .as_ref()
            .map(|(_, m, _, _)| m.positions.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_tile_actor_mesh_uvs(&self) -> Vec<u8> {
        self.tile_mesh
            .as_ref()
            .map(|(_, m, _, _)| m.uvs.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_tile_actor_mesh_cba_tsb(&self) -> Vec<u16> {
        self.tile_mesh
            .as_ref()
            .map(|(_, m, _, _)| m.cba_tsb.iter().flatten().copied().collect())
            .unwrap_or_default()
    }

    pub fn play_tile_actor_mesh_indices(&self) -> Vec<u32> {
        self.tile_mesh
            .as_ref()
            .map(|(_, m, _, _)| m.indices.clone())
            .unwrap_or_default()
    }

    pub fn play_tile_actor_mesh_flat_rgba(&self) -> Vec<u8> {
        self.tile_mesh
            .as_ref()
            .map(|(_, _, _, f)| f.clone())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::LegaiaRuntime;

    /// Before a scene exists every accessor answers "no board" rather than
    /// panicking - the page calls these unconditionally each frame.
    ///
    /// `play_tile_actor_mesh` is deliberately not exercised here: its error
    /// arm builds a `JsValue`, which aborts on a non-wasm32 target. The
    /// no-scene path it guards is the same `scene_host.as_ref()` check the
    /// three accessors below take.
    #[test]
    fn a_runtime_with_no_scene_reports_an_empty_board() {
        let rt = LegaiaRuntime::new();
        assert!(rt.play_tile_board_slots().is_empty());
        assert!(rt.play_tile_actor_slots().is_empty());
        assert!(rt.play_tile_board_transforms().is_empty());
        assert!(rt.play_tile_actor_mesh_positions().is_empty());
        assert!(rt.play_tile_actor_mesh_indices().is_empty());
        assert!(rt.play_tile_actor_mesh_flat_rgba().is_empty());
    }
}
