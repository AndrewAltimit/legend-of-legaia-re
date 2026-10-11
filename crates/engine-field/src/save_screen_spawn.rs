//! The field overlay's save-screen hand-off (`FUN_801D841C`): one spawn of
//! the in-field save / load screen driver, with its `+0x5C` word set.
//!
//! The `PORT` tag lives on the item that implements the address, so the
//! liveness audit sees one anchor per port site.
//!
//! REF: FUN_80020DE0 - the descriptor spawner, hosted in
//! `legaia_engine_core::actor_alloc_host`.
//!
//! Read off `ghidra/scripts/funcs/overlay_cutscene_dialogue_801d841c.txt` -
//! disassembly, not the C.
//!
//! `0x801D841C` is **VA-aliased**: the root dump `801d841c.txt` resolves
//! `entry=801D8308` in a different overlay image. The body ported here is the
//! one the cutscene / field band holds, which is the 13-instruction spawner
//! both `overlay_cutscene_dialogue_801d841c.txt` and
//! `overlay_cutscene_mapview_801d841c.txt` attest.

/// The spawn descriptor `FUN_801D841C` hands to `FUN_80020DE0`.
///
/// The whole function is thirteen instructions:
/// `FUN_80020DE0(0x800706BC, _DAT_8007C34C)` followed by a single
/// `sh 1,0x5c(v0)`. `0x800706BC` sits inside the 24-byte spawn-descriptor
/// family at `0x800705FC..0x80070763` that `docs/subsystems/asset-loader.md`
/// already documents (the scene-transition streaming actor uses `0x80070734`
/// out of the same family) - it is **not** a row of the mode table at
/// `0x8007078C`.
///
/// The descriptor's `+0x8` handler word reads `0x80024190` on the disc, which
/// is the **in-field save/load screen driver**
/// ([`crate::field_save_screen_actor`]) - so this routine is a save-screen
/// hand-off, not the spawn of a "screen-flash element" that an earlier reading
/// named it for. Two independent checks agree: the fill-fade sibling
/// `FUN_801EE5D4` spawns the same descriptor at `0x801EE6D8` and leaves
/// `+0x5C` alone, which is that actor's load side; and the two globals
/// `FUN_801ED308` shares with it are written from nowhere but the menu
/// overlay's save/load UI.
pub const SAVE_SCREEN_DESCRIPTOR: u32 = 0x8007_06BC;

/// The pool discriminator, `_DAT_8007C34C` - the system actor pool the
/// driver's node hangs off.
pub const SAVE_SCREEN_POOL: u32 = 0x8007_C34C;

/// The one field `FUN_801D841C` writes on the actor it just spawned:
/// `+0x5C = 1`.
///
/// The store's base register is the `jal`'s **return value**, not the `lui`
/// that preceded it, so this lands on the new actor rather than on a global.
/// `FUN_80024190`'s state 4 branches on it: zero runs the load-side dispatcher
/// `FUN_801DD35C`, non-zero the save-write flow `FUN_801DC6B4`. `1` is
/// therefore "save".
pub const SAVE_SCREEN_FIELD_5C: i16 = 1;

/// The whole of `FUN_801D841C`, which is a spawn and one store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveScreenSpawn {
    /// First argument of `FUN_80020DE0` - [`SAVE_SCREEN_DESCRIPTOR`].
    pub descriptor: u32,
    /// Second argument - [`SAVE_SCREEN_POOL`].
    pub pool: u32,
    /// Written to `+0x5C` of the actor the spawn returns.
    pub field_5c: i16,
}

/// Build the spawn `FUN_801D841C` performs.
///
/// PORT: FUN_801D841C
/// REF: FUN_801ED308 - the panel fade/flash actor, the only caller.
///
/// WIRED: the sole `jal` to `0x801D841C` in the corpus is `0x801ED3DC`, inside
/// `FUN_801ED308`, the world-map panel brightness fade/flash actor. That
/// function is ported and live as
/// `crate::world_map_panel_actors::fade_flash_tick`, its phase-1
/// arm emits `FadeFlashEffect::SpawnSaveScreen` where the `jal` sits, and
/// `crate::world_map_panel_host::PanelActorHost` calls this from
/// that arm's handler. The call path is `World::tick` ->
/// `World::tick_world_map` -> `World::tick_world_map_panels` ->
/// `PanelActorHost::tick`.
///
/// The result is load-bearing rather than recorded: the hand-off it opens is
/// what `PanelActorHost::release_flash_with` requires before it will write the
/// flash counter, which is retail's own precondition - the counter's only
/// writers are the menu overlay's save-side screens, and that overlay is
/// resident only because this spawn put its driver on the pool.
pub fn save_screen_spawn() -> SaveScreenSpawn {
    SaveScreenSpawn {
        descriptor: SAVE_SCREEN_DESCRIPTOR,
        pool: SAVE_SCREEN_POOL,
        field_5c: SAVE_SCREEN_FIELD_5C,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_save_screen_descriptor_is_inside_the_spawn_descriptor_family() {
        assert!((0x8007_05FC..=0x8007_0763).contains(&SAVE_SCREEN_DESCRIPTOR));
        assert_ne!(SAVE_SCREEN_DESCRIPTOR, 0x8007_078C, "not a mode-table row");
        assert_eq!(SAVE_SCREEN_FIELD_5C, 1);
        assert_eq!(SAVE_SCREEN_POOL, 0x8007_C34C);
        assert_eq!(
            save_screen_spawn(),
            SaveScreenSpawn {
                descriptor: SAVE_SCREEN_DESCRIPTOR,
                pool: SAVE_SCREEN_POOL,
                field_5c: SAVE_SCREEN_FIELD_5C,
            }
        );
    }
}
