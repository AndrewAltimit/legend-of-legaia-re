//! Battle presentation decisions both play hosts take in their per-tick
//! effect step, so neither re-spells them.

use legaia_engine_audio::{CueDispatch, classify_cue};
use legaia_engine_core::world::World;

/// The production move-FX trigger, spawn to sound: take the world's pending
/// move-FX spawn (a non-summon spell cast or enemy special whose move-power
/// record carries a spawnable effect list), seat it through
/// [`World::spawn_move_fx`], and resolve the move's sound cue through the
/// retail dispatch decode (`classify_cue` = `FUN_8004FCC8`).
///
/// Returns the SFX ring value to enqueue - the `SfxBank` descriptor id
/// (`docs/formats/sfx-table.md`) the host hands its director's
/// `enqueue_sfx`, which resolves the cue's own `+4` category bank. `None`
/// when nothing spawned or the move carries no cue.
///
/// The cue is a byte (`World::take_pending_move_fx_cue`), so it always lands
/// in the dispatcher's ring band below `0x100`; the `Voice` band (a streamed
/// CD-XA trigger, `id >= 0x100`) cannot arise here, and the two hosts used to
/// differ only in whether they logged that impossible arm. The full ring
/// value goes through: cue `0` resolves to ring value `0xFFFF`, which a
/// `u8` narrowing used to drop on the play page.
pub fn spawn_pending_move_fx(world: &mut World) -> Option<u16> {
    let (move_id, origin) = world.take_pending_move_fx_spawn()?;
    if !world.spawn_move_fx(move_id, origin) {
        return None;
    }
    let cue = world.take_pending_move_fx_cue()?;
    match classify_cue(u32::from(cue)) {
        CueDispatch::Ring { ring_value, .. } => Some(ring_value),
        CueDispatch::Voice { .. } => None,
    }
}

/// The battle world scale (retail's base matrix `DAT_8007BF10`, a 4x uniform
/// scale under the battle camera) - one value, held beside the battle camera
/// it scales (`legaia_engine_camera::projection::BATTLE_WORLD_SCALE`)
/// ([`docs/subsystems/battle-stage-camera.md`](../../../docs/subsystems/battle-stage-camera.md#the-4-world-scale)).
pub const BATTLE_WORLD_SCALE: f32 = legaia_engine_camera::projection::BATTLE_WORLD_SCALE;

/// The world scale a battle draw - actor, backdrop or effect layer - composes
/// under the camera this frame: [`BATTLE_WORLD_SCALE`] in battle, `1` in every
/// other mode.
///
/// The scale is the battle **mode's** base matrix, not a property of the
/// stage: the matrix is a per-mode value and the backdrop is just one more
/// actor drawn through it, so a fight whose stage did not resolve still draws
/// its actors and effects at 4x. Both play hosts take the effect layers'
/// scale from here; the native window used to fall back to `1` (and to an
/// unscaled auto-framing camera) whenever no stage mesh was loaded, while the
/// browser page always drew at 4x.
pub fn battle_world_scale(world: &World) -> f32 {
    if world.mode == legaia_engine_mode::mode::SceneMode::Battle {
        BATTLE_WORLD_SCALE
    } else {
        1.0
    }
}
