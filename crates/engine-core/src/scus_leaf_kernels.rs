//! The scene control-block reset over a [`crate::world::World`].
//!
//! The block's reset values, and the other `SCUS_942.54` leaf kernels, are
//! [`legaia_engine_system::scus_leaf_kernels`], which names no `World`. This
//! module holds the one body that writes world state.

use legaia_engine_system::scus_leaf_kernels::SCENE_CONTROL_BLOCK_RESET;

impl crate::world::World {
    /// Allocate + reset the scene control block. `FUN_8003A024`.
    ///
    /// Called from [`legaia_engine_scene_host::scene_host::SceneHost::load_scene`], which is where
    /// retail runs it: the block is re-allocated per scene and the per-scene
    /// contents are installed afterwards by `FUN_8003A110` from the scene MAN.
    ///
    /// The engine models the two globals the reset clears alongside the block
    /// ([`legaia_engine_system::scus_leaf_kernels::SCENE_RESET_CLEARED_GLOBALS`])
    /// rather than the raw words. `0x8007B450` is the tile-descriptor pointer the field VM's op `0x49` installs a tile
    /// board into (`docs/subsystems/tile-board.md`), so clearing it is what
    /// stops a board surviving a scene change; `0x8007B630` is the camera
    /// **shake amplitude** field-VM `[4C 84 amp]` raises
    /// ([`legaia_engine_field::camera_rig::CameraRig::shake_amplitude`]), so clearing it is what
    /// stops a tremor a script left running from following the player into
    /// the next scene. The block's own `+0x18` / `+0x1C` - the jitter pair
    /// the follow ease last added into the eye - are zeroed in the same run
    /// of stores; the engine keeps that pair on the camera
    /// (`ZoneFollow::shake_offset`), whose scene-entry reset clears it.
    ///
    /// PORT: FUN_8003A024
    pub fn reset_scene_control_block(&mut self) {
        self.scene_control_block = SCENE_CONTROL_BLOCK_RESET;
        // `_DAT_8007B450 = 0` - the tile-board descriptor and everything the
        // engine hangs off it.
        self.board.grid = None;
        self.board.header = None;
        self.board.target = None;
        self.board.armed = false;
        self.board.actor_slots = [None; legaia_engine_minigames::tile_board::TILE_ACTOR_TABLE_LEN];
        self.board.draw_list.clear();
        // `sw zero, -0x49d0(v0)` at `0x8003A07C`: `_DAT_8007B630 = 0`.
        self.camera.shake_amplitude = 0;
    }
}
